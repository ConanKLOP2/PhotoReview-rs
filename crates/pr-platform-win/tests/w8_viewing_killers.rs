//! Mutation killers of lane cl-viewing for `shell_viewing::{compare, crossfade, composer}` (pure logic only, no window, no sleeps).
//! Each test names the mutant (file:line, operator) it kills. Evidence: docs/reviews/CL-VIEWING-2026-10-30.md.
#![cfg(windows)]

#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::sync::Arc;

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::services::render::{DrawContext, GpuImage, RenderError, TextLayout};
use pr_app::viewport::render_geometry::{ColorF, ImageInterpolation};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::shell_viewing::compare::{self, CompareFrame, CompareSide, layout};
use viewing_fakes::FakeCache;

/// One recorded draw call with its full arguments (the shared `RecordingContext` drops colours, rectangles and opacity).
#[derive(Debug, Clone, PartialEq)]
enum Call {
    Fill(RectD, ColorF),
    Push(RectD),
    Pop,
    Image(RectD, f32),
}

#[derive(Default)]
struct Rec {
    calls: Vec<Call>,
}

impl DrawContext for Rec {
    fn clear(&mut self, _: ColorF) {}
    fn draw_image(&mut self, _: &dyn GpuImage, dest: RectD, _: Option<RectD>, _: ImageInterpolation, opacity: f32) {
        self.calls.push(Call::Image(dest, opacity));
    }
    fn fill_rectangle(&mut self, rect: RectD, color: ColorF) {
        self.calls.push(Call::Fill(rect, color));
    }
    fn fill_rounded_rectangle(&mut self, _: RectD, _: f64, _: ColorF) {}
    fn draw_rectangle(&mut self, _: RectD, _: ColorF, _: f64) {}
    fn draw_text(&mut self, _: &dyn TextLayout, _: PointD, _: ColorF) -> Result<(), RenderError> {
        Ok(())
    }
    fn push_clip(&mut self, rect: RectD) {
        self.calls.push(Call::Push(rect));
    }
    fn pop_clip(&mut self) -> Result<(), RenderError> {
        self.calls.push(Call::Pop);
        Ok(())
    }
    fn push_opacity(&mut self, _: f32) {}
    fn pop_opacity(&mut self) -> Result<(), RenderError> {
        Ok(())
    }
    fn try_read_pixels(&mut self) -> Result<Option<PixelBuffer>, RenderError> {
        Ok(None)
    }
}

fn r(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}
fn size(width: f64, height: f64) -> SizeD {
    SizeD { width, height }
}
fn buffer(width: i32, height: i32) -> Arc<PixelBuffer> {
    Arc::new(PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap())
}
fn draw_frame(client: SizeD, frame: &CompareFrame<'_>) -> (i32, Vec<Call>) {
    let mut context = Rec::default();
    let drawn = compare::draw(
        &FakeCache::default(),
        &mut context,
        client,
        frame,
        ImageInterpolation::Linear,
    )
    .unwrap();
    (drawn, context.calls)
}
/// The four fills of a border of `border` with thickness `t` (top, bottom, left bar, right bar), `inner` = height - 2t.
fn border_fills(border: RectD, t: f64, color: ColorF) -> Vec<Call> {
    let inner = border.height - 2.0 * t;
    vec![
        Call::Fill(r(border.x, border.y, border.width, t), color),
        Call::Fill(r(border.x, border.y + border.height - t, border.width, t), color),
        Call::Fill(r(border.x, border.y + t, t, inner), color),
        Call::Fill(r(border.x + border.width - t, border.y + t, t, inner), color),
    ]
}

// ---------- compare.rs: layout ----------

// compare.rs:96-110 `panel` / `net_max` (`-`, `*`, `max(0, ..)` replacements, the NaN arm `||` -> `&&`): clamped and NaN panels.
#[test]
fn compare_panel_clamps_negative_sizes_and_propagates_a_single_nan() {
    assert_eq!(layout::panel(size(10.0, 12.0)), r(8.0, 8.0, 0.0, 0.0));
    assert_eq!(layout::panel(size(16.0, 17.0)), r(8.0, 8.0, 0.0, 1.0));
    assert_eq!(layout::panel(size(1000.0, 600.0)), r(8.0, 8.0, 984.0, 584.0));
    // Only the width is NaN: `a.is_nan() || b.is_nan()` must give NaN (a `&&` would give max(0, NaN) = 0).
    let nan_width = layout::panel(size(f64::NAN, 100.0));
    assert!(nan_width.width.is_nan());
    assert_eq!(nan_width.height, 84.0);
    let nan_height = layout::panel(size(100.0, f64::NAN));
    assert_eq!(nan_height.width, 84.0);
    assert!(nan_height.height.is_nan());
}

// compare.rs:114-126 `border` (`/ 2.0`, `+ column`, `== Right`, `2.0 * CELL_MARGIN`, clamps) at an odd client size.
#[test]
fn compare_border_exact_values_on_an_odd_client_and_both_clamps() {
    // panel 984 x 584 -> column 492 -> cell inset 4 on every side.
    assert_eq!(
        layout::border(size(1000.0, 600.0), CompareSide::Left),
        r(12.0, 12.0, 484.0, 576.0)
    );
    assert_eq!(
        layout::border(size(1000.0, 600.0), CompareSide::Right),
        r(504.0, 12.0, 484.0, 576.0)
    );
    // Width clamp: column 11 -> 3 wide; column 6 -> 0 (not -2). Height clamp: panel height 5 -> 0 (not -3).
    assert_eq!(
        layout::border(size(38.0, 100.0), CompareSide::Left),
        r(12.0, 12.0, 3.0, 76.0)
    );
    assert_eq!(
        layout::border(size(28.0, 100.0), CompareSide::Right),
        r(18.0, 12.0, 0.0, 76.0)
    );
    assert_eq!(
        layout::border(size(100.0, 21.0), CompareSide::Left),
        r(12.0, 12.0, 34.0, 0.0)
    );
}

// compare.rs:130-137 `content` (`+ BORDER_THICKNESS`, `2.0 * BORDER_THICKNESS`, `max(0, ..)`).
#[test]
fn compare_content_exact_values_and_clamps() {
    assert_eq!(
        layout::content(size(1000.0, 600.0), CompareSide::Left),
        r(14.0, 14.0, 480.0, 572.0)
    );
    assert_eq!(
        layout::content(size(1000.0, 600.0), CompareSide::Right),
        r(506.0, 14.0, 480.0, 572.0)
    );
    // Border 3 x 76 -> content width clamped to 0 (3 - 4 = -1).
    assert_eq!(
        layout::content(size(38.0, 100.0), CompareSide::Left),
        r(14.0, 14.0, 0.0, 72.0)
    );
    // Border 34 x 3 -> content height clamped to 0.
    assert_eq!(
        layout::content(size(100.0, 27.0), CompareSide::Left),
        r(14.0, 14.0, 30.0, 0.0)
    );
}

// compare.rs:146 `pixel_width <= 0`, `pixel_height <= 0` (`<=` -> `<`, `||` -> `&&`) and the content guard `content.width > 0 && content.height > 0`
// (`>` -> `>=`, `&&` -> `||`): the empty rectangle sits exactly at the content origin.
#[test]
fn compare_image_empty_cases_return_the_content_origin_with_zero_size() {
    let client = size(1200.0, 800.0);
    let origin = r(14.0, 14.0, 0.0, 0.0);
    assert_eq!(layout::image(client, CompareSide::Left, 0, 20), origin);
    assert_eq!(layout::image(client, CompareSide::Left, 20, 0), origin);
    assert_eq!(layout::image(client, CompareSide::Left, -5, 20), origin);
    assert_eq!(layout::image(client, CompareSide::Left, 20, -5), origin);
    assert_eq!(
        layout::image(client, CompareSide::Right, 0, 0),
        r(606.0, 14.0, 0.0, 0.0)
    );
    // Content width exactly 0 (border 4 wide), height positive.
    assert_eq!(
        layout::image(size(40.0, 100.0), CompareSide::Left, 10, 10),
        r(14.0, 14.0, 0.0, 0.0)
    );
    // Content height exactly 0 (border 4 high), width positive.
    assert_eq!(
        layout::image(size(100.0, 28.0), CompareSide::Left, 10, 10),
        r(14.0, 14.0, 0.0, 0.0)
    );
}

// compare.rs:146-165 `image`: square image in a wide cell, centred by `(content - size) / 2` in x; and a Right-side image (x offset of the column).
#[test]
fn compare_image_centres_in_both_axes_and_on_the_right_column() {
    // Content 480 x 572 (Left of 1000 x 600): 8 x 8 -> scale 60 -> 480 x 480, y = 14 + (572 - 480) / 2 = 60.
    assert_eq!(
        layout::image(size(1000.0, 600.0), CompareSide::Left, 8, 8),
        r(14.0, 60.0, 480.0, 480.0)
    );
    assert_eq!(
        layout::image(size(1000.0, 600.0), CompareSide::Right, 8, 8),
        r(506.0, 60.0, 480.0, 480.0)
    );
    // 8 x 4 (wide): scale 60 -> 480 x 240, y = 14 + (572 - 240) / 2 = 180.
    assert_eq!(
        layout::image(size(1000.0, 600.0), CompareSide::Left, 8, 4),
        r(14.0, 180.0, 480.0, 240.0)
    );
    // 4 x 8 (tall) in a 480 x 572 cell: scale = min(120, 71.5) = 71.5 -> 286 x 572, x = 14 + (480 - 286) / 2 = 111.
    assert_eq!(
        layout::image(size(1000.0, 600.0), CompareSide::Left, 4, 8),
        r(111.0, 14.0, 286.0, 572.0)
    );
}

// compare.rs:170-178 `hit_test` (`&&` -> `||`, `>` -> `>=` on a zero-width border: the point on its edge must not hit).
#[test]
fn compare_hit_test_ignores_zero_width_and_zero_height_borders() {
    // Border widths 0 (column 8), height 76.
    let client = size(32.0, 100.0);
    assert_eq!(layout::border(client, CompareSide::Left).width, 0.0);
    assert_eq!(layout::hit_test(client, PointD { x: 12.0, y: 50.0 }), None);
    assert_eq!(layout::hit_test(client, PointD { x: 20.0, y: 50.0 }), None);
    // Border heights 0 (panel height 8), width positive.
    let flat = size(200.0, 24.0);
    assert_eq!(layout::border(flat, CompareSide::Left).height, 0.0);
    assert_eq!(layout::hit_test(flat, PointD { x: 50.0, y: 12.0 }), None);
    assert_eq!(layout::hit_test(flat, PointD { x: 150.0, y: 12.0 }), None);
}

// compare.rs:170-178 `hit_test` returns Left before Right and each side only inside its own border.
#[test]
fn compare_hit_test_picks_the_side_of_the_point() {
    let client = size(1000.0, 600.0);
    assert_eq!(
        layout::hit_test(client, PointD { x: 100.0, y: 300.0 }),
        Some(CompareSide::Left)
    );
    assert_eq!(
        layout::hit_test(client, PointD { x: 600.0, y: 300.0 }),
        Some(CompareSide::Right)
    );
    assert_eq!(
        layout::hit_test(client, PointD { x: 496.0, y: 300.0 }),
        Some(CompareSide::Left)
    );
    assert_eq!(layout::hit_test(client, PointD { x: 497.0, y: 300.0 }), None);
    assert_eq!(layout::hit_test(client, PointD { x: 503.0, y: 300.0 }), None);
    assert_eq!(
        layout::hit_test(client, PointD { x: 504.0, y: 300.0 }),
        Some(CompareSide::Right)
    );
    assert_eq!(layout::hit_test(client, PointD { x: 100.0, y: 11.0 }), None);
    assert_eq!(
        layout::hit_test(client, PointD { x: 100.0, y: 12.0 }),
        Some(CompareSide::Left)
    );
    assert_eq!(
        layout::hit_test(client, PointD { x: 100.0, y: 588.0 }),
        Some(CompareSide::Left)
    );
    assert_eq!(layout::hit_test(client, PointD { x: 100.0, y: 589.0 }), None);
}

// ---------- compare.rs: draw ----------

// compare.rs:196-211 `draw`, 215-243 `draw_cell`, 247-285 `draw_border`: the exact call sequence (rectangles, colours, order, clip, opacity),
// left selected / right not. Kills the arithmetic of the four border bars, the selected/unselected choice and the count `left + right`.
#[test]
fn compare_draw_exact_call_sequence_left_selected() {
    let image = buffer(8, 8);
    let frame = CompareFrame {
        left: Some(&image),
        right: Some(&image),
        left_selected: true,
        right_selected: false,
    };
    let client = size(1000.0, 600.0);
    let (drawn, calls) = draw_frame(client, &frame);
    assert_eq!(drawn, 2);
    let mut expected = vec![Call::Fill(r(8.0, 8.0, 984.0, 584.0), compare::canvas())];
    expected.extend(border_fills(
        r(12.0, 12.0, 484.0, 576.0),
        2.0,
        compare::selected_color(),
    ));
    expected.push(Call::Push(r(14.0, 14.0, 480.0, 572.0)));
    expected.push(Call::Image(r(14.0, 60.0, 480.0, 480.0), 1.0));
    expected.push(Call::Pop);
    expected.extend(border_fills(
        r(504.0, 12.0, 484.0, 576.0),
        2.0,
        compare::unselected_color(),
    ));
    expected.push(Call::Push(r(506.0, 14.0, 480.0, 572.0)));
    expected.push(Call::Image(r(506.0, 60.0, 480.0, 480.0), 1.0));
    expected.push(Call::Pop);
    assert_eq!(calls, expected);
}

// Same with the selection swapped, and only one image present at a time (count 1 for either side: `left + right` with `+` -> `*` gives 0).
#[test]
fn compare_draw_right_selected_and_single_images_count_one() {
    let image = buffer(8, 8);
    let client = size(1000.0, 600.0);
    let right_only = CompareFrame {
        left: None,
        right: Some(&image),
        left_selected: false,
        right_selected: true,
    };
    let (drawn, calls) = draw_frame(client, &right_only);
    assert_eq!(drawn, 1);
    let mut expected = vec![Call::Fill(r(8.0, 8.0, 984.0, 584.0), compare::canvas())];
    expected.extend(border_fills(
        r(12.0, 12.0, 484.0, 576.0),
        2.0,
        compare::unselected_color(),
    ));
    expected.extend(border_fills(
        r(504.0, 12.0, 484.0, 576.0),
        2.0,
        compare::selected_color(),
    ));
    expected.push(Call::Push(r(506.0, 14.0, 480.0, 572.0)));
    expected.push(Call::Image(r(506.0, 60.0, 480.0, 480.0), 1.0));
    expected.push(Call::Pop);
    assert_eq!(calls, expected);

    let left_only = CompareFrame {
        left: Some(&image),
        right: None,
        left_selected: false,
        right_selected: false,
    };
    assert_eq!(draw_frame(client, &left_only).0, 1);
}

// compare.rs:196-200 `draw`: an empty panel (width or height exactly 0, only one of them) draws nothing (`>` -> `>=`, `&&` -> `||`).
#[test]
fn compare_draw_panel_with_a_single_zero_side_draws_nothing() {
    let empty = CompareFrame::empty();
    assert_eq!(draw_frame(size(16.0, 100.0), &empty), (0, vec![]));
    assert_eq!(draw_frame(size(100.0, 16.0), &empty), (0, vec![]));
    assert_eq!(draw_frame(size(10.0, 10.0), &empty), (0, vec![]));
}

// compare.rs:221-224 `draw_cell`: a cell whose border is empty (width 0 or height 0) draws no border and no image, but the panel is filled
// (`>` -> `>=`, `&&` -> `||`).
#[test]
fn compare_draw_cells_with_an_empty_border_draw_only_the_panel() {
    let image = buffer(8, 8);
    let frame = CompareFrame {
        left: Some(&image),
        right: Some(&image),
        left_selected: true,
        right_selected: true,
    };
    // Border width 0 (panel 16 wide), height positive.
    let (drawn, calls) = draw_frame(size(32.0, 100.0), &frame);
    assert_eq!(drawn, 0);
    assert_eq!(calls, vec![Call::Fill(r(8.0, 8.0, 16.0, 84.0), compare::canvas())]);
    // Border height 0 (panel 8 high), width positive.
    let (drawn, calls) = draw_frame(size(200.0, 24.0), &frame);
    assert_eq!(drawn, 0);
    assert_eq!(calls, vec![Call::Fill(r(8.0, 8.0, 184.0, 8.0), compare::canvas())]);
}

// compare.rs:255-285 `draw_border`: thickness `min(2, min(w, h) / 2)`, `inner > 0` (`>` -> `>=`), `2.0 * thickness` (`*` -> `+`), bar positions.
#[test]
fn compare_draw_border_tiny_cells_have_exact_bars() {
    let frame = CompareFrame::empty();
    // Border 14 x 3: thickness 1.5, inner = 0 exactly -> only the top and bottom bars (a `>=` would add two zero-height bars).
    let (_, calls) = draw_frame(size(60.0, 27.0), &frame);
    let border = layout::border(size(60.0, 27.0), CompareSide::Left);
    assert_eq!(border, r(12.0, 12.0, 14.0, 3.0));
    assert_eq!(calls[0], Call::Fill(r(8.0, 8.0, 44.0, 11.0), compare::canvas()));
    assert_eq!(
        calls[1..3],
        [
            Call::Fill(r(12.0, 12.0, 14.0, 1.5), compare::unselected_color()),
            Call::Fill(r(12.0, 13.5, 14.0, 1.5), compare::unselected_color()),
        ]
    );
    assert_eq!(
        calls[3],
        Call::Fill(r(34.0, 12.0, 14.0, 1.5), compare::unselected_color())
    );
    assert_eq!(calls.len(), 1 + 2 + 2);

    // Border 4 x 20: thickness min(2, 2) = 2; inner = 16.
    let (_, calls) = draw_frame(size(40.0, 44.0), &frame);
    assert_eq!(
        layout::border(size(40.0, 44.0), CompareSide::Left),
        r(12.0, 12.0, 4.0, 20.0)
    );
    assert_eq!(
        calls[1..5],
        border_fills(r(12.0, 12.0, 4.0, 20.0), 2.0, compare::unselected_color())[..]
    );

    // Border 1.5 x 3 -> thickness 0.75, inner 1.5 (positive): all four bars, exact.
    let (_, calls) = draw_frame(size(35.0, 27.0), &frame);
    assert_eq!(
        calls[1..5],
        border_fills(r(12.0, 12.0, 1.5, 3.0), 0.75, compare::unselected_color())[..]
    );
}

// ---------- crossfade.rs ----------

use pr_platform_win::shell_viewing::crossfade::CrossfadeLayer;
use std::rc::Rc;
use viewing_fakes::FakeClock;

fn fade_layer() -> CrossfadeLayer {
    CrossfadeLayer::new(Rc::new(FakeClock::default()), Rc::new(|| {}), 1_000)
}

// crossfade.rs:140 `set_opacity` -> `()`: the setter must be observable through the getter.
#[test]
fn crossfade_set_opacity_is_read_back() {
    let layer = fade_layer();
    assert_eq!(layer.opacity(), 0.0);
    layer.set_opacity(0.25);
    assert_eq!(layer.opacity(), 0.25);
    layer.set_opacity(1.0);
    assert_eq!(layer.opacity(), 1.0);
}

// crossfade.rs:96 `Debug::fmt` -> `Ok(Default::default())` (writes nothing).
#[test]
fn crossfade_debug_names_the_type() {
    assert_eq!(format!("{:?}", fade_layer()), "CrossfadeLayer { .. }");
}

/// A cache whose upload finds the CPU buffer disposed (`try_draw` turns it into `Ok(false)`).
struct DisposedCache;
impl pr_app::services::render::GpuImageCache for DisposedCache {
    fn get_or_upload(&self, _: &PixelBuffer) -> Result<Rc<dyn GpuImage>, RenderError> {
        Err(RenderError::Disposed { object: "PixelBuffer" })
    }
    fn prefetch(&self, _: &Arc<PixelBuffer>) -> Result<(), RenderError> {
        Ok(())
    }
    fn retain(&self, _: &[i64]) -> Result<(), RenderError> {
        Ok(())
    }
    fn budget_bytes(&self) -> i64 {
        0
    }
    fn set_budget_bytes(&self, _: i64) -> Result<(), RenderError> {
        Ok(())
    }
    fn clear(&self) {}
}

fn started(clock: &Rc<FakeClock>) -> CrossfadeLayer {
    let layer = CrossfadeLayer::new(clock.clone(), Rc::new(|| {}), 1_000);
    let frame = r(1.0, 2.0, 60.0, 50.0);
    assert_eq!(
        layer.start(
            buffer(8, 8),
            frame,
            ImageInterpolation::Linear,
            std::time::Duration::from_millis(200)
        ),
        Ok(true)
    );
    layer
}

// crossfade.rs:245:35 `||` -> `&&`, :54 `<=` -> `>` (opacity test): the tween ended with opacity 0 but the image still held (the frame
// that wrote 0 changed nothing because the value was already 0): `draw` must release it and report false.
#[test]
fn crossfade_draw_releases_an_idle_layer_whose_opacity_is_zero() {
    let clock = Rc::new(FakeClock::default());
    let layer = started(&clock);
    clock.tick(0);
    layer.set_opacity(0.0);
    clock.tick(300); // the tween ends; value 0 equals the stored 0, so nothing "changed" and nothing was released
    assert!(layer.is_active());
    assert_eq!(clock.subscribers(), 0);
    let mut context = Rec::default();
    assert_eq!(
        layer.draw(&FakeCache::default(), &mut context, r(0.0, 0.0, 100.0, 100.0)),
        Ok(false)
    );
    assert!(!layer.is_active());
    assert!(context.calls.is_empty());
}

// crossfade.rs:245:61 `&&` -> `||` and :64 `delete !`: while the tween still runs, opacity 0 is "not drawn yet", NOT released.
#[test]
fn crossfade_draw_keeps_an_animating_layer_whose_opacity_is_zero() {
    let clock = Rc::new(FakeClock::default());
    let layer = started(&clock);
    clock.tick(0);
    layer.set_opacity(0.0);
    let mut context = Rec::default();
    assert_eq!(
        layer.draw(&FakeCache::default(), &mut context, r(0.0, 0.0, 100.0, 100.0)),
        Ok(false)
    );
    assert!(layer.is_active());
    assert_eq!(clock.subscribers(), 1);
    assert!(context.calls.is_empty());
}

// crossfade.rs:265:12 `delete !` (`if !drawn { cancel }`): a successful draw leaves the layer active with the exact clip and image calls;
// a draw the cache reports as "buffer disposed" cancels the layer (pop still runs).
#[test]
fn crossfade_draw_cancels_only_when_nothing_was_drawn() {
    let clock = Rc::new(FakeClock::default());
    let layer = started(&clock);
    let viewport = r(0.0, 0.0, 100.0, 100.0);
    let mut context = Rec::default();
    assert_eq!(layer.draw(&FakeCache::default(), &mut context, viewport), Ok(true));
    assert!(layer.is_active());
    assert_eq!(clock.subscribers(), 1);
    assert_eq!(
        context.calls,
        vec![
            Call::Push(viewport),
            Call::Image(r(1.0, 2.0, 60.0, 50.0), 1.0),
            Call::Pop
        ]
    );

    let mut context = Rec::default();
    assert_eq!(layer.draw(&DisposedCache, &mut context, viewport), Ok(false));
    assert!(!layer.is_active());
    assert_eq!(clock.subscribers(), 0);
    assert_eq!(context.calls, vec![Call::Push(viewport), Call::Pop]);
}
