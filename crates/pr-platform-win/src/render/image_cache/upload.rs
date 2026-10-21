//! Port of `PhotoReview.Shell.Rendering/GpuImageCache.cs` lines 301-414 (`IGpuImageBackend`, whose trait is
//! [`GpuImageBackend`](super::policy::GpuImageBackend) in W8-12f, and the class `D2DGpuImageBackend`), baseline `d92d6aae`.
//! Unit W8-12g (native, test kind N, with small pure helpers).
//!
//! The Direct2D side of the cache: an image becomes one `ID2D1Bitmap1` per tile (`compute_tiles`, W8-01c, splits an image over the
//! texture limit with a gutter), a synchronous upload is `create bitmap` + `CopyFromMemory` from the tile origin, a prefetch stripe is
//! `CopyFromMemory` of the rows `[start, end)` that cut through each tile, and `release` gives the bitmaps back. After a device loss
//! the cache calls `release` for images of the OLD device generation too (their COM references are simply dropped).
//!
//! SEAMS (D-W8B3S-4): [`BitmapDevice`] is the narrow slice of `DeviceResources` this file uses (`max_bitmap_size`, `generation`,
//! `create_bitmap`, `release_bitmap`, the device-recreated event). [`ResourcesBitmapDevice`] implements it over the real
//! `Rc<DeviceResources>`; the failure-injection test implements it over a fake. `D2dGpuImageBackend` implements the policy trait
//! [`GpuImageBackend`] with `Image = TiledGpuImage<ID2D1Bitmap1>`; [`D2dGpuImageCache`] is the `pr_app` `GpuImageCache` +
//! `PrefetchStatus` adapter around `BudgetedImageCache<D2dGpuImageBackend<D>>`.
//!
//! UI thread only (the Direct2D factory is single-threaded). No `unsafe` in this file: the only raw-pointer call of the C# class
//! (`CopyFromMemory` from `byte*`) is the length-checked `gfx::d2d::copy_from_memory`.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_app::services::render::RenderError;
use pr_app::services::render::gpu_image::{GpuImage, GpuImageCache, PrefetchStatus};
use pr_app::services::render::surface::DeviceRecreatedHandlerId;
use pr_app::services::render::tiled::{GpuTile, TiledGpuImage};
use pr_app::viewport::render_geometry::compute_tiles;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_U, D2D1_ALPHA_MODE, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED,
};
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_OPTIONS_NONE, ID2D1Bitmap1};

use super::policy::{
    BudgetedImageCache, CacheError, CachedImage, DEFAULT_STRIPE_BYTES, GpuImageBackend, StripeScheduler, StripeToken,
    default_budget_bytes,
};
use crate::gfx::com::GfxError;
use crate::gfx::d2d;
use crate::render::device::{DeviceResources, RenderFault};
use crate::render::surface::D2dRenderSurface;

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

/// A subscribed device-recreated handler with its id.
type RecreatedHandler = (u64, Rc<dyn Fn()>);

/// [`BitmapDevice`] over the real [`DeviceResources`]. The `DeviceRecreated` event of C# lives on the surface, whose subscription API
/// needs `&mut D2dRenderSurface`; this adapter keeps its own handler list (so the cache can subscribe through `&self`) and the owner of
/// the surface forwards the event once with [`attach_to_surface`](Self::attach_to_surface).
pub struct ResourcesBitmapDevice {
    resources: Rc<DeviceResources>,
    handlers: RefCell<Vec<RecreatedHandler>>,
    next_handler_id: Cell<u64>,
}

impl fmt::Debug for ResourcesBitmapDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResourcesBitmapDevice")
            .field("handlers", &self.handlers.borrow().len())
            .finish_non_exhaustive()
    }
}

impl ResourcesBitmapDevice {
    /// Wraps the resources shared with the surface (`D2dRenderSurface::resources()`).
    #[must_use]
    pub fn new(resources: Rc<DeviceResources>) -> Rc<Self> {
        Rc::new(Self {
            resources,
            handlers: RefCell::new(Vec::new()),
            next_handler_id: Cell::new(0),
        })
    }

    /// The wrapped resources.
    #[must_use]
    pub fn resources(&self) -> &Rc<DeviceResources> {
        &self.resources
    }

    /// Subscribes to the surface's `DeviceRecreated` so that every recreation reaches the handlers of this adapter (the cache clears
    /// itself). The surface holds only a `Weak` to the adapter. Returns the surface's handler id (to unsubscribe).
    pub fn attach_to_surface(self: &Rc<Self>, surface: &mut D2dRenderSurface) -> DeviceRecreatedHandlerId {
        let weak: Weak<Self> = Rc::downgrade(self);
        surface.subscribe_device_recreated(Box::new(move || {
            if let Some(device) = weak.upgrade() {
                device.raise_device_recreated();
            }
        }))
    }

    /// Runs every subscribed handler in subscription order. Public so a host without a `D2dRenderSurface` can forward the event.
    pub fn raise_device_recreated(&self) {
        let handlers: Vec<Rc<dyn Fn()>> = self.handlers.borrow().iter().map(|(_, h)| Rc::clone(h)).collect();
        for handler in handlers {
            handler();
        }
    }
}

impl BitmapDevice for ResourcesBitmapDevice {
    fn max_bitmap_size(&self) -> i32 {
        self.resources.max_bitmap_size()
    }

    fn generation(&self) -> i64 {
        self.resources.generation()
    }

    fn create_bitmap(
        &self,
        pixel_width: u32,
        pixel_height: u32,
        alpha_mode: D2D1_ALPHA_MODE,
    ) -> Result<ID2D1Bitmap1, GfxError> {
        let invalid = GfxError::new("CreateBitmap", E_INVALIDARG.0);
        let width = i32::try_from(pixel_width).map_err(|_| invalid)?;
        let height = i32::try_from(pixel_height).map_err(|_| invalid)?;
        self.resources
            .create_bitmap(width, height, None, 0, alpha_mode, D2D1_BITMAP_OPTIONS_NONE)
            .map_err(|fault| match fault {
                RenderFault::Device(error) => error,
                _ => invalid,
            })
    }

    fn release_bitmap(&self, bitmap: ID2D1Bitmap1) {
        self.resources.release_bitmap(Some(bitmap));
    }

    fn subscribe_device_recreated(&self, handler: Box<dyn Fn()>) -> u64 {
        let id = self.next_handler_id.get();
        self.next_handler_id.set(id.wrapping_add(1));
        self.handlers.borrow_mut().push((id, Rc::from(handler)));
        id
    }

    fn unsubscribe_device_recreated(&self, id: u64) {
        self.handlers.borrow_mut().retain(|(existing, _)| *existing != id);
    }
}

/// C# `Upload`: `min(min(device_max, D3D11_MAX_TEXTURE_DIMENSION), override_max.unwrap_or(i32::MAX))` (`MaxTileSize` forces smaller tiles in tests).
#[must_use]
pub fn effective_tile_limit(device_max_bitmap_size: i32, max_tile_size_override: Option<i32>) -> i32 {
    device_max_bitmap_size
        .min(D3D11_MAX_TEXTURE_DIMENSION)
        .min(max_tile_size_override.unwrap_or(i32::MAX))
}

/// C# `alpha = pixels.Layout == Bgr32 ? D2dAlphaMode.Ignore : D2dAlphaMode.Premultiplied` (the X byte of `Bgr32` must not become alpha).
#[must_use]
pub fn alpha_mode_for(layout: PixelLayout) -> D2D1_ALPHA_MODE {
    if layout == PixelLayout::Bgr32 {
        D2D1_ALPHA_MODE_IGNORE
    } else {
        D2D1_ALPHA_MODE_PREMULTIPLIED
    }
}

/// C# `TileOrigin`: byte offset of the pixel `(x, y)` inside a buffer: `y * stride + x * 4`, all in `i64` (a 24 MP row offset passes 2^31
/// for tall images: `i32` would overflow).
#[must_use]
pub fn tile_byte_offset(stride: i32, x: i32, y: i32) -> i64 {
    i64::from(y) * i64::from(stride) + i64::from(x) * 4
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
    let top = start_row.max(texture_y);
    let bottom = end_row.min(texture_y.saturating_add(texture_height));
    if bottom <= top {
        return None;
    }
    Some(StripeCopy {
        source_top: top,
        dest_top: u32::try_from(top - texture_y).ok()?,
        dest_bottom: u32::try_from(bottom - texture_y).ok()?,
        dest_right: u32::try_from(texture_width).ok()?,
    })
}

/// C# `D2DGpuImageBackend` over a [`BitmapDevice`].
pub struct D2dGpuImageBackend<D: BitmapDevice> {
    device: Rc<D>,
    max_tile_size: Option<i32>,
    gutter: i32,
}

impl<D: BitmapDevice> fmt::Debug for D2dGpuImageBackend<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("D2dGpuImageBackend")
            .field("max_tile_size", &self.max_tile_size)
            .field("gutter", &self.gutter)
            .finish_non_exhaustive()
    }
}

impl<D: BitmapDevice> D2dGpuImageBackend<D> {
    /// C# `D2DGpuImageBackend(surface, maxTileSize = null, gutter = TileLayout.DefaultGutter)`.
    pub fn new(device: Rc<D>, max_tile_size: Option<i32>, gutter: i32) -> Self {
        Self {
            device,
            max_tile_size,
            gutter,
        }
    }

    /// C# `MaxTileSize`.
    #[must_use]
    pub fn max_tile_size(&self) -> Option<i32> {
        self.max_tile_size
    }

    /// C# `Gutter`.
    #[must_use]
    pub fn gutter(&self) -> i32 {
        self.gutter
    }

    /// Gives back the bitmaps of the tiles through `release_bitmap` (trap T1: the leak counter is decremented, not only dropped).
    fn release_tiles(&self, tiles: &[GpuTile<ID2D1Bitmap1>]) {
        for tile in tiles {
            if let Some(bitmap) = tile.take_bitmap() {
                self.device.release_bitmap(bitmap);
            }
        }
    }
}

/// `E_INVALIDARG` error named after the failing step.
fn invalid_arg(operation: &'static str) -> GfxError {
    GfxError::new(operation, E_INVALIDARG.0)
}

/// `bytes[tile_byte_offset(..)..]`: the slice that starts at the pixel `(x, y)` and runs to the end of the buffer (trap T4: the tail
/// must not be cut to the tile, `copy_from_memory` checks `pitch * (rows - 1) + width * 4` against it).
fn tail_at(bytes: &[u8], stride: i32, x: i32, y: i32) -> Result<&[u8], GfxError> {
    let offset = usize::try_from(tile_byte_offset(stride, x, y)).map_err(|_| invalid_arg("tile_byte_offset"))?;
    bytes.get(offset..).ok_or_else(|| invalid_arg("tile_byte_offset"))
}

impl<D: BitmapDevice> GpuImageBackend for D2dGpuImageBackend<D> {
    type Image = TiledGpuImage<ID2D1Bitmap1>;

    fn upload(&self, pixels: &PixelBuffer, with_data: bool) -> Result<Rc<Self::Image>, GfxError> {
        let bytes = pixels.bytes().map_err(|_| invalid_arg("PixelBuffer::bytes"))?;
        let stride = pixels.stride();
        let limit = effective_tile_limit(self.device.max_bitmap_size(), self.max_tile_size);
        let specs = compute_tiles(pixels.width(), pixels.height(), limit, self.gutter)
            .map_err(|_| invalid_arg("compute_tiles"))?;
        let alpha_mode = alpha_mode_for(pixels.layout());
        let pitch = u32::try_from(stride).map_err(|_| invalid_arg("PixelBuffer::stride"))?;
        let mut tiles: Vec<GpuTile<ID2D1Bitmap1>> = Vec::with_capacity(specs.len());
        for spec in &specs {
            let texture = spec.texture;
            let created = (|| {
                let width = u32::try_from(texture.width).map_err(|_| invalid_arg("CreateBitmap"))?;
                let height = u32::try_from(texture.height).map_err(|_| invalid_arg("CreateBitmap"))?;
                let bitmap = self.device.create_bitmap(width, height, alpha_mode)?;
                if with_data {
                    let copied = tail_at(bytes, stride, texture.x, texture.y)
                        .and_then(|source| d2d::copy_from_memory(&bitmap, None, source, pitch));
                    if let Err(error) = copied {
                        self.device.release_bitmap(bitmap);
                        return Err(error);
                    }
                }
                Ok(bitmap)
            })();
            match created {
                Ok(bitmap) => tiles.push(GpuTile::new(*spec, Some(bitmap))),
                Err(error) => {
                    self.release_tiles(&tiles);
                    return Err(error);
                }
            }
        }
        // T7: the generation is read after the tiles exist.
        Ok(Rc::new(TiledGpuImage::new(pixels, tiles, self.device.generation())))
    }

    fn upload_rows(
        &self,
        image: &Self::Image,
        pixels: &PixelBuffer,
        start_row: i32,
        end_row: i32,
    ) -> Result<(), GfxError> {
        let bytes = pixels.bytes().map_err(|_| invalid_arg("PixelBuffer::bytes"))?;
        let stride = pixels.stride();
        let pitch = u32::try_from(stride).map_err(|_| invalid_arg("PixelBuffer::stride"))?;
        for tile in image.tiles() {
            let texture = tile.spec().texture;
            let Some(copy) = stripe_copy(texture.y, texture.width, texture.height, start_row, end_row) else {
                continue;
            };
            let rect = D2D_RECT_U {
                left: 0,
                top: copy.dest_top,
                right: copy.dest_right,
                bottom: copy.dest_bottom,
            };
            let source = tail_at(bytes, stride, texture.x, copy.source_top)?;
            // A tile without bitmap (an image already released) is skipped.
            tile.with_bitmap(|bitmap| match bitmap {
                Some(bitmap) => d2d::copy_from_memory(bitmap, Some(rect), source, pitch),
                None => Ok(()),
            })?;
        }
        Ok(())
    }

    fn release(&self, image: &Self::Image) {
        self.release_tiles(image.tiles());
    }

    fn subscribe_device_recreated(&self, handler: Box<dyn Fn()>) -> u64 {
        self.device.subscribe_device_recreated(handler)
    }

    fn unsubscribe_device_recreated(&self, id: u64) {
        self.device.unsubscribe_device_recreated(id);
    }
}

impl<B: 'static> CachedImage for TiledGpuImage<B> {
    fn pixel_buffer_id(&self) -> i64 {
        GpuImage::pixel_buffer_id(self)
    }

    fn pixel_height(&self) -> i32 {
        GpuImage::pixel_height(self)
    }

    fn gpu_bytes(&self) -> i64 {
        GpuImage::gpu_bytes(self)
    }

    fn uploaded_rows(&self) -> i32 {
        TiledGpuImage::uploaded_rows(self)
    }

    fn set_uploaded_rows(&self, rows: i32) {
        TiledGpuImage::set_uploaded_rows(self, rows);
    }

    fn is_complete(&self) -> bool {
        TiledGpuImage::is_complete(self)
    }

    fn is_released(&self) -> bool {
        TiledGpuImage::is_released(self)
    }

    fn set_released(&self, released: bool) {
        TiledGpuImage::set_released(self, released);
    }
}

/// `CacheError` to the `pr_app` error (12f "Error mapping"). `StripeBytesTooSmall` is a construction error only.
fn render_error(error: CacheError) -> RenderError {
    match error {
        CacheError::Disposed { object } => RenderError::Disposed { object },
        CacheError::NegativeBudget { value } => RenderError::NegativeBudget { value },
        CacheError::StripeBytesTooSmall { .. } => RenderError::ArgumentOutOfRange { param: "stripeBytes" },
        CacheError::Backend(GfxError { hresult, operation }) => RenderError::Device { hresult, operation },
    }
}

/// The concrete cache the composition root builds: the budgeted LRU policy ([`BudgetedImageCache`], W8-12f) over the Direct2D backend.
/// Implements `pr_app`'s [`GpuImageCache`] and [`PrefetchStatus`]. UI thread only (`Rc`).
pub struct D2dGpuImageCache<D: BitmapDevice + 'static> {
    inner: Rc<BudgetedImageCache<D2dGpuImageBackend<D>>>,
}

impl<D: BitmapDevice + 'static> fmt::Debug for D2dGpuImageCache<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("D2dGpuImageCache").field("inner", &self.inner).finish()
    }
}

impl<D: BitmapDevice + 'static> D2dGpuImageCache<D> {
    /// C# public constructor `GpuImageCache(surface, dispatcher)`: budget = `default_budget_bytes(surface PIXEL size)`, stripe =
    /// [`DEFAULT_STRIPE_BYTES`], default tiling.
    pub fn new(
        device: Rc<D>,
        scheduler: Rc<dyn StripeScheduler>,
        surface_pixel_width: i32,
        surface_pixel_height: i32,
    ) -> Result<Self, RenderError> {
        Self::with_options(
            device,
            scheduler,
            default_budget_bytes(surface_pixel_width, surface_pixel_height),
            DEFAULT_STRIPE_BYTES,
            None,
            DEFAULT_GUTTER,
        )
    }

    /// C# internal constructors (tests): explicit budget, stripe size, forced tile size and gutter.
    pub fn with_options(
        device: Rc<D>,
        scheduler: Rc<dyn StripeScheduler>,
        budget_bytes: i64,
        stripe_bytes: i32,
        max_tile_size: Option<i32>,
        gutter: i32,
    ) -> Result<Self, RenderError> {
        let backend = D2dGpuImageBackend::new(device, max_tile_size, gutter);
        let inner = BudgetedImageCache::new(backend, scheduler, budget_bytes, stripe_bytes).map_err(render_error)?;
        Ok(Self { inner })
    }

    /// Runs one scheduled stripe (the scheduler's job calls this on the UI thread).
    pub fn run_stripe(&self, token: StripeToken) -> Result<(), RenderError> {
        self.inner.run_stripe(token).map_err(render_error)
    }

    /// The policy cache (counts, `contains`, `total_gpu_bytes`, ...).
    #[must_use]
    pub fn inner(&self) -> &Rc<BudgetedImageCache<D2dGpuImageBackend<D>>> {
        &self.inner
    }

    /// `get_or_upload` with the concrete image type (the draw code and the tests need the tiles; no `dyn GpuImage` downcast).
    pub fn get_or_upload_tiled(&self, pixels: &PixelBuffer) -> Result<Rc<TiledGpuImage<ID2D1Bitmap1>>, RenderError> {
        self.inner.get_or_upload(pixels).map_err(render_error)
    }
}

impl<D: BitmapDevice + 'static> GpuImageCache for D2dGpuImageCache<D> {
    fn get_or_upload(&self, pixels: &PixelBuffer) -> Result<Rc<dyn GpuImage>, RenderError> {
        let image: Rc<dyn GpuImage> = self.get_or_upload_tiled(pixels)?;
        Ok(image)
    }

    fn prefetch(&self, pixels: &Arc<PixelBuffer>) -> Result<(), RenderError> {
        self.inner.prefetch(pixels).map_err(render_error)
    }

    fn retain(&self, pixel_buffer_ids: &[i64]) -> Result<(), RenderError> {
        self.inner.retain(pixel_buffer_ids).map_err(render_error)
    }

    fn budget_bytes(&self) -> i64 {
        self.inner.budget_bytes()
    }

    fn set_budget_bytes(&self, value: i64) -> Result<(), RenderError> {
        self.inner.set_budget_bytes(value).map_err(render_error)
    }

    fn clear(&self) {
        self.inner.clear();
    }
}

impl<D: BitmapDevice + 'static> PrefetchStatus for D2dGpuImageCache<D> {
    fn is_fully_uploaded(&self, pixel_buffer_id: i64) -> bool {
        self.inner.is_fully_uploaded(pixel_buffer_id)
    }

    fn subscribe_upload_completed(&self, handler: Rc<dyn Fn(i64)>) -> u64 {
        self.inner.subscribe_upload_completed(handler)
    }

    fn unsubscribe_upload_completed(&self, id: u64) {
        self.inner.unsubscribe_upload_completed(id);
    }
}
