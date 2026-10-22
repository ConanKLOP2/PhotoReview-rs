using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;

// W8-18q oracle (ActionProfilesDialog and its helper FileFilterText; the F half of the unit). NOT compiled against the baseline or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18q
//     "dialog-action-profiles" => Commands.W818qActionProfilesCases(inputs),
//
// Wiring (the lead does this; nothing new if W8-15b / W8-18d / W8-18i are wired): OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" =
// PhotoReview.Shell.Win32) and PhotoReview.Core.dll. Every dialog type used here is `internal` and is reached by reflection, so no InternalsVisibleTo and no change to the
// C# repo is needed. ReviewAction and FileOperationType (public, Core) are also reached by reflection so this file compiles without the Core reference.
//
// How the dialog is obtained: ActionProfilesDialog is built with its real constructor (it only creates the presenter and the item list, no window). For the layout
// arithmetic a window-less dialog is RuntimeHelpers.GetUninitializedObject with the private base fields _avgCharWidth, _lineHeight and an empty _childHosts set by
// reflection (as W8-18i): _hwnd stays 0, so Item(id) is 0, MeasureItemHeight is 0 and ButtonWidth(ids) is exactly Dx(50). Nothing here creates a window.
//
// Conventions as W8-18d: every case object is the input case with the expected fields added; Invariant culture; the language is the process default (English).
//
// Inputs (tests/golden-inputs/dialog-action-profiles.json), kinds and the fields added:
//   dialog:            actions [{name, shortcut, operation ("Move" | "Copy" | "Recycle"), destination, confirm}]
//                                                       -> title, items [{class, id, text, style, ex_style}], initial_focus_id, default_button_id, resizable, header_ids [..],
//                                                          uses_modal_frame, extra_window_style
//   row_text:          actions [..], index (int)        -> text (string), badge (string or null): RowText(index), in range only
//   file_filter_parse: text (string)                    -> filters [{name, pattern}] (FileFilterText.Parse)
//   preferred:         avg, line (int)                  -> preferred_width, preferred_height, minimum_width, minimum_height (no live control: every measured height is 0)
//   metrics:           avg, line (int), client_width    -> margin, field_height, button_height, left_width (the private Margin, FieldHeight, ButtonHeight, LeftWidth(clientWidth))
//   constants:         (none)                           -> list_header_id, list_id, add_id, remove_id, import_id, export_id, details_header_id, name_label_id, name_id,
//                                                          shortcut_label_id, shortcut_id, operation_label_id, operation_id, destination_label_id, destination_id, confirm_id,
//                                                          cancel_id, apply_id (int each)

static partial class Commands
{
    private const string W818qDialogsAssembly = "PhotoReview";
    private const string W818qCoreAssembly = "PhotoReview.Core";
    private const string W818qNamespace = "PhotoReview.Shell.Win32.Dialogs.Native.";
    private const BindingFlags W818qFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W818qType(string name) => Assembly.Load(W818qDialogsAssembly).GetType(W818qNamespace + name, throwOnError: true)!;

    private static MethodInfo W818qGetter(Type type, string property)
    {
        for (Type? t = type; t is not null; t = t.BaseType)
        {
            PropertyInfo? p = t.GetProperty(property, W818qFlags | BindingFlags.DeclaredOnly);
            if (p?.GetGetMethod(nonPublic: true) is { } getter)
            {
                return getter;
            }
        }

        throw new InvalidOperationException("no property " + property);
    }

    private static object? W818qGet(object dialog, string property) => W818qGetter(dialog.GetType(), property).Invoke(dialog, null);

    private static MethodInfo W818qMethod(Type type, string name)
    {
        for (Type? t = type; t is not null; t = t.BaseType)
        {
            MethodInfo? m = t.GetMethod(name, W818qFlags | BindingFlags.DeclaredOnly);
            if (m is not null)
            {
                return m;
            }
        }

        throw new InvalidOperationException("no method " + name);
    }

    private static JsonArray W818qItems(IEnumerable items)
    {
        var result = new JsonArray();
        foreach (object item in items)
        {
            Type t = item.GetType();
            result.Add(new JsonObject
            {
                ["class"] = t.GetProperty("Class")!.GetValue(item)!.ToString(),
                ["id"] = (int)t.GetProperty("Id")!.GetValue(item)!,
                ["text"] = (string?)t.GetProperty("Text")!.GetValue(item) ?? string.Empty,
                ["style"] = (uint)t.GetProperty("Style")!.GetValue(item)!,
                ["ex_style"] = (uint)t.GetProperty("ExStyle")!.GetValue(item)!,
            });
        }

        return result;
    }

    // A List<ReviewAction> from the case's "actions" array (reflection on the public Core types).
    private static IList W818qActions(JsonObject c)
    {
        Assembly core = Assembly.Load(W818qCoreAssembly);
        Type action = core.GetType("PhotoReview.Core.Settings.ReviewAction", throwOnError: true)!;
        Type operation = core.GetType("PhotoReview.Core.Model.FileOperationType", throwOnError: true)!;
        var list = (IList)Activator.CreateInstance(typeof(List<>).MakeGenericType(action))!;
        foreach (var node in c["actions"]!.AsArray())
        {
            var a = node!.AsObject();
            object item = Activator.CreateInstance(action)!;
            action.GetProperty("Name")!.SetValue(item, a["name"]!.GetValue<string>());
            action.GetProperty("Shortcut")!.SetValue(item, a["shortcut"]!.GetValue<string>());
            action.GetProperty("Operation")!.SetValue(item, Enum.Parse(operation, a["operation"]!.GetValue<string>()));
            action.GetProperty("Destination")!.SetValue(item, a["destination"]!.GetValue<string>());
            action.GetProperty("Confirm")!.SetValue(item, a["confirm"]!.GetValue<bool>());
            list.Add(item);
        }

        return list;
    }

    private static object W818qDialog(JsonObject c)
    {
        // ActionProfilesDialog(IEnumerable<ReviewAction> actions, IShellFileDialog? files = null, IDialogWarningSink? warnings = null, INativeDialogChrome? chrome = null)
        return Activator.CreateInstance(W818qType("ActionProfilesDialog"), W818qFlags, binder: null, args: [W818qActions(c), null, null, null],
            culture: CultureInfo.InvariantCulture)!;
    }

    private static object W818qBare(int avg, int line)
    {
        object dialog = RuntimeHelpers.GetUninitializedObject(W818qType("ActionProfilesDialog"));
        Type baseType = W818qType("NativeDialog");
        baseType.GetField("_avgCharWidth", W818qFlags)!.SetValue(dialog, avg);
        baseType.GetField("_lineHeight", W818qFlags)!.SetValue(dialog, line);
        baseType.GetField("_childHosts", W818qFlags)!.SetValue(dialog, new List<nint>());
        return dialog;
    }

    private static (int, int) W818qSize(object boxed)
    {
        Type t = boxed.GetType();
        return ((int)t.GetField("Item1")!.GetValue(boxed)!, (int)t.GetField("Item2")!.GetValue(boxed)!);
    }

    public static JsonArray W818qActionProfilesCases(JsonObject inputs)
    {
        Type dialogType = W818qType("ActionProfilesDialog");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "dialog":
                {
                    object dialog = W818qDialog(c);
                    c["title"] = (string)W818qGet(dialog, "Title")!;
                    c["items"] = W818qItems((IEnumerable)W818qGet(dialog, "Items")!);
                    c["initial_focus_id"] = (int)W818qGet(dialog, "InitialFocusId")!;
                    c["default_button_id"] = (int)W818qGet(dialog, "DefaultButtonId")!;
                    c["resizable"] = (bool)W818qGet(dialog, "Resizable")!;
                    var ids = new JsonArray();
                    foreach (int id in (IEnumerable<int>)W818qGet(dialog, "HeaderControlIds")!)
                    {
                        ids.Add(id);
                    }

                    c["header_ids"] = ids;
                    c["uses_modal_frame"] = (bool)W818qGet(dialog, "UsesModalFrame")!;
                    c["extra_window_style"] = (uint)W818qGet(dialog, "ExtraWindowStyle")!;
                    break;
                }

                case "row_text":
                {
                    object dialog = W818qDialog(c);
                    // internal (string Text, string? Badge) RowText(int index): a ValueTuple<string, string?>.
                    object tuple = dialogType.GetMethod("RowText", W818qFlags)!.Invoke(dialog, [c["index"]!.GetValue<int>()])!;
                    c["text"] = (string)tuple.GetType().GetField("Item1")!.GetValue(tuple)!;
                    c["badge"] = (string?)tuple.GetType().GetField("Item2")!.GetValue(tuple);
                    break;
                }

                case "file_filter_parse":
                {
                    Type parser = W818qType("FileFilterText");
                    var filters = new JsonArray();
                    foreach (object filter in (IEnumerable)parser.GetMethod("Parse", W818qFlags)!.Invoke(null, [c["text"]!.GetValue<string>()])!)
                    {
                        Type t = filter.GetType();
                        filters.Add(new JsonObject
                        {
                            ["name"] = (string)t.GetProperty("Name")!.GetValue(filter)!,
                            ["pattern"] = (string)t.GetProperty("Pattern")!.GetValue(filter)!,
                        });
                    }

                    c["filters"] = filters;
                    break;
                }

                case "preferred":
                {
                    object dialog = W818qBare(c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    var preferred = W818qSize(W818qMethod(dialogType, "PreferredClientSize").Invoke(dialog, null)!);
                    var minimum = W818qSize(W818qMethod(dialogType, "MinimumClientSize").Invoke(dialog, null)!);
                    c["preferred_width"] = preferred.Item1;
                    c["preferred_height"] = preferred.Item2;
                    c["minimum_width"] = minimum.Item1;
                    c["minimum_height"] = minimum.Item2;
                    break;
                }

                case "metrics":
                {
                    object dialog = W818qBare(c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    c["margin"] = (int)W818qGet(dialog, "Margin")!;
                    c["field_height"] = (int)W818qGet(dialog, "FieldHeight")!;
                    c["button_height"] = (int)W818qGet(dialog, "ButtonHeight")!;
                    c["left_width"] = (int)dialogType.GetMethod("LeftWidth", W818qFlags)!.Invoke(dialog, [c["client_width"]!.GetValue<int>()])!;
                    break;
                }

                case "constants":
                    foreach (var (key, name) in new[]
                    {
                        ("list_header_id", "ListHeaderId"), ("list_id", "ListId"), ("add_id", "AddId"), ("remove_id", "RemoveId"), ("import_id", "ImportId"),
                        ("export_id", "ExportId"), ("details_header_id", "DetailsHeaderId"), ("name_label_id", "NameLabelId"), ("name_id", "NameId"),
                        ("shortcut_label_id", "ShortcutLabelId"), ("shortcut_id", "ShortcutId"), ("operation_label_id", "OperationLabelId"),
                        ("operation_id", "OperationId"), ("destination_label_id", "DestinationLabelId"), ("destination_id", "DestinationId"),
                        ("confirm_id", "ConfirmId"), ("cancel_id", "CancelId"), ("apply_id", "ApplyId"),
                    })
                    {
                        c[key] = (int)dialogType.GetField(name, W818qFlags)!.GetRawConstantValue()!;
                    }

                    break;
                default:
                    throw new InvalidOperationException("unknown dialog-action-profiles kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
