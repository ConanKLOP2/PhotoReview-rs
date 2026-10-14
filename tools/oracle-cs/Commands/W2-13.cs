// W2-13 golden package: SessionWriter deterministic sequences (C# d92d6aae).
// Copy to tools/oracle-cs/Commands/W2-13.cs and add this dispatch line to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W2-13
//     "session-writer-sequence" => Commands.SessionWriterSequenceCases(inputs),
// Input: tests/golden-inputs/session-writer-sequence.json (ready to copy from golden-inputs/). Needs W2-00.cs (class W2Util).
// Only the timing-free parts are pinned: constructor validation, the spans handed to the timer, coalescing by folder key,
// what lands on disk after fire/flush/dispose. Races and bounded waits stay with the ported C# tests.
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.IO;
using PhotoReview.Core.Session;

static partial class Commands
{
    private sealed class W213Paths(string sessions) : IAppPaths
    {
        public string ConfigFile => @"C:\data\config.json";
        public string JournalFile => @"C:\data\operations.jsonl";
        public string SessionsDir => sessions;
        public string LogFile => @"C:\data\logs\app.log";
        public string PreviewCacheDir => @"C:\data\cache";
        public string ThumbnailCacheDir => @"C:\data\thumbnails";
        public string WindowPlacementFile => @"C:\data\window-placement.json";
    }

    private sealed class W213Log : ILog
    {
        public List<string> Errors { get; } = [];
        public bool Enabled => true;
        public void Info(string message) { }
        public void Warn(string message) { }
        public void Error(string message, Exception? ex = null) { lock (Errors) Errors.Add(message); }
    }

    /// <summary>
    /// SessionWriter driven by a hand-fired timer. Case: {name, debounce_ms (number|null = default), ops:[{op:"update", folder, current,
    /// skipped?} | {op:"fire"} | {op:"flush"} | {op:"dispose"}], probe:[folders]}. Reports: constructor exception type, the spans (ms)
    /// requested from the timer, per op the exception type, the number of timer requests so far and the number of session files on
    /// disk, and for each probe folder what Load returns (current path, skipped) after all ops.
    /// </summary>
    public static JsonArray SessionWriterSequenceCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = new JsonObject { ["name"] = c["name"]!.DeepClone(), ["debounce_ms"] = c["debounce_ms"]?.DeepClone() };
            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-writer-" + Guid.NewGuid().ToString("N"));
            var sessions = Path.Combine(root, "Sessions");
            Directory.CreateDirectory(root);
            try
            {
                var store = new SessionStore(new W213Paths(sessions), new PhysicalFileSystem());
                var log = new W213Log();
                var requests = new List<double>();
                var waiters = new List<TaskCompletionSource>();
                Task Delay(TimeSpan span, CancellationToken token)
                {
                    var tcs = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
                    lock (requests) { requests.Add(span.TotalMilliseconds); waiters.Add(tcs); }
                    token.Register(() => tcs.TrySetCanceled(token));
                    return tcs.Task;
                }

                SessionWriter? writer = null;
                try
                {
                    TimeSpan? debounce = c["debounce_ms"] is { } ms ? TimeSpan.FromMilliseconds(ms.GetValue<double>()) : null;
                    writer = new SessionWriter(store, log, debounce, Delay);
                    item["ctor_exception"] = null;
                }
                catch (Exception ex) { item["ctor_exception"] = ex.GetType().Name; }

                var steps = new JsonArray();
                if (writer is not null)
                {
                    foreach (var opNode in c["ops"]!.AsArray())
                    {
                        var op = opNode!["op"]!.GetValue<string>();
                        string? exception = null;
                        try
                        {
                            switch (op)
                            {
                                case "update":
                                    writer.Update(new SessionState
                                    {
                                        Folder = opNode["folder"]!.GetValue<string>(),
                                        CurrentPath = opNode["current"]?.GetValue<string>(),
                                        Skipped = W2Util.Strings(opNode["skipped"]),
                                        UpdatedUtc = new DateTime(2024, 1, 2, 3, 4, 5, DateTimeKind.Utc),
                                    });
                                    break;
                                case "fire":
                                    List<TaskCompletionSource> snapshot;
                                    lock (requests) snapshot = [.. waiters];
                                    foreach (var w in snapshot) w.TrySetResult();
                                    writer.WhenIdleAsync().Wait(TimeSpan.FromSeconds(10));
                                    break;
                                case "flush": writer.Flush(); break;
                                case "dispose": writer.Dispose(); break;
                                default: throw new ArgumentException("unknown op " + op);
                            }
                        }
                        catch (Exception ex) { exception = ex.GetType().Name; }
                        int count;
                        lock (requests) count = requests.Count;
                        steps.Add(new JsonObject
                        {
                            ["op"] = op,
                            ["exception"] = exception,
                            ["timer_requests"] = count,
                            ["files"] = Directory.Exists(sessions) ? Directory.GetFiles(sessions, "*.json").Length : 0,
                        });
                    }
                }
                item["steps"] = steps;
                lock (requests) item["spans_ms"] = new JsonArray(requests.Select(r => (JsonNode?)JsonValue.Create(r)).ToArray());
                var probes = new JsonArray();
                foreach (var folderNode in c["probe"]!.AsArray())
                {
                    var folder = folderNode!.GetValue<string>();
                    var loaded = store.Load(folder);
                    probes.Add(new JsonObject
                    {
                        ["folder"] = folder,
                        ["current"] = loaded.CurrentPath,
                        ["skipped"] = W2Util.StrArray(loaded.Skipped),
                    });
                }
                item["probes"] = probes;
                item["log_errors"] = log.Errors.Count;
                writer?.Dispose();
            }
            finally { try { Directory.Delete(root, true); } catch (IOException) { } }
            result.Add(item);
        }
        return result;
    }
}
