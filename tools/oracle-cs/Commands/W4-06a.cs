using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Preload;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-06a
//     "navigation-pace" => Commands.NavigationPaceScript(inputs),
// WIRING: NavigationPace lives in PhotoReview.Imaging.dll, which tools/oracle-cs/OracleCs.csproj does not reference yet: add
// <Reference Include="PhotoReview.Imaging"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.dll</HintPath></Reference> (same wiring as W4-05a).
// The clock is a fake tick counter, so the output is deterministic (D19/D27). The output reports Stopwatch.Frequency: the Rust port
// assumes 10_000_000 and its test asserts the golden's value equals pr_imaging::navigation_pace::TICKS_PER_SECOND.

// W4-06a: NavigationPace driven by a script of operations on a fake clock.
static partial class Commands
{
    private static double W406aNumber(JsonNode node)
    {
        if (node.GetValueKind() == JsonValueKind.String)
        {
            return node.GetValue<string>() switch
            {
                "NaN" => double.NaN,
                "Infinity" => double.PositiveInfinity,
                "-Infinity" => double.NegativeInfinity,
                var other => throw new ArgumentException("unknown number text " + other),
            };
        }

        return node.GetValue<double>();
    }

    /// <summary>
    /// For each case {name, ops:[...]}: ops are record{index}, advance{ms} (ticks += (long)(ms * Frequency / 1000), as the C# tests do),
    /// steps{from,count,step,interval_ms} (record(from), then count x advance+record(from + i*step)), direction, lead{decode_ms},
    /// delay{decode_ms}. Result per case: {name, frequency, results:[ {op, value} | {op:"delay", ms, ticks} ]} for the query ops only.
    /// </summary>
    public static JsonArray NavigationPaceScript(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            long now = 1_000_000;
            var pace = new NavigationPace(() => now);
            void Advance(double ms) => now += (long)(ms * Stopwatch.Frequency / 1000);
            var outputs = new JsonArray();
            foreach (var opNode in c["ops"]!.AsArray())
            {
                var o = opNode!.AsObject();
                switch (o["op"]!.GetValue<string>())
                {
                    case "record":
                        pace.Record(o["index"]!.GetValue<int>());
                        break;
                    case "advance":
                        Advance(o["ms"]!.GetValue<double>());
                        break;
                    case "steps":
                    {
                        var from = o["from"]!.GetValue<int>();
                        var step = o["step"]!.GetValue<int>();
                        pace.Record(from);
                        for (var i = 1; i <= o["count"]!.GetValue<int>(); i++)
                        {
                            Advance(o["interval_ms"]!.GetValue<double>());
                            pace.Record(from + i * step);
                        }
                        break;
                    }
                    case "direction":
                        outputs.Add(new JsonObject { ["op"] = "direction", ["value"] = pace.Direction });
                        break;
                    case "lead":
                        outputs.Add(new JsonObject { ["op"] = "lead", ["value"] = pace.GetLead(W406aNumber(o["decode_ms"]!)) });
                        break;
                    case "delay":
                    {
                        var d = pace.GetViewerStartDelay(W406aNumber(o["decode_ms"]!));
                        outputs.Add(new JsonObject { ["op"] = "delay", ["ms"] = d.TotalMilliseconds, ["ticks"] = d.Ticks });
                        break;
                    }
                    default:
                        throw new ArgumentException("unknown op " + o["op"]);
                }
            }
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["frequency"] = Stopwatch.Frequency,
                ["results"] = outputs,
            });
        }
        return result;
    }
}
