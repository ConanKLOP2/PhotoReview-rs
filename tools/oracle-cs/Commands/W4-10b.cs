using System.Globalization;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Caching;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Pixels;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-10b
//     "thumbnail-cache-key" => Commands.ThumbnailCacheKey(inputs),
//     "thumbnail-disk-decode" => Commands.ThumbnailDiskDecode(inputs),
//
// WIRING (the wiring step does this; the contract writer did NOT run it):
//   * tools/oracle-cs/OracleCs.csproj: `<Reference Include="PhotoReview.Imaging"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.dll</HintPath></Reference>`
//     (same line W4-02b needs; tools/oracle.ps1 copies the baseline PhotoReview.Imaging.dll next to PhotoReview.Core.dll).
//   * `thumbnail-cache-key` is pure (no file, no clock, no cwd: every input path is absolute, D19).
//   * `thumbnail-disk-decode` runs the real WIC (ThumbnailCache.DecodeDiskThumbnail with PixelBufferImageCodec, the Win32 build's
//     codec). It writes each case's bytes to a fresh temp file, decodes, deletes the file. Generate twice and diff (D27):
//     WIC's Fant scaler is deterministic on one Windows build; if two runner images ever disagree, drop `pixels_sha256` from the
//     Rust comparison and keep it as information only (contract OQ4).

// W4-10b: ThumbnailCache.BuildKey / DecodeDiskThumbnail (both private/internal statics: reached by reflection).
static partial class Commands
{
    private static readonly MethodInfo W410bBuildKey = typeof(ThumbnailCache).GetMethod(
            "BuildKey", BindingFlags.NonPublic | BindingFlags.Static, [typeof(string), typeof(long), typeof(DateTime)])
        ?? throw new InvalidOperationException("ThumbnailCache.BuildKey(string, long, DateTime) not found");

    private static readonly MethodInfo W410bDecodeDisk = typeof(ThumbnailCache).GetMethod(
            "DecodeDiskThumbnail", BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new InvalidOperationException("ThumbnailCache.DecodeDiskThumbnail not found");

    private static readonly MethodInfo W410bIsCacheEntryFailure = typeof(DiskCacheStore).GetMethod(
            "IsCacheEntryFailure", BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new InvalidOperationException("DiskCacheStore.IsCacheEntryFailure not found");

    /// <summary>
    /// For each {name, path, length, last_write_ticks, get_full_path}: the path GetAsync keys by (Path.GetFullPath when
    /// get_full_path, else the text as given), the stamp, the key (BuildKey) and the cache file name. The stamp is rebuilt here
    /// with the C# format and checked against the key, so a change of the private format fails the oracle instead of silently
    /// producing a golden with a stale stamp.
    /// </summary>
    public static JsonArray ThumbnailCacheKey(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var path = c["path"]!.GetValue<string>();
            var length = c["length"]!.GetValue<long>();
            var ticks = c["last_write_ticks"]!.GetValue<long>();
            var useFull = c["get_full_path"]!.GetValue<bool>();
            var keyed = useFull ? Path.GetFullPath(path) : path;
            var lastWriteUtc = new DateTime(ticks, DateTimeKind.Utc);
            var key = (string)W410bBuildKey.Invoke(null, [keyed, length, lastWriteUtc])!;

            var stamp = string.Create(CultureInfo.InvariantCulture,
                $"{keyed}|{length}|{lastWriteUtc.Ticks}|{ThumbnailCache.MaxThumbnailWidth}|orient=1");
            var rebuilt = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(stamp))).ToLowerInvariant();
            if (rebuilt != key) throw new InvalidOperationException($"{c["name"]}: BuildKey format changed (stamp '{stamp}')");

            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["path"] = path,
                ["length"] = length,
                ["last_write_ticks"] = ticks,
                ["get_full_path"] = useFull,
                ["keyed_path"] = keyed,
                ["stamp"] = stamp,
                ["key"] = key,
                ["file_name"] = key + ".png",
            });
        }
        return result;
    }

    /// <summary>
    /// For each {name, png_base64 | missing}: ThumbnailCache.DecodeDiskThumbnail(path, PixelBufferImageCodec.Instance) on a temp
    /// file holding those bytes. Success: the DecodedImage fields, the pixel layout and SHA-256 of the pixel rows (width * 4 bytes
    /// each, top-down). Failure: the exception type, the message (except for COMException, whose text is OS-localized: the HRESULT
    /// instead) and DiskCacheStore.IsCacheEntryFailure (true = ThumbnailCache deletes the entry and falls back to the source).
    /// </summary>
    public static JsonArray ThumbnailDiskDecode(JsonObject inputs)
    {
        var dir = Path.Combine(Path.GetTempPath(), "pr-oracle-thumbdisk-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        var result = new JsonArray();
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var name = c["name"]!.GetValue<string>();
                var missing = c["missing"]?.GetValue<bool>() ?? false;
                var fixture = c["fixture"]?.GetValue<string>();
                var path = Path.Combine(dir, name + ".png");
                if (fixture is not null) File.Copy(Path.Combine(PerfCommands.FixturesDir, fixture), path); // read-only tests/fixtures file
                else if (!missing) File.WriteAllBytes(path, Convert.FromBase64String(c["png_base64"]!.GetValue<string>()));

                var row = new JsonObject { ["name"] = name, ["missing"] = missing };
                if (fixture is not null) row["fixture"] = fixture;
                try
                {
                    var image = (IDecodedImage)W410bDecodeDisk.Invoke(null, [path, PixelBufferImageCodec.Instance])!;
                    var pixels = (PixelBuffer)image.PlatformImage;
                    try
                    {
                        row["ok"] = true;
                        row["pixel_width"] = image.PixelWidth;
                        row["pixel_height"] = image.PixelHeight;
                        row["estimated_bytes"] = image.EstimatedBytes;
                        row["downscaled"] = image.Downscaled;
                        row["orientation"] = image.Orientation;
                        row["actual_backend"] = image.ActualBackend.ToString();
                        row["original_width"] = image.OriginalWidth;
                        row["original_height"] = image.OriginalHeight;
                        row["layout"] = pixels.Layout.ToString();
                        row["stride"] = pixels.Stride;
                        row["pixels_sha256"] = W410bPixelHash(pixels);
                    }
                    finally
                    {
                        pixels.Dispose();
                    }
                }
                catch (TargetInvocationException tie) when (tie.InnerException is not null)
                {
                    var ex = tie.InnerException;
                    row["ok"] = false;
                    row["error_type"] = ex.GetType().Name;
                    if (ex is COMException com) row["hresult"] = "0x" + com.HResult.ToString("X8", CultureInfo.InvariantCulture);
                    else row["error_message"] = ex.Message.Replace(dir, "<TMP>", StringComparison.Ordinal); // D19: no temp path in goldens
                    row["is_cache_entry_failure"] = (bool)W410bIsCacheEntryFailure.Invoke(null, [ex])!;
                }
                finally
                {
                    if (File.Exists(path)) File.Delete(path);
                }
                result.Add(row);
            }
        }
        finally
        {
            try { Directory.Delete(dir, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        }
        return result;
    }

    private static string W410bPixelHash(PixelBuffer pixels)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var rowBytes = pixels.Width * 4;
        for (var y = 0; y < pixels.Height; y++) hash.AppendData(pixels.GetRow(y)[..rowBytes]);
        return Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant();
    }
}
