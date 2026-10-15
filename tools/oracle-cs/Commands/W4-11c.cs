using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Decoding.Wic;
using PhotoReview.Imaging.Metadata;
using PhotoReview.Imaging.Pixels;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-11c
//     "wic-direct-internals" => Commands.WicDirectInternals(inputs),
//     "wic-direct-fixtures" => Commands.WicDirectFixtures(inputs, inputPath),
//     "wic-direct-decode" => Commands.WicDirectDecode(inputs, inputPath),
//     "wic-direct-read-info" => Commands.WicDirectReadInfo(inputs, inputPath),
//
// WIRING (the wiring step does this; the contract writer did NOT run it):
//   * Same project changes as W4-02b (do them once): `<UseWPF>true</UseWPF>` in OracleCs.csproj (fixture generation uses the
//     WPF encoders exactly like tests/PhotoReview.Imaging.Tests/Fixtures/FixtureGenerator.cs) and a reference to the baseline
//     PhotoReview.Imaging.dll (tools/oracle.ps1 copies it next to PhotoReview.Core.dll).
//   * Run order: `wic-direct-fixtures` FIRST. It writes tests/fixtures/wic-direct/* (inputs.output_dir, relative to the input
//     file) and returns a manifest {file, length, sha256, what} that becomes tests/golden/wic-direct-fixtures.json. Commit the
//     files: the WPF JPEG encoder is not bit-stable across Windows builds, so decode/read-info goldens are pinned to the bytes.
//   * `wic-direct-decode` / `wic-direct-read-info` run the REAL WicDirectDecoder (internal members reached by reflection) with
//     PixelBufferImageCodec.Instance, a recording ISourceReader and a fixed MemoryInfo (64 GiB total, 0 load): D19, no machine
//     memory in the golden. Their first row ("environment") records the Windows build and the windowscodecs.dll version: pixel
//     hashes are only comparable on the same WIC build (contract, golden package).
//   * Paths in messages are rewritten to "<fixtures>\..." (D19: no machine path in a golden).

// W4-11c: WicDirectDecoder (pure internals, real-WIC decode and read-info)
static partial class Commands
{
    private static readonly Type W411cType = typeof(WicDirectDecoder);

    private static MethodInfo W411cMethod(string name) =>
        W411cType.GetMethod(name, BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.Static)
        ?? throw new InvalidOperationException("WicDirectDecoder." + name + " not found (renamed at the baseline?)");

    /// <summary>Invokes a static member by reflection and unwraps TargetInvocationException, so the real exception is reported.</summary>
    private static object? W411cInvoke(string name, params object?[] args)
    {
        try
        {
            return W411cMethod(name).Invoke(null, args);
        }
        catch (TargetInvocationException ex) when (ex.InnerException is not null)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(ex.InnerException).Throw();
            throw;
        }
    }

    private static string W411cSanitize(string text, string? fixturesDir) =>
        fixturesDir is null ? text : text.Replace(fixturesDir, "<fixtures>", StringComparison.OrdinalIgnoreCase);

    /// <summary>The facts a Rust test can compare about a C# exception (message is OS text for COM errors: compared by the
    /// Rust test only where the contract says so).</summary>
    private static JsonObject W411cError(Exception ex, string? fixturesDir = null) => new()
    {
        ["type"] = ex.GetType().FullName,
        ["hresult"] = ex.HResult,
        ["message"] = W411cSanitize(ex.Message, fixturesDir),
        ["inner_type"] = ex.InnerException?.GetType().FullName,
        ["is_localized"] = PhotoReview.Core.Localization.UserFacingError.IsLocalized(ex),
        ["fallbackable"] = FallbackImageDecoder.IsFallbackable(ex),
    };

    // ---- wic-direct-internals ---------------------------------------------------------------------------------------

    /// <summary>
    /// Cases {name, fn, ...}; fn = to-int-size {width,height} | output-byte-length {width,height} | ensure-output-fits
    /// {width,height,buffer_length,total,load} | is-opaque-format {guid} | map-transform-options {orientation} | as-invalid-data
    /// {detail, decoder|null} | copy-pixels-guarded {throw: none|com|invalid-op, hresult, message, active} | throw-if-factory-failed
    /// {hr, created}. First row "constants".
    /// </summary>
    public static JsonArray WicDirectInternals(JsonObject inputs)
    {
        var srgb = W411cType.GetField("SrgbExifColorSpace", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null);
        var result = new JsonArray
        {
            new JsonObject
            {
                ["name"] = "constants",
                ["srgb_exif_color_space"] = Convert.ToUInt32(srgb, System.Globalization.CultureInfo.InvariantCulture),
                ["guard_threshold_bytes"] = MemoryHeadroom.GuardThresholdBytes,
                ["opaque_formats"] = new JsonArray(((IEnumerable<Guid>)W411cType
                    .GetField("OpaquePixelFormats", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null)!)
                    .Select(g => (JsonNode?)g.ToString("D")).ToArray()),
            },
        };

        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var fn = c["fn"]!.GetValue<string>();
            var row = (JsonObject)c.DeepClone();
            try
            {
                switch (fn)
                {
                    case "to-int-size":
                    {
                        var size = ((int, int))W411cInvoke("ToIntSize", c["width"]!.GetValue<uint>(), c["height"]!.GetValue<uint>())!;
                        row["ok"] = new JsonObject { ["width"] = size.Item1, ["height"] = size.Item2 };
                        break;
                    }
                    case "output-byte-length":
                        row["ok"] = (long)W411cInvoke("OutputByteLength", c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>())!;
                        break;
                    case "ensure-output-fits":
                    {
                        var consulted = 0;
                        var total = c["total"]!.GetValue<long>();
                        var load = c["load"]!.GetValue<long>();
                        Func<(long TotalAvailable, long Load)> memory = () => { consulted++; return (total, load); };
                        try
                        {
                            W411cInvoke("EnsureOutputFits", c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>(),
                                c["buffer_length"]!.GetValue<long>(), memory);
                            row["ok"] = true;
                        }
                        finally
                        {
                            row["consulted"] = consulted;
                        }
                        break;
                    }
                    case "is-opaque-format":
                        row["ok"] = (bool)W411cInvoke("IsOpaqueFormat", Guid.Parse(c["guid"]!.GetValue<string>()))!;
                        break;
                    case "map-transform-options":
                        row["ok"] = Convert.ToUInt32(W411cInvoke("MapToTransformOptions", c["orientation"]!.GetValue<int>()),
                            System.Globalization.CultureInfo.InvariantCulture);
                        break;
                    case "as-invalid-data":
                    {
                        var inner = new ArgumentException(c["detail"]!.GetValue<string>());
                        var decoderName = c["decoder"]?.GetValue<string>();
                        var mapped = (Exception)(decoderName is null
                            ? W411cInvoke("AsInvalidData", inner, "WicDirect")!
                            : W411cInvoke("AsInvalidData", inner, decoderName)!);
                        row["ok"] = W411cError(mapped);
                        break;
                    }
                    case "copy-pixels-guarded":
                    {
                        var kind = c["throw"]!.GetValue<string>();
                        var message = c["message"]?.GetValue<string>() ?? "";
                        var hr = c["hresult"]?.GetValue<int>() ?? 0;
                        Action copy = kind switch
                        {
                            "none" => () => { },
#pragma warning disable CA2201 // a COMException is exactly what a failing WIC CopyPixels raises
                            "com" => () => throw new COMException(message, hr),
#pragma warning restore CA2201
                            "invalid-op" => () => throw new InvalidOperationException(message),
                            "argument" => () => throw new ArgumentException(message),
                            _ => throw new ArgumentException("unknown throw kind " + kind),
                        };
                        W411cInvoke("CopyPixelsGuarded", copy, c["active"]!.GetValue<bool>());
                        row["ok"] = true;
                        break;
                    }
                    case "throw-if-factory-failed":
                        W411cInvoke("ThrowIfFactoryFailed", c["hr"]!.GetValue<int>(), c["created"]!.GetValue<bool>());
                        row["ok"] = true;
                        break;
                    default:
                        throw new ArgumentException("unknown fn " + fn);
                }
            }
            catch (Exception ex) when (ex is not ArgumentException { Message: var m } || !m.StartsWith("unknown ", StringComparison.Ordinal))
            {
                var error = W411cError(ex);
                if (fn == "throw-if-factory-failed") error.Remove("message"); // OS FormatMessage text, locale dependent
                row["error"] = error;
            }
            result.Add(row);
        }
        return result;
    }

    // ---- wic-direct-fixtures ----------------------------------------------------------------------------------------

    /// <summary>Replica of FixtureGenerator.CreateGradientCheckerboard (Bgra32, 96 dpi, four coloured corners).</summary>
    private static System.Windows.Media.Imaging.BitmapSource W411cGradient(int width, int height)
    {
        var stride = width * 4;
        var pixels = new byte[stride * height];
        var tileSize = Math.Max(4, Math.Min(width, height) / 8);
        var cornerSize = Math.Max(2, Math.Min(8, Math.Min(width, height) / 4));
        for (var y = 0; y < height; y++)
        {
            var g0 = (byte)(y * 255 / Math.Max(1, height - 1));
            for (var x = 0; x < width; x++)
            {
                var o = (y * stride) + (x * 4);
                var r = (byte)(x * 255 / Math.Max(1, width - 1));
                var g = g0;
                var b = ((x / tileSize) + (y / tileSize)) % 2 == 0 ? (byte)210 : (byte)45;
                if (x < cornerSize && y < cornerSize) { r = 255; g = 255; b = 0; }
                else if (x >= width - cornerSize && y < cornerSize) { r = 0; g = 255; b = 255; }
                else if (x < cornerSize && y >= height - cornerSize) { r = 255; g = 0; b = 255; }
                else if (x >= width - cornerSize && y >= height - cornerSize) { r = 255; g = 255; b = 255; }
                pixels[o] = b; pixels[o + 1] = g; pixels[o + 2] = r; pixels[o + 3] = 255;
            }
        }
        var bitmap = System.Windows.Media.Imaging.BitmapSource.Create(width, height, 96, 96,
            System.Windows.Media.PixelFormats.Bgra32, null, pixels, stride);
        bitmap.Freeze();
        return bitmap;
    }

    private static System.Windows.Media.Imaging.BitmapSource W411cSolid(int width, int height, byte b, byte g, byte r)
    {
        var pixels = new byte[width * height * 4];
        for (var i = 0; i < pixels.Length; i += 4) { pixels[i] = b; pixels[i + 1] = g; pixels[i + 2] = r; pixels[i + 3] = 255; }
        var bitmap = System.Windows.Media.Imaging.BitmapSource.Create(width, height, 96, 96,
            System.Windows.Media.PixelFormats.Bgra32, null, pixels, width * 4);
        bitmap.Freeze();
        return bitmap;
    }

    private static byte[] W411cEncode(System.Windows.Media.Imaging.BitmapEncoder encoder,
        System.Windows.Media.Imaging.BitmapSource source, System.Windows.Media.Imaging.BitmapMetadata? metadata = null,
        ReadOnlyCollection<System.Windows.Media.ColorContext>? colorContexts = null)
    {
        encoder.Frames.Add(System.Windows.Media.Imaging.BitmapFrame.Create(source, null, metadata, colorContexts));
        using var stream = new MemoryStream();
        encoder.Save(stream);
        return stream.ToArray();
    }

    private static byte[] W411cJpeg(System.Windows.Media.Imaging.BitmapSource source, int quality = 90,
        System.Windows.Media.Imaging.BitmapMetadata? metadata = null,
        ReadOnlyCollection<System.Windows.Media.ColorContext>? colorContexts = null) =>
        W411cEncode(new System.Windows.Media.Imaging.JpegBitmapEncoder { QualityLevel = quality }, source, metadata, colorContexts);

    private static byte[] W411cPng(System.Windows.Media.Imaging.BitmapSource source,
        ReadOnlyCollection<System.Windows.Media.ColorContext>? colorContexts = null) =>
        W411cEncode(new System.Windows.Media.Imaging.PngBitmapEncoder(), source, null, colorContexts);

    private static System.Windows.Media.Imaging.BitmapSource W411cConvert(System.Windows.Media.Imaging.BitmapSource source,
        System.Windows.Media.PixelFormat format, System.Windows.Media.Imaging.BitmapPalette? palette = null)
    {
        var converted = new System.Windows.Media.Imaging.FormatConvertedBitmap(source, format, palette, 0);
        if (converted.CanFreeze) converted.Freeze();
        return converted;
    }

    private static System.Windows.Media.Imaging.BitmapMetadata W411cJpegMeta(params (string Query, object Value)[] values)
    {
        var metadata = new System.Windows.Media.Imaging.BitmapMetadata("jpg");
        foreach (var (query, value) in values) metadata.SetQuery(query, value);
        return metadata;
    }

    /// <summary>
    /// Writes the fixture set to inputs.output_dir (relative to the inputs file) and reports {file, length, sha256, what}.
    /// inputs.icc_profile (relative to the inputs file) is tests/fixtures/ColorProfiles/DisplayP3-v4.icc.
    /// </summary>
    public static JsonArray WicDirectFixtures(JsonObject inputs, string inputPath)
    {
        var baseDir = Path.GetDirectoryName(Path.GetFullPath(inputPath))!;
        var dir = Path.GetFullPath(Path.Combine(baseDir, inputs["output_dir"]!.GetValue<string>()));
        var icc = Path.GetFullPath(Path.Combine(baseDir, inputs["icc_profile"]!.GetValue<string>()));
        Directory.CreateDirectory(dir);
        var files = new List<(string Name, byte[] Bytes, string What)>();

        // DecodeSizeDecisionMutationTests.Sources
        foreach (var (name, w, h) in new[] { ("plain-64x48", 64, 48), ("flat-64x1", 64, 1), ("flat-1x64", 1, 64), ("short-64x3", 64, 3), ("narrow-3x64", 3, 64) })
            files.Add((name + ".jpg", W411cJpeg(W411cGradient(w, h)), $"GenerateGradientJpeg({w},{h}) q90"));
        files.Add(("rot6-64x48.jpg", W411cJpeg(W411cGradient(64, 48), 90, W411cJpegMeta(("/app1/ifd/{ushort=274}", (ushort)6))), "GenerateJpegWithOrientation(64,48,6)"));
        files.Add(("rot6-64x1.jpg", W411cJpeg(W411cGradient(64, 1), 90, W411cJpegMeta(("/app1/ifd/{ushort=274}", (ushort)6))), "GenerateJpegWithOrientation(64,1,6)"));

        // OrientationFixture / WicDirectDecoderCopyCountTests: 120x80 with every orientation, plus the two invalid values.
        foreach (var o in new ushort[] { 0, 1, 2, 3, 4, 5, 6, 7, 8, 9 })
            files.Add(($"orient-{o}-120x80.jpg", W411cJpeg(W411cGradient(120, 80), 90, W411cJpegMeta(("/app1/ifd/{ushort=274}", o))),
                o is >= 1 and <= 8 ? $"EXIF orientation {o}" : $"EXIF orientation {o} (out of range: read as 1)"));

        // Native DCT pre-scale paths.
        files.Add(("gradient-400x300.jpg", W411cJpeg(W411cGradient(400, 300)), "WicDirectTests.DownscaleProducesTargetWidth"));
        files.Add(("gradient-800x600.jpg", W411cJpeg(W411cGradient(800, 600)), "1/8 DCT reduction available"));

        // Pixel formats (opaque vs alpha output layout).
        var g64 = W411cGradient(64, 48);
        var g32 = W411cGradient(32, 24);
        files.Add(("gradient-64x48-bgra32.png", W411cPng(g64), "GeneratePng is32Bit: Bgra32 -> Pbgra32 output"));
        files.Add(("gradient-64x48-bgr24.png", W411cPng(W411cConvert(g64, System.Windows.Media.PixelFormats.Bgr24)), "GeneratePng 24-bit -> Bgr32 output"));
        files.Add(("gray8-32x24.png", W411cPng(W411cConvert(g32, System.Windows.Media.PixelFormats.Gray8)), "8bpp gray (opaque)"));
        files.Add(("gray16-32x24.png", W411cPng(W411cConvert(g32, System.Windows.Media.PixelFormats.Gray16)), "16bpp gray (opaque)"));
        files.Add(("bw-32x24.png", W411cPng(W411cConvert(g32, System.Windows.Media.PixelFormats.BlackWhite)), "1bpp black/white (opaque)"));
        files.Add(("indexed8-32x24.png", W411cPng(W411cConvert(g32, System.Windows.Media.PixelFormats.Indexed8,
            System.Windows.Media.Imaging.BitmapPalettes.Halftone256)), "8bpp indexed (treated as alpha-capable)"));
        files.Add(("rgb48-32x24.png", W411cPng(W411cConvert(g32, System.Windows.Media.PixelFormats.Rgb48)), "48bpp RGB (opaque)"));
        files.Add(("bgr24-32x24.bmp", W411cEncode(new System.Windows.Media.Imaging.BmpBitmapEncoder(),
            W411cConvert(g32, System.Windows.Media.PixelFormats.Bgr24)), "BMP 24bpp"));
        files.Add(("cmyk32-32x24.tif", W411cEncode(new System.Windows.Media.Imaging.TiffBitmapEncoder(),
            W411cConvert(g32, System.Windows.Media.PixelFormats.Cmyk32)), "TIFF 32bpp CMYK (opaque, no profile)"));

        // EXIF container choice (WicDirectInternalsMutationTests.TiffWithExif / JpegWithExif) and the photo fields.
        var tiffMeta = new System.Windows.Media.Imaging.BitmapMetadata("tiff");
        tiffMeta.SetQuery("/ifd/{ushort=271}", "TiffMaker");
        files.Add(("tiff-make-32x24.tif", W411cEncode(new System.Windows.Media.Imaging.TiffBitmapEncoder(), g32, tiffMeta), "TIFF with Make=TiffMaker"));
        files.Add(("jpeg-make-32x24.jpg", W411cJpeg(g32, 90, W411cJpegMeta(
            ("/app1/ifd/{ushort=271}", "JpegMaker"),
            ("/app1/ifd/{ushort=272}", "ModelX"),
            ("/app1/ifd/exif/{ushort=34855}", (ushort)200),
            ("/app1/ifd/exif/{ushort=36867}", "2024:05:06 07:08:09"))), "JPEG with Make/Model/ISO/DateTimeOriginal"));

        // EXIF ColorSpace (WicDirectInternalsMutationTests.OrangeJpegWithExifColorSpace), quality 100.
        var orange = W411cSolid(32, 24, 20, 120, 230);
        files.Add(("orange-cs-none.jpg", W411cJpeg(orange, 100), "orange, no ColorSpace tag"));
        files.Add(("orange-cs-1.jpg", W411cJpeg(orange, 100, W411cJpegMeta(("/app1/ifd/exif/{ushort=40961}", (ushort)1))), "orange, ColorSpace 1 (sRGB)"));
        files.Add(("orange-cs-2.jpg", W411cJpeg(orange, 100, W411cJpegMeta(("/app1/ifd/exif/{ushort=40961}", (ushort)2))), "orange, ColorSpace 2 (Adobe RGB)"));

        // Embedded ICC (FixtureGenerator.GenerateJpegWithIcc / GeneratePngWithIcc).
        var p3 = new ReadOnlyCollection<System.Windows.Media.ColorContext>(new[] { new System.Windows.Media.ColorContext(new Uri(icc)) });
        files.Add(("icc-p3-64x48.jpg", W411cJpeg(g64, 90, null, p3), "GenerateJpegWithIcc (Display P3 v4)"));
        var alphaPixels = new byte[64 * 48 * 4];
        g64.CopyPixels(alphaPixels, 64 * 4, 0);
        alphaPixels[3] = 64;
        var alpha = System.Windows.Media.Imaging.BitmapSource.Create(64, 48, 96, 96, System.Windows.Media.PixelFormats.Bgra32, null, alphaPixels, 64 * 4);
        alpha.Freeze();
        files.Add(("icc-p3-alpha-64x48.png", W411cPng(alpha, p3), "GeneratePngWithIcc (semi-transparent top-left pixel)"));

        // Damaged inputs.
        var g400 = files.First(f => f.Name == "gradient-400x300.jpg").Bytes;
        files.Add(("truncated-400x300-30.jpg", g400.AsSpan(0, Math.Max(1, (int)(g400.Length * 0.3))).ToArray(), "GenerateTruncatedJpeg(ratio 0.3)"));
        var g128 = W411cJpeg(W411cGradient(128, 96));
        files.Add(("truncated-128x96-60.jpg", g128.AsSpan(0, g128.Length * 6 / 10).ToArray(), "TruncatedJpeg_WrapsNativeDetail (6/10)"));
        files.Add(("text.jpg", System.Text.Encoding.UTF8.GetBytes("This is plain text, not a valid JPEG or image format."), "GenerateTextFile"));
        files.Add(("empty.jpg", [], "GenerateZeroByteFile"));
        var garbage = new byte[64];
        garbage[0] = 0xFF; garbage[1] = 0xD8; garbage[2] = 0xFF; garbage[3] = 0xE0;
        files.Add(("header-garbage.jpg", garbage, "CorruptHeader_WrapsNativeDetail (SOI + APP0 then zeros)"));
        files.Add(("huge-65500.jpg",
        [
            0xFF, 0xD8,
            0xFF, 0xC0, 0x00, 0x0B, 0x08, 0xFF, 0xDC, 0xFF, 0xDC, 0x01, 0x01, 0x11, 0x00,
            0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00,
            0xFF, 0xD9,
        ], "HugeDeclaredDimensions_AreRejectedWithLocalizedText (SOF0 65500x65500, no entropy data)"));

        var result = new JsonArray();
        foreach (var (name, bytes, what) in files)
        {
            File.WriteAllBytes(Path.Combine(dir, name), bytes);
            result.Add(new JsonObject
            {
                ["file"] = name,
                ["length"] = bytes.Length,
                ["sha256"] = Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant(),
                ["what"] = what,
            });
        }
        return result;
    }

    // ---- wic-direct-decode / wic-direct-read-info --------------------------------------------------------------------

    private sealed class W411cRecordingReader : ISourceReader
    {
        public List<(string File, SourceReadPriority Priority, int BufferSize)> Calls { get; } = [];

        public Stream OpenSource(string path, SourceReadPriority priority, int bufferSize = 1024 * 1024)
        {
            Calls.Add((Path.GetFileName(path), priority, bufferSize));
            return PhysicalSourceReader.Instance.OpenSource(path, priority, bufferSize);
        }
    }

    private static WicDirectDecoder W411cDecoder(ISourceReader reader)
    {
        var decoder = new WicDirectDecoder(PixelBufferImageCodec.Instance, reader);
        // init-only internal property: reflection can set it. Fixed budget: the golden never depends on this machine's RAM.
        Func<(long TotalAvailable, long Load)> memory = () => (64L * 1024 * 1024 * 1024, 0L);
        W411cType.GetProperty("MemoryInfo", BindingFlags.NonPublic | BindingFlags.Instance)!.SetValue(decoder, memory);
        return decoder;
    }

    private static JsonObject W411cEnvironment()
    {
        var wic = Path.Combine(Environment.SystemDirectory, "windowscodecs.dll");
        return new JsonObject
        {
            ["name"] = "environment",
            ["os_version"] = Environment.OSVersion.Version.ToString(),
            ["windowscodecs_version"] = File.Exists(wic) ? FileVersionInfo.GetVersionInfo(wic).FileVersion : null,
        };
    }

    private static string W411cPath(JsonObject c, string fixturesDir)
    {
        // "file": a fixture name; "path": a literal path (blank/odd cases); "missing": a name that does not exist in the fixture
        // dir; "missing_dir": a file under a directory that does not exist.
        if (c["path"] is { } literal) return literal.GetValue<string>();
        if (c["missing"] is { } missing) return Path.Combine(fixturesDir, missing.GetValue<string>());
        if (c["missing_dir"] is { } missingDir) return Path.Combine(fixturesDir, "no-such-dir", missingDir.GetValue<string>());
        return Path.Combine(fixturesDir, c["file"]!.GetValue<string>());
    }

    private static JsonArray W411cCalls(W411cRecordingReader reader) =>
        new(reader.Calls.Select(call => (JsonNode?)new JsonObject
        {
            ["file"] = call.File,
            ["priority"] = call.Priority.ToString(),
            ["buffer_size"] = call.BufferSize,
        }).ToArray());

    private static string W411cPixel(PixelBuffer px, int x, int y)
    {
        var row = px.GetRow(y);
        var o = x * 4;
        return $"{row[o]},{row[o + 1]},{row[o + 2]},{row[o + 3]}";
    }

    /// <summary>
    /// Cases {name, file|path|missing|missing_dir, box_w, box_h, apply_orientation=true, source_orientation=null,
    /// from_bytes=false, label=null (the request path when from_bytes; default the file path), priority="Viewer"}.
    /// Result per case: the inputs plus either "ok" {pixel_width, pixel_height, downscaled, orientation, original_width,
    /// original_height, estimated_bytes, backend, layout, stride, exif_encoded (hex of ExifSummaryCodec.Encode), camera_make,
    /// pixels_sha256 (rows of width*4 bytes), samples {tl,tr,bl,br,center} as "b,g,r,a"} or "error", plus "reader_calls".
    /// </summary>
    public static JsonArray WicDirectDecode(JsonObject inputs, string inputPath)
    {
        var fixturesDir = Path.GetFullPath(Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!,
            inputs["fixtures_dir"]!.GetValue<string>()));
        var encode = typeof(ExifSummary).Assembly.GetType("PhotoReview.Imaging.Metadata.ExifSummaryCodec")!
            .GetMethod("Encode", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)!;
        var result = new JsonArray { W411cEnvironment() };
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var row = (JsonObject)c.DeepClone();
            var reader = new W411cRecordingReader();
            var decoder = W411cDecoder(reader);
            var path = W411cPath(c, fixturesDir);
            var fromBytes = c["from_bytes"]?.GetValue<bool>() ?? false;
            // NOT `byte[]? -> ReadOnlyMemory<byte>?`: a null array converts to an EMPTY memory (HasValue true), not to null.
            ReadOnlyMemory<byte>? bytes = fromBytes ? new ReadOnlyMemory<byte>(File.ReadAllBytes(path)) : (ReadOnlyMemory<byte>?)null;
            var requestPath = c["label"]?.GetValue<string>() ?? path;
            var priority = Enum.Parse<SourceReadPriority>(c["priority"]?.GetValue<string>() ?? "Viewer");
            var request = new DecodeRequest(requestPath, new DecodeBox(c["box_w"]?.GetValue<int>() ?? 0, c["box_h"]?.GetValue<int>() ?? 0),
                applyOrientation: c["apply_orientation"]?.GetValue<bool>() ?? true, bytes: bytes, priority: priority,
                sourceOrientation: c["source_orientation"]?.GetValue<int>());
            try
            {
                var decoded = decoder.Decode(request);
                var px = (PixelBuffer)decoded.PlatformImage;
                using var sha = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
                for (var y = 0; y < px.Height; y++) sha.AppendData(px.GetRow(y)[..(px.Width * 4)]);
                int w = px.Width, h = px.Height;
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
                    ["exif_encoded"] = decoded.Exif is null ? null : Convert.ToHexString((byte[])encode.Invoke(null, [decoded.Exif])!).ToLowerInvariant(),
                    ["camera_make"] = decoded.Exif?.CameraMake,
                    ["pixels_sha256"] = Convert.ToHexString(sha.GetHashAndReset()).ToLowerInvariant(),
                    ["samples"] = new JsonObject
                    {
                        ["tl"] = W411cPixel(px, 0, 0),
                        ["tr"] = W411cPixel(px, w - 1, 0),
                        ["bl"] = W411cPixel(px, 0, h - 1),
                        ["br"] = W411cPixel(px, w - 1, h - 1),
                        ["center"] = W411cPixel(px, w / 2, h / 2),
                    },
                };
                px.Dispose();
            }
            catch (Exception ex)
            {
                row["error"] = W411cError(ex, fixturesDir);
            }
            row["reader_calls"] = W411cCalls(reader);
            result.Add(row);
        }
        return result;
    }

    /// <summary>Cases {name, file|path|missing|missing_dir}: "ok" {width, height, orientation, pixel_width, pixel_height} or "error", plus reader_calls.</summary>
    public static JsonArray WicDirectReadInfo(JsonObject inputs, string inputPath)
    {
        var fixturesDir = Path.GetFullPath(Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!,
            inputs["fixtures_dir"]!.GetValue<string>()));
        var result = new JsonArray { W411cEnvironment() };
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var row = (JsonObject)c.DeepClone();
            var reader = new W411cRecordingReader();
            try
            {
                var info = W411cDecoder(reader).ReadInfo(W411cPath(c, fixturesDir));
                row["ok"] = new JsonObject
                {
                    ["pixel_width"] = info.PixelWidth,
                    ["pixel_height"] = info.PixelHeight,
                    ["orientation"] = info.Orientation,
                    ["width"] = info.Width,
                    ["height"] = info.Height,
                };
            }
            catch (Exception ex)
            {
                row["error"] = W411cError(ex, fixturesDir);
            }
            row["reader_calls"] = W411cCalls(reader);
            result.Add(row);
        }
        return result;
    }
}
