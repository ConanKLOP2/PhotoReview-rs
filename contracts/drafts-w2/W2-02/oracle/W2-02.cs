// W2-02: settings-normalize, settings-validate, settings-repair-text
// (SettingsNormalizer.cs, SettingsValidator.cs, SettingsLoadRepairText.cs, SettingsParseResult.cs)
//
// Dispatch lines to add to tools/oracle-cs/Program.cs (inside the `command switch`):
//     "settings-normalize" => Commands.SettingsNormalize(inputs),
//     "settings-validate" => Commands.SettingsValidate(inputs),
//     "settings-repair-text" => Commands.SettingsRepairText(inputs),
//
// Copy this file to tools/oracle-cs/Commands/W2-02.cs and the inputs to tests/golden-inputs/<command>.json.
// Output: tests/golden/<command>.json (run tools/oracle.ps1 -Command <command>). All texts are the built-in ENGLISH catalog
// (the oracle process never switches the ambient Localizer).

using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Settings;

static partial class Commands
{
    private static readonly Regex W202Top = new("^  \"([A-Za-z0-9]+)\": (.*)$", RegexOptions.Compiled);

    /// <summary>Top-level property name -> value text of indented config.json text (see W2-01.cs; duplicated so each file stands alone).</summary>
    private static Dictionary<string, string> W202Segments(string json)
    {
        var result = new Dictionary<string, string>(StringComparer.Ordinal);
        string? name = null;
        var buffer = new List<string>();
        void Flush()
        {
            if (name is null) return;
            var last = buffer[^1];
            if (last.EndsWith(',')) buffer[^1] = last[..^1];
            result[name] = string.Join("\r\n", buffer);
        }
        foreach (var line in json.Split("\r\n"))
        {
            var m = W202Top.Match(line);
            if (m.Success) { Flush(); name = m.Groups[1].Value; buffer = [m.Groups[2].Value]; }
            else if (name is not null && line != "}") buffer.Add(line);
        }
        Flush();
        return result;
    }

    private static JsonObject W202Changed(AppSettings settings)
    {
        var defaults = W202Segments(JsonSerializer.Serialize(new AppSettings(), AppSettingsJsonContext.Default.AppSettings));
        var actual = W202Segments(JsonSerializer.Serialize(settings, AppSettingsJsonContext.Default.AppSettings));
        var changed = new JsonObject();
        foreach (var (key, value) in actual)
            if (!defaults.TryGetValue(key, out var d) || d != value) changed[key] = value;
        return changed;
    }

    private static JsonArray W202Array(IEnumerable<string> items) => new(items.Select(i => (JsonNode?)i).ToArray());

    /// <summary>
    /// What <c>SettingsStore.FinishDeserialize</c> does before Normalize runs in the real pipeline: a null Actions list or Shortcuts object is
    /// replaced by the defaults. (Null entries inside Actions and null strings are left for Normalize, which handles them.)
    /// </summary>
    private static AppSettings? W202Load(string text)
    {
        var s = JsonSerializer.Deserialize(text, AppSettingsJsonContext.Default.AppSettings);
        if (s is null) return null;
        s.Actions ??= ReviewAction.Defaults();
        s.Shortcuts ??= new ShortcutMappings();
        return s;
    }

    /// <summary>The Rust type cannot hold a null string: after Normalize the only ones left are these two, which Rust holds as "".</summary>
    private static void W202Tail(AppSettings s)
    {
        s.UiLanguage ??= "";
        s.ExternalEditorPath ??= "";
    }

    /// <summary>
    /// W2-02. Input "cases": [ { "name", "text" } ] (text must deserialize). For each: Deserialize, fill null Actions/Shortcuts, then
    /// Normalize -> DisableConflictingOptionalShortcuts -> Normalize again -> DisableConflictingOptionalShortcuts again.
    /// Output per case: fixed (first Normalize result, in order), disabled, fixed_again (must be empty), disabled_again (must be empty),
    /// changed (settings segments that differ from defaults after all of that).
    /// </summary>
    public static JsonArray SettingsNormalize(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var c in inputs["cases"]!.AsArray())
        {
            var name = c!["name"]!.GetValue<string>();
            var text = c["text"]!.GetValue<string>();
            var item = new JsonObject { ["name"] = name, ["text"] = text };
            try
            {
                var s = W202Load(text);
                if (s is null) { item["outcome"] = "null"; result.Add(item); continue; }
                var fixedNames = SettingsNormalizer.Normalize(s);
                var disabled = SettingsNormalizer.DisableConflictingOptionalShortcuts(s);
                var fixedAgain = SettingsNormalizer.Normalize(s);
                var disabledAgain = SettingsNormalizer.DisableConflictingOptionalShortcuts(s);
                W202Tail(s);
                item["outcome"] = "ok";
                item["fixed"] = W202Array(fixedNames);
                item["disabled"] = W202Array(disabled);
                item["fixed_again"] = W202Array(fixedAgain);
                item["disabled_again"] = W202Array(disabledAgain);
                item["changed"] = W202Changed(s);
            }
            catch (Exception ex)
            {
                item["outcome"] = "error";
                item["error_type"] = ex.GetType().Name;
            }
            result.Add(item);
        }
        return result;
    }

    // ---- validators ----

    private sealed class W202NonBlank : IKeyNameValidator
    {
        public bool IsValidKeyName(string keyName) => !string.IsNullOrWhiteSpace(keyName);
    }

    private sealed class W202LettersDigits : IKeyNameValidator
    {
        public bool IsValidKeyName(string keyName) => keyName.Length > 0 && keyName.All(char.IsAsciiLetterOrDigit);
    }

    private sealed class W202RejectAll : IKeyNameValidator
    {
        public bool IsValidKeyName(string keyName) => false;
    }

    private sealed class W202KnownSet(IEnumerable<string> names) : IKeyNameValidator
    {
        private readonly HashSet<string> _names = new(names, StringComparer.OrdinalIgnoreCase);
        public bool IsValidKeyName(string keyName) => !string.IsNullOrWhiteSpace(keyName) && _names.Contains(keyName.Trim());
    }

    private static IKeyNameValidator W202MakeValidator(JsonNode spec)
    {
        var kind = spec["kind"]!.GetValue<string>();
        return kind switch
        {
            "non_blank" => new W202NonBlank(),
            "letters_digits" => new W202LettersDigits(),
            "reject_all" => new W202RejectAll(),
            "known" => new W202KnownSet(spec["names"]!.AsArray().Select(n => n!.GetValue<string>())),
            _ => throw new ArgumentException("validator kind " + kind),
        };
    }

    /// <summary>
    /// W2-02. Input "cases": [ { "name", "text", "validator": { "kind": "non_blank" | "letters_digits" | "reject_all" | "known", "names": [...] } } ].
    /// For each: Deserialize (nulls: Actions/Shortcuts defaults, strings "" so the Rust type can hold the state), then
    /// <c>new SettingsValidator(validator).ValidateShortcuts(settings)</c>. Output: message (English text or null).
    /// The validator is given the settings exactly as read (no Normalize), so blank / padded / aliased keys reach it.
    /// </summary>
    public static JsonArray SettingsValidate(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var c in inputs["cases"]!.AsArray())
        {
            var name = c!["name"]!.GetValue<string>();
            var text = c["text"]!.GetValue<string>();
            var item = new JsonObject { ["name"] = name, ["text"] = text, ["validator"] = c["validator"]!.DeepClone() };
            var s = W202Load(text)!;
            foreach (var a in s.Actions.Where(a => a is not null)) { a.Name ??= ""; a.Shortcut ??= ""; a.Destination ??= ""; }
            s.Actions.RemoveAll(a => a is null);
            foreach (var p in typeof(ShortcutMappings).GetProperties().Where(p => p.PropertyType == typeof(string)))
                if (p.GetValue(s.Shortcuts) is null) p.SetValue(s.Shortcuts, "");
            var validator = new SettingsValidator(W202MakeValidator(c["validator"]!));
            item["message"] = validator.ValidateShortcuts(s);
            result.Add(item);
        }
        return result;
    }

    /// <summary>
    /// W2-02. Input "cases": [ { "name", "repairs": [string], "for_import": bool } ]. Output: text of
    /// <c>SettingsLoadRepairText.Build(repairs, forImport)</c> (lines joined with Environment.NewLine = CRLF on Windows).
    /// </summary>
    public static JsonArray SettingsRepairText(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var c in inputs["cases"]!.AsArray())
        {
            var repairs = c!["repairs"]!.AsArray().Select(r => r!.GetValue<string>()).ToList();
            var forImport = c["for_import"]!.GetValue<bool>();
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["repairs"] = W202Array(repairs),
                ["for_import"] = forImport,
                ["text"] = SettingsLoadRepairText.Build(repairs, forImport),
            });
        }
        return result;
    }
}
