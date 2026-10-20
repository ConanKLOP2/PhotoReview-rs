//! Port of `PhotoReview.App.Shared/Menus/ContextMenuModelBuilder.cs` (C-12, 136 lines): builds the right-click menu model
//! and the Tools menu. Visibility and separators come from `pr_core::settings::context_menu::compute` (already ported,
//! W2-20), so the rule is not copied here. Zoom presets and the set-current rule come from `menus::zoom_rules` (W8-01d).
//!
//! Unit W8-02a. Contract: `contracts/W8-02a.md`. Texts come from `pr_l10n::tr` only (D-W8-4); no English literal in
//! production code. Stub: signatures are final, bodies are `todo!()`; the porter removes the `allow` line with the last `todo!()`.

use std::collections::HashSet;
use std::fmt;

use pr_core::settings::context_menu::{self, ContextMenuContext, ContextMenuItemId};

use crate::menus::model::{ContextMenuBuildContext, MenuItemKind, MenuItemModel};
use crate::menus::zoom_rules;

/// Errors of the builder. C# throws `InvalidOperationException` for the two cases; `compute` never returns them for the
/// level it is asked for (checked by the in-module test `top_level_item_rejects_a_submenu_child`), so a caller maps them
/// to a failed menu, never to a panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuBuildError {
    /// An id with no top-level row (`"Context menu item {id} is not a top-level item."`).
    NotTopLevel(ContextMenuItemId),
    /// An id with no Zoom submenu row (`"Context menu item {id} is not a Zoom submenu item."`).
    NotZoomChild(ContextMenuItemId),
}

impl fmt::Display for MenuBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotTopLevel(id) => write!(f, "Context menu item {} is not a top-level item.", id.name()),
            Self::NotZoomChild(id) => write!(f, "Context menu item {} is not a Zoom submenu item.", id.name()),
        }
    }
}

impl std::error::Error for MenuBuildError {}

/// `ContextMenuModelBuilder.Build(context)`: the right-click menu, top level, in `compute` order, with the Zoom submenu
/// nested. Hidden set = `context_menu::effective_hidden(settings)`; Copy items need `has_image && can_copy_path`.
///
/// Errors: only the two [`MenuBuildError`] cases, which `compute` cannot produce for its own level.
pub fn build(context: &ContextMenuBuildContext<'_>) -> Result<Vec<MenuItemModel>, MenuBuildError> {
    let hidden = context_menu::effective_hidden(context.settings);
    let menu_context = ContextMenuContext {
        has_photo: context.has_image && context.can_copy_path,
    };
    context_menu::compute(&hidden, menu_context, None)
        .entries
        .into_iter()
        .map(|entry| match entry.item {
            Some(id) => top_level_item(id, context, &hidden, menu_context),
            None => Ok(separator(format!("Separator.{}", entry.group))),
        })
        .collect()
}

/// `ContextMenuModelBuilder.BuildToolsMenu(hasImages)`: Recovery, Diagnostics, Benchmark, Clear cache, Remove numbered
/// duplicates, Remove original duplicates (this order). The two duplicate removals are disabled without images.
pub fn build_tools_menu(has_images: bool) -> Vec<MenuItemModel> {
    use pr_l10n::tr;
    vec![
        command(
            "Tools.Recovery",
            tr::main_tools_recovery(),
            None,
            tr::main_tools_recovery_automation_name(),
            true,
        ),
        command(
            "Tools.Diagnostics",
            tr::main_tools_diagnostics(),
            None,
            tr::main_tools_diagnostics_automation_name(),
            true,
        ),
        command(
            "Tools.Benchmark",
            tr::main_tools_benchmark(),
            None,
            tr::main_tools_benchmark_automation_name(),
            true,
        ),
        command(
            "Tools.ClearCache",
            tr::main_tools_clear_cache(),
            None,
            tr::main_tools_clear_cache_automation_name(),
            true,
        ),
        command(
            "Tools.RemoveNumberedDuplicates",
            tr::main_tools_remove_numbered_duplicates(),
            None,
            tr::main_tools_remove_numbered_duplicates_automation_name(),
            has_images,
        ),
        command(
            "Tools.RemoveOriginalDuplicates",
            tr::main_tools_remove_original_duplicates(),
            None,
            tr::main_tools_remove_original_duplicates_automation_name(),
            has_images,
        ),
    ]
}

/// `TopLevelItem(id, context, hidden, menuContext)`: one top-level row. The Zoom submenu row carries its children from
/// [`zoom_children`]. Any other id is `Err(NotTopLevel)`.
fn top_level_item(
    id: ContextMenuItemId,
    context: &ContextMenuBuildContext<'_>,
    hidden: &HashSet<ContextMenuItemId>,
    menu_context: ContextMenuContext,
) -> Result<MenuItemModel, MenuBuildError> {
    use pr_l10n::tr;
    let settings = context.settings;
    let shortcuts = &settings.shortcuts;
    let m = match id {
        ContextMenuItemId::Undo => command(
            id.name(),
            tr::main_menu_undo(),
            None,
            tr::main_menu_undo_automation_name(),
            true,
        ),
        ContextMenuItemId::MoveToRecycleBin => command(
            id.name(),
            tr::main_menu_recycle(),
            Some(&shortcuts.send_to_recycle_bin),
            tr::main_menu_recycle_automation_name(),
            true,
        ),
        ContextMenuItemId::Fit => command(
            id.name(),
            tr::main_menu_click_zoom_level_fit(),
            Some(&shortcuts.toggle_fit),
            tr::main_menu_click_zoom_level_fit_automation_name(),
            true,
        ),
        ContextMenuItemId::ZoomToLevel => command(
            id.name(),
            tr::main_menu_zoom_to_level(settings.click_zoom_percent),
            Some(&shortcuts.click_zoom),
            tr::main_menu_zoom_to_level_automation_name(settings.click_zoom_percent),
            true,
        ),
        ContextMenuItemId::ZoomSubmenu => MenuItemModel {
            id: id.name().to_owned(),
            kind: MenuItemKind::Submenu,
            text: tr::main_menu_zoom(),
            shortcut_text: None,
            is_enabled: true,
            is_checked: false,
            children: zoom_children(context, hidden, menu_context)?,
            automation_name: Some(tr::main_menu_zoom_automation_name()),
        },
        ContextMenuItemId::Refresh => command(
            id.name(),
            tr::main_menu_refresh(),
            Some(&shortcuts.refresh),
            tr::main_menu_refresh_automation_name(),
            true,
        ),
        ContextMenuItemId::OpenFolder => command(
            id.name(),
            tr::main_menu_open_folder(),
            None,
            tr::main_menu_open_folder_automation_name(),
            true,
        ),
        ContextMenuItemId::NextFolder => command(
            id.name(),
            tr::main_menu_next_folder(),
            Some(&shortcuts.next_folder),
            tr::main_menu_next_folder_automation_name(),
            true,
        ),
        ContextMenuItemId::PreviousFolder => command(
            id.name(),
            tr::main_menu_previous_folder(),
            Some(&shortcuts.previous_folder),
            tr::main_menu_previous_folder_automation_name(),
            true,
        ),
        ContextMenuItemId::ExternalEditor => command(
            id.name(),
            tr::main_menu_open_in_external_editor(),
            None,
            tr::main_menu_open_in_external_editor_automation_name(),
            true,
        ),
        ContextMenuItemId::CopyFileName => command(
            id.name(),
            tr::main_menu_copy_file_name(),
            None,
            tr::main_menu_copy_file_name_automation_name(),
            true,
        ),
        ContextMenuItemId::CopyFullPath => command(
            id.name(),
            tr::main_menu_copy_full_path(),
            None,
            tr::main_menu_copy_full_path_automation_name(),
            true,
        ),
        ContextMenuItemId::Settings => command(
            id.name(),
            tr::main_menu_settings(),
            None,
            tr::main_menu_settings_automation_name(),
            true,
        ),
        _ => return Err(MenuBuildError::NotTopLevel(id)),
    };
    Ok(m)
}

/// `ZoomChildren(context, hidden, menuContext)`: the children of the Zoom submenu, from `compute(..., Some(ZoomSubmenu))`.
/// Any id without a child row is `Err(NotZoomChild)`.
fn zoom_children(
    context: &ContextMenuBuildContext<'_>,
    hidden: &HashSet<ContextMenuItemId>,
    menu_context: ContextMenuContext,
) -> Result<Vec<MenuItemModel>, MenuBuildError> {
    use pr_l10n::tr;
    let mut children = Vec::new();
    for entry in context_menu::compute(hidden, menu_context, Some(ContextMenuItemId::ZoomSubmenu)).entries {
        let Some(id) = entry.item else {
            children.push(separator(format!("Separator.Zoom.{}", entry.group)));
            continue;
        };
        let shortcuts = &context.settings.shortcuts;
        match id {
            ContextMenuItemId::ZoomFitWidth => children.push(command(
                id.name(),
                tr::main_menu_zoom_fit_width(),
                Some(&shortcuts.fit_width),
                tr::main_menu_zoom_fit_width_automation_name(),
                true,
            )),
            ContextMenuItemId::ZoomFitWidth2 => children.push(command(
                id.name(),
                tr::main_menu_zoom_fit_width2(),
                Some(&shortcuts.fit_width2),
                tr::main_menu_zoom_fit_width2_automation_name(),
                true,
            )),
            ContextMenuItemId::ZoomFitHeight => children.push(command(
                id.name(),
                tr::main_menu_zoom_fit_height(),
                Some(&shortcuts.fit_height),
                tr::main_menu_zoom_fit_height_automation_name(),
                true,
            )),
            ContextMenuItemId::ZoomPresets => {
                for &p in zoom_rules::PRESET_PERCENTS {
                    children.push(MenuItemModel {
                        id: format!("Zoom.Preset.{p}"),
                        kind: MenuItemKind::Toggle,
                        text: tr::main_menu_click_zoom_level_preset(p),
                        shortcut_text: None,
                        is_enabled: true,
                        is_checked: p == context.settings.click_zoom_percent,
                        children: Vec::new(),
                        automation_name: Some(tr::main_menu_click_zoom_level_preset_automation_name(p)),
                    });
                }
                children.push(command(
                    "Zoom.Custom",
                    tr::main_menu_click_zoom_level_custom(),
                    None,
                    tr::main_menu_click_zoom_level_custom_automation_name(),
                    true,
                ));
            }
            ContextMenuItemId::ZoomLevelOptions => {
                children.push(MenuItemModel {
                    id: "Zoom.AlsoSetClickLevel".into(),
                    kind: MenuItemKind::Toggle,
                    text: tr::main_menu_zoom_also_set_click_level(),
                    shortcut_text: None,
                    is_enabled: true,
                    is_checked: context.settings.set_zoom_also_sets_click_level,
                    children: Vec::new(),
                    automation_name: Some(tr::main_menu_zoom_also_set_click_level_automation_name()),
                });
                children.push(command(
                    "Zoom.SetCurrentAsClickLevel",
                    tr::main_menu_zoom_set_current_as_click_level(),
                    None,
                    tr::main_menu_zoom_set_current_as_click_level_automation_name(),
                    zoom_rules::calculate_click_level_from_effective_zoom(context.effective_zoom).is_some(),
                ));
            }
            _ => return Err(MenuBuildError::NotZoomChild(id)),
        }
    }
    Ok(children)
}

/// `Command(id, text, shortcut, automationName, isEnabled)`: a Command with the given text. The shortcut is `None` when
/// it is absent, empty or whitespace only (`string.IsNullOrWhiteSpace`); a non-empty shortcut is kept byte for byte.
fn command(id: &str, text: String, shortcut: Option<&str>, automation_name: String, is_enabled: bool) -> MenuItemModel {
    let shortcut_text = shortcut
        .filter(|s| {
            !s.chars()
                .all(|c| c.is_whitespace() && !matches!(c, '\u{001c}'..='\u{001f}'))
        })
        .map(str::to_owned);
    MenuItemModel {
        id: id.to_owned(),
        kind: MenuItemKind::Command,
        text,
        shortcut_text,
        is_enabled,
        is_checked: false,
        children: Vec::new(),
        automation_name: Some(automation_name),
    }
}

/// `Separator(id)`: kind Separator, empty text, disabled, no shortcut, no automation name, no children.
fn separator(id: String) -> MenuItemModel {
    MenuItemModel {
        id,
        kind: MenuItemKind::Separator,
        text: String::new(),
        shortcut_text: None,
        is_enabled: false,
        is_checked: false,
        children: Vec::new(),
        automation_name: None,
    }
}
