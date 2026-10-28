//! W8-20a: argument decoding and exit code pass-through of `pr_shell::entry`; no window. Port of `Program.cs` (new tests, no C# twin).
#![cfg(windows)]
use std::ffi::OsString;

use pr_shell::entry::{SMOKE_ARGUMENT, args_from_os, run_main};

fn os(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn args_from_os_skips_the_program_name() {
    // new: the CLR's `args` has no program name.
    assert_eq!(
        args_from_os(os(&["PhotoReviewRs.exe", "a.jpg"])),
        vec!["a.jpg".to_string()]
    );
}

#[test]
fn args_from_os_keeps_order_empty_arguments_and_spaces() {
    // new
    assert_eq!(
        args_from_os(os(&["x", "b c", "", "a"])),
        vec!["b c".to_string(), String::new(), "a".to_string()]
    );
}

#[test]
fn args_from_os_replaces_an_unpaired_surrogate_with_u_fffd() {
    // new
    use std::os::windows::ffi::OsStringExt;
    let bad = OsString::from_wide(&[0x61, 0xD800, 0x62]);
    assert_eq!(
        args_from_os(vec![OsString::from("x"), bad]),
        vec!["a\u{FFFD}b".to_string()]
    );
}

#[test]
fn args_from_os_of_nothing_is_empty() {
    // new
    assert!(args_from_os(Vec::<OsString>::new()).is_empty());
}

#[test]
fn run_main_returns_the_code_of_the_runner_unchanged() {
    // new: no clamping (T6).
    for code in [0, 1, 7, 255, 256, -1] {
        assert_eq!(run_main(os(&["x"]), |_| code), code);
    }
}

#[test]
fn run_main_hands_the_runner_the_decoded_arguments() {
    // new
    let mut seen = Vec::new();
    run_main(os(&["x", "--smoke", "p"]), |args| {
        seen = args.to_vec();
        0
    });
    assert_eq!(seen, vec!["--smoke".to_string(), "p".to_string()]);
}

#[test]
fn smoke_argument_is_dash_dash_smoke() {
    // C# Program.SmokeArgument == ShellLaunch.SmokeArgument.
    assert_eq!(SMOKE_ARGUMENT, "--smoke");
}
