//! Port of `PhotoReview.Shell.Rendering/TiledGpuImage.cs` (62 lines, baseline d92d6aae): a GPU image is one or more tiles
//! (an image over the texture limit is split by `TileLayout`, W8-01c); `GpuTile` is one tile (its place in the image plus the
//! Direct2D bitmap, `None` for the fake backend of the cache tests).
//!
//! The Direct2D bitmap type is NOT named here: both structs are generic over the bitmap handle `B` (C# `ID2D1Bitmap1`), so this
//! file has no `windows` type and the bookkeeping is tested with `B = ()` or `B = u32` (D-W8-12a-1). `pr_platform_win::render`
//! (W8-12g) instantiates `TiledGpuImage<ID2D1Bitmap1>`; the draw context (W8-12e) gets it back through
//! [`GpuImage::as_any`](super::gpu_image::GpuImage::as_any).
//!
//! UI thread only. The cache and the holders of an image share it through `Rc`, so the mutable state (`uploaded_rows`,
//! `is_released`, a tile's bitmap) sits in `Cell`/`RefCell` (the C# object is mutated through shared references too).
//!
//! Stub: `new`, `is_complete`, `is_drawable` and the `GpuImage` impl are `todo!()`; plain accessors are real.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use std::any::Any;
use std::cell::{Cell, RefCell};

use pr_imaging::pixels::{PixelBuffer, PixelLayout};

use super::gpu_image::GpuImage;
use crate::viewport::render_geometry::TileSpec;

/// `GpuTile(TileSpec spec, ID2D1Bitmap1? bitmap)`: `spec` is fixed, the bitmap can be replaced or taken (released) later.
#[derive(Debug)]
pub struct GpuTile<B> {
    spec: TileSpec,
    bitmap: RefCell<Option<B>>,
}

impl<B> GpuTile<B> {
    /// C# primary constructor.
    pub fn new(spec: TileSpec, bitmap: Option<B>) -> Self {
        Self {
            spec,
            bitmap: RefCell::new(bitmap),
        }
    }

    /// C# `Spec`.
    pub fn spec(&self) -> TileSpec {
        self.spec
    }

    /// C# `Bitmap != null`.
    pub fn has_bitmap(&self) -> bool {
        self.bitmap.borrow().is_some()
    }

    /// C# `Bitmap = value` (the old bitmap is dropped, which releases the COM reference).
    pub fn set_bitmap(&self, bitmap: Option<B>) {
        *self.bitmap.borrow_mut() = bitmap;
    }

    /// C# `Bitmap = null` that hands the old bitmap to the caller (release on eviction / device change).
    pub fn take_bitmap(&self) -> Option<B> {
        self.bitmap.borrow_mut().take()
    }

    /// C# `tile.Bitmap` read: runs `f` with a borrow of the bitmap (`None` when absent). Do not call back into this tile from `f`.
    pub fn with_bitmap<R>(&self, f: impl FnOnce(Option<&B>) -> R) -> R {
        f(self.bitmap.borrow().as_ref())
    }
}

/// `TiledGpuImage : IGpuImage` (C-10, keyed by `PixelBuffer::id()`, INV-1). Only the cache creates and releases it; a drawer
/// checks [`is_drawable`](Self::is_drawable) and skips an evicted or old-device image instead of drawing a dead pointer.
#[derive(Debug)]
pub struct TiledGpuImage<B> {
    pixel_buffer_id: i64,
    pixel_width: i32,
    pixel_height: i32,
    layout: PixelLayout,
    tiles: Vec<GpuTile<B>>,
    generation: i64,
    gpu_bytes: i64,
    uploaded_rows: Cell<i32>,
    is_released: Cell<bool>,
}

impl<B> TiledGpuImage<B> {
    /// C# constructor `TiledGpuImage(PixelBuffer pixels, GpuTile[] tiles, long generation)`.
    /// Copies `id`, `width`, `height`, `layout` from `pixels`; `uploaded_rows = 0`, `is_released = false`;
    /// `gpu_bytes` = sum over tiles of `texture.width as i64 * texture.height as i64 * 4` (the 4 is bytes per pixel; the texture
    /// rect includes the overlap gutter; 0 for an empty tile list). Use `wrapping_add` (C# unchecked `long +=`).
    pub fn new(pixels: &PixelBuffer, tiles: Vec<GpuTile<B>>, generation: i64) -> Self {
        todo!()
    }

    /// C# `Layout`.
    pub fn layout(&self) -> PixelLayout {
        self.layout
    }

    /// C# `Tiles` (the array is fixed; each tile's bitmap is not).
    pub fn tiles(&self) -> &[GpuTile<B>] {
        &self.tiles
    }

    /// C# `Generation`: `DeviceResources.Generation` when the tiles were created.
    pub fn generation(&self) -> i64 {
        self.generation
    }

    /// C# `UploadedRows` getter: rows from the top already on the GPU (prefetch uploads stripe by stripe).
    pub fn uploaded_rows(&self) -> i32 {
        self.uploaded_rows.get()
    }

    /// C# `UploadedRows` setter. No validation (C# has none).
    pub fn set_uploaded_rows(&self, rows: i32) {
        self.uploaded_rows.set(rows);
    }

    /// C# `IsReleased` getter.
    pub fn is_released(&self) -> bool {
        self.is_released.get()
    }

    /// C# `IsReleased` setter.
    pub fn set_released(&self, released: bool) {
        self.is_released.set(released);
    }

    /// C# `IsComplete` = `UploadedRows >= PixelHeight`.
    pub fn is_complete(&self) -> bool {
        todo!()
    }

    /// C# `IsDrawable(currentGeneration)` = `!IsReleased && IsComplete && Generation == currentGeneration`.
    pub fn is_drawable(&self, current_generation: i64) -> bool {
        todo!()
    }
}

impl<B: 'static> GpuImage for TiledGpuImage<B> {
    fn pixel_buffer_id(&self) -> i64 {
        todo!()
    }

    fn pixel_width(&self) -> i32 {
        todo!()
    }

    fn pixel_height(&self) -> i32 {
        todo!()
    }

    fn gpu_bytes(&self) -> i64 {
        todo!()
    }

    fn as_any(&self) -> &dyn Any {
        todo!()
    }
}
