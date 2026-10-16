//! Pure rules of `PhotoReview.Imaging/Preload/PreloadScheduler.cs` (C# baseline d92d6aae): constants, the diagnostic
//! worker override parser, the preload-window test, the "still wanted" test, the worker limit while the viewer
//! decodes, the memory re-check back-off, the cached headroom probe, the resume verdict, the outcome of one preload
//! and the exact log texts. Contract: `contracts/W4-15a.md` (draft `contracts/drafts-w4/W4-15a/contract.md`). PARITY I-11.
//!
//! Target path: `crates/pr-imaging/src/preload/policy.rs`.
//!
//! No threads, no clock, no globals: every input is a parameter, so the run loop (`queue.rs`) and the scheduler shell
//! (W4-15b) share one definition and the golden `preload-policy` pins it against .NET.

use std::time::Duration;

use pr_core::abstractions::{MemoryProbe, MemorySnapshot};

use crate::decode_types::DecodeBox; // W4-01b
use crate::preload_order::{PreloadOptions, PreloadWindow}; // W4-05b

use super::target::PreloadTarget;

// ---- constants (C# consts / static readonly; the values are part of the behaviour, see the golden) ----------------

/// C# `public const double WholeFolderCacheFillLimit = 0.9`: share of the preview budget a whole-folder pass may fill
/// before it stops adding images beyond the window. Compared INCLUSIVELY (`>=`).
pub const WHOLE_FOLDER_CACHE_FILL_LIMIT: f64 = 0.9;

/// C# `SlowLinkDecodeMsThreshold = 200`: a decode EWMA strictly ABOVE this (ms) is a slow link (Q-R29 C-2).
pub const SLOW_LINK_DECODE_MS_THRESHOLD: f64 = 200.0;

/// C# `SlowLinkViewerBusyWorkerLimit = 1` (never 0: the run loop needs `running < limit` reachable from 0).
pub const SLOW_LINK_VIEWER_BUSY_WORKER_LIMIT: i32 = 1;

/// C# `ResumeCheckBaseDelay = TimeSpan.FromSeconds(2)`: first memory re-check after a pause.
pub const RESUME_CHECK_BASE_DELAY: Duration = Duration::from_secs(2);

/// C# `ResumeCheckMaxDelay = TimeSpan.FromSeconds(30)`.
pub const RESUME_CHECK_MAX_DELAY: Duration = Duration::from_secs(30);

/// C# `MaxResumeChecks = 8`: re-checks armed between two real navigations (2+4+8+16+30*4 s).
pub const MAX_RESUME_CHECKS: i32 = 8;

/// C# `ResumeLoadMargin = 0.05`: pause at `load >= limit`, resume only below `limit - margin` (hysteresis).
pub const RESUME_LOAD_MARGIN: f64 = 0.05;

/// C# `HeadroomRecheckMs = 50`: the cached headroom answer is re-probed after this many ms (strictly more).
pub const HEADROOM_RECHECK_MS: i64 = 50;

/// C# `DisposeDrainTimeout` default `TimeSpan.FromSeconds(3)`: longest `dispose` blocks its caller.
pub const DEFAULT_DISPOSE_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

/// Worker count of the convenience constructor when neither the override nor the environment gives one (C# literal
/// `8` in `workerCountOverride ?? DiagOptionsWorkers() ?? 8`).
pub const CONVENIENCE_DEFAULT_WORKER_COUNT: i32 = 8;

/// Largest value the diagnostic worker override accepts (C# `parsed <= 16`).
pub const MAX_DIAG_WORKERS: i32 = 16;

// ---- small value types -------------------------------------------------------------------------------------------

/// Direction of travel and burst lead as of now (C# tuple `(int Direction, int Lead)` of `CurrentShape()`): read from
/// `NavigationPace::direction()` and `NavigationPace::get_lead(metrics.decode_milliseconds_ewma())`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    /// `+1` forward, `-1` backward (any value `< 0` counts as backward in [`in_preload_window`]).
    pub direction: i32,
    /// Images a key-held burst skips ahead; 0 outside a burst.
    pub lead: i32,
}

impl Shape {
    /// The run loop's initial `seenShape = (Direction: 1, Lead: 0)`.
    pub const INITIAL: Shape = Shape { direction: 1, lead: 0 };
}

/// What one preload item ended as (C# private enum `PreloadOutcome`). The worker's other ending, "the lifetime was
/// cancelled" (C# a cancelled task), is not an outcome: W4-15b models it separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreloadOutcome {
    /// The preview is in the RAM cache afterwards (decoded now, or already cached when the item got its slot).
    Cached,
    /// Dropped before decoding: the user moved past it (see [`is_still_wanted`]).
    Superseded,
    /// Corrupt/unreadable/not cached afterwards: never retried in this run.
    Failed,
    /// The decoder refused the background decode because its queue is full: transient, retried by a later pass.
    Busy,
}

/// C# `ForgetUnlessFailed`: `true` when the path must LEAVE the run's queued set (everything but `Failed`), so a later
/// order pass (user turned back, preview evicted meanwhile) can queue it again.
pub fn forget_unless_failed(outcome: PreloadOutcome) -> bool {
    outcome != PreloadOutcome::Failed
}

/// Verdict of one memory re-check (C# private enum `ResumeVerdict`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeVerdict {
    /// Headroom is back below the hysteresis limit: kick a resumed run.
    Resume,
    /// Still under pressure, or a viewer decode is running: arm the next re-check.
    NotYet,
    /// The cache is at the whole-folder fill limit: stay paused, arm nothing.
    CacheFull,
}

// ---- functions -------------------------------------------------------------------------------------------------

/// C# `internal static int? ParseDiagWorkers(string? value)`: `int.TryParse(value)` (NumberStyles.Integer: leading and
/// trailing white U+0009..U+000D/U+0020, one leading `+`/`-`, ASCII digits, trailing NULs) and `0 <= n <= 16`; anything
/// else (incl. `None`, `""`, `"1.5"`, `"2147483648"`, `"-1"`) is `None`. `"-0"` is `Some(0)`.
///
/// Same accepted set as `pr_core::diagnostics::diag_options` (private `parse_preload_workers`); see contract open
/// question 1 (share it instead of duplicating).
pub fn parse_diag_workers(value: Option<&str>) -> Option<i32> {
    fn is_white(c: char) -> bool {
        matches!(c, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ')
    }

    let text = value?.trim_start_matches(is_white);
    let (negative, digits_and_rest) = if let Some(rest) = text.strip_prefix('+') {
        (false, rest)
    } else if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else {
        (false, text)
    };

    // ASCII digits only (.NET rejects other Unicode digits); leading zeros are fine. The value saturates well past
    // the range limit, which rejects it anyway (as `int` overflow makes `TryParse` fail in .NET).
    let digit_count = digits_and_rest.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    let (digits, rest) = digits_and_rest.split_at(digit_count);
    let magnitude = digits
        .bytes()
        .fold(0_u64, |acc, digit| (acc * 10 + u64::from(digit - b'0')).min(1_000));

    // Trailing white, then nothing but NUL characters.
    if !rest.trim_start_matches(is_white).chars().all(|c| c == '\0') {
        return None;
    }
    if negative && magnitude != 0 {
        return None;
    }
    i32::try_from(magnitude)
        .ok()
        .filter(|n| (0..=MAX_DIAG_WORKERS).contains(n))
}

/// C# `internal static bool InPreloadWindow(int index, int center, (int Direction, int Lead) shape, PreloadWindow window)`:
/// the directional window `PreloadOrderService.Build` queues first. `ahead = (direction < 0 ? -1 : 1) * (index - center)`
/// (unchecked `int`: `wrapping_*`); `ahead > 0` => `ahead <= lead + forward` (wrapping add); `ahead < 0` =>
/// `-ahead <= backward` (wrapping neg); `ahead == 0` (the current image) => `false`.
pub fn in_preload_window(index: i32, center: i32, shape: Shape, window: PreloadWindow) -> bool {
    let sign: i32 = if shape.direction < 0 { -1 } else { 1 };
    let ahead = sign.wrapping_mul(index.wrapping_sub(center));
    if ahead > 0 {
        ahead <= shape.lead.wrapping_add(window.forward)
    } else {
        ahead < 0 && ahead.wrapping_neg() <= window.backward
    }
}

/// C# private `IsStillWanted(int index)` with the centre and shape passed in: `ahead = direction * (index - center)`
/// (NOT normalized to +-1, unlike [`in_preload_window`]; unchecked `int`); `ahead == 0` => `false` (it is the current
/// image: the viewer's own decode handles it); otherwise `lead == 0 || ahead > 0` (during a burst an image behind the
/// user is no longer wanted).
pub fn is_still_wanted(index: i32, center: i32, shape: Shape) -> bool {
    let ahead = shape.direction.wrapping_mul(index.wrapping_sub(center));
    if ahead == 0 {
        return false;
    }
    shape.lead == 0 || ahead > 0
}

/// C# field initializer `Math.Max(2, Environment.ProcessorCount / 3)`: concurrent preload decodes allowed to START while
/// a viewer decode runs (fast link). Integer division truncates.
pub fn viewer_busy_worker_limit(processor_count: i32) -> i32 {
    2.max(processor_count / 3)
}

/// C# loop body: `ActiveViewerDecodes > 0 ? (slowLink ? 1 : Math.Min(workers, viewerBusyLimit)) : workers` with
/// `slowLink = decodeMillisecondsEwma > 200` (strict: exactly 200 is NOT slow). `ActiveViewerDecodes` is read once per
/// pass by the caller.
pub fn worker_limit(workers: i32, active_viewer_decodes: i32, decode_ms_ewma: f64, viewer_busy_limit: i32) -> i32 {
    if active_viewer_decodes > 0 {
        if decode_ms_ewma > SLOW_LINK_DECODE_MS_THRESHOLD {
            SLOW_LINK_VIEWER_BUSY_WORKER_LIMIT
        } else {
            workers.min(viewer_busy_limit)
        }
    } else {
        workers
    }
}

/// C# `internal static TimeSpan ResumeCheckDelay(int check)`: `ResumeCheckBaseDelay * Math.Pow(2, Math.Min(check, 16))`
/// capped at [`RESUME_CHECK_MAX_DELAY`] (`delay < max ? delay : max`). `TimeSpan * double` rounds to whole ticks
/// (100 ns); for `check < 0` the factor is fractional (`-1` => 1 s; not reachable from the scheduler, pinned anyway).
pub fn resume_check_delay(check: i32) -> Duration {
    const NANOS_PER_TICK: u64 = 100;
    let base_ticks = (RESUME_CHECK_BASE_DELAY.as_nanos() / u128::from(NANOS_PER_TICK)) as f64;
    // .NET `TimeSpan * double`: `Math.Round(ticks * factor)` (half to even), then a whole number of ticks. The factor
    // is `Math.Pow(2, min(check, 16))`, exact for every integer exponent (0 for very negative ones).
    let ticks = (base_ticks * 2_f64.powi(check.min(16))).round_ties_even();
    let max_ticks = (RESUME_CHECK_MAX_DELAY.as_nanos() / u128::from(NANOS_PER_TICK)) as f64;
    if ticks < max_ticks {
        // 0 <= ticks < 300_000_000: the conversion is exact.
        Duration::from_nanos(ticks as u64 * NANOS_PER_TICK)
    } else {
        RESUME_CHECK_MAX_DELAY
    }
}

/// C# `Math.Max(0, MemoryLoadLimit - ResumeLoadMargin)`: the load limit the re-check probes with.
pub fn resume_max_load(memory_load_limit: f64) -> f64 {
    let lowered = memory_load_limit - RESUME_LOAD_MARGIN;
    // .NET `Math.Max` propagates NaN; Rust `f64::max` would return the other operand.
    if lowered.is_nan() { lowered } else { lowered.max(0.0) }
}

/// C# `cacheBytes >= fullFolderThresholdBytes * WholeFolderCacheFillLimit`: `long >= long * double` is a DOUBLE
/// comparison (both sides converted to `f64`; inclusive). Used by the whole-folder stop and by the resume verdict.
pub fn fill_limit_reached(cache_bytes: i64, full_folder_threshold_bytes: i64) -> bool {
    (cache_bytes as f64) >= (full_folder_threshold_bytes as f64) * WHOLE_FOLDER_CACHE_FILL_LIMIT
}

/// C# `EvaluateResume()`, called OUTSIDE the scheduler gate. Order and laziness are observable (fakes count probe
/// calls and record the limit): (1) `options.full_folder_threshold_bytes > 0 && fill_limit_reached(target.cache_bytes(),
/// ..)` => `CacheFull` (cache_bytes read only when the threshold is positive); (2) `target.active_viewer_decodes() > 0`
/// => `NotYet` (no probe call); (3) `probe.has_headroom(resume_max_load(options.memory_load_limit),
/// options.reserve_bytes)` => `Resume`, else `NotYet`.
pub fn evaluate_resume(options: &PreloadOptions, target: &dyn PreloadTarget, probe: &dyn MemoryProbe) -> ResumeVerdict {
    if options.full_folder_threshold_bytes > 0
        && fill_limit_reached(target.cache_bytes(), options.full_folder_threshold_bytes)
    {
        return ResumeVerdict::CacheFull;
    }
    if target.active_viewer_decodes() > 0 {
        return ResumeVerdict::NotYet;
    }
    if probe.has_headroom(resume_max_load(options.memory_load_limit), options.reserve_bytes) {
        ResumeVerdict::Resume
    } else {
        ResumeVerdict::NotYet
    }
}

/// C# `struct HeadroomProbeState` + `HasPreloadHeadroomCached` (IMG-03): `GlobalMemoryStatusEx` is a syscall, so the
/// answer is cached and re-probed only when (a) never probed, (b) a decode was queued since the last probe, or (c) more
/// than [`HEADROOM_RECHECK_MS`] elapsed (`now - tick > 50`, strict). `Default` = never probed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeadroomProbeState {
    /// Last answer.
    pub value: bool,
    /// Monotonic ms of the last probe (C# `Environment.TickCount64`).
    pub tick: i64,
    /// A probe has run at least once.
    pub checked: bool,
    /// Set by the run loop each time it queues a decode; cleared by the next probe.
    pub decode_queued_since_check: bool,
}

impl HeadroomProbeState {
    /// C# `HasPreloadHeadroomCached(ref state)` with the clock passed in: re-probes (`probe()` is
    /// `memory_probe.has_headroom(options.memory_load_limit, options.reserve_bytes)`) when the doc rule says so, then
    /// stores `value`, `tick = now_ms`, `checked = true`, `decode_queued_since_check = false`; returns `value`.
    pub fn has_headroom_cached(&mut self, now_ms: i64, probe: impl FnOnce() -> bool) -> bool {
        if !self.checked || self.decode_queued_since_check || now_ms.wrapping_sub(self.tick) > HEADROOM_RECHECK_MS {
            self.value = probe();
            self.tick = now_ms;
            self.checked = true;
            self.decode_queued_since_check = false;
        }
        self.value
    }
}

// ---- log texts (byte-identical to the C# interpolated strings; invariant culture) ------------------------------

/// `Preload kick failed; skipped` (`PreloadAroundAsync` catch; Error, with the exception).
pub const MSG_KICK_FAILED: &str = "Preload kick failed; skipped";
/// `Preload scheduler failed` (run loop catch-all; Error, with the exception).
pub const MSG_SCHEDULER_FAILED: &str = "Preload scheduler failed";
/// `Preload memory re-check failed; preload resumes on the next navigation` (timer callback catch; Error).
pub const MSG_RECHECK_FAILED: &str = "Preload memory re-check failed; preload resumes on the next navigation";
/// `Dispose` past its drain timeout (Warn).
pub const MSG_DRAIN_TIMEOUT: &str =
    "Preload scheduler did not drain in time; leaving in-flight decodes to finish on their own";
/// `Dispose` saw a lifetime task fail with a non-cancellation exception (Error, with the exception).
pub const MSG_FAILED_DURING_DISPOSAL: &str = "Preload scheduler failed during disposal";

/// C# `measured?.ToString("F0", CultureInfo.InvariantCulture)`. The golden section `f0` (generated by .NET) is the
/// authority: .NET formats the EXACT binary value and an exact tie rounds half to EVEN (`0.5` => `"0"`, `2.5` => `"2"`,
/// `3.5` => `"4"`, `1234567.5` => `"1234568"`), like Rust `format!("{:.0}")`; values above 2^53 print every integer
/// digit. Negative values that round to zero print `"-0"` (also `-0.0`). No group separators. NaN and the infinities
/// use the invariant symbols (`NaN`, `Infinity`, `-Infinity`); a measured mean is never one of them.
pub fn format_f0_invariant(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
    } else {
        format!("{value:.0}")
    }
}

/// Text of an optional snapshot field: C# interpolates a null `ulong?`/`uint?` as the empty string.
fn optional_text<T: ToString>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

/// .NET `bool.ToString()`.
fn dotnet_bool(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

/// The Info line written when the order is (re)built:
/// `Preload policy: images={n} box={w}x{h} measuredMeanBytes={F0 or "none"} estimatedBytes={est}
/// capacityBytes={threshold} wholeFolder={True|False} center={c} direction={d} lead={l}` (one line, single spaces;
/// .NET `bool` prints `True`/`False`).
#[allow(clippy::too_many_arguments)] // mirrors the C# interpolation's eight inputs one to one
pub fn format_policy_log(
    images: usize,
    decode_box: DecodeBox,
    measured_mean_bytes: Option<f64>,
    estimated_bytes: i64,
    capacity_bytes: i64,
    whole_folder: bool,
    center: i32,
    shape: Shape,
) -> String {
    let measured = measured_mean_bytes.map_or_else(|| "none".to_owned(), format_f0_invariant);
    format!(
        "Preload policy: images={images} box={}x{} measuredMeanBytes={measured} estimatedBytes={estimated_bytes} \
         capacityBytes={capacity_bytes} wholeFolder={} center={center} direction={} lead={}",
        decode_box.width,
        decode_box.height,
        dotnet_bool(whole_folder),
        shape.direction,
        shape.lead,
    )
}

/// `Preload paused for memory: queued={q} cacheCount={c} cacheBytes={b} availableBytes={a} loadPercent={p}` where a
/// missing snapshot prints EMPTY values (`availableBytes= loadPercent=`), as C# interpolates a null `ulong?`/`uint?`.
pub fn format_paused_log(queued: usize, cache_count: i32, cache_bytes: i64, memory: Option<MemorySnapshot>) -> String {
    format!(
        "Preload paused for memory: queued={queued} cacheCount={cache_count} cacheBytes={cache_bytes} \
         availableBytes={} loadPercent={}",
        optional_text(memory.map(|m| m.available_bytes)),
        optional_text(memory.map(|m| m.load_percent)),
    )
}

/// `Preload progress: queued={q} active={a} cacheCount={c} cacheBytes={b} availableBytes={x}` (x empty when no snapshot).
pub fn format_progress_log(
    queued: usize,
    active: usize,
    cache_count: i32,
    cache_bytes: i64,
    memory: Option<MemorySnapshot>,
) -> String {
    format!(
        "Preload progress: queued={queued} active={active} cacheCount={cache_count} cacheBytes={cache_bytes} \
         availableBytes={}",
        optional_text(memory.map(|m| m.available_bytes)),
    )
}

/// `Preload resumed after memory pause: center={center}`.
pub fn format_resumed_log(center: i32) -> String {
    format!("Preload resumed after memory pause: center={center}")
}

/// `Preload stays paused: cache at its fill limit (cacheBytes={cache_bytes})`.
pub fn format_stays_paused_log(cache_bytes: i64) -> String {
    format!("Preload stays paused: cache at its fill limit (cacheBytes={cache_bytes})")
}

/// `Preload skipped, cannot stat: {path}` (Warn; candidate key lookup failed with an I/O error).
pub fn format_skipped_cannot_stat(path: &str) -> String {
    format!("Preload skipped, cannot stat: {path}")
}

/// `Preload deferred, decoder busy: {path}` (Info, the FIRST busy of a path only).
pub fn format_deferred_busy(path: &str) -> String {
    format!("Preload deferred, decoder busy: {path}")
}

/// `Preload deferred again, decoder busy: {path}` (C# `Debug.WriteLine`, debugger only: never the log).
pub fn format_deferred_busy_again(path: &str) -> String {
    format!("Preload deferred again, decoder busy: {path}")
}

/// `Preload failed: {path}` (Error, with the error).
pub fn format_preload_failed(path: &str) -> String {
    format!("Preload failed: {path}")
}

/// `Preload cancelled by another source: {path} ({message})` (Warn; RV-I14). `message` is the error's `Display`
/// (`DecodeError::Canceled` => `"The operation was canceled."`, .NET's default `OperationCanceledException.Message`).
pub fn format_foreign_cancel(path: &str, message: &str) -> String {
    format!("Preload cancelled by another source: {path} ({message})")
}
