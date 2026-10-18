// W8-02a: ContextMenuModelBuilder.cs + MenuContracts.cs (C-12): the right-click menu model and the Tools menu.
//
// Dispatch line to add to tools/oracle-cs/Program.cs (inside the `command switch`):
//     "menu-model" => Commands.MenuModelCases(inputs),
//
// Copy this file to tools/oracle-cs/Commands/W8-02a.cs and the input to tests/golden-inputs/menu-model.json.
// Output: tests/golden/menu-model.json (run tools/oracle.ps1 -Command menu-model).
//
// Wiring the lead must add (not done by the writer, the oracle is not run here):
//   - tools/oracle.ps1 builds PhotoReview.App.Shared from the baseline archive into $(CSharpCoreDir) (its project references
//     Core, Benchmarking, Imaging, Imaging.Raw, Imaging.LibRaw, Imaging.TurboJpeg and Platform.Windows, all built there already).
//   - tools/oracle-cs/OracleCs.csproj: <Reference Include="PhotoReview.App.Shared"> with HintPath $(CSharpCoreDir)\PhotoReview.App.Shared.dll.
// The `Tr` class is source-generated into App.Shared from en.json, so the English texts come with that build.
// The oracle pins the ambient localizer to English first (as GeneratedTrTests does), so a Vietnamese machine still writes English.

using System.Text.Json.Nodes;
using PhotoReview.App.Menus;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Settings;

static partial class Commands
{
    /// <summary>JSON form of a menu tree: every field of <see cref="MenuItemModel"/>, separators keep an empty text and null nulls.</summary>
    private static JsonArray W802Tree(IReadOnlyList<MenuItemModel> items)
    {
        var array = new JsonArray();
        foreach (var item in items)
        {
            array.Add(new JsonObject
            {
                ["id"] = item.Id,
                ["kind"] = item.Kind.ToString(),
                ["text"] = item.Text,
                ["shortcut"] = item.ShortcutText,
                ["enabled"] = item.IsEnabled,
                ["checked"] = item.IsChecked,
                ["automation"] = item.AutomationName,
                ["children"] = W802Tree(item.Children),
            });
        }
        return array;
    }

    /// <summary>Top-level ids in order, a separator as "|" (the W2-20 render convention).</summary>
    private static string W802Render(IReadOnlyList<MenuItemModel> items) =>
        string.Join(",", items.Select(i => i.Kind == MenuItemKind.Separator ? "|" : i.Id));

    /// <summary>Builds the settings of one case. hidden "default" keeps new AppSettings(); an array applies ApplyHidden; null uses the legacy flags.</summary>
    private static AppSettings W802Settings(JsonObject s)
    {
        var settings = new AppSettings();
        var hidden = s["hidden"];
        if (hidden is JsonArray names)
        {
            ContextMenuItems.ApplyHidden(settings, names.Select(n => Enum.Parse<ContextMenuItemId>(n!.GetValue<string>())).ToList());
        }
        else if (hidden is null && s.ContainsKey("hidden"))
        {
            // JSON null: HiddenContextMenuItems stays null and the legacy flags decide.
            settings.HiddenContextMenuItems = null;
            settings.ShowZoomMenuItems = s["show_zoom"]!.GetValue<bool>();
            settings.ShowFolderMenuItems = s["show_folder"]!.GetValue<bool>();
            settings.ShowRecycleMenuItem = s["show_recycle"]!.GetValue<bool>();
        }
        // else: "default" (or absent) keeps the defaults of new AppSettings().

        settings.ClickZoomPercent = s["click_zoom_percent"]!.GetValue<int>();
        settings.SetZoomAlsoSetsClickLevel = s["set_zoom_also_sets_click_level"]!.GetValue<bool>();
        if (s["shortcuts"] is JsonObject shortcuts)
        {
            foreach (var entry in shortcuts)
            {
                typeof(ShortcutMappings).GetProperty(entry.Key)!.SetValue(settings.Shortcuts, entry.Value!.GetValue<string>());
            }
        }
        return settings;
    }

    /// <summary>
    /// W8-02a. Input (see golden-inputs/menu-model.json):
    ///   "build":          [ { name, settings, has_image, can_copy_path, effective_zoom } ]   Build(context) for each case
    ///   "masks":          [ int ]   hidden subsets of the 12 unlocked top-level items (bit i = i-th item of `top_level_hideable`), Build for has_photo true and false
    ///   "submenu_masks":  [ int ]   hidden subsets of the 5 Zoom children (bit i = i-th item of `zoom_children`), has_photo true and false
    ///   "tools":          [ bool ]  BuildToolsMenu(hasImages)
    /// Output kinds: meta (the bit orders), build, mask, submenu_mask, tools. Every case also writes `tree` (the whole menu, submenus included).
    /// The context follows the test helper of ContextMenuModelBuilderTests: CanUndo true, CanOpenInExternalEditor true, IsCompareVisible false;
    /// IsFit is effective_zoom is null. The mask cases use effective zoom 1.0 and default settings otherwise.
    /// </summary>
    public static JsonArray MenuModelCases(JsonObject inputs)
    {
        // Pin the ambient localizer to English, as the GeneratedTrTests fixture does.
        var previousLocalizer = Localizer.Current;
        Localizer.SetCurrent(BuiltInCatalog.EnglishLocalizer);
        try
        {
            var result = new JsonArray();

            var topHideable = ContextMenuItems.Items.Where(i => i.Parent is null && !i.Locked).Select(i => i.Id).ToList();
            var zoomChildren = ContextMenuItems.Items.Where(i => i.Parent == ContextMenuItemId.ZoomSubmenu).Select(i => i.Id).ToList();
            result.Add(new JsonObject
            {
                ["kind"] = "meta",
                ["top_level_hideable"] = W802Array(topHideable.Select(i => i.ToString())),
                ["zoom_children"] = W802Array(zoomChildren.Select(i => i.ToString())),
            });

            foreach (var c in inputs["build"]!.AsArray())
            {
                var settings = W802Settings(c!["settings"]!.AsObject());
                var zoom = c["effective_zoom"]?.GetValue<double>();
                var context = new ContextMenuBuildContext(settings, c["has_image"]!.GetValue<bool>(), CanUndo: true,
                    IsFit: zoom is null, EffectiveZoom: zoom, CanOpenInExternalEditor: true,
                    CanCopyPath: c["can_copy_path"]!.GetValue<bool>(), IsCompareVisible: false);
                var items = ContextMenuModelBuilder.Build(context);
                result.Add(new JsonObject
                {
                    ["kind"] = "build",
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["render"] = W802Render(items),
                    ["tree"] = W802Tree(items),
                });
            }

            foreach (var maskNode in inputs["masks"]!.AsArray())
            {
                var mask = maskNode!.GetValue<int>();
                var hidden = topHideable.Where((_, i) => (mask & (1 << i)) != 0).ToList();
                foreach (var hasPhoto in new[] { true, false })
                {
                    var settings = new AppSettings();
                    ContextMenuItems.ApplyHidden(settings, hidden);
                    var items = ContextMenuModelBuilder.Build(new ContextMenuBuildContext(settings, hasPhoto, CanUndo: true,
                        IsFit: false, EffectiveZoom: 1.0, CanOpenInExternalEditor: true, CanCopyPath: hasPhoto, IsCompareVisible: false));
                    result.Add(new JsonObject
                    {
                        ["kind"] = "mask",
                        ["mask"] = mask,
                        ["has_photo"] = hasPhoto,
                        ["render"] = W802Render(items),
                        ["tree"] = W802Tree(items),
                    });
                }
            }

            foreach (var maskNode in inputs["submenu_masks"]!.AsArray())
            {
                var mask = maskNode!.GetValue<int>();
                var hidden = zoomChildren.Where((_, i) => (mask & (1 << i)) != 0).ToList();
                foreach (var hasPhoto in new[] { true, false })
                {
                    var settings = new AppSettings();
                    ContextMenuItems.ApplyHidden(settings, hidden);
                    var items = ContextMenuModelBuilder.Build(new ContextMenuBuildContext(settings, hasPhoto, CanUndo: true,
                        IsFit: false, EffectiveZoom: 1.0, CanOpenInExternalEditor: true, CanCopyPath: hasPhoto, IsCompareVisible: false));
                    result.Add(new JsonObject
                    {
                        ["kind"] = "submenu_mask",
                        ["mask"] = mask,
                        ["has_photo"] = hasPhoto,
                        ["render"] = W802Render(items),
                        ["tree"] = W802Tree(items),
                    });
                }
            }

            foreach (var hasImages in inputs["tools"]!.AsArray())
            {
                var items = ContextMenuModelBuilder.BuildToolsMenu(hasImages!.GetValue<bool>());
                result.Add(new JsonObject
                {
                    ["kind"] = "tools",
                    ["has_images"] = hasImages.GetValue<bool>(),
                    ["tree"] = W802Tree(items),
                });
            }

            return result;
        }
        finally
        {
            Localizer.SetCurrent(previousLocalizer);
        }
    }

    private static JsonArray W802Array(IEnumerable<string> items) => new(items.Select(i => (JsonNode?)i).ToArray());
}
