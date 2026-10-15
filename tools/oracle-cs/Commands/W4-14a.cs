using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Model;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Caching;
using PhotoReview.Imaging.Decoding;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-14a
//     "preview-budget-line" => Commands.PreviewBudgetLine(inputs),
//     "preview-current-key" => Commands.PreviewCurrentKey(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * Needs PhotoReview.Imaging.dll referenced by OracleCs.csproj (same step as W4-01a).
//   * PreviewImageService.ResolveCapacity is `internal static`: called through reflection (no InternalsVisibleTo needed).
//   * preview-budget-line: pure arithmetic + text: no file system, no clock, no machine RAM (physical bytes come from the case).
//   * preview-current-key: needs W4-01a.cs (W401Entry, W401Describe, W401Capture). Every entry has Length AND LastWriteUtc,
//     so ImageCacheKey.Create(CatalogEntry, ...) never touches the disk (virtual paths). The service is built with the disk
//     cache disabled (no cleanup task, no directory created) and a do-nothing decoder; its persist workers are shut down.

// W4-14a: PreviewImageService.ResolveCapacity (effective preview-cache budget + the "Memory budgets" start-up line).
static partial class Commands
{
    private static readonly MethodInfo W414aResolveCapacity =
        typeof(PreviewImageService).GetMethod("ResolveCapacity", BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(nameof(PreviewImageService), "ResolveCapacity");

    /// <summary>
    /// Cases {name, requested, percent|null, physical, source|null, window|null: {forward, backward}}:
    /// result {capacity, line} or {throws, message}.
    /// </summary>
    public static JsonArray PreviewBudgetLine(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var requested = c["requested"]!.GetValue<long>();
            int? percent = c["percent"] is { } p ? p.GetValue<int>() : null;
            var physical = c["physical"]!.GetValue<long>();
            long? source = c["source"] is { } s ? s.GetValue<long>() : null;
            PreloadWindow? window = c["window"] is JsonObject w
                ? new PreloadWindow(w["forward"]!.GetValue<int>(), w["backward"]!.GetValue<int>())
                : null;
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["input"] = c.DeepClone() };
            try
            {
                var args = new object?[] { requested, percent, physical, source, null, window };
                var capacity = (long)W414aResolveCapacity.Invoke(null, args)!;
                o["capacity"] = capacity;
                o["line"] = (string)args[4]!;
            }
            catch (TargetInvocationException tie) when (tie.InnerException is { } ex)
            {
                o["throws"] = ex.GetType().Name;
                o["message"] = ex.Message;
            }
            result.Add(o);
        }
        return result;
    }

    private sealed class W414aNoDecoder : IImageDecoder
    {
        public IDecodedImage Decode(DecodeRequest request) => throw new NotSupportedException("W4-14a oracle: no decode");
        public ImageInfo ReadInfo(string path) => throw new NotSupportedException("W4-14a oracle: no decode");
    }

    /// <summary>
    /// PreviewImageService.GetCurrentCacheKey(CatalogEntry) for cases {name, path, length, ticks, original_mode, w, h,
    /// backend|null (null = no currentBackend argument: the C# default), width_only?: true (the Func&lt;int&gt; overload, h ignored)}:
    /// result {key} (W401Describe) or {throws, message}, plus how often the box / backend / mode sources were called while
    /// building that ONE key (the constructor's own backend call is excluded).
    /// </summary>
    public static JsonArray PreviewCurrentKey(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["input"] = c.DeepClone() };
            var original = c["original_mode"]!.GetValue<bool>();
            var w = c["w"]!.GetValue<int>();
            var h = c["h"]!.GetValue<int>();
            var backendName = c["backend"]?.GetValue<string>();
            var widthOnly = c["width_only"]?.GetValue<bool>() ?? false;
            int boxCalls = 0, backendCalls = 0, modeCalls = 0;
            Func<bool> mode = () => { modeCalls++; return original; };
            Func<DecoderBackend>? backend = backendName is null
                ? null
                : () => { backendCalls++; return Enum.Parse<DecoderBackend>(backendName); };
            var dir = Path.Combine(Path.GetTempPath(), "pr-oracle-w414a-never-created");
            PreviewImageService service = widthOnly
                ? new PreviewImageService(new ReviewMetrics(), mode, () => { boxCalls++; return w; }, PixelBufferImageCodec.Instance,
                    capacityBytes: 64L * 1024 * 1024, diskCacheDirectory: dir, disableDiskCacheOverride: true,
                    decoder: new W414aNoDecoder(), currentBackend: backend)
                : new PreviewImageService(new ReviewMetrics(), mode, () => { boxCalls++; return new DecodeBox(w, h); },
                    PixelBufferImageCodec.Instance, capacityBytes: 64L * 1024 * 1024, diskCacheDirectory: dir,
                    disableDiskCacheOverride: true, decoder: new W414aNoDecoder(), currentBackend: backend);
            try
            {
                boxCalls = 0; backendCalls = 0; modeCalls = 0;
                // The entry is built INSIDE the capture: `new CatalogEntry("")` itself throws ArgumentException (RequirePath),
                // before GetCurrentCacheKey runs (then every call count stays 0).
                o["result"] = W401Capture(() => W401Describe(service.GetCurrentCacheKey(
                    W401Entry(c["path"]!.GetValue<string>(), c["length"]!.GetValue<long>(), c["ticks"]!.GetValue<long>())), null), null);
                o["box_calls"] = boxCalls;
                o["backend_calls"] = backendCalls;
                o["mode_calls"] = modeCalls;
            }
            finally
            {
                service.ShutdownPersistWorkersAsync().GetAwaiter().GetResult();
            }
            result.Add(o);
        }
        return result;
    }
}
