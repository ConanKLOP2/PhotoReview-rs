using System.Globalization;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;

// W3-03 golden package: OperationJournal.ReconcilePendingOperations (incl. the ADR 0003 "older builds" downgrade guard) and
// JournalStartupRecovery.RunAsync (C# d92d6aae).
// Copy to tools/oracle-cs/Commands/W3-03.cs and add these dispatch lines to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W3-03
//     "journal-reconcile" => Commands.JournalReconcile(inputs),
//     "journal-startup-recovery" => Commands.JournalStartupRecoveryCases(inputs),
// Inputs: tests/golden-inputs/{journal-reconcile,journal-startup-recovery}.json (regenerate with
// contracts/drafts-w3/W3-03/tools/gen_golden_inputs.py). Self-contained (no W2Util). Touches no real file: the journal and the
// photos live in an in-memory IFileSystem (DispatchProxy), so the output depends only on the inputs (D19).
//
// Case fields (both commands):
//   journal        array of line strings, or null = the journal file does not exist
//   newline        separator AND terminator written after every line (default "\r\n")
//   trailing       false = no terminator after the LAST line (a torn tail; default true)
//   files          [{path, size, time}] photos on disk ("time" = ISO-8601 UTC last write)
//   live           ids whose live marker is held for the whole call (InProcessLiveOperationRegistry)
//   stat_throws    paths for which FileExists / GetFileStat throw ArgumentException (a path the file system rejects)
//   append_failures  the first N OpenAppend calls throw IOException("append-fault") (not a sharing violation: no retry)
//   read_fault     OpenReadShared of the journal throws IOException("locked") (startup only)
//   clock          the journal's clock (ISO-8601 UTC): TimestampUtc of every outcome line
// journal-reconcile only:  cutoff (ISO or null = ReconcilePendingOperations()), runs (1 or 2: the second run pins idempotence)
// journal-startup-recovery only: startup_clock (the IClock RunAsync reads; default = clock), with_log (default true)
//
// Output adds: journal_path; reconcile -> runs[{reconciled: [line], journal_after: text|null}];
// startup -> failed: [line], log: [{level, message, exception_type}] | null, journal_after.
// Lines are JsonSerializer.Serialize(entry), i.e. exactly what OperationJournal writes for that entry.

static partial class Commands
{
    private const string W303LocalAppData = @"C:\Users\test\AppData\Local";

    /// <summary>
    /// In-memory IFileSystem for OperationJournal + reconcile: the journal is a byte list, photos are (size, ticks) stats.
    /// Only the members the journal/reconcile may call are answered; anything else throws InvalidOperationException (it would
    /// escape as a crash of the oracle, proving the code did something unexpected).
    /// </summary>
    public class W303Fs : DispatchProxy
    {
        public string JournalPath { get; set; } = "";
        public List<byte>? Journal { get; set; } // null = missing
        public Dictionary<string, (long Size, long Ticks)> Files { get; } = new(StringComparer.OrdinalIgnoreCase);
        public HashSet<string> StatThrows { get; } = new(StringComparer.OrdinalIgnoreCase);
        public int AppendFailures { get; set; }
        public bool ReadFault { get; set; }

        protected override object? Invoke(MethodInfo? targetMethod, object?[]? args)
        {
            var path = args is { Length: > 0 } && args[0] is string s ? s : null;
            switch (targetMethod!.Name)
            {
                case nameof(IFileSystem.FileExists):
                    if (IsJournal(path!)) return Journal is not null;
                    if (StatThrows.Contains(path!)) throw new ArgumentException("rejected path", nameof(path));
                    return Files.ContainsKey(path!);
                case nameof(IFileSystem.GetFileStat):
                    if (IsJournal(path!)) return Journal is null ? null : new FileStat(Journal.Count, new DateTime(0, DateTimeKind.Utc));
                    if (StatThrows.Contains(path!)) throw new ArgumentException("rejected path", nameof(path));
                    return Files.TryGetValue(path!, out var f) ? new FileStat(f.Size, new DateTime(f.Ticks, DateTimeKind.Utc)) : null;
                case nameof(IFileSystem.DirectoryExists):
                    return string.Equals(path, Path.GetDirectoryName(JournalPath), StringComparison.OrdinalIgnoreCase);
                case nameof(IFileSystem.CreateDirectory):
                    return null;
                case nameof(IFileSystem.EnumerateFiles):
                    return Array.Empty<string>();
                case nameof(IFileSystem.OpenReadShared):
                    if (!IsJournal(path!)) throw new InvalidOperationException("read of a non-journal file: " + path);
                    if (ReadFault) throw new IOException("locked");
                    if (Journal is null) throw new FileNotFoundException("missing", path);
                    return new MemoryStream(Journal.ToArray(), writable: false);
                case nameof(IFileSystem.OpenAppend):
                    if (!IsJournal(path!)) throw new InvalidOperationException("append to a non-journal file: " + path);
                    if (AppendFailures > 0)
                    {
                        AppendFailures--;
                        throw new IOException("append-fault");
                    }
                    Journal ??= [];
                    return new W303AppendStream(Journal);
                default:
                    throw new InvalidOperationException("reconcile called IFileSystem." + targetMethod.Name);
            }
        }

        private bool IsJournal(string path) => string.Equals(path, JournalPath, StringComparison.OrdinalIgnoreCase);
    }

    private sealed class W303AppendStream(List<byte> target) : Stream
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
        public override void Write(byte[] buffer, int offset, int count) => target.AddRange(buffer.AsSpan(offset, count).ToArray());
    }

    private sealed class W303Clock(DateTime utcNow) : IClock
    {
        public DateTime UtcNow { get; } = utcNow;
    }

    private sealed class W303Log : ILog
    {
        public readonly JsonArray Entries = [];
        public bool Enabled => true;
        public void Info(string message) => Add("Info", message, null);
        public void Warn(string message) => Add("Warn", message, null);
        public void Error(string message, Exception? ex = null) => Add("Error", message, ex);

        private void Add(string level, string message, Exception? ex) => Entries.Add(new JsonObject
        {
            ["level"] = level,
            ["message"] = message,
            ["exception_type"] = ex?.GetType().Name,
        });
    }

    private static DateTime W303Time(JsonNode? node) =>
        DateTime.Parse(node!.GetValue<string>(), CultureInfo.InvariantCulture,
            DateTimeStyles.AdjustToUniversal | DateTimeStyles.AssumeUniversal);

    private static string? W303Str(JsonNode? node) => node?.GetValue<string>();

    /// <summary>Builds the file system, journal and live markers of one case. The markers are returned so they stay alive.</summary>
    private static (OperationJournal Journal, W303Fs Fs, List<IDisposable> Markers) W303Setup(JsonObject c)
    {
        var paths = new AppPaths(W303LocalAppData);
        var fs = DispatchProxy.Create<IFileSystem, W303Fs>();
        var state = (W303Fs)(object)fs;
        state.JournalPath = paths.JournalFile;
        if (c["journal"] is JsonArray lines)
        {
            var newline = W303Str(c["newline"]) ?? "\r\n";
            var trailing = c["trailing"]?.GetValue<bool>() ?? true;
            var text = new StringBuilder();
            for (var i = 0; i < lines.Count; i++)
            {
                text.Append(lines[i]!.GetValue<string>());
                if (i < lines.Count - 1 || trailing) text.Append(newline);
            }
            state.Journal = [.. Encoding.UTF8.GetBytes(text.ToString())];
        }
        foreach (var f in c["files"]?.AsArray() ?? [])
            state.Files[f!["path"]!.GetValue<string>()] = (f["size"]!.GetValue<long>(), W303Time(f["time"]).Ticks);
        foreach (var p in c["stat_throws"]?.AsArray() ?? [])
            state.StatThrows.Add(p!.GetValue<string>());
        state.AppendFailures = c["append_failures"]?.GetValue<int>() ?? 0;
        state.ReadFault = c["read_fault"]?.GetValue<bool>() ?? false;

        var registry = new InProcessLiveOperationRegistry();
        var markers = (c["live"]?.AsArray() ?? []).Select(id => registry.Begin(id!.GetValue<string>())).ToList();
        var journal = new OperationJournal(paths, fs, new W303Clock(W303Time(c["clock"])),
            appendRetryDelay: _ => { }, liveOperations: registry);
        return (journal, state, markers);
    }

    private static JsonArray W303Lines(IEnumerable<JournalEntry> entries) =>
        new(entries.Select(e => (JsonNode?)JsonValue.Create(JsonSerializer.Serialize(e))).ToArray());

    private static string? W303JournalText(W303Fs fs) => fs.Journal is null ? null : Encoding.UTF8.GetString(fs.Journal.ToArray());

    /// <summary>OperationJournal.ReconcilePendingOperations(cutoff), run `runs` times on the same journal.</summary>
    public static JsonArray JournalReconcile(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var (journal, fs, markers) = W303Setup(c);
            try
            {
                DateTime? cutoff = c["cutoff"] is { } cut ? W303Time(cut) : null;
                var runs = new JsonArray();
                for (var run = 0; run < (c["runs"]?.GetValue<int>() ?? 1); run++)
                {
                    var reconciled = journal.ReconcilePendingOperations(cutoff);
                    runs.Add(new JsonObject
                    {
                        ["reconciled"] = W303Lines(reconciled),
                        ["journal_after"] = W303JournalText(fs),
                    });
                }
                var output = (JsonObject)c.DeepClone();
                output["journal_path"] = fs.JournalPath;
                output["runs"] = runs;
                result.Add(output);
            }
            finally
            {
                markers.ForEach(m => m.Dispose());
            }
        }
        return result;
    }

    /// <summary>JournalStartupRecovery.RunAsync(journal, startupClock, log) (no compaction files: TryCompact is NotSupported).</summary>
    public static JsonArray JournalStartupRecoveryCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var (journal, fs, markers) = W303Setup(c);
            try
            {
                var startupClock = new W303Clock(W303Time(c["startup_clock"] ?? c["clock"]));
                var log = (c["with_log"]?.GetValue<bool>() ?? true) ? new W303Log() : null;
                var failed = JournalStartupRecovery.RunAsync(journal, startupClock, log).GetAwaiter().GetResult();
                var output = (JsonObject)c.DeepClone();
                output["journal_path"] = fs.JournalPath;
                output["failed"] = W303Lines(failed);
                output["log"] = log?.Entries;
                output["journal_after"] = W303JournalText(fs);
                result.Add(output);
            }
            finally
            {
                markers.ForEach(m => m.Dispose());
            }
        }
        return result;
    }
}
