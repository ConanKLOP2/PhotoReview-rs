//! Port of `PhotoReview.Shell.Rendering/GpuImageCache.cs` lines 301-414 (`IGpuImageBackend`, whose trait is
//! [`GpuImageBackend`](super::policy::GpuImageBackend) in W8-12f, and the class `D2DGpuImageBackend`), baseline `d92d6aae`.
//! Unit W8-12g (native, test kind N, with small pure helpers).
//!
//! The Direct2D side of the cache: an image becomes one `ID2D1Bitmap1` per tile (`TileLayout::compute`, W8-01c, splits an image over the
//! texture limit with a gutter), a synchronous upload is `create bitmap` + `CopyFromMemory` from the tile origin, a prefetch stripe is
//! `CopyFromMemory` of the rows `[start, end)` that cut through each tile, and `release` gives the bitmaps back. After a device loss
//! the cache calls `release` for images of the OLD device generation too (their COM references are simply dropped).
//!
//! PLACEHOLDERS (D-W8B3S-4): the stub compiles against the current crate graph, so it cannot name `TiledGpuImage<ID2D1Bitmap1>`,
//! `TileSpec`/`PixelRect`/`compute_tiles` (all `pr_app`) or W8-12c's `DeviceResources`. [`BitmapDevice`] is the narrow slice of
//! `DeviceResources` this file uses; the porter implements it for the real type (or replaces the parameter by it) and adds
//! `impl GpuImageBackend for D2dGpuImageBackend<D> { type Image = TiledGpuImage<ID2D1Bitmap1>; .. }` once the layering PR is merged.
//!
//! UI thread only (the Direct2D factory is single-threaded).

#![allow(unused_variables, dead_code)] // W8-12g stub: the porter removes this line when the last `todo!()` is gone.

use std::rc::Rc;

use pr_imaging::pixels::PixelLayout;
use windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;

use crate::gfx::com::GfxError;

/// C# `TileLayout.D3D11MaxTextureDimension`: real GPUs at feature level 11 allow 16384; WARP reports a huge value (8 388 608 on the dev
/// machine), so every device is clamped to this limit and tests on WARP take the same tile path as a real GPU.
pub const D3D11_MAX_TEXTURE_DIMENSION: i32 = 16384;
/// C# `TileLayout.DefaultGutter`: the overlap between neighbouring tiles that makes the seams invisible when scaling.
pub const DEFAULT_GUTTER: i32 = 64;

/// The slice of W8-12c `DeviceResources` that the backend uses (C# `_surface.Resources`). UI thread only, `&self` (the resources are
/// shared with the surface through `Rc`).
pub trait BitmapDevice {
    /// C# `Resources.MaxBitmapSize` (`ID2D1RenderTarget::GetMaximumBitmapSize`; 16384 at FL 11, 4096 at FL 9.3).
    fn max_bitmap_size(&self) -> i32;
    /// C# `Resources.Generation`: increases at every device recreation; a bitmap of another generation is never drawn.
    fn generation(&self) -> i64;
    /// C# `Resources.CreateBitmap(width, height, null, 0, alphaMode, None)` = `gfx::d2d::create_bitmap_uninitialized`, plus the
    /// leak counter `LiveBitmapCount++`.
    fn create_bitmap(
        &self,
        pixel_width: u32,
        pixel_height: u32,
        alpha_mode: D2D1_ALPHA_MODE,
    ) -> Result<ID2D1Bitmap1, GfxError>;
    /// C# `Resources.ReleaseBitmap(bitmap)`: drops the COM reference and does `LiveBitmapCount--`. Valid for bitmaps of an old generation.
    fn release_bitmap(&self, bitmap: ID2D1Bitmap1);
    /// C# `Resources`/surface `DeviceRecreated +=`.
    fn subscribe_device_recreated(&self, handler: Box<dyn Fn()>) -> u64;
    /// C# `DeviceRecreated -=`; an unknown id is a no-op.
    fn unsubscribe_device_recreated(&self, id: u64);
}

/// C# `Upload`: `min(min(device_max, D3D11_MAX_TEXTURE_DIMENSION), override_max.unwrap_or(i32::MAX))` (`MaxTileSize` forces smaller tiles in tests).
#[must_use]
pub fn effective_tile_limit(device_max_bitmap_size: i32, max_tile_size_override: Option<i32>) -> i32 {
    todo!("W8-12g")
}

/// C# `alpha = pixels.Layout == Bgr32 ? D2dAlphaMode.Ignore : D2dAlphaMode.Premultiplied` (the X byte of `Bgr32` must not become alpha).
#[must_use]
pub fn alpha_mode_for(layout: PixelLayout) -> D2D1_ALPHA_MODE {
    todo!("W8-12g")
}

/// C# `TileOrigin`: byte offset of the pixel `(x, y)` inside a buffer: `y * stride + x * 4`, all in `i64` (a 24 MP row offset passes 2^31
/// for tall images: `i32` would overflow).
#[must_use]
pub fn tile_byte_offset(stride: i32, x: i32, y: i32) -> i64 {
    todo!("W8-12g")
}

/// The rectangle of one tile that a stripe writes (C# `UploadRows` loop body), in tile-local coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripeCopy {
    /// Source row of the first copied row in the whole image: `max(start_row, texture_y)`.
    pub source_top: i32,
    /// `D2D_RECT_U { left: 0, top: source_top - texture_y, right: texture_width, bottom: source_bottom - texture_y }` (tile-local).
    pub dest_top: u32,
    /// Exclusive bottom, tile-local.
    pub dest_bottom: u32,
    /// `texture_width` (right edge of the destination rectangle).
    pub dest_right: u32,
}

/// C# `UploadRows` per tile: `top = max(start_row, texture_y)`, `bottom = min(end_row, texture_y + texture_height)`; `None` when
/// `bottom <= top` (the stripe does not touch this tile). `texture_*` is the tile's TEXTURE rectangle (gutter included).
#[must_use]
pub fn stripe_copy(
    texture_y: i32,
    texture_width: i32,
    texture_height: i32,
    start_row: i32,
    end_row: i32,
) -> Option<StripeCopy> {
    todo!("W8-12g")
}

/// C# `D2DGpuImageBackend` over a [`BitmapDevice`].
pub struct D2dGpuImageBackend<D: BitmapDevice> {
    device: Rc<D>,
    max_tile_size: Option<i32>,
    gutter: i32,
}

impl<D: BitmapDevice> std::fmt::Debug for D2dGpuImageBackend<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!("W8-12g")
    }
}

impl<D: BitmapDevice> D2dGpuImageBackend<D> {
    /// C# `D2DGpuImageBackend(surface, maxTileSize = null, gutter = TileLayout.DefaultGutter)`.
    pub fn new(device: Rc<D>, max_tile_size: Option<i32>, gutter: i32) -> Self {
        todo!("W8-12g")
    }

    /// C# `MaxTileSize`.
    #[must_use]
    pub fn max_tile_size(&self) -> Option<i32> {
        todo!("W8-12g")
    }

    /// C# `Gutter`.
    #[must_use]
    pub fn gutter(&self) -> i32 {
        todo!("W8-12g")
    }
}

// After the layering PR (D-W8B1-1), in this file:
//
// impl<D: BitmapDevice> GpuImageBackend for D2dGpuImageBackend<D> {
//     type Image = TiledGpuImage<ID2D1Bitmap1>;
//     fn upload(..)                  // effective_tile_limit + compute_tiles + create_bitmap per tile (+ copy_from_memory when with_data);
//                                    // on failure release the bitmaps created so far, then return the error; generation = device.generation()
//     fn upload_rows(..)             // stripe_copy per tile + gfx::d2d::copy_from_memory(bitmap, Some(rect), &bytes[offset..], stride)
//     fn release(..)                 // tile.take_bitmap() -> device.release_bitmap
//     fn subscribe_device_recreated / unsubscribe_device_recreated   // forward to the device
// }
//
// and the pr_app adapter (the concrete cache the composition root builds):
//
// pub struct D2dGpuImageCache { inner: Rc<BudgetedImageCache<D2dGpuImageBackend<..>>> }
// impl GpuImageCache for D2dGpuImageCache { get_or_upload / prefetch / retain / budget_bytes / set_budget_bytes / clear }
