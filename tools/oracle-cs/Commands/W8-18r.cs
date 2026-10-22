using System.Collections;
using System.Globalization;
using System.Linq.Expressions;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;

// W8-18r oracle (BenchmarkDialog: constants, items, column headers, row comparison, sort state and the dialog-unit arithmetic of the layout; the F unit). NOT compiled
// against the baseline or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18r
//     "dialog-benchmark-layout" => Commands.W818rBenchmarkLayoutCases(inputs),
//
// Wiring (the lead does this; nothing new if W8-15b / W8-18d / W8-18i are wired): OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" =
// PhotoReview.Shell.Win32), PhotoReview.App.Shared.dll, PhotoReview.Benchmarking.dll and PhotoReview.Core.dll. The dialog and presenter types used here are `internal`
// and are reached by reflection, so no InternalsVisibleTo and no change to the C# repo is needed.
//
// How the dialog is obtained: BenchmarkDialog is built with its real constructor. Its arguments: initialFolder = the case's value or null, ui = ImmediateUiScheduler.Instance
// (Core), sessionFactory = a delegate built with System.Linq.Expressions that returns null (never called), everything else null. The constructor builds the presenter and the
// item list and subscribes nothing (subscriptions are made in OnInitialized, which needs a window), so no window is created. For the layout arithmetic a window-less dialog
// is RuntimeHelpers.GetUninitializedObject with _avgCharWidth, _lineHeight and an empty _childHosts set by reflection (as W8-18i): _hwnd stays 0, so Item(id) is 0,
// MeasureItemHeight is 0 and ButtonWidth(ids) is exactly Dx(50).
//
// Conventions as W8-18d: every case object is the input case with the expected fields added; Invariant culture for numbers; the UI language is the process default
// (English). The text-column comparisons use pairs that differ in their first letter, so the answer does not depend on the machine's collation tailoring.
//
// Inputs (tests/golden-inputs/dialog-benchmark-layout.json), kinds and the fields added:
//   dialog:         initial_folder (string or null)     -> title, items [{class, id, text, style, ex_style}], initial_focus_id, default_button_id, resizable, header_ids [..],
//                                                          uses_modal_frame, extra_window_style, initial_folder_text (the private _initialFolder when initial_folder is not null)
//   column_headers: (none)                              -> headers [10]
//   compare_rows:   left, right {profile_id, workload ("Sequential"...), samples [double], status ("Pass"|"Warn"|"Fail"|"InsufficientData"), is_best (bool, optional)}, column (int)
//                                                       -> sign (-1, 0, 1: Math.Sign of CompareRows)
//   sort_clicks:    clicks [int]                        -> states [[column, direction]] after each SortBy(column) (the first state is the one after the first click)
//   preferred:      avg, line (int)                     -> preferred_width, preferred_height, minimum_width, minimum_height (no live control: every measured height is 0)
//   metrics:        avg, line (int)                     -> margin, field_height, button_height (the private Margin, FieldHeight, ButtonHeight)
//   constants:      (none)                              -> the 19 ids (header_id ... close_id) and column_count

static partial class Commands
{
    private const string W818rDialogsAssembly = "PhotoReview";
    private const string W818rAppAssembly = "PhotoReview.App.Shared";
    private const string W818rBenchmarkAssembly = "PhotoReview.Benchmarking";
    private const string W818rCoreAssembly = "PhotoReview.Core";
    private const string W818rNamespace = "PhotoReview.Shell.Win32.Dialogs.Native.";
    private const BindingFlags W818rFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W818rType(string name) => Assembly.Load(W818rDialogsAssembly).GetType(W818rNamespace + name, throwOnError: true)!;

    private static MethodInfo W818rGetter(Type type, string property)
    {
        for (Type? t = type; t is not null; t = t.BaseType)
        {
            PropertyInfo? p = t.GetProperty(property, W818rFlags | BindingFlags.DeclaredOnly);
            if (p?.GetGetMethod(nonPublic: true) is { } getter)
            {
                return getter;
            }
        }

        throw new InvalidOperationException("no property " + property);
    }

    private static object? W818rGet(object dialog, string property) => W818rGetter(dialog.GetType(), property).Invoke(dialog, null);

    private static MethodInfo W818rMethod(Type type, string name)
    {
        for (Type? t = type; t is not null; t = t.BaseType)
        {
            MethodInfo? m = t.GetMethod(name, W818rFlags | BindingFlags.DeclaredOnly);
            if (m is not null)
            {
                return m;
            }
        }

        throw new InvalidOperationException("no method " + name);
    }

    private static object W818rNewDialog(string? initialFolder)
    {
        Type app = Assembly.Load(W818rAppAssembly).GetType("PhotoReview.App.Presenters.BenchmarkSessionFactory", throwOnError: true)!;
        MethodInfo invoke = app.GetMethod("Invoke")!;
        var parameters = invoke.GetParameters().Select(p => Expression.Parameter(p.ParameterType, p.Name)).ToArray();
        Delegate factory = Expression.Lambda(app, Expression.Constant(null, invoke.ReturnType), parameters).Compile();
        object ui = Assembly.Load(W818rCoreAssembly).GetType("PhotoReview.Core.Abstractions.ImmediateUiScheduler", throwOnError: true)!
            .GetField("Instance", BindingFlags.Public | BindingFlags.Static)!.GetValue(null)!;
        // BenchmarkDialog(string? initialFolder, IUiScheduler ui, BenchmarkSessionFactory sessionFactory, pickFolder, openInExplorer, scan, save, chrome)
        return Activator.CreateInstance(W818rType("BenchmarkDialog"), W818rFlags, binder: null,
            args: [initialFolder, ui, factory, null, null, null, null, null], culture: CultureInfo.InvariantCulture)!;
    }

    private static object W818rBare(int avg, int line)
    {
        object dialog = RuntimeHelpers.GetUninitializedObject(W818rType("BenchmarkDialog"));
        Type baseType = W818rType("NativeDialog");
        baseType.GetField("_avgCharWidth", W818rFlags)!.SetValue(dialog, avg);
        baseType.GetField("_lineHeight", W818rFlags)!.SetValue(dialog, line);
        baseType.GetField("_childHosts", W818rFlags)!.SetValue(dialog, new List<nint>());
        return dialog;
    }

    private static (int, int) W818rSize(object boxed)
    {
        Type t = boxed.GetType();
        return ((int)t.GetField("Item1")!.GetValue(boxed)!, (int)t.GetField("Item2")!.GetValue(boxed)!);
    }

    private static object W818rRow(JsonObject spec)
    {
        Assembly benchmarking = Assembly.Load(W818rBenchmarkAssembly);
        Type profiles = benchmarking.GetType("PhotoReview.Benchmarking.BenchmarkProfiles", throwOnError: true)!;
        Type phaseType = benchmarking.GetType("PhotoReview.Benchmarking.BenchmarkPhaseResult", throwOnError: true)!;
        Type workload = benchmarking.GetType("PhotoReview.Benchmarking.BenchmarkWorkload", throwOnError: true)!;
        Type status = benchmarking.GetType("PhotoReview.Benchmarking.BenchmarkResultStatus", throwOnError: true)!;
        string id = spec["profile_id"]!.GetValue<string>();
        object? profile = ((IEnumerable)profiles.GetProperty("All")!.GetValue(null)!).Cast<object>()
            .FirstOrDefault(p => (string)p.GetType().GetProperty("Id")!.GetValue(p)! == id);
        if (profile is null)
        {
            throw new InvalidOperationException("unknown profile " + id);
        }

        double[] samples = spec["samples"]!.AsArray().Select(n => n!.GetValue<double>()).ToArray();
        // BenchmarkPhaseResult(string ProfileId, BenchmarkWorkload Workload, IReadOnlyList<double> Samples, BenchmarkResultStatus Status, string? Message, int? ImagesPerSample)
        object phase = Activator.CreateInstance(phaseType, W818rFlags, binder: null,
            args: [id, Enum.Parse(workload, spec["workload"]!.GetValue<string>()), samples, Enum.Parse(status, spec["status"]!.GetValue<string>()), null, null],
            culture: CultureInfo.InvariantCulture)!;
        Type rowType = Assembly.Load(W818rAppAssembly).GetType("PhotoReview.App.BenchmarkResultRow", throwOnError: true)!;
        // BenchmarkResultRow(BenchmarkProfile profile, BenchmarkPhaseResult phase, Exception? error = null)
        object row = Activator.CreateInstance(rowType, W818rFlags, binder: null, args: [profile, phase, null], culture: CultureInfo.InvariantCulture)!;
        if (spec["is_best"]?.GetValue<bool>() == true)
        {
            rowType.GetProperty("IsBest")!.SetValue(row, true);
        }

        return row;
    }

    public static JsonArray W818rBenchmarkLayoutCases(JsonObject inputs)
    {
        Type dialogType = W818rType("BenchmarkDialog");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "dialog":
                {
                    string? folder = c["initial_folder"]?.GetValue<string>();
                    object dialog = W818rNewDialog(folder);
                    c["title"] = (string)W818rGet(dialog, "Title")!;
                    var items = new JsonArray();
                    foreach (object item in (IEnumerable)W818rGet(dialog, "Items")!)
                    {
                        Type t = item.GetType();
                        items.Add(new JsonObject
                        {
                            ["class"] = t.GetProperty("Class")!.GetValue(item)!.ToString(),
                            ["id"] = (int)t.GetProperty("Id")!.GetValue(item)!,
                            ["text"] = (string?)t.GetProperty("Text")!.GetValue(item) ?? string.Empty,
                            ["style"] = (uint)t.GetProperty("Style")!.GetValue(item)!,
                            ["ex_style"] = (uint)t.GetProperty("ExStyle")!.GetValue(item)!,
                        });
                    }

                    c["items"] = items;
                    c["initial_focus_id"] = (int)W818rGet(dialog, "InitialFocusId")!;
                    c["default_button_id"] = (int)W818rGet(dialog, "DefaultButtonId")!;
                    c["resizable"] = (bool)W818rGet(dialog, "Resizable")!;
                    var ids = new JsonArray();
                    foreach (int id in (IEnumerable<int>)W818rGet(dialog, "HeaderControlIds")!)
                    {
                        ids.Add(id);
                    }

                    c["header_ids"] = ids;
                    c["uses_modal_frame"] = (bool)W818rGet(dialog, "UsesModalFrame")!;
                    c["extra_window_style"] = (uint)W818rGet(dialog, "ExtraWindowStyle")!;
                    if (folder is not null)
                    {
                        c["initial_folder_text"] = (string)dialogType.GetField("_initialFolder", W818rFlags)!.GetValue(dialog)!;
                    }

                    break;
                }

                case "column_headers":
                {
                    var headers = new JsonArray();
                    foreach (string header in (string[])dialogType.GetMethod("ColumnHeaders", W818rFlags)!.Invoke(null, null)!)
                    {
                        headers.Add(header);
                    }

                    c["headers"] = headers;
                    break;
                }

                case "compare_rows":
                {
                    object left = W818rRow(c["left"]!.AsObject());
                    object right = W818rRow(c["right"]!.AsObject());
                    int raw = (int)dialogType.GetMethod("CompareRows", W818rFlags)!.Invoke(null, [left, right, c["column"]!.GetValue<int>()])!;
                    c["sign"] = Math.Sign(raw);
                    break;
                }

                case "sort_clicks":
                {
                    object dialog = W818rNewDialog(null);
                    MethodInfo sortBy = dialogType.GetMethod("SortBy", W818rFlags)!;
                    PropertyInfo sort = dialogType.GetProperty("Sort", W818rFlags)!;
                    var states = new JsonArray();
                    foreach (var click in c["clicks"]!.AsArray())
                    {
                        sortBy.Invoke(dialog, [click!.GetValue<int>()]);
                        var (column, direction) = W818rSize(sort.GetValue(dialog)!);
                        states.Add(new JsonArray(column, direction));
                    }

                    c["states"] = states;
                    break;
                }

                case "preferred":
                {
                    object dialog = W818rBare(c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    var preferred = W818rSize(W818rMethod(dialogType, "PreferredClientSize").Invoke(dialog, null)!);
                    var minimum = W818rSize(W818rMethod(dialogType, "MinimumClientSize").Invoke(dialog, null)!);
                    c["preferred_width"] = preferred.Item1;
                    c["preferred_height"] = preferred.Item2;
                    c["minimum_width"] = minimum.Item1;
                    c["minimum_height"] = minimum.Item2;
                    break;
                }

                case "metrics":
                {
                    object dialog = W818rBare(c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    c["margin"] = (int)W818rGet(dialog, "Margin")!;
                    c["field_height"] = (int)W818rGet(dialog, "FieldHeight")!;
                    c["button_height"] = (int)W818rGet(dialog, "ButtonHeight")!;
                    break;
                }

                case "constants":
                    foreach (var (key, name) in new[]
                    {
                        ("header_id", "HeaderId"), ("description_id", "DescriptionId"), ("folder_id", "FolderId"), ("browse_id", "BrowseId"),
                        ("limit_label_id", "LimitLabelId"), ("limit_id", "LimitId"), ("profiles_label_id", "ProfilesLabelId"), ("select_all_id", "SelectAllId"),
                        ("select_none_id", "SelectNoneId"), ("profiles_id", "ProfilesId"), ("run_id", "RunId"), ("quick_check_id", "QuickCheckId"),
                        ("cancel_run_id", "CancelRunId"), ("progress_id", "ProgressId"), ("status_id", "StatusId"), ("results_id", "ResultsId"),
                        ("open_report_id", "OpenReportId"), ("open_folder_id", "OpenFolderId"), ("close_id", "CloseId"), ("column_count", "ColumnCount"),
                    })
                    {
                        c[key] = (int)dialogType.GetField(name, W818rFlags)!.GetRawConstantValue()!;
                    }

                    break;
                default:
                    throw new InvalidOperationException("unknown dialog-benchmark-layout kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
