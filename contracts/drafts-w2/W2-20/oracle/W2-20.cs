// W2-20: context-menu  (ContextMenuLayout.cs: ContextMenuItemId, ContextMenuItems, ContextMenuLayout)
//
// Dispatch line to add to tools/oracle-cs/Program.cs (inside the `command switch`):
//     "context-menu" => Commands.ContextMenuCases(inputs),
//
// Copy this file to tools/oracle-cs/Commands/W2-20.cs and the input to tests/golden-inputs/context-menu.json.
// Output: tests/golden/context-menu.json (run tools/oracle.ps1 -Command context-menu).

using System.Text.Json.Nodes;
using PhotoReview.Core.Settings;

static partial class Commands
{
    private static JsonArray W220Array(IEnumerable<string> items) => new(items.Select(i => (JsonNode?)i).ToArray());

    private static string W220Render(ContextMenuLayout layout) =>
        string.Join(",", layout.Entries.Select(e => e.Item?.ToString() ?? "|"));

    private static JsonArray W220Entries(ContextMenuLayout layout) =>
        new(layout.Entries.Select(e => (JsonNode?)new JsonArray((JsonNode?)(e.Item?.ToString()), e.Group)).ToArray());


    /// <summary>
    /// W2-20. Input:
    ///   "parse_hidden":   [ [ string | null, ... ] ]                    ToStored(ParseHidden(names)) for each list
    ///   "single_names":   [ string ]                                    ToStored(ParseHidden([name])) for each: pins the Enum.TryParse quirks
    ///   "settings_flags": [ { "zoom": bool, "folder": bool, "recycle": bool } ]   LegacyHidden, and EffectiveHidden with a null list
    ///   "effective":      [ { "list": [string|null] | null, "zoom", "folder", "recycle" } ]
    ///   "to_stored":      [ [ id name, ... ] ]                           ToStored(ids)
    ///   "apply_hidden":   [ [ id name, ... ] ]                           ApplyHidden(new AppSettings(), ids)
    ///   "masks":          [ int ]                                        hidden subsets of the 12 unlocked top-level items (bit i = i-th in `top_level_hideable`)
    ///   "submenu_masks":  [ int ]                                        hidden subsets of the 5 Zoom submenu children
    /// Output kinds: items, parse_hidden, single_name, legacy, effective, to_stored, apply_hidden, compute (top level, both contexts), compute_submenu.
    /// </summary>
    public static JsonArray ContextMenuCases(JsonObject inputs)
    {
        var result = new JsonArray();

        // ---- the table ----
        var table = new JsonArray();
        foreach (var i in ContextMenuItems.Items)
            table.Add(new JsonObject
            {
                ["id"] = i.Id.ToString(),
                ["value"] = (int)i.Id,
                ["parent"] = i.Parent?.ToString(),
                ["group"] = i.Group,
                ["locked"] = i.Locked,
                ["needs_photo"] = i.NeedsPhoto,
            });
        result.Add(new JsonObject
        {
            ["kind"] = "items",
            ["items"] = table,
            ["names"] = W220Array(Enum.GetNames<ContextMenuItemId>()),
            ["values"] = new JsonArray(Enum.GetValues<ContextMenuItemId>().Select(v => (JsonNode?)(int)v).ToArray()),
            ["default_hidden"] = W220Array(ContextMenuItems.ToStored(ContextMenuItems.DefaultHidden)),
        });

        foreach (var list in inputs["parse_hidden"]!.AsArray())
        {
            var names = list!.AsArray().Select(n => n?.GetValue<string>()).ToList();
            result.Add(new JsonObject
            {
                ["kind"] = "parse_hidden",
                ["names"] = new JsonArray(names.Select(n => (JsonNode?)n).ToArray()),
                ["stored"] = W220Array(ContextMenuItems.ToStored(ContextMenuItems.ParseHidden(names))),
            });
        }

        foreach (var node in inputs["single_names"]!.AsArray())
        {
            var name = node!.GetValue<string>();
            result.Add(new JsonObject
            {
                ["kind"] = "single_name",
                ["name"] = name,
                ["stored"] = W220Array(ContextMenuItems.ToStored(ContextMenuItems.ParseHidden([name]))),
            });
        }

        foreach (var f in inputs["settings_flags"]!.AsArray())
        {
            var s = new AppSettings { ShowZoomMenuItems = f!["zoom"]!.GetValue<bool>(), ShowFolderMenuItems = f["folder"]!.GetValue<bool>(), ShowRecycleMenuItem = f["recycle"]!.GetValue<bool>(), HiddenContextMenuItems = null };
            result.Add(new JsonObject
            {
                ["kind"] = "legacy",
                ["zoom"] = s.ShowZoomMenuItems,
                ["folder"] = s.ShowFolderMenuItems,
                ["recycle"] = s.ShowRecycleMenuItem,
                ["hidden"] = W220Array(ContextMenuItems.ToStored(ContextMenuItems.LegacyHidden(s))),
                ["effective_with_null_list"] = W220Array(ContextMenuItems.ToStored(ContextMenuItems.EffectiveHidden(s))),
            });
        }

        foreach (var e in inputs["effective"]!.AsArray())
        {
            var s = new AppSettings { ShowZoomMenuItems = e!["zoom"]!.GetValue<bool>(), ShowFolderMenuItems = e["folder"]!.GetValue<bool>(), ShowRecycleMenuItem = e["recycle"]!.GetValue<bool>() };
            s.HiddenContextMenuItems = e["list"] is JsonArray arr ? arr.Select(n => n?.GetValue<string>()!).ToList() : null; // a null element is allowed in the stored list (string?, nullability not tracked)
            result.Add(new JsonObject
            {
                ["kind"] = "effective",
                ["input"] = e.DeepClone(),
                ["hidden"] = W220Array(ContextMenuItems.ToStored(ContextMenuItems.EffectiveHidden(s))),
            });
        }

        foreach (var list in inputs["to_stored"]!.AsArray())
        {
            var ids = list!.AsArray().Select(n => Enum.Parse<ContextMenuItemId>(n!.GetValue<string>())).ToList();
            result.Add(new JsonObject
            {
                ["kind"] = "to_stored",
                ["ids"] = W220Array(ids.Select(i => i.ToString())),
                ["stored"] = W220Array(ContextMenuItems.ToStored(ids)),
            });
        }

        foreach (var list in inputs["apply_hidden"]!.AsArray())
        {
            var ids = list!.AsArray().Select(n => Enum.Parse<ContextMenuItemId>(n!.GetValue<string>())).ToList();
            var s = new AppSettings();
            ContextMenuItems.ApplyHidden(s, ids);
            result.Add(new JsonObject
            {
                ["kind"] = "apply_hidden",
                ["ids"] = W220Array(ids.Select(i => i.ToString())),
                ["stored"] = W220Array(s.HiddenContextMenuItems!),
                ["show_zoom"] = s.ShowZoomMenuItems,
                ["show_folder"] = s.ShowFolderMenuItems,
                ["show_recycle"] = s.ShowRecycleMenuItem,
            });
        }

        var topHideable = ContextMenuItems.Items.Where(i => i.Parent is null && !i.Locked).Select(i => i.Id).ToList();
        result.Add(new JsonObject { ["kind"] = "top_level_hideable", ["ids"] = W220Array(topHideable.Select(i => i.ToString())) });
        foreach (var maskNode in inputs["masks"]!.AsArray())
        {
            var mask = maskNode!.GetValue<int>();
            var hidden = topHideable.Where((_, i) => (mask & (1 << i)) != 0).ToHashSet();
            foreach (var hasPhoto in new[] { true, false })
            {
                var layout = ContextMenuItems.Compute(hidden, new ContextMenuContext(hasPhoto));
                result.Add(new JsonObject
                {
                    ["kind"] = "compute",
                    ["mask"] = mask,
                    ["has_photo"] = hasPhoto,
                    ["render"] = W220Render(layout),
                    ["entries"] = W220Entries(layout),
                });
            }
        }

        var children = ContextMenuItems.Items.Where(i => i.Parent == ContextMenuItemId.ZoomSubmenu).Select(i => i.Id).ToList();
        result.Add(new JsonObject { ["kind"] = "submenu_children", ["ids"] = W220Array(children.Select(i => i.ToString())) });
        foreach (var maskNode in inputs["submenu_masks"]!.AsArray())
        {
            var mask = maskNode!.GetValue<int>();
            var hidden = children.Where((_, i) => (mask & (1 << i)) != 0).ToHashSet();
            var layout = ContextMenuItems.Compute(hidden, new ContextMenuContext(true), ContextMenuItemId.ZoomSubmenu);
            // Also: does the parent show at the top level with only this subset hidden?
            var top = ContextMenuItems.Compute(hidden, new ContextMenuContext(true));
            result.Add(new JsonObject
            {
                ["kind"] = "compute_submenu",
                ["mask"] = mask,
                ["render"] = W220Render(layout),
                ["entries"] = W220Entries(layout),
                ["submenu_visible_on_top_level"] = top.IsVisible(ContextMenuItemId.ZoomSubmenu),
            });
        }

        // Hidden set containing a child id at the top level (ids of the other level are ignored) and Settings (never hidden).
        foreach (var (label, ids) in new (string, ContextMenuItemId[])[]
        {
            ("settings listed", [ContextMenuItemId.Settings]),
            ("child ids at the top level are ignored", [ContextMenuItemId.ZoomFitWidth, ContextMenuItemId.ZoomPresets]),
            ("top-level ids at the submenu level are ignored", [ContextMenuItemId.Fit, ContextMenuItemId.Undo]),
        })
        {
            var set = ids.ToHashSet();
            result.Add(new JsonObject
            {
                ["kind"] = "compute_misc",
                ["label"] = label,
                ["hidden"] = W220Array(ids.Select(i => i.ToString())),
                ["top_photo"] = W220Render(ContextMenuItems.Compute(set, new ContextMenuContext(true))),
                ["submenu"] = W220Render(ContextMenuItems.Compute(set, new ContextMenuContext(true), ContextMenuItemId.ZoomSubmenu)),
                ["settings_visible"] = ContextMenuItems.Compute(set, new ContextMenuContext(false)).IsVisible(ContextMenuItemId.Settings),
            });
        }

        // IsVisible / HasSeparatorBefore on the default layout.
        var def = ContextMenuItems.Compute(new HashSet<ContextMenuItemId>(), new ContextMenuContext(true));
        result.Add(new JsonObject
        {
            ["kind"] = "layout_queries",
            ["visible"] = W220Array(Enum.GetValues<ContextMenuItemId>().Where(def.IsVisible).Select(i => i.ToString())),
            ["separator_before"] = new JsonArray(Enumerable.Range(0, 8).Where(def.HasSeparatorBefore).Select(g => (JsonNode?)g).ToArray()),
        });
        return result;
    }
}
