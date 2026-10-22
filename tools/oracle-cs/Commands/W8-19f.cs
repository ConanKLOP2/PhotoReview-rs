using System.Reflection;
using System.Text.Json.Nodes;

// W8-19f oracle (the F half of the unit): FilesSettingsPage. Uses the shared driver `W819pDeclare` of W8-19d.cs (same partial class). NOT compiled or run by the writer.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19f
//     "settings-page-files" => Commands.W819fSettingsPageFilesCases(inputs),
//
// Run against the C# BASELINE d92d6aae (see W8-19d.cs): the HEAD page has the external editors controls that the Rust port does not have yet.
// Inputs (tests/golden-inputs/settings-page-files.json), kinds:
//   declare        -> see W8-19d.cs ("page": "Files", "lists": ["JournalRadios"]); output list_ids.journal_radios.
//   initial_folder -> FilesSettingsPage.InitialFolder(path). Input "make": "none" (use "path" as given) | "temp_file" (the oracle creates <tmp>/editor.exe and uses it) |
//                     "temp_dir" (uses an existing DIRECTORY as the path). Output "result_kind": "null" | "parent_dir" (the result equals the folder that holds the
//                     temp file) | "other" (then "result" holds the text). The temp folder is deleted before the case returns.
static partial class Commands
{
    public static JsonArray W819fSettingsPageFilesCases(JsonObject inputs)
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
                case "initial_folder":
                    W819fInitialFolder(c);
                    break;
                default:
                    throw new InvalidOperationException("unknown settings-page-files kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }

    private static void W819fInitialFolder(JsonObject c)
    {
        MethodInfo method = W819pPageType("Files").GetMethod("InitialFolder", W819pFlags)!;
        var temp = Path.Combine(Path.GetTempPath(), "w819f-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(temp);
        try
        {
            string path;
            switch (c["make"]!.GetValue<string>())
            {
                case "temp_file":
                    path = Path.Combine(temp, "editor.exe");
                    File.WriteAllText(path, "x");
                    break;
                case "temp_dir":
                    path = temp;
                    break;
                default:
                    path = c["path"]!.GetValue<string>();
                    break;
            }

            var result = (string?)method.Invoke(null, [path]);
            if (result is null)
            {
                c["result_kind"] = "null";
            }
            else if (string.Equals(result, temp, StringComparison.OrdinalIgnoreCase))
            {
                c["result_kind"] = "parent_dir";
            }
            else
            {
                c["result_kind"] = "other";
                c["result"] = result;
            }
        }
        finally
        {
            Directory.Delete(temp, recursive: true);
        }
    }
}
