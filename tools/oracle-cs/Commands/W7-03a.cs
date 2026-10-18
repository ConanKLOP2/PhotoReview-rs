using System.Globalization;
using System.Text.Json.Nodes;
using PhotoReview.Benchmarking;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Diagnostics;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W7-03a
//     "bench-engine-run" => Commands.BenchEngineRun(inputs),
//     "bench-engine-format-ms" => Commands.BenchEngineFormatMs(inputs),
// Wiring (lead): OracleCs.csproj needs `<Reference Include="PhotoReview.Benchmarking">` with the same HintPath pattern
// as PhotoReview.Core, and tools/oracle.ps1 must copy PhotoReview.Benchmarking.dll into $(CSharpCoreDir).
// Needs W2-00.cs (class W2Util).

// W7-03a: BenchmarkEngine.RunPreparedAsync with a scripted executor and a hand-driven TimeProvider.
static partial class Commands
{
    private sealed class W703aClock : TimeProvider
    {
        private long _ticks;
        public override long TimestampFrequency => TimeSpan.TicksPerSecond;
        public override long GetTimestamp() => _ticks;
        public void Advance(long ticks) => _ticks += ticks;
    }

    private sealed class W703aLog : ILog
    {
        public readonly List<string> Lines = [];
        public bool Enabled => true;
        public void Info(string message) => Lines.Add("INFO " + message);
        public void Warn(string message) => Lines.Add("WARN " + message);
        public void Error(string message, Exception? ex = null) => Lines.Add("ERROR " + message + (ex is null ? "" : " | " + ex.GetType().Name));
    }

    private sealed class W703aProgress : IProgress<BenchmarkProgress>
    {
        public readonly JsonArray Events = [];
        public void Report(BenchmarkProgress p) => Events.Add(new JsonObject
        {
            ["profile_id"] = p.ProfileId,
            ["workload"] = p.Workload.ToString(),
            ["completed"] = p.Completed,
            ["total"] = p.Total,
            ["message"] = p.Message,
        });
    }

    /// <summary>The executor's cumulative snapshot after <paramref name="calls"/> measure steps (same shape as BenchmarkEngineTimingTests.Cumulative).</summary>
    private static ReviewMetricsSnapshot W703aCumulative(long calls) =>
        new(CacheHits: calls * 2, CacheMisses: calls, SourceBytesRead: calls * 1000, SourceReads: calls * 5,
            DecodeMilliseconds: calls * 7, PresentedImages: 0, PresentMilliseconds: 0)
        {
            PreloadHits = calls * 3,
            DiskCacheHits = calls,
            SourceOpenCount = calls * 5,
            TopSourceOpens = [new SourceOpenEntry("a.jpg", calls * 5), new SourceOpenEntry("b.jpg", calls)],
        };

    /// <summary>
    /// Per case {name, folder: "temp"|"empty"|"whitespace"|"missing"|"file",
    /// profile: {id, iterations?, warmup?, workers?, blank_id?}, steps: [{prepare_ticks?, measure_ticks?, correct?, metrics_calls? (int|null),
    /// throw?: "prepare"|"measure"|"cancel-prepare"|"cancel-measure", cancel_token_after?: bool}]}.
    /// Operation call k (warm-ups first, then timed iterations) uses steps[k]; past the end it is {prepare 0, measure 10000 ticks, correct, no metrics}.
    /// Time units are TimeSpan ticks (100 ns). "metrics_calls": null = the step returns no metrics; n = W703aCumulative(n).
    /// </summary>
    public static JsonArray BenchEngineRun(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-bench-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
            try
            {
                var folder = c["folder"]!.GetValue<string>() switch
                {
                    "temp" => root,
                    "empty" => "",
                    "whitespace" => "  ",
                    "missing" => Path.Combine(root, "missing"),
                    "file" => Path.Combine(root, "afile.txt"),
                    var other => throw new ArgumentException("unknown folder kind " + other),
                };
                if (c["folder"]!.GetValue<string>() == "file") File.WriteAllBytes(folder, [1]);
                var p = c["profile"]!.AsObject();
                var profile = BenchmarkProfiles.Find(p["id"]!.GetValue<string>())!;
                if (p["iterations"] is { } it) profile = profile with { Iterations = it.GetValue<int>() };
                if (p["warmup"] is { } wu) profile = profile with { WarmupCount = wu.GetValue<int>() };
                if (p["workers"] is { } wk) profile = profile with { Workers = wk.GetValue<int>() };
                if (p["blank_id"]?.GetValue<bool>() == true) profile = profile with { Id = " " };

                var steps = c["steps"]?.AsArray() ?? [];
                var clock = new W703aClock();
                var log = new W703aLog();
                var progress = new W703aProgress();
                var callOrder = new JsonArray();
                var cts = new CancellationTokenSource();
                var call = 0;

                Task<Func<Task<(bool Correct, ReviewMetricsSnapshot? Metrics)>>> Operation(BenchmarkProfile pr, BenchmarkWorkload wl, int iteration, CancellationToken ct)
                {
                    var step = call < steps.Count ? steps[call]!.AsObject() : new JsonObject();
                    call++;
                    callOrder.Add("prepare");
                    clock.Advance(step["prepare_ticks"]?.GetValue<long>() ?? 0);
                    var thrown = step["throw"]?.GetValue<string>();
                    if (thrown == "prepare") throw new InvalidOperationException("boom");
                    if (thrown == "cancel-prepare") throw new OperationCanceledException();
                    var metricsCalls = step.ContainsKey("metrics_calls") ? step["metrics_calls"]?.GetValue<long>() : null;
                    var correct = step["correct"]?.GetValue<bool>() ?? true;
                    return Task.FromResult<Func<Task<(bool Correct, ReviewMetricsSnapshot? Metrics)>>>(() =>
                    {
                        callOrder.Add("measure");
                        clock.Advance(step["measure_ticks"]?.GetValue<long>() ?? 10000);
                        if (thrown == "measure") throw new InvalidOperationException("boom");
                        if (thrown == "cancel-measure") throw new OperationCanceledException();
                        if (step["cancel_token_after"]?.GetValue<bool>() == true) cts.Cancel();
                        return Task.FromResult<(bool, ReviewMetricsSnapshot?)>((correct, metricsCalls is { } n ? W703aCumulative(n) : null));
                    });
                }

                var fullFolder0 = "";
                string Norm(string l) => System.Text.RegularExpressions.Regex.Replace(
                    fullFolder0.Length == 0 ? l : l.Replace(fullFolder0, "<folder>", StringComparison.Ordinal), "runId=[0-9a-f]{32}", "runId=<run>");
                var started = DateTimeOffset.UtcNow;
                var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["profile"] = c["profile"]!.DeepClone(), ["folder_kind"] = c["folder"]!.GetValue<string>() };
                var fullFolder = folder.Trim().Length == 0 ? "" : Path.GetFullPath(folder);
                fullFolder0 = fullFolder;
                try
                {
                    var report = BenchmarkEngine.RunPreparedAsync(folder, profile, Operation, progress, clock, log, cts.Token).GetAwaiter().GetResult();
                    var finished = DateTimeOffset.UtcNow;
                    var phase = report.Phases.Single();
                    row["exception"] = null;
                    row["run_id_is_32_lower_hex"] = report.RunId.Length == 32 && report.RunId.All(ch => ch is (>= '0' and <= '9') or (>= 'a' and <= 'f'));
                    row["started_utc_between_call_start_and_end"] = report.StartedUtc >= started && report.StartedUtc <= finished;
                    row["report_folder_is_full_path"] = report.Folder == fullFolder;
                    row["machine_is_environment_machine_name"] = report.Machine == Environment.MachineName;
                    row["phase_count"] = report.Phases.Count;
                    row["profile_id"] = phase.ProfileId;
                    row["workload"] = phase.Workload.ToString();
                    row["samples"] = new JsonArray(phase.Samples.Select(s => (JsonNode?)JsonValue.Create(s)).ToArray());
                    row["status"] = phase.Status.ToString();
                    row["message"] = phase.Message;
                    row["images_per_sample"] = phase.ImagesPerSample;
                    row["metrics"] = report.Metrics is { } m
                        ? new JsonObject
                        {
                            ["cache_hits"] = m.CacheHits,
                            ["cache_misses"] = m.CacheMisses,
                            ["source_bytes_read"] = m.SourceBytesRead,
                            ["source_reads"] = m.SourceReads,
                            ["decode_milliseconds"] = m.DecodeMilliseconds,
                            ["preload_hits"] = m.PreloadHits,
                            ["disk_cache_hits"] = m.DiskCacheHits,
                            ["source_open_count"] = m.SourceOpenCount,
                            ["top_source_opens"] = new JsonArray(m.TopSourceOpens.Select(e => (JsonNode?)new JsonObject { ["path"] = e.Path, ["count"] = e.Count }).ToArray()),
                        }
                        : null;
                    row["log"] = W2Util.StrArray(log.Lines.Select(Norm));
                }
                catch (Exception ex)
                {
                    row["exception"] = ex.GetType().Name;
                    row["exception_message"] = ex.Message.Replace(root, "<root>", StringComparison.Ordinal);
                    row["profile_problem"] = ex is BenchmarkProfileException bpe ? bpe.Problem.ToString() : null;
                    row["log"] = W2Util.StrArray(log.Lines.Select(Norm));
                }
                row["progress"] = progress.Events;
                row["call_order"] = callOrder;
                result.Add(row);
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch { }
            }
        }
        return result;
    }

    /// <summary>
    /// The private BenchmarkEngine.Ms(double) is exactly <c>value.ToString("F1", InvariantCulture)</c>; for each case {ticks} (TimeSpan ticks,
    /// the value is TimeSpan.FromTicks(ticks).TotalMilliseconds, as the engine formats it) report that text. Also reports
    /// GetElapsedTime for a counter {frequency, start, end}: "elapsed_ticks" (TimeSpan ticks of the delta).
    /// </summary>
    public static JsonArray BenchEngineFormatMs(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            if (c["ticks"] is { } t)
            {
                var ms = TimeSpan.FromTicks(t.GetValue<long>()).TotalMilliseconds;
                row["ticks"] = t.GetValue<long>();
                row["text"] = ms.ToString("F1", CultureInfo.InvariantCulture);
            }
            else
            {
                var frequency = c["frequency"]!.GetValue<long>();
                W703aFrequencyProvider.Frequency = frequency; // set BEFORE construction: the base constructor may read TimestampFrequency
                var provider = new W703aFrequencyProvider();
                var elapsed = provider.GetElapsedTime(c["start"]!.GetValue<long>(), c["end"]!.GetValue<long>());
                row["frequency"] = frequency;
                row["start"] = c["start"]!.GetValue<long>();
                row["end"] = c["end"]!.GetValue<long>();
                row["elapsed_ticks"] = elapsed.Ticks;
            }
            result.Add(row);
        }
        return result;
    }

    private sealed class W703aFrequencyProvider : TimeProvider
    {
        public static long Frequency;
        public override long TimestampFrequency => Frequency;
    }
}
