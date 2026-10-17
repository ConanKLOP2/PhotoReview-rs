//! Port of the decode-path tests of `PhotoReview.Imaging.Tests` for `PreviewImageService` (C# baseline d92d6aae; PARITY
//! I-04, I-07, I-10, I-15, I-16, INV-1, INV-2, INV-8), plus the golden check against
//! `tests/golden/preview-service-sequence.json` (33 sequential scenarios). Contract: `contracts/W4-14b.md`.
//!
//! Port of `PhotoReview.Imaging/Caching/PreviewImageService.cs` lines 401-662, 749-787, 800-875, 919-923.
//!
//! Every C# `await task.WaitAsync(Timeout)` is `wait_timeout(TIMEOUT)` (a hang guard); C# `TaskCompletionSource` gates
//! are the decoder gates below; no test sleeps. The tests named `*_disk` are the disk halves of their C# twins: they need
//! the W4-14c persist path (`disk.rs`) and stay red until W4-14c merges.

mod support {
    pub mod preview_service;
}

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex, PoisonError};
use std::thread::ThreadId;
use std::time::{Duration, Instant, SystemTime};

use pr_core::abstractions::{PhysicalSourceReader, ReadSeek, SourceReadPriority, SourceReader};
use pr_core::diagnostics::diag_options::{DiagOptions, PRE_READ_VAR};
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::diagnostics::perf::{self, PerfEvent, PerfListener, PerfValue, PhotoReviewPerf};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;
use pr_imaging::cache::source_bytes::SourceBytesCache;
use pr_imaging::cache_key::{CacheKeyError, ImageCacheKey, ImageSourceKind, KeyOptions};
use pr_imaging::decode_types::{DecodeBox, DecodeRequest, DecodedImageView, ImageInfo, PlatformImage};
use pr_imaging::decoder_traits::{CancellableImageDecoder, DecodeError, ImageDecoder, ImageDecoderFactory};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_imaging::service::decode::DIAG_PRE_READ_BUFFER_SIZE;
use pr_imaging::service::handle::{CANCEL_POLL_SLICE, PreviewHandle};
use pr_imaging::service::{
    DecodeThreadKind, DecodeThreadSpawner, PreviewError, PreviewImage, PreviewImageService, PreviewResult,
    PreviewServiceConfig, SourceChangePhase, StdThreadSpawner, VIEWER_DECODE_SLOTS,
};
use pr_test_support::TempRoot;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::{Value, json};
use support::preview_service::{
    CountingFactory, FakeDecoder, PHYSICAL_32_GIB, RecordingSpawner, TIMEOUT, config, lock,
};

// ---- helpers --------------------------------------------------------------------------------------------------------

/// Waits for a request with the hang guard of [`TIMEOUT`].
fn finish(handle: PreviewHandle) -> PreviewResult {
    match handle.wait_timeout(TIMEOUT) {
        Ok(result) => result,
        Err(_) => panic!("the request did not finish within {TIMEOUT:?}"),
    }
}

fn finish_ok(handle: PreviewHandle) -> PreviewImage {
    finish(handle).unwrap_or_else(|error| panic!("the request failed: {error:?}"))
}

fn finish_err(handle: PreviewHandle) -> PreviewError {
    match finish(handle) {
        Ok(_) => panic!("the request succeeded but an error was expected"),
        Err(error) => error,
    }
}

/// Writes a small source file `name` (under `root`, creating folders) and returns its path as text.
fn source(root: &TempRoot, name: &str) -> String {
    source_with(root, name, &[1, 2, 3, 4])
}

fn source_with(root: &TempRoot, name: &str, bytes: &[u8]) -> String {
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create the source folder");
    }
    std::fs::write(&path, bytes).expect("write the source file");
    path.to_string_lossy().into_owned()
}

/// .NET ticks of the Unix epoch.
const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;

/// C# `File.SetLastWriteTimeUtc(path, new DateTime(ticks, Utc))`.
fn set_write_ticks(path: &str, ticks: i64) {
    let since_epoch = u64::try_from(ticks - UNIX_EPOCH_TICKS).expect("a time after 1970");
    let time = SystemTime::UNIX_EPOCH + Duration::from_nanos(since_epoch * 100);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open the source for its write time");
    file.set_modified(time).expect("set the write time");
}

/// Moves the source's write time 3 days into the future (C# `File.SetLastWriteTimeUtc(path, UtcNow.AddDays(3))`).
fn touch_future(path: &str) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open the source");
    file.set_modified(SystemTime::now() + Duration::from_secs(3 * 24 * 3600))
        .expect("set the write time");
}

fn build(config: PreviewServiceConfig) -> PreviewImageService {
    PreviewImageService::new(config).expect("the service builds")
}

/// A service with the C# test defaults (64 MiB, disk cache off) over `decoder`.
fn service_over(root: &TempRoot, decoder: Arc<dyn ImageDecoder>) -> (PreviewImageService, Arc<ReviewMetrics>) {
    service_with(root, decoder, |_| {})
}

fn service_with(
    root: &TempRoot,
    decoder: Arc<dyn ImageDecoder>,
    adjust: impl FnOnce(&mut PreviewServiceConfig),
) -> (PreviewImageService, Arc<ReviewMetrics>) {
    let mut config = config(&root.join("cache"), decoder);
    adjust(&mut config);
    let metrics = Arc::clone(&config.metrics);
    (build(config), metrics)
}

fn pv4_files(dir: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "pv4"))
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ---- test decoder ---------------------------------------------------------------------------------------------------

/// What a [`TestDecoder`] decode returns.
#[derive(Debug, Clone, Copy)]
struct ImageSpec {
    width: i32,
    height: i32,
    /// `None` = `request.is_downscale_requested()`.
    downscaled: Option<bool>,
    backend: DecoderBackend,
    original: Option<(i32, i32)>,
    degraded: bool,
    source_bytes_read: Option<i64>,
}

impl Default for ImageSpec {
    fn default() -> Self {
        Self {
            width: 8,
            height: 6,
            downscaled: Some(false),
            backend: DecoderBackend::Wpf,
            original: None,
            degraded: false,
            source_bytes_read: None,
        }
    }
}

/// A decoded image over opaque pixels (persistable by W4-14c).
struct TestImage {
    spec: ImageSpec,
    downscaled: bool,
    platform: PlatformImage,
}

impl TestImage {
    fn new(spec: ImageSpec, downscaled: bool) -> Arc<Self> {
        let mut pixels = PixelBuffer::allocate(spec.width, spec.height, PixelLayout::Bgr32).expect("test pixels");
        pixels.bytes_mut().expect("test pixel bytes").fill(200);
        Arc::new(Self {
            spec,
            downscaled,
            platform: PlatformImage::Pixels(Arc::new(pixels)),
        })
    }
}

impl DecodedImageView for TestImage {
    fn pixel_width(&self) -> i32 {
        self.spec.width
    }
    fn pixel_height(&self) -> i32 {
        self.spec.height
    }
    fn downscaled(&self) -> bool {
        self.downscaled
    }
    fn orientation(&self) -> i32 {
        1
    }
    fn estimated_bytes(&self) -> i64 {
        i64::from(self.spec.width) * i64::from(self.spec.height) * 4
    }
    fn platform_image(&self) -> &PlatformImage {
        &self.platform
    }
    fn actual_backend(&self) -> DecoderBackend {
        self.spec.backend
    }
    fn original_width(&self) -> i32 {
        self.spec.original.map_or(self.spec.width, |(w, _)| w)
    }
    fn original_height(&self) -> i32 {
        self.spec.original.map_or(self.spec.height, |(_, h)| h)
    }
    fn is_degraded_fallback(&self) -> bool {
        self.spec.degraded
    }
    fn source_bytes_read(&self) -> Option<i64> {
        self.spec.source_bytes_read
    }
}

/// One `decode` call as the decoder saw it.
#[derive(Debug, Clone)]
struct Call {
    name: String,
    request: DecodeRequest,
    thread: ThreadId,
}

#[derive(Default)]
struct DecoderState {
    calls: Vec<Call>,
    gate_all: bool,
    gated: HashSet<String>,
    failures: VecDeque<DecodeError>,
}

/// A scripted decoder: records every call (with its thread), blocks gated file names (or every call) until released,
/// fails the next calls from a queue (decided when the call arrives), and returns [`ImageSpec`] images otherwise.
struct TestDecoder {
    state: Mutex<DecoderState>,
    changed: Condvar,
    spec: Mutex<ImageSpec>,
    info: Mutex<Option<ImageInfo>>,
    read_info_calls: AtomicUsize,
    /// Busy-waits this long inside every decode (an elapsed time of at least 1 ms without sleeping).
    spin: Mutex<Duration>,
    panics: AtomicBool,
}

impl TestDecoder {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(DecoderState::default()),
            changed: Condvar::new(),
            spec: Mutex::new(ImageSpec::default()),
            info: Mutex::new(Some(ImageInfo::new(10, 10))),
            read_info_calls: AtomicUsize::new(0),
            spin: Mutex::new(Duration::ZERO),
            panics: AtomicBool::new(false),
        })
    }

    fn with_spec(spec: ImageSpec) -> Arc<Self> {
        let decoder = Self::new();
        *lock(&decoder.spec) = spec;
        decoder
    }

    fn gate_all(&self) {
        lock(&self.state).gate_all = true;
    }

    fn gate(&self, name: &str) {
        lock(&self.state).gated.insert(name.to_owned());
    }

    fn release(&self, name: &str) {
        lock(&self.state).gated.remove(name);
        self.changed.notify_all();
    }

    fn release_all(&self) {
        {
            let mut state = lock(&self.state);
            state.gate_all = false;
            state.gated.clear();
        }
        self.changed.notify_all();
    }

    fn fail_next(&self, error: DecodeError) {
        lock(&self.state).failures.push_back(error);
    }

    /// Waits until at least `count` decodes have started; `false` after [`TIMEOUT`].
    fn wait_calls(&self, count: usize) -> bool {
        let state = lock(&self.state);
        let (state, _) = self
            .changed
            .wait_timeout_while(state, TIMEOUT, |s| s.calls.len() < count)
            .unwrap_or_else(PoisonError::into_inner);
        state.calls.len() >= count
    }

    /// Waits until a decode of the file called `name` has started; `false` after [`TIMEOUT`].
    fn wait_started(&self, name: &str) -> bool {
        let state = lock(&self.state);
        let (state, _) = self
            .changed
            .wait_timeout_while(state, TIMEOUT, |s| !s.calls.iter().any(|call| call.name == name))
            .unwrap_or_else(PoisonError::into_inner);
        state.calls.iter().any(|call| call.name == name)
    }

    fn calls(&self) -> Vec<Call> {
        lock(&self.state).calls.clone()
    }

    fn call_count(&self) -> usize {
        lock(&self.state).calls.len()
    }

    fn calls_for(&self, name: &str) -> usize {
        lock(&self.state).calls.iter().filter(|call| call.name == name).count()
    }
}

impl ImageDecoder for TestDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let name = name_of(&request.path);
        let failure = {
            let mut state = lock(&self.state);
            state.calls.push(Call {
                name: name.clone(),
                request: request.clone(),
                thread: std::thread::current().id(),
            });
            state.failures.pop_front()
        };
        self.changed.notify_all();
        {
            let state = lock(&self.state);
            let _ = self
                .changed
                .wait_timeout_while(state, TIMEOUT, |s| s.gate_all || s.gated.contains(&name))
                .unwrap_or_else(PoisonError::into_inner);
        }
        let spin = *lock(&self.spin);
        let start = Instant::now();
        while start.elapsed() < spin {
            std::hint::spin_loop();
        }
        if self.panics.load(Ordering::SeqCst) {
            panic!("test decoder panic");
        }
        if let Some(error) = failure {
            return Err(error);
        }
        let spec = *lock(&self.spec);
        let downscaled = spec.downscaled.unwrap_or_else(|| request.is_downscale_requested());
        Ok(TestImage::new(spec, downscaled))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        self.read_info_calls.fetch_add(1, Ordering::SeqCst);
        (*lock(&self.info)).ok_or_else(|| DecodeError::Io(io::Error::other("test: read info")))
    }
}

/// A raw full decoder that also implements the cancellable decode and records which entry point ran.
struct CancellableRawDecoder {
    plain: AtomicUsize,
    cancellable: AtomicUsize,
    saw_token_cancelled: AtomicBool,
}

impl ImageDecoder for CancellableRawDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.plain.fetch_add(1, Ordering::SeqCst);
        Ok(TestImage::new(ImageSpec::default(), false))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(8, 6))
    }

    fn as_cancellable(&self) -> Option<&dyn CancellableImageDecoder> {
        Some(self)
    }
}

impl CancellableImageDecoder for CancellableRawDecoder {
    fn decode_cancellable(
        &self,
        request: &DecodeRequest,
        token: &CancelToken,
    ) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.cancellable.fetch_add(1, Ordering::SeqCst);
        self.saw_token_cancelled.store(token.is_canceled(), Ordering::SeqCst);
        assert!(request.bytes.is_none());
        Ok(TestImage::new(ImageSpec::default(), false))
    }
}

/// A factory over a closure that records every backend it was asked for.
struct RecordingFactory {
    created: Mutex<Vec<DecoderBackend>>,
    make: Box<dyn Fn(DecoderBackend) -> Arc<dyn ImageDecoder> + Send + Sync>,
}

impl RecordingFactory {
    fn new(make: impl Fn(DecoderBackend) -> Arc<dyn ImageDecoder> + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            created: Mutex::new(Vec::new()),
            make: Box::new(make),
        })
    }

    fn created(&self) -> Vec<DecoderBackend> {
        lock(&self.created).clone()
    }
}

impl ImageDecoderFactory for RecordingFactory {
    fn create(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        lock(&self.created).push(backend);
        (self.make)(backend)
    }

    fn is_registered(&self, _backend: DecoderBackend) -> bool {
        true
    }
}

/// A spawner that refuses the listed kinds and runs the others on `std::thread`.
struct RefusingSpawner {
    refused: Vec<DecodeThreadKind>,
}

impl DecodeThreadSpawner for RefusingSpawner {
    fn spawn(&self, kind: DecodeThreadKind, job: Box<dyn FnOnce() + Send + 'static>) -> io::Result<()> {
        if self.refused.contains(&kind) {
            return Err(io::Error::other("test: no thread"));
        }
        StdThreadSpawner.spawn(kind, job)
    }
}

/// A source reader that counts its opens and delegates to the physical reader.
#[derive(Default)]
struct CountingReader {
    opens: AtomicUsize,
    buffer_sizes: Mutex<Vec<usize>>,
}

impl SourceReader for CountingReader {
    fn open_source(
        &self,
        path: &Path,
        priority: SourceReadPriority,
        buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        lock(&self.buffer_sizes).push(buffer_size);
        PhysicalSourceReader.open_source(path, priority, buffer_size)
    }
}

/// What a [`ScriptedStream`] does.
#[derive(Clone, Copy)]
enum StreamScript {
    /// Serves the file at most `chunk` bytes per read, failing the first read with `Interrupted`.
    Chunked { chunk: usize },
    /// Claims this length when seeked to the end; every read returns 0 bytes.
    ClaimedLength(u64),
    /// Every read fails with `PermissionDenied`.
    FailingRead,
}

struct ScriptedStream {
    data: Vec<u8>,
    position: usize,
    script: StreamScript,
    interrupted: bool,
}

impl io::Read for ScriptedStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.script {
            StreamScript::FailingRead => Err(io::Error::new(io::ErrorKind::PermissionDenied, "test: read denied")),
            StreamScript::ClaimedLength(_) => Ok(0),
            StreamScript::Chunked { chunk } => {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "test: interrupted"));
                }
                let end = (self.position + chunk.min(buf.len())).min(self.data.len());
                let count = end - self.position;
                buf[..count].copy_from_slice(&self.data[self.position..end]);
                self.position = end;
                Ok(count)
            }
        }
    }
}

impl io::Seek for ScriptedStream {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        let data_length = u64::try_from(self.data.len()).expect("length");
        let length = match self.script {
            StreamScript::ClaimedLength(length) => length,
            _ => data_length,
        };
        let target = match position {
            io::SeekFrom::Start(offset) => offset,
            io::SeekFrom::End(offset) => length.saturating_add_signed(offset),
            io::SeekFrom::Current(offset) => u64::try_from(self.position)
                .expect("position")
                .saturating_add_signed(offset),
        };
        self.position = usize::try_from(target.min(data_length)).expect("position");
        Ok(target)
    }
}

/// A source reader serving a [`ScriptedStream`] over the real file's bytes.
struct ScriptedReader {
    script: StreamScript,
}

impl SourceReader for ScriptedReader {
    fn open_source(
        &self,
        path: &Path,
        _priority: SourceReadPriority,
        _buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(ScriptedStream {
            data: std::fs::read(path)?,
            position: 0,
            script: self.script,
            interrupted: false,
        }))
    }
}

/// Records every perf event.
#[derive(Default)]
struct PerfRecorder {
    events: Mutex<Vec<PerfEvent>>,
}

impl PerfListener for PerfRecorder {
    fn on_event_written(&self, event: &PerfEvent) {
        lock(&self.events).push(event.clone());
    }
}

impl PerfRecorder {
    /// Events named `name` whose `pathId` is `path_id`.
    fn for_path(&self, name: &str, path_id: &str) -> Vec<PerfEvent> {
        lock(&self.events)
            .iter()
            .filter(|event| event.name == name && event.payload.get(1) == Some(&PerfValue::Str(path_id.to_owned())))
            .cloned()
            .collect()
    }
}

fn snapshot(metrics: &ReviewMetrics) -> pr_core::diagnostics::metrics::ReviewMetricsSnapshot {
    metrics.snapshot()
}

// ---- PreviewImageServiceBusyJoinTests -------------------------------------------------------------------------------

/// The C# `BusyOnFirstCallDecoder`: the first decode blocks until released, then fails `Busy`; later calls succeed.
fn busy_on_first_call() -> Arc<TestDecoder> {
    let decoder = TestDecoder::new();
    decoder.gate_all();
    decoder.fail_next(DecodeError::busy_with("test: the gate refused this preload"));
    decoder
}

// C#: PreviewImageServiceBusyJoinTests.GetViewerPreview_JoinedPreloadThrowsBusy_ViewerRetriesAndSucceeds
#[test]
fn get_viewer_preview_joined_preload_throws_busy_viewer_retries_and_succeeds() {
    let root = TempRoot::new("decode-busy-join-viewer");
    let path = source(&root, "images/a.jpg");
    let decoder = busy_on_first_call();
    let (service, metrics) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    let preload = service.get_preview(&path, &key);
    assert!(decoder.wait_calls(1));
    let viewer = service.get_viewer_preview(&path, &key, &CancelToken::none(), Duration::ZERO);
    assert_eq!(snapshot(&metrics).inflight_joins, 1, "the viewer joined synchronously");

    decoder.release_all();

    finish_ok(viewer);
    assert!(finish_err(preload).is_busy());
    assert_eq!(decoder.call_count(), 2);
}

// C#: PreviewImageServiceBusyJoinTests.GetViewerPreview_TwoJoinersOfBusyPreload_BothSucceed
#[test]
fn get_viewer_preview_two_joiners_of_busy_preload_both_succeed() {
    let root = TempRoot::new("decode-busy-join-two");
    let path = source(&root, "images/a.jpg");
    let decoder = busy_on_first_call();
    let (service, metrics) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    let preload = service.get_preview(&path, &key);
    assert!(decoder.wait_calls(1));
    let viewer1 = service.get_viewer_preview(&path, &key, &CancelToken::none(), Duration::ZERO);
    let viewer2 = service.get_viewer_preview(&path, &key, &CancelToken::none(), Duration::ZERO);
    assert_eq!(snapshot(&metrics).inflight_joins, 2);

    decoder.release_all();

    let first = finish_ok(viewer1);
    let second = finish_ok(viewer2);
    assert!(Arc::ptr_eq(&first, &second), "one shared retry decode");
    assert!(finish_err(preload).is_busy());
    assert_eq!(decoder.call_count(), 2, "one refused preload decode + one shared retry");
}

// C#: PreviewImageServiceBusyJoinTests.GetPreview_JoinerOfBusyCreator_Retries
#[test]
fn get_preview_joiner_of_busy_creator_retries() {
    let root = TempRoot::new("decode-busy-join-preload");
    let path = source(&root, "images/a.jpg");
    let decoder = busy_on_first_call();
    let (service, metrics) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    let creator = service.get_preview(&path, &key);
    assert!(decoder.wait_calls(1));
    let joiner = service.get_preview(&path, &key);
    assert_eq!(snapshot(&metrics).inflight_joins, 1);

    decoder.release_all();

    finish_ok(joiner);
    assert!(finish_err(creator).is_busy());
}

// C#: PreviewImageServiceBusyJoinTests.GetPreview_CreatorBusy_RemovesInflightEntry
#[test]
fn get_preview_creator_busy_removes_inflight_entry() {
    let root = TempRoot::new("decode-busy-remove");
    let path = source(&root, "images/a.jpg");
    let decoder = busy_on_first_call();
    let (service, _) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");
    decoder.release_all();

    assert!(finish_err(service.get_preview(&path, &key)).is_busy());

    assert!(!service.has_inflight_preview(&key));
    finish_ok(service.get_preview(&path, &key));
}

// ---- PreviewImageServiceCancellationGapTests ------------------------------------------------------------------------

// C#: PreviewImageServiceCancellationGapTests.GetPreview_CreatorDecodeCancelled_Propagates
#[test]
fn get_preview_creator_decode_cancelled_propagates() {
    let root = TempRoot::new("decode-cancel-gap");
    let path = source(&root, "images/a.jpg");
    let decoder = TestDecoder::new();
    decoder.fail_next(DecodeError::Canceled);
    let (service, _) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    let error = finish_err(service.get_preview(&path, &key));

    assert!(error.is_canceled());
    assert!(matches!(&error, PreviewError::Decode(inner) if matches!(**inner, DecodeError::Canceled)));
    assert_eq!(decoder.call_count(), 1);
}

// ---- PreviewImageServiceGapTests ------------------------------------------------------------------------------------

// C#: PreviewImageServiceGapTests.ViewerJoiner_CreatorCancelledBeforeSlot_JoinerRetriesAndSucceeds
#[test]
fn viewer_joiner_creator_cancelled_before_slot_joiner_retries_and_succeeds() {
    let root = TempRoot::new("decode-gap-joiner");
    let decoder = TestDecoder::with_spec(ImageSpec {
        downscaled: Some(true),
        original: Some((800, 600)),
        ..ImageSpec::default()
    });
    let (service, metrics) = service_over(&root, decoder.clone());
    let a = source(&root, "a.jpg");
    let b = source(&root, "b.jpg");
    let c = source(&root, "c.jpg");
    decoder.gate("a.jpg");
    decoder.gate("b.jpg");
    let key_a = service.get_current_cache_key(&a).expect("key a");
    let key_b = service.get_current_cache_key(&b).expect("key b");
    let key_c = service.get_current_cache_key(&c).expect("key c");
    // Two blocked viewer decodes occupy every viewer slot, so c's decode cannot start.
    let blocker_a = service.get_viewer_preview(&a, &key_a, &CancelToken::none(), Duration::ZERO);
    let blocker_b = service.get_viewer_preview(&b, &key_b, &CancelToken::none(), Duration::ZERO);
    assert!(decoder.wait_started("a.jpg"));
    assert!(decoder.wait_started("b.jpg"));
    assert_eq!(service.active_viewer_decodes(), VIEWER_DECODE_SLOTS);

    let creator_token = CancelToken::new();
    let creator = service.get_viewer_preview(&c, &key_c, &creator_token, Duration::ZERO);
    let joiner = service.get_viewer_preview(&c, &key_c, &CancelToken::none(), Duration::ZERO);
    assert_eq!(
        snapshot(&metrics).inflight_joins,
        1,
        "the second navigation joined the first one"
    );
    creator_token.cancel();

    assert!(finish_err(creator).is_canceled());
    decoder.release("a.jpg"); // frees a slot for the joiner's own retry
    finish_ok(joiner);
    decoder.release("b.jpg");
    finish_ok(blocker_a);
    finish_ok(blocker_b);

    assert_eq!(decoder.calls_for("c.jpg"), 1, "only the joiner's retry decoded");
    assert!(!service.has_inflight_preview(&key_a));
    assert!(!service.has_inflight_preview(&key_b));
    assert!(!service.has_inflight_preview(&key_c));
}

fn clear_cache_during_inflight_decode(disk: bool) {
    let root = TempRoot::new("decode-gap-clear");
    let decoder = TestDecoder::with_spec(ImageSpec {
        downscaled: Some(true),
        original: Some((800, 600)),
        ..ImageSpec::default()
    });
    let disk_dir = root.join("disk");
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        if disk {
            config.disk_cache_directory = Some(disk_dir.clone());
            config.disable_disk_cache_override = Some(false);
        }
    });
    let stale = source(&root, "stale.jpg");
    let fresh = source(&root, "fresh.jpg");
    decoder.gate("stale.jpg");
    let stale_key = service.get_current_cache_key(&stale).expect("stale key");
    let pending = service.get_preview(&stale, &stale_key);
    assert!(decoder.wait_started("stale.jpg"));

    service.clear_cache(); // epoch bump while the decode runs
    decoder.release("stale.jpg");
    finish_ok(pending);

    assert!(service.try_get_cached_preview(&stale_key).is_none());
    // Control: a decode that starts after the clear is cached (and persisted).
    let fresh_key = service.get_current_cache_key(&fresh).expect("fresh key");
    finish_ok(service.get_preview(&fresh, &fresh_key));
    assert!(service.try_get_cached_preview(&fresh_key).is_some());
    if disk {
        assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
        assert_eq!(pv4_files(&disk_dir).len(), 1, "only the fresh decode was persisted");
    }
}

// C#: PreviewImageServiceGapTests.ClearCache_DuringInflightDecode_ResultReturnedButNotCachedOrPersisted (RAM half)
#[test]
fn clear_cache_during_inflight_decode_result_returned_but_not_cached() {
    clear_cache_during_inflight_decode(false);
}

// C#: PreviewImageServiceGapTests.ClearCache_DuringInflightDecode_ResultReturnedButNotCachedOrPersisted (disk half,
// red until W4-14c merges)
#[test]
fn clear_cache_during_inflight_decode_result_returned_but_not_cached_or_persisted_disk() {
    clear_cache_during_inflight_decode(true);
}

// ---- PreviewImageServiceGapTests2 -----------------------------------------------------------------------------------

// C#: PreviewImageServiceGapTests2.DecodeOriginalAsync_CancelledBeforeTheGate_NeverDecodesAndLeavesTheGateUsable
#[test]
fn decode_original_cancelled_before_the_gate_never_decodes_and_leaves_the_gate_usable() {
    let root = TempRoot::new("decode-gap2-gate");
    let first = source(&root, "g1.jpg");
    let second = source(&root, "g2.jpg");
    let decoder = TestDecoder::new();
    let (service, _) = service_over(&root, decoder.clone());
    let cancelled = CancelToken::new();
    cancelled.cancel();

    let key = service.get_current_cache_key(&first).expect("key");
    let handle = service.decode_original(&first, &key, &cancelled);
    assert!(handle.is_ready(), "a cancelled token fails before any thread starts");
    assert!(matches!(finish_err(handle), PreviewError::Canceled));
    assert_eq!(decoder.call_count(), 0);

    // The single gate slot must still be available, and handed back after every completed decode.
    finish_ok(service.decode_original(&first, &key, &CancelToken::none()));
    let second_key = service.get_current_cache_key(&second).expect("key");
    finish_ok(service.decode_original(&second, &second_key, &CancelToken::none()));
    assert_eq!(decoder.call_count(), 2);
}

// ---- PreviewImageServiceDegradedFallbackTests -----------------------------------------------------------------------

fn degraded_service(root: &TempRoot, decoder: Arc<TestDecoder>, disk: bool) -> (PreviewImageService, PathBuf) {
    let disk_dir = root.join("disk");
    let (service, _) = service_with(root, decoder, |config| {
        config.target_decode_box = Arc::new(|| DecodeBox::new(32, 0));
        if disk {
            config.disk_cache_directory = Some(disk_dir.clone());
            config.disable_disk_cache_override = None;
        }
    });
    (service, disk_dir)
}

fn degraded_spec(degraded: bool) -> ImageSpec {
    ImageSpec {
        width: 32,
        height: 24,
        // Would be persisted to disk were it not degraded.
        downscaled: Some(true),
        degraded,
        ..ImageSpec::default()
    }
}

fn degraded_fallback_is_returned_but_not_cached(disk: bool) {
    let root = TempRoot::new("decode-degraded");
    let path = source(&root, "source/a.png");
    let decoder = TestDecoder::with_spec(degraded_spec(true));
    let (service, disk_dir) = degraded_service(&root, decoder.clone(), disk);

    let first = finish_ok(service.get_preview_path(&path));
    if disk {
        assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
        assert!(pv4_files(&disk_dir).is_empty());
    }

    assert!(first.pixel_width() > 0);
    assert!(first.is_degraded_fallback());
    assert_eq!(service.cache_count(), 0);

    finish_ok(service.get_preview_path(&path));
    assert_eq!(decoder.call_count(), 2, "retried, not served from any cache");
}

// C#: PreviewImageServiceDegradedFallbackTests.GetPreviewAsync_DegradedFallback_IsReturnedButNotCachedInRamOrOnDisk
#[test]
fn get_preview_degraded_fallback_is_returned_but_not_cached_in_ram() {
    degraded_fallback_is_returned_but_not_cached(false);
}

// C#: PreviewImageServiceDegradedFallbackTests.GetPreviewAsync_DegradedFallback_IsReturnedButNotCachedInRamOrOnDisk
// (disk half, red until W4-14c merges)
#[test]
fn get_preview_degraded_fallback_is_returned_but_not_cached_in_ram_or_on_disk_disk() {
    degraded_fallback_is_returned_but_not_cached(true);
}

// C#: PreviewImageServiceDegradedFallbackTests.GetPreviewAsync_DegradedFallbackSharedByJoiners_EveryJoinerGetsTheImage
#[test]
fn get_preview_degraded_fallback_shared_by_joiners_every_joiner_gets_the_image() {
    let root = TempRoot::new("decode-degraded-joiners");
    let path = source(&root, "source/a.png");
    let decoder = TestDecoder::with_spec(degraded_spec(true));
    decoder.gate_all();
    let (service, _) = degraded_service(&root, decoder.clone(), false);

    let a = service.get_preview_path(&path);
    let b = service.get_preview_path(&path);
    decoder.release_all();
    let first = finish_ok(a);
    let second = finish_ok(b);

    assert!(first.pixel_width() > 0 && second.pixel_width() > 0);
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(service.cache_count(), 0);
    assert_eq!(decoder.call_count(), 1);
}

fn normal_decode_is_cached(disk: bool) {
    let root = TempRoot::new("decode-degraded-normal");
    let path = source(&root, "source/a.png");
    let decoder = TestDecoder::with_spec(degraded_spec(false));
    let (service, disk_dir) = degraded_service(&root, decoder.clone(), disk);

    finish_ok(service.get_preview_path(&path));
    if disk {
        assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
        assert!(service.wait_for_prune(TIMEOUT));
        assert_eq!(pv4_files(&disk_dir).len(), 1);
    }

    assert_eq!(service.cache_count(), 1);
    finish_ok(service.get_preview_path(&path));
    assert_eq!(decoder.call_count(), 1);
}

// C#: PreviewImageServiceDegradedFallbackTests.GetPreviewAsync_NormalDecode_IsCachedInRamAndPersistedToDisk (RAM half)
#[test]
fn get_preview_normal_decode_is_cached_in_ram() {
    normal_decode_is_cached(false);
}

// C#: PreviewImageServiceDegradedFallbackTests.GetPreviewAsync_NormalDecode_IsCachedInRamAndPersistedToDisk (disk half,
// red until W4-14c merges)
#[test]
fn get_preview_normal_decode_is_cached_in_ram_and_persisted_to_disk_disk() {
    normal_decode_is_cached(true);
}

// ---- PreviewImageServiceFaultTests ----------------------------------------------------------------------------------

fn readers_racing_clears(disk: bool) {
    let root = TempRoot::new("decode-fault-race");
    let path = source(&root, "source/a.png");
    let decoder = TestDecoder::with_spec(ImageSpec {
        width: 32,
        height: 24,
        downscaled: Some(true),
        ..ImageSpec::default()
    });
    let disk_dir = root.join("disk-race");
    let (service, _) = service_with(&root, decoder, |config| {
        if disk {
            config.disk_cache_directory = Some(disk_dir.clone());
            config.disable_disk_cache_override = None;
        }
    });
    let service = Arc::new(service);
    let barrier = Arc::new(Barrier::new(7));
    let stop = Arc::new(AtomicBool::new(false));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));

    let readers: Vec<_> = (0..6)
        .map(|worker| {
            let service = Arc::clone(&service);
            let barrier = Arc::clone(&barrier);
            let failures = Arc::clone(&failures);
            let path = path.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..60 {
                    match service.get_preview_path(&path).wait_timeout(TIMEOUT) {
                        Ok(Ok(image)) if image.pixel_width() > 0 => {}
                        Ok(Ok(_)) => lock(&failures).push("empty image".to_owned()),
                        Ok(Err(error)) => lock(&failures).push(format!("worker {worker}: {error:?}")),
                        Err(_) => lock(&failures).push(format!("worker {worker}: hang")),
                    }
                }
            })
        })
        .collect();
    let clearer = {
        let service = Arc::clone(&service);
        let barrier = Arc::clone(&barrier);
        let stop = Arc::clone(&stop);
        let path = path.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let mut round = 0usize;
            while !stop.load(Ordering::SeqCst) {
                match round % 3 {
                    0 => service.clear_cache(),
                    1 if disk => service.clear_disk(),
                    _ => service.evict_cached_path(&path, None).expect("evict"),
                }
                round += 1;
            }
        })
    };

    for reader in readers {
        reader.join().expect("reader thread");
    }
    stop.store(true, Ordering::SeqCst);
    clearer.join().expect("clearer thread");
    if disk {
        assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
        assert!(service.wait_for_prune(TIMEOUT));
    }

    assert!(lock(&failures).is_empty(), "{:?}", lock(&failures));
    let key = service.get_current_cache_key(&path).expect("key");
    assert!(!service.has_inflight_preview(&key));
}

// C#: PreviewImageServiceFaultTests.ReadersRacingClears_AllGetImages (ClearCache + EvictCachedPath, disk off)
#[test]
fn readers_racing_clears_all_get_images() {
    readers_racing_clears(false);
}

// C#: PreviewImageServiceFaultTests.ReadersRacingClears_AllGetImages (with ClearDisk and the disk cache on; red until
// W4-14c merges)
#[test]
fn readers_racing_clears_all_get_images_disk() {
    readers_racing_clears(true);
}

// C#: PreviewImageServiceFaultTests.LruThrash_StaysWithinBudget_AndCorrect
#[test]
fn lru_thrash_stays_within_budget_and_correct() {
    let root = TempRoot::new("decode-fault-thrash");
    let files: Vec<String> = (0..12).map(|i| source(&root, &format!("thrash/img{i}.png"))).collect();
    // 32x32 previews cost 4096 bytes each; the budget holds about three.
    let decoder = TestDecoder::with_spec(ImageSpec {
        width: 32,
        height: 32,
        downscaled: Some(true),
        ..ImageSpec::default()
    });
    let (service, _) = service_with(&root, decoder, |config| {
        config.capacity_bytes = 3 * 4096 + 100;
        config.target_decode_box = Arc::new(|| DecodeBox::new(32, 0));
    });
    let service = Arc::new(service);
    let files = Arc::new(files);
    let barrier = Arc::new(Barrier::new(8));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));

    let workers: Vec<_> = (0..8u64)
        .map(|worker| {
            let service = Arc::clone(&service);
            let files = Arc::clone(&files);
            let barrier = Arc::clone(&barrier);
            let failures = Arc::clone(&failures);
            std::thread::spawn(move || {
                // A small deterministic LCG per worker (C# `new Random(worker)`).
                let mut state = worker
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                barrier.wait();
                for _ in 0..150 {
                    state = state
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let index = usize::try_from((state >> 33) % 12).expect("index");
                    match finish(service.get_preview_path(&files[index])) {
                        Ok(image) if image.pixel_width() == 32 => {}
                        Ok(image) => lock(&failures).push(format!("width {}", image.pixel_width())),
                        Err(error) => lock(&failures).push(format!("{error:?}")),
                    }
                    if service.cache_bytes() > service.capacity_bytes() {
                        lock(&failures).push(format!(
                            "cache {} > budget {}",
                            service.cache_bytes(),
                            service.capacity_bytes()
                        ));
                    }
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("worker thread");
    }

    assert!(lock(&failures).is_empty(), "{:?}", lock(&failures));
    assert!(
        (1..=4).contains(&service.cache_count()),
        "count {}",
        service.cache_count()
    );
}

/// Decodes the old content, signals, waits for the release, then returns the stale image (C# `GatedDecoder`).
fn source_replaced_during_decode(disk: bool) {
    let root = TempRoot::new("decode-fault-replaced");
    let path = source_with(&root, "replaced/changing.png", &[9; 64]);
    let decoder = TestDecoder::with_spec(ImageSpec {
        width: 32,
        height: 24,
        downscaled: Some(true),
        ..ImageSpec::default()
    });
    decoder.gate_all();
    let disk_dir = root.join("replaced-disk");
    let (service, metrics) = service_with(&root, decoder.clone(), |config| {
        config.target_decode_box = Arc::new(|| DecodeBox::new(32, 0));
        if disk {
            config.disk_cache_directory = Some(disk_dir.clone());
            config.disable_disk_cache_override = None;
        }
    });

    let pending = service.get_preview_path(&path);
    assert!(decoder.wait_calls(1));
    // Same size, later timestamp: only the identity check can tell the new file from the old one.
    std::fs::write(&path, [9u8; 64]).expect("rewrite");
    touch_future(&path);
    decoder.release_all();

    let error = finish_err(pending);
    assert!(error.is_io_exception());
    assert!(matches!(
        &error,
        PreviewError::SourceChanged {
            phase: SourceChangePhase::Decode,
            ..
        }
    ));
    assert_eq!(service.cache_count(), 0);
    assert_eq!(
        service.try_get_known_original_dimensions(&service.get_current_cache_key(&path).expect("key")),
        None
    );
    assert_eq!(
        snapshot(&metrics).source_reads,
        0,
        "a stale decode records no source read"
    );
    if disk {
        assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
        assert!(pv4_files(&disk_dir).is_empty());
    }

    // The failure is not sticky: the next request keys on the new identity and succeeds.
    let (service2, _) = service_over(&root, decoder.clone());
    assert!(finish_ok(service2.get_preview_path(&path)).pixel_width() > 0);
    assert!(!service.has_inflight_preview(&service.get_current_cache_key(&path).expect("key")));
}

// C#: PreviewImageServiceFaultTests.SourceReplacedDuringDecode_IsNeverCachedOrPersisted (RAM half)
#[test]
fn source_replaced_during_decode_is_never_cached() {
    source_replaced_during_decode(false);
}

// C#: PreviewImageServiceFaultTests.SourceReplacedDuringDecode_IsNeverCachedOrPersisted (disk half, red until W4-14c)
#[test]
fn source_replaced_during_decode_is_never_cached_or_persisted_disk() {
    source_replaced_during_decode(true);
}

// C#: PreviewImageServiceFaultTests.OversizedSource_IsNotPreReadIntoMemory (theory rows 100 000 / true and 60 / false)
#[test]
fn oversized_source_is_not_pre_read_into_memory() {
    for (capacity, expect_pre_read) in [(100_000i64, true), (60, false)] {
        let root = TempRoot::new("decode-fault-preread");
        let bytes: Vec<u8> = (0..4096u32).map(|i| u8::try_from(i % 251).expect("byte")).collect();
        let path = source_with(&root, "preread/big.png", &bytes);
        let length = std::fs::metadata(&path).expect("stat").len();
        assert!(length > 60 && length < 100_000);
        let decoder = TestDecoder::new();
        let cache = Arc::new(SourceBytesCache::new(capacity, PHYSICAL_32_GIB, None).expect("source-bytes cache"));
        let (service, metrics) = service_with(&root, decoder.clone(), |config| {
            config.source_bytes_cache = Some(Arc::clone(&cache));
        });

        let image = finish_ok(service.get_preview_path(&path));

        assert!(image.pixel_width() > 0);
        let call = decoder.calls().pop().expect("one decode");
        assert_eq!(call.request.bytes.is_some(), expect_pre_read, "capacity {capacity}");
        if expect_pre_read {
            assert_eq!(call.request.bytes.as_deref(), Some(bytes.as_slice()));
            assert_eq!(cache.count(), 1);
        } else {
            assert_eq!(cache.count(), 0);
        }
        assert_eq!(snapshot(&metrics).source_open_count, 1);
    }
}

// ---- PreviewImageServiceMutationTests -------------------------------------------------------------------------------

// C#: PreviewImageServiceMutationTests.ViewerPreview_PositiveStartDelay_IsHonoured
#[test]
fn viewer_preview_positive_start_delay_is_honoured() {
    let root = TempRoot::new("decode-mut-delay");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    let (service, _) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");
    let token = CancelToken::new();

    let pending = service.get_viewer_preview(&path, &key, &token, Duration::from_secs(3600));
    assert!(service.has_inflight_preview(&key));
    token.cancel();

    assert!(matches!(finish_err(pending), PreviewError::Canceled));
    assert_eq!(decoder.call_count(), 0);
    assert!(!service.has_inflight_preview(&key));
}

// C#: PreviewImageServiceMutationTests.ViewerPreview_NonPositiveStartDelay_DecodesImmediately (row 0; the rows -1 and -5
// are negative `TimeSpan`s, which `Duration` cannot express: callers clamp at 0)
#[test]
fn viewer_preview_non_positive_start_delay_decodes_immediately() {
    let root = TempRoot::new("decode-mut-zero-delay");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    let (service, _) = service_over(&root, decoder.clone());
    // A cancelled token never matters for a zero delay; it is the slot gate that refuses it (asserted elsewhere).
    let key = service.get_current_cache_key(&path).expect("key");

    finish_ok(service.get_viewer_preview(&path, &key, &CancelToken::none(), Duration::ZERO));

    assert_eq!(decoder.call_count(), 1);
}

// C#: PreviewImageServiceMutationTests.ViewerDecode_RunsOnDedicatedThread_PreloadOnPool
#[test]
fn viewer_decode_runs_on_dedicated_thread_preload_on_pool() {
    let root = TempRoot::new("decode-mut-threads");
    let decoder = TestDecoder::new();
    let spawner = RecordingSpawner::new();
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        config.spawner = Some(spawner.clone());
    });
    let viewer_path = source(&root, "viewer.jpg");
    let preload_path = source(&root, "preload.jpg");

    let viewer_key = service.get_current_cache_key(&viewer_path).expect("key");
    finish_ok(service.get_viewer_preview(&viewer_path, &viewer_key, &CancelToken::none(), Duration::ZERO));
    finish_ok(service.get_preview_path(&preload_path));

    let decode_kinds: Vec<DecodeThreadKind> = spawner
        .kinds()
        .into_iter()
        .filter(|kind| {
            matches!(
                kind,
                DecodeThreadKind::Preview | DecodeThreadKind::Viewer | DecodeThreadKind::Original
            )
        })
        .collect();
    assert_eq!(decode_kinds, vec![DecodeThreadKind::Viewer, DecodeThreadKind::Preview]);
    let calls = decoder.calls();
    assert_eq!(calls.len(), 2);
    let viewer = calls
        .iter()
        .find(|call| call.name == "viewer.jpg")
        .expect("viewer decode");
    let preload = calls
        .iter()
        .find(|call| call.name == "preload.jpg")
        .expect("preload decode");
    assert_eq!(viewer.request.priority, SourceReadPriority::Viewer);
    assert_eq!(preload.request.priority, SourceReadPriority::Preload);
    let test_thread = std::thread::current().id();
    assert_ne!(viewer.thread, test_thread);
    assert_ne!(preload.thread, test_thread);
}

// C#: PreviewImageServiceMutationTests.DecodeOriginal_RunsOnDedicatedThread_AndContinuesElsewhere
#[test]
fn decode_original_runs_on_dedicated_thread_and_continues_elsewhere() {
    let root = TempRoot::new("decode-mut-original-thread");
    let decoder = TestDecoder::new();
    let spawner = RecordingSpawner::new();
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        config.spawner = Some(spawner.clone());
    });
    let path = source(&root, "a.jpg");
    let key = service.get_current_cache_key(&path).expect("key");

    let image = finish_ok(service.decode_original(&path, &key, &CancelToken::none()));
    let waiting_thread = std::thread::current().id();

    assert!(image.pixel_width() > 0);
    assert!(spawner.kinds().contains(&DecodeThreadKind::Original));
    let calls = decoder.calls();
    assert_eq!(calls.len(), 1);
    assert_ne!(calls[0].thread, waiting_thread);
}

// ---- PreviewImageServiceStartupKeyTests -----------------------------------------------------------------------------

const PREDICTED: DecodeBox = DecodeBox::new(1280, 768);
const OTHER: DecodeBox = DecodeBox::new(1920, 1152);

fn startup_service(
    root: &TempRoot,
    viewport: DecodeBox,
) -> (PreviewImageService, Arc<ReviewMetrics>, Arc<TestDecoder>) {
    let decoder = TestDecoder::new();
    decoder.gate_all();
    let (service, metrics) = service_with(root, decoder.clone(), |config| {
        config.target_decode_box = Arc::new(move || viewport);
    });
    (service, metrics, decoder)
}

// C#: PreviewImageServiceStartupKeyTests.GetViewerPreviewAsync_ViewportMatchesPredictedBox_JoinsTheEarlyDecode
#[test]
fn get_viewer_preview_viewport_matches_predicted_box_joins_the_early_decode() {
    let root = TempRoot::new("decode-startup-join");
    let path = source(&root, "images/launch.jpg");
    let (service, metrics, decoder) = startup_service(&root, PREDICTED);
    let predicted_key = service.get_current_cache_key_for_box(&path, PREDICTED).expect("key");
    let early = service.get_viewer_preview(&path, &predicted_key, &CancelToken::none(), Duration::ZERO);
    assert!(decoder.wait_calls(1));

    let current_key = service.get_current_cache_key(&path).expect("key");
    let presenter = service.get_viewer_preview(&path, &current_key, &CancelToken::none(), Duration::ZERO);
    assert_eq!(
        snapshot(&metrics).inflight_joins,
        1,
        "joined synchronously, before the early decode finished"
    );

    decoder.release_all();
    let first = finish_ok(early);
    let second = finish_ok(presenter);

    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(decoder.call_count(), 1);
}

// C#: PreviewImageServiceStartupKeyTests.GetViewerPreviewAsync_ViewportDiffersFromPredictedBox_DoesNotJoin
#[test]
fn get_viewer_preview_viewport_differs_from_predicted_box_does_not_join() {
    let root = TempRoot::new("decode-startup-differs");
    let path = source(&root, "images/launch.jpg");
    let (service, metrics, decoder) = startup_service(&root, OTHER);
    let predicted_key = service.get_current_cache_key_for_box(&path, PREDICTED).expect("key");
    let early = service.get_viewer_preview(&path, &predicted_key, &CancelToken::none(), Duration::ZERO);
    assert!(decoder.wait_calls(1));

    let current_key = service.get_current_cache_key(&path).expect("key");
    let presenter = service.get_viewer_preview(&path, &current_key, &CancelToken::none(), Duration::ZERO);
    assert_eq!(snapshot(&metrics).inflight_joins, 0);

    decoder.release_all();
    let first = finish_ok(early);
    let second = finish_ok(presenter);

    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(decoder.call_count(), 2);
    let cached_early = service.try_get_cached_preview(&predicted_key).expect("early entry");
    assert!(Arc::ptr_eq(&first, &cached_early));
    let cached_real = service.try_get_cached_preview(&current_key).expect("real entry");
    assert!(Arc::ptr_eq(&second, &cached_real));
}

// C#: PreviewImageServiceStartupKeyTests.GetViewerPreviewAsync_EarlyDecodeFinished_ViewportKeyHitsTheCache
#[test]
fn get_viewer_preview_early_decode_finished_viewport_key_hits_the_cache() {
    let root = TempRoot::new("decode-startup-hit");
    let path = source(&root, "images/launch.jpg");
    let (service, _, decoder) = startup_service(&root, PREDICTED);
    decoder.release_all();
    let predicted_key = service.get_current_cache_key_for_box(&path, PREDICTED).expect("key");
    let early = finish_ok(service.get_viewer_preview(&path, &predicted_key, &CancelToken::none(), Duration::ZERO));

    let cached = service
        .try_get_cached_preview(&service.get_current_cache_key(&path).expect("key"))
        .expect("a RAM hit");
    assert!(Arc::ptr_eq(&early, &cached));
    assert_eq!(decoder.call_count(), 1);
}

// ---- PreviewImageErrorLocalizationTests -----------------------------------------------------------------------------

/// The language catalog `assets/languages/<language>.json` as a JSON object.
fn catalog(language: &str) -> Value {
    let path = golden::repo_root()
        .join("assets")
        .join("languages")
        .join(format!("{language}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// The UI sentences (English, Vietnamese) of `error` with `{path}` filled in (the App's `UserFacingError` port does the
/// rendering in W8; here the catalog key and the catalog texts are pinned).
fn localized(error: &PreviewError) -> (String, String) {
    let (key, path) = error.catalog_key().expect("a localized error");
    let render = |language: &str| str_of(&catalog(language), key).replace("{path}", path);
    (render("en"), render("vi"))
}

fn localization_service(root: &TempRoot) -> (PreviewImageService, String, ImageCacheKey) {
    let path = source(root, "photo.png");
    let (service, _) = service_over(root, TestDecoder::new());
    let key = service.get_current_cache_key(&path).expect("key");
    // Rewritten by "another program" after the key was taken.
    touch_future(&path);
    (service, path, key)
}

// C#: PreviewImageErrorLocalizationTests.Preview_SourceChangedDuringDecode_IsLocalized
#[test]
fn preview_source_changed_during_decode_is_localized() {
    let root = TempRoot::new("decode-l10n-preview");
    let (service, path, key) = localization_service(&root);

    let error = finish_err(service.get_preview(&path, &key));

    assert!(error.is_io_exception());
    assert_eq!(
        error.catalog_key(),
        Some(("err.io.sourceChangedDuringDecode", path.as_str()))
    );
    let (english, vietnamese) = localized(&error);
    assert_eq!(
        english,
        format!("The image file changed while it was being decoded: {path}")
    );
    assert_eq!(vietnamese, format!("Tệp ảnh đã thay đổi trong lúc giải mã: {path}"));
    assert_eq!(error.to_string(), format!("Image source changed during decode: {path}"));
}

// C#: PreviewImageErrorLocalizationTests.Original_SourceChangedDuringDecode_IsLocalized
#[test]
fn original_source_changed_during_decode_is_localized() {
    let root = TempRoot::new("decode-l10n-original");
    let (service, path, key) = localization_service(&root);

    let error = finish_err(service.decode_original(&path, &key, &CancelToken::none()));

    assert!(error.is_io_exception());
    assert_eq!(
        error.catalog_key(),
        Some(("err.io.sourceChangedDuringDecode", path.as_str()))
    );
    let (english, vietnamese) = localized(&error);
    assert_eq!(
        english,
        format!("The image file changed while it was being decoded: {path}")
    );
    assert_eq!(vietnamese, format!("Tệp ảnh đã thay đổi trong lúc giải mã: {path}"));
    assert_eq!(error.to_string(), format!("Image source changed during decode: {path}"));
}

// C#: PreviewImageErrorLocalizationTests.OriginalDimensions_SourceChanged_IsLocalized
#[test]
fn original_dimensions_source_changed_is_localized() {
    let root = TempRoot::new("decode-l10n-dims");
    let (service, path, key) = localization_service(&root);

    let error = service
        .get_original_dimensions(&path, &key)
        .expect_err("the source changed");

    assert!(error.is_io_exception());
    assert_eq!(
        error.catalog_key(),
        Some(("err.io.sourceChangedReadingDimensions", path.as_str()))
    );
    let (english, vietnamese) = localized(&error);
    assert_eq!(
        english,
        format!("The image file changed while its size was being read: {path}")
    );
    assert_eq!(
        vietnamese,
        format!("Tệp ảnh đã thay đổi trong lúc đọc kích thước: {path}")
    );
    assert_eq!(
        error.to_string(),
        format!("Image source changed while reading dimensions: {path}")
    );
    assert_eq!(
        service.try_get_known_original_dimensions(&key),
        None,
        "nothing is stored"
    );
}

// ---- PreviewImageServiceBudgetLineGapTests --------------------------------------------------------------------------

// C#: PreviewImageServiceBudgetLineGapTests.DiagPreRead_UsesTheInjectedSourceReader
#[test]
fn diag_pre_read_uses_the_injected_source_reader() {
    let root = TempRoot::new("decode-diag-preread");
    let path = source(&root, "images/a.jpg");
    let reader = Arc::new(CountingReader::default());
    let decoder = TestDecoder::new();
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        config.source_reader = Some(reader.clone());
        config.diag = Some(Arc::new(DiagOptions::from_lookup(|name| {
            (name == PRE_READ_VAR).then(|| "1".to_owned())
        })));
    });

    finish_ok(service.get_preview_path(&path));

    assert_eq!(reader.opens.load(Ordering::SeqCst), 1);
    assert_eq!(
        *lock(&reader.buffer_sizes),
        vec![1024 * 1024],
        "C# OpenSource default buffer"
    );
    assert_eq!(DIAG_PRE_READ_BUFFER_SIZE, 1_048_576);
    let call = decoder.calls().pop().expect("one decode");
    assert_eq!(call.request.bytes.as_deref(), Some([1u8, 2, 3, 4].as_slice()));
    assert_eq!(call.request.priority, SourceReadPriority::Preload);
}

fn diag_service(root: &TempRoot, decoder: Arc<TestDecoder>, script: StreamScript) -> PreviewImageService {
    service_with(root, decoder, |config| {
        config.source_reader = Some(Arc::new(ScriptedReader { script }));
        config.diag = Some(Arc::new(DiagOptions::from_lookup(|name| {
            (name == PRE_READ_VAR).then(|| "1".to_owned())
        })));
    })
    .0
}

/// The diagnostic pre-read loops over short and interrupted reads until the buffer is full.
#[test]
fn diag_pre_read_collects_short_and_interrupted_reads() {
    let root = TempRoot::new("decode-diag-chunked");
    let bytes: Vec<u8> = (0..50u8).collect();
    let path = source_with(&root, "a.jpg", &bytes);
    let decoder = TestDecoder::new();
    let service = diag_service(&root, decoder.clone(), StreamScript::Chunked { chunk: 3 });

    finish_ok(service.get_preview_path(&path));

    assert_eq!(
        decoder.calls().pop().expect("one decode").request.bytes.as_deref(),
        Some(bytes.as_slice())
    );
}

/// A read error (other than `Interrupted`) fails the request with `Io`; nothing is decoded.
#[test]
fn diag_pre_read_read_error_is_an_io_error() {
    let root = TempRoot::new("decode-diag-read-error");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    let service = diag_service(&root, decoder.clone(), StreamScript::FailingRead);

    let error = finish_err(service.get_preview_path(&path));

    assert!(matches!(&error, PreviewError::Io(inner) if inner.kind() == io::ErrorKind::PermissionDenied));
    assert_eq!(decoder.call_count(), 0);
}

/// Open question 3: a stream longer than the .NET array limit is refused before any allocation.
#[test]
fn diag_pre_read_refuses_a_length_above_the_dotnet_array_limit() {
    let root = TempRoot::new("decode-diag-huge");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    let service = diag_service(&root, decoder.clone(), StreamScript::ClaimedLength(u64::MAX / 2));

    let error = finish_err(service.get_preview_path(&path));

    assert!(
        matches!(&error, PreviewError::Io(inner) if inner.kind() == io::ErrorKind::InvalidData),
        "{error:?}"
    );
    assert_eq!(decoder.call_count(), 0);
}

/// A source that shrank after the length was taken keeps zero bytes at the end (C# quirk).
#[test]
fn diag_pre_read_of_a_shrunk_stream_keeps_trailing_zeros() {
    let root = TempRoot::new("decode-diag-shrunk");
    let path = source_with(&root, "a.jpg", &[9u8; 8]);
    let decoder = TestDecoder::new();
    // Length claims 12 bytes, the stream only has 8.
    let service = diag_service(&root, decoder.clone(), StreamScript::ClaimedLength(12));

    finish_ok(service.get_preview_path(&path));

    assert_eq!(
        decoder.calls().pop().expect("decode").request.bytes.as_deref(),
        Some([0u8; 12].as_slice())
    );
}

/// Without the diagnostic switch nothing is pre-read and the reader is never opened by the service.
#[test]
fn diag_pre_read_off_reads_nothing_itself() {
    let root = TempRoot::new("decode-diag-preread-off");
    let path = source(&root, "images/a.jpg");
    let reader = Arc::new(CountingReader::default());
    let decoder = TestDecoder::new();
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        config.source_reader = Some(reader.clone());
        config.diag = Some(Arc::new(DiagOptions::from_lookup(|_| None)));
    });

    finish_ok(service.get_preview_path(&path));

    assert_eq!(reader.opens.load(Ordering::SeqCst), 0);
    assert!(decoder.calls().pop().expect("one decode").request.bytes.is_none());
}

/// RAW sources are never pre-read, neither by the source-bytes cache nor by the diagnostic switch (I-10).
#[test]
fn raw_source_is_never_pre_read() {
    let root = TempRoot::new("decode-raw-no-preread");
    let path = source(&root, "images/a.cr2");
    let reader = Arc::new(CountingReader::default());
    let decoder = TestDecoder::new();
    let cache = Arc::new(SourceBytesCache::new(100_000, PHYSICAL_32_GIB, None).expect("cache"));
    let (service, metrics) = service_with(&root, decoder.clone(), |config| {
        config.source_reader = Some(reader.clone());
        config.source_bytes_cache = Some(Arc::clone(&cache));
        config.diag = Some(Arc::new(DiagOptions::from_lookup(|name| {
            (name == PRE_READ_VAR).then(|| "1".to_owned())
        })));
    });

    finish_ok(service.get_preview_path(&path));

    assert_eq!(reader.opens.load(Ordering::SeqCst), 0);
    assert_eq!(cache.count(), 0);
    assert!(decoder.calls().pop().expect("one decode").request.bytes.is_none());
    assert_eq!(snapshot(&metrics).source_open_count, 1);
}

/// A file the source-bytes cache cannot hold falls through to the diagnostic pre-read (C# `else if`).
#[test]
fn too_large_for_the_byte_cache_falls_back_to_the_diag_pre_read() {
    let root = TempRoot::new("decode-diag-fallback");
    let path = source_with(&root, "images/a.jpg", &[7u8; 200]);
    let reader = Arc::new(CountingReader::default());
    let decoder = TestDecoder::new();
    let cache = Arc::new(SourceBytesCache::new(60, PHYSICAL_32_GIB, None).expect("cache"));
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        config.source_reader = Some(reader.clone());
        config.source_bytes_cache = Some(Arc::clone(&cache));
        config.diag = Some(Arc::new(DiagOptions::from_lookup(|name| {
            (name == PRE_READ_VAR).then(|| "1".to_owned())
        })));
    });

    finish_ok(service.get_preview_path(&path));

    assert_eq!(reader.opens.load(Ordering::SeqCst), 1);
    assert_eq!(cache.count(), 0);
    assert_eq!(
        decoder
            .calls()
            .pop()
            .expect("one decode")
            .request
            .bytes
            .map(|b| b.len()),
        Some(200)
    );
}

/// A diagnostic pre-read that cannot open the source fails the request with `Io` and decodes nothing.
#[test]
fn diag_pre_read_open_failure_is_an_io_error() {
    let root = TempRoot::new("decode-diag-open-fails");
    let path = source(&root, "images/a.jpg");
    let decoder = TestDecoder::new();
    let (service, metrics) = service_with(&root, decoder.clone(), |config| {
        config.diag = Some(Arc::new(DiagOptions::from_lookup(|name| {
            (name == PRE_READ_VAR).then(|| "1".to_owned())
        })));
    });
    let key = service.get_current_cache_key(&path).expect("key");
    std::fs::remove_file(&path).expect("delete the source");

    let error = finish_err(service.get_preview(&path, &key));

    assert!(matches!(error, PreviewError::Io(_)));
    assert_eq!(decoder.call_count(), 0);
    assert_eq!(
        snapshot(&metrics).source_open_count,
        0,
        "the open is recorded only after the pre-read"
    );
}

// ---- PreviewImageServiceRawKeyTests ---------------------------------------------------------------------------------

// C#: PreviewImageServiceRawKeyTests.GetOriginalDimensionsAsync_ForARawPathAlreadyDecoded_ReusesTheSeededDimensionsWithoutReadInfo
#[test]
fn get_original_dimensions_for_a_raw_path_already_decoded_reuses_the_seeded_dimensions_without_read_info() {
    let root = TempRoot::new("decode-raw-key");
    let raw = source(&root, "shot.cr2");
    let decoder = TestDecoder::with_spec(ImageSpec {
        width: 16,
        height: 12,
        ..ImageSpec::default()
    });
    let (service, _) = service_over(&root, decoder.clone());

    finish_ok(service.get_preview_path(&raw));
    let key = service.get_current_cache_key(&raw).expect("key");
    assert_eq!(key.source_kind(), ImageSourceKind::RAW_PREVIEW);

    let dimensions = service.get_original_dimensions(&raw, &key).expect("dimensions");

    assert_eq!(dimensions, (16, 12));
    assert_eq!(decoder.read_info_calls.load(Ordering::SeqCst), 0);
}

// ---- PreviewBackendIdentityTests ------------------------------------------------------------------------------------

// C#: PreviewBackendIdentityTests.ExplicitKeySnapshotsBackendAcrossBlockedDecode
#[test]
fn explicit_key_snapshots_backend_across_blocked_decode() {
    let root = TempRoot::new("decode-backend-snapshot");
    let path = source(&root, "source.bin");
    let current = Arc::new(Mutex::new(DecoderBackend::Wpf));
    let inner = TestDecoder::new();
    inner.gate_all();
    let factory_inner = inner.clone();
    let factory = RecordingFactory::new(move |_| factory_inner.clone() as Arc<dyn ImageDecoder>);
    let backend = Arc::clone(&current);
    let (service, _) = service_with(&root, TestDecoder::new(), |config| {
        config.current_backend = Some(Arc::new(move || *lock(&backend)));
        config.decoder_factory = Some(factory.clone());
    });
    let key = service.get_current_cache_key(&path).expect("key");

    *lock(&current) = DecoderBackend::WicDirect;
    let load = service.get_preview(&path, &key);
    assert!(inner.wait_calls(1));
    inner.release_all();
    let image = finish_ok(load);

    assert_eq!(key.backend(), DecoderBackend::Wpf);
    assert_eq!(image.actual_backend(), DecoderBackend::Wpf);
    assert_eq!(factory.created(), vec![DecoderBackend::Wpf]);
}

// C#: PreviewBackendIdentityTests.OriginalDimensionsUseTheBackendCapturedInTheirKey
#[test]
fn original_dimensions_use_the_backend_captured_in_their_key() {
    let root = TempRoot::new("decode-backend-dims");
    let path = source(&root, "source.bin");
    let calls = Arc::new(AtomicUsize::new(0));
    let factory = RecordingFactory::new(|_| {
        let decoder = TestDecoder::new();
        *lock(&decoder.info) = Some(ImageInfo::with_orientation(4, 3, 1));
        decoder as Arc<dyn ImageDecoder>
    });
    let counter = Arc::clone(&calls);
    let (service, _) = service_with(&root, TestDecoder::new(), |config| {
        config.current_backend = Some(Arc::new(move || {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                DecoderBackend::Wpf
            } else {
                DecoderBackend::WicDirect
            }
        }));
        config.decoder_factory = Some(factory.clone());
    });

    let dimensions = service
        .get_original_dimensions(&path, &service.get_current_cache_key(&path).expect("key"))
        .expect("dimensions");

    assert_eq!(dimensions, (4, 3));
    assert_eq!(factory.created(), vec![DecoderBackend::Wpf]);
}

// ---- PreviewDecodeBoxTests ------------------------------------------------------------------------------------------

// C#: PreviewDecodeBoxTests.PreviewBox_FlowsIntoKeyAndDecodeRequest
#[test]
fn preview_box_flows_into_key_and_decode_request() {
    let root = TempRoot::new("decode-box-flow");
    let path = source(&root, "source.bin");
    let decoder = TestDecoder::new();
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        config.target_decode_box = Arc::new(|| DecodeBox::new(2304, 1280));
    });

    let key = service.get_current_cache_key(&path).expect("key");
    finish_ok(service.get_preview(&path, &key));

    assert_eq!(key.target_box(), DecodeBox::new(2304, 1280));
    let calls = decoder.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].request.decode_box(), DecodeBox::new(2304, 1280));
    assert!(calls[0].request.apply_orientation);
    assert_eq!(calls[0].request.source_orientation, None);
    assert_eq!(calls[0].request.path, PathBuf::from(&path));
}

// C#: PreviewDecodeBoxTests.OriginalMode_IgnoresBox_DecodesFullSize
#[test]
fn original_mode_ignores_box_decodes_full_size() {
    let root = TempRoot::new("decode-box-original");
    let path = source(&root, "source.bin");
    let decoder = TestDecoder::new();
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        config.target_decode_box = Arc::new(|| DecodeBox::new(2304, 1280));
        config.is_original_loading_mode = Arc::new(|| true);
    });

    let key = service.get_current_cache_key(&path).expect("key");
    finish_ok(service.get_preview(&path, &key));

    assert!(key.target_box().is_unbounded());
    let calls = decoder.calls();
    assert_eq!(calls.len(), 1);
    assert!(!calls[0].request.is_downscale_requested());
}

// ---- PreviewDecodeEwmaTests -----------------------------------------------------------------------------------------

fn slow_decoder() -> Arc<TestDecoder> {
    let decoder = TestDecoder::new();
    *lock(&decoder.spin) = Duration::from_millis(3);
    decoder
}

// C#: PreviewDecodeEwmaTests.PreviewDecode_FeedsTheDecodeEwma
#[test]
fn preview_decode_feeds_the_decode_ewma() {
    let root = TempRoot::new("decode-ewma-preview");
    let path = source(&root, "photo.jpg");
    let (service, metrics) = service_over(&root, slow_decoder());

    finish_ok(service.get_preview_path(&path));

    assert!(
        metrics.decode_milliseconds_ewma() > 0.0,
        "a preview decode must be sampled"
    );
    assert!(snapshot(&metrics).decode_milliseconds >= 3);
}

// C#: PreviewDecodeEwmaTests.RawFullDecodeOnZoom_DoesNotFeedTheDecodeEwma
#[test]
fn raw_full_decode_on_zoom_does_not_feed_the_decode_ewma() {
    let root = TempRoot::new("decode-ewma-raw");
    let path = source(&root, "zoom.cr2");
    let raw: Arc<dyn ImageDecoder> = slow_decoder();
    let (service, metrics) = service_with(&root, slow_decoder(), |config| {
        config.raw_full_decoder = Some(raw);
        config.is_raw_full_decode_enabled = Some(Arc::new(|| true));
    });

    let key = service.get_current_cache_key(&path).expect("key");
    finish_ok(service.decode_original(&path, &key, &CancelToken::none()));

    assert_eq!(metrics.decode_milliseconds_ewma(), 0.0);
    assert!(snapshot(&metrics).source_reads >= 1, "the read itself is still counted");
}

/// The original decode of a standard file DOES feed the EWMA (`include_in_decode_ewma = kind != RAW_FULL_DECODE`).
#[test]
fn standard_original_decode_feeds_the_decode_ewma() {
    let root = TempRoot::new("decode-ewma-original");
    let path = source(&root, "zoom.jpg");
    let (service, metrics) = service_over(&root, slow_decoder());

    let key = service.get_current_cache_key(&path).expect("key");
    finish_ok(service.decode_original(&path, &key, &CancelToken::none()));

    assert!(metrics.decode_milliseconds_ewma() > 0.0);
    assert_eq!(snapshot(&metrics).source_reads, 1);
    assert_eq!(snapshot(&metrics).source_open_count, 1);
}

// ---- DecodeOriginalTests --------------------------------------------------------------------------------------------

// C#: DecodeOriginalTests.DecodeOriginal_ExifRotated_ReturnsFullSizeAfterOrientation_AndSkipsTheRamCache (fake decoder
// returning the rotated size; the real-JPEG variant is W4-11c)
#[test]
fn decode_original_exif_rotated_returns_full_size_after_orientation_and_skips_the_ram_cache() {
    let root = TempRoot::new("decode-original-rotated");
    let path = source(&root, "rotated.jpg");
    let decoder = TestDecoder::with_spec(ImageSpec {
        width: 100,
        height: 150,
        downscaled: None,
        original: Some((200, 300)),
        ..ImageSpec::default()
    });
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        config.target_decode_box = Arc::new(|| DecodeBox::new(100, 100));
    });
    let key = service.get_current_cache_key(&path).expect("key");
    let preview = finish_ok(service.get_preview(&path, &key));
    let cached_bytes = service.cache_bytes();
    *lock(&decoder.spec) = ImageSpec {
        width: 200,
        height: 300,
        downscaled: None,
        ..ImageSpec::default()
    };

    let original = finish_ok(service.decode_original(&path, &key, &CancelToken::none()));

    assert!(preview.downscaled());
    assert_eq!(original.pixel_width(), 200);
    assert_eq!(original.pixel_height(), 300);
    assert!(!original.downscaled());
    assert_eq!(service.cache_bytes(), cached_bytes);
    assert!(
        service
            .try_get_cached_preview(&ImageCacheKey::create_original(&key))
            .is_none()
    );
    assert_eq!(service.try_get_known_original_dimensions(&key), Some((200, 300)));
}

// C#: DecodeOriginalTests.DecodeOriginal_AlreadyCancelled_NeverStartsTheDecode
#[test]
fn decode_original_already_cancelled_never_starts_the_decode() {
    let root = TempRoot::new("decode-original-cancelled");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    let spawner = RecordingSpawner::new();
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        config.spawner = Some(spawner.clone());
    });
    let key = service.get_current_cache_key(&path).expect("key");
    let token = CancelToken::new();
    token.cancel();

    assert!(finish_err(service.decode_original(&path, &key, &token)).is_canceled());
    assert_eq!(decoder.call_count(), 0);
    assert!(!spawner.kinds().contains(&DecodeThreadKind::Original));
}

// C#: DecodeOriginalTests.DecodeOriginal_RequestsAnUnboundedBox
#[test]
fn decode_original_requests_an_unbounded_box() {
    let root = TempRoot::new("decode-original-unbounded");
    let path = source(&root, "b.jpg");
    let decoder = TestDecoder::new();
    let (service, metrics) = service_over(&root, decoder.clone());

    let key = service.get_current_cache_key(&path).expect("key");
    finish_ok(service.decode_original(&path, &key, &CancelToken::none()));

    let calls = decoder.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].request.decode_box().is_unbounded());
    assert_eq!(calls[0].request.priority, SourceReadPriority::Viewer);
    let snapshot = snapshot(&metrics);
    assert_eq!(
        (snapshot.cache_hits, snapshot.cache_misses),
        (0, 0),
        "an original decode is not a cache lookup"
    );
    assert_eq!(service.cache_count(), 0);
}

// C#: DecodeOriginalTests.DecodeOriginal_RawOnZoom_UsesFullDecoderAndSourceKindTwo
#[test]
fn decode_original_raw_on_zoom_uses_full_decoder_and_source_kind_two() {
    let root = TempRoot::new("decode-original-raw");
    let path = source(&root, "zoom.cr2");
    let preview_decoder = TestDecoder::new();
    let raw_decoder = TestDecoder::new();
    let raw: Arc<dyn ImageDecoder> = raw_decoder.clone();
    let (service, metrics) = service_with(&root, preview_decoder.clone(), |config| {
        config.raw_full_decoder = Some(raw);
        config.is_raw_full_decode_enabled = Some(Arc::new(|| true));
    });
    let key = service.get_current_cache_key(&path).expect("key");

    finish_ok(service.decode_original(&path, &key, &CancelToken::none()));

    assert_eq!(preview_decoder.call_count(), 0);
    let calls = raw_decoder.calls();
    assert_eq!(calls.len(), 1);
    // C# calls the override directly: unbounded box, default priority, no pre-read, no source-open record.
    assert!(calls[0].request.decode_box().is_unbounded());
    assert_eq!(calls[0].request.priority, SourceReadPriority::Viewer);
    assert!(calls[0].request.bytes.is_none());
    assert_eq!(snapshot(&metrics).source_open_count, 0);
    assert!(
        service
            .try_get_known_original_dimensions(&ImageCacheKey::create_original_with_kind(&key, 2))
            .is_some()
    );
    assert!(
        service
            .try_get_known_original_dimensions(&ImageCacheKey::create_original_with_kind(&key, 1))
            .is_none()
    );
}

// C#: DecodeOriginalTests.DecodeOriginal_RawFullDecodeDisabled_UsesConfiguredPreviewDecoder
#[test]
fn decode_original_raw_full_decode_disabled_uses_configured_preview_decoder() {
    let root = TempRoot::new("decode-original-raw-off");
    let path = source(&root, "never.nef");
    let preview_decoder = TestDecoder::new();
    let raw_decoder = TestDecoder::new();
    let raw: Arc<dyn ImageDecoder> = raw_decoder.clone();
    let (service, _) = service_with(&root, preview_decoder.clone(), |config| {
        config.raw_full_decoder = Some(raw);
        config.is_raw_full_decode_enabled = Some(Arc::new(|| false));
    });

    let key = service.get_current_cache_key(&path).expect("key");
    finish_ok(service.decode_original(&path, &key, &CancelToken::none()));

    assert_eq!(preview_decoder.call_count(), 1);
    assert_eq!(raw_decoder.call_count(), 0);
}

// C#: DecodeOriginalTests.DecodeOriginal_RunsAtMostOneFullResolutionDecodeAtATime_AndDropsCancelledWaiters
#[test]
fn decode_original_runs_at_most_one_full_resolution_decode_at_a_time_and_drops_cancelled_waiters() {
    let root = TempRoot::new("decode-original-gate");
    let first = source(&root, "g1.jpg");
    let second = source(&root, "g2.jpg");
    let decoder = TestDecoder::new();
    decoder.gate_all();
    let (service, _) = service_over(&root, decoder.clone());
    let token = CancelToken::new();

    let first_key = service.get_current_cache_key(&first).expect("key");
    let first_task = service.decode_original(&first, &first_key, &CancelToken::none());
    assert!(decoder.wait_started("g1.jpg"), "the first decode never started");
    let second_key = service.get_current_cache_key(&second).expect("key");
    let second_task = service.decode_original(&second, &second_key, &token);

    // Not a timing assertion: the second request is parked in the gate until it is cancelled.
    assert!(!second_task.is_ready());
    token.cancel();
    assert!(matches!(finish_err(second_task), PreviewError::Canceled));

    decoder.release_all();
    finish_ok(first_task);
    assert_eq!(decoder.call_count(), 1);
    assert_eq!(decoder.calls_for("g2.jpg"), 0);
}

/// A cancellable raw full decoder gets `decode_cancellable` with the caller's token (C# `ICancellableImageDecoder`).
#[test]
fn decode_original_raw_cancellable_decoder_gets_the_token() {
    let root = TempRoot::new("decode-original-raw-cancellable");
    let path = source(&root, "zoom.cr2");
    let raw = Arc::new(CancellableRawDecoder {
        plain: AtomicUsize::new(0),
        cancellable: AtomicUsize::new(0),
        saw_token_cancelled: AtomicBool::new(true),
    });
    let raw_decoder: Arc<dyn ImageDecoder> = raw.clone();
    let (service, metrics) = service_with(&root, TestDecoder::new(), |config| {
        config.raw_full_decoder = Some(raw_decoder);
        config.is_raw_full_decode_enabled = Some(Arc::new(|| true));
    });
    let key = service.get_current_cache_key(&path).expect("key");

    finish_ok(service.decode_original(&path, &key, &CancelToken::new()));

    assert_eq!(raw.cancellable.load(Ordering::SeqCst), 1);
    assert_eq!(raw.plain.load(Ordering::SeqCst), 0);
    assert!(!raw.saw_token_cancelled.load(Ordering::SeqCst));
    let snapshot = snapshot(&metrics);
    assert_eq!(snapshot.source_reads, 1);
    assert_eq!(
        snapshot.source_bytes_read, 4,
        "the key length (the image reports no read)"
    );
}

/// An original decode that the source-change check refuses seeds nothing and records no read (INV-1).
#[test]
fn decode_original_source_changed_seeds_nothing() {
    let root = TempRoot::new("decode-original-changed");
    let path = source(&root, "a.jpg");
    let (service, metrics) = service_over(&root, TestDecoder::new());
    let key = service.get_current_cache_key(&path).expect("key");
    touch_future(&path);

    let error = finish_err(service.decode_original(&path, &key, &CancelToken::none()));

    assert!(matches!(
        error,
        PreviewError::SourceChanged {
            phase: SourceChangePhase::Decode,
            ..
        }
    ));
    assert_eq!(service.try_get_known_original_dimensions(&key), None);
    assert_eq!(snapshot(&metrics).source_reads, 0);
    assert_eq!(snapshot(&metrics).source_open_count, 1);
}

// ---- OriginalDimensionsCacheTests -----------------------------------------------------------------------------------

// C#: OriginalDimensionsCacheTests.GetOriginalDimensions_ManyDistinctKeys_StaysWithinCapacity
#[test]
fn get_original_dimensions_many_distinct_keys_stays_within_capacity() {
    let root = TempRoot::new("decode-dims-capacity");
    let decoder = TestDecoder::new();
    *lock(&decoder.info) = Some(ImageInfo::new(7, 5));
    let (service, _) = service_with(&root, decoder, |config| {
        config.original_dimensions_capacity = 100;
    });

    let mut last = String::new();
    for i in 0..300 {
        last = source(&root, &format!("img{i}.jpg"));
        let key = service.get_current_cache_key(&last).expect("key");
        assert_eq!(
            service.get_original_dimensions(&last, &key).expect("dimensions"),
            (7, 5)
        );
    }

    assert!(
        service
            .try_get_known_original_dimensions(&service.get_current_cache_key(&last).expect("key"))
            .is_some()
    );
    let oldest = root.join("img0.jpg").to_string_lossy().into_owned();
    assert!(
        service
            .try_get_known_original_dimensions(&service.get_current_cache_key(&oldest).expect("key"))
            .is_none()
    );
}

// ---- PreviewDecodeRawFullKeyGuardTests ------------------------------------------------------------------------------

// C#: PreviewDecodeRawFullKeyGuardTests.GetPreviewAsync_RawFullDecodeKey_ThrowsAndRecordsNoSourceRead
#[test]
fn get_preview_raw_full_decode_key_throws_and_records_no_source_read() {
    let root = TempRoot::new("decode-raw-full-guard");
    let path = source(&root, "images/a.png");
    let decoder = TestDecoder::new();
    let (service, metrics) = service_over(&root, decoder.clone());
    let key = ImageCacheKey::create(
        &PhysicalFileSystem,
        &path,
        false,
        DecodeBox::new(512, 0),
        KeyOptions {
            source_kind: ImageSourceKind::RAW_FULL_DECODE,
            ..KeyOptions::default()
        },
    )
    .expect("key");

    let error = finish_err(service.get_preview(&path, &key));

    assert!(matches!(error, PreviewError::RawFullDecodeKey));
    assert_eq!(error.to_string(), PreviewError::RAW_FULL_DECODE_KEY_MESSAGE);
    let snapshot = snapshot(&metrics);
    assert_eq!(snapshot.source_reads, 0);
    assert_eq!(snapshot.source_open_count, 0);
    assert_eq!(decoder.call_count(), 0);
}

// ---- ViewerDecodePriorityTests --------------------------------------------------------------------------------------

struct ViewerFixture {
    root: TempRoot,
    decoder: Arc<TestDecoder>,
    service: PreviewImageService,
    spawner: Arc<RecordingSpawner>,
    paths: Vec<String>,
}

impl ViewerFixture {
    fn new(label: &str) -> Self {
        let root = TempRoot::new(label);
        let paths = (0..4u8)
            .map(|i| source_with(&root, &format!("images/v{i}.jpg"), &[1, 2, 3, i]))
            .collect();
        let decoder = TestDecoder::new();
        decoder.gate_all();
        let spawner = RecordingSpawner::new();
        let (service, _) = service_with(&root, decoder.clone(), |config| {
            config.spawner = Some(spawner.clone());
        });
        Self {
            root,
            decoder,
            service,
            spawner,
            paths,
        }
    }

    fn key(&self, i: usize) -> ImageCacheKey {
        self.service.get_current_cache_key(&self.paths[i]).expect("key")
    }

    fn viewer(&self, i: usize, token: &CancelToken, delay: Duration) -> PreviewHandle {
        self.service
            .get_viewer_preview(&self.paths[i], &self.key(i), token, delay)
    }
}

impl Drop for ViewerFixture {
    fn drop(&mut self) {
        self.decoder.release_all();
        let _ = &self.root;
    }
}

// C#: ViewerDecodePriorityTests.ViewerDecode_RunsOnDedicatedPriorityThread (the priority itself is the pr_platform_win
// spawner's native test)
#[test]
fn viewer_decode_runs_on_dedicated_priority_thread() {
    let fixture = ViewerFixture::new("decode-viewer-thread");
    fixture.decoder.release_all();

    finish_ok(fixture.viewer(0, &CancelToken::none(), Duration::ZERO));

    let calls = fixture.decoder.calls();
    assert_eq!(calls.len(), 1);
    assert_ne!(calls[0].thread, std::thread::current().id());
    assert!(fixture.spawner.kinds().contains(&DecodeThreadKind::Viewer));
    assert!(!fixture.spawner.kinds().contains(&DecodeThreadKind::Preview));
}

// C#: ViewerDecodePriorityTests.SupersededViewerDecode_WaitingForSlot_IsNeverStarted
#[test]
fn superseded_viewer_decode_waiting_for_slot_is_never_started() {
    let fixture = ViewerFixture::new("decode-viewer-slot");
    let a = fixture.viewer(0, &CancelToken::none(), Duration::ZERO);
    let b = fixture.viewer(1, &CancelToken::none(), Duration::ZERO);
    assert!(fixture.decoder.wait_calls(2), "both viewer slots busy");
    let superseded = CancelToken::new();
    let c = fixture.viewer(2, &superseded, Duration::ZERO);

    superseded.cancel();

    assert!(matches!(finish_err(c), PreviewError::Canceled));
    fixture.decoder.release_all();
    finish_ok(a);
    finish_ok(b);
    assert_eq!(fixture.decoder.calls_for("v2.jpg"), 0);
    assert!(!fixture.service.has_inflight_preview(&fixture.key(2)));
    assert_eq!(fixture.service.active_viewer_decodes(), 0);
}

// C#: ViewerDecodePriorityTests.SupersededViewerDecode_DuringStartDelay_IsNeverStarted
#[test]
fn superseded_viewer_decode_during_start_delay_is_never_started() {
    let fixture = ViewerFixture::new("decode-viewer-delay");
    fixture.decoder.release_all();
    let superseded = CancelToken::new();
    let pending = fixture.viewer(0, &superseded, Duration::from_secs(30));

    superseded.cancel();

    assert!(matches!(finish_err(pending), PreviewError::Canceled));
    assert_eq!(fixture.decoder.call_count(), 0);
}

// C#: ViewerDecodePriorityTests.PreloadJoiningDroppedViewerDecode_StartsItsOwn
#[test]
fn preload_joining_dropped_viewer_decode_starts_its_own() {
    let fixture = ViewerFixture::new("decode-viewer-dropped");
    fixture.decoder.release_all();
    let superseded = CancelToken::new();
    let viewer = fixture.viewer(0, &superseded, Duration::from_secs(30));
    assert!(fixture.service.has_inflight_preview(&fixture.key(0)));
    let preload = fixture.service.get_preview(&fixture.paths[0], &fixture.key(0)); // joins the pending viewer decode

    superseded.cancel();

    finish_ok(preload);
    assert!(finish_err(viewer).is_canceled());
    assert_eq!(fixture.decoder.calls_for("v0.jpg"), 1);
    let calls = fixture.decoder.calls();
    assert_eq!(
        calls[0].request.priority,
        SourceReadPriority::Preload,
        "the preload decoded on its own lane"
    );
}

// C#: ViewerDecodePriorityTests.StartedViewerDecode_IsKeptAfterSupersede
#[test]
fn started_viewer_decode_is_kept_after_supersede() {
    let fixture = ViewerFixture::new("decode-viewer-kept");
    let superseded = CancelToken::new();
    let viewer = fixture.viewer(0, &superseded, Duration::ZERO);
    assert!(fixture.decoder.wait_calls(1));
    assert_eq!(fixture.service.active_viewer_decodes(), 1);

    superseded.cancel();
    fixture.decoder.release_all();

    finish_ok(viewer);
    assert!(fixture.service.try_get_cached_preview(&fixture.key(0)).is_some());
    assert_eq!(fixture.service.active_viewer_decodes(), 0);
}

// ---- PreviewImageServiceTests ---------------------------------------------------------------------------------------

// C#: PreviewImageServiceTests.PreviewDecodeRecordsExactlyOneSourceRead
#[test]
fn preview_decode_records_exactly_one_source_read() {
    let root = TempRoot::new("decode-pis-one-read");
    let path = source_with(&root, "preview-service/preview-a.png", &[5u8; 321]);
    let (service, metrics) = service_over(&root, TestDecoder::new());

    let decoded = finish_ok(service.get_preview_path(&path));

    let snapshot = snapshot(&metrics);
    assert!(decoded.pixel_width() > 0);
    assert_eq!(snapshot.cache_misses, 1);
    assert_eq!(snapshot.cache_hits, 0);
    assert_eq!(snapshot.source_reads, 1);
    assert_eq!(snapshot.source_bytes_read, 321);
    assert_eq!(service.cache_count(), 1);
    assert!(service.cache_bytes() > 0);
}

// C#: PreviewImageServiceTests.ConcurrentRequestsForSameKeyDecodeOnce
#[test]
fn concurrent_requests_for_same_key_decode_once() {
    let root = TempRoot::new("decode-pis-concurrent");
    let path = source(&root, "preview-service/preview-a.png");
    let (service, metrics) = service_over(&root, TestDecoder::new());
    let service = Arc::new(service);
    let barrier = Arc::new(Barrier::new(16));

    let threads: Vec<_> = (0..16)
        .map(|_| {
            let service = Arc::clone(&service);
            let barrier = Arc::clone(&barrier);
            let path = path.clone();
            std::thread::spawn(move || {
                barrier.wait();
                finish_ok(service.get_preview_path(&path))
            })
        })
        .collect();
    let results: Vec<PreviewImage> = threads
        .into_iter()
        .map(|thread| thread.join().expect("request thread"))
        .collect();

    assert_eq!(snapshot(&metrics).source_reads, 1);
    assert!(results.iter().all(|image| Arc::ptr_eq(image, &results[0])));
}

// C#: PreviewImageServiceTests.SourceOpenIsRecordedOncePerSourceDecode
#[test]
fn source_open_is_recorded_once_per_source_decode() {
    let root = TempRoot::new("decode-pis-open");
    let path = source(&root, "preview-service/preview-a.png");
    let (service, metrics) = service_over(&root, TestDecoder::new());

    finish_ok(service.get_preview_path(&path));
    finish_ok(service.get_preview_path(&path));

    let snapshot = snapshot(&metrics);
    assert_eq!(snapshot.source_open_count, 1);
    assert_eq!(snapshot.top_source_opens.len(), 1);
    assert_eq!(snapshot.top_source_opens[0].path, path);
    assert_eq!(snapshot.top_source_opens[0].count, 1);
}

// C#: PreviewImageServiceTests.SourceByteMetricsExcludeCacheDeliveries
#[test]
fn source_byte_metrics_exclude_cache_deliveries() {
    let root = TempRoot::new("decode-pis-bytes");
    let path = source(&root, "preview-service/preview-a.png");
    let (service, metrics) = service_over(&root, TestDecoder::new());

    let decoded = finish_ok(service.get_preview_path(&path));
    let after_first = snapshot(&metrics);
    let second = finish_ok(service.get_preview_path(&path));
    let after_hit = snapshot(&metrics);

    assert_eq!(after_hit.cache_hits, 1);
    assert_eq!(after_hit.source_reads, 1);
    assert_eq!(after_hit.source_bytes_read, after_first.source_bytes_read);
    assert!(Arc::ptr_eq(&second, &decoded));
}

// C#: PreviewImageServiceTests.EvictionForcesFreshSourceReadOnNextRequest
#[test]
fn eviction_forces_fresh_source_read_on_next_request() {
    let root = TempRoot::new("decode-pis-evict");
    let path = source(&root, "preview-service/preview-a.png");
    let (service, metrics) = service_with(&root, TestDecoder::new(), |config| {
        config.is_original_loading_mode = Arc::new(|| true);
    });

    finish_ok(service.get_preview_path(&path));
    service.evict_cached_path(&path, None).expect("evict");
    finish_ok(service.get_preview_path(&path));

    assert_eq!(snapshot(&metrics).source_reads, 2);
    assert_eq!(service.cache_count(), 1);
}

// C#: PreviewImageServiceTests.OriginalDimensionsAreReadFromRealSourceHeader (fake `read_info`; the real-PNG variant is
// W4-11c)
#[test]
fn original_dimensions_are_read_from_real_source_header() {
    let root = TempRoot::new("decode-pis-header");
    let path = source(&root, "preview-service/preview-a.png");
    let decoder = TestDecoder::new();
    *lock(&decoder.info) = Some(ImageInfo::new(1, 1));
    let (service, metrics) = service_over(&root, decoder.clone());

    let key = service.get_current_cache_key(&path).expect("key");
    let dimensions = service.get_original_dimensions(&path, &key).expect("dimensions");

    assert_eq!(dimensions, (1, 1));
    assert_eq!(decoder.read_info_calls.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot(&metrics).source_open_count, 1);
    // Stored: the second query needs no read.
    assert_eq!(
        service.get_original_dimensions(&path, &key).expect("dimensions"),
        (1, 1)
    );
    assert_eq!(decoder.read_info_calls.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot(&metrics).source_open_count, 1);
}

/// `read_info` reports the displayed size (orientation 5..=8 swaps width and height).
#[test]
fn original_dimensions_are_the_displayed_size() {
    let root = TempRoot::new("decode-pis-header-rotated");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    *lock(&decoder.info) = Some(ImageInfo::with_orientation(300, 200, 6));
    let (service, _) = service_over(&root, decoder);

    let key = service.get_current_cache_key(&path).expect("key");
    assert_eq!(
        service.get_original_dimensions(&path, &key).expect("dimensions"),
        (200, 300)
    );
}

/// A failed `read_info` is `Decode` and stores nothing.
#[test]
fn original_dimensions_read_info_failure_stores_nothing() {
    let root = TempRoot::new("decode-pis-header-fails");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    *lock(&decoder.info) = None;
    let (service, _) = service_over(&root, decoder);

    let key = service.get_current_cache_key(&path).expect("key");
    let error = service
        .get_original_dimensions(&path, &key)
        .expect_err("read_info fails");

    assert!(matches!(&error, PreviewError::Decode(inner) if inner.is_io_exception()));
    assert_eq!(service.try_get_known_original_dimensions(&key), None);
}

// C#: PreviewImageServiceTests.GetOriginalDimensionsAsyncAfterDecodeNeedsNoExtraSourceOpen
#[test]
fn get_original_dimensions_after_decode_needs_no_extra_source_open() {
    let root = TempRoot::new("decode-pis-dims-seeded");
    let path = source(&root, "preview-service/preview-a.png");
    let decoder = TestDecoder::with_spec(ImageSpec {
        width: 64,
        height: 48,
        downscaled: Some(true),
        original: Some((640, 480)),
        ..ImageSpec::default()
    });
    let (service, metrics) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");
    let decoded = finish_ok(service.get_preview(&path, &key));
    let opens_after_decode = snapshot(&metrics).source_open_count;

    let dimensions = service.get_original_dimensions(&path, &key).expect("dimensions");

    assert_eq!(snapshot(&metrics).source_open_count, opens_after_decode);
    assert_eq!(dimensions, (decoded.original_width(), decoded.original_height()));
    assert_eq!(dimensions, (640, 480));
    assert_eq!(decoder.read_info_calls.load(Ordering::SeqCst), 0);
}

/// A downscaled decode whose original size is unknown (it reports its own small size) seeds nothing.
#[test]
fn downscaled_decode_without_a_known_original_seeds_nothing() {
    let root = TempRoot::new("decode-pis-dims-unknown");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::with_spec(ImageSpec {
        width: 64,
        height: 48,
        downscaled: Some(true),
        ..ImageSpec::default()
    });
    let (service, _) = service_over(&root, decoder);
    let key = service.get_current_cache_key(&path).expect("key");

    finish_ok(service.get_preview(&path, &key));

    assert_eq!(service.try_get_known_original_dimensions(&key), None);
}

// ---- FactoryTests ---------------------------------------------------------------------------------------------------

// C#: FactoryTests.PreviewImageServiceIntegratesDecoderFactory (a counting factory: `create` once per backend, the
// per-backend cache reused)
#[test]
fn preview_image_service_integrates_decoder_factory() {
    let root = TempRoot::new("decode-factory");
    let path = source(&root, "valid.png");
    let other = source(&root, "other.png");
    let inner = FakeDecoder::new();
    let factory = CountingFactory::new(inner.clone());
    let backend = Arc::new(Mutex::new(DecoderBackend::Wpf));
    let current = Arc::clone(&backend);
    let (service, _) = service_with(&root, FakeDecoder::new(), |config| {
        config.decoder = None;
        config.decoder_factory = Some(factory.clone());
        config.current_backend = Some(Arc::new(move || *lock(&current)));
        config.is_original_loading_mode = Arc::new(|| true);
    });
    assert_eq!(
        factory.creates(),
        1,
        "the constructor builds the current backend's decoder"
    );

    let key = service.get_current_cache_key(&path).expect("key");
    assert_eq!(key.backend(), DecoderBackend::Wpf);
    let preview = finish_ok(service.get_preview_path(&path));
    assert_eq!((preview.pixel_width(), preview.pixel_height()), (4, 3));
    assert_eq!(factory.creates(), 2, "the per-backend cache is filled from the factory");

    finish_ok(service.get_preview_path(&other));
    assert_eq!(factory.creates(), 2, "the per-backend cache is reused");

    *lock(&backend) = DecoderBackend::WicDirect;
    service.clear_cache();
    finish_ok(service.get_preview_path(&path));
    assert_eq!(factory.creates(), 3, "a new backend is built once");
    assert_eq!(inner.calls(), 3);
}

/// Without a factory the configured decoder serves every backend.
#[test]
fn without_a_factory_the_decoder_serves_every_backend() {
    let root = TempRoot::new("decode-no-factory");
    let path = source(&root, "a.png");
    let decoder = TestDecoder::new();
    let (service, _) = service_with(&root, decoder.clone(), |config| {
        config.current_backend = Some(Arc::new(|| DecoderBackend::TurboJpeg));
    });

    finish_ok(service.get_preview_path(&path));
    assert_eq!(decoder.call_count(), 1);
}

// ---- in-flight table, handles and threads (new Rust tests) ----------------------------------------------------------

#[test]
fn dropped_handle_still_completes_and_clears_the_inflight_entry() {
    let root = TempRoot::new("decode-dropped-handle");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    decoder.gate_all();
    let (service, metrics) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    drop(service.get_preview(&path, &key));
    assert!(service.has_inflight_preview(&key));
    assert!(decoder.wait_calls(1));
    decoder.release_all();

    // The abandoned load still publishes: a later request joins it or hits the cache, never decodes again.
    let image = finish_ok(service.get_preview(&path, &key));
    assert!(
        service
            .try_get_cached_preview(&key)
            .is_some_and(|cached| Arc::ptr_eq(&cached, &image))
    );
    assert!(!service.has_inflight_preview(&key));
    assert_eq!(decoder.call_count(), 1);
    assert_eq!(snapshot(&metrics).source_reads, 1);
}

#[test]
fn load_panic_becomes_panicked_error_for_every_waiter() {
    let root = TempRoot::new("decode-panic");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    decoder.gate_all();
    decoder.panics.store(true, Ordering::SeqCst);
    let (service, _) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    let creator = service.get_preview(&path, &key);
    assert!(decoder.wait_calls(1));
    let joiner = service.get_preview(&path, &key);
    decoder.release_all();

    for error in [finish_err(creator), finish_err(joiner)] {
        match error {
            PreviewError::Panicked(text) => assert_eq!(text, "test decoder panic"),
            other => panic!("expected Panicked, got {other:?}"),
        }
    }
    assert!(!service.has_inflight_preview(&key));
    assert_eq!(decoder.call_count(), 1, "a panic is not retried");
}

#[test]
fn panicking_viewer_decode_still_returns_its_slot_and_counter() {
    let root = TempRoot::new("decode-panic-viewer");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    decoder.panics.store(true, Ordering::SeqCst);
    let (service, _) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    for _ in 0..3 {
        assert!(matches!(
            finish_err(service.get_viewer_preview(&path, &key, &CancelToken::none(), Duration::ZERO)),
            PreviewError::Panicked(_)
        ));
    }
    assert_eq!(service.active_viewer_decodes(), 0);
    decoder.panics.store(false, Ordering::SeqCst);
    finish_ok(service.get_viewer_preview(&path, &key, &CancelToken::none(), Duration::ZERO));
}

#[test]
fn spawn_failure_is_reported_and_leaves_no_inflight_entry() {
    let root = TempRoot::new("decode-spawn-failure");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    let (service, metrics) = service_with(&root, decoder.clone(), |config| {
        config.spawner = Some(Arc::new(RefusingSpawner {
            refused: vec![
                DecodeThreadKind::Preview,
                DecodeThreadKind::Viewer,
                DecodeThreadKind::Original,
            ],
        }));
    });
    let key = service.get_current_cache_key(&path).expect("key");

    let handle = service.get_preview(&path, &key);
    assert!(handle.is_ready(), "a refused start fails at once");
    assert!(matches!(finish_err(handle), PreviewError::Spawn(_)));
    assert!(!service.has_inflight_preview(&key));
    assert_eq!(snapshot(&metrics).cache_misses, 1);

    let viewer = service.get_viewer_preview(&path, &key, &CancelToken::none(), Duration::ZERO);
    assert!(matches!(finish_err(viewer), PreviewError::Spawn(_)));
    let original = service.decode_original(&path, &key, &CancelToken::none());
    assert!(original.is_ready());
    assert!(matches!(finish_err(original), PreviewError::Spawn(_)));
    assert!(!service.has_inflight_preview(&key));
    assert_eq!(decoder.call_count(), 0);
}

#[test]
fn slot_gate_cancel_while_waiting_returns_canceled_within_the_slice() {
    let fixture = ViewerFixture::new("decode-slot-cancel");
    let a = fixture.viewer(0, &CancelToken::none(), Duration::ZERO);
    let b = fixture.viewer(1, &CancelToken::none(), Duration::ZERO);
    assert!(fixture.decoder.wait_calls(2));
    let token = CancelToken::new();
    let waiting = fixture.viewer(2, &token, Duration::ZERO);
    assert!(!waiting.is_ready());

    token.cancel();
    let cancelled_at = Instant::now();
    let error = finish_err(waiting);
    // Generous bound (thread scheduling): the poll slice is 10 ms; a missed cancellation would take the 10 s guard.
    assert!(cancelled_at.elapsed() < CANCEL_POLL_SLICE * 200);
    assert!(matches!(error, PreviewError::Canceled));

    fixture.decoder.release_all();
    finish_ok(a);
    finish_ok(b);
}

#[test]
fn perf_nav_context_flows_to_the_load_thread() {
    let root = TempRoot::new("decode-perf-nav");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    decoder.gate_all();
    let (service, _) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");
    let recorder = Arc::new(PerfRecorder::default());
    let _subscription = PhotoReviewPerf::log().subscribe(recorder.clone());
    let path_id = perf::path_id(&path);

    PhotoReviewPerf::set_nav_context(4242);
    let creator = service.get_preview(&path, &key);
    assert!(decoder.wait_calls(1));
    PhotoReviewPerf::set_nav_context(77);
    let joiner = service.get_preview(&path, &key);
    PhotoReviewPerf::set_nav_context(0);
    decoder.release_all();
    finish_ok(creator);
    finish_ok(joiner);

    let decode = recorder.for_path("Decode", &path_id);
    assert_eq!(decode.len(), 1);
    assert_eq!(
        decode[0].payload[0],
        PerfValue::Int(4242),
        "the creator's nav reached the load thread"
    );
    assert_eq!(decode[0].payload[3], PerfValue::Int(512));
    assert_eq!(decode[0].payload[4], PerfValue::Bool(false));
    assert_eq!(
        decode[0].payload[5],
        PerfValue::Bool(true),
        "a bounded request that did not downscale fell back"
    );
    let verify = recorder.for_path("Verify", &path_id);
    assert_eq!(verify.len(), 1);
    assert_eq!(verify[0].payload[0], PerfValue::Int(4242));
    let join_start = recorder.for_path("JoinStart", &path_id);
    let join_end = recorder.for_path("JoinEnd", &path_id);
    assert_eq!(join_start.len(), 1);
    assert_eq!(join_end.len(), 1);
    assert_eq!(
        join_start[0].payload[0],
        PerfValue::Int(77),
        "a joiner traces under its own nav"
    );
    assert_eq!(join_end[0].payload[0], PerfValue::Int(77));
}

/// The `fallback` flag of the perf `Decode` event is `bounded box && !downscaled`: false for an unbounded (original)
/// decode and for a downscaled one.
#[test]
fn perf_decode_fallback_flag_needs_a_bounded_box_and_no_downscale() {
    let root = TempRoot::new("decode-perf-fallback");
    let original_path = source(&root, "original.jpg");
    let downscaled_path = source(&root, "downscaled.jpg");
    let decoder = TestDecoder::with_spec(ImageSpec {
        downscaled: None,
        ..ImageSpec::default()
    });
    let (service, _) = service_over(&root, decoder);
    let recorder = Arc::new(PerfRecorder::default());
    let _subscription = PhotoReviewPerf::log().subscribe(recorder.clone());

    let key = service.get_current_cache_key(&original_path).expect("key");
    finish_ok(service.decode_original(&original_path, &key, &CancelToken::none()));
    finish_ok(service.get_preview_path(&downscaled_path));

    let original = recorder.for_path("Decode", &perf::path_id(&original_path));
    assert_eq!(original.len(), 1);
    assert_eq!(original[0].payload[3], PerfValue::Int(0));
    assert_eq!(original[0].payload[4], PerfValue::Bool(false));
    assert_eq!(
        original[0].payload[5],
        PerfValue::Bool(false),
        "an unbounded decode is never a fallback"
    );
    let downscaled = recorder.for_path("Decode", &perf::path_id(&downscaled_path));
    assert_eq!(downscaled.len(), 1);
    assert_eq!(downscaled[0].payload[4], PerfValue::Bool(true));
    assert_eq!(downscaled[0].payload[5], PerfValue::Bool(false));
}

/// A positive start delay is waited out before the decode starts (lower bound only: no timing upper bound).
#[test]
fn viewer_preview_positive_start_delay_waits_before_decoding() {
    let root = TempRoot::new("decode-delay-lower-bound");
    let path = source(&root, "a.jpg");
    let (service, _) = service_over(&root, TestDecoder::new());
    let key = service.get_current_cache_key(&path).expect("key");

    let start = Instant::now();
    finish_ok(service.get_viewer_preview(&path, &key, &CancelToken::none(), Duration::from_millis(60)));

    assert!(start.elapsed() >= Duration::from_millis(60));
}

#[test]
fn preview_handle_debug_names_its_state() {
    let root = TempRoot::new("decode-handle-debug");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    decoder.gate_all();
    let (service, _) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    let pending = service.get_preview(&path, &key);
    assert_eq!(format!("{pending:?}"), "PreviewHandle { state: \"load\" }");
    decoder.release_all();
    finish_ok(pending);
    let hit = service.get_preview(&path, &key);
    assert_eq!(format!("{hit:?}"), "PreviewHandle { state: \"ready\" }");
}

#[test]
fn perf_source_read_is_traced_by_the_diag_pre_read() {
    let root = TempRoot::new("decode-perf-source-read");
    let path = source_with(&root, "a.jpg", &[3u8; 77]);
    let (service, _) = service_with(&root, TestDecoder::new(), |config| {
        config.diag = Some(Arc::new(DiagOptions::from_lookup(|name| {
            (name == PRE_READ_VAR).then(|| "1".to_owned())
        })));
    });
    let recorder = Arc::new(PerfRecorder::default());
    let _subscription = PhotoReviewPerf::log().subscribe(recorder.clone());

    finish_ok(service.get_preview_path(&path));

    let reads = recorder.for_path("SourceRead", &perf::path_id(&path));
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].payload[3], PerfValue::Int(77));
}

#[test]
fn wait_timeout_returns_the_handle_when_pending() {
    let root = TempRoot::new("decode-wait-timeout");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    decoder.gate_all();
    let (service, _) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    let handle = service.get_preview(&path, &key);
    assert!(!handle.is_ready());
    let handle = match handle.wait_timeout(Duration::from_millis(1)) {
        Ok(_) => panic!("a blocked decode cannot finish"),
        Err(handle) => handle,
    };
    decoder.release_all();
    finish_ok(handle);
}

#[test]
fn joiner_with_its_own_token_cancelled_does_not_retry() {
    let fixture = ViewerFixture::new("decode-joiner-own-cancel");
    let a = fixture.viewer(0, &CancelToken::none(), Duration::ZERO);
    let b = fixture.viewer(1, &CancelToken::none(), Duration::ZERO);
    assert!(fixture.decoder.wait_calls(2));
    let token = CancelToken::new();
    let creator = fixture.viewer(2, &token, Duration::ZERO);
    let joiner = fixture.viewer(2, &token, Duration::ZERO);

    token.cancel();

    assert!(matches!(finish_err(creator), PreviewError::Canceled));
    assert!(matches!(finish_err(joiner), PreviewError::Canceled));
    fixture.decoder.release_all();
    finish_ok(a);
    finish_ok(b);
    assert_eq!(fixture.decoder.calls_for("v2.jpg"), 0);
}

#[test]
fn joiner_of_a_failed_decode_gets_the_same_error_without_retrying() {
    let root = TempRoot::new("decode-joiner-failure");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    decoder.gate_all();
    decoder.fail_next(DecodeError::InvalidData {
        message: "test: invalid data".to_owned(),
    });
    let (service, metrics) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    let creator = service.get_preview(&path, &key);
    assert!(decoder.wait_calls(1));
    let joiner = service.get_preview(&path, &key);
    decoder.release_all();

    for error in [finish_err(creator), finish_err(joiner)] {
        assert!(matches!(&error, PreviewError::Decode(inner) if matches!(**inner, DecodeError::InvalidData { .. })));
    }
    assert_eq!(decoder.call_count(), 1);
    assert_eq!(
        snapshot(&metrics).source_reads,
        0,
        "a failed decode records no source read"
    );
    assert_eq!(
        snapshot(&metrics).source_open_count,
        1,
        "the open is recorded even when the decode fails"
    );
}

#[test]
fn preload_with_a_cancelled_token_is_refused_and_a_live_one_is_not_passed_on() {
    let root = TempRoot::new("decode-preload");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    decoder.gate_all();
    let (service, metrics) = service_over(&root, decoder.clone());
    let cancelled = CancelToken::new();
    cancelled.cancel();

    let refused = service.preload(&path, &cancelled);
    assert!(refused.is_ready());
    assert!(matches!(finish_err(refused), PreviewError::Canceled));
    assert_eq!(snapshot(&metrics).cache_misses, 0, "nothing was looked up");

    let token = CancelToken::new();
    let started = service.preload(&path, &token);
    assert!(decoder.wait_calls(1));
    token.cancel(); // a started preload is never cancelled
    decoder.release_all();
    finish_ok(started);
    assert_eq!(decoder.calls()[0].request.priority, SourceReadPriority::Preload);
}

#[test]
fn get_preview_path_key_failure_is_ready_key_error() {
    let root = TempRoot::new("decode-path-key-error");
    let (service, metrics) = service_over(&root, TestDecoder::new());
    let missing = root.join("gone.jpg").to_string_lossy().into_owned();

    let handle = service.get_preview_path(&missing);

    assert!(handle.is_ready());
    let error = finish_err(handle);
    assert!(matches!(
        error,
        PreviewError::Key(CacheKeyError::SourceNoLongerExists { .. })
    ));
    assert_eq!(snapshot(&metrics).cache_misses, 0);
}

#[test]
fn cancelled_viewer_request_is_refused_before_the_cache_lookup() {
    let root = TempRoot::new("decode-viewer-cancelled-first");
    let path = source(&root, "a.jpg");
    let (service, metrics) = service_over(&root, TestDecoder::new());
    let key = service.get_current_cache_key(&path).expect("key");
    finish_ok(service.get_preview(&path, &key));
    let token = CancelToken::new();
    token.cancel();

    let handle = service.get_viewer_preview(&path, &key, &token, Duration::ZERO);

    assert!(matches!(finish_err(handle), PreviewError::Canceled));
    assert_eq!(
        snapshot(&metrics).cache_hits,
        0,
        "the token is checked before the RAM lookup"
    );
}

#[test]
fn cache_hit_is_ready_and_counts_a_hit() {
    let root = TempRoot::new("decode-hit-ready");
    let path = source(&root, "a.jpg");
    let (service, metrics) = service_over(&root, TestDecoder::new());
    let key = service.get_current_cache_key(&path).expect("key");
    let first = finish_ok(service.get_preview(&path, &key));

    let viewer = service.get_viewer_preview(&path, &key, &CancelToken::none(), Duration::from_secs(3600));

    assert!(viewer.is_ready());
    assert!(Arc::ptr_eq(&finish_ok(viewer), &first));
    let snapshot = snapshot(&metrics);
    assert_eq!(
        (snapshot.cache_hits, snapshot.cache_misses, snapshot.inflight_joins),
        (1, 1, 0)
    );
}

#[test]
fn source_bytes_read_prefers_the_image_report() {
    let root = TempRoot::new("decode-bytes-report");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::with_spec(ImageSpec {
        source_bytes_read: Some(12345),
        ..ImageSpec::default()
    });
    let (service, metrics) = service_over(&root, decoder);

    finish_ok(service.get_preview_path(&path));

    assert_eq!(snapshot(&metrics).source_bytes_read, 12345);
}

#[test]
fn degraded_fallback_still_seeds_original_dimensions() {
    let root = TempRoot::new("decode-degraded-dims");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::with_spec(ImageSpec {
        width: 64,
        height: 48,
        downscaled: Some(true),
        original: Some((640, 480)),
        degraded: true,
        ..ImageSpec::default()
    });
    let (service, metrics) = service_over(&root, decoder);
    let key = service.get_current_cache_key(&path).expect("key");

    finish_ok(service.get_preview(&path, &key));

    assert_eq!(service.cache_count(), 0);
    assert_eq!(service.try_get_known_original_dimensions(&key), Some((640, 480)));
    assert_eq!(snapshot(&metrics).source_reads, 1);
}

#[test]
fn has_inflight_preview_is_true_while_a_decode_runs() {
    let root = TempRoot::new("decode-inflight");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    decoder.gate_all();
    let (service, _) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    let handle = service.get_preview(&path, &key);
    assert!(service.has_inflight_preview(&key));
    assert!(decoder.wait_calls(1));
    assert!(service.has_inflight_preview(&key));
    decoder.release_all();
    finish_ok(handle);
    assert!(!service.has_inflight_preview(&key), "removed before the waiter resumes");
}

#[test]
fn clear_cache_during_a_decode_starts_a_new_load_for_the_new_epoch() {
    let root = TempRoot::new("decode-epoch-new-load");
    let path = source(&root, "a.jpg");
    let decoder = TestDecoder::new();
    decoder.gate_all();
    let (service, metrics) = service_over(&root, decoder.clone());
    let key = service.get_current_cache_key(&path).expect("key");

    let old = service.get_preview(&path, &key);
    assert!(decoder.wait_calls(1));
    service.clear_cache();
    assert!(
        !service.has_inflight_preview(&key),
        "the old load belongs to the old epoch"
    );
    let new = service.get_preview(&path, &key);
    assert_eq!(
        snapshot(&metrics).inflight_joins,
        0,
        "a request of the new epoch never joins the old load"
    );
    assert!(service.has_inflight_preview(&key));
    decoder.release_all();
    finish_ok(old);
    let fresh = finish_ok(new);

    assert_eq!(decoder.call_count(), 2);
    let cached = service
        .try_get_cached_preview(&key)
        .expect("the new epoch's decode is cached");
    assert!(Arc::ptr_eq(&cached, &fresh));
    assert!(!service.has_inflight_preview(&key));
}

// ---- golden: preview-service-sequence ------------------------------------------------------------------------------

/// The marker a golden decode carries as its platform image (C# `W414bMarker`).
struct GoldenMarker {
    serial: i64,
    decoder: &'static str,
}

struct GoldenImage {
    width: i32,
    height: i32,
    downscaled: bool,
    backend: DecoderBackend,
    original: (i32, i32),
    degraded: bool,
    bytes_read: Option<i64>,
    platform: PlatformImage,
}

impl DecodedImageView for GoldenImage {
    fn pixel_width(&self) -> i32 {
        self.width
    }
    fn pixel_height(&self) -> i32 {
        self.height
    }
    fn downscaled(&self) -> bool {
        self.downscaled
    }
    fn orientation(&self) -> i32 {
        1
    }
    fn estimated_bytes(&self) -> i64 {
        i64::from(self.width) * i64::from(self.height) * 4
    }
    fn platform_image(&self) -> &PlatformImage {
        &self.platform
    }
    fn actual_backend(&self) -> DecoderBackend {
        self.backend
    }
    fn original_width(&self) -> i32 {
        self.original.0
    }
    fn original_height(&self) -> i32 {
        self.original.1
    }
    fn is_degraded_fallback(&self) -> bool {
        self.degraded
    }
    fn source_bytes_read(&self) -> Option<i64> {
        self.bytes_read
    }
}

/// C# `W414bDecoder`: per file name a spec; records every call as the C# oracle prints it.
struct GoldenDecoder {
    name: &'static str,
    specs: Value,
    root: String,
    calls: Mutex<Vec<Value>>,
    serial: AtomicUsize,
}

fn golden_backend(name: &str) -> DecoderBackend {
    match name {
        "Wpf" => DecoderBackend::Wpf,
        "WicDirect" => DecoderBackend::WicDirect,
        "TurboJpeg" => DecoderBackend::TurboJpeg,
        "LibRaw" => DecoderBackend::LibRaw,
        other => panic!("unknown backend {other:?} in the golden"),
    }
}

fn opt_i32(spec: &Value, key: &str) -> Option<i32> {
    spec[key]
        .as_i64()
        .map(|value| i32::try_from(value).expect("an i32 in the golden"))
}

impl GoldenDecoder {
    fn spec(&self, path: &str) -> Value {
        let name = name_of(Path::new(path));
        self.specs
            .get(&name)
            .cloned()
            .unwrap_or_else(|| panic!("no spec for {path}"))
    }

    fn calls_since(&self, start: usize) -> Vec<Value> {
        lock(&self.calls)[start..].to_vec()
    }

    fn call_count(&self) -> usize {
        lock(&self.calls).len()
    }
}

/// C# `Rel(text)`: the temp root (upper-cased spelling first, then as given) becomes `<ROOT>`.
fn rel(root: &str, text: &str) -> String {
    text.replace(&root.to_uppercase(), "<ROOT>").replace(root, "<ROOT>")
}

impl ImageDecoder for GoldenDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let path = request.path.to_string_lossy().into_owned();
        let spec = self.spec(&path);
        lock(&self.calls).push(json!({
            "decoder": self.name,
            "call": "decode",
            "path": rel(&self.root, &path),
            "w": request.target_width,
            "h": request.target_height,
            "apply_orientation": request.apply_orientation,
            "bytes": request.bytes.as_ref().map(|bytes| bytes.len()),
            "priority": format!("{:?}", request.priority),
        }));
        match spec["fails"].as_str() {
            None => {}
            Some("Busy") => return Err(DecodeError::busy_with("test: busy")),
            Some("NotSupported") => {
                return Err(DecodeError::NotSupported {
                    message: "test: not supported".to_owned(),
                    user_text: None,
                    inner_hresult: None,
                });
            }
            Some("Canceled") => return Err(DecodeError::Canceled),
            Some("IO") => return Err(DecodeError::Io(io::Error::other("test: io"))),
            Some("InvalidData") => {
                return Err(DecodeError::InvalidData {
                    message: "test: invalid data".to_owned(),
                });
            }
            Some(other) => panic!("unknown failure {other}"),
        }
        let serial = i64::try_from(self.serial.fetch_add(1, Ordering::SeqCst) + 1).expect("serial");
        let width = opt_i32(&spec, "pixel_w").expect("pixel_w");
        let height = opt_i32(&spec, "pixel_h").expect("pixel_h");
        Ok(Arc::new(GoldenImage {
            width,
            height,
            downscaled: spec["downscaled"].as_bool().unwrap_or(true),
            backend: golden_backend(spec["backend"].as_str().unwrap_or("WicDirect")),
            original: (
                opt_i32(&spec, "original_w").unwrap_or(width),
                opt_i32(&spec, "original_h").unwrap_or(height),
            ),
            degraded: spec["degraded"].as_bool().unwrap_or(false),
            bytes_read: spec["source_bytes_read"].as_i64(),
            platform: PlatformImage::Opaque(Arc::new(GoldenMarker {
                serial,
                decoder: self.name,
            })),
        }))
    }

    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        let spec = self.spec(path);
        lock(&self.calls).push(json!({ "decoder": self.name, "call": "read_info", "path": rel(&self.root, path) }));
        if spec["info_fails"].as_bool() == Some(true) {
            return Err(DecodeError::Io(io::Error::other("test: read info")));
        }
        Ok(ImageInfo::with_orientation(
            opt_i32(&spec, "info_w").unwrap_or(1),
            opt_i32(&spec, "info_h").unwrap_or(1),
            opt_i32(&spec, "info_orientation").unwrap_or(1),
        ))
    }
}

fn golden_image_json(image: &PreviewImage) -> Value {
    let marker = match image.platform_image() {
        PlatformImage::Opaque(any) => any.downcast_ref::<GoldenMarker>(),
        PlatformImage::Pixels(_) => None,
    };
    json!({
        "serial": marker.map_or(-1, |m| m.serial),
        "decoder": marker.map(|m| m.decoder),
        "pixel_w": image.pixel_width(),
        "pixel_h": image.pixel_height(),
        "downscaled": image.downscaled(),
        "actual_backend": format!("{:?}", image.actual_backend()),
        "estimated_bytes": image.estimated_bytes(),
    })
}

/// C# `W414bError`: the exception type name, its message and whether it is localized.
fn golden_error_json(root: &str, error: &PreviewError) -> Value {
    let (throws, localized) = match error {
        PreviewError::SourceChanged { .. } => ("IOException", true),
        PreviewError::Key(CacheKeyError::SourceNoLongerExists { .. }) => ("FileNotFoundException", true),
        PreviewError::Canceled => ("OperationCanceledException", false),
        PreviewError::Decode(inner) => {
            let name = match inner.kind() {
                DecodeError::Busy { .. } => "DecoderBusyException",
                DecodeError::NotSupported { .. } => "NotSupportedException",
                DecodeError::Canceled => "OperationCanceledException",
                DecodeError::Io(_) => "IOException",
                DecodeError::InvalidData { .. } => "InvalidDataException",
                other => panic!("unmapped decode error {other:?}"),
            };
            (name, inner.is_localized())
        }
        other => panic!("unmapped error {other:?}"),
    };
    json!({ "throws": throws, "message": rel(root, &error.to_string()), "localized": localized })
}

fn golden_key_json(root: &str, key: &ImageCacheKey) -> Value {
    json!({
        "path": rel(root, key.path()),
        "length": key.length(),
        "ticks": key.last_write_utc_ticks(),
        "is_original": key.is_original(),
        "width": key.target_width(),
        "height": key.target_height(),
        "box_unbounded": key.target_box().is_unbounded(),
        "orientation_applied": key.orientation_applied(),
        "backend": format!("{:?}", key.backend()),
        "source_kind": key.source_kind(),
    })
}

fn golden_metrics_json(metrics: &ReviewMetrics) -> Value {
    let s = metrics.snapshot();
    json!({
        "cache_hits": s.cache_hits,
        "cache_misses": s.cache_misses,
        "inflight_joins": s.inflight_joins,
        "disk_cache_hits": s.disk_cache_hits,
        "source_reads": s.source_reads,
        "source_bytes_read": s.source_bytes_read,
        "source_open_count": s.source_open_count,
    })
}

/// Runs one golden case; returns a description of every mismatching step.
fn run_golden_case(case: &Value) -> Vec<String> {
    let name = str_of(case, "name");
    let input = &case["input"];
    let cfg = &input["config"];
    let temp = TempRoot::new("decode-golden");
    let root = temp.path().to_string_lossy().into_owned();
    for file in input["files"].as_array().expect("files") {
        let length = usize::try_from(i64_of(file, "length")).expect("length");
        let bytes: Vec<u8> = (0..length).map(|i| u8::try_from(i % 251).expect("byte")).collect();
        let path = source_with(&temp, str_of(file, "name"), &bytes);
        set_write_ticks(&path, i64_of(file, "write_ticks"));
    }
    let original_mode = Arc::new(AtomicBool::new(bool_of(cfg, "original_mode")));
    let target = Arc::new(Mutex::new(DecodeBox::new(
        opt_i32(cfg, "w").expect("w"),
        opt_i32(cfg, "h").expect("h"),
    )));
    let backend = Arc::new(Mutex::new(
        cfg["backend"].as_str().map_or(DecoderBackend::Wpf, golden_backend),
    ));
    let raw_enabled = Arc::new(AtomicBool::new(cfg["raw_full_enabled"].as_bool().unwrap_or(false)));
    let decoder = Arc::new(GoldenDecoder {
        name: "main",
        specs: input["decoder"].clone(),
        root: root.clone(),
        calls: Mutex::new(Vec::new()),
        serial: AtomicUsize::new(0),
    });
    let raw_decoder = input["raw_decoder"].is_object().then(|| {
        Arc::new(GoldenDecoder {
            name: "raw",
            specs: input["raw_decoder"].clone(),
            root: root.clone(),
            calls: Mutex::new(Vec::new()),
            serial: AtomicUsize::new(0),
        })
    });

    let mut config = config(&temp.join("cache-never-used"), decoder.clone());
    let mode = Arc::clone(&original_mode);
    config.is_original_loading_mode = Arc::new(move || mode.load(Ordering::SeqCst));
    let box_value = Arc::clone(&target);
    config.target_decode_box = Arc::new(move || *lock(&box_value));
    let backend_value = Arc::clone(&backend);
    config.current_backend = Some(Arc::new(move || *lock(&backend_value)));
    if let Some(capacity) = cfg["capacity_bytes"].as_i64() {
        config.capacity_bytes = capacity;
    }
    if let Some(capacity) = cfg["original_dimensions_capacity"].as_i64() {
        config.original_dimensions_capacity = i32::try_from(capacity).expect("capacity");
    }
    if let Some(capacity) = cfg["source_bytes_capacity"].as_i64() {
        config.source_bytes_cache = Some(Arc::new(
            SourceBytesCache::new(capacity, PHYSICAL_32_GIB, None).expect("source-bytes cache"),
        ));
    }
    if let Some(raw) = &raw_decoder {
        config.raw_full_decoder = Some(raw.clone());
    }
    let raw_value = Arc::clone(&raw_enabled);
    config.is_raw_full_decode_enabled = Some(Arc::new(move || raw_value.load(Ordering::SeqCst)));
    let metrics = Arc::clone(&config.metrics);
    let service = build(config);

    let mut keys: HashMap<String, ImageCacheKey> = HashMap::new();
    let mut mismatches = Vec::new();
    let steps = case["steps"].as_array().expect("steps");
    for (index, expected) in steps.iter().enumerate() {
        let op = &expected["op"];
        let kind = str_of(op, "op");
        let file_path = || temp.join(str_of(op, "file")).to_string_lossy().into_owned();
        let key_of = |keys: &HashMap<String, ImageCacheKey>| {
            keys.get(str_of(op, "key"))
                .cloned()
                .unwrap_or_else(|| panic!("{name}: unknown key {op}"))
        };
        let main_before = decoder.call_count();
        let raw_before = raw_decoder.as_ref().map_or(0, |raw| raw.call_count());
        let image_or_error = |result: PreviewResult| match result {
            Ok(image) => golden_image_json(&image),
            Err(error) => golden_error_json(&root, &error),
        };
        let result = match kind {
            "key" => match service.get_current_cache_key(&file_path()) {
                Ok(key) => {
                    let described = json!({ "key": golden_key_json(&root, &key) });
                    keys.insert(str_of(op, "as").to_owned(), key);
                    described
                }
                Err(error) => golden_error_json(&root, &PreviewError::Key(error)),
            },
            "preview" => {
                let handle = if op["key"].is_null() {
                    service.get_preview_path(&file_path())
                } else {
                    service.get_preview(&file_path(), &key_of(&keys))
                };
                image_or_error(finish(handle))
            }
            "viewer" => image_or_error(finish(service.get_viewer_preview(
                &file_path(),
                &key_of(&keys),
                &CancelToken::none(),
                Duration::ZERO,
            ))),
            "original" => image_or_error(finish(service.decode_original(
                &file_path(),
                &key_of(&keys),
                &CancelToken::none(),
            ))),
            "dims" => match service.get_original_dimensions(&file_path(), &key_of(&keys)) {
                Ok((w, h)) => json!({ "w": w, "h": h }),
                Err(error) => golden_error_json(&root, &error),
            },
            "known_dims" => match service.try_get_known_original_dimensions(&key_of(&keys)) {
                Some((w, h)) => json!({ "w": w, "h": h }),
                None => json!({ "known": false }),
            },
            "try_cached" => service
                .try_get_cached_preview(&key_of(&keys))
                .map_or_else(|| json!({ "cached": false }), |image| golden_image_json(&image)),
            "try_cached_path" => service
                .try_get_cached_preview_path(&file_path())
                .map_or_else(|| json!({ "cached": false }), |image| golden_image_json(&image)),
            "has_inflight" => json!({ "inflight": service.has_inflight_preview(&key_of(&keys)) }),
            "evict" => {
                let seen = Mutex::new(Vec::<Value>::new());
                let spelling = temp.join(str_of(op, "path_spelling")).to_string_lossy().into_owned();
                let record = |normalized: &str| lock(&seen).push(Value::String(rel(&root, normalized)));
                service.evict_cached_path(&spelling, Some(&record)).expect("evict");
                json!({ "also_invalidate": lock(&seen).clone() })
            }
            "clear" => {
                service.clear_cache();
                json!({})
            }
            "clear_source_bytes" => {
                service.clear_source_bytes_cache();
                json!({})
            }
            "rewrite" => {
                let path = file_path();
                let length = usize::try_from(i64_of(op, "length")).expect("length");
                std::fs::write(&path, vec![0u8; length]).expect("rewrite");
                set_write_ticks(&path, i64_of(op, "write_ticks"));
                json!({})
            }
            "delete" => {
                std::fs::remove_file(file_path()).expect("delete");
                json!({})
            }
            "set_mode" => {
                original_mode.store(bool_of(op, "original"), Ordering::SeqCst);
                json!({})
            }
            "set_box" => {
                *lock(&target) = DecodeBox::new(opt_i32(op, "w").expect("w"), opt_i32(op, "h").expect("h"));
                json!({})
            }
            "set_backend" => {
                *lock(&backend) = golden_backend(str_of(op, "backend"));
                json!({})
            }
            "set_raw_full" => {
                raw_enabled.store(bool_of(op, "enabled"), Ordering::SeqCst);
                json!({})
            }
            other => panic!("{name}: unknown op {other}"),
        };
        let mut calls = decoder.calls_since(main_before);
        if let Some(raw) = &raw_decoder {
            calls.extend(raw.calls_since(raw_before));
        }
        let mut expected_result = expected["result"].clone();
        let mut actual_result = result;
        // `DecodeError::Canceled` carries no message (C# "test: decoder-internal cancel"): compare the type only.
        if expected_result["throws"] == "OperationCanceledException" {
            expected_result["message"] = Value::Null;
            actual_result["message"] = Value::Null;
        }
        let actual = json!({
            "result": actual_result,
            "decoder_calls": calls,
            "metrics": golden_metrics_json(&metrics),
            "cache_count": service.cache_count(),
            "cache_bytes": service.cache_bytes(),
        });
        let wanted = json!({
            "result": expected_result,
            "decoder_calls": expected["decoder_calls"],
            "metrics": expected["metrics"],
            "cache_count": expected["cache_count"],
            "cache_bytes": expected["cache_bytes"],
        });
        if actual != wanted {
            mismatches.push(format!(
                "{name} step {index} ({op}):\n  expected {wanted}\n  actual   {actual}"
            ));
        }
    }
    mismatches
}

#[test]
fn golden_preview_service_sequence() {
    let golden = golden::load("preview-service-sequence");
    assert_eq!(golden.cases.len(), 33);
    let mismatches: Vec<String> = golden.cases.iter().flat_map(run_golden_case).collect();
    assert!(
        mismatches.is_empty(),
        "{} mismatching step(s):\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
