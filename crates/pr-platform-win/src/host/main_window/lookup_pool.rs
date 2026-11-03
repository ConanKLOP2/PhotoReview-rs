//! The small pool of named I/O lanes of the prefetch cache lookups (BACKLOG G14 L3, review `main-window-input-overlay-viewing.md` L3, R0003: lanes are fixed
//! named threads, never one thread per request; owner decision P-74 option C, 2026-11-03).
//! `MainWindowPrefetchHost::start_lookup` used to spawn an OS thread per `navigated()` and a panic inside it was not caught; now every window submits a job
//! here. A panicking job is caught, logged as an error and counted, so the lane survives and the next lookup still runs.
//!
//! Lane count: `min(max(cpus / 2, 1), 4)` with `cpus = std::thread::available_parallelism()` ([`lane_count_for`]). Lookups are short cache reads, so a few
//! lanes are enough, and half the CPUs stay free for the decoders and the UI thread. Lookups of a rapid photo switch therefore run concurrently (up to
//! the lane count) instead of one after another. The workers are started once, on the first lookup, so no later navigation pays a thread start.
//! Not in C# (C# used the thread pool); the observable behaviour (a lookup result is posted to the UI thread at `Background` priority) is unchanged.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use pr_core::abstractions::Log;
use pr_imaging::pixels::PixelBuffer;

use crate::shell_viewing::prefetcher::LookupRequest;

/// Upper bound of the lane count.
pub const MAX_LANES: usize = 4;

/// `min(max(cpus / 2, 1), MAX_LANES)`.
pub fn lane_count_for(cpus: usize) -> usize {
    (cpus / 2).clamp(1, MAX_LANES)
}

/// The lane count of this machine ([`lane_count_for`] of `available_parallelism`, 1 CPU when it is unknown).
pub fn default_lane_count() -> usize {
    lane_count_for(std::thread::available_parallelism().map_or(1, usize::from))
}

type Work = Box<dyn FnOnce() + Send + 'static>;

struct Job {
    work: Work,
    log: Arc<dyn Log>,
}

/// `lanes` worker threads fed through one channel.
pub struct LookupPool {
    sender: Option<Mutex<Sender<Job>>>,
    workers: Vec<JoinHandle<()>>,
    panics: Arc<AtomicUsize>,
}

impl std::fmt::Debug for LookupPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LookupPool")
            .field("lanes", &self.workers.len())
            .finish_non_exhaustive()
    }
}

impl LookupPool {
    /// Starts `lanes` (at least 1) worker threads named `{name}-{n}`. On a failed thread start the already started workers are joined and the error returned.
    pub fn start(name: &str, lanes: usize) -> io::Result<Self> {
        let (sender, receiver) = channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        let panics = Arc::new(AtomicUsize::new(0));
        let mut pool = Self {
            sender: Some(Mutex::new(sender)),
            workers: Vec::new(),
            panics: Arc::clone(&panics),
        };
        for lane in 0..lanes.max(1) {
            let receiver = Arc::clone(&receiver);
            let panics = Arc::clone(&panics);
            // On `?` the Drop of `pool` closes the queue and joins the workers started so far.
            let handle = std::thread::Builder::new()
                .name(format!("{name}-{lane}"))
                .spawn(move || worker_loop(&receiver, &panics))?;
            pool.workers.push(handle);
        }
        Ok(pool)
    }

    /// Queues `work`; a panic inside it is caught, written to `log` as an error and counted. `false` when the pool is shut down.
    pub fn submit(&self, log: Arc<dyn Log>, work: Work) -> bool {
        let Some(sender) = &self.sender else {
            return false;
        };
        match sender.lock() {
            Ok(sender) => sender.send(Job { work, log }).is_ok(),
            Err(_) => false,
        }
    }

    /// How many jobs panicked (and were survived).
    pub fn panic_count(&self) -> usize {
        self.panics.load(Ordering::SeqCst)
    }

    /// Number of worker threads.
    pub fn lanes(&self) -> usize {
        self.workers.len()
    }

    /// The process-wide `prefetch-lookup` pool, started on first use. A failed start is NOT cached: the next call tries again.
    pub fn shared() -> io::Result<&'static LookupPool> {
        static POOL: OnceLock<LookupPool> = OnceLock::new();
        if let Some(pool) = POOL.get() {
            return Ok(pool);
        }
        let pool = LookupPool::start("prefetch-lookup", default_lane_count())?;
        Ok(POOL.get_or_init(|| pool))
    }
}

impl Drop for LookupPool {
    /// Closes the queue (queued jobs still run) and joins every worker.
    fn drop(&mut self) {
        self.sender = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop(receiver: &Mutex<Receiver<Job>>, panics: &AtomicUsize) {
    loop {
        // The guard is a temporary of this statement: it is released before the job runs, so the other lanes can take the next job at once.
        let next = match receiver.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => return,
        };
        let Ok(job) = next else {
            return; // the sender is gone: shutdown
        };
        if catch_unwind(AssertUnwindSafe(job.work)).is_err() {
            panics.fetch_add(1, Ordering::SeqCst);
            job.log
                .error("Prefetch lookup job panicked; the lane keeps running", None);
        }
    }
}

/// The body of one prefetch lookup, free of the window: `lookup` resolves one path from the cache (a key error is a quiet miss), `post` receives the
/// generation and the results in request order. `MainWindowPrefetchHost::start_lookup` passes the preview service and the UI-thread post; tests pass fakes.
pub(super) fn lookup_work<L, P>(request: LookupRequest, lookup: L, post: P) -> Work
where
    L: Fn(&str) -> Option<Arc<PixelBuffer>> + Send + 'static,
    P: FnOnce(u64, Vec<Option<Arc<PixelBuffer>>>) + Send + 'static,
{
    Box::new(move || {
        let found: Vec<Option<Arc<PixelBuffer>>> = request.paths.iter().map(|path| lookup(path)).collect();
        post(request.generation, found);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pr_core::abstractions::NullLog;
    use std::sync::mpsc;
    use std::sync::mpsc::RecvTimeoutError;
    use std::thread::ThreadId;
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(20); // hang guard only; a healthy run never waits

    fn null_log() -> Arc<dyn Log> {
        Arc::new(NullLog)
    }

    #[derive(Default)]
    struct RecordingLog(Mutex<Vec<String>>);
    impl Log for RecordingLog {
        fn enabled(&self) -> bool {
            true
        }
        fn info(&self, _message: &str) {}
        fn warn(&self, _message: &str) {}
        fn error(&self, message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {
            self.0.lock().unwrap().push(message.to_owned());
        }
    }

    #[test]
    fn lane_count_is_half_the_cpus_between_one_and_four() {
        assert_eq!(lane_count_for(0), 1);
        assert_eq!(lane_count_for(1), 1);
        assert_eq!(lane_count_for(2), 1);
        assert_eq!(lane_count_for(4), 2);
        assert_eq!(lane_count_for(7), 3);
        assert_eq!(lane_count_for(8), 4);
        assert_eq!(lane_count_for(64), 4);
        assert!((1..=MAX_LANES).contains(&default_lane_count()));
    }

    // Jobs that all wait for each other can only finish when `lanes` of them run at the same time (a serialized pool would time out).
    #[test]
    fn jobs_run_concurrently_up_to_the_lane_count() {
        let lanes = 3;
        let pool = LookupPool::start("test-pool", lanes).unwrap();
        assert_eq!(pool.lanes(), lanes);
        let (started_tx, started_rx) = mpsc::channel::<ThreadId>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Arc::new(Mutex::new(release_rx));
        for _ in 0..lanes {
            let started_tx = started_tx.clone();
            let release_rx = Arc::clone(&release_rx);
            assert!(pool.submit(
                null_log(),
                Box::new(move || {
                    started_tx.send(std::thread::current().id()).unwrap();
                    let _ = release_rx.lock().unwrap().recv_timeout(WAIT);
                })
            ));
        }
        let ids: std::collections::HashSet<ThreadId> = (0..lanes)
            .map(|_| {
                started_rx
                    .recv_timeout(WAIT)
                    .expect("all lanes started while the first jobs still run")
            })
            .collect();
        assert_eq!(ids.len(), lanes, "each job runs on its own lane");
        assert!(!ids.contains(&std::thread::current().id()));
        for _ in 0..lanes {
            release_tx.send(()).unwrap();
        }
    }

    #[test]
    fn more_jobs_than_lanes_all_run_on_at_most_the_lane_threads() {
        let pool = LookupPool::start("test-pool-many", 2).unwrap();
        let (tx, rx) = mpsc::channel::<ThreadId>();
        for _ in 0..200 {
            let tx = tx.clone();
            assert!(pool.submit(
                null_log(),
                Box::new(move || tx.send(std::thread::current().id()).unwrap())
            ));
        }
        let ids: std::collections::HashSet<ThreadId> = (0..200).map(|_| rx.recv_timeout(WAIT).unwrap()).collect();
        assert!(ids.len() <= 2, "no thread per request: {} threads", ids.len());
    }

    #[test]
    fn panicking_job_does_not_kill_a_lane_and_is_logged_and_counted() {
        let pool = LookupPool::start("test-pool-panic", 1).unwrap();
        let log = Arc::new(RecordingLog::default());
        assert!(pool.submit(log.clone(), Box::new(|| panic!("boom"))));
        let (tx, rx) = mpsc::channel::<u8>();
        assert!(pool.submit(null_log(), Box::new(move || tx.send(7).unwrap())));
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), 7, "the single lane survived the panic");
        assert_eq!(pool.panic_count(), 1);
        assert_eq!(log.0.lock().unwrap().len(), 1, "one error log line");
    }

    #[test]
    fn drop_runs_the_queued_jobs_and_joins_the_workers() {
        let done = Arc::new(AtomicUsize::new(0));
        {
            let pool = LookupPool::start("test-pool-drop", 2).unwrap();
            for _ in 0..50 {
                let done = Arc::clone(&done);
                assert!(pool.submit(
                    null_log(),
                    Box::new(move || {
                        done.fetch_add(1, Ordering::SeqCst);
                    })
                ));
            }
        } // drop joins: every queued job has run when the block ends
        assert_eq!(done.load(Ordering::SeqCst), 50);
    }

    #[test]
    fn submit_after_shutdown_is_false_and_zero_lanes_still_start_one() {
        let mut pool = LookupPool::start("test-pool-shut", 0).unwrap();
        assert_eq!(pool.lanes(), 1);
        pool.sender = None;
        assert!(!pool.submit(null_log(), Box::new(|| {})));
    }

    fn request(generation: u64, paths: &[&str]) -> LookupRequest {
        LookupRequest {
            generation,
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    // The window path (`start_lookup` builds its job with `lookup_work` and submits it to the shared pool): a panicking lookup (first navigation) is
    // survived and the next navigation's lookup still posts its result.
    #[test]
    fn panicking_lookup_does_not_break_the_next_navigation() {
        let pool = LookupPool::start("test-pool-nav", 1).unwrap();
        let log = Arc::new(RecordingLog::default());
        let (tx, rx) = mpsc::channel::<(u64, usize)>();
        let first_tx = tx.clone();
        let first = lookup_work(
            request(1, &["a", "boom"]),
            |path| if path == "boom" { panic!("decoder bug") } else { None },
            move |generation, found| first_tx.send((generation, found.len())).unwrap(),
        );
        assert!(pool.submit(log.clone(), first));
        let second = lookup_work(
            request(2, &["a", "b", "c"]),
            |_| None,
            move |generation, found| tx.send((generation, found.len())).unwrap(),
        );
        assert!(pool.submit(log.clone(), second));
        assert_eq!(
            rx.recv_timeout(WAIT).unwrap(),
            (2, 3),
            "only the second navigation posts; the first one was cut short by its panic"
        );
        assert_eq!(rx.recv_timeout(WAIT), Err(RecvTimeoutError::Disconnected));
        assert_eq!(pool.panic_count(), 1);
        assert_eq!(log.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn lookup_work_posts_the_results_in_request_order() {
        let (tx, rx) = mpsc::channel::<(u64, Vec<bool>)>();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen_in = Arc::clone(&seen);
        let work = lookup_work(
            request(9, &["x", "y"]),
            move |path| {
                seen_in.lock().unwrap().push(path.to_owned());
                None
            },
            move |generation, found| {
                tx.send((generation, found.iter().map(Option::is_some).collect()))
                    .unwrap();
            },
        );
        work();
        assert_eq!(rx.recv().unwrap(), (9, vec![false, false]));
        assert_eq!(*seen.lock().unwrap(), ["x", "y"]);
    }
}
