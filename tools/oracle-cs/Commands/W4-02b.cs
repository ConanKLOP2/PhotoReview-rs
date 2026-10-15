using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Pixels;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-02b
//     "embedded-thumbnail-size" => Commands.EmbeddedThumbnailSize(inputs),
//     "embedded-thumbnail-fixtures" => Commands.EmbeddedThumbnailFixtures(inputs, inputPath),
//     "embedded-thumbnail-read" => Commands.EmbeddedThumbnailRead(inputs, inputPath),
//
// WIRING (the wiring step does this; the contract writer did NOT run it):
//   * tools/oracle-cs/OracleCs.csproj: add `<UseWPF>true</UseWPF>` (fixture generation uses System.Windows.Media.Imaging, exactly
//     like tests/PhotoReview.TestSupport.Windows/Fixtures/EmbeddedThumbnailJpegFixture.cs) and
//     `<Reference Include="PhotoReview.Imaging"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.dll</HintPath></Reference>`
//     (tools/oracle.ps1 must copy the baseline PhotoReview.Imaging.dll next to PhotoReview.Core.dll).
//   * Run order: `embedded-thumbnail-fixtures` FIRST (writes tests/fixtures/embedded-thumbnail/*.jpg|png and a manifest golden),
//     commit those files, then `embedded-thumbnail-read` reads them. The JPEG encoder output is not bit-stable across Windows
//     builds, which is why the files are committed and the golden is pinned to their bytes (manifest has sha256).
//
// The real WIC (no WPF type in the reader since WP-03) runs here: TryRead(path, PixelBufferImageCodec.Instance).

static partial class Commands
{
    private const string W402bBackendNote = "TryRead labels every result DecoderBackend.Wpf (cache identity), never WicDirect.";

    private static string? W402bStr(JsonNode? n) => n is null ? null : n.GetValue<string>();

    // ---- embedded-thumbnail-size --------------------------------------------------------------------------------

    /// <summary>EmbeddedThumbnailReader.IsAcceptableSize (internal: reached by reflection) for each {name, width, height}.</summary>
    public static JsonArray EmbeddedThumbnailSize(JsonObject inputs)
    {
        var type = typeof(EmbeddedThumbnailReader);
        var method = type.GetMethod("IsAcceptableSize", BindingFlags.NonPublic | BindingFlags.Static)
            ?? throw new InvalidOperationException("IsAcceptableSize not found");
        var maxField = type.GetField("MaxThumbnailPixels", BindingFlags.NonPublic | BindingFlags.Static)
            ?? throw new InvalidOperationException("MaxThumbnailPixels not found");
        var max = (long)maxField.GetValue(null)!;

        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var width = c["width"]!.GetValue<int>();
            var height = c["height"]!.GetValue<int>();
            var acceptable = (bool)method.Invoke(null, [width, height])!;
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["width"] = width,
                ["height"] = height,
                ["acceptable"] = acceptable,
                ["max_thumbnail_pixels"] = max,
            });
        }
        return result;
    }

    // ---- fixtures -----------------------------------------------------------------------------------------------

    private static System.Windows.Media.Imaging.BitmapSource W402bSolid(int w, int h, byte b, byte g, byte r)
    {
        var pixels = new byte[w * h * 4];
        for (var i = 0; i < pixels.Length; i += 4) { pixels[i] = b; pixels[i + 1] = g; pixels[i + 2] = r; pixels[i + 3] = 255; }
        var bmp = System.Windows.Media.Imaging.BitmapSource.Create(w, h, 96, 96, System.Windows.Media.PixelFormats.Bgra32, null, pixels, w * 4);
        bmp.Freeze();
        return bmp;
    }

    /// <summary>Four distinct quadrants (TL red, TR green, BL blue, BR white) so every EXIF orientation is visible in the pixels.</summary>
    private static System.Windows.Media.Imaging.BitmapSource W402bQuadrants(int w, int h)
    {
        var pixels = new byte[w * h * 4];
        for (var y = 0; y < h; y++)
        {
            for (var x = 0; x < w; x++)
            {
                var right = x >= w / 2;
                var bottom = y >= h / 2;
                byte b, g, r;
                if (!right && !bottom) (b, g, r) = (0, 0, 255);
                else if (right && !bottom) (b, g, r) = (0, 255, 0);
                else if (!right && bottom) (b, g, r) = (255, 0, 0);
                else (b, g, r) = (255, 255, 255);
                var o = ((y * w) + x) * 4;
                pixels[o] = b; pixels[o + 1] = g; pixels[o + 2] = r; pixels[o + 3] = 255;
            }
        }
        var bmp = System.Windows.Media.Imaging.BitmapSource.Create(w, h, 96, 96, System.Windows.Media.PixelFormats.Bgra32, null, pixels, w * 4);
        bmp.Freeze();
        return bmp;
    }

    private static byte[] W402bJpeg(System.Windows.Media.Imaging.BitmapSource main, System.Windows.Media.Imaging.BitmapSource? thumb, int? orientation)
    {
        System.Windows.Media.Imaging.BitmapMetadata? metadata = null;
        if (orientation.HasValue)
        {
            metadata = new System.Windows.Media.Imaging.BitmapMetadata("jpg");
            metadata.SetQuery("/app1/ifd/{ushort=274}", (ushort)orientation.Value);
        }
        var encoder = new System.Windows.Media.Imaging.JpegBitmapEncoder();
        encoder.Frames.Add(thumb is null
            ? System.Windows.Media.Imaging.BitmapFrame.Create(main, null, metadata, null)
            : System.Windows.Media.Imaging.BitmapFrame.Create(main, thumb, metadata, null));
        using var stream = new MemoryStream();
        encoder.Save(stream);
        return stream.ToArray();
    }

    /// <summary>Port of EmbeddedThumbnailReaderTests.WithClaimedThumbnailSize: patches the first SOF (inside the APP1 thumbnail).</summary>
    private static byte[] W402bClaimed(int width, int height)
    {
        var jpeg = W402bJpeg(W402bSolid(48, 48, 0, 0, 255), W402bSolid(16, 16, 255, 0, 0), null);
        for (var i = 2; i + 9 < jpeg.Length; i++)
        {
            if (jpeg[i] != 0xFF || (jpeg[i + 1] != 0xC0 && jpeg[i + 1] != 0xC2)) continue;
            jpeg[i + 5] = (byte)(height >> 8); jpeg[i + 6] = (byte)height;
            jpeg[i + 7] = (byte)(width >> 8); jpeg[i + 8] = (byte)width;
            return jpeg;
        }
        throw new InvalidOperationException("no SOF found");
    }

    /// <summary>Port of EmbeddedThumbnailReaderTests.WithRealThumbnail: a main JPEG whose APP1 carries a real solid-black thumbnail.</summary>
    private static byte[] W402bRealThumbnail(int side)
    {
        var pixels = new byte[side * side * 3];
        var bmp = System.Windows.Media.Imaging.BitmapSource.Create(side, side, 96, 96, System.Windows.Media.PixelFormats.Bgr24, null, pixels, side * 3);
        var encoder = new System.Windows.Media.Imaging.JpegBitmapEncoder { QualityLevel = 1 };
        encoder.Frames.Add(System.Windows.Media.Imaging.BitmapFrame.Create(bmp));
        using var thumbStream = new MemoryStream();
        encoder.Save(thumbStream);
        var thumb = thumbStream.ToArray();
        if (thumb.Length >= 60000) throw new InvalidOperationException("thumbnail must fit one APP1 segment");

        var seed = W402bJpeg(W402bSolid(480, 480, 0, 0, 255), W402bSolid(16, 16, 255, 0, 0), null);
        var app1 = 2;
        while (seed[app1 + 1] != 0xE1) app1 += 2 + ((seed[app1 + 2] << 8) | seed[app1 + 3]);
        var app1Length = (seed[app1 + 2] << 8) | seed[app1 + 3];
        var app1End = app1 + 2 + app1Length;
        var lengthEntry = -1;
        for (var i = app1 + 10; i + 12 <= app1End; i++)
        {
            if (seed[i] == 0x02 && seed[i + 1] == 0x02 && seed[i + 2] == 0 && seed[i + 3] == 4 && seed[i + 7] == 1) { lengthEntry = i; break; }
        }
        if (lengthEntry <= 0) throw new InvalidOperationException("no JPEGInterchangeFormatLength entry");
        var oldThumbLength = (seed[lengthEntry + 8] << 24) | (seed[lengthEntry + 9] << 16) | (seed[lengthEntry + 10] << 8) | seed[lengthEntry + 11];
        var oldThumbStart = app1End - oldThumbLength;
        if (!(seed[oldThumbStart] == 0xFF && seed[oldThumbStart + 1] == 0xD8)) throw new InvalidOperationException("thumbnail start not found");

        var output = new List<byte>(seed.AsSpan(0, oldThumbStart).ToArray());
        output.AddRange(thumb);
        output.AddRange(seed.AsSpan(app1End).ToArray());
        var newApp1Length = app1Length - oldThumbLength + thumb.Length;
        output[app1 + 2] = (byte)(newApp1Length >> 8); output[app1 + 3] = (byte)newApp1Length;
        output[lengthEntry + 8] = (byte)(thumb.Length >> 24); output[lengthEntry + 9] = (byte)(thumb.Length >> 16);
        output[lengthEntry + 10] = (byte)(thumb.Length >> 8); output[lengthEntry + 11] = (byte)thumb.Length;
        return [.. output];
    }

    private static byte[] W402bPng()
    {
        var encoder = new System.Windows.Media.Imaging.PngBitmapEncoder();
        encoder.Frames.Add(System.Windows.Media.Imaging.BitmapFrame.Create(W402bQuadrants(32, 24)));
        using var stream = new MemoryStream();
        encoder.Save(stream);
        return stream.ToArray();
    }

    /// <summary>
    /// Writes the fixture set to inputs.output_dir (relative to the inputs file) and reports {file, length, sha256, what}.
    /// Deterministic per run; the committed files are the source of truth afterwards.
    /// </summary>
    public static JsonArray EmbeddedThumbnailFixtures(JsonObject inputs, string inputPath)
    {
        var dir = Path.GetFullPath(Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!, inputs["output_dir"]!.GetValue<string>()));
        Directory.CreateDirectory(dir);
        var files = new List<(string Name, byte[] Bytes, string What)>();

        // Mirrors of the C# test fixtures.
        files.Add(("thumb-main480-thumb160.jpg", W402bJpeg(W402bSolid(480, 480, 0, 0, 255), W402bSolid(160, 160, 255, 0, 0), null),
            "EmbeddedThumbnailReaderTests.NormalThumbnail_StillDecodes (main 480 red, thumbnail 160 blue)"));
        files.Add(("plain-main64.jpg", W402bJpeg(W402bSolid(64, 64, 0, 255, 0), null, null),
            "EmbeddedThumbnailJpegFixture.CreateWithoutThumbnail"));
        files.Add(("bomb-claimed-20000.jpg", W402bClaimed(20000, 20000), "WithClaimedThumbnailSize(20000,20000)"));
        files.Add(("bomb-claimed-65535.jpg", W402bClaimed(65535, 65535), "WithClaimedThumbnailSize(65535,65535)"));
        foreach (var side in new[] { 160, 400, 480, 512, 600, 1100 })
            files.Add(($"real-thumb-{side}.jpg", W402bRealThumbnail(side), $"WithRealThumbnail({side}): hand-spliced solid thumbnail"));

        // Orientation matrix: main 48x32 (solid), thumbnail 16x8 with four distinct quadrants.
        var main = W402bSolid(48, 32, 128, 128, 128);
        var thumb = W402bQuadrants(16, 8);
        files.Add(("quad-no-orientation.jpg", W402bJpeg(main, thumb, null), "no orientation tag"));
        for (var o = 1; o <= 8; o++)
            files.Add(($"quad-orientation-{o}.jpg", W402bJpeg(main, thumb, o), $"EXIF orientation {o}"));
        files.Add(("quad-orientation-9.jpg", W402bJpeg(main, thumb, 9), "orientation out of range: treated as 1"));
        files.Add(("quad-orientation-0.jpg", W402bJpeg(main, thumb, 0), "orientation 0: treated as 1"));

        // Truncations of the orientation-6 file (the C# test cuts it at every length below 700, then every 9th byte).
        var o6 = files.First(f => f.Name == "quad-orientation-6.jpg").Bytes;
        var cuts = new (string Label, int Length)[]
        {
            ("0", 0), ("1", 1), ("2", 2), ("3", 3), ("4", 4), ("20", 20), ("100", 100), ("300", 300), ("600", 600),
            ("half", o6.Length / 2), ("minus1", o6.Length - 1),
        };
        foreach (var (label, len) in cuts)
            files.Add(($"truncated-o6-{label}.jpg", o6.AsSpan(0, len).ToArray(), $"quad-orientation-6.jpg cut to {len} bytes"));

        // Not images.
        files.Add(("not-jpeg.png", W402bPng(), "PNG (no EXIF thumbnail)"));
        files.Add(("garbage.jpg", [.. Enumerable.Range(0, 400).Select(i => (byte)((i * 31) + 7))], "400 bytes of noise"));
        files.Add(("text.jpg", System.Text.Encoding.ASCII.GetBytes("this is not a picture\r\n"), "text file named .jpg"));
        files.Add(("empty.jpg", [], "zero bytes"));

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

    // ---- embedded-thumbnail-read --------------------------------------------------------------------------------

    private static string W402bSample(PixelBuffer px, int x, int y)
    {
        var row = px.GetRow(y);
        var o = x * 4;
        return $"{row[o]},{row[o + 1]},{row[o + 2]},{row[o + 3]}";
    }

    /// <summary>
    /// EmbeddedThumbnailReader.TryRead for each case {name, file, kind}. kind: "file" (default; file under inputs.fixture_dir),
    /// "missing" (a path that does not exist), "directory" (the fixture directory itself).
    /// Result: {name, file, kind, result: null | {width, height, layout, estimated_bytes, downscaled, orientation, original_width,
    /// original_height, backend, pixels_sha256, corners:[tl,tr,bl,br as "B,G,R,A"], center}}.
    /// </summary>
    public static JsonArray EmbeddedThumbnailRead(JsonObject inputs, string inputPath)
    {
        var baseDir = Path.GetDirectoryName(Path.GetFullPath(inputPath))!;
        var fixtureDir = Path.GetFullPath(Path.Combine(baseDir, inputs["fixture_dir"]!.GetValue<string>()));
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = W402bStr(c["kind"]) ?? "file";
            var file = W402bStr(c["file"]);
            var path = kind switch
            {
                "directory" => fixtureDir,
                "missing" => Path.Combine(fixtureDir, "does-not-exist-" + (file ?? "x.jpg")),
                _ => Path.Combine(fixtureDir, file!),
            };

            JsonNode? outcome;
            var image = EmbeddedThumbnailReader.TryRead(path, PixelBufferImageCodec.Instance);
            if (image is null)
            {
                outcome = null;
            }
            else
            {
                using var lease = PixelBufferImageCodec.Instance.ToPixels(image.PlatformImage);
                var px = lease.Pixels;
                using var sha = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
                for (var y = 0; y < px.Height; y++) sha.AppendData(px.GetRow(y));
                outcome = new JsonObject
                {
                    ["width"] = image.PixelWidth,
                    ["height"] = image.PixelHeight,
                    ["layout"] = px.Layout.ToString(),
                    ["estimated_bytes"] = image.EstimatedBytes,
                    ["downscaled"] = image.Downscaled,
                    ["orientation"] = image.Orientation,
                    ["original_width"] = image.OriginalWidth,
                    ["original_height"] = image.OriginalHeight,
                    ["backend"] = image.ActualBackend.ToString(),
                    ["pixels_sha256"] = Convert.ToHexString(sha.GetHashAndReset()).ToLowerInvariant(),
                    ["corners"] = new JsonArray(
                        W402bSample(px, 0, 0), W402bSample(px, px.Width - 1, 0),
                        W402bSample(px, 0, px.Height - 1), W402bSample(px, px.Width - 1, px.Height - 1)),
                    ["center"] = W402bSample(px, px.Width / 2, px.Height / 2),
                };
            }

            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["file"] = file,
                ["kind"] = kind,
                ["note"] = W402bBackendNote,
                ["result"] = outcome,
            });
        }
        return result;
    }
}
