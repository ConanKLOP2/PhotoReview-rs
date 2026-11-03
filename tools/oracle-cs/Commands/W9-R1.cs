// W9-R1 FEAT-RENAME, C# baseline 7eaee2a2 (needs RenameNamePolicy, which the older d92d6aae baseline did not have).
// Dispatch in Program.cs: "w9-r1-rename-policy" => Commands.W9R1RenamePolicyCases(inputs),
// Input: tests/golden-inputs/w9-r1-rename-policy.json; output: tests/golden/w9-r1-rename-policy.json.
// A case may give "typed_fill": {"text": "x", "count": 251} instead of "typed" (long names without a long JSON line).
using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.FileActions;

static partial class Commands
{
    private static string? W9R1Typed(JsonObject item) =>
        item["typed_fill"] is JsonObject fill
            ? string.Concat(Enumerable.Repeat(fill["text"]!.GetValue<string>(), fill["count"]!.GetValue<int>()))
            : item["typed"]?.GetValue<string>();

    public static JsonArray W9R1RenamePolicyCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["single"]!.AsArray())
        {
            var item = node!.AsObject();
            var check = RenameNamePolicy.Check(W9R1Typed(item), item["path"]!.GetValue<string>(),
                item["long_path_aware"]?.GetValue<bool>() ?? false);
            result.Add(new JsonObject
            {
                ["kind"] = "single", ["path"] = item["path"]!.GetValue<string>(),
                ["typed"] = W9R1Typed(item), ["problem"] = check.Problem.ToString(),
                ["new_file_name"] = check.NewFileName, ["invalid_character"] = check.InvalidCharacter?.ToString(),
                ["unchanged"] = check.IsUnchanged, ["case_only"] = check.DiffersOnlyByCase,
            });
        }
        foreach (var node in inputs["group"]!.AsArray())
        {
            var item = node!.AsObject();
            var group = new CaptureGroup(item["jpeg"]!.GetValue<string>(), item["raw"]!.GetValue<string>(), item["xmp"]?.GetValue<string>());
            var plan = CaptureGroupRenamePlan.Build(group, W9R1Typed(item), item["long_path_aware"]?.GetValue<bool>() ?? false);
            var names = new JsonObject();
            if (plan.FileNames is not null)
                foreach (var pair in plan.FileNames.OrderBy(p => p.Key, StringComparer.Ordinal)) names[pair.Key] = pair.Value;
            result.Add(new JsonObject
            {
                ["kind"] = "group", ["jpeg"] = group.JpegPath, ["raw"] = group.RawPath, ["xmp"] = group.XmpPath,
                ["typed"] = W9R1Typed(item), ["problem"] = plan.Check.Problem.ToString(),
                ["new_file_name"] = plan.Check.NewFileName, ["unchanged"] = plan.Check.IsUnchanged,
                ["case_only"] = plan.Check.DiffersOnlyByCase, ["names"] = names,
            });
        }
        return result;
    }
}
