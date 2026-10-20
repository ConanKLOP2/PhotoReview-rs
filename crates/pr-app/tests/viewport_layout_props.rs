//! Red property tests (W8-01b, test porter) for `pr_app::viewport::layout`: the five invariants of the C#
//! `ViewportLayoutEnginePropertyTests` (`tests/PhotoReview.Shell.Tests/Viewport/ViewportLayoutEnginePropertyTests.cs` at
//! baseline `d92d6aae`). The C# `new Random(seed)` sequence cannot be reproduced, so a fixed-seed local generator (SplitMix64)
//! builds the inputs: the golden pins the values, these tests pin the invariants. Seeds 1 to 5 match the C# tests, 20 000
//! inputs each. Every test fails through `todo!()` until the bodies land.

use pr_app::viewport::layout::{clamp_offset, compute, wpf_double};
use pr_app::viewport::types::{ScrollBarPolicy, ViewerStretchMode, ViewportInput, ViewportLayout};

const ITERATIONS: usize = 20_000;
const INF: f64 = f64::INFINITY;
const NAN: f64 = f64::NAN;
const POLICIES: [ScrollBarPolicy; 3] = [ScrollBarPolicy::Auto, ScrollBarPolicy::Overlay, ScrollBarPolicy::Hidden];

/// SplitMix64: a small, fixed-seed generator with no dependency. The same seed gives the same inputs on every run.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in [0, n).
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

/// C# `Length(max)` (ViewportLayoutEnginePropertyTests 22-26): 0, a whole number (often on an edge), or a random length.
fn length(rng: &mut Rng, max: f64) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 => (rng.next_f64() * max).round(),
        _ => rng.next_f64() * max,
    }
}

/// C# `Explicit(client)` for the zoom branch (lines 42-49).
fn explicit(rng: &mut Rng, client: f64, thickness: f64) -> f64 {
    match rng.below(6) {
        0 => client,
        1 => (client - thickness).max(0.0),
        2 => NAN,
        _ => length(rng, 12_000.0),
    }
}

/// C# `RandomInputs(seed)` (lines 15-54), with the same branch structure. Fit: MaxImage = client (bounded), or
/// infinite before the first measure. Zoom: explicit Width/Height, infinite MaxImage.
fn random_inputs(seed: u64) -> Vec<ViewportInput> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let cw = length(&mut rng, 4000.0);
        let ch = length(&mut rng, 2500.0);
        let policy = POLICIES[rng.below(POLICIES.len() as u64) as usize];
        let thickness = if rng.below(4) == 0 {
            0.0
        } else {
            10.0 + rng.next_f64() * 10.0
        };
        let bitmap_w = length(&mut rng, 8000.0);
        let bitmap_h = length(&mut rng, 8000.0);
        if rng.below(2) == 0 {
            let bounded = rng.below(5) != 0;
            out.push(ViewportInput {
                client_width: cw,
                client_height: ch,
                scroll_bar_thickness: thickness,
                scroll_bars: policy,
                stretch: ViewerStretchMode::Uniform,
                image_width: NAN,
                image_height: NAN,
                max_image_width: if bounded { cw } else { INF },
                max_image_height: if bounded { ch } else { INF },
                bitmap_width: bitmap_w,
                bitmap_height: bitmap_h,
            });
        } else {
            let width = explicit(&mut rng, cw, thickness);
            let height = explicit(&mut rng, ch, thickness);
            out.push(ViewportInput {
                client_width: cw,
                client_height: ch,
                scroll_bar_thickness: thickness,
                scroll_bars: policy,
                stretch: ViewerStretchMode::None,
                image_width: width,
                image_height: height,
                max_image_width: INF,
                max_image_height: INF,
                bitmap_width: bitmap_w,
                bitmap_height: bitmap_h,
            });
        }
    }
    out
}

/// `ViewportLayoutMath.SanitizeLength` (C# 133) as a reference model: finite and `> 0` keeps the value, else `+0.0`.
/// The production function is private, so the property test restates it.
fn sanitize_length_model(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 { value } else { 0.0 }
}

/// Port of `ClampedOffsets_AreAlwaysWithinZeroAndTheMaximum_AndClampingIsIdempotent` (seed 1; requests from seed 16).
#[test]
fn clamped_offsets_are_within_zero_and_the_maximum_and_clamping_is_idempotent() {
    let mut request_rng = Rng::new(16);
    for input in random_inputs(1) {
        let layout: ViewportLayout = compute(&input);
        assert!(layout.max_horizontal_offset >= 0.0 && layout.max_vertical_offset >= 0.0);

        let request_h =
            request_rng.next_f64() * 3.0 * (layout.max_horizontal_offset + 10.0) - layout.max_horizontal_offset - 10.0;
        let request_v =
            request_rng.next_f64() * 3.0 * (layout.max_vertical_offset + 10.0) - layout.max_vertical_offset - 10.0;
        let (h, v) = clamp_offset(&layout, request_h, request_v);

        assert!(
            h >= 0.0 && h <= layout.max_horizontal_offset,
            "{input:?} -> {layout:?}: h = {h}"
        );
        assert!(
            v >= 0.0 && v <= layout.max_vertical_offset,
            "{input:?} -> {layout:?}: v = {v}"
        );
        assert_eq!(
            clamp_offset(&layout, h, v),
            (h, v),
            "clamping is idempotent for {input:?}"
        );
        if request_h >= 0.0 && request_h <= layout.max_horizontal_offset {
            assert_eq!(h, request_h, "an in-range request is kept for {input:?}");
        }
        if request_v >= 0.0 && request_v <= layout.max_vertical_offset {
            assert_eq!(v, request_v, "an in-range request is kept for {input:?}");
        }
    }
}

/// Port of `AutoBars_AreVisibleExactlyWhenTheContentOverflowsTheirViewport_WhichLosesTheBarsThickness` (seed 2).
#[test]
fn auto_bars_are_visible_exactly_when_the_content_overflows_losing_the_bar_thickness() {
    for input in random_inputs(2)
        .into_iter()
        .filter(|i| i.scroll_bars == ScrollBarPolicy::Auto)
    {
        let layout = compute(&input);
        assert_eq!(
            wpf_double::greater_than(layout.extent_width, layout.viewport_width),
            layout.horizontal_bar_visible,
            "horizontal bar for {input:?} -> {layout:?}"
        );
        assert_eq!(
            wpf_double::greater_than(layout.extent_height, layout.viewport_height),
            layout.vertical_bar_visible,
            "vertical bar for {input:?} -> {layout:?}"
        );
        let thickness = sanitize_length_model(input.scroll_bar_thickness);
        let expected_vw = (input.client_width - if layout.vertical_bar_visible { thickness } else { 0.0 }).max(0.0);
        let expected_vh = (input.client_height - if layout.horizontal_bar_visible { thickness } else { 0.0 }).max(0.0);
        assert!(
            (layout.viewport_width - expected_vw).abs() < 1e-9,
            "viewport_width for {input:?}"
        );
        assert!(
            (layout.viewport_height - expected_vh).abs() < 1e-9,
            "viewport_height for {input:?}"
        );
        assert!(
            (layout.max_horizontal_offset - (layout.extent_width - layout.viewport_width).max(0.0)).abs() < 1e-9,
            "max_horizontal_offset for {input:?}"
        );
        assert!(
            (layout.max_vertical_offset - (layout.extent_height - layout.viewport_height).max(0.0)).abs() < 1e-9,
            "max_vertical_offset for {input:?}"
        );
    }
}

/// Port of `OverlayAndHiddenBars_NeverTakeSpace` (seed 3).
#[test]
fn overlay_and_hidden_bars_never_take_space() {
    for input in random_inputs(3)
        .into_iter()
        .filter(|i| i.scroll_bars != ScrollBarPolicy::Auto)
    {
        let layout = compute(&input);
        assert_eq!(
            layout.viewport_width, input.client_width,
            "viewport_width for {input:?}"
        );
        assert_eq!(
            layout.viewport_height, input.client_height,
            "viewport_height for {input:?}"
        );
        let overlay = input.scroll_bars == ScrollBarPolicy::Overlay;
        assert_eq!(
            layout.horizontal_bar_visible,
            overlay && wpf_double::greater_than(layout.extent_width, layout.viewport_width),
            "horizontal bar for {input:?}"
        );
        assert_eq!(
            layout.vertical_bar_visible,
            overlay && wpf_double::greater_than(layout.extent_height, layout.viewport_height),
            "vertical bar for {input:?}"
        );
    }
}

/// Port of `TheImage_StaysInsideItsSlot_AndIsCentredWhenSmaller` (seed 4).
#[test]
fn the_image_stays_inside_its_slot_and_is_centred_when_smaller() {
    for input in random_inputs(4) {
        let layout = compute(&input);
        let slot_w = layout.extent_width.max(layout.viewport_width);
        let slot_h = layout.extent_height.max(layout.viewport_height);
        let r = layout.image_rect;
        assert!(r.x >= 0.0 && r.y >= 0.0, "{input:?} -> {layout:?}");
        assert!(
            r.x + r.width <= slot_w + 1e-6 && r.y + r.height <= slot_h + 1e-6,
            "{input:?} -> {layout:?}"
        );
        assert!(
            (r.x - (slot_w - r.width) / 2.0).abs() < 1e-6,
            "horizontal centring for {input:?} -> {layout:?}"
        );
        assert!(
            (r.y - (slot_h - r.height) / 2.0).abs() < 1e-6,
            "vertical centring for {input:?} -> {layout:?}"
        );
    }
}

/// Port of `BoundedFit_KeepsTheAspectRatio_TouchesOneSideOfTheBox_AndNeverScrolls` (seed 5).
#[test]
fn bounded_fit_keeps_the_aspect_ratio_touches_one_side_and_never_scrolls() {
    let bounded_fits = random_inputs(5).into_iter().filter(|i| {
        i.stretch == ViewerStretchMode::Uniform
            && i.max_image_width.is_finite()
            && i.client_width > 0.0
            && i.client_height > 0.0
            && i.bitmap_width > 0.0
            && i.bitmap_height > 0.0
    });
    for input in bounded_fits {
        let layout = compute(&input);
        let r = layout.image_rect;
        assert!(
            !(layout.horizontal_bar_visible || layout.vertical_bar_visible),
            "{input:?} -> {layout:?}"
        );
        assert_eq!(layout.max_horizontal_offset, 0.0, "{input:?} -> {layout:?}");
        assert_eq!(layout.max_vertical_offset, 0.0, "{input:?} -> {layout:?}");
        assert!(
            r.width <= input.client_width * (1.0 + 1e-12) && r.height <= input.client_height * (1.0 + 1e-12),
            "{input:?} -> {layout:?}"
        );
        let touches = (r.width - input.client_width).abs() <= 1e-9 * input.client_width
            || (r.height - input.client_height).abs() <= 1e-9 * input.client_height;
        assert!(touches, "{input:?} -> {layout:?}");
        assert!(
            (input.bitmap_width / input.bitmap_height - r.width / r.height).abs() < 1e-6,
            "aspect ratio for {input:?} -> {layout:?}"
        );
    }
}
