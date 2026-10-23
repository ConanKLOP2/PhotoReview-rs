//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/ShortcutsSettingsPage.cs` (baseline `7eaee2a2`): the "Shortcuts" page (WPF `ShortcutsPage`,
//! 29 key capture boxes): one read-only key box per command (the capture itself is the FRAME's key box subclass, W8-19j/19k), a clear button for the 16 optional
//! keys, "restore default keys", the live duplicate-key warning, the "warning sign" mark on the label of a bad box, and the Action profiles button (nested modal).
//! Unit W8-19i. Contract: `contracts/W8-19i.md`.
//!
//! Properties owned: the 29 shortcut texts (`SettingsFormState::shortcut_texts`) and, through the shared JSON, the actions.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use pr_app::presenters::settings::SettingsPresenter;
use pr_app::presenters::settings_field_map::{shortcut_get, shortcut_properties};
use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_app::services::shortcut_key_name::try_parse;
use pr_core::settings::app_settings::EnglishSettingsTexts;
use pr_core::settings::shortcuts::{ShortcutMappings, canonicalize};
use pr_l10n::tr;

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::builder::layout::SettingsKeyCell;
use crate::dialogs::settings::builder::layout::SettingsKeyCellSpec;
use crate::dialogs::settings::context::SettingsPageContext;
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView, text_fn};

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

/// C# `ActionRows` (8 rows). Real code (data).
pub const ACTION_ROWS: [ShortcutRow; 8] = [
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
    optional(
        "OpenInDefaultEditor",
        tr::settings_shortcut_open_in_default_editor,
        tr::settings_shortcut_open_in_default_editor_clear_automation_name,
    ),
    optional(
        "RenameFile",
        tr::settings_shortcut_rename_file,
        tr::settings_shortcut_rename_file_clear_automation_name,
    ),
];

/// C# `AllRows`: navigation, view, actions (29 rows, display order).
pub fn all_rows() -> impl Iterator<Item = &'static ShortcutRow> {
    NAVIGATION_ROWS.iter().chain(VIEW_ROWS.iter()).chain(ACTION_ROWS.iter())
}

/// C# `FindProblems(texts)`: the properties to mark: a mandatory box that is empty or not a valid key name, an optional box that is not empty and not a valid key
/// name, and every box of a group of two or more boxes with the same canonical key (compared ignoring ASCII case).
pub fn find_problems(texts: &BTreeMap<String, String>) -> BTreeSet<String> {
    let mut problems = BTreeSet::new();
    let mut by_key: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (property, raw) in texts {
        let canonical = canonicalize(Some(raw));
        if canonical.is_empty() {
            if !ShortcutMappings::is_optional(property) {
                problems.insert(property.clone());
            }
            continue;
        }
        if try_parse(Some(&canonical)).is_none() {
            problems.insert(property.clone());
            continue;
        }
        by_key
            .entry(canonical.to_ascii_lowercase())
            .or_default()
            .push(property.clone());
    }
    for owners in by_key.values().filter(|owners| owners.len() > 1) {
        problems.extend(owners.iter().cloned());
    }
    problems
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
        Self {
            context: PageContextSlot::default(),
            cells: Vec::new(),
            marked: Rc::new(RefCell::new(BTreeSet::new())),
            warning: 0,
            reset_keys: 0,
            edit_actions: 0,
            warning_text: String::new(),
        }
    }

    /// C# `Ids`.
    pub fn ids(&self) -> ShortcutsIds {
        ShortcutsIds {
            reset_keys: self.reset_keys,
            edit_actions: self.edit_actions,
            warning: self.warning,
        }
    }

    /// C# `Cell(property)`: the key cell of `property` (`None` where C# throws `KeyNotFoundException`).
    pub fn cell(&self, property: &str) -> Option<SettingsKeyCell> {
        self.cells
            .iter()
            .find(|(name, _)| name == property)
            .map(|(_, cell)| *cell)
    }

    /// C# `MarkedProperties` (sorted ordinally).
    pub fn marked_properties(&self) -> Vec<String> {
        self.marked.borrow().iter().cloned().collect()
    }
}

impl SettingsPageView for ShortcutsSettingsPage {
    /// `"Shortcuts"`.
    fn key(&self) -> &str {
        "Shortcuts"
    }

    /// Hint note, warning line, reset button, three groups of key grids, the Action profiles hint and button (see the contract).
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        self.cells.clear();
        self.warning_text.clear();
        page.note(Some(text_fn(tr::settings_group_shortcuts_hint)), false);
        self.warning = page.warning(None);
        self.reset_keys = page.buttons(false, &[text_fn(tr::settings_shortcuts_reset_keys)])[0];
        let mut navigation_cells = Vec::new();
        let marked = Rc::clone(&self.marked);
        page.group(text_fn(tr::settings_group_navigation), |group| {
            add_grid(group, &NAVIGATION_ROWS, &marked, &mut navigation_cells)
        });
        let mut view_cells = Vec::new();
        let marked = Rc::clone(&self.marked);
        page.group(text_fn(tr::settings_group_view), |group| {
            add_grid(group, &VIEW_ROWS, &marked, &mut view_cells)
        });
        let mut action_cells = Vec::new();
        let marked = Rc::clone(&self.marked);
        let edit = &mut self.edit_actions;
        page.group(text_fn(tr::settings_group_actions), |group| {
            add_grid(group, &ACTION_ROWS, &marked, &mut action_cells);
            group.note(Some(text_fn(tr::settings_edit_action_profiles_hint)), false);
            *edit = group.buttons(false, &[text_fn(tr::settings_edit_action_profiles)])[0];
        });
        self.cells = navigation_cells;
        self.cells.extend(view_cells);
        self.cells.extend(action_cells);
    }

    /// Stores the context, then a tooltip (`set_tooltip_text`, evaluated now) on every clear button.
    fn attach(&mut self, context: SettingsPageContext) {
        self.context.attach(context);
        let context = self.context.context();
        for row in all_rows() {
            if let (Some(tip), Some(cell)) = (row.clear_tip, self.cell(row.property)) {
                if cell.clear != 0 {
                    context.set_tooltip_text(cell.clear, &tip());
                }
            }
        }
    }

    fn load(&mut self, state: &SettingsFormState) {
        let context = self.context.context();
        for (property, cell) in &self.cells {
            let text = state.shortcut_texts.get(property).map_or("", String::as_str);
            context.set_text(cell.box_id, text);
        }
        self.refresh();
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        let context = self.context.context();
        for (property, cell) in &self.cells {
            state.shortcut_texts.insert(property.clone(), context.text(cell.box_id));
        }
    }

    /// `EN_CHANGE` of a key box -> `refresh()`; `BN_CLICKED` of reset / edit actions / a clear button; else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        if notification == EN_CHANGE && self.cells.iter().any(|(_, cell)| cell.box_id == id) {
            self.refresh();
            return true;
        }
        if notification != crate::win32::dialog_native::BN_CLICKED {
            return false;
        }
        if id == self.reset_keys {
            self.reset_keys();
            return true;
        }
        if id == self.edit_actions {
            self.edit_actions();
            return true;
        }
        if let Some((_, cell)) = self.cells.iter().find(|(_, cell)| cell.clear != 0 && cell.clear == id) {
            self.context.context().set_text(cell.box_id, "");
            self.refresh();
            return true;
        }
        false
    }

    fn on_language_changed(&mut self) {
        self.warning_text.clear();
        self.refresh();
    }
}

fn add_grid(
    group: &mut SettingsPageBuilder,
    rows: &[ShortcutRow],
    marked: &Rc<RefCell<BTreeSet<String>>>,
    cells: &mut Vec<(String, SettingsKeyCell)>,
) {
    let specs = rows
        .iter()
        .map(|row| {
            let property = row.property;
            let label = row.label;
            let marked = Rc::clone(marked);
            SettingsKeyCellSpec {
                label: text_fn(move || {
                    let is_marked = marked.borrow().contains(property);
                    if is_marked {
                        format!("{MARKER}{}", label())
                    } else {
                        label()
                    }
                }),
                clearable: row.clear_tip.is_some(),
            }
        })
        .collect::<Vec<_>>();
    let built = group.key_grid(&specs);
    cells.extend(
        rows.iter()
            .zip(built)
            .map(|(row, cell)| (row.property.to_owned(), cell)),
    );
}

impl ShortcutsSettingsPage {
    fn refresh(&mut self) {
        let context = self.context.context();
        let texts: BTreeMap<_, _> = self
            .cells
            .iter()
            .map(|(property, cell)| (property.clone(), context.text(cell.box_id)))
            .collect();
        let actions_json = context.actions_json();
        let message = context
            .presenter()
            .borrow_mut()
            .duplicate_shortcut_warning(&texts, &actions_json)
            .unwrap_or_default();
        if message != self.warning_text {
            self.warning_text = message.clone();
            context.set_text(self.warning, &message);
            context.relayout();
        }
        self.update_markers(&texts);
    }

    fn update_markers(&mut self, texts: &BTreeMap<String, String>) {
        let problems = find_problems(texts);
        let old = self.marked.borrow().clone();
        if old == problems {
            return;
        }
        let changed: BTreeSet<_> = old.union(&problems).cloned().collect();
        *self.marked.borrow_mut() = problems;
        let context = self.context.context();
        for property in changed {
            if let (Some(row), Some(cell)) = (all_rows().find(|row| row.property == property), self.cell(&property)) {
                let marked = self.marked.borrow().contains(&property);
                let label = if marked {
                    format!("{MARKER}{}", (row.label)())
                } else {
                    (row.label)()
                };
                context.set_text(cell.label, &label);
            }
        }
        context.relayout();
    }

    fn reset_keys(&mut self) {
        let defaults = ShortcutMappings::default();
        let context = self.context.context();
        for property in shortcut_properties() {
            if let Some(cell) = self.cell(property) {
                context.set_text(cell.box_id, shortcut_get(&defaults, property));
            }
        }
        self.refresh();
    }

    fn edit_actions(&mut self) {
        let context = self.context.context();
        let actions_json = context.collect_actions_json();
        let actions = match SettingsPresenter::actions_for_editor(&actions_json, &EnglishSettingsTexts) {
            Ok(actions) => actions,
            Err(_) => {
                context.show_warning(
                    &tr::dialog_open_editor_failed_title(),
                    &tr::dialog_open_editor_failed_message(),
                );
                return;
            }
        };
        if let Some(edited) = context.edit_action_profiles(&actions) {
            context.commit_actions_json(&SettingsPresenter::actions_to_text(&edited), self.key());
            self.refresh();
        }
    }
}
