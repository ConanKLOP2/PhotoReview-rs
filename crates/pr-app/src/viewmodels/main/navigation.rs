//! Port of `PhotoReview.App.Shared/ViewModels/MainViewModel.cs` lines 327 to 564 (baseline `d92d6aae`): `ToggleCaptureGroupMemberAsync`,
//! `ToggleCompare`, `OpenFolderAsync` / `OpenFolderCoreAsync`, `FolderOwnership`, `OnFolderShown`, `OpenPathAsync`, `NextAsync`,
//! `PreviousAsync`, `FirstAsync`, `LastAsync`, `SkipAsync`, `NavigateSiblingFolderAsync` (contract `contracts/W8-08b.md`).
//! Target: `crates/pr-app/src/viewmodels/main/navigation.rs` (`viewmodels::main::navigation`).
//!
//! D-W8-2: every C# `async Task` here becomes a synchronous method that returns the [`ChangeSet`] of its synchronous part. What C#
//! `await`ed comes back as a [`MainEvent`] applied by [`MainViewModel::pump`](super::state::MainViewModel::pump):
//! - `await WaitForPendingExplorerOrderAsync()` becomes a parked [`Deferred`] command, resumed in submission order when the folder
//!   coordinator reports `order_gate_released` (INV-9);
//! - `await ownership.BeforeOpenAsync` becomes a worker job and `MainEvent::Ownership`;
//! - `await FolderLoadTask` becomes the [`PendingOpen`] record, finished by the `FolderLoadStep::Finished` update;
//! - `await Task.Run(find sibling folder)` becomes a worker job and `MainEvent::SiblingFound`.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]
// Stub only (contract text in doc comments, boxed seams): the porter fixes the doc layout / introduces type aliases and removes this line.
#![allow(clippy::should_implement_trait)]

use std::fmt;
use std::sync::Arc;

use pr_core::catalog::drag_drop::DragDropInputResult;
use pr_core::instance::ownership::{FolderOpenDecision, FolderOwnership, FolderOwnershipError};

use super::state::{ChangeSet, MainEvent, MainViewModel};
use crate::coordinators::folder_load::state::{FolderLoadEvent, FolderLoadUpdate};
use crate::coordinators::sibling_navigator::{SiblingNavError, SiblingSearch};

/// C# `ArgumentException.ThrowIfNullOrWhiteSpace(folder)` of `OpenFolderCoreAsync`: the only synchronous failure of an open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFolderError {
    /// The folder is empty or whitespace-only. Nothing happened.
    BlankFolder,
}

impl fmt::Display for OpenFolderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for OpenFolderError {}

/// Who started an open, i.e. what the C# caller did with the `long?` epoch that `OpenFolderCoreAsync` returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenPurpose {
    /// `OpenFolderAsync`, drop, command line, sibling navigation: the result is ignored.
    User,
    /// Undo reopening the source folder (`UndoCoreAsync`): when the open loaded and no other load started since, a non-empty
    /// `error_message` of the undo is put back as the status (the reload wrote its own).
    UndoReopen { error_message: Option<String> },
    /// `IDuplicateCleanupSink.ReloadFolderAsync`: when the open finishes, `duplicate_controller.on_reload_finished(.., still_current)`
    /// with `still_current = epoch.is_none() || epoch == folder_load_epoch`.
    DuplicateReload,
}

/// The open whose load is running (the Rust image of `await FolderLoadTask` inside `OpenFolderCoreAsync`).
#[derive(Debug, Clone)]
pub(super) struct PendingOpen {
    /// C# `loadEpoch` of this open.
    pub epoch: i64,
    /// The folder as passed to the open (C# `folder`, for `ownership.AfterOpen`).
    pub folder: String,
    pub purpose: OpenPurpose,
}

/// What the ownership worker needs to hand back with its decision (C# locals `folder`, `initialPath`, `epochAtRequest`).
#[derive(Debug, Clone)]
pub struct OwnershipRequest {
    pub folder: String,
    pub initial_path: Option<String>,
    /// C# `epochAtRequest = _folderLoadEpoch` read before the negotiation.
    pub epoch_at_request: i64,
    pub purpose: OpenPurpose,
}

/// A command that C# `await`ed `WaitForPendingExplorerOrderAsync()` first and is now parked (INV-9). Resumed in submission order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Deferred {
    ToggleCaptureMember,
    Next,
    Previous,
    First,
    Last,
    Skip,
    /// A queued file command (`commands`) whose gate turn arrived while the order was pending; the record is in
    /// `queued_file_commands[ticket]`.
    FileTurn {
        ticket: u64,
    },
}

impl MainViewModel {
    // ---- capture group and compare ----

    /// C# `ToggleCaptureGroupMemberAsync()` (line 327). Parks when the Explorer order is pending. Then: no-op when Compare is visible
    /// or the current entry has no capture group; `current = presenter.current_presented_path() or else entry.path`; the next path is the
    /// RAW path when `current` equals the group's JPEG path (OrdinalIgnoreCase), else the JPEG path; `clock.next_interaction()`;
    /// event status cleared; `present(current_index, allow_compare: false, path_override: Some(next))`; `main.CapturePairBadge`
    /// is added (the presenter's `CURRENT_PATH` bit adds it again when the present completes).
    pub fn toggle_capture_group_member(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `ToggleCompare()` (line 343): `enable = !compare.is_visible()`; `compare.set_visible(enable)`; when `current_index >= 0`:
    /// `present(current_index, allow_compare: enable, path_override: presented_path_of_current_entry(), include_capture_group_in_compare:
    /// enable)`. Not parked by the Explorer order (as C#). The returned set holds the compare changes.
    pub fn toggle_compare(&mut self) -> ChangeSet {
        todo!()
    }

    // ---- opening folders ----

    /// C# `OpenFolderAsync(folder, initialPath = null)` (line 378) = `OpenFolderCoreAsync(.., requiredEpoch: null)`.
    /// `Err(BlankFolder)` for an empty or whitespace-only folder (before anything else, even when the window is closed).
    pub fn open_folder(&mut self, folder: &str, initial_path: Option<&str>) -> Result<ChangeSet, OpenFolderError> {
        todo!()
    }

    /// C# `OpenFolderCoreAsync(folder, initialPath, requiredEpoch)` (line 385), with the caller's follow-up carried in `purpose`.
    /// Order: (1) blank folder -> `Err`; (2) `is_closed` -> the open completes at once with `None` (no ownership call, no load);
    /// (3) `required_epoch` set and different from `folder_load_epoch` -> completes with `None`; (4) no ownership -> `start_load`;
    /// (5) with ownership: a worker job calls `before_open(folder, initial_path, none)` and sends `MainEvent::Ownership`
    /// (`epoch_at_request` read first). "Completes with `None`" runs the `purpose` follow-up with `epoch = None` (see
    /// [`finish_open`](Self::finish_open)).
    pub(super) fn open_folder_core(
        &mut self,
        folder: &str,
        initial_path: Option<&str>,
        required_epoch: Option<i64>,
        purpose: OpenPurpose,
    ) -> Result<ChangeSet, OpenFolderError> {
        todo!()
    }

    /// Continuation after the ownership negotiation (C# lines 399 to 419), UI thread. A worker `Err` (canceled / invalid folder) is
    /// logged and completes the open with `None`. Otherwise `superseded = epoch_at_request != folder_load_epoch`:
    /// - `is_closed`, or (`superseded` and `Proceed`): when `Proceed`, `ownership.after_open(folder, shown_folder)` (the AfterOpen
    ///   contract of a granted open); the open completes with `None`;
    /// - not `Proceed`: when `superseded` complete with `None` (the newer open owns the status line); else the event status becomes
    ///   `status.folderOpenedInOtherWindow` (`ForwardedToOtherInstance`) or `status.folderOpenInOtherWindowNoResponse` (`OwnedByOtherInstance`)
    ///   with `{folder}` = the folder's last path segment, `notify_navigation_state_changed()`, complete with `None`;
    /// - `Proceed` and not superseded: `start_load`.
    pub(super) fn on_ownership_decided(
        &mut self,
        request: OwnershipRequest,
        result: Result<FolderOpenDecision, FolderOwnershipError>,
    ) -> ChangeSet {
        todo!()
    }

    /// C# lines 421 to 436: `folder_load_epoch += 1` (this is the open's epoch), event status cleared, `presenter.reset_file_identity()`,
    /// then `folder_coordinator.load(cx, folder, initial_path)` (the coordinator supersedes the previous load: a previous
    /// `pending_open` completes right after this call, as `None`-less "finished"). A `FolderLoadError` from `load` is logged and the
    /// open completes with `None`. The recorded sink calls are replayed (`apply_sink_actions`). A `Finished` step completes the
    /// open now with `Some(epoch)`; `Pending` stores the [`PendingOpen`].
    pub(super) fn start_load(&mut self, folder: &str, initial_path: Option<&str>, purpose: OpenPurpose) -> ChangeSet {
        todo!()
    }

    /// C# `finally { ownership?.AfterOpen(folder, _shownFolder) }`, then `NotifyNavigationStateChanged()`, then the `purpose`
    /// follow-up: `User` nothing; `UndoReopen`: `epoch == Some(folder_load_epoch)` and a non-empty `error_message` -> the event status is
    /// set to it; `DuplicateReload`: `duplicate_controller.on_reload_finished(cx, epoch.is_none() || epoch == Some(folder_load_epoch))`.
    /// `epoch` is `None` when nothing was loaded (closed window, forwarded, refused, superseded while negotiating).
    pub(super) fn finish_open(&mut self, open: PendingOpen, epoch: Option<i64>) -> ChangeSet {
        todo!()
    }

    /// Delivers a folder-load worker event: `folder_coordinator.handle_event(cx, event)`, replays the recorded sink calls, finishes
    /// the [`PendingOpen`] on `FolderLoadStep::Finished`, and resumes the deferred commands when `order_gate_released` is set.
    pub(super) fn handle_folder_event(&mut self, event: FolderLoadEvent) -> ChangeSet {
        todo!()
    }

    /// Releases the Explorer-order gate effect of an update (`update.order_gate_released`): runs the parked commands in submission
    /// order (each re-checks `pending_order()`; one that parks again stops the loop).
    pub(super) fn resume_deferred(&mut self, update: FolderLoadUpdate) -> ChangeSet {
        todo!()
    }

    /// Parks `command` and returns true when `folder_coordinator.pending_order()` (C# `WaitForPendingExplorerOrderAsync` would
    /// await). Completes synchronously (returns false) when nothing is pending, as C#.
    pub(super) fn park_if_order_pending(&mut self, command: Deferred) -> bool {
        todo!()
    }

    /// Runs one parked command (the body that follows the C# `await WaitForPendingExplorerOrderAsync()`).
    pub(super) fn run_deferred(&mut self, command: Deferred) -> ChangeSet {
        todo!()
    }

    /// C# `FolderOwnership` getter (Q-R18).
    pub fn folder_ownership(&self) -> Option<&Arc<dyn FolderOwnership>> {
        todo!()
    }

    /// C# `FolderOwnership` setter: `None` = no instance checks (tests, benchmark host).
    pub fn set_folder_ownership(&mut self, ownership: Option<Arc<dyn FolderOwnership>>) {
        todo!()
    }

    /// C# `_folderLoadEpoch` (R02): bumped each time a folder load starts.
    pub fn folder_load_epoch(&self) -> i64 {
        todo!()
    }

    /// C# `_shownFolder` (Q-R18): the folder whose catalog this window shows.
    pub fn shown_folder(&self) -> Option<&str> {
        todo!()
    }

    /// C# `OnFolderShown(folder)` (line 448): `shown_folder = folder`, `ownership.on_folder_shown(folder)`. Called by the replay of
    /// `on_catalog_ready` / `on_empty` / `on_empty_with_subfolders` (W8-08e).
    pub(super) fn on_folder_shown(&mut self, folder: &str) {
        todo!()
    }

    /// C# `OpenPathAsync(path)` (line 457): a blank path does nothing. `pr_core::catalog::drag_drop::parse(Some(&[path]), raw, webp_heic,
    /// file_system)` with the CURRENT settings; invalid input sets the event status to the localized warning
    /// (`warning.catalog_key()`) and stops; a valid result with a warning first sets the status to it; then
    /// `open_folder(folder_path, initial_image_path)`. NOTE (C# quirk, kept): the open clears the event status when its load starts,
    /// so a warning on a valid input is visible only when the load does not start (ownership forward / refusal).
    pub fn open_path(&mut self, path: &str) -> ChangeSet {
        todo!()
    }

    // ---- stepping through the catalog ----

    /// C# `NextAsync()` (line 479). Parks while the Explorer order is pending. No-op for an empty catalog. `clock.next_interaction()`;
    /// `next = min(current + 1, count - 1)`; event status cleared; `present(next)`. No explicit navigation-state notification
    /// (the presenter drives it, R05).
    pub fn next(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `PreviousAsync()` (line 498): like `next` with `max(current - 1, 0)`.
    pub fn previous(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `FirstAsync()` (line 512): like `next`, target index 0.
    pub fn first(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `LastAsync()` (line 525): like `next`, target index `count - 1`.
    pub fn last(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `SkipAsync()` (line 538). Parks while the Explorer order is pending. No-op unless `0 <= current < count`.
    /// `clock.next_interaction()`; with a session: the current path is appended to `session.skipped` unless already present
    /// (OrdinalIgnoreCase, R2-F-10), `session.updated_utc = now (Utc)`, `persist_session()`; then `next` semantics (index
    /// `min(current + 1, count - 1)`, event status cleared, `present`).
    pub fn skip(&mut self) -> ChangeSet {
        todo!()
    }

    // ---- sibling folder ----

    /// C# `NavigateSiblingFolderAsync(direction)` (line 563) = `SiblingFolderNavigator.NavigateSiblingFolderAsync`:
    /// `sibling_navigator.begin_navigate(direction, session folder, current catalog path)` on the UI thread (`Err` for a direction
    /// other than -1 / +1, `Ok(None)` for "nothing to do"), then a worker job `find_next_image_folder(&search)` that sends
    /// `MainEvent::SiblingFound`.
    pub fn navigate_sibling_folder(&mut self, direction: i32) -> Result<ChangeSet, SiblingNavError> {
        todo!()
    }

    /// Continuation of the sibling search: `sibling_navigator.complete_navigate(&search, target, sink, ctx)`; the recorded
    /// `open_folder` / status is replayed. A worker `Err` is logged (the C# task would fault), nothing changes.
    pub(super) fn on_sibling_found(
        &mut self,
        search: SiblingSearch,
        result: Result<Option<String>, SiblingNavError>,
    ) -> ChangeSet {
        todo!()
    }
}
