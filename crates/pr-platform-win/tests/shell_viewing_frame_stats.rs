//! Red tests (W8-16a, test porter) for `pr_platform_win::shell_viewing::frame_stats`: the port of
//! `PhotoReview.Shell.Win32/Viewing/FrameStats.cs` (baseline `d92d6aae`).
//!
//! Ported from `tests/PhotoReview.Shell.Tests/Viewing/FrameStatsTests.cs` (6 tests, same names in snake_case, each with a
//! `// C#:` comment). Extra tests (`// new:`) pin contract traps T10 and the total-beyond-capacity rule.
//! Golden drivers over `tests/golden/frame-stats.json` live in `shell_viewing_golden.rs`.
//!
//! The C# tests convert with `Stopwatch.Frequency`; the Rust tests use the fixed frequency `TPS` (the same value the golden
//! records as `ticks_per_second`), so the expectations do not depend on the machine.
//!
//! Every test here fails through `todo!()` until the bodies land.

use pr_platform_win::shell_viewing::frame_stats::{BLOCKED_THRESHOLD_MS, CAPACITY, FrameStats, SLOW_CAPACITY};

/// Ticks per second used by every test in this file (the golden's `ticks_per_second`).
const TPS: i64 = 10_000_000;

/// C# `Ms(double ms) => (long)(ms * Stopwatch.Frequency / 1000.0)` with the fixed frequency.
fn ticks(ms: f64) -> i64 {
    (ms * TPS as f64 / 1000.0) as i64
}

/// xunit `Assert.Equal(expected, actual, digits)`: both rounded to `digits` decimals.
fn assert_close(actual: f64, expected: f64, digits: i32) {
    let scale = 10f64.powi(digits);
    let a = (actual * scale).round() / scale;
    let e = (expected * scale).round() / scale;
    assert!(a == e, "actual {actual} != expected {expected} at {digits} digits");
}

// C#: FrameStatsTests.Percentiles_AreNearestRankOverTheRecordedFrames
#[test]
fn percentiles_are_nearest_rank() {
    let mut stats = FrameStats::new(TPS);
    for i in 1..=100_i32 {
        let i = f64::from(i);
        stats.record(ticks(i), ticks(i / 2.0));
    }

    assert_eq!(stats.count(), 100);
    assert_close(stats.frame_ms(50.0), 50.0, 3);
    assert_close(stats.frame_ms(95.0), 95.0, 3);
    assert_close(stats.frame_ms(100.0), 100.0, 3);
    assert_close(stats.present_ms(50.0), 25.0, 3);
    assert_close(stats.max_present_ms(), 50.0, 3);
}

// C#: FrameStatsTests.BlockedPresents_CountsFramesAtOrOverTheThreshold
#[test]
fn blocked_presents_count_at_or_over_threshold() {
    let mut stats = FrameStats::new(TPS);

    stats.record(ticks(3.0), ticks(BLOCKED_THRESHOLD_MS - 0.5));
    stats.record(ticks(30.0), ticks(BLOCKED_THRESHOLD_MS));
    stats.record(ticks(300.0), ticks(250.0));

    assert_eq!(stats.blocked_presents(), 2);
    assert_eq!(stats.total(), 3);
}

// C#: FrameStatsTests.Ring_KeepsOnlyTheLastCapacityFrames_ButCountsAll
#[test]
fn ring_keeps_last_capacity_frames_but_counts_all() {
    let mut stats = FrameStats::new(TPS);
    for i in 0..CAPACITY + 10 {
        stats.record(ticks(if i < 10 { 1000.0 } else { 1.0 }), ticks(0.1));
    }

    assert_eq!(stats.count(), CAPACITY);
    assert_eq!(stats.total(), (CAPACITY + 10) as i64);
    assert_close(stats.frame_ms(100.0), 1.0, 3); // the 10 first frames of 1000 ms were overwritten
}

// C#: FrameStatsTests.Empty_ReportsZeros_AndResetClearsEverything
#[test]
fn empty_reports_zeros_and_reset_clears_counters() {
    let mut stats = FrameStats::new(TPS);
    assert_eq!(stats.frame_ms(95.0), 0.0);
    assert_eq!(stats.max_present_ms(), 0.0);

    stats.record(ticks(5.0), ticks(5.0));
    stats.reset();

    assert_eq!(
        (stats.count(), stats.total(), stats.blocked_presents()),
        (0_usize, 0_i64, 0_i64)
    );
}

// C#: FrameStatsTests.IsSlow_IsTrueWhenEitherEndDrawOrPresentReachesTheThreshold (Theory, 4 rows)
#[test]
fn is_slow_when_either_time_reaches_threshold() {
    let rows: [(f64, f64, bool); 4] = [(0.0, 0.0, false), (7.9, 7.9, false), (8.0, 0.0, true), (0.0, 8.0, true)];
    for (end_draw_ms, present_ms, expected) in rows {
        assert_eq!(
            FrameStats::is_slow(end_draw_ms, present_ms),
            expected,
            "IsSlow({end_draw_ms}, {present_ms})"
        );
    }
}

// C#: FrameStatsTests.Slow_KeepsOnlyBlockedFramesAndIsBounded
#[test]
fn slow_keeps_only_blocked_frames_and_is_bounded() {
    let mut stats = FrameStats::new(TPS);

    stats.record_slow(5.0, 1.0, 1.0, 1.0, "fast");
    stats.record_slow(500.0, 1.0, 480.0, 1.0, "endDraw blocked");
    stats.record_slow(500.0, 1.0, 1.0, 480.0, "present blocked");
    for _ in 0..100 {
        stats.record_slow(500.0, 1.0, 480.0, 1.0, "many");
    }

    assert_eq!(stats.slow().len(), SLOW_CAPACITY);
    assert_eq!(stats.slow()[0].note, "endDraw blocked");
    assert_eq!(stats.slow()[1].note, "present blocked");
}

// new: contract trap T10. C# `Reset` empties the ring and the counters but leaves `Slow` alone.
#[test]
fn reset_keeps_the_slow_list() {
    let mut stats = FrameStats::new(TPS);
    for _ in 0..5 {
        stats.record(ticks(5.0), ticks(5.0));
    }
    stats.record_slow(500.0, 1.0, 480.0, 1.0, "kept");

    stats.reset();

    assert_eq!(stats.count(), 0);
    assert_eq!(stats.total(), 0);
    assert_eq!(stats.blocked_presents(), 0);
    assert_eq!(stats.slow().len(), 1);
    assert_eq!(stats.slow()[0].note, "kept");
}

// new: `Total` and `BlockedPresents` keep counting past `CAPACITY`; only `Count` is capped.
#[test]
fn record_counts_total_beyond_capacity() {
    let mut stats = FrameStats::new(TPS);
    let frames = CAPACITY + 5;
    for _ in 0..frames {
        stats.record(ticks(1.0), ticks(BLOCKED_THRESHOLD_MS));
    }

    assert_eq!(stats.count(), CAPACITY);
    assert_eq!(stats.total(), frames as i64);
    assert_eq!(stats.blocked_presents(), frames as i64);
}
