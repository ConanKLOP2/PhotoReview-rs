using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;

// W8-18p oracle (ZoomPromptDialog and DiagnosticsDialog; the F half of the unit). NOT compiled against the baseline or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18p
//     "dialog-zoom-diagnostics" => Commands.W818pZoomDiagnosticsCases(inputs),
//
// Wiring (the lead does this; nothing new if W8-15b / W8-18d / W8-18i are wired): OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" =
// PhotoReview.Shell.Win32) and PhotoReview.App.Shared.dll. Every type used here is `internal` and is reached by reflection, so no InternalsVisibleTo and no change to
// the C# repo is needed.
//
// How the dialogs are obtained: ZoomPromptDialog is built with its real constructor (it only creates the presenter and stores the chrome, no window).
// DiagnosticsDialog is built with its real constructor too (BuildItems runs in it). For the layout arithmetic a window-less dialog is
// RuntimeHelpers.GetUninitializedObject(type) with the private base fields _avgCharWidth, _lineHeight and an empty _childHosts set by reflection (as W8-18i): _hwnd
// stays 0, so Item(id) is 0, MeasureItemHeight is 0 and ButtonWidth(ids) is exactly Dx(50). Nothing here creates a window.
//
// Conventions as W8-18d: every case object is the input case with the expected fields added; Invariant culture; the language is the process default (English).
//
// Inputs (tests/golden-inputs/dialog-zoom-diagnostics.json), kinds and the fields added:
//   zoom_dialog:        current (int)                      -> title, items [{class, id, text, style, ex_style}], initial_focus_id, default_button_id, resizable, header_ids [],
//                                                             uses_modal_frame, extra_window_style, initial_text (the presenter's box text)
//   diagnostics_dialog: display [15 strings]               -> title, items [...], initial_focus_id, default_button_id, resizable, header_ids [..], uses_modal_frame,
//                                                             extra_window_style
//   diagnostics_text:   display [15 strings]               -> labels [15], values [15] (LabelText / ValueText per row), copy_text (BuildCopyText)
//   diagnostics_row:    row (int)                          -> label, value (strings) or threw (exception type name): rows outside 0..14
//   preferred:          type ("zoom" | "diagnostics"), avg, line (int), error_visible (bool, zoom only)
//                                                          -> preferred_width, preferred_height, minimum_width, minimum_height (PreferredClientSize / MinimumClientSize
//                                                             with no live control, so every measured text height is 0)
//   constants:          (none)                             -> zoom_label_id, zoom_edit_id, zoom_error_id, zoom_cancel_id, zoom_apply_id, diagnostics_header_id,
//                                                             diagnostics_note_id, diagnostics_first_label_id, diagnostics_first_value_id, diagnostics_copy_id,
//                                                             diagnostics_close_id, diagnostics_row_count (int each)

static partial class Commands
{
    private const string W818pDialogsAssembly = "PhotoReview";
    private const string W818pAppAssembly = "PhotoReview.App.Shared";
    private const string W818pNamespace = "PhotoReview.Shell.Win32.Dialogs.Native.";
    private const BindingFlags W818pFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W818pType(string name) => Assembly.Load(W818pDialogsAssembly).GetType(W818pNamespace + name, throwOnError: true)!;

    // A protected virtual member lives on the base class and may be overridden: walk from the runtime type up and take the first declaration.
    private static MethodInfo W818pGetter(Type type, string property)
    {
        for (Type? t = type; t is not null; t = t.BaseType)
        {
            PropertyInfo? p = t.GetProperty(property, W818pFlags | BindingFlags.DeclaredOnly);
            if (p?.GetGetMethod(nonPublic: true) is { } getter)
            {
                return getter;
            }
        }

        throw new InvalidOperationException("no property " + property);
    }

    private static object? W818pGet(object dialog, string property) => W818pGetter(dialog.GetType(), property).Invoke(dialog, null);

    private static MethodInfo W818pMethod(Type type, string name)
    {
        for (Type? t = type; t is not null; t = t.BaseType)
        {
            MethodInfo? m = t.GetMethod(name, W818pFlags | BindingFlags.DeclaredOnly);
            if (m is not null)
            {
                return m;
            }
        }

        throw new InvalidOperationException("no method " + name);
    }

    private static JsonArray W818pItems(object dialog)
    {
        var result = new JsonArray();
        foreach (object item in (System.Collections.IEnumerable)W818pGet(dialog, "Items")!)
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

    private static void W818pCommon(JsonObject c, object dialog)
    {
        c["title"] = (string)W818pGet(dialog, "Title")!;
        c["items"] = W818pItems(dialog);
        c["initial_focus_id"] = (int)W818pGet(dialog, "InitialFocusId")!;
        c["default_button_id"] = (int)W818pGet(dialog, "DefaultButtonId")!;
        c["resizable"] = (bool)W818pGet(dialog, "Resizable")!;
        var ids = new JsonArray();
        foreach (int id in (IEnumerable<int>)W818pGet(dialog, "HeaderControlIds")!)
        {
            ids.Add(id);
        }

        c["header_ids"] = ids;
        c["uses_modal_frame"] = (bool)W818pGet(dialog, "UsesModalFrame")!;
        c["extra_window_style"] = (uint)W818pGet(dialog, "ExtraWindowStyle")!;
    }

    private static object W818pDisplay(JsonObject c)
    {
        Type display = Assembly.Load(W818pAppAssembly).GetType("PhotoReview.App.Presenters.DiagnosticsDisplay", throwOnError: true)!;
        object?[] values = c["display"]!.AsArray().Select(n => (object?)n!.GetValue<string>()).ToArray();
        return Activator.CreateInstance(display, W818pFlags, binder: null, args: values, culture: CultureInfo.InvariantCulture)!;
    }

    private static object W818pDiagnostics(JsonObject c)
    {
        Type dialog = W818pType("DiagnosticsDialog");
        // DiagnosticsDialog(DiagnosticsDisplay display, Func<string, bool>? copy = null, INativeDialogChrome? chrome = null)
        return Activator.CreateInstance(dialog, W818pFlags, binder: null, args: [W818pDisplay(c), null, null], culture: CultureInfo.InvariantCulture)!;
    }

    private static object W818pBare(Type derived, int avg, int line)
    {
        Type baseType = W818pType("NativeDialog");
        object dialog = RuntimeHelpers.GetUninitializedObject(derived);
        baseType.GetField("_avgCharWidth", W818pFlags)!.SetValue(dialog, avg);
        baseType.GetField("_lineHeight", W818pFlags)!.SetValue(dialog, line);
        baseType.GetField("_childHosts", W818pFlags)!.SetValue(dialog, new List<nint>());
        return dialog;
    }

    private static (int, int) W818pSize(object boxed)
    {
        Type t = boxed.GetType();
        return ((int)t.GetField("Item1")!.GetValue(boxed)!, (int)t.GetField("Item2")!.GetValue(boxed)!);
    }

    public static JsonArray W818pZoomDiagnosticsCases(JsonObject inputs)
    {
        Type zoom = W818pType("ZoomPromptDialog");
        Type diagnostics = W818pType("DiagnosticsDialog");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "zoom_dialog":
                {
                    // ZoomPromptDialog(int currentPercent, INativeDialogChrome? chrome = null)
                    object dialog = Activator.CreateInstance(zoom, W818pFlags, binder: null, args: [c["current"]!.GetValue<int>(), null],
                        culture: CultureInfo.InvariantCulture)!;
                    W818pCommon(c, dialog);
                    object presenter = zoom.GetField("_presenter", W818pFlags)!.GetValue(dialog)!;
                    c["initial_text"] = (string)presenter.GetType().GetProperty("InitialText", W818pFlags)!.GetValue(presenter)!;
                    break;
                }

                case "diagnostics_dialog":
                    W818pCommon(c, W818pDiagnostics(c));
                    break;
                case "diagnostics_text":
                {
                    object display = W818pDisplay(c);
                    var labels = new JsonArray();
                    var values = new JsonArray();
                    MethodInfo label = diagnostics.GetMethod("LabelText", W818pFlags)!;
                    MethodInfo value = diagnostics.GetMethod("ValueText", W818pFlags)!;
                    for (int row = 0; row < 15; row++)
                    {
                        labels.Add((string)label.Invoke(null, [row])!);
                        values.Add((string)value.Invoke(null, [display, row])!);
                    }

                    c["labels"] = labels;
                    c["values"] = values;
                    c["copy_text"] = (string)diagnostics.GetMethod("BuildCopyText", W818pFlags)!.Invoke(null, [display])!;
                    break;
                }

                case "diagnostics_row":
                {
                    int row = c["row"]!.GetValue<int>();
                    try
                    {
                        c["label"] = (string)diagnostics.GetMethod("LabelText", W818pFlags)!.Invoke(null, [row])!;
                        c["value"] = (string)diagnostics.GetMethod("ValueText", W818pFlags)!.Invoke(null, [W818pDisplay(c), row])!;
                    }
                    catch (TargetInvocationException ex)
                    {
                        c["threw"] = ex.InnerException!.GetType().Name;
                    }

                    break;
                }

                case "preferred":
                {
                    bool isZoom = c["type"]!.GetValue<string>() == "zoom";
                    object dialog = W818pBare(isZoom ? zoom : diagnostics, c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    if (isZoom)
                    {
                        zoom.GetField("_errorVisible", W818pFlags)!.SetValue(dialog, c["error_visible"]!.GetValue<bool>());
                    }

                    var preferred = W818pSize(W818pMethod(dialog.GetType(), "PreferredClientSize").Invoke(dialog, null)!);
                    var minimum = W818pSize(W818pMethod(dialog.GetType(), "MinimumClientSize").Invoke(dialog, null)!);
                    c["preferred_width"] = preferred.Item1;
                    c["preferred_height"] = preferred.Item2;
                    c["minimum_width"] = minimum.Item1;
                    c["minimum_height"] = minimum.Item2;
                    break;
                }

                case "constants":
                    c["zoom_label_id"] = (int)zoom.GetField("LabelId", W818pFlags)!.GetRawConstantValue()!;
                    c["zoom_edit_id"] = (int)zoom.GetField("EditId", W818pFlags)!.GetRawConstantValue()!;
                    c["zoom_error_id"] = (int)zoom.GetField("ErrorId", W818pFlags)!.GetRawConstantValue()!;
                    c["zoom_cancel_id"] = (int)zoom.GetField("CancelId", W818pFlags)!.GetRawConstantValue()!;
                    c["zoom_apply_id"] = (int)zoom.GetField("ApplyId", W818pFlags)!.GetRawConstantValue()!;
                    foreach (var (key, name) in new[]
                    {
                        ("diagnostics_header_id", "HeaderId"), ("diagnostics_note_id", "NoteId"), ("diagnostics_first_label_id", "FirstLabelId"),
                        ("diagnostics_first_value_id", "FirstValueId"), ("diagnostics_copy_id", "CopyId"), ("diagnostics_close_id", "CloseId"),
                        ("diagnostics_row_count", "RowCount"),
                    })
                    {
                        c[key] = (int)diagnostics.GetField(name, W818pFlags)!.GetRawConstantValue()!;
                    }

                    break;
                default:
                    throw new InvalidOperationException("unknown dialog-zoom-diagnostics kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
