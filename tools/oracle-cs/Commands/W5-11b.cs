using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Raw;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-11b
//     "preview-selector-select" => Commands.PreviewSelectorSelectCases(inputs),
//     "preview-selector-jpeg-frame" => Commands.PreviewSelectorJpegFrameCases(inputs),
//     "adobe-rgb-exif" => Commands.AdobeRgbExifCases(inputs),
// Wiring: tools/oracle-cs/OracleCs.csproj needs the reference next to PhotoReview.Imaging (the same one W5-01b / W5-02b ask for):
//     <Reference Include="PhotoReview.Imaging.Raw"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll</HintPath></Reference>
// and tools/oracle.ps1 must copy PhotoReview.Imaging.Raw.dll into the baseline build folder.
// PreviewSelector.SelectPreview / TryReadJpegFrame are public. IsAdobeRgbExif is `internal` (InternalsVisibleTo lists only test
// projects), so it is bound by reflection to a delegate whose parameter is a ReadOnlySpan<byte> (MethodInfo.Invoke cannot box a span).
// NOT RUN by the contract writer: the wiring step runs it and commits tests/golden/{preview-selector-select,preview-selector-jpeg-frame,adobe-rgb-exif}.json.

// W5-11b: PreviewSelector over synthetic byte ranges.
static partial class Commands
{
    private const int W511bBlockSize = 64 * 1024;
    private const int W511bMaxHeaderBytes = 8 << 20;

    /// <summary>
    /// Same budget semantics as SourceRawHeaderSource / the InMemoryRawHeaderSource fixture (64 KB blocks charged once each, the
    /// last block counts only its bytes), plus a read counter. Modes: memory, throw-invalid (InvalidDataException without an
    /// inner exception: budget exhausted / corrupt) and throw-io (InvalidDataException wrapping an IOException).
    /// </summary>
    private sealed class W511bSource(byte[] bytes, string mode, long declaredLength) : IRawHeaderSource
    {
        private readonly HashSet<long> _touched = [];
        public long TotalBytesRead { get; private set; }
        public int ReadCount { get; private set; }
        public long Length => mode == "memory" ? bytes.Length : declaredLength;

        public ReadOnlySpan<byte> Read(long offset, int count)
        {
            ReadCount++;
            if (mode == "throw-invalid") throw new InvalidDataException("budget exhausted");
            if (mode == "throw-io") throw new InvalidDataException("read failed", new IOException("disk"));

            if (offset < 0 || count < 0) throw new ArgumentOutOfRangeException(nameof(offset));
            if (offset > bytes.Length - count) throw new InvalidDataException("past end");
            if (count > W511bMaxHeaderBytes) throw new InvalidDataException("too large");
            if (count == 0) return ReadOnlySpan<byte>.Empty;
            for (long block = offset / W511bBlockSize; block <= (offset + count - 1) / W511bBlockSize; block++)
            {
                if (_touched.Contains(block)) continue;
                long blockBytes = Math.Min(W511bBlockSize, bytes.Length - (block * W511bBlockSize));
                if (blockBytes > W511bMaxHeaderBytes - TotalBytesRead) throw new InvalidDataException("budget exhausted");
                _touched.Add(block);
                TotalBytesRead += blockBytes;
            }
            return bytes.AsSpan((int)offset, count);
        }
    }

    private delegate bool W511bSpanPredicate(ReadOnlySpan<byte> payload);

    private static byte[] W511bBuild(JsonArray parts)
    {
        var all = new List<byte>();
        foreach (var node in parts)
        {
            var p = node!.AsObject();
            var unit = Convert.FromHexString(p["hex"]!.GetValue<string>()).ToList();
            if (p["pad_to"] is { } pad) while (unit.Count < pad.GetValue<int>()) unit.Add(0);
            int repeat = p["repeat"]?.GetValue<int>() ?? 1;
            for (int i = 0; i < repeat; i++) all.AddRange(unit);
        }
        return [.. all];
    }

    private static long W511bLong(JsonNode node) =>
        node is JsonValue v && v.TryGetValue<string>(out var s) ? long.Parse(s, CultureInfo.InvariantCulture) : node.GetValue<long>();

    private static W511bSource W511bMakeSource(JsonObject c, byte[] bytes, long defaultDeclaredLength) =>
        new(bytes, c["source"]!.GetValue<string>(), c["source_length"] is { } sl ? W511bLong(sl) : defaultDeclaredLength);

    private static EmbeddedPreview W511bPreview(JsonObject p) =>
        new(p["index"]!.GetValue<int>(), W511bLong(p["offset"]!), W511bLong(p["length"]!), EmbeddedPreviewKind.Jpeg,
            p["width"]!.GetValue<int>(), p["height"]!.GetValue<int>(), (PreviewColorSpace)p["color_space"]!.GetValue<int>(),
            p["header_resolved"]?.GetValue<bool>() ?? false);

    private static JsonObject W511bPreviewJson(EmbeddedPreview p) => new()
    {
        ["index"] = p.Index,
        ["offset"] = p.Offset.ToString(CultureInfo.InvariantCulture),
        ["length"] = p.Length.ToString(CultureInfo.InvariantCulture),
        ["width"] = p.Width,
        ["height"] = p.Height,
        ["color_space"] = (int)p.ColorSpace,
        ["header_resolved"] = p.HeaderResolved,
    };

    private static string W511bError(InvalidDataException ex) => ex.InnerException is IOException ? "io-failure" : "invalid-data";

    /// <summary>
    /// Per case {name, source: memory|throw-invalid|throw-io, source_length?, parts:[{hex, pad_to?, repeat?}], previews:[{index, offset,
    /// length (number or decimal string), width, height, color_space 0|1|2, header_resolved?}], box:[w,h], orientation, preburn?:{offset,count}}.
    /// preburn reads that range from the (memory) source before the selection, to spend header budget.
    /// Result: error (none|invalid-data|io-failure; any other exception type fails the run), read_count (reads made by the selection only),
    /// bytes_charged (header budget charged in total), and, when error is none, chosen (preview object or null) and resolved (the out list).
    /// </summary>
    public static JsonArray PreviewSelectorSelectCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var bytes = W511bBuild(c["parts"]!.AsArray());
            var source = W511bMakeSource(c, bytes, 1_000_000);
            if (c["preburn"] is JsonObject burn) _ = source.Read(W511bLong(burn["offset"]!), burn["count"]!.GetValue<int>());
            int readsBefore = source.ReadCount;

            var previews = c["previews"]!.AsArray().Select(p => W511bPreview(p!.AsObject())).ToList();
            var box = c["box"]!.AsArray();
            string error = "none";
            EmbeddedPreview? chosen = null;
            IReadOnlyList<EmbeddedPreview> resolved = [];
            try
            {
                chosen = PreviewSelector.SelectPreview(source, previews,
                    new DecodeBox(box[0]!.GetValue<int>(), box[1]!.GetValue<int>()), c["orientation"]!.GetValue<int>(), out resolved);
            }
            catch (InvalidDataException ex)
            {
                error = W511bError(ex);
            }

            var row = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["source"] = c["source"]!.GetValue<string>(),
                ["source_length"] = source.Length.ToString(CultureInfo.InvariantCulture),
                ["parts"] = c["parts"]!.DeepClone(),
                ["previews"] = c["previews"]!.DeepClone(),
                ["box"] = c["box"]!.DeepClone(),
                ["orientation"] = c["orientation"]!.GetValue<int>(),
                ["error"] = error,
                ["read_count"] = source.ReadCount - readsBefore,
                ["bytes_charged"] = source.TotalBytesRead,
            };
            if (c["preburn"] is { } preburn) row["preburn"] = preburn.DeepClone();
            if (error == "none")
            {
                row["chosen"] = chosen is null ? null : W511bPreviewJson(chosen);
                var list = new JsonArray();
                foreach (var p in resolved) list.Add(W511bPreviewJson(p));
                row["resolved"] = list;
            }
            result.Add(row);
        }
        return result;
    }

    /// <summary>
    /// Per case {name, source, source_length?, parts, offset, length? | length_delta?}: length defaults to (total - offset + length_delta);
    /// a string length is parsed as int64. Calls the public PreviewSelector.TryReadJpegFrame directly.
    /// Result: error (none|invalid-data|io-failure), read_count, bytes_charged, and, when error is none: ok, color_space (the out value,
    /// reported even when ok is false), width/height (only when ok).
    /// </summary>
    public static JsonArray PreviewSelectorJpegFrameCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var bytes = W511bBuild(c["parts"]!.AsArray());
            long offset = W511bLong(c["offset"]!);
            long length = c["length"] is { } l ? W511bLong(l) : bytes.Length - offset + (c["length_delta"]?.GetValue<long>() ?? 0);
            var source = W511bMakeSource(c, bytes, 64);

            string error = "none";
            bool ok = false;
            int width = 0, height = 0;
            var colorSpace = PreviewColorSpace.Unknown;
            try
            {
                ok = PreviewSelector.TryReadJpegFrame(source, offset, length, out width, out height, out colorSpace);
            }
            catch (InvalidDataException ex)
            {
                error = W511bError(ex);
            }

            var row = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["source"] = c["source"]!.GetValue<string>(),
                ["source_length"] = source.Length.ToString(CultureInfo.InvariantCulture),
                ["parts"] = c["parts"]!.DeepClone(),
                ["total_length"] = bytes.Length,
                ["offset"] = offset.ToString(CultureInfo.InvariantCulture),
                ["length"] = length.ToString(CultureInfo.InvariantCulture),
                ["error"] = error,
                ["read_count"] = source.ReadCount,
                ["bytes_charged"] = source.TotalBytesRead,
            };
            if (error == "none")
            {
                row["ok"] = ok;
                row["color_space"] = (int)colorSpace;
                if (ok)
                {
                    row["width"] = width;
                    row["height"] = height;
                }
            }
            result.Add(row);
        }
        return result;
    }

    /// <summary>Per case {name, hex}: the internal PreviewSelector.IsAdobeRgbExif over the decoded bytes. Result adds payload_length and result.</summary>
    public static JsonArray AdobeRgbExifCases(JsonObject inputs)
    {
        var method = typeof(PreviewSelector).GetMethod("IsAdobeRgbExif", BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.Static)!;
        var predicate = method.CreateDelegate<W511bSpanPredicate>();
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var payload = Convert.FromHexString(c["hex"]!.GetValue<string>());
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["hex"] = c["hex"]!.GetValue<string>(),
                ["payload_length"] = payload.Length,
                ["result"] = predicate(payload),
            });
        }
        return result;
    }
}
