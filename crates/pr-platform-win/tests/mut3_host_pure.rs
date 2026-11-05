//! MUT3-HOST (2026-11-06): pure killers for `host/message_loop.rs::wait_timeout_ms`, written as a differential against an independent
//! reference (quotient and remainder of an `i128` division, not the `(a + b - 1) / b` form of the production code) over a grid that contains every
//! boundary of the function: the INFINITE sentinel, `<= 0`, 1, the exact multiples of the frequency, one less and one more, the `0xFFFF_FFFE` cap
//! on both sides, `i64` extremes for the deadline and the frequency, and non-positive frequencies (clamped to 1).
#![cfg(windows)]

use pr_platform_win::host::message_loop::wait_timeout_ms;

const INFINITE: u32 = 0xFFFF_FFFF;

fn reference(until_deadline: i64, frequency: i64) -> u32 {
    if until_deadline == i64::MAX {
        return INFINITE;
    }
    if until_deadline <= 0 {
        return 0;
    }
    let f = i128::from(frequency.max(1));
    let n = i128::from(until_deadline) * 1000;
    let mut ms = n / f;
    if n % f != 0 {
        ms += 1;
    }
    if ms > i128::from(INFINITE - 1) {
        INFINITE - 1
    } else {
        ms as u32
    }
}

fn deadlines(frequency: i64) -> Vec<i64> {
    let f = frequency.max(1);
    let mut v = vec![
        i64::MIN,
        i64::MIN + 1,
        -1_000_000,
        -f,
        -2,
        -1,
        0,
        1,
        2,
        3,
        7,
        9,
        10,
        99,
        100,
        999,
        1000,
        1001,
        4_294_967_295,
    ];
    for m in [
        1_i64,
        2,
        3,
        5,
        10,
        16,
        17,
        100,
        999,
        1000,
        1001,
        60_000,
        4_294_967_294,
        4_294_967_295,
        4_294_967_296,
    ] {
        // Multiples of f / 1000 around the cap and around exact milliseconds.
        let step = (f / 1000).max(1);
        if let Some(base) = step.checked_mul(m) {
            for delta in [-2_i64, -1, 0, 1, 2] {
                if let Some(d) = base.checked_add(delta) {
                    v.push(d);
                }
            }
        }
        if let Some(base) = f.checked_mul(m) {
            for delta in [-1_i64, 0, 1] {
                if let Some(d) = base.checked_add(delta) {
                    v.push(d);
                }
            }
        }
    }
    // Around the cap: the deadline whose exact result is 0xFFFF_FFFD, 0xFFFF_FFFE, 0xFFFF_FFFF and one more, for this frequency.
    for target in [4_294_967_293_i128, 4_294_967_294, 4_294_967_295, 4_294_967_296] {
        let base = target * i128::from(f) / 1000;
        for delta in [-2_i128, -1, 0, 1, 2] {
            let d = base + delta;
            if let Ok(d) = i64::try_from(d) {
                v.push(d);
            }
        }
    }
    v.extend([
        i64::MAX - 2,
        i64::MAX - 1,
        i64::MAX,
        i64::MAX / 1000,
        i64::MAX / 1000 + 1,
        i64::MAX / 999,
    ]);
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn wait_timeout_ms_matches_the_independent_reference_on_the_boundary_grid() {
    let frequencies = [
        i64::MIN,
        -10_000_000,
        -1,
        0,
        1,
        2,
        3,
        7,
        999,
        1000,
        1001,
        10_000,
        3_579_545,
        10_000_000,
        10_000_001,
        2_000_000_000,
        4_294_967_295,
        i64::MAX / 1000,
        i64::MAX - 1,
        i64::MAX,
    ];
    let mut checked = 0_u32;
    for &frequency in &frequencies {
        for deadline in deadlines(frequency) {
            assert_eq!(
                wait_timeout_ms(deadline, frequency),
                reference(deadline, frequency),
                "deadline {deadline}, frequency {frequency}"
            );
            checked += 1;
        }
    }
    assert!(checked > 1000, "grid too small: {checked}");
}

#[test]
fn wait_timeout_ms_named_cases() {
    // Sentinel and non-positive deadlines.
    assert_eq!(wait_timeout_ms(i64::MAX, 10_000_000), INFINITE);
    assert_eq!(wait_timeout_ms(i64::MAX, 1), INFINITE);
    assert_eq!(wait_timeout_ms(0, 10_000_000), 0);
    assert_eq!(wait_timeout_ms(-1, 10_000_000), 0);
    assert_eq!(wait_timeout_ms(i64::MIN, 10_000_000), 0);
    // Ceiling at 10 MHz: 1 tick is 0.0001 ms and rounds up to 1 ms; 10_000 ticks is exactly 1 ms; 10_001 is 2 ms.
    assert_eq!(wait_timeout_ms(1, 10_000_000), 1);
    assert_eq!(wait_timeout_ms(10_000, 10_000_000), 1);
    assert_eq!(wait_timeout_ms(10_001, 10_000_000), 2);
    assert_eq!(wait_timeout_ms(25_000_000, 10_000_000), 2500);
    // Cap: never INFINITE for a finite deadline.
    assert_eq!(wait_timeout_ms(i64::MAX - 1, 10_000_000), INFINITE - 1);
    assert_eq!(wait_timeout_ms(i64::MAX - 1, 1), INFINITE - 1);
    // A zero or negative frequency is clamped to 1 tick per second: 3 ticks are 3000 ms.
    assert_eq!(wait_timeout_ms(3, 0), 3000);
    assert_eq!(wait_timeout_ms(3, -5), 3000);
}
