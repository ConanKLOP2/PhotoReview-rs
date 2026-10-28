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
//!
//! INV-1 / INV-9: every continuation first finds ITS navigation record (same token, expected phase; a newer present replaced the record, so a
//! superseded event finds nothing) and then asks the clock whether the token is still current before it touches the sink, the catalog or a cache.

use std::time::Instant;

use pr_core::abstractions::FileStat;
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::perf::{self, PhotoReviewPerf};
use pr_core::model::LoadingMode;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::{PathError, get_file_name};
use pr_imaging::cache::thumbnails::{ThumbnailError, ThumbnailResult};
use pr_imaging::cache_key::CacheKeyError;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::service::{PreviewImage, PreviewResult};
use pr_l10n::template::describe;

use crate::misc::app_log::AppLog;
use crate::viewmodels::format::status;

use super::photo_info::{PhotoInfo, StatOutcome, StatResult};
use super::present_b::PresentFailure;
use super::state::ImagePresenter;
use super::state::{
    EntryMetadata, Navigation, Phase, PresentError, PresentRequest, PresentSettings, PresenterChanges, PresenterEnv,
    PresenterEvent, StatPurpose, ThumbnailRace, YieldPurpose,
};

/// Thread name of the job that waits for the viewer preview.
const PREVIEW_WAIT_THREAD: &str = "PhotoReviewRs.PresentPreviewWait";
/// Thread name of the job that waits for the thumbnail.
const THUMBNAIL_WAIT_THREAD: &str = "PhotoReviewRs.PresentThumbnailWait";

/// `Stopwatch.ElapsedMilliseconds` of an [`Instant`].
fn elapsed_ms(since: Instant) -> i64 {
    i64::try_from(since.elapsed().as_millis()).unwrap_or(i64::MAX)
}

/// The catalog size as the `int` the status formatters take.
pub(super) fn catalog_count(env: &PresenterEnv<'_>) -> i32 {
    i32::try_from(env.catalog.count()).unwrap_or(i32::MAX)
}

/// Whether a thumbnail failure is the C# `OperationCanceledException` (a cancelled token or a cancelled decode).
pub(super) fn thumbnail_error_is_canceled(error: &ThumbnailError) -> bool {
    match error {
        ThumbnailError::Canceled => true,
        ThumbnailError::Decode(decode) => matches!(decode.kind(), DecodeError::Canceled),
        _ => false,
    }
}

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
        self.begin_present(env, request, 0, None)?;
        Ok(self.take_changes())
    }

    /// The navigation record of `token` when it waits at a phase `accepts`; `None` for an event of a superseded present (the newer one replaced the
    /// record) or an event that does not belong to the phase the record is at. The record stays in place when it is not taken.
    pub(super) fn take_navigation(&mut self, token: i64, accepts: impl FnOnce(&Phase) -> bool) -> Option<Navigation> {
        match &self.navigation {
            Some(nav) if nav.token == token && accepts(&nav.phase) => self.navigation.take(),
            _ => None,
        }
    }

    /// `PresentCoreAsync` up to the first `await` (lines 234 to 296). Steps, in this exact order (the C# order is pinned by tests):
    ///
    /// 1. index range check; `path_override` check (`catalog.index_of(po) != index` is `PathNotInEntry`).
    /// 2. `last_present_started_from_ram = false`; `perf = PhotoReviewPerf::log().is_enabled()`; start the present stopwatch.
    /// 3. `token = clock.next_navigation()`; `catalog.set_current(index)`; `path = path_override.unwrap_or(catalog.path_at(index))`;
    ///    `previous_navigation_path = if stale_not_found_retries > 0 { original_previous } else { current_navigation_path }`; `current_navigation_path = Some(path)`.
    /// 4. a NEW `CancelToken` for the viewer decode; the PREVIOUS one is cancelled (a clone of the token is captured into the navigation, INV-1 / APP-P01).
    /// 5. `zoom_detail.reset()` (apply its effects), `preload.notify_navigation(index)`.
    /// 6. perf: `set_nav_context(token)` and `show_start(token, index, "<LoadingMode>")` (the mode read from `env.settings` ONLY for this mark).
    /// 7. `compare.select(None)`, `current_photo_info = None`, `info_while_original_shown = None`; log `ShowImage start index=.. count=.. token=.. path=..` when enabled.
    /// 8. the initial stat goes to the stat worker with the viewer-decode token as its cancellation token: `Phase::AwaitInitialStat`.
    pub(super) fn begin_present(
        &mut self,
        env: &mut PresenterEnv<'_>,
        request: PresentRequest,
        stale_not_found_retries: u8,
        original_previous_navigation_path: Option<String>,
    ) -> Result<(), PresentError> {
        let index = request.index;
        let Some(slot) = usize::try_from(index).ok().filter(|slot| *slot < env.catalog.count()) else {
            return Ok(());
        };
        if let Some(path_override) = request.path_override.as_deref() {
            if env.catalog.index_of(path_override) != index {
                return Err(PresentError::PathNotInEntry);
            }
        }
        self.track_superseded_navigation();
        self.last_present_started_from_ram = false;

        let perf_log = PhotoReviewPerf::log();
        let perf = perf_log.is_enabled();
        let started = Instant::now();

        // 1. A new navigation generation.
        let token = self.clock.next_navigation();
        env.catalog.set_current(index);
        let path = match request.path_override.as_deref() {
            Some(path_override) => path_override.to_owned(),
            None => env.catalog.path_at(slot).to_owned(),
        };
        let previous_navigation_path = if stale_not_found_retries > 0 {
            original_previous_navigation_path
        } else {
            self.current_navigation_path.clone()
        };
        self.current_navigation_path = Some(path.clone());
        self.changes |= PresenterChanges::CURRENT_PATH;

        // perf(preload): this navigation supersedes the previous one -- drop its viewer decode if it has not started yet (a started one finishes and stays cached).
        let viewer_cancel = CancelToken::new();
        if let Some(superseded) = self.viewer_decode_cancel.replace(viewer_cancel.clone()) {
            superseded.cancel();
        }
        // feat(zoom): drop the previous image's zoom-detail decode and release its original now.
        let effects = self.zoom_detail.reset();
        self.apply_zoom_effects(effects);
        self.preload.notify_navigation(index);

        let perf_path_id = if perf { perf::path_id(&path) } else { String::new() };
        if perf {
            PhotoReviewPerf::set_nav_context(token);
            perf_log.show_start(token, index, &format!("{:?}", env.settings.loading_mode_or_default()));
        }

        if !self.compare.select(None).is_empty() {
            self.changes |= PresenterChanges::COMPARE;
        }
        // Q-R29 option C: the stat below is awaited off the UI thread, so this navigation is "in progress" from here on: the photo information
        // line must not keep describing the previous image meanwhile (it is set again below).
        self.current_photo_info = None;
        self.changes |= PresenterChanges::PHOTO_INFO;
        self.info_while_original_shown = None; // the previous photo's preview info must never be restored onto this one

        let log = AppLog::global();
        if log.enabled() {
            log.info(&format!(
                "ShowImage start index={index} count={} token={token} path={path}",
                env.catalog.count()
            ));
        }

        // 2. The stat runs on the navigation stat worker, never on the UI thread; nothing below looks at a cache before it returns.
        let perf_stat_start = if perf { perf::timestamp() } else { 0 };
        self.submit_stat(token, &path, StatPurpose::Initial, &viewer_cancel);
        self.navigation = Some(Navigation {
            token,
            request,
            path,
            previous_navigation_path,
            stale_not_found_retries,
            viewer_cancel,
            started,
            perf,
            phase: Phase::AwaitInitialStat,
            initial_size: 0,
            initial_entry: None,
            current_key: None,
            settings: None,
            ram_ready: false,
            initial_stat: None,
            image: None,
            original_dimensions: None,
            has_inflight: false,
            perf_path_id,
            perf_stat_start,
            perf_thumb_start: 0,
            perf_assigned: 0,
            perf_compare_start: 0,
            perf_dims_start: 0,
            pending_failure: None,
        });
        Ok(())
    }

    /// The code after the initial `await StatOffUiThreadAsync` (lines 298 to 349). A superseded present is dropped. `Missing`: perf `stat` mark, then the
    /// missing-item removal. `Error`: perf mark, log the error, `current_photo_info = None`, `update_current_image(None, 0, 0, None)`, `compare.clear(ctx)`,
    /// `update_status(status::image_error(file_name, describe(error), ctx), true)`; the navigation ends and the catalog is untouched. `Found`:
    /// `continue_after_initial_found`.
    pub(super) fn on_initial_stat(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: StatResult) {
        let Some(nav) = self.take_navigation(token, |phase| matches!(phase, Phase::AwaitInitialStat)) else {
            return;
        };
        if !self.clock.is_navigation_current(token) {
            return;
        }
        let StatResult { outcome, stat, error } = result;
        match (outcome, stat) {
            (StatOutcome::Found, Some(stat)) => self.continue_after_initial_found(env, nav, stat),
            (StatOutcome::Error, _) => {
                if nav.perf {
                    PhotoReviewPerf::log().stat(token, perf::ms(nav.perf_stat_start));
                }
                // An unreadable file (share hiccup, access denied) is not a missing one: keep it in the catalog.
                let error = error.unwrap_or_else(|| std::io::Error::other("the file could not be inspected"));
                AppLog::global().error(
                    &format!(
                        "ShowImage stat failed token={token} index={} path={}",
                        nav.request.index, nav.path
                    ),
                    Some(&error),
                );
                self.current_photo_info = None;
                self.changes |= PresenterChanges::PHOTO_INFO;
                // The status names this file, so the previous photo must not stay visible under it.
                self.update_current_image(None, 0, 0, None);
                // Same for a visible comparison pair: it is not this file, and the catalog stays untouched.
                if !self.compare.clear(&env.format).is_empty() {
                    self.changes |= PresenterChanges::COMPARE;
                }
                let text = status::image_error(get_file_name(&nav.path), &describe(&error), &env.format);
                self.update_status(text, true);
            }
            // `Missing` (and a `Found` without a stat, which the stat job never produces).
            _ => {
                if nav.perf {
                    PhotoReviewPerf::log().stat(token, perf::ms(nav.perf_stat_start));
                }
                self.begin_missing_run(env, &nav.path, token, true);
            }
        }
    }

    /// Lines 328 to 377 (R7-1, Q-R29 option C): refresh the catalog entry from the stat just taken; build the key from it; RAM lookup; photo info; status.
    ///
    /// - `initial_entry = catalog.find(path)`; when `path_override.is_none()` and the entry does not `matches(&stat)`: if the entry had both length and mtime
    ///   `evict_cached_path(path)`; `catalog.update_metadata(path, stat.length, stat.last_write_utc, None, None)`; re-read the entry.
    /// - the key comes from `get_current_cache_key_for_entry(entry)` where `entry` is the (refreshed) catalog entry, or for a path override (or an entry that left
    ///   the catalog) `CatalogEntry::new(path)` + `with_metadata(stat.length, stat.last_write_utc, None, None)`. An `Err` is a `PresentFailure::CacheKey` through `fail`.
    /// - `ram = try_get_cached_preview(&key)`; `last_present_started_from_ram = ram.is_some()`; `has_inflight = ram.is_none() && has_inflight_preview(&key)`;
    ///   perf `lookup`; on a RAM hit `preload.try_consume_preloaded_key(&key)` then `metrics.record_preload_hit()`.
    /// - `current_photo_info = ram.map(PhotoInfo::from_image)`; the settings are snapshotted HERE from `env.settings` and kept for the whole present;
    ///   `initial_status = if ram { status::ready(..) } else { status::loading(..) }`; `update_status(initial_status, !ram)`; log `ShowImage cache-state ..`.
    ///
    /// Then `begin_preview_race` (miss / in flight) or `present_preview` at once (RAM hit).
    pub(super) fn continue_after_initial_found(
        &mut self,
        env: &mut PresenterEnv<'_>,
        mut nav: Navigation,
        stat: FileStat,
    ) {
        let token = nav.token;
        let path = nav.path.clone();
        let length = i64::try_from(stat.length).unwrap_or(i64::MAX);
        let is_override = nav.request.path_override.is_some();

        // R7-1: the key must describe the file as it is now, not as the folder scan saw it.
        let mut entry = env.catalog.find(&path).cloned();
        if !is_override {
            if let Some(stale) = entry.as_ref().filter(|entry| !entry.matches(&stat)) {
                if stale.length.is_some() && stale.last_write_utc.is_some() {
                    if let Err(error) = self.evict_cached_path(&path) {
                        AppLog::global().error(
                            &format!("ShowImage evict failed token={token} path={path}"),
                            Some(&error),
                        );
                    }
                }
                env.catalog
                    .update_metadata(&path, length, stat.last_write_utc, None, None);
                entry = env.catalog.find(&path).cloned();
            }
        }
        nav.initial_size = length;
        nav.initial_entry = entry.as_ref().map(|entry| EntryMetadata {
            length: entry.length,
            last_write_utc: entry.last_write_utc,
        });

        // Q-R29 option C: the key comes from the stat just taken (no second stat on the UI thread): an entry that left the catalog meanwhile (and a
        // path override) gets a key built from that stat, not from a fresh one.
        let key_entry = match entry.filter(|_| !is_override) {
            Some(entry) => entry,
            None => match CatalogEntry::new(path.as_str()) {
                Ok(entry) => entry.with_metadata(length, stat.last_write_utc, None, None),
                Err(_) => {
                    // A catalog path is never blank (the entry validated it): the C# `ArgumentException` takes the generic failure branch.
                    let failure = PresentFailure::CacheKey(CacheKeyError::InvalidPath(PathError::Argument));
                    self.fail(env, nav, failure);
                    return;
                }
            },
        };
        let key = match self.preview_service.get_current_cache_key_for_entry(&key_entry) {
            Ok(key) => key,
            Err(error) => {
                self.fail(env, nav, PresentFailure::CacheKey(error));
                return;
            }
        };
        let perf_log = PhotoReviewPerf::log();
        if nav.perf {
            perf_log.stat(token, perf::ms(nav.perf_stat_start));
        }

        // 3. RAM hit (records the preload hit).
        let ram = self.preview_service.try_get_cached_preview(&key);
        let ram_ready = ram.is_some();
        self.last_present_started_from_ram = ram_ready;
        let has_inflight = !ram_ready && self.preview_service.has_inflight_preview(&key);
        if nav.perf {
            let result = if ram_ready {
                "ramHit"
            } else if has_inflight {
                "inflight"
            } else {
                "miss"
            };
            perf_log.lookup(token, &nav.perf_path_id, result);
        }
        if ram_ready && self.preload.try_consume_preloaded_key(&key) {
            self.metrics.record_preload_hit();
        }

        // Photo information line: never show the previous image's EXIF; a RAM hit already has this image's.
        self.current_photo_info = ram.as_ref().map(|image| PhotoInfo::from_image(&path, image.as_ref()));
        self.changes |= PresenterChanges::PHOTO_INFO;

        let settings = PresentSettings {
            loading_mode: env.settings.loading_mode_or_default(),
            compare_size_enabled: env.settings.compare_size_enabled,
            compare_hash_enabled: env.settings.compare_hash_enabled,
        };
        let index = nav.request.index;
        let count = catalog_count(env);
        let initial_status = if ram_ready {
            status::ready(index, count, length, get_file_name(&path), &env.format)
        } else {
            status::loading(index, count, length, &env.format)
        };
        self.update_status(initial_status, !ram_ready);

        let log = AppLog::global();
        if log.enabled() {
            log.info(&format!(
                "ShowImage cache-state token={token} path={path} ramReady={ram_ready} cacheBytes={}",
                self.preview_service.cache_bytes()
            ));
        }

        nav.current_key = Some(key);
        nav.settings = Some(settings);
        nav.ram_ready = ram_ready;
        nav.has_inflight = has_inflight;
        nav.initial_stat = Some(stat);
        match ram {
            Some(image) => self.present_preview(env, nav, image),
            None => self.begin_preview_race(env, nav, stat),
        }
    }

    /// Lines 379 to 411: `preview = preview_service.get_viewer_preview(path, &key, &viewer_cancel, preload.get_viewer_decode_delay())` (started immediately, so the thumbnail and
    /// the preview decode run concurrently); one worker job waits on it and sends `PresenterEvent::Preview`. When `settings.loading_mode == Preview && !ram && !has_inflight`:
    /// perf `thumb_start`, `thumbnail_cache.get(path, Some(stat))` (a synchronous `Err` is a `PresentFailure::Thumbnail` through `fail`), one worker job waits on the request
    /// and sends `PresenterEvent::Thumbnail`; `Phase::AwaitPreview { race: Pending }`; otherwise `race: NotStarted`.
    pub(super) fn begin_preview_race(&mut self, env: &mut PresenterEnv<'_>, mut nav: Navigation, stat: FileStat) {
        let token = nav.token;
        let Some(key) = nav.current_key.clone() else {
            // Unreachable: the key is stored before the race starts. Fail the present instead of waiting for an event that never comes.
            let failure = PresentFailure::Preview(pr_imaging::service::PreviewError::Canceled);
            self.fail(env, nav, failure);
            return;
        };
        // Perf: start the preview decode immediately so the two run concurrently; the viewer's decode gets its own priority lane and is dropped (before it
        // starts) when a newer navigation supersedes this one.
        let delay = self.preload.get_viewer_decode_delay();
        let preview = self
            .preview_service
            .get_viewer_preview(&nav.path, &key, &nav.viewer_cancel, delay);
        let inbox = self.inbox.clone();
        self.runner.run(
            PREVIEW_WAIT_THREAD,
            Box::new(move || {
                let result: PreviewResult = preview.wait();
                inbox.send(PresenterEvent::Preview { token, result });
            }),
        );

        let loading_mode = nav
            .settings
            .map_or(LoadingMode::Preview, |settings| settings.loading_mode);
        let mut race = ThumbnailRace::NotStarted;
        if loading_mode == LoadingMode::Preview && !nav.ram_ready && !nav.has_inflight {
            if nav.perf {
                nav.perf_thumb_start = perf::timestamp();
                PhotoReviewPerf::log().thumb_start(token, &nav.perf_path_id);
            }
            // Q-R29 option C: the thumbnail key reuses this navigation's stat.
            match self.thumbnail_cache.get(&nav.path, Some(stat)) {
                Ok(request) => {
                    let inbox = self.inbox.clone();
                    self.runner.run(
                        THUMBNAIL_WAIT_THREAD,
                        Box::new(move || {
                            let result: ThumbnailResult = request.wait();
                            inbox.send(PresenterEvent::Thumbnail { token, result });
                        }),
                    );
                    race = ThumbnailRace::Pending;
                }
                Err(error) => {
                    self.fail(env, nav, PresentFailure::Thumbnail(error));
                    return;
                }
            }
        }
        nav.phase = Phase::AwaitPreview { race };
        self.navigation = Some(nav);
    }

    /// The thumbnail finished (lines 413 to 450). Valid only while the race is `Pending` (a later event after the preview won is dropped). A thumbnail error other than a
    /// cancellation is logged (`ShowImage thumbnail failed ..`) and counts as "no thumbnail"; a cancelled thumbnail is a `PresentFailure::Thumbnail` through `fail`.
    /// Perf `thumb_end`. A superseded present is dropped. With an image: sizes = the known original dimensions of the key or the thumbnail's own `original_width/height`;
    /// `update_current_image(thumbnail.platform_image(), w, h, Some(path))`; perf `TracePresented("thumbnail")`; `sink.apply_initial_view_mode()` (+ zoom sync);
    /// `update_status(status::loading_full_res(index, count, initial_size), true)`. Race := `Shown`. Without an image: race := `Shown` too (nothing to show; keep waiting).
    pub(super) fn on_thumbnail(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: ThumbnailResult) {
        let Some(mut nav) = self.take_navigation(token, |phase| {
            matches!(
                phase,
                Phase::AwaitPreview {
                    race: ThumbnailRace::Pending
                }
            )
        }) else {
            return;
        };
        // The thumbnail is only a placeholder for the preview that is already decoding: whatever the thumbnail read/decode does wrong (a damaged cache file,
        // an unexpected WIC/COM failure) must not fail an image whose preview is fine. Treat a fault as "no thumbnail" and carry on.
        let thumbnail = match result {
            Ok(thumbnail) => thumbnail,
            Err(error) if thumbnail_error_is_canceled(&error) => {
                self.fail(env, nav, PresentFailure::Thumbnail(error));
                return;
            }
            Err(error) => {
                AppLog::global().error(
                    &format!("ShowImage thumbnail failed token={token} path={}", nav.path),
                    Some(&error),
                );
                None
            }
        };
        if nav.perf {
            PhotoReviewPerf::log().thumb_end(token, &nav.perf_path_id, "unknown", perf::ms(nav.perf_thumb_start));
        }

        // INV-1: check the token after the wait.
        if !self.clock.is_navigation_current(token) {
            return;
        }

        if let Some(thumbnail) = thumbnail {
            // Size the placeholder like the source (if any earlier decode already told us its dimensions) so a non-Fit zoom doesn't jump when the preview replaces it.
            let known = nav
                .current_key
                .as_ref()
                .and_then(|key| self.preview_service.try_get_known_original_dimensions(key));
            let (width, height) = known.unwrap_or((thumbnail.original_width(), thumbnail.original_height()));
            self.update_current_image(Some(thumbnail.platform_image().clone()), width, height, Some(&nav.path));
            if nav.perf {
                self.sink.trace_presented(token, "thumbnail", perf::timestamp());
            }
            let log = AppLog::global();
            if log.enabled() {
                log.info(&format!(
                    "ShowImage thumbnail-presented token={token} path={}",
                    nav.path
                ));
            }
            self.sink.apply_initial_view_mode();
            self.sync_viewer_zoom_from_sink();
            let text = status::loading_full_res(nav.request.index, catalog_count(env), nav.initial_size, &env.format);
            self.update_status(text, true);
        }
        // else: the source has no embedded thumbnail (or isn't a JPEG) -- nothing to show yet; the preview wait is still open.
        nav.phase = Phase::AwaitPreview {
            race: ThumbnailRace::Shown,
        };
        self.navigation = Some(nav);
    }

    /// The viewer preview finished (line 467). If the thumbnail is still `Pending` the race becomes `Abandoned` (the thumbnail keeps warming its cache; its event is dropped).
    /// `Err(e)`: `fail(PresentFailure::Preview(e))`. `Ok(image)`: `present_preview`.
    pub(super) fn on_preview(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: PreviewResult) {
        let Some(mut nav) = self.take_navigation(token, |phase| matches!(phase, Phase::AwaitPreview { .. })) else {
            return;
        };
        if let Phase::AwaitPreview { race } = &mut nav.phase {
            if *race == ThumbnailRace::Pending {
                *race = ThumbnailRace::Abandoned;
            }
        }
        match result {
            Ok(image) => self.present_preview(env, nav, image),
            Err(error) => self.fail(env, nav, PresentFailure::Preview(error)),
        }
    }

    /// Lines 467 to 511, the step-5 body shared by the RAM-hit path and `on_preview`:
    ///
    /// 1. `ram_ready`: `metrics.record_cache_hit()`.  2. a superseded present is dropped.
    /// 3. AR16 index rebase: `current = catalog.current_index()`; `if current >= 0 && current == catalog.index_of(path) { request.index = current }`.
    /// 4. `current_photo_info = Some(PhotoInfo::from_image(path, &image))`; `update_current_image(Some(image.platform_image().clone()), .., Some(path))`;
    ///    `metrics.record_ui_assign(ms)`; perf `assign`; log `ShowImage preview-presented ..`.
    /// 5. `sink.on_presented(path)`, then the `on_presented_hook` (if any).
    /// 6. preload kick: with `defer_next_preload_kick && ui_scheduler.is_some()`: the flag clears NOW and `ui.post` an event `FrameYielded { purpose: DeferredKick { index } }`
    ///    (the kick runs when the event is pumped, only if `is_navigation_current(token)`); otherwise `preload.preload_around(index)` at once (the returned `RunHandle` is dropped:
    ///    fire and forget). perf `post_start/post_end("preloadKick")`.
    /// 7. `after_preview_presented` (07j).
    pub(super) fn present_preview(&mut self, env: &mut PresenterEnv<'_>, mut nav: Navigation, image: PreviewImage) {
        let token = nav.token;
        if nav.ram_ready {
            self.metrics.record_cache_hit();
        }
        // INV-1: check the token after the wait.
        if !self.clock.is_navigation_current(token) {
            return;
        }
        // AR16: the background readability probe may have removed other files while this image decoded. It keeps the current entry by path, so re-read its index for
        // preload/status. Both sides are -1 when the entry itself left the catalog meanwhile: that is not "still current".
        let current_index_now = env.catalog.current_index();
        if current_index_now >= 0 && current_index_now == env.catalog.index_of(&nav.path) {
            nav.request.index = current_index_now;
        }

        let perf_log = PhotoReviewPerf::log();
        let perf_assign = if nav.perf { perf::timestamp() } else { 0 };
        let ui_assign = Instant::now();

        self.current_photo_info = Some(PhotoInfo::from_image(&nav.path, image.as_ref()));
        self.changes |= PresenterChanges::PHOTO_INFO;
        self.update_current_image(
            Some(image.platform_image().clone()),
            image.original_width(),
            image.original_height(),
            Some(&nav.path),
        );

        self.metrics.record_ui_assign(elapsed_ms(ui_assign));
        if nav.perf {
            nav.perf_assigned = perf::timestamp();
            let assign_ms = (nav.perf_assigned - perf_assign) as f64 * 1000.0 / perf::STOPWATCH_FREQUENCY as f64;
            perf_log.assign(token, assign_ms, image.pixel_width(), image.pixel_height());
        }

        let log = AppLog::global();
        if log.enabled() {
            let mode = nav
                .settings
                .map_or(LoadingMode::Preview, |settings| settings.loading_mode);
            log.info(&format!(
                "ShowImage preview-presented token={token} path={} mode={mode:?}",
                nav.path
            ));
        }

        self.sink.on_presented(&nav.path);
        if let Some(hook) = self.on_presented_hook.as_mut() {
            hook(&nav.path);
        }

        let perf_kick = if nav.perf { perf::timestamp() } else { 0 };
        if nav.perf {
            perf_log.post_start(token, "preloadKick");
        }
        let index = nav.request.index;
        let scheduler = self.ui_scheduler.clone().filter(|_| self.defer_next_preload_kick);
        match scheduler {
            Some(scheduler) => {
                // P-1 startup: the launch image's frame first, then preloading. The flag is consumed NOW (when the event is posted), not when it runs.
                self.defer_next_preload_kick = false;
                let inbox = self.inbox.clone();
                scheduler.post(Box::new(move || {
                    inbox.send(PresenterEvent::FrameYielded {
                        token,
                        purpose: YieldPurpose::DeferredKick { index },
                    });
                }));
            }
            None => {
                // Fire and forget: the run continues on the scheduler's own workers.
                self.preload.catalog_changed(env.catalog);
                drop(self.preload.preload_around(index));
            }
        }
        if nav.perf {
            perf_log.post_end(token, "preloadKick", perf::ms(perf_kick));
        }

        nav.image = Some(image);
        self.after_preview_presented(env, nav);
    }

    /// The deferred preload kick came due (the C# `await ui.YieldAsync()` returned inside `FireAndLog`): `preload.preload_around(index)` only if
    /// `clock.is_navigation_current(token)`; otherwise the kick is dropped (the newer navigation kicks its own).
    pub(super) fn on_deferred_kick_due(&mut self, env: &mut PresenterEnv<'_>, token: i64, index: i32) {
        if self.clock.is_navigation_current(token) {
            self.preload.catalog_changed(env.catalog);
            drop(self.preload.preload_around(index));
        }
    }
}
