// W9-E7: open-in-default-editor-shortcut  (App.Shared/Input/ShortcutRouter.cs step 12f + ReviewCommand.cs, baseline 7eaee2a2)
//
// Dispatch line in tools/oracle-cs/Program.cs (inside the `command switch`, W9 block):
//     "open-in-default-editor-shortcut" => Commands.W9E7OpenInDefaultEditorCases(inputs),
//
// Reuses the fixture, state and command helpers of W8-05d (`W805dBuildRouter`, `W805dState`, `W805dCommand`): a fixture is { "name", "shortcuts": { ShortcutMappings
// property name: text }, "actions": [..] } exactly as in shortcut-router.json (the property names are the C# names, `OpenInDefaultEditor` included).
//
// Inputs: tests/golden-inputs/open-in-default-editor-shortcut.json
//   "states":    [ { is_fullscreen, has_image, has_compare_pair, is_compare_visible, has_capture_pair } ]
//   "scenarios": [ { "name", "fixture": { ... }, "steps": [ step ] } ]
//     step = { "op": "resolve", "key": KeyId name, "system_key": KeyId name, "modifiers": ModifierKeys bits, "state": index into states }
//          | { "op": "rebuild", "fixture": { ... } }                      (router.Rebuild(settings of the fixture))
// Output: tests/golden/open-in-default-editor-shortcut.json
//   one case per scenario { name, steps: [ { op, ...the step, command: { type, action_index, force_picker } | null } ] }
//   plus one case { "kind": "auto_repeat", "ignores": [ every ReviewCommandType name whose IgnoresAutoRepeat() is true, in enum order ],
//                   "types": [ every ReviewCommandType name with its integer value, in enum order ] }.

using System.Text.Json.Nodes;
using PhotoReview.App.Input;

static partial class Commands
{
    public static JsonArray W9E7OpenInDefaultEditorCases(JsonObject inputs)
    {
        var states = inputs["states"]!.AsArray();
        var result = new JsonArray();
        foreach (var scenarioNode in inputs["scenarios"]!.AsArray())
        {
            var scenario = scenarioNode!.AsObject();
            var router = W805dBuildRouter(scenario["fixture"]!.AsObject());
            var steps = new JsonArray();
            foreach (var stepNode in scenario["steps"]!.AsArray())
            {
                var step = stepNode!.AsObject();
                if (step["op"]!.GetValue<string>() == "rebuild")
                {
                    var fixture = step["fixture"]!.AsObject();
                    router.Rebuild(W805dSettings(fixture)!);
                    steps.Add(new JsonObject { ["op"] = "rebuild", ["fixture"] = fixture["name"]?.GetValue<string>() });
                    continue;
                }

                var key = Enum.Parse<KeyId>(step["key"]!.GetValue<string>());
                var systemKey = Enum.Parse<KeyId>(step["system_key"]!.GetValue<string>());
                var modifiers = (KeyModifiers)step["modifiers"]!.GetValue<int>();
                var stateIndex = step["state"]!.GetValue<int>();
                var state = W805dState(states[stateIndex]!.AsObject());
                var command = router.TryResolve(key, systemKey, modifiers, isFullscreen: state.IsFullscreen, hasImage: state.HasImage,
                    hasComparePair: state.HasComparePair, isCompareVisible: state.IsCompareVisible, hasCapturePair: state.HasCapturePair);
                steps.Add(new JsonObject
                {
                    ["op"] = "resolve",
                    ["key"] = key.ToString(),
                    ["system_key"] = systemKey.ToString(),
                    ["modifiers"] = (int)modifiers,
                    ["state"] = stateIndex,
                    ["command"] = W805dCommand(command),
                });
            }

            result.Add(new JsonObject { ["name"] = scenario["name"]!.GetValue<string>(), ["steps"] = steps });
        }

        var ignores = new JsonArray();
        var types = new JsonArray();
        foreach (var type in Enum.GetValues<ReviewCommandType>())
        {
            types.Add(new JsonObject { ["name"] = type.ToString(), ["value"] = (int)type });
            if (type.IgnoresAutoRepeat()) ignores.Add(type.ToString());
        }

        result.Add(new JsonObject { ["kind"] = "auto_repeat", ["ignores"] = ignores, ["types"] = types });
        return result;
    }
}
