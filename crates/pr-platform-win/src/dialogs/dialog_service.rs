//! Port of `PhotoReview.Shell.Win32/Dialogs/Win32DialogService.cs` (172 lines: the `ISecondaryWindowPort` and `SecondaryWindowKind` declarations and the service) and
//! `SecondaryWindowPortHost.cs` (19 lines), baseline `d92d6aae`. Unit W8-18b. Contract: `contracts/W8-18b.md`.
//!
//! `pr_core::abstractions::DialogService` for the Win32 shell (C-13): confirmations, messages and errors through `task_dialog`, the folder picker through
//! `folder_dialog::FolderPicker`, every other window through the lazily created `SecondaryWindowPort` (its implementation is `secondary_window_port`, W8-18o).
//! The WPF `SecondaryWindowFallback` (117 lines) is retired, so a missing port means "false / nothing happens" (D-W8B5-3).
#![allow(unused_variables, dead_code)] // stub: removed by the porter with the last `todo!()`

use std::cell::{OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use pr_core::abstractions::{BatchReviewItem, DialogService, Log, SettingsTarget, SkippedEntry};
use pr_core::settings::shortcuts::ReviewAction;
use pr_l10n::localizer::Localizer;

use super::folder_dialog::FolderPicker;
use super::task_dialog::TaskDialogPort;
use crate::win32::structs::Hwnd;

/// C# `SecondaryWindowKind`: the windows a `SecondaryWindowPort` can run (the router asks `supports` per kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecondaryWindowKind {
    Settings,
    Recovery,
    Diagnostics,
    Benchmark,
    SkippedFiles,
    BatchReview,
    ZoomPrompt,
    /// WP-35 G2: the Action profiles editor (modal, nestable in Settings).
    ActionProfiles,
    /// FEAT-EXTERNAL-EDITORS: the editors list and the two small dialogs of the context menu (not planned in Wave 8, D-W8B5-4).
    ExternalEditors,
    /// FEAT-RENAME: the one-line box of "Rename..." / F2 (not planned in Wave 8, D-W8B5-4).
    RenamePrompt,
}

/// C# `ISecondaryWindowPort` (C-13b): the door to the secondary windows. Implemented by `secondary_window_port::SecondaryWindowRouter` (W8-18o); the tests use fakes.
/// Every method runs on the UI thread of the owner window. `owner` is the shell window's `HWND` (0 when it is gone).
///
/// Not ported (D-W8B5-4): `ShowExternalEditors`, `PromptAddExternalEditor`, `PromptDefaultExternalEditor`, `PromptRename` (their types do not exist in Rust yet and
/// `pr_core::abstractions::DialogService` has no such members). Their C# default bodies return `null`, so omitting them changes nothing for the ported members.
pub trait SecondaryWindowPort {
    /// `true` when the user saved changes.
    fn show_settings(&self, owner: Hwnd, target: SettingsTarget) -> bool;
    fn show_recovery(&self, owner: Hwnd);
    fn show_diagnostics(&self, owner: Hwnd);
    /// C# `ShowBenchmark(nint, string? folder)`.
    fn show_benchmark(&self, owner: Hwnd, folder: Option<&Path>);
    fn show_skipped_files(&self, owner: Hwnd, entries: &[SkippedEntry]);
    /// `true` when the user confirmed the batch.
    fn show_batch_review(&self, owner: Hwnd, items: &[BatchReviewItem]) -> bool;
    /// WP-19b: the custom click-zoom prompt (`IZoomPromptService`); `None` when cancelled.
    fn prompt_custom_zoom(&self, owner: Hwnd, current_percent: i32) -> Option<i32>;
    /// C# default interface method (`true`): does this port run the window itself? The router asks to choose per window.
    fn supports(&self, kind: SecondaryWindowKind) -> bool {
        true
    }
    /// WP-35 G2: the Action profiles editor. The edited list when the user applies, `None` when cancelled or when the port does not have it (C# default `null`).
    fn show_action_profiles(&self, owner: Hwnd, actions: &[ReviewAction]) -> Option<Vec<ReviewAction>> {
        None
    }
    /// C# `IDisposable.Dispose` of the router (`SecondaryWindowPortHost` calls it when the port was created): closes every secondary dialog that is still open. Default: nothing.
    fn dispose(&self) {}
}

/// The factory of the lazy port (the C# `Func<ISecondaryWindowPort?>`): may answer `None` (no port).
pub type PortFactory = Box<dyn FnOnce() -> Option<Rc<dyn SecondaryWindowPort>>>;

/// C# `Lazy<ISecondaryWindowPort?>`: the factory runs on the first `get`, at most once; its answer (a port or `None`) is kept. Shared (`Rc`) by the dialog service,
/// the zoom prompt service and the `SecondaryWindowPortHost`, like the one C# `Lazy` instance. Single-threaded (the UI thread), so a `OnceCell`.
pub struct LazySecondaryPort {
    factory: RefCell<Option<PortFactory>>,
    value: OnceCell<Option<Rc<dyn SecondaryWindowPort>>>,
}

impl LazySecondaryPort {
    /// C# `new Lazy<ISecondaryWindowPort?>(factory)`.
    pub fn new(factory: PortFactory) -> Self {
        todo!("W8-18b")
    }

    /// An already known answer (tests; the composition root when the router exists).
    pub fn from_value(port: Option<Rc<dyn SecondaryWindowPort>>) -> Self {
        todo!("W8-18b")
    }

    /// C# `Lazy.Value`: runs the factory once, then returns the kept answer. A re-entrant `get` from inside the factory returns `None` (never panics, never runs it twice). Store the answer with `OnceCell::set`, never `get_or_init` (it panics on a re-entrant init).
    pub fn get(&self) -> Option<Rc<dyn SecondaryWindowPort>> {
        todo!("W8-18b")
    }

    /// C# `Lazy.IsValueCreated`.
    pub fn is_value_created(&self) -> bool {
        todo!("W8-18b")
    }
}

/// C# `SecondaryWindowPortHost`: owns the lazy port; `dispose` (the container, or `ShellStartup` before the owner window is destroyed) closes every secondary dialog
/// still open, only when the port was already created (it never creates one just to close it). UI thread.
pub struct SecondaryWindowPortHost {
    pub port: Rc<LazySecondaryPort>,
}

impl SecondaryWindowPortHost {
    pub fn new(port: Rc<LazySecondaryPort>) -> Self {
        Self { port }
    }

    /// C# `Dispose`.
    pub fn dispose(&self) {
        todo!("W8-18b")
    }
}

/// C# `Win32DialogService : IDialogService`.
pub struct Win32DialogService {
    owner: Box<dyn Fn() -> Hwnd>,
    task_dialogs: Box<dyn TaskDialogPort>,
    folder_picker: FolderPicker,
    secondary: Rc<LazySecondaryPort>,
    localizer: Box<dyn Fn() -> Arc<Localizer>>,
    log: Option<Arc<dyn Log>>,
}

impl Win32DialogService {
    /// C# public ctor (`NativeTaskDialogPort` for the message boxes). The pick-folder title comes from `Localizer::current()` at call time.
    pub fn new(owner: Box<dyn Fn() -> Hwnd>, folder_picker: FolderPicker, secondary: Rc<LazySecondaryPort>) -> Self {
        todo!("W8-18b")
    }

    /// C# internal ctor: the task-dialog seam is injected.
    pub fn with_task_dialogs(
        owner: Box<dyn Fn() -> Hwnd>,
        task_dialogs: Box<dyn TaskDialogPort>,
        folder_picker: FolderPicker,
        secondary: Rc<LazySecondaryPort>,
    ) -> Self {
        todo!("W8-18b")
    }

    /// Replaces the localizer source (tests choose the language without global state; D-W8-10c-3).
    pub fn with_localizer(self, localizer: Box<dyn Fn() -> Arc<Localizer>>) -> Self {
        todo!("W8-18b")
    }

    /// Where a real `pick_folder` failure (the C# `COMException` escaping `PickFolder`) is written (D-W8B5-2).
    pub fn with_log(self, log: Arc<dyn Log>) -> Self {
        todo!("W8-18b")
    }

    /// C# `ShowBatchReview(IReadOnlyList<string> paths)`: the path-only overload, every item with an unknown length (`BatchReviewItem { length: None }`).
    pub fn show_batch_review_paths(&self, paths: &[PathBuf]) -> bool {
        todo!("W8-18b")
    }
}

impl DialogService for Win32DialogService {
    fn show_confirmation(&self, title: &str, message: &str) -> bool {
        todo!("W8-18b: task_dialogs.show(.., Confirmation) == IDYES")
    }

    fn show_message(&self, title: &str, message: &str) {
        todo!("W8-18b")
    }

    fn show_error(&self, title: &str, message: &str) {
        todo!("W8-18b")
    }

    fn pick_folder(&self, initial_folder: Option<&Path>) -> Option<PathBuf> {
        todo!("W8-18b: FolderPicker.pick(dialog.pickFolder.title, initial); Err -> log + None")
    }

    fn show_batch_review(&self, items: &[BatchReviewItem]) -> bool {
        todo!("W8-18b")
    }

    fn show_recovery(&self) {
        todo!("W8-18b")
    }

    fn show_diagnostics(&self) {
        todo!("W8-18b")
    }

    fn show_settings(&self) -> bool {
        todo!("W8-18b")
    }

    fn show_settings_at(&self, target: SettingsTarget) -> bool {
        todo!("W8-18b")
    }

    fn show_benchmark(&self, folder: Option<&Path>) {
        todo!("W8-18b")
    }

    fn show_skipped_files(&self, entries: &[SkippedEntry]) {
        todo!("W8-18b")
    }
}

impl std::fmt::Debug for LazySecondaryPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazySecondaryPort").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for SecondaryWindowPortHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecondaryWindowPortHost").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Win32DialogService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Win32DialogService").finish_non_exhaustive()
    }
}
