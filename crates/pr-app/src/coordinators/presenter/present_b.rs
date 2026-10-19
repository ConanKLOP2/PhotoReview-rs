//! Port of `PhotoReview.App.Shared/Coordinators/ImagePresenter.cs` lines 500 to 712 (baseline d92d6aae): the second half of
//! `PresentCoreAsync` (end of the preload kick, step 6 compare or single-image branch, step 7 session and metrics, the three
//! `catch` clauses). Contract: `contracts/W8-07j.md`. Target: `crates/pr-app/src/coordinators/presenter/present_b.rs`
//! (`coordinators::presenter::present_b`).
//!
//! The C# method is a chain of `await`s; each `await` is a `Phase` of the `Navigation` record and each continuation a handler here:
//!
//! ```text
//!  after_preview_presented (07i calls it after the preload kick)
//!        |-- pair && allow_compare --> begin_compare --> AwaitCompare(Previews) --> (Sizes) --> (Hashes) --> finish_compare --> step 7
//!        |                                   `-- a partner preview failed --> compare_partner_failed (single image, error status) --> end
//!        `-- otherwise --> single-image branch: zoom_detail.on_preview_presented, [AwaitFrameYield] --> [AwaitDimensions]
//!                          --> AwaitRefreshStat --> status --> step 7
//!  any error --> fail(): superseded | stale NotFound (AwaitRecheckStat -> re-present / persisted failure / RemoveMissing) | generic | stale log
//! ```
//!
//! Every handler: `take()` the navigation, compare `nav.token` with the event token and the phase with the event kind (a mismatch
//! drops the event), check `clock.is_navigation_current(token)` where C# does (INV-1), and put the navigation back when it waits again.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use std::error::Error;

use pr_core::session::store::BlankFolder;
use pr_core::text::dotnet_path::PathError;
use pr_imaging::cache::thumbnails::ThumbnailError;
use pr_imaging::cache_key::CacheKeyError;
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::service::PreviewError;

use crate::misc::file_hash::FileHashError;
use crate::viewmodels::format::context::FormatContext;

use super::photo_info::StatResult;
use super::state::{CompareSide, ImagePresenter, Navigation, PresenterEnv};

/// What went wrong inside the C# `try` block of `PresentCoreAsync` (the exceptions the three `catch` clauses see), as a value.
#[derive(Debug)]
pub(super) enum PresentFailure {
    /// The viewer preview, an unexpected dimension-lookup error (C# bug class), or a compare partner preview.
    Preview(PreviewError),
    /// A thumbnail failure that C# does not swallow: `Canceled` (C# `OperationCanceledException`), `Disposed` / `BlankPath` (thrown synchronously by `GetAsync`).
    Thumbnail(ThumbnailError),
    /// `catalog compare::build_index` rejected a path (C# exception from `GetComparePair`).
    ComparePair(PathError),
    /// `SessionStore::save` / `SessionWriter::update` with a blank folder (C# `ArgumentException`).
    Session(BlankFolder),
    /// `PreviewImageService::get_current_cache_key_for_entry` failed (C# exception before the `try`; see contract O3).
    CacheKey(CacheKeyError),
    /// A compare hash was cancelled (C# `OperationCanceledException` from `GetAsync`, not swallowed by `TryGetHashAsync`).
    HashCanceled,
}

impl PresentFailure {
    /// C# `ex is OperationCanceledException`: `Preview(e)` with `e.is_canceled()`, `Thumbnail(Canceled)`, `HashCanceled`.
    pub(super) fn is_canceled(&self) -> bool {
        todo!()
    }

    /// C# `ex is FileNotFoundException || ex is DirectoryNotFoundException`: `Preview(Decode(e))` with `e.kind()` `FileNotFound` or `DirectoryNotFound`,
    /// `Preview(Key(SourceNoLongerExists))`, `Preview(Io(e))` / `Preview(SourceBytes(Io(e)))` with `ErrorKind::NotFound`, `CacheKey(SourceNoLongerExists)`.
    /// NOT `SourceChanged` (an `IOException` that is neither).
    pub(super) fn is_not_found(&self) -> bool {
        todo!()
    }

    /// C# `UserFacingError.Describe(ex)`: the localized sentence when the error carries one, else its message. `SourceChanged` renders the catalog key
    /// of `PreviewError::catalog_key()` with `{path}` through `ctx.texts` (D-W8-4: Vietnamese text comes from `pr_l10n`, never from a literal here);
    /// `Preview(Decode(e))` -> `e.describe()`; `Preview(SourceBytes(e))` -> `e.describe()`; `Key(SourceNoLongerExists { full_name })` -> catalog key
    /// `err.io.sourceNoLongerExists` with `{path}`; everything else its `Display`.
    pub(super) fn describe(&self, ctx: &FormatContext<'_>) -> String {
        todo!()
    }

    /// The error as a trait object for `AppLog::error` (C# passes the exception).
    pub(super) fn as_error(&self) -> Option<&(dyn Error + 'static)> {
        todo!()
    }
}

/// Which wait of the compare load is open (the part of `CompareViewModel.LoadAsync` after each `await`).
#[derive(Debug)]
pub(super) enum CompareStage {
    /// Both partner previews were requested; `left` / `right` are filled as their events arrive. The step ends when BOTH are in (C# `Task.WhenAll`).
    Previews {
        left: Option<Result<PlatformImage, PreviewError>>,
        right: Option<Result<PlatformImage, PreviewError>>,
    },
    /// Both file sizes were requested (one worker job).
    Sizes,
    /// Both hashes were requested; a `None` slot is still running.
    Hashes {
        left: Option<Result<String, FileHashError>>,
        right: Option<Result<String, FileHashError>>,
    },
}

/// The locals of `CompareViewModel.LoadAsync` that live across an `await`.
#[derive(Debug)]
pub(super) struct CompareProgress {
    pub(super) pair: (String, String),
    pub(super) stage: CompareStage,
    /// `StatusFormatter.CompareSizeSuffix(..)` or empty (C# `leftSize` / `rightSize`).
    pub(super) left_size: String,
    pub(super) right_size: String,
}

impl ImagePresenter {
    /// The C# code from the end of the preload kick (line 512) to the branch decision (line 526): `pair = get_compare_pair(path,
    /// include_capture_group_in_compare || compare.is_visible())` (an `Err` is a `PresentFailure::ComparePair`); perf marks (`TracePresented("final", ..)` when no pair,
    /// `post_start("compare")` otherwise); then `begin_compare` when `pair.is_some() && request.allow_compare`, else the single-image branch.
    pub(super) fn after_preview_presented(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation) {
        todo!()
    }

    /// Compare branch, lines 528 to 543 plus the start of `CompareViewModel.LoadAsync` (CVM lines 166 to 197, see the contract for the order): `current_photo_info = None`, `update_current_image(None, 0, 0, None)`;
    /// token check; `compare.set_left_path / set_right_path`, `compare.select(Some(path))` (the presented path, C# `initialSelectedPath: path`), `compare.set_visible(true)`,
    /// `set_left_image(None)`, `set_right_image(None)`, in that order; then BOTH partner previews are requested before either is awaited (two worker jobs running
    /// `load_compare_preview_blocking` with the navigation's viewer-decode token and the busy-retry delay). Phase `AwaitCompare(Previews)`.
    pub(super) fn begin_compare(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation, pair: (String, String)) {
        todo!()
    }

    /// One partner preview arrived. When both are in: error handling and the continuation (CVM lines 197 to 240: token check, images, sizes request or empty sizes,
    /// hash decision). Error rule: after BOTH finished, the LEFT error wins if both failed (C# `Task.WhenAll` rethrows the first); a non-cancellation error while the
    /// navigation is current goes to `compare_partner_failed`; a cancellation goes to `fail`.
    pub(super) fn on_compare_preview(
        &mut self,
        env: &mut PresenterEnv<'_>,
        token: i64,
        side: CompareSide,
        result: Result<PlatformImage, PreviewError>,
    ) {
        todo!()
    }

    /// The sizes arrived: token check (`false` returned by `LoadAsync` ends the present), `left_size` / `right_size` suffixes, `compare.set_left_size_text / set_right_size_text`,
    /// then the hash step.
    pub(super) fn on_compare_sizes(
        &mut self,
        env: &mut PresenterEnv<'_>,
        token: i64,
        left: Option<i64>,
        right: Option<i64>,
    ) {
        todo!()
    }

    /// One hash arrived (both needed). A non-cancellation `FileHashError` is logged (`Compare hash failed path=..`) and counts as "unknown"; `FileHashError::Canceled`
    /// is a `PresentFailure::HashCanceled`. Then token check, `hash_text`, `compare.set_hash_text`, status text and `finish_compare`.
    pub(super) fn on_compare_hash(
        &mut self,
        env: &mut PresenterEnv<'_>,
        token: i64,
        side: CompareSide,
        result: Result<String, FileHashError>,
    ) {
        todo!()
    }

    /// The compare partner vanished or cannot be decoded (lines 545 to 560): log `ShowImage compare partner failed token=.. path=.. partner=..`, `compare.clear(ctx)`,
    /// `current_photo_info = PhotoInfo::from_image(path, image)`, `update_current_image(Some(image.platform_image().clone()), image.original_width(), image.original_height(), Some(path))`,
    /// `sink.apply_initial_view_mode()` (+ zoom sync), `zoom_detail.on_preview_presented(token, path, current_key, image)` (apply effects), status
    /// `status::image_error(file_name(partner), describe(error), ctx)` with attention. The present ends here (no step 7).
    pub(super) fn compare_partner_failed(
        &mut self,
        env: &mut PresenterEnv<'_>,
        nav: Navigation,
        pair: (String, String),
        error: PreviewError,
    ) {
        todo!()
    }

    /// Lines 562 to 566 and step 7: `perf TracePresented("compare")`, `update_status(compare.status_text(), false)`, then `finish_present`.
    pub(super) fn finish_compare(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation) {
        todo!()
    }

    /// Single-image branch, lines 568 to 600: `compare.clear(ctx)`, `sink.apply_initial_view_mode()` (+ zoom sync), `zoom_detail.on_preview_presented(token, path, key, image)`;
    /// with a UI scheduler: post the frame yield and wait (`AwaitFrameYield`), without one continue at once. Then the original dimensions (skipped in Original
    /// loading mode, where they are the preview's pixel size; a known value is used without a worker hop), then the refresh stat on the stat worker (`AwaitRefreshStat`).
    pub(super) fn begin_single_image_branch(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation) {
        todo!()
    }

    /// The frame yield returned: `!is_navigation_current(token)` ends; else `begin_dimensions`.
    pub(super) fn on_post_frame_yield(&mut self, env: &mut PresenterEnv<'_>, token: i64) {
        todo!()
    }

    /// Lines 586 to 596: perf `post_start("dims")`; Original mode: `original = (image.pixel_width(), image.pixel_height())`; else the known dimensions of `current_key`
    /// (`service.try_get_known_original_dimensions`) or one worker job running `try_get_original_dimensions`; then the token check and `begin_refresh_stat`.
    pub(super) fn begin_dimensions(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation) {
        todo!()
    }

    /// The dimension lookup returned. `Ok(Some(dims))` / `Ok(None)` (expected failure): `original = dims`. `Err(e)`: a `PresentFailure::Preview(e)` through `fail`.
    pub(super) fn on_dimensions(
        &mut self,
        env: &mut PresenterEnv<'_>,
        token: i64,
        result: Result<Option<(i32, i32)>, PreviewError>,
    ) {
        todo!()
    }

    /// Line 598: the post-present refresh stat goes to the stat worker with no cancellation token (`AwaitRefreshStat`).
    pub(super) fn begin_refresh_stat(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation) {
        todo!()
    }

    /// Lines 599 to 625: token check; `Outcome != Found` (RV-A04): status `with_dimensions(index, count, initial_size, w, h, file_name)` when the dimensions are known, else
    /// `ready(index, count, initial_size, file_name)`, NOT attention, the catalog untouched; `Found`: when `path_override.is_none()` and the initial entry's length or last-write
    /// differs from the new stat, `catalog.update_metadata(path, length, last_write, None, None)`; status with the NEW length. Then `finish_present`.
    pub(super) fn on_refresh_stat(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: StatResult) {
        todo!()
    }

    /// Step 7, lines 627 to 645: token check; when `env.session` is `Some` and a session store exists: `session.current_path = Some(path)`, `session.updated_utc =
    /// JournalDateTime::utc(DateTimeUtc::now())`, then `session_writer.update(session)` when a writer exists (debounced) else `session_store.save(session)` (a `BlankFolder` is a
    /// `PresentFailure::Session` through `fail`); perf `post_start/post_end("session")`; `metrics.record_presented(elapsed ms)`; the navigation ends (`self.navigation = None`).
    pub(super) fn finish_present(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation) {
        todo!()
    }

    /// The `catch` clauses, lines 646 to 711, in this order (first match wins):
    /// 1. `failure.is_canceled() && !is_navigation_current(token)`: log `ShowImage superseded token=.. path=..` (when enabled); end.
    /// 2. `is_navigation_current(token) && failure.is_not_found()`: the stale-NotFound path: request the recheck stat (`AwaitRecheckStat`), see `on_recheck_stat`.
    /// 3. `is_navigation_current(token)`: the generic failure: log `ShowImage failed token=.. index=.. path=..`; `previous_navigation_path` / `same_entry_as_shown` logic (the
    ///    `_currentNavigationPath` restore), `current_photo_info = None`, when not `same_entry_as_shown`: `update_current_image(None, 0, 0, None)` and `compare.clear(ctx)`; status
    ///    `image_error(file_name(path), describe(failure), ctx)` with attention. End.
    /// 4. otherwise (stale): log `ShowImage failed (stale token=.., current=..) index=.. path=..`; nothing else.
    pub(super) fn fail(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation, failure: PresentFailure) {
        todo!()
    }

    /// The code after the stale-NotFound recheck stat (lines 659 to 683): `!is_navigation_current(token)` ends. `Found && stale_not_found_retries >= 3`: log
    /// `ShowImage stale-file failures persisted while the file exists`, `current_photo_info = None`, `update_current_image(None, ..)`, `compare.clear`, error status with attention, end.
    /// `Found`: log (when enabled); `retry_index = catalog.index_of(path)`; `>= 0`: `begin_present(PresentRequest { index: retry_index, .. }, retries + 1,
    /// original_previous_navigation_path)`; end. Anything else (`Missing` OR `Error`): log `ShowImage stale-file token=.. path=..` and `remove_missing_catalog_item(path, token)`.
    pub(super) fn on_recheck_stat(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: StatResult) {
        todo!()
    }
}
