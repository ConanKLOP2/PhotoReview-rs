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
//! Unit W2-17. Stub: bodies are `todo!()`; data definitions are complete. Contract: `contracts/W2-17.md`.

// Stub-only (docs/MULTI-AGENT-PLAN D11): removed by the porter in the commit that removes the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::sync::{Arc, RwLock};

use crate::time::DateTimeUtc;

/// `Stopwatch.Frequency` on Windows 10+: the tick rate of [`timestamp`] (100 ns).
pub const STOPWATCH_FREQUENCY: i64 = 10_000_000;

/// `Stopwatch.GetTimestamp()`: a monotonic tick counter at [`STOPWATCH_FREQUENCY`]. Only differences are meaningful
/// (the CSV `qpcTicks` column is analysed as deltas). Implemented with `std::time::Instant` against a process-wide
/// base captured on first use, so no Windows API is needed in `pr-core`.
pub fn timestamp() -> i64 {
    todo!()
}

/// `PhotoReviewPerf.Ms(startTimestamp)`: milliseconds elapsed since a [`timestamp`] snapshot,
/// `delta * 1000.0 / STOPWATCH_FREQUENCY` evaluated in `f64`.
pub fn ms(start_timestamp: i64) -> f64 {
    todo!()
}

/// `PhotoReviewPerf.MsSinceProcessStart()`: milliseconds since the OS started this process (wall clock). Rust has no
/// portable process start time in `std`: the value is `now - process_start` where `process_start` is captured by
/// [`init_process_start`] (call it first thing in `main`); before that call it is captured lazily at first use.
pub fn ms_since_process_start() -> f64 {
    todo!()
}

/// Captures the process start reference used by [`ms_since_process_start`]. Idempotent. The app may pass the true OS
/// process creation time instead (platform layer, `GetProcessTimes`).
pub fn init_process_start(process_start_utc: Option<DateTimeUtc>) {
    todo!()
}

/// `PhotoReviewPerf.PathId(path)`: stable 16 lower-case hex chars identifying a file path without revealing it: the
/// first 8 bytes of SHA-256 over the UTF-8 bytes of `to_upper_invariant(absolute(path))`. Never write the raw path
/// into perf events, CSV or reports. See the contract for the exact algorithm (verified against .NET).
pub fn path_id(path: &str) -> String {
    todo!()
}

/// `PhotoReviewPerf.StartupMark(phase)`: emits [`PhotoReviewPerf::startup`] with [`ms_since_process_start`] when a
/// listener is attached.
pub fn startup_mark(phase: &str) {
    todo!()
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
        todo!("remove the listener with this id and decrement the enabled counter")
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
        todo!()
    }

    /// Subscribes `listener` until the returned guard is dropped.
    pub fn subscribe(&self, listener: Arc<dyn PerfListener>) -> PerfSubscription {
        todo!()
    }

    /// The nav token of the current logical operation (C# `NavContext`, an `AsyncLocal<long>`). Rust has no async
    /// flow: this is a thread-local, default 0, set by the caller around the work and copied by hand when work moves
    /// to another thread. Preload work not tied to a live navigation uses -1.
    pub fn nav_context() -> i64 {
        todo!()
    }

    pub fn set_nav_context(value: i64) {
        todo!()
    }

    // ---- Event ids: fixed forever once assigned. Add new events with new ids; never renumber. ----

    /// Event 1. Payload `nav, key, inputDelayMs`.
    pub fn key_input(&self, nav: i64, key: &str, input_delay_ms: f64) {
        todo!()
    }

    /// Event 2. Payload `nav, index, mode`.
    pub fn show_start(&self, nav: i64, index: i32, mode: &str) {
        todo!()
    }

    /// Event 3. Payload `nav, ms`.
    pub fn stat(&self, nav: i64, ms: f64) {
        todo!()
    }

    /// Event 4. Payload `nav, pathId, result`.
    pub fn lookup(&self, nav: i64, path_id: &str, result: &str) {
        todo!()
    }

    /// Event 5. Payload `nav, pathId`.
    pub fn thumb_start(&self, nav: i64, path_id: &str) {
        todo!()
    }

    /// Event 6. Payload `nav, pathId, source, ms`.
    pub fn thumb_end(&self, nav: i64, path_id: &str, source: &str, ms: f64) {
        todo!()
    }

    /// Event 7. Payload `nav, pathId`.
    pub fn join_start(&self, nav: i64, path_id: &str) {
        todo!()
    }

    /// Event 8. Payload `nav, pathId, ms`.
    pub fn join_end(&self, nav: i64, path_id: &str, ms: f64) {
        todo!()
    }

    /// Event 9. Payload `nav, pathId, ms, bytes`.
    pub fn disk_cache_read(&self, nav: i64, path_id: &str, ms: f64, bytes: i64) {
        todo!()
    }

    /// Event 11. Payload `nav, pathId, ms, bytes`.
    pub fn source_read(&self, nav: i64, path_id: &str, ms: f64, bytes: i64) {
        todo!()
    }

    /// Event 12. Payload `nav, pathId, ms, targetWidth, downscaled, fallback`.
    pub fn decode(&self, nav: i64, path_id: &str, ms: f64, target_width: i32, downscaled: bool, fallback: bool) {
        todo!()
    }

    /// Event 13. Payload `nav, pathId, ms`.
    pub fn verify(&self, nav: i64, path_id: &str, ms: f64) {
        todo!()
    }

    /// Event 14. Payload `nav, ms, pixelWidth, pixelHeight`.
    pub fn assign(&self, nav: i64, ms: f64, pixel_width: i32, pixel_height: i32) {
        todo!()
    }

    /// Event 15. Payload `nav, msSinceAssign`.
    pub fn rendered(&self, nav: i64, ms_since_assign: f64) {
        todo!()
    }

    /// Event 16. Payload `nav, kind`.
    pub fn presented(&self, nav: i64, kind: &str) {
        todo!()
    }

    /// Event 17. Payload `nav, part`.
    pub fn post_start(&self, nav: i64, part: &str) {
        todo!()
    }

    /// Event 18. Payload `nav, part, ms`.
    pub fn post_end(&self, nav: i64, part: &str, ms: f64) {
        todo!()
    }

    /// Event 19. Payload `slot, pathId, queueWaitMs, kind, ms`.
    pub fn preload_item(&self, slot: i32, path_id: &str, queue_wait_ms: f64, kind: &str, ms: f64) {
        todo!()
    }

    /// Event 20. Payload `loadPercent, availableMb`.
    pub fn preload_paused(&self, load_percent: i32, available_mb: i64) {
        todo!()
    }

    /// Event 21. Payload `reason`.
    pub fn preload_cancel(&self, reason: &str) {
        todo!()
    }

    /// Event 22. Payload `ms, priority, name`.
    pub fn dispatcher_long_op(&self, ms: f64, priority: &str, name: &str) {
        todo!()
    }

    /// Event 23. Payload `gen, phase, ms`.
    pub fn folder(&self, generation: i64, phase: &str, ms: f64) {
        todo!()
    }

    /// Event 24. Payload `flags`.
    pub fn diag_mode(&self, flags: &str) {
        todo!()
    }

    /// Event 25. Payload `nav, msSinceAssign`.
    pub fn rendered_frame(&self, nav: i64, ms_since_assign: f64) {
        todo!()
    }

    /// Event 26. Payload `phase, msSinceProcessStart`.
    pub fn startup(&self, phase: &str, ms_since_process_start: f64) {
        todo!()
    }

    /// Event 27. Payload `gen, phase, value, detail`.
    pub fn folder_info(&self, generation: i64, phase: &str, value: i64, detail: &str) {
        todo!()
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
