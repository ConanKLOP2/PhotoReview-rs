//! Port tests for `pr_core::file_actions::partial_cleanup` (C# `PhotoReview.Core/FileActions/PartialDestinationCleanup.cs`,
//! baseline d92d6aae; RV-C03 / COPY-PARTIAL-01; PARITY F-21 / R01), plus the golden trace `tests/golden/partial-cleanup.json`
//! (inputs in `tests/golden-inputs/partial-cleanup.json`) and Rust-only twins that pin the mutants of the W3-08 contract.
//!
//! Red on purpose (D21, test-first): the stub bodies are `todo!()`, so every test here panics until the porter writes the
//! implementation. Fakes only, except the two `native` tests, which work inside a `TempRoot` and never touch the Recycle Bin.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use pr_core::abstractions::{FileStat, FileSystem};
use pr_core::file_actions::partial_cleanup::{is_same_file, remove_if_partial};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::time::DateTimeUtc;
use pr_test_support::golden::{self, bool_of, i64_of, repo_root, str_of};
use pr_test_support::{FakeFileSystem, FsOp, TempRoot};
use serde_json::Value;

/// C# `Stamp` of the audit tests: 2026-09-29 08:00:00 UTC, as .NET ticks.
const STAMP_TICKS: i64 = 638_000_000_000_000_000;
const DESTINATION: &str = r"C:\photos\selected\a.jpg";

fn stamp() -> DateTimeUtc {
    DateTimeUtc::from_ticks(STAMP_TICKS).expect("the test stamp is inside the .NET DateTime range")
}

fn at(ticks: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(ticks).expect("golden ticks are inside the .NET DateTime range")
}

fn stat(length: u64, ticks: i64) -> FileStat {
    FileStat {
        length,
        last_write_utc: at(ticks),
    }
}

fn destination() -> &'static Path {
    Path::new(DESTINATION)
}

// ---- C# twins (FileActionMutationAuditBTests, M23) ----

// C#: FileActionMutationAuditBTests.RemoveIfPartial_DestinationAsLongAsTheSource_IsKeptForRecoveryToJudge
#[test]
fn remove_if_partial_destination_as_long_as_the_source_is_kept_for_recovery_to_judge() {
    let fs = FakeFileSystem::new();
    fs.add_file_at(DESTINATION, b"jpeg", stamp());
    let created = fs.peek_stat(DESTINATION).expect("the destination was just added");

    remove_if_partial(&fs, destination(), 4, &created);

    assert!(
        fs.file_exists(destination()),
        "a complete copy is never deleted by the cleanup"
    );
}

// C#: FileActionMutationAuditBTests.RemoveIfPartial_DestinationShorterThanTheSource_IsDeleted
#[test]
fn remove_if_partial_destination_shorter_than_the_source_is_deleted() {
    let fs = FakeFileSystem::new();
    fs.add_file_at(DESTINATION, b"jp", stamp());
    let created = fs.peek_stat(DESTINATION).expect("the destination was just added");

    remove_if_partial(&fs, destination(), 4, &created);

    assert!(!fs.file_exists(destination()));
}

// ---- Rust-only twins: each pins one mutant of the W3-08 contract ----

/// Pins `<` -> `<=` on the length guard.
#[test]
fn remove_if_partial_destination_longer_than_the_source_is_kept() {
    let fs = FakeFileSystem::new();
    fs.add_file_at(DESTINATION, b"jpegs", stamp());
    let created = fs.peek_stat(DESTINATION).expect("the destination was just added");

    remove_if_partial(&fs, destination(), 4, &created);

    assert!(fs.file_exists(destination()));
    assert_eq!(fs.file_content(DESTINATION), Some(b"jpegs".to_vec()));
}

/// Pins dropping the time conjunct of `is_same_file` (one tick apart, same length).
#[test]
fn remove_if_partial_replaced_by_a_file_of_the_same_length_but_another_write_time_is_kept() {
    let fs = FakeFileSystem::new();
    fs.add_file_at(DESTINATION, b"jp", at(STAMP_TICKS + 1));
    let created = stat(2, STAMP_TICKS);

    remove_if_partial(&fs, destination(), 4, &created);

    assert!(
        fs.file_exists(destination()),
        "a foreign file of the same length but another write time is not ours"
    );
}

/// Pins dropping the length conjunct of `is_same_file` (shorter than the source, but not the created length).
#[test]
fn remove_if_partial_replaced_by_a_file_of_another_length_is_kept() {
    let fs = FakeFileSystem::new();
    fs.add_file_at(DESTINATION, b"j", stamp());
    let created = stat(2, STAMP_TICKS);

    remove_if_partial(&fs, destination(), 4, &created);

    assert!(fs.file_exists(destination()));
}

/// Pins the `None` return: a missing destination must not reach `delete`.
#[test]
fn remove_if_partial_missing_destination_does_not_call_delete() {
    let fs = FakeFileSystem::new();
    let created = stat(2, STAMP_TICKS);

    remove_if_partial(&fs, destination(), 4, &created);

    assert_eq!(fs.count(FsOp::Delete), 0);
}

/// Pins "a file that cannot be inspected stays" (a denied stat is never treated as a partial file).
#[test]
fn remove_if_partial_stat_failure_does_not_call_delete() {
    let fs = FakeFileSystem::new();
    fs.add_file_at(DESTINATION, b"jp", stamp());
    fs.fail(FsOp::Stat, DESTINATION, io::ErrorKind::PermissionDenied);
    let created = stat(2, STAMP_TICKS);

    remove_if_partial(&fs, destination(), 4, &created);

    assert_eq!(fs.count(FsOp::Delete), 0);
    fs.clear_faults();
    assert!(fs.file_exists(destination()));
}

/// Pins "panic on delete Err": a locked partial file is left in place without a panic.
#[test]
fn remove_if_partial_delete_failure_is_swallowed_and_the_file_stays() {
    let fs = FakeFileSystem::new();
    fs.add_file_at(DESTINATION, b"jp", stamp());
    fs.fail_locked(FsOp::Delete, DESTINATION);
    let created = stat(2, STAMP_TICKS);

    remove_if_partial(&fs, destination(), 4, &created);

    assert!(fs.file_exists(destination()));
    assert_eq!(
        fs.count(FsOp::Delete),
        1,
        "the one delete was attempted and its error swallowed"
    );
}

/// Pins the "evaluated on ONE stat" rule (the stat is taken once and never re-stat).
#[test]
fn remove_if_partial_stats_the_destination_exactly_once() {
    let fs = FakeFileSystem::new();
    fs.add_file_at(DESTINATION, b"jp", stamp());
    let created = stat(2, STAMP_TICKS);
    fs.reset_counts();

    remove_if_partial(&fs, destination(), 4, &created);

    assert_eq!(fs.count(FsOp::Stat), 1);
    assert_eq!(fs.count(FsOp::Delete), 1);
}

/// Pins the sign of `source_size`: zero and negative sources never delete, not even an empty destination.
#[test]
fn remove_if_partial_non_positive_source_size_never_deletes() {
    for source_size in [0_i64, -1] {
        let fs = FakeFileSystem::new();
        fs.add_file_at(DESTINATION, b"", stamp());
        let created = stat(0, STAMP_TICKS);

        remove_if_partial(&fs, destination(), source_size, &created);

        assert!(fs.file_exists(destination()), "source_size {source_size}: must be kept");
        assert_eq!(fs.count(FsOp::Delete), 0, "source_size {source_size}: no delete call");
    }
}

/// A kept destination keeps its bytes (the cleanup never rewrites or truncates).
#[test]
fn remove_if_partial_kept_destination_keeps_its_bytes() {
    let fs = FakeFileSystem::new();
    fs.add_file_at(DESTINATION, b"jpeg", stamp());
    let created = fs.peek_stat(DESTINATION).expect("the destination was just added");

    remove_if_partial(&fs, destination(), 4, &created);

    assert_eq!(fs.file_content(DESTINATION), Some(b"jpeg".to_vec()));
}

/// `is_same_file` is an exact compare on both fields: the four combinations.
#[test]
fn is_same_file_requires_both_length_and_write_time() {
    let created = stat(5, STAMP_TICKS);

    assert!(
        is_same_file(&stat(5, STAMP_TICKS), &created),
        "equal length and write time"
    );
    assert!(!is_same_file(&stat(6, STAMP_TICKS), &created), "length differs");
    assert!(
        !is_same_file(&stat(5, STAMP_TICKS + 1), &created),
        "write time differs by one tick"
    );
    assert!(!is_same_file(&stat(6, STAMP_TICKS + 1), &created), "both differ");
}

// ---- Golden `partial-cleanup` (26 cases: 17 cleanup, 9 same-file) ----

/// Input of one golden case, from `tests/golden-inputs/partial-cleanup.json`, keyed by case name.
fn golden_inputs() -> HashMap<String, Value> {
    let path = repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("partial-cleanup.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .iter()
        .map(|case| (str_of(case, "name").to_owned(), case.clone()))
        .collect()
}

/// `{"len": n, "ticks": t}` as a [`FileStat`].
fn stat_of(value: &Value) -> FileStat {
    let length = u64::try_from(i64_of(value, "len")).expect("golden lengths are non-negative");
    stat(length, i64_of(value, "ticks"))
}

#[test]
fn golden_partial_cleanup_cleanup_cases() {
    let golden = golden::load("partial-cleanup");
    let inputs = golden_inputs();
    let mut checked = 0;

    for case in golden.cases.iter().filter(|c| str_of(c, "kind") == "cleanup") {
        let name = str_of(case, "name");
        let input = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no input for golden case {name}"));

        let fs = FakeFileSystem::new();
        if !input["dest"].is_null() {
            let len = usize::try_from(i64_of(&input["dest"], "len")).expect("golden lengths are non-negative");
            fs.add_file_at(DESTINATION, &vec![0_u8; len], at(i64_of(&input["dest"], "ticks")));
        }
        if bool_of(input, "delete_fails") {
            fs.fail_locked(FsOp::Delete, DESTINATION);
        }
        let created = stat_of(&input["created"]);
        let source_size = i64_of(input, "source_size");

        let existed_before = fs.file_exists(destination());
        // A panic here is the C# `catch` leaking: the golden `threw` is null for every case.
        remove_if_partial(&fs, destination(), source_size, &created);
        let exists_after = fs.file_exists(destination());
        let deleted = existed_before && !exists_after;

        assert_eq!(
            existed_before,
            bool_of(case, "existed_before"),
            "{name}: existed_before"
        );
        assert_eq!(exists_after, bool_of(case, "exists_after"), "{name}: exists_after");
        assert_eq!(deleted, bool_of(case, "deleted"), "{name}: deleted");
        assert!(case["threw"].is_null(), "{name}: golden threw must be null");
        checked += 1;
    }

    assert_eq!(checked, 17, "the golden has 17 cleanup cases");
}

#[test]
fn golden_partial_cleanup_same_file_cases() {
    let golden = golden::load("partial-cleanup");
    let inputs = golden_inputs();
    let mut checked = 0;

    for case in golden.cases.iter().filter(|c| str_of(c, "kind") == "same-file") {
        let name = str_of(case, "name");
        let input = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no input for golden case {name}"));
        let now = stat_of(&input["now"]);
        let created = stat_of(&input["created"]);

        assert_eq!(
            is_same_file(&now, &created),
            bool_of(case, "same_file"),
            "{name}: same_file"
        );
        checked += 1;
    }

    assert_eq!(checked, 9, "the golden has 9 same-file cases");
}

// ---- Optional native twins (real disk, inside a TempRoot; never the Recycle Bin) ----

// C#: none (Rust-only: the C# suite has no real-disk test for this unit)
#[test]
#[ignore = "native: real temp file delete"]
fn remove_if_partial_native_shorter_file_with_the_same_stamp_is_deleted() {
    let root = TempRoot::new("partial-cleanup-delete");
    let dest: PathBuf = root.join("a.jpg");
    fs::write(&dest, b"jp").expect("write the partial file");
    let physical = PhysicalFileSystem;
    let created = physical.get_file_stat(&dest).expect("the partial file exists");

    remove_if_partial(&physical, &dest, 4, &created);

    assert!(!dest.exists());
}

// C#: none (Rust-only: the C# suite has no real-disk test for this unit)
#[test]
#[ignore = "native: real temp file held open"]
fn remove_if_partial_native_held_open_file_stays_without_a_panic() {
    use std::os::windows::fs::OpenOptionsExt;

    let root = TempRoot::new("partial-cleanup-held");
    let dest: PathBuf = root.join("a.jpg");
    fs::write(&dest, b"jp").expect("write the partial file");
    let physical = PhysicalFileSystem;
    let created = physical.get_file_stat(&dest).expect("the partial file exists");
    // share_mode(0): another process cannot open or delete the file while this handle is alive.
    let _held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&dest)
        .expect("hold the partial file open");

    remove_if_partial(&physical, &dest, 4, &created);

    assert!(dest.exists());
}
