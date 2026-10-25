//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/RecoveryDialog.cs` (986 lines, baseline `d92d6aae`): the modal Recovery window, the Win32 view of
//! `RecoveryWindowPresenter` (INV-5 on screen: every Restore (Retry) and Dismiss (Clear) decision is asked here and decided by the presenter and by the
//! delegates the caller passes in; this module never writes a file). Units W8-18t, W8-18u, W8-18v. Contracts: `contracts/W8-18t.md` .. `contracts/W8-18v.md`;
//! decisions D-W8B5-601 .. D-W8B5-616.
//!
//! The C# `RecoveryDialog : NativeDialog, IRecoveryCheckView` becomes a content object (`RecoveryContent`, implements [`DialogContent`], D-W8B5-301) held by the
//! handle [`RecoveryDialog`] together with the [`NativeDialog`]. The slices follow METHOD boundaries (D-W8B5-601), not the plan's line numbers:
//!
//! | file | unit | C# members |
//! |---|---|---|
//! | `mod` | 18t | fields, constructor, seams, `Title`..`WaitCursor` overrides, `OnInitialized`, `OnClosed`, `OnCancel`, the public test surface |
//! | `layout` | 18t | the control ids, `BuildItems`, `Margin`, `FieldHeight`, `ButtonHeight`, `PreferredClientSize`, `MinimumClientSize`, `LayoutControls` |
//! | `list` | 18t | `SelectedRows`, `SingleSelected`, `VisibleRows`, `SelectRows`, `RowView`, `ListItemText` / `Subtitle` / `BadgeColor`, `ToColorRef`, `RefreshEntries`, `OnSelectionChanged`, `OnRowResultChanged` |
//! | `path_panel` | 18u | `BuildDetailsText`, `AppendPath`, `UpdateDetails`, `Relayout`, `StaticTextColor`, `LayoutDetails`, `PlacePathGroup`, `Copy`, `ShowInExplorer`, `StartExplorer` |
//! | `actions` | 18u | `UpdateButtons`, `Enable`, `IsEnabled`, `OnCommand`, `StartChecks`, `IRecoveryCheckView`, the completion pump (new), `Confirm`, `Notify` |
//! | `restore` | 18v | `RetryAsync`, `SetBusy`, `OnRowsChangedByPresenter`, `Alive` |
//! | `dismiss` | 18v | `Dismiss` |
//!
//! Lead-owned: the submodule declarations below are final. The slice files own their own `*State` structs.

pub mod actions;
pub mod dismiss;
pub mod layout;
pub mod list;
pub mod path_panel;
pub mod restore;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::coordinators::zoom_detail::BackgroundRunner;
use pr_app::presenters::recovery_presentation::{RecoveryTexts, RecoveryTimeFormatter};
use pr_app::presenters::recovery_window::{RecoveryWindowOptions, RecoveryWindowPresenter};
use pr_core::file_actions::journal_line::JournalEntry;
use pr_l10n::localizer::Localizer;

use self::actions::ActionsState;
use self::list::{ListState, RowView};
use self::path_panel::DetailsState;
use self::restore::RestoreState;
use crate::dialogs::native::chrome::NativeDialogChrome;
use crate::dialogs::native::dialog::{DialogContent, DialogError, ListItemText, NativeDialog};
use crate::dialogs::native::template::DialogItem;
use crate::dialogs::task_dialog::TaskDialogPort;
use crate::host::message_loop::MessageLoop;
use crate::win32::structs::{Hwnd, Lparam, Lresult, Wparam};

/// The WM_APP message the worker threads post to the dialog window after they put a completion into the inbox (D-W8B5-602). Handled by
/// `actions::on_window_message`.
pub const WM_RECOVERY_COMPLETIONS: u32 = crate::win32::messages::WM_APP + 1;

/// Everything the dialog says comes from the catalog, never from an ambient `Tr` (D-W8B5-606): the localizer, the local-time formatter and the digit group
/// separator of `N0` that `RecoveryTexts` needs. Built once per dialog.
pub struct RecoveryTextSource {
    localizer: Arc<Localizer>,
    time: Rc<dyn RecoveryTimeFormatter>,
    group_separator: String,
}

impl std::fmt::Debug for RecoveryTextSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryTextSource").finish_non_exhaustive()
    }
}

impl RecoveryTextSource {
    /// Plain constructor.
    pub fn new(
        localizer: Arc<Localizer>,
        time: Rc<dyn RecoveryTimeFormatter>,
        group_separator: impl Into<String>,
    ) -> Self {
        Self {
            localizer,
            time,
            group_separator: group_separator.into(),
        }
    }

    /// The borrowed bundle the presenter functions take.
    pub fn texts(&self) -> RecoveryTexts<'_> {
        RecoveryTexts {
            loc: &self.localizer,
            time: &*self.time,
            group_separator: &self.group_separator,
        }
    }

    /// `localizer.format(key, &[])`: a text without arguments (C# `Tr.X`).
    pub fn text(&self, key: &str) -> String {
        self.localizer.format(key, &[])
    }

    /// The localizer (for `RecoveryWindowPresenter::{summary_text, dismiss_prompt, describe_retry}`).
    pub fn localizer(&self) -> &Localizer {
        &self.localizer
    }
}

/// The non-presenter collaborators of the dialog (composition root: 17e / 15j / 18o).
pub struct RecoveryDialogServices {
    /// Texts (see [`RecoveryTextSource`]).
    pub texts: RecoveryTextSource,
    /// Runs the blocking check and retry jobs off the UI thread. Production: `ThreadRunner`; tests: an inline or queued runner (no wall-clock waits).
    pub runner: Arc<dyn BackgroundRunner>,
}

impl std::fmt::Debug for RecoveryDialogServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryDialogServices").finish_non_exhaustive()
    }
}

/// C# `Func<string, bool>` `Copy` seam: puts a path on the clipboard, `true` = success.
pub type CopySeam = Rc<dyn Fn(&str) -> bool>;

/// C# `Action<string>` `Explorer` seam: starts Explorer with an argument string; `Err(message)` = the C# `Win32Exception` / `InvalidOperationException`.
pub type ExplorerSeam = Rc<dyn Fn(&str) -> Result<(), String>>;

/// C# `RecoveryDialogSeams` record. `None` = the real thing (`messages`, `explorer`) or the C# default (`copy`: the button does nothing).
pub struct RecoveryDialogSeams {
    /// C# `Messages`: Confirmation / Information / Error boxes. `None` = `NativeTaskDialogPort::new()`.
    pub messages: Option<Rc<dyn TaskDialogPort>>,
    /// C# `Copy`: puts a path on the clipboard, `true` = success (the result is ignored). `None` = `|_| false`.
    pub copy: Option<CopySeam>,
    /// C# `Explorer`: starts Explorer with an argument string. `Err(message)` is the C# `Win32Exception` / `InvalidOperationException` (D-W8B5-605). `None` =
    /// `path_panel::start_explorer`.
    pub explorer: Option<ExplorerSeam>,
    /// C# `AutoCheck`: start the background check when the dialog is created (default `true`); tests switch it off to drive each step.
    pub auto_check: bool,
}

impl std::fmt::Debug for RecoveryDialogSeams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryDialogSeams").finish_non_exhaustive()
    }
}

impl Default for RecoveryDialogSeams {
    /// All `None`, `auto_check = true` (C# `seams = null`).
    fn default() -> Self {
        Self {
            messages: None,
            copy: None,
            explorer: None,
            auto_check: true,
        }
    }
}

/// The abstract / virtual members and ALL state of the dialog (the C# fields of `RecoveryDialog`). Every method of the slices takes `&RecoveryContent` and the
/// `&NativeDialog` that the hook received. Fields are private to this module tree: the slices (children) read them, nobody else does.
pub struct RecoveryContent {
    /// C# `_presenter`. A borrow is NEVER held across an OS call, a hook, `confirm` or `notify` (D-W8B5-603).
    presenter: RefCell<RecoveryWindowPresenter>,
    texts: RecoveryTextSource,
    /// C# `_messages`.
    messages: Rc<dyn TaskDialogPort>,
    /// C# `_copy`.
    copy: CopySeam,
    /// C# `_explorer`.
    explorer: ExplorerSeam,
    /// C# `_autoCheck`.
    auto_check: bool,
    runner: Arc<dyn BackgroundRunner>,
    list: ListState,
    details: DetailsState,
    actions: ActionsState,
    restore: RestoreState,
}

impl std::fmt::Debug for RecoveryContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryContent").finish_non_exhaustive()
    }
}

impl RecoveryContent {
    /// C# constructor (the base class part, `INativeDialogChrome`, is the [`NativeDialog`]'s): builds the presenter from `entries` and `options`, resolves
    /// the seam defaults, creates the inbox whose wake function posts [`WM_RECOVERY_COMPLETIONS`] to the window stored in `ActionsState::wake_target`.
    pub fn new(
        entries: Vec<JournalEntry>,
        options: RecoveryWindowOptions,
        services: RecoveryDialogServices,
        seams: RecoveryDialogSeams,
    ) -> Rc<Self> {
        Rc::new(Self {
            presenter: RefCell::new(RecoveryWindowPresenter::new(entries, options)),
            texts: services.texts,
            messages: seams
                .messages
                .unwrap_or_else(|| Rc::new(crate::dialogs::task_dialog::NativeTaskDialogPort::new())),
            copy: seams.copy.unwrap_or_else(|| Rc::new(|_| false)),
            explorer: seams.explorer.unwrap_or_else(|| Rc::new(path_panel::start_explorer)),
            auto_check: seams.auto_check,
            runner: services.runner,
            list: ListState::new(),
            details: DetailsState::new(),
            actions: ActionsState::new(),
            restore: RestoreState::new(),
        })
    }
}

impl DialogContent for RecoveryContent {
    /// `texts.text(RECOVERY_TITLE)`.
    fn title(&self) -> String {
        self.texts.text(pr_l10n::keys::RECOVERY_TITLE)
    }
    /// `layout::build_items(&self.texts)`.
    fn items(&self) -> Vec<DialogItem> {
        layout::build_items(&self.texts)
    }
    /// `true`.
    fn resizable(&self) -> bool {
        true
    }
    /// `[layout::HEADER_ID]`.
    fn header_control_ids(&self) -> Vec<i32> {
        vec![layout::HEADER_ID]
    }
    /// `layout::LIST_ID`.
    fn initial_focus_id(&self) -> i32 {
        layout::LIST_ID
    }
    /// `layout::CLOSE_ID` (= `ID_CANCEL`).
    fn default_button_id(&self) -> i32 {
        layout::CLOSE_ID
    }
    /// `layout::preferred_client_size(dialog)`.
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        layout::preferred_client_size(dialog)
    }
    /// `layout::minimum_client_size(dialog)`.
    fn minimum_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        layout::minimum_client_size(dialog)
    }
    /// `layout::layout_controls(dialog, self.details.layout(), w, h)`.
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32) {
        layout::layout_controls(dialog, self.details.layout(), client_width, client_height)
    }
    /// C# `OnInitialized` (lines 183-218): see the contract 18t, rule 3.
    fn on_initialized(&self, dialog: &NativeDialog) {
        use crate::dialogs::native::controls::basic::{ComboBoxControl, ListBoxControl};
        let filter = dialog.item(layout::FILTER_ID);
        for key in [
            pr_l10n::keys::RECOVERY_FILTER_ALL,
            pr_l10n::keys::RECOVERY_FILTER_CAN_RETRY,
            pr_l10n::keys::RECOVERY_FILTER_ALREADY_DONE,
            pr_l10n::keys::RECOVERY_FILTER_PROBLEMS,
        ] {
            ComboBoxControl::add(filter, &self.texts.text(key));
        }
        ComboBoxControl::select(filter, 0);
        for (id, key) in [
            (layout::FILTER_ID, pr_l10n::keys::RECOVERY_FILTER_AUTOMATION_NAME),
            (
                layout::RECHECK_ID,
                pr_l10n::keys::RECOVERY_CHECK_RECHECK_AUTOMATION_NAME,
            ),
            (
                layout::CLEAR_SELECTED_ID,
                pr_l10n::keys::RECOVERY_CLEAR_SELECTED_AUTOMATION_NAME,
            ),
            (layout::CLEAR_ALL_ID, pr_l10n::keys::RECOVERY_CLEAR_ALL_AUTOMATION_NAME),
            (layout::RETRY_ID, pr_l10n::keys::RECOVERY_RETRY_AUTOMATION_NAME),
            (
                layout::COPY_SOURCE_ID,
                pr_l10n::keys::RECOVERY_DETAIL_COPY_PATH_AUTOMATION_NAME,
            ),
            (
                layout::COPY_DESTINATION_ID,
                pr_l10n::keys::RECOVERY_DETAIL_COPY_PATH_AUTOMATION_NAME,
            ),
            (
                layout::SHOW_SOURCE_ID,
                pr_l10n::keys::RECOVERY_DETAIL_SHOW_IN_EXPLORER_AUTOMATION_NAME,
            ),
            (
                layout::SHOW_DESTINATION_ID,
                pr_l10n::keys::RECOVERY_DETAIL_SHOW_IN_EXPLORER_AUTOMATION_NAME,
            ),
        ] {
            dialog.set_tooltip(id, &self.texts.text(key));
        }
        self.actions
            .wake_target
            .store(dialog.hwnd(), std::sync::atomic::Ordering::SeqCst);
        list::refresh_entries(self, dialog);
        self.list.initialized.set(true);
        if self.auto_check {
            actions::start_checks(self, dialog);
        }
        let _ = ListBoxControl::count(dialog.item(layout::LIST_ID));
    }
    /// C# `OnCancel() => !_presenter.CanClose`: `true` (handled, stay open) while a retry runs. `presenter.try_borrow()`; when the presenter is borrowed (a
    /// re-entrant call from inside a presenter method) answer `true`: refusing to close is the safe default (R2-A-01), and a hook never panics.
    fn on_cancel(&self, _dialog: &NativeDialog) -> bool {
        !self.presenter.try_borrow().map(|p| p.can_close()).unwrap_or(false)
    }
    /// `actions::on_command`.
    fn on_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        actions::on_command(self, dialog, id, notification)
    }
    /// `list::list_item_text`.
    fn list_item_text(&self, dialog: &NativeDialog, id: i32, index: i32) -> ListItemText {
        list::list_item_text(self, dialog, id, index)
    }
    /// `list::list_item_subtitle`.
    fn list_item_subtitle(&self, dialog: &NativeDialog, id: i32, index: i32) -> Option<String> {
        list::list_item_subtitle(self, dialog, id, index)
    }
    /// `list::list_item_badge_color`.
    fn list_item_badge_color(&self, dialog: &NativeDialog, id: i32, index: i32) -> Option<u32> {
        list::list_item_badge_color(self, dialog, id, index)
    }
    /// C# `CanCloseByUser => _presenter.CanClose`. `try_borrow()`; a failed borrow answers `false` (do not close; see `on_cancel`).
    fn can_close_by_user(&self) -> bool {
        self.presenter.try_borrow().map(|p| p.can_close()).unwrap_or(false)
    }
    /// C# `WaitCursor => _busy`.
    fn wait_cursor(&self) -> bool {
        restore::is_busy(self)
    }
    /// C# `OnClosed` (lines 220-230): see the contract 18t, rule 4.
    fn on_closed(&self, _dialog: &NativeDialog) {
        self.presenter.borrow_mut().cancel_check();
        self.actions.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.actions.wake_target.store(0, std::sync::atomic::Ordering::SeqCst);
        let _ = self.actions.inbox.drain();
    }
    /// `actions::on_window_message` (only [`WM_RECOVERY_COMPLETIONS`] is handled).
    fn on_window_message(
        &self,
        dialog: &NativeDialog,
        message: u32,
        wparam: Wparam,
        lparam: Lparam,
    ) -> Option<Lresult> {
        actions::on_window_message(self, dialog, message, wparam, lparam)
    }
    /// `path_panel::static_text_color`.
    fn static_text_color(&self, dialog: &NativeDialog, id: i32) -> u32 {
        path_panel::static_text_color(self, dialog, id)
    }
}

/// The Recovery window (C# `RecoveryDialog`): the handle the caller (W8-18o, `Win32SecondaryWindowPort`) and the tests use.
pub struct RecoveryDialog {
    native: NativeDialog,
    content: Rc<RecoveryContent>,
}

impl std::fmt::Debug for RecoveryDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryDialog").finish_non_exhaustive()
    }
}

impl RecoveryDialog {
    /// C# constructor. `chrome = None` = the base class default (`default_chrome()`).
    pub fn new(
        entries: Vec<JournalEntry>,
        options: RecoveryWindowOptions,
        services: RecoveryDialogServices,
        seams: RecoveryDialogSeams,
        chrome: Option<Rc<dyn NativeDialogChrome>>,
    ) -> Self {
        let content = RecoveryContent::new(entries, options, services, seams);
        let native = NativeDialog::with_chrome(content.clone(), chrome);
        Self { native, content }
    }

    /// The base-class handle (`hwnd()`, `is_closed()`, `dpi()`, `close()`, `activate()`, the helpers).
    pub fn native(&self) -> &NativeDialog {
        &self.native
    }

    /// The content (state and slices).
    pub fn content(&self) -> &Rc<RecoveryContent> {
        &self.content
    }

    /// `native.show_modal(owner, None, message_loop)`.
    pub fn show_modal(&self, owner: Hwnd, message_loop: Option<&Rc<MessageLoop>>) -> Result<i32, DialogError> {
        self.native.show_modal(owner, None, message_loop)
    }

    /// `native.show_modeless(owner, message_loop, None, closed)` (what the tests and W8-18o use).
    pub fn show_modeless(
        &self,
        owner: Hwnd,
        message_loop: Option<Rc<MessageLoop>>,
        closed: Option<Box<dyn FnOnce()>>,
    ) -> Result<(), DialogError> {
        self.native.show_modeless(owner, message_loop, None, closed)
    }

    /// C# `Close()` from outside: always closes, even mid-retry.
    pub fn close(&self) {
        self.native.close()
    }

    /// C# `Presenter` (tests): read access under a short borrow.
    pub fn with_presenter<R>(&self, f: impl FnOnce(&RecoveryWindowPresenter) -> R) -> R {
        f(&self.content.presenter.borrow())
    }

    /// C# `SelectedRows()` as indices into the presenter rows, in display order.
    pub fn selected_rows(&self) -> Vec<usize> {
        list::selected_rows(&self.content, &self.native)
    }

    /// C# `VisibleRows` as indices into the presenter rows.
    pub fn visible_rows(&self) -> Vec<usize> {
        list::visible_rows(&self.content)
    }

    /// C# `SelectRows(params int[] visibleIndices)`: selects those visible rows as a user would and updates details and buttons.
    pub fn select_rows(&self, visible_indices: &[i32]) {
        list::select_rows(&self.content, &self.native, visible_indices)
    }

    /// C# `RowView(index)`.
    pub fn row_view(&self, index: i32) -> RowView {
        list::row_view(&self.content, &self.native, index)
    }

    /// C# `IsBusy`.
    pub fn is_busy(&self) -> bool {
        restore::is_busy(&self.content)
    }

    /// C# `IsEnabled(id)`.
    pub fn is_enabled(&self, id: i32) -> bool {
        crate::win32::dialog_native::user32::is_window_enabled(self.native.item(id))
    }

    /// C# `TextColorOf(id)`.
    pub fn text_color_of(&self, id: i32) -> u32 {
        path_panel::static_text_color(&self.content, &self.native, id)
    }

    /// C# `StartChecks()`.
    pub fn start_checks(&self) {
        actions::start_checks(&self.content, &self.native)
    }

    /// Runs `actions::pump` now (tests with an inline or queued runner; the production path is the window message).
    pub fn pump_completions(&self) {
        actions::pump(&self.content, &self.native)
    }

    /// Replaces C# `LastCheckTask`: how many checks `start_checks` launched (a run with no rows launches none).
    pub fn check_runs(&self) -> u32 {
        self.content.actions.check_runs.get()
    }

    /// Replaces C# `LastRetryTask`: how many retries the Retry button launched.
    pub fn retry_runs(&self) -> u32 {
        self.content.restore.retry_runs.get()
    }
}
