using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Model;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Pixels;

// W4-01b golden package: DecodeBox, DecodeRequest, ImageInfo, DecodedImage, DecodedImageSize, PixelLease, DecodeFailureSourceBytes.
// Copy to tools/oracle-cs/Commands/W4-01b.cs and add these dispatch lines to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W4-01b
//     "decode-box" => Commands.DecodeBoxCases(inputs),
//     "decode-types" => Commands.DecodeTypesCases(inputs),
// Needs: W2-00.cs (class W2Util) and a Reference to PhotoReview.Imaging.dll in tools/oracle-cs/OracleCs.csproj (the baseline build of
// src/PhotoReview.Imaging, plus its dependencies in the same folder; tools/oracle.ps1 must copy them next to PhotoReview.Core.dll).
// `DecodedImageSize` is internal, so it is reached by reflection.

static partial class Commands
{
    private static readonly MethodInfo W401bHasKnownOriginal = typeof(DecodedImage).Assembly
        .GetType("PhotoReview.Imaging.Decoding.DecodedImageSize", throwOnError: true)!
        .GetMethod("HasKnownOriginal", BindingFlags.Static | BindingFlags.NonPublic | BindingFlags.Public)!;

    private static JsonObject W401bSize((int Width, int Height) size) => new() { ["width"] = size.Width, ["height"] = size.Height };

    /// <summary>
    /// DecodeBox.Fit / FitStored / IsUnbounded for each case {name, box_width, box_height, source_width, source_height}.
    /// Reports fit, fit_stored_false, fit_stored_true (stored grid size) and is_unbounded.
    /// </summary>
    public static JsonArray DecodeBoxCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var box = new DecodeBox(c["box_width"]!.GetValue<int>(), c["box_height"]!.GetValue<int>());
            int sw = c["source_width"]!.GetValue<int>(), sh = c["source_height"]!.GetValue<int>();
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["box_width"] = box.Width,
                ["box_height"] = box.Height,
                ["source_width"] = sw,
                ["source_height"] = sh,
                ["is_unbounded"] = box.IsUnbounded,
                ["fit"] = W401bSize(box.Fit(sw, sh)),
                ["fit_stored_false"] = W401bSize(box.FitStored(sw, sh, false)),
                ["fit_stored_true"] = W401bSize(box.FitStored(sw, sh, true)),
            });
        }
        return result;
    }

    /// <summary>An IDecodedImage that overrides nothing optional: exercises the interface default members.</summary>
    private sealed class W401bBareImage(int w, int h) : IDecodedImage
    {
        public int PixelWidth => w;
        public int PixelHeight => h;
        public bool Downscaled => true;
        public int Orientation => 1;
        public long EstimatedBytes => 0;
        public object PlatformImage => new object();
    }

    /// <summary>
    /// Mixed cases keyed by "kind":
    ///  image-info      {w, h, orientation}                          -> width, height
    ///  request         {path, width, height, apply, priority, source_orientation|null, bytes_length|null, from_box}
    ///                                                               -> box, is_downscale_requested and the stored fields
    ///  decoded         {w, h, estimated_bytes, platform: "object"|"pixels:WxH", downscaled, orientation, backend, ow, oh, degraded}
    ///                                                               -> properties + has_known_original, or {throws, message}
    ///  interface-defaults {w, h}                                    -> the IDecodedImage default members
    ///  lease           {mode: "default"|"borrowed"|"owned"}         -> pixels_null, owned
    ///  failure-bytes   {first|null, second|null}                    -> try_get, bytes (after the attaches, in order)
    /// </summary>
    public static JsonArray DecodeTypesCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = c["kind"]!.GetValue<string>() };
            switch (c["kind"]!.GetValue<string>())
            {
                case "image-info":
                {
                    var info = new ImageInfo(c["w"]!.GetValue<int>(), c["h"]!.GetValue<int>(), c["orientation"]!.GetValue<int>());
                    o["width"] = info.Width;
                    o["height"] = info.Height;
                    break;
                }
                case "request":
                {
                    var path = c["path"]!.GetValue<string>();
                    var priority = Enum.Parse<SourceReadPriority>(c["priority"]!.GetValue<string>());
                    var apply = c["apply"]!.GetValue<bool>();
                    int? orientation = c["source_orientation"]?.GetValue<int>();
                    ReadOnlyMemory<byte>? bytes = c["bytes_length"] is { } len ? new byte[len.GetValue<int>()] : null;
                    int width = c["width"]!.GetValue<int>(), height = c["height"]!.GetValue<int>();
                    var request = c["from_box"]!.GetValue<bool>()
                        ? new DecodeRequest(path, new DecodeBox(width, height), apply, bytes, priority, orientation)
                        : new DecodeRequest(path, width, apply, bytes, height, priority, orientation);
                    o["box"] = W401bSize((request.Box.Width, request.Box.Height));
                    o["target_width"] = request.TargetWidth;
                    o["target_height"] = request.TargetHeight;
                    o["is_downscale_requested"] = request.IsDownscaleRequested;
                    o["apply_orientation"] = request.ApplyOrientation;
                    o["priority"] = request.Priority.ToString();
                    o["source_orientation"] = request.SourceOrientation;
                    o["bytes_length"] = request.Bytes?.Length;
                    break;
                }
                case "decoded":
                {
                    PixelBuffer? buffer = null;
                    try
                    {
                        var platform = c["platform"]!.GetValue<string>();
                        object image = new object();
                        if (platform.StartsWith("pixels:", StringComparison.Ordinal))
                        {
                            var parts = platform["pixels:".Length..].Split('x');
                            buffer = PixelBuffer.Allocate(int.Parse(parts[0]), int.Parse(parts[1]), PixelLayout.Bgr32);
                            image = buffer;
                        }
                        o["result"] = W2Util.Capture(() =>
                        {
                            var d = new DecodedImage(image, c["w"]!.GetValue<int>(), c["h"]!.GetValue<int>(),
                                c["estimated_bytes"]!.GetValue<long>(), downscaled: c["downscaled"]!.GetValue<bool>(),
                                orientation: c["orientation"]!.GetValue<int>(),
                                actualBackend: Enum.Parse<DecoderBackend>(c["backend"]!.GetValue<string>()),
                                originalWidth: c["ow"]!.GetValue<int>(), originalHeight: c["oh"]!.GetValue<int>(),
                                exif: null, isDegradedFallback: c["degraded"]!.GetValue<bool>());
                            return new JsonObject
                            {
                                ["pixel_width"] = d.PixelWidth,
                                ["pixel_height"] = d.PixelHeight,
                                ["estimated_bytes"] = d.EstimatedBytes,
                                ["downscaled"] = d.Downscaled,
                                ["orientation"] = d.Orientation,
                                ["actual_backend"] = d.ActualBackend.ToString(),
                                ["original_width"] = d.OriginalWidth,
                                ["original_height"] = d.OriginalHeight,
                                ["is_degraded_fallback"] = d.IsDegradedFallback,
                                ["has_exif"] = d.Exif is not null,
                                ["has_known_original"] = (bool)W401bHasKnownOriginal.Invoke(null, [d])!,
                            };
                        });
                    }
                    finally { buffer?.Dispose(); }
                    break;
                }
                case "interface-defaults":
                {
                    IDecodedImage d = new W401bBareImage(c["w"]!.GetValue<int>(), c["h"]!.GetValue<int>());
                    o["actual_backend"] = d.ActualBackend.ToString();
                    o["original_width"] = d.OriginalWidth;
                    o["original_height"] = d.OriginalHeight;
                    o["has_exif"] = d.Exif is not null;
                    o["is_degraded_fallback"] = d.IsDegradedFallback;
                    o["is_source_read_metrics"] = d is ISourceReadMetrics;
                    o["is_raw_preview_info"] = d is IRawPreviewInfo;
                    o["has_known_original"] = (bool)W401bHasKnownOriginal.Invoke(null, [d])!;
                    break;
                }
                case "lease":
                {
                    var mode = c["mode"]!.GetValue<string>();
                    using var pixels = PixelBuffer.Allocate(2, 2, PixelLayout.Bgr32);
                    var lease = mode switch
                    {
                        "default" => default,
                        "borrowed" => new PixelLease(pixels, owned: false),
                        _ => new PixelLease(PixelBuffer.Allocate(2, 2, PixelLayout.Bgr32), owned: true),
                    };
                    o["pixels_null"] = lease.Pixels is null;
                    o["owned"] = lease.Owned;
                    lease.Dispose(); // default: no-op; borrowed: buffer stays usable; owned: freed
                    break;
                }
                case "failure-bytes":
                {
                    var ex = new InvalidOperationException("x");
                    o["try_get_before"] = DecodeFailureSourceBytes.TryGet(ex, out _);
                    foreach (var key in new[] { "first", "second" })
                    {
                        if (c[key] is { } arr)
                            DecodeFailureSourceBytes.Attach(ex, arr.AsArray().Select(b => b!.GetValue<byte>()).ToArray());
                    }
                    var found = DecodeFailureSourceBytes.TryGet(ex, out var got);
                    o["try_get"] = found;
                    o["bytes"] = found ? new JsonArray(got.ToArray().Select(b => (JsonNode?)(int)b).ToArray()) : null;
                    break;
                }
                default:
                    throw new ArgumentException("unknown kind " + c["kind"]);
            }
            result.Add(o);
        }
        return result;
    }
}
