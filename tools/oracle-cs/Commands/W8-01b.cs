using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.App.Input;
using PhotoReview.App.ViewModels;
using PhotoReview.App.Viewport;

// W8-01b: ViewportLayoutEngine.Compute and ViewportLayoutEngine.ClampOffset (PhotoReview.App.Shared/Viewport/ViewportLayoutEngine.cs)
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-01b
//     "viewport-layout-engine" => Commands.ViewportLayoutEngineCases(inputs),
// Wiring (the lead does this; the writer did not run the oracle):
//   * tools/oracle-cs/OracleCs.csproj: reference PhotoReview.App.Shared.dll (HintPath $(CSharpCoreDir)\PhotoReview.App.Shared.dll,
//     or a ProjectReference to src\PhotoReview.App.Shared). App.Shared targets net10.0-windows: check the oracle TargetFramework.
//   * tools/oracle.ps1: add 'PhotoReview.App.Shared.dll' to $requiredDlls and, after the other builds, build
//         src\PhotoReview.App.Shared\PhotoReview.App.Shared.csproj -c Release -o $coreDir
// Both entry points are public (`public static partial`), so no reflection and no InternalsVisibleTo is needed.
// No ambient state: no clock, no cwd, no file system. Output is exact IEEE bits plus the invariant "R" text.
//
// Input case: { "name", "input": { client_width, client_height, scroll_bar_thickness, scroll_bars ("Auto"|"Overlay"|"Hidden"),
//   stretch ("None"|"Uniform"), image_width, image_height, max_image_width, max_image_height, bitmap_width, bitmap_height },
//   "probes": [[horizontal, vertical], ...] }.
// Numbers are JSON numbers or the strings "NaN", "Infinity", "-Infinity", "-0" (a bare JSON -0 is not used: it is a string).
// Output case: { "name", "input" (copied), "layout": { viewport_width, viewport_height, extent_width, extent_height,
//   horizontal_bar_visible, vertical_bar_visible, image_x, image_y, image_width, image_height, max_horizontal_offset,
//   max_vertical_offset }, each double as { "bits": 16 hex digits, "text": "R" }, "clamp": [ { "horizontal", "vertical" } per probe ] }.

static partial class Commands
{
    private static double ViewportNumber(JsonNode? node) => node!.GetValueKind() == JsonValueKind.String
        ? double.Parse(node.GetValue<string>(), NumberStyles.Float, CultureInfo.InvariantCulture)
        : node.GetValue<double>();

    private static JsonObject ViewportNum(double value) => new()
    {
        ["bits"] = BitConverter.DoubleToInt64Bits(value).ToString("X16", CultureInfo.InvariantCulture),
        ["text"] = value.ToString("R", CultureInfo.InvariantCulture),
    };

    /// <summary>
    /// ViewportLayoutEngine.Compute followed by ViewportLayoutEngine.ClampOffset for each probe of the same layout.
    /// A thrown exception is reported as "threw" (none is expected: the engine has no throwing path).
    /// </summary>
    public static JsonArray ViewportLayoutEngineCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var i = c["input"]!.AsObject();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["input"] = i.DeepClone() };
            try
            {
                var input = new ViewportInput(
                    ViewportNumber(i["client_width"]), ViewportNumber(i["client_height"]), ViewportNumber(i["scroll_bar_thickness"]),
                    Enum.Parse<ScrollBarPolicy>(i["scroll_bars"]!.GetValue<string>()),
                    Enum.Parse<ViewerStretchMode>(i["stretch"]!.GetValue<string>()),
                    ViewportNumber(i["image_width"]), ViewportNumber(i["image_height"]),
                    ViewportNumber(i["max_image_width"]), ViewportNumber(i["max_image_height"]),
                    ViewportNumber(i["bitmap_width"]), ViewportNumber(i["bitmap_height"]));
                var layout = ViewportLayoutEngine.Compute(input);
                item["layout"] = new JsonObject
                {
                    ["viewport_width"] = ViewportNum(layout.ViewportWidth),
                    ["viewport_height"] = ViewportNum(layout.ViewportHeight),
                    ["extent_width"] = ViewportNum(layout.ExtentWidth),
                    ["extent_height"] = ViewportNum(layout.ExtentHeight),
                    ["horizontal_bar_visible"] = layout.HorizontalBarVisible,
                    ["vertical_bar_visible"] = layout.VerticalBarVisible,
                    ["image_x"] = ViewportNum(layout.ImageRect.X),
                    ["image_y"] = ViewportNum(layout.ImageRect.Y),
                    ["image_width"] = ViewportNum(layout.ImageRect.Width),
                    ["image_height"] = ViewportNum(layout.ImageRect.Height),
                    ["max_horizontal_offset"] = ViewportNum(layout.MaxHorizontalOffset),
                    ["max_vertical_offset"] = ViewportNum(layout.MaxVerticalOffset),
                };
                var clamp = new JsonArray();
                foreach (var probeNode in c["probes"]!.AsArray())
                {
                    var probe = probeNode!.AsArray();
                    var (h, v) = ViewportLayoutEngine.ClampOffset(layout, ViewportNumber(probe[0]), ViewportNumber(probe[1]));
                    clamp.Add(new JsonObject { ["horizontal"] = ViewportNum(h), ["vertical"] = ViewportNum(v) });
                }
                item["clamp"] = clamp;
            }
            catch (Exception ex)
            {
                item["threw"] = ex.GetType().Name;
            }
            result.Add(item);
        }
        return result;
    }
}
