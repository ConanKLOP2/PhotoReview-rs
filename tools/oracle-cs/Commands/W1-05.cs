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

// W1-05: perf-options
static partial class Commands
{
    /// <summary>
    /// W1-05. Every public const of PerformanceOptions (name, C# type, value). Input: none (an empty object).
    /// </summary>
    public static JsonArray PerfOptions(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var field in typeof(PerformanceOptions).GetFields(BindingFlags.Public | BindingFlags.Static | BindingFlags.DeclaredOnly))
        {
            if (!field.IsLiteral) continue;
            JsonNode value = field.GetRawConstantValue() switch
            {
                long l => JsonValue.Create(l),
                int i => JsonValue.Create(i),
                double d => JsonValue.Create(d),
                bool b => JsonValue.Create(b),
                var other => throw new InvalidOperationException($"unsupported constant type {other?.GetType()}"),
            };
            result.Add(new JsonObject { ["name"] = field.Name, ["type"] = field.FieldType.Name, ["value"] = value });
        }
        return result;
    }
}
