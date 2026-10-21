//! Port of `PhotoReview.Shell.Win32/Viewing/CompareLayer.cs` (166 lines, WP-21, baseline `d92d6aae`). Unit W8-16b (contract `contracts/W8-16b.md`).
//!
//! The compare panel of the main window (C# `ComparePanel` of `MainWindow.xaml`): [`layout`] holds the pure geometry (C# `CompareLayout`, DIP, origin at the
//! client corner) and [`draw`] paints the canvas panel, the two cell borders (lime green when selected, `#555555` otherwise) and the two uniform images
//! (C# `CompareLayer.Draw`). Images go through `pr_app::viewing::image_layer::try_draw` and the GPU cache, keyed by `PixelBuffer::id()` (INV-1).
//!
//! UI thread only. Every C# `Math.Max`/`Math.Min` keeps its .NET NaN semantics (a NaN operand wins), so the porter writes the two private helpers
//! `net_max`/`net_min` (the `pr_app` ones are private).

// Contract stub: the bodies are `todo!()` until the W8-16b port; removed with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::services::render::{DrawContext, GpuImageCache, RenderError};
use pr_app::viewmodels::compare::CompareViewModel;
use pr_app::viewport::render_geometry::{ColorF, ImageInterpolation};
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
        todo!("W8-16b")
    }

    /// The frame `ShellMainWindow.DrawPresentation` builds from the view model (C# `compare.LeftImage as PixelBuffer`, `IsLeftSelected`, ...):
    /// `left_image()` -> `DecodedImageView::platform_image().as_pixels()` (`None` for an opaque image), the two selection flags verbatim.
    pub fn from_view_model(compare: &CompareViewModel) -> CompareFrame<'_> {
        todo!("W8-16b")
    }
}

/// C# `CompareLayer.Canvas` = `#101010`, opaque.
pub fn canvas() -> ColorF {
    todo!("W8-16b")
}

/// C# `CompareLayer.Selected` = `Brushes.LimeGreen` `#32CD32`.
pub fn selected_color() -> ColorF {
    todo!("W8-16b")
}

/// C# `CompareLayer.Unselected` = `#555555`.
pub fn unselected_color() -> ColorF {
    todo!("W8-16b")
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

    /// C# `Panel(client)`: `(8, 8, max(0, w - 16), max(0, h - 16))`.
    pub fn panel(client: SizeD) -> RectD {
        todo!("W8-16b")
    }

    /// C# `Border(client, side)`: the panel split into two equal columns, each cell inset by 4 on every side (the outer box including the 2 DIP border).
    pub fn border(client: SizeD, side: CompareSide) -> RectD {
        todo!("W8-16b")
    }

    /// C# `Content(client, side)`: `border` inset by the 2 DIP border thickness.
    pub fn content(client: SizeD, side: CompareSide) -> RectD {
        todo!("W8-16b")
    }

    /// C# `Image(client, side, pixelWidth, pixelHeight)`: `Stretch=Uniform` inside `content`, centred; an empty rectangle at the content origin when the
    /// bitmap or the content is empty (`<= 0`).
    pub fn image(client: SizeD, side: CompareSide, pixel_width: i32, pixel_height: i32) -> RectD {
        todo!("W8-16b")
    }

    /// C# `HitTest(client, point)`: the cell whose `border` (borders included, gap and margin excluded) contains the point; Left is tested first.
    pub fn hit_test(client: SizeD, point: PointD) -> Option<CompareSide> {
        todo!("W8-16b")
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
    todo!("W8-16b")
}
