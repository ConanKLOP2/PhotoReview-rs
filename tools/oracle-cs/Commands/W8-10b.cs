using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.App.Presenters;
using PhotoReview.Core.Model;
using PhotoReview.Core.Settings;

// W8-10b golden generator (contract: contracts/W8-10b.md, "Golden package").
// NOT RUN by the writer; the lead wires and runs it.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-10b (presenters, App.Shared)
//     "settings-field-map" => Commands.SettingsFieldMapCases(inputs),
//
// Link lines for tools/oracle-cs/OracleCs.csproj (CSharpSrcDir set by tools/oracle.ps1). The three App.Shared files are compiled
// as they are, so the `internal` members are called directly:
//     <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\Presenters\SettingsFieldMap.cs" Link="cs-baseline\SettingsFieldMap.cs" />
//     <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\Presenters\SettingsFormState.cs" Link="cs-baseline\SettingsFormState.cs" />
//     <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\Presenters\ActionListJson.cs" Link="cs-baseline\ActionListJson.cs" />
// Their closure (LanguageOption from Localization, ShortcutKeyName from Services, and the ReviewAction/Tr sources) is linked the same
// way, one file per compile error; never copy code. Core (ContextMenuItems, ShortcutKeyCanonical, PerformanceOptions) is referenced.
//
// Semantics (deterministic, machine-independent):
//  * Settings are built from `new AppSettings()` and then every key of the case is set by reflection (enums by member name, flags
//    and ints as numbers, doubles as numbers, `ActionsText` through ActionListJson.Deserialize, `Shortcuts` as an object).
//  * Forms are built by running EVERY binding's Load (in table order) on the source settings, then the case's overrides.
//  * Doubles are written as strings in "R" (round-trip) form; enums by member name; flags as the integer value.
//  * Refusals are {"message": <English Tr text>, "focus": <SettingsFocusField name>}. The Tr texts come from BuiltInCatalog.EnglishLocalizer.
//  * `TryInt` is private: reached by reflection (BindingFlags.NonPublic). The double parse is written out here, exactly as Apply does it.

static partial class Commands
{
    public static JsonArray SettingsFieldMapCases(JsonObject inputs)
    {
        var cases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var input = node!.AsObject();
            var kind = input["kind"]!.GetValue<string>();
            var result = kind switch
            {
                "tables" => Tables(),
                "load" => LoadCase(input),
                "apply_row" => ApplyRowCase(input),
                "apply_all" => ApplyAllCase(input),
                "shortcuts_read" => ShortcutsReadCase(input),
                "shortcuts_canonicalize" => ShortcutsCanonicalizeCase(input),
                "anchor_to_index" => AnchorToIndexCase(input),
                "index_to_anchor" => IndexToAnchorCase(input),
                "view_mode_to_index" => ViewModeToIndexCase(input),
                "index_to_view_mode" => IndexToViewModeCase(input),
                "try_int" => TryIntCase(input),
                "try_double" => TryDoubleCase(input),
                "double_text" => DoubleTextCase(input),
                _ => throw new ArgumentException("unknown settings-field-map kind " + kind),
            };
            cases.Add(new JsonObject { ["id"] = input["id"]!.GetValue<string>(), ["kind"] = kind, ["result"] = result });
        }
        return cases;
    }

    // ---- tables ----

    private static JsonObject Tables()
    {
        var settable = typeof(AppSettings).GetProperties(BindingFlags.Public | BindingFlags.Instance)
            .Where(p => p.CanRead && p.CanWrite).Select(p => p.Name).Order(StringComparer.Ordinal).ToList();
        var bindings = new JsonArray();
        foreach (var binding in SettingsFieldMap.Bindings)
            bindings.Add(new JsonArray(binding.Properties.Select(p => (JsonNode)JsonValue.Create(p)!).ToArray()));
        var notEdited = new JsonArray();
        foreach (var pair in SettingsFieldMap.NotEditedInDialog)
            notEdited.Add(new JsonArray(JsonValue.Create(pair.Key), JsonValue.Create(pair.Value)));
        return new JsonObject
        {
            ["settable"] = StringArray(settable),
            ["covered_sorted"] = StringArray(SettingsFieldMap.CoveredProperties.Order(StringComparer.Ordinal)),
            ["not_edited_in_order"] = notEdited,
            ["bindings_in_order"] = bindings,
            ["shortcut_names_in_order"] = StringArray(SettingsFieldMap.Shortcuts.Select(a => a.Property)),
        };
    }

    // ---- load ----

    private static JsonObject LoadCase(JsonObject input)
    {
        var source = SettingsFrom(input["settings"]?.AsObject());
        var state = new SettingsFormState();
        foreach (var binding in SettingsFieldMap.Bindings) binding.Load(source, state);
        return FormSnapshot(state);
    }

    // ---- apply ----

    private static JsonObject ApplyRowCase(JsonObject input)
    {
        var row = input["row"]!.GetValue<int>();
        var source = SettingsFrom(input["settings"]?.AsObject());
        var form = FormFrom(source, input["form"]?.AsObject());
        var target = SettingsFrom(input["settings"]?.AsObject());
        var refusal = SettingsFieldMap.Bindings[row].Apply(form, target);
        return new JsonObject { ["refused"] = RefusalJson(refusal), ["settings"] = SettingsSnapshot(target) };
    }

    private static JsonObject ApplyAllCase(JsonObject input)
    {
        var source = SettingsFrom(input["settings"]?.AsObject());
        var form = FormFrom(source, input["form"]?.AsObject());
        var target = SettingsFrom(input["settings"]?.AsObject());
        int? refusedRow = null;
        SettingsFieldError? refusal = null;
        for (var row = 0; row < SettingsFieldMap.Bindings.Count; row++)
        {
            refusal = SettingsFieldMap.Bindings[row].Apply(form, target);
            if (refusal is null) continue;
            refusedRow = row;
            break;
        }
        return new JsonObject
        {
            ["refused_row"] = refusedRow is null ? null : JsonValue.Create(refusedRow.Value),
            ["refused"] = RefusalJson(refusal),
            ["settings"] = SettingsSnapshot(target),
        };
    }

    // ---- shortcuts ----

    private static JsonObject ShortcutsReadCase(JsonObject input)
    {
        var state = new SettingsFormState();
        var texts = input["form"]!["shortcutTexts"]!.AsObject();
        foreach (var (name, value) in texts) state.ShortcutTexts[name] = value!.GetValue<string>();
        return ShortcutsSnapshot(SettingsFieldMap.ReadShortcuts(state));
    }

    private static JsonObject ShortcutsCanonicalizeCase(JsonObject input)
    {
        var raw = ShortcutsFrom(input["raw"]!.AsObject());
        var existing = ShortcutsFrom(input["existing"]!.AsObject());
        return ShortcutsSnapshot(SettingsFieldMap.CanonicalizeShortcuts(raw, existing));
    }

    // ---- index tables ----

    private static JsonObject AnchorToIndexCase(JsonObject input)
    {
        var anchor = Enum.Parse<FitWidthAnchor>(input["anchor"]!.GetValue<string>());
        return new JsonObject { ["index"] = SettingsFieldMap.AnchorToIndex(anchor, FitWidthAnchor.Centre) };
    }

    private static JsonObject IndexToAnchorCase(JsonObject input)
    {
        var index = input["index"]!.GetValue<int>();
        var fallback = Enum.Parse<FitWidthAnchor>(input["fallback"]!.GetValue<string>());
        return new JsonObject { ["anchor"] = SettingsFieldMap.IndexToAnchor(index, fallback).ToString() };
    }

    private static JsonObject ViewModeToIndexCase(JsonObject input)
    {
        var mode = Enum.Parse<InitialViewMode>(input["mode"]!.GetValue<string>());
        return new JsonObject { ["index"] = SettingsFieldMap.ViewModeToIndex(mode) };
    }

    private static JsonObject IndexToViewModeCase(JsonObject input)
    {
        var index = input["index"]!.GetValue<int>();
        return new JsonObject { ["mode"] = SettingsFieldMap.IndexToViewMode(index).ToString() };
    }

    // ---- parsing helpers ----

    private static JsonObject TryIntCase(JsonObject input)
    {
        var method = typeof(SettingsFieldMap).GetMethod("TryInt", BindingFlags.NonPublic | BindingFlags.Static)
            ?? throw new InvalidOperationException("TryInt not found");
        var text = input["text"]!.GetValue<string>();
        var min = input["min"]!.GetValue<int>();
        var max = input["max"]!.GetValue<int>();
        var args = new object?[] { text, min, max, 0 };
        var ok = (bool)method.Invoke(null, args)!;
        return new JsonObject { ["ok"] = ok, ["value"] = ok ? JsonValue.Create((int)args[3]!) : null };
    }

    private static JsonObject TryDoubleCase(JsonObject input)
    {
        var text = input["text"]!.GetValue<string>();
        var ok = double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var value);
        return new JsonObject
        {
            ["ok"] = ok,
            ["value"] = ok ? value.ToString("R", CultureInfo.InvariantCulture) : null,
            ["finite"] = ok && double.IsFinite(value),
        };
    }

    private static JsonObject DoubleTextCase(JsonObject input)
    {
        var value = double.Parse(input["value"]!.GetValue<string>(), NumberStyles.Float, CultureInfo.InvariantCulture);
        return new JsonObject { ["text"] = value.ToString(CultureInfo.InvariantCulture) };
    }

    // ---- builders ----

    /// <summary>`new AppSettings()` with the case's keys set by reflection.</summary>
    private static AppSettings SettingsFrom(JsonObject? values)
    {
        var settings = new AppSettings();
        if (values is null) return settings;
        foreach (var (key, node) in values)
        {
            switch (key)
            {
                case "ActionsText":
                    settings.Actions = ActionListJson.Deserialize(node!.GetValue<string>());
                    break;
                case "Shortcuts":
                    foreach (var (name, value) in node!.AsObject()) SetProperty(settings.Shortcuts, name, value);
                    break;
                default:
                    SetProperty(settings, key, node);
                    break;
            }
        }
        return settings;
    }

    private static ShortcutMappings ShortcutsFrom(JsonObject values)
    {
        var shortcuts = new ShortcutMappings();
        foreach (var (name, value) in values) SetProperty(shortcuts, name, value);
        return shortcuts;
    }

    /// <summary>Form = every binding's Load on the source, then the overrides (camelCase keys, see the contract).</summary>
    private static SettingsFormState FormFrom(AppSettings source, JsonObject? overrides)
    {
        var state = new SettingsFormState();
        foreach (var binding in SettingsFieldMap.Bindings) binding.Load(source, state);
        if (overrides is null) return state;
        foreach (var (key, node) in overrides)
        {
            switch (key)
            {
                case "shortcutTexts":
                    state.ShortcutTexts.Clear();
                    foreach (var (name, value) in node!.AsObject()) state.ShortcutTexts[name] = value!.GetValue<string>();
                    break;
                case "contextMenuShown":
                    state.ContextMenuShown.Clear();
                    foreach (var id in ShownIds(node!)) state.ContextMenuShown.Add(id);
                    break;
                default:
                    SetProperty(state, char.ToUpperInvariant(key[0]) + key[1..], node);
                    break;
            }
        }
        return state;
    }

    private static IEnumerable<ContextMenuItemId> ShownIds(JsonNode node)
    {
        if (node is JsonArray list)
            return list.Select(item => Enum.Parse<ContextMenuItemId>(item!.GetValue<string>())).ToList();
        var text = node.GetValue<string>();
        if (text == "ALL") return ContextMenuItems.Items.Select(item => item.Id).ToList();
        var excluded = Enum.Parse<ContextMenuItemId>(text.Substring("ALL_BUT:".Length));
        return ContextMenuItems.Items.Select(item => item.Id).Where(id => id != excluded).ToList();
    }

    private static void SetProperty(object target, string name, JsonNode? node)
    {
        var property = target.GetType().GetProperty(name)
            ?? throw new ArgumentException("unknown property " + target.GetType().Name + "." + name);
        property.SetValue(target, ConvertTo(property.PropertyType, node));
    }

    private static object? ConvertTo(Type type, JsonNode? node)
    {
        if (node is null) return null;
        var target = Nullable.GetUnderlyingType(type) ?? type;
        if (target == typeof(string)) return node.GetValue<string>();
        if (target == typeof(bool)) return node.GetValue<bool>();
        if (target == typeof(int)) return node.GetValue<int>();
        if (target == typeof(long)) return node.GetValue<long>();
        if (target == typeof(double)) return node.GetValue<double>();
        if (target == typeof(List<string>)) return node.AsArray().Select(item => item!.GetValue<string>()).ToList();
        if (target.IsEnum)
        {
            // Flags (ExifInfoFields, TitleBarFields) are numbers; the other enums are member names.
            if (node.GetValueKind() == System.Text.Json.JsonValueKind.Number) return Enum.ToObject(target, node.GetValue<int>());
            return Enum.Parse(target, node.GetValue<string>());
        }
        throw new ArgumentException("no converter for " + type.FullName);
    }

    private static JsonObject? RefusalJson(SettingsFieldError? refusal) =>
        refusal is null ? null : new JsonObject { ["message"] = refusal.Message, ["focus"] = refusal.Focus.ToString() };

    // ---- snapshots ----

    private static readonly string[] SettingsNames =
    {
        "InitialViewMode", "FitWidthAnchor", "FitWidthAnchor2", "MiddleClickAction", "KeepZoomAcrossImages", "ImageSortMode",
        "ScalingQuality", "DecoderBackend", "LoadingMode", "InstanceMode", "CompareHashEnabled", "CompareSizeEnabled",
        "RawSupportEnabled", "WebpHeicSupportEnabled", "RawFullDecode", "RawPairMode", "LoggingEnabled",
        "AllowPermanentDeleteWithoutRecycleBin", "ConfirmBeforeDelete", "ExternalEditorPath", "JournalDurability",
        "ImageCacheRamPercent", "ShowInfoOverlay", "ShowFileInfo", "ShowFolderInfo", "MouseWheelAction", "ClickToZoomEnabled",
        "KineticPanEnabled", "KineticGlideSmoothing", "ArrowKeyNavigatesAtZoomEdge", "ClickZoomKeyTogglesFit",
        "KeyboardZoomAnchor", "MoveCopyReuseLastFolder", "ShowExifInfo", "ExifInfoFields", "TitleBarFields", "ClickZoomPercent",
        "ArrowPanStepPercent", "KeyboardZoomStepPercent", "ImageTransition", "ImageTransitionMs", "TouchpadSwipeEnabled",
        "TouchpadSwipeDistancePerImage", "PreloadForwardCount", "PreloadBackwardCount", "ToolbarAutoHide", "ToolbarAutoHideDelayMs",
        "InfoOverlayAutoHide", "InfoOverlayAutoHideDelayMs", "ToolbarOpacityPercent", "InfoOverlayFontSize", "ShowZoomIndicator",
        "ShowFolderMenuItems", "ShowZoomMenuItems", "ShowRecycleMenuItem", "UiLanguage",
    };

    /// <summary>The covered AppSettings values after Apply, in the form the Rust test compares.</summary>
    private static JsonObject SettingsSnapshot(AppSettings settings)
    {
        var result = new JsonObject();
        foreach (var name in SettingsNames) result[name] = ValueJson(typeof(AppSettings).GetProperty(name)!.GetValue(settings));
        result["Shortcuts"] = ShortcutsSnapshot(settings.Shortcuts);
        result["Actions"] = ActionListJson.Serialize(settings.Actions);
        result["EffectiveHiddenContextMenuItems"] = StringArray(ContextMenuItems.EffectiveHidden(settings).Select(id => id.ToString()).Order(StringComparer.Ordinal));
        return result;
    }

    private static JsonObject ShortcutsSnapshot(ShortcutMappings shortcuts)
    {
        var result = new JsonObject();
        foreach (var property in typeof(ShortcutMappings).GetProperties(BindingFlags.Public | BindingFlags.Instance)
                     .Where(p => p.PropertyType == typeof(string) && p.CanWrite).OrderBy(p => p.Name, StringComparer.Ordinal))
            result[property.Name] = (string?)property.GetValue(shortcuts);
        return result;
    }

    /// <summary>Every SettingsFormState property except SelectedLanguage (the language picker is W8-10c).</summary>
    private static JsonObject FormSnapshot(SettingsFormState state)
    {
        var result = new JsonObject();
        foreach (var property in typeof(SettingsFormState).GetProperties(BindingFlags.Public | BindingFlags.Instance)
                     .Where(p => p.Name != nameof(SettingsFormState.SelectedLanguage)).OrderBy(p => p.Name, StringComparer.Ordinal))
        {
            var key = char.ToLowerInvariant(property.Name[0]) + property.Name[1..];
            result[key] = ValueJson(property.GetValue(state));
        }
        result["shortcutTexts"] = new JsonObject(state.ShortcutTexts.OrderBy(pair => pair.Key, StringComparer.Ordinal)
            .Select(pair => KeyValuePair.Create(pair.Key, (JsonNode?)JsonValue.Create(pair.Value))));
        result["contextMenuShown"] = StringArray(state.ContextMenuShown.Select(id => id.ToString()).Order(StringComparer.Ordinal));
        return result;
    }

    private static JsonNode? ValueJson(object? value) => value switch
    {
        null => null,
        bool b => JsonValue.Create(b),
        int i => JsonValue.Create(i),
        long l => JsonValue.Create(l),
        double d => JsonValue.Create(d.ToString("R", CultureInfo.InvariantCulture)),
        string s => JsonValue.Create(s),
        Enum e when e.GetType().IsDefined(typeof(FlagsAttribute), inherit: false) => JsonValue.Create(Convert.ToInt32(e, CultureInfo.InvariantCulture)),
        Enum e => JsonValue.Create(e.ToString()),
        _ => JsonValue.Create(value.ToString()),
    };

    private static JsonArray StringArray(IEnumerable<string> values) =>
        new(values.Select(v => (JsonNode)JsonValue.Create(v)!).ToArray());
}
