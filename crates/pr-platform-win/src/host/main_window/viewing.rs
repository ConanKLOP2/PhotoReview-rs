//! Port of `PhotoReview.Shell.Win32/ShellMainWindow.Viewing.cs` (336 lines, baseline `d92d6aae`): the glue between the main window, the view model and the
//! presentation layer (`PresentationComposer`, `PresentationPrefetcher`, W8-16c/16d): image changes with crossfade, upgrade promotion, compare, prefetch,
//! `GpuImageCache.Retain`, the GPU cache clear (INV-2) and `DrawPresentation`. Unit W8-13j. Contract: `contracts/W8-13j.md`.
//!
//! What is NOT in C#: the hub calls the window's `ImageChanged` handler while the view model is borrowed (the sink replay runs inside a view-model call), so the
//! handler only QUEUES the image ([`MainWindowInner::on_hub_image_changed`]) and `apply_changes` (13h) handles the queue after the borrow was released
//! ([`MainWindowInner::drain_pending_images`], D-W8B4M-7). `present` still draws the first image in the same UI turn (`render_now`), as C# does.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, VecDeque};
use std::fmt::Debug;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::services::render::gpu_image::PrefetchStatus;
use pr_app::services::render::{DrawContext, GpuImageCache};
use pr_core::abstractions::{UiDispatcher, UiPriority};
use pr_core::diagnostics::perf::STOPWATCH_FREQUENCY;
use pr_core::model::enums::ImageTransition;
use pr_core::settings::app_settings::{MAX_IMAGE_TRANSITION_MS, MIN_IMAGE_TRANSITION_MS};
use pr_imaging::decode_types::{DecodedImage, DecodedImageView, PlatformImage};
use pr_imaging::pixels::PixelBuffer;
use pr_imaging::service::CacheClearedSubscription;

use super::core::{LocalJob, LocalPoster, MainWindow, MainWindowInner, WindowImageCache, post_to_window, with_window};
use crate::host::contracts::ShellWindowApi;
use crate::shell_viewing::compare::{CompareFrame, CompareSide, layout};
use crate::shell_viewing::composer::{
    ComposedFrame, ComposerOptions, PresentationComposer, PresentationInputs, ScheduleOnUi,
};
use crate::shell_viewing::frame_stats::FrameStats;
use crate::shell_viewing::pacer::FramePacer;
use crate::shell_viewing::prefetcher::{
    LookupRequest, PrefetchCache, PrefetchHost, PresentationPrefetcher, RetryRequest,
};

/// C# `DeferUploadBytes` = 16 MiB: an upgrade of the SAME image whose bitmap is larger than this is uploaded in the background before it replaces the
/// current one (about 4 MP, 6 ms of synchronous upload); a smaller one replaces at once.
pub const DEFER_UPLOAD_BYTES: i64 = 16 * 1024 * 1024;

// D-W8B4X-2: `PresentationComposer` (16c) and `PresentationPrefetcher` (16d) are ported BEFORE this unit, so the window uses the concrete types. The interim
// `ComposerPort`, `PrefetcherPort`, `PresentationInputs`, `CompareFrameOwned`, `ComposedFrameSummary`, `LookupRequest`, `RetryRequest` and `PrefetchHost` of the first
// 13j draft are gone; the owners' types (`composer::{PresentationInputs, ComposedFrame, ComposerError}`, `prefetcher::{LookupRequest, RetryRequest, PrefetchHost}`)
// are imported above. Tests inject fakes through the owners' seams (`PrefetchStatus` cache, `HostFrameClock`, `PrefetchCache`, `PrefetchHost`).

/// C# `OnImageChanging`: the crossfade length for the settings, or `None` when the transition is off (`ImageTransition.None`). The setting is clamped to
/// `MIN_IMAGE_TRANSITION_MS..=MAX_IMAGE_TRANSITION_MS`.
pub fn crossfade_duration(transition: ImageTransition, transition_ms: i32) -> Option<Duration> {
    if transition == ImageTransition::None {
        return None;
    }
    let ms = transition_ms.clamp(MIN_IMAGE_TRANSITION_MS, MAX_IMAGE_TRANSITION_MS);
    Some(Duration::from_millis(u64::try_from(ms).unwrap_or(0)))
}

/// C# `DrawPresentation` tail: does the frame count as "the image the presenter just assigned was drawn"? True when the main image was drawn AND the
/// composer's current image is the very buffer of the hub's current image (during an upgrade upload the frame has the OLD image and does not count),
/// else when compare images were drawn.
pub fn frame_contains_assigned_image(
    frame: &ComposedFrame,
    composer_current: Option<&Arc<PixelBuffer>>,
    hub_image: Option<&Arc<PixelBuffer>>,
) -> bool {
    let same = match (composer_current, hub_image) {
        (Some(current), Some(hub)) => Arc::ptr_eq(current, hub),
        (None, None) => true,
        _ => false,
    };
    if frame.main_drawn && same {
        return true;
    }
    frame.compare_images > 0
}

/// C# `UpdateRetained` compare part: the ids of the compare bitmaps that exist and are not disposed.
pub fn compare_retained_ids(images: [Option<&Arc<PixelBuffer>>; 2]) -> Vec<i64> {
    images
        .into_iter()
        .flatten()
        .filter(|buffer| !buffer.is_disposed())
        .map(|buffer| buffer.id())
        .collect()
}

/// C# `PrefetchHost` of the main window. Holds a `Weak` (never keeps the window alive); every method runs on the UI thread.
/// `path_at` reads the catalog (short borrow of the view model). `start_lookup` moves the request and an `Arc<PreviewImageService>` to a worker thread, which
/// calls `try_get_cached_preview_path` per path (every key error is a quiet miss) and posts `prefetcher.apply_lookup(generation, &found)` to the window at
/// `UiPriority::Background` (skipped when the window is gone). `schedule_retry` is `poster.post_after(delay, Background, ..)` ending in
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
        let window = self.window.upgrade()?;
        // A modal command may hold the view model (BACKLOG G1): no path then, the caller treats it as "not known".
        let view_model = window.view_model.try_borrow().ok()?;
        let catalog = view_model.catalog();
        // `ReviewCatalog::path_at` panics outside the range, so the range test comes first.
        let index = usize::try_from(index).ok().filter(|index| *index < catalog.count())?;
        Some(catalog.path_at(index).to_owned())
    }

    fn start_lookup(&self, request: LookupRequest) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        if window.disposed.get() {
            return;
        }
        let Ok(view_model) = window.view_model.try_borrow() else {
            return; // a modal command holds the view model: the lookup is not needed for a window that cannot navigate meanwhile
        };
        let service = Arc::clone(view_model.preview_service());
        drop(view_model);
        let dispatcher = window.dispatcher.clone();
        let id = window.id;
        // T3: the worker holds only `Send` data (the service, the request, the dispatcher, the window id).
        let spawned = std::thread::Builder::new()
            .name("prefetch-lookup".to_owned())
            .spawn(move || {
                let found: Vec<Option<Arc<PixelBuffer>>> = request
                    .paths
                    .iter()
                    .map(|path| {
                        service
                            .try_get_cached_preview_path(path)
                            .and_then(|image| image.platform_image().as_pixels().cloned())
                    })
                    .collect();
                let generation = request.generation;
                dispatcher.post_with_priority(
                    Box::new(move || {
                        with_window(id, |window| window.apply_prefetch_lookup(generation, &found));
                    }),
                    UiPriority::Background,
                );
            });
        if let Err(error) = spawned {
            window.log.error("Prefetch lookup thread failed to start", Some(&error));
        }
    }

    fn schedule_retry(&self, delay: Duration, request: RetryRequest) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let weak = self.window.clone();
        window.poster.post_after(
            delay,
            UiPriority::Background,
            Box::new(move || {
                let Some(window) = weak.upgrade() else {
                    return;
                };
                if window.disposed.get() {
                    return;
                }
                if let Some(prefetcher) = window.current_prefetcher() {
                    prefetcher.run_retry(request);
                }
            }),
        );
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
    pub(super) composer: RefCell<Option<Rc<PresentationComposer>>>,
    /// C# `_prefetcher`.
    pub(super) prefetcher: RefCell<Option<Rc<PresentationPrefetcher>>>,
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
    /// The compare state of the last paint that could borrow the view model; the paint inside a modal command (mutable borrow held) reuses it.
    pub(super) compare_paint_cache: RefCell<ComparePaintSnapshot>,
}

/// `(visible, left, right, left selected, right selected)` as `draw_presentation` reads them from the view model.
pub(super) type ComparePaintSnapshot = (bool, Option<Arc<DecodedImage>>, Option<Arc<DecodedImage>>, bool, bool);

impl std::fmt::Debug for ViewingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewingState").finish_non_exhaustive()
    }
}

impl Default for ViewingState {
    /// No composer, no prefetcher, empty queue, fresh pacer and statistics, count 0.
    fn default() -> Self {
        Self {
            composer: RefCell::new(None),
            prefetcher: RefCell::new(None),
            composer_subscription: Cell::new(None),
            prefetcher_subscription: Cell::new(None),
            cache_cleared: RefCell::new(None),
            pending_images: RefCell::new(VecDeque::new()),
            pacer: RefCell::new(FramePacer::new()),
            stats: RefCell::new(FrameStats::new(STOPWATCH_FREQUENCY)),
            gpu_cache_clear_count: Cell::new(0),
            compare_paint_cache: RefCell::new((false, None, None, false, false)),
        }
    }
}

impl MainWindowInner {
    /// The composer, cloned out of its cell (T8: no cell borrow is held across a call into it).
    fn current_composer(&self) -> Option<Rc<PresentationComposer>> {
        self.viewing.composer.borrow().clone()
    }

    /// The prefetcher, cloned out of its cell.
    fn current_prefetcher(&self) -> Option<Rc<PresentationPrefetcher>> {
        self.viewing.prefetcher.borrow().clone()
    }

    /// The GPU cache, cloned out of its cell (a cell that is being written counts as a missing cache).
    fn current_cache(&self) -> Option<Rc<WindowImageCache>> {
        self.cache.try_borrow().ok().and_then(|cache| cache.clone())
    }

    /// Composer, cache and prefetch errors are logged, never thrown (the C# exceptions were caught by `Present`).
    fn log_viewing_failure(&self, what: &str, error: &dyn Debug) {
        self.log.error(&format!("{what}: {error:?}"), None);
    }

    /// The job of a finished prefetch lookup, on the UI thread: `prefetcher.apply_lookup(generation, &found)`; an `Err` is logged.
    fn apply_prefetch_lookup(&self, generation: u64, found: &[Option<Arc<PixelBuffer>>]) {
        let Some(prefetcher) = self.current_prefetcher() else {
            return;
        };
        if let Err(error) = prefetcher.apply_lookup(generation, found) {
            self.log_viewing_failure("Prefetch lookup failed", &error);
        }
    }

    /// C# `InitializeViewing(frameClock)` (lines 46-59), called from the constructor before a surface exists: the pacer `Degraded` event logs
    /// "Present(1) blocked the UI thread; the shell now presents without waiting for vblank (WP-21)." as a warning (the `FramePacer` of 16a has no event;
    /// `present` logs once when `observe_present` first returns `true`); subscribes `CacheCleared` on `vm.preview_service()` with a `Send` listener that posts
    /// `clear_gpu_cache` (default priority) through `post_to_window`. (`ImageChanging`, the compare and navigation `PropertyChanged` subscriptions are gone:
    /// they are steps of `apply_changes`.)
    pub(super) fn initialize_viewing(&self) {
        let service = Arc::clone(self.view_model.borrow().preview_service());
        let dispatcher = self.dispatcher.clone();
        let id = self.id;
        // T3: the listener runs on whatever thread called `clear_cache`; it holds only `Send` data and posts to the UI thread.
        let subscription = service.subscribe_cache_cleared(Arc::new(move || {
            post_to_window(&dispatcher, id, UiPriority::Normal, MainWindowInner::clear_gpu_cache);
        }));
        *self.viewing.cache_cleared.borrow_mut() = Some(subscription);
    }

    /// The hub's `ImageChanged` handler (C# `OnImageChanged` -> `HandleImageChanged`): pushes a [`PendingImage`] and posts a safety-net job at `UiPriority::Render`
    /// that calls `drain_pending_images` (a hub call whose caller forgot `apply_changes` is still handled). Does not touch the view model.
    pub(super) fn on_hub_image_changed(&self, image: Option<&PlatformImage>, is_file_change: bool) {
        // T1: only the queue; the view model may be borrowed right now.
        self.viewing.pending_images.borrow_mut().push_back(PendingImage {
            buffer: image.and_then(PlatformImage::as_pixels).cloned(),
            is_file_change,
        });
        let weak = self.weak_self.clone();
        let job: LocalJob = Box::new(move || {
            if let Some(window) = weak.upgrade() {
                if !window.disposed.get() {
                    window.drain_pending_images();
                }
            }
        });
        self.poster.post(UiPriority::Render, job);
    }

    /// Handles every queued image in FIFO order with [`handle_image_changed`](Self::handle_image_changed). Called by `apply_changes` (step 2) and by the
    /// safety-net job; re-entrant calls (the first-image `render_now` can raise messages) find the queue already popped.
    pub(super) fn drain_pending_images(&self) {
        loop {
            // The borrow ends before the handler runs (it can render and re-enter).
            let next = self.viewing.pending_images.borrow_mut().pop_front();
            let Some(pending) = next else {
                break;
            };
            self.handle_image_changed(pending.buffer, pending.is_file_change);
        }
    }

    /// C# `AttachViewing()` (62-73): builds `PresentationComposer::new(cache, frame_clock, invalidate, schedule_on_ui, ComposerOptions::default())` (invalidate =
    /// `window.invalidate`, `schedule_on_ui` = `poster.post_after`) and `PresentationPrefetcher::new(cache, MainWindowPrefetchHost)` (16c/16d), then
    /// [`attach_viewing_with`](Self::attach_viewing_with).
    pub(super) fn attach_viewing(&self) {
        let Some(cache) = self.current_cache() else {
            return;
        };
        let weak = self.weak_self.clone();
        let invalidate: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(window) = weak.upgrade() {
                window.window.invalidate();
            }
        });
        let poster = self.poster.clone();
        let schedule_on_ui: ScheduleOnUi = Rc::new(move |delay, job| {
            poster.post_after(delay, UiPriority::Background, job);
        });
        let status: Rc<dyn PrefetchStatus> = cache.clone();
        let composer = Rc::new(PresentationComposer::new(
            status,
            Rc::clone(&self.frame_clock),
            invalidate,
            schedule_on_ui,
            ComposerOptions::default(),
        ));
        let prefetch_cache: Rc<dyn PrefetchCache> = cache;
        let host = Rc::new(MainWindowPrefetchHost {
            window: self.weak_self.clone(),
        });
        let prefetcher = Rc::new(PresentationPrefetcher::new(prefetch_cache, host));
        self.attach_viewing_with(composer, prefetcher);
    }

    /// The part of `attach_viewing` after construction, so tests can inject composers and prefetchers built over fakes: store both, subscribe `CurrentChanged` ->
    /// `on_composer_current_changed` and `PlanChanged` -> `on_prefetch_plan_changed` (closures with a `Weak`), and `composer.set(image)` when `image` exists and is not disposed.
    pub(super) fn attach_viewing_with(
        &self,
        composer: Rc<PresentationComposer>,
        prefetcher: Rc<PresentationPrefetcher>,
    ) {
        *self.viewing.composer.borrow_mut() = Some(Rc::clone(&composer));
        *self.viewing.prefetcher.borrow_mut() = Some(Rc::clone(&prefetcher));
        let weak = self.weak_self.clone();
        let composer_id = composer.subscribe_current_changed(Rc::new(move || {
            if let Some(window) = weak.upgrade() {
                window.on_composer_current_changed();
            }
        }));
        self.viewing.composer_subscription.set(Some(composer_id));
        let weak = self.weak_self.clone();
        let prefetcher_id = prefetcher.subscribe_plan_changed(Rc::new(move || {
            if let Some(window) = weak.upgrade() {
                window.on_prefetch_plan_changed();
            }
        }));
        self.viewing.prefetcher_subscription.set(Some(prefetcher_id));
        // A bitmap that arrived before the renderer existed.
        let existing = self.image.borrow().clone().filter(|buffer| !buffer.is_disposed());
        if existing.is_some() {
            composer.set(existing);
        }
    }

    /// C# `DisposeViewing()` (75-88), idempotent: drop the cache-cleared subscription, unsubscribe from composer and prefetcher, `composer.dispose()`, composer
    /// and prefetcher to `None`, clear the pending-image queue.
    pub(super) fn dispose_viewing(&self) {
        let cache_cleared = self.viewing.cache_cleared.borrow_mut().take();
        drop(cache_cleared);
        let composer = self.viewing.composer.borrow_mut().take();
        let prefetcher = self.viewing.prefetcher.borrow_mut().take();
        if let (Some(prefetcher), Some(id)) = (prefetcher.as_ref(), self.viewing.prefetcher_subscription.take()) {
            prefetcher.unsubscribe_plan_changed(id);
        }
        if let Some(composer) = composer.as_ref() {
            if let Some(id) = self.viewing.composer_subscription.take() {
                composer.unsubscribe_current_changed(id);
            }
            composer.dispose();
        }
        self.viewing.pending_images.borrow_mut().clear();
    }

    /// C# `OnImageChanging(sender, e)` (96-111): synchronous, BEFORE the new bitmap replaces the old one, so the last presented frame can freeze for the
    /// crossfade. Does nothing when disposed, without a composer, or when `!is_file_change`; `settings.image_transition == None` -> nothing; else
    /// `composer.begin_crossfade(clamp(settings.image_transition_ms, MIN, MAX) ms)` (an `Err` is logged).
    pub(super) fn on_image_changing(&self, is_file_change: bool) {
        if self.disposed.get() || !is_file_change {
            return;
        }
        let Some(composer) = self.current_composer() else {
            return;
        };
        let Ok(view_model) = self.view_model.try_borrow() else {
            return;
        };
        let settings = view_model.settings();
        drop(view_model);
        let Some(duration) = crossfade_duration(settings.image_transition, settings.image_transition_ms) else {
            return;
        };
        if let Err(error) = composer.begin_crossfade(duration) {
            self.log_viewing_failure("Crossfade could not start", &error);
        }
    }

    /// C# `HandleImageChanged(image, isFileChange)` (114-139). Without a composer: `image = buffer`. With one: `replace = composer.set_incoming(buffer,
    /// is_file_change, DEFER_UPLOAD_BYTES)` (an `Err` is logged and counts as `false`); `replace` -> `image = composer.current()`; `update_retained()`. Then
    /// `viewport.invalidate_layout()`, `window.invalidate()`, and when an image exists, the window is not disposed and `!first_image_drawn`: `render_now()` IN
    /// THIS CALL (the "first image drawn" mark must not wait for the next frame tick).
    pub(super) fn handle_image_changed(&self, image: Option<Arc<PixelBuffer>>, is_file_change: bool) {
        if let Some(composer) = self.current_composer() {
            let replace = match composer.set_incoming(image, is_file_change, DEFER_UPLOAD_BYTES) {
                Ok(replace) => replace,
                Err(error) => {
                    self.log_viewing_failure("Image could not be handed to the composer", &error);
                    false
                }
            };
            if replace {
                *self.image.borrow_mut() = composer.current();
            }
            // After `set_incoming`: a pending upgrade must be in the retained set before the cache can evict it.
            self.update_retained();
        } else {
            *self.image.borrow_mut() = image;
        }
        self.viewport.invalidate_layout();
        self.window.invalidate();
        let has_image = self.image.borrow().is_some();
        if has_image && !self.disposed.get() && !self.frame.first_image_drawn.get() {
            // The first image is drawn IN this turn; its mark does not wait for the next frame tick.
            self.render_now();
        }
    }

    /// C# `OnComposerCurrentChanged` (142-147): `image = composer.current()`; `viewport.invalidate_layout()`; `update_retained()`.
    pub(super) fn on_composer_current_changed(&self) {
        let current = self.current_composer().and_then(|composer| composer.current());
        *self.image.borrow_mut() = current;
        self.viewport.invalidate_layout();
        self.update_retained();
    }

    /// C# `OnCompareChanged` (149-158): unless disposed, `update_retained()` and `window.invalidate()`.
    pub(super) fn on_compare_changed(&self) {
        if self.disposed.get() {
            return;
        }
        self.update_retained();
        self.window.invalidate();
    }

    /// C# `OnNavigationChanged` for `CurrentIndex` (160-168): unless disposed or without a prefetcher, `prefetcher.navigated(vm.current_index(), vm.total_files())`.
    pub(super) fn on_navigation_changed(&self) {
        if self.disposed.get() {
            return;
        }
        let Some(prefetcher) = self.current_prefetcher() else {
            return;
        };
        let (index, count) = {
            let Ok(view_model) = self.view_model.try_borrow() else {
                return;
            };
            (
                view_model.current_index(),
                i32::try_from(view_model.total_files()).unwrap_or(i32::MAX),
            )
        };
        prefetcher.navigated(index, count);
    }

    /// C# `OnPrefetchPlanChanged` (170): `update_retained()`.
    pub(super) fn on_prefetch_plan_changed(&self) {
        self.update_retained();
    }

    /// C# `UpdateRetained()` (176-208): needs a cache and a composer; ids = `composer.collect_retained_ids` + `prefetcher.planned_ids()` + (compare visible)
    /// the ids of the left and right bitmaps that are not disposed; `cache.retain(&ids)` (an `Err` is logged, not thrown).
    pub(super) fn update_retained(&self) {
        let (Some(cache), Some(composer)) = (self.current_cache(), self.current_composer()) else {
            return;
        };
        let mut ids = BTreeSet::new();
        composer.collect_retained_ids(&mut ids);
        if let Some(prefetcher) = self.current_prefetcher() {
            ids.extend(prefetcher.planned_ids());
        }
        // A modal command holds the mutable borrow while its nested loop runs: retaining is housekeeping, the next change retains again.
        let compare_ids = {
            let Ok(view_model) = self.view_model.try_borrow() else {
                return;
            };
            let compare = view_model.compare();
            if compare.is_visible() {
                let pixels = |image: Option<&Arc<DecodedImage>>| {
                    image.and_then(|image| image.platform_image().as_pixels().cloned())
                };
                let left = pixels(compare.left_image());
                let right = pixels(compare.right_image());
                compare_retained_ids([left.as_ref(), right.as_ref()])
            } else {
                Vec::new()
            }
        };
        ids.extend(compare_ids);
        let ids: Vec<i64> = ids.into_iter().collect();
        if let Err(error) = cache.retain(&ids) {
            self.log_viewing_failure("GPU cache retain failed", &error);
        }
    }

    /// C# `ClearGpuCache()` (214-225), posted by the cache-cleared listener: unless disposed, `gpu_cache_clear_count += 1`, `cache.clear()`,
    /// `composer.cancel_crossfade()`, `window.invalidate()`. (INV-2: a GPU bitmap must not outlive the RAM entry; the current image still has its CPU buffer
    /// and is uploaded again by the next frame.)
    pub(super) fn clear_gpu_cache(&self) {
        if self.disposed.get() {
            return;
        }
        let count = &self.viewing.gpu_cache_clear_count;
        count.set(count.get().saturating_add(1));
        if let Some(cache) = self.current_cache() {
            cache.clear();
        }
        if let Some(composer) = self.current_composer() {
            composer.cancel_crossfade();
        }
        self.window.invalidate();
    }

    /// C# `DrawPresentation(context)` (229-261): without a composer nothing. `inputs` from the viewer (`effective_scaling_quality`, `scaling_quality`), the
    /// viewport (`image_origin`, `image_actual_width/height`), the window (`client_size_dip` converted to `pr_app`'s `SizeD`, `dpi_scale`) and `vm.compare()`
    /// (short borrow); `frame = composer.draw(context, &inputs)` (an `Err` is logged and the frame goes on). `image_drawn_in_frame = true` when `frame.main_drawn`
    /// and `composer.current()` is the image the presenter assigned (`Arc::ptr_eq` with `hub.current_image()`'s pixels), else when `frame.compare_images > 0`.
    #[allow(dead_code)] // called by 13i `draw_content`; remove this line when 13i is merged.
    pub(super) fn draw_presentation(&self, context: &mut dyn DrawContext) {
        let Some(composer) = self.current_composer() else {
            return;
        };
        let (effective_quality, compare_quality) = {
            let viewer = self.viewer.borrow();
            (viewer.effective_scaling_quality(), viewer.scaling_quality())
        };
        let origin = self.viewport.image_origin();
        let destination = RectD {
            x: origin.x,
            y: origin.y,
            width: self.viewport.image_actual_width(),
            height: self.viewport.image_actual_height(),
        };
        // D-W8B4X-12: the view model is not lent to the composer; the borrow ends with the clones. A modal command (`show_settings`) holds the mutable
        // borrow for its whole nested message loop and that loop paints (C# has no lock: the paint reads the object as it is): `try_borrow` then
        // falls back to the compare state of the last frame that could read it, instead of panicking on the UI thread.
        let (compare_visible, left, right, left_selected, right_selected) = match self.view_model.try_borrow() {
            Ok(view_model) => {
                let compare = view_model.compare();
                let snapshot = (
                    compare.is_visible(),
                    compare.left_image().cloned(),
                    compare.right_image().cloned(),
                    compare.is_left_selected(),
                    compare.is_right_selected(),
                );
                *self.viewing.compare_paint_cache.borrow_mut() = snapshot.clone();
                snapshot
            }
            Err(_) => self.viewing.compare_paint_cache.borrow().clone(),
        };
        let client = self.window.client_size_dip();
        let inputs = PresentationInputs {
            viewport: SizeD {
                width: client.width,
                height: client.height,
            },
            main_destination: destination,
            effective_quality,
            compare_quality,
            dpi_scale: self.window.dpi_scale(),
            compare_visible,
            compare: CompareFrame {
                left: left
                    .as_ref()
                    .and_then(|image| image.platform_image().as_pixels())
                    .map(AsRef::as_ref),
                right: right
                    .as_ref()
                    .and_then(|image| image.platform_image().as_pixels())
                    .map(AsRef::as_ref),
                left_selected,
                right_selected,
            },
        };
        let frame = match composer.draw(context, &inputs) {
            Ok(frame) => frame,
            Err(error) => {
                // The composer popped its clips before reporting, so the frame can still end.
                self.log_viewing_failure("Presentation draw failed", &error);
                return;
            }
        };
        let hub_image = self.hub.current_image();
        let hub_pixels = hub_image.as_ref().and_then(PlatformImage::as_pixels);
        let current = composer.current();
        if frame_contains_assigned_image(&frame, current.as_ref(), hub_pixels) {
            self.frame.image_drawn_in_frame.set(true);
        }
    }

    /// C# `TrySelectCompareAt` (267-285): not disposed and compare visible; `layout::hit_test(client_size_dip, point)` (16b): Left -> select left, Right ->
    /// select right (through the view model, see the contract: `MainViewModel` has no public selector yet), result `true`; otherwise `false`.
    pub(super) fn try_select_compare_at(&self, client_dip: PointD) -> bool {
        if self.disposed.get()
            || !self
                .view_model
                .try_borrow()
                .is_ok_and(|view_model| view_model.compare().is_visible())
        {
            return false;
        }
        let client = self.window.client_size_dip();
        match layout::hit_test(client, client_dip) {
            Some(CompareSide::Left) => {
                self.update_view_model(|view_model| view_model.select_compare_left());
                true
            }
            Some(CompareSide::Right) => {
                self.update_view_model(|view_model| view_model.select_compare_right());
                true
            }
            None => false,
        }
    }
}

impl MainWindow {
    /// C# `GpuCacheClearCount`.
    pub fn gpu_cache_clear_count(&self) -> i32 {
        self.inner.viewing.gpu_cache_clear_count.get()
    }

    /// C# `Composer` (tests and other packs).
    pub fn composer(&self) -> Option<Rc<PresentationComposer>> {
        self.inner.current_composer()
    }

    /// C# `Prefetcher`.
    pub fn prefetcher(&self) -> Option<Rc<PresentationPrefetcher>> {
        self.inner.current_prefetcher()
    }

    /// A new `PrefetchHost` over this window (the same type the prefetcher of `attach_renderer` uses), so tests can drive `path_at`, `start_lookup` and
    /// `schedule_retry` directly. Holds only a `Weak`: it never keeps the window alive and does nothing once the window is gone.
    pub fn prefetch_host(&self) -> Rc<dyn PrefetchHost> {
        Rc::new(MainWindowPrefetchHost {
            window: self.inner.weak_self.clone(),
        })
    }

    /// C# `Stats`: runs `f` on the frame statistics.
    pub fn with_stats<R>(&self, f: impl FnOnce(&mut FrameStats) -> R) -> R {
        f(&mut self.inner.viewing.stats.borrow_mut())
    }

    /// C# `Pacer`: runs `f` on the frame pacer.
    pub fn with_pacer<R>(&self, f: impl FnOnce(&mut FramePacer) -> R) -> R {
        f(&mut self.inner.viewing.pacer.borrow_mut())
    }

    /// C# `TrySelectCompareAt(PointD clientDip)` (`internal`; the pointer input of 14c calls it).
    pub fn try_select_compare_at(&self, client_dip: PointD) -> bool {
        self.inner.try_select_compare_at(client_dip)
    }
}
