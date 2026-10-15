using System.Reflection;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-08a
//     "atomic-cache-file" => Commands.AtomicCacheFileCases(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * AtomicCacheFile is `internal static` in PhotoReview.Imaging.dll (see also the W4-01a note: oracle.ps1 must build Imaging
//     and OracleCs.csproj must reference it). It is reached by reflection below, so no InternalsVisibleTo is needed.
//   * Needs no other oracle file.

// W4-08a: AtomicCacheFile.WriteAsync (5-argument overload with the temp-path seam) on a real temp tree.
static partial class Commands
{
    private static readonly MethodInfo W408Write = typeof(PhotoReview.Imaging.ImageCacheKey).Assembly
        .GetType("PhotoReview.Imaging.Caching.AtomicCacheFile", throwOnError: true)!
        .GetMethods(BindingFlags.Static | BindingFlags.NonPublic | BindingFlags.Public)
        .Single(m => m.Name == "WriteAsync" && m.GetParameters().Length == 5);

    private static byte[] W408Bytes(JsonObject o)
    {
        var one = Convert.FromHexString(o["hex"]?.GetValue<string>() ?? "");
        var repeat = o["repeat"]?.GetValue<int>() ?? 1;
        var all = new byte[one.Length * repeat];
        for (var i = 0; i < repeat; i++) Buffer.BlockCopy(one, 0, all, i * one.Length, one.Length);
        return all;
    }

    /// <summary>Sorted relative path -> lowercase hex (or "&lt;N bytes&gt;" over 32 bytes); directories end in "/" with value "&lt;dir&gt;"; temp GUID names normalised.</summary>
    private static JsonObject W408Snapshot(string root)
    {
        var o = new JsonObject();
        var entries = Directory.EnumerateFileSystemEntries(root, "*", SearchOption.AllDirectories)
            .Select(p => (full: p, rel: Path.GetRelativePath(root, p).Replace('\\', '/')))
            .OrderBy(e => e.rel, StringComparer.Ordinal);
        foreach (var (full, rel) in entries)
        {
            var name = Regex.Replace(rel, @"\.[0-9a-f]{32}\.tmp$", ".<GUID>.tmp");
            if (Directory.Exists(full)) { o[name + "/"] = "<dir>"; continue; }
            var bytes = File.ReadAllBytes(full);
            o[name] = bytes.Length > 32 ? "<" + bytes.Length + " bytes>" : Convert.ToHexString(bytes).ToLowerInvariant();
        }
        return o;
    }

    /// <summary>
    /// Cases {name, steps:[...]}; the steps run in order on one fresh temp root:
    ///  * {op:"write", target, payload:"ok"|"throw"|"throw_after_write"|"cancel_after_write", hex?, repeat?, cancel_before?, temp?}
    ///    -> AtomicCacheFile.WriteAsync(root/target, payload, null, temp is null ? null : root/(target dir)/temp, token).
    ///    "ok" writes hex*repeat; "throw" throws InvalidOperationException("boom") at once; "throw_after_write" writes then throws;
    ///    "cancel_after_write" writes then cancels the token. Result: {op, outcome:"ok"} or {op, outcome:"throws", type}.
    ///  * {op:"precreate", path, hex} writes a file (creating its folder); {op:"mkdir", path}.
    /// Output per case: name, steps (one result each), tree (snapshot of the root after the last step).
    /// </summary>
    public static JsonArray AtomicCacheFileCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.GetFullPath(Path.Combine(Path.GetTempPath(), "pr-oracle-atomic-" + Guid.NewGuid().ToString("N")));
            Directory.CreateDirectory(root);
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            var steps = new JsonArray();
            try
            {
                foreach (var s in c["steps"]!.AsArray())
                {
                    var step = s!.AsObject();
                    var op = step["op"]!.GetValue<string>();
                    var r = new JsonObject { ["op"] = op };
                    switch (op)
                    {
                        case "precreate":
                        {
                            var p = Path.Combine(root, step["path"]!.GetValue<string>());
                            Directory.CreateDirectory(Path.GetDirectoryName(p)!);
                            File.WriteAllBytes(p, W408Bytes(step));
                            break;
                        }
                        case "mkdir":
                            Directory.CreateDirectory(Path.Combine(root, step["path"]!.GetValue<string>()));
                            break;
                        case "write":
                        {
                            var target = Path.Combine(root, step["target"]!.GetValue<string>());
                            var temp = step["temp"]?.GetValue<string>();
                            var tempPath = temp is null ? null : Path.Combine(Path.GetDirectoryName(target)!, temp);
                            var payload = step["payload"]!.GetValue<string>();
                            var bytes = W408Bytes(step);
                            using var cts = new CancellationTokenSource();
                            if (step["cancel_before"]?.GetValue<bool>() == true) cts.Cancel();
                            Action<Stream> action = payload switch
                            {
                                "ok" => st => st.Write(bytes),
                                "throw" => _ => throw new InvalidOperationException("boom"),
                                "throw_after_write" => st => { st.Write(bytes); throw new InvalidOperationException("boom"); },
                                "cancel_after_write" => st => { st.Write(bytes); cts.Cancel(); },
                                _ => throw new ArgumentException("unknown payload " + payload),
                            };
                            try
                            {
                                var task = (Task)W408Write.Invoke(null, new object?[] { target, action, null, tempPath, cts.Token })!;
                                task.GetAwaiter().GetResult();
                                r["outcome"] = "ok";
                            }
                            catch (Exception ex)
                            {
                                var e = ex is TargetInvocationException { InnerException: { } inner } ? inner : ex;
                                r["outcome"] = "throws";
                                // Cancellation subtypes collapse to one name; IOException and UnauthorizedAccessException (rename onto a directory) to "IOException".
                                r["type"] = e is OperationCanceledException ? "OperationCanceledException"
                                          : e is IOException or UnauthorizedAccessException ? "IOException"
                                          : e.GetType().Name;
                            }
                            break;
                        }
                        default:
                            throw new ArgumentException("unknown op " + op);
                    }
                    steps.Add(r);
                }
                o["steps"] = steps;
                o["tree"] = W408Snapshot(root);
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch { }
            }
            result.Add(o);
        }
        return result;
    }
}
