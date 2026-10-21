//! Pixel tests of the scroll-bar painting on the REAL Direct2D draw context (W8-16g): an offscreen WARP surface, pixels read back. Port of
//! `ScrollBarRenderingTests.cs` (4 tests). Client 200x120 DIP, image 600x400 at 100 % (both bars show), canvas `#101010`. No window is created and no
//! desktop state is changed.

#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;
#[path = "common/viewing_scrollbars_fixture.rs"]
mod viewing_scrollbars_fixture;

use pr_app::input::primitives::RectD;
use pr_app::services::render::DrawContext;
use pr_app::viewport::render_geometry::{ColorF, color_from_rgb};
use pr_platform_win::render::surface::D2dRenderSurface;
use viewing_scrollbars_fixture::{RigConfig, ScrollBarRig};

fn rig() -> ScrollBarRig {
    ScrollBarRig::with(RigConfig {
        client_width: 200.0,
        client_height: 120.0,
        image_width: 600,
        image_height: 400,
        ..RigConfig::default()
    })
}

/// Renders the canvas, `before` (e.g. a compare panel) and the bars; returns the rows as BGRA bytes.
struct Pixels {
    bytes: Vec<u8>,
    width: usize,
}

impl Pixels {
    /// (r, g, b) at the DIP point (1 DIP = 1 pixel).
    fn at(&self, x: f64, y: f64) -> (i32, i32, i32) {
        let offset = (y as usize * self.width + x as usize) * 4;
        (
            i32::from(self.bytes[offset + 2]),
            i32::from(self.bytes[offset + 1]),
            i32::from(self.bytes[offset]),
        )
    }
}

fn render(rig: &ScrollBarRig, before: impl FnOnce(&mut dyn DrawContext)) -> Pixels {
    let mut surface = D2dRenderSurface::create_offscreen_surface(200, 120, 1.0, false).unwrap();
    let width = surface.pixel_width() as usize;
    let context = surface.begin_draw_context().unwrap();
    context.clear(color_from_rgb(0x10, 0x10, 0x10, 255));
    before(context);
    rig.bars().draw(context).unwrap();
    let pixels = context.try_read_pixels().unwrap().unwrap();
    let bytes = pixels.bytes().unwrap().to_vec();
    surface.end_draw_and_present().unwrap();
    Pixels { bytes, width }
}

fn assert_gray(expected: i32, actual: (i32, i32, i32), what: &str) {
    assert!(
        (actual.0 - expected).abs() <= 1 && (actual.1 - expected).abs() <= 1 && (actual.2 - expected).abs() <= 1,
        "{what}: expected gray {expected}, found {actual:?}"
    );
}

// C#: ScrollBarRenderingTests.RendersTrackThumbCornerAndLeavesTheRestUntouched
#[test]
#[ignore = "native: needs desktop"]
fn renders_track_thumb_corner_and_leaves_the_rest_untouched() {
    let rig = rig();
    let layout = rig.layout();
    assert!(layout.vertical.visible && layout.horizontal.visible);
    let vertical_thumb = layout.vertical.thumb;
    let horizontal_thumb = layout.horizontal.thumb;

    let pixels = render(&rig, |_| {});

    assert_gray(0x10, pixels.at(100.0, 50.0), "canvas outside the bars");
    assert_gray(
        0x16,
        pixels.at(layout.vertical.bar.x + 0.5, 3.0),
        "vertical bar (track's left margin)",
    );
    assert_gray(
        0x16,
        pixels.at(layout.vertical.bar.x + 5.0, layout.vertical.track.bottom() + 0.5),
        "vertical bar (bottom margin)",
    );
    assert_gray(
        0x4A,
        pixels.at(
            layout.vertical.bar.x + 5.0,
            vertical_thumb.y + vertical_thumb.height / 2.0,
        ),
        "vertical thumb",
    );
    assert_gray(
        0x16,
        pixels.at(layout.vertical.bar.x + 5.0, vertical_thumb.bottom() + 5.0),
        "track below the vertical thumb",
    );
    assert_gray(
        0x4A,
        pixels.at(
            horizontal_thumb.x + horizontal_thumb.width / 2.0,
            layout.horizontal.bar.y + 5.0,
        ),
        "horizontal thumb",
    );
    assert_gray(
        0x16,
        pixels.at(horizontal_thumb.right() + 8.0, layout.horizontal.bar.y + 5.0),
        "track after the horizontal thumb",
    );
    assert_gray(0x16, pixels.at(layout.corner.x + 5.0, layout.corner.y + 5.0), "corner");
}

// C#: ScrollBarRenderingTests.ThumbTurnsHoverGrayWhenTheCursorIsOverIt_AndPressedGrayWhileDragging
#[test]
#[ignore = "native: needs desktop"]
fn thumb_turns_hover_gray_when_the_cursor_is_over_it_and_pressed_gray_while_dragging() {
    let rig = rig();
    let thumb = rig.layout().vertical.thumb;
    let x = thumb.x + 4.0;
    let y = thumb.y + thumb.height / 2.0;

    rig.move_to(x, y);
    assert_gray(0x6A, render(&rig, |_| {}).at(x, y), "hover");

    rig.down(x, y);
    assert_gray(0x7A, render(&rig, |_| {}).at(x, y), "dragging");

    rig.up(x, y);
    rig.move_to(20.0, 20.0);
    assert_gray(0x4A, render(&rig, |_| {}).at(x, y), "after leaving");
}

// C#: ScrollBarRenderingTests.TheThumbHasRoundedEnds_AndTheSquareCornersOfItsBoxStayTrackColored
#[test]
#[ignore = "native: needs desktop"]
fn the_thumb_has_rounded_ends_and_the_square_corners_of_its_box_stay_track_colored() {
    let rig = rig();
    let thumb = rig.layout().vertical.thumb;
    let pixels = render(&rig, |_| {});

    // Radius 4 on an 8 wide thumb: the box's top-left corner (0.5 DIP from the edge) is not covered by the thumb colour.
    assert_gray(
        0x16,
        pixels.at(thumb.x + 0.5, thumb.y + 0.5),
        "top corner of the thumb's box",
    );
    assert_gray(
        0x4A,
        pixels.at(thumb.x + 4.0, thumb.y + 4.0),
        "centre of the rounded end",
    );
}

// C#: ScrollBarRenderingTests.WithTheComparePanelVisible_OnlyTheStripsOutsideItsMarginShowTheBars
#[test]
#[ignore = "native: needs desktop"]
fn with_the_compare_panel_visible_only_the_strips_outside_its_margin_show_the_bars() {
    let rig = rig();
    rig.compare_visible.set(true);
    let layout = rig.layout();
    let thumb = layout.vertical.thumb;

    let pixels = render(&rig, |context| {
        // The compare panel (margin 8) has already covered everything inside it.
        context.fill_rectangle(
            RectD {
                x: 8.0,
                y: 8.0,
                width: 184.0,
                height: 104.0,
            },
            ColorF {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        );
    });

    // Inside the panel (x < 192): still the panel colour although the bar sits at x 190..200.
    assert_eq!(
        pixels.at(layout.vertical.bar.x + 1.0, thumb.y + thumb.height / 2.0),
        (255, 0, 0)
    );
    // Outside the panel (x >= 192): the bars show.
    assert_gray(
        0x4A,
        pixels.at(195.0, thumb.y + thumb.height / 2.0),
        "thumb strip on the right",
    );
    assert_gray(0x16, pixels.at(100.0, 115.0), "strip below the horizontal bar");
}
