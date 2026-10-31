//! Port of `PhotoReview.App.Shared/FileHashService.cs` (C# `IFileHasher`, `FileHashService`, baseline d92d6aae).
//! Unit W8-04e. Contract: `contracts/W8-04e.md`.
//!
//! Content-hash lookup used by the duplicate check (C-06) and by Compare (C-25): SHA-256 as upper-case hex (C# `Convert.ToHexString`),
//! a 16 MiB LRU keyed by path (OrdinalIgnoreCase), one shared computation per path, and a post-hash check that rejects a file that
//! changed while it was read.
//!
//! Threading (D-W8-2): `FileHasher::get` BLOCKS the calling thread. Call it from a worker thread, never the UI thread
//! (the same rule as `SourceBytesCache`). A shared computation runs on its own worker and is completed through its `Condvar`.
//! The C# `async` surface becomes this blocking call; no async runtime is added.
//!
//! OS access is injected: stat through `FileSystem::try_get_file_stat`, the streamed read through `SourceReader` (shared
//! ReadWrite | Delete, SequentialScan, the INV-8 rule), the whole-file path through `SourceBytesCache`.

#![allow(unused_variables, dead_code)] // retained generated test seam fields

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use pr_core::abstractions::{FileSystem, PhysicalSourceReader, SourceReadPriority, SourceReader};
use pr_core::caching::lru::{BoundedLruCache, InvalidCapacity};
use pr_core::catalog::types::is_raw_path;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::PathError;
use pr_core::text::ignore_case_key;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache::source_bytes::{SourceBytesCache, SourceBytesError};
use pr_imaging::decoder_traits::UserText;

/// C# `new BoundedLruCache<string, HashEntry>(16L * 1024 * 1024, ...)`: the byte budget of the hash cache.
pub const HASH_CACHE_CAPACITY_BYTES: i64 = 16 * 1024 * 1024;

/// C# `new FileStream(path, ..., 1024 * 1024, ...)`: the streamed read buffer, also the chunk size for cancellation checks.
pub const HASH_STREAM_BUFFER_BYTES: usize = 1024 * 1024;

/// How often a caller waiting for a shared computation looks at its own cancel token (`CancelToken` has no callback).
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// C# `IFileHasher`: the seam the duplicate check uses so tests can block or cancel a hash deterministically.
/// Blocking (see the module doc); the consumers in B2 (`DuplicateCleanupController`, `CompareViewModel`) take `&dyn FileHasher`.
pub trait FileHasher: Send + Sync {
    /// The upper-case hex SHA-256 of the file at `path` (64 characters). Errors: [`FileHashError::Canceled`] when `cancel` is
    /// set before the hash is published; the file errors otherwise.
    fn get(&self, path: &Path, cancel: &CancelToken) -> Result<String, FileHashError>;
}

/// The localized sentence of the one error this service raises with UI text (`err.io.fileChangedWhileHashing`,
/// "The file changed while its hash was being computed: {path}"). The App supplies the catalog-backed implementation; the default is
/// [`EnglishFileHashTexts`] (C# `UserFacingError.Localized(..., () => Tr.ErrIoFileChangedWhileHashing(path))`).
pub trait FileHashTexts: Send + Sync {
    fn file_changed_while_hashing(&self, path: &str) -> String;
}

/// The built-in English catalog text, as `assets/languages/en.json` renders it.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnglishFileHashTexts;

impl FileHashTexts for EnglishFileHashTexts {
    fn file_changed_while_hashing(&self, path: &str) -> String {
        format!("The file changed while its hash was being computed: {path}")
    }
}

/// Errors of [`FileHasher::get`]. `Clone` because one shared computation hands its error to every caller that waited on it.
/// `Debug` is written by hand because [`UserText`] is a closure.
#[derive(Clone)]
pub enum FileHashError {
    /// C# `OperationCanceledException` from the caller's token (or from a computation whose last waiter left).
    Canceled,
    /// C# `Path.GetFullPath` rejected the path (`ArgumentException` or `PathTooLongException`).
    Path(PathError),
    /// The file is missing or cannot be read (C# `FileNotFoundException`, `IOException`, `UnauthorizedAccessException`).
    Io(Arc<io::Error>),
    /// The whole-file byte cache failed (C# exception from `SourceBytesCache.GetOrRead`); carries its own UI text when it has one.
    Source(SourceBytesError),
    /// C# `IOException` from `CompleteIfUnchanged`: the length or last write time changed while hashing. `user_text` renders
    /// `file_changed_while_hashing` in the language current when it is displayed.
    ChangedWhileHashing { path: String, user_text: UserText },
}

impl fmt::Debug for FileHashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Canceled => f.write_str("operation canceled"),
            Self::Path(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Source(e) => write!(f, "{e}"),
            Self::ChangedWhileHashing { path, .. } => {
                write!(f, "The file changed while its hash was being computed: {path}")
            }
        }
    }
}

impl fmt::Display for FileHashError {
    /// English text for logs. The UI uses [`FileHashError::describe`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for FileHashError {}

impl FileHashError {
    /// C# `UserFacingError.IsLocalized(ex)`: true for [`FileHashError::ChangedWhileHashing`] and for a [`FileHashError::Source`] that
    /// carries UI text.
    pub fn is_localized(&self) -> bool {
        match self {
            Self::ChangedWhileHashing { .. } => true,
            Self::Source(e) => e.is_localized(),
            _ => false,
        }
    }

    /// The UI sentence for this error when it has one (see [`FileHashError::is_localized`]); `None` otherwise.
    pub fn user_text(&self) -> Option<UserText> {
        match self {
            Self::ChangedWhileHashing { user_text, .. } => Some(user_text.clone()),
            Self::Source(e) => e.user_text(),
            _ => None,
        }
    }

    /// C# `UserFacingError.Describe(ex)`: the localized sentence when there is one, else the English [`Display`](fmt::Display) text.
    pub fn describe(&self) -> String {
        match self.user_text() {
            Some(t) => t(),
            None => self.to_string(),
        }
    }
}

/// C# `FileHashService.HashEntry` (`private sealed record`): what the cache keeps for one path. `path` is the full path the
/// entry was computed for (C# `Path`); the key of the cache is `ignore_case_key(path)`.
#[derive(Debug, Clone)]
struct HashEntry {
    path: String,
    length: u64,
    last_write_utc: DateTimeUtc,
    hash: String,
}

/// C# `FileHashService.InFlight` (`private sealed class : IDisposable`): one shared computation for a path.
/// `state` holds the waiter count and the result, `done` wakes every waiter, `cancel` is the computation's token (cancelled
/// only when its last waiter has left, C# `CancelQuietly`).
struct InFlight {
    state: Mutex<InFlightState>,
    done: Condvar,
    cancel: CancelToken,
}

struct InFlightState {
    /// C# `Waiters` (`Interlocked` counter): callers currently waiting on this computation.
    waiters: usize,
    /// The owner thread has started its computation, so a test waiter count cannot race thread startup.
    started: bool,
    /// `None` while the computation runs. Set once, when the computation finishes (ok, error or canceled).
    result: Option<Result<String, FileHashError>>,
}

/// C# `FileHashService` (`public sealed class FileHashService : IFileHasher`).
pub struct FileHashService {
    fs: Arc<dyn FileSystem>,
    reader: Arc<dyn SourceReader>,
    source_bytes: Option<Arc<SourceBytesCache>>,
    texts: Arc<dyn FileHashTexts>,
    /// Keyed by `ignore_case_key(full path)` (C# `StringComparer.OrdinalIgnoreCase`); weight = UTF-16 length * 2 + 64 (C# `Length`).
    cache: Arc<BoundedLruCache<String, HashEntry>>,
    /// Keyed by `ignore_case_key(full path)`. An entry is removed by identity (`Arc::ptr_eq`) when its computation finishes.
    in_flight: Arc<Mutex<HashMap<String, Arc<InFlight>>>>,
    /// C# `_generation`: [`FileHashService::clear`] increments it; a computation that started before a clear is not cached.
    generation: Arc<Mutex<u64>>,
}

impl fmt::Debug for FileHashService {
    /// Required by the workspace lint `missing_debug_implementations`. The porter writes
    /// `f.debug_struct("FileHashService").finish_non_exhaustive()` (the trait objects have no Debug).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileHashService").finish_non_exhaustive()
    }
}

impl FileHashService {
    /// C# `new FileHashService(sourceBytesCache)`: the real file system, `PhysicalSourceReader`, the English texts.
    /// `source_bytes` is the optional whole-file cache (C# `SourceBytesCache?`). `Err` is `InvalidCapacity` and cannot happen for
    /// [`HASH_CACHE_CAPACITY_BYTES`]; it is returned, not unwrapped (AGENTS.md: no `expect` on user-data paths).
    pub fn new(source_bytes: Option<Arc<SourceBytesCache>>) -> Result<Self, InvalidCapacity> {
        Self::with_seams(
            source_bytes,
            Arc::new(pr_core::io::physical::PhysicalFileSystem),
            Arc::new(PhysicalSourceReader),
            Arc::new(EnglishFileHashTexts),
        )
    }

    /// The same service with every OS seam supplied: tests pass fakes (`FileSystem`, `SourceReader`) and their own texts.
    pub fn with_seams(
        source_bytes: Option<Arc<SourceBytesCache>>,
        fs: Arc<dyn FileSystem>,
        reader: Arc<dyn SourceReader>,
        texts: Arc<dyn FileHashTexts>,
    ) -> Result<Self, InvalidCapacity> {
        Ok(Self {
            fs,
            reader,
            source_bytes,
            texts,
            cache: Arc::new(BoundedLruCache::new(HASH_CACHE_CAPACITY_BYTES, |e: &HashEntry| {
                (e.path.encode_utf16().count() as i64)
                    .saturating_mul(2)
                    .saturating_add(64)
            })?),
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            generation: Arc::new(Mutex::new(0)),
        })
    }

    /// C# `Clear()`: bumps the generation and empties the cache. A computation already in flight still returns its hash but does
    /// not publish it (C# `generation == Volatile.Read(ref _generation)` check in `CompleteIfUnchanged`).
    pub fn clear(&self) {
        *self.generation.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        self.cache.clear();
    }

    /// Test seam (C# `HashCacheCount` reflection): the number of cached paths.
    pub fn cache_count_for_tests(&self) -> usize {
        self.cache.count()
    }

    /// Test seam: how many callers are waiting on the in-flight computation for `path` (`None` when there is none). Lets a test
    /// know a second caller has joined before it releases the computation.
    pub fn in_flight_waiters_for_tests(&self, path: &Path) -> Option<usize> {
        let key = ignore_case_key(&path.to_string_lossy());
        self.in_flight
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
            .and_then(|v| {
                let state = v.state.lock().unwrap_or_else(|p| p.into_inner());
                state.started.then_some(state.waiters)
            })
    }

    /// C# `ComputeAndCacheAsync(path, length, lastWriteUtc, generation, token)`, run on the worker of a shared computation.
    /// Whole-file path (C# `CanCache(length)` and not a RAW file, see [`pr_core::catalog::types::is_raw_path`]): reads through
    /// `SourceBytesCache` and hashes the bytes in one call. Otherwise streams the file through `SourceReader` in
    /// [`HASH_STREAM_BUFFER_BYTES`] chunks, checking `cancel` before each chunk.
    fn compute_and_cache(
        &self,
        path: &str,
        length: u64,
        last_write_utc: DateTimeUtc,
        generation: u64,
        cancel: &CancelToken,
    ) -> Result<String, FileHashError> {
        let path_obj = Path::new(path);
        if cancel.is_canceled() {
            return Err(FileHashError::Canceled);
        }
        let hash = if let Some(cache) = &self.source_bytes {
            if length <= i64::MAX as u64 && !is_raw_path(path) && cache.can_cache(length as i64) {
                sha256_hex(&cache.get_or_read(path_obj).map_err(FileHashError::Source)?)
            } else {
                let mut r = self
                    .reader
                    .open_source(path_obj, SourceReadPriority::Viewer, HASH_STREAM_BUFFER_BYTES)
                    .map_err(|e| FileHashError::Io(Arc::new(e)))?;
                Self::hash_stream(&mut *r, cancel)?
            }
        } else {
            let mut r = self
                .reader
                .open_source(path_obj, SourceReadPriority::Viewer, HASH_STREAM_BUFFER_BYTES)
                .map_err(|e| FileHashError::Io(Arc::new(e)))?;
            Self::hash_stream(&mut *r, cancel)?
        };
        self.complete_if_unchanged(path, length, last_write_utc, generation, hash)
    }

    /// C# `CompleteIfUnchanged(path, length, lastWriteUtc, generation, hash)`.
    fn complete_if_unchanged(
        &self,
        path: &str,
        length: u64,
        last_write_utc: DateTimeUtc,
        generation: u64,
        hash: String,
    ) -> Result<String, FileHashError> {
        let stat = self
            .fs
            .try_get_file_stat(Path::new(path))
            .map_err(|e| FileHashError::Io(Arc::new(e)))?
            .ok_or_else(|| FileHashError::Io(Arc::new(io::Error::new(io::ErrorKind::NotFound, "file not found"))))?;
        if stat.length != length || stat.last_write_utc != last_write_utc {
            let p = path.to_owned();
            let t = self.texts.clone();
            return Err(FileHashError::ChangedWhileHashing {
                path: p.clone(),
                user_text: Arc::new(move || t.file_changed_while_hashing(&p)),
            });
        }
        if *self.generation.lock().unwrap_or_else(|p| p.into_inner()) == generation {
            self.cache.set(
                ignore_case_key(path),
                HashEntry {
                    path: path.to_owned(),
                    length,
                    last_write_utc,
                    hash: hash.clone(),
                },
            );
        }
        Ok(hash)
    }

    /// Streams `reader` through the incremental SHA-256 in [`HASH_STREAM_BUFFER_BYTES`] chunks and returns the upper-case hex digest.
    /// `cancel` is checked before every chunk (C# `SHA256.HashDataAsync(stream, token)`).
    fn hash_stream(reader: &mut dyn io::Read, cancel: &CancelToken) -> Result<String, FileHashError> {
        let mut data = Vec::new();
        let mut buf = vec![0; HASH_STREAM_BUFFER_BYTES];
        loop {
            if cancel.is_canceled() {
                return Err(FileHashError::Canceled);
            }
            let n = reader.read(&mut buf).map_err(|e| FileHashError::Io(Arc::new(e)))?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
        }
        if cancel.is_canceled() {
            return Err(FileHashError::Canceled);
        }
        Ok(sha256_hex(&data))
    }
}

fn sha256_hex(data: &[u8]) -> String {
    pr_core::session::store::sha256(data)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

impl FileHasher for FileHashService {
    /// C# `GetAsync(path, cancellationToken)`, as a blocking call. Order of work:
    /// 1. `get_full_path(path)` (C# `Path.GetFullPath`); a rejected path is [`FileHashError::Path`].
    /// 2. `try_get_file_stat` on the full path; `Ok(None)` or an error is [`FileHashError::Io`] (C# `new FileInfo(...).Length` throws).
    /// 3. A cache hit whose length and last write time equal the current stat is returned without reading (C# `TryGet` + `Length` check).
    /// 4. Otherwise `cancel.check()` and join or start the shared computation for the key (see the joining rules below).
    ///
    /// Joining rules (C# `GetAsync` loop): a waiter that is cancelled leaves; when it was the last waiter the computation's token is
    /// cancelled. A waiter that finds a computation already cancelled by its previous waiters removes that entry and starts a fresh
    /// one (the entry is never returned as a result). A cancelled caller always gets [`FileHashError::Canceled`]; it never sees
    /// another caller's cancellation.
    fn get(&self, path: &Path, cancel: &CancelToken) -> Result<String, FileHashError> {
        if cancel.is_canceled() {
            return Err(FileHashError::Canceled);
        }
        let full = pr_core::text::dotnet_path::get_full_path(&path.to_string_lossy()).map_err(FileHashError::Path)?;
        let stat = self
            .fs
            .try_get_file_stat(Path::new(&full))
            .map_err(|e| FileHashError::Io(Arc::new(e)))?
            .ok_or_else(|| FileHashError::Io(Arc::new(io::Error::new(io::ErrorKind::NotFound, "file not found"))))?;
        let key = ignore_case_key(&full);
        if let Some(entry) = self.cache.try_get(&key) {
            if entry.length == stat.length && entry.last_write_utc == stat.last_write_utc {
                return Ok(entry.hash);
            }
            // A fingerprint mismatch invalidates the old entry before a replacement is computed.
            self.cache.remove(&key);
        }
        cancel.check().map_err(|_| FileHashError::Canceled)?;
        loop {
            let (flight, owner) = {
                let mut map = self.in_flight.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(existing) = map.get(&key) {
                    let existing = existing.clone();
                    let mut state = existing.state.lock().unwrap_or_else(|p| p.into_inner());
                    // A finished flight is never joined: its worker may not have unregistered it yet, and its result
                    // belongs to the file as it was then (the cache already holds it when it is still valid).
                    if state.result.is_some() {
                        drop(state);
                        if map.get(&key).is_some_and(|f| Arc::ptr_eq(f, &existing)) {
                            map.remove(&key);
                        }
                        continue;
                    }
                    state.waiters += 1;
                    drop(state);
                    (existing, false)
                } else {
                    let flight = Arc::new(InFlight {
                        state: Mutex::new(InFlightState {
                            waiters: 1,
                            started: false,
                            result: None,
                        }),
                        done: Condvar::new(),
                        cancel: CancelToken::new(),
                    });
                    map.insert(key.clone(), flight.clone());
                    (flight, true)
                }
            };
            if owner {
                let generation = *self.generation.lock().unwrap_or_else(|p| p.into_inner());
                let fs = self.fs.clone();
                let reader = self.reader.clone();
                let source_bytes = self.source_bytes.clone();
                let texts = self.texts.clone();
                let cache = self.cache.clone();
                let generation_counter = self.generation.clone();
                let worker_key = key.clone();
                let worker_map = self.in_flight.clone();
                let worker_path = full.clone();
                let worker_flight = flight.clone();
                thread::spawn(move || {
                    {
                        let mut state = worker_flight.state.lock().unwrap_or_else(|p| p.into_inner());
                        state.started = true;
                        worker_flight.done.notify_all();
                    }
                    // A panic in the reader or the hasher must still complete the flight: otherwise every waiter (the
                    // duplicate check passes a token that never fires) would wait for a result that never comes, and the
                    // dead flight would stay in `in_flight`. The panic is a failed hash, never a hang.
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        compute_and_cache_parts(
                            ComputeContext {
                                fs: fs.as_ref(),
                                reader: reader.as_ref(),
                                source_bytes: source_bytes.as_deref(),
                                texts: texts.clone(),
                                cache: cache.as_ref(),
                                generation_counter: generation_counter.as_ref(),
                            },
                            &worker_path,
                            stat.length,
                            stat.last_write_utc,
                            generation,
                            &worker_flight.cancel,
                        )
                    }))
                    .unwrap_or_else(|payload| {
                        let text = payload
                            .downcast_ref::<&str>()
                            .map(|t| (*t).to_string())
                            .or_else(|| payload.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "unexpected error".to_string());
                        Err(FileHashError::Io(Arc::new(io::Error::other(format!(
                            "hash computation panicked: {text}"
                        )))))
                    });
                    let result = if worker_flight.cancel.is_canceled() {
                        Err(FileHashError::Canceled)
                    } else {
                        result
                    };
                    let mut state = worker_flight.state.lock().unwrap_or_else(|p| p.into_inner());
                    if worker_flight.cancel.is_canceled() {
                        state.result = Some(Err(FileHashError::Canceled));
                    } else {
                        state.result = Some(result);
                    }
                    worker_flight.done.notify_all();
                    drop(state);
                    let mut map = worker_map.lock().unwrap_or_else(|p| p.into_inner());
                    if !worker_flight.cancel.is_canceled()
                        && map.get(&worker_key).is_some_and(|f| Arc::ptr_eq(f, &worker_flight))
                    {
                        map.remove(&worker_key);
                    }
                });
            }
            let mut state = flight.state.lock().unwrap_or_else(|p| p.into_inner());
            while state.result.is_none() && !cancel.is_canceled() {
                // `CancelToken` has no callback, so a cancel does not wake this waiter by itself: wake periodically and look.
                state = flight
                    .done
                    .wait_timeout(state, CANCEL_POLL_INTERVAL)
                    .unwrap_or_else(|p| p.into_inner())
                    .0;
            }
            state.waiters = state.waiters.saturating_sub(1);
            if cancel.is_canceled() {
                if state.waiters == 0 {
                    flight.cancel.cancel();
                }
                return Err(FileHashError::Canceled);
            }
            let result = state.result.clone().unwrap_or(Err(FileHashError::Canceled));
            drop(state);
            if matches!(result, Err(FileHashError::Canceled)) {
                self.cache.remove(&key);
                continue;
            }
            return result;
        }
    }
}

struct ComputeContext<'a> {
    fs: &'a dyn FileSystem,
    reader: &'a dyn SourceReader,
    source_bytes: Option<&'a SourceBytesCache>,
    texts: Arc<dyn FileHashTexts>,
    cache: &'a BoundedLruCache<String, HashEntry>,
    generation_counter: &'a Mutex<u64>,
}

fn compute_and_cache_parts(
    context: ComputeContext<'_>,
    path: &str,
    length: u64,
    last_write_utc: DateTimeUtc,
    generation: u64,
    cancel: &CancelToken,
) -> Result<String, FileHashError> {
    let ComputeContext {
        fs,
        reader,
        source_bytes,
        texts,
        cache,
        generation_counter,
    } = context;
    if cancel.is_canceled() {
        return Err(FileHashError::Canceled);
    }
    let path_obj = Path::new(path);
    let hash = if let Some(bytes_cache) = source_bytes {
        if length <= i64::MAX as u64 && !is_raw_path(path) && bytes_cache.can_cache(length as i64) {
            sha256_hex(&bytes_cache.get_or_read(path_obj).map_err(FileHashError::Source)?)
        } else {
            let mut stream = reader
                .open_source(path_obj, SourceReadPriority::Viewer, HASH_STREAM_BUFFER_BYTES)
                .map_err(|e| FileHashError::Io(Arc::new(e)))?;
            FileHashService::hash_stream(&mut *stream, cancel)?
        }
    } else {
        let mut stream = reader
            .open_source(path_obj, SourceReadPriority::Viewer, HASH_STREAM_BUFFER_BYTES)
            .map_err(|e| FileHashError::Io(Arc::new(e)))?;
        FileHashService::hash_stream(&mut *stream, cancel)?
    };
    let stat = fs
        .try_get_file_stat(path_obj)
        .map_err(|e| FileHashError::Io(Arc::new(e)))?
        .ok_or_else(|| FileHashError::Io(Arc::new(io::Error::new(io::ErrorKind::NotFound, "file not found"))))?;
    if stat.length != length || stat.last_write_utc != last_write_utc {
        let path = path.to_owned();
        return Err(FileHashError::ChangedWhileHashing {
            path: path.clone(),
            user_text: Arc::new(move || texts.file_changed_while_hashing(&path)),
        });
    }
    if cancel.is_canceled() {
        return Err(FileHashError::Canceled);
    }
    if *generation_counter.lock().unwrap_or_else(|p| p.into_inner()) == generation {
        cache.set(
            ignore_case_key(path),
            HashEntry {
                path: path.to_owned(),
                length,
                last_write_utc,
                hash: hash.clone(),
            },
        );
    }
    Ok(hash)
}
