//! Port of `PhotoReview.App.Shared/Coordinators/FolderLoadCoordinator.cs` lines 347 to 468: `ReadabilityProbe`,
//! `StartReadabilityProbe`, `ApplyReadabilityProbeAsync` (AR16, IO05; contract `contracts/W8-07b.md`).
//! Target: `coordinators::folder_load::skipped`.
//!
//! `SkippedEntry` is NOT redefined here: the one in `pr_core::abstractions` is used (path `PathBuf`, reason text, kind).

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::abstractions::{FileSystem, SkippedEntry, SkippedKind};
use pr_core::sync::{CancelToken, Canceled};
use pr_core::text::ignore_case_key;

use super::state::{
    FolderLoadCoordinator, FolderLoadCx, FolderLoadEvent, FolderLoadStep, FolderLoadUpdate, FolderPerf,
    LOGGED_SKIPPED_LIMIT, PROBE_PARALLELISM, ProbeState, WorkerLane,
};

/// The probe job body (runs on `WorkerLane::Probe`; C# `StartReadabilityProbe`): probes `paths` with at most
/// `PROBE_PARALLELISM` threads, one `try_probe_readable` each, checking the token before every file. A failed probe whose file
/// still exists (`file_exists`) is unreadable (reason `""` when the text is empty); a file that is gone is NOT reported.
/// The result is in listing order (index order), never in completion order, kind `SkippedKind::FileUnreadable`.
/// Marks "probeStart"(count) and "probed"(unreadable count). `Err(Canceled)` when the token fired (C# `Parallel.For` throws).
pub(super) fn run_probe(
    file_system: Arc<dyn FileSystem>,
    paths: Arc<[String]>,
    token: CancelToken,
    perf: Option<Arc<dyn FolderPerf>>,
    load_generation: i64,
) -> Result<Vec<SkippedEntry>, Canceled> {
    let mark = |phase: &str, value: usize| {
        if let Some(perf) = &perf {
            perf.mark(load_generation, phase, i64::try_from(value).ok(), "");
        }
    };
    mark("probeStart", paths.len());

    let failures: Mutex<Vec<Option<String>>> = Mutex::new(vec![None; paths.len()]);
    let next = AtomicUsize::new(0);
    let workers = PROBE_PARALLELISM.min(paths.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    if token.is_canceled() {
                        return;
                    }
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    let Some(path) = paths.get(index) else {
                        return;
                    };
                    let path = Path::new(path);
                    // A file deleted since the listing is not "unreadable": only failures pay for the existence check.
                    if let Err(reason) = file_system.try_probe_readable(path) {
                        if file_system.file_exists(path) {
                            let mut slots = failures.lock().unwrap_or_else(PoisonError::into_inner);
                            if let Some(slot) = slots.get_mut(index) {
                                *slot = Some(reason);
                            }
                        }
                    }
                }
            });
        }
    });
    token.check()?;

    // Listing order, so the report is deterministic whatever order the probes finished in.
    let slots = failures.into_inner().unwrap_or_else(PoisonError::into_inner);
    let unreadable: Vec<SkippedEntry> = paths
        .iter()
        .zip(slots)
        .filter_map(|(path, failure)| {
            failure.map(|reason| SkippedEntry {
                path: PathBuf::from(path),
                reason,
                kind: SkippedKind::FileUnreadable,
            })
        })
        .collect();
    mark("probed", unreadable.len());
    Ok(unreadable)
}

impl FolderLoadCoordinator {
    /// C# `ReadabilityProbe` not completed: a probe was started and has not been applied or dropped yet.
    pub fn probe_pending(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|a| matches!(a.probe, ProbeState::Running { .. } | ProbeState::Ready(_)))
    }

    /// C# `StartReadabilityProbe` call site (line 249): submits `run_probe` on `WorkerLane::Probe`. Called by
    /// `on_scan_finished` right after the first `sink.present` of a non-empty catalog, never for an empty folder.
    pub(super) fn start_probe(&mut self) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let file_system = Arc::clone(&self.deps.file_system);
        let paths = Arc::clone(&active.scanned_files);
        let token = active.token.clone();
        let perf = self.deps.perf.clone();
        let load_generation = active.load_generation;
        let load_id = active.load_id;
        active.probe = ProbeState::Running { published: false };
        self.deps.worker.submit(
            WorkerLane::Probe,
            Box::new(move || {
                let outcome = match catch_unwind(AssertUnwindSafe(|| {
                    run_probe(file_system, paths, token, perf, load_generation)
                })) {
                    Ok(Ok(unreadable)) => Ok(unreadable),
                    Ok(Err(Canceled)) => Err(None),
                    Err(_) => Err(Some("the readability probe panicked".to_owned())),
                };
                FolderLoadEvent::ProbeFinished { load_id, outcome }
            }),
        );
    }

    /// Delivers `ProbeFinished` (UI thread). A stale `load_id` is dropped (a failed stale probe is only logged, C#
    /// `FireAndLog`). If the load is still running the result is stored (`ProbeState::Ready`) and applied by `finish_load`;
    /// if the load already finished and published the probe, it is applied now.
    pub(super) fn on_probe_finished(
        &mut self,
        cx: &mut FolderLoadCx<'_>,
        load_id: u64,
        outcome: Result<Vec<SkippedEntry>, Option<String>>,
    ) -> FolderLoadUpdate {
        let Some(active) = self.active.as_mut().filter(|a| a.load_id == load_id) else {
            if let Err(Some(text)) = &outcome {
                self.deps
                    .log
                    .warn(&format!("Readability probe of a superseded folder load failed: {text}"));
            }
            return FolderLoadUpdate {
                step: FolderLoadStep::Ignored,
                order_gate_released: false,
            };
        };
        match active.probe {
            ProbeState::Running { published: true } => {
                active.probe = ProbeState::None;
                self.apply_probe(cx, load_id, outcome);
                if self.active.as_ref().is_some_and(|a| a.load_id == load_id) {
                    self.active = None;
                }
                FolderLoadUpdate {
                    step: FolderLoadStep::Finished,
                    order_gate_released: false,
                }
            }
            ProbeState::Running { published: false } => {
                // The load is still settling its Explorer order: hold the result until `finish_load` (INV-7/INV-9).
                active.probe = ProbeState::Ready(outcome);
                FolderLoadUpdate::default()
            }
            ProbeState::None | ProbeState::Ready(_) => FolderLoadUpdate {
                step: FolderLoadStep::Ignored,
                order_gate_released: false,
            },
        }
    }

    /// C# `ApplyReadabilityProbeAsync` (lines 397 to 468), UI thread. `Err(None)` (canceled) returns silently; `Err(Some)`
    /// logs `error("Readability probe failed in '<folder>'; the catalog keeps every listed file")` and removes nothing. Then,
    /// gated by the LOAD token only (never by the folder generation, a file action bumps that without changing the folder):
    /// canceled -> perf "probeDropped"; no unreadable file -> nothing; `catalog.remove_paths(unreadable paths)` -> empty ->
    /// nothing (already gone); else `log.warn` line, `sink.on_files_skipped(folder, listing_skipped + probe_skipped)` (probe
    /// entries limited to the removed paths, OrdinalIgnoreCase), perf "probeApplied"(removed count), then
    /// `sink.on_unreadable_removed(&removed, current_removed)` where `current_removed` = the current path before removal is in
    /// the removed set.
    pub(super) fn apply_probe(
        &mut self,
        cx: &mut FolderLoadCx<'_>,
        load_id: u64,
        outcome: Result<Vec<SkippedEntry>, Option<String>>,
    ) {
        let Some(active) = self.active.as_ref().filter(|a| a.load_id == load_id) else {
            return;
        };
        let folder = active.folder.clone();
        let load_generation = active.load_generation;
        let token_canceled = active.token.is_canceled();
        let listing_skipped = active.skipped.clone();

        let unreadable = match outcome {
            Err(None) => return, // superseded or closed
            Err(Some(text)) => {
                // Nothing is removed: every file stays reachable and a failing one reports its own error when shown.
                let error = std::io::Error::other(text);
                self.deps.log.error(
                    &format!("Readability probe failed in '{folder}'; the catalog keeps every listed file"),
                    Some(&error),
                );
                return;
            }
            Ok(unreadable) => unreadable,
        };
        // Gated by the load's own token only, NOT by the folder generation.
        if token_canceled {
            self.perf_mark(load_generation, "probeDropped", None);
            return;
        }
        if unreadable.is_empty() {
            return;
        }

        let current_path = cx.catalog.current().map(|entry| entry.path().to_owned());
        let paths: Vec<String> = unreadable
            .iter()
            .map(|entry| entry.path.to_string_lossy().into_owned())
            .collect();
        let removed = cx.catalog.remove_paths(&paths);
        if removed.is_empty() {
            return; // already gone (deleted/moved by the user meanwhile)
        }

        let removed_set: HashSet<String> = removed.iter().map(|p| ignore_case_key(p)).collect();
        let probe_skipped: Vec<SkippedEntry> = unreadable
            .into_iter()
            .filter(|entry| removed_set.contains(&ignore_case_key(&entry.path.to_string_lossy())))
            .collect();
        let listed: Vec<String> = probe_skipped
            .iter()
            .take(LOGGED_SKIPPED_LIMIT)
            .map(|entry| format!("{} ({})", entry.path.display(), entry.reason))
            .collect();
        self.deps.log.warn(&format!(
            "Readability probe skipped {} unreadable file(s) in '{}': {}",
            probe_skipped.len(),
            folder,
            listed.join("; ")
        ));
        // The full list replaces an earlier listing-interrupted report of this load.
        let mut all = listing_skipped;
        all.extend(probe_skipped);
        cx.sink.on_files_skipped(&folder, &all);
        self.perf_mark_value(load_generation, "probeApplied", removed.len());

        let current_removed = current_path.is_some_and(|path| removed_set.contains(&ignore_case_key(&path)));
        cx.sink.on_unreadable_removed(&removed, current_removed);
    }
}
