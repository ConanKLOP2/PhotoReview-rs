// W2-03: settings-parse-text, settings-store  (SettingsStore.cs)
//
// Dispatch lines to add to tools/oracle-cs/Program.cs (inside the `command switch`):
//     "settings-parse-text" => Commands.SettingsParseText(inputs),
//     "settings-store" => Commands.SettingsStoreScenarios(inputs),
//
// Copy this file to tools/oracle-cs/Commands/W2-03.cs and the inputs to tests/golden-inputs/<command>.json.
// Output: tests/golden/<command>.json (run tools/oracle.ps1 -Command <command>). Texts are the built-in ENGLISH catalog.
// settings-store runs the real SettingsStore on a throw-away folder with PhysicalFileSystem (files are created and deleted by the command).

using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using PhotoReview.Core;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.IO;
using PhotoReview.Core.Settings;

static partial class Commands
{
    private static readonly Regex W203Top = new("^  \"([A-Za-z0-9]+)\": (.*)$", RegexOptions.Compiled);
    private static readonly Regex W203Stamp = new(@"\d{14}", RegexOptions.Compiled);

    private static Dictionary<string, string> W203Segments(string json)
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
            var m = W203Top.Match(line);
            if (m.Success) { Flush(); name = m.Groups[1].Value; buffer = [m.Groups[2].Value]; }
            else if (name is not null && line != "}") buffer.Add(line);
        }
        Flush();
        return result;
    }

    /// <summary>Segments of <paramref name="settings"/> that differ from <c>new AppSettings()</c>; null UiLanguage / ExternalEditorPath read as "" (Rust cannot hold null).</summary>
    private static JsonObject W203Changed(AppSettings settings)
    {
        settings.UiLanguage ??= "";
        settings.ExternalEditorPath ??= "";
        var defaults = W203Segments(JsonSerializer.Serialize(new AppSettings(), AppSettingsJsonContext.Default.AppSettings));
        var actual = W203Segments(JsonSerializer.Serialize(settings, AppSettingsJsonContext.Default.AppSettings));
        var changed = new JsonObject();
        foreach (var (key, value) in actual)
            if (!defaults.TryGetValue(key, out var d) || d != value) changed[key] = value;
        return changed;
    }

    private static JsonArray W203Array(IEnumerable<string> items) => new(items.Select(i => (JsonNode?)i).ToArray());

    /// <summary>
    /// W2-03. Input "cases": [ { "name", "text" } ]. Output: <c>SettingsStore.ParseText(text)</c>: outcome ok | error (error_type), is_json_null,
    /// salvaged, reset_settings, disabled_shortcuts, repairs, salvage_cause (exception type name or null), config_version, changed (segments
    /// of the resulting settings that differ from defaults), and repair_text = SettingsLoadRepairText.Build(repairs) (CRLF joined).
    /// </summary>
    public static JsonArray SettingsParseText(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var c in inputs["cases"]!.AsArray())
        {
            var name = c!["name"]!.GetValue<string>();
            var text = c["text"]!.GetValue<string>();
            var item = new JsonObject { ["name"] = name, ["text"] = text };
            try
            {
                var r = SettingsStore.ParseText(text);
                item["outcome"] = "ok";
                item["is_json_null"] = r.IsJsonNull;
                item["salvaged"] = W203Array(r.Salvaged);
                item["reset_settings"] = W203Array(r.ResetSettings);
                item["disabled_shortcuts"] = W203Array(r.DisabledShortcuts);
                item["repairs"] = W203Array(r.Repairs);
                item["salvage_cause"] = r.SalvageCause?.GetType().Name;
                item["config_version"] = r.Settings.ConfigVersion;
                item["repair_text"] = SettingsLoadRepairText.Build(r.Repairs);
                item["changed"] = W203Changed(r.Settings);
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

    // ---- the store scenarios ----

    private sealed class W203Log : ILog
    {
        public readonly List<string> Lines = [];
        public bool Enabled => true;
        public void Info(string message) => Lines.Add("info: " + message);
        public void Warn(string message) => Lines.Add("warn: " + message);
        public void Error(string message, Exception? ex = null) => Lines.Add("error: " + message + (ex is null ? "" : " [" + ex.GetType().Name + "]"));
    }

    private static string W203Normalize(string text, string root) => W203Stamp.Replace(text.Replace(root, "<ROOT>"), "<STAMP>");

    private static byte[] W203FileBytes(JsonNode file) =>
        file["b64"] is { } b64 ? Convert.FromBase64String(b64.GetValue<string>()) : new UTF8Encoding(false).GetBytes(file["text"]!.GetValue<string>());

    /// <summary>
    /// W2-03. Input "scenarios": [ { "name", "files": [ { "dir": "config" | "other", "name", "text" | "b64" } ],
    ///   "steps": [ { "op": "load" } | { "op": "load_explicit", "file": "name in the other dir" } | { "op": "save", "settings": "&lt;config.json text deserialized into the object to save&gt;" } ] } ].
    /// The config dir is &lt;root&gt;\PhotoReview (AppPaths.ConfigFile's folder); "other" is a sibling folder for explicit-path loads.
    /// Output per scenario: steps (per step: repairs, changed, config_version, thrown), files_after (config dir and other dir, sorted, names with the
    /// 14-digit timestamp replaced by &lt;STAMP&gt;, content as b64 and as text when it is valid UTF-8), startup_errors, log (info/warn/error lines with
    /// the root replaced by &lt;ROOT&gt;), changed_events (how many times Changed fired during each step).
    /// </summary>
    public static JsonArray SettingsStoreScenarios(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var sc in inputs["scenarios"]!.AsArray())
        {
            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-store-" + Guid.NewGuid().ToString("N"));
            try
            {
                Directory.CreateDirectory(root);
                var paths = new AppPaths(root);
                var configDir = Path.GetDirectoryName(paths.ConfigFile)!;
                var otherDir = Path.Combine(root, "other");
                Directory.CreateDirectory(configDir);
                Directory.CreateDirectory(otherDir);
                foreach (var f in sc!["files"]!.AsArray())
                {
                    var dir = f!["dir"]!.GetValue<string>() == "other" ? otherDir : configDir;
                    File.WriteAllBytes(Path.Combine(dir, f["name"]!.GetValue<string>()), W203FileBytes(f));
                }

                var log = new W203Log();
                var errors = new List<string>();
                var store = new SettingsStore(paths, new PhysicalFileSystem(), log, (m, ex) => errors.Add(m + " [" + ex.GetType().Name + "]"));
                var changedEvents = 0;
                store.Changed += (_, _) => changedEvents++;
                var steps = new JsonArray();
                foreach (var st in sc["steps"]!.AsArray())
                {
                    var op = st!["op"]!.GetValue<string>();
                    var step = new JsonObject { ["op"] = op };
                    var before = changedEvents;
                    try
                    {
                        switch (op)
                        {
                            case "load":
                                store.Load();
                                break;
                            case "load_explicit":
                                store.Load(Path.Combine(otherDir, st["file"]!.GetValue<string>()));
                                break;
                            case "save":
                                var s = JsonSerializer.Deserialize(st["settings"]!.GetValue<string>(), AppSettingsJsonContext.Default.AppSettings)!;
                                store.Save(s);
                                break;
                            default:
                                throw new ArgumentException("op " + op);
                        }
                    }
                    catch (Exception ex)
                    {
                        step["thrown"] = ex.GetType().Name;
                    }
                    step["repairs"] = W203Array(store.LastLoadRepairs);
                    step["config_version"] = store.Current.ConfigVersion;
                    step["changed"] = W203Changed(store.Current);
                    step["changed_events"] = changedEvents - before;
                    steps.Add(step);
                }

                var files = new JsonArray();
                foreach (var (label, dir) in new[] { ("config", configDir), ("other", otherDir) })
                {
                    foreach (var path in Directory.EnumerateFiles(dir).Order(StringComparer.Ordinal))
                    {
                        var bytes = File.ReadAllBytes(path);
                        string? text = null;
                        try { text = new UTF8Encoding(false, true).GetString(bytes); } catch (DecoderFallbackException) { }
                        files.Add(new JsonObject
                        {
                            ["dir"] = label,
                            ["name"] = W203Stamp.Replace(Path.GetFileName(path), "<STAMP>"),
                            ["b64"] = Convert.ToBase64String(bytes),
                            ["text"] = text,
                        });
                    }
                }
                result.Add(new JsonObject
                {
                    ["name"] = sc["name"]!.GetValue<string>(),
                    ["steps"] = steps,
                    ["files_after"] = files,
                    ["startup_errors"] = W203Array(errors.Select(e => W203Normalize(e, root))),
                    ["log"] = W203Array(log.Lines.Select(l => W203Normalize(l, root))),
                });
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch (IOException) { }
            }
        }
        return result;
    }
}
