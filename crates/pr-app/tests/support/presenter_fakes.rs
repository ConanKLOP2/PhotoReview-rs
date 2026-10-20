//! Shared fakes of the `ImagePresenter` tests (W8-07h..07k). Included from each test crate with
//! `mod support { pub mod preview_fakes; pub mod presenter_fakes; }`.
//!
//! [`RecordingSink`] logs every sink call into a shared [`SinkLog`] the test keeps; [`RecordingPreload`] records the preload
//! controller calls; [`FakeHasher`] answers a hash per file name; [`GatedScheduler`] holds posted UI actions until the test
//! releases them by index; [`PresenterFixture`] wires them to a real `PreviewImageService` over fake decoders, a real
//! `ThumbnailCache` with an injected embedded-thumbnail reader and a `NavigationStatWorker` of its own. Every wait is bounded
//! by [`TIMEOUT`]; no fixed sleeps.

// Each test binary uses a different subset of these helpers.
#![allow(dead_code)]

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pr_app::coordinators::presenter::compare_pair::BusyRetryDelay;
use pr_app::coordinators::presenter::state::{
    ImageChange, ImagePresenter, ImagePresenterDeps, PathHook, PresenterChanges, PresenterEnv,
};
use pr_app::coordinators::traits::{IPreloadController, IPresentationSink};
use pr_app::coordinators::zoom_detail::{BackgroundJob, BackgroundRunner, IndicatorDelay, ThreadRunner};
use pr_app::misc::file_hash::{FileHashError, FileHasher};
use pr_app::viewmodels::compare::CompareViewModel;
use pr_app::viewmodels::format::context::{DateText, FormatContext};
use pr_core::abstractions::{FileSystem, UiAction, UiActionPanicked, UiCompletion, UiScheduler};
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::io::navigation_stat::NavigationStatWorker;
use pr_core::model::enums::RawPairMode;
use pr_core::session::store::{SessionState, SessionStore};
use pr_core::session::writer::SessionWriter;
use pr_core::settings::app_settings::AppSettings;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache::thumbnails::{
    CacheImageInfo, CacheImageReader, EmbeddedThumbnailReaderFn, ThumbnailCache, ThumbnailCacheOptions,
    ThumbnailCacheServices,
};
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::decode_types::{
    DecodeBox, DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PlatformImage,
};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::embedded_thumbnail::{EmbeddedThumbnailBackend, EmbeddedThumbnailFrame, ThumbnailBackendError};
use pr_imaging::pixels::PixelBuffer;
use pr_imaging::pixels::PixelLayout;
use pr_imaging::preload::scheduler::RunHandle;
use pr_imaging::service::PreviewImageService;
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::NumberCulture;
use pr_test_support::TempRoot;

use super::preview_fakes::{PixelsCodec, ServiceOptions, TIMEOUT, build_service_with_metrics, lock};

// ---- the sink -------------------------------------------------------------------------------------------------------

/// One `set_current_image` call as the sink saw it.
#[derive(Debug, Clone)]
pub struct ImageCall {
    pub image: Option<PlatformImage>,
    pub is_file_change: bool,
    pub presented_path: Option<String>,
    pub original_width: i32,
    pub original_height: i32,
    pub is_same_source_swap: bool,
}

/// Everything the sink was told, in call order per kind.
#[derive(Debug, Default)]
pub struct SinkLog {
    pub images: Vec<ImageCall>,
    pub statuses: Vec<String>,
    pub presented_paths: Vec<String>,
    pub traced_kinds: Vec<String>,
    pub initial_view_mode_count: usize,
    /// What `take_viewer_zoom_change` returns next (the view model's re-entrant `ZoomModeChanged`), cleared when taken.
    pub zoom_change: Option<Option<f64>>,
    /// Set once on `apply_initial_view_mode`: the zoom to report right after it (C# `OnApplyInitialViewMode`).
    pub zoom_on_apply_initial_view_mode: Option<Option<f64>>,
}

impl SinkLog {
    /// The `is_file_change` flag of every image call, in order (C# `FileChangeFlags`).
    pub fn file_change_flags(&self) -> Vec<bool> {
        self.images.iter().map(|call| call.is_file_change).collect()
    }

    /// Total number of sink calls of any kind.
    pub fn call_count(&self) -> usize {
        self.images.len()
            + self.statuses.len()
            + self.presented_paths.len()
            + self.traced_kinds.len()
            + self.initial_view_mode_count
    }
}

/// An `IPresentationSink` that writes into a [`SinkLog`] shared with the test.
#[derive(Debug, Clone, Default)]
pub struct RecordingSink {
    log: Arc<Mutex<SinkLog>>,
}

impl RecordingSink {
    pub fn new() -> (Self, Arc<Mutex<SinkLog>>) {
        let sink = Self::default();
        let log = Arc::clone(&sink.log);
        (sink, log)
    }
}

impl IPresentationSink for RecordingSink {
    fn set_current_image(&mut self, image: Option<PlatformImage>, is_file_change: bool, change: &ImageChange<'_>) {
        lock(&self.log).images.push(ImageCall {
            image,
            is_file_change,
            presented_path: change.presented_path.map(str::to_owned),
            original_width: change.original_width,
            original_height: change.original_height,
            is_same_source_swap: change.is_same_source_swap,
        });
    }

    fn set_status_text(&mut self, status: &str) {
        lock(&self.log).statuses.push(status.to_owned());
    }

    fn apply_initial_view_mode(&mut self) {
        let mut log = lock(&self.log);
        log.initial_view_mode_count += 1;
        if let Some(zoom) = log.zoom_on_apply_initial_view_mode {
            log.zoom_change = Some(zoom);
        }
    }

    fn on_presented(&mut self, path: &str) {
        lock(&self.log).presented_paths.push(path.to_owned());
    }

    fn trace_presented(&mut self, _token: i64, kind: &str, _assigned_timestamp: i64) {
        lock(&self.log).traced_kinds.push(kind.to_owned());
    }

    fn take_viewer_zoom_change(&mut self) -> Option<Option<f64>> {
        lock(&self.log).zoom_change.take()
    }
}

// ---- the preload controller -----------------------------------------------------------------------------------------

/// What the preload controller was asked.
#[derive(Debug, Default)]
pub struct PreloadLog {
    pub notified: Vec<i32>,
    pub preload_around: Vec<i32>,
    pub removed_for_path: Vec<String>,
    pub cancels: usize,
    pub clears: usize,
    /// Keys `try_consume_preloaded_key` answers `true` for (once each).
    pub warmed: Vec<ImageCacheKey>,
    pub consumed: Vec<ImageCacheKey>,
    /// What `get_viewer_decode_delay` answers (C# `TestPreloadController.ViewerDecodeDelay`).
    pub viewer_decode_delay: Duration,
}

/// Records every call; `warm(key)` makes the next `try_consume_preloaded_key(key)` a hit.
#[derive(Debug, Default)]
pub struct RecordingPreload {
    log: Mutex<PreloadLog>,
}

impl RecordingPreload {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn warm(&self, key: ImageCacheKey) {
        lock(&self.log).warmed.push(key);
    }

    pub fn notified(&self) -> Vec<i32> {
        lock(&self.log).notified.clone()
    }

    /// Sets the delay `get_viewer_decode_delay` answers from now on (a key-held burst in the C# tests).
    pub fn set_viewer_decode_delay(&self, delay: Duration) {
        lock(&self.log).viewer_decode_delay = delay;
    }

    pub fn preload_around_calls(&self) -> Vec<i32> {
        lock(&self.log).preload_around.clone()
    }

    pub fn removed_for_path(&self) -> Vec<String> {
        lock(&self.log).removed_for_path.clone()
    }

    pub fn total_calls(&self) -> usize {
        let log = lock(&self.log);
        log.notified.len() + log.preload_around.len() + log.removed_for_path.len() + log.cancels + log.clears
    }
}

impl IPreloadController for RecordingPreload {
    fn preload_around(&self, center: i32) -> RunHandle {
        lock(&self.log).preload_around.push(center);
        RunHandle::completed()
    }

    fn try_consume_preloaded_key(&self, key: &ImageCacheKey) -> bool {
        let mut log = lock(&self.log);
        match log.warmed.iter().position(|warmed| warmed == key) {
            Some(position) => {
                log.warmed.remove(position);
                log.consumed.push(key.clone());
                true
            }
            None => false,
        }
    }

    fn cancel(&self) {
        lock(&self.log).cancels += 1;
    }

    fn remove_preloaded_keys_for_path(&self, normalized_path: &str) {
        lock(&self.log).removed_for_path.push(normalized_path.to_owned());
    }

    fn clear_preloaded_keys(&self) {
        lock(&self.log).clears += 1;
    }

    fn notify_navigation(&self, index: i32) {
        lock(&self.log).notified.push(index);
    }

    fn get_viewer_decode_delay(&self) -> Duration {
        lock(&self.log).viewer_decode_delay
    }
}

// ---- hasher ---------------------------------------------------------------------------------------------------------

/// Answers a fixed hash per file NAME (`default_hash` for the others) and counts the calls.
#[derive(Debug)]
pub struct FakeHasher {
    default_hash: String,
    by_name: Mutex<Vec<(String, String)>>,
    calls: AtomicUsize,
}

impl FakeHasher {
    pub fn new(default_hash: &str) -> Arc<Self> {
        Arc::new(Self {
            default_hash: default_hash.to_owned(),
            by_name: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
        })
    }

    pub fn set(&self, file_name: &str, hash: &str) {
        lock(&self.by_name).push((file_name.to_owned(), hash.to_owned()));
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl FileHasher for FakeHasher {
    fn get(&self, path: &Path, cancel: &CancelToken) -> Result<String, FileHashError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if cancel.is_canceled() {
            return Err(FileHashError::Canceled);
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let hash = lock(&self.by_name)
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, hash)| hash.clone())
            .unwrap_or_else(|| self.default_hash.clone());
        Ok(hash)
    }
}

// ---- UI scheduler ---------------------------------------------------------------------------------------------------

/// Holds every posted action until the test releases it by its post index (0 = the first post).
#[derive(Default)]
pub struct GatedScheduler {
    posted: Mutex<Vec<Option<UiAction>>>,
}

impl std::fmt::Debug for GatedScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatedScheduler")
            .field("posted", &self.posted_count())
            .finish()
    }
}

impl GatedScheduler {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// How many actions were posted so far.
    pub fn posted_count(&self) -> usize {
        lock(&self.posted).len()
    }

    /// Runs the action posted as number `index` (once). Returns false when there is none or it already ran.
    pub fn release(&self, index: usize) -> bool {
        let action = lock(&self.posted).get_mut(index).and_then(Option::take);
        match action {
            Some(action) => {
                action();
                true
            }
            None => false,
        }
    }

    /// Releases every action not run yet, in post order; returns how many ran.
    pub fn release_all(&self) -> usize {
        let mut ran = 0;
        let mut index = 0;
        while index < self.posted_count() {
            if self.release(index) {
                ran += 1;
            }
            index += 1;
        }
        ran
    }
}

impl UiScheduler for GatedScheduler {
    fn post(&self, action: UiAction) {
        lock(&self.posted).push(Some(action));
    }

    fn invoke(&self, action: UiAction) -> UiCompletion {
        let (sender, completion) = UiCompletion::channel();
        let wrapped: UiAction = Box::new(move || {
            action();
            let _ = sender.send(Ok::<(), UiActionPanicked>(()));
        });
        lock(&self.posted).push(Some(wrapped));
        completion
    }
}

// ---- formatting -----------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct Dates;

impl DateText for Dates {
    fn general(&self, _: DateTimeUtc) -> Option<String> {
        None
    }

    fn invariant_general(&self, _: DateTimeUtc) -> String {
        String::new()
    }
}

/// Owns the English texts so a [`FormatContext`] can be lent for one call (`PresenterEnv::format`).
pub struct FormatFixture {
    texts: Localizer,
    numbers: NumberCulture,
    dates: Dates,
}

impl std::fmt::Debug for FormatFixture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormatFixture").finish_non_exhaustive()
    }
}

impl FormatFixture {
    pub fn new() -> Self {
        let mut warnings = Vec::new();
        let catalog = LanguageCatalog::try_parse(
            include_str!("../../../../assets/languages/en.json"),
            "en.json",
            &mut warnings,
        )
        .unwrap_or_else(|| panic!("en.json did not parse: {warnings:?}"));
        Self {
            texts: Localizer::create(&catalog, &[], &warnings),
            numbers: NumberCulture::invariant(),
            dates: Dates,
        }
    }

    pub fn ctx(&self) -> FormatContext<'_> {
        FormatContext {
            texts: &self.texts,
            numbers: &self.numbers,
            group_separator: ",",
            dates: &self.dates,
        }
    }
}

impl Default for FormatFixture {
    fn default() -> Self {
        Self::new()
    }
}

/// The per-call environment (C# had long-lived references): no session.
pub fn env<'a>(
    catalog: &'a mut ReviewCatalog,
    settings: &'a AppSettings,
    format: &'a FormatFixture,
) -> PresenterEnv<'a> {
    PresenterEnv {
        catalog,
        settings,
        session: None,
        format: format.ctx(),
    }
}

// ---- the thumbnail cache --------------------------------------------------------------------------------------------

struct NoThumbnailBackend;

impl EmbeddedThumbnailBackend for NoThumbnailBackend {
    fn open(
        &self,
        _stream: Box<dyn pr_core::abstractions::ReadSeek>,
    ) -> Result<Option<Box<dyn EmbeddedThumbnailFrame>>, ThumbnailBackendError> {
        Ok(None)
    }
}

/// Never reads a disk entry (the thumbnails of these tests come from the injected embedded reader).
struct NoDiskReader;

impl CacheImageReader for NoDiskReader {
    fn decode(
        &self,
        _encoded: &[u8],
        _layout: Option<PixelLayout>,
        _decode_box: DecodeBox,
        _expected_width: i32,
        _expected_height: i32,
    ) -> Result<(PixelBuffer, CacheImageInfo), DecodeError> {
        Err(DecodeError::InvalidData {
            message: "no disk thumbnails in this fake".to_owned(),
        })
    }
}

/// A thumbnail cache whose embedded-thumbnail reader finds nothing (`Ok(None)`: the present shows no placeholder).
pub fn build_thumbnail_cache(dir: &Path) -> Arc<ThumbnailCache> {
    build_thumbnail_cache_with(dir, Arc::new(|_path: &str, _cancel: &CancelToken| Ok(None)))
}

/// A thumbnail cache whose embedded-thumbnail reader is `reader` (a gated, faulting or fast one).
pub fn build_thumbnail_cache_with(dir: &Path, reader: EmbeddedThumbnailReaderFn) -> Arc<ThumbnailCache> {
    let services = ThumbnailCacheServices {
        platform_codec: Arc::new(PixelsCodec),
        cache_image_reader: Arc::new(NoDiskReader),
        embedded_backend: Arc::new(NoThumbnailBackend),
        fs: Arc::new(pr_core::io::physical::PhysicalFileSystem),
    };
    let mut options = ThumbnailCacheOptions::new(dir);
    options.max_ram_bytes = 1024 * 1024;
    options.embedded_reader = Some(reader);
    match ThumbnailCache::new(services, options) {
        Ok(cache) => Arc::new(cache),
        Err(error) => panic!("the thumbnail cache did not build: {error:?}"),
    }
}

// ---- background runner ----------------------------------------------------------------------------------------------

/// Counts the jobs handed to the real thread runner (a presenter that starts nothing counts zero).
#[derive(Debug, Default)]
pub struct CountingRunner {
    jobs: AtomicUsize,
}

impl CountingRunner {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn jobs(&self) -> usize {
        self.jobs.load(Ordering::SeqCst)
    }
}

impl BackgroundRunner for CountingRunner {
    fn run(&self, name: &'static str, job: BackgroundJob) {
        self.jobs.fetch_add(1, Ordering::SeqCst);
        ThreadRunner.run(name, job);
    }
}

// ---- the fixture ----------------------------------------------------------------------------------------------------

/// One presenter over fakes, with the catalog, settings and format texts it borrows per call.
pub struct PresenterFixture {
    pub root: TempRoot,
    pub catalog: ReviewCatalog,
    pub settings: AppSettings,
    pub format: FormatFixture,
    pub clock: Arc<GenerationClock>,
    pub service: Arc<PreviewImageService>,
    /// Shared by the service and the presenter (C# passes one `ReviewMetrics` to both).
    pub metrics: Arc<ReviewMetrics>,
    pub preload: Arc<RecordingPreload>,
    pub sink_log: Arc<Mutex<SinkLog>>,
    pub presenter: ImagePresenter,
    pub wakes: Arc<AtomicUsize>,
    /// The live session lent to every call (C# `getSession`); `None` = no session.
    pub session: Option<SessionState>,
}

impl std::fmt::Debug for PresenterFixture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresenterFixture").finish_non_exhaustive()
    }
}

/// Optional parts of a [`PresenterFixture`].
#[derive(Default)]
pub struct FixtureOptions {
    pub service: ServiceOptions,
    pub scheduler: Option<Arc<dyn UiScheduler>>,
    pub hasher: Option<Arc<dyn FileHasher>>,
    pub indicator_delay: Option<IndicatorDelay>,
    pub runner: Option<Arc<dyn BackgroundRunner>>,
    pub on_presented_hook: Option<PathHook>,
    /// The presenter's stat file system (C# `fileSystem`); `None` = the physical one.
    pub file_system: Option<Arc<dyn FileSystem>>,
    /// A thumbnail cache of the test's own (default: one whose embedded reader finds nothing).
    pub thumbnails: Option<Arc<ThumbnailCache>>,
    pub session_store: Option<Arc<SessionStore>>,
    pub session_writer: Option<Arc<SessionWriter>>,
    /// C# `busyRetryDelay`; default = instant (returns `true`).
    pub busy_retry_delay: Option<BusyRetryDelay>,
}

impl PresenterFixture {
    /// A presenter over `decoder` with a catalog of the files `names` (each written as a tiny file in the temp root).
    pub fn new(decoder: Arc<dyn ImageDecoder>, names: &[&str]) -> Self {
        Self::with_options(decoder, names, FixtureOptions::default())
    }

    pub fn with_options(decoder: Arc<dyn ImageDecoder>, names: &[&str], options: FixtureOptions) -> Self {
        let root = TempRoot::new("presenter");
        let metrics = Arc::new(ReviewMetrics::new());
        let service = build_service_with_metrics(&root.join("cache"), decoder, options.service, Arc::clone(&metrics));
        let thumbnails = options
            .thumbnails
            .unwrap_or_else(|| build_thumbnail_cache(&root.join("thumbs")));
        let clock = Arc::new(GenerationClock::default());
        let preload = RecordingPreload::new();
        let (sink, sink_log) = RecordingSink::new();
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        let format = FormatFixture::new();
        let mut catalog = ReviewCatalog::new();
        let entries: Vec<CatalogEntry> = names
            .iter()
            .map(|name| {
                let path = root.join(name);
                std::fs::write(&path, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap_or_else(|error| panic!("write {name}: {error}"));
                CatalogEntry::new(path.to_string_lossy().into_owned())
                    .unwrap_or_else(|error| panic!("entry {name}: {error:?}"))
            })
            .collect();
        catalog.reset(entries, RawPairMode::Separate, None);
        let hasher: Arc<dyn FileHasher> = options.hasher.unwrap_or_else(|| FakeHasher::new("HASH"));
        let deps = ImagePresenterDeps {
            clock: Arc::clone(&clock),
            preview_service: Arc::clone(&service),
            thumbnail_cache: thumbnails,
            preload: Arc::clone(&preload) as Arc<dyn IPreloadController>,
            hasher,
            metrics: Arc::clone(&metrics),
            session_store: options.session_store,
            session_writer: options.session_writer,
            sink: Box::new(sink),
            file_system: options.file_system,
            on_presented_hook: options.on_presented_hook,
            ui_scheduler: options.scheduler,
            busy_retry_delay: Some(
                options
                    .busy_retry_delay
                    .unwrap_or_else(|| Arc::new(|_delay: Duration, _cancel: &CancelToken| true)),
            ),
            runner: options.runner,
            stat_worker: Some(NavigationStatWorker::new()),
            wake: Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
            indicator_delay: options.indicator_delay,
        };
        let presenter = ImagePresenter::new(deps, CompareViewModel::new(&format.ctx()));
        Self {
            root,
            catalog,
            settings: AppSettings::default(),
            format,
            clock,
            service,
            metrics,
            preload,
            sink_log,
            presenter,
            wakes,
            session: None,
        }
    }

    /// The path of the catalog file `name`.
    pub fn path(&self, name: &str) -> String {
        self.root.join(name).to_string_lossy().into_owned()
    }

    /// `pump` with the fixture's environment.
    pub fn pump(&mut self) -> PresenterChanges {
        let mut env = PresenterEnv {
            catalog: &mut self.catalog,
            settings: &self.settings,
            session: self.session.as_mut(),
            format: self.format.ctx(),
        };
        self.presenter.pump(&mut env)
    }

    /// `while !is_idle { wait_for_event; pump }`, bounded by [`TIMEOUT`]; returns the accumulated changes.
    pub fn pump_until_idle(&mut self) -> PresenterChanges {
        let deadline = Instant::now() + TIMEOUT;
        let mut changes = PresenterChanges::NONE;
        while !self.presenter.is_idle() {
            assert!(
                Instant::now() < deadline,
                "the presenter did not become idle in {TIMEOUT:?}"
            );
            self.presenter.wait_for_event(Duration::from_millis(50));
            changes |= self.pump();
        }
        changes
    }

    /// `pump` until `done` holds (the zoom loader's decode or any other completion), bounded by [`TIMEOUT`].
    pub fn pump_until(&mut self, mut done: impl FnMut(&PresenterFixture) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while !done(self) {
            assert!(
                Instant::now() < deadline,
                "the awaited state was not reached in {TIMEOUT:?}"
            );
            self.presenter.wait_for_event(Duration::from_millis(50));
            self.pump();
        }
    }
}

/// `pump_until_idle` for a bare presenter (tests that build their own environment).
pub fn pump_until_idle(
    presenter: &mut ImagePresenter,
    catalog: &mut ReviewCatalog,
    settings: &AppSettings,
    format: &FormatFixture,
) -> PresenterChanges {
    let deadline = Instant::now() + TIMEOUT;
    let mut changes = PresenterChanges::NONE;
    while !presenter.is_idle() {
        assert!(
            Instant::now() < deadline,
            "the presenter did not become idle in {TIMEOUT:?}"
        );
        presenter.wait_for_event(Duration::from_millis(50));
        changes |= presenter.pump(&mut env(catalog, settings, format));
    }
    changes
}

// ---- RAW-like fixtures ----------------------------------------------------------------------------------------------

/// The embedded-JPEG RAW fixture of `ZoomDetailTests` (lines 1007 to 1072): an image of `width x height` pixels whose
/// `IRawPreviewInfo` reports the embedded JPEG size (`0` = not a RAW preview).
#[derive(Debug)]
pub struct RawView {
    inner: Arc<DecodedImage>,
    embedded: (i32, i32),
}

impl DecodedImageView for RawView {
    fn pixel_width(&self) -> i32 {
        self.inner.pixel_width()
    }
    fn pixel_height(&self) -> i32 {
        self.inner.pixel_height()
    }
    fn downscaled(&self) -> bool {
        self.inner.downscaled()
    }
    fn orientation(&self) -> i32 {
        1
    }
    fn estimated_bytes(&self) -> i64 {
        self.inner.estimated_bytes()
    }
    fn platform_image(&self) -> &PlatformImage {
        self.inner.platform_image()
    }
    fn original_width(&self) -> i32 {
        self.inner.original_width()
    }
    fn original_height(&self) -> i32 {
        self.inner.original_height()
    }
    fn embedded_preview_size(&self) -> Option<(i32, i32)> {
        (self.embedded.0 > 0).then_some(self.embedded)
    }
}

pub fn raw_view(width: i32, height: i32, downscaled: bool, embedded: (i32, i32)) -> Arc<dyn DecodedImageView> {
    let inner = DecodedImage::new(
        PlatformImage::Opaque(Arc::new(0_u8)),
        width,
        height,
        i64::from(width) * i64::from(height) * 4,
        DecodedImageOptions {
            downscaled,
            original_width: 6000,
            original_height: 4000,
            ..DecodedImageOptions::default()
        },
    )
    .unwrap_or_else(|error| panic!("test image: {error}"));
    Arc::new(RawView {
        inner: Arc::new(inner),
        embedded,
    })
}

/// C# `FullForNamesDecoder(params string[] fullNames)`: a true full decode for the listed file names, the embedded JPEG otherwise.
#[derive(Debug)]
pub struct FullForNamesDecoder {
    pub full_names: Vec<String>,
}

impl ImageDecoder for FullForNamesDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let name = std::path::Path::new(&request.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(if self.full_names.iter().any(|full| full.eq_ignore_ascii_case(&name)) {
            raw_view(6000, 4000, false, (0, 0))
        } else {
            raw_view(6000, 4000, false, (2000, 1333))
        })
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(6000, 4000))
    }
}

/// A RAW's regular decode with the embedded-JPEG size in its `IRawPreviewInfo` (C# `EmbeddedRawDecoder`).
#[derive(Debug, Default)]
pub struct EmbeddedInfoRawDecoder;

impl ImageDecoder for EmbeddedInfoRawDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Ok(raw_view(2000, 1333, true, (2000, 1333)))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(6000, 4000))
    }
}

/// An ordinary image whose bounded decode is a plain downscaled preview and whose full decode is a RAW-preview-like image (embedded size set):
/// the held original then still is "the embedded JPEG" for the presenter.
#[derive(Debug, Default)]
pub struct EmbeddedOnFullDecoder;

impl ImageDecoder for EmbeddedOnFullDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Ok(if request.decode_box().is_unbounded() {
            raw_view(6000, 4000, false, (2000, 1333))
        } else {
            raw_view(600, 400, true, (0, 0))
        })
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(6000, 4000))
    }
}
