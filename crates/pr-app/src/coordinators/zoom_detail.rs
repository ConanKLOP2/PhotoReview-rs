//! Port of `PhotoReview.App.Shared/Coordinators/ZoomDetailLoader.cs` (310 lines, baseline d92d6aae): the on-demand
//! full-resolution decode of the CURRENT image while the viewer is zoomed (feat(zoom), ADR 0008). Contract:
//! `contracts/W8-07g.md`. Target: `crates/pr-app/src/coordinators/zoom_detail.rs` (`coordinators::zoom_detail`).
//!
//! Conventions (D-W8-1, D-W8-2, R0003):
//! - UI-thread only. No `async`, no futures, no runtime. The decode runs on a worker (`BackgroundRunner`) that waits on the
//!   `PreviewHandle` and puts the outcome into the loader's [`CompletionInbox`]; the UI thread applies it in [`ZoomDetailLoader::pump`],
//!   which re-checks the navigation generation (INV-1, INV-9) and the target / load identity.
//! - The C# callbacks (`_show`, `OriginalShown`, `RawDecodeIndicatorChanged`) are returned as [`ZoomDetailEffect`] values, in the
//!   order C# invoked them. The caller (the image presenter, W8-07h) applies them. No closure points back into the presenter.
//! - Target / load / indicator identity is a monotonically increasing `u64` (C# `ReferenceEquals` on the `Target` record, the
//!   `CancellationTokenSource` and the indicator source).
//!
//! Also defined here because this is the first unit that needs them, and the presenter units (W8-07h..k) reuse them:
//! [`CompletionInbox`], [`BackgroundRunner`] / [`ThreadRunner`], [`WakeFn`], [`same_platform_image`].
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::types::is_raw_path;
use pr_core::sync::CancelToken;
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::service::{PreviewImage, PreviewImageService, PreviewResult};

// ---- Shared seams (used by the presenter units too) -------------------------------------------------------------------

/// Called on any thread whenever a completion was put into an inbox ("the UI thread has work"). The shell implements it as
/// `UiScheduler::post(|| pump)`; tests count calls. It must not block and must not call back into the inbox owner.
pub type WakeFn = Arc<dyn Fn() + Send + Sync>;

/// A unit of blocking work for a worker thread.
pub type BackgroundJob = Box<dyn FnOnce() + Send + 'static>;

/// Runs blocking work (a `PreviewHandle::wait`, a stat, a hash, a delay) off the UI thread. Production: [`ThreadRunner`]
/// (one named `std::thread` per job). Tests inject an inline or queued runner so no wall-clock time is involved.
pub trait BackgroundRunner: Send + Sync {
    /// Runs `job` somewhere else than the caller's stack frame. `name` is the thread name for the production runner.
    fn run(&self, name: &'static str, job: BackgroundJob);
}

/// Production [`BackgroundRunner`]: `std::thread::Builder::new().name(name).spawn(job)`. A spawn failure is logged with
/// `AppLog` and the job is dropped (the navigation then stays unfinished until the next one supersedes it, like a C# task that
/// never completes).
#[derive(Debug, Default, Clone, Copy)]
pub struct ThreadRunner;

impl BackgroundRunner for ThreadRunner {
    fn run(&self, name: &'static str, job: BackgroundJob) {
        todo!()
    }
}

/// Worker-to-UI mailbox (C# `Task` continuation resumed on the UI context). `send` is called on workers, `drain` on the UI thread.
/// Cloning shares the same queue.
pub struct CompletionInbox<T> {
    shared: Arc<InboxShared<T>>,
}

struct InboxShared<T> {
    queue: Mutex<VecDeque<T>>,
    ready: Condvar,
    wake: WakeFn,
}

impl<T> Clone for CompletionInbox<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> fmt::Debug for CompletionInbox<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompletionInbox").finish_non_exhaustive()
    }
}

impl<T> CompletionInbox<T> {
    /// An empty inbox that calls `wake` after every `send`.
    pub fn new(wake: WakeFn) -> Self {
        Self {
            shared: Arc::new(InboxShared {
                queue: Mutex::new(VecDeque::new()),
                ready: Condvar::new(),
                wake,
            }),
        }
    }

    /// Appends `event`, wakes a thread blocked in [`wait_ready`](Self::wait_ready), then calls the wake function (outside the lock).
    pub fn send(&self, event: T) {
        {
            let mut queue = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
            queue.push_back(event);
        }
        self.shared.ready.notify_all();
        (self.shared.wake)();
    }

    /// Takes every queued event in arrival order (UI thread).
    pub fn drain(&self) -> Vec<T> {
        let mut queue = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
        queue.drain(..).collect()
    }

    /// Number of queued events.
    pub fn len(&self) -> usize {
        self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// True when nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Blocks until an event is queued or `timeout` elapsed; true when an event is queued. Hang guard for tests and probes
    /// (AGENTS.md: no fixed-delay asserts); never call it on the UI thread of the app.
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let queue = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
        let (queue, _) = self
            .shared
            .ready
            .wait_timeout_while(queue, timeout, |queue| queue.is_empty())
            .unwrap_or_else(PoisonError::into_inner);
        !queue.is_empty()
    }
}

/// C# `ReferenceEquals(a.PlatformImage, b.PlatformImage)`: same pixel buffer allocation (`Arc::ptr_eq`), or the same opaque
/// object. Never compares pixel contents. Different variants are never the same image.
pub fn same_platform_image(a: &PlatformImage, b: &PlatformImage) -> bool {
    todo!()
}

// ---- Public types of the loader ---------------------------------------------------------------------------------------------

/// C# `ZoomDetailRefreshResult`: what `Refresh` did about resolution. Discriminants are the C# values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomDetailRefreshResult {
    /// The shown bitmap is already the full-resolution image (or there is no image): nothing to load.
    AlreadyFullResolution = 0,
    /// The preview already has at least as many pixels as the screen shows: nothing to load.
    PreviewSufficient = 1,
    /// The held original was put on screen at once.
    ShowedOriginal = 2,
    /// A full-resolution decode started; it replaces the preview when done.
    LoadingOriginal = 3,
    /// Like `LoadingOriginal`, after an earlier decode of this image had failed.
    RetryingOriginal = 4,
}

/// What a loader call asks its caller to do, in the order the C# code invoked the callbacks.
#[derive(Debug, Clone)]
pub enum ZoomDetailEffect {
    /// C# `_show(image, w, h)`: display this platform image with this original size (preview: the preview's recorded original
    /// size; held original: ITS OWN pixel size, ADR 0008 amendment R4). The presenter wraps it in `ShowZoomDetailImage`.
    ShowImage {
        image: PlatformImage,
        original_width: i32,
        original_height: i32,
    },
    /// C# event `OriginalShown(path)`: raised right after the original replaced the preview (measure seam).
    OriginalShown { path: String },
    /// C# event `RawDecodeIndicatorChanged(visible)`: raised only when the visibility really changed.
    RawDecodeIndicatorChanged { visible: bool },
}

/// Effects of one call (C# callbacks), possibly empty.
pub type ZoomDetailEffects = Vec<ZoomDetailEffect>;

/// How long a RAW full decode runs before its indicator shows (C# `TimeSpan.FromMilliseconds(300)`).
pub const RAW_INDICATOR_DELAY: Duration = Duration::from_millis(300);

/// C# `IndicatorDelay` test seam (`Func<TimeSpan, CancellationToken, Task>`), blocking form: waits `delay` or until the token
/// is cancelled; returns true when the delay elapsed, false when it was cancelled. Runs on a [`BackgroundRunner`] thread.
pub type IndicatorDelay = Arc<dyn Fn(Duration, &CancelToken) -> bool + Send + Sync>;

/// The production delay: waits in slices of at most 20 ms and re-checks the token between slices (`CancelToken` has no wait
/// primitive). Tests never use it (they inject a gate they release themselves).
pub fn default_indicator_delay() -> IndicatorDelay {
    todo!()
}

/// What a worker reports back to the loader (private protocol between the worker jobs and `pump`; tests do not construct it).
pub(crate) enum ZoomDetailEvent {
    /// The decode of load `load_id` for target `target_id` ended.
    OriginalDecoded {
        load_id: u64,
        target_id: u64,
        result: PreviewResult,
    },
    /// The indicator delay of `indicator_id` elapsed (it was not cancelled).
    IndicatorElapsed { indicator_id: u64 },
}

/// C# `private sealed record Target(...)`; identity is `id`, not value equality (C# uses `ReferenceEquals`).
#[derive(Debug, Clone)]
struct Target {
    id: u64,
    token: i64,
    path: String,
    key: ImageCacheKey,
    preview: PlatformImage,
    preview_pixel_width: i32,
    original_width: i32,
    original_height: i32,
}

/// C# `_cts` (the in-flight decode of the current target). Present = "already decoding this target" and `PendingLoad != null`.
#[derive(Debug, Clone)]
struct LoadSlot {
    id: u64,
    cancel: CancelToken,
}

/// C# `_indicatorCts`.
#[derive(Debug, Clone)]
struct IndicatorSlot {
    id: u64,
    cancel: CancelToken,
}

/// C# `ZoomDetailLoader`.
pub struct ZoomDetailLoader {
    service: Arc<PreviewImageService>,
    clock: Arc<GenerationClock>,
    runner: Arc<dyn BackgroundRunner>,
    inbox: CompletionInbox<ZoomDetailEvent>,
    indicator_delay: IndicatorDelay,
    target: Option<Target>,
    original: Option<PreviewImage>,
    showing_original: bool,
    /// C# `double? _zoom`: `None` = Fit.
    zoom: Option<f64>,
    /// C# `_refreshHold` (Q-TOUCHPAD-REFRESH).
    refresh_hold: bool,
    load: Option<LoadSlot>,
    /// C# `_failedToken`: navigation token whose full-resolution decode failed; -1 = none.
    failed_token: i64,
    indicator: Option<IndicatorSlot>,
    raw_indicator_visible: bool,
    next_id: u64,
}

impl fmt::Debug for ZoomDetailLoader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZoomDetailLoader")
            .field("zoom", &self.zoom)
            .field("showing_original", &self.showing_original)
            .field("refresh_hold", &self.refresh_hold)
            .field("has_original", &self.original.is_some())
            .field("loading", &self.load.is_some())
            .finish_non_exhaustive()
    }
}

impl ZoomDetailLoader {
    /// C# constructor `(previewService, clock, show, uiScheduler)`. `show` and `uiScheduler` are gone (effects + inbox); `runner`
    /// runs the decode wait and the indicator delay; `wake` is called after every completion arrived (see [`WakeFn`]).
    /// Starts nothing, reads no file. Indicator delay: [`default_indicator_delay`].
    pub fn new(
        service: Arc<PreviewImageService>,
        clock: Arc<GenerationClock>,
        runner: Arc<dyn BackgroundRunner>,
        wake: WakeFn,
    ) -> Self {
        todo!()
    }

    /// C# `IndicatorDelay { set }` (internal test seam): replaces the 300 ms wait.
    #[must_use]
    pub fn with_indicator_delay(self, delay: IndicatorDelay) -> Self {
        todo!()
    }

    /// C# `HeldOriginal`: the full-resolution decode held for the current image.
    pub fn held_original(&self) -> Option<&PreviewImage> {
        todo!()
    }

    /// C# `IsShowingOriginal`.
    pub fn is_showing_original(&self) -> bool {
        todo!()
    }

    /// C# `PendingLoad != null`: a decode is in flight (its outcome has not been pumped yet).
    pub fn pending_load(&self) -> bool {
        todo!()
    }

    /// C# `IsRawDecodeIndicatorVisible`.
    pub fn is_raw_decode_indicator_visible(&self) -> bool {
        todo!()
    }

    /// Number of worker completions waiting for [`pump`](Self::pump).
    pub fn queued_events(&self) -> usize {
        todo!()
    }

    /// Hang guard for tests: blocks until a worker completion is queued or `timeout` elapsed.
    pub fn wait_for_event(&self, timeout: Duration) -> bool {
        todo!()
    }

    /// C# `Reset()` (navigation started or the presentation was cleared): cancels the not-yet-started decode, stops the RAW
    /// indicator (effect `RawDecodeIndicatorChanged { visible: false }` when it was visible), drops the target and the held
    /// original (the ~96 MB pixels), clears `showing_original` and the refresh hold. Does NOT reset the failed-token latch.
    pub fn reset(&mut self) -> ZoomDetailEffects {
        todo!()
    }

    /// C# `SetZoom(double?)`: `None` = Fit. A zoom different from the stored one (IEEE `!=` on the `Option`) ends the refresh
    /// hold; then `update`.
    pub fn set_zoom(&mut self, zoom: Option<f64>) -> ZoomDetailEffects {
        todo!()
    }

    /// C# `Refresh(double effectiveZoom)` (Q-TOUCHPAD-REFRESH): see the contract for the five outcomes and their order.
    pub fn refresh(&mut self, effective_zoom: f64) -> (ZoomDetailRefreshResult, ZoomDetailEffects) {
        todo!()
    }

    /// C# `OnPreviewPresented(token, path, key, preview)`: `reset`, then arm a target for a downscaled preview unless the
    /// RAW / Original-mode rules say the shown bitmap is already as good as it gets; then `update`.
    pub fn on_preview_presented(
        &mut self,
        token: i64,
        path: &str,
        key: &ImageCacheKey,
        preview: &PreviewImage,
    ) -> ZoomDetailEffects {
        todo!()
    }

    /// Applies every queued worker completion (decode outcomes, indicator delays) on the UI thread, in arrival order, and
    /// returns their effects. This is the C# `LoadAsync` / `ShowIndicatorAfterDelayAsync` continuation code.
    pub fn pump(&mut self) -> ZoomDetailEffects {
        todo!()
    }

    // ---- private, one per C# member ----

    /// C# `Update()`.
    fn update(&mut self, effects: &mut ZoomDetailEffects) {
        todo!()
    }

    /// C# `LoadAsync` continuation: the code after `await DecodeOriginalAsync`, the `catch` blocks and the `finally`.
    fn on_original_decoded(
        &mut self,
        load_id: u64,
        target_id: u64,
        result: PreviewResult,
        effects: &mut ZoomDetailEffects,
    ) {
        todo!()
    }

    /// C# `CancelPending()`: cancels and forgets the in-flight decode token.
    fn cancel_pending(&mut self) {
        todo!()
    }

    /// C# `StartRawDecodeIndicator(loadToken)`: `stop`, then a new slot and a worker that runs the delay.
    fn start_raw_decode_indicator(&mut self, effects: &mut ZoomDetailEffects) {
        todo!()
    }

    /// C# `StopRawDecodeIndicator()` (idempotent): forget and cancel the slot, hide the indicator.
    fn stop_raw_decode_indicator(&mut self, effects: &mut ZoomDetailEffects) {
        todo!()
    }

    /// C# `SetRawDecodeIndicatorVisible(visible)`: pushes an effect only on a transition.
    fn set_raw_decode_indicator_visible(&mut self, visible: bool, effects: &mut ZoomDetailEffects) {
        todo!()
    }

    /// C# `ShowIndicatorAfterDelayAsync` tail (`ShowIfCurrent`): visible only when the slot is still the current one and not cancelled.
    fn on_indicator_elapsed(&mut self, indicator_id: u64, effects: &mut ZoomDetailEffects) {
        todo!()
    }
}
