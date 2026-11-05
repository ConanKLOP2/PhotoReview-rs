//! MUT5 (2026-11-06): the `unwrap_or(INFINITE - 1)` arm of `host/message_loop.rs::wait_timeout_ms` (line 170, column 72: `- -> /`, and `- -> +`).
//!
//! The mutated expression is `u32::try_from(ms.min(0xFFFF_FFFE)).unwrap_or(<mutated constant>)`. The mutant differs from the original only when
//! `try_from` fails, i.e. only when `ms.min(0xFFFF_FFFE)` is negative or above `u32::MAX`. The input classes of `wait_timeout_ms(until, frequency)`:
//!
//! * A: `until == i64::MAX` -> returns `INFINITE` before the expression;
//! * B: `until <= 0` -> returns `0` before the expression;
//! * C: `1 <= until < i64::MAX`, any `frequency` (`frequency.max(1) >= 1`) -> the expression is evaluated, with
//!   `ms = ceil(until * 1000 / f)` in `i128` (no overflow: `until * 1000 < 2^73`). The numerator is `>= 1000` and `f >= 1`, so `ms >= 1`; and
//!   `ms.min(0xFFFF_FFFE) <= 0xFFFF_FFFE < u32::MAX`. Hence `try_from` always succeeds in class C and the `unwrap_or` arm is dead code: the two
//!   classes that reach the expression's argument can never reach the arm, so no input can tell the mutant from the original. No test can kill it.
//!
//! The test below runs the REAL function at the extremes of class C (smallest and largest `until`, smallest and largest `frequency`, the cap
//! boundary, the three `frequency` values that `max(1)` collapses) and checks the result against the `i128` reference, which asserts that the
//! `try_from` input is in `[1, 0xFFFF_FFFE]` there. `ms` is non-decreasing in `until` and non-increasing in `frequency`, so the extremes bound the
//! whole class. This is an extremal run plus the argument above, NOT a run over all of `i64 x i64`; the report states it that way.
#![cfg(windows)]

use pr_platform_win::host::message_loop::wait_timeout_ms;

const INFINITE: u32 = 0xFFFF_FFFF;
const CAP: i128 = 0xFFFF_FFFE;

fn ms(until: i64, frequency: i64) -> i128 {
    let f = i128::from(frequency.max(1));
    (i128::from(until) * 1000 + f - 1) / f
}

#[test]
fn class_c_never_reaches_the_unwrap_or_arm_at_the_extremes() {
    let untils = [
        1i64,
        2,
        999,
        1000,
        1001,
        i64::MAX / 1000,
        i64::MAX / 1000 + 1,
        i64::MAX - 2,
        i64::MAX - 1,
    ];
    let frequencies = [
        i64::MIN,
        -1,
        0,
        1,
        2,
        999,
        1000,
        10_000_000,
        i64::MAX / 1000,
        i64::MAX - 1,
        i64::MAX,
    ];
    for &until in &untils {
        for &frequency in &frequencies {
            let wide = ms(until, frequency);
            assert!(wide >= 1, "until {until} frequency {frequency}: ms {wide} < 1");
            let capped = wide.min(CAP);
            assert!(
                u32::try_from(capped).is_ok(),
                "until {until} frequency {frequency}: the try_from argument {capped} does not fit u32"
            );
            assert_eq!(
                i128::from(wait_timeout_ms(until, frequency)),
                capped,
                "until {until} frequency {frequency}"
            );
        }
    }
    // The cap boundary: ms just below, at and above 0xFFFF_FFFE with frequency 1000 (ms == until).
    for ms_value in [0xFFFF_FFFDi64, 0xFFFF_FFFE, 0xFFFF_FFFF, 0x1_0000_0000, 0x1_0000_0001] {
        assert_eq!(
            i128::from(wait_timeout_ms(ms_value, 1000)),
            i128::from(ms_value).min(CAP),
            "ms {ms_value}"
        );
    }
    // Classes A and B never reach the expression.
    assert_eq!(wait_timeout_ms(i64::MAX, 1), INFINITE);
    assert_eq!(wait_timeout_ms(0, 1), 0);
    assert_eq!(wait_timeout_ms(i64::MIN, i64::MAX), 0);
}
