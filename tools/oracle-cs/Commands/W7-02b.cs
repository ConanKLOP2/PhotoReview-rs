using System.Text.Json.Nodes;
using PhotoReview.Benchmarking;
using PhotoReview.Core.Abstractions;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W7-02b
//     "slow-link-source-reader" => Commands.SlowLinkSourceReaderCases(inputs),
//     "bench-recycle-bin" => Commands.BenchRecycleBinCases(inputs),
// Needs W2-00.cs (class W2Util) and a reference to PhotoReview.Benchmarking.dll: add to tools/oracle-cs/OracleCs.csproj
//     <Reference Include="PhotoReview.Benchmarking"><HintPath>$(CSharpCoreDir)\PhotoReview.Benchmarking.dll</HintPath></Reference>
// and make tools/oracle.ps1 build src\PhotoReview.Benchmarking\PhotoReview.Benchmarking.csproj into $coreDir and list the DLL in
// $requiredDlls (the same wiring step W7-02a needs; do it once). ThrottledReadStream is internal, so it is only observed through behaviour.
// Deterministic: no clock, no temp path or GUID reaches the output.

// W7-02b: SlowLinkSourceReader (pass-through of path/priority/bufferSize, byte and seek fidelity, inner failure) and
// BenchmarkRecycleBin (opt-in switch parsing, factory use, the temp-copy deleter's File.Delete semantics).
static partial class Commands
{
    private const string W702bPath = @"C:\bench\photo-001.jpg";

    private static byte[] W702bData(int length)
    {
        var data = new byte[length];
        for (var i = 0; i < length; i++) data[i] = (byte)((i * 31 + 7) % 251);
        return data;
    }

    private sealed class W702bInner(byte[] data, bool fails) : ISourceReader
    {
        public int Calls;
        public string? Path;
        public SourceReadPriority? Priority;
        public int? BufferSize;

        public Stream OpenSource(string path, SourceReadPriority priority, int bufferSize = 1024 * 1024)
        {
            Calls++;
            Path = path;
            Priority = priority;
            BufferSize = bufferSize;
            if (fails) throw new FileNotFoundException("inner open failed", path);
            return new MemoryStream(data, writable: false);
        }
    }

    /// <summary>
    /// SlowLinkSourceReader.OpenSource for each case {name, length, bandwidth_bytes_per_second|null, priority, buffer_size|null,
    /// inner_fails, ops:[{op:"read", count}|{op:"seek", offset, origin}]}. The inner reader serves bytes (i*31+7)%251. Reported:
    /// what the inner saw (calls, path, priority, buffer size), the open exception type, each op's result (read: n + checksum
    /// (s = (s*131 + b) mod 1000000007 over the bytes read); seek: new position; a throw: {"throws": type}), final position,
    /// length, can_read, can_seek.
    /// </summary>
    public static JsonArray SlowLinkSourceReaderCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var inner = new W702bInner(W702bData(c["length"]!.GetValue<int>()), c["inner_fails"]?.GetValue<bool>() == true);
            SharedBandwidthLimiter? limiter = c["bandwidth_bytes_per_second"] is { } bps ? new SharedBandwidthLimiter(bps.GetValue<long>()) : null;
            var reader = new SlowLinkSourceReader(inner, limiter);
            var priority = Enum.Parse<SourceReadPriority>(c["priority"]!.GetValue<string>());
            var item = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["length"] = c["length"]!.GetValue<int>(),
                ["bandwidth_bytes_per_second"] = c["bandwidth_bytes_per_second"]?.DeepClone(),
                ["priority"] = priority.ToString(),
                ["buffer_size"] = c["buffer_size"]?.DeepClone(),
                ["inner_fails"] = c["inner_fails"]?.GetValue<bool>() == true,
                ["ops"] = c["ops"]!.DeepClone(),
            };
            Stream? stream = null;
            try
            {
                stream = c["buffer_size"] is { } buf
                    ? reader.OpenSource(W702bPath, priority, buf.GetValue<int>())
                    : reader.OpenSource(W702bPath, priority);
                item["open_throws"] = null;
            }
            catch (Exception ex)
            {
                item["open_throws"] = ex.GetType().Name;
            }
            item["inner_calls"] = inner.Calls;
            item["inner_path"] = inner.Path;
            item["inner_priority"] = inner.Priority?.ToString();
            item["inner_buffer_size"] = inner.BufferSize;
            var ops = new JsonArray();
            if (stream is not null)
            {
                using (stream)
                {
                    foreach (var opNode in c["ops"]!.AsArray())
                    {
                        var op = opNode!.AsObject();
                        ops.Add(W2Util.Capture(() =>
                        {
                            if (op["op"]!.GetValue<string>() == "read")
                            {
                                var buffer = new byte[op["count"]!.GetValue<int>()];
                                var n = stream.Read(buffer, 0, buffer.Length);
                                long s = 0;
                                for (var i = 0; i < n; i++) s = (s * 131 + buffer[i]) % 1_000_000_007;
                                return new JsonObject { ["read"] = n, ["checksum"] = s };
                            }
                            return new JsonObject
                            {
                                ["position"] = stream.Seek(op["offset"]!.GetValue<long>(), Enum.Parse<SeekOrigin>(op["origin"]!.GetValue<string>())),
                            };
                        }));
                    }
                    item["final_position"] = stream.Position;
                    item["final_length"] = stream.Length;
                    item["can_read"] = stream.CanRead;
                    item["can_seek"] = stream.CanSeek;
                }
            }
            item["op_results"] = ops;
            result.Add(item);
        }
        return result;
    }

    private sealed class W702bMarkerBin : IRecycleBin
    {
        public void SendToRecycleBin(string path) { }
        public bool TryRestore(string originalPath, long expectedSize, DateTime expectedLastWriteUtc) => true;
    }

    /// <summary>
    /// BenchmarkRecycleBin. Cases by "kind":
    /// - "env" {value|null}: sets PHOTOREVIEW_BENCH_REAL_RECYCLE_BIN to value (null = removed), reports use_real_bin, how often Create
    ///   called the factory and whether it returned the factory's instance.
    /// - "delete" {setup}: Create() with the variable unset, then SendToRecycleBin on a path prepared by setup ("file", "missing-file",
    ///   "missing-dir", "directory", "readonly-file", "empty-path", "file-as-parent"); reports the outcome ("ok" or the exception type)
    ///   and whether the target still exists as a file / as a directory.
    /// - "members": CanRecycle, FitsInRecycleBin, DeletePermanently (exception type) and TryRestore of the default deleter.
    /// </summary>
    public static JsonArray BenchRecycleBinCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind };
            try
            {
                switch (kind)
                {
                    case "env":
                        {
                            var value = W2Util.Str(c["value"]);
                            Environment.SetEnvironmentVariable(BenchmarkRecycleBin.RealBinEnvironmentVariable, value);
                            var marker = new W702bMarkerBin();
                            var factoryCalls = 0;
                            var bin = BenchmarkRecycleBin.Create(() => { factoryCalls++; return marker; });
                            item["value"] = value;
                            item["use_real_bin"] = BenchmarkRecycleBin.UseRealBin;
                            item["factory_calls"] = factoryCalls;
                            item["returned_factory_instance"] = ReferenceEquals(bin, marker);
                            break;
                        }
                    case "delete":
                        {
                            Environment.SetEnvironmentVariable(BenchmarkRecycleBin.RealBinEnvironmentVariable, null);
                            var setup = c["setup"]!.GetValue<string>();
                            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-benchbin-" + Guid.NewGuid().ToString("N"));
                            Directory.CreateDirectory(root);
                            try
                            {
                                string target;
                                switch (setup)
                                {
                                    case "file":
                                        target = Path.Combine(root, "copy.jpg");
                                        File.WriteAllBytes(target, [1]);
                                        break;
                                    case "missing-file":
                                        target = Path.Combine(root, "missing.jpg");
                                        break;
                                    case "missing-dir":
                                        target = Path.Combine(root, "no-such-dir", "copy.jpg");
                                        break;
                                    case "directory":
                                        target = Path.Combine(root, "subdir");
                                        Directory.CreateDirectory(target);
                                        break;
                                    case "readonly-file":
                                        target = Path.Combine(root, "readonly.jpg");
                                        File.WriteAllBytes(target, [1]);
                                        File.SetAttributes(target, FileAttributes.ReadOnly);
                                        break;
                                    case "empty-path":
                                        target = "";
                                        break;
                                    case "file-as-parent":
                                        File.WriteAllBytes(Path.Combine(root, "plain.txt"), [1]);
                                        target = Path.Combine(root, "plain.txt", "child.jpg");
                                        break;
                                    default:
                                        throw new ArgumentException("unknown setup " + setup);
                                }
                                var bin = BenchmarkRecycleBin.Create(() => throw new InvalidOperationException("the real bin must not be created"));
                                string outcome;
                                try { bin.SendToRecycleBin(target); outcome = "ok"; }
                                catch (Exception ex) { outcome = ex.GetType().Name; }
                                item["setup"] = setup;
                                item["outcome"] = outcome;
                                item["target_file_exists_after"] = target.Length > 0 && File.Exists(target);
                                item["target_directory_exists_after"] = target.Length > 0 && Directory.Exists(target);
                            }
                            finally
                            {
                                try
                                {
                                    foreach (var f in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories)) File.SetAttributes(f, FileAttributes.Normal);
                                    Directory.Delete(root, recursive: true);
                                }
                                catch { }
                            }
                            break;
                        }
                    case "members":
                        {
                            Environment.SetEnvironmentVariable(BenchmarkRecycleBin.RealBinEnvironmentVariable, null);
                            IRecycleBin bin = BenchmarkRecycleBin.Create(() => throw new InvalidOperationException("the real bin must not be created"));
                            item["can_recycle"] = bin.CanRecycle(@"X:\anything.jpg");
                            item["fits_zero"] = bin.FitsInRecycleBin(@"X:\anything.jpg", 0);
                            item["fits_huge"] = bin.FitsInRecycleBin(@"X:\anything.jpg", long.MaxValue);
                            item["delete_permanently"] = W2Util.Capture(() => { bin.DeletePermanently(@"X:\anything.jpg"); return "ok"; });
                            item["try_restore"] = bin.TryRestore(@"X:\anything.jpg", 5, new DateTime(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc));
                            break;
                        }
                    default:
                        throw new ArgumentException("unknown kind " + kind);
                }
            }
            finally
            {
                Environment.SetEnvironmentVariable(BenchmarkRecycleBin.RealBinEnvironmentVariable, null);
            }
            result.Add(item);
        }
        return result;
    }
}
