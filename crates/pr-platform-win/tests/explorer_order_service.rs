#[path = "../src/explorer_order_service.rs"]
mod explorer_order_service;

use std::error::Error;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use explorer_order_service::{
    ExplorerOrderService, ExplorerQuery, MonotonicClock, QueryError, normalize_timeout, resolve_cancellation,
};
use pr_core::abstractions::{ExplorerOrderProvider, Log};
use pr_core::catalog::explorer_types::{
    ExplorerGroupState, ExplorerOrderStatus, ExplorerQueryProgress, ExplorerViewSnapshot,
};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;

const HANG_GUARD: Duration = Duration::from_secs(5);
const LONG: Duration = Duration::from_secs(30);

#[derive(Default)]
struct MemoryLog(Mutex<Vec<String>>);
impl Log for MemoryLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, message: &str) {
        self.0.lock().unwrap().push(format!("I:{message}"));
    }
    fn warn(&self, message: &str) {
        self.0.lock().unwrap().push(format!("W:{message}"));
    }
    fn error(&self, message: &str, _error: Option<&(dyn Error + 'static)>) {
        self.0.lock().unwrap().push(format!("E:{message}"));
    }
}

#[derive(Default)]
struct ManualClock(AtomicUsize);
impl ManualClock {
    fn advance(&self, duration: Duration) {
        self.0.fetch_add(duration.as_millis() as usize, Ordering::SeqCst);
    }
}
impl MonotonicClock for ManualClock {
    fn timestamp(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst) as u64)
    }
}

fn available(folder: &str, path: &str) -> ExplorerViewSnapshot {
    ExplorerViewSnapshot {
        folder: folder.into(),
        ordered_paths: vec![path.into()],
        sort_columns: vec![],
        group_state: ExplorerGroupState::None,
        status: ExplorerOrderStatus::Available,
        reason: None,
        captured_utc: DateTimeUtc::MIN,
    }
}
fn status(folder: &str, status: ExplorerOrderStatus, reason: &str) -> ExplorerViewSnapshot {
    ExplorerViewSnapshot {
        folder: folder.into(),
        ordered_paths: vec![],
        sort_columns: vec![],
        group_state: ExplorerGroupState::Unknown,
        status,
        reason: Some(reason.into()),
        captured_utc: DateTimeUtc::MIN,
    }
}
fn path(name: &str) -> String {
    format!(r"C:\photos\{name}")
}
fn folder(name: &str) -> String {
    format!(r"C:\photos\{name}")
}
fn provider<F>(query: F) -> ExplorerOrderService
where
    F: for<'a> Fn(
            &str,
            Option<&'a (dyn Fn(ExplorerQueryProgress) + Sync)>,
            usize,
            &CancelToken,
        ) -> Result<ExplorerViewSnapshot, QueryError>
        + Send
        + 'static,
{
    let query: Box<ExplorerQuery> = Box::new(query);
    ExplorerOrderService::with_query(query, Some(Arc::new(MemoryLog::default()))).unwrap()
}
fn provider_with<F>(
    query: F,
    log: Arc<MemoryLog>,
    clock: Arc<dyn MonotonicClock>,
    age: Duration,
) -> ExplorerOrderService
where
    F: for<'a> Fn(
            &str,
            Option<&'a (dyn Fn(ExplorerQueryProgress) + Sync)>,
            usize,
            &CancelToken,
        ) -> Result<ExplorerViewSnapshot, QueryError>
        + Send
        + 'static,
{
    let query: Box<ExplorerQuery> = Box::new(query);
    ExplorerOrderService::with_seams(query, Some(log), clock, age).unwrap()
}
fn get(service: &ExplorerOrderService, folder: &str, timeout: Duration, cancel: &CancelToken) -> ExplorerViewSnapshot {
    service.try_get_snapshot_progressive(Path::new(folder), timeout, None, 16, cancel)
}

#[cfg(windows)]
fn current_thread_apartment() -> (i32, i32, i32) {
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoGetApartmentType(apartment: *mut i32, qualifier: *mut i32) -> i32;
    }
    let mut apartment = -1;
    let mut qualifier = -1;
    // SAFETY: both output pointers refer to initialized writable integers for the duration of the COM query.
    let hr = unsafe { CoGetApartmentType(&mut apartment, &mut qualifier) };
    (hr, apartment, qualifier)
}

#[cfg(not(windows))]
fn current_thread_apartment() -> (i32, i32, i32) {
    (0, 0, 0)
}

#[test]
fn query_result_and_progress_arguments_are_passed_through() {
    // C#: ExplorerOrderServiceSeamTests.QueryResultAndProgressArgumentsArePassedThrough
    let service = provider(|folder, progress, batch_size, _| {
        assert_eq!(batch_size, 128);
        if let Some(progress) = progress {
            progress(ExplorerQueryProgress {
                items_read: 4,
                item_count: 9,
                com_calls: 2,
            });
        }
        Ok(available(folder, &path("a.jpg")))
    });
    let seen = Mutex::new(Vec::new());
    let progress = |value| seen.lock().unwrap().push(value);
    let result = service.try_get_snapshot_progressive(
        Path::new(&folder("one")),
        LONG,
        Some(&progress),
        999,
        &CancelToken::none(),
    );
    assert_eq!(result.status, ExplorerOrderStatus::Available);
    assert_eq!(result.ordered_paths, vec![path("a.jpg")]);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![ExplorerQueryProgress {
            items_read: 4,
            item_count: 9,
            com_calls: 2
        }]
    );
}

#[test]
fn timeout_releases_caller_cancels_work_and_frees_pump() {
    // C#: ExplorerOrderServiceSeamTests.TimeoutReleasesCallerCancelsWorkAndFreesPump
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (token_tx, token_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let query_calls = Arc::clone(&calls);
    let service = Arc::new(provider(move |folder, _, _, token| {
        if query_calls.fetch_add(1, Ordering::SeqCst) == 0 {
            token_tx.send(token.clone()).unwrap();
            started_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(HANG_GUARD);
        }
        Ok(available(folder, "done.jpg"))
    }));
    let caller_service = Arc::clone(&service);
    let (result_tx, result_rx) = mpsc::channel();
    thread::spawn(move || {
        result_tx
            .send(get(
                &caller_service,
                &folder("timeout"),
                Duration::from_millis(35),
                &CancelToken::none(),
            ))
            .unwrap();
    });
    started_rx.recv_timeout(HANG_GUARD).unwrap();
    let token = token_rx.recv_timeout(HANG_GUARD).unwrap();
    assert_eq!(
        result_rx.recv_timeout(HANG_GUARD).unwrap().status,
        ExplorerOrderStatus::TimedOut
    );
    assert!(token.is_canceled());
    release_tx.send(()).unwrap();
    assert_eq!(
        get(&service, &folder("again"), LONG, &CancelToken::none()).status,
        ExplorerOrderStatus::Available
    );
}

#[test]
fn timeout_is_timed_out_whether_the_query_throws_or_returns_canceled() {
    // C#: ExplorerOrderServiceSeamTests.TimeoutIsTimedOutWhetherTheQueryThrowsOrReturnsCanceled
    let canceled = status(&folder("x"), ExplorerOrderStatus::Canceled, "canceled");
    let timed = resolve_cancellation(canceled, false, false, true);
    assert_eq!(timed.status, ExplorerOrderStatus::TimedOut);
    assert_eq!(timed.reason.as_deref(), Some("timeout"));
    let provider = provider(|_, _, _, _| Err(QueryError::canceled("timeout cancellation")));
    let result = get(&provider, &folder("exception"), Duration::ZERO, &CancelToken::none());
    assert_eq!(result.status, ExplorerOrderStatus::TimedOut);
}

#[test]
fn caller_cancellation_beats_expired_timeout() {
    // C#: ExplorerOrderServiceSeamTests.CallerCancellationBeatsExpiredTimeout
    let canceled = status(&folder("x"), ExplorerOrderStatus::Canceled, "canceled");
    assert_eq!(
        resolve_cancellation(canceled, false, true, true).status,
        ExplorerOrderStatus::Canceled
    );
}

#[test]
fn resolve_cancellation_picks_the_right_status() {
    // C#: ExplorerOrderServiceSeamTests.ResolveCancellationPicksTheRightStatus
    let canceled = status(&folder("x"), ExplorerOrderStatus::Canceled, "canceled");
    assert_eq!(
        resolve_cancellation(canceled, false, false, true).status,
        ExplorerOrderStatus::TimedOut
    );
}

#[test]
fn resolve_cancellation_keeps_other_statuses() {
    // C#: ExplorerOrderServiceSeamTests.ResolveCancellationKeepsOtherStatuses
    let available = available(&folder("x"), "x.jpg");
    assert_eq!(resolve_cancellation(available.clone(), false, false, true), available);
    let failed = status(&folder("x"), ExplorerOrderStatus::Failed, "query-failed|IOException");
    assert_eq!(resolve_cancellation(failed.clone(), false, false, true), failed);
}

#[test]
fn negative_timeout_means_expired_not_exception() {
    // C#: ExplorerOrderServiceSeamTests.NegativeTimeoutMeansExpiredNotException
    // Rust's public timeout type is Duration, which cannot represent a negative value; zero is its expired equivalent.
    let service = provider(|folder, _, _, _| Ok(available(folder, "fast.jpg")));
    assert_eq!(
        get(&service, &folder("zero"), Duration::ZERO, &CancelToken::none()).status,
        ExplorerOrderStatus::TimedOut
    );
}

#[test]
fn cancellation_mid_query_returns_canceled() {
    // C#: ExplorerOrderServiceSeamTests.CancellationMidQueryReturnsCanceled
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (token_tx, token_rx) = mpsc::channel();
    let service = Arc::new(provider(move |folder, _, _, token| {
        token_tx.send(token.clone()).unwrap();
        started_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(HANG_GUARD);
        if token.is_canceled() {
            Ok(status(folder, ExplorerOrderStatus::Canceled, "canceled"))
        } else {
            Ok(available(folder, "x.jpg"))
        }
    }));
    let caller_service = Arc::clone(&service);
    let (done_tx, done_rx) = mpsc::channel();
    let caller = CancelToken::new();
    let caller_for_thread = caller.clone();
    thread::spawn(move || {
        done_tx
            .send(get(&caller_service, &folder("cancel"), LONG, &caller_for_thread))
            .unwrap();
    });
    started_rx.recv_timeout(HANG_GUARD).unwrap();
    let query_token = token_rx.recv_timeout(HANG_GUARD).unwrap();
    caller.cancel();
    assert_eq!(
        done_rx.recv_timeout(HANG_GUARD).unwrap().status,
        ExplorerOrderStatus::Canceled
    );
    assert!(query_token.is_canceled());
    release_tx.send(()).unwrap();
}

#[test]
fn already_cancelled_token_never_starts_the_query() {
    // C#: ExplorerOrderServiceSeamTests.AlreadyCancelledTokenNeverStartsTheQuery
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let service = provider(move |folder, _, _, _| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(available(folder, "x.jpg"))
    });
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        get(&service, &folder("cancelled"), LONG, &token).status,
        ExplorerOrderStatus::Canceled
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn query_that_throws_is_reported_as_failed_and_the_pump_survives() {
    // C#: ExplorerOrderServiceSeamTests.QueryThatThrowsIsReportedAsFailedAndThePumpSurvives
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let log = Arc::new(MemoryLog::default());
    let query_log = Arc::clone(&log);
    let service = provider_with(
        move |folder, _, _, _| {
            if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(QueryError::new("IOException", "boom"))
            } else {
                Ok(available(folder, "a.jpg"))
            }
        },
        query_log,
        Arc::new(explorer_order_service::SystemMonotonicClock::default()),
        Duration::from_secs(30),
    );
    let failed = get(&service, &folder("f"), LONG, &CancelToken::none());
    let recovered = get(&service, &folder("f"), LONG, &CancelToken::none());
    assert_eq!(failed.status, ExplorerOrderStatus::Failed);
    assert_eq!(failed.reason.as_deref(), Some("query-failed|IOException"));
    assert!(
        log.0
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains("native view query failed"))
    );
    assert_eq!(recovered.status, ExplorerOrderStatus::Available);
}

#[test]
fn query_that_cancels_itself_without_an_external_cancel_is_canceled() {
    // C#: ExplorerOrderServiceSeamTests.QueryThatCancelsItselfWithoutAnExternalCancelIsCanceled
    let service = provider(|_, _, _, _| Err(QueryError::canceled("self canceled")));
    assert_eq!(
        get(&service, &folder("self-cancel"), LONG, &CancelToken::none()).status,
        ExplorerOrderStatus::Canceled
    );
}

#[test]
fn queries_are_serialized_on_one_sta_thread() {
    // C#: ExplorerOrderServiceSeamTests.QueriesAreSerializedOnOneStaThread
    let threads = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&threads);
    let sta = Arc::new(Mutex::new(Vec::new()));
    let observed_sta = Arc::clone(&sta);
    let service = Arc::new(provider(move |folder, _, _, _| {
        observed.lock().unwrap().push(thread::current().id());
        observed_sta.lock().unwrap().push(current_thread_apartment());
        Ok(available(folder, "a.jpg"))
    }));
    let workers: Vec<_> = (0..24)
        .map(|index| {
            let service = Arc::clone(&service);
            thread::spawn(move || get(&service, &folder(&format!("{index}")), LONG, &CancelToken::none()))
        })
        .collect();
    for worker in workers {
        assert_eq!(worker.join().unwrap().status, ExplorerOrderStatus::Available);
    }
    let ids = threads.lock().unwrap();
    assert_eq!(ids.len(), 24);
    assert!(ids.iter().all(|id| *id == ids[0]));
    assert!(
        sta.lock()
            .unwrap()
            .iter()
            .all(|(hr, apartment, _)| *hr >= 0 && matches!(*apartment, 0 | 3)),
        "{sta:?}"
    );
}

#[test]
fn prefetch_is_joined_once_regardless_of_path_spelling() {
    // C#: ExplorerOrderServiceSeamTests.PrefetchIsJoinedOnceRegardlessOfPathSpelling
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let service = provider(move |folder, _, _, _| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(available(folder, "a.jpg"))
    });
    service.prefetch(Path::new(&folder("P")), LONG);
    assert_eq!(
        get(&service, &format!("{}\\", folder("p")), LONG, &CancelToken::none()).status,
        ExplorerOrderStatus::Available
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        get(&service, &folder("p"), LONG, &CancelToken::none()).status,
        ExplorerOrderStatus::Available
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn prefetch_for_other_folder_is_cancelled() {
    // C#: ExplorerOrderServiceSeamTests.PrefetchForOtherFolderIsCancelled
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (token_tx, token_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let service = provider(move |folder, _, _, token| {
        if observed.fetch_add(1, Ordering::SeqCst) == 0 {
            token_tx.send(token.clone()).unwrap();
            started_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(HANG_GUARD);
            return if token.is_canceled() {
                Ok(status(folder, ExplorerOrderStatus::Canceled, "canceled"))
            } else {
                Ok(available(folder, "wrong.jpg"))
            };
        }
        Ok(available(folder, "b.jpg"))
    });
    service.prefetch(Path::new(&folder("A")), LONG);
    started_rx.recv_timeout(HANG_GUARD).unwrap();
    let first_token = token_rx.recv_timeout(HANG_GUARD).unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let service = Arc::new(service);
    let request_service = Arc::clone(&service);
    thread::spawn(move || {
        done_tx
            .send(get(&request_service, &folder("B"), LONG, &CancelToken::none()))
            .unwrap()
    });
    release_tx.send(()).unwrap();
    assert_eq!(
        done_rx.recv_timeout(HANG_GUARD).unwrap().status,
        ExplorerOrderStatus::Available
    );
    assert!(first_token.is_canceled());
}

#[test]
fn new_prefetch_supersedes_and_cancels_the_previous_one() {
    // C#: ExplorerOrderServiceSeamTests.NewPrefetchSupersedesAndCancelsThePreviousOne
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (token_tx, token_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let service = provider(move |folder, _, _, token| {
        if observed.fetch_add(1, Ordering::SeqCst) == 0 {
            token_tx.send(token.clone()).unwrap();
            started_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(HANG_GUARD);
            return Ok(status(folder, ExplorerOrderStatus::Canceled, "canceled"));
        }
        Ok(available(folder, "new.jpg"))
    });
    service.prefetch(Path::new(&folder("pf1")), LONG);
    started_rx.recv_timeout(HANG_GUARD).unwrap();
    let token = token_rx.recv_timeout(HANG_GUARD).unwrap();
    service.prefetch(Path::new(&folder("pf2")), LONG);
    assert!(token.is_canceled());
    release_tx.send(()).unwrap();
    assert_eq!(
        get(&service, &folder("pf2"), LONG, &CancelToken::none()).status,
        ExplorerOrderStatus::Available
    );
}

#[test]
fn stale_prefetch_is_not_joined() {
    // C#: ExplorerOrderServiceSeamTests.StalePrefetchIsNotJoined
    let clock = Arc::new(ManualClock::default());
    let query_clock = Arc::clone(&clock);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let service = provider_with(
        move |folder, _, _, _| {
            let n = observed.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                query_clock.advance(Duration::from_secs(31));
            }
            Ok(available(folder, if n == 0 { "old.jpg" } else { "new.jpg" }))
        },
        Arc::new(MemoryLog::default()),
        clock,
        Duration::from_secs(30),
    );
    service.prefetch(Path::new(&folder("stale")), LONG);
    assert!(service.wait_prefetch_cleanup(HANG_GUARD));
    let result = get(&service, &folder("stale"), LONG, &CancelToken::none());
    assert_eq!(result.ordered_paths, vec!["new.jpg"]);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn fresh_prefetch_is_joined() {
    // C#: ExplorerOrderServiceSeamTests.FreshPrefetchIsJoined
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let clock = Arc::new(ManualClock::default());
    let service = provider_with(
        move |folder, _, _, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(available(folder, "fresh.jpg"))
        },
        Arc::new(MemoryLog::default()),
        clock.clone(),
        Duration::from_secs(30),
    );
    service.prefetch(Path::new(&folder("fresh")), LONG);
    clock.advance(Duration::from_secs(29));
    assert_eq!(
        get(&service, &folder("fresh"), LONG, &CancelToken::none()).status,
        ExplorerOrderStatus::Available
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn joined_prefetch_honours_caller_timeout() {
    // C#: ExplorerOrderServiceSeamTests.JoinedPrefetchHonoursCallerTimeout
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (token_tx, token_rx) = mpsc::channel();
    let service = provider(move |folder, _, _, token| {
        token_tx.send(token.clone()).unwrap();
        started_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(HANG_GUARD);
        Ok(status(folder, ExplorerOrderStatus::Canceled, "canceled"))
    });
    service.prefetch(Path::new(&folder("slow")), LONG);
    started_rx.recv_timeout(HANG_GUARD).unwrap();
    let token = token_rx.recv_timeout(HANG_GUARD).unwrap();
    let result = get(&service, &folder("slow"), Duration::ZERO, &CancelToken::none());
    assert_eq!(result.status, ExplorerOrderStatus::TimedOut);
    assert!(token.is_canceled());
    release_tx.send(()).unwrap();
}

#[test]
fn joined_prefetch_honours_caller_cancellation() {
    // C#: ExplorerOrderServiceSeamTests.JoinedPrefetchHonoursCallerCancellation
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (token_tx, token_rx) = mpsc::channel();
    let service = provider(move |folder, _, _, token| {
        token_tx.send(token.clone()).unwrap();
        started_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(HANG_GUARD);
        Ok(status(folder, ExplorerOrderStatus::Canceled, "canceled"))
    });
    service.prefetch(Path::new(&folder("jc")), LONG);
    started_rx.recv_timeout(HANG_GUARD).unwrap();
    let token = token_rx.recv_timeout(HANG_GUARD).unwrap();
    let caller = CancelToken::new();
    caller.cancel();
    let result = get(&service, &folder("jc"), LONG, &caller);
    assert_eq!(result.status, ExplorerOrderStatus::Canceled);
    assert!(token.is_canceled());
    release_tx.send(()).unwrap();
}

#[test]
fn invalid_folder_is_reported_not_thrown() {
    // C#: ExplorerOrderServiceSeamTests.InvalidFolderIsReportedNotThrown
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let service = provider(move |folder, _, _, _| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(available(folder, "x.jpg"))
    });
    for invalid in ["", "   ", "C:\\photos\\a\0b"] {
        let result = get(&service, invalid, LONG, &CancelToken::none());
        assert_eq!(result.status, ExplorerOrderStatus::Failed);
        assert_eq!(result.reason.as_deref(), Some("query-failed|ArgumentException"));
        service.prefetch(Path::new(invalid), LONG);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn dispose_is_idempotent_and_later_use_is_cancelled_not_thrown() {
    // C#: ExplorerOrderServiceSeamTests.DisposeIsIdempotentAndLaterUseIsCancelledNotThrown
    let service = provider(|folder, _, _, _| Ok(available(folder, "x.jpg")));
    service.dispose();
    service.dispose();
    service.prefetch(Path::new(&folder("late")), LONG);
    assert_eq!(
        get(&service, &folder("late"), LONG, &CancelToken::none()).status,
        ExplorerOrderStatus::Canceled
    );
}

#[test]
fn dispose_cancels_prefetch_and_does_not_hang_on_a_stuck_query() {
    // C#: ExplorerOrderServiceSeamTests.DisposeCancelsPrefetchAndDoesNotHangOnAStuckQuery
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let log = Arc::new(MemoryLog::default());
    let query_log = Arc::clone(&log);
    let query: Box<ExplorerQuery> = Box::new(move |folder, _, _, _| {
        started_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(HANG_GUARD);
        Ok(available(folder, "x.jpg"))
    });
    let service = Arc::new(ExplorerOrderService::with_query(query, Some(query_log)).unwrap());
    service.prefetch(Path::new(&folder("stuck")), LONG);
    started_rx.recv_timeout(HANG_GUARD).unwrap();
    let disposing = Arc::clone(&service);
    let (done_tx, done_rx) = mpsc::channel();
    thread::spawn(move || {
        disposing.dispose();
        done_tx.send(()).unwrap();
    });
    done_rx
        .recv_timeout(HANG_GUARD)
        .expect("dispose exceeded the external hang guard");
    assert!(
        log.0
            .lock()
            .unwrap()
            .iter()
            .any(|entry| entry.contains("did not exit within 200 ms"))
    );
    release_tx.send(()).unwrap();
}

#[test]
fn concurrent_prefetch_and_requests_never_throw_or_leave_the_service_stuck() {
    // C#: ExplorerOrderServiceSeamTests.ConcurrentPrefetchAndRequestsNeverThrowOrLeaveTheServiceStuck
    let service = Arc::new(provider(|folder, _, _, token| {
        if token.is_canceled() {
            Ok(status(folder, ExplorerOrderStatus::Canceled, "canceled"))
        } else {
            Ok(available(folder, "a.jpg"))
        }
    }));
    let workers: Vec<_> = (0..30)
        .map(|index| {
            let service = Arc::clone(&service);
            thread::spawn(move || {
                service.prefetch(Path::new(&folder(&format!("r{}", index % 3))), LONG);
                let result = get(
                    &service,
                    &folder(&format!("r{}", index % 5)),
                    LONG,
                    &CancelToken::none(),
                );
                assert!(matches!(
                    result.status,
                    ExplorerOrderStatus::Available | ExplorerOrderStatus::Canceled
                ));
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(
        get(&service, &folder("done"), LONG, &CancelToken::none()).status,
        ExplorerOrderStatus::Available
    );
}

#[test]
fn prefetch_racing_dispose_stores_nothing() {
    // C#: ExplorerOrderServiceSeamTests.PrefetchRacingDispose_StoresNothing
    let service = Arc::new(provider(|folder, _, _, token| {
        if token.is_canceled() {
            Ok(status(folder, ExplorerOrderStatus::Canceled, "canceled"))
        } else {
            Ok(available(folder, "a.jpg"))
        }
    }));
    let disposing = Arc::clone(&service);
    service.set_before_prefetch_store_hook(Some(Arc::new(move || disposing.dispose())));
    service.prefetch(Path::new(&folder("race")), LONG);
    assert!(!service.wait_prefetch_cleanup(Duration::from_millis(1)));
    assert_eq!(
        get(&service, &folder("race"), LONG, &CancelToken::none()).status,
        ExplorerOrderStatus::Canceled
    );
}

#[test]
fn finished_prefetch_releases_token_source_and_later_cancel_is_safe() {
    // C#: ExplorerOrderServiceSeamTests.FinishedPrefetch_ReleasesTokenSource_AndLaterCancelIsSafe
    // CancelToken owns only an Arc<AtomicBool>, so query completion needs no native token-source disposal.
    let service = provider(|folder, _, _, _| Ok(available(folder, "a.jpg")));
    service.prefetch(Path::new(&folder("finished")), LONG);
    assert!(service.wait_prefetch_cleanup(HANG_GUARD));
    assert_eq!(
        get(&service, &folder("other"), LONG, &CancelToken::none()).status,
        ExplorerOrderStatus::Available
    );
}

#[test]
fn shell_class_not_registered_is_reported_as_shell_unavailable() {
    // C#: ShellComWp33Tests.Service_ShellClassNotRegistered_IsReportedAsShellUnavailable
    let service = provider(|_, _, _, _| Err(QueryError::shell_open(0x8004_0154_u32 as i32)));
    let result = get(&service, &folder("no-shell"), LONG, &CancelToken::none());
    assert_eq!(result.status, ExplorerOrderStatus::NativeViewUnavailable);
    assert_eq!(result.reason.as_deref(), Some("shell-unavailable"));
}

#[test]
fn other_com_failure_opening_the_shell_is_failed_not_unavailable() {
    // C#: ShellComWp33Tests.Service_OtherComFailureOpeningTheShell_IsFailedNotUnavailable
    let service = provider(|_, _, _, _| Err(QueryError::shell_open(0x8000_4005_u32 as i32)));
    let result = get(&service, &folder("shell-failure"), LONG, &CancelToken::none());
    assert_eq!(result.status, ExplorerOrderStatus::Failed);
    assert_eq!(result.reason.as_deref(), Some("query-failed|COMException"));
}

#[test]
fn timeout_normalization_keeps_zero_and_large_values_are_infinite() {
    assert_eq!(normalize_timeout(Duration::ZERO), Some(Duration::ZERO));
    assert_eq!(normalize_timeout(Duration::from_millis(u32::MAX as u64)), None);
}
