//! Offscreen WARP twins of `PresentationRenderingTests.cs` / `PixelGoldenCrossfadeTests.cs` that need the `PresentationComposer` (W8-16c). No window or
//! desktop state is changed; every test is `#[ignore]` (run with `--include-ignored`). No sleeps: the frame clock is a fake.
#![cfg(windows)]

#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::primitives::{RectD, SizeD};
use pr_app::services::render::gpu_image::PrefetchStatus;
use pr_app::viewport::render_geometry::color_from_rgb;
use pr_core::model::enums::ScalingQuality;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
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

const SIDE: i32 = 64;

fn solid(b: u8, g: u8, r: u8) -> Arc<PixelBuffer> {
    let mut pixels = PixelBuffer::allocate(SIDE, SIDE, PixelLayout::Pbgra32).unwrap();
    for pixel in pixels.try_get_span_mut().unwrap().unwrap().chunks_exact_mut(4) {
        pixel.copy_from_slice(&[b, g, r, 255]);
    }
    Arc::new(pixels)
}

fn gradient() -> Arc<PixelBuffer> {
    let mut pixels = PixelBuffer::allocate(SIDE, SIDE, PixelLayout::Pbgra32).unwrap();
    for (index, pixel) in pixels
        .try_get_span_mut()
        .unwrap()
        .unwrap()
        .chunks_exact_mut(4)
        .enumerate()
    {
        let (x, y) = ((index % SIDE as usize) as u8, (index / SIDE as usize) as u8);
        pixel.copy_from_slice(&[x * 4, y * 4, (x + y) * 2, 255]);
    }
    Arc::new(pixels)
}

struct Rig {
    surface: D2dRenderSurface,
    clock: Rc<FakeClock>,
    composer: PresentationComposer,
}

fn rig() -> Rig {
    let surface = D2dRenderSurface::create_offscreen_surface(SIDE, SIDE, 1.0, false).unwrap();
    let device = ResourcesBitmapDevice::new(Rc::clone(surface.resources()));
    let cache: Rc<dyn PrefetchStatus> = Rc::new(D2dGpuImageCache::new(device, Rc::new(NoStripes), SIDE, SIDE).unwrap());
    let clock = Rc::new(FakeClock::default());
    let composer = PresentationComposer::new(
        cache,
        Rc::clone(&clock) as Rc<dyn HostFrameClock>,
        Rc::new(|| {}),
        Rc::new(|_, _| {}),
        // 1 tick = 1 ms, like the fake clock; the timestamp is only read by the upgrade / settle paths, which these rows do not use.
        ComposerOptions {
            timestamp: Some(Rc::new(|| 0)),
            ticks_per_second: 1_000,
            settle_ms: 120,
        },
    );
    Rig {
        surface,
        clock,
        composer,
    }
}

fn full() -> RectD {
    RectD {
        x: 0.0,
        y: 0.0,
        width: f64::from(SIDE),
        height: f64::from(SIDE),
    }
}

fn inputs<'a>(destination: RectD, viewport: f64, compare: Option<CompareFrame<'a>>) -> PresentationInputs<'a> {
    PresentationInputs {
        viewport: SizeD {
            width: viewport,
            height: viewport,
        },
        main_destination: destination,
        effective_quality: ScalingQuality::Linear,
        compare_quality: ScalingQuality::Linear,
        dpi_scale: 1.0,
        compare_visible: compare.is_some(),
        compare: compare.unwrap_or(CompareFrame {
            left: None,
            right: None,
            left_selected: false,
            right_selected: false,
        }),
    }
}

/// Clears to opaque black, draws one composed frame and returns the surface bytes.
fn frame(rig: &mut Rig, input: &PresentationInputs<'_>) -> Vec<u8> {
    let context = rig.surface.begin_draw_context().unwrap();
    context.clear(color_from_rgb(0, 0, 0, 255));
    rig.composer.draw(context, input).unwrap();
    let pixels = context.try_read_pixels().unwrap().unwrap();
    let bytes = pixels.bytes().unwrap().to_vec();
    rig.surface.end_draw_and_present().unwrap();
    bytes
}

fn at(bytes: &[u8], x: usize, y: usize) -> [u8; 4] {
    let offset = (y * SIDE as usize + x) * 4;
    [bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]
}

// C#: PresentationRenderingTests.MainImage_At1To1_IsByteIdenticalToTheSource
#[test]
#[ignore = "native: needs desktop"]
fn main_image_at_one_to_one_is_byte_identical_to_the_source() {
    let mut rig = rig();
    let source = gradient();
    rig.composer.set(Some(Arc::clone(&source)));

    let bytes = frame(&mut rig, &inputs(full(), f64::from(SIDE), None));

    assert_eq!(bytes.as_slice(), source.bytes().unwrap());
}

// C#: PresentationRenderingTests.MainImage_IsClippedToTheViewport
#[test]
#[ignore = "native: needs desktop"]
fn main_image_is_clipped_to_the_viewport() {
    let mut rig = rig();
    rig.composer.set(Some(solid(0, 0, 255)));

    let bytes = frame(&mut rig, &inputs(full(), 32.0, None));

    assert_eq!(at(&bytes, 16, 16), [0, 0, 255, 255]);
    assert_eq!(at(&bytes, 48, 16), [0, 0, 0, 255], "outside the 32x32 viewport clip");
    assert_eq!(at(&bytes, 16, 48), [0, 0, 0, 255]);
}

// C#: PresentationRenderingTests.Compare_HidesTheMainImageBeneathItsPanel
#[test]
#[ignore = "native: needs desktop"]
fn compare_hides_the_main_image_beneath_its_panel() {
    let mut rig = rig();
    rig.composer.set(Some(solid(0, 0, 255)));
    let frame_inputs = inputs(
        full(),
        f64::from(SIDE),
        Some(CompareFrame {
            left: None,
            right: None,
            left_selected: false,
            right_selected: false,
        }),
    );

    let bytes = frame(&mut rig, &frame_inputs);

    assert_eq!(
        at(&bytes, 32, 32),
        [0x10, 0x10, 0x10, 255],
        "the opaque canvas panel covers the main image"
    );
    assert_eq!(
        at(&bytes, 2, 2),
        [0, 0, 255, 255],
        "the 8 DIP panel margin still shows the main image"
    );
}

// C#: PixelGoldenCrossfadeTests (crossfade under a compare frame)
#[test]
#[ignore = "native: needs desktop"]
fn crossfade_under_a_compare_frame_is_hidden_by_the_panel_and_still_blends_in_the_margin() {
    let mut rig = rig();
    rig.composer.set(Some(solid(0, 0, 255))); // red, the frame the user saw
    frame(&mut rig, &inputs(full(), f64::from(SIDE), None));
    assert!(rig.composer.begin_crossfade(Duration::from_millis(200)).unwrap());
    rig.composer.set(Some(solid(255, 0, 0))); // blue, already current
    rig.clock.tick(0);
    rig.clock.tick(100); // QuadraticOut at 100/200 ms: the old layer is at 0.25
    let frame_inputs = inputs(
        full(),
        f64::from(SIDE),
        Some(CompareFrame {
            left: None,
            right: None,
            left_selected: false,
            right_selected: false,
        }),
    );

    let bytes = frame(&mut rig, &frame_inputs);

    assert_eq!(at(&bytes, 32, 32), [0x10, 0x10, 0x10, 255]);
    let margin = at(&bytes, 2, 2);
    assert!((i32::from(margin[0]) - 191).abs() <= 2, "blue channel {margin:?}");
    assert!((i32::from(margin[2]) - 64).abs() <= 2, "red channel {margin:?}");
}
