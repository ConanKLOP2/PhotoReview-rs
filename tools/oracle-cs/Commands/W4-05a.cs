using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Imaging;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-05a
//     "adaptive-preview-box" => Commands.AdaptivePreviewBox(inputs),
//     "adaptive-preview-quantize" => Commands.AdaptivePreviewQuantize(inputs),
// WIRING: AdaptivePreviewPolicy and DecodeBox live in PhotoReview.Imaging.dll, which tools/oracle-cs/OracleCs.csproj does not
// reference yet: add <Reference Include="PhotoReview.Imaging"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.dll</HintPath></Reference>
// (tools/oracle.ps1 must copy the baseline build output of that project next to PhotoReview.Core.dll).
// The class is pure (no I/O, clock or culture), so the output is deterministic (D19/D27).

// W4-05a: AdaptivePreviewPolicy.CalculateTargetDecodeBox / Quantize
static partial class Commands
{
    /// <summary>A JSON number, or one of the strings "NaN", "Infinity", "-Infinity", "-0" (JSON has no such literals).</summary>
    private static double W405aNumber(JsonNode node)
    {
        if (node.GetValueKind() == JsonValueKind.String)
        {
            return node.GetValue<string>() switch
            {
                "NaN" => double.NaN,
                "Infinity" => double.PositiveInfinity,
                "-Infinity" => double.NegativeInfinity,
                "-0" => -0.0,
                var other => throw new ArgumentException("unknown number text " + other),
            };
        }

        return node.GetValue<double>();
    }

    /// <summary>
    /// CalculateTargetDecodeBox for each case {name, viewport_width, viewport_height, dpi_scale?, quality_multiplier?}.
    /// An omitted dpi_scale / quality_multiplier uses the C# default (1.0), which pins the default arguments.
    /// Result: {name, width, height} or {name, error: "ArgumentOutOfRangeException", param} (ParamName = first invalid argument).
    /// </summary>
    public static JsonArray AdaptivePreviewBox(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var width = W405aNumber(c["viewport_width"]!);
            var height = W405aNumber(c["viewport_height"]!);
            var dpi = c["dpi_scale"] is { } d ? W405aNumber(d) : 1.0;
            var quality = c["quality_multiplier"] is { } q ? W405aNumber(q) : 1.0;
            var entry = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            try
            {
                // The explicit defaults are the C# defaults (dpiScale = 1.0, qualityMultiplier = 1.0): same code path.
                var box = c["dpi_scale"] is null && c["quality_multiplier"] is null
                    ? AdaptivePreviewPolicy.CalculateTargetDecodeBox(width, height)
                    : AdaptivePreviewPolicy.CalculateTargetDecodeBox(width, height, dpi, quality);
                entry["width"] = box.Width;
                entry["height"] = box.Height;
            }
            catch (ArgumentOutOfRangeException ex)
            {
                entry["error"] = "ArgumentOutOfRangeException";
                entry["param"] = ex.ParamName;
            }

            result.Add(entry);
        }

        return result;
    }

    /// <summary>Quantize for each case {name, side}: {name, result} (the int, NaN included: pins the saturating (int) cast).</summary>
    public static JsonArray AdaptivePreviewQuantize(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["result"] = AdaptivePreviewPolicy.Quantize(W405aNumber(c["side"]!)),
            });
        }

        return result;
    }
}
