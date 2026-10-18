using System.Diagnostics;
using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Shell.Rendering;
using PhotoReview.Shell.Win32.Viewing;

// W8-16a: ScalingPolicy, FrameStats, FramePacer, FrameLog (the pure parts of Shell.Win32/Viewing). ImageLayer is NOT here
// (it draws through D2D types; its tests are fakes, see contracts/W8-16a.md).
// Destination: tools/oracle-cs/Commands/W8-16a.cs (this draft is moved there by the lead).
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-16a
//     "scaling-policy" => Commands.W816aScalingPolicyCases(inputs),
//     "frame-stats" => Commands.W816aFrameStatsCases(inputs),
//     "frame-pacer" => Commands.W816aFramePacerCases(inputs),
//     "frame-log" => Commands.W816aFrameLogCases(inputs),
//
// Wiring (the lead does this, the writer did not run it):
//   * tools/oracle-cs/OracleCs.csproj: add a Reference to PhotoReview.Shell.Rendering.dll (HintPath $(CSharpCoreDir)) for the
//     ImageInterpolation enum, and compile the four pure C# files of the baseline into the oracle assembly so that the internal
//     types are reached directly (no reflection, no InternalsVisibleTo, the C# repo is not edited):
//         <Compile Include="$(CSharpRepo)\src\PhotoReview.Shell.Win32\Viewing\ScalingPolicy.cs" />
//         <Compile Include="$(CSharpRepo)\src\PhotoReview.Shell.Win32\Viewing\FrameStats.cs" />
//         <Compile Include="$(CSharpRepo)\src\PhotoReview.Shell.Win32\Viewing\FramePacer.cs" />
//         <Compile Include="$(CSharpRepo)\src\PhotoReview.Shell.Win32\Viewing\FrameLog.cs" />
//   * tools/oracle.ps1: build PhotoReview.Shell.Rendering into $coreDir (it has no WPF dependency).
//
// Determinism: frame times are converted with the machine's Stopwatch.Frequency, and every frame-stats and frame-log output
// records that frequency as "ticks_per_second"; the Rust test converts with the same frequency. The FrameLog file is a temp
// file that is deleted; only its lines are kept. The env-var cases use a fixed missing directory, never a temp path.

static partial class Commands
{
    private static double W816aNum(JsonNode? node)
    {
        if (node is JsonValue v && v.TryGetValue<string>(out var text))
        {
            return double.Parse(text, NumberStyles.Float, CultureInfo.InvariantCulture);
        }

        return node!.GetValue<double>();
    }

    /// <summary>The conversion the C# tests use: <c>(long)(ms * Stopwatch.Frequency / 1000.0)</c>.</summary>
    private static long W816aTicks(double ms) => (long)(ms * Stopwatch.Frequency / 1000.0);

    public static JsonArray W816aScalingPolicyCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind };
            var sw = c["sw"]!.GetValue<int>();
            var sh = c["sh"]!.GetValue<int>();
            var dw = W816aNum(c["dw"]);
            var dh = W816aNum(c["dh"]);
            if (kind == "choose")
            {
                var requested = Enum.Parse<ImageInterpolation>(c["requested"]!.GetValue<string>());
                var inMotion = c["in_motion"]!.GetValue<bool>();
                o["interpolation"] = ScalingPolicy.Choose(requested, sw, sh, dw, dh, inMotion).ToString();
            }
            else
            {
                o["heavy"] = ScalingPolicy.IsHeavyDownscale(sw, sh, dw, dh);
            }

            result.Add(o);
        }

        return result;
    }

    private static void W816aFrameSummary(JsonObject o, FrameStats stats)
    {
        o["count"] = stats.Count;
        o["total"] = stats.Total;
        o["blocked_presents"] = stats.BlockedPresents;
        o["frame_p50"] = stats.FrameMs(50);
        o["frame_p95"] = stats.FrameMs(95);
        o["frame_p100"] = stats.FrameMs(100);
        o["present_p50"] = stats.PresentMs(50);
        o["max_present_ms"] = stats.MaxPresentMs();
    }

    public static JsonArray W816aFrameStatsCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var stats = new FrameStats();
            var o = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["kind"] = kind,
                ["ticks_per_second"] = Stopwatch.Frequency,
            };
            switch (kind)
            {
                case "series":
                {
                    var n = c["n"]!.GetValue<int>();
                    for (var i = 1; i <= n; i++)
                    {
                        stats.Record(W816aTicks(i), W816aTicks(i / 2.0));
                    }

                    W816aFrameSummary(o, stats);
                    break;
                }
                case "empty":
                {
                    W816aFrameSummary(o, stats);
                    break;
                }
                case "samples":
                {
                    foreach (var pair in c["samples"]!.AsArray())
                    {
                        var p = pair!.AsArray();
                        stats.Record(W816aTicks(W816aNum(p[0])), W816aTicks(W816aNum(p[1])));
                    }

                    W816aFrameSummary(o, stats);
                    break;
                }
                case "ring":
                {
                    var firstMs = W816aNum(c["first_ms"]);
                    var firstCount = c["first_count"]!.GetValue<int>();
                    var restMs = W816aNum(c["rest_ms"]);
                    var total = c["total"]!.GetValue<int>();
                    var presentMs = W816aNum(c["present_ms"]);
                    for (var i = 0; i < total; i++)
                    {
                        stats.Record(W816aTicks(i < firstCount ? firstMs : restMs), W816aTicks(presentMs));
                    }

                    W816aFrameSummary(o, stats);
                    break;
                }
                case "reset_keeps_slow":
                {
                    for (var i = 0; i < 5; i++)
                    {
                        stats.Record(W816aTicks(5), W816aTicks(5));
                    }

                    stats.RecordSlow(500, 1, 480, 1, "kept");
                    stats.Reset();
                    W816aFrameSummary(o, stats);
                    o["slow_count"] = stats.Slow.Count;
                    break;
                }
                case "is_slow":
                {
                    o["slow"] = FrameStats.IsSlow(W816aNum(c["end_draw_ms"]), W816aNum(c["present_ms"]));
                    break;
                }
                case "slow_bounded":
                {
                    foreach (var entry in c["entries"]!.AsArray())
                    {
                        W816aRecordSlow(stats, entry!.AsObject());
                    }

                    var repeat = c["repeat"]!.AsObject();
                    var count = repeat["count"]!.GetValue<int>();
                    for (var i = 0; i < count; i++)
                    {
                        W816aRecordSlow(stats, repeat["entry"]!.AsObject());
                    }

                    var notes = new JsonArray();
                    foreach (var slow in stats.Slow)
                    {
                        notes.Add(slow.Note);
                    }

                    o["slow_count"] = stats.Slow.Count;
                    o["slow_notes"] = notes;
                    break;
                }
                default:
                    throw new ArgumentException($"unknown frame-stats kind '{kind}'");
            }

            result.Add(o);
        }

        return result;
    }

    private static void W816aRecordSlow(FrameStats stats, JsonObject e) =>
        stats.RecordSlow(
            W816aNum(e["frame_ms"]),
            W816aNum(e["draw_ms"]),
            W816aNum(e["end_draw_ms"]),
            W816aNum(e["present_ms"]),
            e["note"]!.GetValue<string>());

    public static JsonArray W816aFramePacerCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind };
            var pacer = new FramePacer();
            switch (kind)
            {
                case "mode_for":
                {
                    var mode = pacer.ModeFor(c["fullscreen"]!.GetValue<bool>(), c["awaiting"]!.GetValue<bool>());
                    o["mode"] = mode.ToString();
                    break;
                }
                case "defer":
                {
                    var flags = new JsonArray();
                    foreach (var ready in c["ready"]!.AsArray())
                    {
                        flags.Add(pacer.ShouldDefer(ready!.GetValue<bool>()));
                    }

                    o["deferred"] = flags;
                    o["deferred_total"] = pacer.DeferredTotal;
                    break;
                }
                case "observe":
                {
                    var raisedCount = 0;
                    pacer.Degraded += (_, _) => raisedCount++;
                    var flags = new JsonArray();
                    foreach (var step in c["steps"]!.AsArray())
                    {
                        var s = step!.AsObject();
                        var mode = Enum.Parse<PresentMode>(s["mode"]!.GetValue<string>());
                        var before = raisedCount;
                        pacer.ObservePresent(mode, W816aNum(s["present_ms"]));
                        flags.Add(raisedCount > before);
                    }

                    var after = c["after"]!.AsObject();
                    o["raised"] = flags;
                    o["raised_total"] = raisedCount;
                    o["degraded"] = pacer.IsDegraded;
                    o["mode_after"] = pacer.ModeFor(after["fullscreen"]!.GetValue<bool>(), after["awaiting"]!.GetValue<bool>()).ToString();
                    break;
                }
                default:
                    throw new ArgumentException($"unknown frame-pacer kind '{kind}'");
            }

            result.Add(o);
        }

        return result;
    }

    public static JsonArray W816aFrameLogCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind };
            switch (kind)
            {
                case "format":
                {
                    o["line"] = FrameLog.Format(
                        W816aNum(c["gap_ms"]),
                        W816aNum(c["frame_ms"]),
                        W816aNum(c["end_draw_ms"]),
                        W816aNum(c["present_ms"]));
                    break;
                }
                case "env":
                {
                    string? asked = null;
                    var value = c["value"] is JsonValue jv && jv.TryGetValue<string>(out var text) ? text : null;
                    var log = FrameLog.FromEnvironment(name =>
                    {
                        asked = name;
                        return value;
                    });
                    o["asked"] = asked;
                    o["is_null"] = log is null;
                    log?.Dispose();
                    break;
                }
                case "record":
                {
                    var path = Path.Combine(Path.GetTempPath(), "w816a-framelog-" + Guid.NewGuid().ToString("N") + ".log");
                    try
                    {
                        var log = new FrameLog(path);
                        foreach (var frame in c["frames"]!.AsArray())
                        {
                            var f = frame!.AsObject();
                            log.Record(
                                W816aTicks(W816aNum(f["start_ms"])),
                                W816aTicks(W816aNum(f["end_ms"])),
                                W816aNum(f["end_draw_ms"]),
                                W816aNum(f["present_ms"]));
                        }

                        log.Dispose();
                        var lines = new JsonArray();
                        foreach (var line in File.ReadAllLines(path))
                        {
                            lines.Add(line);
                        }

                        o["lines"] = lines;
                    }
                    finally
                    {
                        File.Delete(path);
                    }

                    o["ticks_per_second"] = Stopwatch.Frequency;
                    break;
                }
                default:
                    throw new ArgumentException($"unknown frame-log kind '{kind}'");
            }

            result.Add(o);
        }

        return result;
    }
}
