using System.Globalization;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.IO;
using PhotoReview.Core.Model;

// W3-01 golden package: OperationJournal (append, reads, Dismiss, AppendIfUnchangedSince, ReadLatestEntry, TryCompact) and
// JournalTransaction (C# d92d6aae). Copy to tools/oracle-cs/Commands/W3-01.cs and add these dispatch lines to the
// `command switch` in tools/oracle-cs/Program.cs (before the `_ =>` line):
//     // W3-01
//     "journal-stream-lines" => Commands.JournalStreamLines(inputs),
//     "journal-ops" => Commands.JournalOps(inputs),
//     "journal-committed-moves" => Commands.JournalCommittedMoves(inputs),
//     "journal-compact" => Commands.JournalCompact(inputs),
//     "journal-transaction" => Commands.JournalTransactionCases(inputs),
// Needs W2-00.cs (class W2Util). Inputs: tests/golden-inputs/<command>.json.
//
// Determinism (D19): every clock is a settable fake starting at 2026-09-25T00:00:00Z; nothing reported depends on the temp
// path, the process directory or file times. Journals live in a fresh temp folder per case (real PhysicalFileSystem, real
// PhysicalJournalCompactionFiles); only their BYTES are reported, so the Rust tests replay every case on FakeFileSystem.
//
// Internal C# members are reached by reflection: JournalLineParser.TryParse(string) (to turn an input line into a
// JournalEntry exactly as the journal's own reader does), OperationJournal.AppendIfUnchangedSince / ReadLatestEntry, and the
// whole internal JournalTransaction class.
//
// Byte specs (an element of "initial", "lines", "append_raw"... ):
//   {"text": s, "eol": e}  UTF-8 of s followed by e (default "\r\n"; "" = no terminator)
//   {"hex": h}             raw bytes, nothing appended
// Entry spec (an "entry", "anchor", element of "entries"/"own_since", "prepared", "retry_of"): a journal line TEXT, parsed
//   with JournalLineParser.TryParse(string); it must be accepted.
// Generated journal (journal-committed-moves, journal-compact): "segments" concatenated in order, then "pad_to" (optional):
//   {"entries": [from, to], "type": "Move"|"Copy"|"Recycle" (default "Move"), "move_every": k (optional: line i is a Move when
//      i % k == 0, else a Recycle; overrides "type"), "states": [..] (default ["Committed"]; one line per state per i, in that
//      order), "pad": p (default 0), "eol": e (default "\r\n")}
//      line = JsonSerializer.Serialize(new JournalEntry(Id: "e" + i:D6, type, state, Source: "C:\photos\" + (p > 0 ? new string('x', p)
//      + "\" : "") + "e" + i:D6 + ".jpg", Destination: Recycle ? null : "C:\photos\sel\e" + i:D6 + ".jpg", Size: 1, LastWriteUtc: base,
//      TimestampUtc: base + (i * 10 + stateIndex) s)) + eol
//   {"long_move": n, "id": s (default "big"), "eol": e (default "")}: JsonSerializer.Serialize(new JournalEntry(s, Move, Committed,
//      "C:\photos\" + new string('a', n) + ".jpg", "C:\sel\" + s + ".jpg", 4, base, base)) + eol
//   {"repeat_text": s, "count": n, "eol": e (default "")}: s repeated n times, then e
//   {"text": .., "eol": ..} and {"hex": ..} as above.
//   "pad_to": N  -> a malformed first line of 'x' bytes ending in "\r\n" is PREPENDED so that the file is exactly N bytes long
//                   (N - rest >= 3, else the case fails).
// FNV-1a 64 (offset 0xcbf29ce484222325, prime 0x100000001b3, lowercase 16 hex digits) fingerprints big outputs.
static partial class Commands
{
    private static readonly DateTime W301Base = new(2026, 9, 25, 0, 0, 0, DateTimeKind.Utc);

    private sealed class W301Clock : IClock
    {
        public DateTime UtcNow { get; set; } = W301Base;
    }

    private sealed class W301Paths(string journal) : IAppPaths
    {
        public string ConfigFile => @"C:\data\config.json";
        public string JournalFile => journal;
        public string SessionsDir => @"C:\data\Sessions";
        public string LogFile => @"C:\data\logs\app.log";
        public string PreviewCacheDir => @"C:\data\cache";
        public string ThumbnailCacheDir => @"C:\data\thumbnails";
        public string WindowPlacementFile => @"C:\data\window-placement.json";
    }

    /// <summary>The real file system; the next <see cref="FailAppends"/> OpenAppend calls throw IOException("disk full") (not a
    /// sharing violation, so the journal does not retry). Counts OpenAppend calls.</summary>
    private sealed class W301Fs : IFileSystem
    {
        private readonly PhysicalFileSystem _inner = new();
        public int FailAppends { get; set; }
        public int Appends { get; private set; }

        public bool FileExists(string path) => _inner.FileExists(path);
        public bool DirectoryExists(string path) => _inner.DirectoryExists(path);
        public FileStat? GetFileStat(string path) => _inner.GetFileStat(path);
        public void Move(string source, string destination) => _inner.Move(source, destination);
        public void Copy(string source, string destination) => _inner.Copy(source, destination);
        public void Delete(string path) => _inner.Delete(path);
        public Stream OpenReadShared(string path, int bufferSize = 65536) => _inner.OpenReadShared(path, bufferSize);

        public Stream OpenAppend(string path, bool durable)
        {
            Appends++;
            if (FailAppends > 0)
            {
                FailAppends--;
                throw new IOException("disk full");
            }
            return _inner.OpenAppend(path, durable);
        }

        public void WriteAllTextAtomic(string path, string text, bool durable = true) => _inner.WriteAllTextAtomic(path, text, durable);
        public string ReadAllText(string path) => _inner.ReadAllText(path);
        public IEnumerable<string> EnumerateFiles(string directory, string pattern = "*") => _inner.EnumerateFiles(directory, pattern);
        public IEnumerable<string> EnumerateDirectories(string directory) => _inner.EnumerateDirectories(directory);
        public void CreateDirectory(string path) => _inner.CreateDirectory(path);
    }

    /// <summary>The real compaction primitives with a hook at the start of OpenReadDenyWriters (the moment the journal is locked).</summary>
    private sealed class W301HookedFiles(IJournalCompactionFiles inner) : IJournalCompactionFiles
    {
        public Action? BeforeLock { get; set; }

        public Stream OpenReadDenyWriters(string path)
        {
            var hook = BeforeLock;
            BeforeLock = null;
            hook?.Invoke();
            return inner.OpenReadDenyWriters(path);
        }

        public IStagedReplacement CreateStagedReplacement(string tempPath) => inner.CreateStagedReplacement(tempPath);
    }

    private static Func<string, JournalEntry?>? s_w301Parse;

    private static JournalEntry W301Entry(JsonNode? node)
    {
        if (s_w301Parse is null)
        {
            var parser = typeof(JournalEntry).Assembly.GetType("PhotoReview.Core.FileActions.JournalLineParser", throwOnError: true)!;
            var method = parser.GetMethod("TryParse", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static, [typeof(string)])
                ?? throw new InvalidOperationException("JournalLineParser.TryParse(string) not found");
            s_w301Parse = (Func<string, JournalEntry?>)Delegate.CreateDelegate(typeof(Func<string, JournalEntry?>), method);
        }
        var text = node!.GetValue<string>();
        return s_w301Parse(text) ?? throw new ArgumentException("input is not an accepted journal line: " + text);
    }

    private static JsonNode? W301Line(JournalEntry? entry) => entry is null ? null : JsonValue.Create(JsonSerializer.Serialize(entry));

    private static JsonArray W301Lines(IEnumerable<JournalEntry> entries) => new(entries.Select(W301Line).ToArray());

    private static byte[] W301Spec(JsonObject spec)
    {
        if (spec["hex"] is { } hex) return Convert.FromHexString(hex.GetValue<string>());
        var eol = spec["eol"]?.GetValue<string>() ?? "\r\n";
        return Encoding.UTF8.GetBytes(spec["text"]!.GetValue<string>() + eol);
    }

    private static byte[] W301Specs(JsonNode? specs)
    {
        using var ms = new MemoryStream();
        if (specs is JsonArray array)
        {
            foreach (var spec in array) ms.Write(W301Spec(spec!.AsObject()));
        }
        return ms.ToArray();
    }

    private static string W301Id(int i) => "e" + i.ToString("D6", CultureInfo.InvariantCulture);

    private static byte[] W301Build(JsonObject c)
    {
        using var ms = new MemoryStream();
        foreach (var node in c["segments"]!.AsArray())
        {
            var seg = node!.AsObject();
            if (seg["entries"] is JsonArray range)
            {
                var from = range[0]!.GetValue<int>();
                var to = range[1]!.GetValue<int>();
                var every = seg["move_every"]?.GetValue<int>();
                var fixedType = Enum.Parse<FileOperationType>(seg["type"]?.GetValue<string>() ?? "Move");
                var states = seg["states"] is JsonArray s ? s.Select(x => Enum.Parse<JournalState>(x!.GetValue<string>())).ToList() : [JournalState.Committed];
                var pad = seg["pad"]?.GetValue<int>() ?? 0;
                var eol = seg["eol"]?.GetValue<string>() ?? "\r\n";
                var folder = pad > 0 ? new string('x', pad) + @"\" : "";
                for (var i = from; i < to; i++)
                {
                    var type = every is { } k ? (i % k == 0 ? FileOperationType.Move : FileOperationType.Recycle) : fixedType;
                    var id = W301Id(i);
                    for (var si = 0; si < states.Count; si++)
                    {
                        var entry = new JournalEntry(id, type, states[si], @"C:\photos\" + folder + id + ".jpg",
                            type == FileOperationType.Recycle ? null : @"C:\photos\sel\" + id + ".jpg", 1, W301Base,
                            W301Base.AddSeconds(i * 10 + si));
                        ms.Write(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(entry) + eol));
                    }
                }
            }
            else if (seg["repeat_text"] is { } repeat)
            {
                var text = string.Concat(Enumerable.Repeat(repeat.GetValue<string>(), seg["count"]!.GetValue<int>()));
                ms.Write(Encoding.UTF8.GetBytes(text + (seg["eol"]?.GetValue<string>() ?? "")));
            }
            else if (seg["long_move"] is { } longMove)
            {
                var id = seg["id"]?.GetValue<string>() ?? "big";
                var entry = new JournalEntry(id, FileOperationType.Move, JournalState.Committed,
                    @"C:\photos\" + new string('a', longMove.GetValue<int>()) + ".jpg", @"C:\sel\" + id + ".jpg", 4, W301Base, W301Base);
                ms.Write(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(entry) + (seg["eol"]?.GetValue<string>() ?? "")));
            }
            else
            {
                ms.Write(W301Spec(seg));
            }
        }
        var rest = ms.ToArray();
        if (c["pad_to"] is not { } padTo) return rest;
        var padLength = padTo.GetValue<int>() - rest.Length;
        if (padLength < 3) throw new InvalidOperationException("pad_to is too small for the segments");
        var all = new byte[padLength + rest.Length];
        Array.Fill(all, (byte)'x', 0, padLength - 2);
        all[padLength - 2] = (byte)'\r';
        all[padLength - 1] = (byte)'\n';
        rest.CopyTo(all, padLength);
        return all;
    }

    private static string W301Fnv(byte[] bytes)
    {
        unchecked
        {
            var hash = 0xcbf29ce484222325UL;
            foreach (var b in bytes)
            {
                hash ^= b;
                hash *= 0x100000001b3UL;
            }
            return hash.ToString("x16", CultureInfo.InvariantCulture);
        }
    }

    private static string W301Temp() => Path.Combine(Path.GetTempPath(), "pr-oracle-journal-" + Guid.NewGuid().ToString("N"));

    private static JsonObject W301Throws(Exception ex)
    {
        while (ex is TargetInvocationException { InnerException: { } inner }) ex = inner;
        var o = new JsonObject { ["throws"] = ex.GetType().Name };
        if (ex is JournalCodedException coded) o["code"] = coded.Code;
        return o;
    }

    /// <summary>
    /// The C# full reader's line rules: `new StreamReader(new MemoryStream(bytes), Encoding.UTF8, detectEncodingFromByteOrderMarks:
    /// true)` + ReadLine() until null, exactly as OperationJournal.ReadEntries reads the file. Case {name, hex}. Each line is
    /// reported as its UTF-16 code units (4 uppercase hex digits each) and, when it is well-formed UTF-16, as text.
    /// </summary>
    public static JsonArray JournalStreamLines(JsonObject inputs)
    {
        var strict = new UTF8Encoding(false, true);
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var bytes = Convert.FromHexString(c["hex"]!.GetValue<string>());
            var lines = new List<string>();
            using (var reader = new StreamReader(new MemoryStream(bytes), Encoding.UTF8, detectEncodingFromByteOrderMarks: true))
            {
                while (reader.ReadLine() is { } line) lines.Add(line);
            }
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["hex"] = c["hex"]!.GetValue<string>(),
                ["lines_utf16"] = new JsonArray(lines.Select(l => (JsonNode?)JsonValue.Create(
                    string.Concat(l.Select(ch => ((int)ch).ToString("X4", CultureInfo.InvariantCulture))))).ToArray()),
                ["lines"] = new JsonArray(lines.Select(l =>
                {
                    try
                    {
                        strict.GetBytes(l);
                        return (JsonNode?)JsonValue.Create(l);
                    }
                    catch (EncoderFallbackException)
                    {
                        return null;
                    }
                }).ToArray()),
            });
        }
        return result;
    }

    /// <summary>
    /// A script of OperationJournal calls on one journal file. Case {name, initial?: [byte specs] (absent = no file and no folder),
    /// ops: [...]}. Every op may carry "now": seconds after the base (sets the journal clock first) and "fail_append": n (the next n
    /// OpenAppend calls of THIS op throw IOException("disk full")). Ops:
    ///   {"op":"append", "entry"}                                   -> {} or {"throws"}
    ///   {"op":"append_raw", "bytes": [byte specs]}                 another process appends raw bytes (FileShare.Read, like the app)
    ///   {"op":"dismiss", "entries": [..]}                          -> {"dismissed":[lines], "skipped":[lines]}
    ///   {"op":"read_committed_moves"}                              -> {"entries":[lines]}
    ///   {"op":"read_pending_and_failed"}                           -> {"entries":[lines]}
    ///   {"op":"read_latest", "id"}                                 -> {"entry": line|null}
    ///   {"op":"append_if_unchanged_since", "anchor", "own_since":[..], "entry"} -> {"appended": bool}
    /// Every step also reports "append_opens" (OpenAppend calls made by the op). After the script: "final_exists", "final_hex".
    /// </summary>
    public static JsonArray JournalOps(JsonObject inputs)
    {
        var appendIfUnchangedSince = typeof(OperationJournal).GetMethod("AppendIfUnchangedSince", BindingFlags.Instance | BindingFlags.NonPublic)
            ?? throw new InvalidOperationException("OperationJournal.AppendIfUnchangedSince not found");
        var readLatestEntry = typeof(OperationJournal).GetMethod("ReadLatestEntry", BindingFlags.Instance | BindingFlags.NonPublic)
            ?? throw new InvalidOperationException("OperationJournal.ReadLatestEntry not found");
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = W301Temp();
            Directory.CreateDirectory(root);
            try
            {
                var dir = Path.Combine(root, "data");
                var path = Path.Combine(dir, "operations.jsonl");
                if (c["initial"] is JsonArray initial)
                {
                    Directory.CreateDirectory(dir);
                    File.WriteAllBytes(path, W301Specs(initial));
                }
                var clock = new W301Clock();
                var fs = new W301Fs();
                var journal = new OperationJournal(new W301Paths(path), fs, clock, appendRetryDelay: _ => { });
                var steps = new JsonArray();
                foreach (var opNode in c["ops"]!.AsArray())
                {
                    var op = opNode!.AsObject();
                    if (op["now"] is { } now) clock.UtcNow = W301Base.AddSeconds(now.GetValue<double>());
                    fs.FailAppends = op["fail_append"]?.GetValue<int>() ?? 0;
                    var opensBefore = fs.Appends;
                    var kind = op["op"]!.GetValue<string>();
                    JsonObject step;
                    try
                    {
                        switch (kind)
                        {
                            case "append":
                                journal.Append(W301Entry(op["entry"]));
                                step = new JsonObject();
                                break;
                            case "append_raw":
                                Directory.CreateDirectory(dir);
                                using (var other = new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.Read))
                                    other.Write(W301Specs(op["bytes"]));
                                step = new JsonObject();
                                break;
                            case "dismiss":
                                var outcome = journal.Dismiss(op["entries"]!.AsArray().Select(W301Entry).ToList());
                                step = new JsonObject { ["dismissed"] = W301Lines(outcome.Dismissed), ["skipped"] = W301Lines(outcome.Skipped) };
                                break;
                            case "read_committed_moves":
                                step = new JsonObject { ["entries"] = W301Lines(journal.ReadCommittedMoves()) };
                                break;
                            case "read_pending_and_failed":
                                step = new JsonObject { ["entries"] = W301Lines(journal.ReadPendingAndFailedOperations()) };
                                break;
                            case "read_latest":
                                step = new JsonObject
                                {
                                    ["entry"] = W301Line((JournalEntry?)readLatestEntry.Invoke(journal, [op["id"]!.GetValue<string>()])),
                                };
                                break;
                            case "append_if_unchanged_since":
                                var ownSince = op["own_since"]!.AsArray().Select(W301Entry).ToList();
                                var appended = (bool)appendIfUnchangedSince.Invoke(journal,
                                    [W301Entry(op["anchor"]), (IReadOnlyList<JournalEntry>)ownSince, W301Entry(op["entry"])])!;
                                step = new JsonObject { ["appended"] = appended };
                                break;
                            default:
                                throw new ArgumentException("unknown op " + kind);
                        }
                    }
                    catch (Exception ex) when (ex is not ArgumentException)
                    {
                        step = W301Throws(ex);
                    }
                    fs.FailAppends = 0;
                    step["op"] = kind;
                    step["append_opens"] = fs.Appends - opensBefore;
                    steps.Add(step);
                }
                var exists = File.Exists(path);
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["initial"] = c["initial"]?.DeepClone(),
                    ["ops"] = c["ops"]!.DeepClone(),
                    ["steps"] = steps,
                    ["final_exists"] = exists,
                    ["final_hex"] = exists ? Convert.ToHexString(File.ReadAllBytes(path)) : null,
                });
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
            }
        }
        return result;
    }

    /// <summary>
    /// ReadCommittedMoves on a generated journal (see "Generated journal" above). Reports the file length, which reader applies
    /// (length >= 1 MiB: "tail"), the number of committed Moves returned and their Ids in order.
    /// </summary>
    public static JsonArray JournalCommittedMoves(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = W301Temp();
            Directory.CreateDirectory(root);
            try
            {
                var path = Path.Combine(root, "operations.jsonl");
                var bytes = W301Build(c);
                File.WriteAllBytes(path, bytes);
                var moves = new OperationJournal(new W301Paths(path), new PhysicalFileSystem(), new W301Clock()).ReadCommittedMoves();
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["segments"] = c["segments"]!.DeepClone(),
                    ["pad_to"] = c["pad_to"]?.DeepClone(),
                    ["length"] = bytes.Length,
                    ["fnv64"] = W301Fnv(bytes),
                    ["reader"] = bytes.Length >= 1024 * 1024 ? "tail" : "full",
                    ["count"] = moves.Count,
                    ["ids"] = W2Util.StrArray(moves.Select(m => m.Id)),
                    ["last_line"] = moves.Count == 0 ? null : JsonSerializer.Serialize(moves[^1]),
                });
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
            }
        }
        return result;
    }

    /// <summary>
    /// TryCompact on a generated journal with the real Windows primitives. Case: generated journal fields, plus optional
    /// "missing_dir": true (no folder, no file), "no_compaction_files": true (NotSupported), "before_lock_append": [byte specs]
    /// (another process appends them right before the journal is locked) or "before_lock_rewrite": {segments, pad_to?} (another
    /// compaction replaced the file meanwhile). Reports the result and the final file (length, FNV-1a 64, number of '\n'), the
    /// number of leftover *.compact.tmp files and the committed Moves a fresh journal reads afterwards (count, last Id).
    /// </summary>
    public static JsonArray JournalCompact(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = W301Temp();
            Directory.CreateDirectory(root);
            try
            {
                var dir = Path.Combine(root, "data");
                var path = Path.Combine(dir, "operations.jsonl");
                byte[]? initial = null;
                if (c["missing_dir"]?.GetValue<bool>() != true)
                {
                    Directory.CreateDirectory(dir);
                    initial = W301Build(c);
                    File.WriteAllBytes(path, initial);
                }
                var hooked = new W301HookedFiles(new PhysicalJournalCompactionFiles());
                if (c["before_lock_append"] is JsonArray add)
                {
                    var extra = W301Specs(add);
                    hooked.BeforeLock = () =>
                    {
                        using var other = new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.Read);
                        other.Write(extra);
                    };
                }
                else if (c["before_lock_rewrite"] is JsonObject rewrite)
                {
                    var replacement = W301Build(rewrite);
                    hooked.BeforeLock = () => File.WriteAllBytes(path, replacement);
                }
                var journal = new OperationJournal(new W301Paths(path), new PhysicalFileSystem(), new W301Clock(),
                    compactionFiles: c["no_compaction_files"]?.GetValue<bool>() == true ? null : hooked);
                var r = journal.TryCompact();
                var exists = File.Exists(path);
                var final = exists ? File.ReadAllBytes(path) : Array.Empty<byte>();
                IReadOnlyList<JournalEntry> moves = exists ? new OperationJournal(new W301Paths(path), new PhysicalFileSystem(), new W301Clock()).ReadCommittedMoves() : Array.Empty<JournalEntry>();
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["case"] = c.DeepClone(),
                    ["initial_length"] = initial?.Length,
                    ["initial_fnv64"] = initial is null ? null : W301Fnv(initial),
                    ["outcome"] = r.Outcome.ToString(),
                    ["bytes_before"] = r.BytesBefore,
                    ["bytes_after"] = r.BytesAfter,
                    ["error_set"] = r.Error is not null,
                    ["final_exists"] = exists,
                    ["final_length"] = final.Length,
                    ["final_fnv64"] = W301Fnv(final),
                    ["final_newlines"] = final.Count(b => b == (byte)'\n'),
                    ["leftover_temp_files"] = Directory.Exists(dir) ? Directory.GetFiles(dir, "*" + ".compact.tmp").Length : 0,
                    ["moves_after"] = moves.Count,
                    ["last_move_after"] = moves.Count == 0 ? null : moves[^1].Id,
                });
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
            }
        }
        return result;
    }

    /// <summary>
    /// One JournalTransaction driven step by step. Case {name, initial?: [byte specs], prepared: line, retry_of?: line,
    /// tx_now?: seconds (the transaction's own clock; default 100), steps: [...]}. The journal clock stays at the base. Each step
    /// may carry "fail_append": n (the next n OpenAppend calls throw IOException("disk full")). Steps:
    ///   {"do":"try_begin"} -> {"result": bool} | throws     {"do":"begin"} -> {} | throws
    ///   {"do":"verify_stat", "length": n|null, "code": c} -> {} | {"throws","code"}   (VerifyDestination(FileStat?, code))
    ///   {"do":"mark"}                                     (MarkMutationCompleted)
    ///   {"do":"commit"} -> {"entry": line, "journal_error_set": bool}
    ///   {"do":"fail", "error": {"io": message} | {"coded": code} | {"canceled": true}} -> {"entry": line|null, "journal_error_set"}
    ///   {"do":"dismiss_rolled_back"} -> {"entry": line|null, "journal_error_set"}
    ///   {"do":"other_append", "entry": line}              (another journal instance on the same file appends)
    ///   {"do":"dispose"}
    /// After every step: "is_prepared", "superseded", "mutation_completed", "live" (registry.IsLive(prepared.Id)), "append_opens".
    /// After the script: "final_hex".
    /// </summary>
    public static JsonArray JournalTransactionCases(JsonObject inputs)
    {
        var txType = typeof(OperationJournal).Assembly.GetType("PhotoReview.Core.FileActions.JournalTransaction", throwOnError: true)!;
        const BindingFlags Any = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;
        MethodInfo M(string name, params Type[] types) => txType.GetMethod(name, Any, types)
            ?? throw new InvalidOperationException("JournalTransaction." + name + " not found");
        var tryBegin = M("TryBegin");
        var begin = M("Begin");
        var verifyStat = M("VerifyDestination", typeof(FileStat), typeof(string));
        var mark = M("MarkMutationCompleted");
        var commit = M("Commit", typeof(string).MakeByRefType());
        var fail = M("Fail", typeof(Exception), typeof(string).MakeByRefType());
        var dismissRolledBack = M("DismissRolledBack", typeof(string).MakeByRefType());
        var dispose = M("Dispose");
        bool Prop(object tx, string name) => (bool)txType.GetProperty(name, Any)!.GetValue(tx)!;

        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = W301Temp();
            Directory.CreateDirectory(root);
            try
            {
                var dir = Path.Combine(root, "data");
                var path = Path.Combine(dir, "operations.jsonl");
                Directory.CreateDirectory(dir);
                if (c["initial"] is JsonArray initial) File.WriteAllBytes(path, W301Specs(initial));
                var fs = new W301Fs();
                var registry = new InProcessLiveOperationRegistry();
                var journal = new OperationJournal(new W301Paths(path), fs, new W301Clock(), appendRetryDelay: _ => { }, liveOperations: registry);
                var other = new OperationJournal(new W301Paths(path), new PhysicalFileSystem(), new W301Clock(), appendRetryDelay: _ => { });
                var txClock = new W301Clock { UtcNow = W301Base.AddSeconds(c["tx_now"]?.GetValue<double>() ?? 100) };
                var prepared = W301Entry(c["prepared"]);
                JournalEntry? retryOf = c["retry_of"] is { } r ? W301Entry(r) : null;
                var tx = Activator.CreateInstance(txType, Any, null, [journal, txClock, prepared, retryOf], CultureInfo.InvariantCulture)!;
                var steps = new JsonArray();
                foreach (var stepNode in c["steps"]!.AsArray())
                {
                    var s = stepNode!.AsObject();
                    var kind = s["do"]!.GetValue<string>();
                    fs.FailAppends = s["fail_append"]?.GetValue<int>() ?? 0;
                    var opensBefore = fs.Appends;
                    JsonObject step;
                    try
                    {
                        switch (kind)
                        {
                            case "try_begin":
                                step = new JsonObject { ["result"] = (bool)tryBegin.Invoke(tx, null)! };
                                break;
                            case "begin":
                                begin.Invoke(tx, null);
                                step = new JsonObject();
                                break;
                            case "verify_stat":
                                var stat = s["length"] is { } len ? new FileStat(len.GetValue<long>(), W301Base) : null;
                                verifyStat.Invoke(tx, [stat, s["code"]!.GetValue<string>()]);
                                step = new JsonObject();
                                break;
                            case "mark":
                                mark.Invoke(tx, null);
                                step = new JsonObject();
                                break;
                            case "commit":
                            {
                                object?[] args = [null];
                                var entry = (JournalEntry)commit.Invoke(tx, args)!;
                                step = new JsonObject { ["entry"] = W301Line(entry), ["journal_error_set"] = args[0] is not null };
                                break;
                            }
                            case "fail":
                            {
                                var e = s["error"]!.AsObject();
                                Exception failure = e["io"] is { } io ? new IOException(io.GetValue<string>())
                                    : e["coded"] is { } code ? new JournalCodedException(code.GetValue<string>())
                                    : new OperationCanceledException();
                                object?[] args = [failure, null];
                                var entry = (JournalEntry?)fail.Invoke(tx, args);
                                step = new JsonObject { ["entry"] = W301Line(entry), ["journal_error_set"] = args[1] is not null };
                                break;
                            }
                            case "dismiss_rolled_back":
                            {
                                object?[] args = [null];
                                var entry = (JournalEntry?)dismissRolledBack.Invoke(tx, args);
                                step = new JsonObject { ["entry"] = W301Line(entry), ["journal_error_set"] = args[0] is not null };
                                break;
                            }
                            case "other_append":
                                other.Append(W301Entry(s["entry"]));
                                step = new JsonObject();
                                break;
                            case "dispose":
                                dispose.Invoke(tx, null);
                                step = new JsonObject();
                                break;
                            default:
                                throw new ArgumentException("unknown step " + kind);
                        }
                    }
                    catch (TargetInvocationException ex)
                    {
                        step = W301Throws(ex);
                    }
                    fs.FailAppends = 0;
                    step["do"] = kind;
                    step["append_opens"] = fs.Appends - opensBefore;
                    step["is_prepared"] = Prop(tx, "IsPrepared");
                    step["superseded"] = Prop(tx, "Superseded");
                    step["mutation_completed"] = Prop(tx, "MutationCompleted");
                    step["live"] = registry.IsLive(prepared.Id);
                    steps.Add(step);
                }
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["case"] = c.DeepClone(),
                    ["steps"] = steps,
                    ["final_hex"] = File.Exists(path) ? Convert.ToHexString(File.ReadAllBytes(path)) : null,
                });
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
            }
        }
        return result;
    }
}
