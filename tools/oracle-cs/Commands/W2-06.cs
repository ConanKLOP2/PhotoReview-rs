using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Model;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W2-06
//     "capture-groups" => Commands.CaptureGroups(inputs),
// Needs W2-00.cs (class W2Util).

// W2-06: CaptureGroupBuilder.Build / GroupEntries
static partial class Commands
{
    private static JsonNode? W206Group(CaptureGroup? g) => g is null
        ? null
        : new JsonObject { ["jpeg"] = g.JpegPath, ["raw"] = g.RawPath, ["xmp"] = g.XmpPath, ["paths"] = W2Util.StrArray(g.Paths) };

    /// <summary>
    /// Cases: {kind:"build", name, images:[...], sidecars:[...]|null} => "groups":[{jpeg,raw,xmp,paths}];
    /// {kind:"group-entries", name, mode, entries:[{path,length|null}], sidecars} => "result":[{path,length,group}].
    /// </summary>
    public static JsonArray CaptureGroups(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var sidecars = c["sidecars"] is null ? null : W2Util.Strings(c["sidecars"]);
            if (kind == "build")
            {
                var groups = CaptureGroupBuilder.Build(W2Util.NullableStrings(c["images"])!, sidecars);
                result.Add(new JsonObject
                {
                    ["kind"] = kind,
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["images"] = c["images"]!.DeepClone(),
                    ["sidecars"] = c["sidecars"]?.DeepClone(),
                    ["groups"] = new JsonArray(groups.Select(W206Group).ToArray()),
                });
            }
            else
            {
                var mode = Enum.Parse<RawPairMode>(c["mode"]!.GetValue<string>());
                var entries = c["entries"]!.AsArray().Select(e =>
                {
                    var o = e!.AsObject();
                    var entry = new CatalogEntry(o["path"]!.GetValue<string>());
                    return o["length"] is { } len ? entry with { Length = len.GetValue<long>() } : entry;
                }).ToList();
                var grouped = CaptureGroupBuilder.GroupEntries(entries, mode, sidecars);
                result.Add(new JsonObject
                {
                    ["kind"] = kind,
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["mode"] = mode.ToString(),
                    ["entries"] = c["entries"]!.DeepClone(),
                    ["sidecars"] = c["sidecars"]?.DeepClone(),
                    ["result"] = new JsonArray(grouped.Select(e => (JsonNode?)new JsonObject
                    {
                        ["path"] = e.Path,
                        ["length"] = e.Length,
                        ["group"] = W206Group(e.CaptureGroup),
                    }).ToArray()),
                });
            }
        }
        return result;
    }
}
