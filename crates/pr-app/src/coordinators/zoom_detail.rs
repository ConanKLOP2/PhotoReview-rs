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
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::types::is_raw_path;
use pr_core::sync::CancelToken;
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::service::{PreviewImage, PreviewImageService, PreviewResult};

use crate::misc::app_log::AppLog;

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
        if let Err(error) = std::thread::Builder::new().name(name.to_owned()).spawn(job) {
            AppLog::global().error(&format!("Could not start the worker thread {name}"), Some(&error));
        }
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
    match (a, b) {
        (PlatformImage::Pixels(a), PlatformImage::Pixels(b)) => Arc::ptr_eq(a, b),
        (PlatformImage::Opaque(a), PlatformImage::Opaque(b)) => std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b)),
        _ => false,
    }
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
    /// Longest wait between two checks of the token.
    const SLICE: Duration = Duration::from_millis(20);
    Arc::new(|delay: Duration, cancel: &CancelToken| {
        let deadline = Instant::now() + delay;
        loop {
            if cancel.is_canceled() {
                return false;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return true;
            }
            std::thread::sleep(remaining.min(SLICE));
        }
    })
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
    started: Instant,
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
    /// FEAT-RENAME (C# `RetargetPath(long newToken, string newPath)`, W9-R6 stub): the file behind the current target was
    /// renamed (same pixels). Cancels the not-yet-started decode, re-arms the target under `new_path` and the navigation
    /// `new_token` with a NEW target `id` (a decode already running for the old path must see another target and drop its
    /// pixels), keeps a held original and the shown state, moves a latched failure to the new token, and restarts the
    /// decode when zoomed without an original. No target is a no-op; a blank path is a caller bug (debug assertion).
    pub fn retarget_path(&mut self, new_token: i64, new_path: &str) -> ZoomDetailEffects {
        let _ = (new_token, new_path);
        todo!()
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
        Self {
            service,
            clock,
            runner,
            inbox: CompletionInbox::new(wake),
            indicator_delay: default_indicator_delay(),
            target: None,
            original: None,
            showing_original: false,
            zoom: None,
            refresh_hold: false,
            load: None,
            failed_token: -1,
            indicator: None,
            raw_indicator_visible: false,
            next_id: 1,
        }
    }

    /// C# `IndicatorDelay { set }` (internal test seam): replaces the 300 ms wait.
    #[must_use]
    pub fn with_indicator_delay(mut self, delay: IndicatorDelay) -> Self {
        self.indicator_delay = delay;
        self
    }

    /// C# `HeldOriginal`: the full-resolution decode held for the current image.
    pub fn held_original(&self) -> Option<&PreviewImage> {
        self.original.as_ref()
    }

    /// C# `IsShowingOriginal`.
    pub fn is_showing_original(&self) -> bool {
        self.showing_original
    }

    /// C# `PendingLoad != null`: a decode is in flight (its outcome has not been pumped yet).
    pub fn pending_load(&self) -> bool {
        self.load.is_some()
    }

    /// C# `IsRawDecodeIndicatorVisible`.
    pub fn is_raw_decode_indicator_visible(&self) -> bool {
        self.raw_indicator_visible
    }

    /// Number of worker completions waiting for [`pump`](Self::pump).
    pub fn queued_events(&self) -> usize {
        self.inbox.len()
    }

    /// Hang guard for tests: blocks until a worker completion is queued or `timeout` elapsed.
    pub fn wait_for_event(&self, timeout: Duration) -> bool {
        self.inbox.wait_ready(timeout)
    }

    /// C# `Reset()` (navigation started or the presentation was cleared): cancels the not-yet-started decode, stops the RAW
    /// indicator (effect `RawDecodeIndicatorChanged { visible: false }` when it was visible), drops the target and the held
    /// original (the ~96 MB pixels), clears `showing_original` and the refresh hold. Does NOT reset the failed-token latch.
    pub fn reset(&mut self) -> ZoomDetailEffects {
        let mut effects = ZoomDetailEffects::new();
        self.cancel_pending();
        self.stop_raw_decode_indicator(&mut effects);
        self.target = None;
        self.original = None;
        self.showing_original = false;
        self.refresh_hold = false;
        effects
    }

    /// C# `SetZoom(double?)`: `None` = Fit. A zoom different from the stored one (IEEE `!=` on the `Option`) ends the refresh
    /// hold; then `update`.
    pub fn set_zoom(&mut self, zoom: Option<f64>) -> ZoomDetailEffects {
        // A Refresh's "full resolution at Fit" lasts until the view actually changes zoom (a repeated Fit keeps it).
        if zoom != self.zoom {
            self.refresh_hold = false;
        }
        self.zoom = zoom;
        let mut effects = ZoomDetailEffects::new();
        self.update(&mut effects);
        effects
    }

    /// C# `Refresh(double effectiveZoom)` (Q-TOUCHPAD-REFRESH): see the contract for the five outcomes and their order.
    pub fn refresh(&mut self, effective_zoom: f64) -> (ZoomDetailRefreshResult, ZoomDetailEffects) {
        let mut effects = ZoomDetailEffects::new();
        // No target: the shown bitmap is already full resolution (Original mode, small source, RAW without full decode) or no image.
        let Some((token, original_width, preview_pixel_width)) = self
            .target
            .as_ref()
            .map(|target| (target.token, target.original_width, target.preview_pixel_width))
            .filter(|(token, _, _)| self.clock.is_navigation_current(*token))
        else {
            return (ZoomDetailRefreshResult::AlreadyFullResolution, effects);
        };
        let retried = self.failed_token == token;
        // Cleared before the early returns below: a Refresh always re-arms the retry.
        self.failed_token = -1;
        if self.showing_original {
            return (ZoomDetailRefreshResult::AlreadyFullResolution, effects);
        }
        // NaN and non-positive zooms land here, like C# `!(effectiveZoom > 0)`.
        if effective_zoom.is_nan()
            || effective_zoom <= 0.0
            || effective_zoom * f64::from(original_width) <= f64::from(preview_pixel_width)
        {
            return (ZoomDetailRefreshResult::PreviewSufficient, effects);
        }

        self.refresh_hold = true;
        let held = self.original.is_some();
        self.update(&mut effects);
        let result = if held || self.showing_original {
            ZoomDetailRefreshResult::ShowedOriginal
        } else if retried {
            ZoomDetailRefreshResult::RetryingOriginal
        } else {
            ZoomDetailRefreshResult::LoadingOriginal
        };
        (result, effects)
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
        let mut effects = self.reset();
        if !preview.downscaled() {
            return effects; // already full resolution
        }
        // A RAW's shown image is only its embedded JPEG (downscaled while smaller than the sensor). The only thing that can
        // serve more pixels is the RAW full decoder: when it is not in use never arm a target in ANY loading mode -- the
        // ordinary decoder would just re-decode the same embedded JPEG and report its own smaller size.
        if !self.service.is_raw_full_decode_request(path) && (is_raw_path(path) || key.is_original()) {
            // (a RAW path in any mode, or an Original-mode key of an ordinary image, which is already full resolution)
            return effects;
        }
        let id = self.next_id();
        self.target = Some(Target {
            id,
            token,
            path: path.to_owned(),
            key: key.clone(),
            preview: preview.platform_image().clone(),
            preview_pixel_width: preview.pixel_width(),
            original_width: preview.original_width(),
            original_height: preview.original_height(),
        });
        self.update(&mut effects);
        effects
    }

    /// Applies every queued worker completion (decode outcomes, indicator delays) on the UI thread, in arrival order, and
    /// returns their effects. This is the C# `LoadAsync` / `ShowIndicatorAfterDelayAsync` continuation code.
    pub fn pump(&mut self) -> ZoomDetailEffects {
        let mut effects = ZoomDetailEffects::new();
        for event in self.inbox.drain() {
            match event {
                ZoomDetailEvent::OriginalDecoded {
                    load_id,
                    target_id,
                    result,
                } => self.on_original_decoded(load_id, target_id, result, &mut effects),
                ZoomDetailEvent::IndicatorElapsed { indicator_id } => {
                    self.on_indicator_elapsed(indicator_id, &mut effects);
                }
            }
        }
        effects
    }

    // ---- private, one per C# member ----

    /// The next identity of a target, load or indicator slot.
    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// C# `Update()`.
    fn update(&mut self, effects: &mut ZoomDetailEffects) {
        let Some(target) = self.target.clone() else {
            return;
        };
        if !self.clock.is_navigation_current(target.token) {
            return;
        }

        if self.zoom.is_none() && !self.refresh_hold {
            if self.showing_original {
                self.showing_original = false;
                effects.push(ZoomDetailEffect::ShowImage {
                    image: target.preview,
                    original_width: target.original_width,
                    original_height: target.original_height,
                });
            }
            return;
        }

        if let Some(original) = &self.original {
            if !self.showing_original {
                effects.push(ZoomDetailEffect::ShowImage {
                    image: original.platform_image().clone(),
                    original_width: original.pixel_width(),
                    original_height: original.pixel_height(),
                });
                self.showing_original = true;
                effects.push(ZoomDetailEffect::OriginalShown { path: target.path });
            }
            return;
        }

        // The preview already has at least as many pixels as the screen shows: no decode needed. (A Refresh hold was set only
        // after Refresh measured the opposite, so it skips this check.)
        if !self.refresh_hold
            && self
                .zoom
                .is_some_and(|zoom| zoom * f64::from(target.original_width) <= f64::from(target.preview_pixel_width))
        {
            return;
        }
        if self.load.is_some() {
            return; // already decoding this target
        }
        if self.failed_token == target.token {
            return;
        }

        let cancel = CancelToken::new();
        let load_id = self.next_id();
        self.load = Some(LoadSlot {
            id: load_id,
            cancel: cancel.clone(),
            started: Instant::now(),
        });
        if self.service.is_raw_full_decode_request(&target.path) {
            self.start_raw_decode_indicator(effects);
        }
        let service = Arc::clone(&self.service);
        let inbox = self.inbox.clone();
        let target_id = target.id;
        let (path, key) = (target.path, target.key);
        self.runner.run(
            "zoom-detail-decode",
            Box::new(move || {
                let result = service.decode_original(&path, &key, &cancel).wait();
                inbox.send(ZoomDetailEvent::OriginalDecoded {
                    load_id,
                    target_id,
                    result,
                });
            }),
        );
    }

    /// C# `LoadAsync` continuation: the code after `await DecodeOriginalAsync`, the `catch` blocks and the `finally`.
    fn on_original_decoded(
        &mut self,
        load_id: u64,
        target_id: u64,
        result: PreviewResult,
        effects: &mut ZoomDetailEffects,
    ) {
        let slot_owner = self.load.as_ref().is_some_and(|load| load.id == load_id);
        let elapsed_ms = self
            .load
            .as_ref()
            .filter(|load| load.id == load_id)
            .map_or(0, |load| load.started.elapsed().as_millis());
        let same_target = self.target.as_ref().filter(|target| target.id == target_id);
        let mut accepted = false;
        let mut failed_token = None;
        match result {
            Ok(original) => {
                // INV-1: a navigation (or a newer target) superseded this decode -- drop the pixels.
                if let Some(target) = same_target.filter(|target| self.clock.is_navigation_current(target.token)) {
                    let log = AppLog::global();
                    if log.enabled() {
                        log.info(&format!(
                            "ZoomDetail original ready token={} {}x{} ms={} path={}",
                            target.token,
                            original.pixel_width(),
                            original.pixel_height(),
                            elapsed_ms,
                            target.path
                        ));
                    }
                    self.original = Some(original);
                    accepted = true;
                }
            }
            // A cancelled decode is not a failure: no latch, nothing shown.
            Err(error) if error.is_canceled() => {}
            Err(error) => {
                // Zoom detail is best effort: on any decode failure the preview simply stays on screen.
                let (token, path) = same_target.map_or((-1, ""), |target| (target.token, target.path.as_str()));
                AppLog::global().error(
                    &format!("ZoomDetail original decode failed token={token} path={path}"),
                    Some(&error),
                );
                failed_token = same_target.map(|target| target.token);
            }
        }
        if let Some(token) = failed_token {
            self.failed_token = token;
        }
        // Only the load that still owns the slot may touch the shared state. A superseded load finishing late must not
        // kill the newer load's RAW indicator.
        if slot_owner {
            self.stop_raw_decode_indicator(effects);
            self.load = None;
        }
        if accepted {
            self.update(effects);
        }
    }

    /// C# `CancelPending()`: cancels and forgets the in-flight decode token.
    fn cancel_pending(&mut self) {
        if let Some(load) = self.load.take() {
            load.cancel.cancel();
        }
    }

    /// C# `StartRawDecodeIndicator(loadToken)`: `stop`, then a new slot and a worker that runs the delay.
    fn start_raw_decode_indicator(&mut self, effects: &mut ZoomDetailEffects) {
        self.stop_raw_decode_indicator(effects);
        let cancel = CancelToken::new();
        let indicator_id = self.next_id();
        self.indicator = Some(IndicatorSlot {
            id: indicator_id,
            cancel: cancel.clone(),
        });
        let delay = Arc::clone(&self.indicator_delay);
        let inbox = self.inbox.clone();
        self.runner.run(
            "zoom-detail-indicator",
            Box::new(move || {
                if delay(RAW_INDICATOR_DELAY, &cancel) {
                    inbox.send(ZoomDetailEvent::IndicatorElapsed { indicator_id });
                }
            }),
        );
    }

    /// C# `StopRawDecodeIndicator()` (idempotent): forget and cancel the slot, hide the indicator.
    fn stop_raw_decode_indicator(&mut self, effects: &mut ZoomDetailEffects) {
        let slot = self.indicator.take();
        self.set_raw_decode_indicator_visible(false, effects);
        if let Some(slot) = slot {
            slot.cancel.cancel();
        }
    }

    /// C# `SetRawDecodeIndicatorVisible(visible)`: pushes an effect only on a transition.
    fn set_raw_decode_indicator_visible(&mut self, visible: bool, effects: &mut ZoomDetailEffects) {
        if self.raw_indicator_visible == visible {
            return;
        }
        self.raw_indicator_visible = visible;
        effects.push(ZoomDetailEffect::RawDecodeIndicatorChanged { visible });
    }

    /// C# `ShowIndicatorAfterDelayAsync` tail (`ShowIfCurrent`): visible only when the slot is still the current one and not cancelled.
    fn on_indicator_elapsed(&mut self, indicator_id: u64, effects: &mut ZoomDetailEffects) {
        let current = self
            .indicator
            .as_ref()
            .is_some_and(|slot| slot.id == indicator_id && !slot.cancel.is_canceled());
        if current {
            self.set_raw_decode_indicator_visible(true, effects);
        }
    }
}
