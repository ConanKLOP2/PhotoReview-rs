using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Model;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W3-06
//     "recovery-retry" => Commands.RecoveryRetry(inputs),
// Standalone: does not use W2Util. Never touches the real Recycle Bin or the real disk: everything runs on the scripted
// in-memory W306Fs / W306Bin below. Deterministic (D19): two fixed clocks, no real time, no temp folder.

// W3-06: RecoveryRetryService.RetryMoveOrCopyAsync over a scripted world. Each case:
//   files:   [{path, len, ticks}]           files on disk before (content = len zero bytes, last write = ticks, Kind Utc)
//   binned:  [{path, len, ticks}]           items already in the (fake) Recycle Bin, restorable by TryRestore
//   seed:    [entry]                         appended to the journal in order before the retry (journal clock)
//   snapshot: index into seed (default: 0) the entry handed to RetryMoveOrCopyAsync
//   journal_clock_ticks / service_clock_ticks: the two fixed clocks (journal; service = the transaction's clock)
//   confirmed: bool                          confirmedFinishCancelled
//   cancel_before: bool                      token cancelled before the call
//   gate: "none" | "free" | "held"           FileActionService passed as fileActionGate; "held" = TryBegin() before the call
//   live: bool                               the snapshot's Id is live (marker held on the journal's registry) during the call
//   bin: null | {no_bin:[paths], fits:bool, restore:"ok"|"refuse-leave-file"|"ok-no-file", recycle_noop:bool}
//   allow_permanent: null | bool             allowPermanentDelete (null = parameter not passed)
//   hooks: [{on:"stat"|"move"|"copy"|"append"|"restore"|"recycle", path?, nth, do:[action]}]
//          counted per (on, path) from the START of the retry (seed appends are not counted); a hook runs at the start of
//          its call, BEFORE the real operation. Actions, in order:
//            {delete: path} | {write: {path, len, ticks}} | {cancel: true}
//            {throw: message, exception?: "IOException"|"UnauthorizedAccessException"|"ArgumentException"|"NotSupportedException"}
//            {leave_source: true}   (move only) copy to the destination, keep the source, return normally
//            {partial: n, message}  (copy only) create the destination with the first n bytes, then throw IOException(message)
// Output (added to a copy of the input case): result {succeeded, message, superseded, journal_persisted, journal_error, entry} or threw (exception type name);
//   journal_text (the whole journal file after the call, exact text), files_after, binned_after, ops (every file-system and
//   bin call outside the journal folder, in call order), gate_busy_after, live_after.
static partial class Commands
{
    private const string W306JournalPath = @"C:\data\operations.jsonl";
    private const string W306JournalDir = @"C:\data";

    private sealed class W306Paths : IAppPaths
    {
        public string ConfigFile => @"C:\data\config.json";
        public string JournalFile => W306JournalPath;
        public string SessionsDir => @"C:\data\Sessions";
        public string LogFile => @"C:\data\logs\app.log";
        public string PreviewCacheDir => @"C:\data\cache";
        public string ThumbnailCacheDir => @"C:\data\thumbnails";
        public string WindowPlacementFile => @"C:\data\window-placement.json";
    }

    private sealed class W306Clock(DateTime now) : IClock
    {
        public DateTime UtcNow => now;
    }

    private sealed class W306File(string path, byte[] data, DateTime lastWriteUtc)
    {
        public string Path { get; set; } = path;
        public byte[] Data { get; set; } = data;
        public DateTime LastWriteUtc { get; set; } = lastWriteUtc;
    }

    private static bool W306IsJournalPath(string path) =>
        string.Equals(path, W306JournalDir, StringComparison.OrdinalIgnoreCase)
        || path.StartsWith(W306JournalDir + @"\", StringComparison.OrdinalIgnoreCase);

    /// <summary>The scripted world: files, directories, hooks, cancellation and the call log.</summary>
    private sealed class W306World
    {
        public readonly Dictionary<string, W306File> Files = new(StringComparer.OrdinalIgnoreCase);
        public readonly HashSet<string> Dirs = new(StringComparer.OrdinalIgnoreCase);
        public readonly List<string> Ops = [];
        public readonly List<(string Path, byte[] Data, DateTime LastWriteUtc)> Binned = [];
        public readonly Dictionary<string, int> Counts = new(StringComparer.OrdinalIgnoreCase);
        public JsonArray Hooks = new();
        public bool Counting;
        public CancellationTokenSource Cts = new();

        public void Log(string op, string path, string? text = null)
        {
            if (W306IsJournalPath(path)) return;
            Ops.Add(text ?? op + " " + path);
        }

        public void AddDirs(string? dir)
        {
            while (!string.IsNullOrEmpty(dir))
            {
                Dirs.Add(dir);
                dir = System.IO.Path.GetDirectoryName(dir);
            }
        }

        public void Write(string path, long len, long ticks)
        {
            AddDirs(System.IO.Path.GetDirectoryName(path));
            Files[path] = new W306File(path, new byte[len], new DateTime(ticks, DateTimeKind.Utc));
        }

        /// <summary>Counts the call and runs the matching hook's actions. Returns the special action that the caller must
        /// perform instead of the real operation ("leave_source" / "partial"), or null.</summary>
        public JsonObject? Hook(string on, string? path)
        {
            if (!Counting) return null;
            var key = on + "|" + (path ?? "");
            Counts[key] = Counts.TryGetValue(key, out var n) ? n + 1 : 1;
            foreach (var node in Hooks)
            {
                var h = node!.AsObject();
                if (h["on"]!.GetValue<string>() != on) continue;
                if (path is not null && !string.Equals(h["path"]?.GetValue<string>(), path, StringComparison.OrdinalIgnoreCase)) continue;
                if ((h["nth"]?.GetValue<int>() ?? 1) != Counts[key]) continue;
                foreach (var actionNode in h["do"]!.AsArray())
                {
                    var a = actionNode!.AsObject();
                    if (a["delete"] is { } del) Files.Remove(del.GetValue<string>());
                    else if (a["write"] is { } w)
                        Write(w["path"]!.GetValue<string>(), w["len"]!.GetValue<long>(), w["ticks"]!.GetValue<long>());
                    else if (a["cancel"] is not null) Cts.Cancel();
                    else if (a["throw"] is { } message)
                    {
                        var text = message.GetValue<string>();
                        throw (a["exception"]?.GetValue<string>() ?? "IOException") switch
                        {
                            "IOException" => (Exception)new IOException(text),
                            "UnauthorizedAccessException" => new UnauthorizedAccessException(text),
                            "ArgumentException" => new ArgumentException(text),
                            "NotSupportedException" => new NotSupportedException(text),
                            var other => new InvalidOperationException("unknown exception " + other),
                        };
                    }
                    else if (a["leave_source"] is not null || a["partial"] is not null) return a;
                    else throw new InvalidOperationException("unknown hook action " + a.ToJsonString());
                }
            }
            return null;
        }
    }

    private sealed class W306AppendStream(W306World world, string path) : Stream
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

        public override void Write(byte[] buffer, int offset, int count)
        {
            if (!world.Files.TryGetValue(path, out var file))
            {
                file = new W306File(path, [], DateTime.MinValue);
                world.Files[path] = file;
            }
            file.Data = [.. file.Data, .. buffer.AsSpan(offset, count).ToArray()];
        }
    }

    /// <summary>In-memory IFileSystem with Windows semantics (case-insensitive, no overwrite, parent folder required).
    /// TryCopyNew is the interface default (FileExists, then Copy), like the Rust FakeFileSystem wrapper must use.</summary>
    private sealed class W306Fs(W306World world) : IFileSystem
    {
        public bool FileExists(string path)
        {
            world.Log("exists", path);
            return world.Files.ContainsKey(path);
        }

        public bool DirectoryExists(string path)
        {
            world.Log("dir", path);
            return world.Dirs.Contains(path);
        }

        public FileStat? GetFileStat(string path)
        {
            world.Log("stat", path);
            world.Hook("stat", path);
            return world.Files.TryGetValue(path, out var f) ? new FileStat(f.Data.LongLength, f.LastWriteUtc) : null;
        }

        public void Move(string source, string destination)
        {
            world.Log("move", source, "move " + source + " > " + destination);
            var special = world.Hook("move", source);
            if (!world.Files.TryGetValue(source, out var f)) throw new FileNotFoundException("not found: " + source);
            if (world.Files.ContainsKey(destination)) throw new IOException("exists: " + destination);
            RequireParent(destination);
            world.Files[destination] = new W306File(destination, f.Data, f.LastWriteUtc);
            if (special?["leave_source"] is null) world.Files.Remove(source);
        }

        public void Copy(string source, string destination)
        {
            world.Log("copy", source, "copy " + source + " > " + destination);
            var special = world.Hook("copy", source);
            if (!world.Files.TryGetValue(source, out var f)) throw new FileNotFoundException("not found: " + source);
            if (world.Files.ContainsKey(destination)) throw new IOException("exists: " + destination);
            RequireParent(destination);
            if (special?["partial"] is { } partial)
            {
                var n = partial.GetValue<int>();
                world.Files[destination] = new W306File(destination, f.Data.Take(n).ToArray(), f.LastWriteUtc);
                throw new IOException(special["message"]!.GetValue<string>());
            }
            world.Files[destination] = new W306File(destination, f.Data, f.LastWriteUtc);
        }

        private void RequireParent(string path)
        {
            var dir = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(dir) && !world.Dirs.Contains(dir))
                throw new DirectoryNotFoundException("missing folder: " + dir);
        }

        public void Delete(string path)
        {
            world.Log("delete", path);
            world.Files.Remove(path);
        }

        public Stream OpenReadShared(string path, int bufferSize = 65536)
        {
            world.Log("open-read", path);
            if (!world.Files.TryGetValue(path, out var f)) throw new FileNotFoundException("not found: " + path);
            return new MemoryStream(f.Data.ToArray(), writable: false);
        }

        public Stream OpenAppend(string path, bool durable)
        {
            world.Log("open-append", path);
            world.Hook("append", null);
            RequireParent(path);
            return new W306AppendStream(world, path);
        }

        public void WriteAllTextAtomic(string path, string text, bool durable = true)
        {
            world.Log("write", path);
            RequireParent(path);
            world.Files[path] = new W306File(path, System.Text.Encoding.UTF8.GetBytes(text), DateTime.MinValue);
        }

        public string ReadAllText(string path)
        {
            world.Log("read-text", path);
            if (!world.Files.TryGetValue(path, out var f)) throw new FileNotFoundException("not found: " + path);
            return System.Text.Encoding.UTF8.GetString(f.Data);
        }

        public IEnumerable<string> EnumerateFiles(string directory, string pattern = "*") =>
            throw new InvalidOperationException("RecoveryRetry must not list files");

        public IEnumerable<string> EnumerateDirectories(string directory) =>
            throw new InvalidOperationException("RecoveryRetry must not list folders");

        public void CreateDirectory(string path)
        {
            world.Log("mkdir", path);
            world.AddDirs(path);
        }
    }

    /// <summary>Scripted Recycle Bin over the world (never the real one).</summary>
    private sealed class W306Bin(W306World world, JsonObject config) : IRecycleBin
    {
        private bool NoBin(string path) =>
            config["no_bin"]?.AsArray().Any(p => string.Equals(p!.GetValue<string>(), path, StringComparison.OrdinalIgnoreCase)) == true;

        public void SendToRecycleBin(string path)
        {
            world.Log("bin.recycle", path, "bin.recycle " + path);
            world.Hook("recycle", path);
            if (config["recycle_noop"]?.GetValue<bool>() == true) return;
            if (!world.Files.Remove(path, out var f)) throw new FileNotFoundException("not found: " + path);
            world.Binned.Add((f.Path, f.Data, f.LastWriteUtc));
        }

        public bool CanRecycle(string path) => !NoBin(path);

        public void DeletePermanently(string path)
        {
            world.Log("bin.delete-permanently", path, "bin.delete-permanently " + path);
            world.Files.Remove(path);
        }

        public bool TryRestore(string originalPath, long expectedSize, DateTime expectedLastWriteUtc)
        {
            world.Log("bin.restore", originalPath, "bin.restore " + originalPath);
            world.Hook("restore", originalPath);
            switch (config["restore"]?.GetValue<string>() ?? "ok")
            {
                case "ok-no-file":
                    return true;
                case "refuse-leave-file":
                    if (!world.Files.ContainsKey(originalPath)) world.Write(originalPath, expectedSize, expectedLastWriteUtc.Ticks);
                    return false;
                default:
                    if (world.Files.ContainsKey(originalPath)) return false;
                    var index = world.Binned.FindLastIndex(b => string.Equals(b.Path, originalPath, StringComparison.OrdinalIgnoreCase)
                        && b.Data.LongLength == expectedSize && b.LastWriteUtc == expectedLastWriteUtc);
                    if (index < 0) return false;
                    var item = world.Binned[index];
                    world.Binned.RemoveAt(index);
                    world.AddDirs(Path.GetDirectoryName(originalPath));
                    world.Files[originalPath] = new W306File(originalPath, item.Data, item.LastWriteUtc);
                    return true;
            }
        }

        public bool FitsInRecycleBin(string path, long fileSize)
        {
            world.Log("bin.fits", path, "bin.fits " + path + " " + fileSize);
            return config["fits"]?.GetValue<bool>() ?? true;
        }
    }

    private static JournalEntry W306Entry(JsonNode? n)
    {
        var o = n!.AsObject();
        static DateTime T(JsonNode? t) => new(t!.GetValue<long>(), DateTimeKind.Utc);
        static string? S(JsonNode? s) => s?.GetValue<string>();
        static bool? B(JsonNode? b) => b?.GetValue<bool>();
        return new JournalEntry(
            o["id"]!.GetValue<string>(),
            Enum.Parse<FileOperationType>(o["type"]!.GetValue<string>()),
            Enum.Parse<JournalState>(o["state"]!.GetValue<string>()),
            o["source"]!.GetValue<string>(),
            S(o["destination"]),
            o["size"]!.GetValue<long>(),
            T(o["last_write_ticks"]),
            T(o["timestamp_ticks"]),
            Error: S(o["error"]),
            ErrorCode: S(o["error_code"]),
            Permanent: B(o["permanent"]),
            Undo: B(o["undo"]),
            GroupId: S(o["group_id"]),
            GroupMembers: o["members"] is JsonArray members
                ? members.Select(m => new JournalGroupMember(
                    m!["source"]!.GetValue<string>(), S(m["destination"]), m["size"]!.GetValue<long>(), T(m["last_write_ticks"]),
                    m["permanent"]?.GetValue<bool>() ?? false)).ToArray()
                : null);
    }

    private static JsonObject? W306EntryJson(JournalEntry? e) => e is null ? null : new JsonObject
    {
        ["id"] = e.Id,
        ["type"] = e.Type.ToString(),
        ["state"] = e.State.ToString(),
        ["source"] = e.Source,
        ["destination"] = e.Destination,
        ["size"] = e.Size,
        ["last_write_ticks"] = e.LastWriteUtc.Ticks,
        ["timestamp_ticks"] = e.TimestampUtc.Ticks,
        ["error"] = e.Error,
        ["error_code"] = e.ErrorCode,
        ["permanent"] = e.Permanent,
        ["undo"] = e.Undo,
        ["group_id"] = e.GroupId,
        ["members"] = e.GroupMembers is null ? null : new JsonArray(e.GroupMembers.Select(m => (JsonNode?)new JsonObject
        {
            ["source"] = m.Source,
            ["destination"] = m.Destination,
            ["size"] = m.Size,
            ["last_write_ticks"] = m.LastWriteUtc.Ticks,
            ["permanent"] = m.Permanent,
        }).ToArray()),
    };

    public static JsonArray RecoveryRetry(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var world = new W306World();
            world.AddDirs(W306JournalDir);
            foreach (var f in c["files"]?.AsArray() ?? new JsonArray())
                world.Write(f!["path"]!.GetValue<string>(), f["len"]!.GetValue<long>(), f["ticks"]!.GetValue<long>());
            foreach (var b in c["binned"]?.AsArray() ?? new JsonArray())
                world.Binned.Add((b!["path"]!.GetValue<string>(), new byte[b["len"]!.GetValue<long>()],
                    new DateTime(b["ticks"]!.GetValue<long>(), DateTimeKind.Utc)));
            world.Hooks = c["hooks"]?.AsArray() ?? new JsonArray();

            var fs = new W306Fs(world);
            var journalClock = new W306Clock(new DateTime(c["journal_clock_ticks"]!.GetValue<long>(), DateTimeKind.Utc));
            var serviceClock = new W306Clock(new DateTime(c["service_clock_ticks"]!.GetValue<long>(), DateTimeKind.Utc));
            var journal = new OperationJournal(new W306Paths(), fs, journalClock);
            var seed = c["seed"]!.AsArray().Select(W306Entry).ToList();
            foreach (var entry in seed) journal.Append(entry);
            var snapshot = seed[c["snapshot"]?.GetValue<int>() ?? 0];

            var binConfig = c["bin"] as JsonObject;
            var bin = binConfig is null ? null : new W306Bin(world, binConfig);
            Func<bool>? allow = c["allow_permanent"] is { } ap ? (ap.GetValue<bool>() ? () => true : () => false) : null;
            var gateMode = c["gate"]?.GetValue<string>() ?? "none";
            FileActionService? gate = gateMode == "none" ? null
                : new FileActionService(journal, fs, serviceClock, bin ?? new W306Bin(world, new JsonObject()));
            if (gateMode == "held" && !gate!.TryBegin()) throw new InvalidOperationException("gate");
            var service = new RecoveryRetryService(journal, fs, serviceClock, bin, allow, gate);

            world.Ops.Clear();
            world.Counting = true;
            if (c["cancel_before"]?.GetValue<bool>() == true) world.Cts.Cancel();
            var row = (JsonObject)c.DeepClone(); // golden = input + expected (Rust tests read the golden only)
            var marker = c["live"]?.GetValue<bool>() == true ? journal.LiveOperations.Begin(snapshot.Id) : null;
            try
            {
                var r = service.RetryMoveOrCopyAsync(snapshot, c["confirmed"]?.GetValue<bool>() ?? false, world.Cts.Token)
                    .GetAwaiter().GetResult();
                row["result"] = new JsonObject
                {
                    ["succeeded"] = r.Succeeded,
                    ["message"] = r.Message,
                    ["superseded"] = r.Superseded,
                    ["journal_persisted"] = r.JournalPersisted,
                    ["journal_error"] = r.JournalError,
                    ["entry"] = W306EntryJson(r.Entry),
                };
                row["threw"] = null;
            }
            catch (Exception ex)
            {
                row["result"] = null;
                row["threw"] = ex.GetType().Name;
            }
            finally
            {
                marker?.Dispose();
                world.Counting = false;
            }

            row["journal_text"] = world.Files.TryGetValue(W306JournalPath, out var jf) ? System.Text.Encoding.UTF8.GetString(jf.Data) : null;
            row["files_after"] = new JsonArray(world.Files.Values
                .Where(f => !W306IsJournalPath(f.Path))
                .OrderBy(f => f.Path, StringComparer.OrdinalIgnoreCase)
                .Select(f => (JsonNode?)new JsonObject { ["path"] = f.Path, ["len"] = f.Data.LongLength, ["ticks"] = f.LastWriteUtc.Ticks })
                .ToArray());
            row["binned_after"] = new JsonArray(world.Binned.Select(b => (JsonNode?)JsonValue.Create(b.Path)).ToArray());
            row["ops"] = new JsonArray(world.Ops.Select(o => (JsonNode?)JsonValue.Create(o)).ToArray());
            row["gate_busy_after"] = gate?.IsBusy;
            row["live_after"] = journal.LiveOperations.IsLive(snapshot.Id);
            result.Add(row);
        }
        return result;
    }
}
