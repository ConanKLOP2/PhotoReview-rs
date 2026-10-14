using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Model;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W2-05
//     "image-sort-entries" => Commands.ImageSortEntries(inputs),
//     "image-sort-paths" => Commands.ImageSortPaths(inputs),
//     "sibling-folders" => Commands.SiblingFolders(inputs),
// Needs W2-00.cs (class W2Util).

// W2-05: ImageSortService.SortEntries / Sort(files, ...) / SiblingFolderService.GetSorted
static partial class Commands
{
    private sealed class W205ReverseOrdinal : INaturalComparer
    {
        public int Compare(string? x, string? y) => string.CompareOrdinal(y, x);
    }

    /// <summary>Same order as the managed comparer but another type, so ImageSortService takes its generic (non fast-path) branch.</summary>
    private sealed class W205Opaque : INaturalComparer
    {
        public int Compare(string? x, string? y) => ManagedNaturalComparer.Instance.Compare(x, y);
    }

    private static INaturalComparer? W205Comparer(string? name) => name switch
    {
        null => null,
        "managed" => ManagedNaturalComparer.Instance,
        "reverse-ordinal" => new W205ReverseOrdinal(),
        "opaque" => new W205Opaque(),
        _ => throw new ArgumentException("unknown comparer " + name),
    };

    private sealed class W205Log : ILog
    {
        public readonly List<(string Message, string? ExceptionType)> Errors = [];
        public bool Enabled => true;
        public void Info(string message) { }
        public void Warn(string message) { }
        public void Error(string message, Exception? ex = null) => Errors.Add((message, ex?.GetType().Name));
    }

    /// <summary>
    /// ImageSortService.SortEntries for each case {name, mode, comparer?, entries:[{path, length|null}]}: the resulting paths in order
    /// (entries with the same path are told apart by "length", also reported back).
    /// </summary>
    public static JsonArray ImageSortEntries(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var mode = Enum.Parse<ImageSortMode>(c["mode"]!.GetValue<string>());
            var entries = c["entries"]!.AsArray().Select(e =>
            {
                var o = e!.AsObject();
                var entry = new CatalogEntry(o["path"]!.GetValue<string>());
                return o["length"] is { } len ? entry with { Length = len.GetValue<long>() } : entry;
            }).ToList();
            var sorted = ImageSortService.SortEntries(entries, mode, W205Comparer(W2Util.Str(c["comparer"])));
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["mode"] = mode.ToString(),
                ["comparer"] = W2Util.Str(c["comparer"]),
                ["entries"] = c["entries"]!.DeepClone(),
                ["sorted"] = new JsonArray(sorted.Select(e => (JsonNode?)new JsonObject
                {
                    ["path"] = e.Path,
                    ["length"] = e.Length,
                }).ToArray()),
            });
        }
        return result;
    }

    /// <summary>
    /// ImageSortService.Sort(files, mode, ...) on real temp files for each case {name, mode (enum name) | mode_text (string, may be null),
    /// files:[{name, size|null}], comparer?}: size null = the file is not created (missing). "listed" is the order given to Sort.
    /// Result: "sorted" names and the names the log reported as failed size reads (message must contain the full path).
    /// </summary>
    public static JsonArray ImageSortPaths(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-sort-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
            try
            {
                var names = new List<string>();
                foreach (var f in c["files"]!.AsArray())
                {
                    var o = f!.AsObject();
                    var name = o["name"]!.GetValue<string>();
                    names.Add(name);
                    if (o["size"] is { } size) File.WriteAllBytes(Path.Combine(root, name), new byte[size.GetValue<int>()]);
                }
                var files = names.Select(n => Path.Combine(root, n)).ToList();
                var log = new W205Log();
                var comparer = W205Comparer(W2Util.Str(c["comparer"]));
                List<string> sorted;
                string modeLabel;
                if (c.ContainsKey("mode_text"))
                {
                    var text = W2Util.Str(c["mode_text"]);
                    sorted = ImageSortService.Sort(files, text, comparer, log);
                    modeLabel = text is null ? "<null>" : "text:" + text;
                }
                else
                {
                    var mode = Enum.Parse<ImageSortMode>(c["mode"]!.GetValue<string>());
                    sorted = ImageSortService.Sort(files, mode, comparer, log);
                    modeLabel = mode.ToString();
                }
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["mode"] = modeLabel,
                    ["comparer"] = W2Util.Str(c["comparer"]),
                    ["listed"] = W2Util.StrArray(names),
                    ["sorted"] = W2Util.StrArray(sorted.Select(Path.GetFileName)),
                    ["failed_size_reads"] = W2Util.StrArray(log.Errors.Select(e => names.First(n => e.Message.Contains(Path.Combine(root, n), StringComparison.Ordinal)))),
                    ["failed_exception_types"] = W2Util.StrArray(log.Errors.Select(e => e.ExceptionType)),
                    ["messages_have_prefix"] = log.Errors.All(e => e.Message.StartsWith("Failed to read file size for ", StringComparison.Ordinal)),
                });
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch { }
            }
        }
        return result;
    }

    /// <summary>
    /// SiblingFolderService.GetSorted on a real temp tree for each case {name, folders:[{name, attr: "none"|"hidden"|"system"}], current:
    /// relative folder (may name a missing parent), expect_log}: the sorted sibling NAMES (Path.GetFileName), the number of logged
    /// errors and the exception type of the first one.
    /// </summary>
    public static JsonArray SiblingFolders(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-sib-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
            try
            {
                foreach (var f in c["folders"]!.AsArray())
                {
                    var o = f!.AsObject();
                    var dir = Directory.CreateDirectory(Path.Combine(root, o["name"]!.GetValue<string>()));
                    var attr = o["attr"]?.GetValue<string>() ?? "none";
                    if (attr == "hidden") dir.Attributes |= FileAttributes.Hidden;
                    if (attr == "system") dir.Attributes |= FileAttributes.System;
                }
                var log = new W205Log();
                var current = Path.Combine(root, c["current"]!.GetValue<string>());
                var siblings = SiblingFolderService.GetSorted(current, c["no_log"]?.GetValue<bool>() == true ? null : log);
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["folders"] = c["folders"]!.DeepClone(),
                    ["current"] = c["current"]!.GetValue<string>(),
                    ["siblings"] = W2Util.StrArray(siblings.Select(Path.GetFileName)),
                    ["logged_errors"] = log.Errors.Count,
                    ["first_error_exception"] = log.Errors.Count > 0 ? log.Errors[0].ExceptionType : null,
                });
            }
            finally
            {
                try
                {
                    foreach (var d in new DirectoryInfo(root).EnumerateDirectories()) d.Attributes = FileAttributes.Directory;
                    Directory.Delete(root, recursive: true);
                }
                catch { }
            }
        }
        return result;
    }
}
