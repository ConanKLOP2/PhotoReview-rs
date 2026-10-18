//! Port of `PhotoReview.Shell.Rendering/Contracts/GpuImageContracts.cs` (33 lines, C-10, baseline d92d6aae): the GPU image handle
//! and the GPU image cache contract. Keyed by `PixelBuffer::id()`, never by path (INV-1).
//!
//! Traits only, no `windows` types (D-W8-3). The real cache is `pr_platform_win::render::image_cache` (W8-12f policy, W8-12g upload).
//! Everything here is UI-thread only: no `Send`/`Sync` bound (a Direct2D bitmap is not `Send`).
//!
//! Stub: traits have no bodies to fill; the file compiles as is. The `allow` stays out.

use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;

use pr_imaging::pixels::PixelBuffer;

use super::error::RenderError;

/// `IGpuImage` (C-10): an image resident on the GPU. Only the cache creates and releases it; a draw context checks that the
/// image is its own concrete type and still drawable, and skips it otherwise (it never draws a dead pointer).
///
/// `as_any` is the Rust form of the C# `image is not TiledGpuImage tiled` test in `D2DDrawContext.DrawImage`: the implementation
/// returns `self`, the draw context calls `downcast_ref::<TiledGpuImage<ID2D1Bitmap1>>()`.
pub trait GpuImage: Any {
    /// `PixelBufferId`: `PixelBuffer::id()` of the source buffer (cache key, INV-1).
    fn pixel_buffer_id(&self) -> i64;

    /// `PixelWidth`, in source pixels.
    fn pixel_width(&self) -> i32;

    /// `PixelHeight`, in source pixels.
    fn pixel_height(&self) -> i32;

    /// `GpuBytes`: bytes of all textures, tile gutters included.
    fn gpu_bytes(&self) -> i64;

    /// Upcast for the draw context's concrete-type check (see the type doc).
    fn as_any(&self) -> &dyn Any;
}

/// `IGpuImageCache` (C-10). UI thread only. All methods take `&self`: the cache is shared (`Rc`) by the surface's
/// device-recreated handler, the prefetch ticks and the compositor, and keeps its state behind interior mutability (D-W8-12a-3).
pub trait GpuImageCache {
    /// `GetOrUpload(PixelBuffer)`: the cached image (completing a half-prefetched one at once) or a fresh synchronous upload.
    /// The image becomes the "current" one that eviction never touches. A buffer that is already disposed is
    /// `Err(RenderError::Disposed { object: "PhotoReview.Imaging.Pixels.PixelBuffer" })`, a disposed cache `Err(Disposed { object: "PhotoReview.Shell.Rendering.GpuImageCache" })`.
    /// The same buffer id returns the SAME `Rc` (pointer-equal) while the image stays cached.
    fn get_or_upload(&self, pixels: &PixelBuffer) -> Result<Rc<dyn GpuImage>, RenderError>;

    /// `Prefetch(PixelBuffer)`: upload in background stripes when idle (C# `UiPriority.Background`); no-op when already cached.
    /// A buffer that is already disposed is silently ignored (`Ok(())`). The cache keeps the buffer only until the last stripe
    /// is uploaded: it holds an [`Arc`] clone for that time and drops the partial image when the buffer's other owners are gone
    /// (the Rust form of "source disposed midway", D-W8-12a-4).
    fn prefetch(&self, pixels: &Arc<PixelBuffer>) -> Result<(), RenderError>;

    /// `Retain(IReadOnlyCollection<long>)`: keep exactly these ids (plus LRU up to the budget); replaces the previous set.
    fn retain(&self, pixel_buffer_ids: &[i64]) -> Result<(), RenderError>;

    /// `BudgetBytes` getter. Default of the real cache: 6 screen-sized images (not a second copy of the preload RAM on the GPU).
    fn budget_bytes(&self) -> i64;

    /// `BudgetBytes` setter: negative is `Err(ArgumentOutOfRange { param: "value" })`; lowering the budget evicts at once.
    fn set_budget_bytes(&self, value: i64) -> Result<(), RenderError>;

    /// `Clear()`: release every image (also used when the device is recreated). Never fails, also after dispose.
    fn clear(&self);
}
