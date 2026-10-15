using System.Text.Json.Nodes;
using PhotoReview.Imaging.Decoding;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-07b
//     "memory-headroom" => Commands.MemoryHeadroomCases(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * MemoryHeadroom lives in PhotoReview.Imaging.dll (same build/reference step as W4-01a; do it once).
//   * Needs no other oracle file. ReadGcMemoryInfo is deliberately NOT called (machine dependent).

// W4-07b: MemoryHeadroom.HasHeadroom / OutputHasHeadroom and the two public constants.
static partial class Commands
{
    /// <summary>
    /// Cases {name, fn: "has"|"output", value, total, load} (all long). Result: first row {name:"constants", guard_threshold_bytes, output_peak_factor},
    /// then each case echoed with "fits".
    /// </summary>
    public static JsonArray MemoryHeadroomCases(JsonObject inputs)
    {
        var result = new JsonArray
        {
            new JsonObject
            {
                ["name"] = "constants",
                ["guard_threshold_bytes"] = MemoryHeadroom.GuardThresholdBytes,
                ["output_peak_factor"] = MemoryHeadroom.OutputPeakFactor,
            },
        };
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var fn = c["fn"]!.GetValue<string>();
            var value = c["value"]!.GetValue<long>();
            var total = c["total"]!.GetValue<long>();
            var load = c["load"]!.GetValue<long>();
            var fits = fn switch
            {
                "has" => MemoryHeadroom.HasHeadroom(value, total, load),
                "output" => MemoryHeadroom.OutputHasHeadroom(value, total, load),
                _ => throw new ArgumentException("unknown fn " + fn),
            };
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["fn"] = fn,
                ["value"] = value,
                ["total"] = total,
                ["load"] = load,
                ["fits"] = fits,
            });
        }
        return result;
    }
}
