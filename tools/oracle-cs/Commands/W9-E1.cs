// W9-E1: external-editor-rules  (Core/Settings/ExternalEditor.cs: ExternalEditor + ExternalEditors, baseline 7eaee2a2)
//
// Dispatch line in tools/oracle-cs/Program.cs (inside the `command switch`, W9 block):
//     "external-editor-rules" => Commands.W9E1ExternalEditorRulesCases(inputs),
//
// Inputs: tests/golden-inputs/external-editor-rules.json  { "cases": [ ... ] }. Output: tests/golden/external-editor-rules.json.
// Every case has "kind" and "name"; the output copies both and adds the result fields.
//   kind "constants"                                          -> max_editors, default_arguments, legacy_editor_id
//   kind "name"           { path }                            -> value   (NameFromPath; path may be null)
//   kind "display_name"   { editor_name, path }               -> value   (new ExternalEditor { Name, Path }.DisplayName)
//   kind "arguments"      { template, path }                  -> value, or error "ArgumentException" (empty path)
//   kind "effective"      { settings, find_ids }              -> effective (list), effective_default_index, default_index, find { id: index }
//   kind "reconcile"      { settings }                        -> repaired + settings
//   kind "apply_legacy"   { settings, legacy_path }           -> settings after ApplyLegacyPath
//   kind "resolve"        { path, paired_raw, send_raw, existing } -> value (existing = files that exist, OrdinalIgnoreCase)
// "settings" = { "editors": [ { id, name, path, arguments } | null ] (absent = empty list; "editors_null": true = the property is null),
//                "default_id": string|null, "legacy_path": string|null }. A missing editor field keeps the C# default (fresh Id / "" / "\"{file}\"").
// Output "settings" = { "editors": [ { id, name, path, arguments } ], "default_index": int (-1 = DefaultExternalEditorId names no editor),
//                       "default_id_blank": bool, "legacy_path": string }. An id the input did not contain is written "<fresh>" (a GUID is random), the id
//                       "legacy" stays. Index results: -1 = null, -2 = a value that is not an element of the list (a copy: the implicit legacy editor).
// Nothing reads the clock, a file or the machine.

using System.Text.Json.Nodes;
using PhotoReview.Core.Settings;

static partial class Commands
{
    private static string? W9E1Str(JsonNode? node) => node is null ? null : node.GetValue<string>();

    private static AppSettings W9E1Settings(JsonObject s, HashSet<string> knownIds)
    {
        var settings = new AppSettings();
        if (s["editors_null"]?.GetValue<bool>() == true)
        {
            settings.ExternalEditors = null!;
        }
        else if (s["editors"] is JsonArray list)
        {
            settings.ExternalEditors = [];
            foreach (var node in list)
            {
                if (node is null)
                {
                    settings.ExternalEditors.Add(null!);
                    continue;
                }

                var o = node.AsObject();
                var editor = new ExternalEditor();
                if (o.ContainsKey("id")) editor.Id = W9E1Str(o["id"])!;
                if (o.ContainsKey("name")) editor.Name = W9E1Str(o["name"])!;
                if (o.ContainsKey("path")) editor.Path = W9E1Str(o["path"])!;
                if (o.ContainsKey("arguments")) editor.Arguments = W9E1Str(o["arguments"])!;
                if (editor.Id is { Length: > 0 }) knownIds.Add(editor.Id);
                settings.ExternalEditors.Add(editor);
            }
        }

        if (s.ContainsKey("default_id")) settings.DefaultExternalEditorId = W9E1Str(s["default_id"])!;
        if (s.ContainsKey("legacy_path")) settings.ExternalEditorPath = W9E1Str(s["legacy_path"])!;
        return settings;
    }

    private static JsonObject W9E1Editor(ExternalEditor e, HashSet<string> knownIds) => new()
    {
        ["id"] = e.Id is null ? null : (knownIds.Contains(e.Id) || e.Id == ExternalEditors.LegacyEditorId ? e.Id : "<fresh>"),
        ["name"] = e.Name,
        ["path"] = e.Path,
        ["arguments"] = e.Arguments,
    };

    private static JsonObject W9E1Dump(AppSettings settings, HashSet<string> knownIds)
    {
        var editors = new JsonArray();
        var index = -1;
        var i = 0;
        foreach (var e in settings.ExternalEditors ?? [])
        {
            editors.Add(W9E1Editor(e, knownIds));
            if (index < 0 && string.Equals(e.Id, settings.DefaultExternalEditorId, StringComparison.Ordinal)) index = i;
            i++;
        }

        return new JsonObject
        {
            ["editors"] = editors,
            ["default_index"] = index,
            ["default_id_blank"] = string.IsNullOrEmpty(settings.DefaultExternalEditorId),
            ["legacy_path"] = settings.ExternalEditorPath,
        };
    }

    private static int W9E1IndexOf(IReadOnlyList<ExternalEditor> list, ExternalEditor? editor)
    {
        if (editor is null) return -1;
        for (var i = 0; i < list.Count; i++)
        {
            if (ReferenceEquals(list[i], editor)) return i;
        }

        return -2;
    }

    public static JsonArray W9E1ExternalEditorRulesCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var item = new JsonObject { ["kind"] = kind, ["name"] = W9E1Str(c["name"]) };
            switch (kind)
            {
                case "constants":
                    item["max_editors"] = ExternalEditors.MaxEditors;
                    item["default_arguments"] = ExternalEditor.DefaultArguments;
                    item["legacy_editor_id"] = ExternalEditors.LegacyEditorId;
                    break;
                case "name":
                    item["value"] = ExternalEditors.NameFromPath(W9E1Str(c["path"]));
                    break;
                case "display_name":
                    item["value"] = new ExternalEditor { Name = W9E1Str(c["editor_name"])!, Path = W9E1Str(c["path"])! }.DisplayName;
                    break;
                case "arguments":
                    try
                    {
                        item["value"] = ExternalEditors.BuildArguments(W9E1Str(c["template"]), W9E1Str(c["path"])!);
                    }
                    catch (ArgumentException ex)
                    {
                        item["error"] = ex.GetType().Name;
                    }

                    break;
                case "effective":
                {
                    var known = new HashSet<string>(StringComparer.Ordinal);
                    var settings = W9E1Settings(c["settings"]!.AsObject(), known);
                    var effective = ExternalEditors.Effective(settings);
                    var arr = new JsonArray();
                    foreach (var e in effective) arr.Add(W9E1Editor(e, known));
                    item["effective"] = arr;
                    item["effective_default_index"] = W9E1IndexOf(effective, ExternalEditors.EffectiveDefault(settings));
                    var list = settings.ExternalEditors ?? [];
                    item["default_index"] = W9E1IndexOf(list, ExternalEditors.Default(settings));
                    var finds = new JsonObject();
                    foreach (var id in c["find_ids"]?.AsArray() ?? [])
                    {
                        finds[id is null ? "<null>" : id.GetValue<string>()] = W9E1IndexOf(list, ExternalEditors.Find(settings, W9E1Str(id)));
                    }

                    item["find"] = finds;
                    break;
                }
                case "reconcile":
                {
                    var known = new HashSet<string>(StringComparer.Ordinal);
                    var settings = W9E1Settings(c["settings"]!.AsObject(), known);
                    item["repaired"] = ExternalEditors.Reconcile(settings);
                    item["settings"] = W9E1Dump(settings, known);
                    break;
                }
                case "apply_legacy":
                {
                    var known = new HashSet<string>(StringComparer.Ordinal);
                    var settings = W9E1Settings(c["settings"]!.AsObject(), known);
                    ExternalEditors.ApplyLegacyPath(settings, W9E1Str(c["legacy_path"]));
                    item["settings"] = W9E1Dump(settings, known);
                    break;
                }
                case "resolve":
                {
                    var existing = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
                    foreach (var p in c["existing"]?.AsArray() ?? []) existing.Add(p!.GetValue<string>());
                    item["value"] = ExternalEditors.ResolveFileToSend(W9E1Str(c["path"])!, W9E1Str(c["paired_raw"]),
                        c["send_raw"]!.GetValue<bool>(), existing.Contains);
                    break;
                }
                default:
                    throw new InvalidOperationException("unknown kind " + kind);
            }

            result.Add(item);
        }

        return result;
    }
}
