using System.Globalization;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Model;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W2-00 (shared .NET Path helpers) -- this file is ALSO required by every other W2-*.cs oracle file (helpers W2Util)
//     "dotnet-path" => Commands.DotnetPath(inputs),

// W2-00: dotnet-path. System.IO.Path (Windows) behaviours the Wave 2 catalog/file-action units depend on.
static class W2Util
{
    /// <summary>Runs <paramref name="f"/>; a throw becomes {"throws": "<ExceptionType>"} so the golden pins WHICH exception.</summary>
    public static JsonNode? Capture(Func<JsonNode?> f)
    {
        try { return f(); }
        catch (Exception ex) { return new JsonObject { ["throws"] = ex.GetType().Name }; }
    }

    public static JsonArray StrArray(IEnumerable<string?> items) => new(items.Select(s => (JsonNode?)(s is null ? null : JsonValue.Create(s))).ToArray());

    public static List<string> Strings(JsonNode? node) => node is null ? [] : node.AsArray().Select(n => n!.GetValue<string>()).ToList();

    public static List<string?> NullableStrings(JsonNode? node) => node is null ? [] : node.AsArray().Select(n => n?.GetValue<string>()).ToList();

    public static string? Str(JsonNode? node) => node?.GetValue<string>();
}

static partial class Commands
{
    /// <summary>
    /// Path.GetDirectoryName (string and ReadOnlySpan overloads), GetFileName, GetFileNameWithoutExtension, GetExtension,
    /// EndsInDirectorySeparator, TrimEndingDirectorySeparator, IsPathRooted, IsPathFullyQualified for "paths"; GetFullPath for
    /// "full_paths" (absolute inputs only: relative ones depend on the process directory); GetInvalidPathChars.
    /// </summary>
    public static JsonArray DotnetPath(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var path in W2Util.NullableStrings(inputs["paths"]))
        {
            result.Add(new JsonObject
            {
                ["kind"] = "path",
                ["path"] = path,
                ["directory_name"] = W2Util.Capture(() => Path.GetDirectoryName(path)),
                ["directory_name_raw"] = W2Util.Capture(() => Path.GetDirectoryName(path.AsSpan()).ToString()),
                ["file_name"] = W2Util.Capture(() => Path.GetFileName(path)),
                ["stem"] = W2Util.Capture(() => Path.GetFileNameWithoutExtension(path)),
                ["extension"] = W2Util.Capture(() => Path.GetExtension(path)),
                ["ends_in_separator"] = W2Util.Capture(() => Path.EndsInDirectorySeparator(path)),
                ["trim_ending"] = W2Util.Capture(() => Path.TrimEndingDirectorySeparator(path)),
                ["rooted"] = W2Util.Capture(() => Path.IsPathRooted(path)),
                ["fully_qualified"] = W2Util.Capture(() => Path.IsPathFullyQualified(path)),
            });
        }

        foreach (var path in W2Util.NullableStrings(inputs["full_paths"]))
        {
            result.Add(new JsonObject
            {
                ["kind"] = "full",
                ["path"] = path,
                ["full"] = W2Util.Capture(() => Path.GetFullPath(path!)),
            });
        }

        result.Add(new JsonObject
        {
            ["kind"] = "invalid_path_chars",
            ["code_points"] = new JsonArray(Path.GetInvalidPathChars().Select(c => (JsonNode?)JsonValue.Create((int)c)).ToArray()),
        });
        return result;
    }
}
