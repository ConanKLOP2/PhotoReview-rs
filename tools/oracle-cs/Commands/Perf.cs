using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using PhotoReview.PerfAnalysis;

/// <summary>
/// Golden commands for the pr-perf crate (W1-16 .. W1-21): perf-csv, perf-nav, perf-stats, perf-rules, perf-report,
/// perf-analyze. Public API of PhotoReview.PerfAnalysis.dll only (NaNAsNullDoubleConverter is internal, so a
/// behaviour-identical copy is used for the json_double cases).
/// Conventions: doubles that are NaN/Infinity are written as JSON null (JsonNode cannot hold them); doubles in INPUTS may be
/// JSON numbers or the strings "NaN", "Infinity", "-Infinity", "1e999"; maps are written as [[key, value], ...] to keep
/// the C# enumeration order.
/// </summary>
static class PerfCommands
{
    /// <summary>tests/fixtures: set by Program.cs. Used by {"fixture": "diag/perf-sample.csv"} content nodes.</summary>
    public static string FixturesDir = "";

    private static readonly CultureInfo Inv = CultureInfo.InvariantCulture;

    // ------------------------------------------------------------------ helpers

    private static JsonNode? D(double? v) => v is { } x && double.IsFinite(x) ? JsonValue.Create(x) : null;
    private static JsonNode? L(long? v) => v is { } x ? JsonValue.Create(x) : null;
    private static JsonNode? I(int? v) => v is { } x ? JsonValue.Create(x) : null;
    private static JsonNode? S(string? v) => v is null ? null : JsonValue.Create(v);
    private static JsonNode? B(bool? v) => v is { } x ? JsonValue.Create(x) : null;

    private static double Num(JsonNode? n) => n switch
    {
        null => throw new ArgumentException("missing number"),
        JsonValue v when v.TryGetValue<string>(out var s) => double.Parse(s, NumberStyles.Float, Inv),
        _ => n.GetValue<double>(),
    };

    private static double? NumOrNull(JsonNode? n) => n is null ? null : Num(n);
    private static int? IntOrNull(JsonNode? n) => n is null ? null : n.GetValue<int>();

    private static JsonArray Pairs<T>(IEnumerable<KeyValuePair<string, T>> map, Func<T, JsonNode?> value)
    {
        var a = new JsonArray();
        foreach (var kv in map) a.Add(new JsonArray(JsonValue.Create(kv.Key), value(kv.Value)));
        return a;
    }

    private static string NewTemp() => Directory.CreateTempSubdirectory("oracle-perf-").FullName;

    private static void Cleanup(string dir)
    {
        try { Directory.Delete(dir, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
    }

    private static string FullPath(string root, string rel) => Path.Combine(root, rel.Replace('/', Path.DirectorySeparatorChar));

    private static string WriteFile(string root, string rel, string content, bool bom = false)
    {
        var full = FullPath(root, rel);
        Directory.CreateDirectory(Path.GetDirectoryName(full)!);
        File.WriteAllText(full, content, new UTF8Encoding(bom));
        return full;
    }

    /// <summary>A content node is a string, {"fixture": "<path under tests/fixtures>"} or {"gen": {...}} (see GenCsv).
    /// The expanded text is echoed into the output case so Rust tests never need the fixture or the generator.</summary>
    private static string Content(JsonNode? node)
    {
        if (node is JsonValue v && v.TryGetValue<string>(out var s)) return s;
        var o = node!.AsObject();
        if (o["fixture"] is { } f) return File.ReadAllText(Path.Combine(FixturesDir, f.GetValue<string>()));
        if (o["gen"] is JsonObject g) return GenCsv(g);
        throw new ArgumentException("content must be a string, {fixture} or {gen}");
    }

    /// <summary>Deterministic synthetic perf CSV (qpcFrequency 10 MHz: 10_000 ticks = 1 ms). Input {"navs": N, "workers": W?}:
    /// N navigations cycling through six shapes (kind index = kinds[(n-1) % kinds.Length], default [1,2,3,4,5,0]: 0 RamHit, 1 SourceMiss, 2 DiskCacheHit, 3 InflightJoin,
    /// 4 Thumbnail+SourceMiss, 5 superseded = Incomplete), plus Startup, Folder, Preload*, DispatcherLongOp and PostEnd rows.
    /// Optional decode_pct (default 100) scales the Decode durations of shapes 1 and 4 (integer tenths: tenths * pct / 100).</summary>
    private static string GenCsv(JsonObject g)
    {
        var navs = g["navs"]!.GetValue<int>();
        var workers = g["workers"]?.GetValue<int>();
        var decodePct = g["decode_pct"]?.GetValue<int>() ?? 100;
        var shapes = g["kinds"]?.AsArray().Select(k => k!.GetValue<int>()).ToArray() ?? [1, 2, 3, 4, 5, 0];
        var sb = new StringBuilder();
        sb.Append("# commit=gen diag=").Append(workers is { } w ? "PHOTOREVIEW_DIAG_PRELOAD_WORKERS=" + w.ToString(Inv) : "").Append(" qpcFrequency=10000000\n");
        sb.Append("utcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text\n");
        const long utcBase = 638600000000000000L;
        long last = 0;
        void Row(long qpc, int thread, string ev, string nav, string pathId, string a, string b, string c, string d, string text)
        {
            sb.Append((utcBase + qpc).ToString(Inv)).Append(',').Append(qpc.ToString(Inv)).Append(',').Append(thread.ToString(Inv)).Append(',')
              .Append(ev).Append(',').Append(nav).Append(',').Append(pathId).Append(',').Append(a).Append(',').Append(b).Append(',')
              .Append(c).Append(',').Append(d).Append(',').Append(text).Append('\n');
            if (qpc > last) last = qpc;
        }
        static string Ms(long tenths) => (tenths / 10).ToString(Inv) + "." + (tenths % 10).ToString(Inv);

        long t = 1_000_000;
        Row(t, 1, "Startup", "", "", "250", "", "", "", "appStartup");
        Row(t + 500_000, 1, "Startup", "", "", "300", "", "", "", "servicesBuilt");
        Row(t + 600_000, 1, "DispatcherLongOp", "", "", "700.0", "", "", "", "Normal;WindowCreate"); // before the first Folder(start): excluded
        t += 2_000_000;
        Row(t, 1, "Folder", "1", "", "0", "", "", "", "start");
        Row(t + 1_000_000, 1, "Folder", "1", "", "120.5", "", "", "", "catalogReady");
        Row(t + 1_500_000, 1, "Folder", "1", "", "180.0", "", "", "", "explorerApplied");
        t += 2_500_000;
        for (var n = 1; n <= navs; n++)
        {
            var nav = n.ToString(Inv);
            var pid = "p" + (1000 + n).ToString(Inv);
            var kind = shapes[(n - 1) % shapes.Length];
            t = last + 150_000 + (n * 37 % 11) * 10_000; // 15..25 ms after the previous nav's last row
            Row(t, 1, "KeyInput", "", "", Ms(10 + n * 7 % 40), "", "", "", "Right");
            var s = t + 20_000 + (n % 4) * 5_000;
            Row(s, 1, "ShowStart", nav, "", ((n - 1) % 5).ToString(Inv), "", "", "", n % 3 == 0 ? "Fast" : "Preview");
            Row(s + 5_000, 1, "Stat", nav, "", Ms(8 + n % 5), "", "", "", "");
            long work; // ticks between the Lookup and the Assign
            switch (kind)
            {
                case 0:
                    Row(s + 10_000, 1, "Lookup", nav, pid, "", "", "", "", "ramHit");
                    work = 5_000;
                    break;
                case 1:
                    Row(s + 10_000, 1, "Lookup", nav, pid, "", "", "", "", "miss");
                    Row(s + 100_000, 4, "SourceRead", nav, pid, Ms(80 + n % 7 * 10), "", "", "", "");
                    Row(s + 500_000, 4, "Decode", nav, pid, Ms((300 + n * 13 % 40 * 10) * decodePct / 100), "1920", "1", "0", "");
                    Row(s + 900_000, 4, "Verify", nav, pid, "0.5", "", "", "", "");
                    work = 900_000;
                    break;
                case 2:
                    Row(s + 10_000, 1, "Lookup", nav, pid, "", "", "", "", "miss");
                    Row(s + 200_000, 4, "DiskCacheRead", nav, pid, Ms(100 + n % 9 * 10), "204800", "", "", "");
                    Row(s + 300_000, 4, "Verify", nav, pid, "0.5", "", "", "", "");
                    work = 300_000;
                    break;
                case 3:
                    Row(s + 10_000, 1, "Lookup", nav, pid, "", "", "", "", "inflight");
                    Row(s + 20_000, 1, "JoinStart", nav, pid, "", "", "", "", "");
                    Row(s + 200_000, 1, "JoinEnd", nav, pid, Ms(120 + n % 6 * 10), "", "", "", "");
                    work = 200_000;
                    break;
                case 4:
                    Row(s + 10_000, 1, "Lookup", nav, pid, "", "", "", "", "miss");
                    Row(s + 20_000, 1, "ThumbStart", nav, pid, "", "", "", "", "");
                    Row(s + 50_000, 4, "ThumbEnd", nav, pid, "20.0", "", "", "", "decode");
                    Row(s + 50_500, 1, "ThumbEnd", nav, pid, "20.5", "", "", "", "unknown");
                    Row(s + 60_000, 1, "Presented", nav, "", "", "", "", "", "thumbnail");
                    Row(s + 100_000, 4, "Decode", nav, pid, Ms((400 + n % 5 * 10) * decodePct / 100), "2560", "1", "0", "");
                    Row(s + 600_000, 4, "Verify", nav, pid, "0.5", "", "", "", "");
                    work = 600_000;
                    break;
                default: // 5: superseded mid-decode, never presented
                    Row(s + 10_000, 1, "Lookup", nav, pid, "", "", "", "", "miss");
                    Row(s + 100_000, 4, "Decode", nav, pid, "48.0", "1920", "1", "0", "");
                    work = -1;
                    break;
            }
            if (work >= 0)
            {
                var e = s + 10_000 + work;
                Row(e + 10_000, 1, "Assign", nav, "", Ms(15 + n % 10), "1920", "1080", "", "");
                Row(e + 40_000, 1, "Rendered", nav, "", Ms(30 + n % 8), "", "", "", "");
                Row(e + 50_000, 1, "Presented", nav, "", "", "", "", "", "final");
                Row(e + 100_000, 1, "RenderedFrame", nav, "", Ms(180 + n % 30), "", "", "", "");
                if (n % 4 == 0)
                {
                    Row(e + 200_000, 6, "PostEnd", nav, "", Ms(50 + n % 9), "", "", "", "preloadKick");
                    Row(e + 210_000, 6, "PostEnd", nav, "", Ms(20 + n % 5), "", "", "", "hash");
                }
            }
            if (n % 3 == 0)
                Row(s + 300_000, 5, "PreloadItem", "", pid, (n % 4).ToString(Inv), Ms(200 + n % 30), Ms(1000 + n % 7 * 50), "", n % 2 == 0 ? "decoded" : (n % 9 == 0 ? "skipped" : "hit"));
            if (n % 10 == 0) Row(s + 310_000, 5, "PreloadPaused", "", "", "85", "512", "", "", "");
            if (n % 15 == 0) Row(s + 320_000, 5, "PreloadCancel", "", "", "3", "", "", "", "");
            if (n % 7 == 0)
                Row(s + 330_000, 1, "DispatcherLongOp", "", "", n % 14 == 0 ? "12.5" : Ms(170 + n % 50), "", "", "", n % 2 == 0 ? "Background;SessionSave" : "Normal;LayoutUpdate");
        }
        sb.Append("# dropped=0\n");
        return sb.ToString();
    }

    /// <summary>Writes a {"rel/path": content-node, ...} map under root; returns the map with every content expanded to text.</summary>
    private static JsonObject WriteFiles(string root, JsonObject files)
    {
        var echoed = new JsonObject();
        foreach (var (rel, node) in files)
        {
            if (node is JsonObject o && o["raw_hex"] is { } hex)
            {
                var full = FullPath(root, rel);
                Directory.CreateDirectory(Path.GetDirectoryName(full)!);
                File.WriteAllBytes(full, Convert.FromHexString(hex.GetValue<string>()));
                echoed[rel] = new JsonObject { ["raw_hex"] = hex.GetValue<string>() };
                continue;
            }
            var bom = node is JsonObject ob && (ob["bom"]?.GetValue<bool>() ?? false);
            var text = Content(node is JsonObject oc && oc["content"] is { } c ? c : node);
            WriteFile(root, rel, text, bom);
            echoed[rel] = bom ? new JsonObject { ["content"] = text, ["bom"] = true } : JsonValue.Create(text);
        }
        return echoed;
    }

    // ------------------------------------------------------------------ nav spec <-> NavRecord

    /// <summary>Nav spec fields (all optional): nav (default 1), mode, t_input, t_pre, t_stat, lookup_result, t_thumb,
    /// has_thumbnail, t_join, t_disk, t_read, t_decode, t_verify, t_assign, t_render, t_render_frame, post_ms {name: ms},
    /// first_visual, final_visual, incomplete (default FALSE here, unlike the C# default true), kind (default "Unknown").</summary>
    private static NavRecord NavFromJson(JsonNode node)
    {
        var o = node.AsObject();
        var n = new NavRecord
        {
            Nav = o["nav"]?.GetValue<long>() ?? 1,
            Mode = o["mode"]?.GetValue<string>() ?? "",
            TInputMs = NumOrNull(o["t_input"]),
            TPreMs = NumOrNull(o["t_pre"]),
            TStatMs = NumOrNull(o["t_stat"]),
            LookupResult = o["lookup_result"]?.GetValue<string>() ?? "",
            TThumbMs = NumOrNull(o["t_thumb"]),
            HasThumbnail = o["has_thumbnail"]?.GetValue<bool>() ?? false,
            TJoinMs = NumOrNull(o["t_join"]),
            TDiskMs = NumOrNull(o["t_disk"]),
            TReadMs = NumOrNull(o["t_read"]),
            TDecodeMs = NumOrNull(o["t_decode"]),
            TVerifyMs = NumOrNull(o["t_verify"]),
            TAssignMs = NumOrNull(o["t_assign"]),
            TRenderMs = NumOrNull(o["t_render"]),
            TRenderFrameMs = NumOrNull(o["t_render_frame"]),
            FirstVisualMs = NumOrNull(o["first_visual"]),
            FinalVisualMs = NumOrNull(o["final_visual"]),
            Incomplete = o["incomplete"]?.GetValue<bool>() ?? false,
            Kind = o["kind"]?.GetValue<string>() ?? "Unknown",
        };
        if (o["post_ms"] is JsonObject post) foreach (var (k, v) in post) n.PostMs[k] = Num(v);
        return n;
    }

    private static JsonObject NavToJson(NavRecord n) => new()
    {
        ["nav"] = n.Nav,
        ["mode"] = n.Mode,
        ["t_input"] = D(n.TInputMs),
        ["t_pre"] = D(n.TPreMs),
        ["t_stat"] = D(n.TStatMs),
        ["lookup_result"] = n.LookupResult,
        ["t_thumb"] = D(n.TThumbMs),
        ["has_thumbnail"] = n.HasThumbnail,
        ["t_join"] = D(n.TJoinMs),
        ["t_disk"] = D(n.TDiskMs),
        ["t_read"] = D(n.TReadMs),
        ["t_decode"] = D(n.TDecodeMs),
        ["t_verify"] = D(n.TVerifyMs),
        ["t_assign"] = D(n.TAssignMs),
        ["t_render"] = D(n.TRenderMs),
        ["t_render_frame"] = D(n.TRenderFrameMs),
        ["post_ms"] = Pairs(n.PostMs, v => D(v)),
        ["first_visual"] = D(n.FirstVisualMs),
        ["final_visual"] = D(n.FinalVisualMs),
        ["incomplete"] = n.Incomplete,
        ["kind"] = n.Kind,
    };

    /// <summary>A nav list is either an array of nav specs or {"seq": {...}}: navs 1..count with FinalVisualMs = i * final_step;
    /// optional first_step (FirstVisualMs = i * first_step), frame_step (TRenderFrameMs = i * frame_step), incomplete_after k
    /// (navs i &gt; k are Incomplete), kinds [..] (Kind cycles through the list by i - 1), t_decode_last K + t_decode_value V
    /// (the last K navs get TDecodeMs = V). Everything else keeps the NavFromJson defaults.</summary>
    private static List<NavRecord> Navs(JsonNode? node)
    {
        if (node is null) return [];
        if (node is JsonObject o && o["seq"] is JsonObject s)
        {
            var count = s["count"]!.GetValue<int>();
            var step = Num(s["final_step"]);
            var firstStep = NumOrNull(s["first_step"]);
            var frameStep = NumOrNull(s["frame_step"]);
            var incompleteAfter = s["incomplete_after"]?.GetValue<int>();
            var kinds = s["kinds"]?.AsArray().Select(k => k!.GetValue<string>()).ToList();
            var decodeLast = s["t_decode_last"]?.GetValue<int>() ?? 0;
            var decodeValue = NumOrNull(s["t_decode_value"]);
            var list = new List<NavRecord>();
            for (var i = 1; i <= count; i++)
            {
                var n = new NavRecord { Nav = i, FinalVisualMs = i * step, Incomplete = incompleteAfter is { } a && i > a };
                if (firstStep is { } f) n.FirstVisualMs = i * f;
                if (frameStep is { } fr) n.TRenderFrameMs = i * fr;
                if (kinds is { Count: > 0 }) n.Kind = kinds[(i - 1) % kinds.Count];
                if (i > count - decodeLast) n.TDecodeMs = decodeValue;
                list.Add(n);
            }
            return list;
        }
        return [.. node.AsArray().Select(n => NavFromJson(n!))];
    }

    /// <summary>"sorted" of a nearest_rank case: an array of numbers or {"range": N} = 1.0, 2.0 .. N.0.</summary>
    private static List<double> Sorted(JsonNode node) =>
        node is JsonObject o ? [.. Enumerable.Range(1, o["range"]!.GetValue<int>()).Select(i => (double)i)] : [.. node.AsArray().Select(x => Num(x))];

    private static FolderGenSummary GenFromJson(JsonNode node)
    {
        var o = node.AsObject();
        return new FolderGenSummary
        {
            Gen = o["gen"]?.GetValue<long>() ?? 1,
            T1CatalogReadyMs = NumOrNull(o["t1"]),
            T2Ms = NumOrNull(o["t2"]),
            T3Ms = NumOrNull(o["t3"]),
            T3Phase = o["t3_phase"]?.GetValue<string>() ?? "",
        };
    }

    private static JsonObject GenToJson(FolderGenSummary g) => new()
    {
        ["gen"] = g.Gen, ["t1"] = D(g.T1CatalogReadyMs), ["t2"] = D(g.T2Ms), ["t3"] = D(g.T3Ms), ["t3_phase"] = g.T3Phase,
    };

    // ------------------------------------------------------------------ perf-csv (W1-16)

    /// <summary>inputs: files [{name, content|{fixture}|{gen}, bom?, raw_hex?}], nums [text], qpc [{freq, delta}].
    /// Writes perf-&lt;name&gt;.csv, reads it with PerfCsvReader.Read and dumps header metadata + every row.</summary>
    public static JsonArray Csv(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["files"]!.AsArray())
        {
            var spec = node!.AsObject();
            var name = spec["name"]!.GetValue<string>();
            var root = NewTemp();
            try
            {
                var rel = "perf-" + name + ".csv";
                JsonObject echoed;
                if (spec["raw_hex"] is { } hex)
                {
                    Directory.CreateDirectory(root);
                    File.WriteAllBytes(FullPath(root, rel), Convert.FromHexString(hex.GetValue<string>()));
                    echoed = new JsonObject { ["raw_hex"] = hex.GetValue<string>() };
                }
                else
                {
                    var text = Content(spec["content"]);
                    var bom = spec["bom"]?.GetValue<bool>() ?? false;
                    WriteFile(root, rel, text, bom);
                    echoed = new JsonObject { ["content"] = text, ["bom"] = bom };
                }
                var file = PerfCsvReader.Read(FullPath(root, rel));
                var rows = new JsonArray();
                foreach (var r in file.Rows)
                {
                    rows.Add(new JsonArray(
                        r.UtcTicks, r.QpcTicks, r.Thread, r.Event, r.Nav, r.PathId, r.A, r.B, r.C, r.D, r.Text,
                        D(r.ANum), D(r.BNum), D(r.CNum), D(r.DNum), L(r.NavId)));
                }
                var item = new JsonObject { ["kind"] = "file", ["name"] = name };
                foreach (var (k, v) in echoed) item[k] = v?.DeepClone();
                item["qpc_frequency"] = file.QpcFrequency;
                item["dropped_rows"] = file.DroppedRows;
                item["diag_flags"] = Pairs(file.DiagFlags, v => JsonValue.Create(v));
                item["rows"] = rows;
                result.Add(item);
            }
            finally { Cleanup(root); }
        }
        foreach (var t in inputs["nums"]!.AsArray())
        {
            var s = t!.GetValue<string>();
            var row = new PerfRow(0, 0, 0, "", s, "", s, s, s, s, "");
            result.Add(new JsonObject { ["kind"] = "num", ["text"] = s, ["a_num"] = D(row.ANum), ["nav_id"] = L(row.NavId) });
        }
        foreach (var q in inputs["qpc"]!.AsArray())
        {
            var freq = q!["freq"]!.GetValue<long>();
            var delta = q["delta"]!.GetValue<long>();
            var file = new PerfCsvFile { Path = "x", DiagFlags = new Dictionary<string, string>(), QpcFrequency = freq, DroppedRows = 0, Rows = [] };
            result.Add(new JsonObject { ["kind"] = "qpc", ["freq"] = freq, ["delta"] = delta, ["ms"] = D(file.QpcToMs(delta)) });
        }
        return result;
    }

    // ------------------------------------------------------------------ perf-nav (W1-19)

    /// <summary>inputs: files [{name, content|{fixture}|{gen}}], classify [{lookup, has_thumbnail, t_disk, t_decode}].</summary>
    public static JsonArray Nav(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["files"]!.AsArray())
        {
            var spec = node!.AsObject();
            var name = spec["name"]!.GetValue<string>();
            var text = Content(spec["content"]);
            var root = NewTemp();
            try
            {
                var path = WriteFile(root, "perf-" + name + ".csv", text);
                var a = PerfAnalyzeNavBuilder.Build(PerfCsvReader.Read(path));
                result.Add(new JsonObject
                {
                    ["kind"] = "build",
                    ["name"] = name,
                    ["content"] = text,
                    ["navs"] = new JsonArray(a.Navs.Select(n => (JsonNode?)NavToJson(n)).ToArray()),
                    ["preload_items"] = new JsonArray(a.PreloadItems.Select(p => (JsonNode?)new JsonObject
                    {
                        ["slot"] = p.Slot, ["path_id"] = p.PathId, ["queue_wait_ms"] = D(p.QueueWaitMs), ["kind"] = p.Kind, ["ms"] = D(p.Ms),
                    }).ToArray()),
                    ["preload_paused_count"] = a.PreloadPausedCount,
                    ["preload_cancel_count"] = a.PreloadCancelCount,
                    ["folder_gens"] = new JsonArray(a.FolderGens.Select(g => (JsonNode?)GenToJson(g)).ToArray()),
                    ["dispatcher_long_ops"] = new JsonArray(a.DispatcherLongOps.Select(d => (JsonNode?)new JsonObject
                    {
                        ["ms"] = D(d.Ms), ["priority"] = d.Priority, ["name"] = d.Name,
                    }).ToArray()),
                    ["dispatcher_long_ops_before_start_count"] = a.DispatcherLongOpsBeforeStartCount,
                    ["startup"] = Pairs(a.Startup, v => D(v)),
                });
            }
            finally { Cleanup(root); }
        }
        foreach (var c in inputs["classify"]!.AsArray())
        {
            var n = new NavRecord
            {
                LookupResult = c!["lookup"]!.GetValue<string>(),
                HasThumbnail = c["has_thumbnail"]!.GetValue<bool>(),
                TDiskMs = NumOrNull(c["t_disk"]),
                TDecodeMs = NumOrNull(c["t_decode"]),
            };
            result.Add(new JsonObject
            {
                ["kind"] = "classify", ["lookup"] = n.LookupResult, ["has_thumbnail"] = n.HasThumbnail,
                ["t_disk"] = D(n.TDiskMs), ["t_decode"] = D(n.TDecodeMs), ["result"] = PerfAnalyzeNavBuilder.ClassifyKind(n),
            });
        }
        return result;
    }

    // ------------------------------------------------------------------ perf-stats (W1-17)

    /// <summary>inputs: nearest_rank [{sorted: [num], p}], decile [{navs: [navspec]}], group_summary [{key, navs}],
    /// run_meta [{files: {rel: text}, csv: rel, diag: {k: v}}], phase_names: true.</summary>
    public static JsonArray Stats(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var c in inputs["nearest_rank"]!.AsArray())
        {
            var sorted = Sorted(c!["sorted"]!);
            var p = Num(c["p"]);
            result.Add(new JsonObject
            {
                ["kind"] = "nearest_rank", ["sorted"] = c["sorted"]!.DeepClone(), ["p"] = c["p"]!.DeepClone(),
                ["value"] = D(PerfStats.NearestRank(sorted, p)),
            });
        }
        if (inputs["sweep_max_n"] is { } sweepNode)
        {
            // For n = 1..max_n and whole percents 0..100 over sorted = 1.0 .. n.0 the nearest-rank value IS the rank.
            var maxN = sweepNode.GetValue<int>();
            var sweep = new JsonArray();
            for (var n = 1; n <= maxN; n++)
            {
                var sorted = Enumerable.Range(1, n).Select(i => (double)i).ToList();
                sweep.Add(new JsonArray(Enumerable.Range(0, 101).Select(p => (JsonNode?)(int)PerfStats.NearestRank(sorted, p)).ToArray()));
            }
            result.Add(new JsonObject { ["kind"] = "nearest_rank_sweep", ["max_n"] = maxN, ["ranks_by_n"] = sweep });
        }
        foreach (var c in inputs["decile"]!.AsArray())
        {
            var navs = Navs(c!["navs"]);
            result.Add(new JsonObject
            {
                ["kind"] = "decile", ["navs"] = c["navs"]!.DeepClone(),
                ["shares"] = Pairs(PerfStats.SlowestDecilePhaseShare(navs), v => D(v)),
            });
        }
        foreach (var c in inputs["group_summary"]!.AsArray())
        {
            var k = c!["key"]!;
            var key = new GroupKey(k["scenario"]!.GetValue<string>(), k["mode"]!.GetValue<string>(), k["cond"]!.GetValue<string>(), IntOrNull(k["workers"]));
            var s = GroupSummary.Build(key, Navs(c["navs"]));
            result.Add(new JsonObject
            {
                ["kind"] = "group_summary", ["key"] = c["key"]!.DeepClone(), ["navs"] = c["navs"]!.DeepClone(),
                ["key_string"] = key.ToString(), ["count"] = s.Count, ["incomplete"] = s.Incomplete, ["low_sample_warning"] = s.LowSampleWarning,
                ["first_p50"] = D(s.FirstP50), ["first_p95"] = D(s.FirstP95), ["first_max"] = D(s.FirstMax),
                ["final_p50"] = D(s.FinalP50), ["final_p95"] = D(s.FinalP95), ["final_max"] = D(s.FinalMax),
                ["rendered_p50"] = D(s.RenderedFrameP50), ["rendered_p95"] = D(s.RenderedFrameP95), ["rendered_max"] = D(s.RenderedFrameMax),
                ["hit_rate"] = D(s.HitRate),
                ["kind_counts"] = Pairs(s.KindCounts, v => JsonValue.Create(v)),
                ["slowest_phase_share"] = Pairs(s.SlowestPhaseShare, v => D(v)),
            });
        }
        foreach (var c in inputs["run_meta"]!.AsArray())
        {
            var root = NewTemp();
            try
            {
                var files = WriteFiles(root, c!["files"]!.AsObject());
                var diag = new Dictionary<string, string>();
                if (c["diag"] is JsonObject d) foreach (var (dk, dv) in d) diag[dk] = dv!.GetValue<string>();
                var file = new PerfCsvFile
                {
                    Path = FullPath(root, c["csv"]!.GetValue<string>()), DiagFlags = diag, QpcFrequency = 1, DroppedRows = 0, Rows = [],
                };
                var m = RunFileMeta.Load(file);
                result.Add(new JsonObject
                {
                    ["kind"] = "run_meta", ["files"] = files, ["csv"] = c["csv"]!.GetValue<string>(), ["diag"] = c["diag"]?.DeepClone(),
                    ["scenario"] = m.Scenario, ["mode"] = m.Mode, ["cond"] = m.Cond,
                    ["preload_workers"] = I(m.PreloadWorkers), ["gc_time_percent"] = D(m.GcTimePercent), ["frame_time_p95_ms"] = D(m.FrameTimeP95Ms),
                });
            }
            finally { Cleanup(root); }
        }
        if (inputs["phase_names"]?.GetValue<bool>() == true)
            result.Add(new JsonObject { ["kind"] = "phase_names", ["names"] = new JsonArray(PerfStats.PhaseGetters().Select(p => (JsonNode?)p.Name).ToArray()) });
        return result;
    }

    // ------------------------------------------------------------------ perf-rules (W1-18)

    private static RulesConfig Cfg(JsonNode? rulesJson, string root)
    {
        if (rulesJson is null) return RulesConfig.Default();
        var path = WriteFile(root, "rules.json", rulesJson.GetValue<string>());
        return RulesConfig.Load(path);
    }

    private static JsonObject RuleOut(RuleResult r) => new() { ["triggered"] = B(r.Triggered), ["evidence"] = r.Evidence, ["note"] = S(r.Note) };

    /// <summary>inputs: cases [{rule, rules_json?: text|null, ...args}] (see the switch), get [{rules_json, rule, key, default}],
    /// load_error [{rules_json}], fmt [text].</summary>
    public static JsonArray Rules(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = NewTemp();
            try
            {
                var cfg = Cfg(c["rules_json"], root);
                var rule = c["rule"]!.GetValue<string>();
                RuleResult r = rule switch
                {
                    "R-IO" => PerfRules.EvaluateRIo(cfg, Navs(c["navs"])),
                    "R-DEC" => PerfRules.EvaluateRDec(cfg, Navs(c["navs"])),
                    "R-UI" => PerfRules.EvaluateRUi(cfg, Navs(c["navs"]), Num(c["ram_hit_final_p95"]), NumOrNull(c["frame_time_p95"])),
                    "R-PRE" => PerfRules.EvaluateRPre(cfg, Num(c["ram_hit_final_p95"]), Num(c["non_ram_hit_share_pct"]), c["is_burst"]!.GetValue<bool>()),
                    "R-CONT" => PerfRules.EvaluateRCont(cfg, NumOrNull(c["low_decode"]), NumOrNull(c["high_decode"]), IntOrNull(c["low_workers"]), IntOrNull(c["high_workers"])),
                    "R-THREAD" => PerfRules.EvaluateRThread(cfg, c["dispatcher_long_op_count"]!.GetValue<int>(), NumOrNull(c["t_input_p95"])),
                    "R-GC" => PerfRules.EvaluateRGc(cfg, NumOrNull(c["gc_time_percent"])),
                    "R-DISK" => PerfRules.EvaluateRDisk(cfg, Navs(c["disk_navs"]), Navs(c["source_miss_navs"])),
                    "R-FOLDER" => PerfRules.EvaluateRFolder(cfg, [.. c["gens"]!.AsArray().Select(g => GenFromJson(g!))]),
                    _ => throw new ArgumentException("unknown rule " + rule),
                };
                var item = (JsonObject)c.DeepClone();
                item["kind"] = "evaluate";
                var o = RuleOut(r);
                item["triggered"] = o["triggered"]?.DeepClone();
                item["evidence"] = r.Evidence;
                item["note"] = S(r.Note);
                item["result_rule"] = r.Rule;
                result.Add(item);
            }
            finally { Cleanup(root); }
        }
        foreach (var node in inputs["get"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = NewTemp();
            try
            {
                var cfg = Cfg(c["rules_json"], root);
                var item = (JsonObject)c.DeepClone();
                item["kind"] = "get";
                item["value"] = D(cfg.Get(c["rule"]!.GetValue<string>(), c["key"]!.GetValue<string>(), Num(c["default"])));
                result.Add(item);
            }
            finally { Cleanup(root); }
        }
        foreach (var node in inputs["load_error"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = NewTemp();
            try
            {
                var path = WriteFile(root, "rules.json", c["rules_json"]!.GetValue<string>());
                string type = "none", prefixOk = "";
                try { RulesConfig.Load(path); }
                catch (Exception ex)
                {
                    type = ex.GetType().Name;
                    prefixOk = ex.Message.StartsWith($"Invalid rules file '{path}': ", StringComparison.Ordinal).ToString();
                }
                var item = (JsonObject)c.DeepClone();
                item["kind"] = "load_error";
                item["error_type"] = type;
                item["message_has_path_prefix"] = prefixOk == "True";
                result.Add(item);
            }
            finally { Cleanup(root); }
        }
        foreach (var t in inputs["fmt"]!.AsArray())
        {
            var s = t!.GetValue<string>();
            var v = double.Parse(s, NumberStyles.Float, Inv);
            result.Add(new JsonObject
            {
                ["kind"] = "fmt", ["text"] = s,
                ["f1"] = FormattableString.Invariant($"{v:F1}"),
                ["f2"] = FormattableString.Invariant($"{v:F2}"),
                ["default"] = FormattableString.Invariant($"{v}"),
            });
        }
        return result;
    }

    // ------------------------------------------------------------------ perf-report (W1-20)

    /// <summary>inputs: reports [{name, csv_file_count, groups: [group spec]}], json_string [text].
    /// Group spec fields (all optional unless noted): key {scenario, mode, cond, workers} (required), count, incomplete,
    /// first {p50,p95,max}, final {...}, rendered {...} (default NaN), hit_rate (default NaN), kind_counts [[k, n]],
    /// slowest_phase_share [[k, v]], preload_workers, gc, frame_p95, preload_items [{slot, path_id, queue_wait, kind, ms}],
    /// paused, cancel, dispatcher [{ms, priority, name}], dispatcher_before, dropped, folder_gens [{gen,t1,t2,t3,t3_phase}],
    /// startup_runs [[[phase, ms], ...], ...], rules [{rule, triggered, evidence, note}].</summary>
    public static JsonArray Report(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["reports"]!.AsArray())
        {
            var spec = node!.AsObject();
            var ar = new PerfAnalyze.AnalysisResult { CsvFileCount = spec["csv_file_count"]?.GetValue<int>() ?? 1 };
            foreach (var gn in spec["groups"]!.AsArray()) ar.Groups.Add(GroupFromJson(gn!.AsObject()));
            var root = NewTemp();
            try
            {
                var md = Path.Combine(root, "summary.md");
                var js = Path.Combine(root, "summary.json");
                PerfAnalyzeReport.WriteMarkdown(md, ar);
                PerfAnalyzeReport.WriteJson(js, ar);
                var item = (JsonObject)spec.DeepClone();
                item["kind"] = "report";
                item["markdown"] = File.ReadAllText(md);
                item["json"] = File.ReadAllText(js);
                result.Add(item);
            }
            finally { Cleanup(root); }
        }
        foreach (var t in inputs["json_string"]!.AsArray())
        {
            var s = t!.GetValue<string>();
            result.Add(new JsonObject { ["kind"] = "json_string", ["text"] = s, ["json"] = JsonSerializer.Serialize(s) });
        }
        return result;
    }

    private static PerfAnalyze.GroupResult GroupFromJson(JsonObject g)
    {
        var k = g["key"]!;
        var s = new GroupSummary { Key = new GroupKey(k["scenario"]!.GetValue<string>(), k["mode"]!.GetValue<string>(), k["cond"]!.GetValue<string>(), IntOrNull(k["workers"])) };
        s.Count = g["count"]?.GetValue<int>() ?? 0;
        s.Incomplete = g["incomplete"]?.GetValue<int>() ?? 0;
        void Triple(string name, Action<double> p50, Action<double> p95, Action<double> max)
        {
            if (g[name] is not JsonObject t) return;
            p50(Num(t["p50"])); p95(Num(t["p95"])); max(Num(t["max"]));
        }
        Triple("first", v => s.FirstP50 = v, v => s.FirstP95 = v, v => s.FirstMax = v);
        Triple("final", v => s.FinalP50 = v, v => s.FinalP95 = v, v => s.FinalMax = v);
        Triple("rendered", v => s.RenderedFrameP50 = v, v => s.RenderedFrameP95 = v, v => s.RenderedFrameMax = v);
        if (g["hit_rate"] is { } hr) s.HitRate = Num(hr);
        if (g["kind_counts"] is JsonArray kc) foreach (var p in kc) s.KindCounts[p![0]!.GetValue<string>()] = p[1]!.GetValue<int>();
        if (g["slowest_phase_share"] is JsonArray sp) foreach (var p in sp) s.SlowestPhaseShare[p![0]!.GetValue<string>()] = Num(p[1]);
        s.PreloadWorkers = IntOrNull(g["preload_workers"]);
        s.GcTimePercent = NumOrNull(g["gc"]);
        s.FrameTimeP95Ms = NumOrNull(g["frame_p95"]);
        if (g["preload_items"] is JsonArray pi)
            foreach (var p in pi) s.PreloadItems.Add(new PreloadItemRow(p!["slot"]!.GetValue<int>(), p["path_id"]!.GetValue<string>(), Num(p["queue_wait"]), p["kind"]!.GetValue<string>(), Num(p["ms"])));
        s.PreloadPausedCount = g["paused"]?.GetValue<int>() ?? 0;
        s.PreloadCancelCount = g["cancel"]?.GetValue<int>() ?? 0;
        if (g["dispatcher"] is JsonArray dl)
            foreach (var p in dl) s.DispatcherLongOps.Add(new DispatcherLongOpRow(Num(p!["ms"]), p["priority"]!.GetValue<string>(), p["name"]!.GetValue<string>()));
        s.DispatcherLongOpsBeforeStartCount = g["dispatcher_before"]?.GetValue<int>() ?? 0;
        s.DroppedRows = g["dropped"]?.GetValue<long>() ?? 0;
        if (g["folder_gens"] is JsonArray fg) foreach (var p in fg) s.FolderGens.Add(GenFromJson(p!));
        if (g["startup_runs"] is JsonArray sr)
            foreach (var run in sr)
            {
                var d = new Dictionary<string, double>(StringComparer.Ordinal);
                foreach (var p in run!.AsArray()) d[p![0]!.GetValue<string>()] = Num(p[1]);
                s.StartupRuns.Add(d);
            }
        var rules = new List<RuleResult>();
        if (g["rules"] is JsonArray ra)
            foreach (var p in ra)
                rules.Add(new RuleResult(p!["rule"]!.GetValue<string>(), p["triggered"]?.GetValue<bool>(), p["evidence"]!.GetValue<string>(), p["note"]?.GetValue<string>()));
        return new PerfAnalyze.GroupResult { Summary = s, Rules = rules };
    }

    // ------------------------------------------------------------------ perf-analyze (W1-21)

    /// <summary>Behaviour-identical copy of the internal NaNAsNullDoubleConverter.Write (the type is internal to the DLL).</summary>
    private sealed class LocalNaNAsNull : JsonConverter<double>
    {
        public override double Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options) =>
            reader.TokenType == JsonTokenType.Null ? double.NaN : reader.GetDouble();

        public override void Write(Utf8JsonWriter writer, double value, JsonSerializerOptions options)
        {
            if (double.IsFinite(value)) writer.WriteNumberValue(value); else writer.WriteNullValue();
        }
    }

    /// <summary>inputs: runs [{name, files: {rel: text|{fixture}|{gen}|{raw_hex}}, rules_json?, rules_file?, missing_dir?, missing_rules?}],
    /// burst [scenarioKey], json_double [text]. A run that throws records error {type, message} with the run dir replaced by "&lt;RUNDIR&gt;".</summary>
    public static JsonArray Analyze(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["runs"]!.AsArray())
        {
            var spec = node!.AsObject();
            var root = NewTemp();
            var rulesRoot = NewTemp(); // the rules file lives outside the run dir so it is never scanned
            try
            {
                var runDir = spec["missing_dir"]?.GetValue<bool>() == true ? Path.Combine(root, "does-not-exist") : root;
                var files = WriteFiles(root, spec["files"]?.AsObject() ?? []);
                string? rulesPath = null;
                if (spec["rules_json"] is { } rj) rulesPath = WriteFile(rulesRoot, "rules.json", rj.GetValue<string>());
                if (spec["missing_rules"]?.GetValue<bool>() == true) rulesPath = Path.Combine(rulesRoot, "nope.json");
                var item = new JsonObject { ["kind"] = "run", ["name"] = spec["name"]!.GetValue<string>(), ["files"] = files };
                if (spec["rules_json"] is { } r2) item["rules_json"] = r2.DeepClone();
                if (spec["missing_dir"] is { } md0) item["missing_dir"] = md0.DeepClone();
                if (spec["missing_rules"] is { } mr0) item["missing_rules"] = mr0.DeepClone();
                try
                {
                    var res = PerfAnalyze.RunAsync(runDir, rulesPath).GetAwaiter().GetResult();
                    item["csv_file_count"] = res.CsvFileCount;
                    item["groups"] = new JsonArray(res.Groups.Select(g => (JsonNode?)new JsonObject
                    {
                        ["key_string"] = g.Summary.Key.ToString(), ["scenario"] = g.Summary.Key.Scenario, ["mode"] = g.Summary.Key.Mode,
                        ["cond"] = g.Summary.Key.Cond, ["workers"] = I(g.Summary.Key.PreloadWorkers),
                        ["count"] = g.Summary.Count, ["incomplete"] = g.Summary.Incomplete, ["dropped_rows"] = g.Summary.DroppedRows,
                        ["gc_time_percent"] = D(g.Summary.GcTimePercent), ["frame_time_p95_ms"] = D(g.Summary.FrameTimeP95Ms),
                        ["rules"] = new JsonArray(g.Rules.Select(x => (JsonNode?)new JsonObject
                        {
                            ["rule"] = x.Rule, ["triggered"] = B(x.Triggered), ["evidence"] = x.Evidence, ["note"] = S(x.Note),
                        }).ToArray()),
                    }).ToArray());
                    item["summary_md_name"] = Path.GetFileName(res.SummaryMdPath);
                    item["summary_json_name"] = Path.GetFileName(res.SummaryJsonPath);
                    item["summary_md"] = File.ReadAllText(res.SummaryMdPath);
                    item["summary_json"] = File.ReadAllText(res.SummaryJsonPath);
                }
                catch (Exception ex)
                {
                    item["error"] = new JsonObject
                    {
                        ["type"] = ex.GetType().Name,
                        ["message"] = ex.Message.Replace(root, "<RUNDIR>").Replace(rulesRoot, "<RULESDIR>"),
                    };
                }
                result.Add(item);
            }
            finally { Cleanup(root); Cleanup(rulesRoot); }
        }
        foreach (var t in inputs["burst"]!.AsArray())
        {
            var s = t!.GetValue<string>();
            result.Add(new JsonObject { ["kind"] = "burst", ["key"] = s, ["is_burst"] = PerfAnalyze.IsBurstScenario(s) });
        }
        var opts = new JsonSerializerOptions { Converters = { new LocalNaNAsNull() } };
        foreach (var t in inputs["json_double"]!.AsArray())
        {
            var s = t!.GetValue<string>();
            var v = double.Parse(s, NumberStyles.Float, Inv);
            result.Add(new JsonObject { ["kind"] = "json_double", ["text"] = s, ["json"] = JsonSerializer.Serialize(v, opts) });
        }
        return result;
    }
}
