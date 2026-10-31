//! ST-APP-RACES M2: a panic inside the `FileHashService` worker thread must complete the flight with a failed hash.
//! Before the fix the worker died without storing a result, so a waiter with `CancelToken::none()` (the duplicate check)
//! waited for ever and the dead flight stayed in `in_flight`. Rust-only (C# `Task` faults propagate to the awaiting caller).
//!
//! Fakes only. The call runs on a helper thread and the wait is bounded by 30 s, so a regression fails instead of hanging.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use pr_app::misc::file_hash::{EnglishFileHashTexts, FileHashError, FileHashService, FileHasher};
use pr_core::abstractions::{FileSystem, ReadSeek, SourceReadPriority, SourceReader};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_test_support::FakeFileSystem;

const BOUND: Duration = Duration::from_secs(30);

/// Panics on the first open, reads the fake disk afterwards.
struct PanicOnceReader {
    disk: Arc<FakeFileSystem>,
    opens: AtomicUsize,
}

impl SourceReader for PanicOnceReader {
    fn open_source(&self, path: &Path, _p: SourceReadPriority, _buffer: usize) -> io::Result<Box<dyn ReadSeek>> {
        if self.opens.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("reader exploded");
        }
        let bytes = self
            .disk
            .file_content(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no fake file"))?;
        Ok(Box::new(io::Cursor::new(bytes)))
    }
}

fn get_bounded(service: &Arc<FileHashService>, path: &Path) -> Result<String, FileHashError> {
    let (tx, rx) = mpsc::channel();
    let service = Arc::clone(service);
    let path: PathBuf = path.to_path_buf();
    thread::spawn(move || {
        let _ = tx.send(service.get(&path, &CancelToken::none()));
    });
    rx.recv_timeout(BOUND)
        .expect("get never returned: the worker panic left the waiter hanging")
}

// New test (ST-APP-RACES M2).
#[test]
fn a_panic_in_the_hash_worker_fails_the_call_and_the_next_call_recomputes() {
    let path = PathBuf::from(r"C:\hash-panic\photo.bin");
    let disk = Arc::new(FakeFileSystem::new());
    let stamp = DateTimeUtc::from_system_time(UNIX_EPOCH + Duration::from_secs(1_767_323_045)).unwrap();
    disk.add_file_at(&path, &[7u8; 4096], stamp);
    let service = Arc::new(
        FileHashService::with_seams(
            None,
            Arc::clone(&disk) as Arc<dyn FileSystem>,
            Arc::new(PanicOnceReader {
                disk: Arc::clone(&disk),
                opens: AtomicUsize::new(0),
            }),
            Arc::new(EnglishFileHashTexts),
        )
        .unwrap(),
    );

    let first = get_bounded(&service, &path);
    match first {
        Err(FileHashError::Io(error)) => assert!(error.to_string().contains("reader exploded"), "{error}"),
        other => panic!("expected an Io failure carrying the panic text, got {other:?}"),
    }
    // The dead flight is gone: a second call starts a fresh computation and succeeds.
    assert!(get_bounded(&service, &path).is_ok());
    assert_eq!(service.cache_count_for_tests(), 1);
}
