//! Helpers of the `presenter_present_*` / `presenter_missing` / `presenter_compare_pair` test files (W8-07i..07k), on top of
//! `presenter_fakes.rs` and `preview_fakes.rs`. Included with `mod support { pub mod present_helpers; pub mod presenter_fakes; pub mod preview_fakes; }`.
//!
//! Decoders that a test gates, records or makes fail ([`GatedDecoder`], [`PerPathGatedDecoder`], [`RecordingDecoder`], [`RewritingDecoder`],
//! [`FaultingGatedDecoder`], [`OnDecodeDecoder`], [`ThrowingInfoDecoder`]), a stat file system a test holds, scripts or watches ([`ScriptedFs`]), and the
//! capture of the process-wide `AppLog` ([`captured_log`]). Every wait is bounded by [`TIMEOUT`]; there is no fixed sleep.

// Each test binary uses a different subset of these helpers.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::thread::ThreadId;

use pr_app::coordinators::presenter::state::{PresentError, PresentRequest, PresenterChanges, PresenterEnv};
use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_core::abstractions::{AppendStream, FileStat, FileSystem, Log, ReadSeek};
use pr_core::io::physical::PhysicalFileSystem;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::service::PreviewImage;

use super::presenter_fakes::PresenterFixture;
use super::preview_fakes::{Gate, TIMEOUT, lock, sized_image};

/// The ordinal-ignore-case key of a path in the maps below (the test paths are ASCII).
fn key_of(path: &str) -> String {
    path.to_lowercase()
}

// ---- driving the presenter -----------------------------------------------------------------------------------------------

/// `present(request)` with the fixture's environment (catalog, settings, session, format texts).
pub fn present_request(f: &mut PresenterFixture, request: PresentRequest) -> Result<PresenterChanges, PresentError> {
    let mut env = PresenterEnv {
        catalog: &mut f.catalog,
        settings: &f.settings,
        session: f.session.as_mut(),
        format: f.format.ctx(),
    };
    f.presenter.present(&mut env, request)
}

/// `present(PresentRequest::new(index))`; a `PresentError` fails the test.
pub fn start(f: &mut PresenterFixture, index: i32) {
    present_request(f, PresentRequest::new(index)).unwrap_or_else(|error| panic!("present {index}: {error}"));
}

/// `start(index)` then pump until the presenter is idle (C# `await presenter.PresentAsync(index)`).
pub fn present(f: &mut PresenterFixture, index: i32) {
    start(f, index);
    f.pump_until_idle();
}

/// `present_request` then pump until idle.
pub fn present_with(f: &mut PresenterFixture, request: PresentRequest) {
    present_request(f, request).unwrap_or_else(|error| panic!("present: {error}"));
    f.pump_until_idle();
}

/// Waits (bounded) until ONE worker completion is queued, then pumps: the deterministic way to run exactly the next continuation.
pub fn pump_one(f: &mut PresenterFixture) -> PresenterChanges {
    assert!(
        f.presenter.wait_for_event(TIMEOUT),
        "no worker completion arrived in {TIMEOUT:?}"
    );
    f.pump()
}

/// The statuses the sink was given, in order.
pub fn statuses(f: &PresenterFixture) -> Vec<String> {
    lock(&f.sink_log).statuses.clone()
}

/// The paths the sink was told were presented, in order.
pub fn presented_paths(f: &PresenterFixture) -> Vec<String> {
    lock(&f.sink_log).presented_paths.clone()
}

/// How many `set_current_image` calls carried an image.
pub fn images_shown(f: &PresenterFixture) -> usize {
    lock(&f.sink_log)
        .images
        .iter()
        .filter(|call| call.image.is_some())
        .count()
}

// ---- decoders ------------------------------------------------------------------------------------------------------------

/// A counting gate: `release()` lets exactly ONE decode through (C# `SemaphoreSlim(0, 1)` + `Release()`); a decode waits at most [`TIMEOUT`].
#[derive(Debug, Default)]
struct Permits {
    available: Mutex<usize>,
    changed: Condvar,
}

impl Permits {
    fn release(&self) {
        *lock(&self.available) += 1;
        self.changed.notify_all();
    }

    fn take(&self) -> bool {
        let guard = lock(&self.available);
        let (mut guard, _) = self
            .changed
            .wait_timeout_while(guard, TIMEOUT, |available| *available == 0)
            .unwrap_or_else(PoisonError::into_inner);
        if *guard == 0 {
            return false;
        }
        *guard -= 1;
        true
    }
}

/// C# `GatedDecoder`: every decode blocks until the test calls [`Self::release`] (once per decode), then returns `image`.
pub struct GatedDecoder {
    image: PreviewImage,
    permits: Permits,
    started: Mutex<usize>,
    started_changed: Condvar,
}

impl std::fmt::Debug for GatedDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatedDecoder").finish_non_exhaustive()
    }
}

impl GatedDecoder {
    pub fn new(image: PreviewImage) -> Arc<Self> {
        Arc::new(Self {
            image,
            permits: Permits::default(),
            started: Mutex::new(0),
            started_changed: Condvar::new(),
        })
    }

    /// Lets one blocked (or future) decode finish.
    pub fn release(&self) {
        self.permits.release();
    }

    /// Lets `count` decodes finish (test teardown).
    pub fn release_many(&self, count: usize) {
        for _ in 0..count {
            self.permits.release();
        }
    }

    /// Waits until `count` decodes entered the decoder; `false` when [`TIMEOUT`] passes first.
    pub fn wait_started(&self, count: usize) -> bool {
        let guard = lock(&self.started);
        let (guard, _) = self
            .started_changed
            .wait_timeout_while(guard, TIMEOUT, |started| *started < count)
            .unwrap_or_else(PoisonError::into_inner);
        *guard >= count
    }

    pub fn started(&self) -> usize {
        *lock(&self.started)
    }
}

impl ImageDecoder for GatedDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        *lock(&self.started) += 1;
        self.started_changed.notify_all();
        if !self.permits.take() {
            return Err(DecodeError::Io(io::Error::new(
                io::ErrorKind::TimedOut,
                "the decode gate was never released",
            )));
        }
        Ok(Arc::clone(&self.image))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(self.image.pixel_width(), self.image.pixel_height()))
    }
}

/// C# `ImmediateDecoder`: returns `image` at once.
pub struct ImmediateDecoder {
    pub image: PreviewImage,
}

impl std::fmt::Debug for ImmediateDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImmediateDecoder").finish_non_exhaustive()
    }
}

impl ImageDecoder for ImmediateDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Ok(Arc::clone(&self.image))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(self.image.pixel_width(), self.image.pixel_height()))
    }
}

/// C# `RecordingDecoder`: records every path it decodes (as given by the request) and answers a 1 x 1 image.
#[derive(Debug, Default)]
pub struct RecordingDecoder {
    paths: Mutex<Vec<String>>,
}

impl RecordingDecoder {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn paths(&self) -> Vec<String> {
        lock(&self.paths).clone()
    }
}

impl ImageDecoder for RecordingDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        lock(&self.paths).push(request.path.to_string_lossy().into_owned());
        Ok(sized_image(1, 1, 1, 1, false))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(1, 1))
    }
}

/// C# `RewritingDecoder`: appends one byte to the source file during the decode (a deterministic stand-in for another program rewriting it).
#[derive(Debug)]
pub struct RewritingDecoder {
    path: PathBuf,
    calls: AtomicUsize,
}

impl RewritingDecoder {
    pub fn new(path: &str) -> Arc<Self> {
        Arc::new(Self {
            path: PathBuf::from(path),
            calls: AtomicUsize::new(0),
        })
    }

    pub fn decode_calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ImageDecoder for RewritingDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        use std::io::Write;
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(DecodeError::Io)?;
        file.write_all(&[0]).map_err(DecodeError::Io)?;
        Ok(sized_image(1, 1, 1, 1, false))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(1, 1))
    }
}

/// C# `PerPathGatedDecoder`: the decode of each path blocks until [`Self::open`] is called for it; a gate nobody opened is recorded in
/// [`Self::timed_out`] (APP-T23) and the decode fails.
#[derive(Debug, Default)]
pub struct PerPathGatedDecoder {
    gates: Mutex<HashMap<String, Arc<Gate>>>,
    started: Mutex<Vec<String>>,
    started_changed: Condvar,
    timed_out: Mutex<Vec<String>>,
}

impl PerPathGatedDecoder {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn gate(&self, path: &str) -> Arc<Gate> {
        Arc::clone(lock(&self.gates).entry(key_of(path)).or_default())
    }

    pub fn open(&self, path: &str) {
        self.gate(path).open();
    }

    /// Waits until the decode of `path` entered the decoder; `false` when [`TIMEOUT`] passes first.
    pub fn wait_started(&self, path: &str) -> bool {
        let key = key_of(path);
        let guard = lock(&self.started);
        let (guard, _) = self
            .started_changed
            .wait_timeout_while(guard, TIMEOUT, |started| !started.contains(&key))
            .unwrap_or_else(PoisonError::into_inner);
        guard.contains(&key)
    }

    pub fn timed_out(&self) -> Vec<String> {
        lock(&self.timed_out).clone()
    }
}

impl ImageDecoder for PerPathGatedDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let path = request.path.to_string_lossy().into_owned();
        lock(&self.started).push(key_of(&path));
        self.started_changed.notify_all();
        if !self.gate(&path).wait() {
            lock(&self.timed_out).push(path.clone());
            return Err(DecodeError::Io(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("decode gate for {path} was never opened"),
            )));
        }
        Ok(sized_image(1, 1, 1, 1, false))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(1, 1))
    }
}

/// C# `FaultingGatedDecoder`: blocks until [`Self::release_and_throw`], signals [`Self::wait_threw`] just before it fails with `marker`.
#[derive(Debug)]
pub struct FaultingGatedDecoder {
    pub marker: String,
    gate: Arc<Gate>,
    threw: Arc<Gate>,
}

impl FaultingGatedDecoder {
    pub fn new(marker: &str) -> Arc<Self> {
        Arc::new(Self {
            marker: marker.to_owned(),
            gate: Gate::new(),
            threw: Gate::new(),
        })
    }

    pub fn release_and_throw(&self) {
        self.gate.open();
    }

    /// Waits until the decode failed; `false` when [`TIMEOUT`] passes first.
    pub fn wait_threw(&self) -> bool {
        self.threw.wait()
    }
}

impl ImageDecoder for FaultingGatedDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.gate.wait();
        self.threw.open();
        Err(DecodeError::InvalidOperation {
            message: self.marker.clone(),
        })
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(1, 1))
    }
}

/// C# `OnDecodeDecoder`: runs `on_decode` (on the decode thread) when the decode of `watched` starts, then answers `image`.
pub struct OnDecodeDecoder {
    watched: String,
    on_decode: Box<dyn Fn() + Send + Sync>,
    image: PreviewImage,
}

impl std::fmt::Debug for OnDecodeDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnDecodeDecoder")
            .field("watched", &self.watched)
            .finish()
    }
}

impl OnDecodeDecoder {
    pub fn new(watched: &str, image: PreviewImage, on_decode: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            watched: key_of(watched),
            on_decode: Box::new(on_decode),
            image,
        })
    }
}

impl ImageDecoder for OnDecodeDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if key_of(&request.path.to_string_lossy()) == self.watched {
            (self.on_decode)();
        }
        Ok(Arc::clone(&self.image))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(self.image.pixel_width(), self.image.pixel_height()))
    }
}

/// C# `ThrowingInfoDecoder`: decodes like `image`, but `read_info` (the original-dimension lookup) fails with `failure()`.
pub struct ThrowingInfoDecoder {
    image: PreviewImage,
    failure: Box<dyn Fn() -> DecodeError + Send + Sync>,
}

impl std::fmt::Debug for ThrowingInfoDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThrowingInfoDecoder").finish_non_exhaustive()
    }
}

impl ThrowingInfoDecoder {
    pub fn new(image: PreviewImage, failure: impl Fn() -> DecodeError + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            image,
            failure: Box::new(failure),
        })
    }
}

impl ImageDecoder for ThrowingInfoDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Ok(Arc::clone(&self.image))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Err((self.failure)())
    }
}

// ---- the stat file system -------------------------------------------------------------------------------------------------

/// Answers one call of a scripted stat; the argument is the 1-based call number for that path.
pub type StatScript = Arc<dyn Fn(usize) -> io::Result<Option<FileStat>> + Send + Sync>;

/// C# `DelayedStatFileSystem` / `StatThrowingFileSystem` / `StatFailsAfterFirstFileSystem` in one: forwards to the physical file system, but a stat of a HELD path
/// blocks (off the caller's thread only) until released, a stat of a SCRIPTED path is answered by its script, and every stat is recorded with its thread.
#[derive(Default)]
pub struct ScriptedFs {
    inner: PhysicalFileSystem,
    held: Mutex<HashMap<String, Arc<Gate>>>,
    scripts: Mutex<HashMap<String, StatScript>>,
    calls: Mutex<HashMap<String, usize>>,
    stats: Mutex<Vec<(String, ThreadId)>>,
    stats_changed: Condvar,
    on_caller: Mutex<Vec<String>>,
    caller: Mutex<Option<ThreadId>>,
}

impl std::fmt::Debug for ScriptedFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedFs").finish_non_exhaustive()
    }
}

impl ScriptedFs {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Makes every stat of `path` (from a thread other than the caller's) wait for [`Self::release`].
    pub fn hold(&self, path: &str) {
        lock(&self.held).insert(key_of(path), Gate::new());
    }

    pub fn release(&self, path: &str) {
        if let Some(gate) = lock(&self.held).get(&key_of(path)) {
            gate.open();
        }
    }

    pub fn release_all(&self) {
        for gate in lock(&self.held).values() {
            gate.open();
        }
    }

    /// Answers the stats of `path` with `script` instead of the disk.
    pub fn script(&self, path: &str, script: impl Fn(usize) -> io::Result<Option<FileStat>> + Send + Sync + 'static) {
        lock(&self.scripts).insert(key_of(path), Arc::new(script));
    }

    /// Every stat of `path` fails with `error()`.
    pub fn fail_with(&self, path: &str, error: impl Fn() -> io::Error + Send + Sync + 'static) {
        self.script(path, move |_| Err(error()));
    }

    /// The thread whose stats must never happen (recorded in [`Self::stats_on_caller_thread`], never blocked).
    pub fn set_caller_thread(&self, thread: Option<ThreadId>) {
        *lock(&self.caller) = thread;
    }

    /// Waits until a stat of `path` entered the file system; `false` when [`TIMEOUT`] passes first.
    pub fn wait_started(&self, path: &str) -> bool {
        let key = key_of(path);
        let guard = lock(&self.stats);
        let (guard, _) = self
            .stats_changed
            .wait_timeout_while(guard, TIMEOUT, |stats| !stats.iter().any(|(stat, _)| *stat == key))
            .unwrap_or_else(PoisonError::into_inner);
        guard.iter().any(|(stat, _)| *stat == key)
    }

    /// Every stat so far: lower-cased path and thread.
    pub fn stats(&self) -> Vec<(String, ThreadId)> {
        lock(&self.stats).clone()
    }

    /// How many times `path` was statted.
    pub fn stat_count(&self, path: &str) -> usize {
        lock(&self.stats)
            .iter()
            .filter(|(stat, _)| *stat == key_of(path))
            .count()
    }

    pub fn stats_on_caller_thread(&self) -> Vec<String> {
        lock(&self.on_caller).clone()
    }
}

impl FileSystem for ScriptedFs {
    fn file_exists(&self, path: &Path) -> bool {
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.inner.directory_exists(path)
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.try_get_file_stat(path).ok().flatten()
    }

    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        let key = key_of(&path.to_string_lossy());
        let thread = std::thread::current().id();
        let call = {
            let mut calls = lock(&self.calls);
            let count = calls.entry(key.clone()).or_insert(0);
            *count += 1;
            *count
        };
        lock(&self.stats).push((key.clone(), thread));
        self.stats_changed.notify_all();
        if *lock(&self.caller) == Some(thread) {
            lock(&self.on_caller).push(key.clone());
        } else {
            let gate = lock(&self.held).get(&key).cloned();
            if let Some(gate) = gate {
                gate.wait();
            }
        }
        let script = lock(&self.scripts).get(&key).cloned();
        match script {
            Some(script) => script(call),
            None => self.inner.try_get_file_stat(path),
        }
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.move_file(source, destination)
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.copy_file(source, destination)
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        self.inner.delete(path)
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.inner.open_read_shared(path, buffer_size)
    }

    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.inner.open_append(path, durable)
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.inner.write_all_text_atomic(path, text, durable)
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.inner.read_all_text(path)
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_files(directory, pattern)
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_directories(directory)
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.inner.create_directory(path)
    }
}

// ---- the application log --------------------------------------------------------------------------------------------------

/// An `AppLog` target that keeps every line (C# `CapturedAppLog`). The log is process-wide, so the lines of parallel tests mix:
/// a test filters by a text only it writes (its unique temp path).
#[derive(Debug, Default)]
pub struct CapturedLog {
    lines: Mutex<Vec<String>>,
}

impl CapturedLog {
    /// Every captured line.
    pub fn lines(&self) -> Vec<String> {
        lock(&self.lines).clone()
    }

    /// The captured lines that contain `needle`.
    pub fn lines_with(&self, needle: &str) -> Vec<String> {
        lock(&self.lines)
            .iter()
            .filter(|line| line.contains(needle))
            .cloned()
            .collect()
    }
}

impl Log for CapturedLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, message: &str) {
        lock(&self.lines).push(format!("INFO {message}"));
    }

    fn warn(&self, message: &str) {
        lock(&self.lines).push(format!("WARN {message}"));
    }

    fn error(&self, message: &str, error: Option<&(dyn std::error::Error + 'static)>) {
        let line = match error {
            Some(error) => format!("ERROR {message}: {error}"),
            None => format!("ERROR {message}"),
        };
        lock(&self.lines).push(line);
    }
}

impl AppLogTarget for CapturedLog {
    fn set_enabled(&self, _enabled: bool) {}

    fn flush(&self) {}

    fn shutdown(&self) {}

    fn file_path(&self) -> PathBuf {
        PathBuf::new()
    }
}

/// Installs the capture as the target of the process-wide `AppLog` (once per test binary) and returns it.
pub fn captured_log() -> Arc<CapturedLog> {
    static CAPTURE: OnceLock<Arc<CapturedLog>> = OnceLock::new();
    Arc::clone(CAPTURE.get_or_init(|| {
        let log = Arc::new(CapturedLog::default());
        AppLog::global().set_instance(Some(Arc::clone(&log) as Arc<dyn AppLogTarget>));
        log
    }))
}
