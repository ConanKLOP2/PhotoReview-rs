//! Port of `PhotoReview.Core/Diagnostics/DiagOptions.cs` (C# baseline e441d67a).
//!
//! Diagnostic-only environment variables `PHOTOREVIEW_DIAG_*`. C# parses them once per process on first access
//! (a `Lazy<Snapshot>`) so a later environment change never makes two reads disagree; with no variable set every
//! flag is "disabled" and the app must behave exactly as if this class did not exist.
//!
//! Rust shape: [`DiagOptions`] is a plain value. [`DiagOptions::from_lookup`] parses from any variable lookup
//! (tests pass a closure; process environment mutation is `unsafe` in Rust 2024 and is not used), and
//! [`DiagOptions::current`] is the process-wide, parse-once snapshot (C# `DiagOptions.PreRead` etc.).
//! [`DiagOptionsCache`] is the snapshot machinery (C# `Lazy` + `ResetForTests`) made testable.
//!
//! Names use the Rust prefix (`PHOTOREVIEW_RS_DIAG_*`, docs/adr/R0002, lead decision in contracts/README.md): these
//! variables change behaviour (disk cache off, worker count), so a value set to diagnose the C# app must not leak into
//! this one. C# name `PHOTOREVIEW_DIAG_X` = Rust name `PHOTOREVIEW_RS_DIAG_X`; golden inputs use the C# names and the
//! test maps them.

use std::sync::{Arc, Mutex};

/// `PHOTOREVIEW_RS_DIAG_PREREAD`.
pub const PRE_READ_VAR: &str = "PHOTOREVIEW_RS_DIAG_PREREAD";
/// `PHOTOREVIEW_RS_DIAG_PRELOAD_WORKERS`.
pub const PRELOAD_WORKERS_VAR: &str = "PHOTOREVIEW_RS_DIAG_PRELOAD_WORKERS";
/// `PHOTOREVIEW_RS_DIAG_DISABLE_DISKCACHE`.
pub const DISABLE_DISK_CACHE_VAR: &str = "PHOTOREVIEW_RS_DIAG_DISABLE_DISKCACHE";
/// `PHOTOREVIEW_RS_DIAG_FORCE_LOG`.
pub const FORCE_LOG_VAR: &str = "PHOTOREVIEW_RS_DIAG_FORCE_LOG";
/// `PHOTOREVIEW_RS_DIAG_INSTANCE_LABEL`.
pub const INSTANCE_LABEL_VAR: &str = "PHOTOREVIEW_RS_DIAG_INSTANCE_LABEL";

/// The parsed values of the five variables (C# private record `Snapshot`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiagOptions {
    /// `PHOTOREVIEW_DIAG_PREREAD=1`: read the whole source file into memory before decoding.
    pub pre_read: bool,
    /// `PHOTOREVIEW_DIAG_PRELOAD_WORKERS=0..16`: override of the preload worker count; out-of-range or unparsable
    /// values are `None`.
    pub preload_workers: Option<u32>,
    /// `PHOTOREVIEW_DIAG_DISABLE_DISKCACHE=1`.
    pub disable_disk_cache: bool,
    /// `PHOTOREVIEW_DIAG_FORCE_LOG=1`.
    pub force_log: bool,
    /// `PHOTOREVIEW_DIAG_INSTANCE_LABEL`: the RAW text (not trimmed), `None` when unset, empty or whitespace-only.
    pub instance_label: Option<String>,
}

impl DiagOptions {
    /// C# `ReadFromEnvironment`, with the variable lookup injected: `get(name)` returns the value of the
    /// environment variable `name`, or `None` when it is unset.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let _ = get;
        todo!("port DiagOptions.ReadFromEnvironment")
    }

    /// [`from_lookup`](Self::from_lookup) over the real process environment (`std::env::var_os`, lossy UTF-8; an
    /// unset variable is `None`).
    pub fn from_environment() -> Self {
        todo!("read the process environment")
    }

    /// The process-wide snapshot: parsed from the environment on first call, the same `Arc` afterwards
    /// (C# `DiagOptions.PreRead` etc. all read one `Lazy<Snapshot>`).
    pub fn current() -> Arc<DiagOptions> {
        todo!("read PROCESS_CACHE")
    }

    /// C# `ResetForTests`: forget the process-wide snapshot so the next [`current`](Self::current) re-parses.
    pub fn reset_for_tests() {
        todo!("clear PROCESS_CACHE")
    }

    /// C# `AnyEnabled`: any variable in effect (`pre_read || preload_workers.is_some() || disable_disk_cache ||
    /// force_log || instance_label.is_some()`). `PRELOAD_WORKERS=0` counts.
    pub fn any_enabled(&self) -> bool {
        todo!("port DiagOptions.AnyEnabled")
    }

    /// C# `Describe`: `"(none)"`, or the active flags joined by `;` in this order: `PREREAD=1`,
    /// `PRELOAD_WORKERS=<n>`, `DISABLE_DISKCACHE=1`, `FORCE_LOG=1`, `INSTANCE_LABEL=<raw label>`.
    pub fn describe(&self) -> String {
        todo!("port DiagOptions.Describe")
    }
}

/// Parse-once holder (C# `static volatile Lazy<Snapshot>` + `ResetForTests`). `get_or_load` runs the loader only
/// when the slot is empty and always hands out the stored value afterwards; `reset` empties the slot. A poisoned
/// lock is recovered (`PoisonError::into_inner`), never `unwrap`ped.
#[derive(Debug, Default)]
pub struct DiagOptionsCache {
    slot: Mutex<Option<Arc<DiagOptions>>>,
}

impl DiagOptionsCache {
    pub const fn new() -> Self {
        Self { slot: Mutex::new(None) }
    }

    pub fn get_or_load(&self, load: impl FnOnce() -> DiagOptions) -> Arc<DiagOptions> {
        let _ = (&self.slot, load);
        todo!("port Lazy<Snapshot>")
    }

    pub fn reset(&self) {
        todo!("port ResetForTests")
    }
}

/// The process-wide cache behind [`DiagOptions::current`].
pub static PROCESS_CACHE: DiagOptionsCache = DiagOptionsCache::new();
