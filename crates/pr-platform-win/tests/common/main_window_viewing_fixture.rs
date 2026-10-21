//! Helpers of the W8-13j tests (`host_main_window_viewing.rs`): small images, a window with its renderer attached, and one composer frame drawn on the window's
//! surface (the crossfade freezes `last_drawn`, which only a draw sets). UI thread only. Needs `main_window_fixture` (declared as a sibling module).
#![allow(dead_code)] // each test uses a different part

use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::primitives::{RectD, SizeD};
use pr_core::model::enums::ScalingQuality;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::shell_viewing::compare::CompareFrame;
use pr_platform_win::shell_viewing::composer::{PresentationComposer, PresentationInputs};

use crate::main_window_fixture::{BuildOptions, Built, WarpHook, build};

/// A solid `side` x `side` PBGRA image.
pub fn solid(side: i32, value: u8) -> Arc<PixelBuffer> {
    let mut pixels = PixelBuffer::allocate(side, side, PixelLayout::Pbgra32).unwrap();
    for pixel in pixels.try_get_span_mut().unwrap().unwrap().chunks_exact_mut(4) {
        pixel.copy_from_slice(&[value, value, value, 255]);
    }
    Arc::new(pixels)
}

/// Makes the hidden window and attaches the renderer over a WARP offscreen surface (the composer, the prefetcher and the cache exist afterwards).
pub fn built_with_renderer() -> Rc<Built> {
    let built = build(BuildOptions::default());
    let (width, height) = built.shell.client_size_pixels();
    built
        .window
        .attach_renderer(Some(&WarpHook::new(width, height)))
        .expect("attach_renderer");
    built
}

/// Draws one frame of `composer` on the window's surface (so `last_drawn` is the composer's current image).
pub fn draw_one_frame(built: &Built, composer: &PresentationComposer) {
    built
        .window
        .with_surface(|surface| {
            let context = surface.begin_draw_context().expect("begin draw");
            let inputs = PresentationInputs {
                viewport: SizeD {
                    width: 64.0,
                    height: 64.0,
                },
                main_destination: RectD {
                    x: 0.0,
                    y: 0.0,
                    width: 64.0,
                    height: 64.0,
                },
                effective_quality: ScalingQuality::HighQuality,
                compare_quality: ScalingQuality::HighQuality,
                dpi_scale: 1.0,
                compare_visible: false,
                compare: CompareFrame::empty(),
            };
            composer.draw(context, &inputs).expect("draw");
            surface.end_draw_and_present().expect("present");
        })
        .expect("the surface is free");
}
