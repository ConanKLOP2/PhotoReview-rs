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

// W1-01: model-enums
static partial class Commands
{
    /// <summary>
    /// W1-01. Members (name, numeric value, [JsonAlias] aliases, the text the lenient converter writes) of the 16 persisted
    /// Model enums, plus SortModePolicy.UsesExplorerOrder for every ImageSortMode member. Input: "enums" (type names).
    /// </summary>
    public static JsonArray ModelEnums(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["enums"]!.AsArray())
        {
            var name = node!.GetValue<string>();
            var type = typeof(LoadingMode).Assembly.GetType("PhotoReview.Core.Model." + name, throwOnError: true)!;
            var members = new JsonArray();
            foreach (var field in type.GetFields(BindingFlags.Public | BindingFlags.Static))
            {
                var value = field.GetValue(null)!;
                var aliases = new JsonArray();
                foreach (var attr in field.GetCustomAttributes<JsonAliasAttribute>(inherit: false))
                    foreach (var alias in attr.Aliases)
                        aliases.Add(alias);
                var member = new JsonObject
                {
                    ["name"] = field.Name,
                    ["value"] = Convert.ToInt32(value, CultureInfo.InvariantCulture),
                    ["aliases"] = aliases,
                    ["json"] = JsonSerializer.Deserialize<string>(JsonSerializer.Serialize(value, type)),
                };
                if (type == typeof(ImageSortMode))
                    member["uses_explorer_order"] = SortModePolicy.UsesExplorerOrder((ImageSortMode)value);
                members.Add(member);
            }
            result.Add(new JsonObject { ["enum"] = name, ["members"] = members });
        }
        return result;
    }
}
