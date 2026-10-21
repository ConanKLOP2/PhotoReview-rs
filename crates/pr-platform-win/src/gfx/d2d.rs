//! Port of `PhotoReview.Shell.Interop/Graphics/D2D1.cs` (476 lines, WP-13b), baseline `d92d6aae`. Unit W8-11h (audit; thin helpers).
//!
//! Direct2D 1.1 (`ID2D1Factory1`, `ID2D1Device`, `ID2D1DeviceContext`, `ID2D1Bitmap1`, brushes, every struct and enum) comes from
//! `windows-rs`; the 140 hand-written slots, the `Reserved` placeholders and the ABI notes of the C# file are retired. What stays is
//! the device/target recipe of `DeviceResources` (device context, target bitmap properties, swap chain surface to bitmap) and two
//! safe wrappers for the calls that take raw pointers (`CopyFromMemory`, CPU read-back). Table: `contracts/W8-11h.md`.
//!
//! Thread affinity: the factory is `D2D1_FACTORY_TYPE_SINGLE_THREADED`, so a factory and everything created from it is used only
//! on the thread that created it (the UI thread, or the keep-warm thread for its own offscreen device).

use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_U, D2D_SIZE_U, D2D1_ALPHA_MODE, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEBUG_LEVEL, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_OPTIONS,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_MAP_OPTIONS_READ, D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device,
    ID2D1DeviceContext, ID2D1Factory1, ID2D1Image, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface, IDXGISwapChain1};

use super::com::{GfxError, ResultExt};
use super::dxgi::back_buffer;

/// Logical DPI of an unscaled surface (C# `96 * DpiScale`).
pub const BASE_DPI: f32 = 96.0;

/// `D2D1CreateFactory::<ID2D1Factory1>(D2D1_FACTORY_TYPE_SINGLE_THREADED, { debugLevel })` (C# `D2D1.CreateFactory(SingleThreaded)`).
/// `debug_level` is `D2D1_DEBUG_LEVEL` as a number (0 none, the only value the renderer uses). The multi-threaded factory is not offered.
pub fn create_factory(debug_level: u32) -> Result<ID2D1Factory1, GfxError> {
    let options = D2D1_FACTORY_OPTIONS {
        debugLevel: D2D1_DEBUG_LEVEL(i32::try_from(debug_level).unwrap_or(0)),
    };
    // SAFETY: `options` is a local that outlives the call; the factory type is a constant. The factory is single threaded: the
    // caller keeps it and everything created from it on this thread.
    unsafe { D2D1CreateFactory::<ID2D1Factory1>(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&options)) }
        .gfx("D2D1CreateFactory")
}

/// `DeviceResources.CreateDevice` lines 279 to 282: `factory.CreateDevice(dxgi_device)`, then
/// `device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)`.
pub fn create_device_and_context(
    factory: &ID2D1Factory1,
    dxgi_device: &IDXGIDevice,
) -> Result<(ID2D1Device, ID2D1DeviceContext), GfxError> {
    // SAFETY: `factory` and `dxgi_device` are live owned interfaces borrowed for the call, used on the factory's thread.
    let device = unsafe { factory.CreateDevice(dxgi_device) }.gfx("ID2D1Factory1::CreateDevice")?;
    // SAFETY: `device` is the live owned device created above.
    let context = unsafe { device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) }
        .gfx("ID2D1Device::CreateDeviceContext")?;
    Ok((device, context))
}

/// `context.CreateSolidColorBrush(&color, None)`; the renderer keeps ONE white brush and changes colour/opacity per call.
pub fn create_solid_color_brush(
    context: &ID2D1DeviceContext,
    color: D2D1_COLOR_F,
) -> Result<ID2D1SolidColorBrush, GfxError> {
    // SAFETY: `color` is a local that outlives the call; the brush properties are optional (None); `context` is a live owned interface.
    unsafe { context.CreateSolidColorBrush(&color, None) }.gfx("ID2D1DeviceContext::CreateSolidColorBrush")
}

/// `context.GetMaximumBitmapSize()` clamped to `i32::MAX` (C# `Math.Min(.., int.MaxValue)`; 16384 on feature level 11, 4096 on 9.3).
#[must_use]
pub fn max_bitmap_size(context: &ID2D1DeviceContext) -> i32 {
    // SAFETY: `context` is a live owned interface; the call takes no arguments.
    let size = unsafe { context.GetMaximumBitmapSize() };
    i32::try_from(size).unwrap_or(i32::MAX)
}

/// `(96.0 * dpi_scale) as f32` (the cast saturates; DPI scales are small positive numbers).
fn scaled_dpi(dpi_scale: f64) -> f32 {
    (f64::from(BASE_DPI) * dpi_scale) as f32
}

/// Properties of the render target bitmap (pure; C# `DeviceResources.CreateTarget`): `B8G8R8A8_UNORM`; alpha `PREMULTIPLIED` for an
/// offscreen target and `IGNORE` for a swap chain back buffer; `dpiX = dpiY = (96.0 * dpi_scale) as f32`; options `TARGET`, plus
/// `CANNOT_DRAW` when `has_swap_chain`; no colour context.
#[must_use]
pub fn target_bitmap_properties(has_swap_chain: bool, dpi_scale: f64) -> D2D1_BITMAP_PROPERTIES1 {
    let dpi = scaled_dpi(dpi_scale);
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: if has_swap_chain {
                D2D1_ALPHA_MODE_IGNORE
            } else {
                D2D1_ALPHA_MODE_PREMULTIPLIED
            },
        },
        dpiX: dpi,
        dpiY: dpi,
        bitmapOptions: if has_swap_chain {
            D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW
        } else {
            D2D1_BITMAP_OPTIONS_TARGET
        },
        colorContext: std::mem::ManuallyDrop::new(None),
    }
}

/// Properties of an image bitmap (pure; C# `DeviceResources.CreateBitmap`): `B8G8R8A8_UNORM`, the given alpha mode
/// (`PREMULTIPLIED` for `Pbgra32`, `IGNORE` for `Bgr32`), 96 dpi, the given options, no colour context.
#[must_use]
pub fn image_bitmap_properties(alpha_mode: D2D1_ALPHA_MODE, options: D2D1_BITMAP_OPTIONS) -> D2D1_BITMAP_PROPERTIES1 {
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: alpha_mode,
        },
        dpiX: BASE_DPI,
        dpiY: BASE_DPI,
        bitmapOptions: options,
        colorContext: std::mem::ManuallyDrop::new(None),
    }
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
    if let Some(swap_chain) = swap_chain {
        let props = target_bitmap_properties(true, dpi_scale);
        let surface: IDXGISurface = back_buffer(swap_chain)?;
        // SAFETY: `props` and `surface` are locals that outlive the call; `context` is a live owned interface; the colour context
        // inside `props` is `None`. The bitmap keeps its own reference to the surface.
        let bitmap = unsafe { context.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }
            .gfx("ID2D1DeviceContext::CreateBitmapFromDxgiSurface");
        drop(surface);
        bitmap
    } else {
        let props = target_bitmap_properties(false, dpi_scale);
        let size = D2D_SIZE_U {
            width: pixel_width,
            height: pixel_height,
        };
        // SAFETY: `size` and `props` are locals that outlive the call; there is no source data (null pointer, pitch 0);
        // `context` is a live owned interface.
        unsafe { context.CreateBitmap(size, None, 0, &props) }.gfx("ID2D1DeviceContext::CreateBitmap")
    }
}

/// `context.SetTarget(bitmap)` then `context.SetDpi(d, d)` with `d = (96.0 * dpi_scale) as f32` (C# `CreateTarget` last two lines).
pub fn attach_target(context: &ID2D1DeviceContext, bitmap: &ID2D1Bitmap1, dpi_scale: f64) {
    let dpi = scaled_dpi(dpi_scale);
    // SAFETY: `context` and `bitmap` are live owned interfaces of the same factory, used on its thread; the context takes its own
    // reference to the target.
    unsafe {
        context.SetTarget(bitmap);
        context.SetDpi(dpi, dpi);
    }
}

/// `context.SetTarget(None)` (C# `ReleaseTarget` first line). The caller drops its target bitmap right after; only then is the back
/// buffer free for `ResizeBuffers`.
pub fn detach_target(context: &ID2D1DeviceContext) {
    // SAFETY: `context` is a live owned interface; a null target is the documented way to release the current one.
    unsafe { context.SetTarget(None::<&ID2D1Image>) };
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
    let props = image_bitmap_properties(alpha_mode, options);
    let size = D2D_SIZE_U {
        width: pixel_width,
        height: pixel_height,
    };
    // SAFETY: `size` and `props` are locals that outlive the call; there is no source data (null pointer, pitch 0); `context` is a
    // live owned interface.
    unsafe { context.CreateBitmap(size, None, 0, &props) }.gfx("ID2D1DeviceContext::CreateBitmap")
}

/// Validates the arguments of [`copy_from_memory`] and returns the rectangle to write (`dest`, or the whole bitmap). Pure.
fn checked_copy_rect(
    bitmap_width: u32,
    bitmap_height: u32,
    dest: Option<D2D_RECT_U>,
    pixels_len: usize,
    pitch: u32,
) -> Result<D2D_RECT_U, GfxError> {
    let invalid = GfxError::new("CopyFromMemory", E_INVALIDARG.0);
    let rect = dest.unwrap_or(D2D_RECT_U {
        left: 0,
        top: 0,
        right: bitmap_width,
        bottom: bitmap_height,
    });
    if rect.right <= rect.left || rect.bottom <= rect.top || rect.right > bitmap_width || rect.bottom > bitmap_height {
        return Err(invalid);
    }
    let row_bytes = u64::from(rect.right - rect.left) * 4;
    let rows = u64::from(rect.bottom - rect.top);
    if u64::from(pitch) < row_bytes {
        return Err(invalid);
    }
    let needed = u64::from(pitch) * (rows - 1) + row_bytes;
    if (pixels_len as u64) < needed {
        return Err(invalid);
    }
    Ok(rect)
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
    // SAFETY: `bitmap` is a live owned interface; the call takes no pointers.
    let size = unsafe { bitmap.GetPixelSize() };
    let rect = checked_copy_rect(size.width, size.height, dest, pixels.len(), pitch)?;
    // SAFETY: `rect` is a local that outlives the call; `pixels` is borrowed for the call and `checked_copy_rect` proved that it holds
    // at least `pitch * (rect height - 1) + rect width * 4` bytes, which is everything Direct2D reads (rows of `rect width * 4`
    // bytes, `pitch` apart); the call copies the data before it returns.
    unsafe { bitmap.CopyFromMemory(Some(&rect), pixels.as_ptr().cast(), pitch) }.gfx("ID2D1Bitmap::CopyFromMemory")
}

/// Unmaps a mapped bitmap when dropped, so `?` early returns cannot leave it mapped.
struct MapGuard<'a>(&'a ID2D1Bitmap1);

impl Drop for MapGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: the guard is created only right after a successful `Map` of this bitmap and unmaps it exactly once; an error
        // is ignored (nothing useful can be done in `drop`).
        let _ = unsafe { self.0.Unmap() };
    }
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
    let invalid = GfxError::new("ID2D1Bitmap1::Map", E_INVALIDARG.0);
    let row_len = usize::try_from(u64::from(pixel_width) * 4).map_err(|_| invalid)?;
    let rows = usize::try_from(pixel_height).map_err(|_| invalid)?;
    let total = row_len.checked_mul(rows).ok_or(invalid)?;
    if total == 0 {
        return Ok(Vec::new());
    }
    let cpu = create_bitmap_uninitialized(
        context,
        pixel_width,
        pixel_height,
        D2D1_ALPHA_MODE_PREMULTIPLIED,
        D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    )?;
    // SAFETY: `cpu` and `source` are live owned interfaces; destination point and source rectangle are None (whole bitmap, origin).
    unsafe { cpu.CopyFromBitmap(None, source, None) }.gfx("ID2D1Bitmap::CopyFromBitmap")?;
    // SAFETY: `cpu` is a live CPU-readable bitmap that is not mapped yet and not used by a draw.
    let mapped = unsafe { cpu.Map(D2D1_MAP_OPTIONS_READ) }.gfx("ID2D1Bitmap1::Map")?;
    let _guard = MapGuard(&cpu);
    let pitch = usize::try_from(mapped.pitch).map_err(|_| invalid)?;
    if mapped.bits.is_null() || pitch < row_len {
        return Err(invalid);
    }
    let mut out = Vec::with_capacity(total);
    for y in 0..rows {
        let offset = y.checked_mul(pitch).ok_or(invalid)?;
        // SAFETY: the mapping stays valid until `_guard` unmaps it at the end of this function; `bits` is non-null and points at
        // `pitch * rows` bytes (the whole bitmap, one row per `pitch`); row `y` starts at `y * pitch` and we read `row_len <= pitch`
        // bytes of it (both checked above), so the slice lies inside the mapping.
        let row = unsafe { std::slice::from_raw_parts(mapped.bits.add(offset), row_len) };
        out.extend_from_slice(row);
    }
    Ok(out)
}

/// `context.EndDraw(None, None)` as the HRESULT number (0 on success): the form the `EndDrawFault` test seam of `DeviceResources`
/// rewrites. The caller maps it with `com::is_device_lost`.
#[must_use]
pub fn end_draw_hresult(context: &ID2D1DeviceContext) -> i32 {
    // SAFETY: `context` is a live owned interface; the two tag out-pointers are None.
    match unsafe { context.EndDraw(None, None) } {
        Ok(()) => 0,
        Err(e) => e.code().0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: u32, top: u32, right: u32, bottom: u32) -> D2D_RECT_U {
        D2D_RECT_U {
            left,
            top,
            right,
            bottom,
        }
    }

    // C#: Brush_Primitives_ClipAndCopyFromMemory_Work (the argument contract the C# trusted)
    #[test]
    fn copy_from_memory_argument_checks() {
        let bad = GfxError::new("CopyFromMemory", E_INVALIDARG.0);
        // whole 4x4 bitmap needs 64 bytes at pitch 16
        assert!(checked_copy_rect(4, 4, None, 64, 16).is_ok());
        assert_eq!(checked_copy_rect(4, 4, None, 63, 16), Err(bad), "short slice");
        assert_eq!(checked_copy_rect(4, 4, None, 64, 15), Err(bad), "pitch below width * 4");
        assert_eq!(
            checked_copy_rect(4, 4, Some(rect(2, 2, 2, 3)), 64, 16),
            Err(bad),
            "empty width"
        );
        assert_eq!(
            checked_copy_rect(4, 4, Some(rect(2, 3, 3, 2)), 64, 16),
            Err(bad),
            "inverted height"
        );
        assert_eq!(
            checked_copy_rect(4, 4, Some(rect(1, 1, 5, 3)), 64, 16),
            Err(bad),
            "right outside"
        );
        assert_eq!(
            checked_copy_rect(4, 4, Some(rect(1, 1, 3, 5)), 64, 16),
            Err(bad),
            "bottom outside"
        );
        // 2-row rect, pitch 100, width 2: exactly pitch * 1 + 8 = 108 bytes pass, 107 fail
        assert!(checked_copy_rect(4, 4, Some(rect(1, 1, 3, 3)), 108, 100).is_ok());
        assert_eq!(checked_copy_rect(4, 4, Some(rect(1, 1, 3, 3)), 107, 100), Err(bad));
        // huge pitch does not overflow
        assert_eq!(checked_copy_rect(4, 4, None, 64, u32::MAX), Err(bad));
    }
}
