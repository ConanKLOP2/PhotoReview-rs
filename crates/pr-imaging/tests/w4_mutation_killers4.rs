//! Port of PreviewImageService.ShutdownPersistWorkersAsync (C# baseline 7eaee2a2).
//! Completion receives are hang guards, never elapsed-time or negative-delay assertions.
use pr_test_support::TempRoot;
use std::sync::Arc;
mod support {
    pub mod preview_service;
}

type BackgroundJob = (
    pr_imaging::service::DecodeThreadKind,
    Box<dyn FnOnce() + Send + 'static>,
);
#[derive(Default)]
struct QueuedBackgroundSpawner(std::sync::Mutex<Vec<BackgroundJob>>);
impl pr_imaging::service::DecodeThreadSpawner for QueuedBackgroundSpawner {
    fn spawn(
        &self,
        kind: pr_imaging::service::DecodeThreadKind,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> std::io::Result<()> {
        self.0.lock().unwrap().push((kind, job));
        Ok(())
    }
}

#[test]
fn unbounded_persist_shutdown_completes_waiting_workers_and_is_idempotent() {
    // Mutants service/disk.rs:270:9 (shutdown body -> ()), 275:59 (> -> == / >=).
    // C#: ShutdownPersistWorkersAsync completes the channel and joins all workers, including a repeated call.
    // recv_timeout is only a hang guard for completion, never an elapsed-time or negative-delay assertion.
    use support::preview_service::{FakeDecoder, config};
    let shutdown_guard = std::time::Duration::from_secs(5);
    let root = TempRoot::new("cx-surv-unbounded-shutdown");
    let spawner = Arc::new(QueuedBackgroundSpawner::default());
    let mut cfg = config(&root.path().join("cache"), FakeDecoder::new());
    cfg.spawner = Some(spawner.clone());
    cfg.clock = Some(Arc::new(pr_test_support::FakeClock::new()));
    let service = Arc::new(pr_imaging::service::PreviewImageService::new(cfg).unwrap());
    let jobs = std::mem::take(&mut *spawner.0.lock().unwrap());
    assert_eq!(jobs.len(), 2);
    let (tx, rx) = std::sync::mpsc::channel();
    let shutdown_service = service.clone();
    let shutdown_tx = tx.clone();
    let shutdown = std::thread::spawn(move || {
        shutdown_service.shutdown_persist_workers();
        shutdown_tx.send(()).unwrap();
    });
    let workers: Vec<_> = jobs
        .into_iter()
        .map(|(kind, job)| {
            assert_eq!(kind, pr_imaging::service::DecodeThreadKind::PersistWorker);
            let worker_tx = tx.clone();
            std::thread::spawn(move || {
                job();
                worker_tx.send(()).unwrap();
            })
        })
        .collect();
    drop(tx);
    for _ in 0..3 {
        rx.recv_timeout(shutdown_guard)
            .expect("shutdown must complete every worker and return");
    }
    shutdown.join().unwrap();
    for worker in workers {
        worker.join().unwrap();
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let repeated = std::thread::spawn(move || {
        service.shutdown_persist_workers();
        tx.send(()).unwrap();
    });
    rx.recv_timeout(shutdown_guard)
        .expect("a second shutdown with zero live workers must return");
    repeated.join().unwrap();
}

#[test]
fn guarded_shutdown_reports_queued_workers_then_finished_workers_without_waiting() {
    // Mutants service/disk.rs:281 (return true/false), 288 (== -> !=).
    // C#: the bounded shutdown reports false until queued workers have actually exited.
    use support::preview_service::{FakeDecoder, config};
    let root = TempRoot::new("cx-surv-guarded-shutdown");
    let spawner = Arc::new(QueuedBackgroundSpawner::default());
    let mut cfg = config(&root.path().join("cache"), FakeDecoder::new());
    cfg.spawner = Some(spawner.clone());
    cfg.clock = Some(Arc::new(pr_test_support::FakeClock::new()));
    let service = pr_imaging::service::PreviewImageService::new(cfg).unwrap();
    assert!(!service.shutdown_persist_workers_timeout(std::time::Duration::ZERO));
    let jobs = std::mem::take(&mut *spawner.0.lock().unwrap());
    assert_eq!(jobs.len(), 2);
    for (kind, job) in jobs {
        assert_eq!(kind, pr_imaging::service::DecodeThreadKind::PersistWorker);
        job();
    }
    assert!(service.shutdown_persist_workers_timeout(std::time::Duration::ZERO));
}
