using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Benchmarking;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Model;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Preload;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W7-03b
//     "bench-profile-scope" => Commands.BenchProfileScope(inputs),
//     "bench-windowed-target" => Commands.BenchWindowedTarget(inputs),
// WIRING (done by the lead, not by a porter; this file does not run before that):
//   * tools/oracle-cs/OracleCs.csproj: add a Reference to PhotoReview.Benchmarking.dll (HintPath $(CSharpCoreDir)\PhotoReview.Benchmarking.dll);
//     tools/oracle.ps1: build src/PhotoReview.Benchmarking from the baseline archive and copy PhotoReview.Benchmarking.dll next to the other
//     assemblies (it references PhotoReview.Core/Imaging/Platform.Windows; only the static BenchmarkProfileScope and the internal
//     WindowedPreloadTarget are touched here, so Platform.Windows only has to be loadable, not used).
//   * The C# repo is never edited: WindowedPreloadTarget is `internal` (InternalsVisibleTo is only granted to PhotoReview.Integration.Tests), so
//     it is built and driven by REFLECTION, like the internals of W4-15b.
// Determinism: only absolute virtual paths (never touched on disk: the cache keys come from the entries' own length/write time), no clock, no
// temp path. Relative paths are forbidden in the inputs (Path.GetFullPath would read the process directory).

// W7-03b: BenchmarkProfileScope.ApplyLogging and the WindowedPreloadTarget of BenchmarkImageExecutor.
static partial class Commands
{
    private static readonly DateTime W703bWritten = new(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc);

    private static BenchmarkProfile W703bProfile(bool fullFolder, int next, int previous, bool detailedLogging) =>
        new("w703b", "w703b", "w703b", LoadingMode.Preview, 1, next, previous, fullFolder, 0, false, detailedLogging, BenchmarkWorkload.Sequential);

    /// <summary>
    /// ApplyLogging per case {name, initial: bool, detailed_logging: bool, use_write_count: bool, external: [bool...] (values another party writes
    /// through the SAME setter while the scope is open, each one counted as a write), dispose_twice: bool (after the first dispose the flag is flipped
    /// directly, without a counted write, and the scope is disposed again: the second dispose must restore nothing)}.
    /// Result: during (flag right after ApplyLogging), after_externals (flag after the external writes), after (after the first dispose),
    /// after_second_dispose (null unless dispose_twice), writes (counted setter calls in total).
    /// </summary>
    public static JsonArray BenchProfileScope(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var enabled = c["initial"]!.GetValue<bool>();
            var writes = 0;
            void Set(bool value) { enabled = value; writes++; }
            var profile = W703bProfile(false, 0, 0, c["detailed_logging"]!.GetValue<bool>());
            var useCount = c["use_write_count"]!.GetValue<bool>();

            var scope = BenchmarkProfileScope.ApplyLogging(profile, () => enabled, Set, useCount ? () => writes : null);
            var during = enabled;
            foreach (var external in (c["external"]?.AsArray() ?? []))
                Set(external!.GetValue<bool>());
            var afterExternals = enabled;
            scope.Dispose();
            var after = enabled;
            bool? afterSecond = null;
            if (c["dispose_twice"]?.GetValue<bool>() == true)
            {
                enabled = !enabled; // not a counted write
                scope.Dispose();
                afterSecond = enabled;
            }
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["during"] = during,
                ["after_externals"] = afterExternals,
                ["after"] = after,
                ["after_second_dispose"] = afterSecond,
                ["writes"] = writes,
            });
        }
        return result;
    }

    /// <summary>An inner IPreloadTarget that answers every cache probe with a constant, so the wrapper's verdict is observable.</summary>
    private sealed class W703bInner(bool cached) : IPreloadTarget
    {
        public bool TryGetCachedPreview(string path) => cached;
        public bool TryGetCachedPreview(ImageCacheKey key) => cached;
        public Task PreloadAsync(string path, CancellationToken cancellationToken = default) => Task.CompletedTask;
        public ImageCacheKey GetCurrentCacheKey(string path) => throw new NotSupportedException();
        public ImageCacheKey GetCurrentCacheKey(CatalogEntry entry) => throw new NotSupportedException();
        public int CacheCount => 0;
        public long CacheBytes => 0;
    }

    /// <summary>
    /// WindowedPreloadTarget per case {name, full_folder, next, previous, inner_cached: bool, files: [absolute paths], ops: [{center, probes: [paths]}]}.
    /// For each op: SetCenter(center) then, for every probe, the verdict of TryGetCachedPreview(string) with the probe AS GIVEN ("by_path") and of
    /// TryGetCachedPreview(ImageCacheKey) with a key built from the probe (key path = upper-invariant full path; "by_key"; null when the key cannot be built).
    /// With inner_cached=false the verdict is exactly "outside the window"; with inner_cached=true it is always true (the wrapper only ORs).
    /// If the constructor throws, the case result is {"throws": type} and the ops are skipped.
    /// </summary>
    public static JsonArray BenchWindowedTarget(JsonObject inputs)
    {
        var type = typeof(BenchmarkImageExecutor).Assembly.GetType("PhotoReview.Benchmarking.WindowedPreloadTarget", throwOnError: true)!;
        var flags = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var profile = W703bProfile(c["full_folder"]!.GetValue<bool>(), c["next"]!.GetValue<int>(), c["previous"]!.GetValue<int>(), false);
            var files = W2Util.Strings(c["files"]).ToArray();
            var innerCached = c["inner_cached"]?.GetValue<bool>() ?? false;
            var item = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["full_folder"] = c["full_folder"]!.GetValue<bool>(),
                ["next"] = c["next"]!.GetValue<int>(),
                ["previous"] = c["previous"]!.GetValue<int>(),
                ["inner_cached"] = innerCached,
                ["files"] = c["files"]!.DeepClone(),
            };
            object target;
            try
            {
                target = Activator.CreateInstance(type, flags, null, [new W703bInner(innerCached), files, profile], null)!;
            }
            catch (TargetInvocationException ex)
            {
                item["throws"] = ex.InnerException!.GetType().Name;
                result.Add(item);
                continue;
            }
            var setCenter = type.GetMethod("SetCenter", flags)!;
            var byPath = type.GetMethod("TryGetCachedPreview", flags, null, [typeof(string)], null)!;
            var byKey = type.GetMethod("TryGetCachedPreview", flags, null, [typeof(ImageCacheKey)], null)!;
            var ops = new JsonArray();
            foreach (var opNode in c["ops"]!.AsArray())
            {
                var op = opNode!.AsObject();
                setCenter.Invoke(target, [op["center"]!.GetValue<int>()]);
                var probes = new JsonArray();
                foreach (var probe in W2Util.Strings(op["probes"]))
                {
                    bool? keyVerdict;
                    try
                    {
                        var key = ImageCacheKey.Create(new CatalogEntry(probe).WithMetadata(1000, W703bWritten), false, new DecodeBox(100, 100));
                        keyVerdict = (bool)byKey.Invoke(target, [key])!;
                    }
                    catch (Exception) { keyVerdict = null; }
                    probes.Add(new JsonObject
                    {
                        ["path"] = probe,
                        ["by_path"] = (bool)byPath.Invoke(target, [probe])!,
                        ["by_key"] = keyVerdict,
                    });
                }
                ops.Add(new JsonObject { ["center"] = op["center"]!.GetValue<int>(), ["probes"] = probes });
            }
            item["ops"] = ops;
            result.Add(item);
        }
        return result;
    }
}
