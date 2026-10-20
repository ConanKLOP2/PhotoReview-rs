//! Port of `PhotoReview.App.Shared/AppLog.cs` (C# baseline d92d6aae): the process-wide logging facade over `FileLog`.
//!
//! C# keeps one static `Instance` (default `FileLog.Default`), a write counter for `Enabled` and one lock around the
//! forced-write sequence. Rust has no statics here: [`AppLog`] is a value. The composition root creates the process
//! one ([`AppLog::global`]); tests create their own. [`AppLog`] also implements [`Log`], so the pure modules of W8 take
//! `&dyn Log` and never name this type.
//!
//! Machine-read output and rules (`FileLog`, W2-16) are unchanged: disabled means no thread, no directory, no file.

use std::cell::RefCell;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, RwLock};

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
        FileLog::set_enabled(self, enabled);
    }

    fn flush(&self) {
        FileLog::flush(self);
    }

    fn shutdown(&self) {
        FileLog::shutdown(self);
    }

    fn file_path(&self) -> PathBuf {
        FileLog::file_path(self).to_path_buf()
    }
}

/// C# `FileLog.Default`: forwards every call to [`FileLog::default_instance`] (process-wide, `AppPaths::from_environment`).
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultFileLog;

/// The `FileLog` that [`DefaultFileLog`] forwards to: the process-wide default or, in unit tests only, a per-thread
/// stand-in so the forwarding can be observed without touching the real application data folder.
fn default_target() -> &'static FileLog {
    #[cfg(test)]
    if let Some(target) = tests::TEST_TARGET.with(std::cell::Cell::get) {
        return target;
    }
    FileLog::default_instance()
}

impl Log for DefaultFileLog {
    fn enabled(&self) -> bool {
        default_target().enabled()
    }

    fn info(&self, message: &str) {
        default_target().info(message);
    }

    fn warn(&self, message: &str) {
        default_target().warn(message);
    }

    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        default_target().error(message, error);
    }
}

impl AppLogTarget for DefaultFileLog {
    fn set_enabled(&self, enabled: bool) {
        default_target().set_enabled(enabled);
    }

    fn flush(&self) {
        default_target().flush();
    }

    fn shutdown(&self) {
        default_target().shutdown();
    }

    fn file_path(&self) -> PathBuf {
        default_target().file_path().to_path_buf()
    }
}

thread_local! {
    /// Addresses of the [`AppLog`] values whose forced lock this thread already holds (C# `lock` is re-entrant).
    static HELD_FORCED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Holds the forced lock of one [`AppLog`] for the duration of a forced write. A nested call on the owning thread
/// finds its key already held and takes no lock (trap 4).
struct ForcedSection<'a> {
    guard: Option<MutexGuard<'a, ()>>,
    key: usize,
}

impl<'a> ForcedSection<'a> {
    fn enter(lock: &'a Mutex<()>, key: usize) -> Self {
        let nested = HELD_FORCED
            .try_with(|held| held.borrow().contains(&key))
            .unwrap_or(false);
        if nested {
            return Self { guard: None, key };
        }
        // Poisoned by an earlier panic in a forced write: the protected value is `()`, so taking it is safe (trap 3).
        let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = HELD_FORCED.try_with(|held| held.borrow_mut().push(key));
        Self {
            guard: Some(guard),
            key,
        }
    }
}

impl Drop for ForcedSection<'_> {
    fn drop(&mut self) {
        if self.guard.is_none() {
            return;
        }
        let key = self.key;
        let _ = HELD_FORCED.try_with(|held| {
            let mut held = held.borrow_mut();
            if let Some(position) = held.iter().rposition(|&entry| entry == key) {
                held.remove(position);
            }
        });
    }
}

/// Restores the `Enabled` flag saved by [`AppLog::write_forced`] when dropped, so a panic in the write still restores it.
struct EnabledRestore<'a> {
    log: &'a AppLog,
    was: bool,
}

impl Drop for EnabledRestore<'_> {
    fn drop(&mut self) {
        self.log.set_enabled(self.was);
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
        Self {
            instance: RwLock::new(instance.unwrap_or_else(|| Arc::new(DefaultFileLog))),
            enabled_writes: AtomicI32::new(0),
            forced_lock: Mutex::new(()),
        }
    }

    /// The process-wide facade (`OnceLock`, created with `AppLog::new(None)` on first use). Only the composition root
    /// and the App-level code call it; the pure modules receive `&dyn Log`.
    pub fn global() -> &'static AppLog {
        static GLOBAL: OnceLock<AppLog> = OnceLock::new();
        GLOBAL.get_or_init(|| AppLog::new(None))
    }

    /// C# `Instance { get }`.
    pub fn instance(&self) -> Arc<dyn AppLogTarget> {
        Arc::clone(&self.instance.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// C# `Instance { set }`: `None` selects [`DefaultFileLog`]. Does NOT count as an `Enabled` write.
    pub fn set_instance(&self, instance: Option<Arc<dyn AppLogTarget>>) {
        let next: Arc<dyn AppLogTarget> = instance.unwrap_or_else(|| Arc::new(DefaultFileLog));
        *self.instance.write().unwrap_or_else(PoisonError::into_inner) = next;
    }

    /// C# `Enabled { get }` (delegates to the instance).
    pub fn enabled(&self) -> bool {
        self.instance().enabled()
    }

    /// C# `Enabled { set }`: increments the write counter first (even when the value does not change), then delegates.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled_writes.fetch_add(1, Ordering::SeqCst);
        self.instance().set_enabled(enabled);
    }

    /// C# `EnabledWriteCount` (`Volatile.Read`).
    pub fn enabled_write_count(&self) -> i32 {
        self.enabled_writes.load(Ordering::SeqCst)
    }

    /// C# `FilePath` (delegates to the instance).
    pub fn file_path(&self) -> PathBuf {
        self.instance().file_path()
    }

    /// C# `Info(message)`.
    pub fn info(&self, message: &str) {
        self.instance().info(message);
    }

    /// C# `Warn(message)`.
    pub fn warn(&self, message: &str) {
        self.instance().warn(message);
    }

    /// C# `Error(message, exception = null)`.
    pub fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        self.instance().error(message, error);
    }

    /// C# `WriteForced(Action write)` (internal): under [`forced_lock`](Self), saves `Enabled`, sets it to `true`
    /// (one counted write), runs `write`, flushes, and restores the saved value in a drop guard (one more counted
    /// write). A panic in `write` propagates AFTER the restore; the flush runs only when `write` returned.
    pub fn write_forced(&self, write: impl FnOnce()) {
        let key = self as *const AppLog as usize;
        let _section = ForcedSection::enter(&self.forced_lock, key);
        let was = self.enabled();
        self.set_enabled(true);
        let _restore = EnabledRestore { log: self, was };
        write();
        self.flush();
    }

    /// C# `ErrorForced(message, ex)` (internal): records an error while logging is off, then restores the flag.
    pub fn error_forced(&self, message: &str, error: &(dyn Error + 'static)) {
        self.write_forced(|| self.error(message, Some(error)));
    }

    /// C# `Flush()`.
    pub fn flush(&self) {
        self.instance().flush();
    }

    /// C# `Shutdown()`.
    pub fn shutdown(&self) {
        self.instance().shutdown();
    }
}

impl Log for AppLog {
    fn enabled(&self) -> bool {
        AppLog::enabled(self)
    }

    fn info(&self, message: &str) {
        AppLog::info(self, message);
    }

    fn warn(&self, message: &str) {
        AppLog::warn(self, message);
    }

    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        AppLog::error(self, message, error);
    }
}

impl std::fmt::Debug for AppLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppLog").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    use pr_core::diagnostics::file_log::DEFAULT_MAX_LOG_BYTES;
    use pr_test_support::TempRoot;

    use super::*;

    thread_local! {
        /// Per-thread replacement for the default log (see `default_target`).
        pub(super) static TEST_TARGET: Cell<Option<&'static FileLog>> = const { Cell::new(None) };
    }

    #[test]
    fn default_file_log_forwards_every_call_to_its_target() {
        let temp = TempRoot::new("app-log-default-forward");
        let path = temp.join("default.log");
        let target: &'static FileLog = Box::leak(Box::new(
            FileLog::new(&path, DEFAULT_MAX_LOG_BYTES).expect("the test log path is absolute"),
        ));
        TEST_TARGET.with(|cell| cell.set(Some(target)));
        let log = DefaultFileLog;

        assert!(!Log::enabled(&log));
        assert!(!path.exists());
        AppLogTarget::set_enabled(&log, true);
        assert!(Log::enabled(&log));
        assert!(target.enabled());
        assert_eq!(AppLogTarget::file_path(&log), target.file_path());

        Log::info(&log, "fwd-info");
        Log::warn(&log, "fwd-warn");
        Log::error(&log, "fwd-error", None);
        AppLogTarget::flush(&log);
        assert_eq!(target.pending_count(), 0);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(text.contains("fwd-info"), "{text}");
        assert!(text.contains("fwd-warn"), "{text}");
        assert!(text.contains("fwd-error"), "{text}");

        AppLogTarget::set_enabled(&log, false);
        assert!(!Log::enabled(&log));
        AppLogTarget::set_enabled(&log, true);
        AppLogTarget::shutdown(&log);
        assert!(!Log::enabled(&log));
        assert!(target.wait_writer_exit(Duration::from_secs(60)));
        TEST_TARGET.with(|cell| cell.set(None));
    }
}
