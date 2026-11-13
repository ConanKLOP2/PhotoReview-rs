//! Killer tests for the .NET `PathInternal` port in `viewmodels/info_overlay.rs` (`is_device`, `root_length`).
//! Expected values are derived by hand from the .NET `GetRootLength` / `IsDevice` rules, not from the Rust output.
//! `InfoOverlayChanges::none` -> `Default::default()` is an equivalent mutant (derived `Default` is `Self(0)`).

use pr_app::viewmodels::info_overlay::{display_name, is_device_for_tests, root_length_for_tests};
use std::sync::mpsc;
use std::time::Duration;

/// Runs `root_length` on a worker thread; a hang (mutant infinite loop) fails after 20 s. The spinner is leaked.
fn root_bounded(text: &str) -> usize {
    let (tx, rx) = mpsc::channel();
    let owned = text.to_owned();
    std::thread::spawn(move || {
        let _ = tx.send(root_length_for_tests(&owned));
    });
    rx.recv_timeout(Duration::from_secs(20))
        .expect("root_length did not finish (infinite loop)")
}

#[test]
fn is_device_cases() {
    // Extended prefix (first clause), device prefix with either separator, and negatives.
    assert!(is_device_for_tests(r"\\?\C:\x"));
    assert!(is_device_for_tests(r"\\.\C:\x"));
    assert!(is_device_for_tests(r"\??\C:\x"));
    assert!(is_device_for_tests(r"\\?\"));
    assert!(is_device_for_tests("//?/C:/x"));
    assert!(is_device_for_tests("//./x"));
    assert!(is_device_for_tests(r"\\.\"));
    assert!(is_device_for_tests(r"/\.\x"));
    // Negatives: second char neither backslash nor `?`, wrong third char, short, plain UNC.
    assert!(!is_device_for_tests(r"\x?\y"));
    assert!(!is_device_for_tests(r"\\x\y"));
    assert!(!is_device_for_tests(r"\\?"));
    assert!(!is_device_for_tests(r"C:\x"));
    assert!(!is_device_for_tests(r"\\srv\share"));
    assert!(!is_device_for_tests(""));
}

#[test]
fn root_length_device_roots() {
    assert_eq!(root_bounded(r"\\?\C:\x"), 7);
    assert_eq!(root_bounded(r"\\.\C:\x"), 7);
    assert_eq!(root_bounded("//?/C:/x"), 7);
    assert_eq!(root_bounded(r"\??\C:\x"), 7);
    assert_eq!(root_bounded(r"\\?\"), 4);
    assert_eq!(root_bounded(r"\\.\"), 4);
    assert_eq!(root_bounded(r"\\?\a"), 5);
    assert_eq!(root_bounded(r"\\?\ab\c"), 7);
    // Empty segment right after the prefix: no separator is consumed (`index > 4` guard).
    assert_eq!(root_bounded(r"\\?\\x"), 4);
    // A device path that is not a UNC.
    assert_eq!(root_bounded(r"\\?\UNCx\a"), 9);
}

#[test]
fn root_length_unc() {
    assert_eq!(root_bounded(r"\\srv\share\dir"), 11);
    assert_eq!(root_bounded(r"\\?\UNC\srv\share\dir"), 17);
    assert_eq!(root_bounded(r"\\srv"), 5);
    assert_eq!(root_bounded(r"\\srv\share"), 11);
    assert_eq!(root_bounded(r"\\"), 2);
    assert_eq!(root_bounded(r"\\?"), 3);
}

#[test]
fn root_length_drive_and_rooted() {
    assert_eq!(root_bounded(r"\x"), 1);
    assert_eq!(root_bounded(r"\"), 1);
    assert_eq!(root_bounded("C:"), 2);
    assert_eq!(root_bounded(r"C:\"), 3);
    assert_eq!(root_bounded(r"C:\x"), 3);
    assert_eq!(root_bounded("C:x"), 2);
    assert_eq!(root_bounded(r"1:\x"), 0);
}

#[test]
fn root_length_short_strings() {
    assert_eq!(root_bounded(""), 0);
    assert_eq!(root_bounded("a"), 0);
    assert_eq!(root_bounded("ab"), 0);
    assert_eq!(root_bounded("abc"), 0);
    assert_eq!(root_bounded("abcd"), 0);
}

#[test]
fn display_name_reaches_root_logic() {
    // Not a device (second char is `x`): root is 1, so the file name is `foo`.
    // Bounded: the `+=` -> `*=` mutants of the separator scans spin for ever, so an unbounded call would hang the binary.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send([
            display_name(r"\x?\foo"),
            display_name(r"C:foo"),
            display_name(r"\\srv\share"),
            display_name(r"\\?\C:\x"),
        ]);
    });
    let names = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("display_name did not finish within 20 s (infinite loop)");
    assert_eq!(names, ["foo", "foo", r"\\srv\share", "x"]);
}
