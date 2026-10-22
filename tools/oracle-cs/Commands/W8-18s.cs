using System.Collections;
using System.Globalization;
using System.Linq.Expressions;
using System.Reflection;
using System.Text.Json.Nodes;

// W8-18s oracle (BenchmarkDialog: the row colours of the result grid, the profile list text and badge, the static text colours; the F half of the unit).
// NOT compiled against the baseline or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18s
//     "dialog-benchmark-results" => Commands.W818sBenchmarkResultsCases(inputs),
//
// Wiring (the lead does this; nothing new if W8-18r is wired): the same assembly references as W8-18r (PhotoReview, PhotoReview.App.Shared, PhotoReview.Benchmarking,
// PhotoReview.Core). Everything is reached by reflection; no InternalsVisibleTo and no change to the C# repo.
//
// How the dialog is obtained: BenchmarkDialog is built with its real constructor (as in W8-18r: ImmediateUiScheduler, a session factory that returns null, every other
// argument null); no window is created. The result rows are added to `Presenter.Rows` (an ObservableCollection nobody listens to before OnInitialized). The private
// RowColors, the protected ListItemText and StaticTextColor are called by reflection on that object; they never touch the window.
//
// Conventions as W8-18d: every case object is the input case with the expected fields added; the UI language is the process default (English).
//
// Inputs (tests/golden-inputs/dialog-benchmark-results.json), kinds and the fields added:
//   row_color:    rows [{profile_id, workload, samples, status, is_best (optional)}], displayed_row (int), source (int; may be outside the rows)
//                                                       -> back, text (uint): the two colours of RowColors(displayedRow, source)
//   profile_list: (none)                                -> items [{id, text, badge}] for every profile of the registry, in order: ListItemText(ProfilesId, index) and the profile id
//   static_color: id (int)                              -> value (uint): StaticTextColor(id)
//   palette:      key (string)                          -> value (uint): DialogColors.FromKey(key) for the four row keys (Dark.SuccessBackground, Dark.ErrorBackground,
//                                                          Dark.Panel, Dark.RowAlt) and Dark.TextSubtle / Dark.Window

static partial class Commands
{
    private const string W818sDialogsAssembly = "PhotoReview";
    private const string W818sAppAssembly = "PhotoReview.App.Shared";
    private const string W818sBenchmarkAssembly = "PhotoReview.Benchmarking";
    private const string W818sCoreAssembly = "PhotoReview.Core";
    private const string W818sNamespace = "PhotoReview.Shell.Win32.Dialogs.Native.";
    private const BindingFlags W818sFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W818sType(string name) => Assembly.Load(W818sDialogsAssembly).GetType(W818sNamespace + name, throwOnError: true)!;

    private static MethodInfo W818sMethod(Type type, string name)
    {
        for (Type? t = type; t is not null; t = t.BaseType)
        {
            MethodInfo? m = t.GetMethod(name, W818sFlags | BindingFlags.DeclaredOnly);
            if (m is not null)
            {
                return m;
            }
        }

        throw new InvalidOperationException("no method " + name);
    }

    private static object W818sNewDialog()
    {
        Type app = Assembly.Load(W818sAppAssembly).GetType("PhotoReview.App.Presenters.BenchmarkSessionFactory", throwOnError: true)!;
        MethodInfo invoke = app.GetMethod("Invoke")!;
        var parameters = invoke.GetParameters().Select(p => Expression.Parameter(p.ParameterType, p.Name)).ToArray();
        Delegate factory = Expression.Lambda(app, Expression.Constant(null, invoke.ReturnType), parameters).Compile();
        object ui = Assembly.Load(W818sCoreAssembly).GetType("PhotoReview.Core.Abstractions.ImmediateUiScheduler", throwOnError: true)!
            .GetField("Instance", BindingFlags.Public | BindingFlags.Static)!.GetValue(null)!;
        return Activator.CreateInstance(W818sType("BenchmarkDialog"), W818sFlags, binder: null,
            args: [null, ui, factory, null, null, null, null, null], culture: CultureInfo.InvariantCulture)!;
    }

    private static object W818sRow(JsonObject spec)
    {
        Assembly benchmarking = Assembly.Load(W818sBenchmarkAssembly);
        Type profiles = benchmarking.GetType("PhotoReview.Benchmarking.BenchmarkProfiles", throwOnError: true)!;
        Type phaseType = benchmarking.GetType("PhotoReview.Benchmarking.BenchmarkPhaseResult", throwOnError: true)!;
        Type workload = benchmarking.GetType("PhotoReview.Benchmarking.BenchmarkWorkload", throwOnError: true)!;
        Type status = benchmarking.GetType("PhotoReview.Benchmarking.BenchmarkResultStatus", throwOnError: true)!;
        string id = spec["profile_id"]!.GetValue<string>();
        object profile = ((IEnumerable)profiles.GetProperty("All")!.GetValue(null)!).Cast<object>()
            .First(p => (string)p.GetType().GetProperty("Id")!.GetValue(p)! == id);
        double[] samples = spec["samples"]!.AsArray().Select(n => n!.GetValue<double>()).ToArray();
        object phase = Activator.CreateInstance(phaseType, W818sFlags, binder: null,
            args: [id, Enum.Parse(workload, spec["workload"]!.GetValue<string>()), samples, Enum.Parse(status, spec["status"]!.GetValue<string>()), null, null],
            culture: CultureInfo.InvariantCulture)!;
        Type rowType = Assembly.Load(W818sAppAssembly).GetType("PhotoReview.App.BenchmarkResultRow", throwOnError: true)!;
        object row = Activator.CreateInstance(rowType, W818sFlags, binder: null, args: [profile, phase, null], culture: CultureInfo.InvariantCulture)!;
        if (spec["is_best"]?.GetValue<bool>() == true)
        {
            rowType.GetProperty("IsBest")!.SetValue(row, true);
        }

        return row;
    }

    public static JsonArray W818sBenchmarkResultsCases(JsonObject inputs)
    {
        Type dialogType = W818sType("BenchmarkDialog");
        Type colors = W818sType("DialogColors");
        const int profilesId = 109;
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "row_color":
                {
                    object dialog = W818sNewDialog();
                    object presenter = dialogType.GetProperty("Presenter", W818sFlags)!.GetValue(dialog)!;
                    var rows = (IList)presenter.GetType().GetProperty("Rows")!.GetValue(presenter)!;
                    foreach (var spec in c["rows"]!.AsArray())
                    {
                        rows.Add(W818sRow(spec!.AsObject()));
                    }

                    // private (uint Back, uint Text) RowColors(int displayedRow, nint source)
                    object tuple = dialogType.GetMethod("RowColors", W818sFlags)!.Invoke(dialog,
                        [c["displayed_row"]!.GetValue<int>(), (nint)c["source"]!.GetValue<int>()])!;
                    c["back"] = (uint)tuple.GetType().GetField("Item1")!.GetValue(tuple)!;
                    c["text"] = (uint)tuple.GetType().GetField("Item2")!.GetValue(tuple)!;
                    break;
                }

                case "profile_list":
                {
                    object dialog = W818sNewDialog();
                    object presenter = dialogType.GetProperty("Presenter", W818sFlags)!.GetValue(dialog)!;
                    var items = (IList)presenter.GetType().GetProperty("ProfileItems")!.GetValue(presenter)!;
                    MethodInfo listItemText = W818sMethod(dialogType, "ListItemText");
                    var result = new JsonArray();
                    for (int i = 0; i < items.Count; i++)
                    {
                        object profile = items[i]!.GetType().GetProperty("Profile")!.GetValue(items[i])!;
                        object tuple = listItemText.Invoke(dialog, [profilesId, i])!;
                        result.Add(new JsonObject
                        {
                            ["id"] = (string)profile.GetType().GetProperty("Id")!.GetValue(profile)!,
                            ["text"] = (string)tuple.GetType().GetField("Item1")!.GetValue(tuple)!,
                            ["badge"] = (string?)tuple.GetType().GetField("Item2")!.GetValue(tuple),
                        });
                    }

                    c["items"] = result;
                    break;
                }

                case "static_color":
                {
                    object dialog = W818sNewDialog();
                    c["value"] = (uint)W818sMethod(dialogType, "StaticTextColor").Invoke(dialog, [c["id"]!.GetValue<int>()])!;
                    break;
                }

                case "palette":
                    c["value"] = (uint)colors.GetMethod("FromKey", W818sFlags)!.Invoke(null, [c["key"]!.GetValue<string>()])!;
                    break;
                default:
                    throw new InvalidOperationException("unknown dialog-benchmark-results kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
