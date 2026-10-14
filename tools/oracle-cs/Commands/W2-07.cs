using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.FileActions;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W2-07
//     "compare-pair" => Commands.ComparePair(inputs),
//     "numbered-pattern" => Commands.NumberedPattern(inputs),
//     "duplicates" => Commands.Duplicates(inputs),
// Needs W2-00.cs (class W2Util).

// W2-07: ComparePairService.Find / BuildIndex, the two numbering regexes, DuplicateFinder.FindAsync
static partial class Commands
{
    private static JsonNode? W207Pair((string Left, string Right)? pair) =>
        pair is { } p ? new JsonArray(p.Left, p.Right) : null;

    /// <summary>
    /// Cases {name, files:[...], queries:[...]}: for each query the Find result; "index" = BuildIndex entries sorted by key (ordinal),
    /// "index_find_agrees" = BuildIndex(files)[file] equals Find(files, file) for every file (true/false per file listed).
    /// Absolute paths only (relative ones depend on the process directory).
    /// </summary>
    public static JsonArray ComparePair(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var files = W2Util.Strings(c["files"]);
            var finds = new JsonArray();
            foreach (var q in W2Util.Strings(c["queries"]))
            {
                finds.Add(new JsonObject { ["query"] = q, ["pair"] = W2Util.Capture(() => W207Pair(ComparePairService.Find(files, q))) });
            }

            JsonNode? index;
            JsonNode? agrees;
            try
            {
                var built = ComparePairService.BuildIndex(files);
                index = new JsonArray(built.OrderBy(kv => kv.Key, StringComparer.Ordinal)
                    .Select(kv => (JsonNode?)new JsonObject { ["key"] = kv.Key, ["left"] = kv.Value.Left, ["right"] = kv.Value.Right }).ToArray());
                var agree = new JsonArray();
                foreach (var f in files)
                {
                    var found = ComparePairService.Find(files, f);
                    var has = built.TryGetValue(f, out var p);
                    agree.Add(new JsonObject { ["file"] = f, ["agrees"] = found is null ? !has : has && found.Value == p });
                }
                agrees = agree;
            }
            catch (Exception ex)
            {
                index = new JsonObject { ["throws"] = ex.GetType().Name };
                agrees = null;
            }

            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["files"] = c["files"]!.DeepClone(),
                ["finds"] = finds,
                ["index"] = index,
                ["index_find_agrees"] = agrees,
            });
        }
        return result;
    }

    /// <summary>
    /// For each stem: ComparePairService.NumberedStemPattern group 1 (null = no match), NumberedSuffix, and DuplicateFinder.NumberedPattern.IsMatch.
    /// All three are private; they are read by reflection so the REAL code (not a copy of the pattern) answers.
    /// </summary>
    public static JsonArray NumberedPattern(JsonObject inputs)
    {
        var stemPattern = (Regex)typeof(ComparePairService).GetMethod("NumberedStemPattern", BindingFlags.NonPublic | BindingFlags.Static)!.Invoke(null, null)!;
        var suffix = typeof(ComparePairService).GetMethod("NumberedSuffix", BindingFlags.NonPublic | BindingFlags.Static)!;
        var dupPattern = (Regex)typeof(DuplicateFinder).GetField("NumberedPattern", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null)!;
        var variantOf = typeof(ComparePairService).GetMethod("IsNumberedVariantOf", BindingFlags.NonPublic | BindingFlags.Static)!;
        var result = new JsonArray();
        foreach (var stem in W2Util.Strings(inputs["stems"]))
        {
            var m = stemPattern.Match(stem);
            result.Add(new JsonObject
            {
                ["stem"] = stem,
                ["stem_base"] = m.Success ? m.Groups[1].Value : null,
                // NumberedSuffix takes a candidate PATH and looks at its stem: ".jpg" is appended so the stem is exactly the input.
                ["suffix"] = (int)suffix.Invoke(null, [stem + ".jpg"])!,
                ["duplicate_pattern"] = dupPattern.IsMatch(stem),
            });
        }
        foreach (var pair in inputs["variant_pairs"]!.AsArray())
        {
            var candidate = pair![0]!.GetValue<string>();
            var baseStem = pair[1]!.GetValue<string>();
            result.Add(new JsonObject
            {
                ["candidate"] = candidate,
                ["base_stem"] = baseStem,
                ["is_numbered_variant"] = (bool)variantOf.Invoke(null, [candidate, baseStem])!,
            });
        }
        return result;
    }

    /// <summary>
    /// DuplicateFinder.FindAsync on a real temp folder, cases {name, files:[{name, content}], listed?:[names in call order, may repeat or
    /// differ in case or name an uncreated file], remove_numbered, hash: "content"|"constant"|{"by_name":{name:hashText}},
    /// io_error?:[names whose hash throws IOException], directories?:[names created as folders]}. Result: removed names and, in call order,
    /// the names the hash function was called for.
    /// </summary>
    public static JsonArray Duplicates(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-dup-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
            try
            {
                var created = new List<string>();
                foreach (var f in c["files"]!.AsArray())
                {
                    var o = f!.AsObject();
                    var name = o["name"]!.GetValue<string>();
                    File.WriteAllText(Path.Combine(root, name), o["content"]!.GetValue<string>());
                    created.Add(name);
                }
                foreach (var d in W2Util.Strings(c["directories"])) Directory.CreateDirectory(Path.Combine(root, d));
                var listed = c["listed"] is null ? created : W2Util.Strings(c["listed"]);
                var ioErrors = new HashSet<string>(W2Util.Strings(c["io_error"]), StringComparer.OrdinalIgnoreCase);
                var hashNode = c["hash"]!;
                var hashed = new List<string>();
                string RelName(string full) => Path.GetFileName(full);

                Task<string> Hash(string path, CancellationToken ct)
                {
                    hashed.Add(RelName(path));
                    if (ioErrors.Contains(RelName(path))) throw new IOException("injected");
                    if (hashNode is JsonValue v)
                    {
                        return Task.FromResult(v.GetValue<string>() == "content" ? File.ReadAllText(path) : "constant");
                    }
                    var byName = hashNode["by_name"]!.AsObject();
                    return Task.FromResult(byName[RelName(path)]!.GetValue<string>());
                }

                var removed = DuplicateFinder.FindAsync(
                    listed.Select(n => Path.Combine(root, n)).ToList(),
                    c["remove_numbered"]!.GetValue<bool>(),
                    Hash).GetAwaiter().GetResult();
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["remove_numbered"] = c["remove_numbered"]!.GetValue<bool>(),
                    ["removed"] = W2Util.StrArray(removed.Select(RelName)),
                    ["hashed"] = W2Util.StrArray(hashed),
                });
            }
            finally
            {
                try { Directory.Delete(root, recursive: true); } catch { }
            }
        }
        return result;
    }
}
