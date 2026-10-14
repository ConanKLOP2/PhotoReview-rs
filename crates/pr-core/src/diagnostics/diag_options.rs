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

use std::sync::{Arc, Mutex, PoisonError};

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
        // C# `== "1"`: exact match; an unset variable (`None`) is not "1".
        let flag = |name: &str| get(name).as_deref() == Some("1");
        let preload_workers = get(PRELOAD_WORKERS_VAR).as_deref().and_then(parse_preload_workers);
        // C# `string.IsNullOrWhiteSpace`: empty or all-whitespace is unset; otherwise the raw text (not trimmed).
        let instance_label = get(INSTANCE_LABEL_VAR).filter(|label| !label.chars().all(char::is_whitespace));
        Self {
            pre_read: flag(PRE_READ_VAR),
            preload_workers,
            disable_disk_cache: flag(DISABLE_DISK_CACHE_VAR),
            force_log: flag(FORCE_LOG_VAR),
            instance_label,
        }
    }

    /// [`from_lookup`](Self::from_lookup) over the real process environment (`std::env::var_os`, lossy UTF-8; an
    /// unset variable is `None`).
    pub fn from_environment() -> Self {
        Self::from_lookup(|name| std::env::var_os(name).map(|value| value.to_string_lossy().into_owned()))
    }

    /// The process-wide snapshot: parsed from the environment on first call, the same `Arc` afterwards
    /// (C# `DiagOptions.PreRead` etc. all read one `Lazy<Snapshot>`).
    pub fn current() -> Arc<DiagOptions> {
        PROCESS_CACHE.get_or_load(Self::from_environment)
    }

    /// C# `ResetForTests`: forget the process-wide snapshot so the next [`current`](Self::current) re-parses.
    pub fn reset_for_tests() {
        PROCESS_CACHE.reset();
    }

    /// C# `AnyEnabled`: any variable in effect (`pre_read || preload_workers.is_some() || disable_disk_cache ||
    /// force_log || instance_label.is_some()`). `PRELOAD_WORKERS=0` counts.
    pub fn any_enabled(&self) -> bool {
        self.pre_read
            || self.preload_workers.is_some()
            || self.disable_disk_cache
            || self.force_log
            || self.instance_label.is_some()
    }

    /// C# `Describe`: `"(none)"`, or the active flags joined by `;` in this order: `PREREAD=1`,
    /// `PRELOAD_WORKERS=<n>`, `DISABLE_DISKCACHE=1`, `FORCE_LOG=1`, `INSTANCE_LABEL=<raw label>`.
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.pre_read {
            parts.push("PREREAD=1".to_owned());
        }
        if let Some(workers) = self.preload_workers {
            parts.push(format!("PRELOAD_WORKERS={workers}"));
        }
        if self.disable_disk_cache {
            parts.push("DISABLE_DISKCACHE=1".to_owned());
        }
        if self.force_log {
            parts.push("FORCE_LOG=1".to_owned());
        }
        if let Some(label) = &self.instance_label {
            parts.push(format!("INSTANCE_LABEL={label}"));
        }
        if parts.is_empty() {
            "(none)".to_owned()
        } else {
            parts.join(";")
        }
    }
}

/// C# `int.TryParse(raw, NumberStyles.Integer, CultureInfo.InvariantCulture, out v) && v is >= 0 and <= 16`.
///
/// `NumberStyles.Integer` = leading and trailing white (only U+0009..U+000D and U+0020), one optional `+`/`-` directly
/// before the ASCII digits, then nothing but NUL characters (`'\0'`) after the trailing white. Any failure is `None`.
fn parse_preload_workers(raw: &str) -> Option<u32> {
    fn is_white(c: char) -> bool {
        matches!(c, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ')
    }

    let text = raw.trim_start_matches(is_white);
    let (negative, digits_and_rest) = match text.chars().next() {
        Some('+') => (false, &text[1..]),
        Some('-') => (true, &text[1..]),
        _ => (false, text),
    };

    // Digits: leading zeros are fine; the value saturates past 16, which the range check rejects anyway.
    let digit_count = digits_and_rest.chars().take_while(char::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    let mut value: u64 = 0;
    for digit in digits_and_rest[..digit_count].bytes() {
        value = (value * 10 + u64::from(digit - b'0')).min(1_000);
    }

    let rest = digits_and_rest[digit_count..].trim_start_matches(is_white);
    if !rest.chars().all(|c| c == '\0') {
        return None;
    }
    if negative && value != 0 {
        return None;
    }
    u32::try_from(value).ok().filter(|v| *v <= 16)
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
        // Held across `load` on purpose (C# `Lazy` does the same): the loader only reads variables.
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(stored) = slot.as_ref() {
            return Arc::clone(stored);
        }
        let value = Arc::new(load());
        *slot = Some(Arc::clone(&value));
        value
    }

    pub fn reset(&self) {
        *self.slot.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// The process-wide cache behind [`DiagOptions::current`].
pub static PROCESS_CACHE: DiagOptionsCache = DiagOptionsCache::new();
