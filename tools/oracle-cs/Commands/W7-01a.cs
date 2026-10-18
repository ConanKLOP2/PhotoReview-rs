using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Benchmarking;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Model;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W7-01a
//     "bench-profile" => Commands.BenchProfile(inputs),
//     "bench-phase" => Commands.BenchPhase(inputs),
//     "bench-rank" => Commands.BenchRank(inputs),
//     "bench-json" => Commands.BenchJson(inputs),
// Wiring must also add to tools/oracle-cs/OracleCs.csproj (and have tools/oracle.ps1 copy it into $(CSharpCoreDir)):
//     <Reference Include="PhotoReview.Benchmarking"><HintPath>$(CSharpCoreDir)\PhotoReview.Benchmarking.dll</HintPath></Reference>
// Needs W2-00.cs (class W2Util). Every double is reported as a string ("R" format, invariant) so NaN/-0 survive JSON.

// W7-01a: BenchmarkModels.cs (profile validation + TargetWidth, phase statistics, ranking, report/batch JSON)
static partial class Commands
{
    private static double W701Num(JsonNode? node)
    {
        if (node is JsonObject o) return BitConverter.Int64BitsToDouble(long.Parse(o["bits"]!.GetValue<string>(), NumberStyles.HexNumber, CultureInfo.InvariantCulture));
        if (node is JsonValue v && v.TryGetValue<string>(out var text))
            return text switch
            {
                "NaN" => double.NaN,
                "Infinity" => double.PositiveInfinity,
                "-Infinity" => double.NegativeInfinity,
                "-0" => -0.0,
                _ => throw new ArgumentException("bad number text " + text),
            };
        return node!.GetValue<double>();
    }

    private static string W701R(double d) => d.ToString("R", CultureInfo.InvariantCulture);

    private static int W701Int(JsonObject c, string key, int fallback) => c[key] is { } n ? n.GetValue<int>() : fallback;

    /// <summary>Phase spec: {id, workload (name), samples:[number|"NaN"|...], status (name), message?, images?}.</summary>
    private static BenchmarkPhaseResult W701Phase(JsonNode? node)
    {
        var o = node!.AsObject();
        var samples = o["samples"]!.AsArray().Select(W701Num).ToArray();
        return new BenchmarkPhaseResult(
            o["id"]!.GetValue<string>(),
            Enum.Parse<BenchmarkWorkload>(o["workload"]!.GetValue<string>()),
            samples,
            Enum.Parse<BenchmarkResultStatus>(o["status"]!.GetValue<string>()),
            W2Util.Str(o["message"]),
            o["images"] is { } img ? img.GetValue<int>() : null);
    }

    /// <summary>What the registry derives for a phase (BenchmarkRanking falls back to this when ImagesPerSample is null).</summary>
    private static int? W701Derived(BenchmarkPhaseResult p) =>
        BenchmarkProfiles.Find(p.ProfileId) is { } profile
            ? BenchmarkWorkloadRunner.ImagesPerSample(profile, p.Workload, int.MaxValue)
            : null;

    private static JsonArray W701DerivedList(IEnumerable<BenchmarkPhaseResult> phases) =>
        new(phases.Select(p => (JsonNode?)new JsonObject
        {
            ["id"] = p.ProfileId,
            ["workload"] = p.Workload.ToString(),
            ["images"] = W701Derived(p),
        }).ToArray());

    private static JsonObject W701Stats(BenchmarkPhaseResult p) => new()
    {
        ["count"] = p.Count,
        ["p50"] = W701R(p.P50),
        ["p95"] = W701R(p.P95),
        ["p99"] = W701R(p.P99),
        ["max"] = W701R(p.Max),
        ["effective_images"] = p.EffectiveImagesPerSample,
        ["p95_per_image"] = W701R(p.P95PerImage),
        ["p50_per_image"] = W701R(p.P50PerImage),
    };

    /// <summary>
    /// Cases {name, id|null, loading_mode (int, may be undefined), workers?, next?, previous?, iterations?, warmup?, null_profile?}:
    /// BenchmarkProfileValidation.Validate outcome and TargetWidth(). "representable" = Enum.IsDefined(loading_mode) (Rust cannot hold others).
    /// </summary>
    public static JsonArray BenchProfile(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var mode = (LoadingMode)c["loading_mode"]!.GetValue<int>();
            var profile = new BenchmarkProfile(
                W2Util.Str(c["id"])!, "Name", "Description", mode,
                W701Int(c, "workers", 1), W701Int(c, "next", 0), W701Int(c, "previous", 0), false, 0L, false, false,
                BenchmarkWorkload.Sequential, W701Int(c, "warmup", 1), W701Int(c, "iterations", 30));
            JsonNode validate;
            try
            {
                BenchmarkProfileValidation.Validate(c["null_profile"] is { } ? null! : profile);
                validate = new JsonObject { ["ok"] = true };
            }
            catch (BenchmarkProfileException ex)
            {
                validate = new JsonObject
                {
                    ["ok"] = false,
                    ["problem"] = ex.Problem.ToString(),
                    ["profile_id"] = ex.ProfileId,
                    ["message"] = ex.Message,
                    ["is_argument_exception"] = ex is ArgumentException,
                };
            }
            catch (Exception ex)
            {
                validate = new JsonObject { ["ok"] = false, ["throws"] = ex.GetType().Name };
            }
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["id"] = W2Util.Str(c["id"]),
                ["loading_mode"] = c["loading_mode"]!.GetValue<int>(),
                ["representable"] = Enum.IsDefined(mode),
                ["target_width"] = profile.TargetWidth(),
                ["validate"] = validate,
            });
        }
        return result;
    }

    /// <summary>
    /// Cases {name, phase spec fields, with_samples?}: Count/P50/P95/P99/Max/EffectiveImagesPerSample/P95PerImage/P50PerImage
    /// ("stats"), what the registry derives ("derived_images"), and for with_samples the copy's stats plus the original's
    /// stats re-read AFTER the copy was made (the lazy sorted cache must not be shared).
    /// </summary>
    public static JsonArray BenchPhase(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var phase = W701Phase(c);
            var entry = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["derived_images"] = W701Derived(phase),
                ["stats"] = W701Stats(phase),
            };
            if (c["with_samples"] is { } replacement)
            {
                var copy = phase with { Samples = replacement.AsArray().Select(W701Num).ToArray() };
                entry["copy"] = W701Stats(copy);
                entry["original_after"] = W701Stats(phase);
            }
            result.Add(entry);
        }
        return result;
    }

    /// <summary>
    /// Cases {name, workload (name), phases:[phase spec]}: BenchmarkRanking.Rank. "ranked" = indexes into "phases" of the returned
    /// elements (the same instances come back), "derived" = the registry answer per phase (the Rust test builds its lookup from it).
    /// </summary>
    public static JsonArray BenchRank(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var phases = c["phases"]!.AsArray().Select(W701Phase).ToArray();
            var workload = Enum.Parse<BenchmarkWorkload>(c["workload"]!.GetValue<string>());
            var ranked = BenchmarkRanking.Rank(phases, workload);
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["workload"] = workload.ToString(),
                ["phases"] = c["phases"]!.DeepClone(),
                ["derived"] = W701DerivedList(phases),
                ["ranked"] = new JsonArray(ranked.Select(r => (JsonNode?)Array.FindIndex(phases, p => ReferenceEquals(p, r))).ToArray()),
            });
        }
        return result;
    }

    private static ReviewMetricsSnapshot? W701Metrics(string? kind) => kind switch
    {
        null => null,
        "zero" => new ReviewMetricsSnapshot(0, 0, 0, 0, 0, 0, 0),
        "populated" => new ReviewMetricsSnapshot(10, 2, 123456789, 12, 340, 8, 95)
        {
            PreloadHits = 5,
            InflightJoins = 1,
            DiskCacheHits = 3,
            UiAssignMilliseconds = 17,
            DecoderFallbacks = new Dictionary<DecoderBackend, long> { [DecoderBackend.Wpf] = 1, [DecoderBackend.TurboJpeg] = 3 },
            SourceOpenCount = 14,
            TopSourceOpens = [new SourceOpenEntry("C:\\a\\b.jpg", 3), new SourceOpenEntry("D:\\\u00e9.jpg", 1)],
            StatCount = 40,
            SessionWriteCount = 2,
            CrossThreadPresentCount = 0,
            PresentHistogram = [new PresentLatencyBucket("<=8", 5), new PresentLatencyBucket("<=16", 2), new PresentLatencyBucket(">500", 1)],
        },
        _ => throw new ArgumentException("unknown metrics kind " + kind),
    };

    /// <summary>Report spec: {run_id, utc_ticks, offset_minutes, folder, machine?, metrics? ("zero"|"populated"), phases:[phase spec]}.</summary>
    private static BenchmarkReport W701Report(JsonNode? node)
    {
        var o = node!.AsObject();
        var started = new DateTimeOffset(o["utc_ticks"]!.GetValue<long>(), TimeSpan.Zero)
            .ToOffset(TimeSpan.FromMinutes(o["offset_minutes"]!.GetValue<int>()));
        return new BenchmarkReport(
            o["run_id"]!.GetValue<string>(), started, o["folder"]!.GetValue<string>(),
            o["phases"]!.AsArray().Select(W701Phase).ToArray(),
            W701Metrics(W2Util.Str(o["metrics"])), W2Util.Str(o["machine"]));
    }

    /// <summary>
    /// Cases {name, kind:"report", report:{...}} or {name, kind:"batch", dataset:{folder, files, unsupported:[[ext,count],...], file_limit},
    /// reports:[report spec], outcomes:[phase spec]}: ToJson() text exactly as written on this (Windows) machine, or {"throws": type}.
    /// "derived" lists the registry answer of every phase in document order (reports' phases first, then outcomes).
    /// </summary>
    public static JsonArray BenchJson(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var phases = new List<BenchmarkPhaseResult>();
            Func<string> toJson;
            if (kind == "report")
            {
                var report = W701Report(c["report"]);
                phases.AddRange(report.Phases);
                toJson = report.ToJson;
            }
            else
            {
                var d = c["dataset"]!.AsObject();
                var unsupported = new Dictionary<string, int>();
                foreach (var pair in d["unsupported"]!.AsArray())
                    unsupported.Add(pair![0]!.GetValue<string>(), pair[1]!.GetValue<int>());
                var dataset = new BenchmarkDatasetManifest(d["folder"]!.GetValue<string>(), W2Util.Strings(d["files"]), unsupported, d["file_limit"]!.GetValue<int>());
                var reports = c["reports"]!.AsArray().Select(W701Report).ToArray();
                var outcomes = c["outcomes"]!.AsArray().Select(W701Phase).ToArray();
                foreach (var r in reports) phases.AddRange(r.Phases);
                phases.AddRange(outcomes);
                var summary = new BenchmarkBatchSummary(dataset, reports, outcomes);
                toJson = summary.ToJson;
            }
            var entry = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["kind"] = kind,
                ["derived"] = W701DerivedList(phases),
            };
            try { entry["json"] = toJson(); }
            catch (Exception ex) { entry["throws"] = ex.GetType().Name; }
            result.Add(entry);
        }
        return result;
    }
}
