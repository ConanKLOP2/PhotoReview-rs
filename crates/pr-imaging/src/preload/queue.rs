//! The queue/priority half of C# `PreloadScheduler.RunPreloadSchedulerAsync` (C# baseline d92d6aae, lines 475-688):
//! the order pass (rebuilt when the navigation, the direction/lead or the size calibration changed) and the per-candidate
//! decision (window on the caller's thread, memory headroom, whole-folder fill limit, already queued, key lookup,
//! already cached, busy back-off). Contract: `contracts/W4-15a.md` (draft `contracts/drafts-w4/W4-15a/contract.md`).
//!
//! Target path: `crates/pr-imaging/src/preload/queue.rs`.
//!
//! [`RunQueue`] is one run's state. It is single-threaded (`&mut self`) and `Send`: the run loop of W4-15b drives it,
//! first on the caller's thread and then on the run's own thread. It never waits, never starts a worker and never
//! touches the scheduler's locks; the shell (W4-15b) owns workers, the running set, wake-ups, cancellation and exit.

#![allow(unused_variables, dead_code)] // D11: only while a body is `todo!()`; the porter removes this line.

use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use pr_core::abstractions::{Log, MemoryProbe};
use pr_core::catalog::types::CatalogEntry;
use pr_core::sync::CancelToken;

use crate::cache_key::ImageCacheKey; // W4-01a
use crate::decode_types::DecodeBox; // W4-01b
use crate::preload_backoff::{PreloadBusyBackoff, PreviewSizeSampler}; // W4-06b
use crate::preload_order::PreloadOptions; // W4-05b
use crate::ram_budget::FolderEstimateCache; // W4-07a (pub(crate))

use super::policy::{HeadroomProbeState, PreloadOutcome, Shape};
use super::target::PreloadTarget;

/// What the loop reads at the top of every pass (C# `Interlocked.Read(ref _preloadPriorityVersion)`,
/// `Volatile.Read(ref _preloadCenter)` and `CurrentShape()`), passed in by the shell. Read order matters: the wake
/// sequence first, then `version`, then `shape`, then `center` (C# reads the centre only inside the rebuild, i.e. after
/// the version; a centre newer than the version only costs one extra rebuild, an older one would be a lost update).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassInputs {
    /// The priority version (bumped by every navigation / kick).
    pub version: i64,
    /// The preload centre (current image index) as of now; only used when the order is rebuilt.
    pub center: i32,
    /// Direction and burst lead as of now (re-read every pass, not only on navigation).
    pub shape: Shape,
}

/// Borrowed collaborators of one call (the scheduler's fields). `clock_ms` is the monotonic millisecond clock of the
/// headroom cache (C# `Environment.TickCount64`); `perf_enabled` is `PhotoReviewPerf::log().is_enabled()` read by the
/// shell (the pause line then also emits the perf `preload_paused` event).
pub struct QueueContext<'a> {
    pub target: &'a dyn PreloadTarget,
    pub memory_probe: &'a dyn MemoryProbe,
    pub sizes: &'a PreviewSizeSampler,
    pub busy_backoff: &'a PreloadBusyBackoff,
    pub log: &'a dyn Log,
    pub options: &'a PreloadOptions,
    pub clock_ms: &'a (dyn Fn() -> i64 + Sync),
    pub perf_enabled: bool,
}

impl fmt::Debug for QueueContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueContext")
            .field("options", self.options)
            .field("perf_enabled", &self.perf_enabled)
            .finish_non_exhaustive()
    }
}

/// A candidate the run must hand to a worker now (C# the arguments of `PreloadOneAsync(order.Current, path, key, ct)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Index into the run's entries snapshot.
    pub index: i32,
    /// `entries[index].path()` as scanned (original spelling).
    pub path: String,
    /// The key built from the scan's metadata when the candidate was examined (reused by the worker's cache re-check).
    pub key: ImageCacheKey,
    /// C# `++examinedSinceYield >= workers` after this dispatch: the shell writes the progress line (if the log is on)
    /// and yields (C# `await Task.Delay(1, ct)`; off the caller's thread from then on).
    pub batch_yield: bool,
}

/// The decision for the next candidate (one iteration of the C# inner `while (running.Count < limit &&
/// order.MoveNext())` body). The shell checks `running < limit` BEFORE calling [`RunQueue::next`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueStep {
    /// `order.MoveNext()` was false: this pass has nothing left (the inner loop ends).
    Exhausted,
    /// The lifetime token was cancelled (C# `ThrowIfCancellationRequested` right after `MoveNext`): the run drains and
    /// ends. The candidate is consumed.
    Canceled,
    /// C# perf(Q-R26) `ForceYielding`: on the caller's thread and the candidate is outside the preload window. The
    /// candidate is KEPT; the shell moves the run off the caller's thread and calls `next(ctx, false, ..)`, which
    /// resumes with this same candidate at the headroom check (no second cancellation check, as in C#).
    LeaveCallerThread,
    /// No headroom: the pause line was logged, [`RunQueue::is_paused`] is now `true`; the inner loop ends and the run
    /// exits (the shell's exit check arms the memory re-check).
    Paused,
    /// Whole-folder pass, cache at the fill limit, candidate outside the window: the inner loop ends (C# `break`). The
    /// order is NOT reset: the next pass of the same order resumes after this candidate.
    FillLimitStop,
    /// Examined and nothing to do: already queued in this run, key lookup failed with I/O (warning logged), already
    /// cached (busy history forgotten), or within its busy cooldown.
    Skipped,
    /// Queue this candidate on a worker now. The path is already in the queued set and the headroom cache knows a
    /// decode was queued.
    Dispatch(Candidate),
}

/// An error that ends the run as `"Preload scheduler failed"` (C# an exception escaping the loop body): a non-I/O key
/// error ([`super::target::KeyError::Other`]) at the centre or at a candidate, or `ArgumentOutOfRange` from the
/// whole-folder decision (a negative estimate).
#[derive(Debug)]
pub struct QueueError(pub Box<dyn Error + Send + Sync>);

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W4-15a: the inner error's Display")
    }
}

impl Error for QueueError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        todo!("W4-15a: the inner error")
    }
}

/// The per-run state of C# `RunPreloadSchedulerAsync` that is about WHAT to preload (not about workers).
pub struct RunQueue {
    /// The entries snapshot this run iterates (C# `CatalogEntry[] entries`, taken when the run started). Identity
    /// (`Arc::ptr_eq`) feeds [`FolderEstimateCache`].
    entries: Arc<[CatalogEntry]>,
    /// C# `workers = _workerCount` (the batch-yield size; NOT the per-pass limit).
    workers: i32,
    /// Paths in flight plus paths whose preload failed, by `pr_core::text::ignore_case_key` (C# `HashSet<string>` with
    /// `StringComparer.OrdinalIgnoreCase`).
    queued: HashSet<String>,
    seen_version: i64,
    seen_shape: Shape,
    seen_box: DecodeBox,
    seen_calibrated: bool,
    seen_whole_folder: bool,
    estimate_cache: FolderEstimateCache,
    order_center: i32,
    /// The current order (C# lazy `IEnumerator<int>`; `None` before the first build). `build_preload_order` (W4-05b)
    /// returns an eager `Vec<i32>`.
    order: Option<std::vec::IntoIter<i32>>,
    /// The candidate held back by [`QueueStep::LeaveCallerThread`].
    pending: Option<i32>,
    examined_since_yield: i32,
    headroom: HeadroomProbeState,
    paused: bool,
}

impl fmt::Debug for RunQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunQueue")
            .field("entries", &self.entries.len())
            .field("workers", &self.workers)
            .field("queued", &self.queued.len())
            .field("seen_version", &self.seen_version)
            .field("seen_shape", &self.seen_shape)
            .field("paused", &self.paused)
            .finish_non_exhaustive()
    }
}

impl RunQueue {
    /// A fresh run: `seen_version = -1`, `seen_shape = Shape::INITIAL`, `seen_box = DecodeBox::UNBOUNDED`,
    /// `seen_calibrated = false`, `seen_whole_folder = false`, `order_center = 0`, no order, nothing queued, headroom
    /// never probed, not paused.
    pub fn new(entries: Arc<[CatalogEntry]>, workers: i32) -> Self {
        todo!("W4-15a: see the doc comment")
    }

    /// The snapshot this run iterates.
    pub fn entries(&self) -> &Arc<[CatalogEntry]> {
        &self.entries
    }

    /// Loop head (C# lines 505-533). `calibrated = sizes.mean_bytes(seen_box).is_some()`; when `inputs.version !=
    /// seen_version || inputs.shape != seen_shape || calibrated != seen_calibrated` the order is rebuilt, in this order:
    /// `busy_backoff.begin_pass()`; `box = current_box(inputs.center)`; `measured = sizes.mean_bytes(box)`;
    /// `estimated = estimate_cache.get_or_compute(&entries, box, measured)`; `whole_folder =
    /// should_preload_whole_folder_estimate(estimated, options.full_folder_threshold_bytes, memory_probe,
    /// options.reserve_bytes, options.memory_load_limit)?`; the policy line if `log.enabled()`; `order =
    /// build_preload_order(center, entries.len() as i32, whole_folder, shape.direction, shape.lead, options.window)`;
    /// then `seen_version`, `seen_shape`, `seen_box = box`, `seen_calibrated = measured.is_some()`,
    /// `seen_whole_folder`, `order_center = center`. A pending caller-thread candidate is dropped with the old order.
    /// Returns `Ok(true)` when rebuilt.
    pub fn refresh(&mut self, ctx: &QueueContext<'_>, inputs: PassInputs) -> Result<bool, QueueError> {
        todo!("W4-15a: see the doc comment")
    }

    /// One candidate (C# lines 561-640 minus the worker start). Order of checks, each observable by a test:
    /// 1. `pending` (after `LeaveCallerThread`) or `order.next()`; none => `Exhausted` (also when no order was built);
    /// 2. `cancel.is_canceled()` => `Canceled`;
    /// 3. `on_caller_thread && !in_preload_window(index, order_center, seen_shape, options.window)` => keep as pending,
    ///    `LeaveCallerThread`;
    /// 4. `!headroom.has_headroom_cached(clock_ms(), || memory_probe.has_headroom(limit, reserve))` => take the snapshot
    ///    ONLY if `log.enabled() || perf_enabled`, log [`super::policy::format_paused_log`] (Info, when enabled), emit
    ///    perf `preload_paused(load_percent as i32 or -1, available_bytes / MiB as i64 or -1)` (when perf is on),
    ///    `paused = true` => `Paused`;
    /// 5. `seen_whole_folder && fill_limit_reached(target.cache_bytes(), options.full_folder_threshold_bytes) &&
    ///    !in_preload_window(..)` => `FillLimitStop`;
    /// 6. path already in `queued` (ignore case) => `Skipped`;
    /// 7. `target.get_current_cache_key(entry)`: `Err(Io)` => Warn `format_skipped_cannot_stat(path)`, `Skipped`;
    ///    `Err(Other)` => `Err(QueueError)`;
    /// 8. `target.try_get_cached_preview(&key)` => `busy_backoff.record_success(path)`, `Skipped`;
    /// 9. `busy_backoff.should_skip(path, &key)` => `Skipped`;
    /// 10. insert into `queued`, `headroom.decode_queued_since_check = true`, `examined_since_yield += 1`; when it
    ///     reaches `workers` reset it to 0 and set `batch_yield` => `Dispatch`.
    pub fn next(
        &mut self,
        ctx: &QueueContext<'_>,
        on_caller_thread: bool,
        cancel: &CancelToken,
    ) -> Result<QueueStep, QueueError> {
        todo!("W4-15a: see the doc comment")
    }

    /// A worker of this run finished (or a dispatch completed inline): C# `ForgetUnlessFailed(queued, path, outcome)`.
    pub fn on_finished(&mut self, path: &str, outcome: PreloadOutcome) {
        todo!("W4-15a: remove ignore_case_key(path) unless Failed")
    }

    /// C# `paused`: this run stopped for memory headroom.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// The version the current order was built for (C# `seenVersion`, `-1` before the first build): the shell's exit
    /// check compares it with the live priority version (RV-I12).
    pub fn seen_version(&self) -> i64 {
        self.seen_version
    }

    /// The shape the current order was built for.
    pub fn seen_shape(&self) -> Shape {
        self.seen_shape
    }

    /// The centre the current order was built around.
    pub fn order_center(&self) -> i32 {
        self.order_center
    }

    /// C# `queued.Count` (log lines).
    pub fn queued_count(&self) -> usize {
        self.queued.len()
    }

    /// C# line 644: `(sizes.mean_bytes(seen_box).is_some()) != seen_calibrated`. The shell takes one more pass instead
    /// of exiting when this is true, the run is not paused and nothing is running (Q-R17).
    pub fn calibration_changed(&self, sizes: &PreviewSizeSampler) -> bool {
        todo!("W4-15a: see the doc comment")
    }

    /// C# `CurrentBox(entries, center)`: `DecodeBox::UNBOUNDED` when `center` is outside `0..entries.len()`; else the
    /// `target_box()` of `target.get_current_cache_key(&entries[center])` (no stat: the scan's metadata); `Err(Io)` =>
    /// `UNBOUNDED` (no log); `Err(Other)` => `Err(QueueError)`.
    fn current_box(&self, ctx: &QueueContext<'_>, center: i32) -> Result<DecodeBox, QueueError> {
        todo!("W4-15a: see the doc comment")
    }
}
