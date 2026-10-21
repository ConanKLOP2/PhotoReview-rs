//! Port of `PhotoReview.Shell.Win32/Menus/NativeMenuMapping.cs` (35 lines, baseline `d92d6aae`). Unit W8-14e. Contract: `contracts/W8-14e.md`.
//!
//! The pure half of the native popup host: the display text of an item, its `MF_*` flags, and the command id that `TrackPopupMenuEx` returns back
//! to a model id.
use pr_app::menus::model::MenuItemModel;

use crate::win32::messages::{MF_CHECKED, MF_GRAYED, MF_STRING};

/// C# `DisplayText(item)`: `&` doubled (so it is not a mnemonic), and the shortcut text after a TAB (shown right-aligned like an accelerator), its `&`
/// doubled as well. An absent or empty shortcut gives just the text.
pub fn display_text(item: &MenuItemModel) -> String {
    let text = item.text.replace('&', "&&");
    match item.shortcut_text.as_deref() {
        Some(shortcut) if !shortcut.is_empty() => format!("{text}\t{}", shortcut.replace('&', "&&")),
        _ => text,
    }
}

/// C# `ItemFlags(item)`: `MF_STRING | (MF_GRAYED if !is_enabled) | (MF_CHECKED if is_checked)`.
pub fn item_flags(item: &MenuItemModel) -> u32 {
    let mut flags = MF_STRING;
    if !item.is_enabled {
        flags |= MF_GRAYED;
    }
    if item.is_checked {
        flags |= MF_CHECKED;
    }
    flags
}

/// C# `IdForCommand(ids, command)`: command ids are 1-based positions in `ids` (`0` = cancelled); `None` when `command` is outside `1..=ids.len()`.
pub fn id_for_command(ids: &[String], command: i32) -> Option<&str> {
    let index = usize::try_from(command).ok()?.checked_sub(1)?;
    ids.get(index).map(String::as_str)
}
