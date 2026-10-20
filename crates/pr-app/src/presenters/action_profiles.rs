//! Port of `PhotoReview.App.Shared/Presenters/ActionProfilesPresenter.cs` (baseline d92d6aae, 221 lines): the model behind
//! the Action Profiles editor (W-03 ActionProfilesWindow). It owns the list under edit, the open row, add/remove,
//! import/export and the Apply checks. No window types. Contract: `contracts/W8-10a.md`.
//!
//! Rust rules the bodies must keep (contract "Semantics" and "Traps"):
//! * rows are identified by [`RowId`] (C# compares object references, and a Rust index would shift on Remove);
//! * `Operation` cannot hold an undefined value, so the C# `Enum.IsDefined` checks have no Rust twin;
//! * every message comes from the [`Localizer`] passed in (D-W8-4), never from a literal.
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;

use pr_core::model::enums::FileOperationType;
use pr_core::settings::shortcuts::ReviewAction;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::{describe, LocArg};

use crate::input::key_id::KeyId;

/// `ActionProfileForm` (C# record): the detail fields the view shows. `operation_index` is the combo index:
/// 0 Move, 1 Copy, 2 Recycle (any other value reads as Move, see [`index_to_operation`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionProfileForm {
    pub name: String,
    pub shortcut: String,
    pub operation_index: i32,
    pub destination: String,
    pub confirm: bool,
}

impl ActionProfileForm {
    /// `ActionProfileForm.Empty`: empty texts, operation index 0, confirm off.
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            shortcut: String::new(),
            operation_index: 0,
            destination: String::new(),
            confirm: false,
        }
    }
}

/// `ActionProfilesApplyResult` (C# record). `message` is the localized reason when `accepted` is false.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionProfilesApplyResult {
    pub accepted: bool,
    pub message: Option<String>,
}

/// Opaque identity of one row of the presenter. A new value is issued by [`ActionProfilesPresenter::new`],
/// [`ActionProfilesPresenter::add`] and [`ActionProfilesPresenter::replace_all`]; a value of a removed or replaced row
/// never matches a live row again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowId(u64);

/// `ActionProfilesPresenter`: edits a COPY of the action list. `actions` and `row_ids` are parallel vectors
/// (same length, same order); `loaded` is the row the detail fields belong to (C# `_loaded`).
#[derive(Debug, Clone)]
pub struct ActionProfilesPresenter {
    actions: Vec<ReviewAction>,
    row_ids: Vec<RowId>,
    next_row_id: u64,
    loaded: Option<RowId>,
}

impl ActionProfilesPresenter {
    /// C# ctor: copies every action (the originals are never edited through the presenter).
    pub fn new(actions: &[ReviewAction]) -> Self {
        let actions = actions.to_vec();
        let count = u64::try_from(actions.len()).unwrap_or(u64::MAX);
        let row_ids = (1..=count).map(RowId).collect();
        Self {
            actions,
            row_ids,
            next_row_id: count.saturating_add(1),
            loaded: None,
        }
    }

    /// The list under edit, in order (C# `Actions`).
    pub fn actions(&self) -> &[ReviewAction] {
        &self.actions
    }

    /// The row id at `index`, or `None` when `index` is out of range.
    pub fn row_id(&self, index: usize) -> Option<RowId> {
        self.row_ids.get(index).copied()
    }

    /// The row the detail fields currently belong to (C# `Loaded`, test seam). `None` when nothing is open.
    pub fn loaded(&self) -> Option<RowId> {
        self.loaded
    }

    /// C# `Open(selected)`: the list selection changed. `None` means nothing selected (returns `None` and the view
    /// keeps its fields). An id that is not a live row behaves like `None`. Sets `loaded` in every other case.
    pub fn open(&mut self, selected: Option<RowId>) -> Option<ActionProfileForm> {
        let Some(id) = selected.filter(|id| self.row_ids.contains(id)) else {
            self.loaded = None;
            return None;
        };
        self.loaded = Some(id);
        let index = self.row_ids.iter().position(|candidate| *candidate == id)?;
        let action = self.actions.get(index)?;
        Some(ActionProfileForm {
            name: action.name.clone(),
            shortcut: action.shortcut.clone(),
            operation_index: operation_to_index(action.operation),
            destination: action.destination.clone(),
            confirm: action.confirm,
        })
    }

    /// C# `SaveCurrent(fields)`: writes the fields into the open row (name and destination trimmed, shortcut
    /// canonicalized, operation from the index). `false` when no row is open or the open row is no longer in the list.
    pub fn save_current(&mut self, fields: &ActionProfileForm) -> bool {
        let Some(id) = self.loaded else {
            return false;
        };
        let Some(index) = self.row_ids.iter().position(|candidate| *candidate == id) else {
            return false;
        };
        let Some(action) = self.actions.get_mut(index) else {
            return false;
        };
        action.name = fields.name.trim().to_owned();
        action.shortcut = pr_core::settings::shortcuts::canonicalize(Some(&fields.shortcut));
        action.destination = fields.destination.trim().to_owned();
        action.confirm = fields.confirm;
        action.operation = index_to_operation(fields.operation_index);
        true
    }

    /// C# `Add()`: appends a Move action named by `keys::ACTION_PROFILES_NEW_ACTION_NAME`, shortcut
    /// [`next_free_shortcut`], destination `"Output"`, and returns its row id. The caller selects it.
    pub fn add(&mut self, texts: &Localizer) -> RowId {
        let id = self.allocate_row_id();
        let shortcut = next_free_shortcut(&self.actions);
        let mut action = ReviewAction::new(texts.get("actionProfiles.newActionName"));
        action.shortcut = shortcut;
        action.destination = "Output".to_owned();
        self.actions.push(action);
        self.row_ids.push(id);
        id
    }

    /// C# `Remove(action, selectedIndex)`. Removes `row` if it is live (a stale id removes nothing). Returns the index to
    /// select next (`min(selected_index, len - 1)`), or `None` when the list is now empty (C# `-1`; the open row is dropped).
    pub fn remove(&mut self, row: RowId, selected_index: usize) -> Option<usize> {
        if let Some(index) = self.row_ids.iter().position(|candidate| *candidate == row) {
            self.row_ids.remove(index);
            self.actions.remove(index);
            if self.loaded == Some(row) {
                self.loaded = None;
            }
        }
        if self.actions.is_empty() {
            self.loaded = None;
            None
        } else {
            Some(selected_index.min(self.actions.len() - 1))
        }
    }

    /// C# `ReplaceAll(imported)`: replaces the whole list and issues NEW row ids, so a row id of the old list can no
    /// longer save. `loaded` is cleared (C# keeps the detached object, which `save_current` already refuses).
    pub fn replace_all(&mut self, imported: &[ReviewAction]) {
        self.actions = imported.to_vec();
        self.row_ids.clear();
        let action_count = self.actions.len();
        for _ in 0..action_count {
            let id = self.allocate_row_id();
            self.row_ids.push(id);
        }
        self.loaded = None;
    }

    /// C# `Apply()`: [`has_invalid_actions`] gives `dialog.actionProfilesInvalid.message`; otherwise
    /// [`find_destination_problem`] gives its message; otherwise accepted.
    pub fn apply(&self, texts: &Localizer) -> ActionProfilesApplyResult {
        if has_invalid_actions(&self.actions) {
            return ActionProfilesApplyResult {
                accepted: false,
                message: Some(texts.get("dialog.actionProfilesInvalid.message")),
            };
        }
        if let Some(message) = find_destination_problem(&self.actions, texts) {
            return ActionProfilesApplyResult {
                accepted: false,
                message: Some(message),
            };
        }
        ActionProfilesApplyResult {
            accepted: true,
            message: None,
        }
    }

    fn allocate_row_id(&mut self) -> RowId {
        let id = RowId(self.next_row_id);
        self.next_row_id = self.next_row_id.saturating_add(1);
        id
    }
}

/// C# `OperationToIndex`: Copy is 1, Recycle is 2, Move is 0.
pub fn operation_to_index(operation: FileOperationType) -> i32 {
    match operation {
        FileOperationType::Copy => 1,
        FileOperationType::Recycle => 2,
        _ => 0,
    }
}

/// C# `IndexToOperation`: 1 is Copy, 2 is Recycle, every other value (including negative) is Move.
pub fn index_to_operation(index: i32) -> FileOperationType {
    match index {
        1 => FileOperationType::Copy,
        2 => FileOperationType::Recycle,
        _ => FileOperationType::Move,
    }
}

/// C# `ShortcutKeyName.TryParse(shortcut, out key)` as an `Option`: the parsed key, or `None` when the text is not a
/// usable command key (see W8-04c `services::shortcut_key_name::try_parse`).
pub fn parse_shortcut(shortcut: &str) -> Option<KeyId> {
    crate::services::shortcut_key_name::try_parse(Some(shortcut))
}

/// C# `HasInvalidActions`: true when the list is empty, a name is blank (`trim().is_empty()`), a shortcut does not parse,
/// or two actions parse to the same [`KeyId`] (aliases such as Return/Enter are duplicates). Recycle is not special.
pub fn has_invalid_actions(actions: &[ReviewAction]) -> bool {
    if actions.is_empty() {
        return true;
    }
    let mut seen = HashSet::new();
    for action in actions {
        let Some(key) = parse_shortcut(&action.shortcut) else {
            return true;
        };
        if action.name.trim().is_empty() || !seen.insert(key) {
            return true;
        }
    }
    false
}

/// C# `NextFreeShortcut`: the first of `F6`..`F12` (by parsed key) that no action uses; `"F6"` when all seven are taken
/// so that [`has_invalid_actions`] reports the clash on Apply.
pub fn next_free_shortcut(actions: &[ReviewAction]) -> String {
    let used: HashSet<_> = actions
        .iter()
        .filter_map(|action| parse_shortcut(&action.shortcut))
        .collect();
    [
        KeyId::F6,
        KeyId::F7,
        KeyId::F8,
        KeyId::F9,
        KeyId::F10,
        KeyId::F11,
        KeyId::F12,
    ]
    .into_iter()
    .find(|key| !used.contains(key))
    .unwrap_or(KeyId::F6)
    .name()
    .to_owned()
}

/// C# `FindDestinationProblem`: the localized reason the first Move or Copy action has an unusable destination
/// (`actionProfiles.error.destinationEscapes` or `actionProfiles.error.destinationInvalid`, with `{name}`), or `None`.
/// Recycle actions are skipped.
pub fn find_destination_problem(actions: &[ReviewAction], texts: &Localizer) -> Option<String> {
    for action in actions {
        if action.operation == FileOperationType::Recycle {
            continue;
        }
        match pr_core::file_actions::destination_policy::validate(Some(&action.destination)) {
            pr_core::file_actions::destination_policy::ActionDestinationCheck::Ok => {}
            pr_core::file_actions::destination_policy::ActionDestinationCheck::EscapesSourceFolder => {
                return Some(texts.format(
                    "actionProfiles.error.destinationEscapes",
                    &[LocArg::new("name", action.name.clone())],
                ));
            }
            _ => {
                return Some(texts.format(
                    "actionProfiles.error.destinationInvalid",
                    &[LocArg::new("name", action.name.clone())],
                ));
            }
        }
    }
    None
}

/// C# `TryImport(path, out error)`. `Ok` holds the list (rows are copies); `Err` holds the localized reason:
/// `dialog.importActions.invalid.message` for malformed text, a literal `null`, an empty list or a null element;
/// `dialog.importActions.failed.message` with `{error}` for an I/O or access failure (the error is logged to
/// `AppLog` first). C# `error` is `""` on success, so `Ok` has no error text.
pub fn try_import(path: &Path, texts: &Localizer) -> Result<Vec<ReviewAction>, String> {
    let bytes = std::fs::read(path).map_err(|error| {
        log_io("Action import failed", path, &error);
        texts.format(
            "dialog.importActions.failed.message",
            &[LocArg::new(
                "error",
                format!("{}: {}", path.display(), describe(&error)),
            )],
        )
    })?;
    let text = String::from_utf8(bytes).map_err(|error| {
        log_io("Action import failed", path, &error);
        texts.format(
            "dialog.importActions.failed.message",
            &[LocArg::new("error", format!("{}: {}", path.display(), error))],
        )
    })?;
    let imported = super::action_list_json::deserialize_or_null(&text, "New action")
        .map_err(|_| texts.get("dialog.importActions.invalid.message"))?;
    let Some(actions) = imported else {
        return Err(texts.get("dialog.importActions.invalid.message"));
    };
    if actions.is_empty() {
        return Err(texts.get("dialog.importActions.invalid.message"));
    }
    Ok(actions)
}

/// C# `TryExport(path, actions)`: writes `ActionListJson::serialize_for_export` to `path`. `None` on success; otherwise
/// the localized `dialog.exportActions.failed.message` with `{error}` (logged to `AppLog` first).
pub fn try_export(path: &Path, actions: &[ReviewAction], texts: &Localizer) -> Option<String> {
    match std::fs::write(path, super::action_list_json::serialize_for_export(actions)) {
        Ok(()) => None,
        Err(error) => {
            log_io("Action export failed", path, &error);
            Some(texts.format(
                "dialog.exportActions.failed.message",
                &[LocArg::new(
                    "error",
                    format!("{}: {}", path.display(), describe(&error)),
                )],
            ))
        }
    }
}

fn log_io(message: &str, path: &Path, error: &(dyn Error + 'static)) {
    crate::misc::app_log::AppLog::global().error(&format!("{message}: {}", path.display()), Some(error));
}
