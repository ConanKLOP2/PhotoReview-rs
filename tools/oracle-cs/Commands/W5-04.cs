using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;
using PhotoReview.Imaging.Raw.Tiff;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-04
//     "nef-container" => Commands.NefContainer(inputs),
// Wiring needed once for ALL Wave 5 reader oracles (do it in the first W5 oracle that lands):
//   * tools/oracle-cs/OracleCs.csproj: <Reference Include="PhotoReview.Imaging.Raw"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll</HintPath></Reference>
//   * tools/oracle.ps1: also `dotnet build src\PhotoReview.Imaging.Raw\PhotoReview.Imaging.Raw.csproj -c Release -o $coreDir`
//     and add PhotoReview.Imaging.Raw.dll to the "rebuild if missing" test. Raw references Core and Imaging only.
// Everything used here is public (NefContainerReader, RawContainerInfo, EmbeddedPreview, ExifBlock, SourceRawHeaderSource), so no
// InternalsVisibleTo is needed. The header source is the PRODUCTION SourceRawHeaderSource over a MemoryStream / FileStream, so the
// 64 KiB block charging and the 8 MiB budget are the real ones and "bytes_read" is its TotalBytesRead.
// Corpus files (op "read-corpus") live in <tests/fixtures>/<corpus_dir> (default tests/fixtures/raw-corpus, gitignored, fetched by the
// lead's tools/fetch-raw-samples.ps1 port); env PHOTOREVIEW_RS_RAW_CORPUS overrides the directory. A missing file fails the run unless
// the case says "optional": true, in which case the golden holds { present: false } and the Rust test skips it.

// W5-04: NefContainerReader. One command, one case per entry. Case = { name, op, ... }:
//   can-read     head_hex, ext                                  -> { can_read }
//   read         data                                           -> { outcome: "ok", info... } | { outcome: "throws", throws: "<ExceptionType>" } (+ bytes_read)
//   read-corpus  file_contains, optional?                       -> { present, file_name, file_length, ...as read }
// "data" = { len: int | "fit", le: bool, patches: [...] }: len zero bytes (or, for "fit", exactly as long as the last patch ends)
// with the patches applied IN ORDER. Patch kinds (all numbers decimal or "0x.." strings; "le" of the data node decides byte order):
//   { at, hex }                                   raw bytes
//   { header: { ifd0, sig_hex? } }                bytes 0..8: sig (default II*\0 / MM\0*) + u32 ifd0 offset
//   { ifd: { at, next, entries: [[tag,type,count,value],...] } }
//                                                 u16 count, 12-byte entries (value is written as u16 for SHORT x1, else u32), u32 next
//   { u32: { at, v } }                            one u32
//   { at, jpeg: { w, h, pad?, lossless?, sof?, precision?, corrupt_sof?, clip? } }
//                                                 SyntheticRawBuilder.CreateMinimalJpeg(w, h) (163 bytes) + options: pad zero bytes after EOI;
//                                                 lossless = first FFC0 -> FFC3; sof = FFC0 -> FF(C0+sof); precision = SOF sample precision byte;
//                                                 corrupt_sof = SOF width/height zeroed; clip = silently cut at the end of the buffer
//   { at, budget_jpeg: { segments } }             SOI + <segments> x (FFE0 FFFF + 65531 zero bytes) + the 320x240 minimal JPEG without its SOI
//                                                 (the TiffPreviewHardeningTests.BudgetExhaustingJpeg shape: its frame header lies beyond the 8 MiB budget)
// Output of a successful read: format, sensor_width, sensor_height, orientation, previews[{index, offset, length, kind, width, height,
// color_space, header_resolved}], exif_blocks[{offset, length, is_tiff_header, ifd_is_exif}].
static partial class Commands
{
    private static long W504Num(JsonNode? node)
    {
        if (node is JsonValue v && v.TryGetValue<string>(out var s))
            return s.StartsWith("0x", StringComparison.OrdinalIgnoreCase) ? Convert.ToInt64(s[2..], 16) : long.Parse(s);
        return node!.GetValue<long>();
    }

    private static byte[] W504MinimalJpeg(int width, int height)
    {
        // Byte for byte SyntheticRawBuilder.CreateMinimalJpeg (tests/PhotoReview.Imaging.Tests/Raw/SyntheticRawBuilder.cs at the baseline).
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

    private static byte[] W504Jpeg(JsonObject o)
    {
        var bytes = W504MinimalJpeg(o["w"]!.GetValue<int>(), o["h"]!.GetValue<int>());
        int sof = -1;
        for (int i = 0; i + 1 < bytes.Length && sof < 0; i++)
            if (bytes[i] == 0xFF && bytes[i + 1] == 0xC0) sof = i;
        if (o["lossless"]?.GetValue<bool>() == true) bytes[sof + 1] = 0xC3;
        if (o["sof"] is { } s) bytes[sof + 1] = (byte)(0xC0 + s.GetValue<int>());
        if (o["precision"] is { } p) bytes[sof + 4] = (byte)p.GetValue<int>();
        if (o["corrupt_sof"]?.GetValue<bool>() == true) bytes.AsSpan(sof + 5, 4).Clear();
        int pad = o["pad"]?.GetValue<int>() ?? 0;
        return pad == 0 ? bytes : [.. bytes, .. new byte[pad]];
    }

    private static byte[] W504BudgetJpeg(int segments)
    {
        var tail = W504MinimalJpeg(320, 240);
        using var ms = new MemoryStream();
        ms.Write([0xFF, 0xD8]);
        var app = new byte[2 + 65535];
        app[0] = 0xFF; app[1] = 0xE0; app[2] = 0xFF; app[3] = 0xFF;
        for (int i = 0; i < segments; i++) ms.Write(app);
        ms.Write(tail, 2, tail.Length - 2);
        return ms.ToArray();
    }

    private static byte[] W504Build(JsonObject data)
    {
        bool le = data["le"]!.GetValue<bool>();
        bool fit = data["len"] is JsonValue lv && lv.TryGetValue<string>(out var ls) && ls == "fit";
        byte[] buf = fit ? [] : new byte[data["len"]!.GetValue<int>()];

        void Ensure(long end)
        {
            if (end <= buf.Length) return;
            if (!fit) throw new ArgumentException($"patch ends at {end}, beyond the {buf.Length}-byte buffer");
            Array.Resize(ref buf, checked((int)end));
        }

        void Put(long at, ReadOnlySpan<byte> bytes, bool clip = false)
        {
            if (clip && !fit && at + bytes.Length > buf.Length) bytes = bytes[..(int)(buf.Length - at)];
            Ensure(at + bytes.Length);
            bytes.CopyTo(buf.AsSpan((int)at));
        }

        void U16(long at, ushort v) { Span<byte> b = stackalloc byte[2]; if (le) System.Buffers.Binary.BinaryPrimitives.WriteUInt16LittleEndian(b, v); else System.Buffers.Binary.BinaryPrimitives.WriteUInt16BigEndian(b, v); Put(at, b); }
        void U32(long at, uint v) { Span<byte> b = stackalloc byte[4]; if (le) System.Buffers.Binary.BinaryPrimitives.WriteUInt32LittleEndian(b, v); else System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(b, v); Put(at, b); }

        foreach (var patchNode in data["patches"]!.AsArray())
        {
            var patch = patchNode!.AsObject();
            if (patch["header"] is { } header)
            {
                var h = header.AsObject();
                var sig = h["sig_hex"] is { } sh ? Convert.FromHexString(sh.GetValue<string>()) : (le ? "II*\0"u8.ToArray() : "MM\0*"u8.ToArray());
                Put(0, sig);
                U32(4, (uint)W504Num(h["ifd0"]));
            }
            else if (patch["ifd"] is { } ifdNode)
            {
                var ifd = ifdNode.AsObject();
                long at = W504Num(ifd["at"]);
                var entries = ifd["entries"]!.AsArray();
                U16(at, (ushort)entries.Count);
                for (int i = 0; i < entries.Count; i++)
                {
                    var e = entries[i]!.AsArray();
                    long p = at + 2 + (i * 12L);
                    ushort type = (ushort)W504Num(e[1]);
                    uint count = (uint)W504Num(e[2]);
                    long value = W504Num(e[3]);
                    U16(p, (ushort)W504Num(e[0]));
                    U16(p + 2, type);
                    U32(p + 4, count);
                    if (type == 3 && count == 1) U16(p + 8, (ushort)value); // SHORT x1 is left-justified (TiffBytes.Ifd)
                    else U32(p + 8, (uint)value);
                }
                U32(at + 2 + (entries.Count * 12L), (uint)W504Num(ifd["next"]));
            }
            else if (patch["u32"] is { } u)
            {
                U32(W504Num(u["at"]), (uint)W504Num(u["v"]));
            }
            else if (patch["jpeg"] is { } j)
            {
                Put(W504Num(patch["at"]), W504Jpeg(j.AsObject()), j["clip"]?.GetValue<bool>() == true);
            }
            else if (patch["budget_jpeg"] is { } b)
            {
                Put(W504Num(patch["at"]), W504BudgetJpeg(b["segments"]!.GetValue<int>()));
            }
            else
            {
                Put(W504Num(patch["at"]), Convert.FromHexString(patch["hex"]!.GetValue<string>()));
            }
        }

        return buf;
    }

    private static JsonObject W504Describe(RawContainerInfo info) => new()
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

    private static JsonObject W504ReadStream(Stream stream)
    {
        var result = new JsonObject();
        using var source = new SourceRawHeaderSource(stream);
        try
        {
            var info = new NefContainerReader().Read(source, CancellationToken.None);
            result["outcome"] = "ok";
            foreach (var (key, value) in W504Describe(info)) result[key] = value?.DeepClone();
        }
        catch (Exception ex)
        {
            result["outcome"] = "throws";
            result["throws"] = ex.GetType().Name;
        }

        result["bytes_read"] = source.TotalBytesRead;
        return result;
    }

    public static JsonArray NefContainer(JsonObject inputs)
    {
        var result = new JsonArray();
        string corpusDir = Environment.GetEnvironmentVariable("PHOTOREVIEW_RS_RAW_CORPUS")
            ?? Path.GetFullPath(Path.Combine(PerfCommands.FixturesDir, inputs["corpus_dir"]?.GetValue<string>() ?? "raw-corpus"));
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = c["op"]!.GetValue<string>() };
            foreach (var (key, value) in c)
                if (key is not ("name" or "op")) o[key] = value?.DeepClone();

            switch (c["op"]!.GetValue<string>())
            {
                case "can-read":
                {
                    var head = Convert.FromHexString(c["head_hex"]!.GetValue<string>());
                    o["can_read"] = new NefContainerReader().CanRead(head, c["ext"]!.GetValue<string>());
                    break;
                }
                case "read":
                {
                    var bytes = W504Build(c["data"]!.AsObject());
                    foreach (var (key, value) in W504ReadStream(new MemoryStream(bytes, writable: false))) o[key] = value?.DeepClone();
                    break;
                }
                case "read-corpus":
                {
                    string contains = c["file_contains"]!.GetValue<string>();
                    var file = Directory.Exists(corpusDir)
                        ? Directory.GetFiles(corpusDir, "*.nef")
                            .OrderBy(f => f, StringComparer.OrdinalIgnoreCase)
                            .FirstOrDefault(f => Path.GetFileName(f).Contains(contains, StringComparison.Ordinal))
                        : null;
                    if (file is null)
                    {
                        if (c["optional"]?.GetValue<bool>() != true)
                            throw new FileNotFoundException($"RAW corpus sample '*.nef' containing '{contains}' not found in {corpusDir}");
                        o["present"] = false;
                        break;
                    }

                    o["present"] = true;
                    o["file_name"] = Path.GetFileName(file);
                    o["file_length"] = new FileInfo(file).Length;
                    foreach (var (key, value) in W504ReadStream(new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.Read)))
                        o[key] = value?.DeepClone();
                    break;
                }
                default:
                    throw new ArgumentException("unknown op " + c["op"]);
            }

            result.Add(o);
        }

        return result;
    }
}
