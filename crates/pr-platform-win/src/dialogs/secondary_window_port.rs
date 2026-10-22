//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/Win32SecondaryWindowPort.cs` (374 lines, baseline `d92d6aae`): `Win32SecondaryWindowPort` (lines 36-315) and
//! `SecondaryWindowRouter` (lines 321-374), plus the `BenchmarkDialogSeams` record (22-27) of the same file. Unit W8-18o. Contract: `contracts/W8-18o.md`.
//!
//! The Win32 port is "which dialog opens which window": it runs every secondary window (Settings, Recovery, Diagnostics, Benchmark, Skipped files, Batch
//! review, the zoom prompt, Action profiles) as a native dialog on the UI thread, modal for all but Benchmark, keeps the list of the dialogs that are open, and
//! closes them all (innermost first) on `dispose`. The router in front of it answers `supports` with `true` for everything and sends a window the port cannot
//! run (Settings while a page is missing) to the retired fallback, i.e. to the "cancelled" answer (D-W8B5-3).
//!
//! The C# builds each dialog from the DI container (`IServiceProvider`). Rust has no container (plan section 09): the port holds a [`SecondaryWindowDialogs`], the
//! seam that builds the dialog of each window and says how to read its result, so the whole routing, bookkeeping and dispose logic is tested with fake dialogs
//! (D-W8B5-413). The production implementation of that seam is one adapter in the composition (see the contract, "Unplanned").
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_core::abstractions::{BatchReviewItem, Log, SettingsTarget, SkippedEntry};
use pr_core::settings::shortcuts::ReviewAction;

use super::dialog_service::{SecondaryWindowKind, SecondaryWindowPort};
use super::native::dialog::NativeDialog;
use crate::host::message_loop::MessageLoop;
use crate::win32::structs::Hwnd;

/// C# `Action<NativeDialog>? ready`: called after the dialog is shown and before the modal loop (a test simulates the user there).
pub type ReadyHook = Rc<dyn Fn(&NativeDialog)>;

/// C# method group `ShowActionProfiles` handed to the Settings dialog (`options.ShowActionProfiles`): opens the Action profiles editor on top of the Settings
/// window through the same port. Holds a `Weak` of the port (no cycle, D-W8B3H-18); after the port is gone it answers `None`.
pub type ActionProfilesOpener = Rc<dyn Fn(Hwnd, &[ReviewAction]) -> Option<Vec<ReviewAction>>>;

/// A dialog that is ready to be shown modally and the way to read what the user did. `finish` runs once with the id the dialog ended with (`ID_OK`, `ID_CANCEL`
/// or the custom ids of the dialog) and returns the value the port method must answer; it reads the state of the dialog content (C# `dialog.Saved`,
/// `dialog.Value`, `dialog.Actions`).
pub struct PreparedDialog<T> {
    /// The dialog to show (not shown yet).
    pub dialog: NativeDialog,
    /// The reading of the result.
    pub finish: Box<dyn FnOnce(i32) -> T>,
}

impl<T> std::fmt::Debug for PreparedDialog<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedDialog").finish_non_exhaustive()
    }
}

impl<T> PreparedDialog<T> {
    /// A dialog with the reading of its result.
    pub fn new(dialog: NativeDialog, finish: Box<dyn FnOnce(i32) -> T>) -> Self {
        Self { dialog, finish }
    }
}

/// The seam that replaces the C# `IServiceProvider` plus the `new XDialog(...)` calls of the port (D-W8B5-413). Every method runs on the UI thread, returns
/// `None` when the window cannot be opened (the C# `GetService<...>() is null` returns and the unreadable journal), and never shows anything itself: the port
/// shows what it returns. The production implementation owns the services (`SettingsStore`, `LocalizationService`, `OperationJournal`, the clipboard, the file
/// and folder dialogs, the warning sink, `ReviewMetrics`, the UI scheduler) and passes the chrome to each dialog.
pub trait SecondaryWindowDialogs {
    /// C# `new SettingsDialog(store.Current, store, decoders, localization, updates, options with { Target = target, ShowActionProfiles = ... })`.
    /// `None` when there is no settings store. `show_action_profiles` is the opener for the nested Action profiles window.
    fn settings(
        &self,
        target: SettingsTarget,
        show_action_profiles: ActionProfilesOpener,
    ) -> Option<PreparedDialog<bool>>;

    /// C# `new RecoveryDialog(...)` after reading the pending and failed entries of the journal. `None` when there is no journal, or when it could not be read
    /// (the implementation reports that itself with [`try_read_recovery_entries`], RV-A14, and the port shows nothing).
    fn recovery(&self, owner: Hwnd) -> Option<PreparedDialog<()>>;

    /// C# `new BenchmarkDialog(folder, ...)`: MODELESS, not shown yet; the port shows it (`show_modeless`) and keeps the single instance.
    fn benchmark(&self, folder: Option<&Path>) -> Option<NativeDialog>;

    /// C# `new ActionProfilesDialog(actions, files, warnings, chrome)`; `finish(ID_OK)` is the edited list, any other id `None`.
    fn action_profiles(&self, actions: &[ReviewAction]) -> Option<PreparedDialog<Option<Vec<ReviewAction>>>>;

    /// C# `new DiagnosticsDialog(DiagnosticsPresenter.Build(snapshot, explorer), copy, chrome)`.
    fn diagnostics(&self) -> Option<PreparedDialog<()>>;

    /// C# `new ListDialog(ListDialogContent.ForSkippedFiles(entries), chrome)`.
    fn skipped_files(&self, entries: &[SkippedEntry]) -> Option<PreparedDialog<()>>;

    /// C# `new ListDialog(ListDialogContent.ForBatchReview(items), chrome)`; `finish(id)` is `id == ID_OK`.
    fn batch_review(&self, items: &[BatchReviewItem]) -> Option<PreparedDialog<bool>>;

    /// C# `new ZoomPromptDialog(currentPercent, chrome)`; `finish(ID_OK)` is `Some(value)`, any other id `None`.
    fn custom_zoom(&self, current_percent: i32) -> Option<PreparedDialog<Option<i32>>>;
}

/// The optional constructor arguments of the C# `Win32SecondaryWindowPort` (`ready`, `loop`, `settingsEnabled`; `chrome`, `warnings`, `benchmark`, `settings`
/// and `recovery` seams belong to the [`SecondaryWindowDialogs`] implementation).
#[derive(Default)]
pub struct PortOptions {
    /// C# `ready`.
    pub ready: Option<ReadyHook>,
    /// C# `loop`: the shell loop; the modal loops run nested in it. `None` = the plain loop of `NativeDialog`.
    pub message_loop: Option<Rc<MessageLoop>>,
    /// C# `settingsEnabled`: overrides `settings::catalog::is_complete()`.
    pub settings_enabled: Option<Box<dyn Fn() -> bool>>,
}

impl std::fmt::Debug for PortOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortOptions").finish_non_exhaustive()
    }
}

/// Dialogs the port has opened and not seen closed. `id` identifies one without comparing handles (a closed dialog has `hwnd == 0`).
struct OpenDialog {
    id: u64,
    dialog: NativeDialog,
}

/// The state shared with the `closed` callback of the modeless Benchmark window (the callback holds a `Weak<PortState>`: the dialog owns the callback and the
/// port owns the dialog, so a strong reference would be a cycle for as long as the window stays open, D-W8B5-421).
struct PortState {
    open: RefCell<Vec<OpenDialog>>,
    benchmark: RefCell<Option<OpenDialog>>,
    next_id: Cell<u64>,
    disposed: Cell<bool>,
}

/// C# `Win32SecondaryWindowPort : ISecondaryWindowPort, IDisposable`. UI thread only (`!Send`: the C# `VerifyUiThread` is a compile-time property here).
pub struct Win32SecondaryWindowPort {
    dialogs: Rc<dyn SecondaryWindowDialogs>,
    options: PortOptions,
    log: Arc<dyn Log>,
    state: Rc<PortState>,
    me: Weak<Win32SecondaryWindowPort>,
}

impl Win32SecondaryWindowPort {
    /// C# constructor. Returns an `Rc` because the Settings window needs a `Weak` of the port to reach the Action profiles editor (`Rc::new_cyclic`).
    pub fn new(dialogs: Rc<dyn SecondaryWindowDialogs>, log: Arc<dyn Log>, options: PortOptions) -> Rc<Self> {
        todo!()
    }

    /// C# `OpenCount`: the number of dialogs that are open now (nested ones included; the modeless Benchmark counts).
    pub fn open_count(&self) -> usize {
        todo!()
    }

    /// C# `IsBenchmarkOpen`: the Benchmark window exists and is not closed.
    pub fn is_benchmark_open(&self) -> bool {
        todo!()
    }

    /// C# `SettingsEnabled`: `options.settings_enabled` when given, else `settings::catalog::is_complete()`.
    pub fn settings_enabled(&self) -> bool {
        todo!()
    }

    /// C# `Run(dialog, owner)` (lines 272-290): disposed -> `ID_CANCEL` without showing. Else registers the dialog in `open`, `dialog.show_modal(owner,
    /// ready, message_loop)`, removes it from `open` on every path (also when `show_modal` fails or a hook panics: a guard). `Err(DialogError)` is logged
    /// (`"Secondary window failed"`) and answers `ID_CANCEL` (the C# exception reached the guarded caller, D-W8B5-414). A pending panic is NOT caught here.
    fn run(&self, dialog: &NativeDialog, owner: Hwnd) -> i32 {
        todo!()
    }

    /// `run` + `finish`: the result of a `PreparedDialog`.
    fn run_prepared<T>(&self, prepared: PreparedDialog<T>, owner: Hwnd) -> T {
        todo!()
    }
}

impl std::fmt::Debug for Win32SecondaryWindowPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Win32SecondaryWindowPort").finish_non_exhaustive()
    }
}

impl SecondaryWindowPort for Win32SecondaryWindowPort {
    /// C# `Supports(kind)` (baseline): `true` for Diagnostics, SkippedFiles, BatchReview, ZoomPrompt, Benchmark, ActionProfiles, Recovery; `settings_enabled()`
    /// for Settings; `false` for ExternalEditors and RenamePrompt (not in the baseline, D-W8B5-415).
    fn supports(&self, kind: SecondaryWindowKind) -> bool {
        todo!()
    }

    /// C# `ShowSettings` (lines 90-115): disposed -> `false`; `dialogs.settings(target, opener)` is `None` -> `false`; else `run_prepared` (`finish` reads
    /// `Saved`).
    fn show_settings(&self, owner: Hwnd, target: SettingsTarget) -> bool {
        todo!()
    }

    /// C# `ShowRecovery` (lines 123-160): disposed -> nothing; `dialogs.recovery(owner)` is `None` -> nothing; else `run_prepared`.
    fn show_recovery(&self, owner: Hwnd) {
        todo!()
    }

    /// C# `ShowDiagnostics` (lines 241-252): disposed -> nothing; `dialogs.diagnostics()`; `run_prepared`.
    fn show_diagnostics(&self, owner: Hwnd) {
        todo!()
    }

    /// C# `ShowBenchmark` (lines 181-228): disposed -> nothing. A Benchmark window that is open -> `activate()` it and return. Else `dialogs.benchmark(folder)`
    /// (`None` -> nothing); the dialog is stored as the single instance and registered in `open`; `show_modeless(owner, message_loop, ready, closed)` where
    /// `closed` removes it from `open` and clears the single instance when it is still this one; when `show_modeless` fails the dialog is removed and the
    /// instance cleared (the C# `catch { ...; throw; }`) and the error is logged.
    fn show_benchmark(&self, owner: Hwnd, folder: Option<&Path>) {
        todo!()
    }

    /// C# `ShowSkippedFiles` (lines 254-258).
    fn show_skipped_files(&self, owner: Hwnd, entries: &[SkippedEntry]) {
        todo!()
    }

    /// C# `ShowBatchReview` (lines 260-264): `true` only for `ID_OK`; disposed -> `false`.
    fn show_batch_review(&self, owner: Hwnd, items: &[BatchReviewItem]) -> bool {
        todo!()
    }

    /// C# `PromptCustomZoom` (lines 266-270): `Some(value)` for `ID_OK`; disposed or cancelled -> `None`.
    fn prompt_custom_zoom(&self, owner: Hwnd, current_percent: i32) -> Option<i32> {
        todo!()
    }

    /// C# `ShowActionProfiles` (lines 234-239): the edited list for `ID_OK`, else `None`.
    fn show_action_profiles(&self, owner: Hwnd, actions: &[ReviewAction]) -> Option<Vec<ReviewAction>> {
        todo!()
    }

    /// C# `Dispose` (lines 301-314): idempotent. `disposed = true`; snapshot of `open`; `close()` each from the LAST to the first (innermost first; a modeless
    /// dialog removes itself from `open` when it closes, hence the snapshot). After it every command answers as "cancelled".
    fn dispose(&self) {
        todo!()
    }
}

/// C# `SecondaryWindowRouter : ISecondaryWindowPort, IDisposable` (lines 321-374) without the retired `SecondaryWindowFallback`: a window the native port cannot
/// run answers as cancelled (`false`, `None`, nothing happens), which is what the C# did with a null fallback.
pub struct SecondaryWindowRouter {
    native: Rc<dyn SecondaryWindowPort>,
}

impl SecondaryWindowRouter {
    /// C# constructor.
    pub fn new(native: Rc<dyn SecondaryWindowPort>) -> Self {
        Self { native }
    }

    /// C# `Pick(kind)`: the native port when `native.supports(kind)`, else `None`.
    fn pick(&self, kind: SecondaryWindowKind) -> Option<&Rc<dyn SecondaryWindowPort>> {
        todo!()
    }
}

impl std::fmt::Debug for SecondaryWindowRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecondaryWindowRouter").finish_non_exhaustive()
    }
}

impl SecondaryWindowPort for SecondaryWindowRouter {
    /// C# `Supports(kind) => true`.
    fn supports(&self, kind: SecondaryWindowKind) -> bool {
        todo!()
    }

    /// `pick(Settings)` -> `native.show_settings`, else `false`.
    fn show_settings(&self, owner: Hwnd, target: SettingsTarget) -> bool {
        todo!()
    }

    fn show_recovery(&self, owner: Hwnd) {
        todo!()
    }

    fn show_diagnostics(&self, owner: Hwnd) {
        todo!()
    }

    fn show_benchmark(&self, owner: Hwnd, folder: Option<&Path>) {
        todo!()
    }

    fn show_skipped_files(&self, owner: Hwnd, entries: &[SkippedEntry]) {
        todo!()
    }

    /// `pick(BatchReview)` -> `native.show_batch_review`, else `false`.
    fn show_batch_review(&self, owner: Hwnd, items: &[BatchReviewItem]) -> bool {
        todo!()
    }

    /// `pick(ZoomPrompt)` -> `native.prompt_custom_zoom`, else `None`.
    fn prompt_custom_zoom(&self, owner: Hwnd, current_percent: i32) -> Option<i32> {
        todo!()
    }

    /// `pick(ActionProfiles)` -> `native.show_action_profiles`, else `None`.
    fn show_action_profiles(&self, owner: Hwnd, actions: &[ReviewAction]) -> Option<Vec<ReviewAction>> {
        todo!()
    }

    /// C# `Dispose() => (_native as IDisposable)?.Dispose()`.
    fn dispose(&self) {
        todo!()
    }
}

/// C# `TryReadRecoveryEntries(read, showError)` (lines 163-175, `internal static`, RV-A14): `Ok(entries)` -> `Some(entries)`; an `io::Error` (the C#
/// `IOException` and `UnauthorizedAccessException`) -> `log.error("Could not read the operation journal for the Recovery window", Some(&error))`, then
/// `show_error(&tr::recovery_title(), &tr::recovery_read_failed(error.to_string()))` and `None`. For the production [`SecondaryWindowDialogs::recovery`].
pub fn try_read_recovery_entries<E>(
    read: impl FnOnce() -> Result<Vec<E>, std::io::Error>,
    log: &dyn Log,
    show_error: &dyn Fn(&str, &str),
) -> Option<Vec<E>> {
    todo!()
}
