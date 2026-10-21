//! Port of `PhotoReview.Shell.Interop/Graphics/D2D1.cs` (476 lines, WP-13b), baseline `d92d6aae`. Unit W8-11h (audit; thin helpers).
//!
//! Direct2D 1.1 (`ID2D1Factory1`, `ID2D1Device`, `ID2D1DeviceContext`, `ID2D1Bitmap1`, brushes, every struct and enum) comes from
//! `windows-rs`; the 140 hand-written slots, the `Reserved` placeholders and the ABI notes of the C# file are retired. What stays is
//! the device/target recipe of `DeviceResources` (device context, target bitmap properties, swap chain surface to bitmap) and two
//! safe wrappers for the calls that take raw pointers (`CopyFromMemory`, CPU read-back). Table: `contracts/W8-11h.md`.
//!
//! Thread affinity: the factory is `D2D1_FACTORY_TYPE_SINGLE_THREADED`, so a factory and everything created from it is used only
//! on the thread that created it (the UI thread, or the keep-warm thread for its own offscreen device).

#![allow(unused_variables, dead_code)] // W8-11h stub: the porter removes this line when the last `todo!()` is gone.

use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_U, D2D1_ALPHA_MODE, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS, D2D1_BITMAP_PROPERTIES1, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISwapChain1};

use super::com::GfxError;

/// Logical DPI of an unscaled surface (C# `96 * DpiScale`).
pub const BASE_DPI: f32 = 96.0;

/// `D2D1CreateFactory::<ID2D1Factory1>(D2D1_FACTORY_TYPE_SINGLE_THREADED, { debugLevel })` (C# `D2D1.CreateFactory(SingleThreaded)`).
/// `debug_level` is `D2D1_DEBUG_LEVEL` as a number (0 none, the only value the renderer uses). The multi-threaded factory is not offered.
pub fn create_factory(debug_level: u32) -> Result<ID2D1Factory1, GfxError> {
    todo!("W8-11h")
}

/// `DeviceResources.CreateDevice` lines 279 to 282: `factory.CreateDevice(dxgi_device)`, then
/// `device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)`.
pub fn create_device_and_context(
    factory: &ID2D1Factory1,
    dxgi_device: &IDXGIDevice,
) -> Result<(ID2D1Device, ID2D1DeviceContext), GfxError> {
    todo!("W8-11h")
}

/// `context.CreateSolidColorBrush(&color, None)`; the renderer keeps ONE white brush and changes colour/opacity per call.
pub fn create_solid_color_brush(
    context: &ID2D1DeviceContext,
    color: D2D1_COLOR_F,
) -> Result<ID2D1SolidColorBrush, GfxError> {
    todo!("W8-11h")
}

/// `context.GetMaximumBitmapSize()` clamped to `i32::MAX` (C# `Math.Min(.., int.MaxValue)`; 16384 on feature level 11, 4096 on 9.3).
#[must_use]
pub fn max_bitmap_size(context: &ID2D1DeviceContext) -> i32 {
    todo!("W8-11h")
}

/// Properties of the render target bitmap (pure; C# `DeviceResources.CreateTarget`): `B8G8R8A8_UNORM`; alpha `PREMULTIPLIED` for an
/// offscreen target and `IGNORE` for a swap chain back buffer; `dpiX = dpiY = (96.0 * dpi_scale) as f32`; options `TARGET`, plus
/// `CANNOT_DRAW` when `has_swap_chain`; no colour context.
#[must_use]
pub fn target_bitmap_properties(has_swap_chain: bool, dpi_scale: f64) -> D2D1_BITMAP_PROPERTIES1 {
    todo!("W8-11h")
}

/// Properties of an image bitmap (pure; C# `DeviceResources.CreateBitmap`): `B8G8R8A8_UNORM`, the given alpha mode
/// (`PREMULTIPLIED` for `Pbgra32`, `IGNORE` for `Bgr32`), 96 dpi, the given options, no colour context.
#[must_use]
pub fn image_bitmap_properties(alpha_mode: D2D1_ALPHA_MODE, options: D2D1_BITMAP_OPTIONS) -> D2D1_BITMAP_PROPERTIES1 {
    todo!("W8-11h")
}

/// `DeviceResources.CreateTarget`: with a swap chain, `GetBuffer(0)` as `IDXGISurface` and `CreateBitmapFromDxgiSurface(surface,
/// target_bitmap_properties(true, ..))`; without, `CreateBitmap(size, None, 0, target_bitmap_properties(false, ..))`. The surface is
/// dropped before returning (the bitmap holds its own reference to the back buffer). It does NOT call `SetTarget`
/// (use [`attach_target`]).
pub fn create_target_bitmap(
    context: &ID2D1DeviceContext,
    swap_chain: Option<&IDXGISwapChain1>,
    pixel_width: u32,
    pixel_height: u32,
    dpi_scale: f64,
) -> Result<ID2D1Bitmap1, GfxError> {
    todo!("W8-11h")
}

/// `context.SetTarget(bitmap)` then `context.SetDpi(d, d)` with `d = (96.0 * dpi_scale) as f32` (C# `CreateTarget` last two lines).
pub fn attach_target(context: &ID2D1DeviceContext, bitmap: &ID2D1Bitmap1, dpi_scale: f64) {
    todo!("W8-11h")
}

/// `context.SetTarget(None)` (C# `ReleaseTarget` first line). The caller drops its target bitmap right after; only then is the back
/// buffer free for `ResizeBuffers`.
pub fn detach_target(context: &ID2D1DeviceContext) {
    todo!("W8-11h")
}

/// `CreateBitmap(size, None, 0, image_bitmap_properties(..))`: an image bitmap with undefined contents, filled later by
/// [`copy_from_memory`] (C# `CreateBitmap(width, height, null, 0, alphaMode, options)`).
pub fn create_bitmap_uninitialized(
    context: &ID2D1DeviceContext,
    pixel_width: u32,
    pixel_height: u32,
    alpha_mode: D2D1_ALPHA_MODE,
    options: D2D1_BITMAP_OPTIONS,
) -> Result<ID2D1Bitmap1, GfxError> {
    todo!("W8-11h")
}

/// Safe `ID2D1Bitmap::CopyFromMemory(dest, pixels, pitch)`. `dest = None` is the whole bitmap. Checked BEFORE the call (the C# took a
/// `void*` and trusted the caller): `pitch >= rect width * 4` and `pixels.len() >= pitch * (rect height - 1) + rect width * 4`, else
/// `Err(E_INVALIDARG)` named `"CopyFromMemory"` (never a read past the slice).
pub fn copy_from_memory(
    bitmap: &ID2D1Bitmap1,
    dest: Option<D2D_RECT_U>,
    pixels: &[u8],
    pitch: u32,
) -> Result<(), GfxError> {
    todo!("W8-11h")
}

/// CPU read-back of a render target (C# `D2D1` test helper `ReadPixels`, `D2DDrawContext.TryReadPixels`, `DeviceResources.Synchronize`):
/// creates a `CPU_READ | CANNOT_DRAW` bitmap of `width x height`, `CopyFromBitmap(None, source, None)`, `Map(D2D1_MAP_OPTIONS_READ)`,
/// copies the rows into a TIGHT `width * 4` buffer (BGRA, premultiplied; the pitch of the mapped rect is not the row length), and
/// `Unmap`s on every path (also when the copy fails). Must not be called between `BeginDraw` and `EndDraw` on `source`.
pub fn read_pixels(
    context: &ID2D1DeviceContext,
    source: &ID2D1Bitmap1,
    pixel_width: u32,
    pixel_height: u32,
) -> Result<Vec<u8>, GfxError> {
    todo!("W8-11h")
}

/// `context.EndDraw(None, None)` as the HRESULT number (0 on success): the form the `EndDrawFault` test seam of `DeviceResources`
/// rewrites. The caller maps it with `com::is_device_lost`.
#[must_use]
pub fn end_draw_hresult(context: &ID2D1DeviceContext) -> i32 {
    todo!("W8-11h")
}
