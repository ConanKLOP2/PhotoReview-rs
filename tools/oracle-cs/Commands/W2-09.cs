using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W2-09
//     "snapshot-validator" => Commands.SnapshotValidator(inputs),
// Needs W2-00.cs (class W2Util).

// W2-09: ExplorerSnapshotValidator.TryValidate / NormalizePath / CanonicalizeFolder / SamePath
static partial class Commands
{
    /// <summary>
    /// "cases":[{name, folder, ordered:[...], scanned:[...], status?: ExplorerOrderStatus name (default Available), reason?: string|null}] =>
    /// {valid, reason, ordered}. "normalize":[path] => NormalizePath / CanonicalizeFolder (or the exception type).
    /// "same_path":[[a,b]] => SamePath (or the exception type). All paths absolute (no process-directory dependence).
    /// </summary>
    public static JsonArray SnapshotValidator(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var status = Enum.Parse<ExplorerOrderStatus>(c["status"]?.GetValue<string>() ?? "Available");
            var snapshot = new ExplorerViewSnapshot(
                c["folder"]!.GetValue<string>(),
                W2Util.Strings(c["ordered"]),
                [],
                ExplorerGroupState.None,
                status,
                W2Util.Str(c["reason"]),
                DateTime.UtcNow);
            var valid = ExplorerSnapshotValidator.TryValidate(snapshot, W2Util.Strings(c["scanned"]), out var ordered, out var reason);
            result.Add(new JsonObject
            {
                ["kind"] = "validate",
                ["name"] = c["name"]!.GetValue<string>(),
                ["folder"] = snapshot.Folder,
                ["status"] = status.ToString(),
                ["snapshot_reason"] = W2Util.Str(c["reason"]),
                ["ordered_paths"] = c["ordered"]!.DeepClone(),
                ["scanned"] = c["scanned"]!.DeepClone(),
                ["valid"] = valid,
                ["reason"] = reason,
                ["ordered"] = W2Util.StrArray(ordered),
            });
        }

        foreach (var path in W2Util.Strings(inputs["normalize"]))
        {
            result.Add(new JsonObject
            {
                ["kind"] = "normalize",
                ["path"] = path,
                ["normalized"] = W2Util.Capture(() => ExplorerSnapshotValidator.NormalizePath(path)),
                ["canonical_folder"] = W2Util.Capture(() => ExplorerSnapshotValidator.CanonicalizeFolder(path)),
            });
        }

        foreach (var pair in inputs["same_path"]!.AsArray())
        {
            var a = pair![0]!.GetValue<string>();
            var b = pair[1]!.GetValue<string>();
            result.Add(new JsonObject
            {
                ["kind"] = "same_path",
                ["a"] = a,
                ["b"] = b,
                ["same"] = W2Util.Capture(() => ExplorerSnapshotValidator.SamePath(a, b)),
            });
        }
        return result;
    }
}
