using System.Collections;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.App;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Model;

// W8-10f: PhotoReview.App.Shared/Presenters/RecoveryFilter.cs (RecoveryFilter, RecoveryTone) and
// Presenters/RecoveryPresentation.cs (pure mapping; time-dependent members are NOT in this oracle, see contract D-10f-2).
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-10f
//     "recovery-presentation" => Commands.RecoveryPresentationCases(inputs),
// Wiring (the lead does this; the writer did not run the oracle):
//   * RecoveryPresentation is `internal static` and its records are `internal sealed`: reached by reflection on
//     PhotoReview.App.Shared.dll (the same assembly as RecoveryFilter, which is public). No InternalsVisibleTo, no C# edits.
//   * Culture: CurrentCulture = InvariantCulture (N0 gives "1,234"); CurrentUICulture = the English catalog (as in W2-01).
//   * tools/oracle.ps1: PhotoReview.App.Shared.dll is already built for W8-01b.
// No ambient state: no clock, no time zone, no file system (the directory predicate is a fixed set).
// Input: { "cases": [ { "name", "fn", "input"? } ] }. Output: [ { "name", "fn", "input", "output"? , "threw"? } ].
// "output" shapes: string (text, tone name), bool, null, [r,g,b] for rgb, or for the *_all / *_matrix functions an array of
// { "in": ..., "out": ... } rows in the enumeration order below.

static partial class Commands
{
    private static readonly Type PresentationType = typeof(RecoveryFilter).Assembly
        .GetType("PhotoReview.App.Presenters.RecoveryPresentation", throwOnError: true)!;

    private static readonly DateTime Stamp = new(2026, 1, 2, 3, 4, 5, DateTimeKind.Utc);

    private static object? Call(string name, params object?[] args) =>
        PresentationType.GetMethod(name, BindingFlags.Public | BindingFlags.Static)!.Invoke(null, args);

    private static JournalGroupMember Member(JsonObject m) => new(
        m["source"]!.GetValue<string>(),
        m["destination"]?.GetValue<string>(),
        m["size"]?.GetValue<long>() ?? 0L,
        Stamp,
        m["permanent"]?.GetValue<bool>() ?? false);

    private static JournalEntry Entry(JsonObject e) => new(
        "e1",
        Enum.Parse<FileOperationType>(e["type"]!.GetValue<string>()),
        Enum.Parse<JournalState>(e["state"]?.GetValue<string>() ?? "Committed"),
        e["source"]!.GetValue<string>(),
        e["destination"]?.GetValue<string>(),
        e["size"]?.GetValue<long>() ?? 0L,
        Stamp,
        Stamp,
        ErrorCode: e["error_code"]?.GetValue<string>(),
        Undo: e["undo"]?.GetValue<bool>(),
        GroupMembers: e["group_members"] is JsonArray gm ? gm.Select(n => Member(n!.AsObject())).ToList() : null);

    private static RecoveryPathCheck PathCheck(JsonObject p) => new(
        p["path"]!.GetValue<string>(),
        Enum.Parse<RecoveryPathStatus>(p["status"]!.GetValue<string>()),
        p["size"]?.GetValue<long>(),
        null,
        p["folder_exists"]?.GetValue<bool>() ?? true);

    private static RecoveryCheckResult Check(JsonObject c)
    {
        var members = c["group_members"] is JsonArray arr
            ? arr.Select(n => new RecoveryGroupMemberCheck(Member(n!["member"]!.AsObject()), Check(n["check"]!.AsObject()))).ToList()
            : null;
        return new RecoveryCheckResult(
            Entry(c["entry"]!.AsObject()),
            PathCheck(c["source"]!.AsObject()),
            c["destination"] is JsonObject d ? PathCheck(d) : null,
            Enum.Parse<RecoveryVerdict>(c["verdict"]!.GetValue<string>()),
            members);
    }

    private static JsonNode? Out(object? value) => value switch
    {
        null => null,
        string s => JsonValue.Create(s),
        bool b => JsonValue.Create(b),
        ITuple t => new JsonArray(Enumerable.Range(0, t.Length).Select(i => JsonValue.Create(Convert.ToInt32(t[i]))).ToArray()),
        _ => JsonValue.Create(value.ToString()),
    };

    private static JsonArray Rows(IEnumerable<(string Key, object? Value)> rows) =>
        new(rows.Select(r => (JsonNode?)new JsonObject { ["in"] = r.Key, ["out"] = Out(r.Value) }).ToArray());

    private static JsonNode? Run(string fn, JsonObject input)
    {
        switch (fn)
        {
            case "rgb_all":
                return Rows(Enum.GetValues<RecoveryTone>().Select(t => (t.ToString(), Call("Rgb", t))));
            case "verdict_tone_all":
                return Rows(Enum.GetValues<RecoveryVerdict>().Select(v => (v.ToString(), Call("VerdictTone", v))));
            case "verdict_text_all":
                return Rows(Enum.GetValues<RecoveryVerdict>().Select(v => (v.ToString(), Call("VerdictText", v))));
            case "explain_text_all":
                return Rows(Enum.GetValues<RecoveryVerdict>().Select(v => (v.ToString(), Call("ExplainText", v))));
            case "action_text_all":
                return Rows(Enum.GetValues<RecoveryVerdict>().Select(v => (v.ToString(), Call("ActionText", v))));
            case "path_status_all":
                return Rows(Enum.GetValues<RecoveryPathStatus>().Select(s => (
                    s.ToString(), (object?)(Call("PathStatusText", s) + " | " + Call("PathStatusTone", s)))));
            case "matches_matrix":
                return Rows(from f in Enum.GetValues<RecoveryFilter>()
                            from v in new RecoveryVerdict?[] { null }.Concat(Enum.GetValues<RecoveryVerdict>().Select(x => (RecoveryVerdict?)x))
                            select ($"{f}/{v?.ToString() ?? "null"}", Call("Matches", f, v)));
            case "allows_retry":
                return Out(Call("AllowsRetry", input["check"] is JsonObject c ? Check(c) : null));
            case "format_size":
                return Out(Call("FormatSize", input["bytes"]!.GetValue<long>()));
            case "operation_text_all":
                return Rows(Enum.GetValues<FileOperationType>().Select(o => (o.ToString(), Call("OperationText", o))));
            case "state_text_all":
                return Rows(Enum.GetValues<JournalState>().Select(s => (s.ToString(), Call("StateText", s))));
            case "group_status_text":
                return Out(Call(
                    "GroupStatusText",
                    Enum.Parse<RecoveryVerdict>(input["verdict"]!.GetValue<string>()),
                    Member(input["member"]!.AsObject()),
                    PathCheck(input["source"]!.AsObject()),
                    input["destination"] is JsonObject dest ? PathCheck(dest) : null));
            case "retry_confirm_text":
                return Out(Call(
                    "RetryConfirmText",
                    Entry(input["entry"]!.AsObject()),
                    input["check"] is JsonObject chk ? Check(chk) : null,
                    input["allow_permanent_delete"]?.GetValue<bool>() ?? false));
            case "nearest_existing_folder":
            {
                var existing = input["existing"]!.AsArray().Select(n => n!.GetValue<string>()).ToHashSet(StringComparer.Ordinal);
                Func<string, bool> exists = p => existing.Contains(p);
                return Out(Call("NearestExistingFolder", input["path"]!.GetValue<string>(), exists));
            }
            default:
                throw new ArgumentException($"unknown fn {fn}");
        }
    }

    /// <summary>W8-10f cases. Every value is produced by the C# code; nothing is computed here.</summary>
    public static JsonArray RecoveryPresentationCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var fn = c["fn"]!.GetValue<string>();
            var input = c["input"] as JsonObject ?? new JsonObject();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["fn"] = fn, ["input"] = input.DeepClone() };
            try
            {
                item["output"] = Run(fn, input)?.DeepClone();
            }
            catch (Exception ex)
            {
                item["threw"] = ex.GetType().Name;
            }
            result.Add(item);
        }
        return result;
    }
}
