using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Benchmarking;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Model;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W7-01b
//     "benchmark-profiles" => Commands.BenchmarkProfilesCases(inputs),
//     "benchmark-metrics-since" => Commands.BenchmarkMetricsSinceCases(inputs),
// Wiring (the lead does this, the writer does not run the oracle):
//   * tools/oracle-cs/OracleCs.csproj: add
//         <Reference Include="PhotoReview.Benchmarking"><HintPath>$(CSharpCoreDir)\PhotoReview.Benchmarking.dll</HintPath></Reference>
//   * tools/oracle.ps1: add 'PhotoReview.Benchmarking.dll' to $requiredDlls and, after the Imaging build, build
//         src\PhotoReview.Benchmarking\PhotoReview.Benchmarking.csproj -c Release -o $coreDir
//     (it references Core, Imaging, Platform.Windows and PerfAnalysis; all land in $coreDir).
// No ambient state: nothing reads the clock, the cwd or the file system.

// W7-01b: BenchmarkProfiles (All / Runnable / Find / IsNotImplemented) and BenchmarkMetrics.Since
static partial class Commands
{
    private static JsonObject W701bProfile(BenchmarkProfile p) => new()
    {
        ["id"] = p.Id,
        ["name"] = p.Name,
        ["description"] = p.Description,
        ["loading_mode"] = p.LoadingMode.ToString(),
        ["workers"] = p.Workers,
        ["next_window"] = p.NextWindow,
        ["previous_window"] = p.PreviousWindow,
        ["full_folder"] = p.FullFolder,
        ["memory_reserve_bytes"] = p.MemoryReserveBytes,
        ["disk_cache"] = p.DiskCache,
        ["detailed_logging"] = p.DetailedLogging,
        ["workload"] = p.Workload.ToString(),
        ["warmup_count"] = p.WarmupCount,
        ["iterations"] = p.Iterations,
        ["correctness_only"] = p.CorrectnessOnly,
    };

    /// <summary>
    /// Cases {name, kind}:
    ///  kind "registry": "profiles" (All in order, every field), "runnable_ids", "not_implemented_ids" (ids of All for which IsNotImplemented is true).
    ///  kind "find" + "id": "found_id" (null when Find returns null) and "found_index" (index in All, null when not found);
    ///    "same_instance_twice" (Find(id) called twice returns the same object).
    ///  kind "not_implemented" + "id" + "workload": IsNotImplemented of a copy of Find(id) (or of All[0] with that id when unknown) whose
    ///    Workload is replaced; result "value".
    /// </summary>
    public static JsonArray BenchmarkProfilesCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = c["kind"]!.GetValue<string>() };
            switch (c["kind"]!.GetValue<string>())
            {
                case "registry":
                    item["profiles"] = new JsonArray(BenchmarkProfiles.All.Select(p => (JsonNode?)W701bProfile(p)).ToArray());
                    item["runnable_ids"] = new JsonArray(BenchmarkProfiles.Runnable.Select(p => (JsonNode?)p.Id).ToArray());
                    item["not_implemented_ids"] = new JsonArray(
                        BenchmarkProfiles.All.Where(BenchmarkProfiles.IsNotImplemented).Select(p => (JsonNode?)p.Id).ToArray());
                    break;
                case "find":
                {
                    var id = c["id"]!.GetValue<string>();
                    var found = BenchmarkProfiles.Find(id);
                    item["id"] = id;
                    item["found_id"] = found?.Id;
                    item["found_index"] = found is null ? null : (int?)BenchmarkProfiles.All.ToList().IndexOf(found);
                    item["same_instance_twice"] = ReferenceEquals(found, BenchmarkProfiles.Find(id));
                    break;
                }
                case "not_implemented":
                {
                    var id = c["id"]!.GetValue<string>();
                    var workload = Enum.Parse<BenchmarkWorkload>(c["workload"]!.GetValue<string>());
                    var baseProfile = BenchmarkProfiles.Find(id) ?? (BenchmarkProfiles.All[0] with { Id = id });
                    item["id"] = id;
                    item["workload"] = workload.ToString();
                    item["value"] = BenchmarkProfiles.IsNotImplemented(baseProfile with { Workload = workload });
                    break;
                }
                default:
                    throw new ArgumentException("unknown kind " + c["kind"]);
            }
            result.Add(item);
        }
        return result;
    }

    private static long W701bLong(JsonObject o, string key) => o[key]?.GetValue<long>() ?? 0;

    private static ReviewMetricsSnapshot W701bSnapshot(JsonObject o) => new(
        W701bLong(o, "cache_hits"), W701bLong(o, "cache_misses"), W701bLong(o, "source_bytes_read"), W701bLong(o, "source_reads"),
        W701bLong(o, "decode_milliseconds"), W701bLong(o, "presented_images"), W701bLong(o, "present_milliseconds"))
    {
        PreloadHits = W701bLong(o, "preload_hits"),
        InflightJoins = W701bLong(o, "inflight_joins"),
        DiskCacheHits = W701bLong(o, "disk_cache_hits"),
        UiAssignMilliseconds = W701bLong(o, "ui_assign_milliseconds"),
        // Listed order is the insertion order, and a Dictionary enumerates in insertion order when nothing is removed.
        DecoderFallbacks = (o["decoder_fallbacks"]?.AsArray() ?? []).ToDictionary(
            e => Enum.Parse<DecoderBackend>(e!["backend"]!.GetValue<string>()), e => e!["count"]!.GetValue<long>()),
        SourceOpenCount = W701bLong(o, "source_open_count"),
        TopSourceOpens = [.. (o["top_source_opens"]?.AsArray() ?? []).Select(e => new SourceOpenEntry(e!["path"]!.GetValue<string>(), e["count"]!.GetValue<long>()))],
        StatCount = W701bLong(o, "stat_count"),
        SessionWriteCount = W701bLong(o, "session_write_count"),
        CrossThreadPresentCount = W701bLong(o, "cross_thread_present_count"),
        PresentHistogram = [.. (o["present_histogram"]?.AsArray() ?? []).Select(e => new PresentLatencyBucket(e!["label"]!.GetValue<string>(), e["count"]!.GetValue<long>()))],
    };

    private static JsonObject W701bSnapshotJson(ReviewMetricsSnapshot s) => new()
    {
        ["cache_hits"] = s.CacheHits,
        ["cache_misses"] = s.CacheMisses,
        ["source_bytes_read"] = s.SourceBytesRead,
        ["source_reads"] = s.SourceReads,
        ["decode_milliseconds"] = s.DecodeMilliseconds,
        ["presented_images"] = s.PresentedImages,
        ["present_milliseconds"] = s.PresentMilliseconds,
        ["preload_hits"] = s.PreloadHits,
        ["inflight_joins"] = s.InflightJoins,
        ["disk_cache_hits"] = s.DiskCacheHits,
        ["ui_assign_milliseconds"] = s.UiAssignMilliseconds,
        ["decoder_fallbacks"] = new JsonArray(s.DecoderFallbacks.Select(kv => (JsonNode?)new JsonObject { ["backend"] = kv.Key.ToString(), ["count"] = kv.Value }).ToArray()),
        ["source_open_count"] = s.SourceOpenCount,
        ["top_source_opens"] = new JsonArray(s.TopSourceOpens.Select(e => (JsonNode?)new JsonObject { ["path"] = e.Path, ["count"] = e.Count }).ToArray()),
        ["stat_count"] = s.StatCount,
        ["session_write_count"] = s.SessionWriteCount,
        ["cross_thread_present_count"] = s.CrossThreadPresentCount,
        ["present_histogram"] = new JsonArray(s.PresentHistogram.Select(b => (JsonNode?)new JsonObject { ["label"] = b.Label, ["count"] = b.Count }).ToArray()),
    };

    /// <summary>
    /// BenchmarkMetrics.Since(current, baseline) (internal class, reached by reflection) for each case {name, current, baseline}, both in
    /// the snapshot shape above (every scalar optional, default 0). Result: "threw" (null, or the exception type name: ToDictionary on a
    /// duplicate baseline key throws ArgumentException) and "since" (the snapshot in the same shape, null when it threw).
    /// </summary>
    public static JsonArray BenchmarkMetricsSinceCases(JsonObject inputs)
    {
        var since = typeof(BenchmarkProfiles).Assembly.GetType("PhotoReview.Benchmarking.BenchmarkMetrics", throwOnError: true)!
            .GetMethod("Since", BindingFlags.Public | BindingFlags.Static)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["current"] = c["current"]!.DeepClone(),
                ["baseline"] = c["baseline"]!.DeepClone(),
            };
            try
            {
                var delta = (ReviewMetricsSnapshot)since.Invoke(null, [W701bSnapshot(c["current"]!.AsObject()), W701bSnapshot(c["baseline"]!.AsObject())])!;
                item["threw"] = null;
                item["since"] = W701bSnapshotJson(delta);
            }
            catch (TargetInvocationException ex)
            {
                item["threw"] = ex.InnerException!.GetType().Name;
                item["since"] = null;
            }
            result.Add(item);
        }
        return result;
    }
}
