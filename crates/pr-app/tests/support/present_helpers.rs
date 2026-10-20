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
use pr_app::coordinators::zoom_detail::{BackgroundJob, BackgroundRunner};
use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_core::abstractions::{AppendStream, FileStat, FileSystem, Log, ReadSeek};
use pr_core::io::physical::PhysicalFileSystem;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::service::PreviewImage;

use super::presenter_fakes::PresenterFixture;
use super::preview_fakes::{Gate, TIMEOUT, lock, sized_image};
use pr_core::session::writer::{DebounceTimer, RunningTimer, TimerError, TimerOutcome};
use pr_imaging::service::{DecodeThreadKind, DecodeThreadSpawner, StdThreadSpawner};

/// The ordinal-ignore-case key of a path in the maps below (the test paths are ASCII).
fn key_of(path: &str) -> String {
    path.to_lowercase()
}

/// The ordinal-ignore-case key of a path's file name (the decoders below are told a file NAME: the temp folder is not known before the fixture exists).
fn file_name_key(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
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
/// The path is set once the fixture (and so its temp folder) exists.
#[derive(Debug, Default)]
pub struct RewritingDecoder {
    path: Mutex<Option<PathBuf>>,
    calls: AtomicUsize,
}

impl RewritingDecoder {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The file to rewrite on every decode.
    pub fn set_path(&self, path: &str) {
        *lock(&self.path) = Some(PathBuf::from(path));
    }

    pub fn decode_calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ImageDecoder for RewritingDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        use std::io::Write;
        self.calls.fetch_add(1, Ordering::SeqCst);
        let path = lock(&self.path).clone();
        if let Some(path) = path {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .map_err(DecodeError::Io)?;
            file.write_all(&[0]).map_err(DecodeError::Io)?;
        }
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
pub struct FaultingGatedDecoder {
    pub marker: String,
    gate: Arc<Gate>,
    threw: Arc<Gate>,
    failure: Box<dyn Fn(&str) -> DecodeError + Send + Sync>,
}

impl std::fmt::Debug for FaultingGatedDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaultingGatedDecoder")
            .field("marker", &self.marker)
            .finish_non_exhaustive()
    }
}

impl FaultingGatedDecoder {
    /// Fails with `InvalidOperation { marker }`.
    pub fn new(marker: &str) -> Arc<Self> {
        Self::with_failure(marker, |marker| DecodeError::InvalidOperation {
            message: marker.to_owned(),
        })
    }

    /// Fails with `failure(marker)`.
    pub fn with_failure(marker: &str, failure: impl Fn(&str) -> DecodeError + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            marker: marker.to_owned(),
            gate: Gate::new(),
            threw: Gate::new(),
            failure: Box::new(failure),
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
        Err((self.failure)(&self.marker))
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
        if file_name_key(&request.path) == self.watched {
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

// ---- scripted members, the in-flight failure, the spawner, the timer -----------------------------------------------------------

/// A decoder that answers `image` for every file except the file named `member`: the call number `n` (1-based) of the member's decode is answered by `script(n)`
/// (`Ok(())` = the image, `Err` = that failure). Counts the member's decodes.
pub struct MemberScriptDecoder {
    member: String,
    image: PreviewImage,
    calls: AtomicUsize,
    script: Box<dyn Fn(usize) -> Result<(), DecodeError> + Send + Sync>,
}

impl std::fmt::Debug for MemberScriptDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemberScriptDecoder")
            .field("member", &self.member)
            .finish_non_exhaustive()
    }
}

impl MemberScriptDecoder {
    pub fn new(
        member: &str,
        image: PreviewImage,
        script: impl Fn(usize) -> Result<(), DecodeError> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            member: key_of(member),
            image,
            calls: AtomicUsize::new(0),
            script: Box::new(script),
        })
    }

    /// How many decodes of the member entered the decoder.
    pub fn member_calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ImageDecoder for MemberScriptDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if file_name_key(&request.path) == self.member {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            (self.script)(call)?;
        }
        Ok(Arc::clone(&self.image))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(self.image.pixel_width(), self.image.pixel_height()))
    }
}

/// C# `HoldFirstFailureDecoder`: the decode of `watched` fails with `FileNotFound` while the file is not on disk; the FIRST such failure is held back
/// (`wait_failed` returns, then the error is only returned once `release` ran): the stand-in for "the decode is slow to report that the file was not there".
pub struct HoldFirstFailureDecoder {
    watched: String,
    image: PreviewImage,
    held: AtomicUsize,
    failed: Arc<Gate>,
    release: Arc<Gate>,
}

impl std::fmt::Debug for HoldFirstFailureDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HoldFirstFailureDecoder").finish_non_exhaustive()
    }
}

impl HoldFirstFailureDecoder {
    pub fn new(watched: &str, image: PreviewImage) -> Arc<Self> {
        Arc::new(Self {
            watched: key_of(watched),
            image,
            held: AtomicUsize::new(0),
            failed: Gate::new(),
            release: Gate::new(),
        })
    }

    /// Waits until the held decode found the file missing; `false` when [`TIMEOUT`] passes first.
    pub fn wait_failed(&self) -> bool {
        self.failed.wait()
    }

    /// Lets the held failure be reported.
    pub fn release(&self) {
        self.release.open();
    }
}

impl ImageDecoder for HoldFirstFailureDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if request.path.exists() {
            return Ok(Arc::clone(&self.image));
        }
        let error = DecodeError::FileNotFound {
            message: format!("Could not find file '{}'.", request.path.display()),
        };
        if file_name_key(&request.path) == self.watched && self.held.fetch_add(1, Ordering::SeqCst) == 0 {
            self.failed.open();
            self.release.wait();
        }
        Err(error)
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(self.image.pixel_width(), self.image.pixel_height()))
    }
}

/// A spawner that records the kind of every decode thread it starts, then starts it like the standard one.
#[derive(Debug, Default)]
pub struct RecordingSpawner {
    kinds: Mutex<Vec<DecodeThreadKind>>,
}

impl RecordingSpawner {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn kinds(&self) -> Vec<DecodeThreadKind> {
        lock(&self.kinds).clone()
    }

    pub fn count(&self, kind: DecodeThreadKind) -> usize {
        lock(&self.kinds).iter().filter(|recorded| **recorded == kind).count()
    }
}

impl DecodeThreadSpawner for RecordingSpawner {
    fn spawn(&self, kind: DecodeThreadKind, job: Box<dyn FnOnce() + Send + 'static>) -> io::Result<()> {
        lock(&self.kinds).push(kind);
        StdThreadSpawner.spawn(kind, job)
    }
}

/// A debounce timer that never fires: a started timer ends only when it is cancelled (by `flush` or `dispose`).
#[derive(Debug, Default)]
pub struct NeverFiringTimer;

struct WaitForCancel {
    cancel: pr_core::sync::CancelToken,
}

impl RunningTimer for WaitForCancel {
    fn wait(self: Box<Self>) -> Result<TimerOutcome, TimerError> {
        loop {
            if self.cancel.is_canceled() {
                return Ok(TimerOutcome::Canceled);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

impl DebounceTimer for NeverFiringTimer {
    fn start(
        &self,
        _span: std::time::Duration,
        cancel: &pr_core::sync::CancelToken,
    ) -> Result<Box<dyn RunningTimer>, TimerError> {
        Ok(Box::new(WaitForCancel { cancel: cancel.clone() }))
    }
}

/// The script of a [`NamedScriptDecoder`]: `(file name in lower case, 1-based call number of that file)` to the outcome of the decode.
type NameScript = Box<dyn Fn(&str, usize) -> Result<(), DecodeError> + Send + Sync>;

/// A decoder that answers `image` unless `script(file name in lower case, call number of that file)` says otherwise (call numbers are 1-based per file).
pub struct NamedScriptDecoder {
    image: PreviewImage,
    calls: Mutex<HashMap<String, usize>>,
    script: NameScript,
}

impl std::fmt::Debug for NamedScriptDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NamedScriptDecoder").finish_non_exhaustive()
    }
}

impl NamedScriptDecoder {
    pub fn new(
        image: PreviewImage,
        script: impl Fn(&str, usize) -> Result<(), DecodeError> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            image,
            calls: Mutex::new(HashMap::new()),
            script: Box::new(script),
        })
    }

    /// How many decodes of the file named `name` (lower case) entered the decoder.
    pub fn calls_of(&self, name: &str) -> usize {
        lock(&self.calls).get(name).copied().unwrap_or(0)
    }
}

impl ImageDecoder for NamedScriptDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let name = file_name_key(&request.path);
        let call = {
            let mut calls = lock(&self.calls);
            let count = calls.entry(name.clone()).or_insert(0);
            *count += 1;
            *count
        };
        (self.script)(&name, call)?;
        Ok(Arc::clone(&self.image))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(self.image.pixel_width(), self.image.pixel_height()))
    }
}

/// A background runner that holds the jobs of the named threads until the test releases them (every other job starts a thread at once): the test decides
/// WHEN a worker result may arrive, so "superseded while the sizes / hashes are computed" needs no timing.
#[derive(Default)]
pub struct HoldRunner {
    hold: Vec<&'static str>,
    held: Mutex<Vec<(&'static str, BackgroundJob)>>,
}

impl std::fmt::Debug for HoldRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HoldRunner")
            .field("hold", &self.hold)
            .finish_non_exhaustive()
    }
}

impl HoldRunner {
    /// Holds the jobs started with one of `names`.
    pub fn new(names: &[&'static str]) -> Arc<Self> {
        Arc::new(Self {
            hold: names.to_vec(),
            held: Mutex::new(Vec::new()),
        })
    }

    /// How many jobs of `name` are held right now.
    pub fn held_count(&self, name: &str) -> usize {
        lock(&self.held).iter().filter(|(held, _)| *held == name).count()
    }

    /// Starts every held job of `name` on its own thread; returns how many.
    pub fn release(&self, name: &str) -> usize {
        let jobs: Vec<(&'static str, BackgroundJob)> = {
            let mut held = lock(&self.held);
            let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut *held)
                .into_iter()
                .partition(|(held, _)| *held == name);
            *held = rest;
            matching
        };
        let count = jobs.len();
        for (job_name, job) in jobs {
            spawn_job(job_name, job);
        }
        count
    }
}

fn spawn_job(name: &'static str, job: BackgroundJob) {
    if let Err(error) = std::thread::Builder::new().name(name.to_owned()).spawn(job) {
        panic!("could not start {name}: {error}");
    }
}

impl BackgroundRunner for HoldRunner {
    fn run(&self, name: &'static str, job: BackgroundJob) {
        if self.hold.contains(&name) {
            lock(&self.held).push((name, job));
        } else {
            spawn_job(name, job);
        }
    }
}

/// A decoder whose header read (`read_info`, the original-dimension lookup) signals `wait_info_started` and then blocks until `release_info`.
pub struct GatedInfoDecoder {
    image: PreviewImage,
    started: Arc<Gate>,
    started_flag: std::sync::atomic::AtomicBool,
    release: Arc<Gate>,
}

impl std::fmt::Debug for GatedInfoDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatedInfoDecoder").finish_non_exhaustive()
    }
}

impl GatedInfoDecoder {
    pub fn new(image: PreviewImage) -> Arc<Self> {
        Arc::new(Self {
            image,
            started: Gate::new(),
            started_flag: std::sync::atomic::AtomicBool::new(false),
            release: Gate::new(),
        })
    }

    pub fn wait_info_started(&self) -> bool {
        self.started.wait()
    }

    /// True once a header read entered the decoder (non-blocking).
    pub fn info_started(&self) -> bool {
        self.started_flag.load(Ordering::SeqCst)
    }

    pub fn release_info(&self) {
        self.release.open();
    }
}

impl ImageDecoder for GatedInfoDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Ok(Arc::clone(&self.image))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        self.started_flag.store(true, Ordering::SeqCst);
        self.started.open();
        self.release.wait();
        Ok(ImageInfo::new(self.image.pixel_width(), self.image.pixel_height()))
    }
}
