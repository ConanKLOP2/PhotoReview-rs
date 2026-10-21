//! Port of `PhotoReview.Shell.Win32/Viewing/CompareLayer.cs` (166 lines, WP-21, baseline `d92d6aae`). Unit W8-16b (contract `contracts/W8-16b.md`).
//!
//! The compare panel of the main window (C# `ComparePanel` of `MainWindow.xaml`): [`layout`] holds the pure geometry (C# `CompareLayout`, DIP, origin at the
//! client corner) and [`draw`] paints the canvas panel, the two cell borders (lime green when selected, `#555555` otherwise) and the two uniform images
//! (C# `CompareLayer.Draw`). Images go through `pr_app::viewing::image_layer::try_draw` and the GPU cache, keyed by `PixelBuffer::id()` (INV-1).
//!
//! UI thread only. Every C# `Math.Max`/`Math.Min` keeps its .NET NaN semantics (a NaN operand wins), so the porter writes the two private helpers
//! `net_max`/`net_min` (the `pr_app` ones are private).

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::services::render::{DrawContext, GpuImageCache, RenderError};
use pr_app::viewing::image_layer::try_draw;
use pr_app::viewmodels::compare::CompareViewModel;
use pr_app::viewport::render_geometry::{ColorF, ImageInterpolation, color_from_rgb};
use pr_imaging::decode_types::DecodedImageView;
use pr_imaging::pixels::PixelBuffer;

/// C# `CompareSide`: `Left = 0`, `Right = 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompareSide {
    Left = 0,
    Right = 1,
}

/// C# `CompareFrame(PixelBuffer? Left, PixelBuffer? Right, bool LeftSelected, bool RightSelected)`: two images (`None` while loading) and the
/// selected cell. Borrowed: the frame lives for one `draw` call, the owner of the buffers is the view model / the composer.
#[derive(Debug, Clone, Copy)]
pub struct CompareFrame<'a> {
    pub left: Option<&'a PixelBuffer>,
    pub right: Option<&'a PixelBuffer>,
    pub left_selected: bool,
    pub right_selected: bool,
}

impl CompareFrame<'_> {
    /// No images, nothing selected (the `default(CompareFrame)` of C#).
    pub fn empty() -> CompareFrame<'static> {
        CompareFrame {
            left: None,
            right: None,
            left_selected: false,
            right_selected: false,
        }
    }

    /// The frame `ShellMainWindow.DrawPresentation` builds from the view model (C# `compare.LeftImage as PixelBuffer`, `IsLeftSelected`, ...):
    /// `left_image()` -> `DecodedImageView::platform_image().as_pixels()` (`None` for an opaque image), the two selection flags verbatim.
    pub fn from_view_model(compare: &CompareViewModel) -> CompareFrame<'_> {
        CompareFrame {
            left: compare
                .left_image()
                .and_then(|image| image.platform_image().as_pixels())
                .map(AsRef::as_ref),
            right: compare
                .right_image()
                .and_then(|image| image.platform_image().as_pixels())
                .map(AsRef::as_ref),
            left_selected: compare.is_left_selected(),
            right_selected: compare.is_right_selected(),
        }
    }
}

/// C# `CompareLayer.Canvas` = `#101010`, opaque.
pub fn canvas() -> ColorF {
    color_from_rgb(0x10, 0x10, 0x10, 0xFF)
}

/// C# `CompareLayer.Selected` = `Brushes.LimeGreen` `#32CD32`.
pub fn selected_color() -> ColorF {
    color_from_rgb(0x32, 0xCD, 0x32, 0xFF)
}

/// C# `CompareLayer.Unselected` = `#555555`.
pub fn unselected_color() -> ColorF {
    color_from_rgb(0x55, 0x55, 0x55, 0xFF)
}

/// C# `CompareLayout` (static class, pure): the XAML layout of `ComparePanel` in DIP.
pub mod layout {
    use super::{CompareSide, PointD, RectD, SizeD};

    /// C# `PanelMargin` = `ComparePanel Margin="8"`.
    pub const PANEL_MARGIN: f64 = 8.0;
    /// C# `CellMargin` = `CompareLeftBorder Margin="4"`.
    pub const CELL_MARGIN: f64 = 4.0;
    /// C# `BorderThickness` = 2.
    pub const BORDER_THICKNESS: f64 = 2.0;

    fn net_max(a: f64, b: f64) -> f64 {
        if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
    }

    fn net_min(a: f64, b: f64) -> f64 {
        if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
    }

    /// C# `Panel(client)`: `(8, 8, max(0, w - 16), max(0, h - 16))`.
    pub fn panel(client: SizeD) -> RectD {
        RectD {
            x: PANEL_MARGIN,
            y: PANEL_MARGIN,
            width: net_max(0.0, client.width - (2.0 * PANEL_MARGIN)),
            height: net_max(0.0, client.height - (2.0 * PANEL_MARGIN)),
        }
    }

    /// C# `Border(client, side)`: the panel split into two equal columns, each cell inset by 4 on every side (the outer box including the 2 DIP border).
    pub fn border(client: SizeD, side: CompareSide) -> RectD {
        let panel = panel(client);
        let column = panel.width / 2.0;
        let cell_x = panel.x + if side == CompareSide::Right { column } else { 0.0 };
        RectD {
            x: cell_x + CELL_MARGIN,
            y: panel.y + CELL_MARGIN,
            width: net_max(0.0, column - (2.0 * CELL_MARGIN)),
            height: net_max(0.0, panel.height - (2.0 * CELL_MARGIN)),
        }
    }

    /// C# `Content(client, side)`: `border` inset by the 2 DIP border thickness.
    pub fn content(client: SizeD, side: CompareSide) -> RectD {
        let border = border(client, side);
        RectD {
            x: border.x + BORDER_THICKNESS,
            y: border.y + BORDER_THICKNESS,
            width: net_max(0.0, border.width - (2.0 * BORDER_THICKNESS)),
            height: net_max(0.0, border.height - (2.0 * BORDER_THICKNESS)),
        }
    }

    /// C# `Image(client, side, pixelWidth, pixelHeight)`: `Stretch=Uniform` inside `content`, centred; an empty rectangle at the content origin when the
    /// bitmap or the content is empty (`<= 0`).
    pub fn image(client: SizeD, side: CompareSide, pixel_width: i32, pixel_height: i32) -> RectD {
        let content = content(client, side);
        if pixel_width <= 0 || pixel_height <= 0 || !(content.width > 0.0 && content.height > 0.0) {
            return RectD {
                x: content.x,
                y: content.y,
                width: 0.0,
                height: 0.0,
            };
        }
        let scale = net_min(
            content.width / f64::from(pixel_width),
            content.height / f64::from(pixel_height),
        );
        let width = f64::from(pixel_width) * scale;
        let height = f64::from(pixel_height) * scale;
        RectD {
            x: content.x + ((content.width - width) / 2.0),
            y: content.y + ((content.height - height) / 2.0),
            width,
            height,
        }
    }

    /// C# `HitTest(client, point)`: the cell whose `border` (borders included, gap and margin excluded) contains the point; Left is tested first.
    pub fn hit_test(client: SizeD, point: PointD) -> Option<CompareSide> {
        for side in [CompareSide::Left, CompareSide::Right] {
            let rect = border(client, side);
            if rect.width > 0.0 && rect.height > 0.0 && rect.contains(point) {
                return Some(side);
            }
        }
        None
    }
}

/// C# `CompareLayer.Draw(cache, context, client, frame, interpolation)`: fills the panel with the canvas colour, then draws the left and the right
/// cell (border first, then the image under a clip to the cell content). Returns the number of images actually drawn (0..=2). An empty panel draws
/// nothing and returns 0. A disposed or missing buffer still gets its border. `Err` only when the GPU cache or the draw context fails (a disposed
/// buffer or an upload that finds the buffer disposed is `Ok(false)` inside `try_draw`, not an error); `pop_clip` runs even when the draw fails.
pub fn draw(
    cache: &dyn GpuImageCache,
    context: &mut dyn DrawContext,
    client: SizeD,
    frame: &CompareFrame<'_>,
    interpolation: ImageInterpolation,
) -> Result<i32, RenderError> {
    let panel = layout::panel(client);
    if !(panel.width > 0.0 && panel.height > 0.0) {
        return Ok(0);
    }
    context.fill_rectangle(panel, canvas());
    let left = draw_cell(
        cache,
        context,
        client,
        CompareSide::Left,
        frame.left,
        frame.left_selected,
        interpolation,
    )?;
    let right = draw_cell(
        cache,
        context,
        client,
        CompareSide::Right,
        frame.right,
        frame.right_selected,
        interpolation,
    )?;
    Ok(left + right)
}

fn draw_cell(
    cache: &dyn GpuImageCache,
    context: &mut dyn DrawContext,
    client: SizeD,
    side: CompareSide,
    pixels: Option<&PixelBuffer>,
    selected: bool,
    interpolation: ImageInterpolation,
) -> Result<i32, RenderError> {
    let border = layout::border(client, side);
    if !(border.width > 0.0 && border.height > 0.0) {
        return Ok(0);
    }
    draw_border(
        context,
        border,
        if selected { selected_color() } else { unselected_color() },
    );
    let Some(pixels) = pixels.filter(|pixels| !pixels.is_disposed()) else {
        return Ok(0);
    };
    let image = layout::image(client, side, pixels.width(), pixels.height());
    context.push_clip(layout::content(client, side));
    let drawn = try_draw(cache, context, pixels, image, interpolation, 1.0, 1);
    let popped = context.pop_clip();
    let drawn = drawn?;
    popped?;
    Ok(i32::from(drawn))
}

fn draw_border(context: &mut dyn DrawContext, border: RectD, color: ColorF) {
    let thickness = layout::BORDER_THICKNESS.min(border.width.min(border.height) / 2.0);
    context.fill_rectangle(
        RectD {
            x: border.x,
            y: border.y,
            width: border.width,
            height: thickness,
        },
        color,
    );
    context.fill_rectangle(
        RectD {
            x: border.x,
            y: border.bottom() - thickness,
            width: border.width,
            height: thickness,
        },
        color,
    );
    let inner = border.height - (2.0 * thickness);
    if inner > 0.0 {
        context.fill_rectangle(
            RectD {
                x: border.x,
                y: border.y + thickness,
                width: thickness,
                height: inner,
            },
            color,
        );
        context.fill_rectangle(
            RectD {
                x: border.right() - thickness,
                y: border.y + thickness,
                width: thickness,
                height: inner,
            },
            color,
        );
    }
}
