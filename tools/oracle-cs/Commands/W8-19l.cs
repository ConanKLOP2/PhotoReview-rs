using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;

// W8-19l oracle (the F half of the unit): which page and control own each refused field, what Save (TrySaveAll) shows for a valid and for a refused form, and what Export and
// Import ask of the file dialog and show to the user. NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19l
//     "settings-dialog-apply" => Commands.W819lSettingsDialogApplyCases(inputs),
//
// Needs W8-19j.cs (the window-less dialog driver and the two DispatchProxy recorders) and W8-10b.cs (`SettingsFrom`). Build and run against the C# BASELINE d92d6aae. The
// recorders implement the `internal` interfaces IDialogWarningSink and IShellFileDialog through System.Reflection.DispatchProxy (see W8-18v.cs for the fallback if the runtime
// refuses). No window is created (Hwnd == 0): no page has controls, so CollectAll() is the presenter's LoadFields() of the case's settings. That is exactly what the Rust
// window-less tests do (`SettingsDialog::new` without `show_modal`).
//
// Limits: a SaveFailed outcome (the store cannot write) needs an IAppPaths and a file system and is covered by a hand-written Rust test with a failing store; the NAVIGATION that
// follows a refused field (ActivatePage + FocusControl) needs controls and is covered by the native tests. Messages that carry an operating system or JSON parser text
// (Import of a broken file) are recorded as `message_nonempty` only.
//
// Inputs (tests/golden-inputs/settings-dialog-apply.json), kinds and the fields added:
//   focus_targets: (none)                       -> targets: [{ field (SettingsFocusField name), page (index of the FIRST page whose FocusTarget is not null, or null), id (int or null) }]
//   save_flow:     settings (object, optional), calls (int >= 1), pin_message (bool)
//                                               -> results [bool per call], saved (property Saved), warnings [{ title, message_nonempty, message? (only when pin_message) }]
//   export:        settings (optional), files (bool: Options.Files set), save_result ("none" | "file")
//                                               -> calls [{ call:"save", title, filters, default_extension, suggested_file_name, initial_folder_set }],
//                                                  warnings, file_written (bool), logging_enabled_in_file (bool or null)
//   import:        mode ("from_path" | "button"), files (bool), pick ("cancel" | "file") for "button", file_text (string, the file content for "from_path" and "file")
//                                               -> calls [{ call:"open", ... }], warnings, logging_enabled (Settings.LoggingEnabled after), file_read (bool)

static partial class Commands
{
    private static JsonArray W819lWarnings(bool pinMessage)
    {
        var warnings = new JsonArray();
        foreach (var call in W819jWarningProxy.Calls)
        {
            var w = new JsonObject { ["title"] = (string)call["title"]!, ["message_nonempty"] = ((string)call["message"]!).Length > 0 };
            if (pinMessage)
            {
                w["message"] = (string)call["message"]!;
            }

            warnings.Add(w);
        }

        return warnings;
    }

    public static JsonArray W819lSettingsDialogApplyCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        Assembly app = Assembly.Load("PhotoReview.App.Shared");
        Type focusField = app.GetTypes().Single(t => t.Name == "SettingsFocusField");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            W819jWarningProxy.Calls.Clear();
            W819jFilesProxy.Calls.Clear();
            W819jFilesProxy.OpenResult = null;
            W819jFilesProxy.SaveResult = null;
            object warnings = W819jProxy("IDialogWarningSink", typeof(W819jWarningProxy));
            switch (c["kind"]!.GetValue<string>())
            {
                case "focus_targets":
                {
                    object d = W819jNewDialog(null, null, warnings);
                    var pages = ((IEnumerable)W819jGet(d, "Pages")!).Cast<object>().ToList();
                    var targets = new JsonArray();
                    foreach (var name in Enum.GetNames(focusField))
                    {
                        object field = Enum.Parse(focusField, name);
                        int? page = null;
                        int? id = null;
                        for (var i = 0; i < pages.Count && page is null; i++)
                        {
                            MethodInfo m = pages[i].GetType().GetMethod("FocusTarget", W819jFlags)!;
                            if (m.Invoke(pages[i], [field]) is int found)
                            {
                                page = i;
                                id = found;
                            }
                        }

                        targets.Add(new JsonObject { ["field"] = name, ["page"] = page, ["id"] = id });
                    }

                    c["targets"] = targets;
                    break;
                }

                case "save_flow":
                {
                    object d = W819jNewDialog(c["settings"] as JsonObject, null, warnings);
                    var results = new JsonArray();
                    for (var i = 0; i < c["calls"]!.GetValue<int>(); i++)
                    {
                        results.Add((bool)W819jCall(d, "TrySaveAll")!);
                    }

                    c["results"] = results;
                    c["saved"] = (bool)W819jGet(d, "Saved")!;
                    c["warnings"] = W819lWarnings(c["pin_message"]!.GetValue<bool>());
                    break;
                }

                case "export":
                {
                    string? path = null;
                    object? files = null;
                    if (c["files"]!.GetValue<bool>())
                    {
                        files = W819jProxy("IShellFileDialog", typeof(W819jFilesProxy));
                        if (c["save_result"]!.GetValue<string>() == "file")
                        {
                            path = Path.Combine(Path.GetTempPath(), "w819l-export-" + Guid.NewGuid().ToString("N") + ".json");
                            W819jFilesProxy.SaveResult = path;
                        }
                    }

                    object d = W819jNewDialog(c["settings"] as JsonObject, files, warnings);
                    W819jCall(d, "ExportSettings");
                    c["calls"] = new JsonArray(W819jFilesProxy.Calls.Select(x => (JsonNode?)x.DeepClone()).ToArray());
                    c["warnings"] = W819lWarnings(false);
                    bool written = path is not null && File.Exists(path);
                    c["file_written"] = written;
                    c["logging_enabled_in_file"] = written ? (bool?)JsonNode.Parse(File.ReadAllText(path!))!["LoggingEnabled"]!.GetValue<bool>() : null;
                    if (path is not null && written)
                    {
                        File.Delete(path);
                    }

                    break;
                }

                case "import":
                {
                    string? path = null;
                    string mode = c["mode"]!.GetValue<string>();
                    if (mode == "from_path" || c["pick"]?.GetValue<string>() == "file")
                    {
                        path = Path.Combine(Path.GetTempPath(), "w819l-import-" + Guid.NewGuid().ToString("N") + ".json");
                        File.WriteAllText(path, c["file_text"]!.GetValue<string>());
                    }

                    object? files = null;
                    if (c["files"]!.GetValue<bool>())
                    {
                        files = W819jProxy("IShellFileDialog", typeof(W819jFilesProxy));
                        W819jFilesProxy.OpenResult = mode == "button" ? path : null;
                    }

                    object d = W819jNewDialog(null, files, warnings);
                    if (mode == "from_path")
                    {
                        W819jCall(d, "ImportSettingsFrom", path);
                    }
                    else
                    {
                        W819jCall(d, "ImportSettings");
                    }

                    c["calls"] = new JsonArray(W819jFilesProxy.Calls.Select(x => (JsonNode?)x.DeepClone()).ToArray());
                    c["warnings"] = W819lWarnings(false);
                    object settings = W819jGet(d, "Settings")!;
                    c["logging_enabled"] = (bool)settings.GetType().GetProperty("LoggingEnabled")!.GetValue(settings)!;
                    c["file_read"] = path is not null;
                    if (path is not null)
                    {
                        File.Delete(path);
                    }

                    break;
                }

                default:
                    throw new InvalidOperationException("unknown settings-dialog-apply kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
