//! Port of `PhotoReview.App.Shared/Coordinators/ImagePresenter.cs` lines 1 to 232 (baseline d92d6aae) plus `UpdateCurrentImage`,
//! `UpdateStatus`, `ResetFileIdentity` and the properties of lines 800 to 832 and 932 (they are needed by `ShowZoomDetailImage` and
//! `ClearPresentation`, so they live with the state, see the contract). Contract: `contracts/W8-07h.md`. Target:
//! `crates/pr-app/src/coordinators/presenter/state.rs` (`coordinators::presenter::state`).
//!
//! `ImagePresenter` is defined HERE; W8-07i..k add `impl ImagePresenter { .. }` blocks in their own files (fields are `pub(super)`).
//!
//! Design (D-W8-1, D-W8-2, R0003):
//! - UI-thread only; no `async`. The C# `PresentAsync` task becomes a `Navigation` record resumed by [`ImagePresenter::pump`] when a
//!   worker completion (stat, preview, thumbnail, dimensions, compare previews and hashes) arrives in the presenter inbox. Every
//!   completion carries the navigation token and is dropped when `GenerationClock::is_navigation_current(token)` is false (INV-1).
//! - The presenter OWNS the sink (`Box<dyn IPresentationSink>`), the compare view model and the zoom-detail loader. It does NOT own the
//!   catalog, the settings, the session or the format context: those are lent for one call in [`PresenterEnv`].
//! - Mutators return [`PresenterChanges`] (a bit set); the shell and the main view model repaint / raise their notifications from it.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]
// Stub only (contract text in doc comments, boxed seams): the porter fixes the doc layout / introduces type aliases and removes this line.
#![allow(clippy::type_complexity, clippy::enum_variant_names)]

use std::fmt;
use std::ops::{BitOr, BitOrAssign};
use std::sync::Arc;
use std::time::Instant;

use pr_core::abstractions::{FileSystem, UiScheduler};
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::io::navigation_stat::NavigationStatWorker;
use pr_core::model::LoadingMode;
use pr_core::session::store::{SessionState, SessionStore};
use pr_core::session::writer::SessionWriter;
use pr_core::settings::app_settings::AppSettings;
use pr_core::sync::CancelToken;
use pr_imaging::cache::thumbnails::{ThumbnailCache, ThumbnailResult};
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::service::{PreviewError, PreviewImage, PreviewImageService, PreviewResult};

use crate::coordinators::traits::{IPreloadController, IPresentationSink};
use crate::coordinators::zoom_detail::{
    BackgroundRunner, CompletionInbox, IndicatorDelay, WakeFn, ZoomDetailEffects, ZoomDetailLoader,
};
use crate::misc::file_hash::{FileHashError, FileHasher};
use crate::viewmodels::compare::CompareViewModel;
use crate::viewmodels::format::context::FormatContext;

use super::compare_pair::{BusyRetryDelay, ComparePairCache};
use super::missing::MissingRun;
use super::photo_info::{PhotoInfo, StatResult};
use super::present_b::CompareProgress;

// ---- Public value types ----------------------------------------------------------------------------------------------------

/// Which presenter-owned properties changed during a call (D-W8-1: the Rust form of the C# `PropertyChanged` the main view model
/// raised from the sink callbacks). Combine with `|`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresenterChanges(pub u32);

impl PresenterChanges {
    /// Nothing changed.
    pub const NONE: Self = Self(0);
    /// `CurrentImage` / `CurrentOriginalWidth` / `CurrentOriginalHeight` / `IsSameSourceSwap`.
    pub const CURRENT_IMAGE: Self = Self(1);
    /// `StatusText` / `StatusNeedsAttention`.
    pub const STATUS: Self = Self(2);
    /// `CurrentPhotoInfo`.
    pub const PHOTO_INFO: Self = Self(4);
    /// Anything the compare view model reports (it is owned by the presenter; read `compare()`).
    pub const COMPARE: Self = Self(8);
    /// `ZoomDetail.IsRawDecodeIndicatorVisible` flipped.
    pub const RAW_DECODE_INDICATOR: Self = Self(16);
    /// `CurrentPresentedPath`.
    pub const CURRENT_PATH: Self = Self(32);

    /// True when every bit of `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// True when no bit is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for PresenterChanges {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for PresenterChanges {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// The arguments of C# `PresentAsync(index, allowCompare = true, pathOverride = null, includeCaptureGroupInCompare = false)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentRequest {
    pub index: i32,
    pub allow_compare: bool,
    pub path_override: Option<String>,
    pub include_capture_group_in_compare: bool,
}

impl PresentRequest {
    /// `PresentAsync(index)`: compare allowed, no override, capture group not forced into the compare.
    pub fn new(index: i32) -> Self {
        Self {
            index,
            allow_compare: true,
            path_override: None,
            include_capture_group_in_compare: false,
        }
    }
}

/// The synchronous `ArgumentException` of `PresentCoreAsync` (line 240).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentError {
    /// "The presentation path must belong to the selected catalog entry." (`pathOverride` is not a member of entry `index`).
    PathNotInEntry,
}

impl fmt::Display for PresentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The presentation path must belong to the selected catalog entry.")
    }
}

impl std::error::Error for PresentError {}

/// Everything the presenter reads from the application for ONE call (instead of the C# long-lived references): the shared catalog
/// (UI-thread only, `&mut`), the settings value (C# `Func<AppSettings>`), the live session (C# `Func<SessionState?>`) and the format context for status
/// text (D-W8-4). Built by the main view model per call.
#[derive(Debug)]
pub struct PresenterEnv<'a> {
    pub catalog: &'a mut ReviewCatalog,
    pub settings: &'a AppSettings,
    pub session: Option<&'a mut SessionState>,
    pub format: FormatContext<'a>,
}

/// What the sink's image callback reads from the presenter at the moment of the call (C# `MainViewModel.NotifyCurrentImageChanged` reads
/// `_presenter.CurrentPresentedPath`, `IsSameSourceSwap`, `CurrentOriginalWidth/Height`). Passed to `IPresentationSink::set_current_image` (B1 change, see the contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageChange<'a> {
    /// C# `CurrentPresentedPath`.
    pub presented_path: Option<&'a str>,
    /// C# `CurrentOriginalWidth`: 0 when no image.
    pub original_width: i32,
    /// C# `CurrentOriginalHeight`: 0 when no image.
    pub original_height: i32,
    /// C# `IsSameSourceSwap`: true only during a zoom-detail swap of the SAME source.
    pub is_same_source_swap: bool,
}

/// Construction arguments of [`ImagePresenter::new`] (the C# constructor parameters minus the catalog, the settings and the session getters, which are per-call, and minus the compare view model).
pub struct ImagePresenterDeps {
    pub clock: Arc<GenerationClock>,
    pub preview_service: Arc<PreviewImageService>,
    pub thumbnail_cache: Arc<ThumbnailCache>,
    pub preload: Arc<dyn IPreloadController>,
    /// C# `FileHashService` (the compare hash source); `Arc<dyn FileHasher>` so tests block or cancel a hash.
    pub hasher: Arc<dyn FileHasher>,
    pub metrics: Arc<ReviewMetrics>,
    pub session_store: Option<Arc<SessionStore>>,
    pub session_writer: Option<Arc<SessionWriter>>,
    pub sink: Box<dyn IPresentationSink>,
    /// C# `IFileSystem? fileSystem`; `None` = `PhysicalFileSystem` (C# `new FileInfo(path)`).
    pub file_system: Option<Arc<dyn FileSystem>>,
    /// C# `Action<string>? onPresentedHook`.
    pub on_presented_hook: Option<Box<dyn FnMut(&str)>>,
    /// C# `IUiScheduler? uiScheduler`: the Rust form of `YieldAsync` is `post`. `None` = no yields (synchronous steps, tests).
    pub ui_scheduler: Option<Arc<dyn UiScheduler>>,
    /// C# `busyRetryDelay`; `None` = `default_busy_retry_delay()`.
    pub busy_retry_delay: Option<BusyRetryDelay>,
    /// Runs the worker waits (preview, thumbnail, dimensions, compare, hashes, indicator); `None` = `ThreadRunner`.
    pub runner: Option<Arc<dyn BackgroundRunner>>,
    /// C# `StatWorker` test seam; `None` = `NavigationStatWorker::shared().clone()`.
    pub stat_worker: Option<NavigationStatWorker>,
    /// Called on any thread after a completion was queued; the shell posts `pump` to the UI thread from it.
    pub wake: WakeFn,
    /// C# `ZoomDetail.IndicatorDelay` test seam; `None` = the 300 ms default.
    pub indicator_delay: Option<IndicatorDelay>,
}

impl fmt::Debug for ImagePresenterDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImagePresenterDeps").finish_non_exhaustive()
    }
}

// ---- Internal protocol (visible to the other files of this module) -----------------------------------------------------------

/// The settings read ONCE per present (C# `var settings = _getSettings()` after the initial stat, line 369) and reused by every later step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PresentSettings {
    pub(super) loading_mode: LoadingMode,
    pub(super) compare_size_enabled: bool,
    pub(super) compare_hash_enabled: bool,
}

/// C# `initialEntry.Length` / `LastWriteUtc` after the R7-1 refresh (the entry object itself is not kept).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EntryMetadata {
    pub(super) length: Option<i64>,
    pub(super) last_write_utc: Option<pr_core::time::DateTimeUtc>,
}

/// The thumbnail-versus-preview race of step 4 (C# `Task.WhenAny`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThumbnailRace {
    /// No thumbnail was requested (RAM hit, preview already in flight, or not Preview mode).
    NotStarted,
    /// A thumbnail is being loaded; its event is accepted.
    Pending,
    /// The thumbnail event was handled (shown or nothing to show); later thumbnail events are dropped.
    Shown,
    /// The preview won first; a later thumbnail event is dropped.
    Abandoned,
}

/// Which wait of the present is open (the `await` the C# method is suspended at).
#[derive(Debug)]
pub(super) enum Phase {
    /// Line 296: the initial stat.
    AwaitInitialStat,
    /// Lines 411 / 467: the viewer preview (and maybe the thumbnail).
    AwaitPreview { race: ThumbnailRace },
    /// Line 582: the post-frame yield.
    AwaitFrameYield,
    /// Line 591: the original-dimension lookup.
    AwaitDimensions,
    /// Line 598: the refresh stat.
    AwaitRefreshStat,
    /// `CompareViewModel.LoadAsync` awaits.
    AwaitCompare(CompareProgress),
    /// Line 659: the stat that re-checks a stale FileNotFound.
    AwaitRecheckStat,
}

/// The locals of one `PresentCoreAsync` call that live across an `await`. No `Debug`: `PreviewImage` is a trait object.
pub(super) struct Navigation {
    /// `_clock.NextNavigation()` of this present.
    pub(super) token: i64,
    /// The request; `index` is rebased by AR16 (line 478).
    pub(super) request: PresentRequest,
    /// `pathOverride ?? catalog.PathAt(index)`.
    pub(super) path: String,
    /// C# `previousNavigationPath`.
    pub(super) previous_navigation_path: Option<String>,
    /// C# `staleNotFoundRetries` (at most 3).
    pub(super) stale_not_found_retries: u8,
    /// C# `viewerDecodeToken`: a CLONE taken at the start (INV-1 / APP-P01: never re-read from the shared field).
    pub(super) viewer_cancel: CancelToken,
    /// C# `presentStopwatch`.
    pub(super) started: Instant,
    /// C# `perf`.
    pub(super) perf: bool,
    pub(super) phase: Phase,
    /// C# `initialSize` (`initialStat.Length`).
    pub(super) initial_size: i64,
    pub(super) initial_entry: Option<EntryMetadata>,
    pub(super) current_key: Option<ImageCacheKey>,
    pub(super) settings: Option<PresentSettings>,
    pub(super) ram_ready: bool,
    /// The stat of the initial `Found` outcome (the thumbnail key reuses it).
    pub(super) initial_stat: Option<pr_core::abstractions::FileStat>,
    /// The decoded preview shown by step 5.
    pub(super) image: Option<PreviewImage>,
    /// The original dimensions for the status line (`None` = unknown).
    pub(super) original_dimensions: Option<(i32, i32)>,
}

/// Which continuation a stat event resumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StatPurpose {
    Initial,
    Refresh,
    Recheck,
    NextForMissing,
}

/// Why the next frame callback was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum YieldPurpose {
    /// The deferred preload kick of `DeferNextPreloadKick` for `index`.
    DeferredKick { index: i32 },
    /// The post-frame yield before the status bookkeeping (line 582).
    PostFrame,
}

/// Which partner of a compare pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CompareSide {
    Left,
    Right,
}

/// Worker-to-UI completions of the presenter. Each carries the token of the present (or missing run) that requested it.
pub(super) enum PresenterEvent {
    Stat {
        token: i64,
        purpose: StatPurpose,
        result: StatResult,
    },
    Thumbnail {
        token: i64,
        result: ThumbnailResult,
    },
    Preview {
        token: i64,
        result: PreviewResult,
    },
    FrameYielded {
        token: i64,
        purpose: YieldPurpose,
    },
    Dimensions {
        token: i64,
        result: Result<Option<(i32, i32)>, PreviewError>,
    },
    ComparePreview {
        token: i64,
        side: CompareSide,
        result: Result<PlatformImage, PreviewError>,
    },
    CompareSizes {
        token: i64,
        left: Option<i64>,
        right: Option<i64>,
    },
    CompareHash {
        token: i64,
        side: CompareSide,
        result: Result<String, FileHashError>,
    },
}

// ---- The presenter -------------------------------------------------------------------------------------------------------

/// C# `ImagePresenter`: coordinates the whole presentation of an image (`PresentAsync`, `RemoveMissingCatalogItemAsync`), the placeholder
/// thumbnail, vanished images, preload kicks, the compare integration and the session save; keeps INV-1 (a stale frame never overwrites
/// a newer one).
pub struct ImagePresenter {
    pub(super) clock: Arc<GenerationClock>,
    pub(super) preview_service: Arc<PreviewImageService>,
    pub(super) thumbnail_cache: Arc<ThumbnailCache>,
    pub(super) preload: Arc<dyn IPreloadController>,
    pub(super) compare: CompareViewModel,
    pub(super) hasher: Arc<dyn FileHasher>,
    pub(super) metrics: Arc<ReviewMetrics>,
    pub(super) session_store: Option<Arc<SessionStore>>,
    pub(super) session_writer: Option<Arc<SessionWriter>>,
    pub(super) sink: Box<dyn IPresentationSink>,
    pub(super) file_system: Arc<dyn FileSystem>,
    pub(super) on_presented_hook: Option<Box<dyn FnMut(&str)>>,
    pub(super) original_shown_hook: Option<Box<dyn FnMut(&str)>>,
    pub(super) ui_scheduler: Option<Arc<dyn UiScheduler>>,
    pub(super) busy_retry_delay: BusyRetryDelay,
    pub(super) runner: Arc<dyn BackgroundRunner>,
    pub(super) stat_worker: NavigationStatWorker,
    pub(super) inbox: CompletionInbox<PresenterEvent>,
    pub(super) zoom_detail: ZoomDetailLoader,
    pub(super) compare_cache: ComparePairCache,
    /// C# `_viewerDecodeCts`: the token of the present in progress; replaced (and the old one cancelled) by every new present.
    pub(super) viewer_decode_cancel: Option<CancelToken>,
    /// The present in progress (its locals), `None` when idle or while a handler owns it.
    pub(super) navigation: Option<Navigation>,
    /// The `RemoveMissingCatalogItemAsync` run in progress.
    pub(super) missing: Option<MissingRun>,
    /// C# `_presentedFilePath`: the file anchoring the displayed bitmap (crossfade decision).
    pub(super) presented_file_path: Option<String>,
    /// C# `_currentNavigationPath` (= `CurrentPresentedPath`).
    pub(super) current_navigation_path: Option<String>,
    /// C# `_infoWhileOriginalShown`.
    pub(super) info_while_original_shown: Option<PhotoInfo>,
    pub(super) current_image: Option<PlatformImage>,
    pub(super) current_original_width: i32,
    pub(super) current_original_height: i32,
    pub(super) is_same_source_swap: bool,
    pub(super) status_text: String,
    pub(super) status_needs_attention: bool,
    pub(super) last_present_started_from_ram: bool,
    pub(super) defer_next_preload_kick: bool,
    pub(super) current_photo_info: Option<PhotoInfo>,
    /// Changes accumulated by the current call; taken by the public entry points.
    pub(super) changes: PresenterChanges,
}

impl fmt::Debug for ImagePresenter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImagePresenter")
            .field("current_navigation_path", &self.current_navigation_path)
            .field("status_text", &self.status_text)
            .field("busy", &(self.navigation.is_some() || self.missing.is_some()))
            .finish_non_exhaustive()
    }
}

impl ImagePresenter {
    /// C# constructor. Stores the dependencies, builds the `ZoomDetailLoader` and the inbox, starts nothing, reads no file, decodes nothing.
    /// The C# `ArgumentNullException` checks have no Rust form (non-null by type).
    pub fn new(deps: ImagePresenterDeps, compare: CompareViewModel) -> Self {
        todo!()
    }

    // ---- C# properties ----

    /// C# `Compare`.
    pub fn compare(&self) -> &CompareViewModel {
        todo!()
    }

    /// Mutable access for the main view model (C# shares the instance: `Select`, `Toggle`, `SelectLeft`, ...).
    pub fn compare_mut(&mut self) -> &mut CompareViewModel {
        todo!()
    }

    /// C# `Clock`.
    pub fn clock(&self) -> &Arc<GenerationClock> {
        todo!()
    }

    /// C# `Sink` (the shell hooks the concrete sink through it).
    pub fn sink_mut(&mut self) -> &mut dyn IPresentationSink {
        todo!()
    }

    /// C# `CurrentImage`.
    pub fn current_image(&self) -> Option<&PlatformImage> {
        todo!()
    }

    /// C# `CurrentPresentedPath` (= `_currentNavigationPath`).
    pub fn current_presented_path(&self) -> Option<&str> {
        todo!()
    }

    /// C# `CurrentOriginalWidth`.
    pub fn current_original_width(&self) -> i32 {
        todo!()
    }

    /// C# `CurrentOriginalHeight`.
    pub fn current_original_height(&self) -> i32 {
        todo!()
    }

    /// C# `IsSameSourceSwap`.
    pub fn is_same_source_swap(&self) -> bool {
        todo!()
    }

    /// C# `StatusText`.
    pub fn status_text(&self) -> &str {
        todo!()
    }

    /// C# `StatusNeedsAttention` (Q-R34).
    pub fn status_needs_attention(&self) -> bool {
        todo!()
    }

    /// C# `LastPresentStartedFromRam`.
    pub fn last_present_started_from_ram(&self) -> bool {
        todo!()
    }

    /// C# `IsCompareVisible` (`compare.is_visible()`).
    pub fn is_compare_visible(&self) -> bool {
        todo!()
    }

    /// C# `CurrentPhotoInfo`.
    pub fn current_photo_info(&self) -> Option<&PhotoInfo> {
        todo!()
    }

    /// C# `DeferNextPreloadKick { get; }`.
    pub fn defer_next_preload_kick(&self) -> bool {
        todo!()
    }

    /// C# `DeferNextPreloadKick { set; }` (P-1 startup: the launch image's frame first, then preloading).
    pub fn set_defer_next_preload_kick(&mut self, value: bool) {
        todo!()
    }

    /// C# `ZoomDetail` (read-only view: `held_original`, `is_showing_original`, `is_raw_decode_indicator_visible`, `pending_load`). Mutations go through the presenter
    /// (`set_viewer_zoom`, `refresh_zoom_detail`, `reset_zoom_detail`) so their effects are applied.
    pub fn zoom_detail(&self) -> &ZoomDetailLoader {
        todo!()
    }

    /// Replaces the `OriginalShown` measure hook (C# `ZoomDetail.OriginalShown += ..`).
    pub fn set_original_shown_hook(&mut self, hook: Option<Box<dyn FnMut(&str)>>) {
        todo!()
    }

    // ---- commands ----

    /// C# `ClearPresentation()`: `current_navigation_path = None`, `current_photo_info = None`, `info_while_original_shown = None`, `zoom_detail.reset()` (apply effects),
    /// `update_current_image(None, 0, 0, None)`, `compare.clear(ctx)`. Does not touch a present in flight (its token is what guards it).
    pub fn clear_presentation(&mut self, env: &mut PresenterEnv<'_>) -> PresenterChanges {
        todo!()
    }

    /// C# `EvictCachedPath(path)`: `preview_service.evict_cached_path(path, Some(&|normalized| preload.remove_preloaded_keys_for_path(normalized)))`. Per path: never clears the
    /// whole cache. The error of the service (invalid / too long path) is returned.
    pub fn evict_cached_path(&self, path: &str) -> Result<(), PreviewError> {
        todo!()
    }

    /// C# `ResetFileIdentity()`: `presented_file_path = None` (called once when a folder starts loading, so its first image never fades).
    pub fn reset_file_identity(&mut self) {
        todo!()
    }

    // ---- the pump ----

    /// Applies every completion that arrived since the last call, on the UI thread: first the zoom-detail loader's (decode done, indicator delay elapsed), then the
    /// presenter's, in arrival order, repeating until both inboxes are empty (a handler may itself queue an event, e.g. through an inline UI scheduler). This is the only
    /// place where the continuations of `PresentCoreAsync` / `RemoveMissingCatalogItemAsync` run. Returns the accumulated changes.
    pub fn pump(&mut self, env: &mut PresenterEnv<'_>) -> PresenterChanges {
        todo!()
    }

    /// True when no present, no missing-run and no worker completion is outstanding (nothing left for `pump` to resume).
    pub fn is_idle(&self) -> bool {
        todo!()
    }

    /// Hang guard for tests: blocks until a worker completion is queued (presenter or zoom loader) or `timeout` elapsed. Never called by production code.
    pub fn wait_for_event(&self, timeout: std::time::Duration) -> bool {
        todo!()
    }

    // ---- private helpers shared with the other files ----

    /// Routes one event to its continuation (the table of the contract).
    pub(super) fn dispatch(&mut self, env: &mut PresenterEnv<'_>, event: PresenterEvent) {
        todo!()
    }

    /// Returns and clears the accumulated changes.
    pub(super) fn take_changes(&mut self) -> PresenterChanges {
        todo!()
    }

    /// C# `UpdateCurrentImage(image, originalWidth, originalHeight, path)` (lines 800 to 818). Order: dimensions (0 when `image` is `None`), `current_image`, then
    /// `is_file_change = ImageTransitionDecision::should_transition(presented_file_path, image.and(path), compare.is_visible())`, then `presented_file_path = None` when
    /// `image` is `None` else `path` when given (a same-source swap passes no path and keeps the identity, R10), then `sink.set_current_image(image, is_file_change, &ImageChange { .. })`,
    /// then the zoom sync (`sink.take_viewer_zoom_change()` forwarded to the zoom loader). Marks `CURRENT_IMAGE`.
    pub(super) fn update_current_image(
        &mut self,
        image: Option<PlatformImage>,
        original_width: i32,
        original_height: i32,
        path: Option<&str>,
    ) {
        todo!()
    }

    /// C# `UpdateStatus(status, needsAttention)`: stores both, then `sink.set_status_text(status)`. Marks `STATUS`.
    pub(super) fn update_status(&mut self, status: String, needs_attention: bool) {
        todo!()
    }
}
