using System.Reflection;
using System.Text;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Model;

// W3-05 golden package: UndoService (C# d92d6aae, src/PhotoReview.Core/FileActions/UndoService.cs).
// Copy to tools/oracle-cs/Commands/W3-05.cs and add this dispatch line to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W3-05
//     "undo-scenarios" => Commands.UndoScenarios(inputs),
// Inputs: tests/golden-inputs/undo-scenarios.json (regenerate with contracts/drafts-w3/W3-05/tools/gen_golden_inputs.py).
// Self-contained (no W2Util). Runs the REAL UndoService + OperationJournal (+ FileActionService only as the shared gate) over an
// in-memory IFileSystem and a scripted fake IRecycleBin. NEVER touches the real file system or the real Recycle Bin.
// The language stays the built-in English (Localizer.Current is never set), which is what EnglishUndoTexts reproduces.
//
// Determinism (D19): fixed clock, explicit file times, journal Ids/GroupIds (Guid.NewGuid "N") replaced by <guid1>, <guid2>, ...
// in order of first appearance. The Rust golden test must build the SAME fakes; their exact semantics are in the contract
// (contracts/W3-05.md, "Golden package") and in the comments below. Error texts of the fake file system are chosen to equal
// pr_test_support::FakeFileSystem's ("not found: <path>", "exists: <path>") so messages compare byte for byte.

static partial class Commands
{
    private const string W305JournalPath = @"C:\data\operations.jsonl";

    private sealed class W305Clock(DateTime now) : IClock
    {
        public DateTime UtcNow => now;
    }

    private sealed class W305Paths : IAppPaths
    {
        public string ConfigFile => @"C:\data\config.json";
        public string JournalFile => W305JournalPath;
        public string SessionsDir => @"C:\data\Sessions";
        public string LogFile => @"C:\data\logs\app.log";
        public string PreviewCacheDir => @"C:\data\cache";
        public string ThumbnailCacheDir => @"C:\data\thumbnails";
        public string WindowPlacementFile => @"C:\data\window-placement.json";
    }

    /// <summary>
    /// In-memory IFileSystem with the semantics of pr_test_support::FakeFileSystem (case-insensitive keys, the spelling of the
    /// latest write kept, no overwrite on move, the destination's parent folder must exist, adding a file adds its folders)
    /// plus the per-step hooks of a scenario. Hooks only fire on calls made by the code under test, never on scripted
    /// "by hand" actions.
    /// </summary>
    private sealed class W305Fs : IFileSystem
    {
        private sealed class Entry
        {
            public required string Path;
            public required byte[] Data;
            public required DateTime LastWriteUtc;
        }

        private readonly Dictionary<string, Entry> _files = new(StringComparer.OrdinalIgnoreCase);
        private readonly HashSet<string> _dirs = new(StringComparer.OrdinalIgnoreCase);

        // ---- per-step hooks (reset before every step) ----
        /// <summary>Move(source, _) throws IOException(message) BEFORE touching anything when source (OrdinalIgnoreCase) is a key.</summary>
        public Dictionary<string, string> MoveFail { get; } = new(StringComparer.OrdinalIgnoreCase);
        /// <summary>Move copies (keeps the source), like a cross-volume move whose source delete failed.</summary>
        public bool MoveLeavesSource { get; set; }
        /// <summary>Actions run right after the Nth SUCCESSFUL Move of the step (1-based).</summary>
        public List<(int Nth, JsonArray Actions)> AfterMove { get; } = [];
        /// <summary>Actions run at the Nth OpenAppend call of the step (1-based), before the fail check and before the stream exists.</summary>
        public List<(int Nth, JsonArray Actions)> OnAppend { get; } = [];
        /// <summary>OpenAppend calls numbered >= this throw IOException(AppendFailMessage) (after the OnAppend actions).</summary>
        public int? AppendFailFrom { get; set; }
        public string AppendFailMessage { get; set; } = "simulated journal failure";
        public int Moves { get; private set; }
        public int Appends { get; private set; }

        public void ResetHooks()
        {
            MoveFail.Clear();
            MoveLeavesSource = false;
            AfterMove.Clear();
            OnAppend.Clear();
            AppendFailFrom = null;
            AppendFailMessage = "simulated journal failure";
            Moves = 0;
            Appends = 0;
        }

        // ---- scripted ("by hand") actions ----
        public void AddDirs(string? dir)
        {
            for (var d = dir; !string.IsNullOrEmpty(d); d = System.IO.Path.GetDirectoryName(d)) _dirs.Add(d);
        }

        public void Add(string path, long size, DateTime lastWriteUtc)
        {
            AddDirs(System.IO.Path.GetDirectoryName(path));
            _files[path] = new Entry { Path = path, Data = Enumerable.Repeat((byte)'x', checked((int)size)).ToArray(), LastWriteUtc = lastWriteUtc };
        }

        public void RemoveByHand(string path) => _files.Remove(path);

        public void MoveByHand(string from, string to)
        {
            var entry = _files[from];
            _files.Remove(from);
            AddDirs(System.IO.Path.GetDirectoryName(to));
            _files[to] = new Entry { Path = to, Data = entry.Data, LastWriteUtc = entry.LastWriteUtc };
        }

        public void AppendRaw(string path, string text) => AppendBytes(path, Encoding.UTF8.GetBytes(text));

        public void AppendBytes(string path, byte[] bytes)
        {
            AddDirs(System.IO.Path.GetDirectoryName(path));
            if (_files.TryGetValue(path, out var entry)) entry.Data = [.. entry.Data, .. bytes];
            else _files[path] = new Entry { Path = path, Data = bytes, LastWriteUtc = new DateTime(0, DateTimeKind.Utc) };
        }

        public Action<JsonArray>? RunActions { get; set; }

        public IEnumerable<(string Path, long Size, long Ticks)> Listing() =>
            _files.Values.Where(e => !string.Equals(e.Path, W305JournalPath, StringComparison.OrdinalIgnoreCase))
                .OrderBy(e => e.Path, StringComparer.OrdinalIgnoreCase)
                .Select(e => (e.Path, (long)e.Data.Length, e.LastWriteUtc.Ticks));

        public string ReadText(string path) => _files.TryGetValue(path, out var e) ? Encoding.UTF8.GetString(e.Data) : "";

        private void RequireParent(string path)
        {
            var parent = System.IO.Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(parent) && !_dirs.Contains(parent)) throw new DirectoryNotFoundException("not found: " + parent);
        }

        // ---- IFileSystem ----
        public bool FileExists(string path) => _files.ContainsKey(path);
        public bool DirectoryExists(string path) => _dirs.Contains(path);

        public FileStat? GetFileStat(string path) =>
            _files.TryGetValue(path, out var e) ? new FileStat(e.Data.Length, DateTime.SpecifyKind(e.LastWriteUtc, DateTimeKind.Utc)) : null;

        public void Move(string source, string destination)
        {
            if (MoveFail.TryGetValue(source, out var message)) throw new IOException(message);
            if (!_files.TryGetValue(source, out var entry)) throw new FileNotFoundException("not found: " + source);
            if (_files.ContainsKey(destination)) throw new IOException("exists: " + destination);
            RequireParent(destination);
            if (!MoveLeavesSource) _files.Remove(source);
            _files[destination] = new Entry { Path = destination, Data = entry.Data, LastWriteUtc = entry.LastWriteUtc };
            Moves++;
            foreach (var (nth, actions) in AfterMove) if (nth == Moves) RunActions!(actions);
        }

        public void Copy(string source, string destination)
        {
            if (!_files.TryGetValue(source, out var entry)) throw new FileNotFoundException("not found: " + source);
            if (_files.ContainsKey(destination)) throw new IOException("exists: " + destination);
            RequireParent(destination);
            _files[destination] = new Entry { Path = destination, Data = entry.Data, LastWriteUtc = entry.LastWriteUtc };
        }

        public void Delete(string path) => _files.Remove(path);

        public Stream OpenReadShared(string path, int bufferSize = 65536) =>
            _files.TryGetValue(path, out var e) ? new MemoryStream(e.Data.ToArray(), writable: false) : throw new FileNotFoundException("not found: " + path);

        public Stream OpenAppend(string path, bool durable)
        {
            Appends++;
            foreach (var (nth, actions) in OnAppend) if (nth == Appends) RunActions!(actions);
            if (AppendFailFrom is { } from && Appends >= from) throw new IOException(AppendFailMessage);
            RequireParent(path);
            if (!_files.ContainsKey(path)) _files[path] = new Entry { Path = path, Data = [], LastWriteUtc = new DateTime(0, DateTimeKind.Utc) };
            return new AppendStream(this, path);
        }

        private sealed class AppendStream(W305Fs owner, string path) : Stream
        {
            public override bool CanRead => false;
            public override bool CanSeek => false;
            public override bool CanWrite => true;
            public override long Length => throw new NotSupportedException();
            public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
            public override void Flush() { }
            public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
            public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
            public override void SetLength(long value) => throw new NotSupportedException();
            public override void Write(byte[] buffer, int offset, int count) =>
                owner.AppendBytes(path, buffer.AsSpan(offset, count).ToArray());
        }

        public void WriteAllTextAtomic(string path, string text, bool durable = true)
        {
            RequireParent(path);
            _files[path] = new Entry { Path = path, Data = Encoding.UTF8.GetBytes(text), LastWriteUtc = new DateTime(0, DateTimeKind.Utc) };
        }

        public string ReadAllText(string path) =>
            _files.TryGetValue(path, out var e) ? Encoding.UTF8.GetString(e.Data) : throw new FileNotFoundException("not found: " + path);

        public IEnumerable<string> EnumerateFiles(string directory, string pattern = "*") =>
            _files.Values.Where(e => string.Equals(System.IO.Path.GetDirectoryName(e.Path), directory, StringComparison.OrdinalIgnoreCase))
                .Select(e => e.Path).ToList();

        public IEnumerable<string> EnumerateDirectories(string directory) =>
            _dirs.Where(d => string.Equals(System.IO.Path.GetDirectoryName(d), directory, StringComparison.OrdinalIgnoreCase)).ToList();

        public void CreateDirectory(string path)
        {
            if (_files.ContainsKey(path)) throw new IOException("a file exists: " + path);
            AddDirs(path);
        }
    }

    /// <summary>
    /// Scripted Recycle Bin over W305Fs (never the real bin). TryRestore records the path, then acts by the mode of that path
    /// (OrdinalIgnoreCase), else the default mode:
    ///   restore              add the file (size 'x' bytes, the expected write time), return true
    ///   fail                 return false, touch nothing
    ///   restore_report_false add the file, return false (the C5 case: the bin lies about a successful restore)
    ///   report_true_only     return true, touch nothing
    ///   restore_other_stamp  add the file with the expected write time + 1 h, return true
    ///   restore_wrong_size   add a 3-byte file with the expected write time, return true
    /// </summary>
    private sealed class W305Bin(W305Fs fs) : IRecycleBin
    {
        public string Default { get; set; } = "restore";
        public Dictionary<string, string> ByPath { get; } = new(StringComparer.OrdinalIgnoreCase);
        public List<string> Calls { get; } = [];

        public void SendToRecycleBin(string path) => throw new NotSupportedException("the undo never recycles");

        public bool TryRestore(string originalPath, long expectedSize, DateTime expectedLastWriteUtc)
        {
            Calls.Add(originalPath);
            var mode = ByPath.TryGetValue(originalPath, out var m) ? m : Default;
            switch (mode)
            {
                case "restore": fs.Add(originalPath, expectedSize, expectedLastWriteUtc); return true;
                case "fail": return false;
                case "restore_report_false": fs.Add(originalPath, expectedSize, expectedLastWriteUtc); return false;
                case "report_true_only": return true;
                case "restore_other_stamp": fs.Add(originalPath, expectedSize, expectedLastWriteUtc.AddHours(1)); return true;
                case "restore_wrong_size": fs.Add(originalPath, 3, expectedLastWriteUtc); return true;
                default: throw new ArgumentException("unknown bin mode " + mode);
            }
        }
    }

    private static DateTime W305Time(JsonNode? ticks) => new(ticks!.GetValue<long>(), DateTimeKind.Utc);

    private static string? W305Str(JsonNode? node) => node?.GetValue<string>();

    private static FileOperationType W305Op(JsonNode? node) => Enum.Parse<FileOperationType>(node!.GetValue<string>());

    private static JournalGroupMember W305Member(JsonNode? node)
    {
        var o = node!.AsObject();
        return new JournalGroupMember(o["source"]!.GetValue<string>(), W305Str(o["destination"]), o["size"]!.GetValue<long>(),
            W305Time(o["ticks"]), o["permanent"]?.GetValue<bool>() ?? false);
    }

    private static JournalEntry? W305Entry(JsonNode? node)
    {
        if (node is null) return null;
        var o = node.AsObject();
        return new JournalEntry(o["id"]!.GetValue<string>(), W305Op(o["type"]), Enum.Parse<JournalState>(o["state"]!.GetValue<string>()),
            o["source"]!.GetValue<string>(), W305Str(o["destination"]), o["size"]!.GetValue<long>(), W305Time(o["last_write_ticks"]),
            W305Time(o["timestamp_ticks"]), W305Str(o["error"]), W305Str(o["error_code"]), (o["permanent"] is { } permanent ? permanent.GetValue<bool>() : (bool?)null),
            (o["undo"] is { } undoFlag ? undoFlag.GetValue<bool>() : (bool?)null), W305Str(o["group_id"]),
            o["group_members"] is JsonArray members ? members.Select(W305Member).ToArray() : null);
    }

    private static FileActionResult W305Single(JsonNode? node)
    {
        var o = node!.AsObject();
        return new FileActionResult(o["succeeded"]!.GetValue<bool>(), W305Op(o["operation"]), o["source"]!.GetValue<string>(),
            W305Str(o["destination_path"]), o["size"]!.GetValue<long>(), W305Time(o["last_write_ticks"]), W305Str(o["error"]),
            Rejected: o["rejected"]?.GetValue<bool>() ?? false, PermanentlyDeleted: o["permanently_deleted"]?.GetValue<bool>() ?? false);
    }

    private static CaptureGroupActionResult W305Group(JsonNode? node)
    {
        var o = node!.AsObject();
        var members = o["members"]!.AsArray().Select(m =>
        {
            var mo = m!.AsObject();
            return new CaptureGroupMemberResult(W305Member(mo["member"]), mo["completed"]!.GetValue<bool>(), mo["conflict"]?.GetValue<bool>() ?? false,
                W305Str(mo["error"]), SourceExists: mo["source_exists"]?.GetValue<bool>() ?? false);
        }).ToArray();
        return new CaptureGroupActionResult(o["succeeded"]!.GetValue<bool>(), o["rejected"]?.GetValue<bool>() ?? false, W305Op(o["operation"]),
            o["group_id"]!.GetValue<string>(), W305Entry(o["entry"]), members, W305Str(o["error"]),
            PermanentlyDeleted: o["permanently_deleted"]?.GetValue<bool>() ?? false);
    }

    private static JsonObject W305Result(UndoResult r) => new()
    {
        ["succeeded"] = r.Succeeded,
        ["operation"] = r.Operation?.ToString(),
        ["source"] = r.Source,
        ["destination"] = r.Destination,
        ["error_message"] = r.ErrorMessage,
        ["rejected"] = r.Rejected,
        ["restored_paths"] = r.RestoredPaths is null ? null : new JsonArray(r.RestoredPaths.Select(p => (JsonNode?)JsonValue.Create(p)).ToArray()),
    };

    private static string W305NormalizeGuids(string text)
    {
        var map = new Dictionary<string, string>(StringComparer.Ordinal);
        return Regex.Replace(text, "[0-9a-f]{32}", m =>
        {
            if (!map.TryGetValue(m.Value, out var name)) map[m.Value] = name = "<guid" + (map.Count + 1) + ">";
            return name;
        });
    }

    /// <summary>
    /// Runs every scripted scenario (see the contract for the schema). Output per case: the input plus "steps_out" (one object per
    /// step: "result", "bin_calls", "override_calls", "state") and "final" ("files" without the journal, "journal" text with
    /// normalized Ids).
    /// </summary>
    public static JsonArray UndoScenarios(JsonObject inputs)
    {
        var fingerprintCount = typeof(UndoService).GetProperty("MoveFingerprintCount", BindingFlags.Instance | BindingFlags.NonPublic)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var fs = new W305Fs();
            void Run(JsonArray actions)
            {
                foreach (var a in actions)
                {
                    var ao = a!.AsObject();
                    if (ao["add"] is { } add) fs.Add(add.GetValue<string>(), ao["size"]!.GetValue<long>(), W305Time(ao["ticks"]));
                    else if (ao["delete"] is { } del) fs.RemoveByHand(del.GetValue<string>());
                    else if (ao["move"] is { } mv) fs.MoveByHand(mv.GetValue<string>(), ao["to"]!.GetValue<string>());
                    else if (ao["append_line"] is { } line) fs.AppendRaw(W305JournalPath, line.GetValue<string>() + "\r\n");
                    else throw new ArgumentException("unknown action " + ao.ToJsonString());
                }
            }
            fs.RunActions = Run;
            foreach (var d in c["dirs"]?.AsArray() ?? new JsonArray()) fs.AddDirs(d!.GetValue<string>());
            foreach (var f in c["files"]?.AsArray() ?? new JsonArray())
                fs.Add(f!["path"]!.GetValue<string>(), f["size"]!.GetValue<long>(), W305Time(f["ticks"]));
            foreach (var line in c["journal_lines"]?.AsArray() ?? new JsonArray()) fs.AppendRaw(W305JournalPath, line!.GetValue<string>() + "\r\n");

            var bin = new W305Bin(fs);
            void SetBin(JsonNode? spec)
            {
                if (spec is null) return;
                bin.Default = W305Str(spec["default"]) ?? "restore";
                bin.ByPath.Clear();
                foreach (var (path, mode) in spec["by_path"]?.AsObject() ?? new JsonObject()) bin.ByPath[path] = mode!.GetValue<string>();
            }
            SetBin(c["bin"]);

            var clock = new W305Clock(W305Time(c["clock_ticks"]));
            var undoClock = c["undo_clock_ticks"] is { } uc ? new W305Clock(W305Time(uc)) : clock;
            var journal = new OperationJournal(new W305Paths(), fs, clock);
            var shared = c["shared_gate"]?.GetValue<bool>() ?? false;
            var gate = shared ? new FileActionService(journal, fs, clock, bin) : null;
            var overrideCalls = new List<(string From, string To)>();
            Func<string, string, Task>? moveOverride = (c["override"]?.GetValue<bool>() ?? false)
                ? (from, to) => { overrideCalls.Add((from, to)); fs.Move(from, to); return Task.CompletedTask; }
                : null;
            var undo = new UndoService(journal, fs, bin, gate, moveOverride, undoClock);

            var stepsOut = new JsonArray();
            foreach (var stepNode in c["steps"]!.AsArray())
            {
                var step = stepNode!.AsObject();
                fs.ResetHooks();
                bin.Calls.Clear();
                overrideCalls.Clear();
                if (step["hooks"] is JsonObject hooks)
                {
                    foreach (var (path, message) in hooks["move_fail"]?.AsObject() ?? new JsonObject()) fs.MoveFail[path] = message!.GetValue<string>();
                    fs.MoveLeavesSource = hooks["move_leaves_source"]?.GetValue<bool>() ?? false;
                    foreach (var h in hooks["after_move"]?.AsArray() ?? new JsonArray()) fs.AfterMove.Add((h!["nth"]!.GetValue<int>(), h["actions"]!.AsArray()));
                    foreach (var h in hooks["on_append"]?.AsArray() ?? new JsonArray()) fs.OnAppend.Add((h!["nth"]!.GetValue<int>(), h["actions"]!.AsArray()));
                    fs.AppendFailFrom = hooks["append_fail_from"]?.GetValue<int>();
                    if (W305Str(hooks["append_fail_message"]) is { } msg) fs.AppendFailMessage = msg;
                }

                JsonNode? stepResult = null;
                switch (step["op"]!.GetValue<string>())
                {
                    case "register": undo.Register(W305Single(step["result"])); break;
                    case "register_group": undo.RegisterGroup(W305Group(step["result"])); break;
                    case "undo_last": stepResult = W305Result(undo.UndoLastAsync().GetAwaiter().GetResult()); break;
                    case "undo_move": stepResult = W305Result(undo.UndoMoveAsync().GetAwaiter().GetResult()); break;
                    case "push_history": undo.MoveHistory.Push((step["source"]!.GetValue<string>(), step["destination"]!.GetValue<string>())); break;
                    case "gate_begin": stepResult = new JsonObject { ["began"] = gate?.TryBegin() ?? undo.TryBegin() }; break;
                    case "gate_end": if (gate is not null) gate.End(); else undo.End(); break;
                    case "fs": Run(step["actions"]!.AsArray()); break;
                    case "set_bin": SetBin(step["bin"]); break;
                    default: throw new ArgumentException("unknown op " + step["op"]);
                }

                stepsOut.Add(new JsonObject
                {
                    ["result"] = stepResult,
                    ["bin_calls"] = new JsonArray(bin.Calls.Select(p => (JsonNode?)JsonValue.Create(p)).ToArray()),
                    ["override_calls"] = new JsonArray(overrideCalls.Select(call => (JsonNode?)new JsonArray(call.From, call.To)).ToArray()),
                    ["state"] = new JsonObject
                    {
                        // Oldest first (C# enumerates the Stack top first).
                        ["move_history"] = new JsonArray(undo.MoveHistory.Reverse().Select(h => (JsonNode?)new JsonArray(h.Source, h.Destination)).ToArray()),
                        ["has_last_action"] = undo.LastUndoAction is not null,
                        ["fingerprints"] = (int)fingerprintCount.GetValue(undo)!,
                        ["busy"] = undo.IsBusy,
                    },
                });
            }

            var output = (JsonObject)c.DeepClone();
            output["steps_out"] = stepsOut;
            output["final"] = new JsonObject
            {
                ["files"] = new JsonArray(fs.Listing().Select(f => (JsonNode?)new JsonObject
                {
                    ["path"] = f.Path,
                    ["size"] = f.Size,
                    ["ticks"] = f.Ticks,
                }).ToArray()),
                ["journal"] = W305NormalizeGuids(fs.ReadText(W305JournalPath)),
            };
            result.Add(output);
        }
        return result;
    }
}
