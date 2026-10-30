//! Port of PreviewImageService.ShutdownPersistWorkersAsync (C# baseline 7eaee2a2).
use std::sync::Arc;
use std::time::Duration;

use pr_imaging::service::{DecodeThreadKind, DecodeThreadSpawner, PreviewImageService};
use pr_test_support::{FakeClock, TempRoot};

mod support {
    pub mod preview_service;
}

#[derive(Default)]
struct QueuedSpawner(std::sync::Mutex<Vec<Box<dyn FnOnce() + Send + 'static>>>);
impl DecodeThreadSpawner for QueuedSpawner {
    fn spawn(&self, _: DecodeThreadKind, job: Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<()> {
        self.0.lock().unwrap().push(job);
        Ok(())
    }
}

#[test]
fn bounded_shutdown_observer_tracks_queued_then_completed_workers() {
    // Mutants service/disk.rs:299:50 (> -> == / < / >=), 281/293 (bool bodies), 304 (== -> !=).
    // C#: TryComplete precedes Task.WhenAll; captured workers remain live until explicitly executed.
    let root = TempRoot::new("w4c-queued-shutdown");
    let spawner = Arc::new(QueuedSpawner::default());
    let mut cfg =
        support::preview_service::config(&root.path().join("cache"), support::preview_service::FakeDecoder::new());
    cfg.disable_disk_cache_override = Some(false);
    cfg.clock = Some(Arc::new(FakeClock::new()));
    cfg.spawner = Some(spawner.clone());
    let service = PreviewImageService::new(cfg).unwrap();
    assert!(!service.shutdown_persist_workers_timeout(Duration::ZERO));
    let mut observations = Vec::new();
    assert!(
        !service.shutdown_persist_workers_timeout_observed_for_tests(Duration::ZERO, |live, pending| {
            assert_eq!((live, pending), (2, true));
            observations.push((live, pending));
        })
    );
    assert!(!observations.is_empty()); // Condvar may recheck after a spurious wake; each observation must agree.
    for job in std::mem::take(&mut *spawner.0.lock().unwrap()) {
        job();
    }
    assert!(service.shutdown_persist_workers_timeout(Duration::ZERO));
    let mut observations = Vec::new();
    assert!(
        service.shutdown_persist_workers_timeout_observed_for_tests(Duration::ZERO, |live, pending| {
            assert_eq!((live, pending), (0, false));
            observations.push((live, pending));
        })
    );
    assert_eq!(observations, [(0, false)]);
}

#[test]
fn decode_and_cache_reads_only_existing_entries_before_source_fallback() {
    // Mutant service/decode.rs:417:33: replace disk_entry_exists match guard with true.
    // C#: File.Exists precedes TryReadDiskEntry. Observe the read attempt before opening an absent file.
    use std::sync::atomic::{AtomicUsize, Ordering};
    for present in [false, true] {
        let root = TempRoot::new("w4c-disk-read-guard");
        let source = root.path().join("source.jpg");
        let fs = Arc::new(pr_test_support::FakeFileSystem::new());
        fs.add_file(&source, b"fake source");
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = reads.clone();
        let spawner = Arc::new(QueuedSpawner::default());
        let decoder = support::preview_service::FakeDecoder::new();
        let cache_dir = root.path().join("cache");
        let mut cfg = support::preview_service::config(&cache_dir, decoder.clone());
        cfg.disable_disk_cache_override = Some(false);
        cfg.file_system = Some(fs);
        cfg.clock = Some(Arc::new(FakeClock::new()));
        cfg.spawner = Some(spawner.clone());
        cfg.before_disk_read_for_tests = Some(Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }));
        let service = PreviewImageService::new(cfg).unwrap();
        let path = source.to_str().unwrap();
        let key = service.get_current_cache_key(path).unwrap();
        let entry = pr_imaging::service::disk::disk_cache_path(&cache_dir, &key);
        if present {
            std::fs::create_dir_all(&cache_dir).unwrap();
            std::fs::write(&entry, b"broken pv4 entry").unwrap();
        }
        assert_eq!(entry.is_file(), present);
        let handle = service.get_preview(path, &key);
        // Worker jobs were queued first; get_preview appends its job synchronously.
        let mut background = std::mem::take(&mut *spawner.0.lock().unwrap());
        background.pop().expect("preview job is last")();
        assert!(handle.is_ready());
        assert!(handle.wait_timeout(Duration::ZERO).unwrap().is_ok());
        assert_eq!(decoder.calls(), 1);
        assert_eq!(reads.load(Ordering::SeqCst), usize::from(present));
        let mut shutdown_observed = false;
        assert!(
            !service.shutdown_persist_workers_timeout_observed_for_tests(Duration::ZERO, |live, pending| {
                assert_eq!((live, pending), (2, true));
                shutdown_observed = true;
            })
        );
        assert!(
            shutdown_observed,
            "queue completion must be observed before executing workers"
        );
        for job in background {
            job();
        }
        assert!(service.shutdown_persist_workers_timeout(Duration::ZERO));
    }
}
