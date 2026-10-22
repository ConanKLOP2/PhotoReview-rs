//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/ShortcutsSettingsPage.cs` (323 lines, baseline `d92d6aae`): the "Shortcuts" page (WPF `ShortcutsPage`,
//! 27 key capture boxes): one read-only key box per command (the capture itself is the FRAME's key box subclass, W8-19j/19k), a clear button for the 14 optional
//! keys, "restore default keys", the live duplicate-key warning, the "warning sign" mark on the label of a bad box, and the Action profiles button (nested modal).
//! Unit W8-19i. Contract: `contracts/W8-19i.md`.
//!
//! Properties owned: the 27 shortcut texts (`SettingsFormState::shortcut_texts`) and, through the shared JSON, the actions.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_l10n::tr;

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::builder::layout::SettingsKeyCell;
use crate::dialogs::settings::context::SettingsPageContext;
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView};

/// C# `Marker`: the prefix of the label of a box with a problem (U+26A0 and a space). A symbol, not language text.
pub const MARKER: &str = "\u{26A0} ";

/// `EN_CHANGE` (C# `private const int EnChange = 0x0300` of the page).
pub const EN_CHANGE: u32 = 0x0300;

/// C# `ShortcutRow(Property, Label, ClearTip)`: one row of the page. `clear_tip` `None` = a mandatory key (no clear button).
#[derive(Debug, Clone, Copy)]
pub struct ShortcutRow {
    /// A `ShortcutMappings` property name (`"Next"`).
    pub property: &'static str,
    /// The command name provider.
    pub label: fn() -> String,
    /// The tooltip provider of the clear button.
    pub clear_tip: Option<fn() -> String>,
}

const fn required(property: &'static str, label: fn() -> String) -> ShortcutRow {
    ShortcutRow {
        property,
        label,
        clear_tip: None,
    }
}

const fn optional(property: &'static str, label: fn() -> String, clear_tip: fn() -> String) -> ShortcutRow {
    ShortcutRow {
        property,
        label,
        clear_tip: Some(clear_tip),
    }
}

/// C# `NavigationRows` (8 rows; row order = Tab order and reading order). Real code (data).
pub const NAVIGATION_ROWS: [ShortcutRow; 8] = [
    required("Next", tr::settings_shortcut_next),
    required("Previous", tr::settings_shortcut_previous),
    required("FirstImage", tr::settings_shortcut_first_image),
    optional(
        "LastImage",
        tr::settings_shortcut_last_image,
        tr::settings_shortcut_last_image_clear_automation_name,
    ),
    required("NextFolder", tr::settings_shortcut_next_folder),
    required("PreviousFolder", tr::settings_shortcut_previous_folder),
    optional(
        "OpenFolder",
        tr::settings_shortcut_open_folder,
        tr::settings_shortcut_open_folder_clear_automation_name,
    ),
    optional(
        "ToggleCaptureMember",
        tr::settings_shortcut_toggle_capture_member,
        tr::settings_shortcut_toggle_capture_member_clear_automation_name,
    ),
];

/// C# `ViewRows` (13 rows). Real code (data).
pub const VIEW_ROWS: [ShortcutRow; 13] = [
    required("ZoomIn", tr::settings_shortcut_zoom_in),
    required("ZoomOut", tr::settings_shortcut_zoom_out),
    optional(
        "ZoomActualSize",
        tr::settings_shortcut_zoom_actual_size,
        tr::settings_shortcut_zoom_actual_size_clear_automation_name,
    ),
    required("ToggleFit", tr::settings_shortcut_toggle_fit),
    required("Fullscreen", tr::settings_shortcut_fullscreen),
    optional(
        "ToggleInfoOverlay",
        tr::settings_shortcut_toggle_info_overlay,
        tr::settings_shortcut_toggle_info_overlay_clear_automation_name,
    ),
    optional(
        "ClickZoom",
        tr::settings_shortcut_click_zoom,
        tr::settings_shortcut_click_zoom_clear_automation_name,
    ),
    optional(
        "FitWidth",
        tr::settings_shortcut_fit_width,
        tr::settings_shortcut_fit_width_clear_automation_name,
    ),
    optional(
        "FitHeight",
        tr::settings_shortcut_fit_height,
        tr::settings_shortcut_fit_height_clear_automation_name,
    ),
    optional(
        "FitWidth2",
        tr::settings_shortcut_fit_width2,
        tr::settings_shortcut_fit_width2_clear_automation_name,
    ),
    optional(
        "ToggleKeepZoom",
        tr::settings_shortcut_toggle_keep_zoom,
        tr::settings_shortcut_toggle_keep_zoom_clear_automation_name,
    ),
    optional(
        "CustomZoom",
        tr::settings_shortcut_custom_zoom,
        tr::settings_shortcut_custom_zoom_clear_automation_name,
    ),
    optional(
        "Refresh",
        tr::settings_shortcut_refresh,
        tr::settings_shortcut_refresh_clear_automation_name,
    ),
];

/// C# `ActionRows` (6 rows). Real code (data).
pub const ACTION_ROWS: [ShortcutRow; 6] = [
    required("Skip", tr::settings_shortcut_skip),
    required("Undo", tr::settings_shortcut_undo),
    required("Compare", tr::settings_shortcut_compare),
    optional(
        "MoveToFolder",
        tr::settings_shortcut_move_to_folder,
        tr::settings_shortcut_move_to_folder_clear_automation_name,
    ),
    optional(
        "CopyToFolder",
        tr::settings_shortcut_copy_to_folder,
        tr::settings_shortcut_copy_to_folder_clear_automation_name,
    ),
    required("SendToRecycleBin", tr::settings_shortcut_recycle),
];

/// C# `AllRows`: navigation, view, actions (27 rows, display order).
pub fn all_rows() -> impl Iterator<Item = &'static ShortcutRow> {
    NAVIGATION_ROWS.iter().chain(VIEW_ROWS.iter()).chain(ACTION_ROWS.iter())
}

/// C# `FindProblems(texts)`: the properties to mark: a mandatory box that is empty or not a valid key name, an optional box that is not empty and not a valid key
/// name, and every box of a group of two or more boxes with the same canonical key (compared ignoring ASCII case).
pub fn find_problems(texts: &BTreeMap<String, String>) -> BTreeSet<String> {
    todo!()
}

/// C# `Ids` (internal, for tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutsIds {
    pub reset_keys: i32,
    pub edit_actions: i32,
    pub warning: i32,
}

/// C# `ShortcutsSettingsPage`.
#[derive(Debug)]
pub struct ShortcutsSettingsPage {
    context: PageContextSlot,
    /// C# `_cells` (insertion-ordered `Dictionary`): `(property, cell)` in `all_rows()` order after `declare`.
    cells: Vec<(String, SettingsKeyCell)>,
    /// C# `_marked`; shared with the label providers the builder keeps (UI thread only).
    marked: Rc<RefCell<BTreeSet<String>>>,
    warning: i32,
    reset_keys: i32,
    edit_actions: i32,
    warning_text: String,
}

impl Default for ShortcutsSettingsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl ShortcutsSettingsPage {
    /// Ids 0, no cells, nothing marked, detached context.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `Ids`.
    pub fn ids(&self) -> ShortcutsIds {
        todo!()
    }

    /// C# `Cell(property)`: the key cell of `property` (`None` where C# throws `KeyNotFoundException`).
    pub fn cell(&self, property: &str) -> Option<SettingsKeyCell> {
        todo!()
    }

    /// C# `MarkedProperties` (sorted ordinally).
    pub fn marked_properties(&self) -> Vec<String> {
        todo!()
    }
}

impl SettingsPageView for ShortcutsSettingsPage {
    /// `"Shortcuts"`.
    fn key(&self) -> &str {
        todo!()
    }

    /// Hint note, warning line, reset button, three groups of key grids, the Action profiles hint and button (see the contract).
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        todo!()
    }

    /// Stores the context, then a tooltip (`set_tooltip_text`, evaluated now) on every clear button.
    fn attach(&mut self, context: SettingsPageContext) {
        todo!()
    }

    fn load(&mut self, state: &SettingsFormState) {
        todo!()
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        todo!()
    }

    /// `EN_CHANGE` of a key box -> `refresh()`; `BN_CLICKED` of reset / edit actions / a clear button; else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        todo!()
    }

    fn on_language_changed(&mut self) {
        todo!()
    }
}
