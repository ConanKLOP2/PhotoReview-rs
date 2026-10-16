using System.IO;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-01b
//     "raw-contract-records" => Commands.RawContractRecords(inputs),
//     "raw-header-budget" => Commands.RawHeaderBudget(inputs),
// Needs a Reference to PhotoReview.Imaging.Raw.dll in OracleCs.csproj (the wiring step adds it, and copies the dll next to
// PhotoReview.Imaging.dll in tools/oracle.ps1; today only Core/Imaging are referenced).

// W5-01b: EmbeddedPreview / ExifBlock record facts and the header-source read/budget contract.
static partial class Commands
{
    /// <summary>
    /// Cases {name, type: "preview"|"block", a: [...], b: [...]} where a/b are constructor argument lists.
    /// preview: [index, offset, length, kind("Jpeg"), width, height, colorSpace(name), headerResolved?]  (headerResolved omitted = ctor default)
    /// block:   [offset, length, isTiffHeader, ifdIsExif?]
    /// Reports the final field values of "a", whether a.Equals(b), whether the hash codes of a and b are equal and whether
    /// b == a with-copy (`a with { }`) equals a. Also the enum names and numeric values (case "enums").
    /// </summary>
    public static JsonArray RawContractRecords(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var type = c["type"]!.GetValue<string>();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["type"] = type };
            if (type == "enums")
            {
                var kinds = new JsonObject();
                foreach (var v in Enum.GetValues<EmbeddedPreviewKind>()) kinds[v.ToString()] = (int)v;
                var spaces = new JsonObject();
                foreach (var v in Enum.GetValues<PreviewColorSpace>()) spaces[v.ToString()] = (int)v;
                item["embedded_preview_kind"] = kinds;
                item["preview_color_space"] = spaces;
            }
            else if (type == "preview")
            {
                var a = Preview(c["a"]!.AsArray());
                var b = Preview(c["b"]!.AsArray());
                item["index"] = a.Index;
                item["offset"] = a.Offset;
                item["length"] = a.Length;
                item["kind"] = a.Kind.ToString();
                item["width"] = a.Width;
                item["height"] = a.Height;
                item["color_space"] = a.ColorSpace.ToString();
                item["header_resolved"] = a.HeaderResolved;
                item["equals"] = a.Equals(b);
                item["hash_equal"] = a.GetHashCode() == b.GetHashCode();
                item["with_copy_equals"] = (a with { }).Equals(a);
            }
            else if (type == "block")
            {
                var a = Block(c["a"]!.AsArray());
                var b = Block(c["b"]!.AsArray());
                item["offset"] = a.Offset;
                item["length"] = a.Length;
                item["is_tiff_header"] = a.IsTiffHeader;
                item["ifd_is_exif"] = a.IfdIsExif;
                item["equals"] = a.Equals(b);
                item["hash_equal"] = a.GetHashCode() == b.GetHashCode();
                item["with_copy_equals"] = (a with { }).Equals(a);
            }
            else throw new ArgumentException("unknown type " + type);
            result.Add(item);
        }
        return result;

        static EmbeddedPreview Preview(JsonArray x)
        {
            var kind = Enum.Parse<EmbeddedPreviewKind>(x[3]!.GetValue<string>());
            var space = Enum.Parse<PreviewColorSpace>(x[6]!.GetValue<string>());
            return x.Count > 7
                ? new EmbeddedPreview(x[0]!.GetValue<int>(), x[1]!.GetValue<long>(), x[2]!.GetValue<long>(), kind,
                    x[4]!.GetValue<int>(), x[5]!.GetValue<int>(), space, x[7]!.GetValue<bool>())
                : new EmbeddedPreview(x[0]!.GetValue<int>(), x[1]!.GetValue<long>(), x[2]!.GetValue<long>(), kind,
                    x[4]!.GetValue<int>(), x[5]!.GetValue<int>(), space);
        }

        static ExifBlock Block(JsonArray x) => x.Count > 3
            ? new ExifBlock(x[0]!.GetValue<long>(), x[1]!.GetValue<long>(), x[2]!.GetValue<bool>(), x[3]!.GetValue<bool>())
            : new ExifBlock(x[0]!.GetValue<long>(), x[1]!.GetValue<long>(), x[2]!.GetValue<bool>());
    }

    /// <summary>
    /// Cases {name, length, steps:[{offset, count}]} over a SourceRawHeaderSource on a MemoryStream whose byte i is (i % 251).
    /// Per step reports ok/len/first/last/sum (sum of the returned bytes, uint wrap), the error type name ("InvalidDataException",
    /// "ArgumentOutOfRangeException") and ParamName, and TotalBytesRead after the step (the block-charging contract).
    /// The Rust twin runs the same steps against InMemoryRawHeaderSource (and W5-12 against SourceRawHeaderSource).
    /// </summary>
    public static JsonArray RawHeaderBudget(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            int length = c["length"]!.GetValue<int>();
            var data = new byte[length];
            for (int i = 0; i < data.Length; i++) data[i] = (byte)(i % 251);
            using var source = new SourceRawHeaderSource(new MemoryStream(data));
            var steps = new JsonArray();
            foreach (var s in c["steps"]!.AsArray())
            {
                long offset = s!["offset"]!.GetValue<long>();
                int count = s["count"]!.GetValue<int>();
                var step = new JsonObject { ["offset"] = offset, ["count"] = count };
                try
                {
                    var span = source.Read(offset, count);
                    uint sum = 0;
                    foreach (var b in span) sum = unchecked(sum + b);
                    step["ok"] = true;
                    step["len"] = span.Length;
                    step["first"] = span.Length > 0 ? span[0] : null;
                    step["last"] = span.Length > 0 ? span[^1] : null;
                    step["sum"] = sum;
                }
                catch (Exception ex)
                {
                    step["ok"] = false;
                    step["error"] = ex.GetType().Name;
                    step["param"] = (ex as ArgumentOutOfRangeException)?.ParamName;
                }
                step["total_bytes_read"] = source.TotalBytesRead;
                steps.Add(step);
            }
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["length"] = length,
                ["source_length"] = source.Length,
                ["steps"] = steps,
            });
        }
        return result;
    }
}
