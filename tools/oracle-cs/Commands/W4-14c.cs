using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Caching;
using PhotoReview.Imaging.Decoding;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-14c
//     "preview-disk-cache-path" => Commands.PreviewDiskCachePath(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * Needs PhotoReview.Imaging.dll referenced by OracleCs.csproj (same step as W4-01a) and W4-01a.cs (W401FromEntrySpec,
//     W401Describe): every key spec has length AND ticks, so no file system access (virtual paths).
//   * GetDiskCachePath is a private instance method: called through reflection on a service whose disk cache is DISABLED
//     (no startup cleanup task, nothing created on disk; GetDiskCachePath itself does not look at the switch). The
//     directory string is only combined, never touched.
//   * Deterministic: SHA-256 of the identity string; no clock, no machine state.

// W4-14c: PreviewImageService.GetDiskCachePath (the PERSISTED file name of a preview disk-cache entry, PARITY D-06).
static partial class Commands
{
    private static readonly MethodInfo W414cGetDiskCachePath =
        typeof(PreviewImageService).GetMethod("GetDiskCachePath", BindingFlags.NonPublic | BindingFlags.Instance)
        ?? throw new MissingMethodException(nameof(PreviewImageService), "GetDiskCachePath");

    private sealed class W414cNoDecoder : IImageDecoder
    {
        public IDecodedImage Decode(DecodeRequest request) => throw new NotSupportedException("W4-14c oracle: no decode");
        public ImageInfo ReadInfo(string path) => throw new NotSupportedException("W4-14c oracle: no decode");
    }

    /// <summary>
    /// Cases {name, directory, key: {path, length, ticks, original, w, h, orient?, backend?, source_kind?}} (W4-01a entry spec):
    /// result {key (described), file_name, path} or {throws, message}.
    /// </summary>
    public static JsonArray PreviewDiskCachePath(JsonObject inputs)
    {
        var result = new JsonArray();
        var services = new Dictionary<string, PreviewImageService>(StringComparer.Ordinal);
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var directory = c["directory"]!.GetValue<string>();
                if (!services.TryGetValue(directory, out var service))
                {
                    service = new PreviewImageService(new ReviewMetrics(), () => false, () => 256, PixelBufferImageCodec.Instance,
                        capacityBytes: 64L * 1024 * 1024, diskCacheDirectory: directory, disableDiskCacheOverride: true,
                        decoder: new W414cNoDecoder());
                    services[directory] = service;
                }
                var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["input"] = c.DeepClone() };
                try
                {
                    var key = W401FromEntrySpec(c["key"]!.AsObject());
                    var path = (string)W414cGetDiskCachePath.Invoke(service, [key])!;
                    o["key"] = W401Describe(key, null);
                    o["file_name"] = Path.GetFileName(path);
                    o["path"] = path;
                }
                catch (Exception ex)
                {
                    var inner = ex is TargetInvocationException { InnerException: { } i } ? i : ex;
                    o["throws"] = inner.GetType().Name;
                    o["message"] = inner.Message;
                }
                result.Add(o);
            }
        }
        finally
        {
            foreach (var service in services.Values) service.ShutdownPersistWorkersAsync().GetAwaiter().GetResult();
        }
        return result;
    }
}
