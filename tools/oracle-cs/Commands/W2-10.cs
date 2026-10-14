using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Model;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W2-10
//     "review-catalog" => Commands.ReviewCatalogTrace(inputs),
// Needs W2-00.cs (class W2Util).

// W2-10: ReviewCatalog scripted traces (like lru-trace): after EVERY operation the full observable state is recorded.
static partial class Commands
{
    private static CaptureGroup? W210Group(JsonNode? g)
    {
        if (g is not JsonObject o) return null;
        return new CaptureGroup(o["jpeg"]!.GetValue<string>(), o["raw"]!.GetValue<string>(), W2Util.Str(o["xmp"]));
    }

    private static CatalogEntry W210Entry(JsonNode? e)
    {
        var o = e!.AsObject();
        var entry = new CatalogEntry(o["path"]!.GetValue<string>());
        if (o["length"] is { } len) entry = entry with { Length = len.GetValue<long>() };
        if (o["width"] is { } w) entry = entry with { Width = w.GetValue<int>() };
        if (o["group"] is JsonObject) entry = entry with { CaptureGroup = W210Group(o["group"]) };
        return entry;
    }

    private static JsonNode? W210Snapshot(ReviewCatalog catalog, List<string> probes, string op, JsonNode? opResult)
    {
        var state = new JsonObject
        {
            ["op"] = op,
            ["result"] = opResult,
            ["count"] = catalog.Count,
            ["current_index"] = catalog.CurrentIndex,
            ["current_path"] = catalog.Current?.Path,
            ["structural_version"] = catalog.StructuralVersion,
            ["paths"] = W2Util.StrArray(catalog.Paths),
            ["entries"] = new JsonArray(catalog.EntriesSnapshot().Select(e => (JsonNode?)new JsonObject
            {
                ["path"] = e.Path,
                ["length"] = e.Length,
                ["width"] = e.Width,
                ["last_write_ticks"] = e.LastWriteUtc?.Ticks,
                ["group"] = e.CaptureGroup is null ? null : new JsonObject
                {
                    ["jpeg"] = e.CaptureGroup.JpegPath,
                    ["raw"] = e.CaptureGroup.RawPath,
                    ["xmp"] = e.CaptureGroup.XmpPath,
                },
            }).ToArray()),
        };
        var lookups = new JsonObject();
        foreach (var p in probes) lookups[p] = catalog.IndexOf(p);
        state["index_of"] = lookups;
        return state;
    }

    /// <summary>
    /// Cases {name, probes:[paths to IndexOf after each op], ops:[...]}. Ops (field "op"): reset{entries:[{path,length?,width?}], mode, sidecars?},
    /// setCurrent{index}, remove{path}, restore{path,index}, restoreMembers{paths,index,group?{jpeg,raw,xmp?},rawEnabled?(default true)},
    /// replaceOrder{paths}, removePaths{paths}, updateMetadata{path,length,ticks,width?,height?}. The result of each op (bool / index / removed
    /// paths) is recorded with the full state afterwards.
    /// </summary>
    public static JsonArray ReviewCatalogTrace(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var catalog = new ReviewCatalog();
            var probes = W2Util.Strings(c["probes"]);
            var steps = new JsonArray();
            steps.Add(W210Snapshot(catalog, probes, "new", null));
            foreach (var opNode in c["ops"]!.AsArray())
            {
                var o = opNode!.AsObject();
                var op = o["op"]!.GetValue<string>();
                JsonNode? opResult = null;
                switch (op)
                {
                    case "reset":
                        catalog.Reset(o["entries"]!.AsArray().Select(W210Entry).ToList(),
                            Enum.Parse<RawPairMode>(o["mode"]?.GetValue<string>() ?? "Separate"),
                            o["sidecars"] is null ? null : W2Util.Strings(o["sidecars"]));
                        break;
                    case "setCurrent":
                        opResult = catalog.SetCurrent(o["index"]!.GetValue<int>());
                        break;
                    case "remove":
                        opResult = catalog.Remove(o["path"]!.GetValue<string>());
                        break;
                    case "restore":
                        opResult = catalog.Restore(o["path"]!.GetValue<string>(), o["index"]!.GetValue<int>());
                        break;
                    case "restoreMembers":
                        opResult = catalog.RestoreMembers(W2Util.Strings(o["paths"]), o["index"]!.GetValue<int>(),
                            W210Group(o["group"]), o["rawEnabled"]?.GetValue<bool>() ?? true);
                        break;
                    case "replaceOrder":
                        opResult = catalog.ReplaceOrder(W2Util.Strings(o["paths"]));
                        break;
                    case "removePaths":
                        opResult = W2Util.StrArray(catalog.RemovePaths(W2Util.Strings(o["paths"])));
                        break;
                    case "updateMetadata":
                        opResult = catalog.UpdateMetadata(o["path"]!.GetValue<string>(), o["length"]!.GetValue<long>(),
                            new DateTime(o["ticks"]!.GetValue<long>(), DateTimeKind.Utc), o["width"]?.GetValue<int>(), o["height"]?.GetValue<int>());
                        break;
                    default:
                        throw new ArgumentException("unknown op " + op);
                }
                steps.Add(W210Snapshot(catalog, probes, op, opResult));
            }
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["ops"] = c["ops"]!.DeepClone(),
                ["probes"] = c["probes"]!.DeepClone(),
                ["steps"] = steps,
            });
        }
        return result;
    }
}
