using System.Security.Cryptography;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;
using PhotoReview.Imaging.Raw.Tiff;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-08
//     "rw2-container" => Commands.Rw2Container(inputs),
//     "rw2-corpus" => Commands.Rw2Corpus(inputs),
// Needs: OracleCs.csproj gets a third <Reference Include="PhotoReview.Imaging.Raw"> (HintPath $(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll) and
// tools/oracle.ps1 must build src\PhotoReview.Imaging.Raw\PhotoReview.Imaging.Raw.csproj into the same folder (it references Core and
// Imaging only). Rw2ContainerReader, IRawHeaderSource, RawContainerInfo, EmbeddedPreview, ExifBlock and RawContainerLimits are public, so no
// InternalsVisibleTo is needed. Does not need W2-00.cs.

// W5-08: Rw2ContainerReader.CanRead / Read.
//
// rw2-container: one case per entry. Case = { name, op, data, ... }; "data" is { "len": N, "patches": [ { "at": offset, "hex": ".." } ] }
// (N zero bytes with the patches written over them) or { "hex": ".." } (exact bytes).
//   op "can-read"  data = the first-64-bytes span (any length), ext          -> { can_read }
//   op "read"      data = the whole file, budget (long, optional)            -> { ok, error, info, bytes_read }
//     error: null | "too-short" ("File too short for RW2 container.") | "invalid-header" ("Invalid RW2 header.") |
//            "source" (any other InvalidDataException: out-of-range read or exhausted header budget) | "unexpected:<Type>"
//     info: { format, sensor_width, sensor_height, orientation, previews[], exif_blocks[] } (null unless ok)
//     bytes_read: bytes charged by the in-memory source (whole distinct 64 KiB blocks, last block counts its own length); pins which
//                 blocks the reader touches. budget defaults to RawContainerLimits.MaxHeaderBytes.
//
// rw2-corpus: one case per RW2 file of the RAW corpus: { name, file, sha256 } -> { present, sha256_ok, length, ok, error, info, bytes_read }.
// The corpus directory is <fixtures>/raw-corpus (fixtures = tests/fixtures, set from the input path like the W4-10b fixtures; gitignored,
// fetched by the lead). A missing file is { present: false } and never an error, so oracle.ps1 stays green on a machine without the corpus.
static partial class Commands
{
    private const int W508BlockSize = 64 * 1024; // SourceRawHeaderSource.BlockSize (public const; the oracle repeats it to stay independent)
    private const string W508TooShort = "File too short for RW2 container.";
    private const string W508InvalidHeader = "Invalid RW2 header.";

    /// <summary>The test <c>InMemoryRawHeaderSource</c> (same exceptions, same block accounting) with a settable budget.</summary>
    private sealed class W508Source(byte[] bytes, long budget) : IRawHeaderSource
    {
        private readonly HashSet<long> _touched = [];

        public long Length => bytes.Length;

        public long TotalBytesRead { get; private set; }

        public ReadOnlySpan<byte> Read(long offset, int count)
        {
            if (offset < 0 || count < 0)
                throw new ArgumentOutOfRangeException(nameof(offset), "Offset and count must be non-negative.");
            if (offset > bytes.Length - count)
                throw new InvalidDataException($"Attempted to read past end of memory (offset: {offset}, count: {count}, length: {bytes.Length}).");
            if (count > RawContainerLimits.MaxHeaderBytes)
                throw new InvalidDataException($"RAW header read of {count} bytes exceeds hard limit of {RawContainerLimits.MaxHeaderBytes} bytes.");
            if (count == 0)
                return ReadOnlySpan<byte>.Empty;

            long startBlock = offset / W508BlockSize;
            long endBlock = (offset + count - 1) / W508BlockSize;
            for (long block = startBlock; block <= endBlock; block++)
            {
                if (_touched.Contains(block))
                    continue;
                long blockBytes = Math.Min(W508BlockSize, bytes.Length - (block * W508BlockSize));
                if (blockBytes > budget - TotalBytesRead)
                    throw new InvalidDataException($"RAW header read exceeded hard limit of {budget} bytes.");
                _touched.Add(block);
                TotalBytesRead += blockBytes;
            }

            return bytes.AsSpan((int)offset, count);
        }
    }

    private static byte[] W508Data(JsonNode? node)
    {
        var o = node!.AsObject();
        if (o["hex"] is { } hex) return Convert.FromHexString(hex.GetValue<string>());
        var bytes = new byte[o["len"]!.GetValue<int>()];
        if (o["patches"] is { } patches)
        {
            foreach (var p in patches.AsArray())
            {
                var po = p!.AsObject();
                Convert.FromHexString(po["hex"]!.GetValue<string>()).CopyTo(bytes, po["at"]!.GetValue<int>());
            }
        }

        return bytes;
    }

    private static JsonObject W508Info(RawContainerInfo info) => new()
    {
        ["format"] = info.Format.ToString(),
        ["sensor_width"] = info.SensorWidth,
        ["sensor_height"] = info.SensorHeight,
        ["orientation"] = info.Orientation,
        ["previews"] = new JsonArray(info.Previews.Select(p => (JsonNode?)new JsonObject
        {
            ["index"] = p.Index,
            ["offset"] = p.Offset,
            ["length"] = p.Length,
            ["kind"] = p.Kind.ToString(),
            ["width"] = p.Width,
            ["height"] = p.Height,
            ["color_space"] = p.ColorSpace.ToString(),
            ["header_resolved"] = p.HeaderResolved,
        }).ToArray()),
        ["exif_blocks"] = new JsonArray(info.ExifBlocks.Select(b => (JsonNode?)new JsonObject
        {
            ["offset"] = b.Offset,
            ["length"] = b.Length,
            ["is_tiff_header"] = b.IsTiffHeader,
            ["ifd_is_exif"] = b.IfdIsExif,
        }).ToArray()),
    };

    /// <summary>Runs <c>Rw2ContainerReader.Read</c> on <paramref name="bytes"/> and writes ok/error/info/bytes_read into <paramref name="item"/>.</summary>
    private static void W508Read(JsonObject item, byte[] bytes, long budget)
    {
        var source = new W508Source(bytes, budget);
        try
        {
            var info = new Rw2ContainerReader().Read(source, CancellationToken.None);
            item["ok"] = true;
            item["error"] = null;
            item["info"] = W508Info(info);
        }
        catch (InvalidDataException ex)
        {
            item["ok"] = false;
            item["error"] = ex.Message == W508TooShort ? "too-short" : ex.Message == W508InvalidHeader ? "invalid-header" : "source";
            item["info"] = null;
        }
        catch (Exception ex)
        {
            item["ok"] = false;
            item["error"] = "unexpected:" + ex.GetType().Name;
            item["info"] = null;
        }

        item["bytes_read"] = source.TotalBytesRead;
    }

    public static JsonArray Rw2Container(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            var data = W508Data(c["data"]);
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op, ["data"] = c["data"]!.DeepClone() };
            switch (op)
            {
                case "can-read":
                {
                    var ext = c["ext"]!.GetValue<string>();
                    item["ext"] = ext;
                    item["can_read"] = new Rw2ContainerReader().CanRead(data, ext);
                    break;
                }
                case "read":
                {
                    long budget = c["budget"]?.GetValue<long>() ?? RawContainerLimits.MaxHeaderBytes;
                    item["budget"] = c["budget"]?.DeepClone();
                    W508Read(item, data, budget);
                    break;
                }
                default:
                    throw new ArgumentException("unknown op " + op);
            }

            result.Add(item);
        }

        return result;
    }

    public static JsonArray Rw2Corpus(JsonObject inputs)
    {
        var dir = Path.GetFullPath(Path.Combine(PerfCommands.FixturesDir, "raw-corpus"));
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var file = c["file"]!.GetValue<string>();
            var path = Path.Combine(dir, file);
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["file"] = file, ["sha256"] = c["sha256"]!.GetValue<string>() };
            if (!File.Exists(path))
            {
                item["present"] = false;
                result.Add(item);
                continue;
            }

            var bytes = File.ReadAllBytes(path);
            item["present"] = true;
            item["sha256_ok"] = Convert.ToHexString(SHA256.HashData(bytes)).Equals(c["sha256"]!.GetValue<string>(), StringComparison.OrdinalIgnoreCase);
            item["length"] = bytes.Length;
            W508Read(item, bytes, RawContainerLimits.MaxHeaderBytes);
            result.Add(item);
        }

        return result;
    }
}
