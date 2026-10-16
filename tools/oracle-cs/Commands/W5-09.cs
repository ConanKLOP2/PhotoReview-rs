using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;
using PhotoReview.Imaging.Raw.Raf;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-09
//     "raf-container" => Commands.RafContainerCases(inputs),
//     "raf-can-read" => Commands.RafCanReadCases(inputs),
//     "raf-corpus" => Commands.RafCorpusCases(inputs),
// Wiring: tools/oracle-cs/OracleCs.csproj needs the reference (shared with the other W5 oracles):
//     <Reference Include="PhotoReview.Imaging.Raw"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll</HintPath></Reference>
// and tools/oracle.ps1 must build/copy PhotoReview.Imaging.Raw.dll next to the other baseline DLLs.
// Everything used here is public (RafContainerReader, SourceRawHeaderSource, RawContainerInfo, EmbeddedPreview, ExifBlock).
// raf-corpus additionally needs the environment variable PHOTOREVIEW_RAW_CORPUS = folder holding the three RAF samples
// (fetched with the C# tools/fetch-raw-samples.ps1 -TargetDir <folder> -FormatFilter RAF; the oracle never fetches).

// W5-09: RafContainerReader.CanRead / Read over synthetic byte buffers and over the real RAF corpus.
static partial class Commands
{
    /// <summary>
    /// Builds the file bytes of a case from "parts" (concatenated in order) and "patches", then applies "truncate_to".
    /// Part kinds (each may carry "repeat": n copies of the unit and "pad_to": zero-pad the unit to that many bytes):
    ///   {hex}  {ascii}  {zeros: n}
    ///   {raf_header: {jpeg_offset, jpeg_length, cfa_offset, cfa_length}}  100 bytes: "FUJIFILMCCD-RAW " + "0201" + "FF-TEST\0",
    ///                                                                     zeros up to 84, then the four big-endian uint32
    ///   {cfa: {count?, records: [{tag, height, width, size?}]}}  count (default records.Length) as BE uint32, then per record
    ///                                                              tag u16, size u16 (default 4), data = height u16 + width u16
    ///                                                              cut or zero-padded to size
    ///   {pad_to_total: n}  zero-pads the whole buffer so far up to n bytes (no-op when already longer)
    /// Patches {offset, hex | u32 | u16} overwrite bytes in place (u32/u16 big-endian); the offset must lie inside the buffer.
    /// </summary>
    private static byte[] W509Build(JsonObject c)
    {
        var all = new List<byte>();
        foreach (var node in c["parts"]!.AsArray())
        {
            var p = node!.AsObject();
            var unit = new List<byte>();
            if (p["hex"] is { } h) unit.AddRange(Convert.FromHexString(h.GetValue<string>()));
            else if (p["ascii"] is { } a) unit.AddRange(Encoding.Latin1.GetBytes(a.GetValue<string>()));
            else if (p["zeros"] is { } z) unit.AddRange(new byte[z.GetValue<int>()]);
            else if (p["raf_header"] is { } rh)
            {
                var o = rh.AsObject();
                unit.AddRange(Encoding.ASCII.GetBytes("FUJIFILMCCD-RAW 0201FF-TEST\0"));
                while (unit.Count < 84) unit.Add(0);
                foreach (var key in new[] { "jpeg_offset", "jpeg_length", "cfa_offset", "cfa_length" })
                    W509AddU32(unit, o[key]!.GetValue<long>());
            }
            else if (p["cfa"] is { } cf)
            {
                var o = cf.AsObject();
                var records = o["records"]!.AsArray();
                W509AddU32(unit, o["count"]?.GetValue<long>() ?? records.Count);
                foreach (var rn in records)
                {
                    var r = rn!.AsObject();
                    int size = r["size"]?.GetValue<int>() ?? 4;
                    W509AddU16(unit, r["tag"]!.GetValue<int>());
                    W509AddU16(unit, size);
                    var data = new List<byte>();
                    W509AddU16(data, r["height"]?.GetValue<int>() ?? 0);
                    W509AddU16(data, r["width"]?.GetValue<int>() ?? 0);
                    while (data.Count < size) data.Add(0);
                    unit.AddRange(data.Take(size));
                }
            }
            else if (p["pad_to_total"] is { } pt)
            {
                while (all.Count < pt.GetValue<int>()) all.Add(0);
                continue;
            }
            else throw new ArgumentException("unknown part in case " + c["name"]);

            if (p["pad_to"] is { } pad) while (unit.Count < pad.GetValue<int>()) unit.Add(0);
            int repeat = p["repeat"]?.GetValue<int>() ?? 1;
            for (int i = 0; i < repeat; i++) all.AddRange(unit);
        }

        var bytes = all.ToArray();
        if (c["patches"] is { } patches)
        {
            foreach (var pn in patches.AsArray())
            {
                var p = pn!.AsObject();
                int offset = p["offset"]!.GetValue<int>();
                byte[] value;
                if (p["hex"] is { } h) value = Convert.FromHexString(h.GetValue<string>());
                else if (p["u32"] is { } u32) { value = new byte[4]; BinaryPrimitives.WriteUInt32BigEndian(value, (uint)u32.GetValue<long>()); }
                else if (p["u16"] is { } u16) { value = new byte[2]; BinaryPrimitives.WriteUInt16BigEndian(value, (ushort)u16.GetValue<int>()); }
                else throw new ArgumentException("unknown patch in case " + c["name"]);
                value.CopyTo(bytes, offset);
            }
        }

        if (c["truncate_to"] is { } t) bytes = bytes.AsSpan(0, t.GetValue<int>()).ToArray();
        return bytes;
    }

    private static void W509AddU16(List<byte> list, int value)
    {
        Span<byte> b = stackalloc byte[2];
        BinaryPrimitives.WriteUInt16BigEndian(b, (ushort)value);
        list.AddRange(b.ToArray());
    }

    private static void W509AddU32(List<byte> list, long value)
    {
        Span<byte> b = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32BigEndian(b, (uint)value);
        list.AddRange(b.ToArray());
    }

    private static readonly string[] W509ReaderMessages = ["File too short for RAF container.", "Invalid RAF magic header."];

    /// <summary>
    /// Reads with the production SourceRawHeaderSource over the stream and fills the shared result fields:
    /// ok, bytes_charged (TotalBytesRead, also after a failure) and either the info fields or error {type, origin, message?}.
    /// origin is "reader" for the two messages the reader itself raises (their text is pinned), "source" for anything the
    /// header source raises (the Rust port compares the kind, not the text: InMemoryRawHeaderSource words them its own way).
    /// </summary>
    private static void W509ReadInto(JsonObject row, Stream stream, bool cancelled)
    {
        using var source = new SourceRawHeaderSource(stream);
        var token = cancelled ? new CancellationToken(canceled: true) : CancellationToken.None;
        try
        {
            var info = new RafContainerReader().Read(source, token);
            row["ok"] = true;
            row["format"] = info.Format.ToString();
            row["sensor_width"] = info.SensorWidth;
            row["sensor_height"] = info.SensorHeight;
            row["orientation"] = info.Orientation;
            var previews = new JsonArray();
            foreach (var p in info.Previews)
            {
                previews.Add(new JsonObject
                {
                    ["index"] = p.Index,
                    ["offset"] = p.Offset,
                    ["length"] = p.Length,
                    ["kind"] = p.Kind.ToString(),
                    ["width"] = p.Width,
                    ["height"] = p.Height,
                    ["color_space"] = p.ColorSpace.ToString(),
                    ["header_resolved"] = p.HeaderResolved,
                });
            }

            row["previews"] = previews;
            var blocks = new JsonArray();
            foreach (var b in info.ExifBlocks)
            {
                blocks.Add(new JsonObject
                {
                    ["offset"] = b.Offset,
                    ["length"] = b.Length,
                    ["is_tiff_header"] = b.IsTiffHeader,
                    ["ifd_is_exif"] = b.IfdIsExif,
                });
            }

            row["exif_blocks"] = blocks;
        }
        catch (InvalidDataException ex)
        {
            bool fromReader = W509ReaderMessages.Contains(ex.Message);
            row["ok"] = false;
            var error = new JsonObject
            {
                ["type"] = ex.GetType().Name,
                ["origin"] = fromReader ? "reader" : "source",
                ["io_failure"] = ex.InnerException is IOException or ObjectDisposedException,
            };
            if (fromReader) error["message"] = ex.Message;
            row["error"] = error;
        }

        row["bytes_charged"] = source.TotalBytesRead;
    }

    /// <summary>
    /// Per case {name, parts, patches?, truncate_to?, cancelled?}: RafContainerReader.Read over the built bytes.
    /// Result adds total_length, ok, bytes_charged and the info fields or the error (see W509ReadInto). "cancelled": the
    /// token passed is already cancelled (the reader never looks at it, so the result is the same as without).
    /// </summary>
    public static JsonArray RafContainerCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var bytes = W509Build(c);
            var row = (JsonObject)c.DeepClone();
            row["total_length"] = bytes.Length;
            W509ReadInto(row, new MemoryStream(bytes, writable: false), c["cancelled"]?.GetValue<bool>() ?? false);
            result.Add(row);
        }

        return result;
    }

    /// <summary>Per case {name, head_hex, extension}: RafContainerReader.CanRead(head, extension) as "result".</summary>
    public static JsonArray RafCanReadCases(JsonObject inputs)
    {
        var reader = new RafContainerReader();
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var head = Convert.FromHexString(c["head_hex"]!.GetValue<string>());
            var row = (JsonObject)c.DeepClone();
            row["result"] = reader.CanRead(head, c["extension"]!.GetValue<string>());
            result.Add(row);
        }

        return result;
    }

    /// <summary>
    /// Per case {name, file_name}: the real corpus RAF read through SourceRawHeaderSource over a FileStream (the test path of
    /// Cr3AndRafCorpusTests / RafSensorSizeTests). Folder = env PHOTOREVIEW_RAW_CORPUS_DIR (or PHOTOREVIEW_RAW_CORPUS); a missing folder or file records present=false. Adds file_length, sha256 (informational) and
    /// preview_starts_with_soi (the bytes at the preview offset are FF D8).
    /// </summary>
    public static JsonArray RafCorpusCases(JsonObject inputs)
    {
        // No corpus on this machine (downloads need the user's yes): every case is recorded with present=false and the Rust twin skips it.
        var dir = Environment.GetEnvironmentVariable("PHOTOREVIEW_RAW_CORPUS_DIR") is { Length: > 0 } d1 ? d1 : Environment.GetEnvironmentVariable("PHOTOREVIEW_RAW_CORPUS");

        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var path = string.IsNullOrWhiteSpace(dir) ? "" : Path.Combine(dir, c["file_name"]!.GetValue<string>());
            var row = (JsonObject)c.DeepClone();
            if (path.Length == 0 || !File.Exists(path))
            {
                row["present"] = false;
                result.Add(row);
                continue;
            }

            row["present"] = true;
            var fileBytes = File.ReadAllBytes(path);
            row["file_length"] = fileBytes.LongLength;
            row["sha256"] = Convert.ToHexString(SHA256.HashData(fileBytes)).ToLowerInvariant();
            using var fs = File.OpenRead(path);
            W509ReadInto(row, fs, cancelled: false);
            if (row["previews"] is JsonArray { Count: > 0 } previews)
            {
                long offset = previews[0]!["offset"]!.GetValue<long>();
                row["preview_starts_with_soi"] = fileBytes[offset] == 0xFF && fileBytes[offset + 1] == 0xD8;
            }

            result.Add(row);
        }

        return result;
    }
}
