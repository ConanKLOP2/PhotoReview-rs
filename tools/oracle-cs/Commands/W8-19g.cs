using System.Collections;
using System.Reflection;
using System.Text.Json.Nodes;

// W8-19g oracle (the F half of the unit): GeneralSettingsPage (the declaration and the static tables; the language picker, the update check and the shell status need a
// presenter and a window and are covered by fakes in Rust). Uses the shared driver `W819pDeclare` of W8-19d.cs. NOT compiled or run by the writer.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19g
//     "settings-page-general" => Commands.W819gSettingsPageGeneralCases(inputs),
//
// Run against the C# BASELINE d92d6aae (see W8-19d.cs). Inputs (tests/golden-inputs/settings-page-general.json), kinds:
//   declare   -> see W8-19d.cs ("page": "General").
//   sort_tags -> output sort_tags: the strings of GeneralSettingsPage.SortTags in order.
static partial class Commands
{
    public static JsonArray W819gSettingsPageGeneralCases(JsonObject inputs)
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
                case "sort_tags":
                {
                    var tags = new JsonArray();
                    foreach (var tag in (IEnumerable)W819pPageType("General").GetField("SortTags", W819pFlags)!.GetValue(null)!)
                    {
                        tags.Add((string)tag);
                    }

                    c["sort_tags"] = tags;
                    break;
                }

                default:
                    throw new InvalidOperationException("unknown settings-page-general kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
