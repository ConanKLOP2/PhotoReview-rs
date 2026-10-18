using System.Globalization;
using System.Numerics;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.App.Input;
using PhotoReview.Core.Model;
using PhotoReview.Shell.Rendering;

// W8-01c (from contract oracle-commands.cs)
// Command `render-geometry`: the pure part of PhotoReview.Shell.Rendering/RenderGeometry.cs at d92d6aae.
// The writer did not run this; the lead wires and runs it (see the contract, "Golden package").
//
// Paste into `static class Commands` in tools/oracle-cs/Program.cs (this file is the body of `static partial class Commands`).
// switch case:  "render-geometry" => Commands.RenderGeometry(inputs),
// csproj: the oracle must reference PhotoReview.Shell.Rendering (and App.Shared, Shell.Interop) built into $coreDir.
//
// Output conventions (both sides of the golden):
//   - a double is written as its 16 hex digits of IEEE bits (BitConverter.DoubleToInt64Bits, "X16"),
//   - a float is written as its 8 hex digits (BitConverter.SingleToInt32Bits, "X8"),
//   - a RectD is an array [x, y, width, height] of double bit strings; a PixelRect is an array of ints,
//   - NaN inputs come from JSON strings "NaN", "Infinity", "-Infinity" (as in the percentile command).
//   - internal types (TileLayout, DrawGeometry, PixelRect) are reached by reflection; their public members only.
static partial class Commands
{
    private static readonly Assembly RenderAssembly = typeof(RenderColors).Assembly;

    private static Type RenderType(string name) =>
        RenderAssembly.GetType("PhotoReview.Shell.Rendering." + name, throwOnError: true)!;

    private static T RenderCall<T>(string typeName, string method, params object?[] args) =>
        (T)RenderType(typeName).GetMethod(method, BindingFlags.Public | BindingFlags.Static)!.Invoke(null, args)!;

    private static double RgNumber(JsonNode? node) => node!.GetValueKind() == JsonValueKind.String
        ? double.Parse(node.GetValue<string>(), NumberStyles.Float, CultureInfo.InvariantCulture)
        : node.GetValue<double>();

    private static int RgInt(JsonNode? node) => node!.GetValue<int>();

    private static RectD RgRect(JsonNode? node)
    {
        var values = node!.AsArray();
        return new RectD(RgNumber(values[0]), RgNumber(values[1]), RgNumber(values[2]), RgNumber(values[3]));
    }

    private static JsonArray RgRectBits(RectD rect) => new JsonArray(
        DoubleBits(rect.X), DoubleBits(rect.Y), DoubleBits(rect.Width), DoubleBits(rect.Height));

    private static string DoubleBits(double value) =>
        BitConverter.DoubleToInt64Bits(value).ToString("X16", CultureInfo.InvariantCulture);

    private static string FloatBits(float value) =>
        BitConverter.SingleToInt32Bits(value).ToString("X8", CultureInfo.InvariantCulture);

    private static object? RgProperty(object value, string name) =>
        value.GetType().GetProperty(name)!.GetValue(value);

    private static JsonArray RgPixelRect(object pixel) => new JsonArray(
        (int)RgProperty(pixel, "X")!, (int)RgProperty(pixel, "Y")!,
        (int)RgProperty(pixel, "Width")!, (int)RgProperty(pixel, "Height")!);

    public static JsonArray RenderGeometry(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var input = node!.AsObject();
            var item = (JsonObject)input.DeepClone();
            switch (input["kind"]!.GetValue<string>())
            {
                case "dark_canvas":
                {
                    var color = RenderColors.DarkCanvas;
                    item["color"] = new JsonArray(FloatBits(color.R), FloatBits(color.G), FloatBits(color.B), FloatBits(color.A));
                    break;
                }
                case "color_from_rgb":
                {
                    var color = RenderColors.FromRgb(
                        (byte)RgInt(input["r"]), (byte)RgInt(input["g"]), (byte)RgInt(input["b"]), (byte)RgInt(input["a"]));
                    item["color"] = new JsonArray(FloatBits(color.R), FloatBits(color.G), FloatBits(color.B), FloatBits(color.A));
                    break;
                }
                case "interpolation":
                {
                    var quality = Enum.Parse<ScalingQuality>(input["quality"]!.GetValue<string>());
                    item["interpolation"] = ImageInterpolations.FromScalingQuality(quality).ToString();
                    break;
                }
                case "transposed":
                {
                    item["transposed"] = ImageTransforms.IsTransposed(RgInt(input["orientation"]));
                    break;
                }
                case "unoriented_rect":
                {
                    var rect = ImageTransforms.UnorientedRect(RgInt(input["orientation"]), RgRect(input["destination"]));
                    item["rect"] = RgRectBits(rect);
                    break;
                }
                case "exif_matrix":
                {
                    var m = ImageTransforms.ForExifOrientation(RgInt(input["orientation"]), RgRect(input["destination"]));
                    item["matrix"] = new JsonArray(
                        FloatBits(m.M11), FloatBits(m.M12), FloatBits(m.M21), FloatBits(m.M22), FloatBits(m.M31), FloatBits(m.M32));
                    break;
                }
                case "tile_layout":
                {
                    try
                    {
                        var compute = RenderType("TileLayout").GetMethod("Compute", BindingFlags.Public | BindingFlags.Static)!;
                        var tiles = (Array)compute.Invoke(null, new object[]
                        {
                            RgInt(input["width"]), RgInt(input["height"]), RgInt(input["max"]), RgInt(input["gutter"]),
                        })!;
                        var list = new JsonArray();
                        foreach (object tile in tiles)
                        {
                            list.Add(new JsonObject
                            {
                                ["texture"] = RgPixelRect(RgProperty(tile, "Texture")!),
                                ["core"] = RgPixelRect(RgProperty(tile, "Core")!),
                            });
                        }

                        item["tiles"] = list;
                        item["threw"] = null;
                    }
                    catch (TargetInvocationException ex) when (ex.InnerException is ArgumentOutOfRangeException range)
                    {
                        item["threw"] = nameof(ArgumentOutOfRangeException);
                        item["param"] = range.ParamName;
                    }

                    break;
                }
                case "intersect":
                {
                    var rect = RenderCall<RectD>("DrawGeometry", "Intersect", RgRect(input["a"]), RgRect(input["b"]));
                    item["rect"] = RgRectBits(rect);
                    break;
                }
                case "is_empty":
                {
                    item["empty"] = RenderCall<bool>("DrawGeometry", "IsEmpty", RgRect(input["rect"]));
                    break;
                }
                case "pixel_to_rect":
                {
                    var values = input["pixel"]!.AsArray();
                    object pixel = Activator.CreateInstance(
                        RenderType("PixelRect"), RgInt(values[0]), RgInt(values[1]), RgInt(values[2]), RgInt(values[3]))!;
                    var rect = RenderCall<RectD>("DrawGeometry", "ToRectD", pixel);
                    item["rect"] = RgRectBits(rect);
                    break;
                }
                case "map_to_destination":
                {
                    var rect = RenderCall<RectD>(
                        "DrawGeometry", "MapToDestination",
                        RgRect(input["source"]), RgRect(input["destination"]), RgRect(input["part"]));
                    item["rect"] = RgRectBits(rect);
                    break;
                }
                case "snap":
                {
                    var d = input["device"]!.AsArray();
                    var device = new Matrix3x2(
                        (float)RgNumber(d[0]), (float)RgNumber(d[1]), (float)RgNumber(d[2]),
                        (float)RgNumber(d[3]), (float)RgNumber(d[4]), (float)RgNumber(d[5]));
                    var args = new object?[] { device, RgRect(input["destination"]), RgRect(input["source"]), null };
                    var method = RenderType("DrawGeometry").GetMethod("TrySnapPixelExact", BindingFlags.Public | BindingFlags.Static)!;
                    bool exact = (bool)method.Invoke(null, args)!;
                    item["exact"] = exact;
                    item["snapped"] = RgRectBits((RectD)args[3]!);
                    break;
                }
                default:
                    throw new InvalidOperationException("unknown render-geometry kind: " + input["kind"]);
            }

            result.Add(item);
        }

        return result;
    }
}
