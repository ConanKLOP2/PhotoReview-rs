//! Fakes of the preload scheduler tests (port of the private fakes of `PreloadSchedulerMutationTests.cs`,
//! `PreloadSafetyTests.cs`, `BurstPreloadTests.cs`, ... at C# baseline d92d6aae, shared here so that every
//! `preload_scheduler_*.rs` file builds the same target, probe, clock and timers).
//!
//! DIVERGENCE (test seam): `pr_core::sync::CancelToken` has no callback registration, so a gated fake decode cannot be
//! woken by `cancel()` the way C# `gate.Task.WaitAsync(token)` is. [`Gate::wait`] therefore re-checks the token every
//! 5 ms while it waits on the gate's condition variable; no assertion depends on that interval.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use pr_core::abstractions::{Log, MemoryProbe, MemorySnapshot};
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::sync::CancelToken;
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::navigation_pace::{NavigationPace, TICKS_PER_SECOND};
use pr_imaging::preload::scheduler::{
    CallbackError, EntriesSnapshotFn, EnvLookup, MillisecondClock, PreloadScheduler, PreloadSchedulerDeps, RunHandle,
    SnapshotVersionFn,
};
use pr_imaging::preload::target::{KeyError, PreloadTarget};
use pr_imaging::preload::timer::{OneShotTimer, OneShotTimerFactory, TimerCallback};
use pr_imaging::preload_order::{PreloadOptions, PreloadWindow};
use pr_test_support::FakeFileSystem;

/// C# `Guard` (`TimeSpan.FromSeconds(10)`): a hang guard only, never a timing assertion.
pub const GUARD: Duration = Duration::from_secs(10);

/// Locks ignoring poison (a panicking fake must not wedge the test that reports it).
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- gate (C# TaskCompletionSource) -----------------------------------------------------------------------------------

/// One-shot gate: `open` releases every waiter (C# `TaskCompletionSource.TrySetResult`).
#[derive(Debug, Default)]
pub struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn open(&self) {
        *lock(&self.open) = true;
        self.cv.notify_all();
    }

    pub fn is_open(&self) -> bool {
        *lock(&self.open)
    }

    /// Waits until the gate opens (`true`), the hang guard expires (`false`), or `cancel` is canceled (`false`).
    pub fn wait(&self, cancel: Option<&CancelToken>) -> bool {
        let start = Instant::now();
        let mut open = lock(&self.open);
        loop {
            if *open {
                return true;
            }
            if cancel.is_some_and(CancelToken::is_canceled) || start.elapsed() >= GUARD {
                return false;
            }
            let (guard, _) = self
                .cv
                .wait_timeout(open, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner);
            open = guard;
        }
    }
}

// ---- log (C# RecordingLog with a signal) -------------------------------------------------------------------------------

/// Captures every log line; `wait_error` lets a test block until the scheduler logged an error (the lines are written
/// on the run thread, after the state change they describe).
#[derive(Debug)]
pub struct CaptureLog {
    enabled: bool,
    /// Golden mode: errors are recorded without the exception suffix (the C# oracle logs the message only).
    plain: bool,
    lines: Mutex<Vec<(&'static str, String)>>,
    cv: Condvar,
}

impl CaptureLog {
    pub fn new() -> Arc<Self> {
        Self::with_enabled(false)
    }

    pub fn with_enabled(enabled: bool) -> Arc<Self> {
        Arc::new(Self {
            enabled,
            plain: false,
            lines: Mutex::new(Vec::new()),
            cv: Condvar::new(),
        })
    }

    /// Golden mode: message-only errors (C# oracle format).
    pub fn plain(enabled: bool) -> Arc<Self> {
        Arc::new(Self {
            enabled,
            plain: true,
            lines: Mutex::new(Vec::new()),
            cv: Condvar::new(),
        })
    }

    /// Drains every line logged so far as `level: message`, in order.
    pub fn take_lines(&self) -> Vec<String> {
        lock(&self.lines)
            .drain(..)
            .map(|(level, message)| format!("{level}: {message}"))
            .collect()
    }

    fn push(&self, level: &'static str, message: String) {
        lock(&self.lines).push((level, message));
        self.cv.notify_all();
    }

    /// Messages logged at `level` ("info", "warn", "error"), in order.
    pub fn of(&self, level: &str) -> Vec<String> {
        lock(&self.lines)
            .iter()
            .filter(|(l, _)| *l == level)
            .map(|(_, m)| m.clone())
            .collect()
    }

    pub fn errors(&self) -> Vec<String> {
        self.of("error")
    }

    pub fn warnings(&self) -> Vec<String> {
        self.of("warn")
    }

    pub fn infos(&self) -> Vec<String> {
        self.of("info")
    }

    /// Waits (guarded) until an error starting with `prefix` was logged; `true` when it was.
    pub fn wait_error(&self, prefix: &str) -> bool {
        let start = Instant::now();
        let mut lines = lock(&self.lines);
        loop {
            if lines.iter().any(|(l, m)| *l == "error" && m.starts_with(prefix)) {
                return true;
            }
            if start.elapsed() >= GUARD {
                return false;
            }
            let (guard, _) = self
                .cv
                .wait_timeout(lines, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner);
            lines = guard;
        }
    }
}

impl Log for CaptureLog {
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn info(&self, message: &str) {
        self.push("info", message.to_owned());
    }

    fn warn(&self, message: &str) {
        self.push("warn", message.to_owned());
    }

    fn error(&self, message: &str, error: Option<&(dyn std::error::Error + 'static)>) {
        let text = match error {
            Some(e) if !self.plain => format!("{message} :: {e}"),
            _ => message.to_owned(),
        };
        self.push("error", text);
    }
}

// ---- memory probes ----------------------------------------------------------------------------------------------------

/// C# private `Probe(Func<bool> hasHeadroom)` / `FakeMemoryProbe(bool)`: the answer is whatever the closure says; the
/// snapshot is the constant `(50 %, 16 GiB)` of the C# `DelegateMemoryProbe`.
pub struct FakeProbe {
    headroom: Box<dyn Fn() -> bool + Send + Sync>,
    calls: AtomicUsize,
}

impl FakeProbe {
    pub fn new(headroom: impl Fn() -> bool + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            headroom: Box::new(headroom),
            calls: AtomicUsize::new(0),
        })
    }

    pub fn answering(answer: bool) -> Arc<Self> {
        Self::new(move || answer)
    }

    /// C# `Calls`: how many times `has_headroom` was asked.
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl MemoryProbe for FakeProbe {
    fn has_headroom(&self, _maximum_load: f64, _reserve_bytes: i64) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.headroom)()
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        Some(MemorySnapshot {
            load_percent: 50,
            available_bytes: 16 << 30,
        })
    }
}

// ---- clock (C# FakeClock feeding NavigationPace) ----------------------------------------------------------------------

/// C# `FakeClock`: ticks start at 1_000_000 and advance only when the test says so.
#[derive(Debug)]
pub struct FakeClock {
    ticks: Arc<AtomicI64>,
}

impl FakeClock {
    pub fn new() -> Self {
        Self {
            ticks: Arc::new(AtomicI64::new(1_000_000)),
        }
    }

    /// Timestamp source for [`NavigationPace::with_timestamp`].
    pub fn timestamp(&self) -> Box<dyn Fn() -> i64 + Send + Sync> {
        let ticks = Arc::clone(&self.ticks);
        Box::new(move || ticks.load(Ordering::SeqCst))
    }

    pub fn advance_ms(&self, milliseconds: f64) {
        let delta = (milliseconds * TICKS_PER_SECOND as f64 / 1000.0).round() as i64;
        self.ticks.fetch_add(delta, Ordering::SeqCst);
    }

    /// A pace whose timestamps come from this clock.
    pub fn pace(&self) -> Arc<NavigationPace> {
        Arc::new(NavigationPace::with_timestamp(self.timestamp()))
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

// ---- one-shot timers (C# ManualTime / ManualTimer) --------------------------------------------------------------------

/// One timer armed through [`ManualTimers`].
#[derive(Clone)]
pub struct ManualTimerState {
    pub due: Duration,
    pub disposed: Arc<AtomicBool>,
    callback: TimerCallback,
}

/// Manual factory: timers never fire on their own. `fire` runs the callback EVEN AFTER dispose, like C#.
#[derive(Default)]
pub struct ManualTimers {
    created: Mutex<Vec<ManualTimerState>>,
}

struct ManualTimerHandle {
    disposed: Arc<AtomicBool>,
}

impl OneShotTimer for ManualTimerHandle {
    fn dispose(&self) {
        self.disposed.store(true, Ordering::SeqCst);
    }
}

impl OneShotTimerFactory for ManualTimers {
    fn create_timer(&self, due: Duration, callback: TimerCallback) -> Box<dyn OneShotTimer> {
        let disposed = Arc::new(AtomicBool::new(false));
        lock(&self.created).push(ManualTimerState {
            due,
            disposed: Arc::clone(&disposed),
            callback,
        });
        Box::new(ManualTimerHandle { disposed })
    }
}

impl ManualTimers {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// C# `Timers.Count`: every timer ever armed.
    pub fn created(&self) -> usize {
        lock(&self.created).len()
    }

    /// Due times of every armed timer, in order.
    pub fn dues(&self) -> Vec<Duration> {
        lock(&self.created).iter().map(|t| t.due).collect()
    }

    /// Timers armed and not disposed.
    pub fn live(&self) -> Vec<ManualTimerState> {
        lock(&self.created)
            .iter()
            .filter(|t| !t.disposed.load(Ordering::SeqCst))
            .cloned()
            .collect()
    }

    /// Fires the `index`-th armed timer (also when it was disposed: C# runs the callback regardless).
    pub fn fire(&self, index: usize) {
        let callback = lock(&self.created)[index].callback.clone();
        callback();
    }

    /// Index of the newest timer that is not disposed (C# `Pending`).
    pub fn pending_index(&self) -> Option<usize> {
        lock(&self.created)
            .iter()
            .rposition(|t| !t.disposed.load(Ordering::SeqCst))
    }

    /// `true` when the `index`-th timer was disposed.
    pub fn is_disposed(&self, index: usize) -> bool {
        lock(&self.created)[index].disposed.load(Ordering::SeqCst)
    }

    /// Fires the newest live timer; panics when none is live (a test bug).
    pub fn fire_latest_live(&self) {
        let callback = lock(&self.created)
            .iter()
            .rev()
            .find(|t| !t.disposed.load(Ordering::SeqCst))
            .map(|t| t.callback.clone())
            .expect("a live timer");
        callback();
    }
}

// ---- target (C# private Target : IPreloadTarget) ----------------------------------------------------------------------

/// Preload body of the fake target: `Err` is the decode error the C# `PreloadAsync` task faults with.
pub type PreloadHook = Arc<dyn Fn(&str, &CancelToken) -> Result<(), DecodeError> + Send + Sync>;
/// C# `KeyFailure`: `Some` makes `get_current_cache_key(entry)` fail for that path.
pub type KeyFailureHook = Arc<dyn Fn(&str) -> Option<KeyError> + Send + Sync>;

/// Fake preview cache. Keys are built like the real target (`ImageCacheKey::create` over the fake file system), so
/// the warmed-key set holds the same identities the scheduler compares.
pub struct FakeTarget {
    fs: Arc<FakeFileSystem>,
    cached: Mutex<HashSet<ImageCacheKey>>,
    disk_cached: Mutex<HashSet<String>>,
    decode_box: Mutex<DecodeBox>,
    started: Mutex<Vec<String>>,
    started_cv: Condvar,
    started_gates: Mutex<HashMap<String, Arc<Gate>>>,
    preload_thread: Mutex<HashMap<String, ThreadId>>,
    key_thread: Mutex<HashMap<String, ThreadId>>,
    key_calls: Mutex<HashMap<String, usize>>,
    on_preload: Mutex<Option<PreloadHook>>,
    key_failure: Mutex<Option<KeyFailureHook>>,
    active_viewer: AtomicI32,
    bytes: AtomicI64,
}

impl FakeTarget {
    pub fn new(fs: &Arc<FakeFileSystem>) -> Arc<Self> {
        Arc::new(Self {
            fs: Arc::clone(fs),
            cached: Mutex::new(HashSet::new()),
            disk_cached: Mutex::new(HashSet::new()),
            decode_box: Mutex::new(DecodeBox::new(100, 100)),
            started: Mutex::new(Vec::new()),
            started_cv: Condvar::new(),
            started_gates: Mutex::new(HashMap::new()),
            preload_thread: Mutex::new(HashMap::new()),
            key_thread: Mutex::new(HashMap::new()),
            key_calls: Mutex::new(HashMap::new()),
            on_preload: Mutex::new(None),
            key_failure: Mutex::new(None),
            active_viewer: AtomicI32::new(0),
            bytes: AtomicI64::new(0),
        })
    }

    /// C# `OnPreload` (`Func<string, CancellationToken, Task>`).
    pub fn set_on_preload(&self, hook: impl Fn(&str, &CancelToken) -> Result<(), DecodeError> + Send + Sync + 'static) {
        *lock(&self.on_preload) = Some(Arc::new(hook));
    }

    /// C# `KeyFailure`.
    pub fn set_key_failure(&self, hook: impl Fn(&str) -> Option<KeyError> + Send + Sync + 'static) {
        *lock(&self.key_failure) = Some(Arc::new(hook));
    }

    /// C# `HasDiskCachedPreview` (settable): the on-disk preview cache holds `path`.
    pub fn set_disk_cached(&self, path: &str) {
        lock(&self.disk_cached).insert(path.to_ascii_uppercase());
    }

    /// C# `ActiveViewerDecodes` (settable).
    pub fn set_active_viewer(&self, decodes: i32) {
        self.active_viewer.store(decodes, Ordering::SeqCst);
    }

    /// C# `CacheBytes` (settable).
    pub fn set_bytes(&self, bytes: i64) {
        self.bytes.store(bytes, Ordering::SeqCst);
    }

    /// C# `Box` (settable decode box of the current key).
    pub fn set_box(&self, decode_box: DecodeBox) {
        *lock(&self.decode_box) = decode_box;
    }

    /// The paths `preload` was called with, in call order (C# `Started`).
    pub fn started(&self) -> Vec<String> {
        lock(&self.started).clone()
    }

    /// C# `StartedCount(path)` (case-insensitive, like `OrdinalIgnoreCase`).
    pub fn started_count(&self, path: &str) -> usize {
        lock(&self.started)
            .iter()
            .filter(|p| p.eq_ignore_ascii_case(path))
            .count()
    }

    /// C# `WhenStarted(path)`: waits (guarded) until `preload` was entered for `path`; `true` when it was.
    pub fn wait_started(&self, path: &str) -> bool {
        self.started_gate(path).wait(None)
    }

    /// Waits (guarded) until `preload` was entered `count` times in total; `true` when it was.
    pub fn wait_started_count(&self, count: usize) -> bool {
        let start = Instant::now();
        let mut started = lock(&self.started);
        while started.len() < count {
            if start.elapsed() >= GUARD {
                return false;
            }
            let (guard, _) = self
                .started_cv
                .wait_timeout(started, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner);
            started = guard;
        }
        true
    }

    /// The thread `preload` ran on for `path` (C# `PreloadThread`).
    pub fn preload_thread(&self, path: &str) -> Option<ThreadId> {
        lock(&self.preload_thread)
            .iter()
            .find(|(p, _)| p.eq_ignore_ascii_case(path))
            .map(|(_, t)| *t)
    }

    /// The thread `get_current_cache_key(entry)` ran on for `path` (C# `KeyThread`).
    pub fn key_thread(&self, path: &str) -> Option<ThreadId> {
        lock(&self.key_thread)
            .iter()
            .find(|(p, _)| p.eq_ignore_ascii_case(path))
            .map(|(_, t)| *t)
    }

    /// Number of `get_current_cache_key(entry)` calls for `path` (C# `KeyCalls`).
    pub fn key_calls(&self, path: &str) -> usize {
        lock(&self.key_calls)
            .iter()
            .find(|(p, _)| p.eq_ignore_ascii_case(path))
            .map_or(0, |(_, n)| *n)
    }

    /// Marks `path` as already cached (C# `MarkCached`).
    pub fn mark_cached(&self, path: &str) {
        let key = self.key(path);
        lock(&self.cached).insert(key);
    }

    /// C# `Evict` (a compare/zoom decode pushed `path` out of the RAM cache).
    pub fn evict(&self, path: &str) {
        let key = self.key(path);
        lock(&self.cached).remove(&key);
    }

    /// The key the target builds for `path` (C# `GetCurrentCacheKey(path)`); panics on a missing file (a test bug).
    pub fn key(&self, path: &str) -> ImageCacheKey {
        let decode_box = *lock(&self.decode_box);
        ImageCacheKey::create(&*self.fs, path, false, decode_box, KeyOptions::default())
            .unwrap_or_else(|e| panic!("key for {path}: {e:?}"))
    }

    /// C# `TryGetCachedPreview(path)` convenience used by tests.
    pub fn is_cached(&self, path: &str) -> bool {
        lock(&self.cached).contains(&self.key(path))
    }

    fn started_gate(&self, path: &str) -> Arc<Gate> {
        let mut gates = lock(&self.started_gates);
        let key = path.to_ascii_uppercase();
        Arc::clone(gates.entry(key).or_insert_with(|| Arc::new(Gate::default())))
    }
}

impl PreloadTarget for FakeTarget {
    fn try_get_cached_preview_for_path(&self, path: &str) -> bool {
        self.is_cached(path)
    }

    fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool {
        lock(&self.cached).contains(key)
    }

    fn preload(&self, path: &str, cancel: &CancelToken) -> Result<(), DecodeError> {
        lock(&self.preload_thread).insert(path.to_owned(), thread::current().id());
        lock(&self.started).push(path.to_owned());
        self.started_cv.notify_all();
        self.started_gate(path).open();
        let hook = lock(&self.on_preload).clone();
        if let Some(hook) = hook {
            hook(path, cancel)?;
        }
        let key = self.key(path);
        lock(&self.cached).insert(key);
        Ok(())
    }

    fn get_current_cache_key_for_path(&self, path: &str) -> Result<ImageCacheKey, KeyError> {
        Ok(ImageCacheKey::create(
            &*self.fs,
            path,
            false,
            *lock(&self.decode_box),
            KeyOptions::default(),
        )?)
    }

    fn get_current_cache_key(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        let path = entry.path();
        lock(&self.key_thread).insert(path.to_owned(), thread::current().id());
        *lock(&self.key_calls).entry(path.to_owned()).or_insert(0) += 1;
        let failure = lock(&self.key_failure).clone();
        if let Some(error) = failure.and_then(|hook| hook(path)) {
            return Err(error);
        }
        Ok(ImageCacheKey::create_for_entry(
            &*self.fs,
            entry,
            false,
            *lock(&self.decode_box),
            KeyOptions::default(),
        )?)
    }

    fn cache_count(&self) -> i32 {
        i32::try_from(lock(&self.cached).len()).unwrap_or(i32::MAX)
    }

    fn cache_bytes(&self) -> i64 {
        self.bytes.load(Ordering::SeqCst)
    }

    fn active_viewer_decodes(&self) -> i32 {
        self.active_viewer.load(Ordering::SeqCst)
    }

    fn has_disk_cached_preview(&self, key: &ImageCacheKey) -> bool {
        lock(&self.disk_cached).contains(key.path())
    }
}

// ---- fixtures and scheduler construction ------------------------------------------------------------------------------

/// Catalog entries `prefix-0.jpg .. prefix-(count-1).jpg`, registered on the fake file system (C# `Entries(count)`).
pub fn entries(fs: &FakeFileSystem, count: usize, prefix: &str) -> Arc<[CatalogEntry]> {
    entries_with_extension(fs, count, prefix, "jpg")
}

/// [`entries`] with another file extension (C# `.cr3` RAW fixtures).
pub fn entries_with_extension(fs: &FakeFileSystem, count: usize, prefix: &str, extension: &str) -> Arc<[CatalogEntry]> {
    (0..count)
        .map(|i| {
            let path = format!("C:\\preload-fake\\{prefix}-{i}.{extension}");
            fs.add_file(&path, b"image");
            CatalogEntry::new(path).expect("valid fake path")
        })
        .collect::<Vec<_>>()
        .into()
}

/// The snapshot callback of a fixed catalog (C# `() => entries`).
pub fn snapshot_of(entries: Arc<[CatalogEntry]>) -> EntriesSnapshotFn {
    Arc::new(move || -> Result<Arc<[CatalogEntry]>, CallbackError> { Ok(Arc::clone(&entries)) })
}

/// The viewer-busy worker limit for `workers` on this machine (C# `ViewerBusyLimit`).
pub fn viewer_busy_limit(workers: i32) -> i32 {
    let cpus = thread::available_parallelism().map_or(1, |n| i32::try_from(n.get()).unwrap_or(i32::MAX));
    workers.min(2.max(cpus / 3))
}

/// Construction options of [`create`] (every field defaults to the C# default; the environment is EMPTY).
pub struct Setup {
    pub workers: i32,
    pub window: PreloadWindow,
    pub whole_folder: bool,
    /// C# `FullFolderThresholdBytes` override (`None` = derived from `whole_folder`).
    pub full_folder_threshold: Option<i64>,
    /// C# `MemoryLoadLimit` (`None` = the PreloadOptions default 0.80).
    pub memory_load_limit: Option<f64>,
    pub metrics: Option<Arc<ReviewMetrics>>,
    pub pace: Option<Arc<NavigationPace>>,
    pub log: Option<Arc<dyn Log>>,
    pub probe: Option<Arc<dyn MemoryProbe>>,
    pub snapshot_version: Option<SnapshotVersionFn>,
    pub timers: Option<Arc<dyn OneShotTimerFactory>>,
    pub env_lookup: Option<EnvLookup>,
    pub processor_count: Option<i32>,
    pub clock_ms: Option<MillisecondClock>,
    pub prefetch: Option<pr_imaging::preload::scheduler::PrefetchSourceBytesFn>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            workers: 8,
            window: PreloadWindow::DEFAULT,
            whole_folder: false,
            full_folder_threshold: None,
            memory_load_limit: None,
            metrics: None,
            pace: None,
            log: None,
            probe: None,
            snapshot_version: None,
            timers: None,
            env_lookup: None,
            processor_count: None,
            clock_ms: None,
            prefetch: None,
        }
    }
}

impl Setup {
    pub fn new(workers: i32, window: PreloadWindow) -> Self {
        Self {
            workers,
            window,
            ..Self::default()
        }
    }

    pub fn full_folder_threshold(mut self, bytes: i64) -> Self {
        self.full_folder_threshold = Some(bytes);
        self
    }

    /// C# default `FullFolderThresholdBytes` (16 GiB) instead of the mutation-test `-1` of a non-whole-folder setup.
    pub fn memory_load_limit(mut self, limit: f64) -> Self {
        self.memory_load_limit = Some(limit);
        self
    }

    /// C# `prefetchSourceBytes` (BLOCKING source read on the worker thread).
    pub fn prefetch(
        mut self,
        prefetch: impl Fn(&str, &CancelToken) -> Result<(), DecodeError> + Send + Sync + 'static,
    ) -> Self {
        self.prefetch = Some(Arc::new(prefetch));
        self
    }

    pub fn default_threshold(mut self) -> Self {
        self.full_folder_threshold = Some(16 << 30);
        self
    }

    pub fn whole_folder(mut self) -> Self {
        self.whole_folder = true;
        self
    }

    pub fn metrics(mut self, metrics: Arc<ReviewMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub fn pace(mut self, pace: Arc<NavigationPace>) -> Self {
        self.pace = Some(pace);
        self
    }

    pub fn log(mut self, log: &Arc<CaptureLog>) -> Self {
        self.log = Some(Arc::clone(log) as Arc<dyn Log>);
        self
    }

    pub fn probe(mut self, probe: Arc<dyn MemoryProbe>) -> Self {
        self.probe = Some(probe);
        self
    }

    pub fn timers(mut self, timers: Arc<dyn OneShotTimerFactory>) -> Self {
        self.timers = Some(timers);
        self
    }

    pub fn snapshot_version(mut self, version: SnapshotVersionFn) -> Self {
        self.snapshot_version = Some(version);
        self
    }

    pub fn env(mut self, lookup: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Self {
        self.env_lookup = Some(Arc::new(lookup));
        self
    }

    pub fn processor_count(mut self, count: i32) -> Self {
        self.processor_count = Some(count);
        self
    }

    pub fn clock_ms(mut self, clock: MillisecondClock) -> Self {
        self.clock_ms = Some(clock);
        self
    }
}

/// C# `Create(target, entries, workers, window, ...)`: the main constructor with the given setup.
pub fn create<T: PreloadTarget + 'static>(
    target: &Arc<T>,
    entries: &Arc<[CatalogEntry]>,
    setup: Setup,
) -> PreloadScheduler {
    create_with_snapshot(target, snapshot_of(Arc::clone(entries)), setup)
}

/// [`create`] with an arbitrary entries callback (C# `() => entries` replaced by a changing or failing one).
pub fn create_with_snapshot<T: PreloadTarget + 'static>(
    target: &Arc<T>,
    snapshot: EntriesSnapshotFn,
    setup: Setup,
) -> PreloadScheduler {
    let mut options = PreloadOptions {
        worker_count: setup.workers,
        full_folder_threshold_bytes: setup.full_folder_threshold.unwrap_or(if setup.whole_folder {
            1_000_000_000
        } else {
            -1
        }),
        window: setup.window,
        ..PreloadOptions::default()
    };
    if let Some(limit) = setup.memory_load_limit {
        options.memory_load_limit = limit;
    }
    let probe = setup.probe.unwrap_or_else(|| FakeProbe::answering(true));
    let deps = PreloadSchedulerDeps {
        options: Some(options),
        log: setup.log,
        prefetch_source_bytes: setup.prefetch,
        pace: setup.pace,
        snapshot_version: setup.snapshot_version,
        timers: setup.timers,
        env_lookup: Some(setup.env_lookup.unwrap_or_else(|| Arc::new(|_: &str| None))),
        processor_count: setup.processor_count,
        clock_ms: setup.clock_ms,
    };
    PreloadScheduler::new(
        Arc::clone(target) as Arc<dyn PreloadTarget>,
        setup.metrics.unwrap_or_else(|| Arc::new(ReviewMetrics::new())),
        snapshot,
        probe,
        deps,
    )
}

/// A gated decode body: returns `Ok` when `gate` opens; `Err(Canceled)` when the lifetime token is canceled first (C#
/// `gate.Task.WaitAsync(token)`). Pass `None` as the token in [`gated_ignoring_cancel`] for C# gates that ignore it.
pub fn gated(gate: &Arc<Gate>) -> impl Fn(&str, &CancelToken) -> Result<(), DecodeError> + Send + Sync + 'static {
    let gate = Arc::clone(gate);
    move |_path, token| {
        if gate.wait(Some(token)) {
            Ok(())
        } else {
            Err(DecodeError::Canceled)
        }
    }
}

/// A gated decode body that ignores cancellation (C# `OnPreload = (_, _) => gate.Task`).
pub fn gated_ignoring_cancel(
    gate: &Arc<Gate>,
) -> impl Fn(&str, &CancelToken) -> Result<(), DecodeError> + Send + Sync + 'static {
    let gate = Arc::clone(gate);
    move |_path, _token| {
        if gate.wait(None) {
            Ok(())
        } else {
            Err(DecodeError::Canceled)
        }
    }
}

/// Counts the candidates `scheduler` dispatches, on the dispatching thread (`set_dispatched_for_tests`).
///
/// DIVERGENCE (test seam): C# fakes read `Started` synchronously because `PreloadOneAsync` enters the target's
/// `PreloadAsync` on the dispatching thread before the loop examines the next candidate; Rust decodes on a pool thread
/// (R0003), so a probe that must react to "a decode was started" reads this counter instead (same point of the loop).
pub fn count_dispatches(scheduler: &PreloadScheduler, counter: &Arc<AtomicUsize>) {
    let counter = Arc::clone(counter);
    scheduler.set_dispatched_for_tests(Some(Arc::new(move |_path: &str| {
        counter.fetch_add(1, Ordering::SeqCst);
    })));
}

/// Waits (guarded) for `run` to complete; `true` when it did.
pub fn finished(run: &RunHandle) -> bool {
    run.wait_timeout(GUARD)
}

/// A fresh fake file system (keys are built from it).
pub fn fake_fs() -> Arc<FakeFileSystem> {
    Arc::new(FakeFileSystem::new())
}

/// An `io::Error` of the given kind with a message (C# `IOException` / `UnauthorizedAccessException`).
pub fn io_error(kind: io::ErrorKind, message: &str) -> io::Error {
    io::Error::new(kind, message)
}
