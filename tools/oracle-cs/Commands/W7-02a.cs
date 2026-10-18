using System.Diagnostics;
using System.Text.Json.Nodes;
using PhotoReview.Benchmarking;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.IO;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W7-02a
//     "slow-link-fs" => Commands.SlowLinkFs(inputs),
// Needs W2-00.cs (class W2Util) and a <Reference Include="PhotoReview.Benchmarking"> in OracleCs.csproj (tools/oracle.ps1 must
// copy PhotoReview.Benchmarking.dll next to PhotoReview.Core.dll). Wiring does that; this file is not run by the contract writer.
//
// W7-02a: SlowLinkFileSystem / SharedBandwidthLimiter / ThrottledReadStream.
// Deterministic output only: timing is reported as a boolean LOWER bound ("lower_bound_met"), never as a measured number.
static partial class Commands
{
    private static string? W702TypeOf(Action a)
    {
        try { a(); return null; }
        catch (Exception ex) { return ex.GetType().Name; }
    }

    private static string W702Hex(byte[] buffer, int count)
    {
        var take = Math.Min(count, 8);
        return Convert.ToHexString(buffer, 0, take).ToLowerInvariant();
    }

    /// <summary>Deterministic content: byte i = (i * 7 + 3) % 251.</summary>
    private static byte[] W702Content(int size) => Enumerable.Range(0, size).Select(i => (byte)((i * 7 + 3) % 251)).ToArray();

    private static SharedBandwidthLimiter? W702Limiter(JsonNode? bps) => bps is null ? null : new SharedBandwidthLimiter(bps.GetValue<long>());

    /// <summary>
    /// Cases {name, kind, ...}:
    ///  - "limiter-ctor" {bytes_per_second}: "throws" = exception type name or null.
    ///  - "limiter-consume-immediate" {bytes_per_second, bytes:[int..]}: Consume of each value (all &lt;= 0) returns; "completed", "fast" (&lt; 500 ms in total).
    ///  - "limiter-consume-lower-bound" {bytes_per_second, bytes, lower_bound_ms}: Consume completes (also when bytes &gt; one second's budget);
    ///    "completed" and "lower_bound_met" (elapsed &gt;= lower_bound_ms; the bucket starts empty so the true time is bytes/bps).
    ///  - "fs-forwarding" {files:[{name,size}], dirs:[name], bandwidth_bps?, probe_files:[name], probe_dirs:[name]}: latency 0; every forwarded
    ///    metadata call versus the same call on the inner PhysicalFileSystem. Names are sorted ordinal for stable output.
    ///  - "latency-lower-bound" {latency_ms, file_count, op: "file_exists"|"directory_exists"|"get_file_stat"|"try_probe_readable"|
    ///    "enumerate_files_with_stat"|"enumerate_readable_files_with_stat"|"resolve_real_path", repeat}: total time of `repeat` calls
    ///    (enumerate ops: `repeat` listings) is at least latency * calls * 0.8 where calls = repeat (or repeat * file_count for the enumerate ops).
    ///  - "throttled-read" {size, bandwidth_bps?, buffer_size?, ops:[{op:"read",count}|{op:"seek",origin:"begin|current|end",offset}|{op:"position"}|{op:"length"}|{op:"read_to_end"}]}:
    ///    reads the file through OpenReadShared of the decorator; per op the result ("n", "hex" of the first 8 bytes read, "position").
    ///    "is_throttled" = the returned stream is a ThrottledReadStream (Rust twin: a fake time source slept at least once).
    ///  - "open-missing" {bandwidth_bps?}: OpenReadShared of a missing file: "throws" = exception type.
    /// </summary>
    public static JsonArray SlowLinkFs(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind };
            switch (kind)
            {
                case "limiter-ctor":
                    item["bytes_per_second"] = c["bytes_per_second"]!.GetValue<long>();
                    item["throws"] = W702TypeOf(() => _ = new SharedBandwidthLimiter(c["bytes_per_second"]!.GetValue<long>()));
                    break;

                case "limiter-consume-immediate":
                {
                    var limiter = new SharedBandwidthLimiter(c["bytes_per_second"]!.GetValue<long>());
                    var clock = Stopwatch.StartNew();
                    foreach (var b in c["bytes"]!.AsArray()) limiter.Consume(b!.GetValue<int>());
                    item["completed"] = true;
                    item["fast"] = clock.ElapsedMilliseconds < 500;
                    break;
                }

                case "limiter-consume-lower-bound":
                {
                    var limiter = new SharedBandwidthLimiter(c["bytes_per_second"]!.GetValue<long>());
                    var clock = Stopwatch.StartNew();
                    limiter.Consume(c["bytes"]!.GetValue<int>());
                    item["completed"] = true;
                    item["lower_bound_met"] = clock.ElapsedMilliseconds >= c["lower_bound_ms"]!.GetValue<long>();
                    break;
                }

                case "fs-forwarding":
                {
                    var root = Path.Combine(Path.GetTempPath(), "pr-oracle-slowlink-" + Guid.NewGuid().ToString("N"));
                    Directory.CreateDirectory(root);
                    try
                    {
                        foreach (var d in c["dirs"]!.AsArray()) Directory.CreateDirectory(Path.Combine(root, d!.GetValue<string>()));
                        foreach (var f in c["files"]!.AsArray())
                            File.WriteAllBytes(Path.Combine(root, f!["name"]!.GetValue<string>()), W702Content(f["size"]!.GetValue<int>()));
                        var inner = new PhysicalFileSystem();
                        var slow = new SlowLinkFileSystem(inner, TimeSpan.Zero, W702Limiter(c["bandwidth_bps"]));
                        string P(string n) => Path.Combine(root, n);
                        item["file_exists"] = new JsonObject(c["probe_files"]!.AsArray().Select(n => KeyValuePair.Create(n!.GetValue<string>(), (JsonNode?)slow.FileExists(P(n.GetValue<string>())))));
                        item["directory_exists"] = new JsonObject(c["probe_dirs"]!.AsArray().Select(n => KeyValuePair.Create(n!.GetValue<string>(), (JsonNode?)slow.DirectoryExists(P(n.GetValue<string>())))));
                        item["stat_length"] = new JsonObject(c["probe_files"]!.AsArray().Select(n =>
                        {
                            var stat = slow.GetFileStat(P(n!.GetValue<string>()));
                            return KeyValuePair.Create(n.GetValue<string>(), (JsonNode?)(stat is null ? null : JsonValue.Create(stat.Length)));
                        }));
                        item["probe_readable"] = new JsonObject(c["probe_files"]!.AsArray().Select(n =>
                            KeyValuePair.Create(n!.GetValue<string>(), (JsonNode?)slow.TryProbeReadable(P(n.GetValue<string>()), out _))));
                        var skipped = 0;
                        item["enumerate_with_stat"] = new JsonArray(slow.EnumerateFilesWithStat(root, _ => true, _ => skipped++)
                            .OrderBy(e => Path.GetFileName(e.Path), StringComparer.Ordinal)
                            .Select(e => (JsonNode?)new JsonObject { ["name"] = Path.GetFileName(e.Path), ["length"] = e.Stat?.Length }).ToArray());
                        item["enumerate_with_stat_filtered_to_jpg"] = W2Util.StrArray(slow.EnumerateFilesWithStat(root, p => p.EndsWith(".jpg", StringComparison.Ordinal), _ => skipped++)
                            .Select(e => Path.GetFileName(e.Path)).OrderBy(n => n, StringComparer.Ordinal));
                        item["enumerate_readable_with_stat"] = W2Util.StrArray(slow.EnumerateReadableFilesWithStat(root, _ => true, _ => skipped++)
                            .Select(e => Path.GetFileName(e.Path)).OrderBy(n => n, StringComparer.Ordinal));
                        item["enumerate_files"] = W2Util.StrArray(slow.EnumerateFiles(root).Select(Path.GetFileName).OrderBy(n => n, StringComparer.Ordinal));
                        item["enumerate_directories"] = W2Util.StrArray(slow.EnumerateDirectories(root).Select(Path.GetFileName).OrderBy(n => n, StringComparer.Ordinal));
                        item["resolve_real_path_keeps_name"] = c["probe_files"]!.AsArray().Count == 0
                            || Path.GetFileName(slow.ResolveRealPath(P(c["probe_files"]![0]!.GetValue<string>()))) == c["probe_files"]![0]!.GetValue<string>();
                        item["skipped"] = skipped;
                    }
                    finally { try { Directory.Delete(root, recursive: true); } catch { } }
                    break;
                }

                case "latency-lower-bound":
                {
                    var root = Path.Combine(Path.GetTempPath(), "pr-oracle-slowlink-" + Guid.NewGuid().ToString("N"));
                    Directory.CreateDirectory(root);
                    try
                    {
                        var fileCount = c["file_count"]!.GetValue<int>();
                        for (var i = 0; i < fileCount; i++) File.WriteAllBytes(Path.Combine(root, $"f{i}.jpg"), W702Content(i + 1));
                        var latencyMs = c["latency_ms"]!.GetValue<int>();
                        var op = c["op"]!.GetValue<string>();
                        var repeat = c["repeat"]!.GetValue<int>();
                        var slow = new SlowLinkFileSystem(new PhysicalFileSystem(), TimeSpan.FromMilliseconds(latencyMs), null);
                        var target = Path.Combine(root, "f0.jpg");
                        var clock = Stopwatch.StartNew();
                        for (var r = 0; r < repeat; r++)
                        {
                            switch (op)
                            {
                                case "file_exists": slow.FileExists(target); break;
                                case "directory_exists": slow.DirectoryExists(root); break;
                                case "get_file_stat": slow.GetFileStat(target); break;
                                case "try_probe_readable": slow.TryProbeReadable(target, out _); break;
                                case "resolve_real_path": slow.ResolveRealPath(target); break;
                                case "enumerate_files_with_stat": slow.EnumerateFilesWithStat(root, _ => true, _ => { }).ToList(); break;
                                case "enumerate_readable_files_with_stat": slow.EnumerateReadableFilesWithStat(root, _ => true, _ => { }).ToList(); break;
                                default: throw new ArgumentException("unknown op " + op);
                            }
                        }
                        clock.Stop();
                        var calls = op.StartsWith("enumerate", StringComparison.Ordinal) ? repeat * fileCount : repeat;
                        item["calls"] = calls;
                        item["lower_bound_met"] = clock.ElapsedMilliseconds >= (long)(latencyMs * calls * 0.8);
                    }
                    finally { try { Directory.Delete(root, recursive: true); } catch { } }
                    break;
                }

                case "throttled-read":
                {
                    var root = Path.Combine(Path.GetTempPath(), "pr-oracle-slowlink-" + Guid.NewGuid().ToString("N"));
                    Directory.CreateDirectory(root);
                    try
                    {
                        var file = Path.Combine(root, "data.bin");
                        File.WriteAllBytes(file, W702Content(c["size"]!.GetValue<int>()));
                        var slow = new SlowLinkFileSystem(new PhysicalFileSystem(), TimeSpan.Zero, W702Limiter(c["bandwidth_bps"]));
                        using var stream = c["buffer_size"] is { } bs ? slow.OpenReadShared(file, bs.GetValue<int>()) : slow.OpenReadShared(file);
                        item["is_throttled"] = stream.GetType().Name == "ThrottledReadStream";
                        item["can_seek"] = stream.CanSeek;
                        item["can_write"] = stream.CanWrite;
                        var opResults = new JsonArray();
                        foreach (var o in c["ops"]!.AsArray())
                        {
                            var op = o!["op"]!.GetValue<string>();
                            var r = new JsonObject { ["op"] = op };
                            switch (op)
                            {
                                case "read":
                                {
                                    var buffer = new byte[o["count"]!.GetValue<int>()];
                                    var n = stream.Read(buffer, 0, buffer.Length);
                                    r["n"] = n;
                                    r["hex"] = W702Hex(buffer, n);
                                    break;
                                }
                                case "read_to_end":
                                {
                                    using var ms = new MemoryStream();
                                    stream.CopyTo(ms);
                                    var all = ms.ToArray();
                                    r["n"] = all.Length;
                                    r["sum"] = all.Sum(b => (long)b);
                                    break;
                                }
                                case "seek":
                                {
                                    var origin = o["origin"]!.GetValue<string>() switch
                                    {
                                        "begin" => SeekOrigin.Begin,
                                        "current" => SeekOrigin.Current,
                                        _ => SeekOrigin.End,
                                    };
                                    r["position"] = stream.Seek(o["offset"]!.GetValue<long>(), origin);
                                    break;
                                }
                                case "position": r["position"] = stream.Position; break;
                                case "length": r["length"] = stream.Length; break;
                                default: throw new ArgumentException("unknown op " + op);
                            }
                            opResults.Add(r);
                        }
                        item["results"] = opResults;
                    }
                    finally { try { Directory.Delete(root, recursive: true); } catch { } }
                    break;
                }

                case "open-missing":
                {
                    var missing = Path.Combine(Path.GetTempPath(), "pr-oracle-slowlink-missing-" + Guid.NewGuid().ToString("N"), "nope.bin");
                    var slow = new SlowLinkFileSystem(new PhysicalFileSystem(), TimeSpan.Zero, W702Limiter(c["bandwidth_bps"]));
                    item["throws"] = W702TypeOf(() => slow.OpenReadShared(missing).Dispose());
                    break;
                }

                default:
                    throw new ArgumentException("unknown kind " + kind);
            }
            result.Add(item);
        }
        return result;
    }
}
