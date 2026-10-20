//! Port of `PhotoReview.Shell.Rendering/Contracts/RenderSurfaceContracts.cs` (94 lines, C-09, baseline d92d6aae): the Direct2D
//! render surface, its factory and the draw context, as traits. Draw coordinates are DIP (the surface calls `SetDpi`), surface
//! sizes are pixels.
//!
//! No `windows` types (D-W8-3): the window handle is an `isize` (C# `nint`), the waitable object likewise. The real surface is
//! `pr_platform_win::render::surface` (W8-12d), the real draw context `render::draw_context` (W8-12e); consumers
//! (`PresentationComposer`, overlay) are tested with a recording fake `DrawContext`.
//! `ColorF` and `ImageInterpolation` (also in the C# file) are owned by W8-01c, `viewport::render_geometry` (D-W8c-1).
//!
//! Calls that cannot fail in C# return `()`. "Outside BeginDraw/EndDraw" (C# `InvalidOperationException`) cannot happen: the
//! `&mut dyn DrawContext` of `begin_draw` borrows the surface.

use pr_imaging::pixels::PixelBuffer;

use super::error::RenderError;
use super::gpu_image::GpuImage;
use super::text::TextLayout;
use crate::input::primitives::{PointD, RectD};
use crate::viewport::render_geometry::{ColorF, ImageInterpolation};

/// `PresentResult` (C# values 0..=3, same order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentResult {
    Presented = 0,
    Occluded = 1,
    /// The device was lost and has been rebuilt: the caller must redraw the frame.
    DeviceRecreated = 2,
    Failed = 3,
}

/// `RenderSurfaceOptions(bool PreferWarp = false, bool AllowTearing = false, int MaxFrameLatency = 1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSurfaceOptions {
    pub prefer_warp: bool,
    pub allow_tearing: bool,
    pub max_frame_latency: i32,
}

impl Default for RenderSurfaceOptions {
    /// `prefer_warp = false`, `allow_tearing = false`, `max_frame_latency = 1` (NOT the derived 0).
    fn default() -> Self {
        Self {
            prefer_warp: false,
            allow_tearing: false,
            max_frame_latency: 1,
        }
    }
}

/// Identifies one `DeviceRecreated` subscription (replaces C# delegate identity for `-=`; same pattern as `FrameHandlerId`).
pub type DeviceRecreatedHandlerId = u64;

/// A `DeviceRecreated` subscriber. Runs on the UI thread (no `Send` bound).
pub type DeviceRecreatedHandler = Box<dyn FnMut()>;

/// `IRenderSurface` (C-09). The C# `IDisposable` is `Drop` of the implementation (idempotent release, UI thread). Methods
/// that touch the device must be called on the thread that created the surface; a call from another thread is
/// `Err(InvalidOperation { message: "D2DRenderSurface must be used on the thread that created it." })`.
pub trait RenderSurface {
    /// `Hwnd` (`nint`); 0 for an offscreen surface.
    fn hwnd(&self) -> isize;

    /// `PixelWidth`, in pixels.
    fn pixel_width(&self) -> i32;

    /// `PixelHeight`, in pixels.
    fn pixel_height(&self) -> i32;

    /// `DpiScale` = `GetDpiForWindow / 96`.
    fn dpi_scale(&self) -> f64;

    /// `FrameLatencyWaitHandle` (`nint`) for the message loop (C-05); 0 when the surface has no swap chain (offscreen / WARP).
    fn frame_latency_wait_handle(&self) -> isize;

    /// `IsWarp`: true when the surface fell back to WARP (RDP, no GPU) or is a test surface.
    fn is_warp(&self) -> bool;

    /// `Resize` = `ResizeBuffers`; UI thread. `Err(InvalidOperation)` between `begin_draw` and `end_draw_and_present`
    /// (`"Resize is not allowed between BeginDraw and EndDrawAndPresent."`). A device loss inside is repaired (recreate +
    /// `DeviceRecreated` handlers), not reported.
    fn resize(&mut self, pixel_width: i32, pixel_height: i32, dpi_scale: f64) -> Result<(), RenderError>;

    /// `BeginDraw()`: DIP -> px through `SetDpi`. The draw context borrows the surface until the next
    /// `end_draw_and_present` (C# returns the same long-lived object; Rust expresses "valid only inside the frame" in the borrow).
    /// A second call before `end_draw_and_present` is
    /// `Err(InvalidOperation { "BeginDraw was already called; call EndDrawAndPresent first." })`.
    fn begin_draw(&mut self) -> Result<&mut dyn DrawContext, RenderError>;

    /// `EndDrawAndPresent()`: `EndDraw` then `Present(1, 0)` (sync 0 when `allow_tearing` or the immediate-present switch is on).
    /// An `EndDraw` HRESULT < 0 that is not a device loss gives `Ok(Failed)`; `DXGI_STATUS_OCCLUDED` gives `Ok(Occluded)`; no swap chain gives `Ok(Presented)`.
    /// `D2DERR_RECREATE_TARGET` / `DXGI_ERROR_DEVICE_REMOVED` -> recreate everything, raise `DeviceRecreated`, `Ok(DeviceRecreated)`.
    /// `Err(InvalidOperation { "EndDrawAndPresent without BeginDraw." })` without `begin_draw`; and, AFTER presenting,
    /// `Err(InvalidOperation { "Unbalanced PushClip/PopClip or PushOpacity/PopOpacity in the frame." })` when the frame left pushes open.
    fn end_draw_and_present(&mut self) -> Result<PresentResult, RenderError>;

    /// C# `event EventHandler? DeviceRecreated +=`. The `GpuImageCache` must clear everything in its handler. Handlers run
    /// synchronously, on the UI thread, inside `resize` / `end_draw_and_present`, after the new device is up.
    fn subscribe_device_recreated(&mut self, handler: DeviceRecreatedHandler) -> DeviceRecreatedHandlerId;

    /// C# `DeviceRecreated -=`. An unknown id is a no-op.
    fn unsubscribe_device_recreated(&mut self, id: DeviceRecreatedHandlerId);
}

/// `IRenderSurfaceFactory` (C-09 v1.1): where surfaces are created, injected instead of the two `static abstract` members.
/// The real implementation is a stateless 3-line wrapper in W8-12d (C# `D2DRenderSurfaceFactory`, 13 lines).
pub trait RenderSurfaceFactory {
    /// `CreateForWindow(nint hwnd, RenderSurfaceOptions options)`.
    fn create_for_window(
        &self,
        hwnd: isize,
        options: RenderSurfaceOptions,
    ) -> Result<Box<dyn RenderSurface>, RenderError>;

    /// `CreateOffscreen(int, int, double)`: WARP, for tests.
    fn create_offscreen(
        &self,
        pixel_width: i32,
        pixel_height: i32,
        dpi_scale: f64,
    ) -> Result<Box<dyn RenderSurface>, RenderError>;
}

/// The default of the C# optional parameter `float opacity = 1f` of `DrawImage` (Rust has no default arguments).
pub const DEFAULT_IMAGE_OPACITY: f32 = 1.0;

/// `IDrawContext` (C-09). Valid between `begin_draw` and `end_draw_and_present`. Coordinates are DIP.
pub trait DrawContext {
    /// `Clear(ColorF)`.
    fn clear(&mut self, color: ColorF);

    /// `DrawImage(IGpuImage, RectD destination, RectD? sourcePixels, ImageInterpolation, float opacity = 1f)`.
    /// An image that is not the implementation's own, was evicted, is incomplete or belongs to an older device generation is
    /// SKIPPED (counted, not drawn, no error). Pass [`DEFAULT_IMAGE_OPACITY`] for C# `1f`.
    fn draw_image(
        &mut self,
        image: &dyn GpuImage,
        destination: RectD,
        source_pixels: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    );

    /// `FillRectangle(RectD, ColorF)`.
    fn fill_rectangle(&mut self, rect: RectD, color: ColorF);

    /// `FillRoundedRectangle(RectD, double radius, ColorF)`.
    fn fill_rounded_rectangle(&mut self, rect: RectD, radius: f64, color: ColorF);

    /// `DrawRectangle(RectD, ColorF, double strokeWidth)`.
    fn draw_rectangle(&mut self, rect: RectD, color: ColorF, stroke_width: f64);

    /// `DrawText(ITextLayout, PointD origin, ColorF)`. The layout must come from the shell's own text renderer, otherwise
    /// `Err(InvalidArgument { param: "layout", message: "The text layout was not created by the Direct2D text renderer." })`.
    fn draw_text(&mut self, layout: &dyn TextLayout, origin: PointD, color: ColorF) -> Result<(), RenderError>;

    /// `PushClip(RectD)`.
    fn push_clip(&mut self, rect: RectD);

    /// `PopClip()`. Without a matching `push_clip`: `Err(InvalidOperation { "PopClip without PushClip." })`.
    fn pop_clip(&mut self) -> Result<(), RenderError>;

    /// `PushOpacity(float)`.
    fn push_opacity(&mut self, opacity: f32);

    /// `PopOpacity()`. Without a matching `push_opacity`: `Err(InvalidOperation { "PopOpacity without PushOpacity." })`.
    fn pop_opacity(&mut self) -> Result<(), RenderError>;

    /// `TryReadPixels(out PixelBuffer)`: offscreen / WARP only (golden G-PIX). `Ok(Some(buffer))` (a fresh `Pbgra32` buffer the
    /// caller owns), `Ok(None)` (C# `false`) on a window surface, `Err(Device)` when the GPU readback fails.
    fn try_read_pixels(&mut self) -> Result<Option<PixelBuffer>, RenderError>;

    /// NOT in the C# interface (D-W8-12a-5): the Rust form of `context is D2DDrawContext oriented` in `ImageLayer.TryDraw`
    /// (`Shell.Win32/Viewing/ImageLayer.cs` line 43). The real draw context returns `Some(self)`; a fake that cannot draw
    /// oriented images keeps this default `None`, and the caller then falls back to a plain `draw_image` like C#.
    fn as_oriented(&mut self) -> Option<&mut dyn OrientedDrawContext> {
        None
    }
}

/// The part of the C# `D2DDrawContext` that trait-typed callers use beyond `IDrawContext`: `DrawImageOriented`.
/// (`PushTransform`, `PopTransform`, `Transform`, `SkippedImageDraws` stay on the concrete type, W8-12e.)
pub trait OrientedDrawContext: DrawContext {
    /// `DrawImageOriented(IGpuImage, RectD destination, int exifOrientation, RectD? sourcePixels, ImageInterpolation, float opacity = 1f)`:
    /// draws the RAW (unrotated) image so that it appears with EXIF orientation `exif_orientation` (1..=8) inside `destination`
    /// (push `for_exif_orientation`, draw into `unoriented_rect`, pop in a `finally`-equivalent). `source_pixels` is in raw-image pixels.
    fn draw_image_oriented(
        &mut self,
        image: &dyn GpuImage,
        destination: RectD,
        exif_orientation: i32,
        source_pixels: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    );
}
