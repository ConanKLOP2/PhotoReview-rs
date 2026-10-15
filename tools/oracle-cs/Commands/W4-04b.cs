using System.Globalization;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Metadata;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-04b
//     "exif-query" => Commands.ExifQuery(inputs),
// Needs a <Reference Include="PhotoReview.Imaging"> in tools/oracle-cs/OracleCs.csproj (and tools/oracle.ps1 copying
// PhotoReview.Imaging.dll next to PhotoReview.Core.dll). Uses only public API (ExifQueryInterpreter.Read, ExifSummary).

// W4-04b: ExifQueryInterpreter.Read
static partial class Commands
{
    private static object? W404bValue(JsonNode? node)
    {
        if (node is null) return null;
        var o = node.AsObject();
        var type = o["type"]!.GetValue<string>();
        JsonNode v = o["value"]!;
        static long L(JsonNode n) => long.Parse(n.GetValue<string>(), CultureInfo.InvariantCulture);
        static ulong U(JsonNode n) => ulong.Parse(n.GetValue<string>(), CultureInfo.InvariantCulture);
        static T[] Arr<T>(JsonNode n, Func<JsonNode, T> f) => n.AsArray().Select(e => f(e!)).ToArray();
        // 32-bit and 64-bit values are decimal strings in the JSON (no double rounding, no sign surprises).
        return type switch
        {
            "string" => v.GetValue<string>(),
            "chars" => v.GetValue<string>().ToCharArray(),
            "u8" => (byte)v.GetValue<int>(),
            "u16" => (ushort)v.GetValue<int>(),
            "i16" => (short)v.GetValue<int>(),
            "u32" => (uint)L(v),
            "i32" => (int)L(v),
            "u16[]" => Arr(v, e => (ushort)e.GetValue<int>()),
            "i16[]" => Arr(v, e => (short)e.GetValue<int>()),
            "u32[]" => Arr(v, e => (uint)L(e)),
            "i32[]" => Arr(v, e => (int)L(e)),
            "u64" => U(v),
            "i64" => L(v),
            "u64[]" => Arr(v, U),
            "i64[]" => Arr(v, L),
            "double" => double.Parse(v.GetValue<string>(), CultureInfo.InvariantCulture),
            "bytes" => Arr(v, e => (byte)e.GetValue<int>()),
            _ => throw new ArgumentException("unknown value type " + type),
        };
    }

    private static JsonNode? W404bRational(ExifRational? r) =>
        r is { } x ? new JsonObject { ["numerator"] = (long)x.Numerator, ["denominator"] = (long)x.Denominator } : null;

    /// <summary>
    /// ExifQueryInterpreter.Read for each case {name, root (the ifdRoot string, verbatim), values: {"full query path": {type, value}}}.
    /// Result: "queried" = every query path in call order (order and short-circuiting are part of the contract) and "summary" = null or
    /// {date_taken "yyyy-MM-ddTHH:mm:ss" | null, camera_make, camera_model, lens_model, iso, focal_length, f_number, exposure_time}.
    /// </summary>
    public static JsonArray ExifQuery(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var values = new Dictionary<string, object?>(StringComparer.Ordinal);
            foreach (var (path, spec) in c["values"]!.AsObject()) values[path] = W404bValue(spec);
            var queried = new JsonArray();
            object? Query(string path)
            {
                queried.Add(path);
                return values.TryGetValue(path, out var v) ? v : null;
            }
            var summary = ExifQueryInterpreter.Read(Query, c["root"]!.GetValue<string>());
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["root"] = c["root"]!.GetValue<string>(),
                ["queried"] = queried,
                ["summary"] = summary is null ? null : new JsonObject
                {
                    ["date_taken"] = summary.DateTaken?.ToString("yyyy-MM-ddTHH:mm:ss", CultureInfo.InvariantCulture),
                    ["camera_make"] = summary.CameraMake,
                    ["camera_model"] = summary.CameraModel,
                    ["lens_model"] = summary.LensModel,
                    ["iso"] = summary.Iso,
                    ["focal_length"] = W404bRational(summary.FocalLength),
                    ["f_number"] = W404bRational(summary.FNumber),
                    ["exposure_time"] = W404bRational(summary.ExposureTime),
                },
            });
        }
        return result;
    }
}
