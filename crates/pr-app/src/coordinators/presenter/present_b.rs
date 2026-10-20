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

use std::error::Error;
use std::path::Path;
use std::sync::Arc;

use pr_core::catalog::types::is_raw_path;
use pr_core::diagnostics::perf::{self, PhotoReviewPerf};
use pr_core::file_actions::journal_line::JournalDateTime;
use pr_core::model::LoadingMode;
use pr_core::session::store::BlankFolder;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::{PathError, get_file_name};
use pr_core::text::eq_ignore_case;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache::source_bytes::SourceBytesError;
use pr_imaging::cache::thumbnails::ThumbnailError;
use pr_imaging::cache_key::CacheKeyError;
use pr_imaging::decode_types::DecodedImage;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::service::PreviewError;
use pr_l10n::template::{LocArg, describe};

use crate::misc::app_log::AppLog;
use crate::misc::file_hash::FileHashError;
use crate::viewmodels::compare::CompareChanges;
use crate::viewmodels::format::context::FormatContext;
use crate::viewmodels::format::status::{self, CompareStatus};

use super::compare_pair::{compare_file_size, load_compare_preview_blocking};
use super::photo_info::{PhotoInfo, StatOutcome, StatResult, try_get_original_dimensions};
use super::present_a::{catalog_count, thumbnail_error_is_canceled};
use super::state::{
    CompareSide, ImagePresenter, Navigation, Phase, PresentRequest, PresenterChanges, PresenterEnv, PresenterEvent,
    StatPurpose, YieldPurpose,
};

/// Thread name of the jobs that load the two compare previews.
const COMPARE_PREVIEW_THREAD: &str = "PhotoReviewRs.ComparePreview";
/// Thread name of the job that reads the two compare file sizes.
const COMPARE_SIZE_THREAD: &str = "PhotoReviewRs.CompareSizes";
/// Thread name of the jobs that hash the two compare files.
const COMPARE_HASH_THREAD: &str = "PhotoReviewRs.CompareHash";
/// Thread name of the job that reads the original dimensions.
const DIMENSIONS_THREAD: &str = "PhotoReviewRs.OriginalDimensions";
/// C# `MaxStaleNotFoundRetries`: re-presents after a FileNotFound the file system contradicts are bounded.
const MAX_STALE_NOT_FOUND_RETRIES: u8 = 3;

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

/// The catalog text `key` with its `{path}` argument.
fn render_path_text(ctx: &FormatContext<'_>, key: &str, path: &str) -> String {
    ctx.texts.format_with(key, &[LocArg::new("path", path)], ctx.numbers)
}

/// `UserFacingError.Describe` of a cache-key failure: the missing source has a localized sentence.
fn describe_cache_key(error: &CacheKeyError, ctx: &FormatContext<'_>) -> String {
    match error {
        CacheKeyError::SourceNoLongerExists { full_name } => {
            render_path_text(ctx, "err.io.sourceNoLongerExists", full_name)
        }
        other => describe(other),
    }
}

/// `UserFacingError.Describe` of a preview failure.
fn describe_preview_error(error: &PreviewError, ctx: &FormatContext<'_>) -> String {
    if let Some((key, path)) = error.catalog_key() {
        return render_path_text(ctx, key, path);
    }
    match error {
        PreviewError::Decode(decode) => decode.describe(),
        PreviewError::SourceBytes(source) => source.describe(),
        PreviewError::Key(key) => describe_cache_key(key, ctx),
        other => describe(other),
    }
}

impl PresentFailure {
    /// C# `ex is OperationCanceledException`: `Preview(e)` with `e.is_canceled()`, `Thumbnail(Canceled)`, `HashCanceled`.
    pub(super) fn is_canceled(&self) -> bool {
        match self {
            Self::Preview(error) => error.is_canceled(),
            Self::Thumbnail(error) => thumbnail_error_is_canceled(error),
            Self::HashCanceled => true,
            Self::ComparePair(_) | Self::Session(_) | Self::CacheKey(_) => false,
        }
    }

    /// C# `ex is FileNotFoundException || ex is DirectoryNotFoundException`: `Preview(Decode(e))` with `e.kind()` `FileNotFound` or `DirectoryNotFound`,
    /// `Preview(Key(SourceNoLongerExists))`, `Preview(Io(e))` / `Preview(SourceBytes(Io(e)))` with `ErrorKind::NotFound`, `CacheKey(SourceNoLongerExists)`.
    /// NOT `SourceChanged` (an `IOException` that is neither).
    pub(super) fn is_not_found(&self) -> bool {
        match self {
            Self::Preview(error) => match error {
                PreviewError::Decode(decode) => {
                    matches!(
                        decode.kind(),
                        DecodeError::FileNotFound { .. } | DecodeError::DirectoryNotFound { .. }
                    )
                }
                PreviewError::Key(CacheKeyError::SourceNoLongerExists { .. }) => true,
                PreviewError::Io(error) => error.kind() == std::io::ErrorKind::NotFound,
                PreviewError::SourceBytes(SourceBytesError::Io(error)) => error.kind() == std::io::ErrorKind::NotFound,
                _ => false,
            },
            Self::CacheKey(CacheKeyError::SourceNoLongerExists { .. }) => true,
            Self::Thumbnail(_) | Self::ComparePair(_) | Self::Session(_) | Self::CacheKey(_) | Self::HashCanceled => {
                false
            }
        }
    }

    /// C# `UserFacingError.Describe(ex)`: the localized sentence when the error carries one, else its message. `SourceChanged` renders the catalog key
    /// of `PreviewError::catalog_key()` with `{path}` through `ctx.texts` (D-W8-4: Vietnamese text comes from `pr_l10n`, never from a literal here);
    /// `Preview(Decode(e))` -> `e.describe()`; `Preview(SourceBytes(e))` -> `e.describe()`; `Key(SourceNoLongerExists { full_name })` -> catalog key
    /// `err.io.sourceNoLongerExists` with `{path}`; everything else its `Display`.
    pub(super) fn describe(&self, ctx: &FormatContext<'_>) -> String {
        match self {
            Self::Preview(error) => describe_preview_error(error, ctx),
            Self::Thumbnail(error) => describe(error),
            Self::ComparePair(error) => describe(error),
            Self::Session(error) => describe(error),
            Self::CacheKey(error) => describe_cache_key(error, ctx),
            Self::HashCanceled => "The operation was canceled.".to_owned(),
        }
    }

    /// The error as a trait object for `AppLog::error` (C# passes the exception).
    pub(super) fn as_error(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Preview(error) => Some(error),
            Self::Thumbnail(error) => Some(error),
            Self::ComparePair(error) => Some(error),
            Self::Session(error) => Some(error),
            Self::CacheKey(error) => Some(error),
            Self::HashCanceled => None,
        }
    }
}

/// Which wait of the compare load is open (the part of `CompareViewModel.LoadAsync` after each `await`).
#[derive(Debug)]
pub(super) enum CompareStage {
    /// Both partner previews were requested; `left` / `right` are filled as their events arrive. The step ends when BOTH are in (C# `Task.WhenAll`).
    Previews {
        left: Option<Result<Arc<DecodedImage>, PreviewError>>,
        right: Option<Result<Arc<DecodedImage>, PreviewError>>,
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
    /// C# `currentIndex` argument of `LoadAsync`: the (rebased) index at the moment the load started.
    pub(super) current_index: i32,
    /// C# `totalFiles` argument of `LoadAsync`: the catalog count at the moment the load started.
    pub(super) total_files: i32,
}

/// Whether a navigation at `phase` waits in the given compare stage.
fn in_compare_stage(phase: &Phase, wanted: fn(&CompareStage) -> bool) -> bool {
    matches!(phase, Phase::AwaitCompare(progress) if wanted(&progress.stage))
}

impl ImagePresenter {
    /// Marks `COMPARE` when a compare view model setter reported a change.
    fn note_compare(&mut self, changes: CompareChanges) {
        if !changes.is_empty() {
            self.changes |= PresenterChanges::COMPARE;
        }
    }

    /// The C# code from the end of the preload kick (line 512) to the branch decision (line 526): `pair = get_compare_pair(path,
    /// include_capture_group_in_compare || compare.is_visible())` (an `Err` is a `PresentFailure::ComparePair`); perf marks (`TracePresented("final", ..)` when no pair,
    /// `post_start("compare")` otherwise); then `begin_compare` when `pair.is_some() && request.allow_compare`, else the single-image branch.
    pub(super) fn after_preview_presented(&mut self, env: &mut PresenterEnv<'_>, mut nav: Navigation) {
        let token = nav.token;
        nav.perf_compare_start = if nav.perf { perf::timestamp() } else { 0 };
        // While compare is open every presentation (Next/Previous/undo, not only the toggle) compares the capture pair, otherwise navigation drops the
        // pair yet leaves compare visible.
        let include_capture_group = nav.request.include_capture_group_in_compare || self.compare.is_visible();
        let pair = match self.get_compare_pair(env.catalog, &nav.path, include_capture_group) {
            Ok(pair) => pair,
            Err(error) => {
                self.fail(env, nav, PresentFailure::ComparePair(error));
                return;
            }
        };
        if nav.perf {
            if pair.is_none() {
                self.sink.trace_presented(token, "final", nav.perf_assigned);
            } else {
                PhotoReviewPerf::log().post_start(token, "compare");
            }
        }
        match pair {
            Some(pair) if nav.request.allow_compare => self.begin_compare(env, nav, pair),
            _ => self.begin_single_image_branch(env, nav),
        }
    }

    /// Compare branch, lines 528 to 543 plus the start of `CompareViewModel.LoadAsync` (CVM lines 166 to 197, see the contract for the order): `current_photo_info = None`, `update_current_image(None, 0, 0, None)`;
    /// token check; `compare.set_left_path / set_right_path`, `compare.select(Some(path))` (the presented path, C# `initialSelectedPath: path`), `compare.set_visible(true)`,
    /// `set_left_image(None)`, `set_right_image(None)`, in that order; then BOTH partner previews are requested before either is awaited (two worker jobs running
    /// `load_compare_preview_blocking` with the navigation's viewer-decode token and the busy-retry delay). Phase `AwaitCompare(Previews)`.
    pub(super) fn begin_compare(&mut self, env: &mut PresenterEnv<'_>, mut nav: Navigation, pair: (String, String)) {
        let token = nav.token;
        self.current_photo_info = None; // two images: the compare status describes them
        self.changes |= PresenterChanges::PHOTO_INFO;
        self.update_current_image(None, 0, 0, None);
        // `LoadAsync` starts with its own token check.
        if !self.clock.is_navigation_current(token) {
            return;
        }
        let changes = self.compare.set_left_path(Some(pair.0.clone()));
        self.note_compare(changes);
        let changes = self.compare.set_right_path(Some(pair.1.clone()));
        self.note_compare(changes);
        let changes = self.compare.select(Some(nav.path.clone()));
        self.note_compare(changes);
        let changes = self.compare.set_visible(true);
        self.note_compare(changes);
        let changes = self.compare.set_left_image(None);
        self.note_compare(changes);
        let changes = self.compare.set_right_image(None);
        self.note_compare(changes);

        // 1. Load the two images in parallel: both are requested before either result is looked at.
        for (side, path) in [
            (CompareSide::Left, pair.0.clone()),
            (CompareSide::Right, pair.1.clone()),
        ] {
            let service = Arc::clone(&self.preview_service);
            let cancel = nav.viewer_cancel.clone();
            let delay = Arc::clone(&self.busy_retry_delay);
            let inbox = self.inbox.clone();
            self.runner.run(
                COMPARE_PREVIEW_THREAD,
                Box::new(move || {
                    let result = load_compare_preview_blocking(&service, &path, &cancel, &delay);
                    inbox.send(PresenterEvent::ComparePreview { token, side, result });
                }),
            );
        }
        nav.phase = Phase::AwaitCompare(CompareProgress {
            pair,
            stage: CompareStage::Previews {
                left: None,
                right: None,
            },
            left_size: String::new(),
            right_size: String::new(),
            current_index: nav.request.index,
            total_files: catalog_count(env),
        });
        self.navigation = Some(nav);
    }

    /// One partner preview arrived. When both are in: error handling and the continuation (CVM lines 197 to 240: token check, images, sizes request or empty sizes,
    /// hash decision). Error rule: after BOTH finished, the LEFT error wins if both failed (C# `Task.WhenAll` rethrows the first); a non-cancellation error while the
    /// navigation is current goes to `compare_partner_failed`; a cancellation goes to `fail`.
    pub(super) fn on_compare_preview(
        &mut self,
        env: &mut PresenterEnv<'_>,
        token: i64,
        side: CompareSide,
        result: Result<Arc<DecodedImage>, PreviewError>,
    ) {
        let Some(mut nav) = self.take_navigation(token, |phase| {
            in_compare_stage(phase, |stage| matches!(stage, CompareStage::Previews { .. }))
        }) else {
            return;
        };
        let slots = {
            let Phase::AwaitCompare(progress) = &mut nav.phase else {
                return;
            };
            let CompareStage::Previews { left, right } = &mut progress.stage else {
                return;
            };
            match side {
                CompareSide::Left => *left = Some(result),
                CompareSide::Right => *right = Some(result),
            }
            if left.is_some() && right.is_some() {
                Some((left.take(), right.take(), progress.pair.clone()))
            } else {
                None
            }
        };
        // Both finished (C# `Task.WhenAll`); until then the navigation keeps waiting.
        let Some((Some(left), Some(right), pair)) = slots else {
            self.navigation = Some(nav);
            return;
        };
        let (left, right) = match (left, right) {
            (Ok(left), Ok(right)) => (left, right),
            // The LEFT error wins when both failed.
            (Err(error), _) | (Ok(_), Err(error)) => {
                if !error.is_canceled() && self.clock.is_navigation_current(token) {
                    self.compare_partner_failed(env, nav, pair, error);
                } else {
                    self.fail(env, nav, PresentFailure::Preview(error));
                }
                return;
            }
        };
        // Token check after the await.
        if !self.clock.is_navigation_current(token) {
            return;
        }
        let changes = self.compare.set_left_image(Some(left));
        self.note_compare(changes);
        let changes = self.compare.set_right_image(Some(right));
        self.note_compare(changes);

        // 2. Format the file sizes when enabled.
        let size_enabled = nav.settings.is_some_and(|settings| settings.compare_size_enabled);
        if size_enabled {
            // No injected sizes: stat off the UI thread (a NAS/wifi share can stall a metadata call for seconds).
            let inbox = self.inbox.clone();
            let (left_path, right_path) = pair;
            self.runner.run(
                COMPARE_SIZE_THREAD,
                Box::new(move || {
                    let left = compare_file_size(&left_path);
                    let right = compare_file_size(&right_path);
                    inbox.send(PresenterEvent::CompareSizes { token, left, right });
                }),
            );
            if let Phase::AwaitCompare(progress) = &mut nav.phase {
                progress.stage = CompareStage::Sizes;
            }
            self.navigation = Some(nav);
        } else {
            self.finish_compare_sizes(env, nav);
        }
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
        let Some(mut nav) = self.take_navigation(token, |phase| {
            in_compare_stage(phase, |stage| matches!(stage, CompareStage::Sizes))
        }) else {
            return;
        };
        if !self.clock.is_navigation_current(token) {
            return;
        }
        if let Phase::AwaitCompare(progress) = &mut nav.phase {
            if let Some(bytes) = left {
                progress.left_size = status::compare_size_suffix(bytes, &env.format);
            }
            if let Some(bytes) = right {
                progress.right_size = status::compare_size_suffix(bytes, &env.format);
            }
        }
        self.finish_compare_sizes(env, nav);
    }

    /// The end of step 2 (the size texts are set, empty when the sizes are off) and the hash decision of step 3.
    fn finish_compare_sizes(&mut self, env: &mut PresenterEnv<'_>, mut nav: Navigation) {
        let token = nav.token;
        let (left_size, right_size, pair) = match &nav.phase {
            Phase::AwaitCompare(progress) => (
                progress.left_size.clone(),
                progress.right_size.clone(),
                progress.pair.clone(),
            ),
            _ => return,
        };
        let changes = self.compare.set_left_size_text(left_size);
        self.note_compare(changes);
        let changes = self.compare.set_right_size_text(right_size);
        self.note_compare(changes);

        let hash_enabled = nav.settings.is_some_and(|settings| settings.compare_hash_enabled);
        if hash_enabled && is_raw_path(&pair.0) != is_raw_path(&pair.1) {
            // A RAW and a non-RAW file (a JPEG+RAW capture) can never be byte-identical: skip hashing both whole files from disk.
            let text = status::compare_hash_text(Some(false), &env.format);
            self.finish_compare_hashes(env, nav, text);
        } else if hash_enabled {
            for (side, path) in [(CompareSide::Left, pair.0), (CompareSide::Right, pair.1)] {
                let hasher = Arc::clone(&self.hasher);
                let inbox = self.inbox.clone();
                self.runner.run(
                    COMPARE_HASH_THREAD,
                    Box::new(move || {
                        let result = hasher.get(Path::new(&path), &CancelToken::none());
                        inbox.send(PresenterEvent::CompareHash { token, side, result });
                    }),
                );
            }
            if let Phase::AwaitCompare(progress) = &mut nav.phase {
                progress.stage = CompareStage::Hashes {
                    left: None,
                    right: None,
                };
            }
            self.navigation = Some(nav);
        } else {
            let text = status::compare_hash_text(None, &env.format);
            self.finish_compare_hashes(env, nav, text);
        }
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
        let Some(mut nav) = self.take_navigation(token, |phase| {
            in_compare_stage(phase, |stage| matches!(stage, CompareStage::Hashes { .. }))
        }) else {
            return;
        };
        let slots = {
            let Phase::AwaitCompare(progress) = &mut nav.phase else {
                return;
            };
            let CompareStage::Hashes { left, right } = &mut progress.stage else {
                return;
            };
            match side {
                CompareSide::Left => *left = Some(result),
                CompareSide::Right => *right = Some(result),
            }
            if left.is_some() && right.is_some() {
                Some((left.take(), right.take(), progress.pair.clone()))
            } else {
                None
            }
        };
        let Some((Some(left), Some(right), pair)) = slots else {
            self.navigation = Some(nav);
            return;
        };
        // A cancelled hash ends the whole load (`Task.WhenAll` rethrows the cancellation); every other failure is only "unknown".
        if matches!(left, Err(FileHashError::Canceled)) || matches!(right, Err(FileHashError::Canceled)) {
            self.fail(env, nav, PresentFailure::HashCanceled);
            return;
        }
        let log = AppLog::global();
        for (path, hash) in [(&pair.0, &left), (&pair.1, &right)] {
            if let Err(error) = hash {
                log.error(&format!("Compare hash failed path={path}"), Some(error));
            }
        }
        // Token check after the await.
        if !self.clock.is_navigation_current(token) {
            return;
        }
        // A failed hash (file changed / unreadable) must not cancel the comparison: the two loaded images still show, the hash is "unknown".
        let text = match (left, right) {
            (Ok(left), Ok(right)) => status::compare_hash_text(Some(eq_ignore_case(&left, &right)), &env.format),
            _ => status::compare_hash_unknown(&env.format),
        };
        self.finish_compare_hashes(env, nav, text);
    }

    /// The end of `CompareViewModel.LoadAsync` (step 4): the hash text, the status text, then `finish_compare`.
    fn finish_compare_hashes(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation, hash_text: String) {
        let (pair, left_size, right_size, current_index, total_files) = match &nav.phase {
            Phase::AwaitCompare(progress) => (
                progress.pair.clone(),
                progress.left_size.clone(),
                progress.right_size.clone(),
                progress.current_index,
                progress.total_files,
            ),
            _ => return,
        };
        let changes = self.compare.set_hash_text(hash_text);
        self.note_compare(changes);
        let text = status::compare(
            &CompareStatus {
                index: current_index,
                count: total_files,
                left_name: get_file_name(&pair.0),
                left_size: &left_size,
                right_name: get_file_name(&pair.1),
                right_size: &right_size,
                hash_text: self.compare.hash_text(),
            },
            &env.format,
        );
        let changes = self.compare.set_status_text(text);
        self.note_compare(changes);
        self.finish_compare(env, nav);
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
        let token = nav.token;
        // The compare partner vanished or cannot be decoded. That must not take the current image (which is fine) out of the catalog via the stale-file
        // path: show it alone and name the partner.
        let partner = if eq_ignore_case(&pair.0, &nav.path) {
            &pair.1
        } else {
            &pair.0
        };
        AppLog::global().error(
            &format!(
                "ShowImage compare partner failed token={token} path={} partner={partner}",
                nav.path
            ),
            Some(&error),
        );
        let changes = self.compare.clear(&env.format);
        self.note_compare(changes);
        if let Some(image) = nav.image.as_ref() {
            self.current_photo_info = Some(PhotoInfo::from_image(&nav.path, image.as_ref()));
            self.changes |= PresenterChanges::PHOTO_INFO;
            self.update_current_image(
                Some(image.platform_image().clone()),
                image.original_width(),
                image.original_height(),
                Some(&nav.path),
            );
            self.sink.apply_initial_view_mode();
            self.sync_viewer_zoom_from_sink();
            // feat(zoom): match the normal (non-compare) branch -- arm the full-resolution decode target so zooming into this image after a failed compare works at once.
            if let Some(key) = nav.current_key.as_ref() {
                let effects = self.zoom_detail.on_preview_presented(token, &nav.path, key, image);
                self.apply_zoom_effects(effects);
            }
        }
        let text = status::image_error(
            get_file_name(partner),
            &PresentFailure::Preview(error).describe(&env.format),
            &env.format,
        );
        self.update_status(text, true);
    }

    /// Lines 562 to 566 and step 7: `perf TracePresented("compare")`, `update_status(compare.status_text(), false)`, then `finish_present`.
    pub(super) fn finish_compare(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation) {
        let token = nav.token;
        if nav.perf {
            self.sink.trace_presented(token, "compare", perf::timestamp());
        }
        let text = self.compare.status_text().to_owned();
        self.update_status(text, false);
        if nav.perf {
            PhotoReviewPerf::log().post_end(token, "compare", perf::ms(nav.perf_compare_start));
        }
        self.finish_present(env, nav);
    }

    /// Single-image branch, lines 568 to 600: `compare.clear(ctx)`, `sink.apply_initial_view_mode()` (+ zoom sync), `zoom_detail.on_preview_presented(token, path, key, image)`;
    /// with a UI scheduler: post the frame yield and wait (`AwaitFrameYield`), without one continue at once. Then the original dimensions (skipped in Original
    /// loading mode, where they are the preview's pixel size; a known value is used without a worker hop), then the refresh stat on the stat worker (`AwaitRefreshStat`).
    pub(super) fn begin_single_image_branch(&mut self, env: &mut PresenterEnv<'_>, mut nav: Navigation) {
        let token = nav.token;
        let changes = self.compare.clear(&env.format);
        self.note_compare(changes);
        self.sink.apply_initial_view_mode();
        // feat(zoom): if the viewer is (still) zoomed after the initial view mode, start this image's full-resolution decode; the preview stays up until it is ready.
        self.sync_viewer_zoom_from_sink();
        if let (Some(key), Some(image)) = (nav.current_key.as_ref(), nav.image.as_ref()) {
            let effects = self.zoom_detail.on_preview_presented(token, &nav.path, key, image);
            self.apply_zoom_effects(effects);
        }

        // Let the frame containing the new image render before the status/session bookkeeping.
        if let Some(scheduler) = self.ui_scheduler.clone() {
            let inbox = self.inbox.clone();
            scheduler.post(Box::new(move || {
                inbox.send(PresenterEvent::FrameYielded {
                    token,
                    purpose: YieldPurpose::PostFrame,
                });
            }));
            nav.phase = Phase::AwaitFrameYield;
            self.navigation = Some(nav);
            return;
        }
        self.begin_dimensions(env, nav);
    }

    /// The frame yield returned: `!is_navigation_current(token)` ends; else `begin_dimensions`.
    pub(super) fn on_post_frame_yield(&mut self, env: &mut PresenterEnv<'_>, token: i64) {
        let Some(nav) = self.take_navigation(token, |phase| matches!(phase, Phase::AwaitFrameYield)) else {
            return;
        };
        if !self.clock.is_navigation_current(token) {
            return;
        }
        self.begin_dimensions(env, nav);
    }

    /// Lines 586 to 596: perf `post_start("dims")`; Original mode: `original = (image.pixel_width(), image.pixel_height())`; else the known dimensions of `current_key`
    /// (`service.try_get_known_original_dimensions`) or one worker job running `try_get_original_dimensions`; then the token check and `begin_refresh_stat`.
    pub(super) fn begin_dimensions(&mut self, env: &mut PresenterEnv<'_>, mut nav: Navigation) {
        let token = nav.token;
        let mode = nav
            .settings
            .map_or(LoadingMode::Preview, |settings| settings.loading_mode);
        let perf_dims = nav.perf && mode != LoadingMode::Original;
        if perf_dims {
            PhotoReviewPerf::log().post_start(token, "dims");
        }
        let perf_dims_start = if perf_dims { perf::timestamp() } else { 0 };

        let known = if mode == LoadingMode::Original {
            nav.image
                .as_ref()
                .map(|image| (image.pixel_width(), image.pixel_height()))
        } else {
            nav.current_key
                .as_ref()
                .and_then(|key| self.preview_service.try_get_known_original_dimensions(key))
        };
        let Some(dimensions) = known else {
            // A worker reads the header (`ReadInfo`); the result comes back as an event.
            let Some(key) = nav.current_key.clone() else {
                nav.original_dimensions = None;
                self.begin_refresh_stat(env, nav);
                return;
            };
            let service = Arc::clone(&self.preview_service);
            let inbox = self.inbox.clone();
            let path = nav.path.clone();
            self.runner.run(
                DIMENSIONS_THREAD,
                Box::new(move || {
                    let result = try_get_original_dimensions(&service, &path, &key);
                    inbox.send(PresenterEvent::Dimensions { token, result });
                }),
            );
            nav.perf_dims_start = perf_dims_start;
            nav.phase = Phase::AwaitDimensions;
            self.navigation = Some(nav);
            return;
        };
        nav.original_dimensions = Some(dimensions);
        if perf_dims {
            PhotoReviewPerf::log().post_end(token, "dims", perf::ms(perf_dims_start));
        }
        if !self.clock.is_navigation_current(token) {
            return;
        }
        self.begin_refresh_stat(env, nav);
    }

    /// The dimension lookup returned. `Ok(Some(dims))` / `Ok(None)` (expected failure): `original = dims`. `Err(e)`: a `PresentFailure::Preview(e)` through `fail`.
    pub(super) fn on_dimensions(
        &mut self,
        env: &mut PresenterEnv<'_>,
        token: i64,
        result: Result<Option<(i32, i32)>, PreviewError>,
    ) {
        let Some(mut nav) = self.take_navigation(token, |phase| matches!(phase, Phase::AwaitDimensions)) else {
            return;
        };
        match result {
            Ok(dimensions) => {
                nav.original_dimensions = dimensions;
                let mode = nav
                    .settings
                    .map_or(LoadingMode::Preview, |settings| settings.loading_mode);
                if nav.perf && mode != LoadingMode::Original {
                    PhotoReviewPerf::log().post_end(token, "dims", perf::ms(nav.perf_dims_start));
                }
                if !self.clock.is_navigation_current(token) {
                    return;
                }
                self.begin_refresh_stat(env, nav);
            }
            Err(error) => self.fail(env, nav, PresentFailure::Preview(error)),
        }
    }

    /// Line 598: the post-present refresh stat goes to the stat worker with no cancellation token (`AwaitRefreshStat`).
    pub(super) fn begin_refresh_stat(&mut self, _env: &mut PresenterEnv<'_>, mut nav: Navigation) {
        // Q-R29 option C: the post-present refresh stat runs off the UI thread too.
        self.submit_stat(nav.token, &nav.path, StatPurpose::Refresh, &CancelToken::none());
        nav.phase = Phase::AwaitRefreshStat;
        self.navigation = Some(nav);
    }

    /// Lines 599 to 625: token check; `Outcome != Found` (RV-A04): status `with_dimensions(index, count, initial_size, w, h, file_name)` when the dimensions are known, else
    /// `ready(index, count, initial_size, file_name)`, NOT attention, the catalog untouched; `Found`: when `path_override.is_none()` and the initial entry's length or last-write
    /// differs from the new stat, `catalog.update_metadata(path, length, last_write, None, None)`; status with the NEW length. Then `finish_present`.
    pub(super) fn on_refresh_stat(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: StatResult) {
        let Some(nav) = self.take_navigation(token, |phase| matches!(phase, Phase::AwaitRefreshStat)) else {
            return;
        };
        if !self.clock.is_navigation_current(token) {
            return;
        }
        let index = nav.request.index;
        let count = catalog_count(env);
        let file_name = get_file_name(&nav.path);
        let text = match (result.outcome, result.stat) {
            (StatOutcome::Found, Some(current)) => {
                let length = i64::try_from(current.length).unwrap_or(i64::MAX);
                // A capture-group member shown via a path override is a different file from the entry's representative: its stat must not overwrite the
                // representative's Length/LastWriteUtc (that evicted the cache each toggle).
                let changed = nav.initial_entry.is_some_and(|entry| {
                    entry.length != Some(length) || entry.last_write_utc != Some(current.last_write_utc)
                });
                if nav.request.path_override.is_none() && changed {
                    env.catalog
                        .update_metadata(&nav.path, length, current.last_write_utc, None, None);
                }
                // Unknown dimensions (the lookup failed for a file that is shown fine): the status line just omits them.
                match nav.original_dimensions {
                    Some((width, height)) => {
                        status::with_dimensions(index, count, length, width, height, file_name, &env.format)
                    }
                    None => status::ready(index, count, length, file_name, &env.format),
                }
            }
            _ => {
                // RV-A04: the file was decoded and is on screen, but the refresh stat failed (vanished or a share hiccup). Do not leave the status on
                // "Loading" and skip the session save: report it ready with the size known before the stat. The photo stays in the catalog.
                match nav.original_dimensions {
                    Some((width, height)) => {
                        status::with_dimensions(index, count, nav.initial_size, width, height, file_name, &env.format)
                    }
                    None => status::ready(index, count, nav.initial_size, file_name, &env.format),
                }
            }
        };
        self.update_status(text, false);
        self.finish_present(env, nav);
    }

    /// Step 7, lines 627 to 645: token check; when `env.session` is `Some` and a session store exists: `session.current_path = Some(path)`, `session.updated_utc =
    /// JournalDateTime::utc(DateTimeUtc::now())`, then `session_writer.update(session)` when a writer exists (debounced) else `session_store.save(session)` (a `BlankFolder` is a
    /// `PresentFailure::Session` through `fail`); perf `post_start/post_end("session")`; `metrics.record_presented(elapsed ms)`; the navigation ends (`self.navigation = None`).
    pub(super) fn finish_present(&mut self, env: &mut PresenterEnv<'_>, nav: Navigation) {
        let token = nav.token;
        if !self.clock.is_navigation_current(token) {
            return;
        }
        if let (Some(session), Some(store)) = (env.session.as_deref_mut(), self.session_store.as_ref()) {
            session.current_path = Some(nav.path.clone());
            session.updated_utc = JournalDateTime::utc(DateTimeUtc::now());
            let perf_session = if nav.perf { perf::timestamp() } else { 0 };
            if nav.perf {
                PhotoReviewPerf::log().post_start(token, "session");
            }
            let saved = match self.session_writer.as_ref() {
                Some(writer) => writer.update(session), // debounced; flushed on folder change and shutdown
                None => store.save(session),
            };
            if let Err(error) = saved {
                self.fail(env, nav, PresentFailure::Session(error));
                return;
            }
            if nav.perf {
                PhotoReviewPerf::log().post_end(token, "session", perf::ms(perf_session));
            }
        }
        let elapsed = i64::try_from(nav.started.elapsed().as_millis()).unwrap_or(i64::MAX);
        self.metrics.record_presented(elapsed);
    }

    /// The `catch` clauses, lines 646 to 711, in this order (first match wins):
    ///
    /// 1. `failure.is_canceled() && !is_navigation_current(token)`: log `ShowImage superseded token=.. path=..` (when enabled); end.
    /// 2. `is_navigation_current(token) && failure.is_not_found()`: the stale-NotFound path: request the recheck stat (`AwaitRecheckStat`), see `on_recheck_stat`.
    /// 3. `is_navigation_current(token)`: the generic failure: log `ShowImage failed token=.. index=.. path=..`; `previous_navigation_path` / `same_entry_as_shown` logic (the
    ///    `_currentNavigationPath` restore), `current_photo_info = None`, when not `same_entry_as_shown`: `update_current_image(None, 0, 0, None)` and `compare.clear(ctx)`; status
    ///    `image_error(file_name(path), describe(failure), ctx)` with attention. End.
    /// 4. otherwise (stale): log `ShowImage failed (stale token=.., current=..) index=.. path=..`; nothing else.
    pub(super) fn fail(&mut self, env: &mut PresenterEnv<'_>, mut nav: Navigation, failure: PresentFailure) {
        let token = nav.token;
        let index = nav.request.index;
        let log = AppLog::global();
        let current = self.clock.is_navigation_current(token);

        if failure.is_canceled() && !current {
            // perf(preload): a newer navigation cancelled this one's not-yet-started viewer decode.
            if log.enabled() {
                log.info(&format!("ShowImage superseded token={token} path={}", nav.path));
            }
            return;
        }

        if current && failure.is_not_found() {
            // A decode that is still in flight is shared by every request for the same key (path + length + mtime), so this FileNotFound may belong to an
            // EARLIER state of the path: the file was moved away while that decode opened it and has come back since (Undo of the Move keeps length and mtime).
            // Trust the file system, not the joined failure.
            self.submit_stat(token, &nav.path, StatPurpose::Recheck, &CancelToken::none());
            nav.phase = Phase::AwaitRecheckStat;
            nav.pending_failure = Some(failure);
            self.navigation = Some(nav);
            return;
        }

        if current {
            log.error(
                &format!("ShowImage failed token={token} index={index} path={}", nav.path),
                failure.as_error(),
            );
            // A failed member switch (e.g. the RAW of a JPG+RAW capture) leaves the previous member on screen: keep the presented path (badge, file actions)
            // on what is really shown. A different entry keeps the new path.
            let previous = nav.previous_navigation_path.as_deref();
            let same_entry_as_shown = previous
                .is_some_and(|previous| eq_ignore_case(previous, &nav.path) || env.catalog.index_of(previous) == index);
            if let Some(previous) = previous {
                if !eq_ignore_case(previous, &nav.path) && same_entry_as_shown {
                    self.current_navigation_path = Some(previous.to_owned());
                    self.changes |= PresenterChanges::CURRENT_PATH;
                }
            }
            self.current_photo_info = None;
            self.changes |= PresenterChanges::PHOTO_INFO;
            if !same_entry_as_shown {
                // A different entry failed: the previous photo must not stay visible under this file's error status, otherwise Delete/Move would act on a
                // file the user never saw (same as the stat-error branch).
                self.update_current_image(None, 0, 0, None);
                let changes = self.compare.clear(&env.format);
                self.note_compare(changes);
            }
            let text = status::image_error(get_file_name(&nav.path), &failure.describe(&env.format), &env.format);
            self.update_status(text, true);
            return;
        }

        log.error(
            &format!(
                "ShowImage failed (stale token={token}, current={}) index={index} path={}",
                self.clock.current_navigation(),
                nav.path
            ),
            failure.as_error(),
        );
    }

    /// The code after the stale-NotFound recheck stat (lines 659 to 683): a superseded present ends. `Found && stale_not_found_retries >= 3`: log
    /// `ShowImage stale-file failures persisted while the file exists`, `current_photo_info = None`, `update_current_image(None, ..)`, `compare.clear`, error status with attention, end.
    /// `Found`: log (when enabled); `retry_index = catalog.index_of(path)`; `>= 0`: `begin_present(PresentRequest { index: retry_index, .. }, retries + 1,
    /// original_previous_navigation_path)`; end. Anything else (`Missing` OR `Error`): log `ShowImage stale-file token=.. path=..` and the missing-item removal.
    pub(super) fn on_recheck_stat(&mut self, env: &mut PresenterEnv<'_>, token: i64, result: StatResult) {
        let Some(mut nav) = self.take_navigation(token, |phase| matches!(phase, Phase::AwaitRecheckStat)) else {
            return;
        };
        if !self.clock.is_navigation_current(token) {
            return;
        }
        let log = AppLog::global();
        let found = result.outcome == StatOutcome::Found;
        if found && nav.stale_not_found_retries >= MAX_STALE_NOT_FOUND_RETRIES {
            // Under load the retry can join yet another failure from before the file came back. A file that is on disk is never dropped from the catalog:
            // report it like an unreadable file and let the user present it again.
            let failure = nav.pending_failure.take();
            log.error(
                &format!(
                    "ShowImage stale-file failures persisted while the file exists token={token} path={}",
                    nav.path
                ),
                failure.as_ref().and_then(PresentFailure::as_error),
            );
            self.current_photo_info = None;
            self.changes |= PresenterChanges::PHOTO_INFO;
            self.update_current_image(None, 0, 0, None);
            let changes = self.compare.clear(&env.format);
            self.note_compare(changes);
            let message = failure
                .as_ref()
                .map_or_else(String::new, |failure| failure.describe(&env.format));
            let text = status::image_error(get_file_name(&nav.path), &message, &env.format);
            self.update_status(text, true);
            return;
        }
        if found {
            if log.enabled() {
                log.info(&format!(
                    "ShowImage stale-file failure ignored (file exists again) token={token} path={}",
                    nav.path
                ));
            }
            let retry_index = env.catalog.index_of(&nav.path);
            if retry_index >= 0 {
                let request = PresentRequest {
                    index: retry_index,
                    allow_compare: nav.request.allow_compare,
                    path_override: nav.request.path_override.clone(),
                    include_capture_group_in_compare: nav.request.include_capture_group_in_compare,
                };
                let retries = nav.stale_not_found_retries.saturating_add(1);
                if let Err(error) = self.begin_present(env, request, retries, nav.previous_navigation_path.clone()) {
                    log.error(
                        &format!("ShowImage stale-file re-present failed token={token} path={}", nav.path),
                        Some(&error),
                    );
                }
            }
            return;
        }
        if log.enabled() {
            log.info(&format!("ShowImage stale-file token={token} path={}", nav.path));
        }
        self.begin_missing_run(env, &nav.path, token);
    }
}
