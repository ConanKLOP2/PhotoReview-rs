using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;
using PhotoReview.App;
using PhotoReview.Core.Localization;

// W8-18u oracle (the F half of the unit): RecoveryDialog.BuildDetailsText (the scrollable details text), StaticTextColor (through TextColorOf), the Explorer
// arguments of ShowInExplorer and the Copy seam. NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18u
//     "recovery-dialog-details" => Commands.W818uRecoveryDialogDetailsCases(inputs),
//
// Wiring (the lead does this; nothing new if W8-18t / W8-18i are wired): the same references as W8-18t (PhotoReview.dll, PhotoReview.Core.dll for Tr,
// PhotoReview.App.Shared.dll). RecoveryDetails, RecoveryPathView and RecoveryGroupMemberView are `internal sealed record`s of namespace PhotoReview.App.Presenters and
// are built with Activator.CreateInstance (non-public constructors); RecoveryTone is public. No InternalsVisibleTo, no change to the C# repo.
//
// A window-less dialog is RuntimeHelpers.GetUninitializedObject(typeof(RecoveryDialog)) (no constructor, no chrome, no window); the private readonly fields the code under
// test reads are set by reflection (_explorer, _copy; _avgCharWidth/_lineHeight/_childHosts are not needed here). The error branch of ShowInExplorer (Win32Exception ->
// Notify) is NOT in the golden: it needs the ITaskDialogPort seam (W8-18v shows how a proxy is made) and is a hand-written Rust test from RecoveryDialogTests.
//
// Conventions as W8-18t: every output case is the input case with the expected fields added; CurrentCulture is Invariant; English catalog.
// The oracle creates a fresh temp root "$ROOT" with the files a.jpg, "with space.jpg" and sub\b.jpg and the folder sub, and removes it afterwards; "$NODRIVE" is
// the first drive letter from Z down to D whose root does not exist (e.g. "Z:"); outputs never contain either value (turned back into the tokens).
//
// Inputs (tests/golden-inputs/recovery-dialog-details.json), kinds and the fields added:
//   details_text: explain, action (string); source, destination ({ title, path, status_text, status_tone, now, journal, note|null } or null);
//                 source_visible, destination_visible (bool); group_heading (string|null); group_members ([{ status, source, destination }] | null)
//                                                    -> text (BuildDetailsText), action_label (Tr.RecoveryDetailAction)
//   text_color:   verdict_tone, source_tone, destination_tone (RecoveryTone names), id_name (e.g. "VerdictBadgeId", a const of RecoveryDialog)
//                                                    -> color (uint: TextColorOf(id))
//   explorer_args: path (string, tokens allowed)      -> args (array of the strings the Explorer seam received, 0 or 1 entries)
//   copy:          path (string)                      -> calls (array of the strings the Copy seam received, 0 or 1 entries)

static partial class Commands
{
    private const string W818uAssembly = "PhotoReview";
    private const string W818uNamespace = "PhotoReview.Shell.Win32.Dialogs.Native.";
    private const string W818uPresenters = "PhotoReview.App.Presenters.";
    private const BindingFlags W818uFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W818uType(string name) => Assembly.Load(W818uAssembly).GetType(W818uNamespace + name, throwOnError: true)!;

    private static Type W818uAppType(string name) => typeof(RecoveryFilter).Assembly.GetType(W818uPresenters + name, throwOnError: true)!;

    private static object W818uNew(Type type, params object?[] args) => Activator.CreateInstance(type, W818uFlags, null, args, CultureInfo.InvariantCulture)!;

    private static RecoveryTone W818uTone(JsonNode? node) => Enum.Parse<RecoveryTone>(node!.GetValue<string>());

    private static object? W818uPathView(JsonNode? node)
    {
        if (node is null)
        {
            return null;
        }

        var o = node.AsObject();
        return W818uNew(W818uAppType("RecoveryPathView"), o["title"]!.GetValue<string>(), o["path"]!.GetValue<string>(), o["status_text"]!.GetValue<string>(),
            W818uTone(o["status_tone"]), o["now"]!.GetValue<string>(), o["journal"]!.GetValue<string>(), o["note"]?.GetValue<string>());
    }

    private static object? W818uMembers(JsonNode? node)
    {
        if (node is null)
        {
            return null;
        }

        Type memberType = W818uAppType("RecoveryGroupMemberView");
        var list = (IList)Activator.CreateInstance(typeof(List<>).MakeGenericType(memberType))!;
        foreach (var item in node.AsArray())
        {
            var m = item!.AsObject();
            list.Add(W818uNew(memberType, m["status"]!.GetValue<string>(), m["source"]!.GetValue<string>(), m["destination"]!.GetValue<string>()));
        }

        return list;
    }

    private static object W818uDetails(JsonObject c) => W818uNew(W818uAppType("RecoveryDetails"),
        string.Empty, false, "verdict", RecoveryTone.Neutral, c["explain"]!.GetValue<string>(), c["action"]!.GetValue<string>(),
        W818uPathView(c["source"]), c["source_visible"]!.GetValue<bool>(), W818uPathView(c["destination"]), c["destination_visible"]!.GetValue<bool>(),
        false, c["group_heading"]?.GetValue<string>(), W818uMembers(c["group_members"]));

    private static object W818uBare(Type dialogType) => RuntimeHelpers.GetUninitializedObject(dialogType);

    private static string W818uRoot(string path, string root, string noDrive) => path.Replace(root, "$ROOT", StringComparison.Ordinal).Replace(noDrive, "$NODRIVE", StringComparison.Ordinal);

    private static string W818uNoDrive()
    {
        for (char letter = 'Z'; letter >= 'D'; letter--)
        {
            if (!Directory.Exists(letter + ":\\"))
            {
                return letter + ":";
            }
        }

        throw new InvalidOperationException("no free drive letter for the $NODRIVE token");
    }

    public static JsonArray W818uRecoveryDialogDetailsCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        Type dialogType = W818uType("RecoveryDialog");
        string root = Path.Combine(Path.GetTempPath(), "w818u-" + Guid.NewGuid().ToString("N"));
        string noDrive = W818uNoDrive();
        Directory.CreateDirectory(Path.Combine(root, "sub"));
        File.WriteAllText(Path.Combine(root, "a.jpg"), "a");
        File.WriteAllText(Path.Combine(root, "with space.jpg"), "a");
        File.WriteAllText(Path.Combine(root, "sub", "b.jpg"), "b");
        var outCases = new JsonArray();
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                switch (c["kind"]!.GetValue<string>())
                {
                    case "details_text":
                        c["text"] = (string)dialogType.GetMethod("BuildDetailsText", W818uFlags)!.Invoke(null, [W818uDetails(c)])!;
                        c["action_label"] = Tr.RecoveryDetailAction;
                        break;

                    case "text_color":
                    {
                        object d = W818uBare(dialogType);
                        dialogType.GetField("_verdictTone", W818uFlags)!.SetValue(d, W818uTone(c["verdict_tone"]));
                        dialogType.GetField("_sourceTone", W818uFlags)!.SetValue(d, W818uTone(c["source_tone"]));
                        dialogType.GetField("_destinationTone", W818uFlags)!.SetValue(d, W818uTone(c["destination_tone"]));
                        int id = (int)dialogType.GetField(c["id_name"]!.GetValue<string>(), W818uFlags)!.GetRawConstantValue()!;
                        c["color"] = (uint)dialogType.GetMethod("TextColorOf", W818uFlags)!.Invoke(d, [id])!;
                        break;
                    }

                    case "explorer_args":
                    {
                        var seen = new List<string>();
                        object d = W818uBare(dialogType);
                        dialogType.GetField("_explorer", W818uFlags)!.SetValue(d, (Action<string>)seen.Add);
                        string path = c["path"]!.GetValue<string>().Replace("$ROOT", root, StringComparison.Ordinal).Replace("$NODRIVE", noDrive, StringComparison.Ordinal);
                        dialogType.GetMethod("ShowInExplorer", W818uFlags)!.Invoke(d, [path]);
                        var args = new JsonArray();
                        foreach (string a in seen)
                        {
                            args.Add(W818uRoot(a, root, noDrive));
                        }

                        c["args"] = args;
                        break;
                    }

                    case "copy":
                    {
                        var seen = new List<string>();
                        object d = W818uBare(dialogType);
                        dialogType.GetField("_copy", W818uFlags)!.SetValue(d, (Func<string, bool>)(p =>
                        {
                            seen.Add(p);
                            return true;
                        }));
                        dialogType.GetMethod("Copy", W818uFlags)!.Invoke(d, [c["path"]!.GetValue<string>()]);
                        var calls = new JsonArray();
                        foreach (string a in seen)
                        {
                            calls.Add(a);
                        }

                        c["calls"] = calls;
                        break;
                    }

                    default:
                        throw new InvalidOperationException("unknown recovery-dialog-details kind");
                }

                outCases.Add(c.DeepClone());
            }
        }
        finally
        {
            try
            {
                Directory.Delete(root, recursive: true);
            }
            catch (IOException)
            {
            }
            catch (UnauthorizedAccessException)
            {
            }
        }

        return outCases;
    }
}
