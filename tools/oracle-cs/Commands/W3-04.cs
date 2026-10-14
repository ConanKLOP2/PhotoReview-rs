using System.Globalization;
using System.Text;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Model;

// W3-04 golden package: FileActionService.ExecuteAsync / ExecuteGroupAsync (C# d92d6aae,
// src/PhotoReview.Core/FileActions/FileActionService.cs) end to end: result records, the journal lines it writes (byte text),
// the disk afterwards, the Recycle Bin calls and the order of every mutating call.
// Copy to tools/oracle-cs/Commands/W3-04.cs and add this dispatch line to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W3-04
//     "file-action-scenarios" => Commands.FileActionScenarios(inputs),
// Inputs: tests/golden-inputs/file-action-scenarios.json (regenerate with contracts/drafts-w3/W3-04/tools/gen_golden_inputs.py).
// Self-contained (no W2Util). Runs on an in-memory disk and an in-memory bin: it never touches a real file, folder or the
// real Recycle Bin. Deterministic (D19): fixed clock, no cwd-dependent output, GUIDs replaced by <id1>, <id2>, ... in order of
// first appearance (journal lines first, then the result).

static partial class Commands
{
    private static readonly DateTime W304Now = new(2026, 10, 1, 0, 0, 0, DateTimeKind.Utc);
    private const string W304JournalFile = @"C:\data\operations.jsonl";
    private const string W304JournalDir = @"C:\data";

    private sealed class W304Clock : IClock
    {
        public DateTime UtcNow => W304Now;
    }

    private sealed class W304Paths : IAppPaths
    {
        public string ConfigFile => @"C:\data\config.json";
        public string JournalFile => W304JournalFile;
        public string SessionsDir => @"C:\data\Sessions";
        public string LogFile => @"C:\data\logs\app.log";
        public string PreviewCacheDir => @"C:\data\cache";
        public string ThumbnailCacheDir => @"C:\data\thumbnails";
        public string WindowPlacementFile => @"C:\data\window-placement.json";
    }

    /// <summary>
    /// The scenario's faults (all optional). Paths are matched OrdinalIgnoreCase. Every message below is part of the golden
    /// (it reaches FileActionResult.Error / the journal's Error), so the Rust ScenarioFs must raise exactly these texts.
    /// </summary>
    private sealed class W304Faults
    {
        public Dictionary<string, string> MoveFail = new(StringComparer.OrdinalIgnoreCase);       // source -> IOException(message)
        public HashSet<string> MoveLeavesSource = new(StringComparer.OrdinalIgnoreCase);          // source: copied, source kept, returns
        public Dictionary<string, int> MoveTruncate = new(StringComparer.OrdinalIgnoreCase);      // source -> destination keeps N bytes
        public bool MoveRound2s;                                                                    // destination stamp floored to 2 s (FAT)
        public Dictionary<string, string> CopyFail = new(StringComparer.OrdinalIgnoreCase);       // source -> IOException(message), nothing written
        public Dictionary<string, (int Bytes, string Message)> CopyPartial = new(StringComparer.OrdinalIgnoreCase); // writes N bytes, throws
        public Dictionary<string, int> CopySize = new(StringComparer.OrdinalIgnoreCase);          // completes with N bytes (stamp of the source)
        public HashSet<string> CopyThenCancel = new(StringComparer.OrdinalIgnoreCase);            // full copy written, then OperationCanceledException
        public Dictionary<string, int> ForeignOnCopy = new(StringComparer.OrdinalIgnoreCase);     // a foreign N-byte file appears at the destination, copy fails "exists"
        public Dictionary<string, string> DeleteFail = new(StringComparer.OrdinalIgnoreCase);     // path -> IOException(message)
        public HashSet<int> AppendFail = [];                                                       // 1-based OpenAppend call numbers that throw
        public HashSet<string> NoBinRoots = new(StringComparer.OrdinalIgnoreCase);                // CanRecycle false under these prefixes
        public Dictionary<string, long> BinCapacity = new(StringComparer.OrdinalIgnoreCase);      // root prefix -> FitsInRecycleBin(size <= cap)
        public Dictionary<string, string> RecycleFail = new(StringComparer.OrdinalIgnoreCase);
        public HashSet<string> RecycleLeaves = new(StringComparer.OrdinalIgnoreCase);
        public Dictionary<string, string> PermanentFail = new(StringComparer.OrdinalIgnoreCase);
        public HashSet<string> PermanentLeaves = new(StringComparer.OrdinalIgnoreCase);
        public (string Op, string Path)? CancelOn;                                                 // cancel the token when this call starts
    }

    private sealed class W304Disk(W304Faults faults, CancellationTokenSource cts) : IFileSystem
    {
        private sealed record Entry(string Path, byte[] Data, DateTime Stamp);

        private readonly Dictionary<string, Entry> _files = new(StringComparer.OrdinalIgnoreCase);
        private readonly Dictionary<string, string> _dirs = new(StringComparer.OrdinalIgnoreCase);
        public List<string> Trace { get; } = [];
        public int Appends { get; private set; }

        public static string Key(string path) => path.Length > 3 ? path.TrimEnd('\\', '/') : path;

        private static string? Parent(string path) => Path.GetDirectoryName(Key(path));

        public void AddDirs(string dir)
        {
            for (var d = (string?)Key(dir); !string.IsNullOrEmpty(d); d = Path.GetDirectoryName(d))
                _dirs.TryAdd(Key(d), Key(d));
        }

        public void AddFile(string path, int bytes, long ticks)
        {
            if (Parent(path) is { } parent) AddDirs(parent);
            _files[Key(path)] = new Entry(path, Enumerable.Range(0, bytes).Select(i => (byte)('a' + i % 26)).ToArray(),
                new DateTime(ticks, DateTimeKind.Utc));
        }

        private void Note(string op, string path)
        {
            if (string.Equals(path, W304JournalFile, StringComparison.OrdinalIgnoreCase)
                || string.Equals(Key(path), W304JournalDir, StringComparison.OrdinalIgnoreCase)) return;
            Trace.Add(op + ":" + path);
            if (faults.CancelOn is { } c && c.Op == op && string.Equals(c.Path, path, StringComparison.OrdinalIgnoreCase)) cts.Cancel();
        }

        public void TraceBin(string op, string path) => Note(op, path);

        public static IOException Exists(string destination) =>
            new($"The file '{destination}' already exists.", unchecked((int)0x800700B7));

        private static FileNotFoundException Missing(string path) => new($"Could not find file '{path}'.", path);

        private void RequireParent(string path)
        {
            if (Parent(path) is { } parent && !_dirs.ContainsKey(Key(parent)))
                throw new DirectoryNotFoundException($"Could not find a part of the path '{path}'.");
        }

        public bool FileExists(string path) => _files.ContainsKey(Key(path));

        public bool DirectoryExists(string path) => _dirs.ContainsKey(Key(path));

        public FileStat? GetFileStat(string path) =>
            _files.TryGetValue(Key(path), out var e) ? new FileStat(e.Data.LongLength, e.Stamp) : null;

        public void Move(string source, string destination)
        {
            Note("move", source);
            if (faults.MoveFail.TryGetValue(source, out var message)) throw new IOException(message);
            if (!_files.TryGetValue(Key(source), out var e)) throw Missing(source);
            if (_files.ContainsKey(Key(destination))) throw Exists(destination);
            RequireParent(destination);
            var data = faults.MoveTruncate.TryGetValue(source, out var keep) ? e.Data.Take(keep).ToArray() : e.Data;
            var stamp = faults.MoveRound2s ? new DateTime(e.Stamp.Ticks - e.Stamp.Ticks % (2 * TimeSpan.TicksPerSecond), DateTimeKind.Utc) : e.Stamp;
            if (!faults.MoveLeavesSource.Contains(source)) _files.Remove(Key(source));
            _files[Key(destination)] = new Entry(destination, data, stamp);
        }

        public void Copy(string source, string destination)
        {
            Note("copy", source);
            if (faults.CopyFail.TryGetValue(source, out var message)) throw new IOException(message);
            if (!_files.TryGetValue(Key(source), out var e)) throw Missing(source);
            if (faults.ForeignOnCopy.TryGetValue(source, out var foreign))
            {
                _files[Key(destination)] = new Entry(destination, new byte[foreign], W304Now);
                throw Exists(destination);
            }
            if (_files.ContainsKey(Key(destination))) throw Exists(destination);
            RequireParent(destination);
            if (faults.CopyPartial.TryGetValue(source, out var partial))
            {
                _files[Key(destination)] = new Entry(destination, e.Data.Take(partial.Bytes).ToArray(), W304Now);
                throw new IOException(partial.Message);
            }
            if (faults.CopyThenCancel.Contains(source))
            {
                _files[Key(destination)] = new Entry(destination, e.Data.ToArray(), e.Stamp);
                throw new OperationCanceledException();
            }
            var data = faults.CopySize.TryGetValue(source, out var size) ? new byte[size] : e.Data.ToArray();
            _files[Key(destination)] = new Entry(destination, data, e.Stamp);
        }

        // TryCopyNew(source, destination, proof) is NOT overridden: the interface default (FileExists, Copy, MarkCreated) is the
        // behaviour the Rust trait default reproduces.

        public void Delete(string path)
        {
            Note("delete", path);
            if (faults.DeleteFail.TryGetValue(path, out var message)) throw new IOException(message);
            _files.Remove(Key(path));
        }

        public void Remove(string path) => _files.Remove(Key(path));

        public Stream OpenReadShared(string path, int bufferSize = 65536) =>
            _files.TryGetValue(Key(path), out var e) ? new MemoryStream(e.Data, writable: false) : throw Missing(path);

        public Stream OpenAppend(string path, bool durable)
        {
            Appends++;
            Trace.Add("append");
            if (faults.AppendFail.Contains(Appends)) throw new IOException("journal unavailable");
            RequireParent(path);
            if (!_files.ContainsKey(Key(path))) _files[Key(path)] = new Entry(path, [], W304Now);
            return new AppendStream(this, path);
        }

        private sealed class AppendStream(W304Disk disk, string path) : Stream
        {
            public override bool CanRead => false;
            public override bool CanSeek => false;
            public override bool CanWrite => true;
            public override long Length => disk._files[Key(path)].Data.LongLength;
            public override long Position { get => Length; set => throw new NotSupportedException(); }
            public override void Flush() { }
            public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
            public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
            public override void SetLength(long value) => throw new NotSupportedException();

            public override void Write(byte[] buffer, int offset, int count)
            {
                var e = disk._files.TryGetValue(Key(path), out var found) ? found : new Entry(path, [], W304Now);
                disk._files[Key(path)] = e with { Data = [.. e.Data, .. buffer.AsSpan(offset, count).ToArray()], Stamp = W304Now };
            }
        }

        public void WriteAllTextAtomic(string path, string text, bool durable = true) => throw new NotSupportedException("not used by W3-04");

        public string ReadAllText(string path) =>
            _files.TryGetValue(Key(path), out var e) ? Encoding.UTF8.GetString(e.Data) : throw Missing(path);

        public IEnumerable<string> EnumerateFiles(string directory, string pattern = "*") =>
            _files.Values.Where(e => string.Equals(Parent(e.Path), Key(directory), StringComparison.OrdinalIgnoreCase))
                .Select(e => e.Path).ToList();

        public IEnumerable<string> EnumerateDirectories(string directory) =>
            _dirs.Values.Where(d => string.Equals(Parent(d), Key(directory), StringComparison.OrdinalIgnoreCase)).ToList();

        public void CreateDirectory(string path)
        {
            Note("mkdir", Key(path));
            if (_files.ContainsKey(Key(path))) throw new IOException($"Cannot create '{path}' because a file with the same name already exists.");
            AddDirs(path);
        }

        public bool TryDeleteEmptyDirectory(string path)
        {
            Note("rmdir", Key(path));
            var key = Key(path);
            if (!_dirs.ContainsKey(key)) return false;
            if (_files.Values.Any(e => string.Equals(Parent(e.Path), key, StringComparison.OrdinalIgnoreCase))) return false;
            if (_dirs.Values.Any(d => !string.Equals(d, key, StringComparison.OrdinalIgnoreCase)
                && string.Equals(Parent(d), key, StringComparison.OrdinalIgnoreCase))) return false;
            _dirs.Remove(key);
            return true;
        }

        public JsonArray FilesJson() => new(_files.Values
            .Where(e => !string.Equals(e.Path, W304JournalFile, StringComparison.OrdinalIgnoreCase))
            .OrderBy(e => e.Path, StringComparer.OrdinalIgnoreCase)
            .Select(e => (JsonNode?)new JsonObject
            {
                ["path"] = e.Path,
                ["size"] = e.Data.LongLength,
                ["ticks"] = e.Stamp.Ticks.ToString(CultureInfo.InvariantCulture),
            }).ToArray());

        public string JournalText() =>
            _files.TryGetValue(Key(W304JournalFile), out var e) ? Encoding.UTF8.GetString(e.Data) : string.Empty;
    }

    private sealed class W304Bin(W304Faults faults, W304Disk disk) : IRecycleBin
    {
        public List<string> Recycled { get; } = [];
        public List<string> PermanentlyDeleted { get; } = [];
        public JsonArray FitsCalls { get; } = [];

        public bool CanRecycle(string path) => !faults.NoBinRoots.Any(root => path.StartsWith(root, StringComparison.OrdinalIgnoreCase));

        public bool FitsInRecycleBin(string path, long fileSize)
        {
            FitsCalls.Add(new JsonObject { ["path"] = path, ["size"] = fileSize });
            foreach (var (root, capacity) in faults.BinCapacity)
                if (path.StartsWith(root, StringComparison.OrdinalIgnoreCase)) return fileSize <= capacity;
            return true;
        }

        public void SendToRecycleBin(string path)
        {
            disk.TraceBin("recycle", path);
            if (faults.RecycleFail.TryGetValue(path, out var message)) throw new IOException(message);
            if (faults.RecycleLeaves.Contains(path)) return; // the shell reports success, the file stays (B-06)
            if (!disk.FileExists(path)) throw new FileNotFoundException($"Could not find file '{path}'.", path);
            disk.Remove(path);
            Recycled.Add(path);
        }

        public void DeletePermanently(string path)
        {
            disk.TraceBin("permanent", path);
            if (faults.PermanentFail.TryGetValue(path, out var message)) throw new IOException(message);
            if (faults.PermanentLeaves.Contains(path)) return;
            disk.Remove(path);
            PermanentlyDeleted.Add(path);
        }

        public bool TryRestore(string originalPath, long expectedSize, DateTime expectedLastWriteUtc) => false;
    }

    private sealed class W304MoveOverride(W304Disk disk, string kind)
    {
        public Task Run(string from, string to)
        {
            disk.Move(from, to);
            throw kind switch
            {
                "move-then-cancel" => new OperationCanceledException(),
                "move-then-fail" => new IOException("override failed after the move"),
                _ => new ArgumentException("unknown move_override " + kind),
            };
        }
    }

    private static IEnumerable<JsonNode?> W304Items(JsonNode? n) => n is null ? Enumerable.Empty<JsonNode?>() : n.AsArray();

    private static W304Faults W304ParseFaults(JsonObject? f)
    {
        var faults = new W304Faults();
        if (f is null) return faults;
        static IEnumerable<KeyValuePair<string, JsonNode?>> Obj(JsonNode? n) =>
            n is null ? Enumerable.Empty<KeyValuePair<string, JsonNode?>>() : n.AsObject();
        static IEnumerable<string> Arr(JsonNode? n) => W304Items(n).Select(x => x!.GetValue<string>());
        foreach (var (k, v) in Obj(f["move_fail"])) faults.MoveFail[k] = v!.GetValue<string>();
        foreach (var p in Arr(f["move_leaves_source"])) faults.MoveLeavesSource.Add(p);
        foreach (var (k, v) in Obj(f["move_truncate"])) faults.MoveTruncate[k] = v!.GetValue<int>();
        faults.MoveRound2s = f["move_round_2s"]?.GetValue<bool>() ?? false;
        foreach (var (k, v) in Obj(f["copy_fail"])) faults.CopyFail[k] = v!.GetValue<string>();
        foreach (var (k, v) in Obj(f["copy_partial"])) faults.CopyPartial[k] = (v!["bytes"]!.GetValue<int>(), v["message"]!.GetValue<string>());
        foreach (var (k, v) in Obj(f["copy_size"])) faults.CopySize[k] = v!.GetValue<int>();
        foreach (var p in Arr(f["copy_then_cancel"])) faults.CopyThenCancel.Add(p);
        foreach (var (k, v) in Obj(f["foreign_on_copy"])) faults.ForeignOnCopy[k] = v!.GetValue<int>();
        foreach (var (k, v) in Obj(f["delete_fail"])) faults.DeleteFail[k] = v!.GetValue<string>();
        foreach (var n in W304Items(f["append_fail"])) faults.AppendFail.Add(n!.GetValue<int>());
        foreach (var p in Arr(f["no_bin_roots"])) faults.NoBinRoots.Add(p);
        foreach (var (k, v) in Obj(f["bin_capacity"])) faults.BinCapacity[k] = v!.GetValue<long>();
        foreach (var (k, v) in Obj(f["recycle_fail"])) faults.RecycleFail[k] = v!.GetValue<string>();
        foreach (var p in Arr(f["recycle_leaves"])) faults.RecycleLeaves.Add(p);
        foreach (var (k, v) in Obj(f["permanent_fail"])) faults.PermanentFail[k] = v!.GetValue<string>();
        foreach (var p in Arr(f["permanent_leaves"])) faults.PermanentLeaves.Add(p);
        if (f["cancel_on"] is JsonObject c) faults.CancelOn = (c["op"]!.GetValue<string>(), c["path"]!.GetValue<string>());
        return faults;
    }

    /// <summary>
    /// FileActionService on an in-memory disk for each case {name, files:[{path, bytes, ticks}], dirs?:[...], request:{kind:
    /// "single"|"group", op, source | jpeg/raw/xmp, destination?, allow_permanent?}, faults?:{...}, pre_cancelled?, move_override?,
    /// probe_dirs?:[...]}. Output: the result (every field), the journal lines, the disk and bin afterwards, the capacity
    /// questions asked, the ordered trace of mutating calls, and whether the gate is still held.
    /// </summary>
    public static JsonArray FileActionScenarios(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var faults = W304ParseFaults(c["faults"] as JsonObject);
            using var cts = new CancellationTokenSource();
            var disk = new W304Disk(faults, cts);
            disk.AddDirs(W304JournalDir);
            foreach (var d in W304Items(c["dirs"])) disk.AddDirs(d!.GetValue<string>());
            foreach (var f in c["files"]!.AsArray())
                disk.AddFile(f!["path"]!.GetValue<string>(), f["bytes"]!.GetValue<int>(),
                    long.Parse(f["ticks"]!.GetValue<string>(), CultureInfo.InvariantCulture));
            var bin = new W304Bin(faults, disk);
            var clock = new W304Clock();
            var journal = new OperationJournal(new W304Paths(), disk, clock);
            Func<string, string, Task>? moveOverride = c["move_override"]?.GetValue<string>() is { } kind
                ? (Func<string, string, Task>)new W304MoveOverride(disk, kind).Run
                : null;
            var service = new FileActionService(journal, disk, clock, bin, moveOverride);
            if (c["pre_cancelled"]?.GetValue<bool>() == true) cts.Cancel();

            var r = c["request"]!.AsObject();
            var op = Enum.Parse<FileOperationType>(r["op"]!.GetValue<string>());
            var destination = r["destination"]?.GetValue<string>();
            var allowPermanent = r["allow_permanent"]?.GetValue<bool>() ?? false;

            JsonObject outcome;
            var ids = new Dictionary<string, string>(StringComparer.Ordinal);
            string? Norm(string? s) => s is null ? null : Regex.Replace(s, "[0-9a-f]{32}", m =>
            {
                if (!ids.TryGetValue(m.Value, out var id)) ids[m.Value] = id = "<id" + (ids.Count + 1).ToString(CultureInfo.InvariantCulture) + ">";
                return id;
            });

            object raw;
            if (r["kind"]!.GetValue<string>() == "single")
            {
                raw = service.ExecuteAsync(new FileActionRequest(r["source"]!.GetValue<string>(), op, destination, allowPermanent), cts.Token)
                    .GetAwaiter().GetResult();
            }
            else
            {
                var group = new CaptureGroup(r["jpeg"]!.GetValue<string>(), r["raw"]!.GetValue<string>(), r["xmp"]?.GetValue<string>());
                raw = service.ExecuteGroupAsync(new CaptureGroupActionRequest(group, op, destination, allowPermanent), cts.Token)
                    .GetAwaiter().GetResult();
            }

            // Journal lines first, so <idN> numbering follows the file.
            var text = disk.JournalText();
            var lines = new JsonArray(text.Split("\r\n").Where(l => l.Length > 0).Select(l => (JsonNode?)JsonValue.Create(Norm(l))).ToArray());
            var journalEndsWithCrLf = text.Length == 0 || text.EndsWith("\r\n", StringComparison.Ordinal);

            if (raw is FileActionResult s)
            {
                outcome = new JsonObject
                {
                    ["succeeded"] = s.Succeeded,
                    ["rejected"] = s.Rejected,
                    ["operation"] = s.Operation.ToString(),
                    ["source"] = s.Source,
                    ["destination_path"] = s.DestinationPath,
                    ["size"] = s.Size,
                    ["last_write_ticks"] = s.LastWriteUtc.Ticks.ToString(CultureInfo.InvariantCulture),
                    ["error"] = s.Error,
                    ["journal_persisted"] = s.JournalPersisted,
                    ["journal_error"] = s.JournalError,
                    ["permanently_deleted"] = s.PermanentlyDeleted,
                    ["source_removed"] = s.SourceRemoved,
                };
            }
            else
            {
                var g = (CaptureGroupActionResult)raw;
                outcome = new JsonObject
                {
                    ["succeeded"] = g.Succeeded,
                    ["rejected"] = g.Rejected,
                    ["operation"] = g.Operation.ToString(),
                    ["group_id"] = Norm(g.GroupId),
                    ["entry"] = g.Entry is null ? null : new JsonObject
                    {
                        ["id"] = Norm(g.Entry.Id),
                        ["state"] = g.Entry.State.ToString(),
                        ["source"] = g.Entry.Source,
                        ["destination"] = g.Entry.Destination,
                        ["error"] = g.Entry.Error,
                        ["error_code"] = g.Entry.ErrorCode,
                        ["permanent"] = g.Entry.Permanent,
                        ["group_id"] = Norm(g.Entry.GroupId),
                        ["member_count"] = g.Entry.GroupMembers?.Count,
                    },
                    ["members"] = new JsonArray(g.Members.Select(m => (JsonNode?)new JsonObject
                    {
                        ["source"] = m.Member.Source,
                        ["destination"] = m.Member.Destination,
                        ["size"] = m.Member.Size,
                        ["last_write_ticks"] = m.Member.LastWriteUtc.Ticks.ToString(CultureInfo.InvariantCulture),
                        ["permanent"] = m.Member.Permanent,
                        ["completed"] = m.Completed,
                        ["conflict"] = m.Conflict,
                        ["error"] = m.Error,
                        ["source_exists"] = m.SourceExists,
                        ["destination_exists"] = m.DestinationExists,
                        ["state_known"] = m.StateKnown,
                    }).ToArray()),
                    ["error"] = g.Error,
                    ["journal_persisted"] = g.JournalPersisted,
                    ["journal_error"] = g.JournalError,
                    ["permanently_deleted"] = g.PermanentlyDeleted,
                    ["skipped_missing"] = g.SkippedMissing is null
                        ? null
                        : new JsonArray(g.SkippedMissing.Select(p => (JsonNode?)JsonValue.Create(p)).ToArray()),
                };
            }

            var copy = c.DeepClone().AsObject();
            copy["expected"] = new JsonObject
            {
                ["result"] = outcome,
                ["journal_lines"] = lines,
                ["journal_ends_with_crlf"] = journalEndsWithCrLf,
                ["files_after"] = disk.FilesJson(),
                ["dirs_after"] = new JsonArray(W304Items(c["probe_dirs"]).Select(d => (JsonNode?)new JsonObject
                {
                    ["path"] = d!.GetValue<string>(),
                    ["exists"] = disk.DirectoryExists(d.GetValue<string>()),
                }).ToArray()),
                ["recycled"] = new JsonArray(bin.Recycled.Select(p => (JsonNode?)JsonValue.Create(p)).ToArray()),
                ["permanently_deleted_paths"] = new JsonArray(bin.PermanentlyDeleted.Select(p => (JsonNode?)JsonValue.Create(p)).ToArray()),
                ["fits_calls"] = bin.FitsCalls.DeepClone(),
                ["trace"] = new JsonArray(disk.Trace.Select(t => (JsonNode?)JsonValue.Create(t)).ToArray()),
                ["busy_after"] = service.IsBusy,
                ["token_cancelled"] = cts.IsCancellationRequested,
            };
            result.Add(copy);
        }
        return result;
    }
}
