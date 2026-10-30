//! cl-misc killers for `pr_bench::slow_link_fs` mutants that the fake-clock suite cannot see because they sit on the REAL clock and on diagnostics.
//! Each test names the mutant (`slow_link_fs.rs:line`, operator). No window, no file system; the only real wait is one bounded 20 ms sleep.

use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pr_bench::slow_link_fs::{
    InvalidBandwidth, SharedBandwidthLimiter, SystemTimeSource, TICKS_PER_SECOND, ThrottledReadStream, TimeSource,
};

// slow_link_fs.rs:53:9 `now_ticks -> 0 / 1 / -1`, :53:56 `/ -> %` and `/ -> *`: after about 5 ms of spinning the 100 ns tick count must be at least
// 50 000 and far below the `* 100` blow-up (5 s is 50 000 000 ticks).
#[test]
fn system_time_source_counts_100_ns_ticks_since_creation() {
    let clock = SystemTimeSource::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(5) {
        std::hint::spin_loop();
    }
    let ticks = clock.now_ticks();
    let spent = start.elapsed();
    assert!(ticks >= 50_000, "ticks {ticks}");
    let ceiling = i64::try_from(spent.as_nanos() / 100).unwrap() + 5 * TICKS_PER_SECOND;
    assert!(ticks <= ceiling, "ticks {ticks} above {ceiling}");
    assert!(clock.now_ticks() >= ticks, "the clock never goes backwards");
}

// slow_link_fs.rs:57:9 `sleep -> ()`: the real sleep waits at least the requested time.
#[test]
fn system_time_source_sleep_waits_for_the_requested_time() {
    let clock = SystemTimeSource::default();
    let start = Instant::now();
    clock.sleep(Duration::from_millis(20));
    assert!(start.elapsed() >= Duration::from_millis(20));
}

// slow_link_fs.rs:69:9 `InvalidBandwidth::fmt -> Ok(Default)` is covered by the Display assertion in `limiter_rejects_non_positive_rate`; this pins it
// for a value that is not in that test.
#[test]
fn invalid_bandwidth_display_names_the_value() {
    let error = InvalidBandwidth { bytes_per_second: -7 };
    assert_eq!(error.to_string(), "bytesPerSecond must be greater than zero (was -7)");
}

// slow_link_fs.rs:95:9 and :188:9 `Debug::fmt -> Ok(Default)`: both types print their name.
#[test]
fn limiter_and_throttled_stream_debug_name_their_types() {
    let limiter = Arc::new(SharedBandwidthLimiter::new(1_000_000).unwrap());
    assert!(format!("{limiter:?}").contains("SharedBandwidthLimiter"));
    let stream = ThrottledReadStream::new(Box::new(Cursor::new(vec![1_u8, 2, 3])), Arc::clone(&limiter));
    assert!(format!("{stream:?}").contains("ThrottledReadStream"));
}

// slow_link_fs.rs:124:9 `bytes_per_second -> 0 / 1 / -1`.
#[test]
fn limiter_reports_its_rate() {
    assert_eq!(
        SharedBandwidthLimiter::new(123_456).unwrap().bytes_per_second(),
        123_456
    );
}

// slow_link_fs.rs:202:9 `read -> Ok(0) / Ok(1)`: the stream hands out the inner bytes (fake clock with a wide-open limiter).
#[test]
fn throttled_stream_returns_the_inner_bytes() {
    let clock = Arc::new(StepClock::default());
    let limiter = Arc::new(SharedBandwidthLimiter::with_time_source(i64::from(i32::MAX), clock).unwrap());
    let mut stream = ThrottledReadStream::new(Box::new(Cursor::new(vec![9_u8, 8, 7, 6])), limiter);
    let mut buffer = [0_u8; 3];
    assert_eq!(stream.read(&mut buffer).unwrap(), 3);
    assert_eq!(buffer, [9, 8, 7]);
    assert_eq!(stream.read(&mut buffer).unwrap(), 1);
    assert_eq!(buffer[0], 6);
    assert_eq!(stream.read(&mut buffer).unwrap(), 0);
}

/// Fake clock: `sleep` advances the ticks and records the request (no real wait). A mutated loop that never ends trips the call budget and panics, which
/// ends the helper thread instead of spinning until the mutation timeout.
#[derive(Default)]
struct StepClock {
    ticks: AtomicI64,
    calls: AtomicUsize,
    slept: Mutex<Vec<Duration>>,
}

impl StepClock {
    fn budget(&self) {
        assert!(
            self.calls.fetch_add(1, Ordering::SeqCst) < 100_000,
            "call budget exhausted: the loop does not end"
        );
    }

    fn advance(&self, by: Duration) {
        self.ticks
            .fetch_add(i64::try_from(by.as_nanos() / 100).unwrap(), Ordering::SeqCst);
    }

    fn total(&self) -> Duration {
        self.slept.lock().unwrap().iter().sum()
    }
}

impl TimeSource for StepClock {
    fn now_ticks(&self) -> i64 {
        self.budget();
        self.ticks.load(Ordering::SeqCst)
    }

    fn sleep(&self, duration: Duration) {
        self.budget();
        self.slept.lock().unwrap().push(duration);
        self.advance(duration);
    }
}

/// Runs `consume(bytes)` on a helper thread; `Err` when it panicked (call budget) or did not finish within 20 s.
fn consume_bounded(clock: &Arc<StepClock>, rate: i64, bytes: i32) -> Result<(), String> {
    let limiter = SharedBandwidthLimiter::with_time_source(rate, clock.clone()).unwrap();
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        limiter.consume(bytes);
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(20))
        .map_err(|e| e.to_string())
}

// slow_link_fs.rs:134:25 `remaining > 0 -> >=` (never ends), :139:23 `-= -> += / /=` (never ends / wrong chunks), :151:43 `available >= needed -> <` and
// :155:38 `- -> /` (wrong wait): a 250-byte request at 100 B/s is metered in three chunks (100, 100, 50) and waits 1 s + 1 s + 500 ms of fake time.
#[test]
fn consume_terminates_and_waits_the_exact_time_per_chunk() {
    let clock = Arc::new(StepClock::default());
    consume_bounded(&clock, 100, 250).expect("consume must return");
    assert_eq!(
        clock.total(),
        Duration::from_millis(2_500),
        "{:?}",
        clock.slept.lock().unwrap()
    );
    assert_eq!(clock.slept.lock().unwrap().first(), Some(&Duration::from_secs(1)));
}

// slow_link_fs.rs:155:38 `missing = needed - available -> needed + available`: with 50 of the 100 needed bytes already in the bucket the wait is 50 ms, not
// 150 ms (the bucket starts empty, so only a refilled bucket tells `-` from `+`).
#[test]
fn a_partly_filled_bucket_waits_only_for_the_missing_bytes() {
    let clock = Arc::new(StepClock::default());
    let limiter = SharedBandwidthLimiter::with_time_source(1_000, clock.clone()).unwrap();
    clock.advance(Duration::from_millis(50)); // 50 bytes accrue
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        limiter.consume(100);
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(20))
        .expect("consume must return");
    assert_eq!(clock.total(), Duration::from_millis(50));
}

// slow_link_fs.rs:203:17 `read > 0 -> == / <` (reads are not metered); `-> >=` is equivalent: `consume(0)` returns at once (`bytes <= 0`), so metering an
// empty read changes nothing and no test can see it.
#[test]
fn throttled_stream_meters_the_bytes_it_reads() {
    let clock = Arc::new(StepClock::default());
    let limiter = Arc::new(SharedBandwidthLimiter::with_time_source(100, clock.clone()).unwrap());
    let mut stream = ThrottledReadStream::new(Box::new(Cursor::new(vec![0_u8; 80])), limiter);
    let mut buffer = [0_u8; 50];
    assert_eq!(stream.read(&mut buffer).unwrap(), 50);
    assert_eq!(clock.total(), Duration::from_millis(500), "50 bytes at 100 B/s");
    assert_eq!(stream.read(&mut buffer).unwrap(), 30);
    assert_eq!(clock.total(), Duration::from_millis(800));
    assert_eq!(stream.read(&mut buffer).unwrap(), 0);
    assert_eq!(clock.total(), Duration::from_millis(800), "end of stream costs nothing");
}
