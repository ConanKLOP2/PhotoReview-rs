//! Port of `PhotoReview.App.Shared/Menus/ContextMenuModelBuilder.cs` (C-12, 136 lines): builds the right-click menu model
//! and the Tools menu. Visibility and separators come from `pr_core::settings::context_menu::compute` (already ported,
//! W2-20), so the rule is not copied here. Zoom presets and the set-current rule come from `menus::zoom_rules` (W8-01d).
//!
//! Unit W8-02a. Contract: `contracts/W8-02a.md`. Texts come from `pr_l10n::tr` only (D-W8-4); no English literal in
//! production code. Stub: signatures are final, bodies are `todo!()`; the porter removes the `allow` line with the last `todo!()`.

#![allow(unused_variables, dead_code, unused_imports)] // D11: removed by the porter with the last todo!()

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
    todo!("W8-02a porter: see contracts/W8-02a.md, Semantics 1-9")
}

/// `ContextMenuModelBuilder.BuildToolsMenu(hasImages)`: Recovery, Diagnostics, Benchmark, Clear cache, Remove numbered
/// duplicates, Remove original duplicates (this order). The two duplicate removals are disabled without images.
pub fn build_tools_menu(has_images: bool) -> Vec<MenuItemModel> {
    todo!("W8-02a porter: see contracts/W8-02a.md, Semantics 10")
}

/// `TopLevelItem(id, context, hidden, menuContext)`: one top-level row. The Zoom submenu row carries its children from
/// [`zoom_children`]. Any other id is `Err(NotTopLevel)`.
fn top_level_item(
    id: ContextMenuItemId,
    context: &ContextMenuBuildContext<'_>,
    hidden: &HashSet<ContextMenuItemId>,
    menu_context: ContextMenuContext,
) -> Result<MenuItemModel, MenuBuildError> {
    todo!("W8-02a porter: table of Semantics 3")
}

/// `ZoomChildren(context, hidden, menuContext)`: the children of the Zoom submenu, from `compute(..., Some(ZoomSubmenu))`.
/// Any id without a child row is `Err(NotZoomChild)`.
fn zoom_children(
    context: &ContextMenuBuildContext<'_>,
    hidden: &HashSet<ContextMenuItemId>,
    menu_context: ContextMenuContext,
) -> Result<Vec<MenuItemModel>, MenuBuildError> {
    todo!("W8-02a porter: table of Semantics 4")
}

/// `Command(id, text, shortcut, automationName, isEnabled)`: a Command with the given text. The shortcut is `None` when
/// it is absent, empty or whitespace only (`string.IsNullOrWhiteSpace`); a non-empty shortcut is kept byte for byte.
fn command(id: &str, text: String, shortcut: Option<&str>, automation_name: String, is_enabled: bool) -> MenuItemModel {
    todo!("W8-02a porter: Semantics 9")
}

/// `Separator(id)`: kind Separator, empty text, disabled, no shortcut, no automation name, no children.
fn separator(id: String) -> MenuItemModel {
    todo!("W8-02a porter: Semantics 2")
}
