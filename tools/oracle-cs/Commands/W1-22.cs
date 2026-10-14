using System.Globalization;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using PhotoReview.Core.Caching;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Model;
using PhotoReview.Core.Settings;
using PhotoReview.Core.Updates;

// W1-22 (from contract oracle-commands.cs)
static partial class Commands
{
// Paste into `static class Commands` in tools/oracle-cs/Program.cs.
// Needs: using System.Globalization; using PhotoReview.Core.Abstractions;
// switch case:  "vblank" => Commands.VBlankCases(inputs),

    /// <summary>
    /// VBlankEstimator (public, pure): feeds a sequence of Add(timestamp) / Reset() operations and records the state
    /// after EVERY operation (Count, Current), so the point where a refresh-rate change drops the old samples is pinned too.
    /// ops: a JSON number or a decimal string (beyond 2^53) = Add; the string "reset" = Reset().
    /// </summary>
    public static JsonArray VBlankCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var estimator = new VBlankEstimator();
            var trace = new JsonArray();
            foreach (var op in node!["ops"]!.AsArray())
            {
                if (op!.GetValueKind() == JsonValueKind.String && op.GetValue<string>() == "reset") estimator.Reset();
                else
                {
                    var timestamp = op.GetValueKind() == JsonValueKind.String
                        ? long.Parse(op.GetValue<string>(), CultureInfo.InvariantCulture)
                        : op.GetValue<long>();
                    estimator.Add(timestamp);
                }
                var timing = estimator.Current;
                trace.Add(new JsonObject
                {
                    ["count"] = estimator.Count,
                    ["last"] = timing?.LastVBlank,
                    ["period"] = timing?.RefreshPeriod,
                });
            }
            result.Add(new JsonObject { ["name"] = node["name"]!.GetValue<string>(), ["ops"] = node["ops"]!.DeepClone(), ["trace"] = trace });
        }
        return result;
    }
}
