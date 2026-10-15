using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Catalog;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Preload;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-07a
//     "ram-budget-scalar" => Commands.RamBudgetScalar(inputs),
//     "ram-budget-estimate" => Commands.RamBudgetEstimate(inputs),
// WIRING: RamBudgetPolicy lives in PhotoReview.Imaging.dll (reference as in oracle/W4-05a.cs). Pure and deterministic (D19/D27);
// GetPhysicalMemoryBytes and the internal FolderEstimateCache are NOT exercised (OS value / internal type: Rust unit tests only).

// W4-07a: RamBudgetPolicy
static partial class Commands
{
    /// <summary>Probe with a fixed headroom answer.</summary>
    private sealed class W407aProbe(bool headroom) : IMemoryProbe
    {
        public bool HasHeadroom(double maximumLoad, long reserveBytes) => headroom;
        public MemorySnapshot? GetSnapshot() => null;
    }

    private static long W407aLong(JsonObject c, string key) => c[key] is { } n ? n.GetValue<long>() : 0;

    private static PreloadWindow W407aWindow(JsonObject c) =>
        c["window"] is JsonObject w ? new PreloadWindow(w["forward"]!.GetValue<int>(), w["backward"]!.GetValue<int>()) : PreloadWindow.Default;

    /// <summary>
    /// Scalar functions. Case {name, op, requested?, percent?, physical?, source?, estimated?, capacity?, headroom?, window?{forward,backward}}.
    /// op: clamp-to-physical | clamp-source-bytes | clamp-preview | minimum-window-bytes | minimum-cache-percent | clamp-cache-percent |
    /// bytes-for-percent | preview-bytes-for-percent | source-bytes-for-percent | should-preload.
    /// Result {name, result} (number, or bool for should-preload) or {name, error: "ArgumentOutOfRangeException", param}.
    /// </summary>
    public static JsonArray RamBudgetScalar(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var entry = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            var requested = W407aLong(c, "requested");
            var physical = W407aLong(c, "physical");
            var source = W407aLong(c, "source");
            var percent = c["percent"] is { } p ? p.GetValue<int>() : 0;
            var window = W407aWindow(c);
            try
            {
                switch (c["op"]!.GetValue<string>())
                {
                    case "clamp-to-physical": entry["result"] = RamBudgetPolicy.ClampToPhysicalMemory(requested, physical); break;
                    case "clamp-source-bytes": entry["result"] = RamBudgetPolicy.ClampSourceBytesToPhysicalMemory(requested, physical); break;
                    case "clamp-preview": entry["result"] = RamBudgetPolicy.ClampPreviewToPhysicalMemory(requested, physical, source); break;
                    case "minimum-window-bytes": entry["result"] = RamBudgetPolicy.MinimumPreviewWindowBytes(window); break;
                    case "minimum-cache-percent": entry["result"] = RamBudgetPolicy.MinimumCachePercent(physical, window); break;
                    case "clamp-cache-percent": entry["result"] = RamBudgetPolicy.ClampCachePercent(percent, physical, window); break;
                    case "bytes-for-percent": entry["result"] = RamBudgetPolicy.BytesForPercent(percent, physical); break;
                    case "preview-bytes-for-percent": entry["result"] = RamBudgetPolicy.PreviewBytesForPercent(percent, physical, source, window); break;
                    case "source-bytes-for-percent": entry["result"] = RamBudgetPolicy.SourceBytesForPercent(requested, percent, physical, window); break;
                    case "should-preload":
                        entry["result"] = RamBudgetPolicy.ShouldPreloadWholeFolderEstimate(
                            W407aLong(c, "estimated"), W407aLong(c, "capacity"),
                            new W407aProbe(c["headroom"] is { } h ? h.GetValue<bool>() : true), 0, 0.9);
                        break;
                    default: throw new ArgumentException("unknown op");
                }
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

    /// <summary>
    /// EstimateFolderPreviewBytes. Case {name, box:{width,height}, measured?: number|"Infinity"|"NaN", entries:[{path, length?, width?, height?}]}.
    /// Result {name, result: long} or {name, error: "OverflowException"}.
    /// </summary>
    public static JsonArray RamBudgetEstimate(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var entries = new List<CatalogEntry>();
            foreach (var e in c["entries"]!.AsArray())
            {
                var o = e!.AsObject();
                entries.Add(new CatalogEntry(o["path"]!.GetValue<string>())
                {
                    Length = o["length"] is { } l ? l.GetValue<long>() : null,
                    Width = o["width"] is { } w ? w.GetValue<int>() : null,
                    Height = o["height"] is { } h ? h.GetValue<int>() : null,
                });
            }

            double? measured = null;
            if (c["measured"] is { } m)
            {
                measured = m.GetValueKind() == JsonValueKind.String
                    ? m.GetValue<string>() switch
                    {
                        "NaN" => double.NaN,
                        "Infinity" => double.PositiveInfinity,
                        "-Infinity" => double.NegativeInfinity,
                        var other => throw new ArgumentException("unknown number text " + other),
                    }
                    : m.GetValue<double>();
            }

            var box = new DecodeBox(c["box"]!["width"]!.GetValue<int>(), c["box"]!["height"]!.GetValue<int>());
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            try
            {
                item["result"] = RamBudgetPolicy.EstimateFolderPreviewBytes(entries, box, measured);
            }
            catch (OverflowException)
            {
                item["error"] = "OverflowException";
            }

            result.Add(item);
        }

        return result;
    }
}
