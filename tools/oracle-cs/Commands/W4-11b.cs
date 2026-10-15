using System.Linq.Expressions;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;
using PhotoReview.Imaging.Decoding.Wic;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-11b
//     "wic-codec-probe" => Commands.WicCodecProbeCases(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * WicCodecAvailability and WicCodecSupport are public; `Probe` and `DecoderEntry` are internal and are reached by
//     reflection / expression trees below, so no InternalsVisibleTo is needed. Needs no other oracle file.
//   * Only the pure `Probe` is differential. EnumerateDecoders / HasMediaFoundationDecoder are machine dependent and are
//     covered by the `#[ignore = "native: needs WIC"]` test, never by a golden.

// W4-11b: WicCodecAvailability.Probe through its two seams, plus the three GUID constants.
static partial class Commands
{
    private static readonly Type W411bAsm = typeof(WicCodecAvailability);
    private static readonly MethodInfo W411bProbe = W411bAsm.GetMethod("Probe", BindingFlags.Static | BindingFlags.NonPublic)!;
    private static readonly Type W411bEntry = W411bAsm.GetNestedType("DecoderEntry", BindingFlags.NonPublic)!;

    /// <summary>
    /// Cases {name, decoders:[{guid, name?, ext?}], enumerate?:"ok"|"throws:&lt;msg&gt;", hevc:"found"|"missing"|"throws:&lt;msg&gt;"}.
    /// Output per case: name, webp, heifContainer, hevcDecoder, heif, detail, hevcAskedFor (the GUID passed to the video
    /// seam, or null when it was not called), supports {none, webp, heif}.
    /// </summary>
    public static JsonArray WicCodecProbeCases(JsonObject inputs)
    {
        var result = new JsonArray();

        var consts = new JsonObject
        {
            ["webp"] = WicCodecAvailability.ContainerFormatWebp.ToString(),
            ["heif"] = WicCodecAvailability.ContainerFormatHeif.ToString(),
        };

        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var decoders = c["decoders"]?.AsArray() ?? new JsonArray();
            var list = Array.CreateInstance(W411bEntry, decoders.Count);
            for (var i = 0; i < decoders.Count; i++)
            {
                var d = decoders[i]!.AsObject();
                list.SetValue(Activator.CreateInstance(W411bEntry,
                    Guid.Parse(d["guid"]!.GetValue<string>()),
                    d["name"]?.GetValue<string>() ?? "name",
                    d["ext"]?.GetValue<string>() ?? ".ext"), i);
            }

            var enumerate = c["enumerate"]?.GetValue<string>() ?? "ok";
            var listType = typeof(IReadOnlyList<>).MakeGenericType(W411bEntry);
            var funcType = typeof(Func<>).MakeGenericType(listType);
            Expression body = enumerate.StartsWith("throws:", StringComparison.Ordinal)
                ? Expression.Throw(Expression.Constant(new InvalidOperationException(enumerate["throws:".Length..])), listType)
                : Expression.Constant(list, listType);
            var enumerateDelegate = Expression.Lambda(funcType, body).Compile();

            Guid? asked = null;
            var hevc = c["hevc"]!.GetValue<string>();
            Func<Guid, bool> hasVideo = subtype =>
            {
                asked = subtype;
                if (hevc.StartsWith("throws:", StringComparison.Ordinal))
                    throw new InvalidOperationException(hevc["throws:".Length..]);
                return hevc == "found";
            };

            var support = (WicCodecSupport)W411bProbe.Invoke(null, new object[] { enumerateDelegate, hasVideo })!;
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["webp"] = support.WebP,
                ["heifContainer"] = support.HeifContainer,
                ["hevcDecoder"] = support.HevcDecoder,
                ["heif"] = support.Heif,
                ["detail"] = support.Detail,
                ["hevcAskedFor"] = asked?.ToString(),
                ["supports"] = new JsonObject
                {
                    ["none"] = support.Supports(WicImageFormat.None),
                    ["webp"] = support.Supports(WicImageFormat.WebP),
                    ["heif"] = support.Supports(WicImageFormat.Heif),
                },
            });
        }

        // Constant check row (named so the Rust test can pin the three GUIDs without reading C#).
        result.Add(new JsonObject
        {
            ["name"] = "constants",
            ["webp"] = consts["webp"]!.GetValue<string>(),
            ["heif"] = consts["heif"]!.GetValue<string>(),
            ["hevc"] = W411bAsm.GetField("MediaSubtypeHevc", BindingFlags.Static | BindingFlags.NonPublic)!.GetValue(null)!.ToString(),
        });
        return result;
    }
}
