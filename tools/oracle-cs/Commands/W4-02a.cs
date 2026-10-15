using System.Reflection;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Pixels;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-02a
//     "orientation-values" => Commands.OrientationValues(inputs),
//     "orientation-read-wic" => Commands.OrientationReadWic(inputs),
//     "orientation-pixels" => Commands.OrientationPixels(inputs),
// Wiring needs (the wiring step, not this draft): OracleCs.csproj gets a <Reference Include="PhotoReview.Imaging"> to the
// baseline PhotoReview.Imaging.dll, copied next to PhotoReview.Core.dll by oracle.ps1.
// ExifOrientation.ReadFromWic and the two query constants are `internal`: reached by reflection below, so no
// InternalsVisibleTo is needed. The WPF half (WpfExifOrientation.CreateTransform) is NOT callable (it needs PresentationCore);
// the transform table is covered through PixelOps.ApplyOrientation, which the C# tests prove byte-equal to the WPF transform.

// W4-02a: ExifOrientation.Normalize / IsTransposed / ReadFromWic and PixelOps.ApplyOrientation pixel mapping.
static partial class Commands
{
    private static readonly MethodInfo W402aReadFromWic =
        typeof(ExifOrientation).GetMethod("ReadFromWic", BindingFlags.NonPublic | BindingFlags.Static)!;

    private static string W402aConst(string name) =>
        (string)typeof(ExifOrientation).GetField(name, BindingFlags.NonPublic | BindingFlags.Static)!.GetRawConstantValue()!;

    /// <summary>Case {name, values:[int]} -> {normalize:[int], transposed:[bool]} (same order), plus the two query strings.</summary>
    public static JsonArray OrientationValues(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var values = c["values"]!.AsArray().Select(v => v!.GetValue<int>()).ToArray();
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["values"] = c["values"]!.DeepClone(),
                ["normalize"] = new JsonArray(values.Select(v => (JsonNode?)ExifOrientation.Normalize(v)).ToArray()),
                ["transposed"] = new JsonArray(values.Select(v => (JsonNode?)ExifOrientation.IsTransposed(v)).ToArray()),
                ["exif_query"] = W402aConst("ExifOrientationQuery"),
                ["windows_query"] = W402aConst("WindowsOrientationQuery"),
            });
        }
        return result;
    }

    /// <summary>A query answer spec: null | {type: byte|short|ushort|int|uint|string|long|ushort[]|throw, value|values}.</summary>
    private static object? W402aValue(JsonNode? spec)
    {
        if (spec is null) return null;
        var o = spec.AsObject();
        var type = o["type"]!.GetValue<string>();
        return type switch
        {
            "byte" => (byte)o["value"]!.GetValue<int>(),
            "short" => (short)o["value"]!.GetValue<int>(),
            "ushort" => (ushort)o["value"]!.GetValue<int>(),
            "int" => o["value"]!.GetValue<int>(),
            "uint" => (uint)o["value"]!.GetValue<long>(),
            "long" => o["value"]!.GetValue<long>(),
            "string" => o["value"]!.GetValue<string>(),
            "ushort[]" => o["values"]!.AsArray().Select(v => (ushort)v!.GetValue<int>()).ToArray(),
            "throw" => throw new InvalidCastException("spec says throw"),
            _ => throw new ArgumentException("unknown value type " + type),
        };
    }

    /// <summary>Case {name, exif: spec, windows: spec} -> {orientation, asked:[\"exif\"|\"windows\" in call order]}.</summary>
    public static JsonArray OrientationReadWic(JsonObject inputs)
    {
        var result = new JsonArray();
        var exifQuery = W402aConst("ExifOrientationQuery");
        var windowsQuery = W402aConst("WindowsOrientationQuery");
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var asked = new List<string>();
            Func<string, object?> query = name =>
            {
                asked.Add(name);
                if (name == exifQuery) return W402aValue(c["exif"]);
                if (name == windowsQuery) return W402aValue(c["windows"]);
                throw new InvalidOperationException("unexpected query " + name);
            };
            var orientation = (int)W402aReadFromWic.Invoke(null, [query])!;
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["exif"] = c["exif"]?.DeepClone(),
                ["windows"] = c["windows"]?.DeepClone(),
                ["orientation"] = orientation,
                ["asked"] = new JsonArray(asked.Select(a => (JsonNode?)(a == exifQuery ? "exif" : "windows")).ToArray()),
            });
        }
        return result;
    }

    /// <summary>
    /// Case {name, orientation, width, height}: a Bgr32 buffer whose pixel (x, y) holds the id y*width+x, run through
    /// PixelOps.ApplyOrientation. Reports the new size and, for every destination pixel in row-major order, the source id
    /// that landed there; "same_instance" is true when the source buffer itself came back (orientation outside 2..8).
    /// </summary>
    public static JsonArray OrientationPixels(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            int orientation = c["orientation"]!.GetValue<int>(), w = c["width"]!.GetValue<int>(), h = c["height"]!.GetValue<int>();
            var src = PixelBuffer.Allocate(w, h, PixelLayout.Bgr32);
            for (var y = 0; y < h; y++)
            {
                var row = MemoryMarshal.Cast<byte, uint>(src.GetRow(y));
                for (var x = 0; x < w; x++) row[x] = (uint)((y * w) + x);
            }
            var dst = PixelOps.ApplyOrientation(src, orientation);
            var ids = new JsonArray();
            for (var y = 0; y < dst.Height; y++)
            {
                var row = MemoryMarshal.Cast<byte, uint>(dst.GetRow(y));
                for (var x = 0; x < dst.Width; x++) ids.Add((long)row[x]);
            }
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["orientation"] = orientation,
                ["width"] = w,
                ["height"] = h,
                ["same_instance"] = ReferenceEquals(src, dst),
                ["out_width"] = dst.Width,
                ["out_height"] = dst.Height,
                ["source_ids"] = ids,
            });
            dst.Dispose();
        }
        return result;
    }
}
