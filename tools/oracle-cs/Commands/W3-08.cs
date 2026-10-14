using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.IO;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W3-08
//     "partial-cleanup" => Commands.PartialCleanup(inputs),
// Standalone: does not use W2Util.

// W3-08: PartialDestinationCleanup.RemoveIfPartial / IsSameFile (internal static class: called through reflection).
static partial class Commands
{
    /// <summary>
    /// Case kinds:
    ///  "cleanup":   {name, dest:{len,ticks}|null, created:{len,ticks}, source_size, delete_fails}. A real temp file of "len" zero bytes with
    ///               last write "ticks" (UTC) is the destination; "created" is a FileStat built from the numbers; delete_fails holds the
    ///               file open with FileShare.None so File.Delete throws. Output: existed_before, exists_after, deleted, threw (exception type or null).
    ///  "same-file": {name, now:{len,ticks}, created:{len,ticks}}. Output: same_file.
    /// </summary>
    public static JsonArray PartialCleanup(JsonObject inputs)
    {
        var type = typeof(IFileSystem).Assembly.GetType("PhotoReview.Core.FileActions.PartialDestinationCleanup")!;
        const BindingFlags Flags = BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic;
        var remove = type.GetMethod("RemoveIfPartial", Flags)!;
        var isSame = type.GetMethod("IsSameFile", Flags)!;
        static FileStat Stat(JsonNode? n) => new(n!["len"]!.GetValue<long>(), new DateTime(n["ticks"]!.GetValue<long>(), DateTimeKind.Utc));

        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind };
            if (kind == "same-file")
            {
                row["same_file"] = (bool)isSame.Invoke(null, [Stat(c["now"]), Stat(c["created"])])!;
                result.Add(row);
                continue;
            }

            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-partial-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
            FileStream? holder = null;
            try
            {
                var destination = Path.Combine(root, "dest.jpg");
                var existedBefore = c["dest"] is not null;
                if (existedBefore)
                {
                    File.WriteAllBytes(destination, new byte[c["dest"]!["len"]!.GetValue<int>()]);
                    File.SetLastWriteTimeUtc(destination, new DateTime(c["dest"]!["ticks"]!.GetValue<long>(), DateTimeKind.Utc));
                    if (c["delete_fails"]!.GetValue<bool>())
                        holder = new FileStream(destination, FileMode.Open, FileAccess.Read, FileShare.None);
                }

                string? threw = null;
                try
                {
                    remove.Invoke(null, [new PhysicalFileSystem(), destination, c["source_size"]!.GetValue<long>(), Stat(c["created"])]);
                }
                catch (TargetInvocationException ex)
                {
                    threw = ex.InnerException!.GetType().Name;
                }

                holder?.Dispose();
                holder = null;
                var existsAfter = File.Exists(destination);
                row["existed_before"] = existedBefore;
                row["exists_after"] = existsAfter;
                row["deleted"] = existedBefore && !existsAfter;
                row["threw"] = threw;
                result.Add(row);
            }
            finally
            {
                holder?.Dispose();
                try { Directory.Delete(root, recursive: true); } catch { }
            }
        }
        return result;
    }
}
