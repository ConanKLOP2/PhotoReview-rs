//! Port of `PhotoReview.App.Shared/Coordinators/FolderLoadCoordinator.cs` lines 270 to 345 and 470 to 544: the late Explorer
//! order (second half of `LoadAsync`), the `catch`/`finally` blocks, `PendingOrder` and `Dispose`
//! (contract `contracts/W8-07b.md`). Target: `coordinators::folder_load::reload`.
//!
//! The plan names this unit "reload", but the baseline has no reload method: a reload is another `load` call
//! (`IDuplicateCleanupSink::reload_folder` forwards to `open_folder`). Nothing here starts a load.

use std::sync::Arc;

use pr_core::catalog::explorer_types::ExplorerViewSnapshot;
use pr_core::catalog::snapshot_validator::try_validate;

use super::state::{
    ExplorerSlot, FolderLoadCoordinator, FolderLoadCx, FolderLoadError, FolderLoadStep, FolderLoadUpdate, LoadPhase,
    ProbeState,
};

fn ignored() -> FolderLoadUpdate {
    FolderLoadUpdate {
        step: FolderLoadStep::Ignored,
        order_gate_released: false,
    }
}

impl FolderLoadCoordinator {
    /// C# `PendingOrder` not completed: a file-open load presented its file before the Explorer snapshot was in and has not
    /// settled the order yet (INV-9). Navigation and file actions must wait until a later update has
    /// `order_gate_released == true`. Always false for a folder open, for app-decided sort modes, and after `dispose`.
    pub fn pending_order(&self) -> bool {
        self.order_gate_open
    }

    /// C# `Dispose`: idempotent. Marks disposed, releases the order gate, cancels the active token. No sink call follows
    /// (events of the active load are dropped). Returns the update (`order_gate_released` is true when a gate was open).
    pub fn dispose(&mut self) -> FolderLoadUpdate {
        if self.disposed {
            return ignored();
        }
        self.disposed = true;
        let order_gate_released = std::mem::take(&mut self.order_gate_open);
        if let Some(active) = self.active.take() {
            active.token.cancel();
        }
        FolderLoadUpdate {
            step: FolderLoadStep::Finished,
            order_gate_released,
        }
    }

    /// Delivers `ExplorerFinished` (UI thread). Stores the snapshot in the active load's slot (a stale `load_id` is dropped,
    /// test `LoadAsync_StaleExplorerSnapshotFromSupersededLoad_IsIgnored`); when the load is in `AwaitingOrder` it continues with
    /// `apply_late_order`, otherwise the scan continuation finds the slot filled.
    pub(super) fn on_explorer_finished(
        &mut self,
        cx: &mut FolderLoadCx<'_>,
        load_id: u64,
        snapshot: Option<Arc<ExplorerViewSnapshot>>,
    ) -> FolderLoadUpdate {
        let Some(active) = self.active.as_mut().filter(|a| a.load_id == load_id) else {
            return ignored();
        };
        match active.phase {
            LoadPhase::Idle => ignored(),
            LoadPhase::Scanning => {
                active.explorer = snapshot.map_or(ExplorerSlot::Failed, ExplorerSlot::Done);
                FolderLoadUpdate::default()
            }
            LoadPhase::AwaitingOrder => {
                active.explorer = snapshot.map_or(ExplorerSlot::Failed, ExplorerSlot::Done);
                self.apply_late_order(cx)
            }
        }
    }

    /// C# lines 273 to 311, run when the snapshot is known and the first image/empty branch is over:
    /// 1. token canceled or `!is_folder_current(load_generation)`: no effect, `finish_load`.
    /// 2. `!is_interaction_current(interaction_generation)`: perf "explorerIgnored" (INV-7), `finish_load`.
    /// 3. `try_validate(snapshot, scanned_files)`: `Err` -> perf "explorerFallback"; `Ok(order)` -> `catalog.replace_order(&order)`:
    ///    false -> "explorerIgnored"; true -> `may_replace = initial_path.is_none() && current_navigation == presentation_generation
    ///    && catalog.count() > 0`, `sink.on_order_applied(order.len(), catalog.current_index(), current_kept: !may_replace)`, perf
    ///    "explorerApplied", and when `may_replace`: `catalog.set_current(0)` then `sink.present(0, clock.current_navigation())`.
    /// 4. `finish_load`. A failed query (`None` snapshot) is the C# exception path: it is reported like any load failure while the
    ///    load is still current (`sink.on_failed`), otherwise logged.
    pub(super) fn apply_late_order(&mut self, cx: &mut FolderLoadCx<'_>) -> FolderLoadUpdate {
        let Some(active) = self.active.as_ref() else {
            return FolderLoadUpdate::default();
        };
        let load_id = active.load_id;
        let load_generation = active.load_generation;
        let interaction_generation = active.interaction_generation;
        let presentation_generation = active.presentation_generation;
        let has_initial_path = active.initial_path.is_some();
        let token = active.token.clone();
        let scanned_files = Arc::clone(&active.scanned_files);
        let snapshot = match &active.explorer {
            ExplorerSlot::Done(snapshot) | ExplorerSlot::Unavailable(snapshot) => Arc::clone(snapshot),
            ExplorerSlot::Failed => {
                // C#: awaiting a faulted task throws.
                let error = FolderLoadError::Io(std::io::Error::other("The Explorer order query failed."));
                self.report_load_error(cx, load_id, &error);
                return self.finish_load(cx, load_id);
            }
            ExplorerSlot::Pending(_) => return FolderLoadUpdate::default(),
        };
        self.perf_mark(load_generation, "explorerAwaited", None);

        if token.is_canceled() || !self.deps.clock.is_folder_current(load_generation) {
            return self.finish_load(cx, load_id);
        }
        if !self.deps.clock.is_interaction_current(interaction_generation) {
            self.perf_mark(load_generation, "explorerIgnored", None);
            return self.finish_load(cx, load_id);
        }

        match try_validate(&snapshot, &scanned_files) {
            Ok(order) => {
                if cx.catalog.replace_order(&order) {
                    let may_replace = !has_initial_path
                        && self.deps.clock.current_navigation() == presentation_generation
                        && cx.catalog.count() > 0;
                    let count = i32::try_from(order.len()).unwrap_or(i32::MAX);
                    cx.sink
                        .on_order_applied(count, cx.catalog.current_index(), !may_replace);
                    self.perf_mark(load_generation, "explorerApplied", None);
                    if may_replace {
                        cx.catalog.set_current(0);
                        cx.sink.present(0, self.deps.clock.current_navigation());
                    }
                } else {
                    self.perf_mark(load_generation, "explorerIgnored", None);
                }
            }
            Err(_) => self.perf_mark(load_generation, "explorerFallback", None),
        }
        self.finish_load(cx, load_id)
    }

    /// C# `finally` block (lines 327 to 344), run exactly once per load on every exit path (applied, ignored, timed out,
    /// failed, canceled, superseded): releases the load's order gate; publishes the readability probe when this load is
    /// still the active one (a result already in is applied now, see `skipped.rs`), otherwise drops it.
    /// The active load is cleared (`phase` becomes `Idle`) unless a newer load replaced it.
    pub(super) fn finish_load(&mut self, cx: &mut FolderLoadCx<'_>, load_id: u64) -> FolderLoadUpdate {
        let mut update = FolderLoadUpdate {
            step: FolderLoadStep::Finished,
            order_gate_released: false,
        };
        // A superseded load (a newer one replaced the active slot) touches nothing: its gate was released by `load`.
        let Some(active) = self.active.as_mut().filter(|a| a.load_id == load_id) else {
            return update;
        };
        if active.owns_order_gate {
            active.owns_order_gate = false;
            update.order_gate_released = std::mem::take(&mut self.order_gate_open);
        }
        active.phase = LoadPhase::Idle;
        match std::mem::replace(&mut active.probe, ProbeState::None) {
            ProbeState::None => {}
            ProbeState::Running { .. } => active.probe = ProbeState::Running { published: true },
            ProbeState::Ready(outcome) => self.apply_probe(cx, load_id, outcome),
        }
        // Keep the slot only while a published probe is still on its way.
        if let Some(active) = self.active.as_ref().filter(|a| a.load_id == load_id) {
            if !matches!(active.probe, ProbeState::Running { .. }) {
                self.active = None;
            }
        }
        update
    }

    /// Shared `catch` blocks (lines 313 to 326): cancellation while the token is canceled is silent; an error while the token is
    /// live and `is_folder_current(load_generation)` goes to `cx.sink.on_failed(folder, error)`; any other error is only logged
    /// (`log.warn("Superseded folder load failed in '<folder>': <error>")`, RV-A05).
    pub(super) fn report_load_error(&mut self, cx: &mut FolderLoadCx<'_>, load_id: u64, error: &dyn std::error::Error) {
        let Some(active) = self.active.as_ref().filter(|a| a.load_id == load_id) else {
            return;
        };
        if active.token.is_canceled() {
            return;
        }
        if self.deps.clock.is_folder_current(active.load_generation) {
            cx.sink.on_failed(&active.folder, error);
        } else {
            // RV-A05: this load was superseded and then failed; the newer load owns the UI.
            self.deps.log.warn(&format!(
                "Superseded folder load failed in '{}': {error}",
                active.folder
            ));
        }
    }
}
