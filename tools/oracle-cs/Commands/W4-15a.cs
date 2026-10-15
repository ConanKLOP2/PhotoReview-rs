using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Preload;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-15a
//     "preload-policy" => Commands.PreloadPolicy(inputs),
// WIRING: needs the PhotoReview.Imaging.dll reference (see the W4-05a header). PreloadScheduler's pure members are
// `internal static` (ParseDiagWorkers, InPreloadWindow, ResumeCheckDelay) or `private` (IsStillWanted, CurrentShape,
// EvaluateResume, the consts); the C# repo is never edited, so this file reaches them by REFLECTION
// (BindingFlags.NonPublic). The four `*-log`/`f0`/`fill-limit` sections evaluate the C# interpolated strings and the
// comparison COPIED VERBATIM from PreloadScheduler.cs (line numbers below): what they pin is .NET's formatting
// ("F0" rounding, bool => True/False, null => empty) and double comparison, which the Rust port must reproduce.
// Deterministic (D19/D27): no I/O, no wall clock (NavigationPace gets a fake Stopwatch clock), invariant culture.

// W4-15a: pure rules and log texts of PreloadScheduler.
static partial class Commands
{
    private const BindingFlags W415aStatic = BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic;
    private const BindingFlags W415aInstance = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;

    private sealed class W415aTarget : IPreloadTarget
    {
        public long Bytes { get; set; }
        public int Viewer { get; set; }
        public int CacheBytesReads { get; private set; }
        public bool TryGetCachedPreview(string path) => false;
        public bool TryGetCachedPreview(ImageCacheKey key) => false;
        public Task PreloadAsync(string path, CancellationToken cancellationToken = default) => Task.CompletedTask;
        public ImageCacheKey GetCurrentCacheKey(string path) => throw new InvalidOperationException("not used");
        public ImageCacheKey GetCurrentCacheKey(CatalogEntry entry) => throw new InvalidOperationException("not used");
        public int CacheCount => 0;
        public long CacheBytes { get { CacheBytesReads++; return Bytes; } }
        public int ActiveViewerDecodes => Viewer;
    }

    private sealed class W415aProbe : IMemoryProbe
    {
        public double Load { get; set; }
        public List<(double Max, long Reserve)> Calls { get; } = [];
        public bool HasHeadroom(double maximumLoad, long reserveBytes)
        {
            Calls.Add((maximumLoad, reserveBytes));
            return Load < maximumLoad;
        }
        public MemorySnapshot? GetSnapshot() => new(50, 16UL * 1024 * 1024 * 1024);
    }

    private static object W415aConst(string name) =>
        typeof(PreloadScheduler).GetField(name, W415aStatic)!.GetValue(null)!;

    private static JsonNode? W415aNum(double value) =>
        double.IsFinite(value) ? JsonValue.Create(value) : JsonValue.Create(value.ToString("R", CultureInfo.InvariantCulture));

    /// <summary>
    /// Input {cases:[{section, name, ...}]}. Sections and their fields:
    ///   parse-diag-workers {value: string|null} -> {result: int|null}
    ///   in-preload-window {index, center, direction, lead, forward, backward} -> {result}
    ///   is-still-wanted {center, index, records:[int], step_ms, decode_ms|null} -> {direction, lead, result}
    ///   resume-check-delay {check} -> {ticks, seconds}
    ///   resume-verdict {threshold, cache_bytes, viewer, load, memory_load_limit} -> {verdict, probe_calls:[{max, reserve}], cache_bytes_reads}
    ///   fill-limit {cache_bytes, threshold} -> {result}
    ///   f0 {value} -> {text}
    ///   policy-log {images, box_width, box_height, measured|null, estimated, capacity, whole_folder, center, direction, lead} -> {text}
    ///   paused-log / progress-log {queued, active?, cache_count, cache_bytes, load_percent|null, available_bytes|null} -> {text}
    ///   constants {} -> {values:{...}}
    /// </summary>
    public static JsonArray PreloadPolicy(JsonObject inputs)
    {
        var previous = CultureInfo.CurrentCulture;
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        try
        {
            var result = new JsonArray();
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var outCase = JsonNode.Parse(c.ToJsonString())!.AsObject();
                switch (c["section"]!.GetValue<string>())
                {
                    case "parse-diag-workers":
                    {
                        var parse = typeof(PreloadScheduler).GetMethod("ParseDiagWorkers", W415aStatic)!;
                        var value = c["value"] is null ? null : c["value"]!.GetValue<string>();
                        var parsed = (int?)parse.Invoke(null, [value]);
                        outCase["result"] = parsed is { } p ? JsonValue.Create(p) : null;
                        break;
                    }
                    case "in-preload-window":
                    {
                        var method = typeof(PreloadScheduler).GetMethod("InPreloadWindow", W415aStatic)!;
                        var shape = (c["direction"]!.GetValue<int>(), c["lead"]!.GetValue<int>());
                        var window = new PreloadWindow(c["forward"]!.GetValue<int>(), c["backward"]!.GetValue<int>());
                        outCase["result"] = (bool)method.Invoke(null,
                            [c["index"]!.GetValue<int>(), c["center"]!.GetValue<int>(), shape, window])!;
                        break;
                    }
                    case "is-still-wanted":
                    {
                        // A fake Stopwatch clock: each record advances it by step_ms (Stopwatch.Frequency units).
                        long ticks = 1_000_000;
                        var pace = new NavigationPace(() => ticks);
                        var stepTicks = (long)Math.Round(c["step_ms"]!.GetValue<double>() * System.Diagnostics.Stopwatch.Frequency / 1000.0);
                        foreach (var r in c["records"]!.AsArray())
                        {
                            ticks += stepTicks;
                            pace.Record(r!.GetValue<int>());
                        }
                        var metrics = new ReviewMetrics();
                        if (c["decode_ms"] is { } decodeMs) metrics.RecordSourceRead(1, decodeMs.GetValue<long>(), includeInDecodeEwma: true);
                        using var scheduler = new PreloadScheduler(new W415aTarget(), metrics, () => [],
                            options: new PreloadOptions(WorkerCount: 1), memoryProbe: new W415aProbe(), pace: pace);
                        typeof(PreloadScheduler).GetField("_preloadCenter", W415aInstance)!.SetValue(scheduler, c["center"]!.GetValue<int>());
                        var shape = ((int, int))typeof(PreloadScheduler).GetMethod("CurrentShape", W415aInstance)!.Invoke(scheduler, null)!;
                        outCase["direction"] = shape.Item1;
                        outCase["lead"] = shape.Item2;
                        outCase["result"] = (bool)typeof(PreloadScheduler).GetMethod("IsStillWanted", W415aInstance)!
                            .Invoke(scheduler, [c["index"]!.GetValue<int>()])!;
                        break;
                    }
                    case "resume-check-delay":
                    {
                        var delay = (TimeSpan)typeof(PreloadScheduler).GetMethod("ResumeCheckDelay", W415aStatic)!
                            .Invoke(null, [c["check"]!.GetValue<int>()])!;
                        outCase["ticks"] = delay.Ticks;
                        outCase["seconds"] = delay.TotalSeconds;
                        break;
                    }
                    case "resume-verdict":
                    {
                        var target = new W415aTarget { Bytes = c["cache_bytes"]!.GetValue<long>(), Viewer = c["viewer"]!.GetValue<int>() };
                        var probe = new W415aProbe { Load = c["load"]!.GetValue<double>() };
                        var options = new PreloadOptions(WorkerCount: 1, MemoryLoadLimit: c["memory_load_limit"]!.GetValue<double>(),
                            FullFolderThresholdBytes: c["threshold"]!.GetValue<long>());
                        using var scheduler = new PreloadScheduler(target, new ReviewMetrics(), () => [], options: options, memoryProbe: probe);
                        var verdict = typeof(PreloadScheduler).GetMethod("EvaluateResume", W415aInstance)!.Invoke(scheduler, null)!;
                        outCase["verdict"] = verdict.ToString();
                        var limits = new JsonArray();
                        foreach (var (max, reserve) in probe.Calls)
                            limits.Add(new JsonObject { ["max"] = W415aNum(max), ["reserve"] = reserve });
                        outCase["probe_calls"] = limits;
                        outCase["cache_bytes_reads"] = target.CacheBytesReads;
                        break;
                    }
                    case "fill-limit":
                    {
                        long cacheBytes = c["cache_bytes"]!.GetValue<long>(), threshold = c["threshold"]!.GetValue<long>();
                        // PreloadScheduler.cs lines 598 and 420, verbatim:
                        outCase["result"] = cacheBytes >= threshold * PreloadScheduler.WholeFolderCacheFillLimit;
                        break;
                    }
                    case "f0":
                    {
                        double? measured = c["value"]!.GetValue<double>();
                        // PreloadScheduler.cs line 525 (the measured part), verbatim:
                        outCase["text"] = measured?.ToString("F0", CultureInfo.InvariantCulture) ?? "none";
                        break;
                    }
                    case "policy-log":
                    {
                        var images = c["images"]!.GetValue<int>();
                        var box = new DecodeBox(c["box_width"]!.GetValue<int>(), c["box_height"]!.GetValue<int>());
                        double? measured = c["measured"] is { } m ? m.GetValue<double>() : null;
                        var estimated = c["estimated"]!.GetValue<long>();
                        var capacity = c["capacity"]!.GetValue<long>();
                        var wholeFolder = c["whole_folder"]!.GetValue<bool>();
                        var center = c["center"]!.GetValue<int>();
                        var shape = (Direction: c["direction"]!.GetValue<int>(), Lead: c["lead"]!.GetValue<int>());
                        // PreloadScheduler.cs line 525, verbatim (entries.Length -> images, _options.FullFolderThresholdBytes -> capacity):
                        outCase["text"] = $"Preload policy: images={images} box={box.Width}x{box.Height} measuredMeanBytes={measured?.ToString("F0", CultureInfo.InvariantCulture) ?? "none"} estimatedBytes={estimated} capacityBytes={capacity} wholeFolder={wholeFolder} center={center} direction={shape.Direction} lead={shape.Lead}";
                        break;
                    }
                    case "paused-log":
                    case "progress-log":
                    {
                        var queuedCount = c["queued"]!.GetValue<int>();
                        var active = c["active"]?.GetValue<int>() ?? 0;
                        var cacheCount = c["cache_count"]!.GetValue<int>();
                        var cacheBytes = c["cache_bytes"]!.GetValue<long>();
                        MemorySnapshot? memory = c["load_percent"] is { } lp && c["available_bytes"] is { } ab
                            ? new MemorySnapshot(lp.GetValue<uint>(), ab.GetValue<ulong>())
                            : null;
                        // PreloadScheduler.cs lines 582 and 634, verbatim (queued.Count -> queuedCount, running.Count -> active,
                        // _target.CacheCount/CacheBytes -> cacheCount/cacheBytes):
                        outCase["text"] = c["section"]!.GetValue<string>() == "paused-log"
                            ? $"Preload paused for memory: queued={queuedCount} cacheCount={cacheCount} cacheBytes={cacheBytes} availableBytes={memory?.AvailableBytes} loadPercent={memory?.LoadPercent}"
                            : $"Preload progress: queued={queuedCount} active={active} cacheCount={cacheCount} cacheBytes={cacheBytes} availableBytes={memory?.AvailableBytes}";
                        break;
                    }
                    case "constants":
                    {
                        using var scheduler = new PreloadScheduler(new W415aTarget(), new ReviewMetrics(), () => [],
                            options: new PreloadOptions(WorkerCount: 1), memoryProbe: new W415aProbe());
                        var drain = (TimeSpan)typeof(PreloadScheduler).GetProperty("DisposeDrainTimeout", W415aInstance)!.GetValue(scheduler)!;
                        outCase["values"] = new JsonObject
                        {
                            ["WholeFolderCacheFillLimit"] = PreloadScheduler.WholeFolderCacheFillLimit,
                            ["SlowLinkDecodeMsThreshold"] = (double)W415aConst("SlowLinkDecodeMsThreshold"),
                            ["SlowLinkViewerBusyWorkerLimit"] = (int)W415aConst("SlowLinkViewerBusyWorkerLimit"),
                            ["ResumeCheckBaseDelayTicks"] = ((TimeSpan)W415aConst("ResumeCheckBaseDelay")).Ticks,
                            ["ResumeCheckMaxDelayTicks"] = ((TimeSpan)W415aConst("ResumeCheckMaxDelay")).Ticks,
                            ["MaxResumeChecks"] = (int)W415aConst("MaxResumeChecks"),
                            ["ResumeLoadMargin"] = (double)W415aConst("ResumeLoadMargin"),
                            ["HeadroomRecheckMs"] = (long)W415aConst("HeadroomRecheckMs"),
                            ["DisposeDrainTimeoutTicks"] = drain.Ticks,
                        };
                        break;
                    }
                    default:
                        throw new InvalidOperationException($"unknown section {c["section"]}");
                }
                result.Add(outCase);
            }
            return result;
        }
        finally
        {
            CultureInfo.CurrentCulture = previous;
        }
    }
}
