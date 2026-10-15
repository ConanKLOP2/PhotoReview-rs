using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Localization;
using PhotoReview.Imaging.Caching;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-10a
//     "source-bytes-scenarios" => Commands.SourceBytesScenarios(inputs),
// WIRING: SourceBytesCache lives in PhotoReview.Imaging.dll (same reference W4-05a adds to tools/oracle-cs/OracleCs.csproj).
// Determinism (D19/D27): every case runs in its own fresh temp folder that is deleted afterwards; the output holds no path,
// no time and no exception message (only the exception type, ParamName and UserFacingError.IsLocalized). File times are
// only ever moved RELATIVE to the time the file got when it was written. Capacities are tiny (<= 1 MiB), so the
// RamBudgetPolicy clamp (20 % of physical RAM) never changes them on any machine; each case reports capacity_bytes anyway.
// Single-threaded only: the race tests stay unit tests (they need the internal test seams).

// W4-10a: SourceBytesCache (whole-file and range reads, TryGetRange, TryPrefetch, Evict, Clear, CanCache, CanCacheRange)
static partial class Commands
{
    /// <summary>Byte i of a generated file: (i + seed) % 251, or the constant "fill" when given.</summary>
    private static byte[] W410aContent(JsonObject spec)
    {
        var size = spec["size"]!.GetValue<int>();
        var seed = spec["seed"]?.GetValue<int>() ?? 0;
        var fill = spec["fill"]?.GetValue<int>();
        var bytes = new byte[size];
        for (var i = 0; i < size; i++) bytes[i] = fill is { } f ? (byte)f : (byte)((i + seed) % 251);
        return bytes;
    }

    /// <summary>FNV-1a 64 of the bytes as 16 lowercase hex digits (the Rust test computes the same).</summary>
    private static string W410aFnv(byte[] bytes)
    {
        var hash = 0xcbf29ce484222325UL;
        foreach (var b in bytes)
        {
            hash ^= b;
            hash *= 0x100000001b3UL;
        }

        return hash.ToString("x16");
    }

    /// <summary>
    /// Copy of SourceBytesCacheMutationTests.ChunkedStream: serves fixed bytes in chunks of at most <c>chunk</c>,
    /// <c>Length</c> can be forced, and the data can end early (<c>eofAfter</c>).
    /// </summary>
    private sealed class W410aChunkedStream(byte[] data, int chunk, long? lengthOverride, int? eofAfter) : Stream
    {
        private long _pos;
        public override bool CanRead => true;
        public override bool CanSeek => true;
        public override bool CanWrite => false;
        public override long Length => lengthOverride ?? data.Length;
        public override long Position { get => _pos; set => _pos = value; }
        public override void Flush() { }
        public override int Read(byte[] buffer, int offset, int count)
        {
            var limit = eofAfter ?? data.Length;
            var n = (int)Math.Max(0, Math.Min(Math.Min(count, chunk), limit - _pos));
            Array.Copy(data, _pos, buffer, offset, n);
            _pos += n;
            return n;
        }
        public override long Seek(long offset, SeekOrigin origin) => _pos = origin switch
        {
            SeekOrigin.Begin => offset,
            SeekOrigin.Current => _pos + offset,
            _ => Length + offset,
        };
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }

    /// <summary>
    /// Counts opens and records their priority. Without a "reader" spec it forwards to PhysicalSourceReader (byte-for-byte the
    /// default reader). Spec: {"serve": "file"|"fill"|"bytes", "fill"?, "bytes"?, "chunk", "length_override"?, "eof_after"?}
    /// or {"throw_on_open": true} (InvalidOperationException, like the C# "must not open" fake).
    /// </summary>
    private sealed class W410aReader(JsonObject? spec) : ISourceReader
    {
        public int Opens;
        public readonly List<string> Priorities = [];

        public Stream OpenSource(string path, SourceReadPriority priority, int bufferSize = 1024 * 1024)
        {
            Opens++;
            Priorities.Add(priority.ToString());
            if (spec is null) return PhysicalSourceReader.Instance.OpenSource(path, priority, bufferSize);
            if (spec["throw_on_open"]?.GetValue<bool>() == true) throw new InvalidOperationException("must not open");
            byte[] data = spec["serve"]!.GetValue<string>() switch
            {
                "file" => File.ReadAllBytes(path),
                "fill" => Enumerable.Repeat((byte)spec["fill"]!.GetValue<int>(), (int)new FileInfo(path).Length).ToArray(),
                "bytes" => spec["bytes"]!.AsArray().Select(n => (byte)n!.GetValue<int>()).ToArray(),
                var other => throw new ArgumentException("unknown serve " + other),
            };
            return new W410aChunkedStream(data, spec["chunk"]!.GetValue<int>(), spec["length_override"]?.GetValue<long>(),
                spec["eof_after"]?.GetValue<int>());
        }
    }

    private static JsonObject W410aError(Exception ex)
    {
        var error = new JsonObject { ["type"] = ex.GetType().Name };
        if (ex is ArgumentException arg) error["param"] = arg.ParamName;
        error["localized"] = UserFacingError.IsLocalized(ex);
        return error;
    }

    /// <summary>
    /// SourceBytesCache scenarios. Case: {name, capacity, files?: [{name, size, seed?, fill?}], dirs?: [name], reader?: {...},
    /// steps: [...]}. A step names its target with "file" (Path.Combine(root, file)) or "raw_path" (used verbatim).
    /// Ops: read, read_priority {priority}, read_range {offset, count, length?, ticks?, ticks_delta?, priority?},
    /// try_get_range {same fields}, prefetch, evict, clear, can_cache {value}, can_cache_range {value},
    /// write {file, size, seed?, fill?, add_minutes?} (create or rewrite, then move LastWriteTimeUtc by add_minutes from the
    /// time the previous version had), delete {file}.
    /// "length" defaults to the file's FileInfo.Length and "ticks" to its LastWriteTimeUtc.Ticks (+ ticks_delta).
    /// Output per step: {op, bytes?: {len, fnv, same_as}, hit?: bytes|null, value?: bool, error?: {type, param?, localized},
    /// count, current_size, opens (this step), priorities (this step)}. same_as = index of the earliest earlier step whose
    /// returned array is the same instance; omitted (null) for empty arrays, whose identity is not part of the contract.
    /// </summary>
    public static JsonArray SourceBytesScenarios(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-srcbytes-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
            try
            {
                foreach (var f in c["files"]?.AsArray() ?? [])
                {
                    var spec = f!.AsObject();
                    File.WriteAllBytes(Path.Combine(root, spec["name"]!.GetValue<string>()), W410aContent(spec));
                }

                foreach (var d in c["dirs"]?.AsArray() ?? []) Directory.CreateDirectory(Path.Combine(root, d!.GetValue<string>()));

                var reader = new W410aReader(c["reader"]?.AsObject());
                var cache = new SourceBytesCache(c["capacity"]!.GetValue<long>(), reader);
                var seen = new List<(int Step, byte[] Bytes)>();
                var steps = new JsonArray();
                var index = 0;
                foreach (var s in c["steps"]!.AsArray())
                {
                    var step = s!.AsObject();
                    var op = step["op"]!.GetValue<string>();
                    var path = step["raw_path"]?.GetValue<string>() ?? (step["file"] is { } file ? Path.Combine(root, file.GetValue<string>()) : null);
                    var opensBefore = reader.Opens;
                    var prioritiesBefore = reader.Priorities.Count;
                    var entry = new JsonObject { ["op"] = op };

                    JsonObject Bytes(byte[] bytes)
                    {
                        int? same = bytes.Length == 0 ? null : seen.Where(x => ReferenceEquals(x.Bytes, bytes)).Select(x => (int?)x.Step).FirstOrDefault();
                        seen.Add((index, bytes));
                        return new JsonObject { ["len"] = bytes.Length, ["fnv"] = W410aFnv(bytes), ["same_as"] = same };
                    }

                    long Length() => step["length"]?.GetValue<long>() ?? new FileInfo(path!).Length;
                    long Ticks() => step["ticks"]?.GetValue<long>() ?? new FileInfo(path!).LastWriteTimeUtc.Ticks + (step["ticks_delta"]?.GetValue<long>() ?? 0);
                    SourceReadPriority Priority() => Enum.Parse<SourceReadPriority>(step["priority"]?.GetValue<string>() ?? "Viewer");

                    try
                    {
                        switch (op)
                        {
                            case "read":
                                entry["bytes"] = Bytes(cache.GetOrRead(path!));
                                break;
                            case "read_priority":
                                entry["bytes"] = Bytes(cache.GetOrRead(path!, Priority()));
                                break;
                            case "read_range":
                                entry["bytes"] = Bytes(step["priority"] is null
                                    ? cache.GetOrReadRange(path!, Length(), Ticks(), step["offset"]!.GetValue<long>(), step["count"]!.GetValue<int>())
                                    : cache.GetOrReadRange(path!, Length(), Ticks(), step["offset"]!.GetValue<long>(), step["count"]!.GetValue<int>(), Priority()));
                                break;
                            case "try_get_range":
                                var hit = cache.TryGetRange(path!, Length(), Ticks(), step["offset"]!.GetValue<long>(), step["count"]!.GetValue<int>(), out var got);
                                entry["value"] = hit;
                                entry["hit"] = hit ? Bytes(got) : null;
                                if (!hit) entry["miss_len"] = got.Length; // C#: the out value is a non-null empty array on a miss
                                break;
                            case "prefetch":
                                entry["value"] = cache.TryPrefetch(path!);
                                break;
                            case "evict":
                                cache.Evict(path!);
                                break;
                            case "clear":
                                cache.Clear();
                                break;
                            case "can_cache":
                                entry["value"] = cache.CanCache(step["value"]!.GetValue<long>());
                                break;
                            case "can_cache_range":
                                entry["value"] = cache.CanCacheRange(step["value"]!.GetValue<long>());
                                break;
                            case "write":
                                var previous = File.Exists(path!) ? File.GetLastWriteTimeUtc(path!) : (DateTime?)null;
                                File.WriteAllBytes(path!, W410aContent(step));
                                if (step["add_minutes"] is { } minutes && previous is { } before)
                                    File.SetLastWriteTimeUtc(path!, before.AddMinutes(minutes.GetValue<int>()));
                                break;
                            case "delete":
                                File.Delete(path!);
                                break;
                            default:
                                throw new ArgumentException("unknown op " + op);
                        }
                    }
                    catch (Exception ex) when (op is not ("write" or "delete"))
                    {
                        entry["error"] = W410aError(ex);
                    }

                    entry["count"] = cache.Count;
                    entry["current_size"] = cache.CurrentSize;
                    entry["opens"] = reader.Opens - opensBefore;
                    entry["priorities"] = W2Util.StrArray(reader.Priorities.Skip(prioritiesBefore));
                    steps.Add(entry);
                    index++;
                }

                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["capacity_bytes"] = cache.CapacityBytes,
                    ["steps"] = steps,
                });
            }
            finally
            {
                try { Directory.Delete(root, true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
            }
        }

        return result;
    }
}
