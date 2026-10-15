//! Port of `PhotoReview.Core/Session/SessionStore.cs` and `SessionJsonContext.cs` (C# baseline d92d6aae).
//!
//! One JSON file per reviewed folder (`<sessions_dir>\<SHA-256 hex of UPPER(canonical folder)>.json`) that remembers the
//! current image and the skipped images. Persistence is best effort (a locked or full disk must never break navigation):
//! [`SessionStore::save`] logs and swallows I/O failures, [`SessionStore::load`] turns a missing, empty, partial or corrupt
//! file into "no session". The file is written atomically without fsync (ADR 0007 section 2) and its bytes are identical to
//! the C# app's (2-space indent, CRLF, `System.Text.Json` default escaping, `UpdatedUtc` as ISO-8601).
//!
//! The constructor starts a background sweep that deletes `*.tmp` leftovers older than a day (C# R14).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use serde::Serialize;
use serde::de::{self, Deserializer as _, IgnoredAny, MapAccess, Visitor};
use serde_json::value::RawValue;

use crate::abstractions::{FileSystem, Log, NullLog};
use crate::app_paths::AppPaths;
use crate::file_actions::errors::get_path_root;
use crate::file_actions::journal_line::{DotNetJsonFormatter, JournalDateTime};
use crate::io::physical::PhysicalFileSystem;
use crate::text::ordinal_upper;
use crate::time::DateTimeUtc;

/// Age after which a `*.tmp` file in the sessions folder is a crash leftover: one day, in .NET ticks (C# `StaleTempAge`).
/// A temp file is stale only when it is STRICTLY older (exactly one day old is kept).
pub const STALE_TEMP_AGE_TICKS: i64 = 864_000_000_000;

/// C# `SessionState`: what is remembered about one folder.
///
/// `skipped` cannot hold the C# `null` elements a hand-edited file may contain (README D5-style divergence: nulls are
/// dropped on load). `updated_utc` is a C# `DateTime` with its kind (see [`JournalDateTime`]); the default is
/// `default(DateTime)` (0001-01-01, Unspecified), which is written as `"0001-01-01T00:00:00"`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionState {
    /// The folder as the caller spelled it (stored verbatim, never canonicalised).
    pub folder: String,
    /// The image shown last; `None` = nothing remembered.
    pub current_path: Option<String>,
    /// Images the user skipped, in order.
    pub skipped: Vec<String>,
    /// When the state was recorded.
    pub updated_utc: JournalDateTime,
}

impl SessionState {
    /// C# `new SessionState { Folder = folder }`: every other member default.
    pub fn new(folder: impl Into<String>) -> Self {
        Self {
            folder: folder.into(),
            ..Self::default()
        }
    }
}

/// The folder argument was empty or only white space (C# `ArgumentException.ThrowIfNullOrWhiteSpace`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlankFolder;

impl fmt::Display for BlankFolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The value cannot be an empty string or composed entirely of whitespace.")
    }
}

impl std::error::Error for BlankFolder {}

/// Why a folder string cannot name a session file (the .NET exceptions `Path.GetFullPath` throws).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPathError {
    /// Empty or white space only (`ArgumentException` from `ThrowIfNullOrWhiteSpace`).
    Blank,
    /// `Path.GetFullPath` rejected it: an embedded NUL or another invalid form (`ArgumentException` / `NotSupportedException`).
    Invalid,
    /// Longer than the OS allows (`PathTooLongException`).
    TooLong,
}

impl fmt::Display for SessionPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Blank => "The value cannot be an empty string or composed entirely of whitespace.",
            Self::Invalid => "The folder path is not valid.",
            Self::TooLong => "The folder path is too long.",
        })
    }
}

impl std::error::Error for SessionPathError {}

impl From<BlankFolder> for SessionPathError {
    fn from(_: BlankFolder) -> Self {
        Self::Blank
    }
}

/// Why a session file's text is "corrupt" (C# `JsonException`): the whole file is rejected, never partly used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionJsonError;

impl fmt::Display for SessionJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the session file is not valid session JSON")
    }
}

impl std::error::Error for SessionJsonError {}

/// Counts session writes for `ReviewMetrics` (C# `ReviewMetrics.RecordSessionWrite`). `ReviewMetrics` (W2-17) implements it;
/// any `Fn()` closure does too, so tests can pass a counter.
pub trait SessionWriteCounter: Send + Sync {
    /// One session file was written (called after a successful write, never for `load`).
    fn record_session_write(&self);
}

impl<F: Fn() + Send + Sync> SessionWriteCounter for F {
    fn record_session_write(&self) {
        self();
    }
}

/// Completion flag of the background startup sweep.
#[derive(Debug, Default)]
struct SweepDone {
    done: Mutex<bool>,
    changed: Condvar,
}

impl SweepDone {
    fn mark_done(&self) {
        *self.done.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    fn wait(&self) {
        let mut done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        while !*done {
            done = self.changed.wait(done).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Marks the sweep done when dropped, also while unwinding from a panic inside the sweep thread.
struct DoneGuard(Arc<SweepDone>);

impl Drop for DoneGuard {
    fn drop(&mut self) {
        self.0.mark_done();
    }
}

/// Loads and saves [`SessionState`] files. Share it as `Arc<SessionStore>`.
pub struct SessionStore {
    sessions_dir: PathBuf,
    file_system: Arc<dyn FileSystem>,
    metrics: Option<Arc<dyn SessionWriteCounter>>,
    log: Arc<dyn Log>,
    startup_sweep: Arc<SweepDone>,
}

impl fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionStore")
            .field("sessions_dir", &self.sessions_dir)
            .finish_non_exhaustive()
    }
}

impl SessionStore {
    /// C# `new SessionStore(paths, fileSystem, metrics)`: keeps `paths.sessions_dir` and starts the background sweep of
    /// stale temp files on a thread named `PhotoReview.SessionSweep` (it runs `sweep_stale_temp_files(DateTimeUtc::now())`
    /// and then marks the sweep done, also when it panics). The log starts as [`NullLog`](crate::abstractions::NullLog)
    /// (C# defaults to `FileLog.Default`); set the real one with [`with_log`](Self::with_log).
    pub fn new(
        paths: &AppPaths,
        file_system: Arc<dyn FileSystem>,
        metrics: Option<Arc<dyn SessionWriteCounter>>,
    ) -> Self {
        let sessions_dir = paths.sessions_dir.clone();
        let startup_sweep = Arc::new(SweepDone::default());
        let spawned = {
            let fs = Arc::clone(&file_system);
            let dir = sessions_dir.clone();
            let guard = DoneGuard(Arc::clone(&startup_sweep));
            std::thread::Builder::new()
                .name("PhotoReview.SessionSweep".to_owned())
                .spawn(move || {
                    let _guard = guard;
                    sweep(fs.as_ref(), &dir, DateTimeUtc::now());
                })
        };
        if spawned.is_err() {
            // No thread: nothing sweeps, and waiters must not hang.
            startup_sweep.mark_done();
        }
        Self {
            sessions_dir,
            file_system,
            metrics,
            log: Arc::new(NullLog),
            startup_sweep,
        }
    }

    /// C# parameterless constructor: `AppPaths::from_environment()` over the real file system
    /// ([`PhysicalFileSystem`](crate::io::physical::PhysicalFileSystem)), no metrics.
    pub fn from_environment() -> Self {
        Self::new(&AppPaths::from_environment(), Arc::new(PhysicalFileSystem), None)
    }

    /// C# `Log` test seam / DI: the log that receives the corrupt-file and failed-write warnings.
    #[must_use]
    pub fn with_log(mut self, log: Arc<dyn Log>) -> Self {
        self.log = log;
        self
    }

    /// C# `StartupSweepTask`: blocks until the background startup sweep finished. Callable any number of times from any
    /// thread. `load`/`save` never wait for the sweep.
    pub fn wait_for_startup_sweep(&self) {
        self.startup_sweep.wait();
    }

    /// C# `SweepStaleTempFiles(utcNow)` (internal): deletes the files `*.tmp` of the sessions folder whose last write is
    /// STRICTLY more than [`STALE_TEMP_AGE_TICKS`] before `utc_now`; returns how many were deleted. Best effort, never fails:
    /// a listing error returns 0, a per-file stat/delete error skips that file and continues.
    pub fn sweep_stale_temp_files(&self, utc_now: DateTimeUtc) -> usize {
        sweep(self.file_system.as_ref(), &self.sessions_dir, utc_now)
    }

    /// C# `Load(folder)`: the stored state, or "no session" (`SessionState::new(folder)`) when the folder string cannot name
    /// a file, the file is missing, unreadable, empty, partial, corrupt, or JSON `null`. A corrupt file also logs one
    /// warning `Session file '<path>' is empty or corrupt; starting without a session.` (not for `null`, not for I/O errors).
    /// A loaded state whose `folder` is blank gets the requested `folder`. `Err` only for a blank `folder` argument.
    pub fn load(&self, folder: &str) -> Result<SessionState, BlankFolder> {
        if folder.trim().is_empty() {
            return Err(BlankFolder);
        }
        // RV-C05: a malformed folder string (embedded NUL, overlong) cannot name a session file: "no session".
        let Ok(path) = self.get_path(folder) else {
            return Ok(SessionState::new(folder));
        };
        if !self.file_system.file_exists(&path) {
            return Ok(SessionState::new(folder));
        }
        let Ok(text) = self.file_system.read_all_text(&path) else {
            return Ok(SessionState::new(folder));
        };
        match Self::deserialize_state(&text) {
            Ok(Some(mut state)) => {
                if state.folder.trim().is_empty() {
                    state.folder = folder.to_owned();
                }
                Ok(state)
            }
            Ok(None) => Ok(SessionState::new(folder)),
            Err(_) => {
                // ADR 0007 section 2: without fsync a power loss can leave an empty/partial session file.
                self.log.warn(&format!(
                    "Session file '{}' is empty or corrupt; starting without a session.",
                    path.display()
                ));
                Ok(SessionState::new(folder))
            }
        }
    }

    /// C# `Save(state)`: creates the sessions folder, serialises with [`serialize_state`](Self::serialize_state), writes
    /// atomically WITHOUT fsync (`write_all_text_atomic(path, json, false)`), then counts the write in the metrics. Any
    /// I/O or path failure is logged as the warning `Session write failed for '<folder>': <reason>` and swallowed.
    /// `Err` only for a blank `state.folder`.
    pub fn save(&self, state: &SessionState) -> Result<(), BlankFolder> {
        if state.folder.trim().is_empty() {
            return Err(BlankFolder);
        }
        if let Err(reason) = self.try_save(state) {
            // Session persistence is best effort (a locked or full disk must not break navigation).
            self.log
                .warn(&format!("Session write failed for '{}': {reason}", state.folder));
        }
        Ok(())
    }

    fn try_save(&self, state: &SessionState) -> Result<(), String> {
        self.file_system
            .create_directory(&self.sessions_dir)
            .map_err(|e| e.to_string())?;
        let path = self.get_path(&state.folder).map_err(|e| e.to_string())?;
        let json = Self::serialize_state(state);
        // ADR 0007 section 2: atomic, no fsync.
        self.file_system
            .write_all_text_atomic(&path, &json, false)
            .map_err(|e: io::Error| e.to_string())?;
        if let Some(metrics) = &self.metrics {
            metrics.record_session_write();
        }
        Ok(())
    }

    /// C# `GetPath(folder)`: `<sessions_dir>\<key>.json` where key is [`session_key`](Self::session_key) of
    /// [`canonical_folder`](Self::canonical_folder)`(folder)`.
    pub fn get_path(&self, folder: &str) -> Result<PathBuf, SessionPathError> {
        let canonical = Self::canonical_folder(folder)?;
        let key = Self::session_key(&canonical);
        Ok(self.sessions_dir.join(format!("{key}.json")))
    }

    /// C# `CanonicalFolder(folder)` (internal static): `Path.GetFullPath` without a trailing separator; a drive/UNC root
    /// keeps its separator semantics of `Path.GetPathRoot` (`C:\` stays `C:\`, `\\server\share\` becomes `\\server\share`).
    /// Every spelling of one folder gives the same string (pinned by golden `session-key`).
    pub fn canonical_folder(folder: &str) -> Result<String, SessionPathError> {
        if folder.trim().is_empty() {
            return Err(SessionPathError::Blank);
        }
        if folder.contains('\0') {
            return Err(SessionPathError::Invalid);
        }
        // .NET `Path.GetFullPath` keeps an NT-namespace path (`\??\C:`) exactly as typed (no `..` resolution);
        // `std::path::absolute` would root it on the current drive.
        let full = if folder.starts_with(r"\??\") {
            folder.to_owned()
        } else {
            std::path::absolute(folder)
                .map_err(|e| {
                    if e.raw_os_error() == Some(206) {
                        SessionPathError::TooLong
                    } else {
                        SessionPathError::Invalid
                    }
                })?
                .to_string_lossy()
                .into_owned()
        };
        let root = get_path_root(&full).unwrap_or_default();
        if full.encode_utf16().count() > root.encode_utf16().count() {
            Ok(full.trim_end_matches(['\\', '/']).to_owned())
        } else {
            Ok(root)
        }
    }

    /// File name stem for a canonical folder: SHA-256 of the UTF-8 bytes of [`to_upper_invariant`](Self::to_upper_invariant)
    /// `(canonical)`, as 64 UPPERCASE hex digits (C# `Convert.ToHexString`).
    pub fn session_key(canonical: &str) -> String {
        use fmt::Write as _;
        let digest = sha256(Self::to_upper_invariant(canonical).as_bytes());
        let mut hex = String::with_capacity(64);
        for byte in digest {
            // Writing to a String cannot fail.
            let _ = write!(hex, "{byte:02X}");
        }
        hex
    }

    /// C# `string.ToUpperInvariant()` for the characters that occur in paths: per char
    /// [`text::ordinal_upper`](crate::text::ordinal_upper), except `U+017F` (LATIN SMALL LETTER LONG S) which maps to `S`.
    /// (The two tables agree on all 1422 other non-ASCII mappings; verified against .NET 10.)
    pub fn to_upper_invariant(text: &str) -> String {
        text.chars()
            .map(|c| if c == '\u{017F}' { 'S' } else { ordinal_upper(c) })
            .collect()
    }

    /// The exact file text C# writes for `state` (`JsonSerializer.Serialize(state, SessionJsonContext...)`): see the contract.
    pub fn serialize_state(state: &SessionState) -> String {
        const NL: &str = "\r\n";
        let mut out = String::with_capacity(128 + state.folder.len());
        out.push('{');
        out.push_str(NL);
        out.push_str("  \"Folder\": ");
        out.push_str(&json_string(&state.folder));
        out.push(',');
        out.push_str(NL);
        out.push_str("  \"CurrentPath\": ");
        match &state.current_path {
            Some(path) => out.push_str(&json_string(path)),
            None => out.push_str("null"),
        }
        out.push(',');
        out.push_str(NL);
        out.push_str("  \"Skipped\": ");
        if state.skipped.is_empty() {
            out.push_str("[]");
        } else {
            out.push('[');
            out.push_str(NL);
            for (index, item) in state.skipped.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                    out.push_str(NL);
                }
                out.push_str("    ");
                out.push_str(&json_string(item));
            }
            out.push_str(NL);
            out.push_str("  ]");
        }
        out.push(',');
        out.push_str(NL);
        out.push_str("  \"UpdatedUtc\": ");
        out.push_str(&json_string(&state.updated_utc.to_json()));
        out.push_str(NL);
        out.push('}');
        out
    }

    /// C# `JsonSerializer.Deserialize<SessionState>(json)`: `Ok(None)` for JSON `null`, `Ok(Some(state))` for an object
    /// (missing members keep their defaults, `Skipped: null` becomes empty), `Err` where C# throws `JsonException`.
    pub fn deserialize_state(json: &str) -> Result<Option<SessionState>, SessionJsonError> {
        if nests_too_deep(json.as_bytes()) {
            return Err(SessionJsonError);
        }
        let mut de = serde_json::Deserializer::from_str(json);
        let value = (&mut de).deserialize_any(StateVisitor).map_err(|_| SessionJsonError)?;
        de.end().map_err(|_| SessionJsonError)?;
        Ok(value)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------------------------------------------

/// The sweep of [`SessionStore::sweep_stale_temp_files`]; never fails.
fn sweep(file_system: &dyn FileSystem, sessions_dir: &Path, utc_now: DateTimeUtc) -> usize {
    // A missing sessions folder is the normal first run.
    let Ok(temps) = file_system.enumerate_files(sessions_dir, "*.tmp") else {
        return 0;
    };
    let mut removed = 0;
    for temp in temps {
        // Per file: one locked or vanished temp file must not stop the sweep of the rest.
        let Some(stat) = file_system.get_file_stat(&temp) else {
            continue;
        };
        let age = utc_now.ticks().saturating_sub(stat.last_write_utc.ticks());
        if age > STALE_TEMP_AGE_TICKS && file_system.delete(&temp).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// One JSON string literal in the `System.Text.Json` default-encoder spelling.
fn json_string(text: &str) -> String {
    let mut serializer = serde_json::Serializer::with_formatter(Vec::new(), DotNetJsonFormatter);
    match Serialize::serialize(text, &mut serializer) {
        Ok(()) => String::from_utf8(serializer.into_inner()).unwrap_or_default(),
        // Serialising a str into a Vec cannot fail; an empty literal keeps the file parseable if it ever did.
        Err(_) => "\"\"".to_owned(),
    }
}

/// Maximum nesting of containers in a file (the root object plus 63 nested arrays is the deepest accepted text).
const MAX_NESTING: usize = 64;

/// `true` when some container nests deeper than [`MAX_NESTING`]. Counts `{` and `[` outside strings; never panics.
fn nests_too_deep(text: &[u8]) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &b in text {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > MAX_NESTING {
                        return true;
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    false
}

struct StateVisitor;

impl<'de> Visitor<'de> for StateVisitor {
    type Value = Option<SessionState>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a session object or null")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut state = SessionState::default();
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "Folder" => state.folder = member(&mut map, decode_nullable_string)?.unwrap_or_default(),
                "CurrentPath" => state.current_path = member(&mut map, decode_nullable_string)?,
                "Skipped" => state.skipped = member(&mut map, decode_skipped)?,
                "UpdatedUtc" => state.updated_utc = member(&mut map, decode_datetime)?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(Some(state))
    }
}

/// Reads one member value as raw text and decodes it; a decode failure rejects the whole file.
fn member<'de, A, T>(map: &mut A, decode: fn(&str) -> Option<T>) -> Result<T, A::Error>
where
    A: MapAccess<'de>,
{
    let raw = map.next_value::<Box<RawValue>>()?;
    decode(raw.get()).ok_or_else(|| de::Error::custom("session file rejected"))
}

/// A JSON string, or `null` (= `None`). Outer `None`: any other JSON type.
fn decode_nullable_string(raw: &str) -> Option<Option<String>> {
    match raw.as_bytes().first()? {
        b'n' => Some(None),
        b'"' => serde_json::from_str::<String>(raw).ok().map(Some),
        _ => None,
    }
}

/// `null` (empty list) or an array of strings and nulls; null elements are dropped (documented divergence).
fn decode_skipped(raw: &str) -> Option<Vec<String>> {
    match raw.as_bytes().first()? {
        b'n' => Some(Vec::new()),
        b'[' => serde_json::from_str::<Vec<Option<String>>>(raw)
            .ok()
            .map(|items| items.into_iter().flatten().collect()),
        _ => None,
    }
}

/// A timestamp: a JSON string that [`JournalDateTime::parse_json`] accepts.
fn decode_datetime(raw: &str) -> Option<JournalDateTime> {
    let text = serde_json::from_str::<String>(raw).ok()?;
    JournalDateTime::parse_json(&text)
}

// ---------------------------------------------------------------------------------------------------------------
// SHA-256 (FIPS 180-4). Hand written because `pr-core` has no hashing dependency yet (contract Q1 alternative).
// ---------------------------------------------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
    0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
    0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
    0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

/// SHA-256 of `data` (D33: the one shared helper; W4-10b and W4-14c import it from here).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64).wrapping_mul(8).to_be_bytes());

    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (slot, word) in w.iter_mut().zip(block.chunks_exact(4)) {
            *slot = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for (k, wi) in SHA256_K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(*k).wrapping_add(*wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut out = [0u8; 32];
    for (chunk, word) in out.chunks_exact_mut(4).zip(h) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sha256;

    fn hex(bytes: [u8; 32]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha256_matches_the_rfc_6234_vectors() {
        assert_eq!(
            hex(sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }
}
