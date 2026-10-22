//! Port of `RecoveryDialog.cs`: the details pane of the selected row (`BuildDetailsText`, `AppendPath`, `UpdateDetails`, `Relayout`, `StaticTextColor`, lines
//! 383-542), its layout (`LayoutDetails`, `PlacePathGroup`, lines 925-985) and the two path buttons (`Copy`, `ShowInExplorer`, `StartExplorer`, lines 798-839).
//! Unit W8-18u. Contract: `contracts/W8-18u.md`.
//! Read-only: the only OS effects are the clipboard seam and Explorer (`/select,"file"` or `"nearest existing folder"`); nothing here writes a file.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::{Cell, RefCell};

use pr_app::presenters::recovery_filter::RecoveryTone;
use pr_app::presenters::recovery_window::RecoveryDetails;

use super::RecoveryContent;
use super::layout::{DetailsLayout, RecoveryLayoutBase};
use crate::dialogs::native::dialog::NativeDialog;

/// C# `Crlf`.
pub const CRLF: &str = "\r\n";

/// State of the details pane (C# `_verdictTone`, `_sourceTone`, `_destinationTone`, `_sourcePath`, `_destinationPath` and the five layout flags).
#[derive(Debug)]
pub struct DetailsState {
    /// The layout flags (`_hasDetails`, `_errorVisible`, `_sourceVisible`, `_destinationVisible`, `_noDestinationVisible`).
    pub(super) layout: Cell<DetailsLayout>,
    /// C# `_verdictTone`.
    pub(super) verdict_tone: Cell<RecoveryTone>,
    /// C# `_sourceTone`.
    pub(super) source_tone: Cell<RecoveryTone>,
    /// C# `_destinationTone`.
    pub(super) destination_tone: Cell<RecoveryTone>,
    /// C# `_sourcePath`: the path the Source "Copy path" / "Show in Explorer" act on (empty = none shown).
    pub(super) source_path: RefCell<String>,
    /// C# `_destinationPath`.
    pub(super) destination_path: RefCell<String>,
}

impl DetailsState {
    /// Nothing shown, neutral tones, empty paths.
    pub fn new() -> Self {
        Self {
            layout: Cell::new(DetailsLayout::default()),
            verdict_tone: Cell::new(RecoveryTone::Neutral),
            source_tone: Cell::new(RecoveryTone::Neutral),
            destination_tone: Cell::new(RecoveryTone::Neutral),
            source_path: RefCell::new(String::new()),
            destination_path: RefCell::new(String::new()),
        }
    }

    /// The five layout flags.
    pub fn layout(&self) -> DetailsLayout {
        self.layout.get()
    }
}

impl Default for DetailsState {
    fn default() -> Self {
        Self::new()
    }
}

/// C# `BuildDetailsText(details)` (lines 386-416), pure. `action_label` is `Tr.RecoveryDetailAction` (`RECOVERY_DETAIL_ACTION`). Text, in order, all line
/// breaks `\r\n`: `explain_text`; when `action_text` is non-empty: blank line, `"{action_label}: {action_text}"`; when `source_visible` and `source` is `Some`:
/// the path block; the same for the destination; when `group_members` is `Some`: blank line, `group_heading` (empty when `None`), then per member a blank
/// line and `status`, `source`, `destination` on three lines. A path block (`AppendPath`): blank line, `title`; `now_text` on its own line when non-empty;
/// `journal_text` when non-empty; `note` when `Some` and non-empty.
pub fn build_details_text(details: &RecoveryDetails, action_label: &str) -> String {
    todo!()
}

/// The C# `UpdateDetails()` (lines 437-522), see the contract 18u rule 2: the pane for ONE selected row, or hidden for none / several.
pub fn update_details(content: &RecoveryContent, dialog: &NativeDialog) {
    todo!()
}

/// C# `Relayout()`: `dialog.run_layout()` when the dialog is initialised (`ListState::initialized`).
pub fn relayout(content: &RecoveryContent, dialog: &NativeDialog) {
    todo!()
}

/// Pure core of C# `StaticTextColor(id)` (lines 532-542), the three tones being the dialog state: `VERDICT_BADGE_ID` / `SOURCE_STATUS_ID` / `DESTINATION_STATUS_ID` =
/// `to_color_ref` of the verdict / source / destination tone; `ERROR_ID` = palette key `Dark.ErrorText`; `SUMMARY_ID`, `CHECK_STATUS_ID` =
/// `DialogColors::text_secondary()`; `VERDICT_LABEL_ID`, `FILTER_EMPTY_ID`, `DETAILS_EMPTY_ID`, `NO_DESTINATION_ID` = palette key `Dark.TextMuted`;
/// `SOURCE_PATH_ID`, `DESTINATION_PATH_ID`, `DETAILS_ID` = `DialogColors::text_strong()`; anything else `DialogColors::text()`. A missing palette key falls
/// back to `DialogColors::text()` (never a panic).
pub fn text_color_for(id: i32, verdict: RecoveryTone, source: RecoveryTone, destination: RecoveryTone) -> u32 {
    todo!()
}

/// C# `StaticTextColor(id)`: `text_color_for(id, ..)` with the three tones stored in `DetailsState`.
pub fn static_text_color(content: &RecoveryContent, dialog: &NativeDialog, id: i32) -> u32 {
    todo!()
}

/// The five control ids of one path group (title, status, path box, Copy, Show).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathGroupIds {
    /// `SOURCE_TITLE_ID` / `DESTINATION_TITLE_ID`.
    pub title: i32,
    /// `SOURCE_STATUS_ID` / `DESTINATION_STATUS_ID`.
    pub status: i32,
    /// `SOURCE_PATH_ID` / `DESTINATION_PATH_ID`.
    pub path: i32,
    /// `COPY_SOURCE_ID` / `COPY_DESTINATION_ID`.
    pub copy: i32,
    /// `SHOW_SOURCE_ID` / `SHOW_DESTINATION_ID`.
    pub show: i32,
}

/// The source group ids.
pub const SOURCE_GROUP: PathGroupIds = PathGroupIds {
    title: super::layout::SOURCE_TITLE_ID,
    status: super::layout::SOURCE_STATUS_ID,
    path: super::layout::SOURCE_PATH_ID,
    copy: super::layout::COPY_SOURCE_ID,
    show: super::layout::SHOW_SOURCE_ID,
};

/// The destination group ids.
pub const DESTINATION_GROUP: PathGroupIds = PathGroupIds {
    title: super::layout::DESTINATION_TITLE_ID,
    status: super::layout::DESTINATION_STATUS_ID,
    path: super::layout::DESTINATION_PATH_ID,
    copy: super::layout::COPY_DESTINATION_ID,
    show: super::layout::SHOW_DESTINATION_ID,
};

/// C# `LayoutDetails(x, top, width, bottom)` (lines 925-965): the centred "details empty" static always; when `!details.has_details` stop; verdict label and
/// badge; the error static when `error_visible`; the source group when `source_visible`; the destination group when `destination_visible`; the "no
/// destination" static when `no_destination_visible`; `DETAILS_ID` takes the rest (`max(Dy(30), bottom - y)`).
pub fn layout_details(
    base: &dyn RecoveryLayoutBase,
    details: DetailsLayout,
    x: i32,
    top: i32,
    width: i32,
    bottom: i32,
) {
    todo!()
}

/// C# `PlacePathGroup(x, y, width, ...)` (lines 967-985): returns the next `y`. The path box height is `min(max(wrapped + Dy(4), FieldHeight), 6 * LineHeight
/// + Dy(4))` (NOT `i32::clamp`: it panics when `min > max`, R0003; C# `Math.Clamp` would throw too, but the bounds can not cross by construction).
pub fn place_path_group(base: &dyn RecoveryLayoutBase, x: i32, y: i32, width: i32, ids: PathGroupIds) -> i32 {
    todo!()
}

/// C# `Copy(path)`: a non-empty path goes to the `copy` seam; the result is ignored (a busy clipboard still lets the user select the text and press Ctrl+C).
pub fn copy(content: &RecoveryContent, path: &str) {
    todo!()
}

/// Pure core of C# `ShowInExplorer` (lines 807-834): `None` for a blank path; `Some("/select,\"{path}\"")` when `file_exists(path)`; else
/// `nearest_existing_folder(path, directory_exists)` -> `Some("\"{folder}\"")`, or `None` when there is no existing ancestor.
pub fn explorer_arguments(
    path: &str,
    file_exists: &dyn Fn(&str) -> bool,
    directory_exists: &dyn Fn(&str) -> bool,
) -> Option<String> {
    todo!()
}

/// C# `ShowInExplorer(path)`: `explorer_arguments` with the real file system (`std::fs::metadata`, read-only), then the `explorer` seam; `Err(message)` ->
/// `actions::notify(RECOVERY_EXPLORER_FAILED_TITLE, message, warning = true)`.
pub fn show_in_explorer(content: &RecoveryContent, dialog: &NativeDialog, path: &str) {
    todo!()
}

/// C# `StartExplorer(arguments)`: `explorer.exe` with the argument string passed VERBATIM (`std::os::windows::process::CommandExt::raw_arg`: the quotes of
/// `/select,"C:\a b\x.jpg"` must reach Explorer unchanged); the child is not waited for. A failed spawn is `Err(e.to_string())`.
pub fn start_explorer(arguments: &str) -> Result<(), String> {
    todo!()
}
