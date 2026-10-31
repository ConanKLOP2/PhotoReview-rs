# FIX-WORKER-PANICS (2026-10-31): a panic in a background job must not strand the UI

Status: fixed on this branch (reliability / data-safety lane). Source: [ST-APP-RACES-2026-10-31.md](ST-APP-RACES-2026-10-31.md) H1, M1, M2, M3.
Base: master plus the open PRs #462 (fix-restore-m3) and #464 (fix-dupclean-m1), merged into the branch first (same files).

## Verified on the real code, and what C# does
- C# `DuplicateCleanupController.RemoveDuplicatesAsync` wraps `DuplicateFinder.FindAsync` in `try { } catch (Exception) { } finally { }`; the
  `finally` releases the hash slot. `FolderLoadCoordinator.LoadAsync` catches any exception of its scan task and reports it. A faulted `Task`
  never leaves the caller waiting. So the Rust gap is real: a Rust panic in a worker thread kills the thread and no event is ever sent.
- H1 (high): the `WorkerLane::Hashing` job had no `catch_unwind`; a panicking hasher meant no `HashFinished`, so `duplicate_gate_turn` never
  released the file-action gate (Undo, Recycle, Move/Copy, Rename no-ops until exit). Confirmed (red test).
- M1: `run_scan` panic left the load in `Scanning`. Confirmed (red test).
- M2: `FileHashService` worker thread panic left the flight without result: waiters with `CancelToken::none()` waited for ever and the
  `in_flight` entry stayed. Confirmed (red test hangs until its 30 s bound).
- M3 (presenter, zoom-detail, sibling-overlay jobs): not changed, see below.

## Design
The ST-APP-RACES proposal was one `catch_unwind` in `MainWorkerPort::submit`. That cannot work as a central fix: `submit` is generic over the
event type `E` and has no event to send for a panic, so each enum would need a panic variant and every consumer a new arm. Instead each job
converts a panic into the failure outcome its coordinator already handles, so the existing consumers release the gate/phase unchanged
(no enum gains a variant, no non-panic path changes):
- H1: `catch_unwind` around `duplicates::find` in the hash job; a panic becomes `HashOutcome::Failed(text)` (status "duplicate check failed: <text>"),
  `on_hash_finished` releases the slot and returns `Done`, so the gate is released. Nothing is recycled.
- M1: `catch_unwind(run_scan)` in the scan job; a panic becomes `ScanFailure::Failed(FolderLoadError::Io(text))`, delivered to `on_failed`.
- M2: `catch_unwind` around `compute_and_cache_parts` in the worker thread; a panic becomes `FileHashError::Io("hash computation panicked: ..")`,
  stored as the flight result, so waiters wake and the `in_flight` entry is removed by the existing code. Not cached.
Panic text is carried in the user-visible failure text (the panic hook already logs the payload with its location).

## Tests (red before, green after)
- `duplicate_cleanup::a_panicking_hasher_ends_the_run_with_a_failure_and_recycles_nothing`: before, the panic escaped the job (no event); after, `Done`, failed status, no recycle, slot released.
- `folder_load_load::a_panic_in_the_scan_job_fails_the_load_instead_of_leaving_it_scanning`: before, panic escaped; after, `on_failed`, phase `Idle`.
- `file_hash_worker_panic::a_panic_in_the_hash_worker_fails_the_call_and_the_next_call_recomputes`: before, waiter timed out (30 s); after, `Io` error then a second call succeeds and caches.
Green: `cargo test -p pr-app --test duplicate_cleanup` 46, `folder_load_load` 57 (+3 ignored), `file_hash*` 22 (all pass); fmt and one `cargo clippy -p pr-app --all-targets -- -D warnings` clean.

## Left for later (M3 and the generic runner), with the reason
- `ThreadRunner` / presenter / zoom-detail / sibling-overlay jobs each send a different completion event (cache keys, generations, indicator
  tokens); a panic needs a per-job failure result, and a `catch_unwind` in `ThreadRunner` alone could only log (no event can be invented
  there). That is a larger per-job design: DIVERGENCE only noted here, not fixed. Panics there stay a dropped result (stale image/overlay until
  the next navigation), not a held file-action gate.
- Explorer lane already catches panics; the recycle, undo and cache-clear jobs already did.
