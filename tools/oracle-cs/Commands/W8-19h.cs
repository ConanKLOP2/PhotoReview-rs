using System.Text.Json.Nodes;

// W8-19h oracle (the F half of the unit): MouseSettingsPage (the declaration, including the one-tick-box-per-menu-item group built from SettingsPresenter.ContextMenuRows()).
// Uses the shared driver `W819pDeclare` of W8-19d.cs. NOT compiled or run by the writer.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19h
//     "settings-page-mouse" => Commands.W819hSettingsPageMouseCases(inputs),
//
// Run against the C# BASELINE d92d6aae (see W8-19d.cs): the HEAD page calls ContextMenuRows(includeShellOnly: true), which the Rust presenter does not have.
// Inputs (tests/golden-inputs/settings-page-mouse.json), kinds:
//   declare -> see W8-19d.cs ("page": "Mouse", "menu_checks": true); output menu_checks: array of { item (ContextMenuItemId name), id } in dictionary order.
static partial class Commands
{
    public static JsonArray W819hSettingsPageMouseCases(JsonObject inputs)
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
                default:
                    throw new InvalidOperationException("unknown settings-page-mouse kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
