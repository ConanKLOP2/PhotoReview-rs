//! N2 (mutation close-out, 2026-11-05): run-proofs for the survivors of `viewport/layout.rs` and `viewport/types.rs`
//! (`format_fixed`) that M1 (`docs/reviews/M1-MUT-2026-11-04.md`) only argued from reading the code. Hand written, no golden file.
//!
//! The layout helpers are private, so they are reached through the public `compute`. The test file holds a MODEL of the
//! original pipeline (a line-by-line copy of `ViewportLayoutEngine.Compute` as ported) in which each mutated comparison can
//! be switched to its mutant:
//!
//! | site | original | mutant |
//! |---|---|---|
//! | `wpf_double::greater_than` 77:16 | `value1 > value2` | `>=` |
//! | `net_max` 96:10 | `a > b` | `>=` |
//! | `net_min` 108:10 | `a < b` | `<=` |
//! | `arrange_length` 349:15 | `length < unclipped` | `<=` and `==` |
//! | `arrange_length` 353:22 | `effective_max < length` | `<=` |
//! | `alignment_offset` 361:12 | `ink > client` | `>=` |
//!
//! Three things are checked on every sampled input (3,000,000 of them: random picks from a pool of awkward values that
//! includes NaN, both infinities, both zeros, `f64::MAX`, subnormal-scale naturals and 128 ordinary and 128 overflow-scale round-off
//! witnesses `fl(n * fl(m / n)) > m`, drawn per iteration from a small set so that operands coincide often):
//!
//! 1. the REAL `compute` equals the model of the original, bit for bit (this is the check that fails under a mutant that is
//!    observable);
//! 2. every model of a mutant equals the model of the original, bit for bit;
//! 3. counters prove the sample reaches the inputs on which the mutated comparison takes the other branch (equal operands),
//!    and, for `net_max`/`net_min`, that it never does (they return on `a == b` and on NaN before the mutated line).
//!
//! `wpf_double::greater_than` is public and is also compared with a reference over a dense lattice of f64 neighbours, and
//! `format_fixed` (types.rs 207:23) is compared with an exact integer oracle on all dyadic values `n / 256`.

use pr_app::input::primitives::RectD;
use pr_app::viewport::layout::{compute, wpf_double};
use pr_app::viewport::types::{ScrollBarPolicy, ViewerStretchMode, ViewportInput, ViewportLayout, format_fixed};

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

fn layouts_same(a: &ViewportLayout, b: &ViewportLayout) -> bool {
    same(a.viewport_width, b.viewport_width)
        && same(a.viewport_height, b.viewport_height)
        && same(a.extent_width, b.extent_width)
        && same(a.extent_height, b.extent_height)
        && a.horizontal_bar_visible == b.horizontal_bar_visible
        && a.vertical_bar_visible == b.vertical_bar_visible
        && same(a.image_rect.x, b.image_rect.x)
        && same(a.image_rect.y, b.image_rect.y)
        && same(a.image_rect.width, b.image_rect.width)
        && same(a.image_rect.height, b.image_rect.height)
        && same(a.max_horizontal_offset, b.max_horizontal_offset)
        && same(a.max_vertical_offset, b.max_vertical_offset)
}

// ---------------------------------------------------------------------------------------------------------------------
// Model of the original pipeline with the six mutation sites switchable
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Cmp {
    Lt,
    Le,
    Eq,
}

impl Cmp {
    fn test(self, a: f64, b: f64) -> bool {
        match self {
            Cmp::Lt => a < b,
            Cmp::Le => a <= b,
            Cmp::Eq => a == b,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Variant {
    gt77_ge: bool,
    max96_ge: bool,
    min108_le: bool,
    al349: Cmp,
    al353: Cmp,
    off361_ge: bool,
}

const ORIG: Variant = Variant {
    gt77_ge: false,
    max96_ge: false,
    min108_le: false,
    al349: Cmp::Lt,
    al353: Cmp::Lt,
    off361_ge: false,
};

/// Branch divergence counters, one per site: the mutated comparison took another branch than the original would.
#[derive(Default, Debug, Clone, Copy)]
struct Hits {
    gt77: u64,
    max96: u64,
    min108: u64,
    al349: u64,
    /// 349:15 only: `slot < unclipped` held (the round-off case; for `Eq` the mutant skipped the assignment).
    al349_slot_below: u64,
    al353: u64,
    off361: u64,
    /// A `-0.0` seen as `slot`, `unclipped`, `max`, `effective_max` or `length` in `arrange_length` (the only values on which
    /// `<` and `<=` could produce different bits when the operands compare equal).
    neg_zero: u64,
}

fn close(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let eps = (a.abs() + b.abs() + 10.0) * f64::EPSILON;
    let delta = a - b;
    -eps < delta && eps > delta
}

fn gt(v: &Variant, h: &mut Hits, a: f64, b: f64) -> bool {
    let (o, m) = (a > b, a >= b);
    if v.gt77_ge && o != m {
        h.gt77 += 1;
    }
    (if v.gt77_ge { m } else { o }) && !close(a, b)
}

fn net_max(v: &Variant, h: &mut Hits, a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        return if a.is_sign_positive() { a } else { b };
    }
    let (o, m) = (a > b, a >= b);
    if v.max96_ge && o != m {
        h.max96 += 1;
    }
    if if v.max96_ge { m } else { o } { a } else { b }
}

fn net_min(v: &Variant, h: &mut Hits, a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        return if a.is_sign_negative() { a } else { b };
    }
    let (o, m) = (a < b, a <= b);
    if v.min108_le && o != m {
        h.min108 += 1;
    }
    if if v.min108_le { m } else { o } { a } else { b }
}

fn arrange_length(v: &Variant, h: &mut Hits, slot: f64, unclipped: f64, max: f64) -> f64 {
    let mut length = slot;
    for value in [slot, unclipped, max] {
        if value == 0.0 && value.is_sign_negative() {
            h.neg_zero += 1;
        }
    }
    let o = length < unclipped;
    let m = v.al349.test(length, unclipped);
    if v.al349 != Cmp::Lt {
        if o != m {
            h.al349 += 1;
        }
        if o {
            h.al349_slot_below += 1;
        }
    }
    if m {
        length = unclipped;
    }
    let effective_max = net_max(v, h, unclipped, max);
    if effective_max == 0.0 && effective_max.is_sign_negative() {
        h.neg_zero += 1;
    }
    let o = effective_max < length;
    let m = v.al353.test(effective_max, length);
    if v.al353 != Cmp::Lt && o != m {
        h.al353 += 1;
    }
    if m {
        length = effective_max;
    }
    length
}

fn alignment_offset(v: &Variant, h: &mut Hits, client: f64, ink: f64) -> f64 {
    let (o, m) = (ink > client, ink >= client);
    if v.off361_ge && o != m {
        h.off361 += 1;
    }
    if if v.off361_ge { m } else { o } {
        0.0
    } else {
        (client - ink) * 0.5
    }
}

fn sanitize_length(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 { value } else { 0.0 }
}

fn is_zero(value: f64) -> bool {
    value.abs() < 10.0 * f64::EPSILON
}

#[derive(Clone, Copy)]
struct Image {
    uniform: bool,
    width: f64,
    height: f64,
    max_width: f64,
    max_height: f64,
    natural_width: f64,
    natural_height: f64,
}

#[derive(Clone, Copy)]
struct Measured {
    width: f64,
    height: f64,
    unclipped_width: f64,
    unclipped_height: f64,
}

fn sanitize_explicit(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        f64::NAN
    }
}

fn sanitize_max(value: f64) -> f64 {
    if value.is_nan() || value < 0.0 {
        f64::INFINITY
    } else {
        value
    }
}

impl Image {
    fn from_input(input: &ViewportInput) -> Self {
        Self {
            uniform: input.stretch == ViewerStretchMode::Uniform,
            width: sanitize_explicit(input.image_width),
            height: sanitize_explicit(input.image_height),
            max_width: sanitize_max(input.max_image_width),
            max_height: sanitize_max(input.max_image_height),
            natural_width: sanitize_length(input.bitmap_width),
            natural_height: sanitize_length(input.bitmap_height),
        }
    }

    fn max_w(&self, v: &Variant, h: &mut Hits) -> f64 {
        let width = if self.width.is_nan() { f64::INFINITY } else { self.width };
        let m = net_min(v, h, width, self.max_width);
        net_max(v, h, m, 0.0)
    }

    fn max_h(&self, v: &Variant, h: &mut Hits) -> f64 {
        let height = if self.height.is_nan() {
            f64::INFINITY
        } else {
            self.height
        };
        let m = net_min(v, h, height, self.max_height);
        net_max(v, h, m, 0.0)
    }

    fn min_w(&self, v: &Variant, h: &mut Hits) -> f64 {
        let width = if self.width.is_nan() { 0.0 } else { self.width };
        let mw = self.max_w(v, h);
        let m = net_min(v, h, mw, width);
        net_max(v, h, m, 0.0)
    }

    fn min_h(&self, v: &Variant, h: &mut Hits) -> f64 {
        let height = if self.height.is_nan() { 0.0 } else { self.height };
        let mh = self.max_h(v, h);
        let m = net_min(v, h, mh, height);
        net_max(v, h, m, 0.0)
    }

    fn stretch_to_box(&self, v: &Variant, h: &mut Hits, box_width: f64, box_height: f64) -> (f64, f64) {
        let mut scale_x = 1.0;
        let mut scale_y = 1.0;
        let constrained_width = box_width != f64::INFINITY;
        let constrained_height = box_height != f64::INFINITY;
        if constrained_width || constrained_height {
            scale_x = if is_zero(self.natural_width) {
                0.0
            } else {
                box_width / self.natural_width
            };
            scale_y = if is_zero(self.natural_height) {
                0.0
            } else {
                box_height / self.natural_height
            };
            if !constrained_width {
                scale_x = scale_y;
            } else if !constrained_height {
                scale_y = scale_x;
            } else if self.uniform {
                let scale = net_min(v, h, scale_x, scale_y);
                scale_x = scale;
                scale_y = scale;
            }
        }
        (self.natural_width * scale_x, self.natural_height * scale_y)
    }

    fn measure(&self, v: &Variant, h: &mut Hits) -> Measured {
        let max_w = self.max_w(v, h);
        let max_h = self.max_h(v, h);
        let (override_width, override_height) = self.stretch_to_box(v, h, max_w, max_h);
        let min_w = self.min_w(v, h);
        let min_h = self.min_h(v, h);
        let unclipped_width = net_max(v, h, override_width, min_w);
        let unclipped_height = net_max(v, h, override_height, min_h);
        Measured {
            width: net_min(v, h, unclipped_width, max_w),
            height: net_min(v, h, unclipped_height, max_h),
            unclipped_width,
            unclipped_height,
        }
    }

    fn arrange(&self, v: &Variant, h: &mut Hits, measured: Measured, vw: f64, vh: f64) -> RectD {
        let slot_width = net_max(v, h, measured.width, vw);
        let slot_height = net_max(v, h, measured.height, vh);
        let max_w = self.max_w(v, h);
        let max_h = self.max_h(v, h);
        let arrange_width = arrange_length(v, h, slot_width, measured.unclipped_width, max_w);
        let arrange_height = arrange_length(v, h, slot_height, measured.unclipped_height, max_h);
        let (render_width, render_height) = self.stretch_to_box(v, h, arrange_width, arrange_height);
        let ink_w = net_min(v, h, render_width, max_w);
        let ink_h = net_min(v, h, render_height, max_h);
        RectD {
            x: alignment_offset(v, h, slot_width, ink_w),
            y: alignment_offset(v, h, slot_height, ink_h),
            width: render_width,
            height: render_height,
        }
    }
}

fn barred_length(v: &Variant, h: &mut Hits, client: f64, thickness: f64, visible: bool) -> f64 {
    if visible {
        net_max(v, h, 0.0, client - thickness)
    } else {
        client
    }
}

// Mirrors the production `resolve_bars` signature (six inputs) plus the variant and the counters.
#[allow(clippy::too_many_arguments)]
fn resolve_bars(
    v: &Variant,
    h: &mut Hits,
    policy: ScrollBarPolicy,
    ew: f64,
    eh: f64,
    cw: f64,
    ch: f64,
    thickness: f64,
) -> (bool, bool, f64, f64) {
    match policy {
        ScrollBarPolicy::Overlay => {
            let horizontal = gt(v, h, ew, cw);
            let vertical = gt(v, h, eh, ch);
            return (horizontal, vertical, cw, ch);
        }
        ScrollBarPolicy::Hidden => return (false, false, cw, ch),
        ScrollBarPolicy::Auto => {}
    }
    let mut horizontal = gt(v, h, ew, cw);
    let mut vertical = gt(v, h, eh, ch);
    if horizontal != vertical {
        if !horizontal {
            let barred = barred_length(v, h, cw, thickness, true);
            horizontal = gt(v, h, ew, barred);
        } else {
            let barred = barred_length(v, h, ch, thickness, true);
            vertical = gt(v, h, eh, barred);
        }
    }
    let viewport_width = barred_length(v, h, cw, thickness, vertical);
    let viewport_height = barred_length(v, h, ch, thickness, horizontal);
    (horizontal, vertical, viewport_width, viewport_height)
}

fn model_compute(v: &Variant, h: &mut Hits, input: &ViewportInput) -> ViewportLayout {
    let cw = sanitize_length(input.client_width);
    let ch = sanitize_length(input.client_height);
    let element = Image::from_input(input);
    let desired = element.measure(v, h);
    let (hb, vb, vw, vh) = resolve_bars(
        v,
        h,
        input.scroll_bars,
        desired.width,
        desired.height,
        cw,
        ch,
        sanitize_length(input.scroll_bar_thickness),
    );
    let image_rect = element.arrange(v, h, desired, vw, vh);
    let max_h = net_max(v, h, 0.0, desired.width - vw);
    let max_v = net_max(v, h, 0.0, desired.height - vh);
    ViewportLayout {
        viewport_width: vw,
        viewport_height: vh,
        extent_width: desired.width,
        extent_height: desired.height,
        horizontal_bar_visible: hb,
        vertical_bar_visible: vb,
        image_rect,
        max_horizontal_offset: max_h,
        max_vertical_offset: max_v,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Input generation
// ---------------------------------------------------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Round-off witnesses: `n * (m / n)` rounds above `m` (the case that makes `slot < unclipped` possible).
fn witnesses(rng: &mut Rng, wanted: usize) -> Vec<(f64, f64)> {
    let mut found = Vec::new();
    while found.len() < wanted {
        let jitter = |r: u64| 1.0 + (r & 0xFFFF_FFFF) as f64 * f64::EPSILON;
        let n = (1.0 + (rng.next() % 4_000_000) as f64 / 1000.0) * jitter(rng.next());
        let m = (1.0 + (rng.next() % 4_000_000) as f64 / 1000.0) * jitter(rng.next());
        let u = n * (m / n);
        if u > m {
            found.push((n, m));
        }
    }
    found
}

/// Round-off witnesses at overflow scale: `n * (m / n)` rounds above `m` (or to +inf) for `m` at or near `f64::MAX`.
fn extreme_witnesses(rng: &mut Rng, wanted: usize) -> Vec<(f64, f64)> {
    let mut found = Vec::new();
    while found.len() < wanted {
        let exp = -45 + (rng.next() % 70) as i32; // n in about [2.8e-14, 1e6]
        let n = (1.0 + (rng.next() & 0xF_FFFF_FFFF_FFFF) as f64 / (1u64 << 52) as f64) * 2f64.powi(exp);
        let m = match rng.next() % 4 {
            0 => f64::MAX,
            1 => f64::from_bits(f64::MAX.to_bits() - (rng.next() % 64)),
            2 => f64::MAX / 2.0,
            _ => f64::MAX / 3.0,
        };
        if n * (m / n) > m {
            found.push((n, m));
        }
    }
    found
}

fn base_pool(wit: &[(f64, f64)]) -> Vec<f64> {
    let mut p = vec![
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        -0.0,
        0.0,
        1e-15,
        3e-15,
        1e-300,
        5e-324,
        0.5,
        1.0,
        2.0,
        3.0,
        7.0,
        10.0,
        99.5,
        100.0,
        100.0 + 100.0 * f64::EPSILON,
        100.0 - 100.0 * f64::EPSILON,
        150.0,
        200.0,
        333.333,
        640.0,
        800.0,
        800.0 + 800.0 * f64::EPSILON,
        1000.0,
        1e6,
        1e15,
        f64::MAX,
        f64::from_bits(f64::MAX.to_bits() - 1),
    ];
    for &(n, m) in wit {
        let u = n * (m / n);
        p.extend([
            n,
            m,
            u,
            f64::from_bits(m.to_bits() + 1),
            f64::from_bits(m.to_bits() - 1),
        ]);
    }
    p
}

fn random_input(rng: &mut Rng, pool: &[f64]) -> ViewportInput {
    // A small per-iteration set, so that different fields hold the same value often and operands coincide.
    let mut set = [0.0f64; 10];
    for slot in set.iter_mut().take(6) {
        *slot = pool[rng.below(pool.len())];
    }
    set[6] = f64::NAN;
    set[7] = f64::INFINITY;
    set[8] = 0.0;
    set[9] = 1.0;
    let pick = |rng: &mut Rng| set[rng.below(10)];
    ViewportInput {
        client_width: pick(rng),
        client_height: pick(rng),
        scroll_bar_thickness: pick(rng),
        scroll_bars: match rng.below(3) {
            0 => ScrollBarPolicy::Auto,
            1 => ScrollBarPolicy::Overlay,
            _ => ScrollBarPolicy::Hidden,
        },
        stretch: if rng.below(4) == 0 {
            ViewerStretchMode::None
        } else {
            ViewerStretchMode::Uniform
        },
        image_width: pick(rng),
        image_height: pick(rng),
        max_image_width: pick(rng),
        max_image_height: pick(rng),
        bitmap_width: pick(rng),
        bitmap_height: pick(rng),
    }
}

/// `(name, variant, equivalent)`: 361:12 `>=` and 349:15 `==` are NOT equivalent (found by this run, see the two killers below).
fn variants() -> Vec<(&'static str, Variant, bool)> {
    vec![
        ("77:16 > -> >=", Variant { gt77_ge: true, ..ORIG }, true),
        ("96:10 > -> >=", Variant { max96_ge: true, ..ORIG }, true),
        (
            "108:10 < -> <=",
            Variant {
                min108_le: true,
                ..ORIG
            },
            true,
        ),
        ("349:15 < -> <=", Variant { al349: Cmp::Le, ..ORIG }, true),
        ("349:15 < -> ==", Variant { al349: Cmp::Eq, ..ORIG }, false),
        ("353:22 < -> <=", Variant { al353: Cmp::Le, ..ORIG }, true),
        (
            "361:12 > -> >=",
            Variant {
                off361_ge: true,
                ..ORIG
            },
            false,
        ),
    ]
}

#[test]
fn layout_mutation_sites_are_unobservable_through_compute() {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut wit = witnesses(&mut rng, 128);
    wit.extend(extreme_witnesses(&mut rng, 128));
    let pool = base_pool(&wit);
    let vs = variants();
    let mut hits = vec![Hits::default(); vs.len()];
    let mut off361_observable = 0u64;
    let mut eq349_observable = 0u64;
    let iterations = 3_000_000u64;
    for i in 0..iterations {
        let input = random_input(&mut rng, &pool);
        let real = compute(&input);
        let orig = model_compute(&ORIG, &mut Hits::default(), &input);
        assert!(
            layouts_same(&real, &orig),
            "real compute != model of the original (iteration {i}): {input:?}
{real:?}
{orig:?}"
        );
        for (k, (name, v, equivalent)) in vs.iter().enumerate() {
            let m = model_compute(v, &mut hits[k], &input);
            if *equivalent {
                assert!(
                    layouts_same(&m, &orig),
                    "{name} changes compute (iteration {i}): {input:?}
{m:?}
{orig:?}"
                );
            } else if !layouts_same(&m, &orig) {
                if name.starts_with("361") {
                    // An infinite extent gives `(inf - inf) * 0.5 = NaN` in the original.
                    assert!(
                        orig.image_rect.x.is_nan() || orig.image_rect.y.is_nan(),
                        "{name}: {input:?}"
                    );
                    off361_observable += 1;
                } else {
                    // 349:15 `==`: only when a length overflows (a field near f64::MAX makes `slot < unclipped` with an
                    // infinite `unclipped`); at ordinary scales it is unobservable (see the witness test below).
                    let huge = [
                        input.client_width,
                        input.client_height,
                        input.image_width,
                        input.image_height,
                        input.max_image_width,
                        input.max_image_height,
                        input.bitmap_width,
                        input.bitmap_height,
                    ]
                    .iter()
                    .any(|v| v.abs() >= 1e290);
                    assert!(huge, "{name} differs without a huge field: {input:?}");
                    eq349_observable += 1;
                }
            }
        }
    }
    println!("hits: {hits:?}; 361:12 observable on {off361_observable} inputs, 349:15 == on {eq349_observable}");
    // The sample reaches the equal-operand branch of every site except the two that cannot be reached by control flow.
    assert!(hits[0].gt77 > 1000, "77:16 {:?}", hits[0]);
    assert_eq!(
        hits[1].max96, 0,
        "96:10 is unreachable: `a == b` and NaN return above it"
    );
    assert_eq!(
        hits[2].min108, 0,
        "108:10 is unreachable: `a == b` and NaN return above it"
    );
    assert!(hits[3].al349 > 1000, "349:15 <= {:?}", hits[3]);
    assert!(
        hits[4].al349 > 1000 && hits[4].al349_slot_below > 100,
        "349:15 == {:?}",
        hits[4]
    );
    assert!(hits[5].al353 > 100, "353:22 <= {:?}", hits[5]);
    assert!(hits[6].off361 > 1000, "361:12 {:?}", hits[6]);
    // `<` and `<=` could only return different bits on equal operands that differ in bits, that is on a -0.0 against a +0.0;
    // no such value ever appears as slot, unclipped, max or effective_max, in any of the 3,000,000 inputs.
    assert!(
        hits.iter().all(|h| h.neg_zero == 0),
        "a -0.0 reached arrange_length: {hits:?}"
    );
    assert!(
        off361_observable > 0,
        "the sample should contain the infinite-extent input that separates 361:12"
    );
    assert!(
        eq349_observable > 0,
        "the sample should contain an overflowing input that separates 349:15 `==`"
    );
}

#[test]
fn arrange_length_with_an_overflowing_unclipped_height_takes_the_unclipped_branch() {
    // Killer for `layout.rs:349:15` `<` -> `==` in `arrange_length` (`if length < unclipped { length = unclipped }`).
    // M1 argued the `==` variant equivalent "in practice" (2,000,000 random normal-scale pairs). It is not: with an explicit
    // height of f64::MAX and a bitmap whose `natural * (MAX / natural)` rounds up to +inf, the desired height is clamped to
    // MAX but the unclipped height is +inf, so `slot (MAX) < unclipped (inf)`. The original then arranges at `unclipped = inf`;
    // an unconstrained (+inf) box makes the Fill scale copy the width scale (`494.8.. / MAX`) and the height becomes
    // `natural_height * that scale`, about 6.7e-304, centred at y ~ 9.0e307. With `==` the length stays MAX and the height is +inf.
    let input = ViewportInput {
        client_width: 1000.0,
        client_height: 618.519510434281,
        scroll_bar_thickness: 494.84535169217844,
        scroll_bars: ScrollBarPolicy::Auto,
        stretch: ViewerStretchMode::None,
        image_width: 494.84535169217844,
        image_height: f64::MAX,
        max_image_width: 494.84535169217844,
        max_image_height: f64::NAN,
        bitmap_width: f64::MAX,
        bitmap_height: 241.82117968866316,
    };
    let natural_h = 241.82117968866316_f64;
    assert!(
        natural_h * (f64::MAX / natural_h) > f64::MAX,
        "the bitmap height must be a round-off witness at MAX"
    );
    let layout = compute(&input);
    assert_eq!(layout.extent_height, f64::MAX);
    let scale = 494.84535169217844 / f64::MAX; // Fill: the unconstrained height copies the width scale
    assert_eq!(layout.image_rect.height, natural_h * scale);
    assert!(layout.image_rect.height < 1e-300 && layout.image_rect.height > 0.0);
    assert!(layout.image_rect.y > 1e307, "y = {:?}", layout.image_rect.y);
    assert!(layout.image_rect.height.is_finite());
}

#[test]
fn alignment_offset_with_an_infinite_ink_and_slot_is_nan_not_zero() {
    // Killer for `layout.rs:361:12` `>` -> `>=` in `alignment_offset` (`ink > client ? 0 : (client - ink) * 0.5`).
    // M1 argued the survivor equivalent because "every length is sanitized to be finite"; that is wrong for the extent: a
    // finite bitmap of f64::MAX scaled by `max_height / natural_height` overflows to +inf (`MAX * (MAX / 468.76) = inf`), so the
    // desired width, the slot and the ink are all +inf, `ink > client` is false and the C# formula gives
    // `(inf - inf) * 0.5 = NaN` for the x offset. With `>=` the branch returns 0.0 instead.
    let input = ViewportInput {
        client_width: 3189.0,
        client_height: 2000.0,
        scroll_bar_thickness: 0.0,
        scroll_bars: ScrollBarPolicy::Hidden,
        stretch: ViewerStretchMode::Uniform,
        image_width: f64::NAN,
        image_height: f64::NAN,
        max_image_width: f64::NAN,
        max_image_height: f64::MAX,
        bitmap_width: f64::MAX,
        bitmap_height: 468.0,
    };
    let layout = compute(&input);
    assert_eq!(layout.extent_width, f64::INFINITY);
    assert_eq!(layout.image_rect.width, f64::INFINITY);
    assert!(layout.image_rect.x.is_nan(), "x = {:?}", layout.image_rect.x);
    // The finite axis is unaffected: the height fits the client (centred, no NaN).
    assert!(layout.image_rect.y.is_finite(), "y = {:?}", layout.image_rect.y);
    // The model of the original agrees (so the model is a faithful copy on this input).
    let orig = model_compute(&ORIG, &mut Hits::default(), &input);
    assert!(orig.image_rect.x.is_nan());
}

#[test]
fn layout_round_off_witnesses_drive_slot_below_unclipped_on_both_axes() {
    // Targeted pass for the `==` variant of 349:15 (the only one that is not a pure equal-operand case): a uniform or
    // filling image whose max box is a round-off witness, in a client area smaller than the box, so `slot < unclipped`
    // happens in the real pipeline. The final rectangle must still equal the original model's (checked on the real `compute`).
    let mut rng = Rng(0xCAFE_F00D_1234_5678);
    let wit = witnesses(&mut rng, 4000);
    let eq = Variant { al349: Cmp::Eq, ..ORIG };
    let mut below = 0u64;
    for (idx, &(n, m)) in wit.iter().enumerate() {
        let (n2, m2) = wit[(idx * 7 + 3) % wit.len()];
        for uniform in [true, false] {
            for client in [1.0, 10.0, m.min(m2) / 2.0] {
                let input = ViewportInput {
                    client_width: client,
                    client_height: client,
                    scroll_bar_thickness: 0.0,
                    scroll_bars: ScrollBarPolicy::Hidden,
                    stretch: if uniform {
                        ViewerStretchMode::Uniform
                    } else {
                        ViewerStretchMode::None
                    },
                    image_width: f64::NAN,
                    image_height: f64::NAN,
                    max_image_width: m,
                    max_image_height: m2,
                    bitmap_width: n,
                    bitmap_height: n2,
                };
                let real = compute(&input);
                let orig = model_compute(&ORIG, &mut Hits::default(), &input);
                let mut h_eq = Hits::default();
                let mutant = model_compute(&eq, &mut h_eq, &input);
                assert!(layouts_same(&real, &orig), "real != original model: {input:?}");
                assert!(
                    layouts_same(&mutant, &orig),
                    "`==` changes compute: {input:?}
{mutant:?}
{orig:?}"
                );
                below += h_eq.al349_slot_below;
            }
        }
    }
    assert!(below > 2000, "slot < unclipped reached {below} times");
}

// ---------------------------------------------------------------------------------------------------------------------
// wpf_double::greater_than 77:16 (public)
// ---------------------------------------------------------------------------------------------------------------------

fn reference_greater_than(a: f64, b: f64) -> bool {
    a > b && !close(a, b)
}

#[test]
fn wpf_greater_than_equals_the_reference_on_every_f64_neighbour_lattice() {
    // Equal operands (the only inputs on which `>` and `>=` differ): never greater, for every class of f64.
    let mut equal_checked = 0u64;
    for top in 0u64..(1 << 16) {
        for filler in [0u64, 1, 0x5555_5555_5555, 0xFFFF_FFFF_FFFF] {
            let x = f64::from_bits((top << 48) | filler);
            assert!(!wpf_double::greater_than(x, x), "{x:?}");
            equal_checked += 1;
        }
    }
    assert_eq!(equal_checked, 4 * 65_536);
    // Neighbour lattice: every ordered pair of values taken within 40 ulps of a set of centres.
    let centres = [0.0, 1.0, 10.0, 99.5, 800.0, 1e6, 1e15, f64::MAX, 5e-324, 1e-300];
    let mut pairs = 0u64;
    for c in centres {
        for k in -40i64..=40 {
            for j in -40i64..=40 {
                let a = f64::from_bits((f64::to_bits(c) as i64 + k) as u64);
                let b = f64::from_bits((f64::to_bits(c) as i64 + j) as u64);
                for (x, y) in [(a, b), (-a, b), (a, -b)] {
                    assert_eq!(
                        wpf_double::greater_than(x, y),
                        reference_greater_than(x, y),
                        "{x:?} {y:?}"
                    );
                    pairs += 1;
                }
            }
        }
    }
    assert!(pairs > 100_000);
    // NaN and infinity classes.
    let specials = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -0.0, 1.0, -1.0];
    for a in specials {
        for b in specials {
            assert_eq!(
                wpf_double::greater_than(a, b),
                reference_greater_than(a, b),
                "{a:?} {b:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// format_fixed 207:23
// ---------------------------------------------------------------------------------------------------------------------

/// Exact oracle for the non-negative dyadic value `n / 256` at `decimals` places, rounded half to even.
fn dyadic_oracle(n: u64, decimals: u32) -> (String, u8, bool) {
    let scaled = u128::from(n) * 10u128.pow(decimals);
    let q = scaled / 256;
    let r = scaled % 256;
    let round_up = r * 2 > 256 || (r * 2 == 256 && q % 2 == 1);
    let q = if round_up { q + 1 } else { q };
    let mut digits = q.to_string();
    while digits.len() <= decimals as usize {
        digits.insert(0, '0');
    }
    let text = if decimals == 0 {
        digits
    } else {
        let split = digits.len() - decimals as usize;
        format!("{}.{}", &digits[..split], &digits[split..])
    };
    // First dropped digit of the exact expansion, and whether it is an exact tie.
    let first_dropped = (u128::from(n) * 10u128.pow(decimals + 1) / 256 % 10) as u8;
    (text, first_dropped, r * 2 == 256)
}

#[test]
fn format_fixed_first_dropped_digit_five_is_taken_by_the_earlier_arm() {
    // In `round_up` the arm `Some(b'5')` precedes `Some(b) => *b > b'5'`, so the second arm never sees '5'; `>` and `>=`
    // can only be told apart by a '5' reaching it. Exhaustive over every first dropped digit 0..=9 (each with tie and
    // non-tie continuations) on the exact dyadic values n / 256, n in 0..=8191, at 0..=8 decimals.
    let mut seen_digit = [false; 10];
    let (mut ties, mut five_not_tie) = (0u64, 0u64);
    let mut checked = 0u64;
    for n in 0u64..=8191 {
        let value = n as f64 / 256.0;
        for decimals in 0u32..=8 {
            let (expected, dropped, tie) = dyadic_oracle(n, decimals);
            assert_eq!(
                format_fixed(value, decimals as usize),
                expected,
                "n {n} decimals {decimals}"
            );
            seen_digit[dropped as usize] = true;
            if tie {
                ties += 1;
            } else if dropped == 5 {
                five_not_tie += 1;
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 8192 * 9);
    assert!(seen_digit.iter().all(|&s| s), "{seen_digit:?}");
    assert!(ties > 100 && five_not_tie > 100, "{ties} {five_not_tie}");
}
