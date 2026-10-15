using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Model;
using PhotoReview.Imaging;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-01a
//     "image-cache-key" => Commands.ImageCacheKeyCases(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * ImageCacheKey lives in PhotoReview.Imaging.dll, which tools/oracle.ps1 does not build yet: add
//     `dotnet build src\PhotoReview.Imaging\PhotoReview.Imaging.csproj -c Release -o $coreDir` next to the Core build and a
//     `<Reference Include="PhotoReview.Imaging"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.dll</HintPath></Reference>` in
//     tools/oracle-cs/OracleCs.csproj (Imaging only references Core; WPF files live in PhotoReview.Imaging.Wpf, not built here).
//   * Needs no other oracle file.

// W4-01a: ImageCacheKey.Create (4 overloads), CreateOriginal (2 overloads), MatchesCurrentSource, record-struct equality.
static partial class Commands
{
    private static JsonNode? W401Capture(Func<JsonNode?> f, string? root)
    {
        try { return f(); }
        catch (Exception ex)
        {
            var o = new JsonObject { ["throws"] = ex.GetType().Name, ["message"] = ex.Message };
            if (ex is FileNotFoundException fnf) o["file_name"] = W401Rel(fnf.FileName, root);
            return o;
        }
    }

    /// <summary>Replaces the (upper-cased or not) temp root prefix by "&lt;ROOT&gt;" so the golden does not depend on the machine.</summary>
    private static string? W401Rel(string? path, string? root)
    {
        if (path is null || root is null) return path;
        var rootUpper = root.ToUpperInvariant();
        if (path.StartsWith(rootUpper, StringComparison.Ordinal)) return "<ROOT>" + path[rootUpper.Length..];
        if (path.StartsWith(root, StringComparison.OrdinalIgnoreCase)) return "<ROOT>" + path[root.Length..];
        return path;
    }

    private static DecoderBackend W401Backend(JsonObject o) =>
        Enum.Parse<DecoderBackend>(o["backend"]?.GetValue<string>() ?? "Wpf");

    private static bool W401Orient(JsonObject o) => o["orient"]?.GetValue<bool>() ?? true;

    private static byte W401Kind(JsonObject o) => (byte)(o["source_kind"]?.GetValue<int>() ?? 0);

    private static DecodeBox W401Box(JsonObject o) => new(o["w"]?.GetValue<int>() ?? 0, o["h"]?.GetValue<int>() ?? 0);

    private static CatalogEntry W401Entry(string path, long? length, long? ticks)
    {
        var entry = new CatalogEntry(path);
        if (length is { } l) entry = entry with { Length = l };
        if (ticks is { } t) entry = entry with { LastWriteUtc = new DateTime(t, DateTimeKind.Utc) };
        return entry;
    }

    /// <summary>Entry-overload key of a spec {path, length, ticks, original, w, h, orient?, backend?, source_kind?} (no file system when both are set).</summary>
    private static ImageCacheKey W401FromEntrySpec(JsonObject o) =>
        ImageCacheKey.Create(
            W401Entry(o["path"]!.GetValue<string>(), o["length"]?.GetValue<long>(), o["ticks"]?.GetValue<long>()),
            o["original"]?.GetValue<bool>() ?? false, W401Box(o), W401Orient(o), W401Backend(o), W401Kind(o));

    private static JsonObject W401Describe(ImageCacheKey k, string? root) => new()
    {
        ["path"] = W401Rel(k.Path, root),
        ["length"] = k.Length,
        ["ticks"] = k.LastWriteUtcTicks,
        ["is_original"] = k.IsOriginal,
        ["width"] = k.TargetWidth,
        ["height"] = k.TargetHeight,
        ["box_unbounded"] = k.TargetBox.IsUnbounded,
        ["orientation_applied"] = k.OrientationApplied,
        ["backend"] = k.Backend.ToString(),
        ["source_kind"] = k.SourceKind,
    };

    /// <summary>
    /// Cases {name, kind, ...}:
    ///  * "entry": {path, length?, ticks?, original, w, h, orient?, backend?, source_kind?}. With length AND ticks the key is built from the entry without
    ///    touching the disk (virtual paths allowed); with either missing it falls back to the real file (use "file" cases for that).
    ///    Result: "key" (described) or "throws"/"message".
    ///  * "pair": {a: spec, b: spec} (entry specs) -> equal (==), equals_method, hash_equal, path_equal, both keys described.
    ///  * "original_of": {source: spec, kind?} -> source, original (CreateOriginal(source)), original_kind (CreateOriginal(source, kind)).
    ///  * "file": {file:{name, length, write_ticks, create?, directory?}, overload: "path"|"width"|"fileinfo"|"entry-no-stat", original, w, h, orient?, backend?, source_kind?,
    ///    then?: "rewrite"|"delete"|"touch"|"grow", then_length?, then_ticks?} on a real temp file (create:false = never created).
    ///    Result: key (path relative to "&lt;ROOT&gt;") + matches_current_source before and after "then" + recreated_equals (a fresh key equals
    ///    the first one; null when the file is gone).
    /// </summary>
    public static JsonArray ImageCacheKeyCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind };
            switch (kind)
            {
                case "entry":
                    o["input"] = c.DeepClone();
                    o["result"] = W401Capture(() => W401Describe(W401FromEntrySpec(c), null), null);
                    break;
                case "pair":
                {
                    var a = W401FromEntrySpec(c["a"]!.AsObject());
                    var b = W401FromEntrySpec(c["b"]!.AsObject());
                    o["a"] = W401Describe(a, null);
                    o["b"] = W401Describe(b, null);
                    o["equal"] = a == b;
                    o["not_equal"] = a != b;
                    o["equals_method"] = a.Equals(b);
                    o["hash_equal"] = a.GetHashCode() == b.GetHashCode();
                    o["path_equal"] = string.Equals(a.Path, b.Path, StringComparison.Ordinal);
                    break;
                }
                case "original_of":
                {
                    var source = W401FromEntrySpec(c["source"]!.AsObject());
                    o["source"] = W401Describe(source, null);
                    o["original"] = W401Describe(ImageCacheKey.CreateOriginal(source), null);
                    var k = (byte)(c["kind_arg"]?.GetValue<int>() ?? 2);
                    o["kind_arg"] = k;
                    o["original_kind"] = W401Describe(ImageCacheKey.CreateOriginal(source, k), null);
                    break;
                }
                case "file":
                    W401File(c, o);
                    break;
                default:
                    throw new ArgumentException("unknown kind " + kind);
            }
            result.Add(o);
        }
        return result;
    }

    private static void W401File(JsonObject c, JsonObject o)
    {
        var root = Path.GetFullPath(Path.Combine(Path.GetTempPath(), "pr-oracle-key-" + Guid.NewGuid().ToString("N")));
        Directory.CreateDirectory(root);
        try
        {
            var f = c["file"]!.AsObject();
            var path = Path.Combine(root, f["name"]!.GetValue<string>());
            var length = f["length"]!.GetValue<int>();
            var ticks = f["write_ticks"]!.GetValue<long>();
            if (f["directory"]?.GetValue<bool>() == true)
            {
                Directory.CreateDirectory(path);
            }
            else if (f["create"]?.GetValue<bool>() != false)
            {
                File.WriteAllBytes(path, new byte[length]);
                File.SetLastWriteTimeUtc(path, new DateTime(ticks, DateTimeKind.Utc));
            }
            var overload = c["overload"]!.GetValue<string>();
            var original = c["original"]?.GetValue<bool>() ?? false;
            var box = W401Box(c);
            ImageCacheKey Build() => overload switch
            {
                "path" => ImageCacheKey.Create(path, original, box, W401Orient(c), W401Backend(c), W401Kind(c)),
                "width" => ImageCacheKey.Create(path, original, box.Width, W401Orient(c), W401Backend(c), W401Kind(c)),
                "fileinfo" => ImageCacheKey.Create(new FileInfo(path), original, box, W401Orient(c), W401Backend(c), W401Kind(c)),
                "entry-no-stat" => ImageCacheKey.Create(new CatalogEntry(path), original, box, W401Orient(c), W401Backend(c), W401Kind(c)),
                _ => throw new ArgumentException("unknown overload " + overload),
            };
            o["input"] = c.DeepClone();
            ImageCacheKey? first = null;
            o["result"] = W401Capture(() =>
            {
                first = Build();
                return W401Describe(first.Value, root);
            }, root);
            if (first is not { } key) return;

            o["matches_before"] = key.MatchesCurrentSource();
            switch (c["then"]?.GetValue<string>())
            {
                case "rewrite":
                    File.WriteAllBytes(path, new byte[c["then_length"]!.GetValue<int>()]);
                    break;
                case "grow":
                    File.WriteAllBytes(path, new byte[length + 1]);
                    File.SetLastWriteTimeUtc(path, new DateTime(ticks, DateTimeKind.Utc));
                    break;
                case "touch":
                    File.SetLastWriteTimeUtc(path, new DateTime(c["then_ticks"]!.GetValue<long>(), DateTimeKind.Utc));
                    break;
                case "delete":
                    File.Delete(path);
                    break;
                case null:
                    break;
                default:
                    throw new ArgumentException("unknown then");
            }
            o["matches_after"] = key.MatchesCurrentSource();
            ImageCacheKey? again = null;
            try { again = Build(); } catch (FileNotFoundException) { }
            o["recreated_equals"] = again is { } a ? a == key : null;
        }
        finally
        {
            try { Directory.Delete(root, recursive: true); } catch { }
        }
    }
}
