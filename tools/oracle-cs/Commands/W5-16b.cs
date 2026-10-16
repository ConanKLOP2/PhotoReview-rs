using System.Reflection;
using System.Text.Json.Nodes;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-16b
//     "libraw-memory-guard" => Commands.LibRawMemoryGuard(inputs),
// OracleCs.csproj needs one more <Reference Include="PhotoReview.Imaging.LibRaw"> (HintPath $(CSharpCoreDir)\PhotoReview.Imaging.LibRaw.dll) and
// tools/oracle.ps1 must build that project next to PhotoReview.Imaging. DecodeMemoryGuard, its RawBufferFamily and RawStructure are INTERNAL and
// the oracle assembly is not in InternalsVisibleTo (the C# repo is never edited), so everything is reached by reflection. Loading the assembly
// does not load libraw.dll (DecodeMemoryGuard calls no P/Invoke). The oracle needs no native library.

// W5-16b: DecodeMemoryGuard. One command, one case per entry. Case = { name, op, ... }.
//   op "constants"                                                       -> { values: { WorkingImageBytesPerPixel, RgbOutputBytesPerPixel,
//                                                                              BitmapBytesPerPixel, BayerRawBytesPerPixel, LinearRawBytesPerPixel,
//                                                                              FloatRawBytesPerPixel } }
//   op "is-bayer"  { colors:int, filters:uint }                           -> { value: bool }
//   op "classify"  { decoder: string|null, structure: {colors,filters}|null } -> { family: "Bayer"|"Linear"|"Float" }
//   op "estimate"  { width,height,target_width,target_height,raw_width,raw_height : int, family: "Bayer"|"Linear"|"Float" } -> { bytes: long }
//   op "estimate-defaults" { width,height,target_width,target_height }    -> { bytes: long }   (the C# optional arguments left out)
static partial class Commands
{
    private static readonly Assembly W516bAssembly = typeof(PhotoReview.Imaging.LibRaw.LibRawDecoder).Assembly;
    private static readonly Type W516bGuard = W516bAssembly.GetType("PhotoReview.Imaging.LibRaw.DecodeMemoryGuard", throwOnError: true)!;
    private static readonly Type W516bFamily = W516bGuard.GetNestedType("RawBufferFamily", BindingFlags.NonPublic | BindingFlags.Public)!;
    private static readonly Type W516bStructure = W516bGuard.GetNestedType("RawStructure", BindingFlags.NonPublic | BindingFlags.Public)!;
    private const BindingFlags W516bStatic = BindingFlags.Static | BindingFlags.NonPublic | BindingFlags.Public;
    private const BindingFlags W516bInstance = BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public;

    private static object W516bMakeStructure(JsonNode node) =>
        Activator.CreateInstance(W516bStructure, node["colors"]!.GetValue<int>(), unchecked((uint)node["filters"]!.GetValue<long>()))!;

    public static JsonArray LibRawMemoryGuard(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op };
            switch (op)
            {
                case "constants":
                {
                    var values = new JsonObject();
                    foreach (var n in new[] { "WorkingImageBytesPerPixel", "RgbOutputBytesPerPixel", "BitmapBytesPerPixel",
                                              "BayerRawBytesPerPixel", "LinearRawBytesPerPixel", "FloatRawBytesPerPixel" })
                        values[n] = (int)W516bGuard.GetField(n, W516bStatic)!.GetRawConstantValue()!;
                    o["values"] = values;
                    break;
                }
                case "is-bayer":
                {
                    var s = W516bMakeStructure(c);
                    o["value"] = (bool)W516bStructure.GetProperty("IsBayer", W516bInstance)!.GetValue(s)!;
                    break;
                }
                case "classify":
                {
                    var decoder = c["decoder"] is { } d ? d.GetValue<string>() : null;
                    object? structure = c["structure"] is { } st ? W516bMakeStructure(st) : null;
                    var family = W516bGuard.GetMethod("Classify", W516bStatic)!.Invoke(null, [decoder, structure])!;
                    o["family"] = family.ToString();
                    break;
                }
                case "estimate":
                {
                    var family = Enum.Parse(W516bFamily, c["family"]!.GetValue<string>());
                    o["bytes"] = (long)W516bGuard.GetMethod("EstimatePeakBytes", W516bStatic)!.Invoke(null,
                    [
                        c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>(),
                        c["target_width"]!.GetValue<int>(), c["target_height"]!.GetValue<int>(),
                        c["raw_width"]!.GetValue<int>(), c["raw_height"]!.GetValue<int>(), family,
                    ])!;
                    break;
                }
                case "estimate-defaults":
                {
                    var family = Enum.Parse(W516bFamily, "Bayer");
                    o["bytes"] = (long)W516bGuard.GetMethod("EstimatePeakBytes", W516bStatic)!.Invoke(null,
                    [
                        c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>(),
                        c["target_width"]!.GetValue<int>(), c["target_height"]!.GetValue<int>(), 0, 0, family,
                    ])!;
                    break;
                }
                default: throw new ArgumentException("unknown op " + op);
            }
            result.Add(o);
        }
        return result;
    }
}
