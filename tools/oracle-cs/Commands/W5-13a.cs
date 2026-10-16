using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.TurboJpeg.Native;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-13a
//     "turbojpeg-native-types" => Commands.TurboJpegNativeTypes(inputs),
//     "turbojpeg-native-handle" => Commands.TurboJpegNativeHandle(inputs),
//     "turbojpeg-native-decode" => Commands.TurboJpegNativeDecode(inputs, inputPath),
//
// Wiring (once for Wave 5, shared with W5-13b/W5-14b):
//   * tools/oracle.ps1 also builds src/PhotoReview.Imaging.TurboJpeg/PhotoReview.Imaging.TurboJpeg.csproj into $coreDir.
//     That csproj runs tools/fetch-native.ps1 of the ARCHIVED C# tree when native/x64/turbojpeg.dll is missing (curl from
//     nuget.org, SHA-256 pinned in native/turbojpeg.sha256); the archive has no DLL, so the first build downloads it.
//   * OracleCs.csproj: <Reference Include="PhotoReview.Imaging.TurboJpeg"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.TurboJpeg.dll</HintPath></Reference>
//     and copy $(CSharpCoreDir)\turbojpeg.dll next to oracle-cs.exe (a <None Include=... CopyToOutputDirectory> item, or a
//     Copy-Item in oracle.ps1). TurboJpegNative uses DllImportSearchPath.AssemblyDirectory: the DLL must sit beside
//     PhotoReview.Imaging.TurboJpeg.dll in the oracle output folder.
//   * turbojpeg-native-types and turbojpeg-native-handle never load turbojpeg.dll (fakes only); turbojpeg-native-decode does
//     and throws a clear error when it is missing.
// TurboJpegNative and the SafeTurboJpegHandle seam constructor are `internal` (InternalsVisibleTo names only the test
// assembly): reached by reflection; the tj3* methods with pointer parameters through delegates of the same signature.

// W5-13a: TurboJpegNative (enums, TjScalingFactor, WrapInitResult, tj3* call sequence) and SafeTurboJpegHandle.
static partial class Commands
{
    private const BindingFlags W513aStatic = BindingFlags.Static | BindingFlags.NonPublic | BindingFlags.Public;
    private const BindingFlags W513aInstance = BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public;

    private static readonly Type W513aNativeType =
        typeof(SafeTurboJpegHandle).Assembly.GetType("PhotoReview.Imaging.TurboJpeg.Native.TurboJpegNative", throwOnError: true)!;

    private static readonly Type W513aDecoderType =
        typeof(SafeTurboJpegHandle).Assembly.GetType("PhotoReview.Imaging.TurboJpeg.TurboJpegDecoder", throwOnError: true)!;

    private unsafe delegate int W513aHeaderFn(SafeTurboJpegHandle handle, byte* jpegBuf, nuint jpegSize);
    private delegate int W513aGetFn(SafeTurboJpegHandle handle, int param);
    private delegate int W513aSetFn(SafeTurboJpegHandle handle, int param, int value);
    private delegate int W513aScaleFn(SafeTurboJpegHandle handle, TjScalingFactor factor);
    private unsafe delegate int W513aDecompressFn(SafeTurboJpegHandle handle, byte* jpegBuf, nuint jpegSize, byte* dstBuf, int pitch, int pixelFormat);

    private static T W513aNativeDelegate<T>(string name) where T : Delegate =>
        (T)Delegate.CreateDelegate(typeof(T), W513aNativeType.GetMethod(name, W513aStatic) ?? throw new MissingMethodException(name));

    private static string W513aHex(byte[] bytes) => Convert.ToHexString(bytes).ToLowerInvariant();

    private static JsonObject W513aException(Exception ex)
    {
        if (ex is TargetInvocationException { InnerException: { } inner }) ex = inner;
        return new JsonObject { ["type"] = ex.GetType().Name, ["message"] = ex.Message };
    }

    private static TjScalingFactor W513aFactor(JsonNode node)
    {
        var a = node.AsArray();
        return new TjScalingFactor(a[0]!.GetValue<int>(), a[1]!.GetValue<int>());
    }

    /// <summary>libjpeg-turbo tjPixelSize[] (the Rust stub's `bytes_per_pixel`; C# has no twin, the decoder only uses Bgrx).</summary>
    private static int W513aPixelSize(TjPixelFormat format) => format switch
    {
        TjPixelFormat.Rgb or TjPixelFormat.Bgr => 3,
        TjPixelFormat.Gray => 1,
        _ => 4,
    };

    /// <summary>
    /// Cases {name, kind}:
    ///   "enum" {type: "TjInit"|"TjParam"|"TjPixelFormat"} -> members [{name, value}] in declaration order;
    ///   "dll-name" -> dll_name (TurboJpegNative.DllName);
    ///   "factor-equality" {a: [n, d], b: [n, d]} -> equals, op_equal, op_not_equal, equals_object, hash_equal;
    ///   "factor-to-string" {factor: [n, d]} -> text; also one: ToString of TjScalingFactor.One;
    ///   "output-buffer" {width, height, factor: [n, d]} -> TurboJpegDecoder.CalculateOutputBuffer (private, W5-14b owns it):
    ///      {width, height, stride, buffer_length} or exception {type, message}. The Rust `TjScalingFactor::scaled` must give the
    ///      same width/height (the message of the overflow exception carries "{w}x{h}" too).
    /// </summary>
    public static JsonArray TurboJpegNativeTypes(JsonObject inputs)
    {
        var result = new JsonArray();
        var calc = W513aDecoderType.GetMethod("CalculateOutputBuffer", W513aStatic)
            ?? throw new MissingMethodException("TurboJpegDecoder.CalculateOutputBuffer");
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind };
            switch (kind)
            {
                case "enum":
                {
                    var typeName = c["type"]!.GetValue<string>();
                    row["type"] = typeName;
                    var type = typeName switch
                    {
                        "TjInit" => typeof(TjInit),
                        "TjParam" => typeof(TjParam),
                        "TjPixelFormat" => typeof(TjPixelFormat),
                        _ => throw new ArgumentException("unknown enum " + typeName),
                    };
                    var members = new JsonArray();
                    foreach (var field in type.GetFields(BindingFlags.Public | BindingFlags.Static).OrderBy(f => f.MetadataToken))
                    {
                        members.Add(new JsonObject { ["name"] = field.Name, ["value"] = (int)field.GetValue(null)! });
                    }
                    row["members"] = members;
                    break;
                }
                case "dll-name":
                    row["dll_name"] = (string)W513aNativeType.GetField("DllName", W513aStatic)!.GetValue(null)!;
                    break;
                case "factor-equality":
                {
                    var a = W513aFactor(c["a"]!);
                    var b = W513aFactor(c["b"]!);
                    row["a"] = c["a"]!.DeepClone();
                    row["b"] = c["b"]!.DeepClone();
                    row["equals"] = a.Equals(b);
                    row["op_equal"] = a == b;
                    row["op_not_equal"] = a != b;
                    row["equals_object"] = a.Equals((object)b);
                    row["hash_equal"] = a.GetHashCode() == b.GetHashCode();
                    break;
                }
                case "factor-to-string":
                {
                    var f = c["factor"] is null ? TjScalingFactor.One : W513aFactor(c["factor"]!);
                    row["factor"] = new JsonArray(f.Num, f.Denom);
                    row["text"] = f.ToString();
                    break;
                }
                case "output-buffer":
                {
                    int w = c["width"]!.GetValue<int>(), h = c["height"]!.GetValue<int>();
                    var f = W513aFactor(c["factor"]!);
                    row["width_in"] = w;
                    row["height_in"] = h;
                    row["factor"] = new JsonArray(f.Num, f.Denom);
                    try
                    {
                        var t = ((int, int, int, int))calc.Invoke(null, [w, h, f])!;
                        row["width"] = t.Item1;
                        row["height"] = t.Item2;
                        row["stride"] = t.Item3;
                        row["buffer_length"] = t.Item4;
                    }
                    catch (TargetInvocationException tie)
                    {
                        row["exception"] = W513aException(tie);
                    }
                    break;
                }
                default:
                    throw new ArgumentException("unknown kind " + kind);
            }
            result.Add(row);
        }
        return result;
    }

    /// <summary>
    /// Cases {name, kind, raw (long), owns (bool)}:
    ///   "seam" {destroy: "record"|"throw", disposes: n} -> internal ctor (IntPtr, bool, Action&lt;IntPtr&gt;); reports is_invalid,
    ///      is_closed_before, then per Dispose call an entry of dispose_errors (null or {type, message}), destroyed (raw values
    ///      the seam saw, in order), is_closed_after, raw_after (DangerousGetHandle);
    ///   "public-ctor" (raw 0 or -1 only: a valid value would reach the real tj3Destroy) -> new SafeTurboJpegHandle(raw, owns);
    ///      is_invalid, dispose_error, is_closed_after;
    ///   "wrap" -> TurboJpegNative.WrapInitResult(raw): exception {type, message}, or is_invalid, is_closed, raw_value; the
    ///      handle is then SetHandleAsInvalid (the pointer is fake) and is_closed_after_set_invalid is reported;
    ///   "set-invalid" {destroy: "record"} -> seam handle, SetHandleAsInvalid, Dispose: destroyed, is_closed_after.
    /// None of these loads turbojpeg.dll.
    /// </summary>
    public static JsonArray TurboJpegNativeHandle(JsonObject inputs)
    {
        var result = new JsonArray();
        var seamCtor = typeof(SafeTurboJpegHandle).GetConstructor(W513aInstance, [typeof(IntPtr), typeof(bool), typeof(Action<IntPtr>)])
            ?? throw new MissingMethodException("SafeTurboJpegHandle(IntPtr, bool, Action<IntPtr>)");
        var wrap = W513aNativeType.GetMethod("WrapInitResult", W513aStatic) ?? throw new MissingMethodException("WrapInitResult");
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var raw = new IntPtr(c["raw"]!.GetValue<long>());
            var owns = c["owns"]?.GetValue<bool>() ?? true;
            var row = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind, ["raw"] = (long)raw, ["owns"] = owns,
            };
            switch (kind)
            {
                case "seam":
                case "set-invalid":
                {
                    var destroyed = new List<long>();
                    var mode = c["destroy"]!.GetValue<string>();
                    row["destroy"] = mode;
                    Action<IntPtr> destroy = mode switch
                    {
                        "record" => p => destroyed.Add((long)p),
                        "throw" => p => { destroyed.Add((long)p); throw new InvalidOperationException("destroy failed"); },
                        _ => throw new ArgumentException("unknown destroy " + mode),
                    };
                    var handle = (SafeTurboJpegHandle)seamCtor.Invoke([raw, owns, destroy]);
                    row["is_invalid"] = handle.IsInvalid;
                    row["is_closed_before"] = handle.IsClosed;
                    if (kind == "set-invalid")
                    {
                        handle.SetHandleAsInvalid();
                        row["is_closed_after_set_invalid"] = handle.IsClosed;
                    }
                    var disposes = c["disposes"]?.GetValue<int>() ?? 1;
                    var errors = new JsonArray();
                    for (var i = 0; i < disposes; i++)
                    {
                        try
                        {
                            handle.Dispose();
                            errors.Add(null);
                        }
                        catch (Exception ex)
                        {
                            errors.Add(W513aException(ex));
                        }
                    }
                    row["dispose_errors"] = errors;
                    row["destroyed"] = new JsonArray(destroyed.Select(v => (JsonNode)v).ToArray());
                    row["is_closed_after"] = handle.IsClosed;
                    row["raw_after"] = (long)handle.DangerousGetHandle();
                    break;
                }
                case "public-ctor":
                {
                    if (raw != IntPtr.Zero && raw != new IntPtr(-1)) throw new ArgumentException("public-ctor only for 0/-1 (no real tj3Destroy)");
                    var handle = new SafeTurboJpegHandle(raw, owns);
                    row["is_invalid"] = handle.IsInvalid;
                    try
                    {
                        handle.Dispose();
                        row["dispose_error"] = null;
                    }
                    catch (Exception ex)
                    {
                        row["dispose_error"] = W513aException(ex);
                    }
                    row["is_closed_after"] = handle.IsClosed;
                    break;
                }
                case "wrap":
                {
                    try
                    {
                        var handle = (SafeTurboJpegHandle)wrap.Invoke(null, [raw])!;
                        row["is_invalid"] = handle.IsInvalid;
                        row["is_closed"] = handle.IsClosed;
                        row["raw_value"] = (long)handle.DangerousGetHandle();
                        handle.SetHandleAsInvalid(); // fake pointer: never reach the real tj3Destroy
                        row["is_closed_after_set_invalid"] = handle.IsClosed;
                    }
                    catch (TargetInvocationException tie)
                    {
                        row["exception"] = W513aException(tie);
                    }
                    break;
                }
                default:
                    throw new ArgumentException("unknown kind " + kind);
            }
            result.Add(row);
        }
        return result;
    }

    /// <summary>
    /// Native (loads turbojpeg.dll beside the oracle). Cases {name, fixture (relative to inputs.fixtures_dir), factor?: [n, d],
    /// format?: TjPixelFormat name (default "Bgrx"), strict?: bool (default true)}. The call sequence is the one of
    /// TurboJpegDecoder.Decode on ONE fresh decompressor: CreateDecompressor; when strict, tj3Set(ScanLimit, 500) and
    /// tj3Set(StopOnWarning, 1) (results reported); tj3DecompressHeader (header_result; on failure header_error and stop);
    /// tj3Get of Subsamp, JpegWidth, JpegHeight, Precision, Colorspace, Progressive, Arithmetic, Lossless (params);
    /// when factor is given tj3SetScalingFactor (scale_result; the effective factor is the given one on 0, else 1/1 as the
    /// decoder does); scaled_width/height = TJSCALED; pitch = scaled_width * pixel size; a destination of pitch * scaled_height
    /// bytes pre-filled with 0xCD (skipped above 64 MiB: decompress_skipped = true); tj3Decompress8 (decompress_result; on
    /// failure decompress_error; on success sha256 of the whole destination). Every row carries turbojpeg_dll_sha256.
    /// Error strings are TurboJpegNative.GetErrorMessage (null when the native pointer is null).
    /// </summary>
    public static unsafe JsonArray TurboJpegNativeDecode(JsonObject inputs, string inputPath)
    {
        var dllPath = Path.Combine(Path.GetDirectoryName(W513aNativeType.Assembly.Location)!, "turbojpeg.dll");
        if (!File.Exists(dllPath)) throw new FileNotFoundException("turbojpeg-native-decode needs turbojpeg.dll next to PhotoReview.Imaging.TurboJpeg.dll", dllPath);
        var dllSha = W513aHex(SHA256.HashData(File.ReadAllBytes(dllPath)));
        var fixturesDir = Path.GetFullPath(Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!, inputs["fixtures_dir"]!.GetValue<string>()));

        var create = W513aNativeType.GetMethod("CreateDecompressor", W513aStatic) ?? throw new MissingMethodException("CreateDecompressor");
        var errorOf = W513aNativeType.GetMethod("GetErrorMessage", W513aStatic) ?? throw new MissingMethodException("GetErrorMessage");
        var header = W513aNativeDelegate<W513aHeaderFn>("tj3DecompressHeader");
        var get = W513aNativeDelegate<W513aGetFn>("tj3Get");
        var set = W513aNativeDelegate<W513aSetFn>("tj3Set");
        var scale = W513aNativeDelegate<W513aScaleFn>("tj3SetScalingFactor");
        var decompress = W513aNativeDelegate<W513aDecompressFn>("tj3Decompress8");
        string? Error(SafeTurboJpegHandle h) => (string?)errorOf.Invoke(null, [h]);

        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var fixture = c["fixture"]!.GetValue<string>();
            var format = Enum.Parse<TjPixelFormat>(c["format"]?.GetValue<string>() ?? "Bgrx");
            var strict = c["strict"]?.GetValue<bool>() ?? true;
            var row = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(), ["fixture"] = fixture, ["format"] = format.ToString(),
                ["format_value"] = (int)format, ["strict"] = strict, ["turbojpeg_dll_sha256"] = dllSha,
            };
            var bytes = File.ReadAllBytes(Path.Combine(fixturesDir, fixture));
            row["fixture_length"] = bytes.Length;
            using var h = (SafeTurboJpegHandle)create.Invoke(null, null)!;
            if (strict)
            {
                row["scan_limit_result"] = set(h, (int)TjParam.ScanLimit, 500);
                row["stop_on_warning_result"] = set(h, (int)TjParam.StopOnWarning, 1);
            }
            fixed (byte* p = bytes) // an empty array gives a null pointer here; libjpeg-turbo rejects size 0 either way
            {
                var headerResult = header(h, p, (nuint)bytes.Length);
                row["header_result"] = headerResult;
                if (headerResult != 0)
                {
                    row["header_error"] = Error(h);
                    result.Add(row);
                    continue;
                }
                var parameters = new JsonObject();
                foreach (var param in new[] { TjParam.Subsamp, TjParam.JpegWidth, TjParam.JpegHeight, TjParam.Precision, TjParam.Colorspace, TjParam.Progressive, TjParam.Arithmetic, TjParam.Lossless })
                {
                    parameters[param.ToString()] = get(h, (int)param);
                }
                row["params"] = parameters;
                int width = parameters["JpegWidth"]!.GetValue<int>(), height = parameters["JpegHeight"]!.GetValue<int>();

                var effective = TjScalingFactor.One;
                if (c["factor"] is { } factorNode)
                {
                    var factor = W513aFactor(factorNode);
                    row["factor"] = new JsonArray(factor.Num, factor.Denom);
                    var scaleResult = scale(h, factor);
                    row["scale_result"] = scaleResult;
                    if (scaleResult != 0) row["scale_error"] = Error(h);
                    else effective = factor;
                }
                row["effective_factor"] = new JsonArray(effective.Num, effective.Denom);
                long scaledW = ((long)width * effective.Num + effective.Denom - 1) / effective.Denom;
                long scaledH = ((long)height * effective.Num + effective.Denom - 1) / effective.Denom;
                row["scaled_width"] = scaledW;
                row["scaled_height"] = scaledH;
                long pitch = scaledW * W513aPixelSize(format);
                long length = pitch * scaledH;
                row["pitch"] = pitch;
                if (width <= 0 || height <= 0 || length <= 0 || length > 64L * 1024 * 1024 || pitch > int.MaxValue)
                {
                    row["decompress_skipped"] = true;
                    result.Add(row);
                    continue;
                }
                var dst = new byte[length];
                Array.Fill(dst, (byte)0xCD);
                fixed (byte* d = dst)
                {
                    var decompressResult = decompress(h, p, (nuint)bytes.Length, d, (int)pitch, (int)format);
                    row["decompress_result"] = decompressResult;
                    if (decompressResult != 0) row["decompress_error"] = Error(h);
                    else row["pixels_sha256"] = W513aHex(SHA256.HashData(dst));
                }
            }
            result.Add(row);
        }
        return result;
    }
}
