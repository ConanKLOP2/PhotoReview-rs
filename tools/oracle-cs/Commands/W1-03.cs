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

// W1-03: flag-fields
static partial class Commands
{
    /// <summary>
    /// W1-03. Members (name, value) in declaration order of the two [Flags] enums. Input: "types".
    /// Each case: type, members[], all (value of All), default (value of Default).
    /// </summary>
    public static JsonArray FlagFields(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["types"]!.AsArray())
        {
            var name = node!.GetValue<string>();
            var type = typeof(LoadingMode).Assembly.GetType("PhotoReview.Core.Model." + name, throwOnError: true)!;
            var members = new JsonArray();
            foreach (var field in type.GetFields(BindingFlags.Public | BindingFlags.Static))
                members.Add(new JsonObject { ["name"] = field.Name, ["value"] = Convert.ToInt32(field.GetValue(null), CultureInfo.InvariantCulture) });
            result.Add(new JsonObject
            {
                ["type"] = name,
                ["members"] = members,
                ["all"] = Convert.ToInt32(Enum.Parse(type, "All"), CultureInfo.InvariantCulture),
                ["default"] = Convert.ToInt32(Enum.Parse(type, "Default"), CultureInfo.InvariantCulture),
            });
        }
        return result;
    }
}
