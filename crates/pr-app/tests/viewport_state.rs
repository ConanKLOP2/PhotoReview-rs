//! Red tests (W8-01a, test porter) for `pr_app::viewport::state::ViewportState`: delayed layout, the remembered scroll
//! request, and the engine call counts. They use a hand-written `FakeEngine`, so they do not depend on W8-01b.
//! The scripted golden `viewport-state` is in `viewport_golden.rs`.
//!
//! Port of `PhotoReview.App.Shared/Viewport/ViewportState.cs` (baseline `d92d6aae`). Every test here fails through `todo!()`.

use std::cell::Cell;

use pr_app::input::primitives::{PointD, RectD};
use pr_app::viewport::state::{LayoutEngine, ViewportState};
use pr_app::viewport::types::{ScrollBarPolicy, ViewerStretchMode, ViewportInput, ViewportLayout};

/// A hand-written engine: the viewport is the client size, the extent is the image size (Stretch None), no bars.
/// `clamp_offset` clamps the upper bound first, then the lower bound at 0, as the C# `CoerceOffset` does.
/// Counts every call so the tests can check when the engine runs.
#[derive(Debug, Default)]
struct FakeEngine {
    compute_calls: Cell<usize>,
    clamp_calls: Cell<usize>,
}

impl LayoutEngine for FakeEngine {
    fn compute(&self, input: &ViewportInput) -> ViewportLayout {
        self.compute_calls.set(self.compute_calls.get() + 1);
        ViewportLayout {
            viewport_width: input.client_width,
            viewport_height: input.client_height,
            extent_width: input.image_width,
            extent_height: input.image_height,
            horizontal_bar_visible: false,
            vertical_bar_visible: false,
            image_rect: RectD {
                x: 0.0,
                y: 0.0,
                width: input.image_width,
                height: input.image_height,
            },
            max_horizontal_offset: (input.image_width - input.client_width).max(0.0),
            max_vertical_offset: (input.image_height - input.client_height).max(0.0),
        }
    }

    fn clamp_offset(&self, layout: &ViewportLayout, horizontal: f64, vertical: f64) -> (f64, f64) {
        self.clamp_calls.set(self.clamp_calls.get() + 1);
        let clamp_axis = |value: f64, extent: f64, viewport: f64| {
            let max = (extent - viewport).max(0.0);
            let upper = if value > max { max } else { value };
            if upper < 0.0 { 0.0 } else { upper }
        };
        (
            clamp_axis(horizontal, layout.extent_width, layout.viewport_width),
            clamp_axis(vertical, layout.extent_height, layout.viewport_height),
        )
    }
}

/// A `ViewportInput` for the fake engine: a stretch-None image of `image_width` x `image_height` DIP in a client area.
fn input_for(client_width: f64, client_height: f64, image_width: f64, image_height: f64) -> ViewportInput {
    ViewportInput {
        client_width,
        client_height,
        scroll_bar_thickness: 17.0,
        scroll_bars: ScrollBarPolicy::Auto,
        stretch: ViewerStretchMode::None,
        image_width,
        image_height,
        max_image_width: f64::INFINITY,
        max_image_height: f64::INFINITY,
        bitmap_width: image_width,
        bitmap_height: image_height,
    }
}

// C#: ViewportEngineWpfParityTests.RequestedOffset_IsRememberedAcrossAShrinkAndRegrowOfTheViewport_LikeViewportState
// The C# half runs the real ScrollViewer (W8-01b). Only the state half is ported here, against `FakeEngine`:
// the request 1900 survives a shrink to 1500 (max 1500, so the effective offset is 1500) and comes back at 1900.
#[test]
fn requested_offset_is_remembered_across_shrink_and_regrow_like_viewport_state() {
    let mut state = ViewportState::new(FakeEngine::default());
    state.set_input(input_for(1000.0, 600.0, 3000.0, 2000.0));
    state.update_layout();

    state.scroll_to(1900.0, 0.0);
    state.update_layout();
    assert_eq!(state.horizontal_offset(), 1900.0);

    for width in [1500.0, 1000.0] {
        state.set_input(input_for(width, 600.0, 3000.0, 2000.0));
        state.update_layout();
        let expected = if width == 1500.0 { 1500.0 } else { 1900.0 };
        assert_eq!(state.horizontal_offset(), expected, "client width {width}");
    }
}

// New test: a fresh state is pending; the first update computes and clamps once, a second update calls no engine.
#[test]
fn fresh_state_is_pending_and_update_does_not_compute_twice() {
    let mut state = ViewportState::new(FakeEngine::default());
    assert!(state.is_layout_pending(), "a fresh state has input to lay out");

    state.update_layout();
    assert!(!state.is_layout_pending());
    assert_eq!(state.layout().viewport_width, 0.0);

    let returned_again = state.update_layout();
    assert!(!returned_again, "nothing changed: the second update returns false");
    assert!(!state.is_layout_pending());
}

// New test: `set_input` with an equal input is not dirty, so the next update computes nothing and returns false.
// C# `SetInput` returns early when `_hasInput && input.Equals(_input)` (contract semantics note 4).
#[test]
fn set_input_equal_input_is_not_dirty() {
    let mut state = ViewportState::new(FakeEngine::default());
    let input = input_for(800.0, 600.0, 1200.0, 900.0);
    state.set_input(input);
    state.update_layout();
    assert!(!state.is_layout_pending());

    state.set_input(input);
    assert!(
        !state.is_layout_pending(),
        "an equal input must not mark the state dirty"
    );
    assert!(!state.update_layout(), "no pending work: update returns false");
}

// New test: a scroll request alone is pending but needs no compute: the engine layout is reused and only clamped.
// C# `IsLayoutPending = _inputDirty || _scrollPending`; `UpdateLayout` computes only when the input is dirty.
#[test]
fn scroll_request_alone_skips_compute() {
    let mut state = ViewportState::new(FakeEngine::default());
    state.set_input(input_for(800.0, 600.0, 3000.0, 2000.0));
    state.update_layout();
    assert_eq!(state.layout().max_horizontal_offset, 2200.0);

    state.scroll_to(100.0, 0.0);
    assert!(state.is_layout_pending(), "a scroll request makes the layout pending");
    assert_eq!(
        state.horizontal_offset(),
        0.0,
        "scroll_to changes no visible value before update"
    );

    state.update_layout();
    assert_eq!(state.horizontal_offset(), 100.0);
    assert!(!state.is_layout_pending());
}

// New test: a NaN axis in `scroll_to` keeps the previous request (contract semantics note 3); the other axis is stored.
#[test]
fn nan_scroll_axis_keeps_old_request() {
    let mut state = ViewportState::new(FakeEngine::default());
    state.set_input(input_for(800.0, 600.0, 3000.0, 2000.0));
    state.update_layout();

    state.scroll_to(30.0, 40.0);
    state.update_layout();
    assert_eq!(state.horizontal_offset(), 30.0);
    assert_eq!(state.vertical_offset(), 40.0);

    state.scroll_to(f64::NAN, 50.0);
    state.update_layout();
    assert_eq!(
        state.horizontal_offset(),
        30.0,
        "a NaN horizontal request keeps the old value"
    );
    assert_eq!(state.vertical_offset(), 50.0);
}

// New test: a request made before any input is kept and applied with the first input (no clamp before a layout exists).
#[test]
fn request_before_input_is_applied_with_first_input() {
    let mut state = ViewportState::new(FakeEngine::default());
    state.scroll_to(40.0, 0.0);
    assert!(state.is_layout_pending());
    assert_eq!(state.horizontal_offset(), 0.0);

    state.set_input(input_for(800.0, 600.0, 3000.0, 2000.0));
    state.update_layout();
    assert_eq!(state.horizontal_offset(), 40.0);
    assert_eq!(state.image_origin(), PointD { x: -40.0, y: 0.0 });
}

// New test (Q1 mutation killer): `input()` returns the last accepted input, also before it is laid out
// (C# `ViewportState.Input`). Kills `input -> Default::default()`; a default input differs from this one.
#[test]
fn input_returns_the_last_accepted_input_even_before_layout() {
    let mut state = ViewportState::new(FakeEngine::default());
    let first = input_for(800.0, 600.0, 3000.0, 2000.0);
    state.set_input(first);
    assert_eq!(state.input(), first, "pending input is already visible");
    state.update_layout();
    assert_eq!(state.input(), first);

    let second = input_for(1000.0, 700.0, 3000.0, 2000.0);
    state.set_input(second);
    assert_eq!(
        state.input(),
        second,
        "a changed input replaces the old one before layout"
    );
}

// New test (Q1 mutation killer): `scroll_home` is `scroll_to(0, 0)` (C# `ViewportState.ScrollHome`).
// Kills `scroll_home -> ()`: after a scroll to (100, 50) the home request must bring both offsets back to 0.
#[test]
fn scroll_home_requests_offset_zero_on_both_axes() {
    let mut state = ViewportState::new(FakeEngine::default());
    state.set_input(input_for(800.0, 600.0, 3000.0, 2000.0));
    state.scroll_to(100.0, 50.0);
    state.update_layout();
    assert_eq!(state.horizontal_offset(), 100.0);
    assert_eq!(state.vertical_offset(), 50.0);

    state.scroll_home();
    assert!(state.is_layout_pending(), "scroll_home is a scroll request");
    assert!(state.update_layout(), "both offsets changed");
    assert_eq!(state.horizontal_offset(), 0.0);
    assert_eq!(state.vertical_offset(), 0.0);
}

// New test (Q1 mutation killer): `update_layout` returns true iff the (layout, horizontal, vertical) triple changed,
// each component on its own (C# `return _layout != previous || ... `). Kills the `!=`/`||` mutants of the return value.
#[test]
fn update_layout_return_value_reports_each_component_change_on_its_own() {
    let mut state = ViewportState::new(FakeEngine::default());
    state.set_input(input_for(800.0, 600.0, 3000.0, 2000.0));
    assert!(state.update_layout(), "first layout differs from the default one");

    // Layout only: the client width changes the layout, the (zero) offsets stay.
    state.set_input(input_for(900.0, 600.0, 3000.0, 2000.0));
    assert!(state.update_layout(), "layout changed, offsets did not");
    assert_eq!((state.horizontal_offset(), state.vertical_offset()), (0.0, 0.0));

    // Horizontal offset only.
    state.scroll_to(100.0, 0.0);
    assert!(state.update_layout(), "only the horizontal offset changed");

    // Vertical offset only.
    state.scroll_to(100.0, 40.0);
    assert!(state.update_layout(), "only the vertical offset changed");

    // Nothing changed: the same request again.
    state.scroll_to(100.0, 40.0);
    assert!(!state.update_layout(), "same request, same layout: no change");
}

// New test (Q1 mutation killer): `image_origin` = image_rect origin minus the offsets on BOTH axes, and
// `to_image_element` subtracts that origin (C# `ImageOrigin`, `ToImageElement`). Distinct values per axis so a swapped sign
// or operator on either axis changes the result.
#[test]
fn image_origin_and_to_image_element_use_both_offsets() {
    let mut state = ViewportState::new(FakeEngine::default());
    state.set_input(input_for(800.0, 600.0, 3000.0, 2000.0));
    state.scroll_to(30.0, 40.0);
    state.update_layout();

    assert_eq!(state.image_origin(), PointD { x: -30.0, y: -40.0 });
    assert_eq!(
        state.to_image_element(PointD { x: 100.0, y: 200.0 }),
        PointD { x: 130.0, y: 240.0 },
        "viewport point minus origin = point + offsets"
    );
    assert_eq!(
        state.to_image_element(PointD { x: -30.0, y: -40.0 }),
        PointD { x: 0.0, y: 0.0 }
    );
}

// New test (Q1 mutation killer): the image actual size is the laid-out image rect (C# `ImageActualWidth/Height`).
#[test]
fn image_actual_size_is_the_laid_out_image_rect() {
    let mut state = ViewportState::new(FakeEngine::default());
    assert_eq!((state.image_actual_width(), state.image_actual_height()), (0.0, 0.0));
    state.set_input(input_for(800.0, 600.0, 3000.0, 2000.0));
    state.update_layout();
    assert_eq!(state.image_actual_width(), 3000.0);
    assert_eq!(state.image_actual_height(), 2000.0);
}

// New test (Q1 mutation killer): a scroll request is stored as C# `Math.Max(0.0, value)`: negative values and `-0.0`
// become the literal +0.0 (observable through the sign bit of the effective offset), positive values are kept.
// Kills the `non_negative` mutants (`> -> >=` keeps -0.0, constants, `> -> < / ==`).
#[test]
fn scroll_request_is_max_zero_value_with_positive_zero() {
    let mut state = ViewportState::new(FakeEngine::default());
    state.set_input(input_for(800.0, 600.0, 3000.0, 2000.0));

    state.scroll_to(-0.0, -5.0);
    state.update_layout();
    assert_eq!(
        state.horizontal_offset().to_bits(),
        0.0f64.to_bits(),
        "-0.0 becomes +0.0"
    );
    assert_eq!(state.vertical_offset().to_bits(), 0.0f64.to_bits(), "-5 becomes +0.0");

    state.scroll_to(12.5, 7.25);
    state.update_layout();
    assert_eq!((state.horizontal_offset(), state.vertical_offset()), (12.5, 7.25));
}
