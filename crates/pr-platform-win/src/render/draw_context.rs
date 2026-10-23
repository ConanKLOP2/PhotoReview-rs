//! Port of `PhotoReview.Shell.Rendering/D2DDrawContext.cs` (359 lines, WP-15 / C-09), baseline `d92d6aae`. Unit W8-12e.
//! Contract: `contracts/W8-12e.md`.
//!
//! The drawing half of the renderer: the per-frame state machine (opacity stack, transform stack, clip depth), `DrawImage` over the tiles
//! of a `TiledGpuImage<ID2D1Bitmap1>`, the primitives, `DrawText` over a DirectWrite layout, and the offscreen read-back. Opacity is a
//! multiplier on each brush / bitmap call (no layers); transforms are applied to the context at once (`SetTransform`).
//!
//! The geometry (intersect, map to destination, pixel-exact snapping, EXIF orientation matrices) is pure and lives in
//! `pr_app::viewport::render_geometry` (W8-01c). `SetTransform` and `DrawTextLayout` take `windows-numerics` types (`Matrix3x2`,
//! `Vector2`); they are named `D2dMatrix` here to keep them apart from the `pr_app` matrix.
//!
//! Thread affinity: UI thread only, `!Send` (it owns an `Rc<DeviceResources>`).
//!
//! Types (the layering PR landed, D-W8B3R-1): [`PointD`], [`RectD`], [`ColorF`] and [`Matrix3x2`] are the `pr_app` types (re-exported
//! here, so the paths of the contract keep working). `pr_app`'s `Matrix3x2::compose` is the contract's `multiply` (self applied first).
//! The inherent methods keep the contract signatures (`image: &dyn Any`, `layout: &dyn NativeTextLayout`); `impl DrawContext` /
//! `OrientedDrawContext` at the end of the file are the adapter to the `pr_app` traits (`&dyn GpuImage`, `&dyn TextLayout`).

use std::any::Any;
use std::fmt;
use std::rc::Rc;

use pr_app::services::render::tiled::{GpuTile, TiledGpuImage};
use pr_app::services::render::{DrawContext, GpuImage, OrientedDrawContext, RenderError, TextLayout};
use pr_app::viewport::render_geometry::{
    for_exif_orientation, intersect, map_to_destination, pixel_rect_to_rect_d, rect_is_empty, try_snap_pixel_exact,
    unoriented_rect,
};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use windows::Win32::Foundation::E_OUTOFMEMORY;
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_INTERPOLATION_MODE,
    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_ROUNDED_RECT, ID2D1Bitmap1, ID2D1DeviceContext,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::IDWriteTextLayout;
use windows_numerics::{Matrix3x2 as D2dMatrix, Vector2};

pub use pr_app::input::primitives::{PointD, RectD};
pub use pr_app::viewport::render_geometry::{ColorF, Matrix3x2};

use super::device::resources::{ImageInterpolation, to_d2d_interpolation};
use super::device::{DeviceResources, RenderFault};
use super::text::renderer::DWriteTextLayoutLease;
use crate::gfx::com::{GfxError, ResultExt};
use crate::gfx::d2d;

/// C# `INativeTextLayout` (D2DDrawContext.cs lines 355 to 359): a text layout that `draw_text` can draw. Implemented by the layout of the
/// DirectWrite text renderer (W8-12i); the `TextLayout` trait object of `pr_app` reaches it through `as_any().downcast_ref()`.
pub trait NativeTextLayout {
    /// C# `NativeLayout`.
    fn native_layout(&self) -> &IDWriteTextLayout;
}

/// `ToD2D(RectD)` (C# line 325): `(x, y, x + width, y + height)` as `f32` (the C# `Right`/`Bottom` are computed in `f64` first).
#[must_use]
pub fn to_d2d_rect(rect: RectD) -> D2D_RECT_F {
    D2D_RECT_F {
        left: rect.x as f32,
        top: rect.y as f32,
        right: rect.right() as f32,
        bottom: rect.bottom() as f32,
    }
}

/// `ToD2D(ColorF, opacity)` (C# line 328): `(r, g, b, a * opacity)`.
#[must_use]
pub fn to_d2d_color(color: ColorF, opacity: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a * opacity,
    }
}

/// `SetTransform`'s argument: `(m11, m12, m21, m22, dx = m31, dy = m32)`, not transposed (trap T2).
fn to_d2d_matrix(matrix: Matrix3x2) -> D2dMatrix {
    D2dMatrix {
        M11: matrix.m11,
        M12: matrix.m12,
        M21: matrix.m21,
        M22: matrix.m22,
        M31: matrix.m31,
        M32: matrix.m32,
    }
}

/// The pure per-frame stacks of the draw context (C# fields `_opacity`, `_transforms`, `_currentOpacity`, `_transform`, `_clipDepth`).
/// No COM call happens here, so every rule is testable without a device; `D2dDrawContext` applies the results to Direct2D.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrameState {
    pub(crate) opacity_stack: Vec<f32>,
    pub(crate) transform_stack: Vec<Matrix3x2>,
    pub(crate) current_opacity: f32,
    pub(crate) transform: Matrix3x2,
    pub(crate) clip_depth: i32,
}

impl FrameState {
    /// C# `Begin` state: empty stacks, opacity 1, identity, clip depth 0.
    pub(crate) fn new() -> Self {
        Self {
            opacity_stack: Vec::new(),
            transform_stack: Vec::new(),
            current_opacity: 1.0,
            transform: Matrix3x2::IDENTITY,
            clip_depth: 0,
        }
    }

    /// Back to the [`FrameState::new`] state.
    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    /// C# `PushOpacity`: push `current_opacity`, then `current_opacity *= clamp(opacity, 0, 1)` (`Math.Clamp` on a NaN input is NaN in
    /// C#; NaN stays NaN here, see contract trap).
    pub(crate) fn push_opacity(&mut self, opacity: f32) {
        self.opacity_stack.push(self.current_opacity);
        // `f32::clamp` returns NaN for a NaN input, like `Math.Clamp` (T9): do not replace it by `max`/`min`.
        self.current_opacity *= opacity.clamp(0.0, 1.0);
    }

    /// C# `PopOpacity`: empty is `Err(InvalidOperation "PopOpacity without PushOpacity.")`.
    pub(crate) fn pop_opacity(&mut self) -> Result<(), RenderFault> {
        match self.opacity_stack.pop() {
            Some(previous) => {
                self.current_opacity = previous;
                Ok(())
            }
            None => Err(RenderFault::InvalidOperation {
                message: "PopOpacity without PushOpacity.",
            }),
        }
    }

    /// C# `PushClip` bookkeeping: `clip_depth += 1`.
    pub(crate) fn push_clip(&mut self) {
        self.clip_depth = self.clip_depth.saturating_add(1);
    }

    /// C# `PopClip` bookkeeping: depth 0 is `Err(InvalidOperation "PopClip without PushClip.")`, else `clip_depth -= 1`.
    pub(crate) fn pop_clip(&mut self) -> Result<(), RenderFault> {
        if self.clip_depth <= 0 {
            return Err(RenderFault::InvalidOperation {
                message: "PopClip without PushClip.",
            });
        }
        self.clip_depth -= 1;
        Ok(())
    }

    /// C# `PushTransform`: pushes the current matrix, the new current matrix is `transform.multiply(old)`; returns it.
    pub(crate) fn push_transform(&mut self, transform: Matrix3x2) -> Matrix3x2 {
        self.transform_stack.push(self.transform);
        // Row vectors: `transform` is applied FIRST, then the old matrix (T1).
        self.transform = transform.compose(self.transform);
        self.transform
    }

    /// C# `PopTransform`: empty is `Err(InvalidOperation "PopTransform without PushTransform.")`, else restores and returns the
    /// previous matrix.
    pub(crate) fn pop_transform(&mut self) -> Result<Matrix3x2, RenderFault> {
        match self.transform_stack.pop() {
            Some(previous) => {
                self.transform = previous;
                Ok(previous)
            }
            None => Err(RenderFault::InvalidOperation {
                message: "PopTransform without PushTransform.",
            }),
        }
    }

    /// C# `End`: `clip_depth == 0 && opacity_stack.is_empty() && transform_stack.is_empty()`.
    pub(crate) fn is_balanced(&self) -> bool {
        self.clip_depth == 0 && self.opacity_stack.is_empty() && self.transform_stack.is_empty()
    }
}

/// Drawing context of one frame (C# `D2DDrawContext`). Obtained from `D2dRenderSurface::begin_draw_context`; it cannot outlive the frame.
pub struct D2dDrawContext {
    resources: Rc<DeviceResources>,
    frame: FrameState,
    active: bool,
    skipped_image_draws: i64,
}

impl fmt::Debug for D2dDrawContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("D2dDrawContext")
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

// The `unsafe` Direct2D calls, one small helper each (windows 0.62.2 declares every draw call `unsafe fn`, D-W8B3G-6). All of them are
// called only between `BeginDraw` and `EndDraw` of `context` (guaranteed by `D2dDrawContext::active`), with an interface owned by the
// caller for the duration of the call, and with pointers to locals that live until the call returns.

fn d2d_clear(context: &ID2D1DeviceContext, color: &D2D1_COLOR_F) {
    // SAFETY: `context` is a live owned interface with a target and an open draw; `color` is a fully initialised local that outlives
    // the call.
    unsafe { context.Clear(Some(color)) };
}

fn d2d_fill_rectangle(context: &ID2D1DeviceContext, rect: &D2D_RECT_F, brush: &ID2D1SolidColorBrush) {
    // SAFETY: `context` and `brush` are live owned interfaces of the same device; `rect` is an initialised local that outlives the call.
    unsafe { context.FillRectangle(rect, brush) };
}

fn d2d_fill_rounded_rectangle(context: &ID2D1DeviceContext, rect: &D2D1_ROUNDED_RECT, brush: &ID2D1SolidColorBrush) {
    // SAFETY: as `d2d_fill_rectangle`; `rect` is an initialised local that outlives the call.
    unsafe { context.FillRoundedRectangle(rect, brush) };
}

fn d2d_draw_rectangle(
    context: &ID2D1DeviceContext,
    rect: &D2D_RECT_F,
    brush: &ID2D1SolidColorBrush,
    stroke_width: f32,
) {
    // SAFETY: as `d2d_fill_rectangle`; there is no stroke style (None).
    unsafe { context.DrawRectangle(rect, brush, stroke_width, None) };
}

fn d2d_draw_text_layout(
    context: &ID2D1DeviceContext,
    origin: Vector2,
    layout: &IDWriteTextLayout,
    brush: &ID2D1SolidColorBrush,
) {
    // SAFETY: `context`, `layout` and `brush` are live owned (or borrowed for the call) interfaces; the origin is passed by value.
    unsafe { context.DrawTextLayout(origin, layout, brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT) };
}

fn d2d_push_clip(context: &ID2D1DeviceContext, rect: &D2D_RECT_F) {
    // SAFETY: `context` is a live owned interface with an open draw; `rect` is an initialised local that outlives the call.
    unsafe { context.PushAxisAlignedClip(rect, D2D1_ANTIALIAS_MODE_ALIASED) };
}

fn d2d_pop_clip(context: &ID2D1DeviceContext) {
    // SAFETY: `context` is a live owned interface; the caller tracks the clip depth, so a matching `PushAxisAlignedClip` is open.
    unsafe { context.PopAxisAlignedClip() };
}

fn d2d_set_transform(context: &ID2D1DeviceContext, matrix: &D2dMatrix) {
    // SAFETY: `context` is a live owned interface; `matrix` is an initialised local that outlives the call.
    unsafe { context.SetTransform(matrix) };
}

fn d2d_draw_bitmap(
    context: &ID2D1DeviceContext,
    bitmap: &ID2D1Bitmap1,
    destination: &D2D_RECT_F,
    opacity: f32,
    mode: D2D1_INTERPOLATION_MODE,
    source: &D2D_RECT_F,
) {
    // SAFETY: `context` and `bitmap` are live interfaces of the same device (the tile generation was checked against the device);
    // `destination` and `source` are initialised locals that outlive the call; there is no perspective matrix (None).
    unsafe { context.DrawBitmap(bitmap, Some(destination), opacity, mode, Some(source), None) };
}

fn set_brush(brush: &ID2D1SolidColorBrush, color: &D2D1_COLOR_F, opacity: f32) {
    // SAFETY: `brush` is a live owned interface; `color` is an initialised local that outlives the call; `SetOpacity` takes a number.
    unsafe {
        brush.SetColor(color);
        brush.SetOpacity(opacity);
    }
}

impl D2dDrawContext {
    /// C# internal constructor; called by the surface.
    pub(crate) fn new(resources: Rc<DeviceResources>) -> Self {
        Self {
            resources,
            frame: FrameState::new(),
            active: false,
            skipped_image_draws: 0,
        }
    }

    /// C# `SkippedImageDraws`: draws skipped because the image was not drawable (evicted, half uploaded, older device generation,
    /// not a tiled image of this renderer).
    #[must_use]
    pub fn skipped_image_draws(&self) -> i64 {
        self.skipped_image_draws
    }

    /// C# `Transform`: current local-DIP to target-DIP matrix.
    #[must_use]
    pub fn transform(&self) -> Matrix3x2 {
        self.frame.transform
    }

    /// The C# `Context` getter: the device context while the frame is open. A draw call outside a frame cannot happen in Rust (the
    /// context is only reachable through the borrow of `begin_draw_context`, contract note 10): `debug_assert!`, and in release the
    /// call is skipped (`None`; also `None` when the device is disposed).
    fn context(&self) -> Option<ID2D1DeviceContext> {
        debug_assert!(
            self.active,
            "The draw context is only valid between BeginDraw and EndDrawAndPresent."
        );
        if !self.active {
            return None;
        }
        self.resources.context().ok()
    }

    /// C# `PrepareBrush`: the shared brush with the colour (straight alpha) AND the current opacity set (T5).
    fn prepare_brush(&self, color: ColorF) -> Option<ID2D1SolidColorBrush> {
        let brush = self.resources.brush().ok()?;
        set_brush(&brush, &to_d2d_color(color, 1.0), self.frame.current_opacity);
        Some(brush)
    }

    /// C# `SetTransform(transform)` for the matrix already stored in `frame.transform`.
    fn apply_transform(&self, context: &ID2D1DeviceContext) {
        d2d_set_transform(context, &to_d2d_matrix(self.frame.transform));
    }

    /// C# `Clear(color)`: `ID2D1DeviceContext::Clear` with the colour as given (alpha NOT multiplied by the pushed opacity).
    pub fn clear(&mut self, color: ColorF) {
        if let Some(context) = self.context() {
            d2d_clear(&context, &to_d2d_color(color, 1.0));
        }
    }

    /// C# `DrawImage`. `image` is `GpuImage::as_any()`; anything that is not `TiledGpuImage<ID2D1Bitmap1>`, or is not
    /// `is_drawable(resources.generation())`, increments `skipped_image_draws` and returns. Then: source = `sourcePixels ?? bounds`
    /// intersected with the image bounds; empty source or destination returns silently; a requested source that was clipped remaps the
    /// destination (`map_to_destination`); pixel-exact snapping at `transform * scale(dpi_scale)` switches to nearest neighbour; alpha is
    /// `clamp(opacity, 0, 1) * current_opacity`; one tile is drawn without a clip, several tiles each clipped to their core.
    pub fn draw_image(
        &mut self,
        image: &dyn Any,
        destination: RectD,
        source_pixels: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    ) {
        let Some(context) = self.context() else {
            return;
        };
        let tiled = match image.downcast_ref::<TiledGpuImage<ID2D1Bitmap1>>() {
            Some(tiled) if tiled.is_drawable(self.resources.generation()) => tiled,
            _ => {
                self.skipped_image_draws = self.skipped_image_draws.wrapping_add(1);
                return;
            }
        };

        let bounds = RectD {
            x: 0.0,
            y: 0.0,
            width: f64::from(tiled.pixel_width()),
            height: f64::from(tiled.pixel_height()),
        };
        let source = intersect(source_pixels.unwrap_or(bounds), bounds);
        if rect_is_empty(source) || rect_is_empty(destination) {
            return;
        }

        let mut destination = destination;
        if let Some(requested) = source_pixels
            && source != requested
        {
            // The requested source lies partly outside the image: draw only the intersection, at its place in the destination.
            destination = map_to_destination(requested, destination, source);
        }

        let mut mode = to_d2d_interpolation(interpolation);
        let dpi = self.resources.dpi_scale() as f32;
        let device = self.frame.transform.compose(Matrix3x2::scale(dpi));
        let snap = try_snap_pixel_exact(device, destination, source);
        if snap.exact {
            // 1 source pixel = 1 device pixel: copy byte for byte (G-PIX "identical at 100 %").
            destination = snap.snapped;
            mode = D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR;
        }

        // `f32::clamp` keeps a NaN opacity NaN, like `Math.Clamp` (T9).
        let alpha = opacity.clamp(0.0, 1.0) * self.frame.current_opacity;
        let tiles = tiled.tiles();
        let clip_to_core = tiles.len() != 1;
        for tile in tiles {
            draw_tile(&context, tile, source, destination, mode, alpha, clip_to_core);
        }
    }

    /// C# `DrawImageOriented`: `push_transform(for_exif_orientation(o, dest))`, `draw_image` into `unoriented_rect(o, dest)`,
    /// `pop_transform` (the pop also runs if the draw returned early). `o` outside 1..=8 behaves as the C# helpers define it (W8-01c).
    pub fn draw_image_oriented(
        &mut self,
        image: &dyn Any,
        destination: RectD,
        exif_orientation: i32,
        source_pixels: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    ) {
        self.push_transform(for_exif_orientation(exif_orientation, destination));
        self.draw_image(
            image,
            unoriented_rect(exif_orientation, destination),
            source_pixels,
            interpolation,
            opacity,
        );
        // The C# `finally`: the pop runs on every path (T8). It can only fail when the push was skipped (inactive context).
        let _ = self.pop_transform();
    }

    /// C# `FillRectangle`.
    pub fn fill_rectangle(&mut self, rect: RectD, color: ColorF) {
        let Some(context) = self.context() else {
            return;
        };
        if let Some(brush) = self.prepare_brush(color) {
            d2d_fill_rectangle(&context, &to_d2d_rect(rect), &brush);
        }
    }

    /// C# `FillRoundedRectangle` (`radius_x = radius_y = radius as f32`, via `FillRoundedRectangle` on the context).
    pub fn fill_rounded_rectangle(&mut self, rect: RectD, radius: f64, color: ColorF) {
        let Some(context) = self.context() else {
            return;
        };
        if let Some(brush) = self.prepare_brush(color) {
            let rounded = D2D1_ROUNDED_RECT {
                rect: to_d2d_rect(rect),
                radiusX: radius as f32,
                radiusY: radius as f32,
            };
            d2d_fill_rounded_rectangle(&context, &rounded, &brush);
        }
    }

    /// C# `DrawRectangle` (stroke width `as f32`, no stroke style).
    pub fn draw_rectangle(&mut self, rect: RectD, color: ColorF, stroke_width: f64) {
        let Some(context) = self.context() else {
            return;
        };
        if let Some(brush) = self.prepare_brush(color) {
            d2d_draw_rectangle(&context, &to_d2d_rect(rect), &brush, stroke_width as f32);
        }
    }

    /// C# `DrawText`: `DrawTextLayout(origin, layout, brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT)`. The C# rejection of a foreign
    /// layout (`ArgumentException`) is not here: it is the failed `downcast_ref` of the `DrawContext` adapter, which returns
    /// `Err(InvalidArgument { param: "layout", message: "The text layout was not created by the Direct2D text renderer." })`.
    pub fn draw_text(&mut self, layout: &dyn NativeTextLayout, origin: PointD, color: ColorF) {
        self.draw_native_layout(layout.native_layout(), origin, color);
    }

    /// Shared body of `draw_text` and the `DrawContext::draw_text` adapter.
    fn draw_native_layout(&mut self, layout: &IDWriteTextLayout, origin: PointD, color: ColorF) {
        let Some(context) = self.context() else {
            return;
        };
        if let Some(brush) = self.prepare_brush(color) {
            // L2 (WP-27): ENABLE_COLOR_FONT draws colour glyphs (Segoe UI Emoji); ordinary text is identical to NONE.
            let origin = Vector2::new(origin.x as f32, origin.y as f32);
            d2d_draw_text_layout(&context, origin, layout, &brush);
        }
    }

    /// C# `PushClip`: `PushAxisAlignedClip(rect, D2D1_ANTIALIAS_MODE_ALIASED)`, `clip_depth += 1`.
    pub fn push_clip(&mut self, rect: RectD) {
        let Some(context) = self.context() else {
            return;
        };
        d2d_push_clip(&context, &to_d2d_rect(rect));
        self.frame.push_clip();
    }

    /// C# `PopClip`: depth 0 is `Err(InvalidOperation "PopClip without PushClip.")` (checked before touching the context).
    pub fn pop_clip(&mut self) -> Result<(), RenderFault> {
        self.frame.pop_clip()?;
        if let Some(context) = self.context() {
            d2d_pop_clip(&context);
        }
        Ok(())
    }

    /// C# `PushOpacity`: stores the current value, multiplies by `clamp(opacity, 0, 1)`.
    pub fn push_opacity(&mut self, opacity: f32) {
        if self.context().is_some() {
            self.frame.push_opacity(opacity);
        }
    }

    /// C# `PopOpacity`: empty stack is `Err(InvalidOperation "PopOpacity without PushOpacity.")`.
    pub fn pop_opacity(&mut self) -> Result<(), RenderFault> {
        self.frame.pop_opacity()
    }

    /// C# `PushTransform`: remembers the current matrix and applies `transform * current` (row vectors: `transform` first).
    pub fn push_transform(&mut self, transform: Matrix3x2) {
        let Some(context) = self.context() else {
            return;
        };
        self.frame.push_transform(transform);
        self.apply_transform(&context);
    }

    /// C# `PopTransform`: empty stack is `Err(InvalidOperation "PopTransform without PushTransform.")`.
    pub fn pop_transform(&mut self) -> Result<(), RenderFault> {
        self.frame.pop_transform()?;
        if let Some(context) = self.context() {
            self.apply_transform(&context);
        }
        Ok(())
    }

    /// C# `TryReadPixels`: a window surface is `Ok(None)`; an offscreen surface `Flush`es the context and returns the whole target as a
    /// new `Pbgra32` `PixelBuffer` of the surface size (`gfx::d2d::read_pixels`, tight rows). `Err(Device)` when a COM call fails
    /// (the staging bitmap is released and the half-built buffer dropped on every path).
    pub fn try_read_pixels(&mut self) -> Result<Option<PixelBuffer>, RenderFault> {
        let resources = &self.resources;
        if resources.has_swap_chain() {
            return Ok(None);
        }
        let context = resources.context()?;
        // SAFETY: `context` is a live owned interface; both tag out-pointers are None. Flush submits the open draw's commands.
        unsafe { context.Flush(None, None) }.gfx("ID2D1DeviceContext::Flush")?;
        let width = resources.pixel_width();
        let height = resources.pixel_height();
        let target = resources.target()?;
        // The sizes are `>= 1` (`clamp_dimension`), so the conversions cannot fail.
        let w = u32::try_from(width).unwrap_or(1);
        let h = u32::try_from(height).unwrap_or(1);
        // Tight BGRA rows (the mapped pitch can be larger than `width * 4`, T11); the staging bitmap is dropped inside.
        let rows = d2d::read_pixels(&context, &target, w, h)?;
        let out_of_memory = GfxError::new("PixelBuffer::allocate", E_OUTOFMEMORY.0);
        let mut buffer = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).map_err(|_| out_of_memory)?;
        let destination = buffer.bytes_mut().map_err(|_| out_of_memory)?;
        if destination.len() != rows.len() {
            return Err(RenderFault::Device(out_of_memory));
        }
        destination.copy_from_slice(&rows);
        Ok(Some(buffer))
    }

    /// C# `Begin` (internal): `active = true`, clear both stacks, opacity 1, clip depth 0, identity transform applied to the context.
    pub(crate) fn begin(&mut self) {
        self.active = true;
        self.frame.reset();
        if let Ok(context) = self.resources.context() {
            self.apply_transform(&context);
        }
    }

    /// C# `End` (internal): pops every dangling clip (Direct2D's `EndDraw` fails on unbalanced clips), `active = false`; returns
    /// `balanced` = clip depth, opacity stack and transform stack were all empty BEFORE the pops.
    pub(crate) fn end(&mut self) -> bool {
        // T6: `balanced` first, then the pops; the opacity and transform stacks are not unwound (the next `begin` resets them).
        let balanced = self.frame.is_balanced();
        let context = self.resources.context().ok();
        while self.frame.clip_depth > 0 {
            if let Some(context) = &context {
                d2d_pop_clip(context);
            }
            self.frame.clip_depth -= 1;
        }
        self.active = false;
        balanced
    }
}

/// C# `DrawTile` (lines 287 to 323): one tile of the image, clipped to its core when the image has several tiles.
fn draw_tile(
    context: &ID2D1DeviceContext,
    tile: &GpuTile<ID2D1Bitmap1>,
    source: RectD,
    destination: RectD,
    mode: D2D1_INTERPOLATION_MODE,
    alpha: f32,
    clip_to_core: bool,
) {
    // A clone (AddRef) so the tile's `RefCell` is not borrowed during the draw.
    let Some(bitmap) = tile.with_bitmap(|bitmap| bitmap.cloned()) else {
        return;
    };
    let spec = tile.spec();
    let texture = pixel_rect_to_rect_d(spec.texture);
    let core = intersect(pixel_rect_to_rect_d(spec.core), source);
    if rect_is_empty(core) {
        return;
    }

    // The overlap border (gutter) of the tile is drawn too, so interpolation at the edge uses the real pixels of the neighbour, and
    // then clipped to the core: every device pixel belongs to exactly one tile (aliased clip at the pixel centre).
    let region = intersect(texture, source);
    let dest = map_to_destination(source, destination, region);
    let dest_rect = to_d2d_rect(dest);
    let source_rect = D2D_RECT_F {
        left: (region.x - texture.x) as f32,
        top: (region.y - texture.y) as f32,
        right: (region.right() - texture.x) as f32,
        bottom: (region.bottom() - texture.y) as f32,
    };
    if clip_to_core {
        let clip = to_d2d_rect(map_to_destination(source, destination, core));
        d2d_push_clip(context, &clip);
        d2d_draw_bitmap(context, &bitmap, &dest_rect, alpha, mode, &source_rect);
        d2d_pop_clip(context);
    } else {
        d2d_draw_bitmap(context, &bitmap, &dest_rect, alpha, mode, &source_rect);
    }
}

/// Adapter from the `pr_app` error type: the contract's `RenderFault` converted field by field.
pub(crate) fn render_error(fault: RenderFault) -> RenderError {
    match fault {
        RenderFault::Disposed { object } => RenderError::Disposed { object },
        RenderFault::InvalidOperation { message } => RenderError::InvalidOperation { message },
        RenderFault::InvalidArgument { param, message } => RenderError::InvalidArgument { param, message },
        RenderFault::ArgumentOutOfRange { param } => RenderError::ArgumentOutOfRange { param },
        RenderFault::Device(error) => RenderError::Device {
            hresult: error.hresult,
            operation: error.operation,
        },
    }
}

impl DrawContext for D2dDrawContext {
    fn clear(&mut self, color: ColorF) {
        D2dDrawContext::clear(self, color);
    }

    fn draw_image(
        &mut self,
        image: &dyn GpuImage,
        destination: RectD,
        source_pixels: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    ) {
        D2dDrawContext::draw_image(self, image.as_any(), destination, source_pixels, interpolation, opacity);
    }

    fn fill_rectangle(&mut self, rect: RectD, color: ColorF) {
        D2dDrawContext::fill_rectangle(self, rect, color);
    }

    fn fill_rounded_rectangle(&mut self, rect: RectD, radius: f64, color: ColorF) {
        D2dDrawContext::fill_rounded_rectangle(self, rect, radius, color);
    }

    fn draw_rectangle(&mut self, rect: RectD, color: ColorF, stroke_width: f64) {
        D2dDrawContext::draw_rectangle(self, rect, color, stroke_width);
    }

    fn draw_text(&mut self, layout: &dyn TextLayout, origin: PointD, color: ColorF) -> Result<(), RenderError> {
        // C# `layout is not INativeTextLayout`: the layout of the shell's text renderer (W8-12i) is the only native one.
        let native = layout
            .as_any()
            .downcast_ref::<DWriteTextLayoutLease>()
            .and_then(DWriteTextLayoutLease::native_layout);
        match native {
            Some(native) => {
                self.draw_native_layout(&native, origin, color);
                Ok(())
            }
            None => Err(RenderError::InvalidArgument {
                param: "layout",
                message: "The text layout was not created by the Direct2D text renderer.",
            }),
        }
    }

    fn push_clip(&mut self, rect: RectD) {
        D2dDrawContext::push_clip(self, rect);
    }

    fn pop_clip(&mut self) -> Result<(), RenderError> {
        D2dDrawContext::pop_clip(self).map_err(render_error)
    }

    fn push_opacity(&mut self, opacity: f32) {
        D2dDrawContext::push_opacity(self, opacity);
    }

    fn pop_opacity(&mut self) -> Result<(), RenderError> {
        D2dDrawContext::pop_opacity(self).map_err(render_error)
    }

    fn try_read_pixels(&mut self) -> Result<Option<PixelBuffer>, RenderError> {
        D2dDrawContext::try_read_pixels(self).map_err(render_error)
    }

    fn as_oriented(&mut self) -> Option<&mut dyn OrientedDrawContext> {
        Some(self)
    }
}

impl OrientedDrawContext for D2dDrawContext {
    fn draw_image_oriented(
        &mut self,
        image: &dyn GpuImage,
        destination: RectD,
        exif_orientation: i32,
        source_pixels: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    ) {
        D2dDrawContext::draw_image_oriented(
            self,
            image.as_any(),
            destination,
            exif_orientation,
            source_pixels,
            interpolation,
            opacity,
        );
    }
}

/// Test kit shared by the draw-context and surface tests (C# `RenderTestImages`; private to the render module, D-W8B3R-10).
#[cfg(test)]
pub(crate) mod test_kit {
    use pr_app::viewport::render_geometry::compute_tiles;
    use pr_imaging::pixels::{PixelBuffer, PixelLayout};
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED};
    use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_OPTIONS_NONE, ID2D1Bitmap1};

    use super::{ColorF, D2dDrawContext, GpuTile, PointD, RectD, TiledGpuImage};
    use crate::render::device::DeviceResources;
    use crate::render::surface::{D2dRenderSurface, PresentResult};

    /// Small deterministic generator (xorshift64); the C# used `System.Random(seed)`, only determinism matters.
    pub(crate) struct Rng(u64);

    impl Rng {
        pub(crate) fn new(seed: u64) -> Self {
            Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
        }

        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn byte(&mut self) -> u8 {
            (self.next() >> 32) as u8
        }

        fn below(&mut self, n: u64) -> u64 {
            (self.next() >> 16) % n
        }
    }

    pub(crate) fn color(r: f32, g: f32, b: f32, a: f32) -> ColorF {
        ColorF { r, g, b, a }
    }

    pub(crate) fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
        RectD { x, y, width, height }
    }

    pub(crate) fn point(x: f64, y: f64) -> PointD {
        PointD { x, y }
    }

    fn to_byte(value: f64) -> u8 {
        value.round_ties_even().clamp(0.0, 255.0) as u8
    }

    /// C# `CreateSmooth`: slowly varying background plus a few sharp edges (Pbgra32 alpha 255, or Bgr32 with X = 0x55).
    pub(crate) fn create_smooth(width: i32, height: i32, layout: PixelLayout) -> PixelBuffer {
        let mut buffer = PixelBuffer::allocate(width, height, layout).unwrap();
        for y in 0..height {
            let row = buffer.get_row_mut(y).unwrap();
            for x in 0..width {
                let fx = f64::from(x) / f64::from(width);
                let fy = f64::from(y) / f64::from(height);
                let mut r = 128.0 + 100.0 * (f64::from(x) / 37.0 + f64::from(y) / 53.0).sin();
                let mut g = 30.0 + 190.0 * fx * (1.0 - 0.5 * fy);
                let mut b = 128.0 + 90.0 * (f64::from(x) / 71.0 - f64::from(y) / 29.0).cos();
                if fx > 0.3 && fx < 0.45 && fy > 0.2 && fy < 0.7 {
                    r = 240.0;
                    g = 230.0;
                    b = 20.0;
                }
                let i = x as usize * 4;
                row[i] = to_byte(b);
                row[i + 1] = to_byte(g);
                row[i + 2] = to_byte(r);
                row[i + 3] = if layout == PixelLayout::Pbgra32 { 255 } else { 0x55 };
            }
        }
        buffer
    }

    /// C# `CreateNoise`: seeded noise, valid premultiplied Pbgra32 (B, G, R <= A), about a quarter of the pixels partly transparent.
    pub(crate) fn create_noise(width: i32, height: i32, seed: u64, translucent: bool) -> PixelBuffer {
        let mut rng = Rng::new(seed);
        let mut buffer = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
        for y in 0..height {
            let row = buffer.get_row_mut(y).unwrap();
            for pixel in row.chunks_exact_mut(4) {
                let alpha = if translucent && rng.below(4) == 0 {
                    rng.byte()
                } else {
                    255
                };
                for channel in &mut pixel[..3] {
                    *channel = (u32::from(rng.byte()) * u32::from(alpha) / 255) as u8;
                }
                pixel[3] = alpha;
            }
        }
        buffer
    }

    /// C# `CreateSolid`.
    pub(crate) fn create_solid(width: i32, height: i32, b: u8, g: u8, r: u8, a: u8) -> PixelBuffer {
        let mut buffer = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
        for y in 0..height {
            for pixel in buffer.get_row_mut(y).unwrap().chunks_exact_mut(4) {
                pixel.copy_from_slice(&[b, g, r, a]);
            }
        }
        buffer
    }

    /// C# `Crop`: a region as a new Pbgra32 buffer; a Bgr32 source gets A = 255 (like `D2D1_ALPHA_MODE_IGNORE`).
    pub(crate) fn crop(source: &PixelBuffer, x: i32, y: i32, width: i32, height: i32) -> PixelBuffer {
        let mut result = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
        for row in 0..height {
            let from = &source.get_row(y + row).unwrap()[x as usize * 4..(x + width) as usize * 4];
            let target = result.get_row_mut(row).unwrap();
            target.copy_from_slice(from);
            if source.layout() == PixelLayout::Bgr32 {
                for pixel in target.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
            }
        }
        result
    }

    /// C# `ResizeArea`: area-weighted (box) reference for downscaling.
    pub(crate) fn resize_area(source: &PixelBuffer, width: i32, height: i32) -> PixelBuffer {
        let (sw, sh) = (source.width() as usize, source.height() as usize);
        let (w, h) = (width as usize, height as usize);
        let mut horizontal = vec![0.0_f64; w * sh * 4];
        let sx = sw as f64 / w as f64;
        for y in 0..sh {
            let row = source.get_row(y as i32).unwrap();
            for dx in 0..w {
                let start = dx as f64 * sx;
                let end = start + sx;
                let mut acc = [0.0_f64; 4];
                let last = sw.min(end.ceil() as usize);
                for x in (start as usize)..last {
                    let weight = end.min(x as f64 + 1.0) - start.max(x as f64);
                    let i = x * 4;
                    acc[0] += f64::from(row[i]) * weight;
                    acc[1] += f64::from(row[i + 1]) * weight;
                    acc[2] += f64::from(row[i + 2]) * weight;
                    let alpha = if source.layout() == PixelLayout::Bgr32 {
                        255.0
                    } else {
                        f64::from(row[i + 3])
                    };
                    acc[3] += alpha * weight;
                }
                let o = (y * w + dx) * 4;
                for c in 0..4 {
                    horizontal[o + c] = acc[c] / sx;
                }
            }
        }
        let mut result = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
        let sy = sh as f64 / h as f64;
        for dy in 0..h {
            let start = dy as f64 * sy;
            let end = start + sy;
            let target = result.get_row_mut(dy as i32).unwrap();
            for dx in 0..w {
                for c in 0..4 {
                    let mut sum = 0.0;
                    for y in (start as usize)..sh.min(end.ceil() as usize) {
                        let weight = end.min(y as f64 + 1.0) - start.max(y as f64);
                        sum += horizontal[(y * w + dx) * 4 + c] * weight;
                    }
                    target[dx * 4 + c] = to_byte(sum / sy);
                }
            }
        }
        result
    }

    /// C# `ResizeBilinear`: pixel-centre bilinear with edge clamping (D2D `LINEAR`), for upscaling.
    pub(crate) fn resize_bilinear(source: &PixelBuffer, width: i32, height: i32) -> PixelBuffer {
        let mut result = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
        let sx = f64::from(source.width()) / f64::from(width);
        let sy = f64::from(source.height()) / f64::from(height);
        for dy in 0..height {
            let fy = ((f64::from(dy) + 0.5) * sy - 0.5).clamp(0.0, f64::from(source.height() - 1));
            let y0 = fy as i32;
            let y1 = (y0 + 1).min(source.height() - 1);
            let wy = fy - f64::from(y0);
            let row0 = source.get_row(y0).unwrap().to_vec();
            let row1 = source.get_row(y1).unwrap().to_vec();
            let target = result.get_row_mut(dy).unwrap();
            for dx in 0..width {
                let fx = ((f64::from(dx) + 0.5) * sx - 0.5).clamp(0.0, f64::from(source.width() - 1));
                let x0 = fx as usize;
                let x1 = (x0 + 1).min(source.width() as usize - 1);
                let wx = fx - x0 as f64;
                for c in 0..4 {
                    let top = f64::from(row0[x0 * 4 + c]) * (1.0 - wx) + f64::from(row0[x1 * 4 + c]) * wx;
                    let bottom = f64::from(row1[x0 * 4 + c]) * (1.0 - wx) + f64::from(row1[x1 * 4 + c]) * wx;
                    target[dx as usize * 4 + c] = to_byte(top * (1.0 - wy) + bottom * wy);
                }
            }
        }
        result
    }

    /// C# `Psnr` (peak 255, B G R plus A when both are Pbgra32); identical gives +infinity.
    pub(crate) fn psnr(expected: &PixelBuffer, actual: &PixelBuffer) -> f64 {
        assert_eq!((expected.width(), expected.height()), (actual.width(), actual.height()));
        let channels = if expected.layout() == PixelLayout::Pbgra32 && actual.layout() == PixelLayout::Pbgra32 {
            4
        } else {
            3
        };
        let mut squares = 0.0_f64;
        let mut count = 0_u64;
        for y in 0..expected.height() {
            let e = expected.get_row(y).unwrap();
            let a = actual.get_row(y).unwrap();
            for (pe, pa) in e.chunks_exact(4).zip(a.chunks_exact(4)) {
                for c in 0..channels {
                    let d = f64::from(pe[c]) - f64::from(pa[c]);
                    squares += d * d;
                    count += 1;
                }
            }
        }
        if squares == 0.0 {
            f64::INFINITY
        } else {
            10.0 * (255.0 * 255.0 / (squares / count as f64)).log10()
        }
    }

    /// C# `AssertIdentical`: byte for byte, reports the first differing pixel.
    pub(crate) fn assert_identical(expected: &PixelBuffer, actual: &PixelBuffer) {
        assert_eq!((expected.width(), expected.height()), (actual.width(), actual.height()));
        for y in 0..expected.height() {
            let e = expected.get_row(y).unwrap();
            let a = actual.get_row(y).unwrap();
            if e == a {
                continue;
            }
            let i = e.iter().zip(a).position(|(x, y)| x != y).unwrap();
            panic!(
                "pixel ({}, {y}) channel {}: expected {}, got {}",
                i / 4,
                "BGRA".as_bytes()[i % 4] as char,
                e[i],
                a[i]
            );
        }
    }

    /// C# `Pixel`: one pixel as (B, G, R, A).
    pub(crate) fn pixel(buffer: &PixelBuffer, x: i32, y: i32) -> (u8, u8, u8, u8) {
        let row = buffer.get_row(y).unwrap();
        let i = x as usize * 4;
        (row[i], row[i + 1], row[i + 2], row[i + 3])
    }

    /// Uploads `pixels` as a `TiledGpuImage<ID2D1Bitmap1>` the way the cache will (W8-12g): `compute_tiles` + `create_bitmap` with the
    /// tile's texture region (gutter included) + `copy_from_memory`; complete (`uploaded_rows == height`), generation of `resources`
    /// (D-W8B3R-5).
    pub(crate) fn upload(
        resources: &DeviceResources,
        pixels: &PixelBuffer,
        max_tile: i32,
        gutter: i32,
    ) -> TiledGpuImage<ID2D1Bitmap1> {
        let specs = compute_tiles(pixels.width(), pixels.height(), max_tile, gutter).unwrap();
        let bytes = pixels.bytes().unwrap();
        let stride = pixels.stride() as usize;
        let alpha_mode = if pixels.layout() == PixelLayout::Pbgra32 {
            D2D1_ALPHA_MODE_PREMULTIPLIED
        } else {
            D2D1_ALPHA_MODE_IGNORE
        };
        let tiles = specs
            .iter()
            .map(|spec| {
                let texture = spec.texture;
                let offset = texture.y as usize * stride + texture.x as usize * 4;
                let bitmap = resources
                    .create_bitmap(
                        texture.width,
                        texture.height,
                        Some(&bytes[offset..]),
                        stride as u32,
                        alpha_mode,
                        D2D1_BITMAP_OPTIONS_NONE,
                    )
                    .unwrap();
                GpuTile::new(*spec, Some(bitmap))
            })
            .collect();
        let image = TiledGpuImage::new(pixels, tiles, resources.generation());
        image.set_uploaded_rows(pixels.height());
        image
    }

    /// One whole-texture image (the default device limit).
    pub(crate) fn upload_single(resources: &DeviceResources, pixels: &PixelBuffer) -> TiledGpuImage<ID2D1Bitmap1> {
        upload(resources, pixels, resources.max_bitmap_size(), 64)
    }

    /// Gives back every bitmap of the image (the cache's eviction path).
    pub(crate) fn release(resources: &DeviceResources, image: &TiledGpuImage<ID2D1Bitmap1>) {
        for tile in image.tiles() {
            resources.release_bitmap(tile.take_bitmap());
        }
        image.set_released(true);
    }

    /// C# `Render`: one frame drawn by `draw`, then the whole target read back, then `EndDrawAndPresent` must say `Presented`.
    pub(crate) fn render(surface: &mut D2dRenderSurface, draw: impl FnOnce(&mut D2dDrawContext)) -> PixelBuffer {
        let context = surface.begin_draw_context().unwrap();
        draw(context);
        let pixels = context
            .try_read_pixels()
            .unwrap()
            .expect("offscreen surfaces read back");
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Presented);
        pixels
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use pr_app::input::primitives::SizeD;
    use pr_app::services::render::RenderSurface;
    use pr_imaging::pixels::apply_orientation;
    use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

    use super::test_kit::{
        assert_identical, color, create_noise, create_smooth, create_solid, crop, pixel, point, psnr, rect, release,
        render, resize_area, resize_bilinear, upload, upload_single,
    };
    use super::*;
    use crate::gfx::dwrite;
    use crate::render::device::tests::HiddenWindow;
    use crate::render::surface::{D2dRenderSurface, PresentResult};

    const TRANSPARENT: ColorF = ColorF {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    const BLACK: ColorF = ColorF {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    const WHITE: ColorF = ColorF {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    fn surface(width: i32, height: i32, dpi: f64) -> D2dRenderSurface {
        D2dRenderSurface::create_offscreen_surface(width, height, dpi, false).unwrap()
    }

    fn invalid(message: &'static str) -> RenderFault {
        RenderFault::InvalidOperation { message }
    }

    // ------------------------------------------------------------------ pure

    // C#: D2DDrawContext fields `_currentOpacity = 1f`, `_transform = Identity`, `_clipDepth = 0`
    #[test]
    fn frame_state_new_is_identity_opacity_one() {
        let state = FrameState::new();
        assert_eq!(state.current_opacity, 1.0);
        assert_eq!(state.transform, Matrix3x2::IDENTITY);
        assert_eq!(state.clip_depth, 0);
        assert!(state.opacity_stack.is_empty() && state.transform_stack.is_empty());
        assert!(state.is_balanced());
    }

    // C#: D2DRenderSurfaceTests.PushOpacity_Nested_MultipliesAndPopRestoresOuterValue (the numbers)
    #[test]
    fn push_opacity_multiplies_and_pop_restores_the_outer_value() {
        let mut state = FrameState::new();
        state.push_opacity(0.5);
        state.push_opacity(0.5);
        assert_eq!(state.current_opacity, 0.25);
        state.pop_opacity().unwrap();
        assert_eq!(state.current_opacity, 0.5);
        state.pop_opacity().unwrap();
        assert_eq!(state.current_opacity, 1.0);
    }

    // C#: D2DDrawContext.PushOpacity `Math.Clamp(opacity, 0f, 1f)` (NaN stays NaN in .NET and in `f32::clamp`)
    #[test]
    fn push_opacity_clamps_to_unit_range() {
        let mut state = FrameState::new();
        state.push_opacity(-1.0);
        assert_eq!(state.current_opacity, 0.0);
        state.reset();
        state.push_opacity(2.0);
        assert_eq!(state.current_opacity, 1.0);
        state.reset();
        state.push_opacity(f32::NAN);
        assert!(state.current_opacity.is_nan(), "NaN is not turned into a bound (T9)");
    }

    // C#: D2DRenderSurfaceTests.Misuse_IsRejected lines 495 to 497
    #[test]
    fn pop_on_empty_stacks_report_the_csharp_messages() {
        let mut state = FrameState::new();
        assert_eq!(state.pop_clip().unwrap_err(), invalid("PopClip without PushClip."));
        assert_eq!(
            state.pop_opacity().unwrap_err(),
            invalid("PopOpacity without PushOpacity.")
        );
        assert_eq!(
            state.pop_transform().unwrap_err(),
            invalid("PopTransform without PushTransform.")
        );
        assert!(state.is_balanced(), "a failed pop changes nothing");
    }

    // C#: D2DDrawContext.PushClip / PopClip depth bookkeeping
    #[test]
    fn push_clip_pop_clip_balance() {
        let mut state = FrameState::new();
        state.push_clip();
        state.push_clip();
        assert_eq!(state.clip_depth, 2);
        assert!(!state.is_balanced());
        state.pop_clip().unwrap();
        state.pop_clip().unwrap();
        assert!(state.is_balanced());
    }

    // C#: `_clipDepth == 0` guard of PopClip (T12)
    #[test]
    fn clip_depth_never_goes_negative() {
        let mut state = FrameState::new();
        assert!(state.pop_clip().is_err());
        assert!(state.pop_clip().is_err());
        assert_eq!(state.clip_depth, 0);
        state.push_clip();
        assert_eq!(state.clip_depth, 1);
    }

    // C#: D2DDrawContext.End `balanced`
    #[test]
    fn is_balanced_requires_all_three_stacks_empty() {
        for mask in 0..8_u8 {
            let mut state = FrameState::new();
            if mask & 1 != 0 {
                state.push_clip();
            }
            if mask & 2 != 0 {
                state.push_opacity(0.5);
            }
            if mask & 4 != 0 {
                state.push_transform(Matrix3x2::translation(1.0, 2.0));
            }
            assert_eq!(state.is_balanced(), mask == 0, "mask {mask:#05b}");
        }
    }

    // C#: D2DRenderSurfaceTests.PushTransform_Nested_AppliesInnerFirstThenOuter (the matrices)
    #[test]
    fn push_transform_nested_applies_inner_first_then_outer() {
        // The C# test: outer translation(10, 0), inner scale(2): the point is scaled first, then moved.
        let mut state = FrameState::new();
        state.push_transform(Matrix3x2::translation(10.0, 0.0));
        let nested = state.push_transform(Matrix3x2::scale(2.0));
        assert_eq!(nested, Matrix3x2::new(2.0, 0.0, 0.0, 2.0, 10.0, 0.0));
        assert_eq!(nested.transform_point(1.0, 1.0), (12.0, 2.0));
        assert_eq!(state.transform, nested);
        // The contract's example: outer scale(2), inner translation(10, 0): moved first, then scaled (`m31 = 20`).
        let mut state = FrameState::new();
        state.push_transform(Matrix3x2::scale(2.0));
        let nested = state.push_transform(Matrix3x2::translation(10.0, 0.0));
        assert_eq!((nested.m11, nested.m31), (2.0, 20.0));
        // Popping returns each previous matrix.
        assert_eq!(state.pop_transform().unwrap(), Matrix3x2::scale(2.0));
        assert_eq!(state.pop_transform().unwrap(), Matrix3x2::IDENTITY);
    }

    // C#: D2DRenderSurfaceTests.PushTransform_TranslatesAndPopRestores
    #[test]
    fn push_transform_translates_and_pop_restores() {
        let mut state = FrameState::new();
        let pushed = state.push_transform(Matrix3x2::translation(10.0, 0.0));
        assert_eq!(pushed, Matrix3x2::translation(10.0, 0.0));
        assert_eq!(state.pop_transform().unwrap(), Matrix3x2::IDENTITY);
        assert_eq!(state.transform, Matrix3x2::IDENTITY);
        assert!(state.is_balanced());
    }

    // C#: D2DDrawContext.Begin
    #[test]
    fn reset_returns_to_new_state() {
        let mut state = FrameState::new();
        state.push_clip();
        state.push_opacity(0.3);
        state.push_transform(Matrix3x2::scale(3.0));
        state.reset();
        assert_eq!(state, FrameState::new());
    }

    // C#: System.Numerics `a * b` applies `a` first; `compose` is the contract's `multiply`
    #[test]
    fn matrix_multiply_applies_self_first_and_identity_is_neutral() {
        let a = Matrix3x2::scale(2.0);
        let b = Matrix3x2::translation(10.0, 5.0);
        assert_eq!(a.compose(b).transform_point(1.0, 1.0), (12.0, 7.0));
        assert_eq!(b.compose(a).transform_point(1.0, 1.0), (22.0, 12.0));
        assert_eq!(a.compose(Matrix3x2::IDENTITY), a);
        assert_eq!(Matrix3x2::IDENTITY.compose(a), a);
    }

    // C#: D2DDrawContext.ToD2D(RectD) computes Right/Bottom in double (trap T3)
    #[test]
    fn to_d2d_rect_computes_right_and_bottom_in_f64() {
        let r = to_d2d_rect(rect(0.1, 0.2, 0.2, 0.3));
        assert_eq!((r.left, r.top), (0.1_f64 as f32, 0.2_f64 as f32));
        assert_eq!(r.right, (0.1_f64 + 0.2_f64) as f32);
        assert_eq!(r.bottom, (0.2_f64 + 0.3_f64) as f32);
        // 2^24 + 1 is not an f32: adding after the cast would give 16777216 + 1 = 16777216 (ties to even).
        let r = to_d2d_rect(rect(16_777_217.0, 0.0, 1.0, 1.0));
        assert_eq!(r.left, 16_777_216.0);
        assert_eq!(r.right, 16_777_218.0);
    }

    // C#: D2DDrawContext.ToD2D(ColorF, opacity)
    #[test]
    fn to_d2d_color_multiplies_only_alpha() {
        let c = to_d2d_color(color(0.25, 0.5, 0.75, 0.8), 0.5);
        assert_eq!((c.r, c.g, c.b), (0.25, 0.5, 0.75));
        assert_eq!(c.a, 0.4);
        assert_eq!(to_d2d_color(color(1.0, 1.0, 1.0, 0.5), 1.0).a, 0.5);
    }

    // C#: D2DDrawContext.SetTransform: Dx = M31, Dy = M32, no transpose (trap T2)
    #[test]
    fn to_d2d_matrix_maps_fields_without_transposing() {
        let m = to_d2d_matrix(Matrix3x2::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0));
        assert_eq!(
            (m.M11, m.M12, m.M21, m.M22, m.M31, m.M32),
            (1.0, 2.0, 3.0, 4.0, 5.0, 6.0)
        );
    }

    // Adapter: RenderFault -> RenderError field by field
    #[test]
    fn render_error_converts_every_variant() {
        use crate::gfx::com::GfxError;
        assert_eq!(
            render_error(RenderFault::Disposed { object: "X" }),
            RenderError::Disposed { object: "X" }
        );
        assert_eq!(
            render_error(invalid("m")),
            RenderError::InvalidOperation { message: "m" }
        );
        assert_eq!(
            render_error(RenderFault::InvalidArgument {
                param: "p",
                message: "m"
            }),
            RenderError::InvalidArgument {
                param: "p",
                message: "m"
            }
        );
        assert_eq!(
            render_error(RenderFault::ArgumentOutOfRange { param: "p" }),
            RenderError::ArgumentOutOfRange { param: "p" }
        );
        assert_eq!(
            render_error(RenderFault::Device(GfxError::new("op", -7))),
            RenderError::Device {
                hresult: -7,
                operation: "op"
            }
        );
    }

    // ---------------------------------------------------------------- native

    // C#: D2DRenderSurfaceTests.Clear_DarkCanvas_ReadsBackThemeColor
    #[test]
    #[ignore = "native: needs desktop"]
    fn clear_dark_canvas_reads_back_theme_color() {
        let mut surface = surface(8, 4, 1.0);
        let pixels = render(&mut surface, |dc| {
            dc.clear(pr_app::viewport::render_geometry::dark_canvas())
        });
        assert_eq!(
            (pixels.width(), pixels.height(), pixels.layout()),
            (8, 4, PixelLayout::Pbgra32)
        );
        assert_eq!(pixel(&pixels, 7, 3), (0x10, 0x10, 0x10, 255));
    }

    // C#: D2DRenderSurfaceTests.Primitives_FillDrawRoundedClipAndOpacity (the exact pixel table)
    #[test]
    #[ignore = "native: needs desktop"]
    fn primitives_fill_draw_rounded_clip_and_opacity() {
        let mut surface = surface(40, 40, 1.0);
        let red = color(1.0, 0.0, 0.0, 1.0);
        let pixels = render(&mut surface, |dc| {
            dc.clear(BLACK);
            dc.fill_rectangle(rect(0.0, 0.0, 10.0, 10.0), WHITE);
            dc.draw_rectangle(rect(20.5, 0.5, 9.0, 9.0), red, 1.0);
            dc.fill_rounded_rectangle(rect(0.0, 20.0, 20.0, 20.0), 8.0, WHITE);
            dc.push_clip(rect(30.0, 30.0, 5.0, 5.0));
            dc.fill_rectangle(rect(20.0, 20.0, 20.0, 20.0), red);
            dc.pop_clip().unwrap();
            dc.push_opacity(0.5);
            dc.fill_rectangle(rect(10.0, 0.0, 5.0, 5.0), WHITE);
            dc.pop_opacity().unwrap();
        });
        assert_eq!(pixel(&pixels, 5, 5), (255, 255, 255, 255));
        assert_eq!(pixel(&pixels, 20, 5), (0, 0, 255, 255), "left edge of the red frame");
        assert_eq!(pixel(&pixels, 25, 5), (0, 0, 0, 255), "inside the frame: empty");
        assert_eq!(
            pixel(&pixels, 10, 30),
            (255, 255, 255, 255),
            "middle of the rounded shape"
        );
        assert_eq!(pixel(&pixels, 0, 20), (0, 0, 0, 255), "rounded corner: not filled");
        assert_eq!(pixel(&pixels, 32, 32), (0, 0, 255, 255), "inside the clip");
        assert_eq!(pixel(&pixels, 25, 25), (0, 0, 0, 255), "outside the clip");
        let (b, ..) = pixel(&pixels, 12, 2);
        assert!((125..=131).contains(&b), "opacity 0.5 gave {b}");
    }

    // C#: D2DRenderSurfaceTests.DrawImage_OpacityMultipliesWithPushedOpacity
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_opacity_multiplies_with_pushed_opacity() {
        let source = create_solid(4, 4, 255, 255, 255, 255);
        let mut surface = surface(4, 4, 1.0);
        let image = upload_single(surface.resources(), &source);
        let pixels = render(&mut surface, |dc| {
            dc.clear(BLACK);
            dc.push_opacity(0.5);
            dc.draw_image(&image, rect(0.0, 0.0, 4.0, 4.0), None, ImageInterpolation::Linear, 0.5);
            dc.pop_opacity().unwrap();
        });
        let (b, _, _, a) = pixel(&pixels, 1, 1);
        assert!((62..=66).contains(&b), "0.5 * 0.5 gave {b}");
        assert_eq!(a, 255);
    }

    // C#: D2DRenderSurfaceTests.DrawImage_EmptySourceOrDestination_IsSkipped (native regression for mutant 12e-3)
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_empty_source_or_destination_leaves_target_untouched() {
        let source = create_solid(4, 4, 0, 0, 255, 255);
        let clear = color(0.0, 1.0, 0.0, 1.0);
        let cleared = create_solid(8, 8, 0, 255, 0, 255);

        let mut source_empty_surface = surface(8, 8, 1.0);
        let source_empty_image = upload_single(source_empty_surface.resources(), &source);
        let source_empty = render(&mut source_empty_surface, |dc| {
            dc.clear(clear);
            dc.draw_image(
                &source_empty_image,
                rect(1.0, 1.0, 6.0, 6.0),
                Some(rect(2.0, 1.0, 0.0, 2.0)),
                ImageInterpolation::Linear,
                1.0,
            );
        });
        assert_identical(&cleared, &source_empty);

        let mut destination_empty_surface = surface(8, 8, 1.0);
        let destination_empty_image = upload_single(destination_empty_surface.resources(), &source);
        let destination_empty = render(&mut destination_empty_surface, |dc| {
            dc.clear(clear);
            dc.draw_image(
                &destination_empty_image,
                rect(1.0, 1.0, 0.0, 6.0),
                Some(rect(0.0, 0.0, 4.0, 4.0)),
                ImageInterpolation::Linear,
                1.0,
            );
        });
        assert_identical(&cleared, &destination_empty);

        let mut control_surface = surface(4, 4, 1.0);
        let control_image = upload_single(control_surface.resources(), &source);
        let control = render(&mut control_surface, |dc| {
            dc.clear(BLACK);
            dc.draw_image(
                &control_image,
                rect(0.0, 0.0, 4.0, 4.0),
                Some(rect(0.0, 0.0, 4.0, 4.0)),
                ImageInterpolation::NearestNeighbor,
                1.0,
            );
        });
        assert_identical(&source, &control);
    }
    // C#: D2DRenderSurfaceTests.PushTransform_Nested_AppliesInnerFirstThenOuter (real pixels)
    #[test]
    #[ignore = "native: needs desktop"]
    fn push_transform_nested_applies_inner_first_then_outer_pixels() {
        let mut surface = surface(30, 10, 1.0);
        let pixels = render(&mut surface, |dc| {
            dc.clear(BLACK);
            dc.push_transform(Matrix3x2::translation(10.0, 0.0)); // outer
            dc.push_transform(Matrix3x2::scale(2.0)); // inner: scale first, move after
            dc.fill_rectangle(rect(1.0, 1.0, 2.0, 2.0), WHITE); // x 2..6, then +10 = 12..16
            dc.pop_transform().unwrap();
            dc.pop_transform().unwrap();
        });
        assert_eq!(pixel(&pixels, 13, 3).0, 255);
        assert_eq!(pixel(&pixels, 15, 5).0, 255);
        assert_eq!(pixel(&pixels, 11, 3).0, 0);
        assert_eq!(pixel(&pixels, 23, 3).0, 0, "where the reversed order would draw");
    }

    // C#: D2DRenderSurfaceTests.PushOpacity_Nested_MultipliesAndPopRestoresOuterValue (real pixels)
    #[test]
    #[ignore = "native: needs desktop"]
    fn push_opacity_nested_multiplies_and_pop_restores_outer_value() {
        let mut surface = surface(30, 5, 1.0);
        let pixels = render(&mut surface, |dc| {
            dc.clear(BLACK);
            dc.push_opacity(0.5);
            dc.push_opacity(0.5);
            dc.fill_rectangle(rect(0.0, 0.0, 5.0, 5.0), WHITE); // 0.25
            dc.pop_opacity().unwrap();
            dc.fill_rectangle(rect(10.0, 0.0, 5.0, 5.0), WHITE); // 0.5
            dc.pop_opacity().unwrap();
            dc.fill_rectangle(rect(20.0, 0.0, 5.0, 5.0), WHITE); // 1
        });
        assert!((61..=67).contains(&pixel(&pixels, 2, 2).0));
        assert!((125..=131).contains(&pixel(&pixels, 12, 2).0));
        assert_eq!(pixel(&pixels, 22, 2).0, 255);
    }

    // C#: D2DRenderSurfaceTests.PushTransform_TranslatesAndPopRestores (real pixels)
    #[test]
    #[ignore = "native: needs desktop"]
    fn push_transform_translates_and_pop_restores_pixels() {
        let mut surface = surface(20, 10, 1.0);
        let pixels = render(&mut surface, |dc| {
            dc.clear(BLACK);
            dc.push_transform(Matrix3x2::translation(10.0, 0.0));
            dc.fill_rectangle(rect(0.0, 0.0, 5.0, 5.0), WHITE);
            dc.pop_transform().unwrap();
            dc.fill_rectangle(rect(0.0, 5.0, 5.0, 5.0), WHITE);
            assert_eq!(dc.transform(), Matrix3x2::IDENTITY);
        });
        assert_eq!(pixel(&pixels, 12, 2).0, 255);
        assert_eq!(pixel(&pixels, 2, 2).0, 0);
        assert_eq!(pixel(&pixels, 2, 7).0, 255);
    }

    /// C# `DWriteTestLayout`: a minimal DirectWrite layout for the test (the real text renderer is W8-12i).
    struct TestLayout {
        _format: IDWriteTextFormat,
        layout: IDWriteTextLayout,
    }

    impl TestLayout {
        fn new(text: &str, size: f32) -> Self {
            let factory = dwrite::create_factory().unwrap();
            let format = dwrite::create_text_format(&factory, "Segoe UI", true, size, "vi-VN").unwrap();
            let layout = dwrite::create_text_layout(&factory, text, &format, 200.0).unwrap();
            Self {
                _format: format,
                layout,
            }
        }
    }

    impl NativeTextLayout for TestLayout {
        fn native_layout(&self) -> &IDWriteTextLayout {
            &self.layout
        }
    }

    /// C# `ForeignLayout`: an `ITextLayout` that is not native.
    struct ForeignLayout;

    impl TextLayout for ForeignLayout {
        fn size(&self) -> SizeD {
            SizeD {
                width: 1.0,
                height: 1.0,
            }
        }

        fn text(&self) -> &str {
            "x"
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    // C#: D2DRenderSurfaceTests.DrawText_NativeLayout_ProducesInk_OtherLayoutRejected (ink half, then the adapter's rejection half)
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_text_native_layout_produces_ink_and_foreign_layout_is_rejected() {
        let mut surface = surface(120, 32, 1.0);
        let layout = TestLayout::new("Tiếng Việt", 18.0);
        let pixels = render(&mut surface, |dc| {
            dc.clear(WHITE);
            dc.draw_text(&layout, point(2.0, 2.0), BLACK);
            let rejected = DrawContext::draw_text(dc, &ForeignLayout, point(0.0, 0.0), BLACK);
            assert_eq!(
                rejected,
                Err(RenderError::InvalidArgument {
                    param: "layout",
                    message: "The text layout was not created by the Direct2D text renderer.",
                })
            );
        });
        let dark = (0..32)
            .flat_map(|y| (0..120).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel(&pixels, x, y).2 < 128)
            .count();
        assert!(dark > 30, "only {dark} dark pixels");
    }

    // C#: D2DRenderSurfaceTests.DrawImage_OneToOne_IsByteIdentical_EvenAtFractionalOffset (4 theory rows)
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_one_to_one_is_byte_identical_even_at_fractional_offset() {
        let rows = [
            (ImageInterpolation::NearestNeighbor, PixelLayout::Pbgra32),
            (ImageInterpolation::Linear, PixelLayout::Pbgra32),
            (ImageInterpolation::HighQualityCubic, PixelLayout::Pbgra32),
            (ImageInterpolation::HighQualityCubic, PixelLayout::Bgr32),
        ];
        for (interpolation, layout) in rows {
            let mut surface = surface(80, 60, 1.0);
            let source = if layout == PixelLayout::Pbgra32 {
                create_noise(80, 60, 7, true)
            } else {
                create_smooth(80, 60, PixelLayout::Bgr32)
            };
            let image = upload_single(surface.resources(), &source);
            // Shifted by 0.4 px: the renderer must move it to the pixel grid (no half-pixel interpolation at 100 %).
            let pixels = render(&mut surface, |dc| {
                dc.clear(TRANSPARENT);
                dc.draw_image(&image, rect(0.4, -0.4, 80.0, 60.0), None, interpolation, 1.0);
            });
            assert_identical(&crop(&source, 0, 0, 80, 60), &pixels);
        }
    }

    // C#: D2DRenderSurfaceTests.DrawImage_OneToOne_AtDpi150_IsByteIdentical
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_one_to_one_at_dpi_150_is_byte_identical() {
        let mut surface = surface(90, 60, 1.5);
        let source = create_noise(90, 60, 3, true);
        let image = upload_single(surface.resources(), &source);
        // ADR 0008: 100 % = 1 source pixel per device pixel = width / DpiScale DIP.
        let pixels = render(&mut surface, |dc| {
            dc.clear(TRANSPARENT);
            dc.draw_image(
                &image,
                rect(0.0, 0.0, 90.0 / 1.5, 60.0 / 1.5),
                None,
                ImageInterpolation::HighQualityCubic,
                1.0,
            );
        });
        assert_identical(&crop(&source, 0, 0, 90, 60), &pixels);
    }

    // C#: D2DRenderSurfaceTests.DrawImageOriented_MatchesPixelOpsApplyOrientation (orientations 1..8)
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_oriented_matches_pixel_ops_apply_orientation() {
        for orientation in 1..=8_i32 {
            let raw = create_noise(40, 24, orientation as u64, true);
            let reference = apply_orientation(crop(&raw, 0, 0, 40, 24), orientation).unwrap();
            let (rw, rh) = (reference.width(), reference.height());
            let mut surface = surface(rw + 6, rh + 4, 1.0);
            let image = upload_single(surface.resources(), &raw);
            let pixels = render(&mut surface, |dc| {
                dc.clear(TRANSPARENT);
                dc.draw_image_oriented(
                    &image,
                    rect(3.0, 2.0, f64::from(rw), f64::from(rh)),
                    orientation,
                    None,
                    ImageInterpolation::HighQualityCubic,
                    1.0,
                );
            });
            assert_identical(&reference, &crop(&pixels, 3, 2, rw, rh));
        }
    }

    // C#: D2DRenderSurfaceTests.DrawImage_FitDownscale_MatchesAreaReferenceByPsnr (3 rows)
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_fit_downscale_matches_area_reference_by_psnr() {
        let source = create_smooth(1200, 800, PixelLayout::Pbgra32);
        let rows = [
            (ImageInterpolation::HighQualityCubic, 300, 200, 40.0),
            (ImageInterpolation::HighQualityCubic, 449, 299, 40.0),
            (ImageInterpolation::Linear, 600, 400, 32.0),
        ];
        for (interpolation, width, height, min_psnr) in rows {
            let mut surface = surface(width, height, 1.0);
            let image = upload_single(surface.resources(), &source);
            let pixels = render(&mut surface, |dc| {
                dc.draw_image(
                    &image,
                    rect(0.0, 0.0, f64::from(width), f64::from(height)),
                    None,
                    interpolation,
                    1.0,
                );
            });
            let value = psnr(&resize_area(&source, width, height), &pixels);
            println!("PSNR downscale {interpolation:?} 1200x800->{width}x{height}: {value:.2} dB");
            assert!(
                value >= min_psnr,
                "PSNR {value:.2} dB < {min_psnr} ({interpolation:?} {width}x{height})"
            );
        }
    }

    // C#: D2DRenderSurfaceTests.DrawImage_Upscale_MatchesBilinearReferenceByPsnr
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_upscale_matches_bilinear_reference_by_psnr() {
        let source = create_smooth(120, 80, PixelLayout::Pbgra32);
        for (interpolation, min_psnr) in [
            (ImageInterpolation::Linear, 45.0),
            (ImageInterpolation::HighQualityCubic, 28.0),
        ] {
            let mut surface = surface(300, 200, 1.0);
            let image = upload_single(surface.resources(), &source);
            let pixels = render(&mut surface, |dc| {
                dc.draw_image(&image, rect(0.0, 0.0, 300.0, 200.0), None, interpolation, 1.0);
            });
            let value = psnr(&resize_bilinear(&source, 300, 200), &pixels);
            println!("PSNR upscale {interpolation:?} 2.5x: {value:.2} dB");
            assert!(value >= min_psnr, "PSNR {value:.2} dB < {min_psnr} ({interpolation:?})");
        }
    }

    // C#: D2DRenderSurfaceTests.DrawImage_NearestNeighborUpscale_ReplicatesSourcePixels
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_nearest_neighbor_upscale_replicates_source_pixels() {
        let source = create_noise(10, 6, 11, false);
        let mut surface = surface(40, 24, 1.0);
        let image = upload_single(surface.resources(), &source);
        let pixels = render(&mut surface, |dc| {
            dc.draw_image(
                &image,
                rect(0.0, 0.0, 40.0, 24.0),
                None,
                ImageInterpolation::NearestNeighbor,
                1.0,
            );
        });
        for y in 0..24 {
            for x in 0..40 {
                assert_eq!(pixel(&pixels, x, y), pixel(&source, x / 4, y / 4), "({x}, {y})");
            }
        }
    }

    // C#: D2DRenderSurfaceTests.DrawImage_SourceRectangle_PansAtOneToOne
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_source_rectangle_pans_at_one_to_one() {
        let source = create_noise(200, 120, 5, true);
        let mut surface = surface(64, 32, 1.0);
        let image = upload_single(surface.resources(), &source);
        let pixels = render(&mut surface, |dc| {
            dc.clear(TRANSPARENT);
            dc.draw_image(
                &image,
                rect(0.0, 0.0, 64.0, 32.0),
                Some(rect(101.0, 47.0, 64.0, 32.0)),
                ImageInterpolation::Linear,
                1.0,
            );
        });
        assert_identical(&crop(&source, 101, 47, 64, 32), &pixels);
    }

    // C#: D2DRenderSurfaceTests.DrawImage_SourceBeyondImage_DrawsOnlyIntersectionInPlace
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_source_beyond_image_draws_only_intersection_in_place() {
        let source = create_solid(10, 10, 0, 0, 255, 255);
        let mut surface = surface(20, 20, 1.0);
        let image = upload_single(surface.resources(), &source);
        // Source (-10, -10, 20, 20) -> destination 20x20: the image covers the lower-right quarter only.
        let pixels = render(&mut surface, |dc| {
            dc.clear(TRANSPARENT);
            dc.draw_image(
                &image,
                rect(0.0, 0.0, 20.0, 20.0),
                Some(rect(-10.0, -10.0, 20.0, 20.0)),
                ImageInterpolation::NearestNeighbor,
                1.0,
            );
        });
        assert_eq!(pixel(&pixels, 9, 9), (0, 0, 0, 0));
        assert_eq!(pixel(&pixels, 10, 10), (0, 0, 255, 255));
        assert_eq!(pixel(&pixels, 19, 19), (0, 0, 255, 255));
    }

    // C#: D2DRenderSurfaceTests.TiledImage_ForcedSmallTiles_MatchesSingleTexture (7 rows; the cache-driven twin is W8-12g)
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_with_forced_small_tiles_matches_a_single_texture() {
        let source = create_smooth(500, 300, PixelLayout::Pbgra32);
        let rows = [
            (ImageInterpolation::NearestNeighbor, 1.0),
            (ImageInterpolation::Linear, 1.0),
            (ImageInterpolation::Linear, 0.5),
            (ImageInterpolation::Linear, 1.7),
            (ImageInterpolation::HighQualityCubic, 0.37),
            (ImageInterpolation::HighQualityCubic, 0.5),
            (ImageInterpolation::HighQualityCubic, 1.7),
        ];
        for (interpolation, scale) in rows {
            let width = (500.0_f64 * scale).round() as i32;
            let height = (300.0_f64 * scale).round() as i32;
            let mut surface = surface(width, height, 1.0);
            let single = upload_single(surface.resources(), &source);
            let tiled = upload(surface.resources(), &source, 128, 16);
            assert!(tiled.tiles().len() > 9, "test must actually tile");
            let target = rect(0.0, 0.0, f64::from(width), f64::from(height));
            let expected = render(&mut surface, |dc| {
                dc.draw_image(&single, target, None, interpolation, 1.0)
            });
            let actual = render(&mut surface, |dc| {
                dc.draw_image(&tiled, target, None, interpolation, 1.0)
            });
            let value = psnr(&expected, &actual);
            println!("PSNR tiled vs single {interpolation:?} x{scale}: {value:.2} dB");
            if scale == 1.0 {
                assert_identical(&expected, &actual);
            } else {
                assert!(
                    value >= 45.0,
                    "PSNR {value:.2} dB: tile seams visible ({interpolation:?} x{scale})"
                );
            }
        }
    }

    // C#: D2DRenderSurfaceTests.EvictedImage_IsSkippedNotDrawn (without the cache: a released image)
    #[test]
    #[ignore = "native: needs desktop"]
    fn released_image_is_skipped_and_counted() {
        let source = create_solid(4, 4, 255, 0, 0, 255);
        let mut surface = surface(4, 4, 1.0);
        let image = upload_single(surface.resources(), &source);
        release(surface.resources(), &image);
        assert_eq!(surface.resources().live_bitmap_count(), 0);
        let pixels = render(&mut surface, |dc| {
            dc.clear(TRANSPARENT);
            dc.draw_image(&image, rect(0.0, 0.0, 4.0, 4.0), None, ImageInterpolation::Linear, 1.0);
            assert_eq!(dc.skipped_image_draws(), 1);
        });
        assert_eq!(pixel(&pixels, 2, 2), (0, 0, 0, 0));
    }

    // C#: D2DRenderSurfaceTests.DeviceLost_RecreateTarget... (the "older generation is skipped" half)
    #[test]
    #[ignore = "native: needs desktop"]
    fn image_of_an_older_generation_is_skipped_after_recreate() {
        let source = create_noise(16, 16, 9, true);
        let mut surface = surface(16, 16, 1.0);
        let before = upload_single(surface.resources(), &source);
        surface.recreate_device().unwrap();
        assert_eq!(surface.device_generation(), 1);
        let pixels = render(&mut surface, |dc| {
            dc.clear(TRANSPARENT);
            dc.draw_image(
                &before,
                rect(0.0, 0.0, 16.0, 16.0),
                None,
                ImageInterpolation::Linear,
                1.0,
            );
            assert_eq!(dc.skipped_image_draws(), 1);
        });
        assert_eq!(pixel(&pixels, 3, 3), (0, 0, 0, 0));
        // Uploading again on the new device draws.
        let after = upload_single(surface.resources(), &source);
        let pixels = render(&mut surface, |dc| {
            dc.draw_image(
                &after,
                rect(0.0, 0.0, 16.0, 16.0),
                None,
                ImageInterpolation::Linear,
                1.0,
            );
        });
        assert_identical(&source, &pixels);
    }

    // New: a value that is not a TiledGpuImage<ID2D1Bitmap1> is skipped (C# `image is not TiledGpuImage`), T7
    #[test]
    #[ignore = "native: needs desktop"]
    fn foreign_image_type_is_skipped() {
        let mut surface = surface(4, 4, 1.0);
        let pixels = render(&mut surface, |dc| {
            dc.clear(TRANSPARENT);
            dc.draw_image(&7_u32, rect(0.0, 0.0, 4.0, 4.0), None, ImageInterpolation::Linear, 1.0);
            // a tiled image of another bitmap type (the fake backend of the cache tests) is foreign too
            let fake = TiledGpuImage::<()>::new(&create_solid(4, 4, 1, 2, 3, 255), Vec::new(), 0);
            fake.set_uploaded_rows(4);
            dc.draw_image(&fake, rect(0.0, 0.0, 4.0, 4.0), None, ImageInterpolation::Linear, 1.0);
            assert_eq!(dc.skipped_image_draws(), 2);
        });
        assert_eq!(pixel(&pixels, 1, 1), (0, 0, 0, 0));
    }

    // New (T8): the pop of `draw_image_oriented` runs even when `draw_image` returns early
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_image_oriented_pops_the_transform_even_when_the_image_is_skipped() {
        let source = create_solid(4, 4, 1, 2, 3, 255);
        let mut surface = surface(8, 8, 1.0);
        let image = upload_single(surface.resources(), &source);
        release(surface.resources(), &image);
        let context = surface.begin_draw_context().unwrap();
        context.draw_image_oriented(
            &image,
            rect(0.0, 0.0, 4.0, 4.0),
            6,
            None,
            ImageInterpolation::Linear,
            1.0,
        );
        assert_eq!(context.skipped_image_draws(), 1);
        assert_eq!(context.transform(), Matrix3x2::IDENTITY);
        assert!(
            context.pop_transform().is_err(),
            "nothing is left on the transform stack"
        );
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Presented);
        // an empty destination also returns early inside `draw_image`
        let fresh = upload_single(surface.resources(), &source);
        let context = surface.begin_draw_context().unwrap();
        context.draw_image_oriented(
            &fresh,
            rect(0.0, 0.0, 0.0, 0.0),
            6,
            None,
            ImageInterpolation::Linear,
            1.0,
        );
        assert_eq!(context.transform(), Matrix3x2::IDENTITY);
        assert_eq!(
            context.skipped_image_draws(),
            1,
            "the counter lives as long as the surface; this draw was not skipped"
        );
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Presented);
    }

    // New: the DrawContext adapter reaches the same code through `&dyn GpuImage` and `as_oriented`
    #[test]
    #[ignore = "native: needs desktop"]
    fn draw_context_trait_adapter_draws_and_exposes_the_oriented_context() {
        let source = create_noise(12, 8, 4, true);
        let reference = apply_orientation(crop(&source, 0, 0, 12, 8), 6).unwrap();
        let mut surface = surface(8, 12, 1.0);
        let image = upload_single(surface.resources(), &source);
        let dyn_surface: &mut dyn RenderSurface = &mut surface;
        let context = dyn_surface.begin_draw().unwrap();
        context.clear(TRANSPARENT);
        let oriented = context
            .as_oriented()
            .expect("the Direct2D context draws oriented images");
        oriented.draw_image_oriented(
            &image,
            rect(0.0, 0.0, 8.0, 12.0),
            6,
            None,
            ImageInterpolation::HighQualityCubic,
            1.0,
        );
        let pixels = context.try_read_pixels().unwrap().unwrap();
        assert_eq!(
            context.pop_clip(),
            Err(RenderError::InvalidOperation {
                message: "PopClip without PushClip."
            })
        );
        assert_eq!(dyn_surface.end_draw_and_present().unwrap(), PresentResult::Presented);
        assert_identical(&reference, &pixels);
    }

    // C#: TryReadPixels on a window surface returns false
    #[test]
    #[ignore = "native: needs desktop"]
    fn try_read_pixels_is_none_on_a_window_surface() {
        let window = HiddenWindow::new();
        let options = pr_app::services::render::RenderSurfaceOptions {
            prefer_warp: true,
            ..Default::default()
        };
        let mut surface = D2dRenderSurface::create_window_surface(window.raw(), options).unwrap();
        let context = surface.begin_draw_context().unwrap();
        context.clear(WHITE);
        assert!(context.try_read_pixels().unwrap().is_none());
        surface.end_draw_and_present().unwrap();
    }

    // C#: TryReadPixels returns the surface size in PIXELS, Pbgra32 (contract: 8x4 at DPI 1.25 is an 8x4 buffer)
    #[test]
    #[ignore = "native: needs desktop"]
    fn try_read_pixels_returns_a_pbgra32_buffer_of_the_surface_size() {
        let mut surface = surface(8, 4, 1.25);
        let pixels = render(&mut surface, |dc| dc.clear(color(0.0, 1.0, 0.0, 1.0)));
        assert_eq!(
            (pixels.width(), pixels.height(), pixels.layout()),
            (8, 4, PixelLayout::Pbgra32)
        );
        assert_eq!(pixel(&pixels, 7, 3), (0, 255, 0, 255));
        // two reads give independent buffers
        let again = render(&mut surface, |dc| dc.clear(BLACK));
        assert_ne!(again.id(), pixels.id());
        assert_eq!(pixel(&pixels, 0, 0), (0, 255, 0, 255));
    }

    // C#: D2DDrawContext.End pops dangling clips; EndDrawAndPresent then reports the unbalanced frame (with 12d)
    #[test]
    #[ignore = "native: needs desktop"]
    fn end_pops_dangling_clips_and_reports_unbalanced() {
        let mut surface = surface(8, 8, 1.0);
        let context = surface.begin_draw_context().unwrap();
        context.push_clip(rect(0.0, 0.0, 4.0, 4.0));
        context.push_clip(rect(1.0, 1.0, 2.0, 2.0));
        assert_eq!(
            surface.end_draw_and_present(),
            Err(invalid(
                "Unbalanced PushClip/PopClip or PushOpacity/PopOpacity in the frame."
            ))
        );
        // Direct2D's EndDraw succeeded (the clips were popped), so the next frame works and nothing leaks from the old one.
        let pixels = render(&mut surface, |dc| dc.clear(color(0.0, 1.0, 0.0, 1.0)));
        assert_eq!(pixel(&pixels, 7, 7).1, 255);
        let context = surface.begin_draw_context().unwrap();
        assert_eq!(context.transform(), Matrix3x2::IDENTITY);
        assert!(context.pop_clip().is_err(), "the clip depth was reset by `begin`");
        surface.end_draw_and_present().unwrap();
    }

    // Pending opacity/transform pushes are NOT unwound by `end`; `begin` resets them (contract note 1)
    #[test]
    #[ignore = "native: needs desktop"]
    fn begin_resets_opacity_and_transform_left_by_an_unbalanced_frame() {
        let mut surface = surface(8, 8, 1.0);
        let context = surface.begin_draw_context().unwrap();
        context.push_opacity(0.25);
        context.push_transform(Matrix3x2::scale(3.0));
        assert!(surface.end_draw_and_present().is_err());
        let pixels = render(&mut surface, |dc| {
            assert_eq!(dc.transform(), Matrix3x2::IDENTITY);
            dc.clear(BLACK);
            dc.fill_rectangle(rect(0.0, 0.0, 8.0, 8.0), WHITE);
        });
        assert_eq!(pixel(&pixels, 4, 4), (255, 255, 255, 255), "opacity is 1 again");
    }
}
