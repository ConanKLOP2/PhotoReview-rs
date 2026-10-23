//! W9-E6: the "Open in External Editor" menu entry of `ContextMenuModelBuilder.ExternalEditorItem` (baseline `7eaee2a2`, `ContextMenuModelBuilder.cs` lines 84-122).
//! Contract: `contracts/W9-E6.md`. With no editor the entry stays the single command it was; with one or more it is a submenu. The builder arm
//! `ContextMenuItemId::ExternalEditor` in `menus::builder` calls [`external_editor_item`] (the porter wires it; the stub does not, so the merged behaviour and
//! tests are unchanged until then). Every body is `todo!()`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use crate::menus::model::{ContextMenuBuildContext, MenuItemModel};

/// C# `ExternalEditorOpenPrefix`: id of one editor entry is `"ExternalEditor.Open." + editor.id`. The shell controller parses the same text
/// (`pr_platform_win::shell_menus::controller::EXTERNAL_EDITOR_OPEN_PREFIX`, W9-E9; a test compares the two).
pub const EXTERNAL_EDITOR_OPEN_PREFIX: &str = "ExternalEditor.Open.";
/// C# `ExternalEditorAddId`: the "Add more editors..." entry.
pub const EXTERNAL_EDITOR_ADD_ID: &str = "ExternalEditor.Add";
/// C# `ExternalEditorSetDefaultId`: the "Set default editor..." entry.
pub const EXTERNAL_EDITOR_SET_DEFAULT_ID: &str = "ExternalEditor.SetDefault";
/// The separator before the two fixed entries (`Separator("Separator.ExternalEditor")`).
pub const EXTERNAL_EDITOR_SEPARATOR_ID: &str = "Separator.ExternalEditor";

/// C# `ExternalEditorItem(context)`.
///
/// `editors = ExternalEditors::effective(settings)`. Empty: `Command("ExternalEditor", main.menu.openInExternalEditor, None, main.menu.openInExternalEditor.automationName)`
/// (enabled, as the merged arm). Otherwise a `MenuItemKind::Submenu` with id `"ExternalEditor"`, text `main.menu.openInExternalEditor`, no shortcut, enabled, not
/// checked, automation name `main.menu.openInExternalEditor.automationName`, and the children in this order: one `MenuItemKind::Toggle` per editor, in list order
/// (id `EXTERNAL_EDITOR_OPEN_PREFIX + editor.id`, text `editor.display_name()`, shortcut = `settings.shortcuts.open_in_default_editor` ONLY on the default
/// editor and only when it is not blank, else `None`; enabled = `context.has_image`; checked = it is the default editor (`effective_default`, an unknown id = the
/// first); automation name `main.menu.externalEditor.item.automationName {name}`); the separator `EXTERNAL_EDITOR_SEPARATOR_ID`; the command
/// `EXTERNAL_EDITOR_ADD_ID` (`main.menu.externalEditor.add`, no shortcut, automation `main.menu.externalEditor.add.automationName`, always enabled); the command
/// `EXTERNAL_EDITOR_SET_DEFAULT_ID` (`main.menu.externalEditor.setDefault`, `.automationName`, always enabled).
pub fn external_editor_item(context: &ContextMenuBuildContext<'_>) -> MenuItemModel {
    todo!()
}
