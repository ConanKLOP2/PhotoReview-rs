using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Benchmarking;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W7-03c
//     "bench-random" => Commands.BenchRandom(inputs),
//     "bench-select-indices" => Commands.BenchSelectIndices(inputs),
//     "bench-images-per-sample" => Commands.BenchImagesPerSample(inputs),
//     "bench-prepare-edge" => Commands.BenchPrepareEdge(inputs),
// Needs W2-00.cs (class W2Util: Capture, Str). Wiring must also (1) add a Reference to PhotoReview.Benchmarking.dll in
// tools/oracle-cs/OracleCs.csproj and have tools/oracle.ps1 copy it next to PhotoReview.Core.dll, and (2) NOT add
// InternalsVisibleTo: the internal/private members (SelectParallelIndices, StableHash) are reached by reflection below.
// Nothing here touches the disk, the clock or the Recycle Bin: PrepareIterationAsync is only called with an EMPTY file
// list and null executor/bin, so every path ends before any I/O (the not-implemented guard or an index/range error).

// W7-03c: BenchmarkWorkloadRunner pure behaviour (seeded Random, file selection, ImagesPerSample, empty-folder errors).
static partial class Commands
{
    private const BindingFlags W703cNonPublicStatic = BindingFlags.NonPublic | BindingFlags.Static;

    private static T W703cInvoke<T>(string method, params object?[] args)
    {
        try
        {
            return (T)typeof(BenchmarkWorkloadRunner).GetMethod(method, W703cNonPublicStatic)!.Invoke(null, args)!;
        }
        catch (TargetInvocationException ex) when (ex.InnerException is not null)
        {
            throw ex.InnerException;
        }
    }

    private static BenchmarkProfile W703cProfile(JsonObject c)
    {
        var id = c["profile_id"]!.GetValue<string>();
        var profile = BenchmarkProfiles.Find(id) ?? throw new ArgumentException("unknown profile " + id);
        if (c["workers"] is { } workers) profile = profile with { Workers = workers.GetValue<int>() };
        if (c["workload"] is { } workload) profile = profile with { Workload = Enum.Parse<BenchmarkWorkload>(workload.GetValue<string>()) };
        if (c["id"] is { } newId) profile = profile with { Id = newId.GetValue<string>() };
        return profile;
    }

    private static JsonNode? W703cThrows(Exception ex)
    {
        var o = new JsonObject { ["throws"] = ex.GetType().Name };
        if (ex is BenchmarkProfileException bpe)
        {
            o["problem"] = bpe.Problem.ToString();
            o["profile_id"] = bpe.ProfileId;
            o["message"] = bpe.Message;
        }
        return o;
    }

    /// <summary>
    /// Direct System.Random and CreateSeededRandom. Case {name, seed:int} or {name, text:string}, plus draws:[{max:int}|{} ]
    /// ({} = Next()). Result: "hash" (StableHash, text cases) or "seed", and "values": one number or {"throws"} per draw.
    /// A draw that throws leaves the generator unchanged (the range check comes first).
    /// </summary>
    public static JsonArray BenchRandom(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            Random random;
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            if (c["text"] is { } text)
            {
                var s = text.GetValue<string>();
                o["text"] = s;
                o["hash"] = W703cInvoke<int>("StableHash", s);
                random = BenchmarkWorkloadRunner.CreateSeededRandom(s);
            }
            else
            {
                var seed = c["seed"]!.GetValue<int>();
                o["seed"] = seed;
                random = new Random(seed);
            }
            var values = new JsonArray();
            foreach (var d in c["draws"]!.AsArray())
            {
                var draw = d!.AsObject();
                try
                {
                    values.Add(draw["max"] is { } max ? random.Next(max.GetValue<int>()) : random.Next());
                }
                catch (Exception ex)
                {
                    values.Add(W703cThrows(ex));
                }
            }
            o["draws"] = c["draws"]!.DeepClone();
            o["values"] = values;
            result.Add(o);
        }
        return result;
    }

    /// <summary>
    /// SelectParallelIndices over a run of iterations sharing ONE seeded Random. Case {name, profile_id, workers?, workload?, id?,
    /// file_count, iterations:[int], seed_text?}; the generator is CreateSeededRandom(seed_text ?? the profile's final Id).
    /// Result: "picked" = one int array (or {"throws"}) per iteration, and "images_per_sample" for the final workload.
    /// </summary>
    public static JsonArray BenchSelectIndices(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var profile = W703cProfile(c);
            var fileCount = c["file_count"]!.GetValue<int>();
            var random = BenchmarkWorkloadRunner.CreateSeededRandom(W2Util.Str(c["seed_text"]) ?? profile.Id);
            var picked = new JsonArray();
            foreach (var it in c["iterations"]!.AsArray())
            {
                try
                {
                    var indices = W703cInvoke<int[]>("SelectParallelIndices", fileCount, profile, profile.Workload, it!.GetValue<int>(), random);
                    picked.Add(new JsonArray(indices.Select(i => (JsonNode?)i).ToArray()));
                }
                catch (Exception ex)
                {
                    picked.Add(W703cThrows(ex));
                }
            }
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["profile_id"] = profile.Id,
                ["workers"] = profile.Workers,
                ["workload"] = profile.Workload.ToString(),
                ["file_count"] = fileCount,
                ["iterations"] = c["iterations"]!.DeepClone(),
                ["seed_text"] = W2Util.Str(c["seed_text"]) ?? profile.Id,
                ["picked"] = picked,
                ["images_per_sample"] = BenchmarkWorkloadRunner.ImagesPerSample(profile, profile.Workload, fileCount),
            });
        }
        return result;
    }

    /// <summary>
    /// ImagesPerSample over the cross product workloads x workers x file_counts (inputs are three lists, flattened in that order).
    /// The profile is the registry's fast-sequential with only Workers changed.
    /// </summary>
    public static JsonArray BenchImagesPerSample(JsonObject inputs)
    {
        var result = new JsonArray();
        var baseProfile = BenchmarkProfiles.Find("fast-sequential")!;
        foreach (var wl in inputs["workloads"]!.AsArray())
            foreach (var w in inputs["workers"]!.AsArray())
                foreach (var fc in inputs["file_counts"]!.AsArray())
                {
                    var workload = Enum.Parse<BenchmarkWorkload>(wl!.GetValue<string>());
                    var profile = baseProfile with { Workers = w!.GetValue<int>() };
                    result.Add(new JsonObject
                    {
                        ["workload"] = workload.ToString(),
                        ["workers"] = profile.Workers,
                        ["file_count"] = fc!.GetValue<int>(),
                        ["images_per_sample"] = BenchmarkWorkloadRunner.ImagesPerSample(profile, workload, fc.GetValue<int>()),
                    });
                }
        return result;
    }

    /// <summary>
    /// PrepareIterationAsync with an empty file list, no executor and no Recycle Bin (never any I/O). Case {name, profile_id, id?,
    /// workload (the workload argument; the profile keeps its own), profile_workload?, iteration}. Result: "stage" = where the first
    /// exception surfaced ("prepare" = PrepareIterationAsync itself, "measure" = invoking the returned step, "none"), plus the
    /// exception type, and for a BenchmarkProfileException its Problem, ProfileId and Message.
    /// </summary>
    public static JsonArray BenchPrepareEdge(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var profile = W703cProfile(c);
            if (c["profile_workload"] is { } pw) profile = profile with { Workload = Enum.Parse<BenchmarkWorkload>(pw.GetValue<string>()) };
            var workload = Enum.Parse<BenchmarkWorkload>(c["workload"]!.GetValue<string>());
            var iteration = c["iteration"]!.GetValue<int>();
            var random = BenchmarkWorkloadRunner.CreateSeededRandom(profile.Id);
            var o = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["profile_id"] = profile.Id,
                ["profile_workload"] = profile.Workload.ToString(),
                ["workload"] = workload.ToString(),
                ["iteration"] = iteration,
            };
            string stage = "prepare";
            try
            {
                var measure = BenchmarkWorkloadRunner.PrepareIterationAsync(
                    null!, [], profile, workload, iteration, random, null!, CancellationToken.None).GetAwaiter().GetResult();
                stage = "measure";
                measure().GetAwaiter().GetResult();
                stage = "none";
            }
            catch (Exception ex)
            {
                var t = W703cThrows(ex)!.AsObject();
                foreach (var kv in t.ToList()) { t.Remove(kv.Key); o[kv.Key] = kv.Value; }
            }
            o["stage"] = stage;
            result.Add(o);
        }
        return result;
    }
}
