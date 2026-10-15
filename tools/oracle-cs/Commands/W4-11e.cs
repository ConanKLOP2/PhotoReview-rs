using System.Reflection;
using System.Reflection.Emit;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Pixels;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-11e
//     "wic-bgrx-to-bgr" => Commands.WicBgrxToBgrCases(inputs),
//     "wic-cache-classify" => Commands.WicCacheClassifyCases(inputs),
//     "wic-scale-resize" => Commands.WicScaleResizeCases(inputs),
//     "wic-cache-codec" => Commands.WicCacheCodecCases(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * WicPixelScaler / WicImageEncoder / WicCacheImageReader are `internal static`; they are reached by reflection and by
//     DynamicMethod (the span-taking methods cannot go through MethodInfo.Invoke: ReadOnlySpan<byte> is a ref struct), so no
//     InternalsVisibleTo is needed. PixelBuffer, PixelLayout and DecodeBox are public (PhotoReview.Imaging.dll, the same
//     reference W4-16a needs). Needs no other oracle file.
//   * wic-bgrx-to-bgr and wic-cache-classify are pure. wic-scale-resize and wic-cache-codec call the real WIC of the machine
//     that runs the oracle: deterministic for one Windows version (D19), so the Rust twins are `#[ignore = "native: needs WIC"]`
//     tests that run on windows-latest. If the runner's WIC differs from the golden machine, compare sizes, layouts and the
//     round-trip properties only and drop the *Fnv columns (open question 3 of the contract).

// W4-11e: WicImageEncoder.Bgrx32ToBgr24 / EncodePng / EncodeJpeg, WicCacheImageReader.Decode / IsOutOfMemory / IsOpaqueFormat,
// WicPixelScaler.Resize.
static partial class Commands
{
    private const ulong W411eFnvOffset = 14695981039346656037UL;
    private const ulong W411eFnvPrime = 1099511628211UL;

    private static readonly Assembly W411eAsm = typeof(PixelBuffer).Assembly;
    private static readonly Type W411eEncoder = W411eAsm.GetType("PhotoReview.Imaging.Caching.WicImageEncoder", throwOnError: true)!;
    private static readonly Type W411eReader = W411eAsm.GetType("PhotoReview.Imaging.Caching.WicCacheImageReader", throwOnError: true)!;
    private static readonly Type W411eScaler = W411eAsm.GetType("PhotoReview.Imaging.Pixels.WicPixelScaler", throwOnError: true)!;
    private static readonly Type W411eInfo = W411eAsm.GetType("PhotoReview.Imaging.Caching.WicCacheImageInfo", throwOnError: true)!;
    private const BindingFlags W411eStatic = BindingFlags.Static | BindingFlags.NonPublic | BindingFlags.Public;

    private delegate void W411eBgrxFn(byte[] source, byte[] destination);

    private delegate PixelBuffer W411eDecodeFn(byte[] encoded, PixelLayout? layout, DecodeBox box, int expectedWidth, int expectedHeight,
        out int sourceWidth, out int sourceHeight, out bool downscaled);

    /// <summary>Bgrx32ToBgr24(ReadOnlySpan, Span) wrapped over byte[] (a span cannot be passed through MethodInfo.Invoke).</summary>
    private static readonly Lazy<W411eBgrxFn> W411eBgrx = new(() =>
    {
        var target = W411eEncoder.GetMethod("Bgrx32ToBgr24", W411eStatic)!;
        var dm = new DynamicMethod("w411e_bgrx", typeof(void), [typeof(byte[]), typeof(byte[])], typeof(Commands).Module, skipVisibility: true);
        var il = dm.GetILGenerator();
        il.Emit(OpCodes.Ldarg_0);
        il.Emit(OpCodes.Newobj, typeof(ReadOnlySpan<byte>).GetConstructor([typeof(byte[])])!);
        il.Emit(OpCodes.Ldarg_1);
        il.Emit(OpCodes.Newobj, typeof(Span<byte>).GetConstructor([typeof(byte[])])!);
        il.Emit(OpCodes.Call, target);
        il.Emit(OpCodes.Ret);
        return (W411eBgrxFn)dm.CreateDelegate(typeof(W411eBgrxFn));
    });

    /// <summary>Decode(ReadOnlySpan, PixelLayout?, DecodeBox, int, int, out WicCacheImageInfo) flattened to byte[] and three out values.</summary>
    private static readonly Lazy<W411eDecodeFn> W411eDecode = new(() =>
    {
        var target = W411eReader.GetMethod("Decode", W411eStatic)!;
        var dm = new DynamicMethod("w411e_decode", typeof(PixelBuffer),
            [typeof(byte[]), typeof(PixelLayout?), typeof(DecodeBox), typeof(int), typeof(int),
                typeof(int).MakeByRefType(), typeof(int).MakeByRefType(), typeof(bool).MakeByRefType()],
            typeof(Commands).Module, skipVisibility: true);
        var il = dm.GetILGenerator();
        var info = il.DeclareLocal(W411eInfo);
        il.Emit(OpCodes.Ldarg_0);
        il.Emit(OpCodes.Newobj, typeof(ReadOnlySpan<byte>).GetConstructor([typeof(byte[])])!);
        il.Emit(OpCodes.Ldarg_1);
        il.Emit(OpCodes.Ldarg_2);
        il.Emit(OpCodes.Ldarg_3);
        il.Emit(OpCodes.Ldarg_S, (byte)4);
        il.Emit(OpCodes.Ldloca, info);
        il.Emit(OpCodes.Call, target);
        il.Emit(OpCodes.Ldarg_S, (byte)5);
        il.Emit(OpCodes.Ldloca, info);
        il.Emit(OpCodes.Call, W411eInfo.GetProperty("SourceWidth")!.GetMethod!);
        il.Emit(OpCodes.Stind_I4);
        il.Emit(OpCodes.Ldarg_S, (byte)6);
        il.Emit(OpCodes.Ldloca, info);
        il.Emit(OpCodes.Call, W411eInfo.GetProperty("SourceHeight")!.GetMethod!);
        il.Emit(OpCodes.Stind_I4);
        il.Emit(OpCodes.Ldarg_S, (byte)7);
        il.Emit(OpCodes.Ldloca, info);
        il.Emit(OpCodes.Call, W411eInfo.GetProperty("Downscaled")!.GetMethod!);
        il.Emit(OpCodes.Stind_I1);
        il.Emit(OpCodes.Ret);
        return (W411eDecodeFn)dm.CreateDelegate(typeof(W411eDecodeFn));
    });

    private static string W411eFnv(ReadOnlySpan<byte> bytes)
    {
        var h = W411eFnvOffset;
        foreach (var b in bytes) h = unchecked((h ^ b) * W411eFnvPrime);
        return h.ToString("x16");
    }

    private static string W411eHex(ReadOnlySpan<byte> bytes) => Convert.ToHexString(bytes).ToLowerInvariant();

    private static string W411eErrorName(Exception ex) => (ex is TargetInvocationException { InnerException: { } inner } ? inner : ex).GetType().Name;

    private static PixelLayout W411eLayout(string text) => text == "pbgra32" ? PixelLayout.Pbgra32 : PixelLayout.Bgr32;

    /// <summary>
    /// Deterministic content (the Rust test re-implements these lines): byte i (row-major, no padding) =
    /// (i * 131 + seed * 17 + i / 251) mod 256 in 64-bit arithmetic. With <c>alpha</c> (Pbgra32 only) byte 3 of pixel (x, y) is
    /// 255 - (x * 255 / max(width - 1, 1)) and the colour bytes are clamped to it, so the buffer is valid premultiplied data.
    /// </summary>
    private static PixelBuffer W411ePattern(int width, int height, PixelLayout layout, int seed, bool alpha)
    {
        var px = PixelBuffer.Allocate(width, height, layout);
        if (!px.TryGetSpan(out var all)) throw new InvalidOperationException("too large");
        for (var i = 0; i < all.Length; i++) all[i] = (byte)((((long)i * 131) + ((long)seed * 17) + (i / 251)) & 0xFF);
        if (alpha && layout == PixelLayout.Pbgra32)
        {
            for (var y = 0; y < height; y++)
            {
                var row = px.GetRow(y);
                for (var x = 0; x < width; x++)
                {
                    var a = (byte)(255 - (x * 255 / Math.Max(width - 1, 1)));
                    row[(x * 4) + 3] = a;
                    for (var c = 0; c < 3; c++) row[(x * 4) + c] = Math.Min(row[(x * 4) + c], a);
                }
            }
        }

        return px;
    }

    /// <summary>FNV-1a over the meaningful bytes: B,G,R of every pixel for Bgr32 (byte 3 is unused and WIC-defined), all four for Pbgra32.</summary>
    private static string W411eMeaningfulFnv(PixelBuffer px)
    {
        var h = W411eFnvOffset;
        for (var y = 0; y < px.Height; y++)
        {
            var row = px.GetRow(y);
            for (var x = 0; x < px.Width; x++)
            {
                var n = px.Layout == PixelLayout.Pbgra32 ? 4 : 3;
                for (var c = 0; c < n; c++) h = unchecked((h ^ row[(x * 4) + c]) * W411eFnvPrime);
            }
        }

        return h.ToString("x16");
    }

    /// <summary>Cases {name, sourceLen, destLen}. Output: name, throws (type name or null), hex (first 3 * (sourceLen / 4) bytes of the destination).</summary>
    public static JsonArray WicBgrxToBgrCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var sourceLen = c["sourceLen"]!.GetValue<int>();
            var destLen = c["destLen"]!.GetValue<int>();
            var source = new byte[sourceLen];
            for (var i = 0; i < source.Length; i++) source[i] = (byte)(i % 4 == 3 ? 0xEE : ((i * 7) + 1) & 0xFF);
            var destination = new byte[destLen];
            var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            try
            {
                W411eBgrx.Value(source, destination);
                row["throws"] = null;
                row["hex"] = W411eHex(destination.AsSpan(0, Math.Min(destLen, sourceLen / 4 * 3)));
            }
            catch (Exception ex)
            {
                row["throws"] = W411eErrorName(ex);
                row["hex"] = null;
            }

            result.Add(row);
        }

        return result;
    }

    /// <summary>Cases {name, hresult, guid}. Output: name, isOutOfMemory (IsOutOfMemory(new COMException("x", hresult))), isOpaque (IsOpaqueFormat(guid)).</summary>
    public static JsonArray WicCacheClassifyCases(JsonObject inputs)
    {
        var isOom = W411eReader.GetMethod("IsOutOfMemory", W411eStatic)!;
        var isOpaque = W411eReader.GetMethod("IsOpaqueFormat", W411eStatic)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var hresult = unchecked((int)c["hresult"]!.GetValue<long>());
            var guid = Guid.Parse(c["guid"]!.GetValue<string>());
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["isOutOfMemory"] = (bool)isOom.Invoke(null, [new COMException("x", hresult)])!,
                ["isOpaque"] = (bool)isOpaque.Invoke(null, [guid])!,
            });
        }

        return result;
    }

    /// <summary>
    /// Cases {name, layout: "bgr32"|"pbgra32", srcWidth, srcHeight, width, height, seed, alpha?, disposeFirst?}. Output: name, throws,
    /// width, height, layout, fnv (meaningful bytes), sameAsSource (true when the result bytes equal the source bytes), distinctBuffer.
    /// </summary>
    public static JsonArray WicScaleResizeCases(JsonObject inputs)
    {
        var resize = W411eScaler.GetMethod("Resize", W411eStatic)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var layout = W411eLayout(c["layout"]!.GetValue<string>());
            using var source = W411ePattern(c["srcWidth"]!.GetValue<int>(), c["srcHeight"]!.GetValue<int>(), layout,
                c["seed"]?.GetValue<int>() ?? 0, c["alpha"]?.GetValue<bool>() ?? false);
            var sourceFnv = W411eMeaningfulFnv(source);
            if (c["disposeFirst"]?.GetValue<bool>() == true) source.Dispose();
            var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            try
            {
                using var scaled = (PixelBuffer)resize.Invoke(null, [source, c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>()])!;
                row["throws"] = null;
                row["width"] = scaled.Width;
                row["height"] = scaled.Height;
                row["layout"] = scaled.Layout == PixelLayout.Pbgra32 ? "pbgra32" : "bgr32";
                var scaledFnv = W411eMeaningfulFnv(scaled);
                row["fnv"] = scaledFnv;
                row["sameAsSource"] = scaled.Width == source.Width && scaled.Height == source.Height && scaledFnv == sourceFnv;
                row["distinctBuffer"] = scaled.Address != source.Address;
            }
            catch (Exception ex)
            {
                row["throws"] = W411eErrorName(ex);
            }

            result.Add(row);
        }

        return result;
    }

    /// <summary>
    /// Cases {name, encode?: "png"|"jpeg", quality?, layout, width, height, seed, alpha?, payloadHex? (raw payload instead of encoding),
    /// decodeLayout?: "bgr32"|"pbgra32"|null, box?: [w, h], expected?: [w, h]}. Output: name, encodeThrows, encodedLength, encodedFnv,
    /// decodeThrows, width, height, layout, fnv (meaningful bytes of the decoded buffer), sourceWidth, sourceHeight, downscaled.
    /// </summary>
    public static JsonArray WicCacheCodecCases(JsonObject inputs)
    {
        var encodePng = W411eEncoder.GetMethod("EncodePng", W411eStatic)!;
        var encodeJpeg = W411eEncoder.GetMethod("EncodeJpeg", W411eStatic)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            byte[]? payload = null;
            if (c["payloadHex"] is { } hexNode)
            {
                payload = Convert.FromHexString(hexNode.GetValue<string>());
            }
            else
            {
                var layout = W411eLayout(c["layout"]!.GetValue<string>());
                using var pixels = W411ePattern(c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>(), layout,
                    c["seed"]?.GetValue<int>() ?? 0, c["alpha"]?.GetValue<bool>() ?? false);
                using var ms = new MemoryStream();
                try
                {
                    if (c["encode"]!.GetValue<string>() == "jpeg") encodeJpeg.Invoke(null, [pixels, ms, c["quality"]?.GetValue<int>() ?? 90]);
                    else encodePng.Invoke(null, [pixels, ms]);
                    payload = ms.ToArray();
                    row["encodeThrows"] = null;
                    row["encodedLength"] = payload.Length;
                    row["encodedFnv"] = W411eFnv(payload);
                }
                catch (Exception ex)
                {
                    row["encodeThrows"] = W411eErrorName(ex);
                }
            }

            if (payload is not null)
            {
                PixelLayout? decodeLayout = c["decodeLayout"] is { } dl ? W411eLayout(dl.GetValue<string>()) : null;
                var box = c["box"] is { } b ? new DecodeBox(b[0]!.GetValue<int>(), b[1]!.GetValue<int>()) : DecodeBox.Unbounded;
                var expectedWidth = c["expected"] is { } e ? e[0]!.GetValue<int>() : 0;
                var expectedHeight = c["expected"] is { } e2 ? e2[1]!.GetValue<int>() : 0;
                try
                {
                    using var decoded = W411eDecode.Value(payload, decodeLayout, box, expectedWidth, expectedHeight,
                        out var sourceWidth, out var sourceHeight, out var downscaled);
                    row["decodeThrows"] = null;
                    row["width"] = decoded.Width;
                    row["height"] = decoded.Height;
                    row["layout"] = decoded.Layout == PixelLayout.Pbgra32 ? "pbgra32" : "bgr32";
                    row["fnv"] = W411eMeaningfulFnv(decoded);
                    row["sourceWidth"] = sourceWidth;
                    row["sourceHeight"] = sourceHeight;
                    row["downscaled"] = downscaled;
                }
                catch (Exception ex)
                {
                    row["decodeThrows"] = W411eErrorName(ex);
                }
            }

            result.Add(row);
        }

        return result;
    }
}
