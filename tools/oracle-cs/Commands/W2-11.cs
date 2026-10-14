using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Localization;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W2-11
//     "drag-drop" => Commands.DragDrop(inputs),
// Needs W2-00.cs (class W2Util).

// W2-11: DragDropInputService.Parse on a real temp tree
static partial class Commands
{
    private static string? W211WarningName(string? text)
    {
        if (text is null) return null;
        var known = new (string Name, string Text)[]
        {
            ("NoValidInput", Tr.CoreDragDropNoValidInput),
            ("OnlyFirstFolder", Tr.CoreDragDropOnlyFirstFolder),
            ("NoSupportedImages", Tr.CoreDragDropNoSupportedImages),
            ("FolderUnknown", Tr.CoreDragDropFolderUnknown),
            ("MixedFolders", Tr.CoreDragDropMixedFolders),
            ("UnsupportedIgnored", Tr.CoreDragDropUnsupportedIgnored),
        };
        var hit = known.Where(k => k.Text == text).Select(k => k.Name).ToList();
        return hit.Count == 1 ? hit[0] : "UNMAPPED:" + text;
    }

    /// <summary>
    /// Cases {name, dirs:[relative folders created], files:[relative files created (1 byte; parent folders are created)],
    /// paths:[dropped paths, "{root}" = the temp folder]|null, raw?:bool, webp_heic?:bool}. Result: kind, folder_path, initial_image_path,
    /// ignored_path_count, warning (the Tr text mapped back to its member name) with the temp folder written back as "{root}".
    /// </summary>
    public static JsonArray DragDrop(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.GetFullPath(Path.Combine(Path.GetTempPath(), "pr-oracle-dd-" + Guid.NewGuid().ToString("N")));
            Directory.CreateDirectory(root);
            try
            {
                foreach (var d in W2Util.Strings(c["dirs"])) Directory.CreateDirectory(Path.Combine(root, d));
                foreach (var f in W2Util.Strings(c["files"]))
                {
                    var full = Path.Combine(root, f);
                    Directory.CreateDirectory(Path.GetDirectoryName(full)!);
                    File.WriteAllBytes(full, [1]);
                }
                var dropped = c["paths"] is null ? null : W2Util.Strings(c["paths"]).Select(p => p.Replace("{root}", root, StringComparison.Ordinal)).ToList();
                var parsed = DragDropInputService.Parse(dropped, c["raw"]?.GetValue<bool>() ?? false, c["webp_heic"]?.GetValue<bool>() ?? false);
                string? Back(string? p) => p?.Replace(root, "{root}", StringComparison.OrdinalIgnoreCase);
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["dirs"] = c["dirs"]?.DeepClone(),
                    ["files"] = c["files"]?.DeepClone(),
                    ["paths"] = c["paths"]?.DeepClone(),
                    ["raw"] = c["raw"]?.GetValue<bool>() ?? false,
                    ["webp_heic"] = c["webp_heic"]?.GetValue<bool>() ?? false,
                    ["kind"] = parsed.Kind.ToString(),
                    ["is_valid"] = parsed.IsValid,
                    ["folder_path"] = Back(parsed.FolderPath),
                    ["initial_image_path"] = Back(parsed.InitialImagePath),
                    ["ignored_path_count"] = parsed.IgnoredPathCount,
                    ["warning"] = W211WarningName(parsed.Warning),
                });
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch { }
            }
        }
        return result;
    }
}
