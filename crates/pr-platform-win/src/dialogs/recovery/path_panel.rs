//! Port of `RecoveryDialog.cs`: the details pane of the selected row (`BuildDetailsText`, `AppendPath`, `UpdateDetails`, `Relayout`, `StaticTextColor`, lines
//! 383-542), its layout (`LayoutDetails`, `PlacePathGroup`, lines 925-985) and the two path buttons (`Copy`, `ShowInExplorer`, `StartExplorer`, lines 798-839).
//! Unit W8-18u. Contract: `contracts/W8-18u.md`.
//! Read-only: the only OS effects are the clipboard seam and Explorer (`/select,"file"` or `"nearest existing folder"`); nothing here writes a file.
use std::cell::{Cell, RefCell};
use std::process::Command;

use pr_app::presenters::recovery_filter::RecoveryTone;
use pr_app::presenters::recovery_presentation::RecoveryPathView;
use pr_app::presenters::recovery_window::RecoveryDetails;
use pr_app::presenters::recovery_window::RecoveryWindowPresenter;

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
    let mut text = details.explain_text.clone();
    if !details.action_text.is_empty() {
        text.push_str(CRLF);
        text.push_str(CRLF);
        text.push_str(action_label);
        text.push_str(": ");
        text.push_str(&details.action_text);
    }
    fn append(text: &mut String, p: &RecoveryPathView) {
        text.push_str(CRLF);
        text.push_str(CRLF);
        text.push_str(&p.title);
        if !p.now_text.is_empty() {
            text.push_str(CRLF);
            text.push_str(&p.now_text);
        }
        if !p.journal_text.is_empty() {
            text.push_str(CRLF);
            text.push_str(&p.journal_text);
        }
        if let Some(note) = p.note.as_deref().filter(|n| !n.is_empty()) {
            text.push_str(CRLF);
            text.push_str(note);
        }
    }
    if details.source_visible {
        if let Some(p) = &details.source {
            append(&mut text, p);
        }
    }
    if details.destination_visible {
        if let Some(p) = &details.destination {
            append(&mut text, p);
        }
    }
    if let Some(members) = &details.group_members {
        text.push_str(CRLF);
        text.push_str(CRLF);
        text.push_str(details.group_heading.as_deref().unwrap_or(""));
        for m in members {
            text.push_str(CRLF);
            text.push_str(CRLF);
            text.push_str(&m.status);
            text.push_str(CRLF);
            text.push_str(&m.source);
            text.push_str(CRLF);
            text.push_str(&m.destination);
        }
    }
    text
}

/// The C# `UpdateDetails()` (lines 437-522), see the contract 18u rule 2: the pane for ONE selected row, or hidden for none / several.
pub fn update_details(content: &RecoveryContent, dialog: &NativeDialog) {
    use super::layout::*;
    if dialog.item(LIST_ID) == 0 {
        return;
    }
    let selected = super::list::single_selected(content, dialog);
    let mut state = DetailsLayout {
        has_details: selected.is_some(),
        ..DetailsLayout::default()
    };
    dialog.show_item(DETAILS_EMPTY_ID, selected.is_none());
    dialog.show_item(VERDICT_LABEL_ID, selected.is_some());
    dialog.show_item(VERDICT_BADGE_ID, selected.is_some());
    dialog.show_item(DETAILS_ID, selected.is_some());
    let Some(i) = selected else {
        content.details.layout.set(state);
        content.details.source_path.borrow_mut().clear();
        content.details.destination_path.borrow_mut().clear();
        for id in [
            ERROR_ID,
            SOURCE_TITLE_ID,
            SOURCE_STATUS_ID,
            SOURCE_PATH_ID,
            COPY_SOURCE_ID,
            SHOW_SOURCE_ID,
            DESTINATION_TITLE_ID,
            DESTINATION_STATUS_ID,
            DESTINATION_PATH_ID,
            COPY_DESTINATION_ID,
            SHOW_DESTINATION_ID,
            NO_DESTINATION_ID,
        ] {
            dialog.show_item(id, false);
        }
        dialog.set_item_text(DETAILS_ID, "");
        relayout(content, dialog);
        return;
    };
    let details = {
        let p = content.presenter.borrow();
        RecoveryWindowPresenter::details(&p.rows()[i], &content.texts.texts())
    };
    state.has_details = true;
    state.error_visible = details.error_visible;
    state.source_visible = details.source_visible && details.source.is_some();
    state.destination_visible = details.destination_visible && details.destination.is_some();
    state.no_destination_visible = details.no_destination_visible;
    content.details.verdict_tone.set(details.verdict_tone);
    dialog.set_item_text(VERDICT_BADGE_ID, &details.verdict_text);
    dialog.set_item_text(ERROR_ID, &details.error_text);
    dialog.show_item(ERROR_ID, state.error_visible);
    for (shown, path, tone, group, view) in [
        (
            state.source_visible,
            &content.details.source_path,
            &content.details.source_tone,
            SOURCE_GROUP,
            details.source.as_ref(),
        ),
        (
            state.destination_visible,
            &content.details.destination_path,
            &content.details.destination_tone,
            DESTINATION_GROUP,
            details.destination.as_ref(),
        ),
    ] {
        if let Some(v) = view.filter(|_| shown) {
            *path.borrow_mut() = v.path.clone();
            dialog.set_item_text(group.status, &v.status_text);
            dialog.set_item_text(group.path, &v.path);
            tone.set(v.status_tone);
        } else {
            path.borrow_mut().clear();
        }
        for id in [group.title, group.status, group.path, group.copy, group.show] {
            dialog.show_item(id, shown);
        }
    }
    dialog.show_item(NO_DESTINATION_ID, state.no_destination_visible);
    dialog.set_item_text(
        DETAILS_ID,
        &build_details_text(&details, &content.texts.text(pr_l10n::keys::RECOVERY_DETAIL_ACTION)),
    );
    content.details.layout.set(state);
    relayout(content, dialog);
}

/// C# `Relayout()`: `dialog.run_layout()` when the dialog is initialised (`ListState::initialized`).
pub fn relayout(content: &RecoveryContent, dialog: &NativeDialog) {
    if content.list.initialized.get() {
        dialog.run_layout();
    }
}

/// Pure core of C# `StaticTextColor(id)` (lines 532-542), the three tones being the dialog state: `VERDICT_BADGE_ID` / `SOURCE_STATUS_ID` / `DESTINATION_STATUS_ID` =
/// `to_color_ref` of the verdict / source / destination tone; `ERROR_ID` = palette key `Dark.ErrorText`; `SUMMARY_ID`, `CHECK_STATUS_ID` =
/// `DialogColors::text_secondary()`; `VERDICT_LABEL_ID`, `FILTER_EMPTY_ID`, `DETAILS_EMPTY_ID`, `NO_DESTINATION_ID` = palette key `Dark.TextMuted`;
/// `SOURCE_PATH_ID`, `DESTINATION_PATH_ID`, `DETAILS_ID` = `DialogColors::text_strong()`; anything else `DialogColors::text()`. A missing palette key falls
/// back to `DialogColors::text()` (never a panic).
pub fn text_color_for(id: i32, verdict: RecoveryTone, source: RecoveryTone, destination: RecoveryTone) -> u32 {
    use super::layout::*;
    use crate::dialogs::native::chrome::DialogColors;
    match id {
        VERDICT_BADGE_ID => super::list::to_color_ref(verdict),
        SOURCE_STATUS_ID => super::list::to_color_ref(source),
        DESTINATION_STATUS_ID => super::list::to_color_ref(destination),
        ERROR_ID => DialogColors::from_key("Dark.ErrorText").unwrap_or_else(DialogColors::text),
        SUMMARY_ID | CHECK_STATUS_ID => DialogColors::text_secondary(),
        VERDICT_LABEL_ID | FILTER_EMPTY_ID | DETAILS_EMPTY_ID | NO_DESTINATION_ID => {
            DialogColors::from_key("Dark.TextMuted").unwrap_or_else(DialogColors::text)
        }
        SOURCE_PATH_ID | DESTINATION_PATH_ID | DETAILS_ID => DialogColors::text_strong(),
        _ => DialogColors::text(),
    }
}

/// C# `StaticTextColor(id)`: `text_color_for(id, ..)` with the three tones stored in `DetailsState`.
pub fn static_text_color(content: &RecoveryContent, _dialog: &NativeDialog, id: i32) -> u32 {
    text_color_for(
        id,
        content.details.verdict_tone.get(),
        content.details.source_tone.get(),
        content.details.destination_tone.get(),
    )
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
    use super::layout::*;
    let lh = base.line_height();
    let empty_h = base.measure_item_height(DETAILS_EMPTY_ID, width);
    base.place(
        DETAILS_EMPTY_ID,
        x,
        top.wrapping_add((bottom.wrapping_sub(top).wrapping_sub(empty_h) / 2).max(0)),
        width,
        empty_h,
    );
    if !details.has_details {
        return;
    }
    let mut y = top;
    let h = lh;
    let verdict_width = base.measure_text_width(&base.get_item_text(VERDICT_LABEL_ID));
    base.place(VERDICT_LABEL_ID, x, y, verdict_width.wrapping_add(base.dx(2)), h);
    base.place(
        VERDICT_BADGE_ID,
        x.wrapping_add(verdict_width).wrapping_add(base.dx(8)),
        y,
        base.dx(40)
            .max(width.wrapping_sub(verdict_width).wrapping_sub(base.dx(8))),
        h,
    );
    y = y.wrapping_add(lh).wrapping_add(base.dy(6));
    if details.error_visible {
        let eh = base.measure_item_height(ERROR_ID, width);
        base.place(ERROR_ID, x, y, width, eh);
        y = y.wrapping_add(eh).wrapping_add(base.dy(6));
    }
    if details.source_visible {
        y = place_path_group(base, x, y, width, SOURCE_GROUP);
    }
    if details.destination_visible {
        y = place_path_group(base, x, y, width, DESTINATION_GROUP);
    }
    if details.no_destination_visible {
        let nh = base.measure_item_height(NO_DESTINATION_ID, width);
        base.place(NO_DESTINATION_ID, x, y, width, nh);
        y = y.wrapping_add(nh).wrapping_add(base.dy(6));
    }
    base.place(DETAILS_ID, x, y, width, bottom.wrapping_sub(y).max(base.dy(30)));
}

/// C# `PlacePathGroup(x, y, width, ...)` (lines 967-985): returns the next `y`. The path box height is `min(max(wrapped + Dy(4), FieldHeight), 6 * LineHeight
/// + Dy(4))` (NOT `i32::clamp`: it panics when `min > max`, R0003; C# `Math.Clamp` would throw too, but the bounds can not cross by construction).
pub fn place_path_group(base: &dyn RecoveryLayoutBase, x: i32, y: i32, width: i32, ids: PathGroupIds) -> i32 {
    let title_width = base.measure_text_width(&base.get_item_text(ids.title));
    let gap = base.dx(4);
    let title_h = base.line_height();
    let status_h = base.line_height();
    base.place(ids.title, x, y, title_width.wrapping_add(base.dx(2)), title_h);
    base.place(
        ids.status,
        x.wrapping_add(title_width).wrapping_add(base.dx(10)),
        y,
        base.dx(40)
            .max(width.wrapping_sub(title_width).wrapping_sub(base.dx(10))),
        status_h,
    );
    let py = y.wrapping_add(base.line_height()).wrapping_add(base.dy(3));
    let buttons_w = base.button_width(&[ids.copy, ids.show]);
    let path_w = base.dx(60).max(
        width
            .wrapping_sub(buttons_w.wrapping_mul(2))
            .wrapping_sub(gap.wrapping_mul(2)),
    );
    let wrapped = base.measure_text_height(&base.get_item_text(ids.path), path_w.wrapping_sub(base.dx(6)).max(1));
    let path_h = (wrapped.wrapping_add(base.dy(4)).max(super::layout::field_height(base)))
        .min(base.line_height().wrapping_mul(6).wrapping_add(base.dy(4)));
    base.place(ids.path, x, py, path_w, path_h);
    base.place(
        ids.copy,
        x.wrapping_add(path_w).wrapping_add(gap),
        py,
        buttons_w,
        super::layout::button_height(base),
    );
    base.place(
        ids.show,
        x.wrapping_add(path_w)
            .wrapping_add(gap)
            .wrapping_add(buttons_w)
            .wrapping_add(gap),
        py,
        buttons_w,
        super::layout::button_height(base),
    );
    py.wrapping_add(path_h).wrapping_add(base.dy(8))
}

/// C# `Copy(path)`: a non-empty path goes to the `copy` seam; the result is ignored (a busy clipboard still lets the user select the text and press Ctrl+C).
pub fn copy(content: &RecoveryContent, path: &str) {
    if !path.is_empty() {
        let _ = (content.copy)(path);
    }
}

/// Pure core of C# `ShowInExplorer` (lines 807-834): `None` for a blank path; `Some("/select,\"{path}\"")` when `file_exists(path)`; else
/// `nearest_existing_folder(path, directory_exists)` -> `Some("\"{folder}\"")`, or `None` when there is no existing ancestor.
pub fn explorer_arguments(
    path: &str,
    file_exists: &dyn Fn(&str) -> bool,
    directory_exists: &dyn Fn(&str) -> bool,
) -> Option<String> {
    if path.trim().is_empty() {
        return None;
    }
    if file_exists(path) {
        return Some(format!("/select,\"{path}\""));
    }
    pr_app::presenters::recovery_presentation::nearest_existing_folder(path, directory_exists)
        .map(|p| format!("\"{p}\""))
}

/// C# `ShowInExplorer(path)`: `explorer_arguments` with the real file system (`std::fs::metadata`, read-only), then the `explorer` seam; `Err(message)` ->
/// `actions::notify(RECOVERY_EXPLORER_FAILED_TITLE, message, warning = true)`.
pub fn show_in_explorer(content: &RecoveryContent, dialog: &NativeDialog, path: &str) {
    let args = explorer_arguments(path, &|p| std::fs::metadata(p).is_ok_and(|m| m.is_file()), &|p| {
        std::fs::metadata(p).is_ok_and(|m| m.is_dir())
    });
    if let Some(a) = args {
        if let Err(e) = (content.explorer)(&a) {
            super::actions::notify(
                content,
                dialog,
                &content.texts.text(pr_l10n::keys::RECOVERY_EXPLORER_FAILED_TITLE),
                &e,
                true,
            );
        }
    }
}

/// C# `StartExplorer(arguments)`: `explorer.exe` with the argument string passed VERBATIM (`std::os::windows::process::CommandExt::raw_arg`: the quotes of
/// `/select,"C:\a b\x.jpg"` must reach Explorer unchanged); the child is not waited for. A failed spawn is `Err(e.to_string())`.
pub fn start_explorer(arguments: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    Command::new("explorer.exe")
        .raw_arg(arguments)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}
