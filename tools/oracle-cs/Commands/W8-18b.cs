using System.Globalization;
using System.Text.Json.Nodes;
using PhotoReview.Core.Localization;
using PhotoReview.Shell.Win32.Dialogs;

// W8-18b oracle (the F half of the unit): FolderPicker.UsableInitialFolder, FolderPicker.Pick (what reaches the folder dialog) and the English text of
// Tr.DialogPickFolderTitle. The rest of Win32DialogService / Win32ZoomPromptService / TaskDialogs.cs is delegation over seams and needs the internal
// Shell.Interop types, which the oracle cannot reach; those are hand-written expectations from Win32DialogServiceTests (see contracts/W8-18b.md).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18b
//     "folder-picker-policy" => Commands.W818bFolderPickerPolicyCases(inputs),
// Wiring (the lead does this, the writer does not run the oracle):
//   * tools/oracle-cs/OracleCs.csproj: add, inside the existing `<ItemGroup Condition="'$(CSharpSrcDir)' != ''">` (the W7-03d / W8-16e pattern),
//         <Compile Include="$(CSharpSrcDir)\src\PhotoReview.Shell.Win32\Dialogs\FolderPicker.cs" Link="cs-baseline\FolderPicker.cs" />
//     FolderPicker.cs is compiled into the oracle as it is at the baseline (nothing copied by hand). It is `internal` (fine inside the oracle assembly) and uses only
//     System.IO, so no other prerequisite. The `using PhotoReview.Shell.Win32.Dialogs;` above names its namespace (the files compiled from the baseline keep it).
//     `Tr` is the generated class of PhotoReview.Core (already referenced).
//
// Determinism: CurrentCulture is Invariant. The "$ROOT" token in an input path is replaced by a fresh temp root created by the oracle (with the folder "photos",
// the folder "photos\sub" and the file "a.jpg") and deleted afterwards; outputs never contain that root (it is turned back into "$ROOT").
//
// Inputs (tests/golden-inputs/folder-picker-policy.json):
//   "usable": [ { "id", "folder": <string|null> } ]                  -> FolderPicker.UsableInitialFolder(folder)
//   "pick":   [ { "id", "owner": <int>, "title", "folder": <string|null>, "answer": <string|null> } ]
//                                                                     -> new FolderPicker(fake, () => owner).Pick(title, folder); the fake records its Show call
// Output cases (each is the input case plus the fields below, "kind" added):
//   usable -> "result": null (C# null), "same" (the result is the input string, unchanged) or "other:<text>" (anything else, root turned back into "$ROOT")
//   pick   -> "result": the fake's answer as returned by Pick (string|null); "shown": { "owner", "title", "initial": string|null (root turned back into "$ROOT") }
//   title  -> one case { "kind": "title", "id": "pick-folder-title", "text": Tr.DialogPickFolderTitle } (English, default localizer)
static partial class Commands
{
    private sealed class W818bRecordingDialog : IShellFolderDialog
    {
        public string? Answer;
        public (nint Owner, string Title, string? Initial)? Last;

        public string? Show(nint owner, string title, string? initialFolder)
        {
            Last = (owner, title, initialFolder);
            return Answer;
        }
    }

    private static string? W818bExpand(string? text, string root) => text?.Replace("$ROOT", root, StringComparison.Ordinal);

    private static string? W818bCollapse(string? text, string root) => text?.Replace(root, "$ROOT", StringComparison.Ordinal);

    public static JsonArray W818bFolderPickerPolicyCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        var root = Path.Combine(Path.GetTempPath(), "oracle-w8-18b-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(root, "photos", "sub"));
        File.WriteAllText(Path.Combine(root, "a.jpg"), "x");
        try
        {
            foreach (var node in inputs["usable"]!.AsArray())
            {
                var input = node!.AsObject();
                var folder = input["folder"]?.GetValue<string>();
                var expanded = W818bExpand(folder, root);
                var usable = FolderPicker.UsableInitialFolder(expanded);
                var entry = (JsonObject)input.DeepClone();
                entry["kind"] = "usable";
                entry["result"] = usable is null
                    ? null
                    : string.Equals(usable, expanded, StringComparison.Ordinal) ? "same" : "other:" + W818bCollapse(usable, root);
                result.Add(entry);
            }

            foreach (var node in inputs["pick"]!.AsArray())
            {
                var input = node!.AsObject();
                var fake = new W818bRecordingDialog { Answer = input["answer"]?.GetValue<string>() };
                var owner = (nint)input["owner"]!.GetValue<int>();
                var picker = new FolderPicker(fake, () => owner);
                var picked = picker.Pick(input["title"]!.GetValue<string>(), W818bExpand(input["folder"]?.GetValue<string>(), root));
                var entry = (JsonObject)input.DeepClone();
                entry["kind"] = "pick";
                entry["result"] = picked;
                var last = fake.Last!.Value;
                entry["shown"] = new JsonObject
                {
                    ["owner"] = (int)last.Owner,
                    ["title"] = last.Title,
                    ["initial"] = W818bCollapse(last.Initial, root),
                };
                result.Add(entry);
            }
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }

        result.Add(new JsonObject { ["kind"] = "title", ["id"] = "pick-folder-title", ["text"] = Tr.DialogPickFolderTitle });
        return result;
    }
}
