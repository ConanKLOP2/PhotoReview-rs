using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Metadata;
using PhotoReview.Imaging.Pixels;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-08b
//     "preview-entry-read" => Commands.PreviewEntryRead(inputs, inputPath),
//     "preview-entry-write" => Commands.PreviewEntryWrite(inputs),
// WIRING: needs the PhotoReview.Imaging.dll reference (same as W4-04a/W4-06b). PreviewCacheFile.Read / WriteAtomicallyAsync are
// `internal`, so they are reached by REFLECTION (the C# repo is never edited). Real WIC is used (Windows only), exactly like the
// C# tests. Deterministic (D19): temp files live in a fresh GUID folder that is deleted afterwards; no clock, no culture.
// Fixtures: <dir of inputs>/../fixtures/PreviewCache/ (= tests/fixtures/PreviewCache, read-only, already committed).

// W4-08b: PreviewCacheFile.Read (header/EXIF/payload validation, outcome class) and WriteAtomicallyAsync (header + EXIF prefix).
static partial class Commands
{
    private static readonly Type W408bFileType =
        typeof(ExifSummary).Assembly.GetType("PhotoReview.Imaging.Caching.PreviewCacheFile", throwOnError: true)!;

    private const BindingFlags W408bFlags = BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic;

    private static string W408bHex(ReadOnlySpan<byte> b) => Convert.ToHexString(b).ToLowerInvariant();

    private static byte[] W408bHexBytes(string hex) => Convert.FromHexString(hex);

    /// <summary>
    /// Per case {name, base: "exif"|"noexif", ops:[...]}. Ops (applied in order to the fixture bytes): set {offset, hex}, delete {offset, count},
    /// insert {offset, hex}, insert_zeros {offset, count}, zero {offset, count}, truncate {length}, append {hex}, append_zeros {count}.
    /// Result: the case plus {outcome, message, file_bytes, backend, orientation, width, height, original_width, original_height, layout,
    /// has_exif, exif_make, exif_model, exif_iso}.
    /// outcome: "ok" (Read returned), "invalid-data" (InvalidDataException from the header/EXIF/payload checks: a Rust parse_entry error),
    /// "decode-failed" (InvalidDataException from the payload decoder: message starts "Cache entry payload" or ends "could not be decoded.";
    /// Rust parse_entry still succeeds), "end-of-stream" (EndOfStreamException: file shorter than the 24-byte header), "other:TYPE".
    /// </summary>
    public static JsonArray PreviewEntryRead(JsonObject inputs, string inputPath)
    {
        var fixtures = Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!, "..", "fixtures", "PreviewCache");
        var read = W408bFileType.GetMethod("Read", W408bFlags)!;
        var root = Path.Combine(Path.GetTempPath(), "pr-oracle-pv4-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        var result = new JsonArray();
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var file = c["base"]!.GetValue<string>() == "exif" ? "v7-exif-bgr32-96x64.pv4" : "v7-noexif-pbgra32-96x64.pv4";
                var bytes = File.ReadAllBytes(Path.Combine(fixtures, file)).ToList();
                foreach (var opNode in c["ops"]!.AsArray())
                {
                    var op = opNode!.AsObject();
                    var offset = op["offset"]?.GetValue<int>() ?? 0;
                    var count = op["count"]?.GetValue<int>() ?? 0;
                    switch (op["op"]!.GetValue<string>())
                    {
                        case "set":
                            {
                                var data = W408bHexBytes(op["hex"]!.GetValue<string>());
                                for (var i = 0; i < data.Length; i++) bytes[offset + i] = data[i];
                                break;
                            }
                        case "delete": bytes.RemoveRange(offset, count); break;
                        case "insert": bytes.InsertRange(offset, W408bHexBytes(op["hex"]!.GetValue<string>())); break;
                        case "insert_zeros": bytes.InsertRange(offset, new byte[count]); break;
                        case "zero": for (var i = 0; i < count; i++) bytes[offset + i] = 0; break;
                        case "truncate": bytes.RemoveRange(op["length"]!.GetValue<int>(), bytes.Count - op["length"]!.GetValue<int>()); break;
                        case "append": bytes.AddRange(W408bHexBytes(op["hex"]!.GetValue<string>())); break;
                        case "append_zeros": bytes.AddRange(new byte[count]); break;
                        default: throw new ArgumentException("unknown op " + op["op"]);
                    }
                }

                var path = Path.Combine(root, "case.pv4");
                File.WriteAllBytes(path, bytes.ToArray());
                var o = new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["base"] = c["base"]!.GetValue<string>(),
                    ["ops"] = c["ops"]!.DeepClone(),
                };
                try
                {
                    var entry = read.Invoke(null, [path])!;
                    var t = entry.GetType();
                    var pixels = (PixelBuffer)t.GetProperty("Pixels")!.GetValue(entry)!;
                    var exif = (ExifSummary?)t.GetProperty("Exif")!.GetValue(entry);
                    o["outcome"] = "ok";
                    o["message"] = null;
                    o["file_bytes"] = (long)t.GetProperty("FileBytes")!.GetValue(entry)!;
                    o["backend"] = (int)t.GetProperty("ActualBackend")!.GetValue(entry)!;
                    o["orientation"] = (int)t.GetProperty("Orientation")!.GetValue(entry)!;
                    o["width"] = pixels.Width;
                    o["height"] = pixels.Height;
                    o["original_width"] = (int)t.GetProperty("OriginalWidth")!.GetValue(entry)!;
                    o["original_height"] = (int)t.GetProperty("OriginalHeight")!.GetValue(entry)!;
                    o["layout"] = pixels.Layout.ToString();
                    o["has_exif"] = exif is not null;
                    o["exif_make"] = exif?.CameraMake;
                    o["exif_model"] = exif?.CameraModel;
                    o["exif_iso"] = exif?.Iso;
                    pixels.Dispose();
                }
                catch (TargetInvocationException tie)
                {
                    var ex = tie.InnerException!;
                    string outcome;
                    if (ex is InvalidDataException ide)
                        outcome = ide.Message.StartsWith("Cache entry payload", StringComparison.Ordinal)
                            || ide.Message.EndsWith("could not be decoded.", StringComparison.Ordinal) ? "decode-failed" : "invalid-data";
                    else if (ex is EndOfStreamException) outcome = "end-of-stream";
                    else outcome = "other:" + ex.GetType().Name;
                    o["outcome"] = outcome;
                    o["message"] = outcome == "end-of-stream" ? null : ex.Message;
                    foreach (var key in new[] { "file_bytes", "backend", "orientation", "width", "height", "original_width", "original_height", "layout" })
                        o[key] = null;
                    o["has_exif"] = null;
                    o["exif_make"] = null;
                    o["exif_model"] = null;
                    o["exif_iso"] = null;
                }
                result.Add(o);
            }
        }
        finally
        {
            try { Directory.Delete(root, recursive: true); } catch { }
        }
        return result;
    }

    private static ExifSummary? W408bExif(JsonNode? node)
    {
        if (node is not JsonObject e) return null;
        ExifRational? Rat(string key) => e[key] is JsonArray a ? new ExifRational(a[0]!.GetValue<uint>(), a[1]!.GetValue<uint>()) : null;
        return ExifSummary.Create(e["date"]?.GetValue<string>(), e["make"]?.GetValue<string>(), e["model"]?.GetValue<string>(),
            e["lens"]?.GetValue<string>(), e["iso"]?.GetValue<int>(), Rat("focal"), Rat("f_number"), Rat("exposure"));
    }

    /// <summary>
    /// Per case {name, width, height, layout: "bgr32"|"pbgra32", backend, orientation, original_width, original_height, exif?, opacity_verified,
    /// transparent_pixel?: [x, y]}; the pixels are a fixed gradient (alpha 255 except the optional transparent pixel, alpha 254).
    /// Result: the case plus {outcome: "written"|"argument"|"out-of-range"|"other:TYPE", message, prefix_hex (header + EXIF length + EXIF block),
    /// exif_length, payload_starts_with_soi, payload_ends_with_eoi, file_count_after (files left in the cache folder: 1 on success,
    /// 0 on refusal: no temp file survives)}.
    /// </summary>
    public static JsonArray PreviewEntryWrite(JsonObject inputs)
    {
        var write = W408bFileType.GetMethod("WriteAtomicallyAsync", W408bFlags, null,
            [typeof(PixelBuffer), typeof(PhotoReview.Core.Model.DecoderBackend), typeof(int), typeof(int), typeof(int), typeof(string),
                typeof(bool), typeof(ExifSummary), typeof(CancellationToken)], null)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-pv4w-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
            try
            {
                var width = c["width"]!.GetValue<int>();
                var height = c["height"]!.GetValue<int>();
                var layout = c["layout"]!.GetValue<string>() == "pbgra32" ? PixelLayout.Pbgra32 : PixelLayout.Bgr32;
                using var pixels = PixelBuffer.Allocate(width, height, layout);
                for (var y = 0; y < height; y++)
                {
                    var row = pixels.GetRow(y);
                    for (var x = 0; x < width; x++)
                    {
                        row[x * 4] = (byte)(x * 13);
                        row[x * 4 + 1] = (byte)(y * 17);
                        row[x * 4 + 2] = (byte)(x + y);
                        row[x * 4 + 3] = 255;
                    }
                }
                if (c["transparent_pixel"] is JsonArray tp)
                    pixels.GetRow(tp[1]!.GetValue<int>())[tp[0]!.GetValue<int>() * 4 + 3] = 254;

                var cachePath = Path.Combine(root, "cache", "entry.pv4");
                var o = new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["case"] = c.DeepClone(),
                };
                string outcome;
                string? message = null;
                try
                {
                    var backend = (PhotoReview.Core.Model.DecoderBackend)c["backend"]!.GetValue<int>();
                    var task = (Task)write.Invoke(null, [pixels, backend, c["orientation"]!.GetValue<int>(), c["original_width"]!.GetValue<int>(),
                        c["original_height"]!.GetValue<int>(), cachePath, c["opacity_verified"]!.GetValue<bool>(), W408bExif(c["exif"]),
                        CancellationToken.None])!;
                    task.GetAwaiter().GetResult();
                    outcome = "written";
                }
                catch (Exception ex)
                {
                    var inner = ex is TargetInvocationException tie ? tie.InnerException! : ex;
                    outcome = inner is ArgumentOutOfRangeException ? "out-of-range" : inner is ArgumentException ? "argument" : "other:" + inner.GetType().Name;
                    message = inner.Message;
                }
                o["outcome"] = outcome;
                o["message"] = message;
                if (outcome == "written")
                {
                    var bytes = File.ReadAllBytes(cachePath);
                    var exifLength = BitConverter.ToUInt16(bytes, 24);
                    var payloadStart = 26 + exifLength;
                    o["prefix_hex"] = W408bHex(bytes.AsSpan(0, payloadStart));
                    o["exif_length"] = (int)exifLength;
                    o["payload_starts_with_soi"] = bytes[payloadStart] == 0xFF && bytes[payloadStart + 1] == 0xD8;
                    o["payload_ends_with_eoi"] = bytes[^2] == 0xFF && bytes[^1] == 0xD9;
                }
                else
                {
                    o["prefix_hex"] = null;
                    o["exif_length"] = null;
                    o["payload_starts_with_soi"] = null;
                    o["payload_ends_with_eoi"] = null;
                }
                o["file_count_after"] = Directory.Exists(Path.Combine(root, "cache")) ? Directory.GetFiles(Path.Combine(root, "cache")).Length : 0;
                result.Add(o);
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch { }
            }
        }
        return result;
    }
}
