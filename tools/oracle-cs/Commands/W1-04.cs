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

// W1-04: shortcut-canonical
static partial class Commands
{
    /// <summary>
    /// W1-04. ShortcutKeyCanonical and the ShortcutMappings / ReviewAction defaults.
    /// Input: "names" (strings or null given to Canonicalize), "optional_probes" (strings given to IsOptional).
    /// Output kinds: "alias-pair" (the table, in order), "canonicalize", "is-optional", "shortcut-defaults" (property name,
    /// default value, optional flag in reflection order + OptionalNames + the JSON STJ writes for new ShortcutMappings()),
    /// "action-default" (ReviewAction.Defaults() without the localized Name).
    /// </summary>
    public static JsonArray ShortcutCanonical(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var (alias, canonical) in ShortcutKeyCanonical.AliasPairs)
            result.Add(new JsonObject { ["kind"] = "alias-pair", ["alias"] = alias, ["canonical"] = canonical });

        foreach (var node in inputs["names"]!.AsArray())
        {
            var name = node?.GetValue<string>();
            result.Add(new JsonObject { ["kind"] = "canonicalize", ["name"] = name, ["canonical"] = ShortcutKeyCanonical.Canonicalize(name) });
        }

        foreach (var node in inputs["optional_probes"]!.AsArray())
        {
            var name = node!.GetValue<string>();
            result.Add(new JsonObject { ["kind"] = "is-optional", ["name"] = name, ["optional"] = ShortcutMappings.IsOptional(name) });
        }

        var defaults = new ShortcutMappings();
        var properties = new JsonArray();
        foreach (var p in typeof(ShortcutMappings).GetProperties(BindingFlags.Public | BindingFlags.Instance))
        {
            if (p.PropertyType != typeof(string)) continue;
            properties.Add(new JsonObject { ["name"] = p.Name, ["default"] = (string?)p.GetValue(defaults), ["optional"] = ShortcutMappings.IsOptional(p.Name) });
        }
        result.Add(new JsonObject
        {
            ["kind"] = "shortcut-defaults",
            ["properties"] = properties,
            ["optional_names"] = new JsonArray(ShortcutMappings.OptionalNames.Select(n => (JsonNode?)n).ToArray()),
            ["json"] = JsonNode.Parse(JsonSerializer.Serialize(defaults)),
        });

        foreach (var action in ReviewAction.Defaults())
            result.Add(new JsonObject
            {
                ["kind"] = "action-default",
                ["shortcut"] = action.Shortcut,
                ["operation"] = action.Operation.ToString(),
                ["destination"] = action.Destination,
                ["confirm"] = action.Confirm,
            });
        return result;
    }
}
