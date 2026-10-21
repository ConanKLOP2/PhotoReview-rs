//! Port of `PhotoReview.Shell.Win32/Menus/NativeMenuMapping.cs` (35 lines, baseline `d92d6aae`). Unit W8-14e. Contract: `contracts/W8-14e.md`.
//!
//! The pure half of the native popup host: the display text of an item, its `MF_*` flags, and the command id that `TrackPopupMenuEx` returns back
//! to a model id.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::menus::model::MenuItemModel;

/// C# `DisplayText(item)`: `&` doubled (so it is not a mnemonic), and the shortcut text after a TAB (shown right-aligned like an accelerator), its `&`
/// doubled as well. An absent or empty shortcut gives just the text.
pub fn display_text(item: &MenuItemModel) -> String {
    todo!()
}

/// C# `ItemFlags(item)`: `MF_STRING | (MF_GRAYED if !is_enabled) | (MF_CHECKED if is_checked)`.
pub fn item_flags(item: &MenuItemModel) -> u32 {
    todo!()
}

/// C# `IdForCommand(ids, command)`: command ids are 1-based positions in `ids` (`0` = cancelled); `None` when `command` is outside `1..=ids.len()`.
pub fn id_for_command(ids: &[String], command: i32) -> Option<&str> {
    todo!()
}
