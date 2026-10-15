using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Preload;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-15b
//     "preload-scheduler-script" => Commands.PreloadSchedulerScript(inputs),
// WIRING: needs the PhotoReview.Imaging.dll reference (see the W4-05a header). The test seams of PreloadScheduler are
// `internal` (ResumedRunForTests, ResumesForTests, PreloadedKeyCountForTests, TrackedLifetimeCounts); the C# repo is
// never edited, so they are read by REFLECTION.
//
// Determinism (D19/D27): virtual paths (no file is touched: every key comes from the entry's metadata), a SYNCHRONOUS
// fake target (PreloadAsync completes or throws before returning, so every worker finishes inline and the run is one
// sequential flow), a NavigationPace whose fake Stopwatch clock jumps 2 s per read (never a burst: lead is always 0),
// a manual TimeProvider (memory re-checks fire only on a `fire_timer` op), no CachedPreviewBytes (the size calibration
// never flips, so the order is rebuilt only on a kick). The log is enabled; `Preload progress:` lines are DROPPED from
// the output (their `active=` count is 0 here because the target is synchronous, and differs by design in the threaded
// Rust port; contract W4-15b, golden package comparison rules). Every other line is kept, in order.

// W4-15b: PreloadScheduler driven by an operation script.
static partial class Commands
{
    private const BindingFlags W415bInstance = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;
    private static readonly DateTime W415bWritten = new(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc);

    private sealed class W415bLog : ILog
    {
        public List<string> Lines { get; } = [];
        public bool Enabled => true;
        public void Info(string message) { lock (Lines) if (!message.StartsWith("Preload progress:", StringComparison.Ordinal)) Lines.Add("info: " + message); }
        public void Warn(string message) { lock (Lines) Lines.Add("warn: " + message); }
        public void Error(string message, Exception? ex = null) { lock (Lines) Lines.Add("error: " + message); }
    }

    private sealed class W415bProbe : IMemoryProbe
    {
        public double Load { get; set; } = 0.5;
        public bool HasHeadroom(double maximumLoad, long reserveBytes) => Load < maximumLoad;
        public MemorySnapshot? GetSnapshot() => new((uint)(Load * 100), 16UL * 1024 * 1024 * 1024);
    }

    private sealed class W415bTime : TimeProvider
    {
        public List<W415bTimer> Timers { get; } = [];
        public W415bTimer? Pending { get { lock (Timers) return Timers.LastOrDefault(t => !t.Disposed); } }
        public override ITimer CreateTimer(TimerCallback callback, object? state, TimeSpan dueTime, TimeSpan period)
        {
            var timer = new W415bTimer(callback, state, dueTime);
            lock (Timers) Timers.Add(timer);
            return timer;
        }
    }

    private sealed class W415bTimer(TimerCallback callback, object? state, TimeSpan due) : ITimer
    {
        public TimeSpan Due { get; private set; } = due;
        public bool Disposed { get; private set; }
        public void Fire() => callback(state);
        public bool Change(TimeSpan dueTime, TimeSpan period) { Due = dueTime; return !Disposed; }
        public void Dispose() => Disposed = true;
        public ValueTask DisposeAsync() { Dispose(); return ValueTask.CompletedTask; }
    }

    private sealed class W415bTarget(CatalogEntry[] entries, DecodeBox box) : IPreloadTarget
    {
        private readonly Dictionary<string, int> _index = entries
            .Select((e, i) => (e.Path, i)).ToDictionary(t => t.Path, t => t.i, StringComparer.OrdinalIgnoreCase);
        public HashSet<string> Cached { get; } = new(StringComparer.Ordinal); // key paths (upper-invariant)
        public HashSet<string> DiskCached { get; } = new(StringComparer.Ordinal);
        public Dictionary<int, string> Failures { get; } = [];
        public Dictionary<int, string> KeyFailures { get; } = [];
        public List<int> Examined { get; } = [];
        public List<int> Preloads { get; } = [];
        public long Bytes { get; set; }
        public int Viewer { get; set; }

        public string KeyPath(int i) => ImageCacheKey.Create(entries[i], false, box).Path;
        public bool TryGetCachedPreview(string path) => Cached.Contains(GetCurrentCacheKey(path).Path);
        public bool TryGetCachedPreview(ImageCacheKey key) => Cached.Contains(key.Path);
        public ImageCacheKey GetCurrentCacheKey(string path) => ImageCacheKey.Create(entries[_index[path]], false, box);

        public ImageCacheKey GetCurrentCacheKey(CatalogEntry entry)
        {
            var i = _index[entry.Path];
            lock (Examined) Examined.Add(i);
            if (KeyFailures.TryGetValue(i, out var kind))
            {
                throw kind switch
                {
                    "io" => new FileNotFoundException("gone", entry.Path),
                    "access" => new UnauthorizedAccessException("denied"),
                    _ => new ArgumentException("bad path"),
                };
            }
            return ImageCacheKey.Create(entry, false, box);
        }

        public Task PreloadAsync(string path, CancellationToken cancellationToken = default)
        {
            var i = _index[path];
            lock (Preloads) Preloads.Add(i);
            if (Failures.TryGetValue(i, out var kind))
            {
                switch (kind)
                {
                    case "io": throw new IOException("read failed");
                    case "access": throw new UnauthorizedAccessException("denied");
                    case "busy": throw new DecoderBusyException();
                    case "missing_codec": throw new MissingImageCodecException();
                    case "invalid_data": throw new InvalidDataException("corrupt");
                    case "foreign_cancel": throw new OperationCanceledException();
                    case "not_cached": return Task.CompletedTask;
                }
            }
            lock (Cached) Cached.Add(KeyPath(i));
            return Task.CompletedTask;
        }

        public int CacheCount => Cached.Count;
        public long CacheBytes => Bytes;
        public int ActiveViewerDecodes => Viewer;
        public bool HasDiskCachedPreview(ImageCacheKey key) => DiskCached.Contains(key.Path);
    }

    private static JsonArray W415bInts(List<int> list, ref int seen)
    {
        var result = new JsonArray();
        lock (list)
        {
            for (var i = seen; i < list.Count; i++) result.Add(list[i]);
            seen = list.Count;
        }
        return result;
    }

    /// <summary>
    /// Per case: {name, count, workers, forward, backward, threshold, memory_load_limit?, load?, cache_bytes?, viewer?,
    /// box?:[w,h], raw_indices?:[i], cached?:[i], disk_cached?:[i], prefetch?:bool, failures?:{"i":kind},
    /// key_failures?:{"i":kind}, ops:[...]}. Entry i is `C:\w415b\img-<i:D2>.jpg` (`.cr2` for raw_indices), length 1000+i,
    /// written 2026-01-01T00:00:00Z. Ops: preload_around{center}, notify_navigation{index}, cancel, dispose,
    /// set_load{value}, set_cache_bytes{value}, set_viewer{value}, set_failure{index, kind|null}, fire_timer, evict{index},
    /// consume_key{index}, remove_keys_for_path{index}, clear_preloaded_keys.
    /// Result per op: {op, examined, preloads, prefetches, log, timers (due seconds of timers armed during the op),
    /// fired?, consumed?, resumes, preloaded_keys, idle, tracked:[lifetimes, tasks]}.
    /// </summary>
    public static JsonArray PreloadSchedulerScript(JsonObject inputs)
    {
        var guard = TimeSpan.FromSeconds(10);
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var count = c["count"]!.GetValue<int>();
            var raw = c["raw_indices"]?.AsArray().Select(n => n!.GetValue<int>()).ToHashSet() ?? new HashSet<int>();
            var entries = Enumerable.Range(0, count)
                .Select(i => new CatalogEntry($@"C:\w415b\img-{i:D2}{(raw.Contains(i) ? ".cr2" : ".jpg")}").WithMetadata(1000 + i, W415bWritten))
                .ToArray();
            var boxNode = c["box"]?.AsArray();
            var box = boxNode is null ? new DecodeBox(100, 100) : new DecodeBox(boxNode[0]!.GetValue<int>(), boxNode[1]!.GetValue<int>());
            var target = new W415bTarget(entries, box)
            {
                Bytes = c["cache_bytes"]?.GetValue<long>() ?? 0,
                Viewer = c["viewer"]?.GetValue<int>() ?? 0,
            };
            foreach (var i in c["cached"]?.AsArray() ?? new JsonArray()) target.Cached.Add(target.KeyPath(i!.GetValue<int>()));
            foreach (var i in c["disk_cached"]?.AsArray() ?? new JsonArray()) target.DiskCached.Add(target.KeyPath(i!.GetValue<int>()));
            foreach (var (k, v) in c["failures"]?.AsObject() ?? new JsonObject()) target.Failures[int.Parse(k)] = v!.GetValue<string>();
            foreach (var (k, v) in c["key_failures"]?.AsObject() ?? new JsonObject()) target.KeyFailures[int.Parse(k)] = v!.GetValue<string>();

            var prefetches = new List<int>();
            Func<string, CancellationToken, Task>? prefetch = c["prefetch"]?.GetValue<bool>() == true
                ? (path, _) =>
                {
                    var i = Array.FindIndex(entries, e => string.Equals(e.Path, path, StringComparison.OrdinalIgnoreCase));
                    lock (prefetches) prefetches.Add(i);
                    return Task.CompletedTask;
                }
                : null;
            var probe = new W415bProbe { Load = c["load"]?.GetValue<double>() ?? 0.5 };
            var time = new W415bTime();
            var log = new W415bLog();
            long clock = 1_000_000;
            var pace = new NavigationPace(() => clock += 2 * Stopwatch.Frequency);
            var options = new PreloadOptions(
                WorkerCount: c["workers"]!.GetValue<int>(),
                MemoryLoadLimit: c["memory_load_limit"]?.GetValue<double>() ?? 0.80,
                FullFolderThresholdBytes: c["threshold"]!.GetValue<long>())
            { Window = new PreloadWindow(c["forward"]!.GetValue<int>(), c["backward"]!.GetValue<int>()) };
            var scheduler = new PreloadScheduler(target, new ReviewMetrics(), () => entries, options: options,
                memoryProbe: probe, log: log, prefetchSourceBytes: prefetch, pace: pace, timeProvider: time);
            var type = typeof(PreloadScheduler);
            var resumedRun = type.GetProperty("ResumedRunForTests", W415bInstance)!;
            var resumes = type.GetProperty("ResumesForTests", W415bInstance)!;
            var keyCount = type.GetProperty("PreloadedKeyCountForTests", W415bInstance)!;
            var tracked = type.GetProperty("TrackedLifetimeCounts", W415bInstance)!;

            int seenExamined = 0, seenPreloads = 0, seenPrefetches = 0, seenLog = 0, seenTimers = 0;
            var opResults = new JsonArray();
            try
            {
                foreach (var opNode in c["ops"]!.AsArray())
                {
                    var op = opNode!.AsObject();
                    var name = op["op"]!.GetValue<string>();
                    var opResult = new JsonObject { ["op"] = name };
                    switch (name)
                    {
                        case "preload_around":
                            if (!scheduler.PreloadAroundAsync(op["center"]!.GetValue<int>()).Wait(guard)) throw new TimeoutException(c["name"] + ": run did not finish");
                            break;
                        case "notify_navigation": scheduler.NotifyNavigation(op["index"]!.GetValue<int>()); break;
                        case "cancel": scheduler.Cancel(); break;
                        case "dispose": scheduler.Dispose(); break;
                        case "set_load": probe.Load = op["value"]!.GetValue<double>(); break;
                        case "set_cache_bytes": target.Bytes = op["value"]!.GetValue<long>(); break;
                        case "set_viewer": target.Viewer = op["value"]!.GetValue<int>(); break;
                        case "set_failure":
                        {
                            var i = op["index"]!.GetValue<int>();
                            if (op["kind"] is { } kind) target.Failures[i] = kind.GetValue<string>(); else target.Failures.Remove(i);
                            break;
                        }
                        case "fire_timer":
                        {
                            var pending = time.Pending;
                            opResult["fired"] = pending is not null;
                            var before = (Task?)resumedRun.GetValue(scheduler);
                            pending?.Fire();
                            var after = (Task?)resumedRun.GetValue(scheduler);
                            if (after is not null && !ReferenceEquals(after, before) && !after.Wait(guard))
                                throw new TimeoutException(c["name"] + ": resumed run did not finish");
                            break;
                        }
                        case "evict": target.Cached.Remove(target.KeyPath(op["index"]!.GetValue<int>())); break;
                        case "consume_key":
                            opResult["consumed"] = scheduler.TryConsumePreloadedKey(ImageCacheKey.Create(entries[op["index"]!.GetValue<int>()], false, box));
                            break;
                        case "remove_keys_for_path": scheduler.RemovePreloadedKeysForPath(target.KeyPath(op["index"]!.GetValue<int>())); break;
                        case "clear_preloaded_keys": scheduler.ClearPreloadedKeys(); break;
                        default: throw new InvalidOperationException($"unknown op {name}");
                    }
                    opResult["examined"] = W415bInts(target.Examined, ref seenExamined);
                    opResult["preloads"] = W415bInts(target.Preloads, ref seenPreloads);
                    opResult["prefetches"] = W415bInts(prefetches, ref seenPrefetches);
                    var lines = new JsonArray();
                    lock (log.Lines) { for (var i = seenLog; i < log.Lines.Count; i++) lines.Add(log.Lines[i]); seenLog = log.Lines.Count; }
                    opResult["log"] = lines;
                    var timers = new JsonArray();
                    lock (time.Timers) { for (var i = seenTimers; i < time.Timers.Count; i++) timers.Add(time.Timers[i].Due.TotalSeconds); seenTimers = time.Timers.Count; }
                    opResult["timers"] = timers;
                    opResult["resumes"] = (int)resumes.GetValue(scheduler)!;
                    opResult["preloaded_keys"] = (int)keyCount.GetValue(scheduler)!;
                    opResult["idle"] = scheduler.IsIdle;
                    var (lifetimes, tasks) = ((int, int))tracked.GetValue(scheduler)!;
                    opResult["tracked"] = new JsonArray(lifetimes, tasks);
                    opResults.Add(opResult);
                }
            }
            finally
            {
                scheduler.Dispose();
            }
            var outCase = JsonNode.Parse(c.ToJsonString())!.AsObject();
            outCase["results"] = opResults;
            result.Add(outCase);
        }
        return result;
    }
}
