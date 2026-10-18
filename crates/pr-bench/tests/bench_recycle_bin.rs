//! Port of `PhotoReview.Benchmarking/BenchmarkRecycleBin.cs` (C# baseline d92d6aae), W7-02b, plus golden
//! `bench-recycle-bin` (oracle-cs, 18 cases: 10 env spellings, 7 delete setups, members).
//!
//! AGENTS.md "Recycle Bin": no test here calls a real `RecycleBin`. The factory passed to `create` and `create_with`
//! panics or stands in as `FakeRecycleBin`; the default deleter only ever removes files this test created inside a
//! `TempRoot`. The process environment is never modified (edition 2024 makes `set_var` unsafe), so the switch is
//! tested through `is_real_bin_opt_in` and `create_with`.

use std::cell::Cell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_bench::bench_recycle_bin::{
    REAL_BIN_ENVIRONMENT_VARIABLE, create, create_with, is_real_bin_opt_in, use_real_bin,
};
use pr_core::abstractions::RecycleBin;
use pr_core::time::DateTimeUtc;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use pr_test_support::{FakeFileSystem, FakeRecycleBin, TempRoot};
use serde_json::Value;

/// A factory that must never run (the real bin is not opted in).
fn no_real_bin() -> Arc<dyn RecycleBin> {
    panic!("the real bin must not be created");
}

// C#: BenchmarkRecycleBinTests.Create_Default_DeletesTempCopyWithoutRealBin
#[test]
fn create_default_deletes_temp_copy_without_real_bin() {
    assert!(
        !use_real_bin(),
        "{REAL_BIN_ENVIRONMENT_VARIABLE} must not be set for the test run."
    );
    let root = TempRoot::new("bench-bin-default");
    let temp = root.join("PhotoReview-BenchBin.bin");
    fs::write(&temp, [1u8]).expect("write the temp copy");

    let bin = create(no_real_bin);
    bin.send_to_recycle_bin(&temp).expect("the temp copy is deleted");

    assert!(!temp.exists(), "the temp copy must be gone");
}

// New (no C# twin): the constant is the Rust `PHOTOREVIEW_RS_*` name (R0002, D4), not the C# name.
#[test]
fn real_bin_environment_variable_uses_the_rs_prefix() {
    assert_eq!(REAL_BIN_ENVIRONMENT_VARIABLE, "PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN");
}

// New (no C# twin): the real bin, when opted in, is returned as is and receives the delete; the fake stands in for it.
#[test]
fn create_with_true_returns_the_real_bin_and_never_deletes() {
    let file_system = Arc::new(FakeFileSystem::new());
    let path = Path::new(r"C:\fake\photo-001.jpg");
    file_system.add_file(path, b"photo");

    let real = Arc::new(FakeRecycleBin::new(Arc::clone(&file_system)));
    let real_dyn: Arc<dyn RecycleBin> = real.clone();
    let for_factory = Arc::clone(&real_dyn);

    let bin = create_with(true, || for_factory);
    assert!(
        Arc::ptr_eq(&bin, &real_dyn),
        "the opted-in bin is the factory's instance"
    );

    bin.send_to_recycle_bin(path).expect("the fake bin accepts the item");
    assert_eq!(
        real.binned_paths(),
        vec![path.to_path_buf()],
        "the fake recorded the delete"
    );
    assert!(
        file_system.file_content(path).is_none(),
        "the fake file system moved the file into the fake bin"
    );
}

/// Golden `env-*` cases: `Some(value)` through `is_real_bin_opt_in`, then `create_with` with that switch.
fn check_env_case(case: &Value) {
    let name = str_of(case, "name");
    let value = case["value"].as_str();
    assert_eq!(
        is_real_bin_opt_in(value),
        bool_of(case, "use_real_bin"),
        "{name}: opt-in switch for {value:?}"
    );

    let calls = Cell::new(0_i64);
    let factory_instance: Arc<dyn RecycleBin> = Arc::new(FakeRecycleBin::new(Arc::new(FakeFileSystem::new())));
    let returned = create_with(is_real_bin_opt_in(value), || {
        calls.set(calls.get() + 1);
        Arc::clone(&factory_instance)
    });

    assert_eq!(calls.get(), i64_of(case, "factory_calls"), "{name}: factory calls");
    assert_eq!(
        Arc::ptr_eq(&returned, &factory_instance),
        bool_of(case, "returned_factory_instance"),
        "{name}: returned instance is the factory's"
    );
}

/// Setup of a `delete-*` golden case inside `root`. Returns the path to delete. The target path is always
/// `root/target.bin` (or a path below it), so `target_file_exists_after` and `target_directory_exists_after` refer to it.
fn set_up_delete_case(root: &TempRoot, setup: &str) -> PathBuf {
    let target = root.join("target.bin");
    match setup {
        "file" => {
            fs::write(&target, b"bench").expect("write target file");
            target
        }
        "missing-file" => target,
        "missing-dir" => root.join("missing-dir").join("target.bin"),
        "directory" => {
            fs::create_dir(&target).expect("create target directory");
            target
        }
        "readonly-file" => {
            fs::write(&target, b"bench").expect("write target file");
            let mut permissions = fs::metadata(&target).expect("metadata").permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&target, permissions).expect("set read-only");
            target
        }
        "empty-path" => PathBuf::new(),
        "file-as-parent" => {
            let parent = root.join("parent.bin");
            fs::write(&parent, b"bench").expect("write parent file");
            parent.join("target.bin")
        }
        other => panic!("unknown delete setup {other}"),
    }
}

/// Clears the read-only attribute so the `TempRoot` can be removed when the case ends.
// Justified: Windows-only test cleanup of a file this test made itself; the unix world-writable concern does not apply.
#[allow(clippy::permissions_set_readonly_false)]
fn clear_read_only(path: &Path) {
    if let Ok(metadata) = fs::metadata(path) {
        let mut permissions = metadata.permissions();
        if permissions.readonly() {
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions).expect("clear read-only");
        }
    }
}

/// Golden `delete-*` cases: the temp-copy deleter's `send_to_recycle_bin` must match .NET `File.Delete`.
fn check_delete_case(case: &Value) {
    let name = str_of(case, "name");
    let root = TempRoot::new("bench-bin-delete");
    let target = set_up_delete_case(&root, str_of(case, "setup"));

    // The deleter is reached through the public switch with the opt-in off; the factory must not run.
    let bin = create_with(false, no_real_bin);
    let outcome = bin.send_to_recycle_bin(&target);

    // Golden outcome names map to the kinds of the notes: DirectoryNotFound => NotFound,
    // UnauthorizedAccess => PermissionDenied, Argument => InvalidInput. Any other value is a golden bug.
    let expected_kind = match str_of(case, "outcome") {
        "ok" => None,
        "DirectoryNotFoundException" => Some(io::ErrorKind::NotFound),
        "UnauthorizedAccessException" => Some(io::ErrorKind::PermissionDenied),
        "ArgumentException" => Some(io::ErrorKind::InvalidInput),
        other => panic!("{name}: unknown golden outcome {other}; contract bug"),
    };
    match (outcome, expected_kind) {
        (Ok(()), None) => {}
        (Err(err), Some(kind)) => assert_eq!(err.kind(), kind, "{name}: error kind ({err})"),
        (result, expected) => panic!("{name}: expected {expected:?}, got {result:?}"),
    }

    assert_eq!(
        target.is_file(),
        bool_of(case, "target_file_exists_after"),
        "{name}: file exists after"
    );
    assert_eq!(
        target.is_dir(),
        bool_of(case, "target_directory_exists_after"),
        "{name}: directory exists after"
    );
    clear_read_only(&target);
}

/// Golden `members` case: the default deleter's own answers, never a real bin.
fn check_members(case: &Value) {
    let name = str_of(case, "name");
    let bin = create_with(false, no_real_bin);
    // Paths are never touched by these members: the default deleter answers from the path text alone.
    let path = Path::new(r"C:\bench\any-photo.jpg");

    assert_eq!(
        bin.can_recycle(path),
        bool_of(case, "can_recycle"),
        "{name}: can_recycle"
    );
    assert_eq!(
        bin.fits_in_recycle_bin(path, 0),
        bool_of(case, "fits_zero"),
        "{name}: fits_in_recycle_bin(0)"
    );
    assert_eq!(
        bin.fits_in_recycle_bin(path, u64::MAX),
        bool_of(case, "fits_huge"),
        "{name}: fits_in_recycle_bin(u64::MAX)"
    );

    let expected_throw = case["delete_permanently"]["throws"]
        .as_str()
        .unwrap_or_else(|| panic!("{name}: delete_permanently must name the throw"));
    assert_eq!(expected_throw, "NotSupportedException", "{name}: golden throw kind");
    let err = bin
        .delete_permanently(path)
        .err()
        .unwrap_or_else(|| panic!("{name}: delete_permanently must fail"));
    assert_eq!(
        err.kind(),
        io::ErrorKind::Unsupported,
        "{name}: delete_permanently kind"
    );

    let restored = bin.try_restore(path, 0, DateTimeUtc::now());
    assert_eq!(
        restored.expect("try_restore must not fail"),
        bool_of(case, "try_restore"),
        "{name}: try_restore"
    );
}

// C#: none (new golden test). Golden `bench-recycle-bin`, 18 cases.
#[test]
fn golden_bench_recycle_bin() {
    let golden = golden::load("bench-recycle-bin");
    for case in &golden.cases {
        match str_of(case, "kind") {
            "env" => check_env_case(case),
            "delete" => check_delete_case(case),
            "members" => check_members(case),
            other => panic!("{}: unknown case kind {other}", str_of(case, "name")),
        }
    }
}
