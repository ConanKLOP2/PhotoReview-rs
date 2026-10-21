//! Render target, bitmaps and probes of `DeviceResources` (W8-12c): `CreateBitmap` / `ReleaseBitmap` (lines 124 to 153), `Resize`
//! (155 to 175), `TouchGpu` (205 to 218), `Synchronize` (220 to 233), `CreateTarget` / `ReleaseTarget` (351 to 395) and
//! `InterpolationModes` (446 to 456), baseline `d92d6aae`. Contract: `contracts/W8-12c.md`.
//!
//! The plan row calls this unit "brushes, caches, text factories"; the C# file has none of those (the brush is created in `CreateDevice`,
//! W8-12b; the caches are W8-12f/12g, the text factories W8-12i). D-W8B3R-2 records the cut.

use windows::Win32::Graphics::Direct2D::Common::{
    D2D_POINT_2U, D2D_RECT_U, D2D1_ALPHA_MODE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_INTERPOLATION_MODE,
    D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_INTERPOLATION_MODE_LINEAR,
    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_MAP_OPTIONS_READ, ID2D1Bitmap1,
};

use super::create::{clamp_dimension, normalize_dpi_scale};
use super::{DeviceResources, RenderFault, object_disposed};
use crate::gfx::com::{GfxError, ResultExt, check};
use crate::gfx::{d2d, d3d11, dxgi};

/// `ImageInterpolation` of `pr_app` (layering PR landed, D-W8B3R-1): the interim twin is gone. Its discriminants are the C#
/// `ImageInterpolation` values (`HighQualityCubic == 2`), NOT the Direct2D constants (see [`to_d2d_interpolation`]).
pub use pr_app::viewport::render_geometry::ImageInterpolation;

/// C# `InterpolationModes.ToD2D`: `NearestNeighbor` to `D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR` (0), `Linear` to `LINEAR` (1),
/// `HighQualityCubic` to `HIGH_QUALITY_CUBIC` (5). The C# `ArgumentOutOfRangeException` for an unknown value cannot happen (closed enum).
#[must_use]
pub fn to_d2d_interpolation(interpolation: ImageInterpolation) -> D2D1_INTERPOLATION_MODE {
    match interpolation {
        ImageInterpolation::NearestNeighbor => D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
        ImageInterpolation::Linear => D2D1_INTERPOLATION_MODE_LINEAR,
        ImageInterpolation::HighQualityCubic => D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
    }
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
        // Hardening (D-W8B3R-8): the C# cast a non-positive size to `uint`; here it is an error before any COM call.
        let width = match u32::try_from(pixel_width) {
            Ok(width) if width >= 1 => width,
            _ => return Err(RenderFault::ArgumentOutOfRange { param: "pixelWidth" }),
        };
        let height = match u32::try_from(pixel_height) {
            Ok(height) if height >= 1 => height,
            _ => return Err(RenderFault::ArgumentOutOfRange { param: "pixelHeight" }),
        };
        let context = self.context()?;
        let bitmap = d2d::create_bitmap_uninitialized(&context, width, height, alpha_mode, options)?;
        if let Some(data) = data {
            // On failure `bitmap` is dropped here and the count is not touched.
            d2d::copy_from_memory(&bitmap, None, data, pitch)?;
        }
        self.live_bitmap_count.set(self.live_bitmap_count.get() + 1);
        Ok(bitmap)
    }

    /// C# `ReleaseBitmap`: `None` is a no-op; otherwise drops the bitmap (any generation) and `live_bitmap_count -= 1`.
    pub fn release_bitmap(&self, bitmap: Option<ID2D1Bitmap1>) {
        if let Some(bitmap) = bitmap {
            drop(bitmap);
            self.live_bitmap_count.set(self.live_bitmap_count.get() - 1);
        }
    }

    /// C# `Resize`: `Err(Disposed)` after `dispose`; sets size (`clamp_dimension`) and DPI (`normalize_dpi_scale`) FIRST (they stay
    /// changed even if the rest fails), `release_target`, and with a swap chain `d3d11::clear_state_and_flush` then
    /// `dxgi::resize_buffers`; a failing `resize_buffers` is `Ok(BuffersFailed(hresult))`. Then `create_target`; its failure is `Err`.
    pub fn resize(&self, pixel_width: i32, pixel_height: i32, dpi_scale: f64) -> Result<ResizeOutcome, RenderFault> {
        let width = clamp_dimension(pixel_width);
        let height = clamp_dimension(pixel_height);
        {
            let mut state = self.state.borrow_mut();
            if state.disposed {
                return Err(object_disposed());
            }
            state.pixel_width = width;
            state.pixel_height = height;
            state.dpi_scale = normalize_dpi_scale(dpi_scale);
        }
        self.release_target();
        let (swap_chain, d3d) = {
            let state = self.state.borrow();
            (
                state.swap_chain.as_ref().map(|created| created.swap_chain.clone()),
                state.d3d.clone(),
            )
        };
        if let Some(swap_chain) = swap_chain {
            if let Some(d3d) = &d3d {
                d3d11::clear_state_and_flush(d3d);
            }
            // The clamped sizes are `>= 1`, so the conversions cannot fail.
            let w = u32::try_from(width).unwrap_or(1);
            let h = u32::try_from(height).unwrap_or(1);
            if let Err(error) = dxgi::resize_buffers(&swap_chain, w, h) {
                return Ok(buffers_failed(error));
            }
        }
        self.create_target()?;
        Ok(ResizeOutcome::Resized)
    }

    /// C# `TouchGpu` (WP21-STALL keep-warm): `BeginDraw`, `Clear(opaque black)`, `EndDraw` (must be `>= 0`, else `Err(Device)`),
    /// `d3d11::flush`. Never presents. `Err(Disposed)` after `dispose`.
    pub fn touch_gpu(&self) -> Result<(), RenderFault> {
        if self.state.borrow().disposed {
            return Err(object_disposed());
        }
        let context = self.context()?;
        let color = D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        // SAFETY: `context` is a live owned interface used on its owner thread; no draw is open (the previous one was ended, or
        // failed in `EndDraw`, which also closes it); `color` is a local that outlives the `Clear` call.
        unsafe {
            context.BeginDraw();
            context.Clear(Some(&color));
        }
        // The C# calls `Context.EndDraw` directly here (not through the fault seam).
        let hresult = d2d::end_draw_hresult(&context);
        check("ID2D1DeviceContext::EndDraw", hresult)?;
        let d3d = self.d3d_device()?;
        d3d11::flush(&d3d);
        Ok(())
    }

    /// C# `Synchronize`: lazily creates a 1x1 `CPU_READ | CANNOT_DRAW` probe bitmap (counted in `live_bitmap_count`), `CopyFromBitmap`
    /// one pixel of the target, `Map(READ)`, `Unmap` after a successful `Map`. Waits for the GPU; never on the normal draw path.
    pub fn synchronize(&self) -> Result<(), RenderFault> {
        let existing = self.state.borrow().sync_probe.clone();
        let probe = match existing {
            Some(probe) => probe,
            None => {
                let probe = self.create_bitmap(
                    1,
                    1,
                    None,
                    0,
                    D2D1_ALPHA_MODE_PREMULTIPLIED,
                    D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                )?;
                self.state.borrow_mut().sync_probe = Some(probe.clone());
                probe
            }
        };
        let target = self.target()?;
        let origin = D2D_POINT_2U { x: 0, y: 0 };
        let source = D2D_RECT_U {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        };
        // SAFETY: `probe` and `target` are live owned interfaces of this device; `origin` and `source` are locals that outlive the
        // call and describe one pixel inside both bitmaps.
        unsafe { probe.CopyFromBitmap(Some(&origin), &target, Some(&source)) }.gfx("ID2D1Bitmap::CopyFromBitmap")?;
        // SAFETY: `probe` is a live CPU-readable bitmap that is not mapped and not used by a draw.
        unsafe { probe.Map(D2D1_MAP_OPTIONS_READ) }.gfx("ID2D1Bitmap1::Map")?;
        // SAFETY: the `Map` above succeeded, so the bitmap is mapped exactly once and unmapped exactly once here.
        unsafe { probe.Unmap() }.gfx("ID2D1Bitmap1::Unmap")?;
        Ok(())
    }

    /// C# `CreateTarget` (lines 351 to 388): `gfx::d2d::create_target_bitmap(context, swap chain, w, h, dpi_scale)` then
    /// `gfx::d2d::attach_target(context, bitmap, dpi_scale)`; stores the bitmap in `state.target`. The DXGI surface is dropped before
    /// returning (the bitmap keeps its own reference to the back buffer).
    pub(super) fn create_target(&self) -> Result<(), RenderFault> {
        let context = self.context()?;
        let (swap_chain, width, height, dpi_scale) = {
            let state = self.state.borrow();
            (
                state.swap_chain.as_ref().map(|created| created.swap_chain.clone()),
                state.pixel_width,
                state.pixel_height,
                state.dpi_scale,
            )
        };
        // The sizes are `>= 1` (`clamp_dimension`), so the conversions cannot fail.
        let w = u32::try_from(width).unwrap_or(1);
        let h = u32::try_from(height).unwrap_or(1);
        let bitmap = d2d::create_target_bitmap(&context, swap_chain.as_ref(), w, h, dpi_scale)?;
        d2d::attach_target(&context, &bitmap, dpi_scale);
        self.state.borrow_mut().target = Some(bitmap);
        Ok(())
    }

    /// C# `ReleaseTarget` (lines 390 to 395): `gfx::d2d::detach_target`, then drop `state.target` (in that order; only then can
    /// `ResizeBuffers` succeed). A disposed or never-built object is a no-op.
    pub(super) fn release_target(&self) {
        let (context, target) = {
            let mut state = self.state.borrow_mut();
            (state.context.clone(), state.target.take())
        };
        if let Some(context) = &context {
            d2d::detach_target(context);
        }
        drop(target);
    }
}

/// Maps a `GfxError` of `ResizeBuffers` to the outcome the surface understands (pure; `Ok(BuffersFailed(hresult))`).
#[must_use]
pub fn buffers_failed(error: GfxError) -> ResizeOutcome {
    ResizeOutcome::BuffersFailed(error.hresult)
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_ALPHA_MODE_IGNORE};
    use windows::Win32::Graphics::Direct2D::D2D1_BITMAP_OPTIONS_NONE;
    use windows::Win32::Graphics::Dxgi::DXGI_ERROR_INVALID_CALL;

    use super::super::tests::HiddenWindow;
    use super::*;

    fn offscreen(width: i32, height: i32, dpi: f64) -> std::rc::Rc<DeviceResources> {
        DeviceResources::create_offscreen(width, height, dpi, true).unwrap()
    }

    fn pixel_size(bitmap: &ID2D1Bitmap1) -> (u32, u32) {
        // SAFETY: `bitmap` is a live owned interface; the call takes no arguments.
        let size = unsafe { bitmap.GetPixelSize() };
        (size.width, size.height)
    }

    // C#: InterpolationModes.ToD2D
    #[test]
    fn to_d2d_interpolation_maps_to_the_d2d_constants() {
        assert_eq!(to_d2d_interpolation(ImageInterpolation::NearestNeighbor).0, 0);
        assert_eq!(to_d2d_interpolation(ImageInterpolation::Linear).0, 1);
        assert_eq!(to_d2d_interpolation(ImageInterpolation::HighQualityCubic).0, 5);
        // the C# enum value is NOT the Direct2D one
        assert_eq!(ImageInterpolation::HighQualityCubic as i32, 2);
    }

    // C#: DeviceResources.Resize returns the ResizeBuffers HRESULT
    #[test]
    fn buffers_failed_keeps_the_hresult() {
        let hresult = 0x887A_0005_u32 as i32;
        assert_eq!(
            buffers_failed(GfxError::new("x", hresult)),
            ResizeOutcome::BuffersFailed(hresult)
        );
    }

    // New (D-W8B3R-8): sizes below 1 are rejected before any COM call
    #[test]
    fn create_bitmap_rejects_non_positive_sizes_without_com() {
        let resources = DeviceResources::new_unbuilt(0, 8, 8, 1.0, true, 1);
        let create =
            |w, h| resources.create_bitmap(w, h, None, 0, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BITMAP_OPTIONS_NONE);
        let width = RenderFault::ArgumentOutOfRange { param: "pixelWidth" };
        let height = RenderFault::ArgumentOutOfRange { param: "pixelHeight" };
        assert_eq!(create(0, 4).unwrap_err(), width);
        assert_eq!(create(-1, 0).unwrap_err(), width);
        assert_eq!(create(4, 0).unwrap_err(), height);
        assert_eq!(create(4, i32::MIN).unwrap_err(), height);
        assert_eq!(resources.live_bitmap_count(), 0);
    }

    // C#: Resize on a disposed object (ObjectDisposedException.ThrowIf(_disposed, this))
    #[test]
    fn resize_after_dispose_is_disposed_without_com() {
        let resources = DeviceResources::new_unbuilt(0, 8, 8, 1.0, true, 1);
        resources.dispose();
        assert_eq!(resources.resize(4, 4, 1.0).unwrap_err(), object_disposed());
        assert_eq!(resources.touch_gpu().unwrap_err(), object_disposed());
        resources.release_target(); // no-op
        resources.release_bitmap(None);
        assert_eq!(resources.live_bitmap_count(), 0);
    }

    // C#: DeviceKeepAliveNativeTests.TouchGpu_OnWarpOffscreenDevice_RepeatsWithoutLeakingOrThrowing
    #[test]
    #[ignore = "native: needs desktop"]
    fn touch_gpu_repeats_without_leaking_or_throwing() {
        let r = offscreen(16, 16, 1.0);
        assert!(r.is_warp());
        for _ in 0..200 {
            r.touch_gpu().unwrap();
        }
        assert_eq!(r.live_bitmap_count(), 0);
        assert_eq!(r.generation(), 0);
    }

    // C#: DeviceKeepAliveNativeTests.TouchGpu_AfterDispose_Throws
    #[test]
    #[ignore = "native: needs desktop"]
    fn touch_gpu_after_dispose_is_disposed() {
        let r = offscreen(16, 16, 1.0);
        r.dispose();
        assert_eq!(r.touch_gpu().unwrap_err(), object_disposed());
    }

    // C#: D2DRenderSurfaceTests.Resize_Offscreen_ChangesTargetSizeAndDpi (size and dpi part)
    #[test]
    #[ignore = "native: needs desktop"]
    fn resize_offscreen_changes_size_and_dpi() {
        let r = offscreen(8, 8, 1.0);
        assert_eq!(pixel_size(&r.target().unwrap()), (8, 8));
        assert_eq!(r.resize(30, 12, 2.0).unwrap(), ResizeOutcome::Resized);
        assert_eq!((r.pixel_width(), r.pixel_height()), (30, 12));
        assert_eq!(r.dpi_scale(), 2.0);
        assert_eq!(pixel_size(&r.target().unwrap()), (30, 12));
        // SAFETY: the context is live; the call returns the DPI by out parameters.
        let (mut dpi_x, mut dpi_y) = (0.0_f32, 0.0_f32);
        // SAFETY: both out pointers are locals that outlive the call.
        unsafe { r.context().unwrap().GetDpi(&mut dpi_x, &mut dpi_y) };
        assert_eq!((dpi_x, dpi_y), (192.0, 192.0));
        assert_eq!(r.resize(0, -5, -1.0).unwrap(), ResizeOutcome::Resized);
        assert_eq!((r.pixel_width(), r.pixel_height()), (1, 1));
        assert_eq!(r.dpi_scale(), 1.0);
        assert_eq!(pixel_size(&r.target().unwrap()), (1, 1));
    }

    // C#: D2DWindowSurfaceTests window resize (Resize(320, 200, 1))
    #[test]
    #[ignore = "native: needs desktop"]
    fn resize_window_rebuilds_the_back_buffer_target() {
        let window = HiddenWindow::new();
        let r = DeviceResources::create_for_window(window.raw(), true, 1).unwrap();
        assert_eq!(r.resize(320, 200, 1.0).unwrap(), ResizeOutcome::Resized);
        assert_eq!((r.pixel_width(), r.pixel_height()), (320, 200));
        assert_eq!(pixel_size(&r.target().unwrap()), (320, 200));
        assert_eq!(r.query_maximum_frame_latency().unwrap(), 1);
        assert_ne!(r.frame_latency_wait_handle(), 0);
    }

    // C#: Resize comment "ResizeBuffers fails while a back buffer reference exists" (T1)
    #[test]
    #[ignore = "native: needs desktop"]
    fn resize_fails_with_buffers_failed_while_a_target_clone_is_alive() {
        let window = HiddenWindow::new();
        let r = DeviceResources::create_for_window(window.raw(), true, 1).unwrap();
        let keep = r.target().unwrap();
        match r.resize(200, 120, 1.0).unwrap() {
            ResizeOutcome::BuffersFailed(hresult) => {
                assert!(hresult < 0);
                assert_eq!(
                    hresult, DXGI_ERROR_INVALID_CALL.0,
                    "documented DXGI code for a held back buffer"
                );
            }
            ResizeOutcome::Resized => panic!("ResizeBuffers must fail while a target clone is alive"),
        }
        assert!(
            r.target().is_err(),
            "the target stays released after a failed ResizeBuffers"
        );
        drop(keep);
        assert_eq!(r.resize(200, 120, 1.0).unwrap(), ResizeOutcome::Resized);
        assert_eq!(pixel_size(&r.target().unwrap()), (200, 120));
    }

    // C#: CreateBitmap / ReleaseBitmap counting
    #[test]
    #[ignore = "native: needs desktop"]
    fn create_bitmap_counts_and_release_balances() {
        let r = offscreen(8, 8, 1.0);
        let pixels = [0_u8; 64];
        let bitmap = r
            .create_bitmap(
                4,
                4,
                Some(&pixels),
                16,
                D2D1_ALPHA_MODE_PREMULTIPLIED,
                D2D1_BITMAP_OPTIONS_NONE,
            )
            .unwrap();
        assert_eq!(r.live_bitmap_count(), 1);
        r.release_bitmap(Some(bitmap));
        assert_eq!(r.live_bitmap_count(), 0);
        r.release_bitmap(None);
        assert_eq!(r.live_bitmap_count(), 0);
        let error = r
            .create_bitmap(
                4,
                4,
                Some(&pixels[..15]),
                16,
                D2D1_ALPHA_MODE_PREMULTIPLIED,
                D2D1_BITMAP_OPTIONS_NONE,
            )
            .unwrap_err();
        assert_eq!(
            error,
            RenderFault::Device(GfxError::new("CopyFromMemory", error_hresult(&error)))
        );
        assert_eq!(r.live_bitmap_count(), 0, "a failed copy is not counted");
        let ignore = r
            .create_bitmap(2, 2, None, 0, D2D1_ALPHA_MODE_IGNORE, D2D1_BITMAP_OPTIONS_NONE)
            .unwrap();
        assert_eq!(r.live_bitmap_count(), 1);
        r.release_bitmap(Some(ignore));
    }

    fn error_hresult(error: &RenderFault) -> i32 {
        match error {
            RenderFault::Device(e) => e.hresult,
            other => panic!("not a device error: {other:?}"),
        }
    }

    /// Draws `bitmap` 1:1 onto the target of `r` and reads the target back as tight BGRA bytes.
    fn draw_and_read(r: &DeviceResources, bitmap: &ID2D1Bitmap1, width: u32, height: u32) -> Vec<u8> {
        let context = r.context().unwrap();
        let rect = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: width as f32,
            bottom: height as f32,
        };
        let clear = D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        // SAFETY: the context is live with its target set; `rect` and `clear` are locals that outlive the calls; `bitmap` belongs to
        // the same device; BeginDraw/EndDraw are balanced on this thread.
        unsafe {
            context.BeginDraw();
            context.Clear(Some(&clear));
            context.DrawBitmap(
                bitmap,
                Some(&rect),
                1.0,
                to_d2d_interpolation(ImageInterpolation::NearestNeighbor),
                Some(&rect),
                None,
            );
        }
        assert_eq!(d2d::end_draw_hresult(&context), 0);
        d2d::read_pixels(&context, &r.target().unwrap(), width, height).unwrap()
    }

    // C#: CreateBitmap with initial data (pitch and alpha mode)
    #[test]
    #[ignore = "native: needs desktop"]
    fn create_bitmap_with_data_round_trips_through_read_pixels() {
        let r = offscreen(2, 2, 1.0);
        // B, G, R, A rows; premultiplied-valid
        let pattern: [u8; 16] = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 10, 20, 30, 255];
        let bitmap = r
            .create_bitmap(
                2,
                2,
                Some(&pattern),
                8,
                D2D1_ALPHA_MODE_PREMULTIPLIED,
                D2D1_BITMAP_OPTIONS_NONE,
            )
            .unwrap();
        assert_eq!(draw_and_read(&r, &bitmap, 2, 2), pattern.to_vec());
        // a padded pitch is honoured
        let mut padded = vec![0xEE_u8; 24];
        padded[..8].copy_from_slice(&pattern[..8]);
        padded[12..20].copy_from_slice(&pattern[8..]);
        let bitmap2 = r
            .create_bitmap(
                2,
                2,
                Some(&padded),
                12,
                D2D1_ALPHA_MODE_PREMULTIPLIED,
                D2D1_BITMAP_OPTIONS_NONE,
            )
            .unwrap();
        assert_eq!(draw_and_read(&r, &bitmap2, 2, 2), pattern.to_vec());
    }

    // C#: CreateTarget alpha mode (offscreen premultiplied)
    #[test]
    #[ignore = "native: needs desktop"]
    fn offscreen_target_is_premultiplied_and_clear_reads_back() {
        let r = offscreen(4, 4, 1.0);
        let context = r.context().unwrap();
        let color = D2D1_COLOR_F {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 0.5,
        };
        // SAFETY: the context is live with its target set; `color` outlives the call; BeginDraw/EndDraw are balanced.
        unsafe {
            context.BeginDraw();
            context.Clear(Some(&color));
        }
        assert_eq!(r.end_draw().unwrap(), 0);
        let pixels = d2d::read_pixels(&context, &r.target().unwrap(), 4, 4).unwrap();
        for pixel in pixels.chunks(4) {
            let expected = [0_i32, 0, 128, 128];
            for (got, want) in pixel.iter().zip(expected) {
                assert!((i32::from(*got) - want).abs() <= 1, "BGRA {pixel:?}");
            }
        }
    }

    // C#: DeviceResources.Synchronize lazily creates one probe; ReleaseAll frees it
    #[test]
    #[ignore = "native: needs desktop"]
    fn synchronize_creates_one_probe_and_release_all_frees_it() {
        let r = offscreen(8, 8, 1.0);
        r.synchronize().unwrap();
        r.synchronize().unwrap();
        assert_eq!(r.live_bitmap_count(), 1);
        r.recreate().unwrap();
        assert_eq!(r.live_bitmap_count(), 0);
        r.synchronize().unwrap();
        assert_eq!(r.live_bitmap_count(), 1);
        r.dispose();
        assert_eq!(r.live_bitmap_count(), 0);
    }

    // C#: bitmaps of an old generation stay valid COM objects until released
    #[test]
    #[ignore = "native: needs desktop"]
    fn recreate_keeps_old_bitmaps_valid_until_released() {
        let r = offscreen(8, 8, 1.0);
        let old = r
            .create_bitmap(4, 4, None, 0, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BITMAP_OPTIONS_NONE)
            .unwrap();
        r.recreate().unwrap();
        assert_eq!(r.live_bitmap_count(), 1);
        assert_eq!(pixel_size(&old), (4, 4));
        r.release_bitmap(Some(old));
        assert_eq!(r.live_bitmap_count(), 0);
    }
}
