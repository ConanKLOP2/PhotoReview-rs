//! Port of `PhotoReview.App.Shared/AppLog.cs` (C# baseline d92d6aae): the process-wide logging facade over `FileLog`.
//!
//! C# keeps one static `Instance` (default `FileLog.Default`), a write counter for `Enabled` and one lock around the
//! forced-write sequence. Rust has no statics here: [`AppLog`] is a value. The composition root creates the process
//! one ([`AppLog::global`]); tests create their own. [`AppLog`] also implements [`Log`], so the pure modules of W8 take
//! `&dyn Log` and never name this type.
//!
//! Machine-read output and rules (`FileLog`, W2-16) are unchanged: disabled means no thread, no directory, no file.

// D11: temporary allow, the stub bodies are todo!(); removed when the porter fills them.
#![allow(unused_variables, dead_code)]

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::AtomicI32;
use std::sync::{Arc, Mutex, RwLock};

use pr_core::abstractions::Log;
use pr_core::diagnostics::file_log::FileLog;

/// The sink the facade forwards to: the `FileLog` surface `AppLog.cs` calls (`Enabled`, `Flush`, `Shutdown`,
/// `FilePath`, plus the `ILog` methods). [`FileLog`] implements it; [`DefaultFileLog`] is C# `FileLog.Default`.
pub trait AppLogTarget: Log {
    /// C# `Enabled { set }`. `false` flushes, as `FileLog::set_enabled` does.
    fn set_enabled(&self, enabled: bool);
    /// C# `Flush()`.
    fn flush(&self);
    /// C# `Shutdown()`.
    fn shutdown(&self);
    /// C# `FilePath`: the absolute log file path.
    fn file_path(&self) -> PathBuf;
}

impl AppLogTarget for FileLog {
    fn set_enabled(&self, enabled: bool) {
        todo!()
    }

    fn flush(&self) {
        todo!()
    }

    fn shutdown(&self) {
        todo!()
    }

    fn file_path(&self) -> PathBuf {
        todo!()
    }
}

/// C# `FileLog.Default`: forwards every call to [`FileLog::default_instance`] (process-wide, `AppPaths::from_environment`).
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultFileLog;

impl Log for DefaultFileLog {
    fn enabled(&self) -> bool {
        todo!()
    }

    fn info(&self, message: &str) {
        todo!()
    }

    fn warn(&self, message: &str) {
        todo!()
    }

    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        todo!()
    }
}

impl AppLogTarget for DefaultFileLog {
    fn set_enabled(&self, enabled: bool) {
        todo!()
    }

    fn flush(&self) {
        todo!()
    }

    fn shutdown(&self) {
        todo!()
    }

    fn file_path(&self) -> PathBuf {
        todo!()
    }
}

/// C# `AppLog` (static class). One instance per process in production, one per test.
pub struct AppLog {
    /// C# `_instance`. Never empty: `None` in the setter selects [`DefaultFileLog`] (C# `value ?? FileLog.Default`).
    instance: RwLock<Arc<dyn AppLogTarget>>,
    /// C# `_enabledWrites` (`int`, `Interlocked.Increment`): wrapping `i32`, bumped on EVERY `Enabled` write.
    enabled_writes: AtomicI32,
    /// C# `ForcedLogLock`. Re-entrant for the owning thread (C# `lock` is re-entrant): see the contract, trap 4.
    forced_lock: Mutex<()>,
}

impl AppLog {
    /// A facade over `instance` (`None` = [`DefaultFileLog`]). The write counter starts at 0.
    pub fn new(instance: Option<Arc<dyn AppLogTarget>>) -> Self {
        todo!()
    }

    /// The process-wide facade (`OnceLock`, created with `AppLog::new(None)` on first use). Only the composition root
    /// and the App-level code call it; the pure modules receive `&dyn Log`.
    pub fn global() -> &'static AppLog {
        todo!()
    }

    /// C# `Instance { get }`.
    pub fn instance(&self) -> Arc<dyn AppLogTarget> {
        todo!()
    }

    /// C# `Instance { set }`: `None` selects [`DefaultFileLog`]. Does NOT count as an `Enabled` write.
    pub fn set_instance(&self, instance: Option<Arc<dyn AppLogTarget>>) {
        todo!()
    }

    /// C# `Enabled { get }` (delegates to the instance).
    pub fn enabled(&self) -> bool {
        todo!()
    }

    /// C# `Enabled { set }`: increments the write counter first (even when the value does not change), then delegates.
    pub fn set_enabled(&self, enabled: bool) {
        todo!()
    }

    /// C# `EnabledWriteCount` (`Volatile.Read`).
    pub fn enabled_write_count(&self) -> i32 {
        todo!()
    }

    /// C# `FilePath` (delegates to the instance).
    pub fn file_path(&self) -> PathBuf {
        todo!()
    }

    /// C# `Info(message)`.
    pub fn info(&self, message: &str) {
        todo!()
    }

    /// C# `Warn(message)`.
    pub fn warn(&self, message: &str) {
        todo!()
    }

    /// C# `Error(message, exception = null)`.
    pub fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        todo!()
    }

    /// C# `WriteForced(Action write)` (internal): under [`forced_lock`](Self), saves `Enabled`, sets it to `true`
    /// (one counted write), runs `write`, flushes, and restores the saved value in a drop guard (one more counted
    /// write). A panic in `write` propagates AFTER the restore; the flush runs only when `write` returned.
    pub fn write_forced(&self, write: impl FnOnce()) {
        todo!()
    }

    /// C# `ErrorForced(message, ex)` (internal): records an error while logging is off, then restores the flag.
    pub fn error_forced(&self, message: &str, error: &(dyn Error + 'static)) {
        todo!()
    }

    /// C# `Flush()`.
    pub fn flush(&self) {
        todo!()
    }

    /// C# `Shutdown()`.
    pub fn shutdown(&self) {
        todo!()
    }
}

impl Log for AppLog {
    fn enabled(&self) -> bool {
        todo!()
    }

    fn info(&self, message: &str) {
        todo!()
    }

    fn warn(&self, message: &str) {
        todo!()
    }

    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        todo!()
    }
}

impl std::fmt::Debug for AppLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppLog").finish_non_exhaustive()
    }
}
