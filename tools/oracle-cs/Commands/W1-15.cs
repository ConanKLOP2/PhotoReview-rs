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

// W1-15 (from contract oracle-commands.cs)
static partial class Commands
{
// Paste into `static class Commands` in tools/oracle-cs/Program.cs.
// Needs: using System.Globalization; using PhotoReview.Core.Diagnostics;
// switch cases:  "percentile" => Commands.PercentileCases(inputs),  "diag-options" => Commands.DiagOptionsCases(inputs),

    private static double DiagNumber(JsonNode? node) => node!.GetValueKind() == JsonValueKind.String
        ? double.Parse(node.GetValue<string>(), NumberStyles.Float, CultureInfo.InvariantCulture)
        : node.GetValue<double>();

    /// <summary>
    /// BenchmarkStatistics.Percentile. Input numbers are JSON numbers or the strings "NaN", "Infinity", "-Infinity", "-0".
    /// The result is reported as exact IEEE bits (16 hex digits) plus its invariant "R" text; an exception is reported as "threw".
    /// </summary>
    public static JsonArray PercentileCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var values = node!["values"]!.AsArray().Select(DiagNumber).ToArray();
            var p = DiagNumber(node["p"]);
            var item = new JsonObject { ["values"] = node["values"]!.DeepClone(), ["p"] = node["p"]!.DeepClone() };
            try
            {
                var value = BenchmarkStatistics.Percentile(values, p);
                item["threw"] = null;
                item["bits"] = BitConverter.DoubleToInt64Bits(value).ToString("X16", CultureInfo.InvariantCulture);
                item["text"] = value.ToString("R", CultureInfo.InvariantCulture);
            }
            catch (ArgumentOutOfRangeException ex)
            {
                item["threw"] = ex.GetType().Name;
            }
            result.Add(item);
        }
        return result;
    }

    /// <summary>
    /// DiagOptions: each case sets the five PHOTOREVIEW_DIAG_* variables of this process (a missing key or null unsets it),
    /// calls ResetForTests and reads every property. The process variables are restored afterwards.
    /// </summary>
    public static JsonArray DiagOptionsCases(JsonObject inputs)
    {
        string[] names =
        [
            "PHOTOREVIEW_DIAG_PREREAD", "PHOTOREVIEW_DIAG_PRELOAD_WORKERS", "PHOTOREVIEW_DIAG_DISABLE_DISKCACHE",
            "PHOTOREVIEW_DIAG_FORCE_LOG", "PHOTOREVIEW_DIAG_INSTANCE_LABEL",
        ];
        var saved = names.ToDictionary(n => n, Environment.GetEnvironmentVariable);
        var result = new JsonArray();
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var env = node!["env"]!.AsObject();
                foreach (var name in names) Environment.SetEnvironmentVariable(name, env[name]?.GetValue<string>());
                DiagOptions.ResetForTests();
                result.Add(new JsonObject
                {
                    ["env"] = env.DeepClone(),
                    ["pre_read"] = DiagOptions.PreRead,
                    ["preload_workers"] = DiagOptions.PreloadWorkers,
                    ["disable_disk_cache"] = DiagOptions.DisableDiskCache,
                    ["force_log"] = DiagOptions.ForceLog,
                    ["instance_label"] = DiagOptions.InstanceLabel,
                    ["any_enabled"] = DiagOptions.AnyEnabled,
                    ["describe"] = DiagOptions.Describe(),
                });
            }
        }
        finally
        {
            foreach (var (name, value) in saved) Environment.SetEnvironmentVariable(name, value);
            DiagOptions.ResetForTests();
        }
        return result;
    }
}
