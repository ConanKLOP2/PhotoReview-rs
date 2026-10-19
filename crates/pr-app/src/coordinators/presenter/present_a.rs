//! Port of `PhotoReview.App.Shared/Coordinators/ImagePresenter.cs` lines 222 to 511 (baseline d92d6aae): `PresentAsync` and the first
//! half of `PresentCoreAsync` (steps 1 to 5: new navigation generation, initial stat, key refresh, RAM lookup, thumbnail-vs-preview
//! race, present the preview, preload kick). This is the INV-1 choke point. Contract: `contracts/W8-07i.md`. Target:
//! `crates/pr-app/src/coordinators/presenter/present_a.rs` (`coordinators::presenter::present_a`).
//!
//! `PresentCoreAsync` is a chain of awaits; the Rust form keeps ONE `Navigation` record per present and resumes it from
//! `ImagePresenter::pump` when a worker completion arrives (D-W8-2). The phases of this half:
//!
//! ```text
//!  present --> begin_present: token = clock.next_navigation() ...  --> Phase::AwaitInitialStat   (stat worker)
//!  on_initial_stat: Found --> key refresh, RAM lookup, status --> start the viewer preview (and the thumbnail when Preview mode,
//!                                                                  not RAM, not in flight) --> Phase::AwaitPreview
//!                   Missing --> remove_missing_catalog_item (07k)     Error --> error status, nothing else
//!  on_thumbnail / on_preview --> present_preview: PhotoInfo, update_current_image, OnPresented, hook, preload kick
//!                                --> after_preview_presented (07j)
//! ```
//!
//! The RAM-hit path has no wait: `on_initial_stat` goes straight from the lookup to `present_preview`.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]
// Stub only (contract text in doc comments, boxed seams): the porter fixes the doc layout / introduces type aliases and removes this line.
#![allow(clippy::doc_lazy_continuation)]

use pr_core::abstractions::FileStat;
use pr_imaging::cache::thumbnails::ThumbnailResult;
use pr_imaging::service::{PreviewImage, PreviewResult};

use super::photo_info::StatResult;
use super::state::{ImagePresenter, Navigation, PresentError, PresentRequest, PresenterChanges, PresenterEnv};

impl ImagePresenter {
    /// C# `PresentAsync(index, allowCompare, pathOverride, includeCaptureGroupInCompare)` = `PresentCoreAsync(.., staleNotFoundRetries: 0,
    /// originalPreviousNavigationPath: null)`. Returns once the first step is posted (D-W8-2): the stat is on the stat worker, everything else
    /// happens in `pump`. `Err(PresentError::PathNotInEntry)` is the synchronous C# `ArgumentException` (checked BEFORE any state change, after the
    /// index range check). An out-of-range index is `Ok` with no change at all (no token taken, no sink call, `LastPresentStartedFromRam` untouched).
    pub fn present(
        &mut self,
        env: &mut PresenterEnv<'_>,
        request: PresentRequest,
    ) -> Result<PresenterChanges, PresentError> {
        todo!()
    }

    /// `PresentCoreAsync` up to the first `await` (lines 234 to 296). Steps, in this exact order (the C# order is pinned by tests):
    /// 1. index range check; `path_override` check (`catalog.index_of(po) != index` is `PathNotInEntry`).
    /// 2. `last_present_started_from_ram = false`; `perf = PhotoReviewPerf::log().is_enabled()`; start the present stopwatch.
    /// 3. `token = clock.next_navigation()`; `catalog.set_current(index)`; `path = path_override.unwrap_or(catalog.path_at(index))`;
    ///    `previous_navigation_path = if stale_not_found_retries > 0 { original_previous } else { current_navigation_path }`; `current_navigation_path = Some(path)`.
    /// 4. a NEW `CancelToken` for the viewer decode; the PREVIOUS one is cancelled (never disposed: a clone of the token is captured into the navigation, INV-1 / APP-P01).
    /// 5. `zoom_detail.reset()` (apply its effects), `preload.notify_navigation(index)`.
    /// 6. perf: `PhotoReviewPerf::set_nav_context(token)` and `show_start(token, index, "<LoadingMode>")` (the mode read from `env.settings` ONLY for this mark).
    /// 7. `compare.select(None)`, `current_photo_info = None`, `info_while_original_shown = None`; log `ShowImage start index=.. count=.. token=.. path=..` when enabled.
    /// 8. the initial stat goes to the stat worker with the viewer-decode token as its cancellation token: `Phase::AwaitInitialStat`.
    pub(super) fn begin_present(
        &mut self,
        env: &mut PresenterEnv<'_>,
        request: PresentRequest,
        stale_not_found_retries: u8,
        original_previous_navigation_path: Option<String>,
    ) -> Result<(), PresentError> {
        todo!()
    }

    /// The code after the initial `await StatOffUiThreadAsync` (lines 298 to 349). `!is_navigation_current(token)`: drop (log `ShowImage superseded during stat` when the
    /// stat was cancelled). `Missing`: perf `stat` mark, then `remove_missing_catalog_item(path, token)`. `Error`: perf mark, log the error, `current_photo_info = None`,
    /// `update_current_image(None, 0, 0, None)`, `compare.clear(ctx)`, `update_status(status::image_error(file_name, describe(error), ctx), true)`; the navigation ends and the
    /// catalog is untouched. `Found`: `continue_after_initial_found`.
    pub(super) fn on_initial_stat(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: StatResult) {
        todo!()
    }

    /// Lines 328 to 377 (R7-1, Q-R29 option C): refresh the catalog entry from the stat just taken; build the key from it; RAM lookup; photo info; status.
    /// - `initial_entry = catalog.find(path)`; when `path_override.is_none()` and the entry does not `matches(&stat)`: `if entry.length.is_some() && entry.last_write_utc.is_some()`
    ///   `evict_cached_path(path)`; `catalog.update_metadata(path, stat.length as i64, stat.last_write_utc, None, None)`; re-read the entry.
    /// - `current_key = preview_service.get_current_cache_key_for_entry(entry)` where `entry` = the (refreshed) catalog entry, or for a path override (or an entry that left
    ///   the catalog) `CatalogEntry::new(path)` + `with_metadata(stat.length, stat.last_write_utc, None, None)`. An `Err` is a `PresentFailure::CacheKey` through `fail`.
    /// - `ram = preview_service.try_get_cached_preview(&key)`; `last_present_started_from_ram = ram.is_some()`; `has_inflight = ram.is_none() && has_inflight_preview(&key)`;
    ///   perf `lookup(token, path_id, "ramHit" | "inflight" | "miss")`; on a RAM hit `preload.try_consume_preloaded_key(&key)` then `metrics.record_preload_hit()`.
    /// - `current_photo_info = ram.map(|image| PhotoInfo::from_image(path, image))`; `settings` snapshot taken HERE from `env.settings` (loading mode, compare size, compare hash) and
    ///   kept for the whole present; `initial_status = if ram { status::ready(index, count, stat.length, file_name) } else { status::loading(index, count, stat.length) }`;
    ///   `update_status(initial_status, !ram)`; log `ShowImage cache-state ..`.
    /// Then `begin_preview_race` (miss / in flight) or `present_preview` at once (RAM hit).
    pub(super) fn continue_after_initial_found(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation, stat: FileStat) {
        todo!()
    }

    /// Lines 379 to 411: `preview = preview_service.get_viewer_preview(path, &key, &viewer_cancel, preload.get_viewer_decode_delay())` (started immediately, so the thumbnail and the
    /// preview decode run concurrently); one worker job waits on it and sends `PresenterEvent::Preview`. When `settings.loading_mode == Preview && !ram && !has_inflight`: perf
    /// `thumb_start`, `thumbnail_cache.get(path, Some(stat))` (a synchronous `Err` is a `PresentFailure::Thumbnail` through `fail`), one worker job waits on the request and sends
    /// `PresenterEvent::Thumbnail`; `Phase::AwaitPreview { race: Pending }`; otherwise `race: NotStarted`.
    pub(super) fn begin_preview_race(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation, stat: FileStat) {
        todo!()
    }

    /// The thumbnail finished (lines 413 to 450). Valid only while the race is `Pending` (a later event after the preview won is dropped). A thumbnail error other than a
    /// cancellation is logged (`ShowImage thumbnail failed ..`) and counts as "no thumbnail"; a `Canceled` thumbnail error is a `PresentFailure::Thumbnail` through `fail`.
    /// Perf `thumb_end`. `!is_navigation_current(token)`: drop. With an image: sizes = the known original dimensions of the key
    /// (`try_get_known_original_dimensions`) or the thumbnail's own `original_width/height`; `update_current_image(thumbnail.platform_image(), w, h, Some(path))`; perf
    /// `TracePresented("thumbnail")`; `sink.apply_initial_view_mode()` (+ zoom sync); `update_status(status::loading_full_res(index, count, initial_size), true)`. Race := `Shown`. Without an
    /// image: race := `Shown` too (nothing to show; keep waiting for the preview).
    pub(super) fn on_thumbnail(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: ThumbnailResult) {
        todo!()
    }

    /// The viewer preview finished (line 467). If the thumbnail is still `Pending` the race becomes `Abandoned` (the thumbnail keeps warming its cache; its event is dropped).
    /// `Err(e)`: `fail(PresentFailure::Preview(e))`. `Ok(image)`: `present_preview`.
    pub(super) fn on_preview(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: PreviewResult) {
        todo!()
    }

    /// Lines 467 to 511, the step-5 body shared by the RAM-hit path and `on_preview`:
    /// 1. `ram_ready`: `metrics.record_cache_hit()`.  2. `!is_navigation_current(token)`: drop.
    /// 3. AR16 index rebase: `current = catalog.current_index()`; `if current >= 0 && current == catalog.index_of(path) { request.index = current }`.
    /// 4. `current_photo_info = Some(PhotoInfo::from_image(path, &image))`; `update_current_image(Some(image.platform_image().clone()), image.original_width(), image.original_height(), Some(path))`;
    ///    `metrics.record_ui_assign(ms of steps 4)`; perf `assign(token, ms, pixel_width, pixel_height)`; log `ShowImage preview-presented ..`.
    /// 5. `sink.on_presented(path)`, then the `on_presented_hook` (if any).
    /// 6. preload kick: with `defer_next_preload_kick && ui_scheduler.is_some()`: the flag clears NOW and `ui.post` an event `FrameYielded { purpose: DeferredKick { index } }`
    ///    (the kick runs when the event is pumped, only if `is_navigation_current(token)`); otherwise `preload.preload_around(index)` at once (the returned `RunHandle` is dropped:
    ///    fire and forget, a failure cannot happen in Rust, C# `FireAndLog`). perf `post_start/post_end("preloadKick")`.
    /// 7. `after_preview_presented` (07j).
    pub(super) fn present_preview(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation, image: PreviewImage) {
        todo!()
    }

    /// The deferred preload kick came due (the C# `await ui.YieldAsync()` returned inside `FireAndLog`): `preload.preload_around(index)` only if
    /// `clock.is_navigation_current(token)`; otherwise the kick is dropped (the newer navigation kicks its own).
    pub(super) fn on_deferred_kick_due(&mut self, token: i64, index: i32) {
        todo!()
    }
}
