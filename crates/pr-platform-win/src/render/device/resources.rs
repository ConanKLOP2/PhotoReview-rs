//! Render target, bitmaps and probes of `DeviceResources` (W8-12c): `CreateBitmap` / `ReleaseBitmap` (lines 124 to 153), `Resize`
//! (155 to 175), `TouchGpu` (205 to 218), `Synchronize` (220 to 233), `CreateTarget` / `ReleaseTarget` (351 to 395) and
//! `InterpolationModes` (446 to 456), baseline `d92d6aae`. Contract: `contracts/W8-12c.md`.
//!
//! The plan row calls this unit "brushes, caches, text factories"; the C# file has none of those (the brush is created in `CreateDevice`,
//! W8-12b; the caches are W8-12f/12g, the text factories W8-12i). D-W8B3R-2 records the cut.

#![allow(unused_variables, dead_code)] // W8-12c stub: the porter removes this line when the last `todo!()` is gone.

use windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE;
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_OPTIONS, D2D1_INTERPOLATION_MODE, ID2D1Bitmap1};

use super::{DeviceResources, RenderFault};
use crate::gfx::com::GfxError;

/// Interim twin of `pr_app::viewport::render_geometry::ImageInterpolation` (same variants and discriminants), replaced by an import
/// when the layering PR lands (D-W8B3R-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageInterpolation {
    NearestNeighbor = 0,
    Linear = 1,
    HighQualityCubic = 2,
}

/// C# `InterpolationModes.ToD2D`: `NearestNeighbor` to `D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR` (0), `Linear` to `LINEAR` (1),
/// `HighQualityCubic` to `HIGH_QUALITY_CUBIC` (5). The C# `ArgumentOutOfRangeException` for an unknown value cannot happen (closed enum).
#[must_use]
pub fn to_d2d_interpolation(interpolation: ImageInterpolation) -> D2D1_INTERPOLATION_MODE {
    todo!("W8-12c")
}

/// What `resize` did (C# `Resize` returned the `ResizeBuffers` HRESULT: `0` or a failure code, and THREW for any other failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeOutcome {
    /// Target rebuilt at the new size.
    Resized,
    /// `ResizeBuffers` failed with this HRESULT; the caller tests `gfx::com::is_device_lost` and recreates (the target is NOT rebuilt).
    BuffersFailed(i32),
}

impl DeviceResources {
    /// C# `CreateBitmap(width, height, data, pitch, alphaMode, options)`: a `B8G8R8A8_UNORM` bitmap at 96 dpi
    /// (`gfx::d2d::create_bitmap_uninitialized`, then `copy_from_memory(.., data, pitch)` when `data` is `Some`; the C# passed the
    /// initial data to `CreateBitmapEx`). `live_bitmap_count += 1` only after both succeeded. `alpha_mode` is `PREMULTIPLIED` for
    /// `Pbgra32` and `IGNORE` for `Bgr32`.
    pub fn create_bitmap(
        &self,
        pixel_width: i32,
        pixel_height: i32,
        data: Option<&[u8]>,
        pitch: u32,
        alpha_mode: D2D1_ALPHA_MODE,
        options: D2D1_BITMAP_OPTIONS,
    ) -> Result<ID2D1Bitmap1, RenderFault> {
        todo!("W8-12c")
    }

    /// C# `ReleaseBitmap`: `None` is a no-op; otherwise drops the bitmap (any generation) and `live_bitmap_count -= 1`.
    pub fn release_bitmap(&self, bitmap: Option<ID2D1Bitmap1>) {
        todo!("W8-12c")
    }

    /// C# `Resize`: `Err(Disposed)` after `dispose`; sets size (`clamp_dimension`) and DPI (`normalize_dpi_scale`) FIRST (they stay
    /// changed even if the rest fails), `release_target`, and with a swap chain `d3d11::clear_state_and_flush` then
    /// `dxgi::resize_buffers`; a failing `resize_buffers` is `Ok(BuffersFailed(hresult))`. Then `create_target`; its failure is `Err`.
    pub fn resize(&self, pixel_width: i32, pixel_height: i32, dpi_scale: f64) -> Result<ResizeOutcome, RenderFault> {
        todo!("W8-12c")
    }

    /// C# `TouchGpu` (WP21-STALL keep-warm): `BeginDraw`, `Clear(opaque black)`, `EndDraw` (must be `>= 0`, else `Err(Device)`),
    /// `d3d11::flush`. Never presents. `Err(Disposed)` after `dispose`.
    pub fn touch_gpu(&self) -> Result<(), RenderFault> {
        todo!("W8-12c")
    }

    /// C# `Synchronize`: lazily creates a 1x1 `CPU_READ | CANNOT_DRAW` probe bitmap (counted in `live_bitmap_count`), `CopyFromBitmap`
    /// one pixel of the target, `Map(READ)`, `Unmap` (every path). Waits for the GPU; never on the normal draw path.
    pub fn synchronize(&self) -> Result<(), RenderFault> {
        todo!("W8-12c")
    }

    /// C# `CreateTarget` (lines 351 to 388): `gfx::d2d::create_target_bitmap(context, swap chain, w, h, dpi_scale)` then
    /// `gfx::d2d::attach_target(context, bitmap, dpi_scale)`; stores the bitmap in `state.target`. The DXGI surface is dropped before
    /// returning (the bitmap keeps its own reference to the back buffer).
    pub(super) fn create_target(&self) -> Result<(), RenderFault> {
        todo!("W8-12c")
    }

    /// C# `ReleaseTarget` (lines 390 to 395): `gfx::d2d::detach_target`, then drop `state.target` (in that order; only then can
    /// `ResizeBuffers` succeed). A disposed or never-built object is a no-op.
    pub(super) fn release_target(&self) {
        todo!("W8-12c")
    }
}

/// Maps a `GfxError` of `ResizeBuffers` to the outcome the surface understands (pure; `Ok(BuffersFailed(hresult))`).
#[must_use]
pub fn buffers_failed(error: GfxError) -> ResizeOutcome {
    todo!("W8-12c")
}
