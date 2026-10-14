using System.Collections.Immutable;
using System.Globalization;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.Text;
using PhotoReview.Core.IO;
using PhotoReview.Core.Localization;
using PhotoReview.Localization.Generator;

// W2-04 oracle: LanguageCatalog.TryParse, Localizer.Create/Get/Format/FormatPlural/Transform (+ TranslatorModes),
// LanguageLoader (real files in a temp folder), the Roslyn TrGenerator (key table + diagnostics) and the FlatJsonReader.
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `command switch`):
//     "l10n-catalog" => Commands.L10nCatalogCases(inputs),
//     "l10n-localizer" => Commands.L10nLocalizerCases(inputs),
//     "l10n-loader" => Commands.L10nLoaderCases(inputs),
//     "l10n-generator" => Commands.L10nGeneratorCases(inputs),
//     "l10n-flat-json" => Commands.L10nFlatJsonCases(inputs),
//
// tools/oracle-cs/OracleCs.csproj additions needed by this file (the generator and Roslyn):
//     <ItemGroup>
//       <PackageReference Include="Microsoft.CodeAnalysis.CSharp" Version="4.14.0" />
//       <Reference Include="PhotoReview.Localization.Generator"><HintPath>$(CSharpCoreDir)\PhotoReview.Localization.Generator.dll</HintPath></Reference>
//     </ItemGroup>
// (tools/oracle.ps1 already builds PhotoReview.Core, whose build also produces PhotoReview.Localization.Generator.dll in the same folder.)
//
// Every command runs with CultureInfo.CurrentCulture = Invariant (numbers in Format are culture dependent in C#).
static partial class Commands
{
    private static JsonArray L10nStrings(IEnumerable<string> items) => new(items.Select(s => (JsonNode?)s).ToArray());

    /// <summary>The .NET message inside "(...)" of warnings built from exception messages is not reproducible: mask it.</summary>
    private static string L10nMask(string warning, string? root = null)
    {
        if (root is not null) warning = warning.Replace(root, "<ROOT>", StringComparison.Ordinal);
        return Regex.Replace(warning, @"^(.*(?:invalid JSON|unreadable text|cannot read|cannot list files)) \(.*\)$", "$1 (*)", RegexOptions.Singleline);
    }

    private static JsonArray L10nWarnings(IEnumerable<string> warnings, string? root = null) =>
        new(warnings.Select(w => (JsonNode?)L10nMask(w, root)).ToArray());

    /// <summary>{{DEEP:n}} -> n nested arrays; {{DEEPOBJ:n}} -> n nested objects ending in the number 1 (inputs stay small and exact).</summary>
    private static string L10nExpand(string json)
    {
        json = Regex.Replace(json, @"\{\{DEEP:(\d+)\}\}", m => new string('[', int.Parse(m.Groups[1].Value, CultureInfo.InvariantCulture)) + new string(']', int.Parse(m.Groups[1].Value, CultureInfo.InvariantCulture)));
        return Regex.Replace(json, @"\{\{DEEPOBJ:(\d+)\}\}", m => string.Concat(Enumerable.Repeat("{\"a\":", int.Parse(m.Groups[1].Value, CultureInfo.InvariantCulture))) + "1" + new string('}', int.Parse(m.Groups[1].Value, CultureInfo.InvariantCulture)));
    }

    private static JsonObject L10nCatalogJson(LanguageCatalog catalog) => new()
    {
        ["code"] = catalog.Code,
        ["name"] = catalog.Name,
        ["nativeName"] = catalog.NativeName,
        ["nativeNameDeclared"] = catalog.NativeNameDeclared,
        ["plural"] = catalog.Plural.ToString(),
        ["pluralDeclared"] = catalog.PluralDeclared,
        ["authors"] = L10nStrings(catalog.Authors),
        // Dictionary enumeration order = insertion order (no removals); a duplicate key keeps its first position.
        ["entries"] = new JsonArray(catalog.Entries.Select(p => (JsonNode?)new JsonArray((JsonNode?)p.Key, (JsonNode?)p.Value)).ToArray()),
    };

    /// <summary>LanguageCatalog.TryParse(json, source): validity, parsed meta and entries, warnings (the .NET exception text masked).</summary>
    public static JsonArray L10nCatalogCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var warnings = new List<string>();
            var item = (JsonObject)c.DeepClone();
            var json = c["json"]!.GetValue<string>();
            if (c["repeat"] is { } repeat) json = json.Replace("{{REPEAT}}", new string(repeat["char"]!.GetValue<string>()[0], repeat["count"]!.GetValue<int>()), StringComparison.Ordinal);
            json = L10nExpand(json);
            var source = c["source_repeat"] is { } sr ? new string(sr["char"]!.GetValue<string>()[0], sr["count"]!.GetValue<int>()) : c["source"]?.GetValue<string>() ?? "test.json";
            var ok = LanguageCatalog.TryParse(json, source, out var catalog, warnings);
            item["ok"] = ok;
            if (ok) item["catalog"] = L10nCatalogJson(catalog);
            item["warnings"] = L10nWarnings(warnings);
            result.Add(item);
        }

        return result;
    }

    /// <summary>Text of a synced asset (tests/golden-inputs/../../assets/&lt;asset&gt;, e.g. "languages/en.json"); the oracle's
    /// FixturesDir is tests/fixtures, so the assets folder is two levels up.</summary>
    private static string L10nAssetText(string asset) =>
        File.ReadAllText(Path.GetFullPath(Path.Combine(PerfCommands.FixturesDir, "..", "..", "assets", asset)));

    private static object? L10nArg(JsonNode? n)
    {
        if (n is null) return null;
        if (n is JsonObject o)
        {
            return o["t"]!.GetValue<string>() switch
            {
                "double" => o["bits"] is { } b
                    ? BitConverter.Int64BitsToDouble(long.Parse(b.GetValue<string>(), NumberStyles.HexNumber, CultureInfo.InvariantCulture))
                    : o["v"]!.GetValue<double>(),
                "bool" => o["v"]!.GetValue<bool>(),
                "int" => o["v"]!.GetValue<int>(),
                _ => throw new ArgumentException("arg type"),
            };
        }

        var v = n.AsValue();
        if (v.TryGetValue<string>(out var s)) return s;
        return v.GetValue<long>();
    }

    private static LocArg[] L10nArgs(JsonNode? args) =>
        args is JsonObject o ? o.Select(p => new LocArg(p.Key, L10nArg(p.Value))).ToArray() : [];

    private static LanguageCatalog L10nParse(string json, List<string> warnings)
    {
        if (!LanguageCatalog.TryParse(json, "test", out var catalog, warnings)) throw new InvalidOperationException("catalog in oracle input is invalid: " + string.Join("; ", warnings));
        return catalog;
    }

    private static JsonObject L10nDescribe(Localizer localizer, JsonNode? probes)
    {
        var o = new JsonObject
        {
            ["code"] = localizer.Code,
            ["nativeName"] = localizer.NativeName,
            ["plural"] = localizer.Plural.ToString(),
            ["warnings"] = L10nWarnings(localizer.Warnings),
            ["keys"] = L10nStrings(localizer.Keys.OrderBy(k => k, StringComparer.Ordinal)),
        };
        var results = new JsonArray();
        foreach (var p in probes?.AsArray() ?? [])
        {
            var probe = p!.AsObject();
            var key = probe["key"]!.GetValue<string>();
            var item = (JsonObject)probe.DeepClone();
            switch (probe["op"]!.GetValue<string>())
            {
                case "get": item["out"] = localizer.Get(key); break;
                case "contains": item["out"] = localizer.Contains(key); break;
                case "format": item["out"] = localizer.Format(key, L10nArgs(probe["args"])); break;
                case "plural": item["out"] = localizer.FormatPlural(key, probe["count"]!.GetValue<long>(), L10nArgs(probe["args"])); break;
                default: throw new ArgumentException("probe op");
            }

            results.Add(item);
        }

        o["probes"] = results;
        return o;
    }

    /// <summary>Localizer.Create(english, overlays, priorWarnings), optionally transformed by TranslatorModes, then probes.</summary>
    public static JsonArray L10nLocalizerCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var scratch = new List<string>();
            var english = L10nParse(c["english_asset"] is { } ea ? L10nAssetText(ea.GetValue<string>()) : c["english"]!.GetValue<string>(), scratch);
            var overlays = (c["overlays"]?.AsArray() ?? []).Select(o => L10nParse(o!.GetValue<string>(), scratch))
                .Concat((c["overlay_assets"]?.AsArray() ?? []).Select(o => L10nParse(L10nAssetText(o!.GetValue<string>()), scratch))).ToList();
            var prior = (c["prior_warnings"]?.AsArray() ?? []).Select(w => w!.GetValue<string>()).ToList();
            var localizer = Localizer.Create(english, overlays, prior);
            switch (c["mode"]?.GetValue<string>() ?? "none")
            {
                case "pseudo": localizer = TranslatorModes.CreatePseudo(localizer); break;
                case "showkeys": localizer = TranslatorModes.CreateShowKeys(localizer); break;
            }

            var item = (JsonObject)c.DeepClone();
            item["out"] = L10nDescribe(localizer, c["probes"]);
            result.Add(item);
        }

        return result;
    }

    private static void L10nWriteFiles(string dir, JsonObject? files)
    {
        Directory.CreateDirectory(dir);
        foreach (var (name, spec) in files ?? [])
        {
            string text;
            if (spec is JsonObject o)
            {
                text = o["json"]!.GetValue<string>();
                if (o["pad_to_bytes"] is { } pad)
                {
                    var bytes = new byte[pad.GetValue<int>()];
                    Array.Fill(bytes, (byte)' ');
                    Encoding.UTF8.GetBytes(text).CopyTo(bytes, 0);
                    File.WriteAllBytes(Path.Combine(dir, name), bytes);
                    continue;
                }

                if (o["bom"]?.GetValue<bool>() == true)
                {
                    File.WriteAllBytes(Path.Combine(dir, name), [0xEF, 0xBB, 0xBF, .. Encoding.UTF8.GetBytes(text)]);
                    continue;
                }
            }
            else
            {
                text = spec!.GetValue<string>();
            }

            File.WriteAllText(Path.Combine(dir, name), text, new UTF8Encoding(false));
        }
    }

    /// <summary>LanguageLoader over real files: DiscoverLanguages / Load / Resolve with the shipped and user folders.</summary>
    public static JsonArray L10nLoaderCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Directory.CreateTempSubdirectory("oracle-l10n-").FullName;
            var locks = new List<FileStream>();
            try
            {
                var shipped = Path.Combine(root, "App", "Languages");
                var user = Path.Combine(root, "User", "Languages");
                if (c["shipped_missing"]?.GetValue<bool>() != true) L10nWriteFiles(shipped, c["shipped"]?.AsObject());
                string? userDir = user;
                if (c["user_null"]?.GetValue<bool>() == true) userDir = null;
                else if (c["user_missing"]?.GetValue<bool>() != true) L10nWriteFiles(user, c["user"]?.AsObject());
                foreach (var lockedName in c["locked"]?.AsArray() ?? [])
                {
                    var rel = lockedName!.GetValue<string>().Replace('/', Path.DirectorySeparatorChar);
                    locks.Add(new FileStream(Path.Combine(root, rel), FileMode.Open, FileAccess.Read, FileShare.None));
                }

                var loader = new LanguageLoader(new PhysicalFileSystem(), shipped, userDir);
                var ops = new JsonArray();
                foreach (var opNode in c["ops"]!.AsArray())
                {
                    var op = opNode!.AsObject();
                    var item = (JsonObject)op.DeepClone();
                    CultureInfo? culture = op["culture"] is { } cn ? CultureInfo.GetCultureInfo(cn.GetValue<string>()) : null;
                    if (culture is not null)
                    {
                        item["culture_name"] = culture.Name;
                        item["culture_two_letter"] = culture.TwoLetterISOLanguageName;
                    }

                    var code = op["code"]?.GetValue<string>();
                    switch (op["op"]!.GetValue<string>())
                    {
                        case "discover":
                            item["out"] = new JsonArray(loader.DiscoverLanguages().Select(l => (JsonNode?)new JsonObject
                            {
                                ["code"] = l.Code, ["name"] = l.Name, ["nativeName"] = l.NativeName, ["isUserProvided"] = l.IsUserProvided,
                            }).ToArray());
                            break;
                        case "resolve":
                            item["out"] = loader.Resolve(code, culture);
                            break;
                        case "load":
                        {
                            var localizer = loader.Load(code, culture);
                            var described = L10nDescribe(localizer, op["probes"]);
                            described.Remove("keys");
                            described["warnings"] = L10nWarnings(localizer.Warnings, root);
                            item["out"] = described;
                            break;
                        }
                        default: throw new ArgumentException("loader op");
                    }

                    ops.Add(item);
                }

                var entry = (JsonObject)c.DeepClone();
                entry["ops"] = ops;
                result.Add(entry);
            }
            finally
            {
                foreach (var l in locks) l.Dispose();
                Directory.Delete(root, recursive: true);
            }
        }

        return result;
    }

    private sealed class L10nMemoryText(string path, string text) : AdditionalText
    {
        public override string Path { get; } = path;

        public override SourceText GetText(CancellationToken cancellationToken = default) => SourceText.From(text, Encoding.UTF8);
    }

    private static JsonObject L10nRunGenerator(IReadOnlyList<(string Path, string Text)> files)
    {
        var compilation = CSharpCompilation.Create("GeneratorHarness", [], [], new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary, nullableContextOptions: NullableContextOptions.Enable));
        var driver = CSharpGeneratorDriver.Create(new TrGenerator())
            .AddAdditionalTexts(ImmutableArray.CreateRange(files.Select(f => (AdditionalText)new L10nMemoryText(f.Path, f.Text))))
            .RunGeneratorsAndUpdateCompilation(compilation, out _, out var diagnostics);
        var source = driver.GetRunResult().GeneratedTrees.SingleOrDefault()?.ToString();
        var o = new JsonObject
        {
            ["diagnostics"] = new JsonArray(diagnostics
                .Select(d => (id: d.Id, message: d.GetMessage(CultureInfo.InvariantCulture)))
                .OrderBy(d => d.id, StringComparer.Ordinal).ThenBy(d => d.message, StringComparer.Ordinal)
                .Select(d => (JsonNode?)new JsonObject { ["id"] = d.id, ["message"] = d.message }).ToArray()),
            ["generated"] = source is not null,
        };
        if (source is null) return o;

        var consts = new JsonArray();
        var constKeys = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (Match m in Regex.Matches(source, "public const string (\\w+) = \"((?:[^\"\\\\]|\\\\.)*)\";"))
        {
            consts.Add(new JsonObject { ["ident"] = m.Groups[1].Value, ["key"] = m.Groups[2].Value });
            constKeys[m.Groups[1].Value] = m.Groups[2].Value;
        }

        var members = new JsonArray();
        var trIndex = source.IndexOf("public static partial class Tr\n", StringComparison.Ordinal);
        var trText = trIndex < 0 ? "" : source[trIndex..];
        foreach (Match m in Regex.Matches(trText, @"public static string (\w+)(?:\(([^)]*)\))? =>\s*Localizer\.Current\.(Get|Format|FormatPlural)\((TrKeys\.(\w+)|""((?:[^""\\]|\\.)*)"")"))
        {
            var parameters = m.Groups[2].Success
                ? m.Groups[2].Value.Split(',', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries).Select(p => p.Replace("object? ", "").Replace("long ", "").Replace("@", "")).ToList()
                : [];
            var plural = m.Groups[3].Value == "FormatPlural";
            if (plural) parameters.Remove("count");
            members.Add(new JsonObject
            {
                ["ident"] = m.Groups[1].Value,
                ["kind"] = plural ? "plural" : "simple",
                [plural ? "base" : "key"] = plural ? m.Groups[6].Value : constKeys[m.Groups[5].Value],
                ["params"] = L10nStrings(parameters),
            });
        }

        o["consts"] = consts;
        o["members"] = members;
        return o;
    }

    private static readonly string[] L10nPieces = ["{", "}", "{{", "}}", "a", " ", "é", "{x}", "{name}", "{count}", "{1}", "{ x}", "{}", "{x y}", "{X}", "{ñ}", "日本", "{a1}"];

    /// <summary>The Roslyn TrGenerator over en.json texts: diagnostics (id + message), TrKeys constants, Tr members.</summary>
    public static JsonArray L10nGeneratorCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            if (c["fuzz_placeholders"] is { } fuzz)
            {
                var r = new Random(fuzz["seed"]!.GetValue<int>());
                var texts = Enumerable.Range(0, fuzz["count"]!.GetValue<int>())
                    .Select(_ => string.Concat(Enumerable.Range(0, r.Next(0, 8)).Select(_ => L10nPieces[r.Next(L10nPieces.Length)])))
                    .ToList();
                var entries = texts.Select((t, i) => $"\"t.k{i}\": {JsonSerializer.Serialize(t)}");
                var run = L10nRunGenerator([(@"C:\x\Languages\en.json", "{ \"_meta\": {\"code\":\"en\"}, " + string.Join(", ", entries) + " }")]);
                var rejected = run["diagnostics"]!.AsArray()
                    .Where(d => d!["id"]!.GetValue<string>() == "PRLOC004")
                    .Select(d => int.Parse(Regex.Match(d!["message"]!.GetValue<string>(), "Text of 't\\.k(\\d+)'").Groups[1].Value, CultureInfo.InvariantCulture))
                    .ToHashSet();
                item["texts"] = L10nStrings(texts);
                item["generator_rejects"] = new JsonArray(texts.Select((_, i) => (JsonNode?)rejected.Contains(i)).ToArray());
                item["runtime_accepts"] = new JsonArray(texts.Select(t => (JsonNode?)LocTemplate.TryParse(t, out _)).ToArray());
                item["placeholders"] = new JsonArray(texts.Select(t => (JsonNode?)(LocTemplate.TryParse(t, out var tpl) ? L10nStrings(tpl.PlaceholderNames.Distinct()) : null)).ToArray());
                item["other_diagnostics"] = new JsonArray(run["diagnostics"]!.AsArray().Where(d => d!["id"]!.GetValue<string>() != "PRLOC004").Select(d => d!.DeepClone()).ToArray());
            }
            else
            {
                var files = c["files"]!.AsArray().Select(f => (f!["path"]!.GetValue<string>(),
                    f["asset"] is { } asset ? L10nAssetText(asset.GetValue<string>()) : L10nExpand(f["json"]!.GetValue<string>()))).ToList();
                var run = L10nRunGenerator(files);
                foreach (var (k, v) in run) item[k] = v?.DeepClone();
            }

            result.Add(item);
        }

        return result;
    }

    private static readonly string[] L10nTokens =
    [
        "{", "}", "[", "]", ",", ":", "\"", "\"a\"", "\"b\"", "\"_m\"", "\"key.one\"", "\"v\"", "\"\\n\"", "\"\\u0041\"", "\"\\ud83d\\ude00\"",
        "\"\\ud800\"", "\"\\udc00x\"", "\"\\uD83D\"", "\"\\u00\"", "\"\\x\"", "\"tab\there\"", "1", "01", "-", "-0", "1.", "1.5e+3", "1e", "true", "tru", "null", "nul",
        " ", "\n", "\t", "// c\n", "/* c */", "/*", "//", "\uFEFF", "\u00A0", "\u2028", "\0", "'", "a", "日本", "\ud83d\ude00", "\r", "\u2029",
    ];

    private const string L10nValidSample = """
        {
          // comment
          "_meta": { "code": "en", "n": [1, 2.5e3, -0, true, null, {"x": []}] },
          "a.b": "Hello {name}", /* block */
          "c": "\u00e9\ud83d\ude00\n\"q\"",
        }
        """;

    private static readonly JsonDocumentOptions L10nStjOptions = new() { CommentHandling = JsonCommentHandling.Skip, AllowTrailingCommas = true };

    private static List<(string Key, string Value)>? L10nStjFlat(string text, out bool jsonValid)
    {
        jsonValid = false;
        try
        {
            if (text.Length > 0 && text[0] == '\uFEFF') text = text[1..];
            using var doc = JsonDocument.Parse(text, L10nStjOptions);
            jsonValid = true;
            if (doc.RootElement.ValueKind != JsonValueKind.Object) return null;
            var entries = new List<(string, string)>();
            foreach (var p in doc.RootElement.EnumerateObject())
            {
                if (p.Name.StartsWith('_')) continue;
                if (p.Value.ValueKind != JsonValueKind.String) return null;
                entries.Add((p.Name, p.Value.GetString()!));
            }

            return entries;
        }
        catch (Exception ex) when (ex is JsonException or InvalidOperationException or ArgumentException)
        {
            return null;
        }
    }

    private static List<(string Key, string Value)>? L10nGeneratorFlat(string text)
    {
        var readerType = typeof(TrGenerator).Assembly.GetType("PhotoReview.Localization.Generator.FlatJsonReader")!;
        var read = readerType.GetMethod("Read", BindingFlags.Public | BindingFlags.Static)!;
        try
        {
            var list = (System.Collections.IEnumerable)read.Invoke(null, [text])!;
            var entries = new List<(string, string)>();
            foreach (var e in list)
            {
                var t = e.GetType();
                entries.Add(((string)t.GetProperty("Key")!.GetValue(e)!, (string)t.GetProperty("Value")!.GetValue(e)!));
            }

            return entries;
        }
        catch (TargetInvocationException ex) when (ex.InnerException?.GetType().Name == "FlatJsonException")
        {
            return null;
        }
    }

    private static JsonNode? L10nEntries(List<(string Key, string Value)>? entries) =>
        entries is null ? null : new JsonArray(entries.Select(e => (JsonNode?)new JsonArray((JsonNode?)e.Key, (JsonNode?)e.Value)).ToArray());

    private static string L10nSoup(Random r)
    {
        var sb = new StringBuilder();
        if (r.Next(4) != 0) sb.Append('{');
        for (var i = r.Next(0, 14); i > 0; i--) sb.Append(L10nTokens[r.Next(L10nTokens.Length)]);
        if (r.Next(3) != 0) sb.Append('}');
        return sb.ToString();
    }

    private static string L10nMutateValid(Random r)
    {
        var chars = L10nValidSample.ToCharArray();
        var sb = new StringBuilder(L10nValidSample);
        for (var k = r.Next(1, 4); k > 0; k--)
        {
            var i = r.Next(sb.Length);
            switch (r.Next(3))
            {
                case 0: sb.Remove(i, 1); break;
                case 1: sb.Insert(i, L10nTokens[r.Next(L10nTokens.Length)]); break;
                default: sb[i] = chars[r.Next(chars.Length)]; break;
            }
        }

        return sb.ToString();
    }

    /// <summary>For each text: does System.Text.Json (Skip comments, trailing commas, max depth 64) accept it at all, is it a usable flat
    /// catalog for System.Text.Json, and what does the generator's hand-written FlatJsonReader say (they must agree on the flat verdict).</summary>
    public static JsonArray L10nFlatJsonCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var texts = new List<(string Id, string Text)>();
        foreach (var node in inputs["texts"]?.AsArray() ?? [])
            texts.Add((node!["id"]!.GetValue<string>(), node["text"]!.GetValue<string>()));
        foreach (var seed in inputs["soup_seeds"]?.AsArray() ?? [])
        {
            var s = seed!["seed"]!.GetValue<int>();
            var r = new Random(s);
            for (var i = 0; i < seed["count"]!.GetValue<int>(); i++)
                texts.Add(($"soup-{s}-{i}", i % 2 == 0 ? L10nSoup(r) : L10nMutateValid(r)));
        }

        var result = new JsonArray();
        foreach (var (id, text) in texts)
        {
            var expanded = text.Replace("{{DEEP_ARRAYS}}", new string('[', 5000) + new string(']', 5000), StringComparison.Ordinal);
            var stj = L10nStjFlat(expanded, out var jsonValid);
            var gen = L10nGeneratorFlat(expanded);
            result.Add(new JsonObject
            {
                ["id"] = id,
                ["text"] = text,
                ["json_valid"] = jsonValid,
                ["stj_flat"] = L10nEntries(stj),
                ["generator_flat"] = L10nEntries(gen),
                ["agree"] = (stj is null) == (gen is null) && (stj is null || stj.SequenceEqual(gen!)),
            });
        }

        return result;
    }
}
