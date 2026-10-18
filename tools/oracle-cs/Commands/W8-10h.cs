// W8-10h: benchmark-presentation (App.Shared/Presenters/BenchmarkPresentation.cs, 143 lines, baseline d92d6aae).
// Covers BenchmarkText (ProfileKey, ProfileName, ProfileDescription, ProgressMessage, LoadingModeText, WorkloadText, Describe),
// BenchmarkProfileItem (Name, Description), BenchmarkResultRow (CellTexts, StatusText) and the private
// BenchmarkResultRow.MillisecondsText (reached by reflection, like the W8-01a oracle; no C# repo edit).
//
// Destination: tools/oracle-cs/Commands/W8-10h.cs (this draft is moved there by the lead).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     "benchmark-presentation" => Commands.W810hBenchmarkPresentationCases(inputs),
//
// Wiring (the lead does this, the writer does not run the oracle):
//   * tools/oracle-cs/OracleCs.csproj: Reference to PhotoReview.App.Shared.dll (HintPath $(CSharpCoreDir)), as for W8-01a.
//   * tools/oracle.ps1: App.Shared is built into $coreDir (same edit as W8-01a).
//   * Inputs: contracts/W8-10h.md golden-inputs/benchmark-presentation.json -> tests/golden-inputs/.
//     Output: tests/golden/benchmark-presentation.json (run tools/oracle.ps1 -Command benchmark-presentation).
//
// Conventions (shared with W8-01a.cs): every case object is the input case with an "expected" field added; a number may be
// the strings "NaN", "Infinity", "-Infinity"; culture is pinned to Invariant; texts are the built-in ENGLISH catalog (the
// oracle never switches the ambient Localizer). No clock, no cwd, no temp paths.

using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.App;
using PhotoReview.Benchmarking;
using PhotoReview.Core.Model;

static partial class Commands
{
    private static JsonNode W810hStr(string value) => JsonValue.Create(value)!;

    private static string? W810hOptString(JsonObject obj, string key) => obj[key]?.GetValue<string>();

    private static double W810hNum(JsonNode? node)
    {
        if (node is JsonValue value && value.TryGetValue(out string? text))
        {
            return text switch
            {
                "NaN" => double.NaN,
                "Infinity" => double.PositiveInfinity,
                "-Infinity" => double.NegativeInfinity,
                _ => throw new InvalidDataException($"not a number: {text}"),
            };
        }
        return node!.GetValue<double>();
    }

    /// <summary>{"kind":"profile","problem":"NotImplemented","profileId":"x","message":"m"} or {"kind":"other","message":"m"}.</summary>
    private static Exception W810hError(JsonObject error)
    {
        var message = error["message"]!.GetValue<string>();
        if (error["kind"]!.GetValue<string>() == "profile")
        {
            var problem = Enum.Parse<BenchmarkProfileProblem>(error["problem"]!.GetValue<string>());
            return new BenchmarkProfileException(problem, error["profileId"]!.GetValue<string>(), message);
        }
        return new InvalidOperationException(message);
    }

    /// <summary>{"workload","samples":[numbers],"status","message"?,"imagesPerSample"?}.</summary>
    private static BenchmarkPhaseResult W810hPhase(JsonObject phase, string profileId)
    {
        var workload = Enum.Parse<BenchmarkWorkload>(phase["workload"]!.GetValue<string>());
        var samples = phase["samples"]!.AsArray().Select(W810hNum).ToList();
        var status = Enum.Parse<BenchmarkResultStatus>(phase["status"]!.GetValue<string>());
        var message = W810hOptString(phase, "message");
        int? images = phase["imagesPerSample"] is { } im ? im.GetValue<int>() : (int?)null;
        return new BenchmarkPhaseResult(profileId, workload, samples, status, message, images);
    }

    public static JsonArray W810hBenchmarkPresentationCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        CultureInfo.CurrentUICulture = CultureInfo.InvariantCulture;
        var results = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.DeepClone().AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "profile-key":
                    c["expected"] = W810hStr(BenchmarkText.ProfileKey(c["id"]!.GetValue<string>(), c["field"]!.GetValue<string>()));
                    break;
                case "profile-texts":
                {
                    // Registry profile, optionally with id/name/description overridden (absent = registry value).
                    var registry = BenchmarkProfiles.Find(c["baseId"]!.GetValue<string>())!;
                    var profile = registry with
                    {
                        Id = W810hOptString(c, "id") ?? registry.Id,
                        Name = W810hOptString(c, "profileName") ?? registry.Name,
                        Description = W810hOptString(c, "description") ?? registry.Description,
                    };
                    var item = new BenchmarkProfileItem(profile);
                    c["expected"] = new JsonObject
                    {
                        ["name"] = W810hStr(BenchmarkText.ProfileName(profile)),
                        ["description"] = W810hStr(BenchmarkText.ProfileDescription(profile)),
                        ["itemName"] = W810hStr(item.Name),
                        ["itemDescription"] = W810hStr(item.Description),
                    };
                    break;
                }
                case "progress":
                    c["expected"] = W810hStr(BenchmarkText.ProgressMessage(c["message"]!.GetValue<string>()));
                    break;
                case "loading-mode":
                    c["expected"] = W810hStr(BenchmarkText.LoadingModeText(Enum.Parse<LoadingMode>(c["mode"]!.GetValue<string>())));
                    break;
                case "workload":
                    c["expected"] = W810hStr(BenchmarkText.WorkloadText(Enum.Parse<BenchmarkWorkload>(c["workload"]!.GetValue<string>())));
                    break;
                case "describe":
                    c["expected"] = W810hStr(BenchmarkText.Describe(W810hError(c["error"]!.AsObject())));
                    break;
                case "row":
                {
                    var profileId = c["profileId"]!.GetValue<string>();
                    var profile = BenchmarkProfiles.Find(profileId)!;
                    var phase = W810hPhase(c["phase"]!.AsObject(), profileId);
                    var error = c["error"] is JsonObject e ? W810hError(e) : null;
                    var row = new BenchmarkResultRow(profile, phase, error) { IsBest = c["isBest"]!.GetValue<bool>() };
                    c["expected"] = new JsonObject
                    {
                        ["cells"] = new JsonArray(row.CellTexts.Select(t => W810hStr(t)).ToArray()),
                        ["statusText"] = W810hStr(row.StatusText),
                    };
                    break;
                }
                case "milliseconds":
                {
                    // Private BenchmarkResultRow.MillisecondsText(value); Count comes from the phase (count copies of 0.0).
                    var count = c["count"]!.GetValue<int>();
                    var registry = BenchmarkProfiles.All[0];
                    var phase = new BenchmarkPhaseResult(registry.Id, BenchmarkWorkload.Sequential, Enumerable.Repeat(0.0, count).ToList(), BenchmarkResultStatus.Pass);
                    var row = new BenchmarkResultRow(registry, phase, null);
                    var method = typeof(BenchmarkResultRow).GetMethod("MillisecondsText", BindingFlags.NonPublic | BindingFlags.Instance)!;
                    c["expected"] = W810hStr((string)method.Invoke(row, new object[] { W810hNum(c["value"]) })!);
                    break;
                }
                default:
                    throw new InvalidOperationException($"unknown benchmark-presentation kind: {c["kind"]}");
            }
            results.Add(c);
        }
        return results;
    }
}
