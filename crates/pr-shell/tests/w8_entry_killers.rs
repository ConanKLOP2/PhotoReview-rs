//! W8 mutation killers for `pr_shell::entry` (cl-entry lane). No window, no real process, no sleeps; the runner is a fake closure.
//! `production_run` (entry.rs:42) cannot be killed here: it is `ShellStartup::run` with the default options and always opens a real window, so it is recorded as NOT run in
//! docs/reviews/CL-ENTRY-2026-10-30.md.
#![cfg(windows)]
use std::cell::Cell;
use std::ffi::OsString;

use pr_shell::entry::{args_from_os, run_main};

fn os(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

// entry.rs:21:5 `replace args_from_os -> Vec<String> with vec![]`: a non-empty tail must come back.
// entry.rs:21:5 `... with vec![String::new()]` and `... with vec!["xyzzy".into()]`: two distinct values, none equal to the constant.
#[test]
fn args_from_os_returns_exactly_the_tail_after_the_program_name() {
    assert_eq!(
        args_from_os(os(&["prog", "one", "two"])),
        vec!["one".to_string(), "two".to_string()]
    );
}

// entry.rs:21:5 (same three mutants): a program name alone gives an empty vector, so a constant vec![String::new()] / vec!["xyzzy"] is caught.
#[test]
fn args_from_os_of_only_the_program_name_is_empty() {
    assert!(args_from_os(os(&["prog"])).is_empty());
}

// entry.rs:35:5 `replace run_main -> i32 with 0 / 1 / -1`: three distinct runner codes (none of them 0, 1 or -1 at once), and the runner runs exactly once.
#[test]
fn run_main_returns_the_runner_code_and_runs_it_exactly_once() {
    for code in [42, 7, -5] {
        let calls = Cell::new(0);
        let got = run_main(os(&["prog", "a"]), |_| {
            calls.set(calls.get() + 1);
            code
        });
        assert_eq!(got, code);
        assert_eq!(calls.get(), 1);
    }
}
