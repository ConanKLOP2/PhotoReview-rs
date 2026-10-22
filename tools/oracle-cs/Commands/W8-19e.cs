using System.Collections;
using System.Reflection;
using System.Text.Json.Nodes;

// W8-19e oracle (the F half of the unit): DisplaySettingsPage. Uses the shared driver `W819pDeclare` of W8-19d.cs (same partial class). NOT compiled or run by the writer.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19e
//     "settings-page-display" => Commands.W819eSettingsPageDisplayCases(inputs),
//
// Run against the C# BASELINE d92d6aae (see W8-19d.cs). Inputs (tests/golden-inputs/settings-page-display.json), kinds:
//   declare -> see W8-19d.cs ("page": "Display", "lists": ["ExifChecks", "TitleChecks"]); output list_ids.exif_checks / title_checks.
//   orders  -> output exif_order and title_order: arrays of { name (enum member name), value (int) } of the private static ExifOrder / TitleOrder arrays.
static partial class Commands
{
    public static JsonArray W819eSettingsPageDisplayCases(JsonObject inputs)
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
                case "orders":
                {
                    Type page = W819pPageType("Display");
                    c["exif_order"] = W819eEnums((IEnumerable)page.GetField("ExifOrder", W819pFlags)!.GetValue(null)!);
                    c["title_order"] = W819eEnums((IEnumerable)page.GetField("TitleOrder", W819pFlags)!.GetValue(null)!);
                    break;
                }

                default:
                    throw new InvalidOperationException("unknown settings-page-display kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }

    private static JsonArray W819eEnums(IEnumerable values)
    {
        var array = new JsonArray();
        foreach (var value in values)
        {
            array.Add(new JsonObject { ["name"] = value.ToString(), ["value"] = Convert.ToInt32(value) });
        }

        return array;
    }
}
