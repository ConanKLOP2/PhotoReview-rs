using System.Globalization;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Caching;
using PhotoReview.Imaging.Decoding;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-09
//     "disk-cache-prune" => Commands.DiskCachePrune(inputs),
//     "disk-cache-passes" => Commands.DiskCachePasses(inputs),
//     "disk-cache-note-accessed" => Commands.DiskCacheNoteAccessed(inputs),
//     "disk-cache-stale-temp" => Commands.DiskCacheStaleTemp(inputs),
//     "disk-cache-delete" => Commands.DiskCacheDelete(inputs),
//     "cache-entry-failure" => Commands.CacheEntryFailure(inputs),
// Wiring notes (the wiring step does these; this file runs nothing by itself):
//   * DiskCacheStore lives in PhotoReview.Imaging.dll: same build/reference addition as W4-01a (Imaging only references Core).
//   * Internal/private members are reached by reflection (FullScanCount, RunPrunePass, IsCacheEntryFailure), exactly like the C#
//     DiskCacheStoreMutationTests do; no InternalsVisibleTo is needed.
//   * Every case runs in its own fresh folder under %TEMP%; all times are absolute (set explicitly), never "now", so the output does
//     not depend on the clock (D19). Paths in log messages are reported with the case folder replaced by "<ROOT>".
//   * Uses W2Util.Str from W2-00.cs (already wired).

// W4-09: DiskCacheStore (PruneDirectory, RunPrunePass accounting, NoteAccessed throttle, DeleteStaleTempFiles, TryDelete/ClearDirectory,
// IsCacheEntryFailure).
static partial class Commands
{
    private static readonly DateTime W409DefaultTime = new(2020, 1, 1, 0, 0, 0, DateTimeKind.Utc);

    private static readonly PropertyInfo W409FullScanCount =
        typeof(DiskCacheStore).GetProperty("FullScanCount", BindingFlags.Instance | BindingFlags.NonPublic)
        ?? throw new InvalidOperationException("DiskCacheStore.FullScanCount not found");

    private static readonly MethodInfo W409RunPrunePass =
        typeof(DiskCacheStore).GetMethod("RunPrunePass", BindingFlags.Instance | BindingFlags.NonPublic)
        ?? throw new InvalidOperationException("DiskCacheStore.RunPrunePass not found");

    private static readonly MethodInfo W409IsCacheEntryFailure =
        typeof(DiskCacheStore).GetMethod("IsCacheEntryFailure", BindingFlags.Static | BindingFlags.NonPublic)
        ?? throw new InvalidOperationException("DiskCacheStore.IsCacheEntryFailure not found");

    private sealed class W409Log(string root) : ILog
    {
        private readonly List<(string Message, string? Type)> _errors = [];
        public bool Enabled => true;
        public void Info(string message) { }
        public void Warn(string message) { }
        public void Error(string message, Exception? ex = null)
        {
            lock (_errors) _errors.Add((message.Replace(root, "<ROOT>", StringComparison.Ordinal), ex?.GetType().Name));
        }
        public JsonArray ToJson()
        {
            lock (_errors)
            {
                return new JsonArray(_errors.Select(e => (JsonNode?)new JsonObject { ["message"] = e.Message, ["exception"] = e.Type }).ToArray());
            }
        }
    }

    private static DateTime W409Time(JsonNode? node) =>
        node is null
            ? W409DefaultTime
            : DateTime.Parse(node.GetValue<string>(), CultureInfo.InvariantCulture, DateTimeStyles.AdjustToUniversal | DateTimeStyles.AssumeUniversal);

    private static string W409NewRoot()
    {
        var root = Path.Combine(Path.GetTempPath(), "pr-oracle-w409-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        return root;
    }

    private static void W409RemoveRoot(string root)
    {
        if (!Directory.Exists(root)) return;
        foreach (var f in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories)) File.SetAttributes(f, FileAttributes.Normal);
        Directory.Delete(root, recursive: true);
    }

    /// <summary>
    /// Creates one entry of a "files" list: {name, size?, access?, creation?, write?, dir?, readonly?}. Times default to 2020-01-01Z.
    /// Order: content, creation, last write, last access (so the access stamp is the last metadata write).
    /// </summary>
    private static void W409Create(string root, JsonObject spec)
    {
        var path = Path.Combine(root, spec["name"]!.GetValue<string>());
        if (spec["dir"]?.GetValue<bool>() == true)
        {
            Directory.CreateDirectory(path);
            return;
        }
        if (File.Exists(path)) File.SetAttributes(path, FileAttributes.Normal);
        File.WriteAllBytes(path, new byte[spec["size"]?.GetValue<int>() ?? 0]);
        File.SetCreationTimeUtc(path, W409Time(spec["creation"]));
        File.SetLastWriteTimeUtc(path, W409Time(spec["write"]));
        File.SetLastAccessTimeUtc(path, W409Time(spec["access"]));
        if (spec["readonly"]?.GetValue<bool>() == true) File.SetAttributes(path, FileAttributes.ReadOnly);
    }

    /// <summary>Opens every file whose spec has "locked": true with FileShare.None (they stay open until disposed).</summary>
    private static List<FileStream> W409Lock(string root, JsonArray files) =>
        files.Select(f => f!.AsObject())
            .Where(o => o["locked"]?.GetValue<bool>() == true)
            .Select(o => new FileStream(Path.Combine(root, o["name"]!.GetValue<string>()), FileMode.Open, FileAccess.Read, FileShare.None))
            .ToList();

    /// <summary>File names (not directories) directly in <paramref name="root"/>, ordinal order.</summary>
    private static JsonArray W409Survivors(string root)
    {
        if (!Directory.Exists(root)) return [];
        var names = Directory.GetFiles(root).Select(Path.GetFileName).Order(StringComparer.Ordinal).ToArray();
        return new JsonArray(names.Select(n => (JsonNode?)JsonValue.Create(n)).ToArray());
    }

    private static JsonObject W409Throws(Exception ex) => new() { ["throws"] = ex.GetType().Name };

    /// <summary>
    /// DiskCacheStore.PruneDirectory for each case {name, pattern, max_bytes, files:[...], log?: bool (default true),
    /// target?: "missing" | "file" (prune a missing folder / a path that is a file)}. Result: "remaining" (or "throws"), survivors, log.
    /// </summary>
    public static JsonArray DiskCachePrune(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = W409NewRoot();
            try
            {
                var files = c["files"]?.AsArray() ?? [];
                foreach (var f in files) W409Create(root, f!.AsObject());
                var target = W2Util.Str(c["target"]) switch
                {
                    null => root,
                    "missing" => Path.Combine(root, "does-not-exist"),
                    "file" => Path.Combine(root, files[0]!["name"]!.GetValue<string>()),
                    var other => throw new ArgumentException("unknown target " + other),
                };
                var log = new W409Log(root);
                var useLog = c["log"]?.GetValue<bool>() ?? true;
                JsonNode outcome;
                var locks = W409Lock(root, files);
                try
                {
                    try
                    {
                        outcome = JsonValue.Create(DiskCacheStore.PruneDirectory(target, c["pattern"]!.GetValue<string>(),
                            c["max_bytes"]!.GetValue<long>(), useLog ? log : null));
                    }
                    catch (Exception ex) { outcome = W409Throws(ex); }
                }
                finally { foreach (var l in locks) l.Dispose(); }
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["input"] = c.DeepClone(),
                    ["remaining"] = outcome,
                    ["target_exists_after"] = Directory.Exists(target) || File.Exists(target),
                    ["survivors"] = W409Survivors(root),
                    ["log"] = log.ToJson(),
                });
            }
            finally { W409RemoveRoot(root); }
        }
        return result;
    }

    /// <summary>
    /// An instance DiskCacheStore driven step by step, passes run synchronously through the private RunPrunePass. Case
    /// {name, pattern, max_bytes, dir_missing?: bool, steps:[...]}; steps: {op:"file", name, size, access?, creation?} (create or
    /// overwrite), {op:"delete", name} (external delete), {op:"note_written", name}, {op:"pass"}, {op:"pass_n", count},
    /// {op:"clear"} (instance ClearDirectory), {op:"lock", name}, {op:"unlock", name}. After pass/pass_n/clear a trace row
    /// {step, op, full_scan_count, survivors, throws?} is recorded.
    /// </summary>
    public static JsonArray DiskCachePasses(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = W409NewRoot();
            var locks = new Dictionary<string, FileStream>(StringComparer.Ordinal);
            try
            {
                var dir = c["dir_missing"]?.GetValue<bool>() == true ? Path.Combine(root, "not-there") : root;
                var log = new W409Log(root);
                var store = new DiskCacheStore(dir, c["pattern"]!.GetValue<string>(), c["max_bytes"]!.GetValue<long>(), log);
                var trace = new JsonArray();
                var steps = c["steps"]!.AsArray();
                for (var i = 0; i < steps.Count; i++)
                {
                    var s = steps[i]!.AsObject();
                    var op = s["op"]!.GetValue<string>();
                    string? thrown = null;
                    string P() => Path.Combine(root, s["name"]!.GetValue<string>());
                    switch (op)
                    {
                        case "file": W409Create(root, s); break;
                        case "delete": File.Delete(P()); break;
                        case "note_written": store.NoteWritten(P()); break;
                        case "lock": locks[P()] = new FileStream(P(), FileMode.Open, FileAccess.Read, FileShare.None); break;
                        case "unlock": locks.Remove(P(), out var fs); fs!.Dispose(); break;
                        case "pass":
                        case "pass_n":
                            var count = op == "pass" ? 1 : s["count"]!.GetValue<int>();
                            for (var k = 0; k < count; k++)
                            {
                                try { W409RunPrunePass.Invoke(store, null); }
                                catch (TargetInvocationException tie) { thrown = tie.InnerException!.GetType().Name; }
                            }
                            break;
                        case "clear":
                            try { store.ClearDirectory(); }
                            catch (Exception ex) { thrown = ex.GetType().Name; }
                            break;
                        default: throw new ArgumentException("unknown op " + op);
                    }
                    if (op is "pass" or "pass_n" or "clear")
                    {
                        var row = new JsonObject
                        {
                            ["step"] = i,
                            ["op"] = op,
                            ["full_scan_count"] = (int)W409FullScanCount.GetValue(store)!,
                            ["survivors"] = W409Survivors(root),
                        };
                        if (thrown is not null) row["throws"] = thrown;
                        trace.Add(row);
                    }
                }
                foreach (var l in locks.Values) l.Dispose();
                locks.Clear();
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["input"] = c.DeepClone(),
                    ["trace"] = trace,
                    ["log"] = log.ToJson(),
                });
            }
            finally
            {
                foreach (var l in locks.Values) l.Dispose();
                W409RemoveRoot(root);
            }
        }
        return result;
    }

    /// <summary>
    /// NoteAccessed with an injected clock. Case {name, steps:[...]}; the store is (root, "*.png", 1000, utcNow: () => now), now starts at
    /// 2020-01-02T03:04:05Z. Steps: {op:"file", name, access?} (create), {op:"dir", name}, {op:"clock", now}, {op:"access", name},
    /// {op:"access_many", prefix, count} (NoteAccessed on prefix0.png .. prefix{count-1}.png, none of which exist),
    /// {op:"stamp", name, access} (File.SetLastAccessTimeUtc), {op:"read", name} (records File.GetLastAccessTimeUtc).
    /// Result: "reads" [{step, name, access}] and "throws" if a NoteAccessed call threw (it must not).
    /// </summary>
    public static JsonArray DiskCacheNoteAccessed(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = W409NewRoot();
            try
            {
                var now = new DateTime(2020, 1, 2, 3, 4, 5, DateTimeKind.Utc);
                var store = new DiskCacheStore(root, "*.png", 1000, log: null, utcNow: () => now);
                var reads = new JsonArray();
                string? thrown = null;
                var steps = c["steps"]!.AsArray();
                for (var i = 0; i < steps.Count; i++)
                {
                    var s = steps[i]!.AsObject();
                    var op = s["op"]!.GetValue<string>();
                    string P() => Path.Combine(root, s["name"]!.GetValue<string>());
                    try
                    {
                        switch (op)
                        {
                            case "file": W409Create(root, s); break;
                            case "dir": Directory.CreateDirectory(P()); break;
                            case "clock": now = W409Time(s["now"]); break;
                            case "access": store.NoteAccessed(P()); break;
                            case "access_many":
                                var prefix = s["prefix"]!.GetValue<string>();
                                for (var k = 0; k < s["count"]!.GetValue<int>(); k++) store.NoteAccessed(Path.Combine(root, $"{prefix}{k}.png"));
                                break;
                            case "stamp": File.SetLastAccessTimeUtc(P(), W409Time(s["access"])); break;
                            case "read":
                                reads.Add(new JsonObject { ["step"] = i, ["name"] = s["name"]!.GetValue<string>(), ["access"] = File.GetLastAccessTimeUtc(P()) });
                                break;
                            default: throw new ArgumentException("unknown op " + op);
                        }
                    }
                    catch (Exception ex) when (op is "access" or "access_many") { thrown ??= $"step {i}: {ex.GetType().Name}"; }
                }
                var row = new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["input"] = c.DeepClone(),
                    ["reads"] = reads,
                };
                if (thrown is not null) row["throws"] = thrown;
                result.Add(row);
            }
            finally { W409RemoveRoot(root); }
        }
        return result;
    }

    /// <summary>
    /// DiskCacheStore.DeleteStaleTempFiles for each case {name, utc_now, max_files, files:[{name, write?, dir?}], target?: "missing"}.
    /// Result: removed, survivors, log.
    /// </summary>
    public static JsonArray DiskCacheStaleTemp(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = W409NewRoot();
            try
            {
                foreach (var f in c["files"]?.AsArray() ?? []) W409Create(root, f!.AsObject());
                var target = W2Util.Str(c["target"]) == "missing" ? Path.Combine(root, "nope") : root;
                var log = new W409Log(root);
                var removed = DiskCacheStore.DeleteStaleTempFiles(target, W409Time(c["utc_now"]), c["max_files"]!.GetValue<int>(), log);
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["input"] = c.DeepClone(),
                    ["removed"] = removed,
                    ["survivors"] = W409Survivors(root),
                    ["log"] = log.ToJson(),
                });
            }
            finally { W409RemoveRoot(root); }
        }
        return result;
    }

    /// <summary>
    /// TryDelete / static ClearDirectory / instance ClearDirectory. Case {name, op: "try_delete" | "clear_static" | "clear_instance",
    /// files:[{..., locked?, readonly?}], target?: name (try_delete; "" = the empty path, "." = the case folder itself),
    /// pattern? (clear_*), log?: bool (default true)}. Result: result (try_delete bool), throws?, survivors, log.
    /// </summary>
    public static JsonArray DiskCacheDelete(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = W409NewRoot();
            try
            {
                var files = c["files"]?.AsArray() ?? [];
                foreach (var f in files) W409Create(root, f!.AsObject());
                var log = new W409Log(root);
                ILog? useLog = (c["log"]?.GetValue<bool>() ?? true) ? log : null;
                var op = c["op"]!.GetValue<string>();
                var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["input"] = c.DeepClone() };
                var locks = W409Lock(root, files);
                try
                {
                    switch (op)
                    {
                        case "try_delete":
                            var t = c["target"]!.GetValue<string>();
                            var path = t switch { "" => "", "." => root, _ => Path.Combine(root, t) };
                            row["result"] = DiskCacheStore.TryDelete(path, useLog);
                            break;
                        case "clear_static":
                            try { DiskCacheStore.ClearDirectory(root, c["pattern"]!.GetValue<string>(), useLog); }
                            catch (Exception ex) { row["throws"] = ex.GetType().Name; }
                            break;
                        case "clear_instance":
                            try { new DiskCacheStore(root, c["pattern"]!.GetValue<string>(), 1000, useLog).ClearDirectory(); }
                            catch (Exception ex) { row["throws"] = ex.GetType().Name; }
                            break;
                        default: throw new ArgumentException("unknown op " + op);
                    }
                }
                finally { foreach (var l in locks) l.Dispose(); }
                row["survivors"] = W409Survivors(root);
                row["log"] = log.ToJson();
                result.Add(row);
            }
            finally { W409RemoveRoot(root); }
        }
        return result;
    }

    /// <summary>IsCacheEntryFailure for each case {name, type}: the C# exception type built with a fixed message. Result: is_failure.</summary>
    public static JsonArray CacheEntryFailure(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var type = c["type"]!.GetValue<string>();
            Exception ex = type switch
            {
                "Exception" => new Exception("x"),
                "IOException" => new IOException("x"),
                "FileNotFoundException" => new FileNotFoundException("x"),
                "DirectoryNotFoundException" => new DirectoryNotFoundException("x"),
                "EndOfStreamException" => new EndOfStreamException("x"),
                "PathTooLongException" => new PathTooLongException("x"),
                "DecoderMemoryAdmissionException" => new DecoderMemoryAdmissionException(),
                "UnauthorizedAccessException" => new UnauthorizedAccessException("x"),
                "NotSupportedException" => new NotSupportedException("x"),
                "MissingImageCodecException" => new MissingImageCodecException(),
                "PlatformNotSupportedException" => new PlatformNotSupportedException("x"),
                "FormatException" => new FormatException("x"),
                "InvalidDataException" => new InvalidDataException("x"),
                "ArgumentException" => new ArgumentException("x"),
                "ArgumentNullException" => new ArgumentNullException("x"),
                "ArgumentOutOfRangeException" => new ArgumentOutOfRangeException("x"),
                "InvalidOperationException" => new InvalidOperationException("x"),
                "ObjectDisposedException" => new ObjectDisposedException("x"),
                "OverflowException" => new OverflowException("x"),
                "InvalidCastException" => new InvalidCastException("x"),
                "COMException" => new COMException("x", unchecked((int)0x88982F60)),
                "COMException-E_OUTOFMEMORY" => new COMException("x", unchecked((int)0x8007000E)),
                "OperationCanceledException" => new OperationCanceledException("x"),
                "TaskCanceledException" => new TaskCanceledException("x"),
                "OutOfMemoryException" => new OutOfMemoryException("x"),
                "InsufficientMemoryException" => new InsufficientMemoryException("x"),
                "DecoderBusyException" => new DecoderBusyException(),
                "DllNotFoundException" => new DllNotFoundException("x"),
                "BadImageFormatException" => new BadImageFormatException("x"),
                "EntryPointNotFoundException" => new EntryPointNotFoundException("x"),
                "NullReferenceException" => new NullReferenceException("x"),
                "TimeoutException" => new TimeoutException("x"),
                _ => throw new ArgumentException("unknown exception type " + type),
            };
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["type"] = type,
                ["is_failure"] = (bool)W409IsCacheEntryFailure.Invoke(null, [ex])!,
            });
        }
        return result;
    }
}
