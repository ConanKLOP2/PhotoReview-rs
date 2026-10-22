using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;

// W8-19j oracle (the pure half of the unit; the plan says N, the contract adds this table test): the frame of SettingsDialog without a window: the template items, the ids
// and the page id arithmetic, the constants, the page order and the Ctrl+Tab decision. Also holds the SHARED driver of W8-19k, W8-19l and W8-19m (`W819jNewDialog`, `W819jGet`,
// `W819jCall`, the two DispatchProxy classes): they are `partial class Commands`, keep this file when the others are present. NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19j
//     "settings-dialog-frame" => Commands.W819jSettingsDialogFrameCases(inputs),
//
// IMPORTANT: build and run the oracle against the C# BASELINE d92d6aae (a worktree of the C# repo at that sha): the C# HEAD has more controls on the Files page and more
// Shortcuts rows. W8-10b.cs (`SettingsFrom`, the private `new AppSettings()` + overrides helper of the same partial class) must be present.
//
// How a window-less dialog is obtained: the REAL constructor `SettingsDialog(AppSettings current, SettingsStore? store, IImageDecoderFactory? decoderFactory,
// LocalizationService? localization, IUpdateChecker? updateChecker, SettingsDialogOptions? options, INativeDialogChrome? chrome)` with store/decoder/localization/updater/chrome
// null. It builds the presenter, creates the seven REAL pages (`SettingsPageCatalog.CreatePages`), declares them on their builders and builds the item list; it creates no window
// (Hwnd stays 0, so Item(id) is 0 and every OS call on it is a no-op). Options: Version = "9.9.9+oracle" (the item text), Warnings / Files = DispatchProxy recorders when the case
// asks, StartExplorer / OpenUrl = no-ops, ControlKeyDown = the case's value. The UI language is the process default (English).
//
// Conventions as W8-18d: every output case is the input case with the expected fields added; Invariant culture.
//
// Inputs (tests/golden-inputs/settings-dialog-frame.json), kinds and the fields added:
//   frame:      (none)                         -> title, resizable, header_ids [..], initial_focus_id, default_button_id, items [{class, id, text, style, ex_style}],
//                                                 page_keys [..] (Pages[i].Key), page_types [..] (the C# type names), page_item_counts [..] (PageItems(i).Count),
//                                                 active_page_index, active_page_key, saved
//   constants:  (none)                         -> the 21 constants of the frame (header_id ... page_id_stride, wm_app_language, wm_app_queue, three subclass ids)
//   page_math:  indices [int]                  -> page_id_base [int], viewport_id [int] (the static PageIdBase / ViewportId)
//   page_of:    ids [int]                      -> results [int] (the private static PageOf)
//   dialog_key: control (bool), message ("keydown" | "syskeydown" | "keyup"), vk (int)
//                                              -> handled (HandleKeyForTest), active_page_index after, active_page_key after. Only the FORWARD Ctrl+Tab is recorded (the Shift
//                                                 state is read from the real keyboard, which is up in the oracle run).

/// <summary>Records every IDialogWarningSink.ShowWarning(nint owner, string title, string message) call.</summary>
public class W819jWarningProxy : DispatchProxy
{
    public static List<JsonObject> Calls { get; } = [];

    protected override object? Invoke(MethodInfo? targetMethod, object?[]? args)
    {
        Calls.Add(new JsonObject { ["title"] = (string)args![1]!, ["message"] = (string)args[2]! });
        return null;
    }
}

/// <summary>Records IShellFileDialog.PickOpenFile / PickSaveFile and answers the configured paths.</summary>
public class W819jFilesProxy : DispatchProxy
{
    public static List<JsonObject> Calls { get; } = [];

    public static string? OpenResult { get; set; }

    public static string? SaveResult { get; set; }

    protected override object? Invoke(MethodInfo? targetMethod, object?[]? args)
    {
        var filters = new JsonArray();
        foreach (var filter in (IEnumerable)args![2]!)
        {
            Type ft = filter.GetType();
            filters.Add(new JsonObject { ["name"] = (string)ft.GetProperty("Name")!.GetValue(filter)!, ["pattern"] = (string)ft.GetProperty("Pattern")!.GetValue(filter)! });
        }

        if (targetMethod!.Name == "PickOpenFile")
        {
            Calls.Add(new JsonObject { ["call"] = "open", ["title"] = (string)args[1]!, ["filters"] = filters, ["initial_folder_set"] = !string.IsNullOrEmpty((string?)args[3]) });
            return OpenResult;
        }

        Calls.Add(new JsonObject
        {
            ["call"] = "save",
            ["title"] = (string)args[1]!,
            ["filters"] = filters,
            ["default_extension"] = (string?)args[3],
            ["suggested_file_name"] = (string?)args[4],
            ["initial_folder_set"] = !string.IsNullOrEmpty((string?)args[5]),
        });
        return SaveResult;
    }
}

static partial class Commands
{
    private const BindingFlags W819jFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    internal static Assembly W819jShell => Assembly.Load("PhotoReview");

    internal static Type W819jType(string name) => W819jShell.GetType("PhotoReview.Shell.Win32.Dialogs.SettingsUi." + name, throwOnError: true)!;

    /// <summary>A type of the shell assembly by its simple name (the namespace of IDialogWarningSink / IShellFileDialog differs between baselines).</summary>
    internal static Type W819jAnyType(string simpleName) => W819jShell.GetTypes().Single(t => t.Name == simpleName);

    internal static object? W819jGet(object target, string member)
    {
        for (Type? t = target.GetType(); t is not null; t = t.BaseType)
        {
            PropertyInfo? p = t.GetProperty(member, W819jFlags | BindingFlags.DeclaredOnly);
            if (p is not null)
            {
                return p.GetValue(target);
            }

            FieldInfo? f = t.GetField(member, W819jFlags | BindingFlags.DeclaredOnly);
            if (f is not null)
            {
                return f.GetValue(target);
            }
        }

        throw new InvalidOperationException("no member " + member);
    }

    internal static void W819jSetField(object target, string field, object? value)
    {
        for (Type? t = target.GetType(); t is not null; t = t.BaseType)
        {
            FieldInfo? f = t.GetField(field, W819jFlags | BindingFlags.DeclaredOnly);
            if (f is not null)
            {
                f.SetValue(target, value);
                return;
            }
        }

        throw new InvalidOperationException("no field " + field);
    }

    internal static object? W819jCall(object target, string method, params object?[] args)
    {
        for (Type? t = target.GetType(); t is not null; t = t.BaseType)
        {
            MethodInfo? m = t.GetMethods(W819jFlags | BindingFlags.DeclaredOnly).FirstOrDefault(x => x.Name == method && x.GetParameters().Length == args.Length);
            if (m is not null)
            {
                return m.Invoke(target, args);
            }
        }

        throw new InvalidOperationException("no method " + method);
    }

    internal static object W819jProxy(string interfaceName, Type proxyClass)
    {
        MethodInfo create = typeof(DispatchProxy).GetMethods().First(m => m.Name == "Create" && m.GetGenericArguments().Length == 2);
        return create.MakeGenericMethod(W819jAnyType(interfaceName), proxyClass).Invoke(null, null)!;
    }

    /// <summary>The window-less dialog (see the header). `settingsValues` go through SettingsFrom (W8-10b.cs); `controlKeyDown` null = the real keyboard.</summary>
    internal static object W819jNewDialog(JsonObject? settingsValues, object? files, object? warnings, bool? controlKeyDown = null)
    {
        Type optionsType = W819jType("SettingsDialogOptions");
        object options = Activator.CreateInstance(optionsType)!;
        void Set(string name, object? value) => optionsType.GetProperty(name, W819jFlags)!.SetValue(options, value);
        Set("Version", "9.9.9+oracle");
        Set("Files", files);
        Set("Warnings", warnings);
        Set("StartExplorer", new Func<string, string?>(_ => null));
        Set("OpenUrl", new Action<string>(_ => { }));
        if (controlKeyDown is { } down)
        {
            Set("ControlKeyDown", new Func<bool>(() => down));
        }

        Type dialogType = W819jType("SettingsDialog");
        return Activator.CreateInstance(dialogType, W819jFlags, null, [SettingsFrom(settingsValues), null, null, null, null, options, null], CultureInfo.InvariantCulture)!;
    }

    private static JsonObject W819jItem(object item)
    {
        Type t = item.GetType();
        return new JsonObject
        {
            ["class"] = t.GetProperty("Class")!.GetValue(item)!.ToString(),
            ["id"] = (int)t.GetProperty("Id")!.GetValue(item)!,
            ["text"] = (string)t.GetProperty("Text")!.GetValue(item)!,
            ["style"] = Convert.ToUInt32(t.GetProperty("Style")!.GetValue(item), CultureInfo.InvariantCulture),
            ["ex_style"] = Convert.ToUInt32(t.GetProperty("ExStyle")!.GetValue(item), CultureInfo.InvariantCulture),
        };
    }

    private static JsonArray W819jInts(IEnumerable values)
    {
        var array = new JsonArray();
        foreach (var v in values)
        {
            array.Add(Convert.ToInt32(v, CultureInfo.InvariantCulture));
        }

        return array;
    }

    public static JsonArray W819jSettingsDialogFrameCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        Type dialogType = W819jType("SettingsDialog");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "frame":
                {
                    object d = W819jNewDialog(null, null, null);
                    c["title"] = (string)W819jGet(d, "Title")!;
                    c["resizable"] = (bool)W819jGet(d, "Resizable")!;
                    c["header_ids"] = W819jInts((IEnumerable)W819jGet(d, "HeaderControlIds")!);
                    c["initial_focus_id"] = (int)W819jGet(d, "InitialFocusId")!;
                    c["default_button_id"] = (int)W819jGet(d, "DefaultButtonId")!;
                    var items = new JsonArray();
                    foreach (var item in (IEnumerable)W819jGet(d, "Items")!)
                    {
                        items.Add(W819jItem(item));
                    }

                    c["items"] = items;
                    var keys = new JsonArray();
                    var types = new JsonArray();
                    var counts = new JsonArray();
                    var pages = ((IEnumerable)W819jGet(d, "Pages")!).Cast<object>().ToList();
                    for (var i = 0; i < pages.Count; i++)
                    {
                        keys.Add((string)pages[i].GetType().GetProperty("Key")!.GetValue(pages[i])!);
                        types.Add(pages[i].GetType().Name);
                        counts.Add(((ICollection)W819jCall(d, "PageItems", i)!).Count);
                    }

                    c["page_keys"] = keys;
                    c["page_types"] = types;
                    c["page_item_counts"] = counts;
                    c["active_page_index"] = (int)W819jGet(d, "ActivePageIndex")!;
                    c["active_page_key"] = (string)W819jGet(d, "ActivePageKey")!;
                    c["saved"] = (bool)W819jGet(d, "Saved")!;
                    break;
                }

                case "constants":
                {
                    object Const(string name) => dialogType.GetField(name, W819jFlags)!.GetRawConstantValue()!;
                    c["header_id"] = Convert.ToInt32(Const("HeaderId"), CultureInfo.InvariantCulture);
                    c["nav_id"] = Convert.ToInt32(Const("NavId"), CultureInfo.InvariantCulture);
                    c["version_id"] = Convert.ToInt32(Const("VersionId"), CultureInfo.InvariantCulture);
                    c["page_title_id"] = Convert.ToInt32(Const("PageTitleId"), CultureInfo.InvariantCulture);
                    c["scroll_id"] = Convert.ToInt32(Const("ScrollId"), CultureInfo.InvariantCulture);
                    c["viewport_base"] = Convert.ToInt32(Const("ViewportBase"), CultureInfo.InvariantCulture);
                    c["apply_id"] = Convert.ToInt32(Const("ApplyId"), CultureInfo.InvariantCulture);
                    c["defaults_id"] = Convert.ToInt32(Const("DefaultsId"), CultureInfo.InvariantCulture);
                    c["reset_page_id"] = Convert.ToInt32(Const("ResetPageId"), CultureInfo.InvariantCulture);
                    c["export_id"] = Convert.ToInt32(Const("ExportId"), CultureInfo.InvariantCulture);
                    c["import_id"] = Convert.ToInt32(Const("ImportId"), CultureInfo.InvariantCulture);
                    c["page_id_stride"] = Convert.ToInt32(Const("PageIdStride"), CultureInfo.InvariantCulture);
                    // SaveId = IdOk and CancelId = IdCancel are `internal const int` aliases of the base class constants.
                    c["save_id"] = Convert.ToInt32(Const("SaveId"), CultureInfo.InvariantCulture);
                    c["cancel_id"] = Convert.ToInt32(Const("CancelId"), CultureInfo.InvariantCulture);
                    c["wm_app_language"] = Convert.ToUInt32(Const("WmAppLanguage"), CultureInfo.InvariantCulture);
                    c["wm_app_queue"] = Convert.ToUInt32(Const("WmAppQueue"), CultureInfo.InvariantCulture);
                    c["forward_all_subclass_id"] = Convert.ToUInt64(Const("ForwardAllSubclassId"), CultureInfo.InvariantCulture);
                    c["wheel_subclass_id"] = Convert.ToUInt64(Const("WheelSubclassId"), CultureInfo.InvariantCulture);
                    c["key_box_subclass_id"] = Convert.ToUInt64(Const("KeyBoxSubclassId"), CultureInfo.InvariantCulture);
                    break;
                }

                case "page_math":
                {
                    var bases = new JsonArray();
                    var viewports = new JsonArray();
                    foreach (var index in c["indices"]!.AsArray())
                    {
                        bases.Add((int)dialogType.GetMethod("PageIdBase", W819jFlags)!.Invoke(null, [index!.GetValue<int>()])!);
                        viewports.Add((int)dialogType.GetMethod("ViewportId", W819jFlags)!.Invoke(null, [index!.GetValue<int>()])!);
                    }

                    c["page_id_base"] = bases;
                    c["viewport_id"] = viewports;
                    break;
                }

                case "page_of":
                {
                    var results = new JsonArray();
                    MethodInfo pageOf = dialogType.GetMethod("PageOf", W819jFlags)!;
                    foreach (var id in c["ids"]!.AsArray())
                    {
                        results.Add((int)pageOf.Invoke(null, [id!.GetValue<int>()])!);
                    }

                    c["results"] = results;
                    break;
                }

                case "dialog_key":
                {
                    object d = W819jNewDialog(null, null, null, controlKeyDown: c["control"]!.GetValue<bool>());
                    Type msgType = Assembly.Load("PhotoReview.Shell.Interop").GetType("PhotoReview.Shell.Interop.Msg", throwOnError: true)!;
                    object msg = Activator.CreateInstance(msgType)!;
                    msgType.GetField("Hwnd")!.SetValue(msg, (nint)0);
                    msgType.GetField("Message")!.SetValue(msg, c["message"]!.GetValue<string>() switch { "keydown" => 0x0100u, "syskeydown" => 0x0104u, _ => 0x0101u });
                    msgType.GetField("WParam")!.SetValue(msg, (nint)c["vk"]!.GetValue<int>());
                    msgType.GetField("LParam")!.SetValue(msg, (nint)1);
                    c["handled"] = (bool)W819jCall(d, "HandleKeyForTest", msg)!;
                    c["active_page_index"] = (int)W819jGet(d, "ActivePageIndex")!;
                    c["active_page_key"] = (string)W819jGet(d, "ActivePageKey")!;
                    break;
                }

                default:
                    throw new InvalidOperationException("unknown settings-dialog-frame kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
