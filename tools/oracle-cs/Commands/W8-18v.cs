using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;
using PhotoReview.App;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Model;

// W8-18v oracle (the F half of the unit): the Clear (Dismiss) flow of RecoveryDialog up to the point where it needs a window: which questions and notices are shown, with
// which title, text and kind, which entries reach the dismiss delegate, and which rows are left. NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18v
//     "recovery-dialog-dismiss" => Commands.W818vRecoveryDialogDismissCases(inputs),
//
// Wiring (the lead does this; nothing new if W8-18t / W8-18u are wired): same references. The Retry flow is NOT reachable window-less (RetryAsync starts from the selected
// list row, and the list window does not exist), so Retry has no golden: its order is covered by hand-written Rust tests taken from RecoveryDialogTests.
//
// ITaskDialogPort (the message-box seam) is an `internal` interface of PhotoReview.dll. It is implemented with System.Reflection.DispatchProxy, which emits its proxy type
// with the access-check bypass for the assemblies of non-public interfaces: DispatchProxy.Create<ITaskDialogPort, W818vMessageProxy>() through reflection. If the runtime
// refuses (TypeLoadException "not visible"), the fix is an `[assembly: System.Runtime.CompilerServices.IgnoresAccessChecksTo("PhotoReview")]` attribute plus its
// one-line attribute class in the oracle project; no change to the C# repo either way.
//
// A window-less dialog is RuntimeHelpers.GetUninitializedObject(typeof(RecoveryDialog)) (no constructor, no chrome, no window, Hwnd == 0) with the private readonly fields
// _presenter (a real RecoveryWindowPresenter built by reflection from the entries and the dismiss delegate) and _messages set. With Hwnd == 0, Dismiss ends right after
// the presenter call (C# line 690: no RefreshEntries, no "stale" notice), exactly as a dialog whose window is gone: the golden records that.
//
// Conventions as W8-18t: every output case is the input case with the expected fields added; CurrentCulture is Invariant; English catalog (the default localizer).
//
// Inputs (tests/golden-inputs/recovery-dialog-dismiss.json), per case:
//   entries (array of ids, journal order; each becomes a Move/Failed JournalEntry "C:\src\<id>.jpg" -> "C:\dst\<id>.jpg", size 10)
//   rows (array of row indices handed to Dismiss), has_delegate (bool), answer (bool: the user presses Yes), mode ("ok" | "throws"), skip (array of ids the journal reports stale)
// Output fields added:
//   messages (array of { title, message, kind } in the order shown; kind is the TaskDialogKind name), delegate_calls (array of arrays of entry ids),
//   rows_left (array of the ids still in Presenter.Rows)

public class W818vMessageProxy : System.Reflection.DispatchProxy
{
    public static List<JsonObject> Calls { get; } = [];

    public static bool Answer { get; set; } = true;

    // int Show(nint owner, string title, string message, TaskDialogKind kind): IDYES 6 / IDNO 7 for a confirmation, IDOK 1 otherwise (as the C# FakeMessages).
    protected override object? Invoke(MethodInfo? targetMethod, object?[]? args)
    {
        string kind = args![3]!.ToString()!;
        Calls.Add(new JsonObject { ["title"] = (string)args[1]!, ["message"] = (string)args[2]!, ["kind"] = kind });
        return kind == "Confirmation" ? (Answer ? 6 : 7) : 1;
    }
}

static partial class Commands
{
    private const string W818vAssembly = "PhotoReview";
    private const BindingFlags W818vFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;
    private static readonly DateTime W818vStamp = new(2026, 1, 2, 3, 4, 5, DateTimeKind.Utc);

    private static JournalEntry W818vEntry(string id) =>
        new(id, FileOperationType.Move, JournalState.Failed, @"C:\src\" + id + ".jpg", @"C:\dst\" + id + ".jpg", 10L, W818vStamp, W818vStamp);

    public static JsonArray W818vRecoveryDialogDismissCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        Assembly shell = Assembly.Load(W818vAssembly);
        Type dialogType = shell.GetType("PhotoReview.Shell.Win32.Dialogs.Native.RecoveryDialog", throwOnError: true)!;
        Type portType = shell.GetType("PhotoReview.Shell.Win32.Dialogs.ITaskDialogPort", throwOnError: true)!;
        Assembly app = typeof(RecoveryFilter).Assembly;
        Type presenterType = app.GetType("PhotoReview.App.Presenters.RecoveryWindowPresenter", throwOnError: true)!;
        Type rowType = app.GetType("PhotoReview.App.Presenters.RecoveryRowModel", throwOnError: true)!;
        MethodInfo create = typeof(DispatchProxy).GetMethods().First(m => m.Name == "Create" && m.GetGenericArguments().Length == 2);
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var entries = c["entries"]!.AsArray().Select(n => W818vEntry(n!.GetValue<string>())).ToList();
            var delegateCalls = new JsonArray();
            var skip = c["skip"]!.AsArray().Select(n => n!.GetValue<string>()).ToHashSet(StringComparer.Ordinal);
            bool throws = c["mode"]!.GetValue<string>() == "throws";
            Func<IReadOnlyList<JournalEntry>, DismissOutcome>? dismiss = null;
            if (c["has_delegate"]!.GetValue<bool>())
            {
                dismiss = list =>
                {
                    var ids = new JsonArray();
                    foreach (var e in list)
                    {
                        ids.Add(e.Id);
                    }

                    delegateCalls.Add(ids);
                    if (throws)
                    {
                        throw new InvalidOperationException("journal locked");
                    }

                    var skipped = list.Where(e => skip.Contains(e.Id)).ToList();
                    return new DismissOutcome(list.Where(e => !skip.Contains(e.Id)).ToList(), skipped);
                };
            }

            object presenter = Activator.CreateInstance(presenterType, W818vFlags, null, [entries, null, dismiss, null, null], CultureInfo.InvariantCulture)!;
            object proxy = create.MakeGenericMethod(portType, typeof(W818vMessageProxy)).Invoke(null, null)!;
            W818vMessageProxy.Calls.Clear();
            W818vMessageProxy.Answer = c["answer"]!.GetValue<bool>();

            object dialog = RuntimeHelpers.GetUninitializedObject(dialogType);
            dialogType.GetField("_presenter", W818vFlags)!.SetValue(dialog, presenter);
            dialogType.GetField("_messages", W818vFlags)!.SetValue(dialog, proxy);

            var allRows = (IList)presenterType.GetProperty("Rows")!.GetValue(presenter)!;
            var chosen = (IList)Activator.CreateInstance(typeof(List<>).MakeGenericType(rowType))!;
            foreach (var index in c["rows"]!.AsArray())
            {
                chosen.Add(allRows[index!.GetValue<int>()]);
            }

            dialogType.GetMethod("Dismiss", W818vFlags)!.Invoke(dialog, [chosen]);

            var messages = new JsonArray();
            foreach (var call in W818vMessageProxy.Calls)
            {
                messages.Add(call.DeepClone());
            }

            var left = new JsonArray();
            foreach (var row in (IList)presenterType.GetProperty("Rows")!.GetValue(presenter)!)
            {
                var entry = (JournalEntry)rowType.GetProperty("Entry")!.GetValue(row)!;
                left.Add(entry.Id);
            }

            c["messages"] = messages;
            c["delegate_calls"] = delegateCalls;
            c["rows_left"] = left;
            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
