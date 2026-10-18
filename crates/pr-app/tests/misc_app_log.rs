//! Red tests for `pr_app::misc::app_log` (contract W8-04d). Port of `PhotoReview.App.Shared/AppLog.cs` (C# baseline
//! d92d6aae) and the facade parts of `PhotoReview.App.Tests/AppLogTests.cs` and `AppLogDisposeBranchTests.cs`.
//!
//! Every test uses a `FileLog` under a [`TempRoot`]; none reaches `AppLog::global()` or `DefaultFileLog` (trap 5).
//! Flushing is the barrier: no sleeps, no polling (AGENTS.md, Tests).

use std::error::Error;
use std::fs;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_core::diagnostics::file_log::{DEFAULT_MAX_LOG_BYTES, FileLog};
use pr_test_support::TempRoot;

/// A facade over a real `FileLog` at `temp/<name>`; the log file and its directory do not exist yet.
fn facade_at(temp: &TempRoot, name: &str) -> (AppLog, PathBuf) {
    let path = temp.join(name);
    let target: Arc<dyn AppLogTarget> =
        Arc::new(FileLog::new(&path, DEFAULT_MAX_LOG_BYTES).expect("the test log path is absolute"));
    (AppLog::new(Some(target)), path)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn other(text: &str) -> io::Error {
    io::Error::other(text.to_owned())
}

#[test]
fn enabled_defaults_off_for_a_new_file_log() {
    // C#: AppLogTests.LoggingDefaultsOff (the AppLog.Enabled half; the AppSettings half is pr_core::settings)
    let temp = TempRoot::new("app-log-default");
    let (log, _) = facade_at(&temp, "app.log");

    assert!(!log.enabled());
}

#[test]
fn disabled_info_and_error_create_no_directory_or_file() {
    // C#: AppLogTests.DisabledLoggingCreatesNoDirectoryOrFile
    let temp = TempRoot::new("app-log-disabled");
    let (log, path) = facade_at(&temp, "logs/app.log");
    let error = other("test");
    let error_ref: &(dyn Error + 'static) = &error;

    log.info("disabled-info");
    log.error("disabled-error", Some(error_ref));

    assert!(!path.parent().expect("the log path has a parent").exists());
    assert!(!path.exists());
}

#[test]
fn enabled_info_and_error_are_written_after_flush() {
    // C#: AppLogTests.ExplicitlyEnabledLoggingWritesDiagnostics
    let temp = TempRoot::new("app-log-enabled");
    let (log, path) = facade_at(&temp, "app.log");

    log.set_enabled(true);
    log.info("enabled-info");
    log.error("enabled-error", None);
    log.flush();

    let text = read(&path);
    assert!(text.contains("enabled-info"));
    assert!(text.contains("enabled-error"));
}

#[test]
fn turning_logging_off_stops_all_writes() {
    // C#: AppLogTests.TurningLoggingOffStopsAllDiagnosticWrites
    let temp = TempRoot::new("app-log-off");
    let (log, path) = facade_at(&temp, "app.log");
    log.set_enabled(true);
    log.info("enabled-info");
    log.error("enabled-error", None);
    log.flush();
    let before = read(&path);

    log.set_enabled(false);
    log.info("disabled-again");
    log.error("disabled-again", None);
    log.flush();

    assert_eq!(before, read(&path));
}

#[test]
fn write_forced_turns_logging_on_flushes_and_restores_off() {
    // C#: AppLogDisposeBranchTests.WriteForced_LoggingOff_IsOnWhileWritingAndFlushedBeforeReturning
    let temp = TempRoot::new("app-log-forced-off");
    let (log, path) = facade_at(&temp, "app.log");
    let mut enabled_during_write = false;

    log.write_forced(|| {
        enabled_during_write = log.enabled();
        log.info("flushed-marker");
    });

    assert!(enabled_during_write);
    assert!(!log.enabled());
    // No explicit flush here: write_forced itself must have flushed.
    assert!(read(&path).contains("flushed-marker"));
}

#[test]
fn write_forced_keeps_logging_on_when_already_on() {
    // C#: AppLogDisposeBranchTests.WriteForced_LoggingAlreadyEnabled_StaysEnabledAfterwards
    let temp = TempRoot::new("app-log-forced-on");
    let (log, path) = facade_at(&temp, "app.log");
    log.set_enabled(true);
    let writes_before = log.enabled_write_count();

    log.write_forced(|| log.info("already-enabled-marker"));

    assert!(log.enabled());
    assert!(read(&path).contains("already-enabled-marker"));
    // Trap 1: write_forced is two counted Enabled writes (set to true, then restore).
    assert_eq!(log.enabled_write_count(), writes_before + 2);
}

#[test]
fn write_forced_panic_restores_flag_and_releases_lock() {
    // C#: AppLogDisposeBranchTests.WriteForced_WriteThrows_RethrowsAndStillRestoresTheEnabledFlag
    let temp = TempRoot::new("app-log-forced-panic");
    let (log, path) = facade_at(&temp, "app.log");

    let result = catch_unwind(AssertUnwindSafe(|| log.write_forced(|| panic!("write-failed"))));

    assert!(result.is_err(), "the panic must propagate to the caller");
    assert!(!log.enabled(), "the restore guard must run on the panic path");
    // The lock is released too: a second forced write is not blocked and works (trap 3: no poisoning).
    log.write_forced(|| log.info("after-failed-write-marker"));
    assert!(read(&path).contains("after-failed-write-marker"));
}

#[test]
fn write_forced_nested_on_same_thread_does_not_deadlock() {
    // Trap 4 (new): C# lock is re-entrant; a nested write_forced on the owning thread must enter.
    // Bounded by the test harness: a deadlock shows as a hung test (run with a timeout).
    let temp = TempRoot::new("app-log-forced-nested");
    let (log, path) = facade_at(&temp, "app.log");

    log.write_forced(|| {
        log.write_forced(|| log.info("nested-marker"));
        log.info("outer-marker");
    });

    assert!(!log.enabled());
    let text = read(&path);
    assert!(text.contains("nested-marker"));
    assert!(text.contains("outer-marker"));
}

#[test]
fn error_forced_records_message_and_error_while_off() {
    // C#: AppLogDisposeBranchTests.LogStartupErrorForced_RecordsMessageAndExceptionWhileLoggingIsOff (body = error_forced)
    let temp = TempRoot::new("app-log-error-forced");
    let (log, path) = facade_at(&temp, "app.log");
    let error = other("startup-detail");

    log.error_forced("startup-error-marker", &error);

    assert!(!log.enabled());
    let text = read(&path);
    assert!(text.contains("startup-error-marker"));
    assert!(text.contains("startup-detail"));
}

#[test]
fn shutdown_flushes_queued_entries_and_disables() {
    // C#: AppLogDisposeBranchTests.Dispose_ShutsTheLogDown_AfterFlushingWhatWasQueued (here as AppLog::shutdown)
    let temp = TempRoot::new("app-log-shutdown");
    let (log, path) = facade_at(&temp, "app.log");
    log.set_enabled(true);
    log.info("queued-before-shutdown-marker");

    log.shutdown();

    assert!(!log.enabled(), "shutdown switches logging off");
    assert!(read(&path).contains("queued-before-shutdown-marker"));
}

#[test]
fn set_enabled_counts_every_write_even_when_unchanged() {
    // Trap 1 (new): every set_enabled is counted, including an unchanged value; set_instance is not counted.
    let temp = TempRoot::new("app-log-count");
    let (log, _) = facade_at(&temp, "app.log");
    assert_eq!(log.enabled_write_count(), 0);

    log.set_enabled(false);
    log.set_enabled(false);
    log.set_enabled(true);
    assert_eq!(log.enabled_write_count(), 3);

    let (second, _) = facade_at(&temp, "second.log");
    let third: Arc<dyn AppLogTarget> =
        Arc::new(FileLog::new(temp.join("third.log"), DEFAULT_MAX_LOG_BYTES).expect("the test log path is absolute"));
    second.set_instance(Some(third));
    assert_eq!(second.enabled_write_count(), 0);
}

#[test]
fn set_instance_none_selects_default_target() {
    // Trap 5 (new): None selects DefaultFileLog. Checks file_path() only: never logs, never touches the global default.
    let temp = TempRoot::new("app-log-default-target");
    let (log, _) = facade_at(&temp, "app.log");

    log.set_instance(None);

    assert_eq!(log.file_path(), FileLog::default_instance().file_path().to_path_buf());
}

#[test]
fn concurrent_forced_writes_restore_the_enabled_flag() {
    // C#: AppLogTests.ConcurrentForcedLoggingRestoresTheEnabledFlag (8 workers, fixed counts, no sleeps)
    let temp = TempRoot::new("app-log-concurrent-forced");
    let (log, path) = facade_at(&temp, "app.log");
    let log_ref = &log;

    thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|worker| {
                scope.spawn(move || {
                    for i in 0..50 {
                        log_ref.write_forced(|| log_ref.info(&format!("forced-{worker}-{i}")));
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("a forced-write worker panicked");
        }
    });

    assert!(!log.enabled());
    log.flush();
    let text = read(&path);
    assert!(text.contains("forced-0-0"));
    assert!(text.contains("forced-7-49"));
}

#[test]
fn concurrent_logging_preserves_every_entry() {
    // C#: AppLogTests.ConcurrentLoggingPreservesEveryEntry (200 threads, one entry each)
    let temp = TempRoot::new("app-log-concurrent-entries");
    let (log, path) = facade_at(&temp, "app.log");
    log.set_enabled(true);
    let log_ref = &log;

    thread::scope(|scope| {
        let handles: Vec<_> = (0..200)
            .map(|i| scope.spawn(move || log_ref.info(&format!("concurrent-marker-{i}"))))
            .collect();
        for handle in handles {
            handle.join().expect("a logging thread panicked");
        }
    });
    log.flush();

    let text = read(&path);
    for i in 0..200 {
        assert!(text.contains(&format!("concurrent-marker-{i}")), "entry {i} is missing");
    }
}

#[test]
fn concurrent_entries_stay_line_delimited() {
    // C#: AppLogTests.ConcurrentLogEntriesRemainLineDelimited: one entry per line, never two entries on one line.
    let temp = TempRoot::new("app-log-concurrent-lines");
    let (log, path) = facade_at(&temp, "app.log");
    log.set_enabled(true);
    let log_ref = &log;

    thread::scope(|scope| {
        let handles: Vec<_> = (0..200)
            .map(|i| scope.spawn(move || log_ref.info(&format!("concurrent-marker-{i}"))))
            .collect();
        for handle in handles {
            handle.join().expect("a logging thread panicked");
        }
    });
    log.flush();

    let text = read(&path);
    let marked: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("concurrent-marker-"))
        .collect();
    assert_eq!(marked.len(), 200);
    for line in marked {
        assert_eq!(
            line.matches("concurrent-marker-").count(),
            1,
            "two entries share a line: {line}"
        );
    }
}
