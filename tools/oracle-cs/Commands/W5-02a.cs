using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;
using PhotoReview.Imaging.Raw.Tiff;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-02a
//     "tiff-navigator" => Commands.TiffNavigatorCases(inputs),
// Needs W4-03a.cs (static W403Data: { "hex" } | { "len", "patches" } byte nodes).
// OracleCs.csproj needs one more <Reference Include="PhotoReview.Imaging.Raw"> (HintPath $(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll) and
// tools/oracle.ps1 must build that project next to PhotoReview.Imaging (it references Imaging only). Every type used here is public
// (TiffHeaderNavigator, its nested TiffEntry, IRawHeaderSource, RawContainerLimits, ExifBlock): no InternalsVisibleTo.

// W5-02a: TiffHeaderNavigator. One command, one case per entry. Case = { name, op, ... }.
//   Common optional fields: "little" (bool, default true); "fail": "io" | "data" with "fail_after" (int, default 0): the first
//   fail_after Read calls succeed, every later one throws InvalidDataException (with an IOException inner for "io"), like a source
//   whose disk failed / whose header budget ran out. Every result carries "error": null | "io" | "data" (the exception that escaped
//   the navigator; a result field is then absent).
//   "entries" = [ { tag, type, count, value } ] (numbers; count/value are uint). "data" = byte node as in W4-03a.
// ops (fields -> reported):
//   ifd-entries      data, little, offset (long)                     -> { entries:[{tag,type,count,value}], next }
//   tag-unsigned     data, little, entry                             -> { value }          (null | long)
//   tag-array        data, little, entry, max_items? (default 64)    -> { values:[long] }
//   exif-block       data, little, ifd0 (long)                       -> { offset, length, is_tiff_header, ifd_is_exif }
//   single-strip     data, little, entries                           -> { ok, offset, length }
//   crop-size        data, little, entries                           -> { ok, width, height }
//   square-scale     data, little, entries                           -> { value }
//   jpeg-interchange data, little, entries                           -> { ok, offset, length }
//   image-size       data, little, entries                           -> { width, height }
//   tag-value        data, little, entries, tag                      -> { value }
//   exif-dims        data, little, offset (long)                     -> { ok, width, height }
//   soi              data, offset (long), length (long)              -> { value }
//   range            offset, length, file_length (long)              -> { value }
//   clamp            value (long | null)                             -> { value }
//   sensor-size      raw_w, raw_h, crop_w, crop_h, exif_w, exif_h    -> { width, height }
//   get-entry        entries, tag                                    -> { ok, entry:{tag,type,count,value}|null }
//   walk             data, little, ifd0 (long)                       -> the ExerciseTiffNavigator battery of BinaryReaderFuzzTests
//                     (ReadIfdEntries(ifd0) -> ReadTagUnsigned + ReadTagUnsignedArray of the first 64 entries -> next IFD ->
//                     ComputeExifBlock -> strip -> crop -> scale -> image size -> exif dims at ifd0 -> jpeg interchange), reported as
//                     { entries, tag_values:[..], tag_arrays:[[..]], next, next_entries (count or null), exif_block:{..}, strip, crop,
//                       square_scale, image_size, exif_dims, jpeg } in that order. The whole battery stops at the first exception:
//                     "error" is set and the fields computed before it stay.
// ReconcileJpegSize has no op: it calls PreviewSelector.TryReadJpegFrame (W5-11); the Rust twin takes the walker as a parameter and is
// tested with fakes; the real pairing is covered by the container-reader goldens (W5-03..W5-08).
static partial class Commands
{
    private sealed class W502Source(byte[] data, string? fail, int failAfter) : IRawHeaderSource
    {
        private int _reads;

        public long Length => data.Length;

        public ReadOnlySpan<byte> Read(long offset, int count)
        {
            if (fail is not null && _reads++ >= failAfter)
            {
                throw fail == "io"
                    ? new InvalidDataException("read failed", new IOException("disk"))
                    : new InvalidDataException("budget exhausted");
            }

            if (offset < 0 || count < 0) throw new ArgumentOutOfRangeException(nameof(offset));
            if (offset > data.Length - count) throw new InvalidDataException("past end");
            if (count > RawContainerLimits.MaxHeaderBytes) throw new InvalidDataException("over the hard limit");
            return data.AsSpan((int)offset, count);
        }
    }

    private static TiffHeaderNavigator.TiffEntry W502Entry(JsonNode? node)
    {
        var o = node!.AsObject();
        return new TiffHeaderNavigator.TiffEntry(
            (ushort)o["tag"]!.GetValue<int>(), (ushort)o["type"]!.GetValue<int>(), o["count"]!.GetValue<uint>(), o["value"]!.GetValue<uint>());
    }

    private static TiffHeaderNavigator.TiffEntry[] W502Entries(JsonNode? node) =>
        node!.AsArray().Select(W502Entry).ToArray();

    private static JsonNode W502EntryJson(TiffHeaderNavigator.TiffEntry e) => new JsonObject
    {
        ["tag"] = e.Tag,
        ["type"] = e.Type,
        ["count"] = (long)e.Count,
        ["value"] = (long)e.ValueOrOffset,
    };

    private static JsonArray W502Longs(IEnumerable<long> values) => new(values.Select(v => (JsonNode?)JsonValue.Create(v)).ToArray());

    private static JsonObject W502Pair(bool ok, long a, long b, string x, string y) =>
        new() { ["ok"] = ok, [x] = ok ? a : 0, [y] = ok ? b : 0 };

    public static JsonArray TiffNavigatorCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op };
            bool little = c["little"]?.GetValue<bool>() ?? true;
            byte[] data = c["data"] is { } d ? W403Data(d) : [];
            var source = new W502Source(data, c["fail"]?.GetValue<string>(), c["fail_after"]?.GetValue<int>() ?? 0);
            item["error"] = null;
            try
            {
                switch (op)
                {
                    case "ifd-entries":
                    {
                        var entries = TiffHeaderNavigator.ReadIfdEntries(source, c["offset"]!.GetValue<long>(), little, out var next);
                        item["entries"] = new JsonArray(entries.Select(W502EntryJson).ToArray());
                        item["next"] = (long)next;
                        break;
                    }
                    case "tag-unsigned":
                        item["value"] = TiffHeaderNavigator.ReadTagUnsigned(source, W502Entry(c["entry"]), little);
                        break;
                    case "tag-array":
                    {
                        var entry = W502Entry(c["entry"]);
                        var values = c["max_items"] is { } m
                            ? TiffHeaderNavigator.ReadTagUnsignedArray(source, entry, little, m.GetValue<int>())
                            : TiffHeaderNavigator.ReadTagUnsignedArray(source, entry, little);
                        item["values"] = W502Longs(values);
                        break;
                    }
                    case "exif-block":
                    {
                        var block = TiffHeaderNavigator.ComputeExifBlock(source, little, c["ifd0"]!.GetValue<long>());
                        item["offset"] = block.Offset;
                        item["length"] = block.Length;
                        item["is_tiff_header"] = block.IsTiffHeader;
                        item["ifd_is_exif"] = block.IfdIsExif;
                        break;
                    }
                    case "single-strip":
                    {
                        bool ok = TiffHeaderNavigator.TryReadSingleStrip(source, W502Entries(c["entries"]), little, out var o, out var l);
                        foreach (var kv in W502Pair(ok, o, l, "offset", "length")) item[kv.Key] = kv.Value?.DeepClone();
                        break;
                    }
                    case "crop-size":
                    {
                        bool ok = TiffHeaderNavigator.TryReadDefaultCropSize(source, W502Entries(c["entries"]), little, out var w, out var h);
                        foreach (var kv in W502Pair(ok, w, h, "width", "height")) item[kv.Key] = kv.Value?.DeepClone();
                        break;
                    }
                    case "square-scale":
                        item["value"] = TiffHeaderNavigator.HasSquareDefaultScale(source, W502Entries(c["entries"]), little);
                        break;
                    case "jpeg-interchange":
                    {
                        bool ok = TiffHeaderNavigator.TryReadJpegInterchange(source, W502Entries(c["entries"]), little, out var o, out var l);
                        foreach (var kv in W502Pair(ok, o, l, "offset", "length")) item[kv.Key] = kv.Value?.DeepClone();
                        break;
                    }
                    case "image-size":
                    {
                        TiffHeaderNavigator.ReadImageSize(source, W502Entries(c["entries"]), little, out var w, out var h);
                        item["width"] = w;
                        item["height"] = h;
                        break;
                    }
                    case "tag-value":
                        item["value"] = TiffHeaderNavigator.ReadTagValue(source, W502Entries(c["entries"]), (ushort)c["tag"]!.GetValue<int>(), little);
                        break;
                    case "exif-dims":
                    {
                        bool ok = TiffHeaderNavigator.TryReadExifPixelDimensions(source, c["offset"]!.GetValue<long>(), little, out var w, out var h);
                        foreach (var kv in W502Pair(ok, w, h, "width", "height")) item[kv.Key] = kv.Value?.DeepClone();
                        break;
                    }
                    case "soi":
                        item["value"] = TiffHeaderNavigator.StartsWithSoi(source, c["offset"]!.GetValue<long>(), c["length"]!.GetValue<long>());
                        break;
                    case "range":
                        item["value"] = TiffHeaderNavigator.IsRangeInFile(
                            c["offset"]!.GetValue<long>(), c["length"]!.GetValue<long>(), c["file_length"]!.GetValue<long>());
                        break;
                    case "clamp":
                        item["value"] = TiffHeaderNavigator.ClampToInt(c["value"] is { } v ? v.GetValue<long>() : null);
                        break;
                    case "sensor-size":
                    {
                        var (w, h) = TiffHeaderNavigator.ChooseActiveSensorSize(
                            c["raw_w"]!.GetValue<int>(), c["raw_h"]!.GetValue<int>(), c["crop_w"]!.GetValue<int>(),
                            c["crop_h"]!.GetValue<int>(), c["exif_w"]!.GetValue<int>(), c["exif_h"]!.GetValue<int>());
                        item["width"] = w;
                        item["height"] = h;
                        break;
                    }
                    case "get-entry":
                    {
                        bool ok = TiffHeaderNavigator.TryGetEntry(W502Entries(c["entries"]), (ushort)c["tag"]!.GetValue<int>(), out var e);
                        item["ok"] = ok;
                        item["entry"] = ok ? W502EntryJson(e) : null;
                        break;
                    }
                    case "walk":
                        W502Walk(item, source, little, c["ifd0"]!.GetValue<long>());
                        break;
                    default:
                        throw new ArgumentException("unknown op " + op);
                }
            }
            catch (InvalidDataException ex)
            {
                item["error"] = ex.InnerException is IOException ? "io" : "data";
            }

            result.Add(item);
        }

        return result;
    }

    /// <summary>The BinaryReaderFuzzTests.ExerciseTiffNavigator battery with its results recorded in call order.</summary>
    private static void W502Walk(JsonObject item, IRawHeaderSource source, bool little, long ifd0)
    {
        var entries = TiffHeaderNavigator.ReadIfdEntries(source, ifd0, little, out var next);
        item["entries"] = new JsonArray(entries.Select(W502EntryJson).ToArray());
        item["next"] = (long)next;
        var values = new JsonArray();
        var arrays = new JsonArray();
        item["tag_values"] = values;
        item["tag_arrays"] = arrays;
        foreach (var entry in entries.Take(64))
        {
            values.Add(TiffHeaderNavigator.ReadTagUnsigned(source, entry, little));
            arrays.Add(W502Longs(TiffHeaderNavigator.ReadTagUnsignedArray(source, entry, little)));
        }

        item["next_entries"] = next != 0 ? TiffHeaderNavigator.ReadIfdEntries(source, next, little, out _).Count : null;
        var block = TiffHeaderNavigator.ComputeExifBlock(source, little, ifd0);
        item["exif_block"] = new JsonObject
        {
            ["offset"] = block.Offset,
            ["length"] = block.Length,
            ["is_tiff_header"] = block.IsTiffHeader,
            ["ifd_is_exif"] = block.IfdIsExif,
        };
        item["strip"] = W502Pair(TiffHeaderNavigator.TryReadSingleStrip(source, entries, little, out var so, out var sl), so, sl, "offset", "length");
        item["crop"] = W502Pair(TiffHeaderNavigator.TryReadDefaultCropSize(source, entries, little, out var cw, out var ch), cw, ch, "width", "height");
        item["square_scale"] = TiffHeaderNavigator.HasSquareDefaultScale(source, entries, little);
        TiffHeaderNavigator.ReadImageSize(source, entries, little, out var iw, out var ih);
        item["image_size"] = new JsonObject { ["width"] = iw, ["height"] = ih };
        item["exif_dims"] = W502Pair(TiffHeaderNavigator.TryReadExifPixelDimensions(source, ifd0, little, out var ew, out var eh), ew, eh, "width", "height");
        item["jpeg"] = W502Pair(TiffHeaderNavigator.TryReadJpegInterchange(source, entries, little, out var jo, out var jl), jo, jl, "offset", "length");
    }
}
