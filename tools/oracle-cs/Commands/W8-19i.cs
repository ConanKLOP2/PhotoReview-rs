using System.Collections;
using System.Reflection;
using System.Text.Json.Nodes;

// W8-19i oracle (the F half of the unit): ShortcutsSettingsPage (the declaration, the row tables and FindProblems). Uses the shared driver `W819pDeclare` of W8-19d.cs.
// NOT compiled or run by the writer.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19i
//     "settings-page-shortcuts" => Commands.W819iSettingsPageShortcutsCases(inputs),
//
// Run against the C# BASELINE d92d6aae (see W8-19d.cs): the HEAD page has two more Action rows (OpenInDefaultEditor, RenameFile).
// Inputs (tests/golden-inputs/settings-page-shortcuts.json), kinds:
//   declare       -> see W8-19d.cs ("page": "Shortcuts", "cells": true); output cells: array of { property, label, box, clear } in dictionary (insertion) order.
//   rows          -> output rows: ShortcutsSettingsPage.AllRows as { property, label (Label() in English), clear_tip (ClearTip() or null) }; marker (the "Marker" constant).
//   find_problems -> input "texts": object property -> typed text; output "problems": the FindProblems result, sorted ordinally.
static partial class Commands
{
    public static JsonArray W819iSettingsPageShortcutsCases(JsonObject inputs)
    {
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            Type page = W819pPageType("Shortcuts");
            switch (c["kind"]!.GetValue<string>())
            {
                case "declare":
                    W819pDeclare(c);
                    break;
                case "rows":
                {
                    var rows = new JsonArray();
                    foreach (var row in (IEnumerable)page.GetProperty("AllRows", W819pFlags)!.GetValue(null)!)
                    {
                        Type rt = row.GetType();
                        var tip = (Func<string>?)rt.GetProperty("ClearTip")!.GetValue(row);
                        rows.Add(new JsonObject
                        {
                            ["property"] = (string)rt.GetProperty("Property")!.GetValue(row)!,
                            ["label"] = ((Func<string>)rt.GetProperty("Label")!.GetValue(row)!)(),
                            ["clear_tip"] = tip is null ? null : tip(),
                        });
                    }

                    c["rows"] = rows;
                    c["marker"] = (string)page.GetField("Marker", W819pFlags)!.GetRawConstantValue()!;
                    break;
                }

                case "find_problems":
                {
                    var texts = new Dictionary<string, string>(StringComparer.Ordinal);
                    foreach (var pair in c["texts"]!.AsObject())
                    {
                        texts[pair.Key] = pair.Value!.GetValue<string>();
                    }

                    var problems = (IEnumerable<string>)page.GetMethod("FindProblems", W819pFlags)!.Invoke(null, [texts])!;
                    var array = new JsonArray();
                    foreach (var p in problems.Order(StringComparer.Ordinal))
                    {
                        array.Add(p);
                    }

                    c["problems"] = array;
                    break;
                }

                default:
                    throw new InvalidOperationException("unknown settings-page-shortcuts kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
