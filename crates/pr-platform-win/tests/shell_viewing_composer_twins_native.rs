//! Offscreen WARP twins of the three `PresentationRenderingTests.cs` rows that `shell_viewing_composer_native.rs` did not carry (cl-misc, X-viewing-composer):
//! `Orientation_RawBufferDrawsLikeApplyOrientation` (2, 3, 6, 8), `LargeImage_SplitIntoTiles_HasNoSeamAtFit` and
//! `DeviceLostDuringACrossfade_ReuploadsBothImagesAndTheFadeContinues`. No window or desktop state is changed; every test is `#[ignore]` (run with
//! `--include-ignored`). No sleeps: the frame clock is a fake.
#![cfg(windows)]

#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::primitives::{RectD, SizeD};
use pr_app::services::render::gpu_image::PrefetchStatus;
use pr_app::viewing::image_layer::try_draw;
use pr_app::viewport::render_geometry::{ImageInterpolation, color_from_rgb};
use pr_core::model::enums::ScalingQuality;
use pr_imaging::pixels::{PixelBuffer, PixelLayout, apply_orientation};
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::render::image_cache::policy::{StripeScheduler, StripeToken};
use pr_platform_win::render::image_cache::upload::{D2dGpuImageCache, ResourcesBitmapDevice};
use pr_platform_win::render::surface::D2dRenderSurface;
use pr_platform_win::shell_viewing::compare::CompareFrame;
use pr_platform_win::shell_viewing::composer::{ComposerOptions, PresentationComposer, PresentationInputs};
use viewing_fakes::FakeClock;

struct NoStripes;
impl StripeScheduler for NoStripes {
    fn schedule_stripe(&self, _: StripeToken) {}
}

type Cache = D2dGpuImageCache<ResourcesBitmapDevice>;

struct Rig {
    surface: D2dRenderSurface,
    cache: Rc<Cache>,
    clock: Rc<FakeClock>,
    composer: PresentationComposer,
}

fn rig(width: i32, height: i32) -> Rig {
    let mut surface = D2dRenderSurface::create_offscreen_surface(width, height, 1.0, false).unwrap();
    let device = ResourcesBitmapDevice::new(Rc::clone(surface.resources()));
    device.attach_to_surface(&mut surface); // a device recreation clears the cache, like the C# GpuImageCache(surface, dispatcher)
    let cache = Rc::new(D2dGpuImageCache::new(device, Rc::new(NoStripes), width, height).unwrap());
    let clock = Rc::new(FakeClock::default());
    let composer = PresentationComposer::new(
        Rc::clone(&cache) as Rc<dyn PrefetchStatus>,
        Rc::clone(&clock) as Rc<dyn HostFrameClock>,
        Rc::new(|| {}),
        Rc::new(|_, _| {}),
        ComposerOptions {
            timestamp: Some(Rc::new(|| 0)),
            ticks_per_second: 1_000,
            settle_ms: 120,
        },
    );
    Rig {
        surface,
        cache,
        clock,
        composer,
    }
}

fn rect(width: f64, height: f64) -> RectD {
    RectD {
        x: 0.0,
        y: 0.0,
        width,
        height,
    }
}

fn inputs(destination: RectD, width: f64, height: f64) -> PresentationInputs<'static> {
    PresentationInputs {
        viewport: SizeD { width, height },
        main_destination: destination,
        effective_quality: ScalingQuality::Linear,
        compare_quality: ScalingQuality::Linear,
        dpi_scale: 1.0,
        compare_visible: false,
        compare: CompareFrame {
            left: None,
            right: None,
            left_selected: false,
            right_selected: false,
        },
    }
}

/// Clears to the opaque canvas grey, draws one composed frame and returns the surface bytes (B, G, R, A rows, stride `width * 4`).
fn frame(rig: &mut Rig, input: &PresentationInputs<'_>) -> Vec<u8> {
    let context = rig.surface.begin_draw_context().unwrap();
    context.clear(color_from_rgb(0x10, 0x10, 0x10, 255));
    rig.composer.draw(context, input).unwrap();
    let pixels = context.try_read_pixels().unwrap().unwrap();
    let bytes = pixels.bytes().unwrap().to_vec();
    rig.surface.end_draw_and_present().unwrap();
    bytes
}

fn solid(width: i32, height: i32, b: u8, g: u8, r: u8) -> Arc<PixelBuffer> {
    let mut pixels = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
    for pixel in pixels.try_get_span_mut().unwrap().unwrap().chunks_exact_mut(4) {
        pixel.copy_from_slice(&[b, g, r, 255]);
    }
    Arc::new(pixels)
}

/// Stand-in for C# `RenderTestImages.CreateSmooth`: a deterministic opaque gradient with distinct rows and columns (an orientation mix-up changes every row).
fn smooth(width: i32, height: i32) -> PixelBuffer {
    let mut pixels = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
    for y in 0..height {
        for (x, pixel) in pixels.get_row_mut(y).unwrap().chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[(x as i32 * 11) as u8, (y * 17) as u8, (x as i32 * 5 + y * 3) as u8, 255]);
        }
    }
    pixels
}

fn at(bytes: &[u8], stride_pixels: usize, x: usize, y: usize) -> [u8; 4] {
    let offset = (y * stride_pixels + x) * 4;
    [bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]
}

// C#: PresentationRenderingTests.Orientation_RawBufferDrawsLikeApplyOrientation (orientations 2, 3, 6, 8)
#[test]
#[ignore = "native: needs desktop"]
fn orientation_raw_buffer_draws_like_apply_orientation() {
    for orientation in [2_i32, 3, 6, 8] {
        let mut rig = rig(40, 40);
        let raw = smooth(20, 12);
        let reference = apply_orientation(smooth(20, 12), orientation).unwrap();
        let (rw, rh) = (reference.width(), reference.height());
        let destination = rect(f64::from(rw), f64::from(rh));
        let context = rig.surface.begin_draw_context().unwrap();
        context.clear(color_from_rgb(0x10, 0x10, 0x10, 255));
        let drawn = try_draw(
            &*rig.cache,
            context,
            &raw,
            destination,
            ImageInterpolation::NearestNeighbor,
            1.0,
            orientation,
        )
        .unwrap();
        assert!(drawn, "orientation {orientation}");
        let pixels = context.try_read_pixels().unwrap().unwrap();
        let bytes = pixels.bytes().unwrap().to_vec();
        rig.surface.end_draw_and_present().unwrap();

        for y in 0..rh {
            let expected = reference.get_row(y).unwrap();
            let start = (y as usize * 40) * 4;
            assert_eq!(
                &bytes[start..start + rw as usize * 4],
                expected,
                "orientation {orientation}, row {y}"
            );
        }
    }
}

// C#: PresentationRenderingTests.LargeImage_SplitIntoTiles_HasNoSeamAtFit
#[test]
#[ignore = "native: needs desktop"]
fn large_image_split_into_tiles_has_no_seam_at_fit() {
    // 17000 > 16384: two tiles. A one-colour picture scaled to 1000 px: every destination pixel must have the colour (a seam = an odd pixel or a gap).
    let mut rig = rig(1000, 80);
    let image = solid(17000, 1200, 90, 160, 220);
    rig.composer.set(Some(Arc::clone(&image)));

    let bytes = frame(&mut rig, &inputs(rect(1000.0, 70.588), 1000.0, 80.0));

    assert_eq!(rig.cache.inner().count(), 1);
    assert_eq!(
        rig.cache.get_or_upload_tiled(&image).unwrap().tiles().len(),
        2,
        "17000 px wide splits into two tiles"
    );
    for x in 0..1000 {
        assert_eq!(at(&bytes, 1000, x, 20), [90, 160, 220, 255], "x = {x}");
    }
}

// C#: PresentationRenderingTests.DeviceLostDuringACrossfade_ReuploadsBothImagesAndTheFadeContinues
#[test]
#[ignore = "native: needs desktop"]
fn device_lost_during_a_crossfade_reuploads_both_images_and_the_fade_continues() {
    let mut rig = rig(32, 24);
    let box_ = rect(32.0, 24.0);
    rig.composer.set(Some(solid(32, 24, 0, 0, 200)));
    frame(&mut rig, &inputs(box_, 32.0, 24.0));

    assert!(rig.composer.begin_crossfade(Duration::from_millis(200)).unwrap());
    rig.composer
        .set_incoming(Some(solid(32, 24, 200, 0, 0)), true, 0)
        .unwrap();
    frame(&mut rig, &inputs(box_, 32.0, 24.0));
    assert_eq!(rig.cache.inner().count(), 2);

    let generation = rig.surface.device_generation();
    rig.surface.recreate_device().unwrap(); // like D2DERR_RECREATE_TARGET / DEVICE_REMOVED
    assert!(rig.surface.device_generation() > generation);
    assert_eq!(rig.cache.inner().count(), 0, "the GPU cache is cleared");

    rig.clock.tick(20_000);
    rig.clock.tick(20_100); // half the duration: QuadraticOut opacity 0.25
    let bytes = frame(&mut rig, &inputs(box_, 32.0, 24.0));

    assert_eq!(
        rig.cache.inner().count(),
        2,
        "both images are uploaded again from their PixelBuffers"
    );
    let pixel = at(&bytes, 32, 16, 12);
    assert!((i32::from(pixel[2]) - 50).abs() <= 3, "red {pixel:?}");
    assert!((i32::from(pixel[0]) - 150).abs() <= 3, "blue {pixel:?}");
}
