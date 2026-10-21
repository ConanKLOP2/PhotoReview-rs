//! Port of `PhotoReview.Shell.Win32/ShellMainWindow.Viewing.cs` (336 lines, baseline `d92d6aae`): the glue between the main window, the view model and the
//! presentation layer (`PresentationComposer`, `PresentationPrefetcher`, W8-16c/16d): image changes with crossfade, upgrade promotion, compare, prefetch,
//! `GpuImageCache.Retain`, the GPU cache clear (INV-2) and `DrawPresentation`. Unit W8-13j. Contract: `contracts/W8-13j.md`.
//!
//! What is NOT in C#: the hub calls the window's `ImageChanged` handler while the view model is borrowed (the sink replay runs inside a view-model call), so the
//! handler only QUEUES the image ([`MainWindowInner::on_hub_image_changed`]) and `apply_changes` (13h) handles the queue after the borrow was released
//! ([`MainWindowInner::drain_pending_images`], D-W8B4M-7). `present` still draws the first image in the same UI turn (`render_now`), as C# does.

#![allow(unused_variables, dead_code)] // W8-13j stub: the porter removes this line when the last `todo!()` is gone.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, VecDeque};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::services::render::DrawContext;
use pr_core::model::enums::ScalingQuality;
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::pixels::PixelBuffer;
use pr_imaging::service::CacheClearedSubscription;

use super::core::{MainWindow, MainWindowInner};
use crate::shell_viewing::frame_stats::FrameStats;
use crate::shell_viewing::pacer::FramePacer;

/// C# `DeferUploadBytes` = 16 MiB: an upgrade of the SAME image whose bitmap is larger than this is uploaded in the background before it replaces the
/// current one (about 4 MP, 6 ms of synchronous upload); a smaller one replaces at once.
pub const DEFER_UPLOAD_BYTES: i64 = 16 * 1024 * 1024;

/// Interim twin of the C# `PresentationInputs` record (16c `composer::PresentationInputs<'a>`, which borrows the compare frame). 16c's type wins; the adapter
/// that implements [`ComposerPort`] for `PresentationComposer` converts field by field.
#[derive(Debug, Clone)]
pub struct PresentationInputs {
    /// `_window.ClientSizeDip` (converted from the host twin to `pr_app`'s `SizeD`).
    pub viewport: SizeD,
    /// `RectD(origin.X, origin.Y, ImageActualWidth, ImageActualHeight)`.
    pub main_destination: RectD,
    /// `viewer.EffectiveScalingQuality`.
    pub effective_quality: ScalingQuality,
    /// `viewer.ScalingQuality`.
    pub compare_quality: ScalingQuality,
    /// `_window.DpiScale`.
    pub dpi_scale: f64,
    /// `compare.IsVisible`.
    pub compare_visible: bool,
    /// `CompareFrame(left, right, isLeftSelected, isRightSelected)`.
    pub compare: CompareFrameOwned,
}

/// Interim owned twin of 16c's `CompareFrame<'a>` (the left/right bitmaps of the compare view model, selection flags).
#[derive(Debug, Clone, Default)]
pub struct CompareFrameOwned {
    pub left: Option<Arc<PixelBuffer>>,
    pub right: Option<Arc<PixelBuffer>>,
    pub is_left_selected: bool,
    pub is_right_selected: bool,
}

/// What one `draw` did (16c `ComposedFrame`; only the members the window reads). Interim twin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComposedFrameSummary {
    pub main_drawn: bool,
    pub compare_images: i32,
}

/// A composer call failed (16c `ComposerError`: disposed, or the GPU cache refused a prefetch). Logged by the window, never thrown.
pub type ComposerFailure = Box<dyn std::error::Error>;

/// The members of `PresentationComposer` (C#, W8-16c, `shell_viewing::composer`) that the main window calls. INTERIM: an adapter next to 16c's type implements
/// this trait for it (the signatures follow 16c's), or the lead replaces `Rc<dyn ComposerPort>` by `Rc<PresentationComposer>`. UI thread, all `&self`.
pub trait ComposerPort {
    /// C# `Set(PixelBuffer)`.
    fn set(&self, image: Option<Arc<PixelBuffer>>);
    /// C# `SetIncoming(buffer, isFileChange, deferUploadBytes)`: `Ok(true)` = the incoming bitmap replaced `Current` at once.
    fn set_incoming(
        &self,
        image: Option<Arc<PixelBuffer>>,
        is_file_change: bool,
        defer_upload_bytes: i64,
    ) -> Result<bool, ComposerFailure>;
    /// C# `Current`.
    fn current(&self) -> Option<Arc<PixelBuffer>>;
    /// C# `TryPromotePending()`: whether it promoted.
    fn try_promote_pending(&self) -> bool;
    /// C# `BeginCrossfade(TimeSpan)`: `Ok(true)` when a fade started.
    fn begin_crossfade(&self, duration: Duration) -> Result<bool, ComposerFailure>;
    /// C# `CancelCrossfade()`.
    fn cancel_crossfade(&self);
    /// C# `CollectRetainedIds`.
    fn collect_retained_ids(&self, ids: &mut BTreeSet<i64>);
    /// C# `Draw(context, inputs)`.
    fn draw(
        &self,
        context: &mut dyn DrawContext,
        inputs: &PresentationInputs,
    ) -> Result<ComposedFrameSummary, ComposerFailure>;
    /// C# `Crossfade.IsActive` (the note of a slow frame).
    fn crossfade_is_active(&self) -> bool;
    /// C# `CurrentChanged += handler` (an upgrade finished uploading and became the current image); the id unsubscribes.
    fn subscribe_current_changed(&self, handler: Rc<dyn Fn()>) -> u64;
    /// C# `CurrentChanged -= handler`.
    fn unsubscribe_current_changed(&self, id: u64);
    /// C# `Dispose()`: idempotent.
    fn dispose(&self);
}

/// One RAM-cache lookup the host runs off the UI thread (16d `LookupRequest`). Interim twin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupRequest {
    pub generation: u64,
    pub paths: Vec<String>,
}

/// One retry pass the host schedules (16d `RetryRequest`). Interim twin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryRequest {
    pub generation: u64,
    pub index: i32,
    pub count: i32,
}

/// The members of `PresentationPrefetcher` (C#, W8-16d, `shell_viewing::prefetcher`) that the window and its host adapter call. INTERIM, as [`ComposerPort`].
pub trait PrefetcherPort {
    /// C# `Navigated(index, count)`.
    fn navigated(&self, index: i32, count: i32);
    /// C# `PlannedIds`.
    fn planned_ids(&self) -> Vec<i64>;
    /// 16d: the result of a [`LookupRequest`], on the UI thread.
    fn apply_lookup(&self, generation: u64, found: &[Option<Arc<PixelBuffer>>]);
    /// 16d: a [`RetryRequest`] came due, on the UI thread.
    fn run_retry(&self, request: RetryRequest);
    /// C# `PlanChanged += handler`; the id unsubscribes.
    fn subscribe_plan_changed(&self, handler: Rc<dyn Fn()>) -> u64;
    /// C# `PlanChanged -= handler`.
    fn unsubscribe_plan_changed(&self, id: u64);
}

/// C# `IPrefetchHost` as 16d reshaped it (UI thread only; 16d's trait wins, this is its interim shape): the window implements it
/// ([`MainWindowPrefetchHost`], C# `PrefetchHost(ShellMainWindow owner)`).
pub trait PrefetchHost {
    /// C# `PathAt(index)`: the catalog path, `None` outside the catalog.
    fn path_at(&self, index: i32) -> Option<String>;
    /// C# `RunBackground(() => PostToUi(Apply(LookupCached(paths))))`: never inline.
    fn start_lookup(&self, request: LookupRequest);
    /// C# `ScheduleOnUi(delay, pass)`.
    fn schedule_retry(&self, delay: Duration, request: RetryRequest);
}

/// C# `PrefetchHost` of the main window. Holds a `Weak` (never keeps the window alive); every method runs on the UI thread.
/// `path_at` reads the catalog (short borrow of the view model). `start_lookup` moves the request and an `Arc<PreviewImageService>` to a worker thread, which
/// calls `try_get_cached_preview_path` per path (every key error is a quiet miss) and posts `prefetcher.apply_lookup(generation, &found)` through
/// `post_to_window` at `UiPriority::Background` (skipped when the window is gone). `schedule_retry` is `poster.post_after(delay, Background, ..)` ending in
/// `prefetcher.run_retry(request)`.
pub struct MainWindowPrefetchHost {
    pub(super) window: Weak<MainWindowInner>,
}

impl std::fmt::Debug for MainWindowPrefetchHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainWindowPrefetchHost").finish_non_exhaustive()
    }
}

impl PrefetchHost for MainWindowPrefetchHost {
    fn path_at(&self, index: i32) -> Option<String> {
        todo!("W8-13j")
    }
    fn start_lookup(&self, request: LookupRequest) {
        todo!("W8-13j")
    }
    fn schedule_retry(&self, delay: Duration, request: RetryRequest) {
        todo!("W8-13j")
    }
}

/// One image the hub reported, waiting for `drain_pending_images`.
#[derive(Debug, Clone)]
pub struct PendingImage {
    /// `image as PixelBuffer`: `None` for an opaque (non-pixel) platform image, an empty folder or compare mode.
    pub buffer: Option<Arc<PixelBuffer>>,
    /// The hub's `isFileChange`.
    pub is_file_change: bool,
}

/// The fields that `ShellMainWindow.Viewing.cs` added (`_viewingClock` is `MainWindowInner::frame_clock`).
pub struct ViewingState {
    /// C# `_composer`.
    pub(super) composer: RefCell<Option<Rc<dyn ComposerPort>>>,
    /// C# `_prefetcher`.
    pub(super) prefetcher: RefCell<Option<Rc<dyn PrefetcherPort>>>,
    /// Ids of the subscriptions on the composer (`CurrentChanged`) and the prefetcher (`PlanChanged`).
    pub(super) composer_subscription: Cell<Option<u64>>,
    pub(super) prefetcher_subscription: Cell<Option<u64>>,
    /// C# `_onCacheCleared` on `_clearSource` (the preview service); dropping it unsubscribes.
    pub(super) cache_cleared: RefCell<Option<CacheClearedSubscription>>,
    /// Images reported by the hub and not yet handled (FIFO).
    pub(super) pending_images: RefCell<VecDeque<PendingImage>>,
    /// C# `_pacer` (`FramePacer`).
    pub(super) pacer: RefCell<FramePacer>,
    /// C# `Stats` (`FrameStats`, built with `STOPWATCH_FREQUENCY`).
    pub(super) stats: RefCell<FrameStats>,
    /// C# `GpuCacheClearCount`.
    pub(super) gpu_cache_clear_count: Cell<i32>,
}

impl std::fmt::Debug for ViewingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewingState").finish_non_exhaustive()
    }
}

impl Default for ViewingState {
    /// No composer, no prefetcher, empty queue, fresh pacer and statistics, count 0.
    fn default() -> Self {
        todo!("W8-13j")
    }
}

impl MainWindowInner {
    /// C# `InitializeViewing(frameClock)` (lines 46-59), called from the constructor before a surface exists: the pacer `Degraded` event logs
    /// "Present(1) blocked the UI thread; the shell now presents without waiting for vblank (WP-21)." as a warning (the `FramePacer` of 16a has no event;
    /// `present` logs once when `observe_present` first returns `true`); subscribes `CacheCleared` on `vm.preview_service()` with a `Send` listener that posts
    /// `clear_gpu_cache` (default priority) through `post_to_window`. (`ImageChanging`, the compare and navigation `PropertyChanged` subscriptions are gone:
    /// they are steps of `apply_changes`.)
    pub(super) fn initialize_viewing(&self) {
        todo!("W8-13j")
    }

    /// The hub's `ImageChanged` handler (C# `OnImageChanged` -> `HandleImageChanged`): pushes a [`PendingImage`] and posts a safety-net job at `UiPriority::Render`
    /// that calls `drain_pending_images` (a hub call whose caller forgot `apply_changes` is still handled). Does not touch the view model.
    pub(super) fn on_hub_image_changed(&self, image: Option<&PlatformImage>, is_file_change: bool) {
        todo!("W8-13j")
    }

    /// Handles every queued image in FIFO order with [`handle_image_changed`](Self::handle_image_changed). Called by `apply_changes` (step 2) and by the
    /// safety-net job; re-entrant calls (the first-image `render_now` can raise messages) find the queue already popped.
    pub(super) fn drain_pending_images(&self) {
        todo!("W8-13j")
    }

    /// C# `AttachViewing()` (62-73): builds `PresentationComposer::new(cache, frame_clock, invalidate, schedule_on_ui, ComposerOptions::default())` (invalidate =
    /// `window.invalidate`, `schedule_on_ui` = `poster.post_after`) and `PresentationPrefetcher::new(cache, MainWindowPrefetchHost)` (16c/16d), then
    /// [`attach_viewing_with`](Self::attach_viewing_with).
    pub(super) fn attach_viewing(&self) {
        todo!("W8-13j")
    }

    /// The part of `attach_viewing` after construction, so tests can inject fakes: store both, subscribe `CurrentChanged` -> `on_composer_current_changed` and
    /// `PlanChanged` -> `on_prefetch_plan_changed` (closures with a `Weak`), and `composer.set(image)` when `image` exists and is not disposed.
    pub(super) fn attach_viewing_with(&self, composer: Rc<dyn ComposerPort>, prefetcher: Rc<dyn PrefetcherPort>) {
        todo!("W8-13j")
    }

    /// C# `DisposeViewing()` (75-88), idempotent: drop the cache-cleared subscription, unsubscribe from composer and prefetcher, `composer.dispose()`, composer
    /// and prefetcher to `None`, clear the pending-image queue.
    pub(super) fn dispose_viewing(&self) {
        todo!("W8-13j")
    }

    /// C# `OnImageChanging(sender, e)` (96-111): synchronous, BEFORE the new bitmap replaces the old one, so the last presented frame can freeze for the
    /// crossfade. Does nothing when disposed, without a composer, or when `!is_file_change`; `settings.image_transition == None` -> nothing; else
    /// `composer.begin_crossfade(clamp(settings.image_transition_ms, MIN, MAX) ms)` (an `Err` is logged).
    pub(super) fn on_image_changing(&self, is_file_change: bool) {
        todo!("W8-13j")
    }

    /// C# `HandleImageChanged(image, isFileChange)` (114-139). Without a composer: `image = buffer`. With one: `replace = composer.set_incoming(buffer,
    /// is_file_change, DEFER_UPLOAD_BYTES)` (an `Err` is logged and counts as `false`); `replace` -> `image = composer.current()`; `update_retained()`. Then
    /// `viewport.invalidate_layout()`, `window.invalidate()`, and when an image exists, the window is not disposed and `!first_image_drawn`: `render_now()` IN
    /// THIS CALL (the "first image drawn" mark must not wait for the next frame tick).
    pub(super) fn handle_image_changed(&self, image: Option<Arc<PixelBuffer>>, is_file_change: bool) {
        todo!("W8-13j")
    }

    /// C# `OnComposerCurrentChanged` (142-147): `image = composer.current()`; `viewport.invalidate_layout()`; `update_retained()`.
    pub(super) fn on_composer_current_changed(&self) {
        todo!("W8-13j")
    }

    /// C# `OnCompareChanged` (149-158): unless disposed, `update_retained()` and `window.invalidate()`.
    pub(super) fn on_compare_changed(&self) {
        todo!("W8-13j")
    }

    /// C# `OnNavigationChanged` for `CurrentIndex` (160-168): unless disposed or without a prefetcher, `prefetcher.navigated(vm.current_index(), vm.total_files())`.
    pub(super) fn on_navigation_changed(&self) {
        todo!("W8-13j")
    }

    /// C# `OnPrefetchPlanChanged` (170): `update_retained()`.
    pub(super) fn on_prefetch_plan_changed(&self) {
        todo!("W8-13j")
    }

    /// C# `UpdateRetained()` (176-208): needs a cache and a composer; ids = `composer.collect_retained_ids` + `prefetcher.planned_ids()` + (compare visible)
    /// the ids of the left and right bitmaps that are not disposed; `cache.retain(&ids)` (an `Err` is logged, not thrown).
    pub(super) fn update_retained(&self) {
        todo!("W8-13j")
    }

    /// C# `ClearGpuCache()` (214-225), posted by the cache-cleared listener: unless disposed, `gpu_cache_clear_count += 1`, `cache.clear()`,
    /// `composer.cancel_crossfade()`, `window.invalidate()`. (INV-2: a GPU bitmap must not outlive the RAM entry; the current image still has its CPU buffer
    /// and is uploaded again by the next frame.)
    pub(super) fn clear_gpu_cache(&self) {
        todo!("W8-13j")
    }

    /// C# `DrawPresentation(context)` (229-261): without a composer nothing. `inputs` from the viewer (`effective_scaling_quality`, `scaling_quality`), the
    /// viewport (`image_origin`, `image_actual_width/height`), the window (`client_size_dip` converted to `pr_app`'s `SizeD`, `dpi_scale`) and `vm.compare()`
    /// (short borrow); `frame = composer.draw(context, &inputs)` (an `Err` is logged and the frame goes on). `image_drawn_in_frame = true` when `frame.main_drawn`
    /// and `composer.current()` is the image the presenter assigned (`Arc::ptr_eq` with `hub.current_image()`'s pixels), else when `frame.compare_images > 0`.
    pub(super) fn draw_presentation(&self, context: &mut dyn DrawContext) {
        todo!("W8-13j")
    }

    /// C# `TrySelectCompareAt` (267-285): not disposed and compare visible; `CompareLayout::hit_test(client_size_dip, point)` (16b): Left -> select left, Right ->
    /// select right (through the view model, see the contract: `MainViewModel` has no public selector yet), result `true`; otherwise `false`.
    pub(super) fn try_select_compare_at(&self, client_dip: PointD) -> bool {
        todo!("W8-13j")
    }
}

impl MainWindow {
    /// C# `GpuCacheClearCount`.
    pub fn gpu_cache_clear_count(&self) -> i32 {
        todo!("W8-13j")
    }

    /// C# `Composer` (tests and other packs).
    pub fn composer(&self) -> Option<Rc<dyn ComposerPort>> {
        todo!("W8-13j")
    }

    /// C# `Prefetcher`.
    pub fn prefetcher(&self) -> Option<Rc<dyn PrefetcherPort>> {
        todo!("W8-13j")
    }

    /// C# `Stats`: runs `f` on the frame statistics.
    pub fn with_stats<R>(&self, f: impl FnOnce(&mut FrameStats) -> R) -> R {
        todo!("W8-13j")
    }

    /// C# `Pacer`: runs `f` on the frame pacer.
    pub fn with_pacer<R>(&self, f: impl FnOnce(&mut FramePacer) -> R) -> R {
        todo!("W8-13j")
    }

    /// C# `TrySelectCompareAt(PointD clientDip)` (`internal`; the pointer input of 14c calls it).
    pub fn try_select_compare_at(&self, client_dip: PointD) -> bool {
        todo!("W8-13j")
    }
}
