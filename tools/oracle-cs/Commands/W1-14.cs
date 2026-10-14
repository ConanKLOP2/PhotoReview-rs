using System.Globalization;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using PhotoReview.Core.Caching;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Model;
using PhotoReview.Core.Settings;
using PhotoReview.Core.Updates;

// W1-14 (from contract oracle-commands.cs)
static partial class Commands
{
// Paste into `static class Commands` in tools/oracle-cs/Program.cs.
// Needs: using System.Reflection; using PhotoReview.Core.FileActions; using PhotoReview.Core.Model;
// switch case:  "journal-lines" => Commands.JournalLineCases(inputs),
// JournalLineParser is `internal`; it is reached by reflection (delegates over its two public static TryParse overloads).

    private static (JournalEntry? Entry, string? Threw) Attempt(Func<JournalEntry?> parse)
    {
        try { return (parse(), null); }
        catch (Exception ex) { return (null, ex.GetType().Name); }
    }

    private delegate JournalEntry? JournalBytesParser(ReadOnlySpan<byte> utf8Line);

    private static JsonObject JournalDateTimeJson(DateTime value) =>
        new() { ["ticks"] = value.Ticks, ["kind"] = value.Kind.ToString() };

    private static JsonObject JournalMemberJson(JournalGroupMember? m) => m is null
        ? throw new InvalidOperationException("null member survived the parser")
        : new JsonObject
        {
            ["source"] = m.Source,
            ["destination"] = m.Destination,
            ["size"] = m.Size,
            ["last_write"] = JournalDateTimeJson(m.LastWriteUtc),
            ["permanent"] = m.Permanent,
        };

    private static JsonObject JournalEntryJson(JournalEntry e) => new()
    {
        ["id"] = e.Id,
        ["type"] = e.Type.ToString(),
        ["state"] = e.State.ToString(),
        ["source"] = e.Source,
        ["destination"] = e.Destination,
        ["size"] = e.Size,
        ["last_write"] = JournalDateTimeJson(e.LastWriteUtc),
        ["timestamp"] = JournalDateTimeJson(e.TimestampUtc),
        ["error"] = e.Error,
        ["error_code"] = e.ErrorCode,
        ["permanent"] = e.Permanent,
        ["undo"] = e.Undo,
        ["group_id"] = e.GroupId,
        ["group_members"] = e.GroupMembers is null ? null : new JsonArray(e.GroupMembers.Select(m => (JsonNode?)JournalMemberJson(m)).ToArray()),
    };

    private static DateTime JournalDateTimeFrom(JsonNode? node) =>
        new(long.Parse(node!["ticks"]!.GetValue<string>(), System.Globalization.CultureInfo.InvariantCulture), Enum.Parse<DateTimeKind>(node["kind"]!.GetValue<string>(), ignoreCase: true));

    private static JournalEntry JournalEntryFrom(JsonObject o) => new(
        o["id"]!.GetValue<string>(),
        Enum.Parse<FileOperationType>(o["type"]!.GetValue<string>()),
        Enum.Parse<JournalState>(o["state"]!.GetValue<string>()),
        o["source"]!.GetValue<string>(),
        o["destination"]?.GetValue<string>(),
        long.Parse(o["size"]!.GetValue<string>(), System.Globalization.CultureInfo.InvariantCulture),
        JournalDateTimeFrom(o["last_write"]),
        JournalDateTimeFrom(o["timestamp"]),
        o["error"]?.GetValue<string>(),
        o["error_code"]?.GetValue<string>(),
        o["permanent"]?.GetValue<bool>(),
        o["undo"]?.GetValue<bool>(),
        o["group_id"]?.GetValue<string>(),
        o["group_members"]?.AsArray().Select(m => new JournalGroupMember(
            m!["source"]!.GetValue<string>(),
            m["destination"]?.GetValue<string>(),
            long.Parse(m["size"]!.GetValue<string>(), System.Globalization.CultureInfo.InvariantCulture),
            JournalDateTimeFrom(m["last_write"]),
            m["permanent"]!.GetValue<bool>())).ToList());

    /// <summary>
    /// JournalLineParser.TryParse (string and UTF-8 overloads) on hand-built lines, with the parsed entry and its default
    /// System.Text.Json re-serialization (the exact bytes the journal writer produces); plus the writer on explicit entries.
    /// Inputs: "lines" (strings), "byte_lines_hex" (hex of raw bytes), "entries" (objects, see JournalEntryFrom).
    /// </summary>
    public static JsonArray JournalLineCases(JsonObject inputs)
    {
        var type = typeof(JournalEntry).Assembly.GetType("PhotoReview.Core.FileActions.JournalLineParser")
            ?? throw new InvalidOperationException("JournalLineParser not found");
        var parseString = (Func<string, JournalEntry?>)Delegate.CreateDelegate(typeof(Func<string, JournalEntry?>),
            type.GetMethod("TryParse", BindingFlags.Public | BindingFlags.Static, [typeof(string)])!);
        var parseBytes = (JournalBytesParser)Delegate.CreateDelegate(typeof(JournalBytesParser),
            type.GetMethod("TryParse", BindingFlags.Public | BindingFlags.Static, [typeof(ReadOnlySpan<byte>)])!);

        var result = new JsonArray();
        foreach (var node in inputs["lines"]!.AsArray())
        {
            var line = node!.GetValue<string>();
            var item = new JsonObject { ["kind"] = "line", ["line"] = line };
            var (fromString, errString) = Attempt(() => parseString(line));
            var (fromBytes, errBytes) = Attempt(() => parseBytes(System.Text.Encoding.UTF8.GetBytes(line)));
            // The two overloads must behave identically (same result or same exception type).
            if (errString != errBytes || !Equals(fromString, fromBytes))
                throw new InvalidOperationException("string and byte overloads disagree for: " + line);
            item["ok"] = fromString is not null;
            if (errString is not null) item["threw"] = errString;
            if (fromString is not null)
            {
                item["entry"] = JournalEntryJson(fromString);
                item["reserialized"] = JsonSerializer.Serialize(fromString);
            }
            result.Add(item);
        }
        foreach (var node in inputs["byte_lines_hex"]!.AsArray())
        {
            var hex = node!.GetValue<string>();
            var bytes = Convert.FromHexString(hex);
            var item = new JsonObject { ["kind"] = "bytes", ["hex"] = hex };
            var (parsed, err) = Attempt(() => parseBytes(bytes));
            item["ok"] = parsed is not null;
            if (err is not null) item["threw"] = err;
            if (parsed is not null)
            {
                item["entry"] = JournalEntryJson(parsed);
                item["reserialized"] = JsonSerializer.Serialize(parsed);
            }
            result.Add(item);
        }
        foreach (var node in inputs["entries"]!.AsArray())
        {
            var o = node!.AsObject();
            var entry = JournalEntryFrom(o);
            result.Add(new JsonObject { ["kind"] = "write", ["entry"] = o.DeepClone(), ["line"] = JsonSerializer.Serialize(entry) });
        }
        return result;
    }
}
