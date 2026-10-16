using System.IO;
using System.Linq.Expressions;
using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.LibRaw;
using PhotoReview.Imaging.Pixels;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-16c
//     "libraw-decoder" => Commands.LibRawDecoderCases(inputs),
//     "libraw-decoder-native" => Commands.LibRawDecoderNativeCases(inputs),
//
// Wiring (NOT done by the contract writer; the oracle was neither compiled nor run):
//   * OracleCs.csproj: <Reference Include="PhotoReview.Imaging.LibRaw"> (HintPath $(CSharpCoreDir)\PhotoReview.Imaging.LibRaw.dll), the same
//     reference W5-15b adds. LibRawDecoder is public; every helper this file calls is internal (InternalsVisibleTo names only
//     PhotoReview.Imaging.Tests), so they are reached by reflection (D38 precedent).
//   * libraw-decoder needs NO libraw.dll: none of its ops reaches a native call (the decode body is replaced by CoreOverride, guards throw
//     before CreateHandle). libraw-decoder-native needs the pinned libraw.dll (0.22.2) next to PhotoReview.Imaging.LibRaw.dll (the C# build
//     copies native\x64\libraw.dll, fetched by tools/fetch-libraw.ps1 of the C# repo). Without it every native case is written as
//     { "native_unavailable": "<probe reason>" } and the Rust test skips it.
//   * The corpus op reads PHOTOREVIEW_RAW_CORPUS_DIR (or "corpus_dir" of the input); no directory => zero corpus cases and a note on stderr.
//   * Determinism (D19): MemoryInfo is fixed to (64 GiB, 0); temp files live in a fresh folder whose path never reaches the output;
//     corpus entries carry the file NAME only. Run twice and compare byte for byte.

// W5-16c: LibRawDecoder. Case = { name, op, ... }; the op list is in the contract (golden package).
static partial class Commands
{
    private static readonly Type W516cDecoderType = typeof(LibRawDecoder);
    private static readonly Assembly W516cAssembly = typeof(LibRawDecoder).Assembly;
    private const BindingFlags W516cStatic = BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.Static;
    private const BindingFlags W516cInstance = BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.Instance;

    private static MethodInfo W516cMethod(string name, params Type[] types) =>
        W516cDecoderType.GetMethod(name, W516cStatic, types) ?? throw new MissingMethodException(W516cDecoderType.FullName, name);

    private static Type W516cHeaderType => W516cDecoderType.GetNestedType("ProcessedImageHeader", BindingFlags.NonPublic)!;

    private static int W516cSlots() => (int)W516cDecoderType.GetProperty("FullDecodeSlotsAvailable", W516cStatic)!.GetValue(null)!;

    private static int W516cQueued() => (int)W516cDecoderType.GetProperty("FullDecodeQueuedWaiters", W516cStatic)!.GetValue(null)!;

    private static string W516cHex(ReadOnlySpan<byte> b) => Convert.ToHexString(b).ToLowerInvariant();

    private static Exception W516cUnwrap(Exception ex) => ex is TargetInvocationException { InnerException: { } inner } ? W516cUnwrap(inner) : ex;

    private static JsonObject W516cThrows(Exception ex)
    {
        var e = W516cUnwrap(ex);
        return new JsonObject { ["type"] = e.GetType().Name, ["message"] = e.Message };
    }

    /// <summary>Boxed ProcessedImageHeader from the case fields (type, data_size, width, height, colors, bits).</summary>
    private static object W516cHeader(JsonObject c)
    {
        var header = Activator.CreateInstance(W516cHeaderType)!;
        void Set(string field, object value) => W516cHeaderType.GetField(field, BindingFlags.NonPublic | BindingFlags.Instance)!.SetValue(header, value);
        Set("Type", c["type"]?.GetValue<int>() ?? 2);
        Set("DataSize", c["data_size"]?.GetValue<uint>() ?? 100u);
        Set("Width", c["width"]?.GetValue<ushort>() ?? (ushort)10);
        Set("Height", c["height"]?.GetValue<ushort>() ?? (ushort)10);
        Set("Colors", c["colors"]?.GetValue<ushort>() ?? (ushort)3);
        Set("Bits", c["bits"]?.GetValue<ushort>() ?? (ushort)8);
        return header;
    }

    /// <summary>
    /// Bytes of a trim case: "hex" (whole stream), or "fill" { byte_hex, length } plus "patches" [{ at, hex }], plus an optional "append_hex".
    /// </summary>
    private static byte[] W516cBytes(JsonObject c)
    {
        byte[] bytes;
        if (c["hex"] is { } hex) bytes = Convert.FromHexString(hex.GetValue<string>());
        else
        {
            var fill = c["fill"]!.AsObject();
            bytes = new byte[fill["length"]!.GetValue<int>()];
            Array.Fill(bytes, Convert.FromHexString(fill["byte_hex"]!.GetValue<string>())[0]);
        }
        foreach (var p in c["patches"]?.AsArray() ?? [])
        {
            var patch = Convert.FromHexString(p!["hex"]!.GetValue<string>());
            patch.CopyTo(bytes, p["at"]!.GetValue<int>());
        }
        if (c["append_hex"] is { } tail) bytes = [.. bytes, .. Convert.FromHexString(tail.GetValue<string>())];
        return bytes;
    }

    /// <summary>xorshift32 junk (contract semantics 9): x ^= x << 13; x ^= x >> 17; x ^= x << 5; byte = x & 0xFF. Seed 0 is replaced by 1.</summary>
    private static byte[] W516cJunk(uint seed, int length)
    {
        var x = seed == 0 ? 1u : seed;
        var bytes = new byte[length];
        for (var i = 0; i < length; i++)
        {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            bytes[i] = (byte)(x & 0xFF);
        }
        return bytes;
    }

    private static LibRawDecoder W516cDecoder(Action<string>? observer = null)
    {
        var decoder = new LibRawDecoder(PixelBufferImageCodec.Instance, observer);
        Func<(long TotalAvailable, long Load)> memory = () => (64L << 30, 0L);
        W516cDecoderType.GetProperty("MemoryInfo", W516cInstance)!.SetValue(decoder, memory);
        return decoder;
    }

    /// <summary>
    /// Sets CoreOverride (type Func&lt;DecodeRequest, FullDecodeGate.Lease, CancellationToken, IDecodedImage&gt;, Lease is internal) to a body
    /// that records the slot count and throws InvalidDataException("stop here"). Built with an expression tree because the delegate type
    /// cannot be named here.
    /// </summary>
    private static void W516cSetThrowingCore(LibRawDecoder decoder, Action<int> onBody)
    {
        var property = W516cDecoderType.GetProperty("CoreOverride", W516cInstance)!;
        var parameters = property.PropertyType.GetGenericArguments()[..3].Select(Expression.Parameter).ToArray();
        Func<IDecodedImage> body = () =>
        {
            onBody(W516cSlots());
            throw new InvalidDataException("stop here");
        };
        var call = Expression.Invoke(Expression.Constant(body));
        property.SetValue(decoder, Expression.Lambda(property.PropertyType, call, parameters).Compile());
    }

    // ================================================================================================ libraw-decoder (no native)

    public static JsonArray LibRawDecoderCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            var o = (JsonObject)c.DeepClone();
            try
            {
                switch (op)
                {
                    case "constants":
                        o["max_thumbnail_bytes"] = (int)W516cDecoderType.GetField("MaxThumbnailBytes", W516cStatic)!.GetValue(null)!;
                        o["max_queued_preload_decodes"] = (int)W516cDecoderType.GetField("MaxQueuedPreloadDecodes", W516cStatic)!.GetValue(null)!;
                        o["long_path_threshold"] = (int)W516cDecoderType.GetField("LongPathThreshold", W516cStatic)!.GetValue(null)!;
                        o["trailing_eoi_search_window"] = (int)W516cDecoderType.GetField("TrailingEoiSearchWindow", W516cStatic)!.GetValue(null)!;
                        o["header_data_offset"] = (int)W516cHeaderType.GetField("DataOffset", W516cStatic)!.GetValue(null)!;
                        o["header_size"] = System.Runtime.InteropServices.Marshal.SizeOf(W516cHeaderType);
                        break;
                    case "trim":
                    {
                        var bytes = W516cBytes(c);
                        o.Remove("hex");
                        o["input_length"] = bytes.Length;
                        o["input_sha256"] = W516cHex(SHA256.HashData(bytes));
                        try
                        {
                            var trimmed = (byte[])W516cMethod("TrimToEndOfImage", typeof(byte[])).Invoke(null, [bytes])!;
                            o["ok"] = new JsonObject { ["length"] = trimmed.Length, ["same"] = ReferenceEquals(trimmed, bytes) };
                        }
                        catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                        break;
                    }
                    case "extended-path":
                        o["value"] = (string?)W516cMethod("ToExtendedLengthPath", typeof(string)).Invoke(null, [c["path"]!.GetValue<string>()]);
                        break;
                    case "thumbnail-header":
                        try
                        {
                            W516cMethod("ValidateThumbnailHeader", W516cHeaderType.MakeByRefType()).Invoke(null, [W516cHeader(c)]);
                            o["ok"] = true;
                        }
                        catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                        break;
                    case "image-dimensions":
                        try
                        {
                            W516cMethod("ValidateImageDimensions", typeof(int), typeof(int)).Invoke(null, [c["w"]!.GetValue<int>(), c["h"]!.GetValue<int>()]);
                            o["ok"] = true;
                        }
                        catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                        break;
                    case "supported-bitmap":
                        o["value"] = (bool)W516cMethod("IsSupportedBitmap", W516cHeaderType.MakeByRefType()).Invoke(null, [W516cHeader(c)])!;
                        break;
                    case "downscaled":
                        o["value"] = (bool)W516cMethod("IsDownscaled", typeof(int), typeof(int), typeof(int), typeof(int))
                            .Invoke(null, [c["tw"]!.GetValue<int>(), c["th"]!.GetValue<int>(), c["sw"]!.GetValue<int>(), c["sh"]!.GetValue<int>()])!;
                        break;
                    case "failure-memory":
                    {
                        // Only the two out-of-memory codes: they never call libraw_strerror.
                        var code = c["code"]!.GetValue<int>();
                        var ex = c["thumbnail"]?.GetValue<bool>() == true
                            ? (Exception)W516cMethod("CreateThumbnailFailure", typeof(int)).Invoke(null, [code])!
                            : (Exception)W516cMethod("CreateFailure", typeof(int), typeof(string)).Invoke(null, [code, c["operation"]!.GetValue<string>()])!;
                        o["value"] = W516cThrows(ex);
                        break;
                    }
                    case "check-result-canceled":
                    {
                        using var cts = new CancellationTokenSource();
                        cts.Cancel();
                        try
                        {
                            W516cMethod("CheckResult", typeof(int), typeof(string), typeof(CancellationToken))
                                .Invoke(null, [c["code"]!.GetValue<int>(), "op", cts.Token]);
                            o["ok"] = true;
                        }
                        catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                        break;
                    }
                    case "decode-guard":
                    {
                        // Decode with the body replaced: blank path / ApplyOrientation=false / cancelled token are refused before the gate.
                        var bodyRan = false;
                        int? slotsInBody = null;
                        var decoder = W516cDecoder();
                        W516cSetThrowingCore(decoder, slots => { bodyRan = true; slotsInBody = slots; });
                        using var cts = new CancellationTokenSource();
                        if (c["canceled"]?.GetValue<bool>() == true) cts.Cancel();
                        var priority = Enum.Parse<SourceReadPriority>(c["priority"]?.GetValue<string>() ?? "Viewer");
                        var request = new DecodeRequest(c["path"]!.GetValue<string>(), new DecodeBox(c["box_w"]?.GetValue<int>() ?? 100, c["box_h"]?.GetValue<int>() ?? 0),
                            applyOrientation: c["apply_orientation"]?.GetValue<bool>() ?? true, priority: priority);
                        o["slots_before"] = W516cSlots();
                        try
                        {
                            decoder.Decode(request, cts.Token);
                            o["ok"] = true;
                        }
                        catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                        o["body_ran"] = bodyRan;
                        o["slots_in_body"] = slotsInBody;
                        o["slots_after"] = W516cSlots();
                        o["queued_after"] = W516cQueued();
                        break;
                    }
                    case "read-info-guard":
                        try
                        {
                            W516cDecoder().ReadInfo(c["path"]!.GetValue<string>());
                            o["ok"] = true;
                        }
                        catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                        break;
                    case "thumbnail-guard":
                    {
                        using var cts = new CancellationTokenSource();
                        if (c["canceled"]?.GetValue<bool>() == true) cts.Cancel();
                        try
                        {
                            LibRawDecoder.ReadJpegThumbnail(c["path"]!.GetValue<string>(), cts.Token);
                            o["ok"] = true;
                        }
                        catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                        o["slots_after"] = W516cSlots();
                        break;
                    }
                    default:
                        throw new ArgumentException("unknown op");
                }
            }
            catch (Exception ex) when (W516cUnwrap(ex) is not ArgumentException { Message: "unknown op" })
            {
                // A reflection failure is a wiring bug: it is written to the golden (never silently dropped) so the Rust test fails loudly.
                o["oracle_error"] = W516cThrows(ex);
            }
            result.Add(o);
        }
        return result;
    }

    // ================================================================================================ libraw-decoder-native

    public static JsonArray LibRawDecoderNativeCases(JsonObject inputs)
    {
        var result = new JsonArray();
        var available = LibRawAvailability.Probe(out var reason);
        var nativeMethods = W516cAssembly.GetType("PhotoReview.Imaging.LibRaw.LibRawNativeMethods", throwOnError: true)!;
        string FormatError(int code) => (string)nativeMethods.GetMethod("FormatError", W516cStatic)!.Invoke(null, [code])!;

        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            if (op == "corpus")
            {
                foreach (var corpusCase in W516cCorpus(c, available, reason)) result.Add(corpusCase);
                continue;
            }

            var o = (JsonObject)c.DeepClone();
            if (!available)
            {
                o["native_unavailable"] = reason;
                result.Add(o);
                continue;
            }

            switch (op)
            {
                case "version":
                    o["version"] = (string?)nativeMethods.GetMethod("GetVersionString", W516cStatic)!.Invoke(null, null);
                    break;
                case "format-error":
                    o["messages"] = new JsonArray(c["codes"]!.AsArray().Select(code => (JsonNode?)new JsonObject
                    {
                        ["code"] = code!.GetValue<int>(),
                        ["message"] = FormatError(code.GetValue<int>()),
                    }).ToArray());
                    break;
                case "failure":
                {
                    var code = c["code"]!.GetValue<int>();
                    var ex = c["thumbnail"]?.GetValue<bool>() == true
                        ? (Exception)W516cMethod("CreateThumbnailFailure", typeof(int)).Invoke(null, [code])!
                        : (Exception)W516cMethod("CreateFailure", typeof(int), typeof(string)).Invoke(null, [code, c["operation"]!.GetValue<string>()])!;
                    o["value"] = W516cThrows(ex);
                    break;
                }
                case "check-result":
                    try
                    {
                        W516cMethod("CheckResult", typeof(int), typeof(string), typeof(CancellationToken))
                            .Invoke(null, [c["code"]!.GetValue<int>(), c["operation"]!.GetValue<string>(), CancellationToken.None]);
                        o["ok"] = true;
                    }
                    catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                    break;
                case "open-retry":
                {
                    // The scripted open returns codes[i] for the i-th call (0 after the list ends).
                    var codes = c["codes"]!.AsArray().Select(x => x!.GetValue<int>()).ToArray();
                    var calls = new List<string>();
                    Func<string, int> open = p => { calls.Add(p); return calls.Count <= codes.Length ? codes[calls.Count - 1] : 0; };
                    try
                    {
                        W516cMethod("OpenFile", typeof(Func<string, int>), typeof(string), typeof(CancellationToken))
                            .Invoke(null, [open, c["path"]!.GetValue<string>(), CancellationToken.None]);
                        o["ok"] = true;
                    }
                    catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                    o["calls"] = new JsonArray(calls.Select(p => (JsonNode?)p).ToArray());
                    break;
                }
                case "junk":
                {
                    // A file LibRaw rejects: the error, and whether the file can be deleted right after (no leaked handle).
                    var dir = Path.Combine(Path.GetTempPath(), "pr-oracle-w516c-" + Guid.NewGuid().ToString("N"));
                    Directory.CreateDirectory(dir);
                    try
                    {
                        var path = Path.Combine(dir, c["file_name"]!.GetValue<string>());
                        File.WriteAllBytes(path, W516cJunk(c["seed"]!.GetValue<uint>(), c["length"]!.GetValue<int>()));
                        try
                        {
                            switch (c["call"]!.GetValue<string>())
                            {
                                case "read-info": W516cDecoder().ReadInfo(path); break;
                                case "thumbnail": LibRawDecoder.ReadJpegThumbnail(path); break;
                                case "decode": W516cDecoder().Decode(new DecodeRequest(path, new DecodeBox(320, 240))); break;
                                default: throw new ArgumentException("unknown call");
                            }
                            o["ok"] = true;
                        }
                        catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                        File.Delete(path);
                        o["deleted"] = !File.Exists(path);
                        o["slots_after"] = W516cSlots();
                    }
                    finally { Directory.Delete(dir, recursive: true); }
                    break;
                }
                case "missing-file":
                {
                    // Open of a path that does not exist: error text and the stages reached ("configured" only, open order).
                    var stages = new List<string>();
                    try
                    {
                        switch (c["call"]!.GetValue<string>())
                        {
                            case "read-info": W516cDecoder(stages.Add).ReadInfo(c["path"]!.GetValue<string>()); break;
                            case "thumbnail": LibRawDecoder.ReadJpegThumbnail(c["path"]!.GetValue<string>()); break;
                            default: W516cDecoder(stages.Add).Decode(new DecodeRequest(c["path"]!.GetValue<string>(), DecodeBox.Unbounded)); break;
                        }
                        o["ok"] = true;
                    }
                    catch (Exception ex) { o["throws"] = W516cThrows(ex); }
                    o["stages"] = new JsonArray(stages.Select(s => (JsonNode?)s).ToArray());
                    break;
                }
                default:
                    throw new ArgumentException("unknown op " + op);
            }
            result.Add(o);
        }
        return result;
    }

    /// <summary>
    /// Every RAW file of the corpus (sorted OrdinalIgnoreCase): ReadInfo, ReadJpegThumbnail and one Decode per box of "boxes" (pixel hash of
    /// the PixelBuffer rows), plus a from-bytes decode for the names in "from_bytes" (must hash equal to the path decode) and the stage list
    /// of the first box. Zero cases when the corpus directory is missing; one { native_unavailable } case when libraw.dll is missing.
    /// </summary>
    private static IEnumerable<JsonObject> W516cCorpus(JsonObject c, bool available, string? reason)
    {
        var envName = c["corpus_dir_env"]?.GetValue<string>() ?? "PHOTOREVIEW_RAW_CORPUS_DIR";
        var dir = Environment.GetEnvironmentVariable(envName);
        if (string.IsNullOrWhiteSpace(dir)) dir = c["corpus_dir"]?.GetValue<string>();
        if (string.IsNullOrWhiteSpace(dir) || !Directory.Exists(dir))
        {
            Console.Error.WriteLine($"libraw-decoder-native: no corpus directory ({envName} unset or missing); writing zero corpus cases.");
            yield break;
        }
        if (!available)
        {
            yield return new JsonObject { ["name"] = "corpus", ["op"] = "corpus", ["native_unavailable"] = reason };
            yield break;
        }

        var extensions = c["extensions"]!.AsArray().Select(e => e!.GetValue<string>()).ToHashSet(StringComparer.OrdinalIgnoreCase);
        var boxes = c["boxes"]!.AsArray().Select(b => (W: b![0]!.GetValue<int>(), H: b[1]!.GetValue<int>())).ToArray();
        var fromBytes = c["from_bytes"]?.AsArray().Select(n => n!.GetValue<string>()).ToHashSet(StringComparer.OrdinalIgnoreCase) ?? [];
        var files = Directory.GetFiles(dir).Where(p => extensions.Contains(Path.GetExtension(p))).OrderBy(p => p, StringComparer.OrdinalIgnoreCase);
        foreach (var path in files)
        {
            var name = Path.GetFileName(path);
            var o = new JsonObject { ["name"] = "corpus-" + name, ["op"] = "corpus-file", ["file_name"] = name };
            try
            {
                var info = W516cDecoder().ReadInfo(path);
                o["read_info"] = new JsonObject { ["pixel_width"] = info.PixelWidth, ["pixel_height"] = info.PixelHeight, ["orientation"] = info.Orientation };
            }
            catch (Exception ex) { o["read_info"] = new JsonObject { ["throws"] = W516cThrows(ex) }; }

            try
            {
                var jpeg = LibRawDecoder.ReadJpegThumbnail(path);
                o["thumbnail"] = new JsonObject
                {
                    ["length"] = jpeg.Length,
                    ["sha256"] = W516cHex(SHA256.HashData(jpeg)),
                    ["head_hex"] = W516cHex(jpeg.AsSpan(0, Math.Min(4, jpeg.Length))),
                    ["tail_hex"] = W516cHex(jpeg.AsSpan(Math.Max(0, jpeg.Length - 2))),
                };
            }
            catch (Exception ex) { o["thumbnail"] = new JsonObject { ["throws"] = W516cThrows(ex) }; }

            var decodes = new JsonArray();
            for (var i = 0; i < boxes.Length; i++)
            {
                var stages = new List<string>();
                var decoded = W516cDecodeRow(path, boxes[i], null, i == 0 ? stages : null);
                if (i == 0) decoded["stages"] = new JsonArray(stages.Select(s => (JsonNode?)s).ToArray());
                decodes.Add(decoded);
                if (i == 0 && fromBytes.Contains(name))
                {
                    var bytesRow = W516cDecodeRow(path, boxes[i], File.ReadAllBytes(path), null);
                    bytesRow["from_bytes"] = true;
                    decodes.Add(bytesRow);
                }
            }
            o["decodes"] = decodes;
            o["slots_after"] = W516cSlots();
            yield return o;
        }
    }

    private static JsonObject W516cDecodeRow(string path, (int W, int H) box, byte[]? bytes, List<string>? stages)
    {
        var row = new JsonObject { ["box_w"] = box.W, ["box_h"] = box.H };
        try
        {
            var decoder = W516cDecoder(stages is null ? null : stages.Add);
            var request = new DecodeRequest(path, new DecodeBox(box.W, box.H), bytes: bytes is null ? null : new ReadOnlyMemory<byte>(bytes));
            var decoded = decoder.Decode(request);
            var px = (PixelBuffer)decoded.PlatformImage;
            using var sha = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            for (var y = 0; y < px.Height; y++) sha.AppendData(px.GetRow(y)[..(px.Width * 4)]);
            row["ok"] = new JsonObject
            {
                ["pixel_width"] = decoded.PixelWidth,
                ["pixel_height"] = decoded.PixelHeight,
                ["downscaled"] = decoded.Downscaled,
                ["orientation"] = decoded.Orientation,
                ["original_width"] = decoded.OriginalWidth,
                ["original_height"] = decoded.OriginalHeight,
                ["estimated_bytes"] = decoded.EstimatedBytes,
                ["backend"] = decoded.ActualBackend.ToString(),
                ["layout"] = px.Layout.ToString(),
                ["stride"] = px.Stride,
                ["pixels_sha256"] = W516cHex(sha.GetHashAndReset()),
            };
            px.Dispose();
        }
        catch (Exception ex) { row["throws"] = W516cThrows(ex); }
        return row;
    }
}
