using System.Globalization;
using System.Text;
using System.Text.Json.Nodes;
using PhotoReview.App.Localization;
using PhotoReview.Core.Localization;

// W8-04b oracle: TranslationExport.BuildJson, TranslationExport.Export (real files in a temp folder) and
// TranslationExport.ReadNotes, plus the ArgumentException checks of Export (the "invalid_code" kind).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-04b
//     "translation-export" => Commands.TranslationExportCases(inputs),
// Wiring (the lead does this, the writer does not run the oracle):
//   * tools/oracle-cs/OracleCs.csproj: add
//         <Reference Include="PhotoReview.App.Shared"><HintPath>$(CSharpCoreDir)\PhotoReview.App.Shared.dll</HintPath></Reference>
//     (TranslationExport lives in PhotoReview.App.Shared; its HelpText reads the generated Tr class of the same assembly.)
//   * tools/oracle.ps1: build src\PhotoReview.App.Shared\PhotoReview.App.Shared.csproj -c Release -o $coreDir, after
//     the projects it references (Core, Imaging, Platform.Windows if App.Shared references it; check the csproj).
// Determinism: CurrentCulture is Invariant; every export runs in a fresh temp root that is deleted afterwards; outputs
// hold only relative names and file texts, never temp paths. The English help text is whatever Tr.ExportHelp returns
// under the default (English) Localizer; the Rust golden test passes that same text to build_json_with_help.
//
// Output per case: {"id", ...}:
//   build          -> "output": the exact text of BuildJson (one trailing "\n").
//   export         -> "path": relative path of the written file; "files": every file of the user folder after the
//                     last run (name -> text, ordinal name order), so backups show up.
//   read_notes     -> "notes": key -> value of ReadNotes (ordinal key order).
//   invalid_code   -> "threw": "ArgumentException" and "user_dir_created": whether the user folder exists afterwards.
//   Any other exception is NOT caught: the oracle must fail loudly.
static partial class Commands
{
    /// <summary>A catalog given as a JSON object (serialized as-is) or as raw JSON text (may be broken on purpose).</summary>
    private static string? W804bJsonText(JsonNode? node) => node switch
    {
        null => null,
        JsonObject obj => obj.ToJsonString(),
        _ => node.GetValue<string>(),
    };

    private static LanguageCatalog? W804bCatalog(JsonNode? node)
    {
        var text = W804bJsonText(node);
        if (text is null) return null;
        return LanguageCatalog.TryParse(text, "case", out var catalog, new List<string>()) ? catalog : null;
    }

    private static Dictionary<string, string> W804bNotes(JsonObject? obj)
    {
        var notes = new Dictionary<string, string>(StringComparer.Ordinal);
        if (obj is null) return notes;
        foreach (var (key, value) in obj) notes[key] = value!.GetValue<string>();
        return notes;
    }

    /// <summary>Writes each file (value: object, raw text string or null is skipped) into dir; creates dir even when empty.</summary>
    private static void W804bWriteFiles(string dir, JsonObject? files)
    {
        Directory.CreateDirectory(dir);
        if (files is null) return;
        foreach (var (name, value) in files)
        {
            var text = W804bJsonText(value);
            if (text is null) continue;
            File.WriteAllText(Path.Combine(dir, name), text, new UTF8Encoding(encoderShouldEmitUTF8Identifier: false));
        }
    }

    private static JsonObject W804bFilesOf(string dir)
    {
        var files = new JsonObject();
        if (!Directory.Exists(dir)) return files;
        foreach (var file in Directory.GetFiles(dir).OrderBy(f => Path.GetFileName(f), StringComparer.Ordinal))
            files[Path.GetFileName(file)] = File.ReadAllText(file);
        return files;
    }

    public static JsonArray TranslationExportCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var id = c["id"]!.GetValue<string>();
            var kind = c["kind"]!.GetValue<string>();
            // The input is echoed so the golden file is self-contained: the Rust test reads case["input"].
            var item = new JsonObject { ["id"] = id, ["input"] = c.DeepClone() };
            switch (kind)
            {
                case "build":
                {
                    var english = W804bCatalog(c["english"]) ?? BuiltInCatalog.English;
                    var translations = new List<LanguageCatalog>();
                    foreach (var t in c["translations"]?.AsArray() ?? [])
                    {
                        var catalog = W804bCatalog(t);
                        if (catalog is not null) translations.Add(catalog);
                    }
                    var output = TranslationExport.BuildJson(english, c["code"]!.GetValue<string>(), translations,
                        W804bNotes(c["notes"]?.AsObject()));
                    item["output"] = output;
                    break;
                }
                case "export":
                {
                    var root = Directory.CreateTempSubdirectory("oracle-w8b1-").FullName;
                    try
                    {
                        var shipped = Path.Combine(root, "App", "Languages");
                        var user = Path.Combine(root, "User", "Languages");
                        W804bWriteFiles(shipped, c["shipped"]?.AsObject());
                        if (c["user_missing"]?.GetValue<bool>() != true) W804bWriteFiles(user, c["user"]?.AsObject());
                        var runs = c["runs"]?.GetValue<int>() ?? 1;
                        string path = "";
                        for (var run = 1; run <= runs; run++)
                        {
                            if (run > 1 && c["between_runs"] is JsonObject between) W804bWriteFiles(user, between);
                            path = TranslationExport.Export(c["code"]!.GetValue<string>(), shipped, user);
                        }
                        item["path"] = Path.GetRelativePath(user, path);
                        item["files"] = W804bFilesOf(user);
                    }
                    finally
                    {
                        Directory.Delete(root, recursive: true);
                    }
                    break;
                }
                case "read_notes":
                {
                    var root = Directory.CreateTempSubdirectory("oracle-w8b1-").FullName;
                    try
                    {
                        var path = Path.Combine(root, "en.notes.json");
                        if (c["missing"]?.GetValue<bool>() != true)
                        {
                            var text = c["text"]!.GetValue<string>();
                            var bytes = new UTF8Encoding(encoderShouldEmitUTF8Identifier: false).GetBytes(text);
                            var padTo = c["pad_to_bytes"]?.GetValue<int>() ?? 0;
                            if (bytes.Length < padTo)
                            {
                                var padded = new byte[padTo];
                                Array.Fill(padded, (byte)' ');
                                Array.Copy(bytes, padded, bytes.Length);
                                bytes = padded;
                            }
                            File.WriteAllBytes(path, bytes);
                        }
                        var notes = new JsonObject();
                        foreach (var (key, value) in TranslationExport.ReadNotes(path).OrderBy(p => p.Key, StringComparer.Ordinal))
                            notes[key] = value;
                        item["notes"] = notes;
                    }
                    finally
                    {
                        Directory.Delete(root, recursive: true);
                    }
                    break;
                }
                case "invalid_code":
                {
                    var root = Directory.CreateTempSubdirectory("oracle-w8b1-").FullName;
                    try
                    {
                        var shipped = Path.Combine(root, "App");
                        var user = Path.Combine(root, "User", "Languages");
                        try
                        {
                            TranslationExport.Export(c["code"]!.GetValue<string>(), shipped, user);
                            throw new InvalidOperationException("W8-04b: Export accepted an invalid code");
                        }
                        catch (ArgumentException)
                        {
                            item["threw"] = "ArgumentException";
                        }
                        item["user_dir_created"] = Directory.Exists(user);
                    }
                    finally
                    {
                        Directory.Delete(root, recursive: true);
                    }
                    break;
                }
                default:
                    throw new InvalidOperationException($"W8-04b: unknown kind '{kind}' in case '{id}'");
            }
            result.Add(item);
        }
        return result;
    }
}
