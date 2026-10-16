//! Adversarial verification tests for W4-06a (kept out of the commit).
use pr_imaging::navigation_pace::NavigationPace;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

fn pace() -> (NavigationPace, Arc<AtomicI64>) {
    let t = Arc::new(AtomicI64::new(1_000_000));
    let t2 = Arc::clone(&t);
    (
        NavigationPace::with_timestamp(Box::new(move || t2.load(Ordering::SeqCst))),
        t,
    )
}

/// C# TimeSpan.FromMilliseconds(double) truncates to ticks (oracle dotnet 10: 33.3*1.5 -> 499499 ticks).
#[test]
fn viewer_start_delay_truncates_to_ticks_like_timespan() {
    let (p, t) = pace();
    p.record(0);
    for i in 1..=3 {
        t.fetch_add(333_000, Ordering::SeqCst); // 33.3 ms
        p.record(i);
    }
    assert_eq!(p.get_viewer_start_delay(50.0), Duration::from_nanos(499_499 * 100));
}

#[test]
fn viewer_start_delay_truncates_0_7ms() {
    let (p, t) = pace();
    p.record(0);
    for i in 1..=3 {
        t.fetch_add(7_000, Ordering::SeqCst); // 0.7 ms
        p.record(i);
    }
    assert_eq!(p.get_viewer_start_delay(5.0), Duration::from_nanos(10_499 * 100));
}

/// C# `_samples++` is unchecked: after 2^31 steps it wraps negative and the burst reads as inactive.
#[test]
#[ignore = "slow: 2^31 records; run in release"]
fn samples_wrap_like_csharp() {
    let (p, t) = pace();
    p.record(0);
    let mut idx = 0;
    for _ in 0..=(i32::MAX as i64) {
        t.fetch_add(10_000, Ordering::SeqCst);
        idx ^= 1;
        p.record(idx);
    }
    // samples == 2^31 -> int.MinValue in C#: inactive
    assert_eq!(p.get_lead(100.0), 0);
}

#[test]
fn debug_names_the_type() {
    let (p, _t) = pace();
    assert!(format!("{p:?}").starts_with("NavigationPace"));
}
