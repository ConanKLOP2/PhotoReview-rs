using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.App.Input;
using PhotoReview.Core.Model;

// Destination: tools/oracle-cs/Commands/W8-05d.cs (this draft is moved there by the lead).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     "shortcut-router" => Commands.W805dShortcutRouterCases(inputs),
//
// Wiring (the lead does this, the writer does not run the oracle):
//   * tools/oracle-cs/OracleCs.csproj references PhotoReview.App.Shared.dll, as for W8-01a. ShortcutRouter, KeyId,
//     KeyModifiers and ReviewCommand are public in App.Shared; AppSettings, ShortcutMappings, ReviewAction and
//     FileOperationType are public in PhotoReview.Core. ShortcutMappings' setters are reached by reflection, so the
//     property names of the input file are the C# names.
//   * Inputs: tests/golden-inputs/shortcut-router.json. Output: tests/golden/shortcut-router.json.
//
// Output (deterministic, no clock, cwd or file reads):
//   * kind "resolve": one case per (fixture, key pair, modifier mask, state). The count is
//     fixtures x key_pairs x modifiers x states (5 x 40 x 6 x 8 = 9 600 for the committed inputs).
//     `command` is null or { "type": ReviewCommandType name, "action_index": int (-1 when unused), "force_picker": bool }.
//   * kind "sequence": one case per entry of "sequences". Each step copies its input fields; resolve steps get `command`.
//   * Enum values are written with ToString() (the C# names). Culture is not involved (no number formatting).

static partial class Commands
{
    public static JsonArray W805dShortcutRouterCases(JsonObject inputs)
    {
        var fixtures = inputs["fixtures"]!.AsArray();
        var states = inputs["states"]!.AsArray();
        var keyPairs = inputs["key_pairs"]!.AsArray();
        var modifierBits = inputs["modifiers"]!.AsArray();
        var cases = new JsonArray();

        var fixtureByName = new Dictionary<string, JsonObject>(StringComparer.Ordinal);
        foreach (var fixtureNode in fixtures)
        {
            var fixture = fixtureNode!.AsObject();
            fixtureByName[fixture["name"]!.GetValue<string>()] = fixture;
        }

        foreach (var fixtureNode in fixtures)
        {
            var fixture = fixtureNode!.AsObject();
            var fixtureName = fixture["name"]!.GetValue<string>();
            var router = W805dBuildRouter(fixture);

            foreach (var pairNode in keyPairs)
            {
                var pair = pairNode!.AsArray();
                var key = Enum.Parse<KeyId>(pair[0]!.GetValue<string>());
                var systemKey = Enum.Parse<KeyId>(pair[1]!.GetValue<string>());

                foreach (var bitsNode in modifierBits)
                {
                    var modifiers = (KeyModifiers)bitsNode!.GetValue<int>();

                    for (var index = 0; index < states.Count; index++)
                    {
                        var state = W805dState(states[index]!.AsObject());
                        var command = router.TryResolve(
                            key, systemKey, modifiers,
                            isFullscreen: state.IsFullscreen,
                            hasImage: state.HasImage,
                            hasComparePair: state.HasComparePair,
                            isCompareVisible: state.IsCompareVisible,
                            hasCapturePair: state.HasCapturePair);

                        cases.Add(new JsonObject
                        {
                            ["kind"] = "resolve",
                            ["fixture"] = fixtureName,
                            ["key"] = key.ToString(),
                            ["system_key"] = systemKey.ToString(),
                            ["modifiers"] = JsonValue.Create((int)modifiers),
                            ["state"] = JsonValue.Create(index),
                            ["command"] = W805dCommand(command),
                        });
                    }
                }
            }
        }

        foreach (var sequenceNode in inputs["sequences"]!.AsArray())
        {
            var sequence = sequenceNode!.AsObject();
            var sequenceName = sequence["name"]!.GetValue<string>();
            var startName = sequence["start"]!.GetValue<string>();
            var router = W805dBuildRouter(fixtureByName[startName]);
            var steps = new JsonArray();

            foreach (var stepNode in sequence["steps"]!.AsArray())
            {
                var step = stepNode!.AsObject();
                var op = step["op"]!.GetValue<string>();

                if (op == "rebuild")
                {
                    var fixtureName = step["fixture"]!.GetValue<string>();
                    var settings = W805dSettings(fixtureByName[fixtureName])
                        ?? throw new InvalidOperationException("rebuild needs a settings fixture: " + fixtureName);
                    router.Rebuild(settings);
                    steps.Add(new JsonObject
                    {
                        ["op"] = "rebuild",
                        ["fixture"] = JsonValue.Create(fixtureName),
                    });
                    continue;
                }

                var key = Enum.Parse<KeyId>(step["key"]!.GetValue<string>());
                var systemKey = Enum.Parse<KeyId>(step["system_key"]!.GetValue<string>());
                var modifiers = (KeyModifiers)step["modifiers"]!.GetValue<int>();
                var stateIndex = step["state"]!.GetValue<int>();
                var state = W805dState(states[stateIndex]!.AsObject());
                var command = router.TryResolve(
                    key, systemKey, modifiers,
                    isFullscreen: state.IsFullscreen,
                    hasImage: state.HasImage,
                    hasComparePair: state.HasComparePair,
                    isCompareVisible: state.IsCompareVisible,
                    hasCapturePair: state.HasCapturePair);

                steps.Add(new JsonObject
                {
                    ["op"] = "resolve",
                    ["key"] = key.ToString(),
                    ["system_key"] = systemKey.ToString(),
                    ["modifiers"] = JsonValue.Create((int)modifiers),
                    ["state"] = JsonValue.Create(stateIndex),
                    ["command"] = W805dCommand(command),
                });
            }

            cases.Add(new JsonObject
            {
                ["kind"] = "sequence",
                ["name"] = sequenceName,
                ["start"] = startName,
                ["steps"] = steps,
            });
        }

        return cases;
    }

    // ---- fixtures ----

    // A null settings object is C# `new ShortcutRouter()`: no binding at all (Escape still closes).
    private static ShortcutRouter W805dBuildRouter(JsonObject fixture) => new(W805dSettings(fixture));

    // Start from `new AppSettings()` (the C# defaults), then apply the fixture's shortcut strings and, when present,
    // replace the action list. `"router": "empty"` returns null.
    private static AppSettings? W805dSettings(JsonObject fixture)
    {
        if (fixture["router"] is JsonNode mode && mode.GetValue<string>() == "empty")
        {
            return null;
        }

        var settings = new AppSettings();

        if (fixture["shortcuts"] is JsonObject shortcuts)
        {
            foreach (var entry in shortcuts)
            {
                var property = typeof(ShortcutMappings).GetProperty(entry.Key, BindingFlags.Public | BindingFlags.Instance)
                    ?? throw new InvalidOperationException("unknown ShortcutMappings property " + entry.Key);
                property.SetValue(settings.Shortcuts, entry.Value!.GetValue<string>());
            }
        }

        if (fixture["actions"] is JsonArray actions)
        {
            var list = new List<ReviewAction>();
            foreach (var actionNode in actions)
            {
                var action = actionNode!.AsObject();
                list.Add(new ReviewAction
                {
                    Name = action["name"]!.GetValue<string>(),
                    Shortcut = action["shortcut"]!.GetValue<string>(),
                    Operation = Enum.Parse<FileOperationType>(action["operation"]!.GetValue<string>()),
                    Destination = action["destination"]!.GetValue<string>(),
                });
            }

            settings.Actions = list;
        }

        return settings;
    }

    // ---- states and outputs ----

    private sealed record W805dFlags(
        bool IsFullscreen, bool HasImage, bool HasComparePair, bool IsCompareVisible, bool HasCapturePair);

    private static W805dFlags W805dState(JsonObject o) => new(
        o["is_fullscreen"]!.GetValue<bool>(),
        o["has_image"]!.GetValue<bool>(),
        o["has_compare_pair"]!.GetValue<bool>(),
        o["is_compare_visible"]!.GetValue<bool>(),
        o["has_capture_pair"]!.GetValue<bool>());

    private static JsonNode? W805dCommand(ReviewCommand? command)
    {
        if (command is not { } value)
        {
            return null;
        }

        return new JsonObject
        {
            ["type"] = JsonValue.Create(value.Type.ToString()),
            ["action_index"] = JsonValue.Create(value.ActionIndex),
            ["force_picker"] = JsonValue.Create(value.ForcePicker),
        };
    }
}
