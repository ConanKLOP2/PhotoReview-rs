using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;

// W8-19d oracle (the F half of the unit): DiagnosticsSettingsPage and PerformanceSettingsPage, plus the SHARED page driver `W819pDeclare` that the oracle files of
// W8-19e..19i also use (they are all `partial class Commands`; keep this file when the others are present). NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19d
//     "settings-pages-diag-perf" => Commands.W819dSettingsPagesCases(inputs),
//
// IMPORTANT: build and run the oracle against the C# BASELINE d92d6aae (a worktree of the C# repo at that sha). The C# HEAD has more controls on the Files page
// (external editors), two more Shortcuts rows and a different Mouse menu list; the Rust port is at the baseline.
//
// Wiring: none beyond the line above. The pages and the builder are `internal` and are reached by reflection (PhotoReview.dll is referenced), as W8-19c. The default
// localizer is English. `W819cDump` and `W819cType` of W8-19c.cs (same partial class, `private`) are reused.
//
// Inputs (tests/golden-inputs/settings-pages-diag-perf.json), "cases": [ { "name", "kind", ... } ], kinds:
//   declare   -> "page" ("Diagnostics" | "Performance"), "id_base" (int, what SettingsDialog.PageIdBase(index) gives), "fields" (array of private field names of the page,
//                e.g. "_logging"), optional "lists" (array of property names returning IReadOnlyList<int>), optional "menu_checks": true, optional "cells": true.
//                Output: key, title (English), the W819cDump fields (items, texts, notes, key_boxes, warnings, combo_items, sliders, elements), ids (object: field name
//                without the underscore, snake_case, to the id), list_ids (object: property name snake_case to an array of ids), menu_checks (array of {item, id}),
//                cells (array of {property, label, box, clear}).
//   constants -> output turbo_jpeg_index (PerformanceSettingsPage.TurboJpegIndex).
static partial class Commands
{
    private const string W819pNs = "PhotoReview.Shell.Win32.Dialogs.SettingsUi.Pages.";
    private const BindingFlags W819pFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    internal static Type W819pPageType(string page) => Assembly.Load("PhotoReview").GetType(W819pNs + page + "SettingsPage", throwOnError: true)!;

    internal static string W819pSnake(string member)
    {
        var trimmed = member.TrimStart('_');
        var sb = new System.Text.StringBuilder();
        for (var i = 0; i < trimmed.Length; i++)
        {
            if (char.IsUpper(trimmed[i]) && i > 0)
            {
                sb.Append('_');
            }

            sb.Append(char.ToLowerInvariant(trimmed[i]));
        }

        return sb.ToString();
    }

    /// <summary>Creates the page, runs Declare on a fresh SettingsPageBuilder(id_base) and fills the case with the dump and the ids (see the header).</summary>
    internal static void W819pDeclare(JsonObject c)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        Type pageType = W819pPageType(c["page"]!.GetValue<string>());
        ConstructorInfo ctor = pageType.GetConstructors(W819pFlags).Single(k => !k.IsStatic);
        object page = ctor.Invoke(new object?[ctor.GetParameters().Length]);
        c["key"] = (string)pageType.GetProperty("Key", W819pFlags)!.GetValue(page)!;
        c["title"] = (string)pageType.GetProperty("Title", W819pFlags)!.GetValue(page)!;
        Type builderType = W819cType("SettingsPageBuilder");
        object builder = builderType.GetConstructors(W819pFlags).Single(x => x.GetParameters().Length == 1).Invoke([c["id_base"]!.GetValue<int>()]);
        pageType.GetMethod("Declare", W819pFlags)!.Invoke(page, [builder]);
        W819cDump(c, builderType, builder);

        var ids = new JsonObject();
        foreach (var field in c["fields"]!.AsArray())
        {
            var name = field!.GetValue<string>();
            ids[W819pSnake(name)] = (int)pageType.GetField(name, W819pFlags)!.GetValue(page)!;
        }

        c["ids"] = ids;
        if (c["lists"] is JsonArray lists)
        {
            var listIds = new JsonObject();
            foreach (var property in lists)
            {
                var name = property!.GetValue<string>();
                var array = new JsonArray();
                foreach (var id in (IEnumerable)pageType.GetProperty(name, W819pFlags)!.GetValue(page)!)
                {
                    array.Add((int)id);
                }

                listIds[W819pSnake(name)] = array;
            }

            c["list_ids"] = listIds;
        }

        if (c["menu_checks"]?.GetValue<bool>() == true)
        {
            var checks = new JsonArray();
            foreach (var pair in (IEnumerable)pageType.GetProperty("MenuChecks", W819pFlags)!.GetValue(page)!)
            {
                Type t = pair.GetType();
                checks.Add(new JsonObject { ["item"] = t.GetProperty("Key")!.GetValue(pair)!.ToString(), ["id"] = (int)t.GetProperty("Value")!.GetValue(pair)! });
            }

            c["menu_checks"] = checks;
        }

        if (c["cells"]?.GetValue<bool>() == true)
        {
            var cells = new JsonArray();
            foreach (var pair in (IEnumerable)pageType.GetField("_cells", W819pFlags)!.GetValue(page)!)
            {
                Type t = pair.GetType();
                var cell = t.GetProperty("Value")!.GetValue(pair)!;
                Type ct = cell.GetType();
                cells.Add(new JsonObject
                {
                    ["property"] = (string)t.GetProperty("Key")!.GetValue(pair)!,
                    ["label"] = (int)ct.GetProperty("Label")!.GetValue(cell)!,
                    ["box"] = (int)ct.GetProperty("Box")!.GetValue(cell)!,
                    ["clear"] = (int)ct.GetProperty("Clear")!.GetValue(cell)!,
                });
            }

            c["cells"] = cells;
        }
    }

    public static JsonArray W819dSettingsPagesCases(JsonObject inputs)
    {
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "declare":
                    W819pDeclare(c);
                    break;
                case "constants":
                    c["turbo_jpeg_index"] = (int)W819pPageType("Performance").GetField("TurboJpegIndex", W819pFlags)!.GetRawConstantValue()!;
                    break;
                default:
                    throw new InvalidOperationException("unknown settings-pages-diag-perf kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
