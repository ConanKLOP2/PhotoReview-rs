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
// The step commands are named like the C# members (`next`, `previous`, ...); they are inherent methods, not `Iterator::next`.
#![allow(clippy::should_implement_trait)]

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use pr_core::catalog::drag_drop::parse;
use pr_core::file_actions::journal_line::JournalDateTime;
use pr_core::instance::ownership::{FolderOpenDecision, FolderOwnership, FolderOwnershipError};
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::{get_file_name, trim_ending_directory_separator};
use pr_core::text::eq_ignore_case;
use pr_core::time::DateTimeUtc;
use pr_l10n::template::LocArg;

use super::state::{ChangeSet, MainEvent, MainViewModel};
use crate::coordinators::folder_load::state::{FolderLoadCx, FolderLoadEvent, FolderLoadStep, FolderLoadUpdate};
use crate::coordinators::presenter::state::PresentRequest;
use crate::coordinators::sibling_navigator::{SiblingNavError, SiblingSearch};

/// C# `ArgumentException.ThrowIfNullOrWhiteSpace(folder)` of `OpenFolderCoreAsync`: the only synchronous failure of an open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFolderError {
    /// The folder is empty or whitespace-only. Nothing happened.
    BlankFolder,
}

impl fmt::Display for OpenFolderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The .NET message of `ArgumentException.ThrowIfNullOrWhiteSpace`.
            Self::BlankFolder => f.write_str(
                "The value cannot be an empty string or composed entirely of whitespace. (Parameter 'folder')",
            ),
        }
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

/// Which catalog index a step command presents (the pure part of `Next` / `Previous` / `First` / `Last`).
fn step_target(command: &Deferred, current: i32, count: i32) -> i32 {
    match command {
        Deferred::Previous => (current - 1).max(0),
        Deferred::First => 0,
        Deferred::Last => count - 1,
        // `Next` (and `Skip`, which advances like it): the last image stays.
        _ => (current + 1).min(count - 1),
    }
}

impl MainViewModel {
    // ---- capture group and compare ----

    /// C# `ToggleCaptureGroupMemberAsync()` (line 327). Parks when the Explorer order is pending. Then: no-op when Compare is visible
    /// or the current entry has no capture group; `current = presenter.current_presented_path() or else entry.path`; the next path is the
    /// RAW path when `current` equals the group's JPEG path (OrdinalIgnoreCase), else the JPEG path; `clock.next_interaction()`;
    /// event status cleared; `present(current_index, allow_compare: false, path_override: Some(next))`; `main.CapturePairBadge`
    /// is added (the presenter's `CURRENT_PATH` bit adds it again when the present completes).
    pub fn toggle_capture_group_member(&mut self) -> ChangeSet {
        if self.park_if_order_pending(Deferred::ToggleCaptureMember) {
            return ChangeSet::default();
        }
        self.run_deferred(Deferred::ToggleCaptureMember)
    }

    fn toggle_capture_group_member_core(&mut self) -> ChangeSet {
        if self.presenter.compare().is_visible() {
            return ChangeSet::default();
        }
        let Some(entry) = self.catalog.current() else {
            return ChangeSet::default();
        };
        let Some(group) = entry.capture_group.as_ref() else {
            return ChangeSet::default();
        };
        let current_path = self.presenter.current_presented_path().unwrap_or_else(|| entry.path());
        let next_path = if eq_ignore_case(current_path, group.jpeg_path()) {
            group.raw_path().to_owned()
        } else {
            group.jpeg_path().to_owned()
        };
        self.clock.next_interaction();
        self.status_text.clear();
        let mut result = self.present_request(PresentRequest {
            index: self.catalog.current_index(),
            allow_compare: false,
            path_override: Some(next_path),
            include_capture_group_in_compare: false,
        });
        // The badge names the displayed member; do not rely on the presenter's status hook to refresh it.
        result.main.insert(super::state::MainProperty::CapturePairBadge);
        result
    }

    /// C# `ToggleCompare()` (line 343): `enable = !compare.is_visible()`; `compare.set_visible(enable)`; when `current_index >= 0`:
    /// `present(current_index, allow_compare: enable, path_override: presented_path_of_current_entry(), include_capture_group_in_compare:
    /// enable)`. Not parked by the Explorer order (as C#). The returned set holds the compare changes.
    pub fn toggle_compare(&mut self) -> ChangeSet {
        let enable = !self.presenter.compare().is_visible();
        let mut result = ChangeSet {
            compare: self.presenter.compare_mut().set_visible(enable),
            ..ChangeSet::default()
        };
        if self.catalog.current_index() >= 0 {
            // The presented path can be stale (catalog changed since); `present` rejects an override that is not a member of the
            // selected entry, so it is only passed when it really belongs to the current index.
            let path_override = self.presented_path_of_current_entry();
            result.merge(self.present_request(PresentRequest {
                index: self.catalog.current_index(),
                allow_compare: enable,
                path_override,
                include_capture_group_in_compare: enable,
            }));
        }
        result
    }

    // ---- opening folders ----

    /// C# `OpenFolderAsync(folder, initialPath = null)` (line 378) = `OpenFolderCoreAsync(.., requiredEpoch: null)`.
    /// `Err(BlankFolder)` for an empty or whitespace-only folder (before anything else, even when the window is closed).
    pub fn open_folder(&mut self, folder: &str, initial_path: Option<&str>) -> Result<ChangeSet, OpenFolderError> {
        self.open_folder_core(folder, initial_path, None, OpenPurpose::User)
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
        if folder.trim().is_empty() {
            return Err(OpenFolderError::BlankFolder);
        }
        // A forwarded open, drop or undo that reaches a window already closed is dropped.
        if self.is_closed {
            return Ok(self.complete_without_load(folder, purpose));
        }
        if required_epoch.is_some_and(|required| required != self.folder_load_epoch) {
            return Ok(self.complete_without_load(folder, purpose));
        }
        let Some(ownership) = self.folder_ownership.clone() else {
            return Ok(self.start_load(folder, initial_path, purpose));
        };
        // R14: read BEFORE the negotiation is submitted, compared with `!=` when it comes back.
        let request = OwnershipRequest {
            folder: folder.to_owned(),
            initial_path: initial_path.map(str::to_owned),
            epoch_at_request: self.folder_load_epoch,
            purpose,
        };
        let inbox = self.inbox.clone();
        self.pending_worker_jobs += 1;
        // `before_open` can block on a named pipe: it runs on a worker, never on the UI thread.
        self.runner.run(
            "folder-ownership",
            Box::new(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    ownership.before_open(&request.folder, request.initial_path.as_deref(), &CancelToken::none())
                }))
                .unwrap_or(Err(FolderOwnershipError::Canceled));
                inbox.send(MainEvent::Ownership { request, result });
            }),
        );
        Ok(ChangeSet::default())
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
        self.pending_worker_jobs = self.pending_worker_jobs.saturating_sub(1);
        let OwnershipRequest {
            folder,
            initial_path,
            epoch_at_request,
            purpose,
        } = request;
        let decision = match result {
            Ok(decision) => decision,
            Err(error) => {
                self.log.error("The folder ownership check failed", Some(&error));
                return self.complete_without_load(&folder, purpose);
            }
        };
        let superseded = epoch_at_request != self.folder_load_epoch;
        let proceed = decision == FolderOpenDecision::Proceed;
        if self.is_closed || (superseded && proceed) {
            // The window closed (or the user opened another folder, R09) while ownership was being negotiated: do not load.
            // Keep the AfterOpen contract for a granted open.
            if proceed {
                self.after_open(&folder);
            }
            return self.complete_without_load(&folder, purpose);
        }
        if !proceed {
            if superseded {
                // The newer open owns the status line.
                return self.complete_without_load(&folder, purpose);
            }
            let name = get_file_name(trim_ending_directory_separator(&folder)).to_owned();
            let key = if decision == FolderOpenDecision::ForwardedToOtherInstance {
                "status.folderOpenedInOtherWindow"
            } else {
                "status.folderOpenInOtherWindowNoResponse"
            };
            let text = self.with_ctx(|ctx| {
                ctx.texts
                    .format_with(key, &[LocArg::new("folder", name.as_str())], ctx.numbers)
            });
            let mut changes = self.set_status_text(&text);
            changes.merge(self.notify_navigation_state_changed());
            changes.merge(self.complete_without_load(&folder, purpose));
            return changes;
        }
        self.start_load(&folder, initial_path.as_deref(), purpose)
    }

    /// C# lines 421 to 436: `folder_load_epoch += 1` (this is the open's epoch), event status cleared, `presenter.reset_file_identity()`,
    /// then `folder_coordinator.load(cx, folder, initial_path)` (the coordinator supersedes the previous load: a previous
    /// `pending_open` completes right after this call, as `None`-less "finished"). A `FolderLoadError` from `load` is logged and the
    /// open completes with `None`. The recorded sink calls are replayed (`apply_sink_actions`). A `Finished` step completes the
    /// open now with `Some(epoch)`; `Pending` stores the [`PendingOpen`].
    pub(super) fn start_load(&mut self, folder: &str, initial_path: Option<&str>, purpose: OpenPurpose) -> ChangeSet {
        self.folder_load_epoch += 1;
        let epoch = self.folder_load_epoch;
        self.status_text.clear();
        // feat/image-crossfade: the new folder's first image must never fade in from whatever the previous folder left on screen.
        self.presenter.reset_file_identity();
        // The coordinator supersedes the active load inside `load`: no `Finished` ever arrives for it, so its open ends here.
        let superseded = self.pending_open.take();
        let open = PendingOpen {
            epoch,
            folder: folder.to_owned(),
            purpose,
        };
        let loaded = {
            let mut cx = FolderLoadCx {
                catalog: &mut self.catalog,
                sink: &mut self.sink,
            };
            self.folder_coordinator.load(&mut cx, folder, initial_path)
        };
        let mut result = ChangeSet::default();
        let finished_now = match loaded {
            Ok(update) => {
                let finished = update.step == FolderLoadStep::Finished;
                if !finished {
                    self.pending_open = Some(open.clone());
                }
                result.merge(self.apply_sink_actions());
                result.merge(self.resume_deferred(update));
                finished
            }
            Err(error) => {
                self.log.error("The folder load could not start", Some(&error));
                result.merge(self.apply_sink_actions());
                // The C# `finally` still runs for a load task that faulted at once.
                self.after_open(&open.folder);
                result.merge(self.finish_open(open.clone(), None));
                false
            }
        };
        if finished_now {
            result.merge(self.finish_open(open, Some(epoch)));
        }
        if let Some(old) = superseded {
            let old_epoch = old.epoch;
            result.merge(self.finish_open(old, Some(old_epoch)));
        }
        result
    }

    /// The open ended before any load started (closed window, `required_epoch` mismatch, forwarded, refused, superseded while
    /// negotiating, ownership error): only the `purpose` follow-up runs, with `epoch = None`.
    fn complete_without_load(&mut self, folder: &str, purpose: OpenPurpose) -> ChangeSet {
        self.finish_open(
            PendingOpen {
                epoch: 0,
                folder: folder.to_owned(),
                purpose,
            },
            None,
        )
    }

    /// C# `ownership?.AfterOpen(folder, _shownFolder)`.
    fn after_open(&self, folder: &str) {
        if let Some(ownership) = &self.folder_ownership {
            ownership.after_open(folder, self.shown_folder.as_deref());
        }
    }

    /// C# `finally { ownership?.AfterOpen(folder, _shownFolder) }`, then `NotifyNavigationStateChanged()`, then the `purpose`
    /// follow-up: `User` nothing; `UndoReopen`: `epoch == Some(folder_load_epoch)` and a non-empty `error_message` -> the event status is
    /// set to it; `DuplicateReload`: `duplicate_controller.on_reload_finished(cx, epoch.is_none() || epoch == Some(folder_load_epoch))`.
    /// `epoch` is `None` when nothing was loaded (closed window, forwarded, refused, superseded while negotiating): then neither the
    /// `finally` nor the notification of a started load runs, only the follow-up.
    pub(super) fn finish_open(&mut self, open: PendingOpen, epoch: Option<i64>) -> ChangeSet {
        let mut result = ChangeSet::default();
        if epoch.is_some() {
            self.after_open(&open.folder);
            result.merge(self.notify_navigation_state_changed());
        }
        match open.purpose {
            OpenPurpose::User => {}
            OpenPurpose::UndoReopen { error_message } => {
                result.merge(self.on_undo_reopen_finished(epoch, error_message));
            }
            OpenPurpose::DuplicateReload => {
                let still_current = epoch.is_none() || epoch == Some(self.folder_load_epoch);
                result.merge(self.on_duplicate_reload_finished(still_current));
            }
        }
        result
    }

    /// Delivers a folder-load worker event: `folder_coordinator.handle_event(cx, event)`, replays the recorded sink calls, finishes
    /// the [`PendingOpen`] on `FolderLoadStep::Finished`, and resumes the deferred commands when `order_gate_released` is set.
    pub(super) fn handle_folder_event(&mut self, event: FolderLoadEvent) -> ChangeSet {
        let update = {
            let mut cx = FolderLoadCx {
                catalog: &mut self.catalog,
                sink: &mut self.sink,
            };
            self.folder_coordinator.handle_event(&mut cx, event)
        };
        let mut result = self.apply_sink_actions();
        if update.step == FolderLoadStep::Finished {
            if let Some(open) = self.pending_open.take() {
                let epoch = open.epoch;
                result.merge(self.finish_open(open, Some(epoch)));
            }
        }
        result.merge(self.resume_deferred(update));
        result
    }

    /// Releases the Explorer-order gate effect of an update (`update.order_gate_released`): runs the parked commands in submission
    /// order (each re-checks `pending_order()`; one that parks again stops the loop).
    pub(super) fn resume_deferred(&mut self, update: FolderLoadUpdate) -> ChangeSet {
        let mut result = ChangeSet::default();
        if !update.order_gate_released {
            return result;
        }
        while let Some(command) = self.deferred.pop_front() {
            if self.folder_coordinator.pending_order() {
                // A newer load opened a new gate: the command waits for it, at the front (submission order).
                self.deferred.push_front(command);
                break;
            }
            result.merge(self.run_deferred(command));
        }
        result
    }

    /// Parks `command` and returns true when `folder_coordinator.pending_order()` (C# `WaitForPendingExplorerOrderAsync` would
    /// await). Completes synchronously (returns false) when nothing is pending, as C#. A command submitted while others are parked
    /// queues behind them (FIFO), even when the order was settled in between.
    pub(super) fn park_if_order_pending(&mut self, command: Deferred) -> bool {
        if self.folder_coordinator.pending_order() || !self.deferred.is_empty() {
            self.deferred.push_back(command);
            return true;
        }
        false
    }

    /// Runs one parked command (the body that follows the C# `await WaitForPendingExplorerOrderAsync()`).
    pub(super) fn run_deferred(&mut self, command: Deferred) -> ChangeSet {
        match command {
            Deferred::ToggleCaptureMember => self.toggle_capture_group_member_core(),
            Deferred::Skip => self.skip_core(),
            Deferred::FileTurn { ticket } => self.resume_file_turn(ticket),
            step => self.step_core(&step),
        }
    }

    /// C# `FolderOwnership` getter (Q-R18).
    pub fn folder_ownership(&self) -> Option<&Arc<dyn FolderOwnership>> {
        self.folder_ownership.as_ref()
    }

    /// C# `FolderOwnership` setter: `None` = no instance checks (tests, benchmark host).
    pub fn set_folder_ownership(&mut self, ownership: Option<Arc<dyn FolderOwnership>>) {
        self.folder_ownership = ownership;
    }

    /// C# `_folderLoadEpoch` (R02): bumped each time a folder load starts.
    pub fn folder_load_epoch(&self) -> i64 {
        self.folder_load_epoch
    }

    /// C# `_shownFolder` (Q-R18): the folder whose catalog this window shows.
    pub fn shown_folder(&self) -> Option<&str> {
        self.shown_folder.as_deref()
    }

    /// C# `OnFolderShown(folder)` (line 448): `shown_folder = folder`, `ownership.on_folder_shown(folder)`. Called by the replay of
    /// `on_catalog_ready` / `on_empty` / `on_empty_with_subfolders` (W8-08e).
    pub(super) fn on_folder_shown(&mut self, folder: &str) {
        self.shown_folder = Some(folder.to_owned());
        if let Some(ownership) = &self.folder_ownership {
            ownership.on_folder_shown(folder);
        }
    }

    /// C# `OpenPathAsync(path)` (line 457): a blank path does nothing. `pr_core::catalog::drag_drop::parse(Some(&[path]), raw, webp_heic,
    /// file_system)` with the CURRENT settings; invalid input sets the event status to the localized warning
    /// (`warning.catalog_key()`) and stops; a valid result with a warning first sets the status to it; then
    /// `open_folder(folder_path, initial_image_path)`. NOTE (C# quirk, kept): the open clears the event status when its load starts,
    /// so a warning on a valid input is visible only when the load does not start (ownership forward / refusal).
    pub fn open_path(&mut self, path: &str) -> ChangeSet {
        if path.trim().is_empty() {
            return ChangeSet::default();
        }
        let settings = self.settings();
        let input = parse(
            Some(&[path.to_owned()]),
            settings.raw_support_enabled,
            settings.webp_heic_support_enabled,
            self.file_system.as_ref(),
        );
        let warning = input.warning.map(|warning| {
            let key = warning.catalog_key();
            self.with_ctx(|ctx| ctx.texts.get(key))
        });
        let mut result = ChangeSet::default();
        let folder_path = match input.folder_path.as_deref() {
            Some(folder) if input.is_valid() => folder.to_owned(),
            _ => {
                let text = warning.unwrap_or_else(|| self.with_ctx(|ctx| ctx.texts.get("status.noValidInput")));
                return self.set_status_text(&text);
            }
        };
        if let Some(text) = warning {
            result.merge(self.set_status_text(&text));
        }
        match self.open_folder(&folder_path, input.initial_image_path.as_deref()) {
            Ok(changes) => result.merge(changes),
            Err(error) => self.log.error("Could not open the dropped path", Some(&error)),
        }
        result
    }

    // ---- stepping through the catalog ----

    /// C# `NextAsync()` (line 479). Parks while the Explorer order is pending. No-op for an empty catalog. `clock.next_interaction()`;
    /// `next = min(current + 1, count - 1)`; event status cleared; `present(next)`. No explicit navigation-state notification
    /// (the presenter drives it, R05).
    pub fn next(&mut self) -> ChangeSet {
        self.step(Deferred::Next)
    }

    /// C# `PreviousAsync()` (line 498): like `next` with `max(current - 1, 0)`.
    pub fn previous(&mut self) -> ChangeSet {
        self.step(Deferred::Previous)
    }

    /// C# `FirstAsync()` (line 512): like `next`, target index 0.
    pub fn first(&mut self) -> ChangeSet {
        self.step(Deferred::First)
    }

    /// C# `LastAsync()` (line 525): like `next`, target index `count - 1`.
    pub fn last(&mut self) -> ChangeSet {
        self.step(Deferred::Last)
    }

    fn step(&mut self, command: Deferred) -> ChangeSet {
        if self.park_if_order_pending(command.clone()) {
            return ChangeSet::default();
        }
        self.step_core(&command)
    }

    fn step_core(&mut self, command: &Deferred) -> ChangeSet {
        let Ok(count) = i32::try_from(self.catalog.count()) else {
            return ChangeSet::default();
        };
        if count == 0 {
            return ChangeSet::default();
        }
        self.clock.next_interaction();
        let target = step_target(command, self.catalog.current_index(), count);
        self.status_text.clear();
        // R05: no explicit `notify_navigation_state_changed()` -- the presenter drives it through the bridge.
        self.present_request(PresentRequest::new(target))
    }

    /// C# `SkipAsync()` (line 538). Parks while the Explorer order is pending. No-op unless `0 <= current < count`.
    /// `clock.next_interaction()`; with a session: the current path is appended to `session.skipped` unless already present
    /// (OrdinalIgnoreCase, R2-F-10), `session.updated_utc = now (Utc)`, `persist_session()`; then `next` semantics (index
    /// `min(current + 1, count - 1)`, event status cleared, `present`).
    pub fn skip(&mut self) -> ChangeSet {
        if self.park_if_order_pending(Deferred::Skip) {
            return ChangeSet::default();
        }
        self.skip_core()
    }

    fn skip_core(&mut self) -> ChangeSet {
        let current = self.catalog.current_index();
        let Some(index) = usize::try_from(current)
            .ok()
            .filter(|index| *index < self.catalog.count())
        else {
            return ChangeSet::default();
        };
        self.clock.next_interaction();
        let current_path = self.catalog.path_at(index).to_owned();
        if let Some(session) = self.current_session.as_mut() {
            // R2-F-10: no duplicates -- skipping the same image again must not grow the persisted session file.
            if !session
                .skipped
                .iter()
                .any(|skipped| eq_ignore_case(skipped, &current_path))
            {
                session.skipped.push(current_path);
            }
            session.updated_utc = JournalDateTime::utc(DateTimeUtc::now());
            self.persist_session();
        }
        self.step_core(&Deferred::Next)
    }

    // ---- sibling folder ----

    /// C# `NavigateSiblingFolderAsync(direction)` (line 563) = `SiblingFolderNavigator.NavigateSiblingFolderAsync`:
    /// `sibling_navigator.begin_navigate(direction, session folder, current catalog path)` on the UI thread (`Err` for a direction
    /// other than -1 / +1, `Ok(None)` for "nothing to do"), then a worker job `find_next_image_folder(&search)` that sends
    /// `MainEvent::SiblingFound`.
    pub fn navigate_sibling_folder(&mut self, direction: i32) -> Result<ChangeSet, SiblingNavError> {
        let session_folder = self.current_session.as_ref().map(|session| session.folder.as_str());
        let current_path = self.catalog.current().map(|entry| entry.path());
        let Some(search) = self
            .sibling_navigator
            .begin_navigate(direction, session_folder, current_path)?
        else {
            return Ok(ChangeSet::default());
        };
        let navigator = Arc::clone(&self.sibling_navigator);
        let inbox = self.inbox.clone();
        self.pending_worker_jobs += 1;
        // The directory listing blocks: it runs on a worker and touches no view-model state.
        self.runner.run(
            "sibling-folder-search",
            Box::new(move || {
                let result = crate::coordinators::zoom_detail::catch_job(
                    || navigator.find_next_image_folder(&search),
                    |text| Err(SiblingNavError::Panicked(text)),
                );
                inbox.send(MainEvent::SiblingFound { search, result });
            }),
        );
        Ok(ChangeSet::default())
    }

    /// Continuation of the sibling search: `sibling_navigator.complete_navigate(&search, target, sink, ctx)`; the recorded
    /// `open_folder` / status is replayed. A worker `Err` is logged (the C# task would fault), nothing changes.
    pub(super) fn on_sibling_found(
        &mut self,
        search: SiblingSearch,
        result: Result<Option<String>, SiblingNavError>,
    ) -> ChangeSet {
        self.pending_worker_jobs = self.pending_worker_jobs.saturating_sub(1);
        let target = match result {
            Ok(target) => target,
            Err(error) => {
                self.log.error("The sibling folder search failed", Some(&error));
                return ChangeSet::default();
            }
        };
        let navigator = Arc::clone(&self.sibling_navigator);
        let format = Arc::clone(&self.format);
        let sink = &mut self.sink;
        format.with_context(&mut |ctx| {
            navigator.complete_navigate(&search, target.as_deref(), sink, ctx);
        });
        self.apply_sink_actions()
    }

    /// C# `Compare.SelectLeft()` (`CompareViewModel.SelectLeft` = `SelectedPath = LeftPath`), called by the shell on a click on the left
    /// compare cell (`ShellMainWindow.Viewing.cs` `TrySelectCompareAt`). Only the compare view model changes: the returned set holds
    /// the compare bits (`selected_path` when it differs).
    pub fn select_compare_left(&mut self) -> ChangeSet {
        ChangeSet {
            compare: self.presenter.compare_mut().select_left(),
            ..ChangeSet::default()
        }
    }

    /// C# `Compare.SelectRight()` (`SelectedPath = RightPath`); see [`MainViewModel::select_compare_left`].
    pub fn select_compare_right(&mut self) -> ChangeSet {
        ChangeSet {
            compare: self.presenter.compare_mut().select_right(),
            ..ChangeSet::default()
        }
    }

    /// Test seam: the C# tests hold the `ReviewCatalog` object and move its current index behind the presenter's back
    /// (`_catalog.SetCurrent`, `_catalog.Reset`). The view model owns the catalog here, so those tests use this accessor.
    /// Never called by production code.
    #[doc(hidden)]
    pub fn catalog_mut_for_tests(&mut self) -> &mut pr_core::catalog::review_catalog::ReviewCatalog {
        &mut self.catalog
    }

    /// Test seam: the C# tests drive the compare view model directly (`vm.Compare.Select(path)`); here it lives inside the presenter.
    /// Never called by production code (the shell selects through the presenter's own commands).
    #[doc(hidden)]
    pub fn compare_mut_for_tests(&mut self) -> &mut crate::viewmodels::compare::CompareViewModel {
        self.presenter.compare_mut()
    }
}
