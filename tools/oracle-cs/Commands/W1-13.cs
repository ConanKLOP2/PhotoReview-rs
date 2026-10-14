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

// W1-13: path-root, journal-error-text
static partial class Commands
{
    /// <summary>Path.GetPathRoot (the volume key RecycleBinCapacity groups by).</summary>
    public static JsonArray PathRoot(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["paths"]!.AsArray())
        {
            var path = node?.GetValue<string>();
            result.Add(new JsonObject { ["path"] = path, ["root"] = Path.GetPathRoot(path) });
        }
        return result;
    }

    /// <summary>JournalErrors.IsKnown and the invariant English text persisted next to each code.</summary>
    public static JsonArray JournalErrorText(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["codes"]!.AsArray())
        {
            var code = node?.GetValue<string>();
            string? english;
            try { english = JournalErrors.EnglishText(code!); }
            catch (ArgumentException) { english = null; } // unknown (or null) code
            result.Add(new JsonObject { ["code"] = code, ["known"] = JournalErrors.IsKnown(code), ["english"] = english });
        }
        return result;
    }
}
