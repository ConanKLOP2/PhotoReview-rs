using System.Reflection;
using System.Reflection.Emit;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json.Nodes;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-17
//     "rgb-bgra-resample" => Commands.RgbBgraResample(inputs),
//     "rgb-bgra-validate" => Commands.RgbBgraValidate(inputs),
// Needs <Reference Include="PhotoReview.Imaging.LibRaw"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.LibRaw.dll</HintPath></Reference> in
// OracleCs.csproj (add it unless another W5 unit already did; tools/oracle.ps1 must copy that DLL into CSharpCoreDir). libraw.dll is NOT
// needed: RgbBgraResampler is managed code. Self-contained: no other oracle file.
//
// RgbBgraResampler is `internal` and Resize / ResizeToBuffer take ReadOnlySpan<byte> / Span<byte> (ref structs cannot go through
// MethodInfo.Invoke), so two DynamicMethod shims (skipVisibility) convert byte[] -> span with the implicit operators and call the real
// members. Nothing of the C# is re-implemented here.
//
// "rgb-bgra-resample" case = { name, sw, sh, tw, th, channels, source: <spec>, cancelled? }.
//   source spec (the Rust test builds the same bytes; length sw*sh*channels):
//     { kind: "mod", mul, add }        byte[i] = (byte)(((long)i * mul + add) % 251)
//     { kind: "xorshift", seed }       uint s = seed; per byte: s ^= s << 13; s ^= s >> 17; s ^= s << 5; byte = (byte)(s >> 8)
//     { kind: "solid", value }         every byte = value
//     { kind: "solid_rgb", rgb: [r,g,b] }  channels must be 3: pixel = r, g, b
//     { kind: "checker" }              pixel (x + y) % 2 == 0 -> every channel 255, else 0
//     { kind: "gradient" }             pixel (x, y), channel c (0 for gray): (x * 7 + y * 13 + c * 50) % 256
//     { kind: "explicit", bytes: [..] }
//   Result = { name, sw, sh, tw, th, channels, resize_error, buffer_error, length, fnv1a64, head_hex (first 32 bytes), all_hex (output <= 96 bytes),
//              buffer_equal (ResizeToBuffer pixels == Resize pixels), buffer_aligned (Pointer % 64 == 0), buffer_length }
//   errors are the .NET exception type name (null = ok). A cancelled case runs both with an already-cancelled token.
// "rgb-bgra-validate" case = { name, op, ... }, op:
//     "source"   { width, height, data_size, channels }       -> ValidateSourceLength
//     "target"   { width, height }                            -> ValidateTargetLength
//     "supported" { channels }                                -> IsSupportedChannelCount
//     "buffer-new" { width, height }                          -> new BgraBuffer(width, height) (only geometries that fail or are tiny)
//     "resize-to-buffer" { sw, sh, tw, th, channels, source_len } -> ResizeToBuffer(new byte[source_len], ..) (zero-filled, tiny targets only)
//   Result = { name, op, ok: <int|bool|object>, error: null | { type, message } }.
static partial class Commands
{
    private const BindingFlags W517Any = BindingFlags.Static | BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;

    private static readonly Type W517Type =
        Type.GetType("PhotoReview.Imaging.LibRaw.RgbBgraResampler, PhotoReview.Imaging.LibRaw", throwOnError: true)!;

    private static readonly Type W517BufferType = W517Type.GetNestedType("BgraBuffer", BindingFlags.NonPublic | BindingFlags.Public)!;

    private static readonly Lazy<Action<byte[], int, int, byte[], int, int, CancellationToken, int>> W517ResizeShim = new(() =>
    {
        var target = W517Type.GetMethod("Resize", W517Any)!;
        var dm = new DynamicMethod("w517_resize", typeof(void),
            [typeof(byte[]), typeof(int), typeof(int), typeof(byte[]), typeof(int), typeof(int), typeof(CancellationToken), typeof(int)],
            typeof(Commands).Module, skipVisibility: true);
        var il = dm.GetILGenerator();
        il.Emit(OpCodes.Ldarg_0);
        il.Emit(OpCodes.Call, typeof(ReadOnlySpan<byte>).GetMethod("op_Implicit", [typeof(byte[])])!);
        il.Emit(OpCodes.Ldarg_1);
        il.Emit(OpCodes.Ldarg_2);
        il.Emit(OpCodes.Ldarg_3);
        il.Emit(OpCodes.Call, typeof(Span<byte>).GetMethod("op_Implicit", [typeof(byte[])])!);
        il.Emit(OpCodes.Ldarg_S, (byte)4);
        il.Emit(OpCodes.Ldarg_S, (byte)5);
        il.Emit(OpCodes.Ldarg_S, (byte)6);
        il.Emit(OpCodes.Ldarg_S, (byte)7);
        il.Emit(OpCodes.Call, target);
        il.Emit(OpCodes.Ret);
        return (Action<byte[], int, int, byte[], int, int, CancellationToken, int>)dm.CreateDelegate(
            typeof(Action<byte[], int, int, byte[], int, int, CancellationToken, int>));
    });

    private static readonly Lazy<Func<byte[], int, int, int, int, int, CancellationToken, IDisposable>> W517BufferShim = new(() =>
    {
        var target = W517Type.GetMethod("ResizeToBuffer", W517Any)!;
        var dm = new DynamicMethod("w517_resize_to_buffer", typeof(IDisposable),
            [typeof(byte[]), typeof(int), typeof(int), typeof(int), typeof(int), typeof(int), typeof(CancellationToken)],
            typeof(Commands).Module, skipVisibility: true);
        var il = dm.GetILGenerator();
        il.Emit(OpCodes.Ldarg_0);
        il.Emit(OpCodes.Call, typeof(ReadOnlySpan<byte>).GetMethod("op_Implicit", [typeof(byte[])])!);
        il.Emit(OpCodes.Ldarg_1);
        il.Emit(OpCodes.Ldarg_2);
        il.Emit(OpCodes.Ldarg_3);
        il.Emit(OpCodes.Ldarg_S, (byte)4);
        il.Emit(OpCodes.Ldarg_S, (byte)5);
        il.Emit(OpCodes.Ldarg_S, (byte)6);
        il.Emit(OpCodes.Call, target);
        il.Emit(OpCodes.Ret);
        return (Func<byte[], int, int, int, int, int, CancellationToken, IDisposable>)dm.CreateDelegate(
            typeof(Func<byte[], int, int, int, int, int, CancellationToken, IDisposable>));
    });

    private static T W517Get<T>(object instance, string property) =>
        (T)W517BufferType.GetProperty(property, W517Any)!.GetValue(instance)!;

    /// <summary>Calls a reflected member and unwraps TargetInvocationException so the real exception type is reported.</summary>
    private static object? W517Invoke(Func<object?> call)
    {
        try { return call(); }
        catch (TargetInvocationException ex) when (ex.InnerException is not null)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(ex.InnerException).Throw();
            throw; // unreachable
        }
    }

    private static JsonObject W517Error(Exception ex) => new() { ["type"] = ex.GetType().Name, ["message"] = ex.Message };

    private static byte[] W517Source(JsonObject spec, int sw, int sh, int channels)
    {
        var data = new byte[checked(sw * sh * channels)];
        switch (spec["kind"]!.GetValue<string>())
        {
            case "mod":
                {
                    long mul = spec["mul"]!.GetValue<long>(), add = spec["add"]!.GetValue<long>();
                    for (var i = 0; i < data.Length; i++) data[i] = (byte)(((long)i * mul + add) % 251);
                    break;
                }
            case "xorshift":
                {
                    var s = spec["seed"]!.GetValue<uint>();
                    for (var i = 0; i < data.Length; i++)
                    {
                        s ^= s << 13;
                        s ^= s >> 17;
                        s ^= s << 5;
                        data[i] = (byte)(s >> 8);
                    }
                    break;
                }
            case "solid":
                Array.Fill(data, spec["value"]!.GetValue<byte>());
                break;
            case "solid_rgb":
                {
                    var rgb = spec["rgb"]!.AsArray().Select(n => n!.GetValue<byte>()).ToArray();
                    for (var i = 0; i < sw * sh; i++) for (var c = 0; c < 3; c++) data[i * 3 + c] = rgb[c];
                    break;
                }
            case "checker":
                for (var y = 0; y < sh; y++)
                    for (var x = 0; x < sw; x++)
                        Array.Fill(data, (x + y) % 2 == 0 ? (byte)255 : (byte)0, (y * sw + x) * channels, channels);
                break;
            case "gradient":
                for (var y = 0; y < sh; y++)
                    for (var x = 0; x < sw; x++)
                        for (var c = 0; c < channels; c++)
                            data[(y * sw + x) * channels + c] = (byte)((x * 7 + y * 13 + c * 50) % 256);
                break;
            case "explicit":
                {
                    var bytes = spec["bytes"]!.AsArray().Select(n => n!.GetValue<byte>()).ToArray();
                    if (bytes.Length != data.Length) throw new ArgumentException("explicit source length mismatch");
                    bytes.CopyTo(data, 0);
                    break;
                }
            default:
                throw new ArgumentException("unknown source kind");
        }

        return data;
    }

    private static string W517Fnv(byte[] data)
    {
        var hash = 14695981039346656037UL;
        foreach (var b in data)
        {
            hash ^= b;
            hash *= 1099511628211UL;
        }

        return hash.ToString("x16");
    }

    private static string W517Hex(byte[] data, int count)
    {
        var sb = new StringBuilder();
        for (var i = 0; i < Math.Min(count, data.Length); i++) sb.Append(data[i].ToString("x2"));
        return sb.ToString();
    }

    public static JsonArray RgbBgraResample(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            int sw = c["sw"]!.GetValue<int>(), sh = c["sh"]!.GetValue<int>(), tw = c["tw"]!.GetValue<int>(), th = c["th"]!.GetValue<int>();
            var channels = c["channels"]!.GetValue<int>();
            var source = W517Source(c["source"]!.AsObject(), sw, sh, channels);
            using var cts = new CancellationTokenSource();
            if (c["cancelled"] is { } flag && flag.GetValue<bool>()) cts.Cancel();

            var expected = new byte[checked(tw * th * 4)];
            string? resizeError = null;
            try { W517ResizeShim.Value(source, sw, sh, expected, tw, th, cts.Token, channels); }
            catch (Exception ex) { resizeError = ex.GetType().Name; }

            string? bufferError = null;
            var bufferEqual = false;
            var aligned = false;
            var bufferLength = 0;
            try
            {
                using var buffer = W517BufferShim.Value(source, sw, sh, tw, th, channels, cts.Token);
                bufferLength = W517Get<int>(buffer, "Length");
                var pointer = W517Get<IntPtr>(buffer, "Pointer");
                aligned = (long)pointer % 64 == 0;
                var actual = new byte[bufferLength];
                Marshal.Copy(pointer, actual, 0, bufferLength);
                bufferEqual = resizeError is null && actual.AsSpan().SequenceEqual(expected);
            }
            catch (Exception ex) { bufferError = ex.GetType().Name; }

            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["sw"] = sw,
                ["sh"] = sh,
                ["tw"] = tw,
                ["th"] = th,
                ["channels"] = channels,
                ["resize_error"] = resizeError,
                ["buffer_error"] = bufferError,
                ["length"] = expected.Length,
                ["fnv1a64"] = resizeError is null ? W517Fnv(expected) : null,
                ["head_hex"] = resizeError is null ? W517Hex(expected, 32) : null,
                ["all_hex"] = resizeError is null && expected.Length <= 96 ? W517Hex(expected, 96) : null,
                ["buffer_equal"] = bufferEqual,
                ["buffer_aligned"] = aligned,
                ["buffer_length"] = bufferLength,
            });
        }

        return result;
    }

    public static JsonArray RgbBgraValidate(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            JsonNode? ok = null;
            JsonNode? error = null;
            try
            {
                switch (op)
                {
                    case "source":
                        ok = (int)W517Invoke(() => W517Type.GetMethod("ValidateSourceLength", W517Any)!.Invoke(null,
                            [c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>(), c["data_size"]!.GetValue<long>(), c["channels"]!.GetValue<int>()]))!;
                        break;
                    case "target":
                        ok = (int)W517Invoke(() => W517Type.GetMethod("ValidateTargetLength", W517Any)!.Invoke(null,
                            [c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>()]))!;
                        break;
                    case "supported":
                        ok = (bool)W517Invoke(() => W517Type.GetMethod("IsSupportedChannelCount", W517Any)!.Invoke(null, [c["channels"]!.GetValue<int>()]))!;
                        break;
                    case "buffer-new":
                        {
                            var buffer = (IDisposable)W517Invoke(() => Activator.CreateInstance(W517BufferType, W517Any, null,
                                [c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>()], null))!;
                            using (buffer)
                            {
                                ok = new JsonObject
                                {
                                    ["width"] = W517Get<int>(buffer, "Width"),
                                    ["height"] = W517Get<int>(buffer, "Height"),
                                    ["stride"] = W517Get<int>(buffer, "Stride"),
                                    ["length"] = W517Get<int>(buffer, "Length"),
                                    ["aligned"] = (long)W517Get<IntPtr>(buffer, "Pointer") % 64 == 0,
                                };
                            }

                            break;
                        }
                    case "resize-to-buffer":
                        {
                            using var buffer = W517BufferShim.Value(new byte[c["source_len"]!.GetValue<int>()], c["sw"]!.GetValue<int>(), c["sh"]!.GetValue<int>(),
                                c["tw"]!.GetValue<int>(), c["th"]!.GetValue<int>(), c["channels"]!.GetValue<int>(), CancellationToken.None);
                            ok = new JsonObject { ["length"] = W517Get<int>(buffer, "Length"), ["stride"] = W517Get<int>(buffer, "Stride") };
                            break;
                        }
                    default:
                        throw new NotSupportedException("unknown op " + op);
                }
            }
            catch (Exception ex) when (ex is not NotSupportedException)
            {
                error = W517Error(ex);
            }

            result.Add(new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op, ["ok"] = ok, ["error"] = error });
        }

        return result;
    }
}
