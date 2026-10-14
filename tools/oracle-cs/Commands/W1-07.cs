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

// W1-07: image-file-types
static partial class Commands
{
    /// <summary>ImageFileTypes.IsSupported / IsRawPath and the Path.GetExtension they are built on.</summary>
    public static JsonArray ImageFileTypesCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["paths"]!.AsArray())
        {
            var path = node?.GetValue<string>();
            result.Add(new JsonObject
            {
                ["path"] = path,
                ["extension"] = path is null ? null : Path.GetExtension(path),
                ["supported"] = ImageFileTypes.IsSupported(path!),
                ["supported_raw_enabled"] = ImageFileTypes.IsSupported(path!, rawEnabled: true),
                ["is_raw"] = ImageFileTypes.IsRawPath(path),
            });
        }
        return result;
    }
}
