using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;
using PhotoReview.Imaging.Raw.Tiff;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-03
//     "cr2-container" => Commands.Cr2Container(inputs),
//     "cr2-container-corpus" => Commands.Cr2ContainerCorpus(inputs),
// Wiring needed (the contract writer did NOT run or wire any of it):
//   * OracleCs.csproj: a <Reference Include="PhotoReview.Imaging.Raw"> with HintPath $(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll (next to the
//     Core / Imaging references). Cr2ContainerReader, IRawHeaderSource, SourceRawHeaderSource, RawContainerInfo are public: no InternalsVisibleTo.
//   * tools/oracle.ps1: build src\PhotoReview.Imaging.Raw\PhotoReview.Imaging.Raw.csproj (references Core + Imaging, embeds AdobeCompat-v2.icc)
//     into $coreDir like the Imaging project, and add PhotoReview.Imaging.Raw.dll to the "rebuild if missing" check.
//   * Shared with the other W5 reader oracles: the case builder below (W503Build / W503Jpeg) may be hoisted into a W5 helper file.
//   * cr2-container-corpus reads PHOTOREVIEW_RAW_CORPUS_DIR (or "corpus_dir" of its input); no directory => zero cases and a note on stderr.

// W5-03: Cr2ContainerReader.CanRead / Read. Every read case runs through the PRODUCTION header source
// (SourceRawHeaderSource over a MemoryStream, or over the FileStream for the corpus), so the golden pins the header-budget charge
// (bytes_read = whole distinct 64 KiB blocks touched) as well as the result.
//
// Case shapes (input fields are echoed back, result fields are added):
//   { name, op: "can-read", head: {hex}, ext }                          -> { can_read }
//   { name, op: "read", file: {...} | data: {hex}, reported_length?, cancel? }
//        -> { data_length, data_sha256, throws, io_failure, bytes_read, info }   (info / throws are null when the other is set)
//   "file": { little (default true), size, ops: [ ... applied in order ... ] } where an op is one of
//        { op: "header", ifd0, signature? (hex of bytes 0..3; default II*\0 / MM\0*) }       header + IFD0 offset at 4
//        { op: "put", at, hex }  |  { op: "put", at, jpeg: { width, height, lossless?, app_segments? } }
//        { op: "u16" | "u32", at, value }                                                  in the file's byte order
//        { op: "ifd", at, next, entries: [ { tag, type, count, value } ] }                 TiffBytes.Ifd (SHORT x 1 is left-justified)
//   "jpeg": SyntheticRawBuilder.CreateMinimalJpeg(width, height) (163 bytes); lossless => first SOF0 rewritten to SOF3;
//           app_segments N => SOI + N APP0 segments of 65537 bytes + the rest of the minimal JPEG (TiffPreviewHardeningTests.BudgetExhaustingJpeg).
//   "reported_length": the source reports this Length (TiffStripPreviewTests.HugeFileSource); "cancel": the token is canceled before Read.
static partial class Commands
{
    /// <summary>Reports a larger file than the bytes backing it (the strip itself is never read, only its first bytes).</summary>
    private sealed class W503HugeSource(IRawHeaderSource inner, long length) : IRawHeaderSource
    {
        public long Length { get; } = length;

        public ReadOnlySpan<byte> Read(long offset, int count) => inner.Read(offset, count);
    }

    /// <summary>Port of SyntheticRawBuilder.CreateMinimalJpeg (tests/PhotoReview.Imaging.Tests/Raw/SyntheticRawBuilder.cs).</summary>
    private static byte[] W503MinimalJpeg(int width, int height)
    {
        using var ms = new MemoryStream();
        ms.Write([0xFF, 0xD8]);
        ms.Write([0xFF, 0xE0, 0x00, 0x10]);
        ms.Write("JFIF\0"u8);
        ms.Write([0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
        ms.Write([0xFF, 0xDB, 0x00, 0x43, 0x00]);
        for (int i = 0; i < 64; i++) ms.WriteByte(16);
        ms.Write([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        ms.WriteByte((byte)(height >> 8));
        ms.WriteByte((byte)height);
        ms.WriteByte((byte)(width >> 8));
        ms.WriteByte((byte)width);
        ms.WriteByte(3);
        ms.Write([0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        ms.Write([0xFF, 0xC4, 0x00, 0x1F, 0x00]);
        ms.Write([0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        ms.Write([0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B]);
        ms.Write([0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00]);
        ms.Write([0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55]);
        ms.Write([0xFF, 0xD9]);
        return ms.ToArray();
    }

    private static byte[] W503Jpeg(JsonObject spec)
    {
        var jpeg = W503MinimalJpeg(spec["width"]!.GetValue<int>(), spec["height"]!.GetValue<int>());
        if (spec["lossless"]?.GetValue<bool>() == true)
        {
            // TiffBytes.AsLosslessJpeg: the first SOF0 marker becomes SOF3.
            for (int i = 0; i + 1 < jpeg.Length; i++)
            {
                if (jpeg[i] == 0xFF && jpeg[i + 1] == 0xC0)
                {
                    jpeg[i + 1] = 0xC3;
                    break;
                }
            }
        }

        if (spec["app_segments"] is { } segments)
        {
            using var ms = new MemoryStream();
            ms.Write([0xFF, 0xD8]);
            var app = new byte[2 + 65535];
            app[0] = 0xFF;
            app[1] = 0xE0;
            app[2] = 0xFF;
            app[3] = 0xFF;
            for (int i = 0; i < segments.GetValue<int>(); i++) ms.Write(app);
            ms.Write(jpeg, 2, jpeg.Length - 2);
            jpeg = ms.ToArray();
        }

        return jpeg;
    }

    private static byte[] W503Build(JsonObject file)
    {
        bool little = file["little"]?.GetValue<bool>() ?? true;
        var buf = new byte[file["size"]!.GetValue<int>()];

        void U16(int pos, ushort value)
        {
            if (little) BinaryPrimitives.WriteUInt16LittleEndian(buf.AsSpan(pos), value);
            else BinaryPrimitives.WriteUInt16BigEndian(buf.AsSpan(pos), value);
        }

        void U32(int pos, uint value)
        {
            if (little) BinaryPrimitives.WriteUInt32LittleEndian(buf.AsSpan(pos), value);
            else BinaryPrimitives.WriteUInt32BigEndian(buf.AsSpan(pos), value);
        }

        foreach (var node in file["ops"]!.AsArray())
        {
            var op = node!.AsObject();
            switch (op["op"]!.GetValue<string>())
            {
                case "header":
                {
                    byte[] magic = op["signature"] is { } sig
                        ? Convert.FromHexString(sig.GetValue<string>())
                        : (little ? "II*\0"u8.ToArray() : "MM\0*"u8.ToArray());
                    magic.CopyTo(buf, 0);
                    U32(4, op["ifd0"]!.GetValue<uint>());
                    break;
                }
                case "put":
                {
                    byte[] bytes = op["hex"] is { } hex ? Convert.FromHexString(hex.GetValue<string>()) : W503Jpeg(op["jpeg"]!.AsObject());
                    bytes.CopyTo(buf, op["at"]!.GetValue<int>());
                    break;
                }
                case "u16":
                    U16(op["at"]!.GetValue<int>(), op["value"]!.GetValue<ushort>());
                    break;
                case "u32":
                    U32(op["at"]!.GetValue<int>(), op["value"]!.GetValue<uint>());
                    break;
                case "ifd":
                {
                    int at = op["at"]!.GetValue<int>();
                    var entries = op["entries"]!.AsArray();
                    U16(at, (ushort)entries.Count);
                    for (int i = 0; i < entries.Count; i++)
                    {
                        var e = entries[i]!.AsObject();
                        int p = at + 2 + (i * 12);
                        ushort type = e["type"]!.GetValue<ushort>();
                        uint count = e["count"]!.GetValue<uint>();
                        uint value = e["value"]!.GetValue<uint>();
                        U16(p, e["tag"]!.GetValue<ushort>());
                        U16(p + 2, type);
                        U32(p + 4, count);
                        if (type == 3 && count == 1) U16(p + 8, (ushort)value); // SHORT is left-justified in the value field
                        else U32(p + 8, value);
                    }

                    U32(at + 2 + (entries.Count * 12), op["next"]!.GetValue<uint>());
                    break;
                }
                default:
                    throw new ArgumentException("unknown op " + op["op"]);
            }
        }

        return buf;
    }

    private static JsonObject W503Info(RawContainerInfo info) => new()
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

    /// <summary>Runs Cr2ContainerReader.Read and adds throws / io_failure / bytes_read / info to <paramref name="item"/>.</summary>
    private static void W503Read(JsonObject item, IRawHeaderSource source, SourceRawHeaderSource real, bool cancel)
    {
        using var cts = new CancellationTokenSource();
        if (cancel) cts.Cancel();
        try
        {
            var info = new Cr2ContainerReader().Read(source, cts.Token);
            item["throws"] = null;
            item["io_failure"] = null;
            item["info"] = W503Info(info);
        }
        catch (OperationCanceledException)
        {
            item["throws"] = "OperationCanceledException";
            item["io_failure"] = null;
            item["info"] = null;
        }
        catch (InvalidDataException ex)
        {
            item["throws"] = "InvalidDataException";
            item["io_failure"] = ex.InnerException is IOException or ObjectDisposedException;
            item["info"] = null;
        }
        catch (Exception ex)
        {
            // Not a documented failure: the golden pins the type so a Rust panic / wrong error is a visible difference.
            item["throws"] = ex.GetType().Name;
            item["io_failure"] = null;
            item["info"] = null;
        }

        item["bytes_read"] = real.TotalBytesRead;
    }

    public static JsonArray Cr2Container(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            switch (c["op"]!.GetValue<string>())
            {
                case "can-read":
                {
                    var head = Convert.FromHexString(c["head"]!["hex"]!.GetValue<string>());
                    item["can_read"] = new Cr2ContainerReader().CanRead(head, c["ext"]!.GetValue<string>());
                    break;
                }
                case "read":
                {
                    byte[] data = c["file"] is { } file
                        ? W503Build(file.AsObject())
                        : Convert.FromHexString(c["data"]!["hex"]!.GetValue<string>());
                    var real = new SourceRawHeaderSource(new MemoryStream(data, writable: false));
                    IRawHeaderSource source = c["reported_length"] is { } reported ? new W503HugeSource(real, reported.GetValue<long>()) : real;
                    item["data_length"] = data.Length;
                    item["data_sha256"] = Convert.ToHexString(SHA256.HashData(data)).ToLowerInvariant();
                    W503Read(item, source, real, c["cancel"]?.GetValue<bool>() ?? false);
                    break;
                }
                default:
                    throw new ArgumentException("unknown op " + c["op"]);
            }

            result.Add(item);
        }

        return result;
    }

    /// <summary>
    /// TiffCorpusTests.Cr2Corpus_* / AllTiffCorpusFiles_*: every *.cr2 of the (uncommitted) RAW corpus through the production path
    /// (FileStream -> SourceRawHeaderSource -> Cr2ContainerReader), in file-name order. Reports CanRead on the first 64 bytes and whether
    /// each listed preview starts with SOI.
    /// </summary>
    public static JsonArray Cr2ContainerCorpus(JsonObject inputs)
    {
        var result = new JsonArray();
        var envName = inputs["corpus_dir_env"]?.GetValue<string>() ?? "PHOTOREVIEW_RAW_CORPUS_DIR";
        var dir = Environment.GetEnvironmentVariable(envName);
        if (string.IsNullOrWhiteSpace(dir)) dir = inputs["corpus_dir"]?.GetValue<string>();
        if (string.IsNullOrWhiteSpace(dir) || !Directory.Exists(dir))
        {
            Console.Error.WriteLine($"cr2-container-corpus: no corpus directory ({envName} unset or missing); writing zero cases.");
            return result;
        }

        var extension = inputs["extension"]?.GetValue<string>() ?? ".cr2";
        var files = Directory.GetFiles(dir)
            .Where(f => Path.GetExtension(f).Equals(extension, StringComparison.OrdinalIgnoreCase))
            .OrderBy(f => f, StringComparer.OrdinalIgnoreCase);
        foreach (var path in files)
        {
            var item = new JsonObject { ["name"] = Path.GetFileName(path), ["op"] = "corpus-file" };
            using var fs = File.OpenRead(path);
            var header = new byte[64];
            int read = fs.Read(header, 0, header.Length);
            item["length"] = fs.Length;
            item["can_read"] = new Cr2ContainerReader().CanRead(header.AsSpan(0, read), Path.GetExtension(path).ToLowerInvariant());
            fs.Position = 0;
            using var real = new SourceRawHeaderSource(fs);
            W503Read(item, real, real, cancel: false);

            if (item["info"] is JsonObject info)
            {
                using var check = File.OpenRead(path);
                var soi = new JsonArray();
                foreach (var p in info["previews"]!.AsArray())
                {
                    long offset = p!["offset"]!.GetValue<long>();
                    long length = p["length"]!.GetValue<long>();
                    var two = new byte[2];
                    bool starts = false;
                    if (offset >= 0 && length >= 2 && offset + 2 <= check.Length)
                    {
                        check.Position = offset;
                        starts = check.Read(two, 0, 2) == 2 && two[0] == 0xFF && two[1] == 0xD8;
                    }

                    soi.Add(starts);
                }

                item["previews_start_with_soi"] = soi;
            }

            result.Add(item);
        }

        return result;
    }
}
