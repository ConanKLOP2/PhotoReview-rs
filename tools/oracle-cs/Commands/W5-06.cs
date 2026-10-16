using System.Globalization;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;
using PhotoReview.Imaging.Raw.Tiff;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-06
//     "raw-dng-synthetic" => Commands.RawDngSynthetic(inputs),
//     "raw-dng-corpus" => Commands.RawDngCorpus(inputs),
//
// Wiring needs (do these in the wiring step, not here):
//   * OracleCs.csproj: add <Reference Include="PhotoReview.Imaging.Raw"> with a HintPath to the baseline build of
//     PhotoReview.Imaging.Raw.dll (tools/oracle.ps1 must build src\PhotoReview.Imaging.Raw next to PhotoReview.Core.dll).
//     Only PUBLIC members are used (DngContainerReader, SourceRawHeaderSource(Stream), RawContainerInfo, EmbeddedPreview,
//     ExifBlock), so no InternalsVisibleTo is needed. The oracle files of the other W5 readers share the same reference;
//     the helpers below carry the W506 prefix so the files do not collide.
//   * Outputs: tests/golden/raw-dng-synthetic.json, tests/golden/raw-dng-corpus.json. Inputs: golden-inputs/ of this package.
//   * raw-dng-corpus reads the real-camera corpus from the directory in the environment variable PHOTOREVIEW_RAW_CORPUS
//     (the C# repo's tests/Fixtures/raw-corpus after tools/fetch-raw-samples.ps1). Without it the golden holds one
//     "corpus_present": false case and the Rust corpus twin only checks the C# literals; regenerate with the corpus.
//
// Case language of raw-dng-synthetic (the Rust golden test has the same builder, tests/dng_reader_golden.rs):
//   { name, ext (default ".dng"), le (default true), size, ifd0 (default 8), signature_hex (4 bytes, optional),
//     ifds: [ { at, next (default 0), entries: [ {k:"short"|"long"|"at", tag:"0100" (hex), v | type,count,off } ] } ],
//     u32: [[pos, value]...], u16: [[pos, value]...], puts: [ { at, hex } ],
//     jpegs: [ { at, w, h, sof: "c0"|"c3", precision: 8, fill: 0, budget: false } ],
//     gen: "chain-66" | "subifd-65" | "sub64-chain" (replaces ifds/size/ifd0 with the hostile layouts of the C# tests) }
// Writes happen in the order: header, ifds, u32, u16, jpegs, puts (later writes win: "puts" patch bytes inside a jpeg). "short" is written left-justified as a
// 2-byte value (the C# TiffBytes.Ifd rule); every other entry writes the 4 value bytes.

// W5-06: DngContainerReader.CanRead / Read over synthetic TIFF layouts and over the real DNG corpus.
static partial class Commands
{
    // ------------------------------------------------------------------ byte builder (port of the test helper TiffBytes)

    private sealed class W506Buf
    {
        public readonly byte[] B;
        private readonly bool _le;

        public W506Buf(bool le, int size)
        {
            _le = le;
            B = new byte[size];
        }

        public void Header(uint ifd0, byte[]? signature)
        {
            byte[] magic = signature ?? (_le ? [0x49, 0x49, 0x2A, 0x00] : [0x4D, 0x4D, 0x00, 0x2A]);
            magic.CopyTo(B, 0);
            U32(4, ifd0);
        }

        public void Put(int pos, byte[] bytes) => bytes.CopyTo(B, pos);

        public void U16(int pos, ushort v)
        {
            if (_le) System.Buffers.Binary.BinaryPrimitives.WriteUInt16LittleEndian(B.AsSpan(pos), v);
            else System.Buffers.Binary.BinaryPrimitives.WriteUInt16BigEndian(B.AsSpan(pos), v);
        }

        public void U32(int pos, uint v)
        {
            if (_le) System.Buffers.Binary.BinaryPrimitives.WriteUInt32LittleEndian(B.AsSpan(pos), v);
            else System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(B.AsSpan(pos), v);
        }

        public void Ifd(int pos, uint next, IReadOnlyList<(ushort Tag, ushort Type, uint Count, uint Value)> entries)
        {
            U16(pos, (ushort)entries.Count);
            for (int i = 0; i < entries.Count; i++)
            {
                int at = pos + 2 + (i * 12);
                var e = entries[i];
                U16(at, e.Tag);
                U16(at + 2, e.Type);
                U32(at + 4, e.Count);
                if (e.Type == 3 && e.Count == 1) U16(at + 8, (ushort)e.Value);
                else U32(at + 8, e.Value);
            }
            U32(pos + 2 + (entries.Count * 12), next);
        }
    }

    private static (ushort, ushort, uint, uint) W506Short(ushort tag, uint v) => (tag, 3, 1, v);
    private static (ushort, ushort, uint, uint) W506Long(ushort tag, uint v) => (tag, 4, 1, v);
    private static (ushort, ushort, uint, uint) W506At(ushort tag, ushort type, uint count, uint off) => (tag, type, count, off);

    private static (ushort, ushort, uint, uint)[] W506Image(int w, int h, ushort photometric) =>
        [W506Short(0x0100, (ushort)w), W506Short(0x0101, (ushort)h), W506Short(0x0106, photometric)];

    /// <summary>SyntheticRawBuilder.CreateMinimalJpeg (SOI, APP0, DQT, SOF0, DHT, SOS, 6 data bytes, EOI).</summary>
    private static byte[] W506MinimalJpeg(int width, int height)
    {
        using var ms = new MemoryStream();
        ms.Write([0xFF, 0xD8]);
        ms.Write([0xFF, 0xE0, 0x00, 0x10]);
        ms.Write(System.Text.Encoding.ASCII.GetBytes("JFIF\0"));
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

    private static byte[] W506Jpeg(JsonObject spec)
    {
        int w = spec["w"]!.GetValue<int>();
        int h = spec["h"]!.GetValue<int>();
        if (spec["budget"]?.GetValue<bool>() == true)
        {
            // 140 maximal APP0 segments, one 64 KiB block apart: walking to the SOF charges more than the 8 MiB header budget.
            var tailJpeg = W506MinimalJpeg(w, h);
            using var big = new MemoryStream();
            big.Write([0xFF, 0xD8]);
            var app = new byte[2 + 65535];
            app[0] = 0xFF;
            app[1] = 0xE0;
            app[2] = 0xFF;
            app[3] = 0xFF;
            for (int i = 0; i < 140; i++) big.Write(app);
            big.Write(tailJpeg, 2, tailJpeg.Length - 2);
            return big.ToArray();
        }

        var jpeg = W506MinimalJpeg(w, h);
        int sof = Array.FindIndex(jpeg, i => i == 0xC0) - 1; // FF C0
        if ((spec["sof"]?.GetValue<string>() ?? "c0") == "c3") jpeg[sof + 1] = 0xC3;
        jpeg[sof + 4] = (byte)(spec["precision"]?.GetValue<int>() ?? 8);
        int fill = spec["fill"]?.GetValue<int>() ?? 0;
        return [.. jpeg[..2], .. Enumerable.Repeat((byte)0xFF, fill), .. jpeg[2..]];
    }

    private static (ushort, ushort, uint, uint) W506Entry(JsonObject e)
    {
        ushort tag = Convert.ToUInt16(e["tag"]!.GetValue<string>(), 16);
        return e["k"]!.GetValue<string>() switch
        {
            "short" => W506Short(tag, (uint)e["v"]!.GetValue<long>()),
            "long" => W506Long(tag, (uint)e["v"]!.GetValue<long>()),
            "at" => W506At(tag, (ushort)e["type"]!.GetValue<int>(), (uint)e["count"]!.GetValue<long>(), (uint)e["off"]!.GetValue<long>()),
            var k => throw new ArgumentException("unknown entry kind " + k),
        };
    }

    private static byte[] W506Build(JsonObject c)
    {
        bool le = c["le"]?.GetValue<bool>() ?? true;
        switch (c["gen"]?.GetValue<string>())
        {
            case "chain-66":
            {
                // IFD i is 100+i wide; IFD 65 is never reached (the chain stops after 64 IFDs, the last next pointer registers the 65th).
                const int ifdSize = 42;
                var f = new W506Buf(true, 8 + (66 * ifdSize));
                f.Header(8, null);
                for (int i = 0; i < 66; i++)
                {
                    int at = 8 + (i * ifdSize);
                    f.Ifd(at, i < 65 ? (uint)(at + ifdSize) : 0, W506Image(100 + i, 100, 32803));
                }
                return f.B;
            }
            case "subifd-65":
            {
                const int ifdSize = 42, arrayAt = 80, chainIfdAt = 340, subsAt = 400;
                var f = new W506Buf(true, subsAt + (65 * ifdSize) + 8);
                f.Header(8, null);
                f.Ifd(8, chainIfdAt, [.. W506Image(10, 10, 2), W506At(0x014A, 4, 64, arrayAt)]);
                f.Ifd(chainIfdAt, 0, [W506Long(0x014A, (uint)(subsAt + (64 * ifdSize)))]);
                for (int i = 0; i < 64; i++) f.U32(arrayAt + (i * 4), (uint)(subsAt + (i * ifdSize)));
                for (int i = 0; i < 64; i++) f.Ifd(subsAt + (i * ifdSize), 0, W506Image(50, 50, 32803));
                f.Ifd(subsAt + (64 * ifdSize), 0, W506Image(5000, 4000, 32803));
                return f.B;
            }
            case "sub64-chain":
            {
                const int subCount = 64;
                var f = new W506Buf(true, 4000);
                f.Header(8, null);
                f.Ifd(8, 2000, [W506At(0x014A, 4, subCount, 1000)]);
                f.Ifd(2000, 2100, [W506Short(0x0100, 100), W506Short(0x0101, 80)]);
                f.Ifd(2100, 0, [W506Long(0x00FE, 0), W506Short(0x0100, 4000), W506Short(0x0101, 3000), W506Short(0x0106, 32803)]);
                for (int i = 0; i < subCount; i++)
                {
                    f.U32(1000 + (i * 4), (uint)(2200 + (i * 8)));
                    f.Ifd(2200 + (i * 8), 0, []);
                }
                return f.B;
            }
        }

        var buf = new W506Buf(le, c["size"]!.GetValue<int>());
        byte[]? signature = c["signature_hex"] is { } sh ? Convert.FromHexString(sh.GetValue<string>()) : null;
        buf.Header((uint)(c["ifd0"]?.GetValue<long>() ?? 8), signature);
        foreach (var node in c["ifds"]?.AsArray() ?? [])
        {
            var i = node!.AsObject();
            var entries = i["entries"]!.AsArray().Select(e => W506Entry(e!.AsObject())).ToList();
            buf.Ifd(i["at"]!.GetValue<int>(), (uint)(i["next"]?.GetValue<long>() ?? 0), entries);
        }
        foreach (var p in c["u32"]?.AsArray() ?? []) buf.U32(p![0]!.GetValue<int>(), (uint)p[1]!.GetValue<long>());
        foreach (var p in c["u16"]?.AsArray() ?? []) buf.U16(p![0]!.GetValue<int>(), (ushort)p[1]!.GetValue<int>());
        foreach (var j in c["jpegs"]?.AsArray() ?? []) buf.Put(j!["at"]!.GetValue<int>(), W506Jpeg(j.AsObject()));
        foreach (var p in c["puts"]?.AsArray() ?? []) buf.Put(p!["at"]!.GetValue<int>(), Convert.FromHexString(p["hex"]!.GetValue<string>()));
        return buf.B;
    }

    // ------------------------------------------------------------------ reading and reporting

    private static JsonNode W506Info(RawContainerInfo info) => new JsonObject
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

    /// <summary>Adds can_read, then either "info" or "throws" (+ "reader_message" for the two messages DngContainerReader itself raises).</summary>
    private static void W506Read(JsonObject result, byte[] bytes, string ext)
    {
        var reader = new DngContainerReader();
        result["file_length"] = bytes.Length;
        result["can_read"] = reader.CanRead(bytes.AsSpan(0, Math.Min(64, bytes.Length)), ext);
        try
        {
            using var source = new SourceRawHeaderSource(new MemoryStream(bytes, writable: false));
            result["info"] = W506Info(reader.Read(source, CancellationToken.None));
        }
        catch (Exception ex)
        {
            result["throws"] = ex.GetType().Name;
            result["reader_message"] = ex is InvalidDataException && (ex.Message.StartsWith("File too short", StringComparison.Ordinal) || ex.Message.StartsWith("Invalid DNG", StringComparison.Ordinal))
                ? ex.Message
                : null;
        }
    }

    /// <summary>Each case of the language in the file header: the byte layout is built, CanRead and Read are run on it.</summary>
    public static JsonArray RawDngSynthetic(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var ext = c["ext"]?.GetValue<string>() ?? ".dng";
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["ext"] = ext };
            foreach (var kv in c)
            {
                if (kv.Key is not "name" and not "ext") o[kv.Key] = kv.Value?.DeepClone();
            }
            W506Read(o, W506Build(c), ext);
            result.Add(o);
        }
        return result;
    }

    /// <summary>All corpus files with one of "extensions" (default [".dng"]), sorted by name (OrdinalIgnoreCase); one case per file.</summary>
    public static JsonArray RawDngCorpus(JsonObject inputs)
    {
        var result = new JsonArray();
        var dir = Environment.GetEnvironmentVariable("PHOTOREVIEW_RAW_CORPUS");
        var extensions = (inputs["extensions"]?.AsArray().Select(e => e!.GetValue<string>()) ?? [".dng"]).ToList();
        if (string.IsNullOrEmpty(dir) || !Directory.Exists(dir))
        {
            result.Add(new JsonObject { ["name"] = "corpus-absent", ["corpus_present"] = false });
            return result;
        }

        var files = Directory.GetFiles(dir)
            .Where(f => extensions.Any(x => Path.GetExtension(f).Equals(x, StringComparison.OrdinalIgnoreCase)))
            .OrderBy(f => Path.GetFileName(f), StringComparer.OrdinalIgnoreCase);
        foreach (var file in files)
        {
            var o = new JsonObject { ["name"] = Path.GetFileName(file), ["corpus_present"] = true };
            var reader = new DngContainerReader();
            using var fs = File.OpenRead(file);
            var head = new byte[64];
            int n = fs.Read(head, 0, head.Length);
            fs.Position = 0;
            o["file_length"] = fs.Length;
            o["can_read"] = reader.CanRead(head.AsSpan(0, n), Path.GetExtension(file));
            try
            {
                using var source = new SourceRawHeaderSource(fs);
                o["info"] = W506Info(reader.Read(source, CancellationToken.None));
            }
            catch (Exception ex)
            {
                o["throws"] = ex.GetType().Name;
            }
            result.Add(o);
        }
        return result;
    }
}
