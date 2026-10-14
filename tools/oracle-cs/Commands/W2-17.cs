using System.Globalization;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Model;

// W2-17 oracle: PerfCsvListener row mapping / escaping / file format, PhotoReviewPerf.PathId, ReviewMetrics.
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `command switch`):
//     "perf-csv-escape" => Commands.PerfCsvEscapeCases(inputs),
//     "perf-csv-row" => Commands.PerfCsvRowCases(inputs),
//     "perf-csv-file" => Commands.PerfCsvFileCases(inputs),
//     "perf-path-id" => Commands.PerfPathIdCases(inputs),
//     "review-metrics" => Commands.ReviewMetricsCases(inputs),
//
// Needs: PhotoReview.Core only (internal members are reached by reflection; the oracle has no InternalsVisibleTo).
static partial class Commands
{
    private const BindingFlags W217NonPublicStatic = BindingFlags.NonPublic | BindingFlags.Static;

    /// <summary>PerfCsvListener.CsvEscape (internal): quoting and line-break flattening of one field.</summary>
    public static JsonArray PerfCsvEscapeCases(JsonObject inputs)
    {
        var escape = typeof(PerfCsvListener).GetMethod("CsvEscape", W217NonPublicStatic)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var value = node!["value"]!.GetValue<string>();
            result.Add(new JsonObject { ["value"] = value, ["escaped"] = (string)escape.Invoke(null, [value])! });
        }
        return result;
    }

    /// <summary>Typed payload value: {"t":"long|int|double|bool|string|null","v":...} (double also accepts "bits":"3FB999999999999A").</summary>
    private static object? W217Value(JsonNode? node)
    {
        if (node is null) return null;
        var t = node["t"]!.GetValue<string>();
        var v = node["v"];
        return t switch
        {
            "null" => null,
            "long" => v!.GetValue<long>(),
            "int" => v!.GetValue<int>(),
            "bool" => v!.GetValue<bool>(),
            "string" => v!.GetValue<string>(),
            "double" => node["bits"] is { } bits
                ? BitConverter.Int64BitsToDouble(long.Parse(bits.GetValue<string>(), NumberStyles.HexNumber, CultureInfo.InvariantCulture))
                : v!.GetValueKind() == JsonValueKind.String
                    ? double.Parse(v.GetValue<string>() == "-0" ? "-0" : v.GetValue<string>(), NumberStyles.Float, CultureInfo.InvariantCulture)
                    : v.GetValue<double>(),
            _ => throw new ArgumentException("unknown payload type " + t),
        };
    }

    /// <summary>PerfCsvListener.BuildRow (internal): payload names/values -> nav, pathId, a..d, text. The three machine-dependent
    /// columns (utcTicks, qpcTicks, thread) are not reported.</summary>
    public static JsonArray PerfCsvRowCases(JsonObject inputs)
    {
        var build = typeof(PerfCsvListener).GetMethod("BuildRow", W217NonPublicStatic, [typeof(string), typeof(IReadOnlyList<string>), typeof(IReadOnlyList<object>)])!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            string[]? names = c["names"] is JsonArray n ? n.Select(x => x!.GetValue<string>()).ToArray() : null;
            object?[]? values = c["values"] is JsonArray v ? v.Select(W217Value).ToArray() : null;
            var row = build.Invoke(null, [c["event"]!.GetValue<string>(), names, values])!;
            string P(string name) => (string)row.GetType().GetProperty(name, BindingFlags.Public | BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(row)!;
            var item = (JsonObject)c.DeepClone();
            item["out"] = new JsonObject
            {
                ["event"] = P("EventName"), ["nav"] = P("Nav"), ["pathId"] = P("PathId"),
                ["a"] = P("A"), ["b"] = P("B"), ["c"] = P("C"), ["d"] = P("D"), ["text"] = P("Text"),
            };
            result.Add(item);
        }
        return result;
    }

    private static readonly string[] W217DiagVars = ["PHOTOREVIEW_DIAG_PREREAD", "PHOTOREVIEW_DIAG_PRELOAD_WORKERS", "PHOTOREVIEW_DIAG_DISABLE_DISKCACHE"];

    private static object? W217EventArg(JsonNode? node, Type type) => node is null ? null : type switch
    {
        _ when type == typeof(long) => node.GetValue<long>(),
        _ when type == typeof(int) => node.GetValue<int>(),
        _ when type == typeof(bool) => node.GetValue<bool>(),
        _ when type == typeof(string) => node.GetValue<string>(),
        _ when type == typeof(double) => node.GetValueKind() == JsonValueKind.String
            ? double.Parse(node.GetValue<string>(), NumberStyles.Float, CultureInfo.InvariantCulture)
            : node.GetValue<double>(),
        _ => throw new ArgumentException("unsupported event parameter type " + type),
    };

    /// <summary>Whole listener round trip: set the DIAG variables, start a listener on a MemoryStream (StartForTest), call
    /// PhotoReviewPerf.Log.&lt;event&gt;(args) for each event, dispose, return the file text with the machine-dependent
    /// fields masked (header commit and qpcFrequency, first three columns of every data row). Line endings are kept exactly
    /// (header, column header and trailer end CRLF, data rows end LF).</summary>
    public static JsonArray PerfCsvFileCases(JsonObject inputs)
    {
        var start = typeof(PerfCsvListener).GetMethod("StartForTest", W217NonPublicStatic)!;
        var saved = W217DiagVars.ToDictionary(n => n, Environment.GetEnvironmentVariable);
        var result = new JsonArray();
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var env = c["env"]?.AsObject();
                foreach (var name in W217DiagVars) Environment.SetEnvironmentVariable(name, env?[name]?.GetValue<string>());
                var ms = new MemoryStream();
                var listener = (PerfCsvListener)start.Invoke(null, [ms, 1000, null, null])!;
                foreach (var e in c["events"]?.AsArray() ?? [])
                {
                    var method = typeof(PhotoReviewPerf).GetMethod(e!["name"]!.GetValue<string>(), BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly)!;
                    var ps = method.GetParameters();
                    var args = e["args"]!.AsArray().Select((a, i) => W217EventArg(a, ps[i].ParameterType)).ToArray();
                    method.Invoke(PhotoReviewPerf.Log, args);
                }
                listener.Dispose();
                var text = new UTF8Encoding(false).GetString(ms.ToArray());
                var lines = text.Split('\n');
                for (var i = 0; i < lines.Length; i++)
                {
                    var line = lines[i];
                    if (line.StartsWith("# commit=", StringComparison.Ordinal))
                        line = Regex.Replace(Regex.Replace(line, @"commit=\S*", "commit=<commit>"), @"qpcFrequency=\d+", "qpcFrequency=<freq>");
                    else if (!line.StartsWith('#') && !line.StartsWith("utcTicks,", StringComparison.Ordinal))
                        line = Regex.Replace(line, @"^\d+,\d+,\d+,", "<utc>,<qpc>,<thread>,");
                    lines[i] = line;
                }
                var item = (JsonObject)c.DeepClone();
                item["masked_text"] = string.Join("\n", lines);
                result.Add(item);
            }
        }
        finally
        {
            foreach (var (name, value) in saved) Environment.SetEnvironmentVariable(name, value);
        }
        return result;
    }

    /// <summary>PhotoReviewPerf.PathId(path) for absolute paths (the machine-independent part of the contract), plus the
    /// Path.GetFullPath text it hashes before ToUpperInvariant.</summary>
    public static JsonArray PerfPathIdCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["paths"]!.AsArray())
        {
            var path = node!.GetValue<string>();
            result.Add(new JsonObject
            {
                ["path"] = path,
                ["full"] = Path.GetFullPath(path),
                ["id"] = PhotoReviewPerf.PathId(path),
            });
        }
        return result;
    }

    /// <summary>ReviewMetrics: scripted operations, then the serialized snapshot (System.Text.Json, property order and names as
    /// the benchmark report writes them), the EWMA bits and the number of tracked source-open paths.</summary>
    public static JsonArray ReviewMetricsCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var m = new ReviewMetrics();
            var ewma = new JsonArray();
            foreach (var opNode in c["ops"]!.AsArray())
            {
                var op = opNode!.AsObject();
                var name = op["op"]!.GetValue<string>();
                long L(string key, long fallback = 0) => op[key]?.GetValue<long>() ?? fallback;
                switch (name)
                {
                    case "cacheHit": for (var i = 0; i < L("times", 1); i++) m.RecordCacheHit(); break;
                    case "cacheMiss": for (var i = 0; i < L("times", 1); i++) m.RecordCacheMiss(); break;
                    case "sourceRead": m.RecordSourceRead(L("bytes"), L("ms"), op["ewma"]?.GetValue<bool>() ?? true); break;
                    case "presented": m.RecordPresented(L("ms")); break;
                    case "preloadHit": m.RecordPreloadHit(); break;
                    case "inflightJoin": m.RecordInflightJoin(); break;
                    case "diskCacheHit": m.RecordDiskCacheHit(); break;
                    case "uiAssign": m.RecordUiAssign(L("ms")); break;
                    case "fallback": m.RecordDecoderFallback(Enum.Parse<DecoderBackend>(op["backend"]!.GetValue<string>())); break;
                    case "sourceOpen":
                        for (var i = 0; i < L("times", 1); i++) m.RecordSourceOpen(op["path"]!.GetValue<string>());
                        break;
                    case "sourceOpenRange":
                        // prefix + zero-padded index, count distinct paths, each opened once
                        for (var i = 0; i < L("count"); i++) m.RecordSourceOpen(op["prefix"]!.GetValue<string>() + i.ToString("D5", CultureInfo.InvariantCulture) + op["suffix"]!.GetValue<string>());
                        break;
                    case "stat": m.RecordStat(); break;
                    case "sessionWrite": m.RecordSessionWrite(); break;
                    case "crossThreadPresent": m.RecordCrossThreadPresent(); break;
                    case "snapshotEwma": break;
                    default: throw new ArgumentException("unknown op " + name);
                }
                if (name == "sourceRead") ewma.Add(BitConverter.DoubleToInt64Bits(m.DecodeMillisecondsEwma).ToString("X16", CultureInfo.InvariantCulture));
            }
            var json = JsonSerializer.Serialize(m.Snapshot());
            var tracked = (int)typeof(ReviewMetrics).GetProperty("TrackedSourceOpenPaths", BindingFlags.NonPublic | BindingFlags.Instance)!.GetValue(m)!;
            var item = (JsonObject)c.DeepClone();
            var snapshot = JsonNode.Parse(json)!.AsObject();
            if (c["stable_top_only"]?.GetValue<bool>() == true)
            {
                // Ties among once-opened paths after a trim depend on string hash randomization: keep only the entries with Count > 1.
                var top = snapshot["TopSourceOpens"]!.AsArray();
                var kept = new JsonArray(top.Where(e => e!["Count"]!.GetValue<long>() > 1).Select(e => e!.DeepClone()).ToArray());
                snapshot["TopSourceOpens"] = kept;
            }
            else
            {
                item["snapshot_json"] = json;
            }
            item["snapshot"] = snapshot;
            item["ewma_bits_after_each_source_read"] = ewma;
            item["ewma_bits"] = BitConverter.DoubleToInt64Bits(m.DecodeMillisecondsEwma).ToString("X16", CultureInfo.InvariantCulture);
            item["tracked_source_open_paths"] = tracked;
            result.Add(item);
        }
        return result;
    }
}
