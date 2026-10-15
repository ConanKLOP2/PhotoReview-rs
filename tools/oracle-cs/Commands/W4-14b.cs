using System.Text.Json.Nodes;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Model;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Caching;
using PhotoReview.Imaging.Decoding;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-14b
//     "preview-service-sequence" => Commands.PreviewServiceSequence(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * Needs PhotoReview.Imaging.dll referenced by OracleCs.csproj (same step as W4-01a) and W4-01a.cs (W401Describe).
//   * Real temp files (created, given fixed last-write times, deleted at the end); a scripted fake decoder; the disk
//     cache is DISABLED (the disk path is W4-14c's, tested there with a fake payload codec). Every operation is awaited
//     before the next one: no concurrency, so the counters are deterministic. Decode milliseconds and the EWMA are
//     not reported (timing). The "Memory budgets" log line is not reported (it contains the machine's RAM).
//   * Paths in the output are relative to "<ROOT>" (both the given and the upper-cased spelling of the temp root).

// W4-14b: PreviewImageService decode path, sequential scenarios (metrics, decoder calls, RAM cache, original dimensions).
static partial class Commands
{
    private sealed class W414bImage(object marker, int w, int h, bool downscaled, DecoderBackend backend, int ow, int oh,
        bool degraded) : IDecodedImage
    {
        public int PixelWidth => w;
        public int PixelHeight => h;
        public bool Downscaled => downscaled;
        public int Orientation => 1;
        public long EstimatedBytes => (long)w * h * 4;
        public object PlatformImage { get; } = marker;
        public DecoderBackend ActualBackend => backend;
        public int OriginalWidth => ow;
        public int OriginalHeight => oh;
        public bool IsDegradedFallback => degraded;
    }

    private sealed class W414bMeteredImage(object marker, int w, int h, bool downscaled, DecoderBackend backend, int ow, int oh,
        bool degraded, long bytesRead) : IDecodedImage, ISourceReadMetrics
    {
        private readonly W414bImage _inner = new(marker, w, h, downscaled, backend, ow, oh, degraded);
        public int PixelWidth => _inner.PixelWidth;
        public int PixelHeight => _inner.PixelHeight;
        public bool Downscaled => _inner.Downscaled;
        public int Orientation => 1;
        public long EstimatedBytes => _inner.EstimatedBytes;
        public object PlatformImage => _inner.PlatformImage;
        public DecoderBackend ActualBackend => _inner.ActualBackend;
        public int OriginalWidth => _inner.OriginalWidth;
        public int OriginalHeight => _inner.OriginalHeight;
        public bool IsDegradedFallback => _inner.IsDegradedFallback;
        public long SourceBytesRead => bytesRead;
    }

    private sealed class W414bUnknownOp(string op) : Exception("unknown op " + op);

    /// <summary>Marker object carried as PlatformImage: the serial of the decode that produced it (identity check).</summary>
    private sealed record W414bMarker(int Serial, string Decoder);

    /// <summary>Scripted decoder: per file name a spec; records every Decode / ReadInfo call.</summary>
    private sealed class W414bDecoder(string name, JsonObject specs, Func<string, string> rel) : IImageDecoder
    {
        public readonly JsonArray Calls = [];
        public int Serial;

        private JsonObject Spec(string path) =>
            specs[Path.GetFileName(path)] as JsonObject ?? throw new InvalidOperationException("no spec for " + path);

        public IDecodedImage Decode(DecodeRequest request)
        {
            var s = Spec(request.Path);
            Calls.Add(new JsonObject
            {
                ["decoder"] = name,
                ["call"] = "decode",
                ["path"] = rel(request.Path),
                ["w"] = request.TargetWidth,
                ["h"] = request.TargetHeight,
                ["apply_orientation"] = request.ApplyOrientation,
                ["bytes"] = request.Bytes is { } b ? b.Length : null,
                ["priority"] = request.Priority.ToString(),
            });
            switch (s["fails"]?.GetValue<string>())
            {
                case null: break;
                case "Busy": throw new DecoderBusyException("test: busy");
                case "NotSupported": throw new NotSupportedException("test: not supported");
                case "Canceled": throw new OperationCanceledException("test: decoder-internal cancel");
                case "IO": throw new IOException("test: io");
                case "InvalidData": throw new InvalidDataException("test: invalid data");
                default: throw new ArgumentException("unknown failure " + s["fails"]);
            }
            var serial = ++Serial;
            var marker = new W414bMarker(serial, name);
            var w = s["pixel_w"]!.GetValue<int>();
            var h = s["pixel_h"]!.GetValue<int>();
            var down = s["downscaled"]?.GetValue<bool>() ?? true;
            var backend = Enum.Parse<DecoderBackend>(s["backend"]?.GetValue<string>() ?? "WicDirect");
            var ow = s["original_w"]?.GetValue<int>() ?? w;
            var oh = s["original_h"]?.GetValue<int>() ?? h;
            var degraded = s["degraded"]?.GetValue<bool>() ?? false;
            return s["source_bytes_read"] is { } read
                ? new W414bMeteredImage(marker, w, h, down, backend, ow, oh, degraded, read.GetValue<long>())
                : new W414bImage(marker, w, h, down, backend, ow, oh, degraded);
        }

        public ImageInfo ReadInfo(string path)
        {
            var s = Spec(path);
            Calls.Add(new JsonObject { ["decoder"] = name, ["call"] = "read_info", ["path"] = rel(path) });
            if (s["info_fails"]?.GetValue<bool>() == true) throw new IOException("test: read info");
            return new ImageInfo(s["info_w"]?.GetValue<int>() ?? 1, s["info_h"]?.GetValue<int>() ?? 1,
                s["info_orientation"]?.GetValue<int>() ?? 1);
        }
    }

    private static JsonObject W414bMetrics(ReviewMetrics metrics)
    {
        var s = metrics.Snapshot();
        return new JsonObject
        {
            ["cache_hits"] = s.CacheHits,
            ["cache_misses"] = s.CacheMisses,
            ["inflight_joins"] = s.InflightJoins,
            ["disk_cache_hits"] = s.DiskCacheHits,
            ["source_reads"] = s.SourceReads,
            ["source_bytes_read"] = s.SourceBytesRead,
            ["source_open_count"] = s.SourceOpenCount,
        };
    }

    private static JsonObject W414bImageJson(IDecodedImage image) => new()
    {
        ["serial"] = image.PlatformImage is W414bMarker m ? m.Serial : -1,
        ["decoder"] = image.PlatformImage is W414bMarker m2 ? m2.Decoder : null,
        ["pixel_w"] = image.PixelWidth,
        ["pixel_h"] = image.PixelHeight,
        ["downscaled"] = image.Downscaled,
        ["actual_backend"] = image.ActualBackend.ToString(),
        ["estimated_bytes"] = image.EstimatedBytes,
    };

    private static JsonObject W414bError(Exception ex, Func<string, string> rel) => new()
    {
        ["throws"] = ex.GetType().Name,
        ["message"] = rel(ex.Message),
        ["localized"] = UserFacingError.IsLocalized(ex),
    };

    /// <summary>
    /// Cases {name, config, files: [{name, length, write_ticks}], decoder: {file name: spec}, raw_decoder?: {file name: spec}, ops}.
    /// config: {original_mode, w, h, backend|null, capacity_bytes?, original_dimensions_capacity?, source_bytes_capacity?,
    /// raw_full_enabled?}. Decoder spec: {pixel_w, pixel_h, downscaled?, backend?, original_w?, original_h?, degraded?,
    /// source_bytes_read?, fails?: Busy|NotSupported|Canceled|IO|InvalidData, info_w?, info_h?, info_orientation?, info_fails?}.
    /// Ops (each output step echoes the op): key {file, as} | preview {file, key?} | viewer {file, key} | original {file, key} |
    /// dims {file, key} | known_dims {key} | try_cached {key} | try_cached_path {file} | has_inflight {key} | evict {path_spelling}
    /// (relative to the root, may use "..", other case) | clear | clear_source_bytes | rewrite {file, length, write_ticks} |
    /// delete {file} | set_mode {original} | set_box {w, h} | set_backend {backend} | set_raw_full {enabled}.
    /// Every step reports: result, metrics, decoder calls made during the step, cache_count, cache_bytes.
    /// </summary>
    public static JsonArray PreviewServiceSequence(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.GetFullPath(Path.Combine(Path.GetTempPath(), "pr-oracle-w414b-" + Guid.NewGuid().ToString("N")));
            Directory.CreateDirectory(root);
            string Rel(string text) => text
                .Replace(root.ToUpperInvariant(), "<ROOT>", StringComparison.Ordinal)
                .Replace(root, "<ROOT>", StringComparison.OrdinalIgnoreCase);
            PreviewImageService? service = null;
            try
            {
                var cfg = c["config"]!.AsObject();
                var original = cfg["original_mode"]!.GetValue<bool>();
                var box = new DecodeBox(cfg["w"]!.GetValue<int>(), cfg["h"]!.GetValue<int>());
                var backendName = cfg["backend"]?.GetValue<string>();
                var backend = backendName is null ? DecoderBackend.Wpf : Enum.Parse<DecoderBackend>(backendName);
                var rawEnabled = cfg["raw_full_enabled"]?.GetValue<bool>() ?? false;
                foreach (var f in c["files"]!.AsArray())
                {
                    var fo = f!.AsObject();
                    var path = Path.Combine(root, fo["name"]!.GetValue<string>());
                    File.WriteAllBytes(path, Enumerable.Range(0, fo["length"]!.GetValue<int>()).Select(i => (byte)(i % 251)).ToArray());
                    File.SetLastWriteTimeUtc(path, new DateTime(fo["write_ticks"]!.GetValue<long>(), DateTimeKind.Utc));
                }
                var decoder = new W414bDecoder("main", c["decoder"]!.AsObject(), Rel);
                var rawDecoder = c["raw_decoder"] is JsonObject rawSpecs ? new W414bDecoder("raw", rawSpecs, Rel) : null;
                var metrics = new ReviewMetrics();
                SourceBytesCache? sourceBytes = cfg["source_bytes_capacity"] is { } sbc ? new SourceBytesCache(sbc.GetValue<long>()) : null;
                service = new PreviewImageService(metrics, () => original, () => box, PixelBufferImageCodec.Instance,
                    capacityBytes: cfg["capacity_bytes"]?.GetValue<long>() ?? 64L * 1024 * 1024,
                    diskCacheDirectory: Path.Combine(root, "cache-never-used"), disableDiskCacheOverride: true,
                    decoder: decoder, currentBackend: () => backend, sourceBytesCache: sourceBytes,
                    originalDimensionsCapacity: cfg["original_dimensions_capacity"]?.GetValue<int>()
                        ?? PreviewImageService.DefaultOriginalDimensionsCapacity,
                    rawFullDecoder: rawDecoder, isRawFullDecodeEnabled: () => rawEnabled);
                var keys = new Dictionary<string, ImageCacheKey>(StringComparer.Ordinal);
                var steps = new JsonArray();
                foreach (var opNode in c["ops"]!.AsArray())
                {
                    var op = opNode!.AsObject();
                    var kind = op["op"]!.GetValue<string>();
                    string FilePath() => Path.Combine(root, op["file"]!.GetValue<string>());
                    ImageCacheKey Key() => keys[op["key"]!.GetValue<string>()];
                    var mainBefore = decoder.Calls.Count;
                    var rawBefore = rawDecoder?.Calls.Count ?? 0;
                    var step = new JsonObject { ["op"] = op.DeepClone() };
                    try
                    {
                        switch (kind)
                        {
                            case "key":
                            {
                                var k = service.GetCurrentCacheKey(FilePath());
                                keys[op["as"]!.GetValue<string>()] = k;
                                step["result"] = new JsonObject { ["key"] = W401Describe(k, root) };
                                break;
                            }
                            case "preview":
                            {
                                var image = op["key"] is null
                                    ? service.GetPreviewAsync(FilePath()).GetAwaiter().GetResult()
                                    : service.GetPreviewAsync(FilePath(), Key()).GetAwaiter().GetResult();
                                step["result"] = W414bImageJson(image);
                                break;
                            }
                            case "viewer":
                                step["result"] = W414bImageJson(service.GetViewerPreviewAsync(FilePath(), Key(), CancellationToken.None)
                                    .GetAwaiter().GetResult());
                                break;
                            case "original":
                                step["result"] = W414bImageJson(service.DecodeOriginalAsync(FilePath(), Key(), CancellationToken.None)
                                    .GetAwaiter().GetResult());
                                break;
                            case "dims":
                            {
                                var (w, h) = service.GetOriginalDimensionsAsync(FilePath(), Key()).GetAwaiter().GetResult();
                                step["result"] = new JsonObject { ["w"] = w, ["h"] = h };
                                break;
                            }
                            case "known_dims":
                                step["result"] = service.TryGetKnownOriginalDimensions(Key(), out var d)
                                    ? new JsonObject { ["w"] = d.Width, ["h"] = d.Height }
                                    : new JsonObject { ["known"] = false };
                                break;
                            case "try_cached":
                                step["result"] = service.TryGetCachedPreview(Key(), out var hit)
                                    ? W414bImageJson(hit) : new JsonObject { ["cached"] = false };
                                break;
                            case "try_cached_path":
                                step["result"] = service.TryGetCachedPreview(FilePath(), out var hitPath)
                                    ? W414bImageJson(hitPath) : new JsonObject { ["cached"] = false };
                                break;
                            case "has_inflight":
                                step["result"] = new JsonObject { ["inflight"] = service.HasInflightPreview(Key()) };
                                break;
                            case "evict":
                            {
                                var seen = new JsonArray();
                                service.EvictCachedPath(Path.Combine(root, op["path_spelling"]!.GetValue<string>()), p => seen.Add(Rel(p)));
                                step["result"] = new JsonObject { ["also_invalidate"] = seen };
                                break;
                            }
                            case "clear":
                                service.ClearCache();
                                step["result"] = new JsonObject();
                                break;
                            case "clear_source_bytes":
                                service.ClearSourceBytesCache();
                                step["result"] = new JsonObject();
                                break;
                            case "rewrite":
                            {
                                var path = FilePath();
                                File.WriteAllBytes(path, new byte[op["length"]!.GetValue<int>()]);
                                File.SetLastWriteTimeUtc(path, new DateTime(op["write_ticks"]!.GetValue<long>(), DateTimeKind.Utc));
                                step["result"] = new JsonObject();
                                break;
                            }
                            case "delete":
                                File.Delete(FilePath());
                                step["result"] = new JsonObject();
                                break;
                            case "set_mode":
                                original = op["original"]!.GetValue<bool>();
                                step["result"] = new JsonObject();
                                break;
                            case "set_box":
                                box = new DecodeBox(op["w"]!.GetValue<int>(), op["h"]!.GetValue<int>());
                                step["result"] = new JsonObject();
                                break;
                            case "set_backend":
                                backend = Enum.Parse<DecoderBackend>(op["backend"]!.GetValue<string>());
                                step["result"] = new JsonObject();
                                break;
                            case "set_raw_full":
                                rawEnabled = op["enabled"]!.GetValue<bool>();
                                step["result"] = new JsonObject();
                                break;
                            default:
                                throw new W414bUnknownOp(kind);
                        }
                    }
                    catch (Exception ex) when (ex is not W414bUnknownOp)
                    {
                        step["result"] = W414bError(ex, Rel);
                    }
                    var calls = new JsonArray();
                    for (var i = mainBefore; i < decoder.Calls.Count; i++) calls.Add(decoder.Calls[i]!.DeepClone());
                    if (rawDecoder is not null)
                        for (var i = rawBefore; i < rawDecoder.Calls.Count; i++) calls.Add(rawDecoder.Calls[i]!.DeepClone());
                    step["decoder_calls"] = calls;
                    step["metrics"] = W414bMetrics(metrics);
                    step["cache_count"] = service.CacheCount;
                    step["cache_bytes"] = service.CacheBytes;
                    steps.Add(step);
                }
                result.Add(new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["input"] = c.DeepClone(), ["steps"] = steps });
            }
            finally
            {
                service?.ShutdownPersistWorkersAsync().GetAwaiter().GetResult();
                try { Directory.Delete(root, recursive: true); }
                catch (IOException) { }
                catch (UnauthorizedAccessException) { }
            }
        }
        return result;
    }
}
