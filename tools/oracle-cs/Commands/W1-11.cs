using System.Globalization;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using PhotoReview.Core.Caching;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Model;
using PhotoReview.Core.Settings;
using PhotoReview.Core.Updates;

// W1-11 (from contract oracle-commands.cs)
static partial class Commands
{
// Paste into `static class Commands` in tools/oracle-cs/Program.cs.
// Needs: using System.Globalization; using System.Reflection; using PhotoReview.Core.Localization;
// switch cases:  "loc-template" => Commands.LocTemplateCases(inputs),  "loc-render" => Commands.LocRenderCases(inputs),
//                "loc-pseudo" => Commands.LocPseudoCases(inputs),

    private static readonly LocArg[] LocFixedArgs =
        [new("x", "<X>"), new("name", "<N>"), new("count", "<C>"), new("X", "<BIG>")];

    private static JsonArray LocStrings(IEnumerable<string> items) => new(items.Select(s => (JsonNode?)s).ToArray());

    /// <summary>LocTemplate.TryParse (valid and invalid texts), PlaceholderNames, Text after TransformLiterals/AppendLiteral, Render, Literal.</summary>
    public static JsonArray LocTemplateCases(JsonObject inputs)
    {
        var result = new JsonArray();
        var previous = CultureInfo.CurrentCulture;
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        try
        {
            foreach (var node in inputs["texts"]!.AsArray())
            {
                var text = node!.GetValue<string>();
                var item = new JsonObject { ["kind"] = "parse", ["text"] = text };
                var ok = LocTemplate.TryParse(text, out var t);
                item["ok"] = ok;
                if (ok)
                {
                    item["names"] = LocStrings(t!.PlaceholderNames);
                    item["identity_text"] = t.TransformLiterals(s => s).Text;
                    item["appended_text"] = t.AppendLiteral("}{ {x} ").Text;
                    item["appended_render"] = t.AppendLiteral("}{ {x} ").Render(LocFixedArgs);
                    item["render_no_args"] = t.Render([]);
                    item["render_args"] = t.Render(LocFixedArgs);
                }
                result.Add(item);
            }
            foreach (var node in inputs["literal_texts"]!.AsArray())
            {
                var text = node!.GetValue<string>();
                var t = LocTemplate.Literal(text);
                result.Add(new JsonObject
                {
                    ["kind"] = "literal",
                    ["text"] = text,
                    ["template_text"] = t.Text,
                    ["names"] = LocStrings(t.PlaceholderNames),
                    ["render_args"] = t.Render(LocFixedArgs),
                    ["identity_text"] = t.TransformLiterals(s => s).Text,
                    ["appended_text"] = t.AppendLiteral("}{ {x} ").Text,
                });
            }
        }
        finally { CultureInfo.CurrentCulture = previous; }
        return result;
    }

    private sealed class LocThrowingToString { public override string ToString() => throw new InvalidOperationException("boom"); }
    private sealed class LocNullToString { public override string? ToString() => null; }

    private static object? LocArgValue(string type, string? value) => type switch
    {
        "null" => null,
        "str" => value,
        "i64" => long.Parse(value!, CultureInfo.InvariantCulture),
        "u64" => ulong.Parse(value!, CultureInfo.InvariantCulture),
        "f64" => double.Parse(value!, NumberStyles.Float, CultureInfo.InvariantCulture),
        "f32" => float.Parse(value!, NumberStyles.Float, CultureInfo.InvariantCulture),
        "bool" => bool.Parse(value!),
        "char" => value![0],
        "throw" => new LocThrowingToString(),
        "nulltostring" => new LocNullToString(),
        _ => throw new ArgumentException("unknown arg type " + type),
    };

    /// <summary>
    /// How Render formats numbers under each culture (CurrentCulture is set per call), the culture facts the Rust
    /// NumberCulture is built from, and argument scenarios (first match wins, failing formatter, null, bool, char) under the invariant culture.
    /// </summary>
    public static JsonArray LocRenderCases(JsonObject inputs)
    {
        var result = new JsonArray();
        var previous = CultureInfo.CurrentCulture;
        try
        {
            var cultures = inputs["cultures"]!.AsArray().Select(c => c!.GetValue<string>()).ToList();
            foreach (var name in cultures)
            {
                var n = CultureInfo.GetCultureInfo(name).NumberFormat;
                result.Add(new JsonObject
                {
                    ["kind"] = "culture",
                    ["name"] = name,
                    ["negative_sign"] = n.NegativeSign,
                    ["positive_sign"] = n.PositiveSign,
                    ["decimal_separator"] = n.NumberDecimalSeparator,
                    ["nan_symbol"] = n.NaNSymbol,
                    ["positive_infinity_symbol"] = n.PositiveInfinitySymbol,
                    ["negative_infinity_symbol"] = n.NegativeInfinitySymbol,
                });
            }
            var template = LocTemplate.TryParse("{n}", out var t) ? t! : throw new InvalidOperationException();
            foreach (var node in inputs["numbers"]!.AsArray())
            {
                var type = node!["type"]!.GetValue<string>();
                var value = node["value"]!.GetValue<string>();
                var rendered = new JsonObject();
                foreach (var name in cultures)
                {
                    CultureInfo.CurrentCulture = CultureInfo.GetCultureInfo(name);
                    rendered[name] = template.Render([new LocArg("n", LocArgValue(type, value))]);
                }
                result.Add(new JsonObject { ["kind"] = "number", ["type"] = type, ["value"] = value, ["rendered"] = rendered });
            }
            CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
            foreach (var node in inputs["scenarios"]!.AsArray())
            {
                var text = node!["template"]!.GetValue<string>();
                var args = node["args"]!.AsArray()
                    .Select(a => new LocArg(a!["name"]!.GetValue<string>(), LocArgValue(a["type"]!.GetValue<string>(), a["value"]?.GetValue<string>())))
                    .ToArray();
                result.Add(new JsonObject
                {
                    ["kind"] = "scenario",
                    ["template"] = text,
                    ["args"] = node["args"]!.DeepClone(),
                    ["rendered"] = LocTemplate.TryParse(text, out var st) ? st!.Render(args) : throw new InvalidOperationException(text),
                });
            }
        }
        finally { CultureInfo.CurrentCulture = previous; }
        return result;
    }

    /// <summary>TranslatorModes: Accent, Padding (private, by reflection) for 0..padding_max, and CreatePseudo/CreateShowKeys applied to a one-key English catalog.</summary>
    public static JsonArray LocPseudoCases(JsonObject inputs)
    {
        var result = new JsonArray();
        var previous = CultureInfo.CurrentCulture;
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        try
        {
            var padding = typeof(TranslatorModes).GetMethod("Padding", BindingFlags.NonPublic | BindingFlags.Static)
                ?? throw new InvalidOperationException("TranslatorModes.Padding not found");
            var max = inputs["padding_max"]!.GetValue<int>();
            for (var length = 0; length <= max; length++)
                result.Add(new JsonObject { ["kind"] = "padding", ["length"] = length, ["padding"] = (string)padding.Invoke(null, [length])! });

            foreach (var node in inputs["texts"]!.AsArray())
            {
                var text = node!.GetValue<string>();
                var json = new JsonObject
                {
                    ["_meta"] = new JsonObject { ["code"] = "en", ["name"] = "English", ["nativeName"] = "English", ["plural"] = "one-other" },
                    ["k"] = text,
                }.ToJsonString();
                var warnings = new List<string>();
                if (!LanguageCatalog.TryParse(json, "oracle", out var catalog, warnings))
                    throw new InvalidOperationException(string.Join("; ", warnings));
                var english = Localizer.Create(catalog, []);
                var pseudo = TranslatorModes.CreatePseudo(english);
                var keys = TranslatorModes.CreateShowKeys(english);
                var args = new[] { new LocArg("name", "N"), new LocArg("n", 1234.5), new LocArg("x", "<X>") };
                result.Add(new JsonObject
                {
                    ["kind"] = "text",
                    ["text"] = text,
                    ["accent"] = TranslatorModes.Accent(text),
                    ["english_get"] = english.Get("k"),
                    ["pseudo_get"] = pseudo.Get("k"),
                    ["pseudo_format"] = pseudo.Format("k", args),
                    ["show_keys_get"] = keys.Get("k"),
                    ["pseudo_code"] = pseudo.Code,
                });
            }
        }
        finally { CultureInfo.CurrentCulture = previous; }
        return result;
    }
}
