//! Port of `PhotoReview.Core/Diagnostics/PhotoReviewPerf.cs` (C# baseline d92d6aae, 254 lines) and
//! `ShellPerfMarks.cs` (12 lines).
//!
//! C# declares an `EventSource` ("PhotoReview-Perf") with 26 events whose numeric ids are a wire contract. Rust has
//! no EventSource: [`PhotoReviewPerf`] keeps the same event methods, ids and payload shapes, and delivers each event
//! synchronously, on the calling thread, to the subscribed [`PerfListener`]s (the CSV writer is one). With no
//! listener every event method returns after one atomic load (C# `IsEnabled()`), so call sites need no guard for
//! cheap arguments; guard arguments that are expensive to build (PathId hashing) with
//! [`PhotoReviewPerf::is_enabled`], exactly as the C# call sites do.
//!
//! Unit W2-17. Contract: `contracts/W2-17.md`.
//!
//! DIVERGENCES from C# (contract notes 15, 17, 18):
//! - `nav_context` is a thread-local, not an `AsyncLocal`: callers copy it across threads by hand.
//! - `ms_since_process_start` is measured from the first use unless the platform layer calls [`init_process_start`]
//!   with the real OS creation time (C# `Process.StartTime`).
//! - [`path_id`] never panics: a path `get_full_path` rejects (empty, NUL) is hashed as given (C# throws).
//! - SHA-256 is hand-written below (the contract's sanctioned fallback while `sha2` is not a workspace dependency).

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, PoisonError, RwLock};
use std::time::Instant;

use crate::text::dotnet_path;
use crate::text::ordinal_upper;
use crate::time::DateTimeUtc;

/// `Stopwatch.Frequency` on Windows 10+: the tick rate of [`timestamp`] (100 ns).
pub const STOPWATCH_FREQUENCY: i64 = 10_000_000;

/// `Stopwatch.GetTimestamp()`: a monotonic tick counter at [`STOPWATCH_FREQUENCY`]. Only differences are meaningful
/// (the CSV `qpcTicks` column is analysed as deltas). Implemented with `std::time::Instant` against a process-wide
/// base captured on first use, so no Windows API is needed in `pr-core`.
pub fn timestamp() -> i64 {
    static BASE: OnceLock<Instant> = OnceLock::new();
    let base = BASE.get_or_init(Instant::now);
    // +1: never 0, so a caller's "0 = not started" sentinel cannot misfire on the first call.
    i64::try_from(base.elapsed().as_nanos() / 100).unwrap_or(i64::MAX - 1) + 1
}

/// `PhotoReviewPerf.Ms(startTimestamp)`: milliseconds elapsed since a [`timestamp`] snapshot,
/// `delta * 1000.0 / STOPWATCH_FREQUENCY` evaluated in `f64`.
pub fn ms(start_timestamp: i64) -> f64 {
    let delta = timestamp() - start_timestamp;
    delta as f64 * 1000.0 / STOPWATCH_FREQUENCY as f64
}

/// `PhotoReviewPerf.MsSinceProcessStart()`: milliseconds since the OS started this process (wall clock). Rust has no
/// portable process start time in `std`: the value is `now - process_start` where `process_start` is captured by
/// [`init_process_start`] (call it first thing in `main`); before that call it is captured lazily at first use.
pub fn ms_since_process_start() -> f64 {
    let start = *PROCESS_START.get_or_init(DateTimeUtc::now);
    (DateTimeUtc::now().ticks() - start.ticks()) as f64 / 10_000.0
}

/// The process start reference (set once; a `OnceLock` also removes the C# torn-`DateTime?` hazard).
static PROCESS_START: OnceLock<DateTimeUtc> = OnceLock::new();

/// Captures the process start reference used by [`ms_since_process_start`]. Idempotent. The app may pass the true OS
/// process creation time instead (platform layer, `GetProcessTimes`).
pub fn init_process_start(process_start_utc: Option<DateTimeUtc>) {
    // A second call (or a call after the lazy capture) keeps the first value.
    let _ = PROCESS_START.set(process_start_utc.unwrap_or_else(DateTimeUtc::now));
}

/// `PhotoReviewPerf.PathId(path)`: stable 16 lower-case hex chars identifying a file path without revealing it: the
/// first 8 bytes of SHA-256 over the UTF-8 bytes of `to_upper_invariant(absolute(path))`. Never write the raw path
/// into perf events, CSV or reports. See the contract for the exact algorithm (verified against .NET).
pub fn path_id(path: &str) -> String {
    let full = dotnet_path::get_full_path(path).unwrap_or_else(|_| path.to_owned());
    let upper: String = full.chars().map(to_upper_invariant_char).collect();
    let hash = sha256(upper.as_bytes());
    let mut id = String::with_capacity(16);
    for byte in &hash[..8] {
        id.push(char::from(HEX[usize::from(byte >> 4)]));
        id.push(char::from(HEX[usize::from(byte & 0x0F)]));
    }
    id
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// .NET `char.ToUpperInvariant`: the ordinal upper-case mapping plus U+017F (long s) -> `S`. Never `to_uppercase`
/// (sharp s would become `SS`, dotless i `I`).
fn to_upper_invariant_char(c: char) -> char {
    if c == '\u{017F}' { 'S' } else { ordinal_upper(c) }
}

/// SHA-256 (FIPS 180-4) of `data`.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, add) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(add);
        }
    }
    let mut out = [0u8; 32];
    for (chunk, word) in out.chunks_exact_mut(4).zip(state) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sha256;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha256_matches_fips_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }
}

/// `PhotoReviewPerf.StartupMark(phase)`: emits [`PhotoReviewPerf::startup`] with [`ms_since_process_start`] when a
/// listener is attached.
pub fn startup_mark(phase: &str) {
    let log = PhotoReviewPerf::log();
    if log.is_enabled() {
        log.startup(phase, ms_since_process_start());
    }
}

/// One event payload value. C# boxes `long`/`int` (both [`PerfValue::Int`]), `double`, `bool` and `string`.
#[derive(Debug, Clone, PartialEq)]
pub enum PerfValue {
    /// A null string argument.
    Null,
    Int(i64),
    Double(f64),
    Bool(bool),
    Str(String),
}

/// One event as delivered to listeners: C# `EventWrittenEventArgs` (`EventId`, `EventName`, `PayloadNames`, `Payload`).
#[derive(Debug, Clone, PartialEq)]
pub struct PerfEvent {
    /// The fixed wire id (1..=9, 11..=27; 10 was never assigned and must stay unused).
    pub id: u32,
    /// The C# method name (`"KeyInput"`), which is the CSV `event` column.
    pub name: &'static str,
    /// The C# parameter names in declaration order (`["nav", "key", "inputDelayMs"]`).
    pub payload_names: &'static [&'static str],
    pub payload: Vec<PerfValue>,
}

/// C# `EventListener`: receives every event written while subscribed. Must not block and must not panic into the
/// emitting thread (the CSV listener only does a `try_send`).
pub trait PerfListener: Send + Sync {
    fn on_event_written(&self, event: &PerfEvent);
}

/// Keeps a listener subscribed; dropping it unsubscribes (C# `EventListener.Dispose`).
#[derive(Debug)]
pub struct PerfSubscription {
    id: u64,
}

impl Drop for PerfSubscription {
    fn drop(&mut self) {
        let mut listeners = LISTENERS.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(index) = listeners.iter().position(|(id, _)| *id == self.id) {
            listeners.remove(index);
            LISTENER_COUNT.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// The event ids and names, in declaration order, for the "ids are unique and positive" test
/// (C# `PhotoReviewPerfTests.EventIds_UniqueAndInformational`).
pub const EVENT_TABLE: &[(u32, &str)] = &[
    (1, "KeyInput"),
    (2, "ShowStart"),
    (3, "Stat"),
    (4, "Lookup"),
    (5, "ThumbStart"),
    (6, "ThumbEnd"),
    (7, "JoinStart"),
    (8, "JoinEnd"),
    (9, "DiskCacheRead"),
    (11, "SourceRead"),
    (12, "Decode"),
    (13, "Verify"),
    (14, "Assign"),
    (15, "Rendered"),
    (16, "Presented"),
    (17, "PostStart"),
    (18, "PostEnd"),
    (19, "PreloadItem"),
    (20, "PreloadPaused"),
    (21, "PreloadCancel"),
    (22, "DispatcherLongOp"),
    (23, "Folder"),
    (24, "DiagMode"),
    (25, "RenderedFrame"),
    (26, "Startup"),
    (27, "FolderInfo"),
];

/// Registered listeners (process-wide, like the C# EventSource).
static LISTENERS: RwLock<Vec<(u64, Arc<dyn PerfListener>)>> = RwLock::new(Vec::new());
/// Number of subscribed listeners: `is_enabled` is one relaxed load of this.
static LISTENER_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_LISTENER_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static NAV_CONTEXT: Cell<i64> = const { Cell::new(0) };
}

/// The perf event source. C# `PhotoReviewPerf.Log` is a singleton; so is [`PhotoReviewPerf::log`].
#[derive(Debug)]
pub struct PhotoReviewPerf {
    _private: (),
}

impl PhotoReviewPerf {
    /// C# `PhotoReviewPerf.Log`.
    pub fn log() -> &'static PhotoReviewPerf {
        static LOG: PhotoReviewPerf = PhotoReviewPerf { _private: () };
        &LOG
    }

    /// True while at least one listener is subscribed (one relaxed atomic load).
    pub fn is_enabled(&self) -> bool {
        LISTENER_COUNT.load(Ordering::Relaxed) > 0
    }

    /// Subscribes `listener` until the returned guard is dropped.
    pub fn subscribe(&self, listener: Arc<dyn PerfListener>) -> PerfSubscription {
        let id = NEXT_LISTENER_ID.fetch_add(1, Ordering::Relaxed);
        let mut listeners = LISTENERS.write().unwrap_or_else(PoisonError::into_inner);
        listeners.push((id, listener));
        LISTENER_COUNT.fetch_add(1, Ordering::Relaxed);
        PerfSubscription { id }
    }

    /// Delivers one event to every listener on the calling thread. The listener list is cloned under the read lock and
    /// called after releasing it: a listener may subscribe or unsubscribe without deadlocking.
    fn emit(&self, id: u32, name: &'static str, payload_names: &'static [&'static str], payload: Vec<PerfValue>) {
        let listeners: Vec<Arc<dyn PerfListener>> = {
            let guard = LISTENERS.read().unwrap_or_else(PoisonError::into_inner);
            guard.iter().map(|(_, listener)| Arc::clone(listener)).collect()
        };
        if listeners.is_empty() {
            return;
        }
        let event = PerfEvent {
            id,
            name,
            payload_names,
            payload,
        };
        for listener in &listeners {
            listener.on_event_written(&event);
        }
    }

    /// The nav token of the current logical operation (C# `NavContext`, an `AsyncLocal<long>`). Rust has no async
    /// flow: this is a thread-local, default 0, set by the caller around the work and copied by hand when work moves
    /// to another thread. Preload work not tied to a live navigation uses -1.
    pub fn nav_context() -> i64 {
        NAV_CONTEXT.with(Cell::get)
    }

    pub fn set_nav_context(value: i64) {
        NAV_CONTEXT.with(|cell| cell.set(value));
    }

    // ---- Event ids: fixed forever once assigned. Add new events with new ids; never renumber. ----

    /// Event 1. Payload `nav, key, inputDelayMs`.
    pub fn key_input(&self, nav: i64, key: &str, input_delay_ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            1,
            "KeyInput",
            &["nav", "key", "inputDelayMs"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Str(key.to_owned()),
                PerfValue::Double(input_delay_ms),
            ],
        );
    }

    /// Event 2. Payload `nav, index, mode`.
    pub fn show_start(&self, nav: i64, index: i32, mode: &str) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            2,
            "ShowStart",
            &["nav", "index", "mode"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Int(i64::from(index)),
                PerfValue::Str(mode.to_owned()),
            ],
        );
    }

    /// Event 3. Payload `nav, ms`.
    pub fn stat(&self, nav: i64, ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            3,
            "Stat",
            &["nav", "ms"],
            vec![PerfValue::Int(nav), PerfValue::Double(ms)],
        );
    }

    /// Event 4. Payload `nav, pathId, result`.
    pub fn lookup(&self, nav: i64, path_id: &str, result: &str) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            4,
            "Lookup",
            &["nav", "pathId", "result"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Str(path_id.to_owned()),
                PerfValue::Str(result.to_owned()),
            ],
        );
    }

    /// Event 5. Payload `nav, pathId`.
    pub fn thumb_start(&self, nav: i64, path_id: &str) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            5,
            "ThumbStart",
            &["nav", "pathId"],
            vec![PerfValue::Int(nav), PerfValue::Str(path_id.to_owned())],
        );
    }

    /// Event 6. Payload `nav, pathId, source, ms`.
    pub fn thumb_end(&self, nav: i64, path_id: &str, source: &str, ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            6,
            "ThumbEnd",
            &["nav", "pathId", "source", "ms"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Str(path_id.to_owned()),
                PerfValue::Str(source.to_owned()),
                PerfValue::Double(ms),
            ],
        );
    }

    /// Event 7. Payload `nav, pathId`.
    pub fn join_start(&self, nav: i64, path_id: &str) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            7,
            "JoinStart",
            &["nav", "pathId"],
            vec![PerfValue::Int(nav), PerfValue::Str(path_id.to_owned())],
        );
    }

    /// Event 8. Payload `nav, pathId, ms`.
    pub fn join_end(&self, nav: i64, path_id: &str, ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            8,
            "JoinEnd",
            &["nav", "pathId", "ms"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Str(path_id.to_owned()),
                PerfValue::Double(ms),
            ],
        );
    }

    /// Event 9. Payload `nav, pathId, ms, bytes`.
    pub fn disk_cache_read(&self, nav: i64, path_id: &str, ms: f64, bytes: i64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            9,
            "DiskCacheRead",
            &["nav", "pathId", "ms", "bytes"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Str(path_id.to_owned()),
                PerfValue::Double(ms),
                PerfValue::Int(bytes),
            ],
        );
    }

    /// Event 11. Payload `nav, pathId, ms, bytes`.
    pub fn source_read(&self, nav: i64, path_id: &str, ms: f64, bytes: i64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            11,
            "SourceRead",
            &["nav", "pathId", "ms", "bytes"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Str(path_id.to_owned()),
                PerfValue::Double(ms),
                PerfValue::Int(bytes),
            ],
        );
    }

    /// Event 12. Payload `nav, pathId, ms, targetWidth, downscaled, fallback`.
    pub fn decode(&self, nav: i64, path_id: &str, ms: f64, target_width: i32, downscaled: bool, fallback: bool) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            12,
            "Decode",
            &["nav", "pathId", "ms", "targetWidth", "downscaled", "fallback"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Str(path_id.to_owned()),
                PerfValue::Double(ms),
                PerfValue::Int(i64::from(target_width)),
                PerfValue::Bool(downscaled),
                PerfValue::Bool(fallback),
            ],
        );
    }

    /// Event 13. Payload `nav, pathId, ms`.
    pub fn verify(&self, nav: i64, path_id: &str, ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            13,
            "Verify",
            &["nav", "pathId", "ms"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Str(path_id.to_owned()),
                PerfValue::Double(ms),
            ],
        );
    }

    /// Event 14. Payload `nav, ms, pixelWidth, pixelHeight`.
    pub fn assign(&self, nav: i64, ms: f64, pixel_width: i32, pixel_height: i32) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            14,
            "Assign",
            &["nav", "ms", "pixelWidth", "pixelHeight"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Double(ms),
                PerfValue::Int(i64::from(pixel_width)),
                PerfValue::Int(i64::from(pixel_height)),
            ],
        );
    }

    /// Event 15. Payload `nav, msSinceAssign`.
    pub fn rendered(&self, nav: i64, ms_since_assign: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            15,
            "Rendered",
            &["nav", "msSinceAssign"],
            vec![PerfValue::Int(nav), PerfValue::Double(ms_since_assign)],
        );
    }

    /// Event 16. Payload `nav, kind`.
    pub fn presented(&self, nav: i64, kind: &str) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            16,
            "Presented",
            &["nav", "kind"],
            vec![PerfValue::Int(nav), PerfValue::Str(kind.to_owned())],
        );
    }

    /// Event 17. Payload `nav, part`.
    pub fn post_start(&self, nav: i64, part: &str) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            17,
            "PostStart",
            &["nav", "part"],
            vec![PerfValue::Int(nav), PerfValue::Str(part.to_owned())],
        );
    }

    /// Event 18. Payload `nav, part, ms`.
    pub fn post_end(&self, nav: i64, part: &str, ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            18,
            "PostEnd",
            &["nav", "part", "ms"],
            vec![
                PerfValue::Int(nav),
                PerfValue::Str(part.to_owned()),
                PerfValue::Double(ms),
            ],
        );
    }

    /// Event 19. Payload `slot, pathId, queueWaitMs, kind, ms`.
    pub fn preload_item(&self, slot: i32, path_id: &str, queue_wait_ms: f64, kind: &str, ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            19,
            "PreloadItem",
            &["slot", "pathId", "queueWaitMs", "kind", "ms"],
            vec![
                PerfValue::Int(i64::from(slot)),
                PerfValue::Str(path_id.to_owned()),
                PerfValue::Double(queue_wait_ms),
                PerfValue::Str(kind.to_owned()),
                PerfValue::Double(ms),
            ],
        );
    }

    /// Event 20. Payload `loadPercent, availableMb`.
    pub fn preload_paused(&self, load_percent: i32, available_mb: i64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            20,
            "PreloadPaused",
            &["loadPercent", "availableMb"],
            vec![PerfValue::Int(i64::from(load_percent)), PerfValue::Int(available_mb)],
        );
    }

    /// Event 21. Payload `reason`.
    pub fn preload_cancel(&self, reason: &str) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            21,
            "PreloadCancel",
            &["reason"],
            vec![PerfValue::Str(reason.to_owned())],
        );
    }

    /// Event 22. Payload `ms, priority, name`.
    pub fn dispatcher_long_op(&self, ms: f64, priority: &str, name: &str) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            22,
            "DispatcherLongOp",
            &["ms", "priority", "name"],
            vec![
                PerfValue::Double(ms),
                PerfValue::Str(priority.to_owned()),
                PerfValue::Str(name.to_owned()),
            ],
        );
    }

    /// Event 23. Payload `gen, phase, ms`.
    pub fn folder(&self, generation: i64, phase: &str, ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            23,
            "Folder",
            &["gen", "phase", "ms"],
            vec![
                PerfValue::Int(generation),
                PerfValue::Str(phase.to_owned()),
                PerfValue::Double(ms),
            ],
        );
    }

    /// Event 24. Payload `flags`.
    pub fn diag_mode(&self, flags: &str) {
        if !self.is_enabled() {
            return;
        }
        self.emit(24, "DiagMode", &["flags"], vec![PerfValue::Str(flags.to_owned())]);
    }

    /// Event 25. Payload `nav, msSinceAssign`.
    pub fn rendered_frame(&self, nav: i64, ms_since_assign: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            25,
            "RenderedFrame",
            &["nav", "msSinceAssign"],
            vec![PerfValue::Int(nav), PerfValue::Double(ms_since_assign)],
        );
    }

    /// Event 26. Payload `phase, msSinceProcessStart`.
    pub fn startup(&self, phase: &str, ms_since_process_start: f64) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            26,
            "Startup",
            &["phase", "msSinceProcessStart"],
            vec![
                PerfValue::Str(phase.to_owned()),
                PerfValue::Double(ms_since_process_start),
            ],
        );
    }

    /// Event 27. Payload `gen, phase, value, detail`.
    pub fn folder_info(&self, generation: i64, phase: &str, value: i64, detail: &str) {
        if !self.is_enabled() {
            return;
        }
        self.emit(
            27,
            "FolderInfo",
            &["gen", "phase", "value", "detail"],
            vec![
                PerfValue::Int(generation),
                PerfValue::Str(phase.to_owned()),
                PerfValue::Int(value),
                PerfValue::Str(detail.to_owned()),
            ],
        );
    }
}

/// `ShellPerfMarks` (C-18): perf marks that exist only in the Win32 shell, emitted with the same mark names as the
/// WPF app so one probe reads both. Plain constants.
#[derive(Debug, Clone, Copy)]
pub struct ShellPerfMarks;

impl ShellPerfMarks {
    pub const D3D_DEVICE_READY: &'static str = "d3dDeviceReady";
    pub const SWAP_CHAIN_READY: &'static str = "swapChainReady";
    pub const FIRST_PRESENT: &'static str = "firstPresent";
}
