//! N2 (mutation close-out, 2026-11-05): run-proofs for the three survivors of `input/kinetic_pan.rs` that M3
//! (`docs/reviews/M3-MUT-2026-11-04.md`) only argued from reading the code. Hand written, no golden file.
//!
//! * `dotnet_max` 54:19 `||` -> `&&` and 56:17 `>` -> `>=`: every call site passes the literal `+0.0` as the first
//!   operand (`ScrollBounds::max_horizontal`, `max_vertical`, and `next_vblank` inside `GlideFrameClock::advance`). The
//!   first test drives the real function through `ScrollBounds` with the second operand `b` set to EVERY class of f64
//!   (`extent - 0.0 == extent` bit for bit, also for NaN, infinities and `-0.0`), more than 260,000 values covering every
//!   sign, every exponent (subnormals, infinities, NaN) and the top four mantissa bits, with four low-bit fillers. The
//!   function sees `b` only through "NaN / above zero / zero / negative zero / below zero", and each of these classes has
//!   dozens to thousands of representatives, so this is the whole input domain of the function for `a = +0.0`.
//! * `advance` 470:38 `>` -> `>=` (`if rendering_elapsed > 0.0 { last_rendering_ms = rendering_time_ms }`): the clock is
//!   a `Copy` state machine, so it is compared with a model of the ORIGINAL on every call sequence of length 4 drawn from
//!   a 36 symbol alphabet (6 rendering times including NaN, infinity and both zeros; 2 `now` values; 3 timings: none, a
//!   60 Hz grid and a 30 Hz grid that trips the period-change tolerance), in both modes: 2 * 36^4 = 3,359,232 sequences.
//!   The model with the mutant is run alongside; the counters show the lattice reaches the signed-zero case where the
//!   mutant stores different bits (`-0.0` after `+0.0`), and that no output ever differs.

use pr_app::input::kinetic_pan::{GLIDE_PERIOD_CHANGE_TOLERANCE, GLIDE_PRESENT_LEAD_MS, GlideFrameClock, ScrollBounds};
use pr_core::abstractions::DisplayTiming;
use pr_core::model::enums::KineticGlideSmoothing;

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// Spec of `Math.Max(0.0, b)` of .NET Core: NaN propagates, `+0.0` beats `-0.0`.
fn spec_max_with_plus_zero(b: f64) -> f64 {
    if b.is_nan() {
        f64::NAN
    } else if b > 0.0 {
        b
    } else {
        0.0 // b == +0.0, b == -0.0 and every negative b give +0.0
    }
}

#[test]
fn dotnet_max_with_plus_zero_first_operand_over_every_class_of_second_operand() {
    let fillers: [u64; 4] = [0, 1, 0x5555_5555_5555, 0xF_FFFF_FFFF_FFFF];
    let mut count = 0u64;
    let (mut nans, mut pos, mut plus_zero, mut minus_zero, mut neg) = (0u64, 0u64, 0u64, 0u64, 0u64);
    // 1 sign bit + 11 exponent bits + 4 top mantissa bits = 16 bits, 65,536 patterns, times four fillers.
    for top in 0u64..(1 << 16) {
        for filler in fillers {
            let bits = (top << 48) | (filler & 0xFFFF_FFFF_FFFF);
            let b = f64::from_bits(bits);
            // extent - viewport with viewport = +0.0 is `b` unchanged (x - 0.0 == x, -0.0 - 0.0 == -0.0, NaN stays NaN).
            let bounds = ScrollBounds::new(b, b, 0.0, 0.0);
            let h = bounds.max_horizontal();
            let v = bounds.max_vertical();
            let expected = spec_max_with_plus_zero(b);
            assert!(
                same(h, expected),
                "b = {b:?} ({bits:#x}): horizontal {h:?} vs {expected:?}"
            );
            assert!(
                same(v, expected),
                "b = {b:?} ({bits:#x}): vertical {v:?} vs {expected:?}"
            );
            // The two zero results must carry the sign bit of +0.0.
            if !expected.is_nan() && expected == 0.0 {
                assert!(h.is_sign_positive() && v.is_sign_positive(), "b = {b:?}");
            }
            count += 1;
            if b.is_nan() {
                nans += 1;
            } else if b > 0.0 {
                pos += 1;
            } else if b == 0.0 && b.is_sign_positive() {
                plus_zero += 1;
            } else if b == 0.0 {
                minus_zero += 1;
            } else {
                neg += 1;
            }
        }
    }
    assert_eq!(count, 4 * 65_536);
    // Every class has many representatives (+0 and -0 each appear once per filler that keeps the value zero).
    assert!(nans >= 100 && pos > 50_000 && neg > 50_000, "{nans} {pos} {neg}");
    assert!(plus_zero >= 1 && minus_zero >= 1, "{plus_zero} {minus_zero}");
}

#[test]
fn scroll_bounds_difference_nan_and_zero_cases_from_two_operands() {
    // The difference itself can be NaN without NaN inputs (inf - inf) and zero with signed operands.
    let vals = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        -0.0,
        0.0,
        1.0,
        5e-324,
        f64::MAX,
    ];
    for e in vals {
        for p in vals {
            let b = ScrollBounds::new(e, e, p, p);
            let diff = e - p;
            let expected = spec_max_with_plus_zero(diff);
            assert!(same(b.max_horizontal(), expected), "{e:?} - {p:?}");
            assert!(same(b.max_vertical(), expected), "{e:?} - {p:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// GlideFrameClock::advance 470:38
// ---------------------------------------------------------------------------------------------------------------------

/// A model of the C# / production `GlideFrameClock` with the `>` at 470:38 switchable to `>=`.
#[derive(Clone, Copy)]
struct Model {
    predict: bool,
    ticks_per_ms: f64,
    started: bool,
    last_rendering_ms: f64,
    anchored: bool,
    anchor_vblank: i64,
    anchor_period: i64,
    last_index: i64,
    ge: bool,
    /// Times the mutated store ran with different bits than the original would have kept.
    store_bits_differ: u64,
}

impl Model {
    fn new(ge: bool) -> Self {
        Self {
            predict: false,
            ticks_per_ms: 0.0,
            started: false,
            last_rendering_ms: 0.0,
            anchored: false,
            anchor_vblank: 0,
            anchor_period: 0,
            last_index: 0,
            ge,
            store_bits_differ: 0,
        }
    }

    fn start(&mut self, predict: bool, ticks_per_ms: f64) {
        self.predict = predict && ticks_per_ms > 0.0;
        self.ticks_per_ms = ticks_per_ms;
        self.started = false;
        self.anchored = false;
    }

    fn next_vblank(&self, now_ticks: i64, timing: DisplayTiming) -> i64 {
        let target = now_ticks.wrapping_add((GLIDE_PRESENT_LEAD_MS * self.ticks_per_ms) as i64);
        let periods = (target.wrapping_sub(timing.last_vblank) as f64 / timing.refresh_period as f64).ceil();
        // `dotnet_max(0.0, periods)`: periods is never NaN here (the period is positive), and 0.0 is +0.0.
        let m = if periods > 0.0 { periods } else { 0.0 };
        timing
            .last_vblank
            .wrapping_add((m * timing.refresh_period as f64) as i64)
    }

    fn anchor(&mut self, now_ticks: i64, timing: DisplayTiming) {
        self.anchored = true;
        self.anchor_vblank = self.next_vblank(now_ticks, timing);
        self.anchor_period = timing.refresh_period;
        self.last_index = 0;
    }

    fn advance(&mut self, rendering_time_ms: f64, now_ticks: i64, timing: Option<DisplayTiming>) -> f64 {
        let rendering_elapsed = rendering_time_ms - self.last_rendering_ms;
        let usable = timing.filter(|t| t.refresh_period > 0);
        if !self.started {
            self.started = true;
            self.last_rendering_ms = rendering_time_ms;
            if self.predict
                && let Some(first) = usable
            {
                self.anchor(now_ticks, first);
            }
            return 0.0;
        }
        if self.predict
            && let Some(current) = usable
        {
            let period_diff = current.refresh_period.wrapping_sub(self.anchor_period).unsigned_abs() as f64;
            if self.anchored && period_diff <= GLIDE_PERIOD_CHANGE_TOLERANCE * self.anchor_period as f64 {
                let delta = self.next_vblank(now_ticks, current).wrapping_sub(self.anchor_vblank);
                let index = (delta as f64 / current.refresh_period as f64).round_ties_even() as i64;
                let store = if self.ge {
                    rendering_elapsed >= 0.0
                } else {
                    rendering_elapsed > 0.0
                };
                if store {
                    if self.ge
                        && rendering_elapsed.partial_cmp(&0.0) == Some(std::cmp::Ordering::Equal)
                        && self.last_rendering_ms.to_bits() != rendering_time_ms.to_bits()
                    {
                        self.store_bits_differ += 1;
                    }
                    self.last_rendering_ms = rendering_time_ms;
                }
                if index <= self.last_index {
                    return 0.0;
                }
                let advanced =
                    index.wrapping_sub(self.last_index) as f64 * current.refresh_period as f64 / self.ticks_per_ms;
                self.last_index = index;
                return advanced;
            }
            self.anchor(now_ticks, current);
        } else if self.predict {
            self.anchored = false;
        }
        if rendering_elapsed.is_nan() || rendering_elapsed <= 0.0 {
            return 0.0;
        }
        self.last_rendering_ms = rendering_time_ms;
        rendering_elapsed
    }
}

const RENDER_TIMES: [f64; 6] = [f64::NAN, f64::INFINITY, -0.0, 0.0, 1.0, 2.0];
const NOW: [i64; 2] = [0, 1_000_000];

fn timings() -> [Option<DisplayTiming>; 3] {
    [
        None,
        Some(DisplayTiming {
            last_vblank: 0,
            refresh_period: 166_667,
        }),
        Some(DisplayTiming {
            last_vblank: 0,
            refresh_period: 333_334,
        }),
    ]
}

#[test]
fn glide_clock_store_of_equal_rendering_time_is_unobservable_on_every_short_call_sequence() {
    const TICKS_PER_MS: f64 = 10_000.0;
    let tims = timings();
    let mut alphabet: Vec<(f64, i64, Option<DisplayTiming>)> = Vec::new();
    for rt in RENDER_TIMES {
        for now in NOW {
            for t in tims {
                alphabet.push((rt, now, t));
            }
        }
    }
    assert_eq!(alphabet.len(), 36);
    let n = alphabet.len();
    let mut sequences = 0u64;
    let mut store_bits_differ = 0u64;
    let mut nonzero_outputs = 0u64;
    for predict in [false, true] {
        let mode = if predict {
            KineticGlideSmoothing::Predict
        } else {
            KineticGlideSmoothing::Off
        };
        for code in 0..n.pow(4) {
            let mut real = GlideFrameClock::new();
            real.start(mode, TICKS_PER_MS);
            let mut orig = Model::new(false);
            orig.start(predict, TICKS_PER_MS);
            let mut mutant = Model::new(true);
            mutant.start(predict, TICKS_PER_MS);
            let mut c = code;
            for _ in 0..4 {
                let (rt, now, timing) = alphabet[c % n];
                c /= n;
                let r = real.advance(rt, now, timing);
                let o = orig.advance(rt, now, timing);
                let m = mutant.advance(rt, now, timing);
                assert!(
                    same(r, o),
                    "real {r:?} vs original model {o:?} (code {code}, predict {predict})"
                );
                assert!(
                    same(m, o),
                    "mutant model {m:?} vs original model {o:?} (code {code}, predict {predict})"
                );
                if o != 0.0 {
                    nonzero_outputs += 1;
                }
            }
            store_bits_differ += mutant.store_bits_differ;
            sequences += 1;
        }
    }
    assert_eq!(sequences, 2 * 36u64.pow(4));
    // Not vacuous: the stored value does differ in bits (-0.0 against +0.0) in many sequences, and the clock really steps.
    assert!(store_bits_differ > 1000, "{store_bits_differ}");
    assert!(nonzero_outputs > 100_000, "{nonzero_outputs}");
}
