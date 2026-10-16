using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-11c
//     "raw-exif" => Commands.RawExifCases(inputs),
//     "raw-exif-corpus" => Commands.RawExifCorpus(inputs),
// Needs W4-03b.cs (the private-static helper `ExifSummaryNode` of the same partial class). OracleCs.csproj needs the
// <Reference Include="PhotoReview.Imaging.Raw"> that W5-02a adds (HintPath $(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll) next to the
// PhotoReview.Imaging reference of W4-03b. Every type used here is public (RawExif, RawContainerInfo, RawFormat, ExifBlock,
// IRawHeaderSource, RawContainerLimits, EmbeddedPreview): no InternalsVisibleTo, no reflection.
// Output goes to tests/golden/raw-exif.json; inputs are golden-inputs/raw-exif.json.

// W5-11c: RawExif.TryReadExif. Command raw-exif: (block selection, clamping, merge rules, `complete`).
//   Case = { name, format, source:{length, parts:[{at, hex}]}, blocks:[{offset, length, is_tiff, ifd_is_exif}], fail_reads?:[int] }
//   The source is the InMemoryRawHeaderSource of the C# test project, re-implemented here (that class lives in a test assembly the
//   oracle cannot reference): 64 KiB budget blocks charged once per distinct block against RawContainerLimits.MaxHeaderBytes, the same
//   exceptions, the same returned span. Extra: every Read call is recorded (also the failing ones) and the calls whose 0-based index is in
//   "fail_reads" throw InvalidDataException("Simulated failure.") before doing anything else.
//   Result: { name, summary (ExifSummaryNode or null), complete, reads:[[offset,count],..], total_bytes_read, threw:false }
//   (a throw is reported as threw:true + exception: RawExif must never throw).
static partial class Commands
{
    private sealed class W511cSource(byte[] data, HashSet<int> failReads) : IRawHeaderSource
    {
        private const int BlockSize = 64 * 1024; // SourceRawHeaderSource.BlockSize
        private readonly HashSet<long> _touched = [];
        private int _calls;

        public readonly List<(long Offset, int Count)> Reads = [];
        public long TotalBytesRead { get; private set; }
        public long Length => data.Length;

        public ReadOnlySpan<byte> Read(long offset, int count)
        {
            Reads.Add((offset, count));
            if (failReads.Contains(_calls++))
                throw new InvalidDataException("Simulated failure.");

            if (offset < 0 || count < 0)
                throw new ArgumentOutOfRangeException(nameof(offset), "Offset and count must be non-negative.");
            if (offset > data.Length - count)
                throw new InvalidDataException("Attempted to read past end of memory.");
            if (count > RawContainerLimits.MaxHeaderBytes)
                throw new InvalidDataException("RAW header read exceeds hard limit.");
            if (count == 0)
                return ReadOnlySpan<byte>.Empty;

            long startBlock = offset / BlockSize;
            long endBlock = (offset + count - 1) / BlockSize;
            for (long block = startBlock; block <= endBlock; block++)
            {
                if (_touched.Contains(block))
                    continue;
                long blockBytes = Math.Min(BlockSize, data.Length - (block * BlockSize));
                if (blockBytes > RawContainerLimits.MaxHeaderBytes - TotalBytesRead)
                    throw new InvalidDataException("RAW header read exceeded hard limit.");
                _touched.Add(block);
                TotalBytesRead += blockBytes;
            }

            return data.AsSpan((int)offset, count);
        }
    }

    private static long W511cLong(JsonNode? node) =>
        node!.GetValueKind() == JsonValueKind.String
            ? long.Parse(node.GetValue<string>(), CultureInfo.InvariantCulture)
            : node.GetValue<long>();

    public static JsonArray RawExifCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };

            var s = c["source"]!.AsObject();
            var data = new byte[s["length"]!.GetValue<int>()];
            foreach (var part in s["parts"]!.AsArray())
            {
                var bytes = Convert.FromHexString(part!["hex"]!.GetValue<string>());
                bytes.CopyTo(data, part["at"]!.GetValue<int>());
            }

            var blocks = c["blocks"]!.AsArray().Select(b => new ExifBlock(
                W511cLong(b!["offset"]), W511cLong(b["length"]), b["is_tiff"]!.GetValue<bool>(), b["ifd_is_exif"]!.GetValue<bool>())).ToArray();
            var info = new RawContainerInfo(Enum.Parse<RawFormat>(c["format"]!.GetValue<string>()), 0, 0, 1, [], blocks);
            var fails = c["fail_reads"] is JsonArray fr ? fr.Select(x => x!.GetValue<int>()).ToHashSet() : [];
            var source = new W511cSource(data, fails);

            try
            {
                var exif = RawExif.TryReadExif(source, info, out bool complete);
                item["summary"] = ExifSummaryNode(exif);
                item["complete"] = complete;
                item["threw"] = false;
            }
            catch (Exception ex)
            {
                item["threw"] = true;
                item["exception"] = ex.GetType().Name;
            }

            item["reads"] = new JsonArray(source.Reads.Select(r => (JsonNode?)new JsonArray(r.Offset, r.Count)).ToArray());
            item["total_bytes_read"] = source.TotalBytesRead;
            result.Add(item);
        }
        return result;
    }
    /// <summary>
    /// Command raw-exif-corpus: the whole production chain on real files. Directory = input "corpus_dir", else env PHOTOREVIEW_RAW_CORPUS_DIR,
    /// else tests/Fixtures/raw-corpus of the C# repo; no directory => zero cases (the Rust test then only checks that). Every file whose
    /// extension is a RAW extension (RawFileTypes.IsRawExtension), sorted OrdinalIgnoreCase by name: RawContainerReaderRegistry.FindReader on
    /// the first 64 bytes, reader.Read(SourceRawHeaderSource), RawExif.TryReadExif on the SAME source (so the header budget carries over).
    /// Result: { file, reader (type name or null), format, sensor_width, sensor_height, exif_blocks:[{offset,length,is_tiff,ifd_is_exif}],
    ///           read_error (exception type or null), summary, complete, bytes_read }.
    /// </summary>
    public static JsonArray RawExifCorpus(JsonObject inputs)
    {
        var dir = inputs["corpus_dir"]?.GetValue<string>()
            ?? Environment.GetEnvironmentVariable("PHOTOREVIEW_RAW_CORPUS_DIR")
            ?? Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../../tests/Fixtures/raw-corpus"));
        var result = new JsonArray();
        if (!Directory.Exists(dir))
        {
            Console.Error.WriteLine("raw-exif-corpus: no corpus directory " + dir);
            return result;
        }

        var registry = new RawContainerReaderRegistry();
        foreach (var file in Directory.GetFiles(dir).OrderBy(f => f, StringComparer.OrdinalIgnoreCase))
        {
            var ext = Path.GetExtension(file);
            if (!RawFileTypes.IsRawExtension(ext)) continue;

            var item = new JsonObject { ["file"] = Path.GetFileName(file) };
            using var fs = File.OpenRead(file);
            var header = new byte[64];
            int n = fs.Read(header, 0, header.Length);
            fs.Position = 0;
            var reader = registry.FindReader(header.AsSpan(0, n), ext.ToLowerInvariant());
            item["reader"] = reader?.GetType().Name;
            using var source = new SourceRawHeaderSource(fs);
            if (reader is not null)
            {
                try
                {
                    var info = reader.Read(source, CancellationToken.None);
                    item["format"] = info.Format.ToString();
                    item["sensor_width"] = info.SensorWidth;
                    item["sensor_height"] = info.SensorHeight;
                    item["exif_blocks"] = new JsonArray(info.ExifBlocks.Select(b => (JsonNode?)new JsonObject
                    {
                        ["offset"] = b.Offset, ["length"] = b.Length, ["is_tiff"] = b.IsTiffHeader, ["ifd_is_exif"] = b.IfdIsExif,
                    }).ToArray());
                    var exif = RawExif.TryReadExif(source, info, out bool complete);
                    item["summary"] = ExifSummaryNode(exif);
                    item["complete"] = complete;
                }
                catch (Exception ex)
                {
                    item["read_error"] = ex.GetType().Name;
                }
            }

            item["bytes_read"] = source.TotalBytesRead;
            result.Add(item);
        }
        return result;
    }
}
