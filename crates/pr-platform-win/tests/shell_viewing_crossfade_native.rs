//! Offscreen WARP checks for the viewing layers. No window or desktop state is changed.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::primitives::{RectD, SizeD};
use pr_app::viewport::render_geometry::{ImageInterpolation, color_from_rgb};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::render::image_cache::policy::{StripeScheduler, StripeToken};
use pr_platform_win::render::image_cache::upload::{D2dGpuImageCache, ResourcesBitmapDevice};
use pr_platform_win::render::surface::D2dRenderSurface;
use pr_platform_win::shell_viewing::compare::{self, CompareFrame};
use pr_platform_win::shell_viewing::crossfade::CrossfadeLayer;

// This integration target uses only the clock; the shared drawing fakes serve the pure target.
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;
use viewing_fakes::FakeClock;

struct NoStripes;
impl StripeScheduler for NoStripes {
    fn schedule_stripe(&self, _: StripeToken) {}
}

fn solid(b: u8, g: u8, r: u8) -> Arc<PixelBuffer> {
    let mut pixels = PixelBuffer::allocate(64, 64, PixelLayout::Pbgra32).unwrap();
    for pixel in pixels.try_get_span_mut().unwrap().unwrap().chunks_exact_mut(4) {
        pixel.copy_from_slice(&[b, g, r, 255]);
    }
    Arc::new(pixels)
}

fn rig() -> (D2dRenderSurface, D2dGpuImageCache<ResourcesBitmapDevice>) {
    let surface = D2dRenderSurface::create_offscreen_surface(64, 64, 1.0, false).unwrap();
    let device = ResourcesBitmapDevice::new(Rc::clone(surface.resources()));
    let cache = D2dGpuImageCache::new(device, Rc::new(NoStripes), 64, 64).unwrap();
    (surface, cache)
}

fn capture(
    surface: &mut D2dRenderSurface,
    cache: &D2dGpuImageCache<ResourcesBitmapDevice>,
    incoming: &PixelBuffer,
    layer: &CrossfadeLayer,
) -> Vec<u8> {
    let rect = RectD {
        x: 0.0,
        y: 0.0,
        width: 64.0,
        height: 64.0,
    };
    let context = surface.begin_draw_context().unwrap();
    context.clear(color_from_rgb(0, 0, 0, 255));
    pr_app::viewing::image_layer::try_draw(cache, context, incoming, rect, ImageInterpolation::Linear, 1.0, 1).unwrap();
    layer.draw(cache, context, rect).unwrap();
    let pixels = context.try_read_pixels().unwrap().unwrap();
    let bytes = pixels.bytes().unwrap().to_vec();
    surface.end_draw_and_present().unwrap();
    bytes
}

fn sample(
    surface: &mut D2dRenderSurface,
    cache: &D2dGpuImageCache<ResourcesBitmapDevice>,
    incoming: &PixelBuffer,
    layer: &CrossfadeLayer,
) -> [u8; 4] {
    let width = surface.pixel_width() as usize;
    let bytes = capture(surface, cache, incoming, layer);
    let offset = (32 * width + 32) * 4;
    [bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]
}

// C#: PresentationRenderingTests.Crossfade_BlendsTheOutgoingLayerByItsOpacityOverTheIncomingImage
#[test]
#[ignore = "native: needs desktop"]
fn crossfade_blends_the_outgoing_layer_by_its_opacity() {
    let (mut surface, cache) = rig();
    let clock = Rc::new(FakeClock::default());
    let layer = CrossfadeLayer::new(clock.clone(), Rc::new(|| {}), 1_000);
    let red = solid(0, 0, 255);
    let blue = solid(255, 0, 0);
    let rect = RectD {
        x: 0.0,
        y: 0.0,
        width: 64.0,
        height: 64.0,
    };
    layer
        .start(red, rect, ImageInterpolation::Linear, Duration::from_millis(200))
        .unwrap();
    clock.tick(0);
    clock.tick(100);
    let context = surface.begin_draw_context().unwrap();
    context.clear(color_from_rgb(0, 0, 0, 255));
    pr_app::viewing::image_layer::try_draw(&cache, context, &blue, rect, ImageInterpolation::Linear, 1.0, 1).unwrap();
    assert!(layer.draw(&cache, context, rect).unwrap());
    let pixels = context.try_read_pixels().unwrap().unwrap();
    let pixel = &pixels.get_row(32).unwrap()[32 * 4..32 * 4 + 4];
    assert!((i32::from(pixel[0]) - 191).abs() <= 2);
    assert!((i32::from(pixel[2]) - 64).abs() <= 2);
    surface.end_draw_and_present().unwrap();
}

// C#: PresentationRenderingTests.Compare_DrawsBordersCanvasAndUniformImagesInTheirCells
#[test]
#[ignore = "native: needs desktop"]
fn compare_draws_borders_canvas_and_uniform_images_in_their_cells() {
    let (mut surface, cache) = rig();
    let red = solid(0, 0, 255);
    let blue = solid(255, 0, 0);
    let context = surface.begin_draw_context().unwrap();
    context.clear(color_from_rgb(0, 0, 0, 255));
    let frame = CompareFrame {
        left: Some(&red),
        right: Some(&blue),
        left_selected: true,
        right_selected: false,
    };
    assert_eq!(
        compare::draw(
            &cache,
            context,
            SizeD {
                width: 64.0,
                height: 64.0
            },
            &frame,
            ImageInterpolation::Linear
        )
        .unwrap(),
        2
    );
    let pixels = context.try_read_pixels().unwrap().unwrap();
    let row = pixels.get_row(32).unwrap();
    assert_eq!(&row[16 * 4..16 * 4 + 4], &[0, 0, 255, 255]);
    assert_eq!(&row[48 * 4..48 * 4 + 4], &[255, 0, 0, 255]);
    surface.end_draw_and_present().unwrap();
}

// C#: PresentationRenderingTests.Crossfade_Finished_ShowsOnlyTheIncomingImage
#[test]
#[ignore = "native: needs desktop"]
fn crossfade_finished_shows_only_the_incoming_image() {
    let (mut surface, cache) = rig();
    let clock = Rc::new(FakeClock::default());
    let layer = CrossfadeLayer::new(clock.clone(), Rc::new(|| {}), 1_000);
    layer
        .start(
            solid(0, 0, 255),
            RectD {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 64.0,
            },
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    clock.tick(0);
    clock.tick(200);
    assert_eq!(
        sample(&mut surface, &cache, &solid(255, 0, 0), &layer),
        [255, 0, 0, 255]
    );
    assert!(!layer.is_active());
}

// C#: PixelGoldenCrossfadeTests.Crossfade_AtStart_IsExactlyTheOutgoingFrame_AndAtEnd_ExactlyTheIncomingFrame
#[test]
#[ignore = "native: needs desktop"]
fn golden_crossfade_start_and_end_frames_are_exact() {
    for dpi in [1.0, 1.5] {
        let (mut surface, cache) = {
            let surface =
                D2dRenderSurface::create_offscreen_surface((64.0 * dpi) as i32, (64.0 * dpi) as i32, dpi, false)
                    .unwrap();
            let device = ResourcesBitmapDevice::new(Rc::clone(surface.resources()));
            let cache =
                D2dGpuImageCache::new(device, Rc::new(NoStripes), (64.0 * dpi) as i32, (64.0 * dpi) as i32).unwrap();
            (surface, cache)
        };
        let clock = Rc::new(FakeClock::default());
        let layer = CrossfadeLayer::new(clock.clone(), Rc::new(|| {}), 1_000);
        let red = solid(0, 0, 255);
        let blue = solid(255, 0, 0);
        let rect = RectD {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 64.0,
        };
        layer
            .start(red, rect, ImageInterpolation::Linear, Duration::from_millis(200))
            .unwrap();
        clock.tick(0);
        let start = capture(&mut surface, &cache, &blue, &layer);
        assert!(start.chunks_exact(4).all(|pixel| pixel == [0, 0, 255, 255]));
        clock.tick(200);
        let end = capture(&mut surface, &cache, &blue, &layer);
        assert!(end.chunks_exact(4).all(|pixel| pixel == [255, 0, 0, 255]));
    }
}

// C#: PixelGoldenCrossfadeTests.Crossfade_InTheMiddle_IsTheIndependentBlendOfTheTwoEndFrames
#[test]
#[ignore = "native: needs desktop"]
fn golden_crossfade_middle_is_the_blend_of_the_end_frames() {
    let (mut surface, cache) = rig();
    let clock = Rc::new(FakeClock::default());
    let layer = CrossfadeLayer::new(clock.clone(), Rc::new(|| {}), 1_000);
    layer
        .start(
            solid(0, 0, 255),
            RectD {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 64.0,
            },
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    clock.tick(0);
    for (t, alpha) in [(50, 0.5625), (100, 0.25), (150, 0.0625)] {
        clock.tick(t);
        let frame = capture(&mut surface, &cache, &solid(255, 0, 0), &layer);
        for pixel in frame.chunks_exact(4) {
            assert!((f64::from(pixel[0]) - 255.0 * (1.0 - alpha)).abs() <= 2.0);
            assert!((f64::from(pixel[2]) - 255.0 * alpha).abs() <= 2.0);
        }
    }
}

// C#: PixelGoldenCrossfadeTests.Crossfade_RestartedInTheMiddle_DropsTheOldFadeAndStartsAgainFromTheFrameThatWasShown
#[test]
#[ignore = "native: needs desktop"]
fn golden_crossfade_restart_starts_from_the_shown_frame() {
    let (mut surface, cache) = rig();
    let clock = Rc::new(FakeClock::default());
    let layer = CrossfadeLayer::new(clock.clone(), Rc::new(|| {}), 1_000);
    let rect = RectD {
        x: 0.0,
        y: 0.0,
        width: 64.0,
        height: 64.0,
    };
    layer
        .start(
            solid(0, 0, 255),
            rect,
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    clock.tick(0);
    clock.tick(100);
    layer
        .start(
            solid(0, 255, 0),
            rect,
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    clock.tick(100);
    assert_eq!(
        sample(&mut surface, &cache, &solid(255, 0, 0), &layer),
        [0, 255, 0, 255]
    );
    assert_eq!(layer.start_count(), 2);
}

// C#: PresentationRenderingTests.ImageTooLargeToUploadAndDisposedBuffers_AreSkippedWithoutThrowing
#[test]
#[ignore = "native: needs desktop"]
fn disposed_buffers_are_skipped_without_throwing() {
    let (mut surface, cache) = rig();
    let mut disposed = PixelBuffer::allocate(64, 64, PixelLayout::Pbgra32).unwrap();
    disposed.dispose();
    let clock = Rc::new(FakeClock::default());
    let layer = CrossfadeLayer::new(clock, Rc::new(|| {}), 1_000);
    let rect = RectD {
        x: 0.0,
        y: 0.0,
        width: 64.0,
        height: 64.0,
    };
    assert!(
        !layer
            .start(
                Arc::new(disposed),
                rect,
                ImageInterpolation::Linear,
                Duration::from_millis(200)
            )
            .unwrap()
    );
    assert_eq!(
        sample(&mut surface, &cache, &solid(255, 0, 0), &layer),
        [255, 0, 0, 255]
    );
}
