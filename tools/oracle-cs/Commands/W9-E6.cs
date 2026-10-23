// W9-E6: external-editor-menu  (App.Shared/Menus/ContextMenuModelBuilder.cs, ExternalEditorItem, baseline 7eaee2a2)
//
// Dispatch line in tools/oracle-cs/Program.cs (inside the `command switch`, W9 block):
//     "external-editor-menu" => Commands.W9E6ExternalEditorMenuCases(inputs),
//
// Inputs: tests/golden-inputs/external-editor-menu.json  { "cases": [ { name, editors, default_id, legacy_path, shortcut, hidden, has_image } ] }
//   editors      [ { id, name, path } ]  (the list is set as given: NO Reconcile, so an unknown default id or a blank name reaches the builder)
//   default_id   string|null            AppSettings.DefaultExternalEditorId
//   legacy_path  string|null            AppSettings.ExternalEditorPath (the list stays empty = the implicit legacy editor)
//   shortcut     string|null            Shortcuts.OpenInDefaultEditor (null = the default "")
//   hidden       [ ContextMenuItemId name ]  ContextMenuItems.ApplyHidden (absent = the defaults)
//   has_image    bool                   ContextMenuBuildContext.HasImage (CanCopyPath follows it; CanUndo true, IsFit true, no zoom, no compare)
// Output: tests/golden/external-editor-menu.json, per case: render (top-level ids, a separator as "|"), item (the whole "ExternalEditor" entry as
// { id, kind, text, shortcut, enabled, checked, automation, children[] } or null when the layout hides it). The English texts are pinned like W8-02a.

using System.Text.Json.Nodes;
using PhotoReview.App.Menus;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Settings;

static partial class Commands
{
    public static JsonArray W9E6ExternalEditorMenuCases(JsonObject inputs)
    {
        var previousLocalizer = Localizer.Current;
        Localizer.SetCurrent(BuiltInCatalog.EnglishLocalizer);
        try
        {
            var result = new JsonArray();
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var settings = new AppSettings();
                if (c["editors"] is JsonArray editors)
                {
                    foreach (var e in editors)
                    {
                        var o = e!.AsObject();
                        var editor = new ExternalEditor { Id = o["id"]!.GetValue<string>() };
                        if (o.ContainsKey("name")) editor.Name = o["name"]!.GetValue<string>();
                        if (o.ContainsKey("path")) editor.Path = o["path"]!.GetValue<string>();
                        settings.ExternalEditors.Add(editor);
                    }
                }

                if (c["default_id"] is { } defaultId) settings.DefaultExternalEditorId = defaultId.GetValue<string>();
                if (c["legacy_path"] is { } legacy) settings.ExternalEditorPath = legacy.GetValue<string>();
                if (c["shortcut"] is { } shortcut) settings.Shortcuts.OpenInDefaultEditor = shortcut.GetValue<string>();
                if (c["hidden"] is JsonArray hidden)
                {
                    ContextMenuItems.ApplyHidden(settings, hidden.Select(n => Enum.Parse<ContextMenuItemId>(n!.GetValue<string>())).ToList());
                }

                var hasImage = c["has_image"]!.GetValue<bool>();
                var context = new ContextMenuBuildContext(settings, hasImage, CanUndo: true, IsFit: true, EffectiveZoom: null,
                    CanOpenInExternalEditor: true, CanCopyPath: hasImage, IsCompareVisible: false);
                var items = ContextMenuModelBuilder.Build(context);
                var entry = items.FirstOrDefault(i => i.Id == nameof(ContextMenuItemId.ExternalEditor));
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["render"] = W802Render(items),
                    ["item"] = entry is null ? null : W802Tree([entry])[0]!.DeepClone(),
                });
            }

            return result;
        }
        finally
        {
            Localizer.SetCurrent(previousLocalizer);
        }
    }
}
