//! Port of `PhotoReview.Shell.Win32/Viewing/ImageLayer.cs` (54 lines, WP-21, baseline d92d6aae): draws one `PixelBuffer` through
//! the GPU image cache into a destination rectangle (DIP). Shared by the main image, the old crossfade layer and the two compare
//! panes. Stateless: GPU bitmap lifetime belongs to the cache (LRU and retain), buffer lifetime to the CPU owner (the buffer may
//! be disposed between two frames; then nothing is drawn, no error).
//!
//! Orientation: the C# test `context is D2DDrawContext oriented` is `DrawContext::as_oriented()` (W8-12a, D-W8-12a-5): `Some`
//! for the real draw context, `None` for a fake that cannot orient. The Rust twin of `DrawImageOriented` is
//! `OrientedDrawContext::draw_image_oriented` (W8-12a). No change to `DrawContext` is needed here.

use crate::input::primitives::RectD;
use crate::services::render::error::RenderError;
use crate::services::render::gpu_image::GpuImageCache;
use crate::services::render::surface::DrawContext;
use crate::viewport::render_geometry::ImageInterpolation;
use pr_imaging::pixels::PixelBuffer;

/// C# `ImageLayer.TryDraw(cache, context, pixels, destination, interpolation, opacity = 1f, exifOrientation = 1)`.
///
/// Returns `Ok(false)` without touching the cache when `pixels.is_disposed()`, when `destination.width <= 0` or
/// `destination.height <= 0`, or when any destination component is NaN. Otherwise `cache.get_or_upload(pixels)`:
/// `Err(RenderError::Disposed { .. })` there (the CPU side released the buffer under us) gives `Ok(false)`; any other `Err` is
/// returned as is (C# throws; its `catch` is `ObjectDisposedException` only). With `exif_orientation` in `2..=8` the image is
/// drawn through `context.as_oriented()`: `Some(o)` gives `o.draw_image_oriented(gpu, destination, exif_orientation, None,
/// interpolation, opacity)`; `None` falls back to `context.draw_image(gpu, destination, None, interpolation, opacity)` (C#
/// `context is D2DDrawContext oriented` failing). Any other orientation calls `draw_image` with no source rectangle. Returns
/// `Ok(true)` once drawn.
pub fn try_draw(
    cache: &dyn GpuImageCache,
    context: &mut dyn DrawContext,
    pixels: &PixelBuffer,
    destination: RectD,
    interpolation: ImageInterpolation,
    opacity: f32,
    exif_orientation: i32,
) -> Result<bool, RenderError> {
    if pixels.is_disposed() {
        return Ok(false);
    }
    let has_nan =
        destination.x.is_nan() || destination.y.is_nan() || destination.width.is_nan() || destination.height.is_nan();
    if has_nan || destination.width <= 0.0 || destination.height <= 0.0 {
        return Ok(false);
    }

    let gpu = match cache.get_or_upload(pixels) {
        Ok(gpu) => gpu,
        Err(RenderError::Disposed { .. }) => return Ok(false),
        Err(other) => return Err(other),
    };
    let image: &dyn crate::services::render::gpu_image::GpuImage = &*gpu;

    if (2..=8).contains(&exif_orientation)
        && let Some(oriented) = context.as_oriented()
    {
        oriented.draw_image_oriented(image, destination, exif_orientation, None, interpolation, opacity);
        return Ok(true);
    }
    context.draw_image(image, destination, None, interpolation, opacity);
    Ok(true)
}
