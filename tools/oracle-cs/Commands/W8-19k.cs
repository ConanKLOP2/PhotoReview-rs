using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;

// W8-19k oracle (the F half of the unit): the key capture table (virtual key -> shortcut text), the shared action JSON of the working copy, the visibility book-keeping of the
// control accessors and the key box ids of SettingsDialog. NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19k
//     "settings-dialog-state" => Commands.W819kSettingsDialogStateCases(inputs),
//
// Needs W8-19j.cs (the window-less dialog driver `W819jNewDialog`, `W819jGet`, `W819jCall`, `W819jSetField`, same partial class) and W8-10b.cs (`SettingsFrom`). Build and run
// against the C# BASELINE d92d6aae. No window is created: see the header of W8-19j.cs.
//
// The key capture decision is `SettingsPresenter.CaptureShortcut(KeyIdMapping.FromVirtualKey(virtualKey, extended), key.ToString())` (both `internal` in PhotoReview.App.Shared,
// reached by reflection); the dialog adds only the window plumbing (GetDlgCtrlID, SetWindowText, EM_SETSEL), which is native and not in this golden.
//
// Inputs (tests/golden-inputs/settings-dialog-state.json), kinds and the fields added:
//   key_capture: extended (bool), from (int), to (int; inclusive)      -> texts: array of (to - from + 1) entries, null = the key is not swallowed
//   actions_json: settings (object for SettingsFrom, optional), ops [{ op: "get" | "refresh" | "set", json? }]
//                                                                      -> values: one string per op (the ActionsJson after the op)
//   visibility:  ops [{ op: "set", id, visible } | { op: "set_group", group, visible } | { op: "is", id } | { op: "is_group", group }]
//                                                                      -> group_ids (the title ids of all groups, ascending), values: one entry per `is*` op (bool)
//   key_boxes:   probes [int]                                          -> key_box_ids (ascending), results (IsKeyBoxId per probe)

static partial class Commands
{
    public static JsonArray W819kSettingsDialogStateCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        Assembly app = Assembly.Load("PhotoReview.App.Shared");
        Type mapping = app.GetTypes().Single(t => t.Name == "KeyIdMapping");
        Type presenter = app.GetTypes().Single(t => t.Name == "SettingsPresenter");
        MethodInfo fromVirtualKey = mapping.GetMethods(W819jFlags).Single(m => m.Name == "FromVirtualKey" && m.GetParameters().Length == 2);
        MethodInfo capture = presenter.GetMethods(W819jFlags).Single(m => m.Name == "CaptureShortcut" && m.GetParameters().Length == 2);
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "key_capture":
                {
                    var texts = new JsonArray();
                    bool extended = c["extended"]!.GetValue<bool>();
                    for (var vk = c["from"]!.GetValue<int>(); vk <= c["to"]!.GetValue<int>(); vk++)
                    {
                        object key = fromVirtualKey.Invoke(null, [vk, extended])!;
                        texts.Add((string?)capture.Invoke(null, [key, key.ToString()]));
                    }

                    c["texts"] = texts;
                    break;
                }

                case "actions_json":
                {
                    object d = W819jNewDialog(c["settings"] as JsonObject, null, null);
                    var values = new JsonArray();
                    foreach (var opNode in c["ops"]!.AsArray())
                    {
                        var op = opNode!.AsObject();
                        switch (op["op"]!.GetValue<string>())
                        {
                            case "get":
                                break;
                            case "refresh":
                                W819jCall(d, "RefreshActionsJsonFromPages");
                                break;
                            case "set":
                                W819jCall(d, "SetActionsJson", op["json"]!.GetValue<string>(), null);
                                break;
                            default:
                                throw new InvalidOperationException("unknown actions_json op");
                        }

                        values.Add((string)W819jGet(d, "ActionsJson")!);
                    }

                    c["values"] = values;
                    break;
                }

                case "visibility":
                {
                    object d = W819jNewDialog(null, null, null);
                    var groups = (IDictionary)W819jGet(d, "_groupsById")!;
                    var groupIds = groups.Keys.Cast<int>().OrderBy(x => x).ToList();
                    var values = new JsonArray();
                    foreach (var opNode in c["ops"]!.AsArray())
                    {
                        var op = opNode!.AsObject();
                        switch (op["op"]!.GetValue<string>())
                        {
                            case "set":
                                W819jCall(d, "SetControlVisible", op["id"]!.GetValue<int>(), op["visible"]!.GetValue<bool>());
                                break;
                            case "set_group":
                                W819jCall(d, "SetControlVisible", groupIds[op["group"]!.GetValue<int>()], op["visible"]!.GetValue<bool>());
                                break;
                            case "is":
                                values.Add((bool)W819jCall(d, "IsControlVisible", op["id"]!.GetValue<int>())!);
                                break;
                            case "is_group":
                                values.Add((bool)W819jCall(d, "IsControlVisible", groupIds[op["group"]!.GetValue<int>()])!);
                                break;
                            default:
                                throw new InvalidOperationException("unknown visibility op");
                        }
                    }

                    var ids = new JsonArray();
                    foreach (var id in groupIds)
                    {
                        ids.Add(id);
                    }

                    c["group_ids"] = ids;
                    c["values"] = values;
                    break;
                }

                case "key_boxes":
                {
                    object d = W819jNewDialog(null, null, null);
                    var boxes = ((IEnumerable)W819jGet(d, "_keyBoxes")!).Cast<int>().OrderBy(x => x).ToList();
                    var ids = new JsonArray();
                    foreach (var id in boxes)
                    {
                        ids.Add(id);
                    }

                    var results = new JsonArray();
                    foreach (var probe in c["probes"]!.AsArray())
                    {
                        results.Add((bool)W819jCall(d, "IsKeyBoxId", probe!.GetValue<int>())!);
                    }

                    c["key_box_ids"] = ids;
                    c["results"] = results;
                    break;
                }

                default:
                    throw new InvalidOperationException("unknown settings-dialog-state kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
