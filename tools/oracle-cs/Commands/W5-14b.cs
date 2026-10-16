using System.Diagnostics;
using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Localization;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Metadata;
using PhotoReview.Imaging.Pixels;
using PhotoReview.Imaging.TurboJpeg;
using PhotoReview.Imaging.TurboJpeg.Native;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-14b
//     "turbojpeg-fixtures" => Commands.TurboJpegFixtures(inputs, inputPath),
//     "turbojpeg-pure" => Commands.TurboJpegPure(inputs),
//     "turbojpeg-decode" => Commands.TurboJpegDecode(inputs, inputPath),
//     "turbojpeg-read-info" => Commands.TurboJpegReadInfo(inputs, inputPath),
//
// WIRING (the wiring step does this; the contract writer did NOT run it):
//   * Needs W4-11c.cs in the same partial class (W411cGradient, W411cJpeg, W411cJpegMeta are reused for the fixtures).
//   * tools/oracle.ps1 must also build src/PhotoReview.Imaging.TurboJpeg (Release) into the core folder, and OracleCs.csproj must
//     reference PhotoReview.Imaging.TurboJpeg.dll. turbojpeg.dll (libjpeg-turbo 3.x, the C# pin native/turbojpeg.sha256) must sit
//     next to oracle-cs.exe: run the archive's tools/fetch-native.ps1 (it only writes native/x64 inside the ARCHIVE copy, never the
//     C# repo) and copy native/x64/turbojpeg.dll into the oracle output folder. The C# assembly has
//     DefaultDllImportSearchPaths(AssemblyDirectory): the DLL must be in the folder of PhotoReview.Imaging.TurboJpeg.dll.
//   * Run order: `turbojpeg-fixtures` FIRST (writes tests/fixtures/turbojpeg/*, returns the manifest that becomes
//     tests/golden/turbojpeg-fixtures.json; commit the files: the WPF encoder is not bit-stable across Windows builds). Then
//     the three others. Run everything twice and compare byte for byte (D40).
//   * Internals are reached by reflection (MemoryInfo, ReadAllBytes, CalculateOutputBuffer) and by delegates bound to the
//     private/internal span methods (ScanHeader, TryReadSegment, HeaderNeedsMoreData): MethodInfo.Invoke cannot pass a span.
//   * The language stays the built-in English (Localizer.Current is never set): "user_text" is what EnglishTurboJpegTexts reproduces.
//   * Paths in messages are rewritten to "<fixtures>\..." and "<tmp>\..." (D19). MemoryInfo is injected per case (D19).

// W5-14b: TurboJpegDecoder (pure internals, real-native decode and read-info)
static partial class Commands
{
    private static readonly Type W514bType = typeof(TurboJpegDecoder);

    private delegate void W514bScanHeaderFn(ReadOnlySpan<byte> jpeg, bool wantIcc, bool wantExif, out bool hasIcc, out ReadOnlySpan<byte> exifTiff);
    private delegate bool W514bTryReadSegmentFn(ReadOnlySpan<byte> jpeg, ref int offset, out byte marker, out ReadOnlySpan<byte> payload, out bool needsMoreData);
    private delegate bool W514bNeedsMoreFn(ReadOnlySpan<byte> jpeg);

    private static MethodInfo W514bMethod(string name, params Type[] types) =>
        W514bType.GetMethod(name, BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.Static, types)
        ?? throw new InvalidOperationException("TurboJpegDecoder." + name + " not found (renamed at the baseline?)");

    private static readonly W514bScanHeaderFn W514bScanHeader = W514bMethod("ScanHeader",
            typeof(ReadOnlySpan<byte>), typeof(bool), typeof(bool), typeof(bool).MakeByRefType(), typeof(ReadOnlySpan<byte>).MakeByRefType())
        .CreateDelegate<W514bScanHeaderFn>();

    private static readonly W514bTryReadSegmentFn W514bTryReadSegment = W514bMethod("TryReadSegment",
            typeof(ReadOnlySpan<byte>), typeof(int).MakeByRefType(), typeof(byte).MakeByRefType(),
            typeof(ReadOnlySpan<byte>).MakeByRefType(), typeof(bool).MakeByRefType())
        .CreateDelegate<W514bTryReadSegmentFn>();

    private static readonly W514bNeedsMoreFn W514bHeaderNeedsMoreData = W514bMethod("HeaderNeedsMoreData", typeof(ReadOnlySpan<byte>))
        .CreateDelegate<W514bNeedsMoreFn>();

    private static object? W514bInvoke(MethodInfo method, params object?[] args)
    {
        try
        {
            return method.Invoke(null, args);
        }
        catch (TargetInvocationException ex) when (ex.InnerException is not null)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(ex.InnerException).Throw();
            throw;
        }
    }

    /// <summary>A bad golden input (unknown fn/transform/source) must stop the run, not become an "error" row.</summary>
    private static bool W514bIsHarnessError(Exception ex) =>
        ex is ArgumentException a && a.Message.StartsWith("unknown", StringComparison.Ordinal);

    private static string W514bSanitize(string text, string? fixturesDir, string? tmpDir)
    {
        if (fixturesDir is not null) text = text.Replace(fixturesDir, "<fixtures>", StringComparison.OrdinalIgnoreCase);
        if (tmpDir is not null) text = text.Replace(tmpDir, "<tmp>", StringComparison.OrdinalIgnoreCase);
        return text;
    }

    private static JsonObject W514bError(Exception ex, string? fixturesDir = null, string? tmpDir = null)
    {
        var row = new JsonObject
        {
            ["type"] = ex.GetType().FullName,
            ["message"] = W514bSanitize(ex.Message, fixturesDir, tmpDir),
            ["inner_type"] = ex.InnerException?.GetType().FullName,
            ["is_localized"] = UserFacingError.IsLocalized(ex),
            ["user_text"] = W514bSanitize(UserFacingError.Describe(ex), fixturesDir, tmpDir),
            ["fallbackable"] = FallbackImageDecoder.IsFallbackable(ex),
        };
        row["source_bytes_length"] = DecodeFailureSourceBytes.TryGet(ex, out var bytes) ? bytes.Length : null;
        return row;
    }

    private static JsonObject W514bEnvironment()
    {
        var wic = Path.Combine(Environment.SystemDirectory, "windowscodecs.dll");
        var tj = Path.Combine(Path.GetDirectoryName(W514bType.Assembly.Location)!, "turbojpeg.dll");
        return new JsonObject
        {
            ["name"] = "environment",
            ["os_version"] = Environment.OSVersion.Version.ToString(),
            ["windowscodecs_version"] = File.Exists(wic) ? FileVersionInfo.GetVersionInfo(wic).FileVersion : null,
            ["turbojpeg_sha256"] = File.Exists(tj) ? Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(tj))) : null,
            ["turbojpeg_version"] = File.Exists(tj) ? FileVersionInfo.GetVersionInfo(tj).FileVersion : null,
        };
    }

    // ---- turbojpeg-fixtures -----------------------------------------------------------------------------------------

    private static ulong W514bPack(uint numerator, uint denominator) => ((ulong)denominator << 32) | numerator;

    /// <summary>Replica of ExifTestData.EncodeJpegWithExif (q90, full camera, orientation tag only when != 1).</summary>
    private static byte[] W514bExifJpeg(int width, int height, bool withExif, ushort orientation)
    {
        System.Windows.Media.Imaging.BitmapMetadata? meta = null;
        if (withExif)
        {
            meta = new System.Windows.Media.Imaging.BitmapMetadata("jpg");
            meta.SetQuery("/app1/ifd/{ushort=271}", "Canon");
            meta.SetQuery("/app1/ifd/{ushort=272}", "Canon EOS R5");
            meta.SetQuery("/app1/ifd/{ushort=306}", "2024:06:30 09:00:00");
            if (orientation != 1) meta.SetQuery("/app1/ifd/{ushort=274}", orientation);
            meta.SetQuery("/app1/ifd/exif/{ushort=33434}", W514bPack(1, 250));
            meta.SetQuery("/app1/ifd/exif/{ushort=33437}", W514bPack(28, 10));
            meta.SetQuery("/app1/ifd/exif/{ushort=34855}", (ushort)400);
            meta.SetQuery("/app1/ifd/exif/{ushort=36867}", "2024:05:01 14:03:22");
            meta.SetQuery("/app1/ifd/exif/{ushort=37386}", W514bPack(50, 1));
            meta.SetQuery("/app1/ifd/exif/{ushort=42036}", "RF24-70mm F2.8 L IS USM");
        }
        return W411cJpeg(W411cGradient(width, height), 90, meta);
    }

    /// <summary>Writes tests/fixtures/turbojpeg/* and reports {file, length, sha256, what}.</summary>
    public static JsonArray TurboJpegFixtures(JsonObject inputs, string inputPath)
    {
        var baseDir = Path.GetDirectoryName(Path.GetFullPath(inputPath))!;
        var dir = Path.GetFullPath(Path.Combine(baseDir, inputs["output_dir"]!.GetValue<string>()));
        Directory.CreateDirectory(dir);
        var files = new List<(string Name, byte[] Bytes, string What)>();

        // TurboJpegPixelBufferParityTests.Sizes: orientation tag 1 present so the tests can patch it to 2..8 (op set_orientation).
        foreach (var size in inputs["parity_sizes"]!.AsArray())
        {
            var w = size![0]!.GetValue<int>();
            var h = size[1]!.GetValue<int>();
            files.Add(($"parity-{w}x{h}.jpg", W411cJpeg(W411cGradient(w, h), 90, W411cJpegMeta(("/app1/ifd/{ushort=274}", (ushort)1))),
                $"GenerateJpegWithOrientation({w},{h},1) q90"));
        }
        files.Add(("tall-10x1000.jpg", W411cJpeg(W411cGradient(10, 1000)), "GenerateGradientJpeg(10,1000): Downscaled_WhenTheBoxLimitsTheHeight"));
        files.Add(("gradient-256x256.jpg", W411cJpeg(W411cGradient(256, 256)), "GenerateGradientJpeg(256,256): CorruptJpeg_DoesNotLeakPixelBuffer"));
        files.Add(("small-q85-64x48.jpg", W411cJpeg(W411cGradient(64, 48), 85), "SmallJpeg() of the guard/hardening tests (q85, no metadata)"));
        files.Add(("exif-24x16-o6.jpg", W514bExifJpeg(24, 16, true, 6), "ExifTestData.EncodeJpegWithExif(24,16,orientation 6)"));
        files.Add(("exif-64x48-o1.jpg", W514bExifJpeg(64, 48, true, 1), "ExifTestData.EncodeJpegWithExif() full camera"));
        files.Add(("exif-64x48-o6.jpg", W514bExifJpeg(64, 48, true, 6), "ExifTestData.EncodeJpegWithExif(64,48,orientation 6)"));
        files.Add(("noexif-64x48.jpg", W514bExifJpeg(64, 48, false, 1), "ExifTestData.EncodeJpegWithExif(withExif: false)"));
        var partial = new System.Windows.Media.Imaging.BitmapMetadata("jpg");
        partial.SetQuery("/app1/ifd/{ushort=271}", "FUJIFILM");
        partial.SetQuery("/app1/ifd/exif/{ushort=34855}", (ushort)3200);
        files.Add(("exif-partial-64x48.jpg", W411cJpeg(W411cGradient(64, 48), 90, partial), "ExifTestData.EncodeJpegWithPartialExif()"));

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

    // ---- turbojpeg-pure ---------------------------------------------------------------------------------------------

    private sealed class W514bStream(byte[] content, long reportedLength, int chunk) : Stream
    {
        private readonly MemoryStream _inner = new(content);
        public int ReadCalls { get; private set; }
        public override bool CanRead => true;
        public override bool CanSeek => true;
        public override bool CanWrite => false;
        public override long Length => reportedLength;
        public override long Position { get => _inner.Position; set => _inner.Position = value; }
        public override void Flush() { }
        public override int Read(byte[] buffer, int offset, int count) => Read(buffer.AsSpan(offset, count));
        public override int Read(Span<byte> buffer)
        {
            ReadCalls++;
            return _inner.Read(chunk > 0 && buffer.Length > chunk ? buffer[..chunk] : buffer);
        }
        public override long Seek(long offset, SeekOrigin origin) => _inner.Seek(offset, origin);
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }

    private static JsonObject W514bScan(byte[] jpeg, bool wantIcc, bool wantExif)
    {
        W514bScanHeader(jpeg, wantIcc, wantExif, out var hasIcc, out var tiff);
        var start = tiff.IsEmpty ? -1 : (int)Unsafe_Offset(jpeg, tiff);
        return new JsonObject { ["has_icc"] = hasIcc, ["exif_tiff_offset"] = start, ["exif_tiff_length"] = tiff.Length };
    }

    /// <summary>Offset of <paramref name="slice"/> inside <paramref name="whole"/> (the ScanHeader result is a sub-span of the input).</summary>
    private static unsafe long Unsafe_Offset(byte[] whole, ReadOnlySpan<byte> slice)
    {
        fixed (byte* a = whole)
        fixed (byte* b = slice)
            return b - a;
    }

    private static JsonObject W514bWalk(byte[] jpeg)
    {
        var segments = new JsonArray();
        var offset = 2;
        var needsMore = false;
        for (var guard = 0; guard < 100_000; guard++)
        {
            var before = offset;
            if (!W514bTryReadSegment(jpeg, ref offset, out var marker, out var payload, out needsMore))
                break;
            segments.Add(new JsonObject
            {
                ["marker"] = marker,
                ["payload_offset"] = payload.IsEmpty ? offset - 0 : (int)Unsafe_Offset(jpeg, payload),
                ["payload_length"] = payload.Length,
                ["offset_after"] = offset,
            });
            if (offset == before) throw new InvalidOperationException("walker did not advance");
        }
        return new JsonObject { ["segments"] = segments, ["end_offset"] = offset, ["needs_more_data"] = needsMore };
    }

    /// <summary>
    /// Cases {name, fn, ...}; fn = select-scaling-factor {orig_width, orig_height, target_width, target_height} |
    /// calculate-output-buffer {orig_width, orig_height, num, denom} | scan-header {hex, want_icc, want_exif} |
    /// read-exif-orientation {hex} | header-needs-more-data {hex} | walk-segments {hex} | all-walkers {hex} (all of the previous
    /// four, scan-header with the four want combinations) | read-all-bytes {content_length, reported_length, chunk}.
    /// First row "constants". An empty payload has no address: its payload_offset is reported as offset_after (ignore it then).
    /// </summary>
    public static JsonArray TurboJpegPure(JsonObject inputs)
    {
        var result = new JsonArray
        {
            new JsonObject
            {
                ["name"] = "constants",
                ["max_progressive_scans"] = (int)W514bType.GetField("MaxProgressiveScans", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null)!,
                ["header_read_chunk"] = (int)W514bType.GetField("HeaderReadChunk", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null)!,
                ["header_read_cap"] = (int)W514bType.GetField("HeaderReadCap", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null)!,
                // internal consts: the oracle assembly has no InternalsVisibleTo (only PhotoReview.Imaging.Tests does).
                ["extended_header_cap"] = (int)W514bType.GetField("ExtendedHeaderCap", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null)!,
                ["max_source_bytes"] = (long)W514bType.GetField("MaxSourceBytes", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null)!,
                ["guard_threshold_bytes"] = MemoryHeadroom.GuardThresholdBytes,
                ["supported_scaling_factors"] = new JsonArray(((TjScalingFactor[])W514bType
                    .GetField("SupportedScalingFactors", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null)!)
                    .Select(f => (JsonNode?)new JsonObject { ["num"] = f.Num, ["denom"] = f.Denom }).ToArray()),
            },
        };
        var calculate = W514bMethod("CalculateOutputBuffer", typeof(int), typeof(int), typeof(TjScalingFactor));
        var readAll = W514bMethod("ReadAllBytes", typeof(Stream));

        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var fn = c["fn"]!.GetValue<string>();
            var row = (JsonObject)c.DeepClone();
            byte[] Bytes() => Convert.FromHexString(c["hex"]!.GetValue<string>());
            try
            {
                switch (fn)
                {
                    case "select-scaling-factor":
                    {
                        var f = TurboJpegDecoder.SelectScalingFactor(c["orig_width"]!.GetValue<int>(), c["orig_height"]!.GetValue<int>(),
                            c["target_width"]!.GetValue<int>(), c["target_height"]!.GetValue<int>());
                        row["ok"] = new JsonObject { ["num"] = f.Num, ["denom"] = f.Denom };
                        break;
                    }
                    case "calculate-output-buffer":
                    {
                        var t = ((int, int, int, int))W514bInvoke(calculate, c["orig_width"]!.GetValue<int>(), c["orig_height"]!.GetValue<int>(),
                            new TjScalingFactor(c["num"]!.GetValue<int>(), c["denom"]!.GetValue<int>()))!;
                        row["ok"] = new JsonObject { ["width"] = t.Item1, ["height"] = t.Item2, ["stride"] = t.Item3, ["buffer_length"] = t.Item4 };
                        break;
                    }
                    case "scan-header":
                        row["ok"] = W514bScan(Bytes(), c["want_icc"]!.GetValue<bool>(), c["want_exif"]!.GetValue<bool>());
                        break;
                    case "read-exif-orientation":
                        row["ok"] = TurboJpegDecoder.ReadExifOrientation(Bytes());
                        break;
                    case "header-needs-more-data":
                        row["ok"] = W514bHeaderNeedsMoreData(Bytes());
                        break;
                    case "walk-segments":
                        row["ok"] = W514bWalk(Bytes());
                        break;
                    case "all-walkers":
                    {
                        var b = Bytes();
                        row["ok"] = new JsonObject
                        {
                            ["scan_00"] = W514bScan(b, false, false),
                            ["scan_10"] = W514bScan(b, true, false),
                            ["scan_01"] = W514bScan(b, false, true),
                            ["scan_11"] = W514bScan(b, true, true),
                            ["orientation"] = TurboJpegDecoder.ReadExifOrientation(b),
                            ["needs_more_data"] = W514bHeaderNeedsMoreData(b),
                            ["walk"] = W514bWalk(b),
                        };
                        break;
                    }
                    case "read-all-bytes":
                    {
                        var content = new byte[c["content_length"]!.GetValue<int>()];
                        for (var i = 0; i < content.Length; i++) content[i] = (byte)(i * 7 + 3);
                        using var stream = new W514bStream(content, c["reported_length"]!.GetValue<long>(), c["chunk"]!.GetValue<int>());
                        try
                        {
                            var bytes = (byte[])W514bInvoke(readAll, stream)!;
                            row["ok"] = new JsonObject
                            {
                                ["length"] = bytes.Length,
                                ["sha256"] = Convert.ToHexString(SHA256.HashData(bytes)),
                            };
                        }
                        finally
                        {
                            row["read_calls"] = stream.ReadCalls;
                        }
                        break;
                    }
                    default:
                        throw new ArgumentException("unknown fn " + fn);
                }
            }
            catch (Exception ex) when (!W514bIsHarnessError(ex))
            {
                row["error"] = W514bError(ex);
            }
            result.Add(row);
        }
        return result;
    }

    // ---- shared byte transforms (the Rust tests implement the same list; see contract "golden package") -------------

    private static byte[] W514bSegment(byte marker, byte[] payload)
    {
        var length = payload.Length + 2;
        return [0xFF, marker, (byte)(length >> 8), (byte)length, .. payload];
    }

    private static int W514bFindSos(byte[] jpeg)
    {
        for (var i = 2; i + 3 < jpeg.Length;)
        {
            if (jpeg[i] != 0xFF) throw new InvalidOperationException("marker walk");
            if (jpeg[i + 1] == 0xDA) return i;
            i += 2 + ((jpeg[i + 2] << 8) | jpeg[i + 3]);
        }
        throw new InvalidOperationException("no SOS");
    }

    private static void W514bSetOrientation(byte[] jpeg, int value)
    {
        for (var i = 2; i + 3 < jpeg.Length;)
        {
            if (jpeg[i] != 0xFF) break;
            var marker = jpeg[i + 1];
            var length = (jpeg[i + 2] << 8) | jpeg[i + 3];
            if (marker == 0xE1 && length >= 8 && jpeg.AsSpan(i + 4, 6).SequenceEqual("Exif\0\0"u8))
            {
                var t = i + 10;
                var little = jpeg[t] == 0x49;
                int U16(int at) => little ? jpeg[at] | (jpeg[at + 1] << 8) : (jpeg[at] << 8) | jpeg[at + 1];
                int U32(int at) => little ? U16(at) | (U16(at + 2) << 16) : (U16(at) << 16) | U16(at + 2);
                var ifd = t + U32(t + 4);
                var count = U16(ifd);
                for (var e = 0; e < count; e++)
                {
                    var entry = ifd + 2 + e * 12;
                    if (U16(entry) != 0x0112) continue;
                    if (little) { jpeg[entry + 8] = (byte)value; jpeg[entry + 9] = (byte)(value >> 8); }
                    else { jpeg[entry + 8] = (byte)(value >> 8); jpeg[entry + 9] = (byte)value; }
                    return;
                }
                throw new InvalidOperationException("no orientation entry in IFD0");
            }
            i += 2 + length;
        }
        throw new InvalidOperationException("no Exif APP1");
    }

    /// <summary>TurboJpegHardeningTests.ProgressiveGray: 8x8 gray progressive, 64 scans at 0 refine levels, 512 at 7.</summary>
    private static byte[] W514bProgressiveGray(int refineLevels)
    {
        var output = new List<byte> { 0xFF, 0xD8 };
        output.AddRange(W514bSegment(0xDB, [0, .. Enumerable.Repeat((byte)1, 64)]));
        output.AddRange(W514bSegment(0xC2, [8, 0, 8, 0, 8, 1, 1, 0x11, 0]));
        byte[] counts = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        output.AddRange(W514bSegment(0xC4, [0x00, .. counts, 0x00]));
        output.AddRange(W514bSegment(0xC4, [0x10, .. counts, 0x00]));
        void Scan(int ss, int se, int ah, int al)
        {
            output.AddRange(W514bSegment(0xDA, [1, 1, 0x00, (byte)ss, (byte)se, (byte)((ah << 4) | al)]));
            output.Add(0x7F);
        }
        void Coefficient(int k)
        {
            Scan(k, k, 0, refineLevels);
            for (var ah = refineLevels; ah >= 1; ah--) Scan(k, k, ah, ah - 1);
        }
        Coefficient(0);
        for (var k = 1; k <= 63; k++) Coefficient(k);
        output.AddRange([0xFF, 0xD9]);
        return [.. output];
    }

    private static byte[] W514bApply(byte[] input, JsonArray? transforms)
    {
        var jpeg = (byte[])input.Clone();
        if (transforms is null) return jpeg;
        foreach (var node in transforms)
        {
            var t = node!.AsObject();
            switch (t["op"]!.GetValue<string>())
            {
                case "claim_size":
                {
                    var (w, h) = (t["width"]!.GetValue<int>(), t["height"]!.GetValue<int>());
                    var done = false;
                    for (var i = 2; i + 9 < jpeg.Length && !done; i++)
                    {
                        if (jpeg[i] != 0xFF || (jpeg[i + 1] != 0xC0 && jpeg[i + 1] != 0xC2)) continue;
                        jpeg[i + 5] = (byte)(h >> 8); jpeg[i + 6] = (byte)h; jpeg[i + 7] = (byte)(w >> 8); jpeg[i + 8] = (byte)w;
                        done = true;
                    }
                    if (!done) throw new InvalidOperationException("no SOF");
                    break;
                }
                case "bad_precision":
                {
                    var done = false;
                    for (var i = 2; i + 9 < jpeg.Length && !done; i++)
                    {
                        if (jpeg[i] != 0xFF || jpeg[i + 1] != 0xC0) continue;
                        jpeg[i + 4] = 3;
                        done = true;
                    }
                    if (!done) throw new InvalidOperationException("no SOF0");
                    break;
                }
                case "truncate":
                    jpeg = jpeg.AsSpan(0, (int)((long)jpeg.Length * t["num"]!.GetValue<int>() / t["den"]!.GetValue<int>())).ToArray();
                    break;
                case "set_orientation":
                    W514bSetOrientation(jpeg, t["value"]!.GetValue<int>());
                    break;
                case "destroy_entropy_from_half":
                    for (var i = jpeg.Length / 2; i < jpeg.Length - 2; i++) jpeg[i] = 0x55;
                    break;
                case "lead_com":
                {
                    var padded = new List<byte> { 0xFF, 0xD8 };
                    for (var i = 0; i < t["count"]!.GetValue<int>(); i++) padded.AddRange(W514bSegment(0xFE, new byte[t["size"]!.GetValue<int>()]));
                    padded.AddRange(jpeg.AsSpan(2).ToArray());
                    jpeg = [.. padded];
                    break;
                }
                case "pad_com_before_sos":
                {
                    // TurboJpegGuardMutationTests.JpegWithSosCutByTheFirstRead: SOS starts at 65536 - k.
                    var sos = W514bFindSos(jpeg);
                    var comTotal = 65536 - t["sos_bytes_in_first_chunk"]!.GetValue<int>() - sos;
                    jpeg = [.. jpeg.AsSpan(0, 2).ToArray(), .. W514bSegment(0xFE, new byte[comTotal - 4]), .. jpeg.AsSpan(2).ToArray()];
                    break;
                }
                case "sos_at":
                {
                    // LargeHeaderReadInfoTests.ReadInfo_SosSegmentStraddlesTheFirstReadChunk_StillParses.
                    var sos = W514bFindSos(jpeg);
                    var pad = t["offset"]!.GetValue<int>() - sos - 4;
                    jpeg = [.. jpeg.AsSpan(0, 2).ToArray(), .. W514bSegment(0xFE, new byte[pad]), .. jpeg.AsSpan(2).ToArray()];
                    break;
                }
                case "pad_com_bytes":
                {
                    // TurboJpegSharingAndHeaderGrowthGapTests.PaddedFile: COM segments (payload bytes each) until >= bytes written.
                    var payload = t["payload"]!.GetValue<int>();
                    var segment = W514bSegment(0xFE, new byte[payload]);
                    var output = new List<byte> { 0xFF, 0xD8 };
                    for (long written = 0; written < t["bytes"]!.GetValue<long>(); written += segment.Length) output.AddRange(segment);
                    if (t["keep_tail"]?.GetValue<bool>() ?? true) output.AddRange(jpeg.AsSpan(2).ToArray());
                    jpeg = [.. output];
                    break;
                }
                default:
                    throw new ArgumentException("unknown transform " + t["op"]);
            }
        }
        return jpeg;
    }

    /// <summary>Resolves a case source to (path for the decoder, bytes or null). Writes transformed/synthetic bytes to tmp.</summary>
    private static (string? Path, byte[]? Bytes) W514bSource(JsonObject c, string fixturesDir, string tmpDir, bool needFile)
    {
        var name = c["name"]!.GetValue<string>();
        var s = c["source"] as JsonObject;
        if (s is null) return (null, null);
        if (s["missing"] is { } missing) return (Path.Combine(tmpDir, missing.GetValue<string>()), null);
        if (s["missing_dir"] is { } missingDir) return (Path.Combine(tmpDir, "no-such-dir", missingDir.GetValue<string>()), null);
        if (s["path"] is { } literal) return (literal.GetValue<string>(), null);
        if (s["directory"] is not null) return (tmpDir, null);
        if (s["zero_file"] is { } zeroLength)
        {
            var zeroPath = Path.Combine(tmpDir, name + ".jpg");
            using (var fs = new FileStream(zeroPath, FileMode.CreateNew, FileAccess.Write)) fs.SetLength(zeroLength.GetValue<long>());
            return (zeroPath, null);
        }

        byte[] bytes;
        string? fixturePath = null;
        if (s["fixture"] is { } fixture)
        {
            fixturePath = Path.Combine(fixturesDir, fixture.GetValue<string>().Replace('/', '\\'));
            bytes = File.ReadAllBytes(fixturePath);
        }
        else if (s["hex"] is { } h) bytes = Convert.FromHexString(h.GetValue<string>());
        else if (s["progressive_gray"] is { } levels) bytes = W514bProgressiveGray(levels.GetValue<int>());
        else throw new ArgumentException("unknown source in " + name);

        var transforms = s["transforms"] as JsonArray;
        if (transforms is not null) bytes = W514bApply(bytes, transforms);
        if (!needFile) return (null, bytes);
        if (fixturePath is not null && transforms is null) return (fixturePath, null);
        var path = Path.Combine(tmpDir, name + ".jpg");
        File.WriteAllBytes(path, bytes);
        return (path, null);
    }

    private static (TurboJpegDecoder Decoder, Func<int> Consulted) W514bDecoder(JsonObject? memory)
    {
        var decoder = new TurboJpegDecoder(PixelBufferImageCodec.Instance);
        var consulted = 0;
        var total = memory?["total"]?.GetValue<long>() ?? 64L * 1024 * 1024 * 1024;
        var load = memory?["load"]?.GetValue<long>() ?? 0L;
        Func<(long TotalAvailable, long Load)> info = () => { consulted++; return (total, load); };
        W514bType.GetProperty("MemoryInfo", BindingFlags.NonPublic | BindingFlags.Instance)!.SetValue(decoder, info);
        return (decoder, () => consulted);
    }

    private static JsonObject W514bImage(IDecodedImage image, bool fineScale)
    {
        var buffer = (PixelBuffer)image.PlatformImage;
        using var sha = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var xAllFf = true;
        var bgr = new byte[buffer.Width * 3];
        for (var y = 0; y < buffer.Height; y++)
        {
            var row = buffer.GetRow(y);
            for (var x = 0; x < buffer.Width; x++)
            {
                bgr[x * 3] = row[x * 4];
                bgr[x * 3 + 1] = row[x * 4 + 1];
                bgr[x * 3 + 2] = row[x * 4 + 2];
                xAllFf &= row[x * 4 + 3] == 0xFF;
            }
            sha.AppendData(bgr);
        }
        var samples = new JsonArray();
        for (var sy = 0; sy < 4; sy++)
            for (var sx = 0; sx < 4; sx++)
            {
                var x = (2 * sx + 1) * buffer.Width / 8;
                var y = (2 * sy + 1) * buffer.Height / 8;
                var row = buffer.GetRow(y);
                samples.Add(new JsonArray(x, y, row[x * 4], row[x * 4 + 1], row[x * 4 + 2]));
            }
        var result = new JsonObject
        {
            ["pixel_width"] = image.PixelWidth,
            ["pixel_height"] = image.PixelHeight,
            ["buffer_layout"] = buffer.Layout.ToString(),
            ["estimated_bytes"] = image.EstimatedBytes,
            ["downscaled"] = image.Downscaled,
            ["orientation"] = image.Orientation,
            ["actual_backend"] = image.ActualBackend.ToString(),
            ["original_width"] = image.OriginalWidth,
            ["original_height"] = image.OriginalHeight,
            ["exif_null"] = image.Exif is null,
            // ExifSummaryCodec is internal to PhotoReview.Imaging: reached by reflection as in W4-04a/W4-11c.
            ["exif_encoded"] = Convert.ToHexString((byte[])typeof(ExifSummary).Assembly
                .GetType("PhotoReview.Imaging.Metadata.ExifSummaryCodec", throwOnError: true)!
                .GetMethod("Encode", BindingFlags.Public | BindingFlags.Static)!.Invoke(null, [image.Exif])!),
            ["bgr_sha256"] = Convert.ToHexString(sha.GetHashAndReset()),
            ["x_bytes_all_ff"] = fineScale ? null : xAllFf,
            ["samples"] = samples,
        };
        buffer.Dispose();
        return result;
    }

    /// <summary>
    /// Cases {name, source, from: file|bytes|none, path (bytes mode label, "" = null), box [w,h], apply_orientation,
    /// source_orientation?, memory {total, load}, fine_scale?}. First row "environment". Reports ok (image facts) or error,
    /// plus "consulted" (MemoryInfo calls).
    /// </summary>
    public static JsonArray TurboJpegDecode(JsonObject inputs, string inputPath)
    {
        var fixturesDir = Path.GetFullPath(Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!,
            inputs["fixtures_dir"]!.GetValue<string>()));
        var tmpDir = Path.Combine(Path.GetTempPath(), "pr-oracle-w514b-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(tmpDir);
        var result = new JsonArray { W514bEnvironment() };
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var row = (JsonObject)c.DeepClone();
                var from = c["from"]!.GetValue<string>();
                var (decoder, consulted) = W514bDecoder(c["memory"] as JsonObject);
                try
                {
                    var (path, bytes) = W514bSource(c, fixturesDir, tmpDir, needFile: from == "file");
                    if (from != "file")
                    {
                        var label = c["path"]!.GetValue<string>();
                        path = label.Length == 0 ? null : label;
                    }
                    var box = c["box"]!.AsArray();
                    var request = new DecodeRequest(path!, new DecodeBox(box[0]!.GetValue<int>(), box[1]!.GetValue<int>()),
                        c["apply_orientation"]!.GetValue<bool>(),
                        bytes is null ? null : new ReadOnlyMemory<byte>(bytes),
                        SourceReadPriority.Viewer,
                        c["source_orientation"]?.GetValue<int>());
                    var image = decoder.Decode(request);
                    row["ok"] = W514bImage(image, c["fine_scale"]?.GetValue<bool>() ?? false);
                }
                catch (Exception ex) when (!W514bIsHarnessError(ex))
                {
                    row["error"] = W514bError(ex, fixturesDir, tmpDir);
                }
                row["consulted"] = consulted();
                result.Add(row);
            }
        }
        finally
        {
            try { Directory.Delete(tmpDir, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        }
        return result;
    }

    /// <summary>Cases {name, source}: TurboJpegDecoder.ReadInfo(path) (always a file). Reports ok {pixel_width, pixel_height, orientation, width, height} or error.</summary>
    public static JsonArray TurboJpegReadInfo(JsonObject inputs, string inputPath)
    {
        var fixturesDir = Path.GetFullPath(Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!,
            inputs["fixtures_dir"]!.GetValue<string>()));
        var tmpDir = Path.Combine(Path.GetTempPath(), "pr-oracle-w514b-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(tmpDir);
        var result = new JsonArray { W514bEnvironment() };
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var row = (JsonObject)c.DeepClone();
                var (decoder, consulted) = W514bDecoder(null);
                try
                {
                    var (path, _) = W514bSource(c, fixturesDir, tmpDir, needFile: true);
                    var info = decoder.ReadInfo(path!);
                    row["ok"] = new JsonObject
                    {
                        ["pixel_width"] = info.PixelWidth,
                        ["pixel_height"] = info.PixelHeight,
                        ["orientation"] = info.Orientation,
                        ["width"] = info.Width,
                        ["height"] = info.Height,
                    };
                }
                catch (Exception ex) when (!W514bIsHarnessError(ex))
                {
                    row["error"] = W514bError(ex, fixturesDir, tmpDir);
                }
                row["consulted"] = consulted();
                result.Add(row);
            }
        }
        finally
        {
            try { Directory.Delete(tmpDir, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        }
        return result;
    }
}
