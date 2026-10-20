//! Red tests for the cache validity of `pr_app::misc::file_hash` (contract W8-04e, section "tests/file_hash_stale.rs").
//! Port of `tests/PhotoReview.App.Tests/Services/FileHashServiceStaleGapTests.cs` (C# baseline d92d6aae), the cache-validity
//! pin found by Stryker. Real files under a [`TempRoot`]; no sleep (the modified time is set explicitly).
//!
//! Every test calls the stub, so each one fails through `todo!()` until the porter fills the bodies.

use std::fs::{self, File};
use std::time::{Duration, UNIX_EPOCH};

use pr_app::misc::file_hash::{FileHashService, FileHasher};
use pr_core::sync::CancelToken;
use pr_test_support::TempRoot;

/// 2026-01-02 03:04:05 UTC as seconds since the Unix epoch (C# `new DateTime(2026, 1, 2, 3, 4, 5, DateTimeKind.Utc)`).
const STAMP_UNIX_SECONDS: u64 = 1_767_323_045;

fn set_modified(path: &std::path::Path, seconds_since_epoch: u64) {
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(seconds_since_epoch))
        .unwrap();
}

// C#: FileHashServiceStaleGapTests.GetAsync_FileReplacedByAnotherLengthWithTheSameTimestamp_IsRehashedNotServedFromTheCache
#[test]
fn get_rehashes_file_replaced_by_other_length_with_same_mtime() {
    let root = TempRoot::new("hash-stale");
    let path = root.join("a.bin");
    fs::write(&path, [0u8; 100]).unwrap();
    set_modified(&path, STAMP_UNIX_SECONDS);
    let service = FileHashService::new(None).unwrap();
    let first = service.get(&path, &CancelToken::none()).unwrap();

    // A copy tool that preserves the modified time: another length, the same timestamp.
    fs::write(&path, [0u8; 200]).unwrap();
    set_modified(&path, STAMP_UNIX_SECONDS);
    let second = service.get(&path, &CancelToken::none()).unwrap();

    assert_ne!(first, second);
}

// C#: FileHashServiceStaleGapTests.GetAsync_UnchangedFile_ReturnsTheSameHashTwice
#[test]
fn get_unchanged_file_returns_same_hash_twice() {
    let root = TempRoot::new("hash-unchanged");
    let path = root.join("b.bin");
    fs::write(&path, [0u8; 100]).unwrap();
    let service = FileHashService::new(None).unwrap();

    let first = service.get(&path, &CancelToken::none()).unwrap();
    let second = service.get(&path, &CancelToken::none()).unwrap();

    assert_eq!(first, second);
}
