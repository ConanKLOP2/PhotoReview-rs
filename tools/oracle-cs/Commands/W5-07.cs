using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;
using PhotoReview.Imaging.Raw.Tiff;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-07
//     "raw-arw-reader" => Commands.RawArwReader(inputs, inputPath),
//
// WIRING (the wiring step does this; the contract writer did NOT run the oracle):
//   * tools/oracle.ps1 must also build `src/PhotoReview.Imaging.Raw` from the baseline archive and copy
//     PhotoReview.Imaging.Raw.dll next to PhotoReview.Core.dll; tools/oracle-cs/OracleCs.csproj gets
//     `<Reference Include="PhotoReview.Imaging.Raw"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll</HintPath></Reference>`.
//     Every type used below (ArwContainerReader, IRawContainerReader, IRawHeaderSource, RawContainerInfo, EmbeddedPreview,
//     ExifBlock, RawContainerLimits) is PUBLIC: no reflection. The same wiring serves W5-03..W5-09 (their oracles share it).
//   * Corpus cases ("kind": "corpus") read real camera files from the gitignored corpus directory: env PHOTOREVIEW_RS_RAW_CORPUS,
//     else <repo>/tests/fixtures/raw-corpus (the wiring step runs the port of C# tools/fetch-raw-samples.ps1 first; the three ARW
//     files are pinned by SHA-256 in C# tools/raw-samples.txt). A missing file or a hash mismatch THROWS (fail closed), unless the
//     case says "optional": true, in which case the golden records {"skipped": true} (D19: then the Rust test has nothing to compare).
//   * Synthetic cases use the spec builder W507Tiff below, a line port of the test helpers tests/PhotoReview.Imaging.Tests/Raw/Tiff/
//     TiffBytes.cs + SyntheticRawBuilder.CreateMinimalJpeg (the test assembly is not referenced). The Rust tests build the SAME
//     bytes from the SAME spec (tests/common `TiffSpec`), so the golden also pins the builder.
//
// W5-07: ArwContainerReader.CanRead / Read over a header source with the exact budget semantics of InMemoryRawHeaderSource
// (64 KiB blocks charged once each against RawContainerLimits.MaxHeaderBytes), so `bytes_charged` pins how much the reader reads.

static partial class Commands
{
    private const int W507BlockSize = 64 * 1024;

    /// <summary>Line port of tests/PhotoReview.TestSupport.Windows/Fixtures/InMemoryRawHeaderSource.cs (the test assembly is not referenced).</summary>
    private sealed class W507Source(byte[] bytes) : IRawHeaderSource
    {
        private readonly HashSet<long> _touched = [];
        public long TotalBytesRead { get; private set; }
        public long Length => bytes.Length;

        public ReadOnlySpan<byte> Read(long offset, int count)
        {
            if (offset < 0 || count < 0) throw new ArgumentOutOfRangeException(nameof(offset), "Offset and count must be non-negative.");
            if (offset > bytes.Length - count)
                throw new InvalidDataException($"Attempted to read past end of memory (offset: {offset}, count: {count}, length: {bytes.Length}).");
            if (count > RawContainerLimits.MaxHeaderBytes)
                throw new InvalidDataException($"RAW header read of {count} bytes exceeds hard limit of {RawContainerLimits.MaxHeaderBytes} bytes.");
            if (count == 0) return ReadOnlySpan<byte>.Empty;

            long startBlock = offset / W507BlockSize;
            long endBlock = (offset + count - 1) / W507BlockSize;
            for (long block = startBlock; block <= endBlock; block++)
            {
                if (_touched.Contains(block)) continue;
                long blockBytes = Math.Min(W507BlockSize, bytes.Length - (block * W507BlockSize));
                if (blockBytes > RawContainerLimits.MaxHeaderBytes - TotalBytesRead)
                    throw new InvalidDataException($"RAW header read exceeded hard limit of {RawContainerLimits.MaxHeaderBytes} bytes.");
                _touched.Add(block);
                TotalBytesRead += blockBytes;
            }

            return bytes.AsSpan((int)offset, count);
        }
    }

    /// <summary>Port of SyntheticRawBuilder.CreateMinimalJpeg (SOI APP0 DQT SOF0 DHT SOS data EOI).</summary>
    private static byte[] W507Jpeg(int width, int height)
    {
        using var ms = new MemoryStream();
        ms.Write([0xFF, 0xD8]);
        ms.Write([0xFF, 0xE0, 0x00, 0x10]);
        ms.Write(Encoding.ASCII.GetBytes("JFIF\0"));
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

    /// <summary>Port of TiffPreviewHardeningTests.BudgetExhaustingJpeg: 140 APP0 segments of 65 537 bytes before the SOF.</summary>
    private static byte[] W507BudgetJpeg()
    {
        var tail = W507Jpeg(320, 240);
        using var ms = new MemoryStream();
        ms.Write([0xFF, 0xD8]);
        var app = new byte[2 + 65535];
        app[0] = 0xFF; app[1] = 0xE0; app[2] = 0xFF; app[3] = 0xFF;
        for (int i = 0; i < 140; i++) ms.Write(app);
        ms.Write(tail, 2, tail.Length - 2);
        return ms.ToArray();
    }

    /// <summary>Port of TiffBytes: a zero-filled buffer with header / IFD / raw writes at explicit offsets.</summary>
    private sealed class W507Tiff(bool le, int size)
    {
        public readonly byte[] Buffer = new byte[size];

        public void U16(int at, ushort v)
        {
            if (le) BinaryPrimitives.WriteUInt16LittleEndian(Buffer.AsSpan(at), v);
            else BinaryPrimitives.WriteUInt16BigEndian(Buffer.AsSpan(at), v);
        }

        public void U32(int at, uint v)
        {
            if (le) BinaryPrimitives.WriteUInt32LittleEndian(Buffer.AsSpan(at), v);
            else BinaryPrimitives.WriteUInt32BigEndian(Buffer.AsSpan(at), v);
        }

        public void Put(int at, ReadOnlySpan<byte> bytes) => bytes.CopyTo(Buffer.AsSpan(at));

        /// <summary>
        /// Entry kinds: short (type 3 count 1), long (type 4 count 1), at (explicit type/count/offset), raw (explicit type/count/value).
        /// <paramref name="pad"/> filler SHORT entries (tag 0xF000, value 1) are written BEFORE the listed entries, so a hostile
        /// entry count (4096, 4097) does not need thousands of JSON lines.
        /// </summary>
        public void Ifd(int at, uint next, JsonArray entries, int pad)
        {
            int total = pad + entries.Count;
            U16(at, (ushort)total);
            for (int i = 0; i < total; i++)
            {
                int p = at + 2 + (i * 12);
                ushort tag; ushort type; uint count; uint value;
                if (i < pad)
                {
                    tag = 0xF000; type = 3; count = 1; value = 1;
                }
                else
                {
                    var e = entries[i - pad]!.AsObject();
                    string kind = e["k"]!.GetValue<string>();
                    tag = (ushort)e["tag"]!.GetValue<int>();
                    switch (kind)
                    {
                        case "short": type = 3; count = 1; value = e["v"]!.GetValue<uint>(); break;
                        case "long": type = 4; count = 1; value = e["v"]!.GetValue<uint>(); break;
                        case "at": case "raw":
                            type = (ushort)e["type"]!.GetValue<int>(); count = e["count"]!.GetValue<uint>(); value = e["v"]!.GetValue<uint>(); break;
                        default: throw new ArgumentException("unknown entry kind " + kind);
                    }
                }

                U16(p, tag);
                U16(p + 2, type);
                U32(p + 4, count);
                if (type == 3 && count == 1) U16(p + 8, (ushort)value); // SHORT left-justified, as TiffBytes.Ifd does
                else U32(p + 8, value);
            }

            U32(at + 2 + (total * 12), next);
        }
    }

    private static byte[] W507Build(JsonObject src)
    {
        switch (src["kind"]!.GetValue<string>())
        {
            case "hex":
                return Convert.FromHexString(src["hex"]!.GetValue<string>());
            case "tiff":
            {
                bool le = src["le"]?.GetValue<bool>() ?? true;
                var t = new W507Tiff(le, src["size"]!.GetValue<int>());
                var h = src["header"]!.AsObject();
                ReadOnlySpan<byte> sig = h["signature_hex"] is { } sh ? Convert.FromHexString(sh.GetValue<string>()) : default;
                ReadOnlySpan<byte> magic = sig.IsEmpty ? (le ? "II*\0"u8 : "MM\0*"u8) : sig;
                t.Put(0, magic);
                t.U32(4, h["ifd0"]!.GetValue<uint>());
                foreach (var node in src["ops"]!.AsArray())
                {
                    var op = node!.AsObject();
                    int at = op["at"]!.GetValue<int>();
                    switch (op["op"]!.GetValue<string>())
                    {
                        case "ifd": t.Ifd(at, op["next"]!.GetValue<uint>(), op["entries"]!.AsArray(), op["pad"]?.GetValue<int>() ?? 0); break;
                        case "u16": t.U16(at, (ushort)op["v"]!.GetValue<int>()); break;
                        case "u32": t.U32(at, op["v"]!.GetValue<uint>()); break;
                        case "hex": t.Put(at, Convert.FromHexString(op["hex"]!.GetValue<string>())); break;
                        case "ascii": t.Put(at, Encoding.Latin1.GetBytes(op["text"]!.GetValue<string>())); break; // may hold \u0000
                        case "jpeg": t.Put(at, W507Jpeg(op["w"]!.GetValue<int>(), op["h"]!.GetValue<int>())); break;
                        case "budget_jpeg": t.Put(at, W507BudgetJpeg()); break;
                        default: throw new ArgumentException("unknown op " + op["op"]);
                    }
                }

                return t.Buffer;
            }
            case "corpus":
            {
                string dir = Environment.GetEnvironmentVariable("PHOTOREVIEW_RS_RAW_CORPUS") is { Length: > 0 } env
                    ? env
                    : Path.Combine(W507RepoRoot, "tests", "fixtures", "raw-corpus");
                string path = Path.Combine(dir, src["file"]!.GetValue<string>());
                if (!File.Exists(path)) throw new FileNotFoundException("RAW corpus sample missing (fetch it first): " + path);
                var bytes = File.ReadAllBytes(path);
                string expected = src["sha256"]!.GetValue<string>();
                string actual = Convert.ToHexString(SHA256.HashData(bytes));
                if (!actual.Equals(expected, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException($"SHA-256 of {path} is {actual}, expected {expected}");
                return bytes;
            }
            default:
                throw new ArgumentException("unknown source kind " + src["kind"]);
        }
    }

    private static string W507RepoRoot = ".";

    private static JsonObject W507Preview(EmbeddedPreview p) => new()
    {
        ["index"] = p.Index,
        ["offset"] = p.Offset,
        ["length"] = p.Length,
        ["kind"] = p.Kind.ToString(),
        ["width"] = p.Width,
        ["height"] = p.Height,
        ["color_space"] = p.ColorSpace.ToString(),
        ["header_resolved"] = p.HeaderResolved,
    };

    private static JsonObject W507Block(ExifBlock b) => new()
    {
        ["offset"] = b.Offset,
        ["length"] = b.Length,
        ["is_tiff_header"] = b.IsTiffHeader,
        ["ifd_is_exif"] = b.IfdIsExif,
    };

    /// <summary>
    /// cases: {name, op: "can-read"|"read" (default "read"), ext (default ".arw"), source: {kind: hex|tiff|corpus, ...}, cancel?: bool, optional?: bool}.
    /// "can-read": ArwContainerReader.CanRead(first min(64, len) bytes of the source, ext). "read": CanRead as well (reported as can_read) and then
    /// Read(new W507Source, token) (token already canceled when "cancel"), reporting the container info or {exception, message} and bytes_charged.
    /// </summary>
    public static JsonArray RawArwReader(JsonObject inputs, string inputPath)
    {
        W507RepoRoot = Path.GetFullPath(Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!, "..", ".."));
        var result = new JsonArray();
        var reader = new ArwContainerReader();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            string name = c["name"]!.GetValue<string>();
            string ext = c["ext"]?.GetValue<string>() ?? ".arw";
            string op = c["op"]?.GetValue<string>() ?? "read";
            bool optional = c["optional"]?.GetValue<bool>() ?? false;

            byte[] data;
            try
            {
                data = W507Build(c["source"]!.AsObject());
            }
            catch (FileNotFoundException) when (optional)
            {
                result.Add(new JsonObject { ["name"] = name, ["skipped"] = true });
                continue;
            }

            var o = new JsonObject { ["name"] = name, ["op"] = op };
            o["can_read"] = reader.CanRead(data.AsSpan(0, Math.Min(64, data.Length)), ext);
            if (op == "read")
            {
                var source = new W507Source(data);
                using var cts = new CancellationTokenSource();
                if (c["cancel"]?.GetValue<bool>() == true) cts.Cancel();
                try
                {
                    var info = reader.Read(source, cts.Token);
                    o["ok"] = true;
                    o["format"] = info.Format.ToString();
                    o["sensor_width"] = info.SensorWidth;
                    o["sensor_height"] = info.SensorHeight;
                    o["orientation"] = info.Orientation;
                    o["previews"] = new JsonArray(info.Previews.Select(p => (JsonNode)W507Preview(p)).ToArray());
                    o["exif_blocks"] = new JsonArray(info.ExifBlocks.Select(b => (JsonNode)W507Block(b)).ToArray());
                }
                catch (Exception ex)
                {
                    o["ok"] = false;
                    o["exception"] = ex.GetType().Name;
                    // Only the two texts the reader itself raises are pinned; source messages are not part of the contract.
                    o["reader_message"] = (ex.Message is "File too short for ARW container." or "Invalid ARW TIFF header.") ? ex.Message : null;
                }

                o["bytes_charged"] = source.TotalBytesRead;
            }

            result.Add(o);
        }

        return result;
    }
}
