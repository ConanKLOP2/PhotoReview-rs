// W2-01: settings-json  (AppSettings.cs + AppSettingsJsonContext.cs)
//
// Dispatch line to add to tools/oracle-cs/Program.cs (inside the `command switch`):
//     "settings-json" => Commands.SettingsJson(inputs),
//
// Copy this file to tools/oracle-cs/Commands/W2-01.cs and the input to tests/golden-inputs/settings-json.json.
// Output: tests/golden/settings-json.json (run tools/oracle.ps1 -Command settings-json).

using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using PhotoReview.Core.Settings;

static partial class Commands
{
    private static readonly Regex W201Top = new("^  \"([A-Za-z0-9]+)\": (.*)$", RegexOptions.Compiled);

    /// <summary>
    /// Splits config.json text (indented, CRLF) into top-level property -> value text (the text after <c>"Name": </c>, lines joined
    /// with CRLF, no trailing comma). Scalars are one line; Actions / Shortcuts / HiddenContextMenuItems are blocks.
    /// </summary>
    private static Dictionary<string, string> W201Segments(string json)
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
            var m = W201Top.Match(line);
            if (m.Success)
            {
                Flush();
                name = m.Groups[1].Value;
                buffer = [m.Groups[2].Value];
            }
            else if (name is not null && line != "}")
            {
                buffer.Add(line);
            }
        }
        Flush();
        return result;
    }

    private static JsonObject W201Changed(AppSettings settings)
    {
        var defaults = W201Segments(JsonSerializer.Serialize(new AppSettings(), AppSettingsJsonContext.Default.AppSettings));
        var actual = W201Segments(JsonSerializer.Serialize(settings, AppSettingsJsonContext.Default.AppSettings));
        var changed = new JsonObject();
        foreach (var (key, value) in actual)
            if (!defaults.TryGetValue(key, out var d) || d != value) changed[key] = value;
        return changed;
    }

    private static string W201ErrorType(Exception ex) => ex.GetType().Name;

    /// <summary>
    /// Maps a freshly deserialized C# object to what the Rust <c>AppSettings</c> can hold: a null Actions list or Shortcuts object
    /// becomes the defaults (SettingsStore.FinishDeserialize does the same), null strings become "", null Actions entries are
    /// dropped and counted (Rust: <c>read_notes.null_actions</c>), null UiLanguage / ExternalEditorPath become "". Nothing else changes (null HiddenContextMenuItems and null
    /// elements inside it stay; null LastMoveToFolder stays).
    /// </summary>
    private static int W201RustView(AppSettings s)
    {
        var dropped = 0;
        if (s.Actions is null) s.Actions = ReviewAction.Defaults();
        else
        {
            dropped = s.Actions.RemoveAll(a => a is null);
            foreach (var a in s.Actions) { a.Name ??= ""; a.Shortcut ??= ""; a.Destination ??= ""; }
        }
        if (s.Shortcuts is null) s.Shortcuts = new ShortcutMappings();
        else
            foreach (var p in typeof(ShortcutMappings).GetProperties().Where(p => p.PropertyType == typeof(string)))
                if (p.GetValue(s.Shortcuts) is null) p.SetValue(s.Shortcuts, "");
        s.UiLanguage ??= "";
        s.ExternalEditorPath ??= "";
        return dropped;
    }

    private static JsonObject W201Read(string name, string text)
    {
        var item = new JsonObject { ["kind"] = "read", ["name"] = name, ["text"] = text };
        try
        {
            var settings = JsonSerializer.Deserialize(text, AppSettingsJsonContext.Default.AppSettings);
            if (settings is null)
            {
                item["outcome"] = "null";
            }
            else
            {
                item["outcome"] = "ok";
                item["null_actions"] = W201RustView(settings);
                item["changed"] = W201Changed(settings);
            }
        }
        catch (Exception ex)
        {
            item["outcome"] = "error";
            item["error_type"] = W201ErrorType(ex);
        }
        return item;
    }

    /// <summary>
    /// W2-01. Input:
    ///   "read":      [ { "name", "text" } ]                    JsonSerializer.Deserialize(text, AppSettingsJsonContext.Default.AppSettings)
    ///   "scalars":   [ { "property", "tokens": [raw JSON value text] } ]   expands to read cases of {"&lt;property&gt;": token}
    ///   "roundtrip": [ { "name", "text" } ]                    read, then write with the same context (full text)
    ///   "doubles":   [ "0x3FE8000000000000" ]                  IEEE bits written through InfoOverlayFontSize and PreloadMemoryLoadLimit
    ///   "escape_codepoints": [ int ]                           one code point written through LastMoveToFolder
    /// Output kinds: defaults, read, roundtrip, double, escape.
    /// </summary>
    public static JsonArray SettingsJson(JsonObject inputs)
    {
        var result = new JsonArray();
        var ctx = AppSettingsJsonContext.Default.AppSettings;

        // ---- defaults: the whole default config.json text, key order, every public constant ----
        var defaultText = JsonSerializer.Serialize(new AppSettings(), ctx);
        var consts = new JsonObject();
        foreach (var f in typeof(AppSettings).GetFields(BindingFlags.Public | BindingFlags.Static).Where(f => f.IsLiteral))
            consts[f.Name] = JsonValue.Create(Convert.ToDouble(f.GetRawConstantValue(), System.Globalization.CultureInfo.InvariantCulture));
        using (var doc = JsonDocument.Parse(defaultText))
        {
            result.Add(new JsonObject
            {
                ["kind"] = "defaults",
                ["text"] = defaultText,
                ["keys"] = new JsonArray(doc.RootElement.EnumerateObject().Select(p => (JsonNode?)p.Name).ToArray()),
                ["consts"] = consts,
            });
        }

        foreach (var c in inputs["read"]!.AsArray())
            result.Add(W201Read(c!["name"]!.GetValue<string>(), c["text"]!.GetValue<string>()));

        foreach (var group in inputs["scalars"]!.AsArray())
        {
            var property = group!["property"]!.GetValue<string>();
            foreach (var token in group["tokens"]!.AsArray())
            {
                var t = token!.GetValue<string>();
                var item = W201Read(property + " <- " + t, "{\"" + property + "\":" + t + "}");
                item["kind"] = "scalar";
                item["property"] = property;
                item["token"] = t;
                item.Remove("text");
                result.Add(item);
            }
        }

        foreach (var c in inputs["roundtrip"]!.AsArray())
        {
            var item = new JsonObject { ["kind"] = "roundtrip", ["name"] = c!["name"]!.GetValue<string>(), ["text"] = c["text"]!.GetValue<string>() };
            var settings = JsonSerializer.Deserialize(item["text"]!.GetValue<string>(), ctx)!;
            W201RustView(settings);
            item["text_out"] = JsonSerializer.Serialize(settings, ctx);
            result.Add(item);
        }

        foreach (var node in inputs["doubles"]!.AsArray())
        {
            var bitsText = node!.GetValue<string>();
            var d = BitConverter.Int64BitsToDouble(Convert.ToInt64(bitsText, 16));
            var item = new JsonObject { ["kind"] = "double", ["bits"] = bitsText };
            try
            {
                var s = new AppSettings { InfoOverlayFontSize = d, PreloadMemoryLoadLimit = d };
                var segs = W201Segments(JsonSerializer.Serialize(s, ctx));
                item["text"] = segs["InfoOverlayFontSize"];
                item["text_limit"] = segs["PreloadMemoryLoadLimit"];
            }
            catch (Exception ex)
            {
                item["error_type"] = W201ErrorType(ex);
            }
            result.Add(item);
        }

        foreach (var node in inputs["escape_codepoints"]!.AsArray())
        {
            var cp = node!.GetValue<int>();
            var text = char.ConvertFromUtf32(cp);
            var segs = W201Segments(JsonSerializer.Serialize(new AppSettings { LastMoveToFolder = "a" + text + "b" }, ctx));
            result.Add(new JsonObject { ["kind"] = "escape", ["codepoint"] = cp, ["json"] = segs["LastMoveToFolder"] });
        }
        return result;
    }
}
