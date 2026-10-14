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

// W1-06: natural-key
static partial class Commands
{
    /// <summary>ManagedNaturalComparer.BuildNaturalKey: the exact key string (digit-run length prefix, NUL terminators, lower-casing).</summary>
    public static JsonArray NaturalKey(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["names"]!.AsArray())
        {
            var name = node!.GetValue<string>();
            result.Add(new JsonObject { ["name"] = name, ["key"] = ManagedNaturalComparer.BuildNaturalKey(name) });
        }
        return result;
    }
}
