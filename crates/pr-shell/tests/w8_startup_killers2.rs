//! cl-startup mutation regressions for `pr-shell` startup files. Fakes only; no window, no real log file, no user data.
#![cfg(windows)]

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_app::startup::options::EnvSource;
use pr_core::abstractions::Log;
use pr_core::diagnostics::diag_options::DiagOptions;
use pr_shell::startup::graph::{GlobalLog, diag_options, global_log};
use pr_shell::startup::options::ShellStartupOptions;

#[derive(Default)]
struct MemoryLog {
    enabled: AtomicBool,
    lines: Mutex<Vec<String>>,
}

impl Log for MemoryLog {
    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
    fn info(&self, message: &str) {
        self.lines.lock().unwrap().push(format!("info:{message}"));
    }
    fn warn(&self, message: &str) {
        self.lines.lock().unwrap().push(format!("warn:{message}"));
    }
    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        let detail = error.map(ToString::to_string).unwrap_or_default();
        self.lines.lock().unwrap().push(format!("error:{message}|{detail}"));
    }
}

impl AppLogTarget for MemoryLog {
    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
    fn flush(&self) {}
    fn shutdown(&self) {}
    fn file_path(&self) -> PathBuf {
        PathBuf::new()
    }
}

#[derive(Debug)]
struct Boom;
impl std::fmt::Display for Boom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("boom")
    }
}
impl Error for Boom {}

/// One test owns the process-wide `AppLog` instance of this binary (no other test here touches it).
#[test]
fn global_log_forwards_every_member_to_the_process_wide_app_log() {
    // Mutants: crates/pr-shell/src/startup/graph.rs:111 `enabled` -> true/false, :116 `info`, :121 `warn`, :126 `error` -> ().
    let memory = Arc::new(MemoryLog::default());
    let previous = AppLog::global().instance();
    AppLog::global().set_instance(Some(memory.clone()));
    let log = global_log();

    memory.set_enabled(false);
    assert!(!log.enabled());
    memory.set_enabled(true);
    assert!(log.enabled());
    log.info("alpha");
    log.warn("beta");
    log.error("gamma", Some(&Boom));
    log.error("delta", None);
    assert!(GlobalLog.enabled());

    AppLog::global().set_instance(Some(previous));
    let lines = memory.lines.lock().unwrap().clone();
    assert_eq!(
        lines,
        vec!["info:alpha", "warn:beta", "error:gamma|boom", "error:delta|"]
    );
}

#[test]
fn diag_options_is_the_process_wide_snapshot_not_a_fresh_default() {
    // Mutant: crates/pr-shell/src/startup/graph.rs:137 `diag_options` -> `Arc::new(Default::default())`.
    // The snapshot is one cached Arc (C# `Lazy<Snapshot>`): a fresh Arc is a different allocation.
    assert!(Arc::ptr_eq(&diag_options(), &DiagOptions::current()));
    assert!(Arc::ptr_eq(&diag_options(), &diag_options()));
}

struct Env;
impl EnvSource for Env {
    fn var(&self, _name: &str) -> Option<String> {
        None
    }
}

#[test]
fn shell_startup_options_debug_names_the_type_and_its_scalar_members() {
    // Mutant: crates/pr-shell/src/startup/options.rs:76 `<Debug for ShellStartupOptions>::fmt` -> Ok(Default::default()).
    let text = format!("{:?}", ShellStartupOptions::from_env(&Env));
    assert!(text.contains("ShellStartupOptions"), "{text}");
    assert!(text.contains("activate: true"), "{text}");
    assert!(text.contains("smoke_timeout"), "{text}");
}
