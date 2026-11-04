//! N4 (2026-11-05): run-based equivalence proofs for the six `tool_path_guard.rs` survivors of the whole-file mutation run
//! (226:9, 276:62 `|` -> `&` and `|` -> `^`, 299:5 twice, 299:76). Hand-written, no golden file, no sleep.
//!
//! The Windows tests pass on the original and under the mutants they name; that is the proof (the visible result is the same on
//! the whole domain of the difference). 299:5 and 299:76 are in the `#[cfg(not(windows))]` body of `leaf_refuses_replacement`:
//! they are not compiled into this Windows test binary at all, so no test on Windows can tell them apart and no run on
//! Windows can prove them; they stay OPEN until a run on a non-Windows target.

#![cfg(windows)]

use pr_bench::cli::tool_path_guard::write_report_file;
use pr_test_support::TempRoot;

/// 226:9 `delete match arm Some(ERROR_PATH_NOT_FOUND)` in `map_write_error`: a report path whose parent folder is missing fails
/// with Win32 error 3. With the arm, it is `DirectoryNotFoundException`; without it the fallback arm maps `ErrorKind::NotFound`
/// to the same type and the same text, so the visible error is identical (this pins that error, which both versions give).
#[test]
fn a_missing_parent_folder_is_a_directory_not_found_error_with_the_dotnet_text() {
    let root = TempRoot::new("n4-pathnotfound");
    let target = root.join("no-such-folder").join("report.json");
    let text = target.to_string_lossy().into_owned();
    let error = write_report_file(&text, "x").expect_err("the parent folder does not exist");
    assert_eq!(error.type_name, "DirectoryNotFoundException");
    // The write goes to a temp sibling `<name>.<hex>.tmp` first, and the text names that file.
    let expected_start = format!("Could not find a part of the path '{text}.");
    assert!(error.message.starts_with(&expected_start), "{}", error.message);
    assert!(error.message.ends_with(".tmp'."), "{}", error.message);
}

/// 276:62 `FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_DIRECTORY` -> `^` (same value: the bits are distinct) and -> `&` (mask 0, so
/// the up-front refusal never fires and `rename` decides). Replacing a directory, a read-only directory or a read-only file
/// is refused with the path-less `UnauthorizedAccessException` either way; replacing a plain file works either way.
#[test]
#[allow(clippy::permissions_set_readonly_false)] // Windows only: clears the read-only attribute again
fn replacing_a_directory_or_read_only_leaf_is_refused_without_the_path() {
    let root = TempRoot::new("n4-replace-leaf");
    let directory = root.join("dir.json");
    std::fs::create_dir(&directory).expect("directory");
    let read_only_directory = root.join("rodir.json");
    std::fs::create_dir(&read_only_directory).expect("directory");
    let read_only_file = root.join("ro.json");
    std::fs::write(&read_only_file, "keep").expect("file");
    let plain = root.join("plain.json");
    std::fs::write(&plain, "old").expect("file");

    for read_only in [&read_only_directory, &read_only_file] {
        let mut permissions = std::fs::metadata(read_only).expect("metadata").permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(read_only, permissions).expect("set read-only");
    }
    let outcomes: Vec<_> = [&directory, &read_only_directory, &read_only_file]
        .iter()
        .map(|leaf| write_report_file(&leaf.to_string_lossy(), "new"))
        .collect();
    for read_only in [&read_only_directory, &read_only_file] {
        let mut permissions = std::fs::metadata(read_only).expect("metadata").permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(read_only, permissions).expect("clear read-only");
    }
    for (index, outcome) in outcomes.into_iter().enumerate() {
        let error = outcome.expect_err("a directory or read-only leaf cannot be replaced");
        assert_eq!(error.type_name, "UnauthorizedAccessException", "leaf {index}");
        assert_eq!(error.message, "Access to the path is denied.", "leaf {index}");
    }
    assert_eq!(std::fs::read_to_string(&read_only_file).expect("read"), "keep");
    write_report_file(&plain.to_string_lossy(), "new").expect("a plain file is replaced");
    assert_eq!(std::fs::read_to_string(&plain).expect("read"), "new");
}
