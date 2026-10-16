using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;
using PhotoReview.Imaging.Raw.Bmff;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-10b
//     "raw-cr3-reader" => Commands.RawCr3Reader(inputs, inputPath),
//
// WIRING (the wiring step does this; the contract writer did NOT run the oracle):
//   * tools/oracle.ps1 must also build `src/PhotoReview.Imaging.Raw` from the baseline archive and copy PhotoReview.Imaging.Raw.dll next
//     to PhotoReview.Core.dll; tools/oracle-cs/OracleCs.csproj gets
//     `<Reference Include="PhotoReview.Imaging.Raw"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll</HintPath></Reference>`.
//     Every type used below (Cr3ContainerReader, IRawContainerReader, IRawHeaderSource, RawContainerInfo, EmbeddedPreview, ExifBlock,
//     RawContainerLimits) is PUBLIC: no reflection. The same wiring serves W5-03..W5-10a (their oracles share it).
//   * Corpus cases ("kind": "corpus") read real camera files from the gitignored corpus directory: env PHOTOREVIEW_RS_RAW_CORPUS, else
//     <repo>/tests/fixtures/raw-corpus (the wiring step runs the port of C# tools/fetch-raw-samples.ps1 first; the two CR3 files are pinned by
//     SHA-256 in C# tools/raw-samples.txt). A missing file or a hash mismatch THROWS (fail closed), unless the case says "optional": true, in
//     which case the golden records {"skipped": true} (D19: then the Rust test has nothing to compare).
//   * Synthetic cases are described by a tiny box language (see W510bEmit below and contract.md "Golden package"); the Rust test builds the
//     SAME bytes from the SAME spec, and every output row carries file_length + file_fnv1a64 so a builder drift is caught before the reader runs.
//     This file compiled against the baseline sources of Imaging.Raw in a scratch project (see contract.md); it was NOT run.
//
// W5-10b: Cr3ContainerReader.CanRead / Read over a header source with the exact budget semantics of InMemoryRawHeaderSource (64 KiB blocks
// charged once each against RawContainerLimits.MaxHeaderBytes), so `bytes_charged` pins how much, and which blocks, the reader reads.

static partial class Commands
{
    private const int W510bBlockSize = 64 * 1024;

    /// <summary>Line port of tests/PhotoReview.TestSupport.Windows/Fixtures/InMemoryRawHeaderSource.cs (the test assembly is not referenced).</summary>
    private sealed class W510bSource(byte[] bytes) : IRawHeaderSource
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

            long startBlock = offset / W510bBlockSize;
            long endBlock = (offset + count - 1) / W510bBlockSize;
            for (long block = startBlock; block <= endBlock; block++)
            {
                if (_touched.Contains(block)) continue;
                long blockBytes = Math.Min(W510bBlockSize, bytes.Length - (block * W510bBlockSize));
                if (blockBytes > RawContainerLimits.MaxHeaderBytes - TotalBytesRead)
                    throw new InvalidDataException($"RAW header read exceeded hard limit of {RawContainerLimits.MaxHeaderBytes} bytes.");
                _touched.Add(block);
                TotalBytesRead += blockBytes;
            }

            return bytes.AsSpan((int)offset, count);
        }
    }

    /// <summary>Port of SyntheticRawBuilder.CreateMinimalJpeg (SOI APP0 DQT SOF0 DHT SOS data EOI), 163 bytes.</summary>
    private static byte[] W510bJpeg(int width, int height)
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

    /// <summary>Port of Cr3BmffHardeningTests.LosslessJpeg: SOI, SOF3 (len 11, precision 14, h, w, 1 component) and 256 filler bytes (271 bytes).</summary>
    private static byte[] W510bLosslessJpeg(int width, int height)
    {
        using var ms = new MemoryStream();
        ms.Write([0xFF, 0xD8, 0xFF, 0xC3, 0x00, 0x0B, 14]);
        ms.WriteByte((byte)(height >> 8));
        ms.WriteByte((byte)height);
        ms.WriteByte((byte)(width >> 8));
        ms.WriteByte((byte)width);
        ms.Write([1, 1, 0x11, 0]);
        ms.Write(new byte[256]);
        return ms.ToArray();
    }

    /// <summary>State of one build: marks recorded by pass 1 are resolved by pass 2 (box sizes never depend on an offset value).</summary>
    private sealed class W510bBuild
    {
        public readonly Dictionary<string, long> Marks = new();
        public Dictionary<string, long>? Resolved;

        public long Mark(string name)
        {
            if (Resolved is null) return 0; // pass 1: sizes only
            return Resolved.TryGetValue(name, out long v) ? v : throw new ArgumentException("unknown mark " + name);
        }
    }

    /// <summary>Port of TiffBytes: a zero-filled buffer with header / IFD / raw writes at explicit offsets (same spec as the W5-07 oracle).</summary>
    private sealed class W510bTiff(bool le, int size)
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
        /// Entry kinds: short (type 3 count 1), long (type 4 count 1), at (explicit type/count/value). A value may be taken from a mark of the
        /// enclosing file ("v_mark": name, "plus": n added) instead of "v". <paramref name="pad"/> filler SHORT entries (tag 0xF000, value 1)
        /// are written BEFORE the listed entries, so a hostile entry count (4096, 4097) does not need thousands of JSON lines.
        /// </summary>
        public void Ifd(int at, uint next, JsonArray entries, int pad, W510bBuild b, int countOverride)
        {
            int total = pad + entries.Count;
            U16(at, (ushort)(countOverride >= 0 ? countOverride : total));
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
                        case "short": type = 3; count = 1; break;
                        case "long": type = 4; count = 1; break;
                        case "at": type = (ushort)e["type"]!.GetValue<int>(); count = e["count"]!.GetValue<uint>(); break;
                        default: throw new ArgumentException("unknown entry kind " + kind);
                    }

                    if (kind != "at") count = 1;
                    value = e["v_mark"] is { } m
                        ? unchecked((uint)(b.Mark(m.GetValue<string>()) + (e["plus"]?.GetValue<long>() ?? 0)))
                        : e["v"]!.GetValue<uint>();
                }

                int pp = p;
                if (pp + 12 > Buffer.Length) throw new ArgumentException("IFD entry beyond the TIFF buffer");
                U16(pp, tag);
                U16(pp + 2, type);
                U32(pp + 4, count);
                if (type == 3 && count == 1) U16(pp + 8, (ushort)value); // SHORT left-justified, as TiffBytes.Ifd does
                else U32(pp + 8, value);
            }

            int nextAt = at + 2 + (total * 12);
            if (nextAt + 4 <= Buffer.Length) U32(nextAt, next);
        }
    }

    private static byte[] W510bTiffNode(JsonObject o, W510bBuild b)
    {
        bool le = o["le"]?.GetValue<bool>() ?? true;
        var t = new W510bTiff(le, o["size"]!.GetValue<int>());
        ReadOnlySpan<byte> sig = o["signature_hex"] is { } sh ? Convert.FromHexString(sh.GetValue<string>()) : default;
        ReadOnlySpan<byte> magic = sig.IsEmpty ? (le ? "II*\0"u8 : "MM\0*"u8) : sig;
        t.Put(0, magic);
        if (o["magic"] is { } mg) t.U16(2, (ushort)mg.GetValue<int>());
        t.U32(4, o["ifd0"]!.GetValue<uint>());
        foreach (var node in o["ops"]!.AsArray())
        {
            var op = node!.AsObject();
            int at = op["at"]!.GetValue<int>();
            switch (op["op"]!.GetValue<string>())
            {
                case "ifd":
                    t.Ifd(at, op["next"]?.GetValue<uint>() ?? 0, op["entries"]!.AsArray(), op["pad"]?.GetValue<int>() ?? 0, b,
                        op["count"]?.GetValue<int>() ?? -1);
                    break;
                case "u16": t.U16(at, (ushort)op["v"]!.GetValue<int>()); break;
                case "u32": t.U32(at, op["v"]!.GetValue<uint>()); break;
                case "hex": t.Put(at, Convert.FromHexString(op["hex"]!.GetValue<string>())); break;
                default: throw new ArgumentException("unknown tiff op " + op["op"]);
            }
        }

        return t.Buffer;
    }

    private static byte[] W510bBe32(uint v)
    {
        var r = new byte[4];
        BinaryPrimitives.WriteUInt32BigEndian(r, v);
        return r;
    }

    private static byte[] W510bUuidBytes(string which) => which switch
    {
        "canon" => [0x85, 0xC0, 0xB6, 0x87, 0x82, 0x0F, 0x11, 0xE0, 0x81, 0x11, 0xF4, 0xCE, 0x46, 0x2B, 0x6A, 0x48],
        "preview" => [0xEA, 0xF4, 0x2B, 0x5E, 0x1C, 0x98, 0x4B, 0x88, 0xB9, 0xFB, 0xB7, 0xDC, 0x40, 0x6E, 0x4D, 0x16],
        "foreign" => new byte[16],
        _ when which.StartsWith("hex:", StringComparison.Ordinal) => Convert.FromHexString(which[4..]),
        _ => throw new ArgumentException("unknown uuid " + which),
    };

    private static byte[] W510bEmitList(JsonArray nodes, long at, W510bBuild b)
    {
        using var ms = new MemoryStream();
        foreach (var n in nodes)
        {
            var bytes = W510bEmit(n!, at + ms.Length, b);
            ms.Write(bytes);
        }

        return ms.ToArray();
    }

    /// <summary>
    /// The box language. A node is an array (concatenation) or an object with exactly one of these keys:
    /// hex | zeros | ascii (Latin-1) | u16 | u32 | u64 (decimal string) (all big-endian) | mark | off32 | off64 (+ "plus") | u32_len_of (+ "plus") |
    /// jpeg [w,h] | sof3 [w,h] | tiff {..} | box (+ children, size32, large, size64) | uuid (+ children, size32) | repeat (+ node) | slice (an array of nodes, + from, to).
    /// </summary>
    private static byte[] W510bEmit(JsonNode node, long at, W510bBuild b)
    {
        if (node is JsonArray arr) return W510bEmitList(arr, at, b);
        var o = node.AsObject();

        if (o["hex"] is { } hex) return Convert.FromHexString(hex.GetValue<string>());
        if (o["zeros"] is { } z) return new byte[z.GetValue<int>()];
        if (o["ascii"] is { } a) return Encoding.Latin1.GetBytes(a.GetValue<string>());
        if (o["u16"] is { } u16)
        {
            var r = new byte[2];
            BinaryPrimitives.WriteUInt16BigEndian(r, (ushort)u16.GetValue<int>());
            return r;
        }

        if (o["u32"] is { } u32) return W510bBe32(u32.GetValue<uint>());
        if (o["u64"] is { } u64)
        {
            var r = new byte[8];
            BinaryPrimitives.WriteUInt64BigEndian(r, ulong.Parse(u64.GetValue<string>(), System.Globalization.CultureInfo.InvariantCulture));
            return r;
        }

        if (o["mark"] is { } mark)
        {
            b.Marks[mark.GetValue<string>()] = at;
            return [];
        }

        if (o["off32"] is { } off32)
            return W510bBe32(unchecked((uint)(b.Mark(off32.GetValue<string>()) + (o["plus"]?.GetValue<long>() ?? 0))));
        if (o["off64"] is { } off64)
        {
            var r = new byte[8];
            BinaryPrimitives.WriteUInt64BigEndian(r, unchecked((ulong)(b.Mark(off64.GetValue<string>()) + (o["plus"]?.GetValue<long>() ?? 0))));
            return r;
        }

        if (o["u32_len_of"] is { } lenOf)
        {
            var scratch = new W510bBuild { Resolved = b.Resolved };
            long len = W510bEmit(lenOf, at, scratch).Length;
            return W510bBe32(unchecked((uint)(len + (o["plus"]?.GetValue<long>() ?? 0))));
        }

        if (o["jpeg"] is { } jp) return W510bJpeg(jp[0]!.GetValue<int>(), jp[1]!.GetValue<int>());
        if (o["sof3"] is { } s3) return W510bLosslessJpeg(s3[0]!.GetValue<int>(), s3[1]!.GetValue<int>());
        if (o["tiff"] is { } tf) return W510bTiffNode(tf.AsObject(), b);

        if (o["repeat"] is { } rep)
        {
            var one = W510bEmit(o["node"]!, at, new W510bBuild { Resolved = b.Resolved });
            int n = rep.GetValue<int>();
            var all = new byte[one.Length * n];
            for (int i = 0; i < n; i++) one.CopyTo(all, i * one.Length);
            return all;
        }

        if (o["slice"] is { } sl)
        {
            var bytes = W510bEmit(sl, at, b);
            int from = o["from"]?.GetValue<int>() ?? 0;
            int to = o["to"]?.GetValue<int>() ?? bytes.Length;
            return bytes[from..to];
        }

        bool isBox = o["box"] is not null;
        bool isUuid = o["uuid"] is not null;
        if (isBox || isUuid)
        {
            bool large = o["large"]?.GetValue<bool>() ?? false;
            long header = (large ? 16 : 8) + (isUuid ? 16 : 0);
            var kids = o["children"] as JsonArray ?? [];
            byte[] payload = W510bEmitList(kids, at + header, b);
            long total = header + payload.Length;
            string type = isBox ? o["box"]!.GetValue<string>() : "uuid";
            uint size32 = o["size32"] is { } s32 ? s32.GetValue<uint>() : (large ? 1u : (uint)total);

            using var ms = new MemoryStream();
            ms.Write(W510bBe32(size32));
            ms.Write(Encoding.Latin1.GetBytes(type));
            if (large)
            {
                var r = new byte[8];
                BinaryPrimitives.WriteUInt64BigEndian(r, o["size64"] is { } s64
                    ? ulong.Parse(s64.GetValue<string>(), System.Globalization.CultureInfo.InvariantCulture)
                    : (ulong)total); // largesize counts the whole box, uuid bytes included (BmffBoxNavigator.TryReadBox)
                ms.Write(r);
            }

            if (isUuid) ms.Write(W510bUuidBytes(o["uuid"]!.GetValue<string>()));
            ms.Write(payload);
            return ms.ToArray();
        }

        throw new ArgumentException("unknown node " + o.ToJsonString());
    }

    private static string W510bRepoRoot = ".";

    private static byte[] W510bBuildSource(JsonObject src)
    {
        switch (src["kind"]!.GetValue<string>())
        {
            case "nodes":
            {
                var nodes = src["nodes"]!.AsArray();
                var pass1 = new W510bBuild();
                W510bEmitList(nodes, 0, pass1);
                var pass2 = new W510bBuild { Resolved = pass1.Marks };
                byte[] data = W510bEmitList(nodes, 0, pass2);
                if (src["truncate_to"] is { } tt) data = data[..tt.GetValue<int>()];
                return data;
            }
            case "corpus":
            {
                string dir = Environment.GetEnvironmentVariable("PHOTOREVIEW_RS_RAW_CORPUS") is { Length: > 0 } env
                    ? env
                    : Path.Combine(W510bRepoRoot, "tests", "fixtures", "raw-corpus");
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

    private static string W510bFnv1a64(byte[] data)
    {
        ulong h = 14695981039346656037UL;
        foreach (byte x in data)
        {
            h ^= x;
            h = unchecked(h * 1099511628211UL);
        }

        return h.ToString("x16", System.Globalization.CultureInfo.InvariantCulture);
    }

    private static JsonObject W510bPreview(EmbeddedPreview p) => new()
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

    private static JsonObject W510bBlock(ExifBlock b) => new()
    {
        ["offset"] = b.Offset,
        ["length"] = b.Length,
        ["is_tiff_header"] = b.IsTiffHeader,
        ["ifd_is_exif"] = b.IfdIsExif,
    };

    /// <summary>
    /// cases: {name, op: "can-read"|"read" (default "read"), ext (default ".cr3"), source: {kind: nodes|corpus, ...}, cancel?: bool, optional?: bool}.
    /// "can-read": Cr3ContainerReader.CanRead(first min(64, len) bytes of the source, ext). "read": CanRead as well (reported as can_read) and then
    /// Read(new W510bSource, token) (token already canceled when "cancel"), reporting the container info or {exception, reader_message} and
    /// bytes_charged. Every row echoes name, op, ext, cancel and source, and reports file_length and file_fnv1a64 of the built bytes.
    /// </summary>
    public static JsonArray RawCr3Reader(JsonObject inputs, string inputPath)
    {
        W510bRepoRoot = Path.GetFullPath(Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!, "..", ".."));
        var result = new JsonArray();
        var reader = new Cr3ContainerReader();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            string name = c["name"]!.GetValue<string>();
            string ext = c["ext"]?.GetValue<string>() ?? ".cr3";
            string op = c["op"]?.GetValue<string>() ?? "read";
            bool optional = c["optional"]?.GetValue<bool>() ?? false;
            bool cancel = c["cancel"]?.GetValue<bool>() ?? false;

            byte[] data;
            try
            {
                data = W510bBuildSource(c["source"]!.AsObject());
            }
            catch (FileNotFoundException) when (optional)
            {
                result.Add(new JsonObject { ["name"] = name, ["skipped"] = true });
                continue;
            }

            var o = new JsonObject
            {
                ["name"] = name,
                ["op"] = op,
                ["ext"] = ext,
                ["cancel"] = cancel,
                ["source"] = c["source"]!.DeepClone(),
                ["file_length"] = data.Length,
                ["file_fnv1a64"] = W510bFnv1a64(data),
            };
            o["can_read"] = reader.CanRead(data.AsSpan(0, Math.Min(64, data.Length)), ext);
            if (op == "read")
            {
                var source = new W510bSource(data);
                using var cts = new CancellationTokenSource();
                if (cancel) cts.Cancel();
                try
                {
                    var info = reader.Read(source, cts.Token);
                    o["ok"] = true;
                    o["format"] = info.Format.ToString();
                    o["sensor_width"] = info.SensorWidth;
                    o["sensor_height"] = info.SensorHeight;
                    o["orientation"] = info.Orientation;
                    o["previews"] = new JsonArray(info.Previews.Select(p => (JsonNode)W510bPreview(p)).ToArray());
                    o["exif_blocks"] = new JsonArray(info.ExifBlocks.Select(b => (JsonNode)W510bBlock(b)).ToArray());
                }
                catch (Exception ex)
                {
                    o["ok"] = false;
                    o["exception"] = ex.GetType().Name;
                    // Only the text the reader itself raises is pinned; source messages are not part of the contract.
                    o["reader_message"] = ex.Message == "File too short for CR3 container." ? ex.Message : null;
                    o["io_failure"] = ex.InnerException is IOException or ObjectDisposedException;
                }

                o["bytes_charged"] = source.TotalBytesRead;
            }

            result.Add(o);
        }

        return result;
    }
}
