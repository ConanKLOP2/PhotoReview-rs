using System.Reflection;
using System.Reflection.Emit;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Model;

// W3-02 golden package: JournalCompactionPlan.Build and OperationJournal.TryParseTailCommittedMove (C# d92d6aae).
// Copy to tools/oracle-cs/Commands/W3-02.cs and add these dispatch lines to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W3-02
//     "journal-compaction-plan" => Commands.JournalCompactionPlanCases(inputs),
//     "journal-tail-committed-move" => Commands.JournalTailCommittedMoveCases(inputs),
// Inputs: tests/golden-inputs/{journal-compaction-plan,journal-tail-committed-move}.json. Pure: no file system, no clock
// (D19: every timestamp is built from a fixed base; the generators use a seeded System.Random, deterministic across runs).
//
// Both C# members are internal. TryParseTailCommittedMove returns a public type, so a delegate over its MethodInfo is
// enough (as W1-14 does for JournalLineParser). JournalCompactionPlan.Build takes a ReadOnlySpan<byte> (cannot go through
// MethodInfo.Invoke) and returns an internal record struct (cannot be named), so it is called through a DynamicMethod that
// converts byte[] -> ReadOnlySpan<byte>, calls Build and boxes the Result; its three properties are then read by reflection.
//
// Line spec (one element of a case's "lines", or the "line" of a tail case):
//   {"e": [id, type, state, tick, errorCode?], ...options}  a JournalEntry serialized by JsonSerializer.Serialize (the journal
//        writer's exact bytes): Source C:\photos\<id>.jpg, Destination C:\photos\sel\<id>.jpg (null for Recycle), Size 1,
//        LastWriteUtc 2026-09-25Z, TimestampUtc base + tick seconds, Error "boom" iff errorCode given.
//        options: "members": n (GroupId "g-<id>" + n members; 0 = empty list), "blank_member": true (last member Source " "),
//                 "inner_cr": true ('\r' right after the opening '{'), "extra": raw JSON member text inserted before the final
//                 '}' (e.g. ",\"X\":{\"Type\":\"Copy\"}"), "prefix" / "suffix": text before / after the JSON,
//                 "corrupt_utf8": true (the first byte of "photos" becomes 0xFF: invalid UTF-8 inside the Source string)
//   {"raw": text}  UTF-8 text as is
//   {"hex": hex}   raw bytes
//   every spec but "hex" takes "eol" (default "\r\n"; "" = no terminator, e.g. a final partial line). Tail lines ignore "eol".
// Generated cases: "generate": {"kind": "move-window", "superseded": N, "superseded_type": "Move", "trailing_latest": M,
//   "trailing_inner_cr": K} or {"kind": "history", "seed": s, "id_count": n, "all_move": bool} (port of
//   JournalCompactionPlanTests.Generate: per-Id histories randomly interleaved, with scattered blank and garbage lines).
static partial class Commands
{
    private static readonly DateTime W302Base = new(2026, 9, 25, 0, 0, 0, DateTimeKind.Utc);

    private delegate JournalEntry? W302TailParser(ReadOnlySpan<byte> line);

    private static Func<byte[], object>? s_w302Build;

    private static Func<byte[], object> W302Build()
    {
        if (s_w302Build is not null) return s_w302Build;
        var planType = typeof(JournalEntry).Assembly.GetType("PhotoReview.Core.FileActions.JournalCompactionPlan", throwOnError: true)!;
        var build = planType.GetMethod("Build", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)
            ?? throw new InvalidOperationException("JournalCompactionPlan.Build not found");
        var toSpan = typeof(ReadOnlySpan<byte>).GetMethod("op_Implicit", BindingFlags.Public | BindingFlags.Static, [typeof(byte[])])
            ?? throw new InvalidOperationException("ReadOnlySpan<byte>.op_Implicit(byte[]) not found");
        var method = new DynamicMethod("W302Build", typeof(object), [typeof(byte[])], typeof(Commands).Module, skipVisibility: true);
        var il = method.GetILGenerator();
        il.Emit(OpCodes.Ldarg_0);
        il.Emit(OpCodes.Call, toSpan);
        il.Emit(OpCodes.Call, build);
        il.Emit(OpCodes.Box, build.ReturnType);
        il.Emit(OpCodes.Ret);
        return s_w302Build = (Func<byte[], object>)method.CreateDelegate(typeof(Func<byte[], object>));
    }

    private static (byte[] Kept, int Dropped, int KeptLines) W302RunPlan(byte[] snapshot)
    {
        var boxed = W302Build()(snapshot);
        var type = boxed.GetType();
        T Get<T>(string name) => (T)(type.GetProperty(name) ?? throw new InvalidOperationException("Result." + name + " not found")).GetValue(boxed)!;
        return (Get<byte[]>("Kept"), Get<int>("DroppedLines"), Get<int>("KeptLines"));
    }

    private static W302TailParser W302Tail()
    {
        var method = typeof(OperationJournal).GetMethod("TryParseTailCommittedMove", BindingFlags.NonPublic | BindingFlags.Static, [typeof(ReadOnlySpan<byte>)])
            ?? throw new InvalidOperationException("OperationJournal.TryParseTailCommittedMove not found");
        return (W302TailParser)Delegate.CreateDelegate(typeof(W302TailParser), method);
    }

    private static JournalEntry W302Entry(string id, FileOperationType type, JournalState state, int tick, string? errorCode = null,
        int? members = null, bool blankMember = false) =>
        new(id, type, state, $@"C:\photos\{id}.jpg", type == FileOperationType.Recycle ? null : $@"C:\photos\sel\{id}.jpg", 1, W302Base,
            W302Base.AddSeconds(tick), Error: errorCode is null ? null : "boom", ErrorCode: errorCode,
            GroupId: members is null ? null : "g-" + id,
            GroupMembers: members is not { } n ? null : Enumerable.Range(0, n).Select(k => new JournalGroupMember(
                blankMember && k == n - 1 ? " " : $@"C:\photos\{id}-{k}.jpg",
                type == FileOperationType.Recycle ? null : $@"C:\photos\sel\{id}-{k}.jpg", 1, W302Base, false)).ToList());

    private static byte[] W302Line(JsonObject spec, bool withEol)
    {
        if (spec["hex"] is { } hex) return Convert.FromHexString(hex.GetValue<string>());
        var eol = withEol ? spec["eol"]?.GetValue<string>() ?? "\r\n" : "";
        if (spec["raw"] is { } raw) return Encoding.UTF8.GetBytes(raw.GetValue<string>() + eol);

        var e = spec["e"]!.AsArray();
        var entry = W302Entry(e[0]!.GetValue<string>(), Enum.Parse<FileOperationType>(e[1]!.GetValue<string>()),
            Enum.Parse<JournalState>(e[2]!.GetValue<string>()), e[3]!.GetValue<int>(), e.Count > 4 ? e[4]?.GetValue<string>() : null,
            spec["members"]?.GetValue<int>(), spec["blank_member"]?.GetValue<bool>() == true);
        var json = JsonSerializer.Serialize(entry);
        if (spec["extra"] is { } extra) json = json[..^1] + extra.GetValue<string>() + "}";
        if (spec["inner_cr"]?.GetValue<bool>() == true) json = "{\r" + json[1..];
        var text = (spec["prefix"]?.GetValue<string>() ?? "") + json + (spec["suffix"]?.GetValue<string>() ?? "") + eol;
        var bytes = Encoding.UTF8.GetBytes(text);
        if (spec["corrupt_utf8"]?.GetValue<bool>() == true)
        {
            var at = bytes.AsSpan().IndexOf("photos"u8);
            if (at < 0) throw new InvalidOperationException("corrupt_utf8: no 'photos' in the line");
            bytes[at] = 0xFF;
        }
        return bytes;
    }

    private static byte[] W302Concat(IEnumerable<byte[]> lines)
    {
        using var ms = new MemoryStream();
        foreach (var line in lines) ms.Write(line);
        return ms.ToArray();
    }

    private static List<byte[]> W302MoveWindow(JsonObject g)
    {
        var superseded = g["superseded"]!.GetValue<int>();
        var type = Enum.Parse<FileOperationType>(g["superseded_type"]?.GetValue<string>() ?? "Move");
        var lines = new List<byte[]>();
        byte[] Line(JournalEntry entry, bool innerCr = false)
        {
            var json = JsonSerializer.Serialize(entry);
            return Encoding.UTF8.GetBytes((innerCr ? "{\r" + json[1..] : json) + "\r\n");
        }
        for (var i = 0; i < superseded; i++)
        {
            lines.Add(Line(W302Entry($"m{i}", type, JournalState.Committed, i * 2)));
            lines.Add(Line(W302Entry($"m{i}", type, JournalState.Dismissed, i * 2 + 1)));
        }
        var tick = superseded * 2;
        for (var j = 0; j < (g["trailing_latest"]?.GetValue<int>() ?? 0); j++)
            lines.Add(Line(W302Entry($"t{j}", FileOperationType.Move, JournalState.Committed, tick++)));
        for (var j = 0; j < (g["trailing_inner_cr"]?.GetValue<int>() ?? 0); j++)
            lines.Add(Line(W302Entry($"r{j}", FileOperationType.Move, JournalState.Committed, tick++), innerCr: true));
        return lines;
    }

    // Port of JournalCompactionPlanTests.BuildHistory / Generate (same kinds, same order of Random calls).
    private enum W302HistoryKind { PreparedCommitted, PreparedFailed, PreparedFailedDismissed, RetryPreparedFailedPreparedCommitted, Fa01Residue }

    private static List<byte[]> W302History(JsonObject g)
    {
        var rng = new Random(g["seed"]!.GetValue<int>());
        var idCount = g["id_count"]!.GetValue<int>();
        var allMove = g["all_move"]?.GetValue<bool>() == true;
        var kinds = Enum.GetValues<W302HistoryKind>();
        var tick = 0;
        var pending = new List<Queue<JournalEntry>>();
        for (var i = 0; i < idCount; i++)
        {
            var id = $"id{i}";
            var type = allMove ? FileOperationType.Move : (FileOperationType)(i % 3);
            var kind = kinds[rng.Next(kinds.Length)];
            JournalEntry Next(JournalState state, string? code = null) => W302Entry(id, type, state, tick++, code);
            List<JournalEntry> history = kind switch
            {
                W302HistoryKind.PreparedCommitted => [Next(JournalState.Prepared), Next(JournalState.Committed)],
                W302HistoryKind.PreparedFailed => [Next(JournalState.Prepared), Next(JournalState.Failed)],
                W302HistoryKind.PreparedFailedDismissed => [Next(JournalState.Prepared), Next(JournalState.Failed), Next(JournalState.Dismissed)],
                W302HistoryKind.RetryPreparedFailedPreparedCommitted =>
                    [Next(JournalState.Prepared), Next(JournalState.Failed), Next(JournalState.Prepared), Next(JournalState.Committed)],
                _ => [Next(JournalState.Prepared), Next(JournalState.Committed), Next(JournalState.Failed, JournalErrors.PendingUnconfirmed)],
            };
            pending.Add(new Queue<JournalEntry>(history));
        }
        var lines = new List<byte[]>();
        while (pending.Count > 0)
        {
            var index = rng.Next(pending.Count);
            var queue = pending[index];
            lines.Add(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(queue.Dequeue()) + "\r\n"));
            if (queue.Count == 0) pending.RemoveAt(index);
            if (rng.Next(15) == 0) lines.Add(Encoding.UTF8.GetBytes("\r\n"));
            if (rng.Next(15) == 0) lines.Add(Encoding.UTF8.GetBytes($"garbage-{rng.Next()}\r\n"));
        }
        return lines;
    }

    private static readonly UTF8Encoding W302StrictUtf8 = new(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true);

    /// <summary>Puts bytes in the golden as "<key>_text" when they are valid UTF-8, else as "<key>_hex".</summary>
    private static void W302PutBytes(JsonObject o, string key, byte[] bytes)
    {
        try
        {
            o[key + "_text"] = W302StrictUtf8.GetString(bytes);
        }
        catch (DecoderFallbackException)
        {
            o[key + "_hex"] = Convert.ToHexString(bytes);
        }
    }

    /// <summary>
    /// JournalCompactionPlan.Build for each case {name, lines:[spec] | generate:{...}}: the exact snapshot (so the Rust test needs
    /// no generator), its line count (number of '\n'-split lines, a final unterminated one included), the kept bytes and the two
    /// counters.
    /// </summary>
    public static JsonArray JournalCompactionPlanCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            List<byte[]> lines;
            if (c["generate"] is JsonObject g)
            {
                lines = g["kind"]!.GetValue<string>() switch
                {
                    "move-window" => W302MoveWindow(g),
                    "history" => W302History(g),
                    var other => throw new ArgumentException("unknown generator " + other),
                };
            }
            else
            {
                lines = c["lines"]!.AsArray().Select(l => W302Line(l!.AsObject(), withEol: true)).ToList();
            }
            var snapshot = W302Concat(lines);
            var (kept, dropped, keptLines) = W302RunPlan(snapshot);
            var lineCount = 0;
            for (var p = 0; p < snapshot.Length;)
            {
                var nl = Array.IndexOf(snapshot, (byte)'\n', p);
                p = nl < 0 ? snapshot.Length : nl + 1;
                lineCount++;
            }
            if (dropped + keptLines != lineCount) throw new InvalidOperationException(c["name"] + ": line count mismatch");
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            W302PutBytes(o, "snapshot", snapshot);
            o["line_count"] = lineCount;
            W302PutBytes(o, "kept", kept);
            o["dropped_lines"] = dropped;
            o["kept_lines"] = keptLines;
            result.Add(o);
        }
        return result;
    }

    /// <summary>OperationJournal.TryParseTailCommittedMove for each case {name, line: spec}: whether the line is a committed Move, and its Id.</summary>
    public static JsonArray JournalTailCommittedMoveCases(JsonObject inputs)
    {
        var tail = W302Tail();
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var line = W302Line(c["line"]!.AsObject(), withEol: false);
            var entry = tail(line);
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            W302PutBytes(o, "line", line);
            o["committed_move"] = entry is not null;
            o["id"] = entry?.Id;
            result.Add(o);
        }
        return result;
    }
}
