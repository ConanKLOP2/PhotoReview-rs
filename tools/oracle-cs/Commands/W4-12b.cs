using System.IO;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Model;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Metadata;

// W4-12b golden package: FallbackImageDecoder (IsFallbackable whitelist, Decode/ReadInfo fallback flow, backend relabelling, log text,
// metrics, source-bytes hand-over).
// Copy to tools/oracle-cs/Commands/W4-12b.cs and add these dispatch lines to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W4-12b
//     "fallback-is-fallbackable" => Commands.FallbackIsFallbackable(inputs),
//     "fallback-decoder" => Commands.FallbackDecoderCases(inputs),
// Needs a Reference to PhotoReview.Imaging.dll in tools/oracle-cs/OracleCs.csproj (same wiring as W4-01b / W4-01c).
// Not run by the drafting step: the wiring step runs it and writes tests/golden/fallback-*.json.

static partial class Commands
{
    private static Exception W412bMake(string kind, string message, int hresult) => kind switch
    {
        "NotSupported" => new NotSupportedException(message),
        "PlatformNotSupported" => new PlatformNotSupportedException(message),   // derives NotSupportedException
        "MissingCodec" => new MissingImageCodecException(message),             // derives NotSupportedException
        "InvalidCast" => new InvalidCastException(message),
        "Overflow" => new OverflowException(message),
        "Arithmetic" => new ArithmeticException(message),                      // base of Overflow: NOT whitelisted
        "Format" => new FormatException(message),
        "InvalidData" => new InvalidDataException(message),
        "COM" => new COMException(message, hresult),
        "DllNotFound" => new DllNotFoundException(message),
        "BadImageFormat" => new BadImageFormatException(message),
        "EntryPointNotFound" => new EntryPointNotFoundException(message),
        "FileNotFound" => new FileNotFoundException(message),
        "DirectoryNotFound" => new DirectoryNotFoundException(message),
        "OperationCanceled" => new OperationCanceledException(message),
        "TaskCanceled" => new TaskCanceledException(message),                  // derives OperationCanceledException
        "OutOfMemory" => new OutOfMemoryException(message),
        "MemoryAdmission" => new DecoderMemoryAdmissionException(message),     // derives IOException
        "Busy" => new DecoderBusyException(message),
        "InvalidOperation" => new InvalidOperationException(message),
        "Argument" => new ArgumentException(message),
        "ObjectDisposed" => new ObjectDisposedException(message),
        "IO" => new IOException(message),
        "FileLoad" => new FileLoadException(message),                          // derives IOException
        "UnauthorizedAccess" => new UnauthorizedAccessException(message),
        "Generic" => new Exception(message),
        _ => throw new ArgumentException("unknown exception kind " + kind),
    };

    private static readonly ExifSummary W412bExif = new() { CameraModel = "TestCam", Iso = 400 };

    private sealed class W412bImage(DecoderBackend backend, int tag) : IDecodedImage
    {
        public int PixelWidth => 640 + tag;
        public int PixelHeight => 480;
        public bool Downscaled => true;
        public int Orientation => 6;
        public long EstimatedBytes => 1_234_567;
        public object PlatformImage { get; } = new object();
        public DecoderBackend ActualBackend => backend;
        public int OriginalWidth => 6000;
        public int OriginalHeight => 4000;
        public bool IsDegradedFallback => true;
        public ExifSummary? Exif => W412bExif;
    }

    /// <summary>Scripted decoder. Behaviour {kind:"ok", backend?, tag?} | {kind:"throw", exception, message?, hresult?, attach_bytes?}.</summary>
    private sealed class W412bDecoder(JsonObject behaviour) : IImageDecoder
    {
        public int DecodeCalls;
        public int ReadInfoCalls;
        public DecodeRequest? LastRequest;
        public string? LastInfoPath;
        public Exception? Thrown;
        public IDecodedImage? Returned;

        private void MaybeThrow(bool attachBytes)
        {
            if (behaviour["kind"]!.GetValue<string>() != "throw") return;
            var ex = W412bMake(behaviour["exception"]!.GetValue<string>(), behaviour["message"]?.GetValue<string>() ?? "scripted failure",
                behaviour["hresult"]?.GetValue<int>() ?? unchecked((int)0x88982F50));
            if (attachBytes && behaviour["attach_bytes"] is { } n)
                DecodeFailureSourceBytes.Attach(ex, new byte[n.GetValue<int>()]);
            Thrown = ex;
            throw ex;
        }

        public IDecodedImage Decode(DecodeRequest request)
        {
            DecodeCalls++;
            LastRequest = request;
            MaybeThrow(attachBytes: true);
            var backend = Enum.Parse<DecoderBackend>(behaviour["backend"]?.GetValue<string>() ?? "Wpf");
            return Returned = new W412bImage(backend, behaviour["tag"]?.GetValue<int>() ?? 0);
        }

        public ImageInfo ReadInfo(string path)
        {
            ReadInfoCalls++;
            LastInfoPath = path;
            MaybeThrow(attachBytes: false);
            return new ImageInfo(321 + (behaviour["tag"]?.GetValue<int>() ?? 0), 123, 3);
        }
    }

    private sealed class W412bLog : ILog
    {
        public readonly List<string> Warnings = [];
        public bool Enabled => true;
        public void Info(string message) { }
        public void Warn(string message) => Warnings.Add(message);
        public void Error(string message, Exception? ex = null) { }
    }

    /// <summary>Cases {name, exception, message?}: FallbackImageDecoder.IsFallbackable(exception) and the exception's CLR type name.</summary>
    public static JsonArray FallbackIsFallbackable(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["exception"]!.GetValue<string>();
            var ex = W412bMake(kind, c["message"]?.GetValue<string>() ?? "scripted failure", unchecked((int)0x88982F50));
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["exception"] = kind,
                ["type"] = ex.GetType().Name,
                ["fallbackable"] = FallbackImageDecoder.IsFallbackable(ex),
            });
        }
        return result;
    }

    /// <summary>
    /// Cases {name, op: "decode"|"read-info", primary_backend, primary: behaviour, fallback: behaviour,
    /// request: {path, target_width?, target_height?, apply_orientation?, bytes_len?, source_orientation?}}.
    /// Reports the outcome (ok/throw, backend, identity vs the primary's/fallback's own image or exception, preserved properties),
    /// the calls each decoder saw (and the request the fallback received), the warnings logged and the metrics fallback counts.
    /// </summary>
    public static JsonArray FallbackDecoderCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            var primaryBackend = Enum.Parse<DecoderBackend>(c["primary_backend"]!.GetValue<string>());
            var primary = new W412bDecoder(c["primary"]!.AsObject());
            var fallback = new W412bDecoder(c["fallback"]!.AsObject());
            var log = new W412bLog();
            var metrics = new ReviewMetrics();
            var decoder = new FallbackImageDecoder(primary, primaryBackend, fallback, log, metrics);

            var r = c["request"]!.AsObject();
            var path = r["path"]!.GetValue<string>();
            var request = new DecodeRequest(
                path,
                r["target_width"]?.GetValue<int>() ?? 0,
                r["apply_orientation"]?.GetValue<bool>() ?? true,
                r["bytes_len"] is { } n ? new byte[n.GetValue<int>()] : null,
                r["target_height"]?.GetValue<int>() ?? 0,
                SourceReadPriority.Viewer,
                r["source_orientation"]?.GetValue<int>());

            var outcome = new JsonObject();
            try
            {
                if (op == "decode")
                {
                    var decoded = decoder.Decode(request);
                    outcome["kind"] = "ok";
                    outcome["backend"] = decoded.ActualBackend.ToString();
                    outcome["same_as_primary_image"] = ReferenceEquals(decoded, primary.Returned);
                    outcome["same_as_fallback_image"] = ReferenceEquals(decoded, fallback.Returned);
                    outcome["pixel_width"] = decoded.PixelWidth;
                    outcome["pixel_height"] = decoded.PixelHeight;
                    outcome["downscaled"] = decoded.Downscaled;
                    outcome["orientation"] = decoded.Orientation;
                    outcome["estimated_bytes"] = decoded.EstimatedBytes;
                    outcome["original_width"] = decoded.OriginalWidth;
                    outcome["original_height"] = decoded.OriginalHeight;
                    outcome["is_degraded_fallback"] = decoded.IsDegradedFallback;
                    outcome["exif_is_same_object"] = ReferenceEquals(decoded.Exif, W412bExif);
                    outcome["platform_image_same"] =
                        ReferenceEquals(decoded.PlatformImage, (primary.Returned ?? fallback.Returned)!.PlatformImage);
                }
                else
                {
                    var info = decoder.ReadInfo(path);
                    outcome["kind"] = "ok";
                    outcome["info"] = new JsonArray(info.PixelWidth, info.PixelHeight, info.Orientation);
                }
            }
            catch (Exception ex)
            {
                outcome["kind"] = "throw";
                outcome["type"] = ex.GetType().Name;
                outcome["message"] = ex.Message;
                outcome["same_as_primary_exception"] = ReferenceEquals(ex, primary.Thrown);
                outcome["same_as_fallback_exception"] = ReferenceEquals(ex, fallback.Thrown);
            }

            var snapshot = metrics.Snapshot();
            var fallbacks = new JsonObject();
            foreach (var kv in snapshot.DecoderFallbacks.OrderBy(k => (int)k.Key))
                fallbacks[kv.Key.ToString()] = kv.Value;

            var fbRequest = fallback.LastRequest;
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["op"] = op,
                ["outcome"] = outcome,
                ["primary_decode_calls"] = primary.DecodeCalls,
                ["primary_read_info_calls"] = primary.ReadInfoCalls,
                ["fallback_decode_calls"] = fallback.DecodeCalls,
                ["fallback_read_info_calls"] = fallback.ReadInfoCalls,
                ["fallback_info_path"] = fallback.LastInfoPath,
                ["fallback_request"] = fbRequest is { } fr
                    ? new JsonObject
                    {
                        ["path"] = fr.Path,
                        ["target_width"] = fr.TargetWidth,
                        ["target_height"] = fr.TargetHeight,
                        ["apply_orientation"] = fr.ApplyOrientation,
                        ["bytes_len"] = fr.Bytes?.Length,
                        ["source_orientation"] = fr.SourceOrientation,
                    }
                    : null,
                ["warnings"] = new JsonArray(log.Warnings.Select(w => (JsonNode?)JsonValue.Create(w)).ToArray()),
                ["decoder_fallbacks"] = fallbacks,
                ["decoder_fallback_count"] = snapshot.DecoderFallbackCount,
            });
        }
        return result;
    }
}
