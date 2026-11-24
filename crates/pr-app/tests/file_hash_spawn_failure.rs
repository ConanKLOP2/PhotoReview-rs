//! A failed worker thread start in `FileHashService::get` must fail the call and leave no dead flight behind.
//! Before the fix the flight stayed registered with no result, so every later `get` with `CancelToken::none()` joined it and
//! waited for ever. Rust-only (C# `Task.Run` does not throw on thread exhaustion).
//!
//! Fakes only. The calls run on a helper thread and the wait is bounded, so a regression fails instead of hanging.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use pr_app::misc::file_hash::{EnglishFileHashTexts, FileHashError, FileHashService, FileHasher};
use pr_core::abstractions::{FileSystem, PhysicalSourceReader};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_test_support::FakeFileSystem;

const BOUND: Duration = Duration::from_secs(10);

fn get_bounded(service: &Arc<FileHashService>, path: &Path) -> Option<Result<String, FileHashError>> {
    let (tx, rx) = mpsc::channel();
    let service = Arc::clone(service);
    let path: PathBuf = path.to_path_buf();
    thread::spawn(move || {
        let _ = tx.send(service.get(&path, &CancelToken::none()));
    });
    rx.recv_timeout(BOUND).ok()
}

// New test (RF4 C3).
#[test]
fn a_failed_worker_spawn_fails_the_call_and_leaves_no_dead_flight() {
    let path = PathBuf::from(r"C:\hash-spawn\photo.bin");
    let disk = Arc::new(FakeFileSystem::new());
    let stamp = DateTimeUtc::from_system_time(UNIX_EPOCH + Duration::from_secs(1_767_323_045)).unwrap();
    disk.add_file_at(&path, &[7u8; 4096], stamp);
    let service = Arc::new(
        FileHashService::with_seams(
            None,
            Arc::clone(&disk) as Arc<dyn FileSystem>,
            Arc::new(PhysicalSourceReader),
            Arc::new(EnglishFileHashTexts),
        )
        .unwrap()
        .with_spawner_for_tests(Arc::new(|_job| Err(io::Error::other("cannot create thread")))),
    );

    match get_bounded(&service, &path) {
        Some(Err(FileHashError::Io(error))) => assert!(error.to_string().contains("cannot create thread"), "{error}"),
        other => panic!("expected an Io failure carrying the spawn error, got {other:?}"),
    }
    // The dead flight is gone: a second call fails the same way at once instead of joining it and hanging.
    match get_bounded(&service, &path) {
        Some(Err(FileHashError::Io(_))) => {}
        other => panic!("second call must fail fast with Io, got {other:?}"),
    }
    assert_eq!(service.cache_count_for_tests(), 0);
}
