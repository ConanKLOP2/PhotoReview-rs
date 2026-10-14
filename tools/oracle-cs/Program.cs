using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;

// Usage: oracle-cs <command> <inputs.json> <output.json> <csharp-commit>
// Each command reads its case list from tests/golden-inputs/<command>.json and writes tests/golden/<command>.json:
//   { "generator": "oracle-cs <command>", "csharp_commit": "<sha>", "cases": [ { ...input, ...expected } ] }
// Rust tests read the golden file only; they never run this tool.

if (args.Length != 4)
{
    Console.Error.WriteLine("usage: oracle-cs <command> <inputs.json> <output.json> <csharp-commit>");
    return 2;
}

var (command, inputPath, outputPath, commit) = (args[0], args[1], args[2], args[3]);
var inputs = JsonNode.Parse(File.ReadAllText(inputPath))!.AsObject();

if (command == "ordinal-casing-table")
{
    // Writes Rust source, not JSON: the table pr_core::text uses to reproduce StringComparison.OrdinalIgnoreCase.
    var rustPath = inputs["rust_output"]!.GetValue<string>();
    var full = Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!, rustPath);
    var count = Commands.WriteOrdinalCasingTable(full, commit);
    Console.WriteLine($"{command}: {count} mapping(s) -> {full}");
    return 0;
}

JsonArray cases = command switch
{
    "datetime-json" => Commands.DateTimeJson(inputs),
    "ordinal-ignore-case" => Commands.OrdinalIgnoreCase(inputs),
    "natural-cmp" => Commands.NaturalCompare(inputs),
    _ => throw new ArgumentException($"unknown command '{command}'"),
};

var doc = new JsonObject
{
    ["generator"] = $"oracle-cs {command}",
    ["csharp_commit"] = commit,
    ["cases"] = cases,
};
Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(outputPath))!);
var options = new JsonSerializerOptions { WriteIndented = true, Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping };
File.WriteAllText(outputPath, doc.ToJsonString(options) + "\n");
Console.WriteLine($"{command}: {cases.Count} case(s) -> {outputPath}");
return 0;

static class Commands
{
    private static readonly JsonSerializerOptions Plain = new();

    /// <summary>
    /// How System.Text.Json writes and reads a UTC DateTime (the on-disk form of every journal/session timestamp).
    /// Input: "ticks" (UTC DateTime ticks to write) and "parse" (texts to read back).
    /// </summary>
    public static JsonArray DateTimeJson(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["ticks"]!.AsArray())
        {
            var ticks = node!.GetValue<long>();
            var value = new DateTime(ticks, DateTimeKind.Utc);
            result.Add(new JsonObject
            {
                ["kind"] = "write",
                ["ticks"] = ticks,
                ["json"] = JsonSerializer.Deserialize<string>(JsonSerializer.Serialize(value, Plain)),
            });
        }
        foreach (var node in inputs["parse"]!.AsArray())
        {
            var text = node!.GetValue<string>();
            var item = new JsonObject { ["kind"] = "parse", ["json"] = text };
            try
            {
                var parsed = JsonSerializer.Deserialize<DateTime>(JsonSerializer.Serialize(text, Plain), Plain);
                // ToUniversalTime of a Local value depends on this machine's zone; inputs only use Z, offsets or no zone.
                var utc = parsed.Kind == DateTimeKind.Local ? parsed.ToUniversalTime() : parsed;
                item["ok"] = true;
                item["ticks"] = utc.Ticks;
                item["source_kind"] = parsed.Kind.ToString();
            }
            catch (JsonException)
            {
                item["ok"] = false;
            }
            result.Add(item);
        }
        return result;
    }

    /// <summary>string.Equals / string.Compare with StringComparison.OrdinalIgnoreCase (paths, file names).</summary>
    public static JsonArray OrdinalIgnoreCase(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var pair in inputs["pairs"]!.AsArray())
        {
            var a = pair![0]!.GetValue<string>();
            var b = pair[1]!.GetValue<string>();
            result.Add(new JsonObject
            {
                ["a"] = a,
                ["b"] = b,
                ["equals"] = string.Equals(a, b, StringComparison.OrdinalIgnoreCase),
                ["compare"] = Math.Sign(string.Compare(a, b, StringComparison.OrdinalIgnoreCase)),
            });
        }
        return result;
    }

    /// <summary>
    /// Every code point (surrogates excluded) that OrdinalIgnoreCase treats as equal to a different single code point,
    /// mapped to that representative. The representative is the first of ToUpperInvariant(c), ToUpperInvariant(
    /// ToLowerInvariant(c)) that .NET itself reports equal, so e.g. KELVIN SIGN maps to 'K' while DOTLESS I stays itself.
    /// </summary>
    public static int WriteOrdinalCasingTable(string rustPath, string commit)
    {
        var pairs = new List<(int From, int To)>();
        for (var cp = 0x80; cp <= 0x10FFFF; cp++)
        {
            if (cp is >= 0xD800 and <= 0xDFFF) continue;
            var s = char.ConvertFromUtf32(cp);
            foreach (var candidate in new[] { s.ToUpperInvariant(), s.ToLowerInvariant().ToUpperInvariant() })
            {
                if (candidate == s || !IsSingleCodePoint(candidate)) continue;
                if (!string.Equals(s, candidate, StringComparison.OrdinalIgnoreCase)) continue;
                pairs.Add((cp, char.ConvertToUtf32(candidate, 0)));
                break;
            }
        }
        var sb = new System.Text.StringBuilder();
        sb.Append("// @generated by tools/oracle.ps1 -Command ordinal-casing-table from C# PhotoReview ")
          .Append(commit).Append(" (.NET ").Append(Environment.Version).Append("). Do not edit.\n");
        sb.Append("// Non-ASCII code points that StringComparison.OrdinalIgnoreCase maps to another code point, sorted.\n\n");
        sb.Append("pub(super) static ORDINAL_UPPER: &[(u32, u32)] = &[\n");
        foreach (var (from, to) in pairs) sb.Append("    (0x").Append(from.ToString("X4")).Append(", 0x").Append(to.ToString("X4")).Append("),\n");
        sb.Append("];\n");
        Directory.CreateDirectory(Path.GetDirectoryName(rustPath)!);
        File.WriteAllText(rustPath, sb.ToString());
        return pairs.Count;
    }

    private static bool IsSingleCodePoint(string s) =>
        s.Length == 1 ? !char.IsSurrogate(s[0]) : s.Length == 2 && char.IsSurrogatePair(s[0], s[1]);

    /// <summary>ManagedNaturalComparer: pairwise sign and the sorted order of each name list.</summary>
    public static JsonArray NaturalCompare(JsonObject inputs)
    {
        var comparer = ManagedNaturalComparer.Instance;
        var result = new JsonArray();
        foreach (var pair in inputs["pairs"]!.AsArray())
        {
            var a = pair![0]?.GetValue<string>();
            var b = pair[1]?.GetValue<string>();
            result.Add(new JsonObject
            {
                ["kind"] = "pair",
                ["a"] = a,
                ["b"] = b,
                ["compare"] = Math.Sign(comparer.Compare(a, b)),
            });
        }
        foreach (var list in inputs["lists"]!.AsArray())
        {
            var names = list!.AsArray().Select(n => n!.GetValue<string>()).ToList();
            var sorted = names.ToList();
            sorted.Sort(comparer);
            result.Add(new JsonObject
            {
                ["kind"] = "sort",
                ["input"] = new JsonArray(names.Select(n => (JsonNode?)n).ToArray()),
                ["sorted"] = new JsonArray(sorted.Select(n => (JsonNode?)n).ToArray()),
            });
        }
        return result;
    }
}
