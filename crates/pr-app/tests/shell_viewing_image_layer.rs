//! Red tests (W8-16a, test porter) for `pr_app::viewing::image_layer::try_draw`: the port of
//! `PhotoReview.Shell.Win32/Viewing/ImageLayer.cs` (baseline `d92d6aae`).
//!
//! ImageLayer has no C# unit test (it draws through Direct2D types). These are F tests (contract D-16a-6), written from the C#
//! doc comment and the W8-16a contract step 12, with test-local fakes: a `GpuImageCache` that returns a configured result,
//! a recording `DrawContext` that optionally supports `as_oriented()` (the Rust form of `context is D2DDrawContext oriented`,
//! D-W8-12a-5), and a `GpuImage` with an id. The fakes touch no `windows` type.
//!
//! The Direct2D orientation test (`PresentationRenderingTests.Orientation_RawBufferDrawsLikeApplyOrientation`) is not here:
//! it belongs to the native twin of W8-12e (`#[ignore = "native: needs desktop"]`).
//!
//! Every test here fails through `todo!()` until the body lands.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::primitives::{PointD, RectD};
use pr_app::services::render::error::RenderError;
use pr_app::services::render::gpu_image::{GpuImage, GpuImageCache};
use pr_app::services::render::surface::{DrawContext, OrientedDrawContext};
use pr_app::services::render::text::TextLayout;
use pr_app::viewing::image_layer::try_draw;
use pr_app::viewport::render_geometry::{ColorF, ImageInterpolation};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};

// ---------------------------------------------------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------------------------------------------------

/// A cached image: only its buffer id matters to the test.
#[derive(Debug)]
struct FakeImage {
    pixel_buffer_id: i64,
}

impl GpuImage for FakeImage {
    fn pixel_buffer_id(&self) -> i64 {
        self.pixel_buffer_id
    }

    fn pixel_width(&self) -> i32 {
        0
    }

    fn pixel_height(&self) -> i32 {
        0
    }

    fn gpu_bytes(&self) -> i64 {
        0
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// What the fake cache answers from `get_or_upload`.
#[derive(Debug, Clone)]
enum CacheMode {
    /// A fresh image for the buffer.
    Image,
    /// `Err(Disposed)` for the buffer object (the CPU side released it).
    Disposed,
    /// Any other error, returned as is.
    Other(RenderError),
}

#[derive(Debug)]
struct FakeCache {
    mode: CacheMode,
    calls: Cell<u32>,
}

impl FakeCache {
    fn new(mode: CacheMode) -> Self {
        Self {
            mode,
            calls: Cell::new(0),
        }
    }
}

impl GpuImageCache for FakeCache {
    fn get_or_upload(&self, pixels: &PixelBuffer) -> Result<Rc<dyn GpuImage>, RenderError> {
        self.calls.set(self.calls.get() + 1);
        match &self.mode {
            CacheMode::Image => {
                let image: Rc<dyn GpuImage> = Rc::new(FakeImage {
                    pixel_buffer_id: pixels.id(),
                });
                Ok(image)
            }
            CacheMode::Disposed => Err(RenderError::Disposed {
                object: "PhotoReview.Imaging.Pixels.PixelBuffer",
            }),
            CacheMode::Other(error) => Err(error.clone()),
        }
    }

    fn prefetch(&self, _pixels: &Arc<PixelBuffer>) -> Result<(), RenderError> {
        Ok(())
    }

    fn retain(&self, _pixel_buffer_ids: &[i64]) -> Result<(), RenderError> {
        Ok(())
    }

    fn budget_bytes(&self) -> i64 {
        0
    }

    fn set_budget_bytes(&self, _value: i64) -> Result<(), RenderError> {
        Ok(())
    }

    fn clear(&self) {}
}

/// One recorded draw call.
#[derive(Debug, Clone, PartialEq)]
enum DrawCall {
    /// `draw_image` (the plain path).
    Plain {
        image_id: i64,
        destination: RectD,
        source: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    },
    /// `draw_image_oriented` (the oriented path, EXIF 2..=8).
    Oriented {
        image_id: i64,
        destination: RectD,
        exif_orientation: i32,
        source: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    },
}

/// A recording draw context. `supports_oriented` decides whether `as_oriented()` returns `Some(self)`.
#[derive(Debug)]
struct FakeDraw {
    supports_oriented: bool,
    calls: Vec<DrawCall>,
}

impl FakeDraw {
    fn new(supports_oriented: bool) -> Self {
        Self {
            supports_oriented,
            calls: Vec::new(),
        }
    }
}

impl DrawContext for FakeDraw {
    fn clear(&mut self, _color: ColorF) {}

    fn draw_image(
        &mut self,
        image: &dyn GpuImage,
        destination: RectD,
        source_pixels: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    ) {
        self.calls.push(DrawCall::Plain {
            image_id: image.pixel_buffer_id(),
            destination,
            source: source_pixels,
            interpolation,
            opacity,
        });
    }

    fn fill_rectangle(&mut self, _rect: RectD, _color: ColorF) {}

    fn fill_rounded_rectangle(&mut self, _rect: RectD, _radius: f64, _color: ColorF) {}

    fn draw_rectangle(&mut self, _rect: RectD, _color: ColorF, _stroke_width: f64) {}

    fn draw_text(&mut self, _layout: &dyn TextLayout, _origin: PointD, _color: ColorF) -> Result<(), RenderError> {
        Ok(())
    }

    fn push_clip(&mut self, _rect: RectD) {}

    fn pop_clip(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    fn push_opacity(&mut self, _opacity: f32) {}

    fn pop_opacity(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    fn try_read_pixels(&mut self) -> Result<Option<PixelBuffer>, RenderError> {
        Ok(None)
    }

    fn as_oriented(&mut self) -> Option<&mut dyn OrientedDrawContext> {
        if self.supports_oriented {
            Some(self as &mut dyn OrientedDrawContext)
        } else {
            None
        }
    }
}

impl OrientedDrawContext for FakeDraw {
    fn draw_image_oriented(
        &mut self,
        image: &dyn GpuImage,
        destination: RectD,
        exif_orientation: i32,
        source_pixels: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    ) {
        self.calls.push(DrawCall::Oriented {
            image_id: image.pixel_buffer_id(),
            destination,
            exif_orientation,
            source: source_pixels,
            interpolation,
            opacity,
        });
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

fn buffer() -> PixelBuffer {
    PixelBuffer::allocate(4, 4, PixelLayout::Pbgra32).expect("allocate test buffer")
}

fn destination() -> RectD {
    RectD {
        x: 10.0,
        y: 20.0,
        width: 300.0,
        height: 200.0,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------------------------

// new (F, from the C# doc comment): a disposed buffer draws nothing and the cache is never asked.
#[test]
fn disposed_buffer_draws_nothing_and_never_calls_cache() {
    let mut px = buffer();
    px.dispose();
    let cache = FakeCache::new(CacheMode::Image);
    let mut draw = FakeDraw::new(true);

    let drawn = try_draw(
        &cache,
        &mut draw,
        &px,
        destination(),
        ImageInterpolation::HighQualityCubic,
        1.0,
        1,
    );

    assert_eq!(drawn, Ok(false));
    assert_eq!(cache.calls.get(), 0);
    assert!(draw.calls.is_empty());
}

// new (F): an empty or NaN destination draws nothing and never reaches the cache.
#[test]
fn empty_or_nan_destination_draws_nothing() {
    let rects = [
        RectD {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 10.0,
        },
        RectD {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 0.0,
        },
        RectD {
            x: 0.0,
            y: 0.0,
            width: -5.0,
            height: 10.0,
        },
        RectD {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: -5.0,
        },
        RectD {
            x: 0.0,
            y: 0.0,
            width: f64::NAN,
            height: 10.0,
        },
        RectD {
            x: f64::NAN,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        },
    ];
    for rect in rects {
        let px = buffer();
        let cache = FakeCache::new(CacheMode::Image);
        let mut draw = FakeDraw::new(true);

        let drawn = try_draw(&cache, &mut draw, &px, rect, ImageInterpolation::Linear, 1.0, 1);

        assert_eq!(drawn, Ok(false), "{rect:?}");
        assert_eq!(cache.calls.get(), 0, "{rect:?}: the cache must not be asked");
        assert!(draw.calls.is_empty(), "{rect:?}");
    }
}

// new (F): the cache reports the buffer as disposed (`RenderError::Disposed`): `Ok(false)`, nothing drawn.
#[test]
fn cache_disposed_error_is_ok_false() {
    let px = buffer();
    let cache = FakeCache::new(CacheMode::Disposed);
    let mut draw = FakeDraw::new(true);

    let drawn = try_draw(
        &cache,
        &mut draw,
        &px,
        destination(),
        ImageInterpolation::Linear,
        1.0,
        1,
    );

    assert_eq!(drawn, Ok(false));
    assert_eq!(cache.calls.get(), 1);
    assert!(draw.calls.is_empty());
}

// new (F): any other cache error is returned as is (C# throws; only ObjectDisposedException is caught).
#[test]
fn cache_other_error_is_returned() {
    let px = buffer();
    let error = RenderError::Device {
        hresult: -2_005_270_523,
        operation: "CreateBitmapFromWicBitmap",
    };
    let cache = FakeCache::new(CacheMode::Other(error.clone()));
    let mut draw = FakeDraw::new(true);

    let drawn = try_draw(
        &cache,
        &mut draw,
        &px,
        destination(),
        ImageInterpolation::Linear,
        1.0,
        1,
    );

    assert_eq!(drawn, Err(error));
    assert!(draw.calls.is_empty());
}

// new (F): orientation 1 (and any value outside 2..=8) draws through `draw_image` with no source rectangle.
#[test]
fn orientation_1_uses_draw_image_with_no_source() {
    let px = buffer();
    let cache = FakeCache::new(CacheMode::Image);
    let mut draw = FakeDraw::new(true);

    let drawn = try_draw(
        &cache,
        &mut draw,
        &px,
        destination(),
        ImageInterpolation::HighQualityCubic,
        1.0,
        1,
    );

    assert_eq!(drawn, Ok(true));
    assert_eq!(
        draw.calls,
        vec![DrawCall::Plain {
            image_id: px.id(),
            destination: destination(),
            source: None,
            interpolation: ImageInterpolation::HighQualityCubic,
            opacity: 1.0,
        }]
    );
}

// new (F): the orientations outside 2..=8 (0, 9, -1) also take the plain `draw_image` path.
#[test]
fn orientation_outside_2_to_8_uses_draw_image() {
    for exif in [0, 9, -1] {
        let px = buffer();
        let cache = FakeCache::new(CacheMode::Image);
        let mut draw = FakeDraw::new(true);

        let drawn = try_draw(
            &cache,
            &mut draw,
            &px,
            destination(),
            ImageInterpolation::Linear,
            1.0,
            exif,
        );

        assert_eq!(drawn, Ok(true), "exif {exif}");
        assert!(
            matches!(draw.calls.as_slice(), [DrawCall::Plain { source: None, .. }]),
            "exif {exif}: {:?}",
            draw.calls
        );
    }
}

// new (F): orientations 2..=8 go through `as_oriented()` when the context supports it.
#[test]
fn orientation_2_to_8_uses_as_oriented_when_present() {
    for exif in 2..=8 {
        let px = buffer();
        let cache = FakeCache::new(CacheMode::Image);
        let mut draw = FakeDraw::new(true);

        let drawn = try_draw(
            &cache,
            &mut draw,
            &px,
            destination(),
            ImageInterpolation::Linear,
            1.0,
            exif,
        );

        assert_eq!(drawn, Ok(true), "exif {exif}");
        assert_eq!(
            draw.calls,
            vec![DrawCall::Oriented {
                image_id: px.id(),
                destination: destination(),
                exif_orientation: exif,
                source: None,
                interpolation: ImageInterpolation::Linear,
                opacity: 1.0,
            }],
            "exif {exif}"
        );
    }
}

// new (F): when `as_oriented()` is `None` (C# `context is D2DDrawContext oriented` false), a rotated image falls back to a
// plain `draw_image` with no source rectangle, and still returns `Ok(true)`.
#[test]
fn as_oriented_none_falls_back_to_draw_image() {
    let px = buffer();
    let cache = FakeCache::new(CacheMode::Image);
    let mut draw = FakeDraw::new(false);

    let drawn = try_draw(
        &cache,
        &mut draw,
        &px,
        destination(),
        ImageInterpolation::Linear,
        1.0,
        6,
    );

    assert_eq!(drawn, Ok(true));
    assert_eq!(
        draw.calls,
        vec![DrawCall::Plain {
            image_id: px.id(),
            destination: destination(),
            source: None,
            interpolation: ImageInterpolation::Linear,
            opacity: 1.0,
        }]
    );
}

// new (F): the opacity reaches both draw paths unchanged.
#[test]
fn opacity_is_passed_through() {
    let px = buffer();
    let cache = FakeCache::new(CacheMode::Image);

    let mut plain = FakeDraw::new(true);
    let drawn = try_draw(
        &cache,
        &mut plain,
        &px,
        destination(),
        ImageInterpolation::Linear,
        0.25,
        1,
    );
    assert_eq!(drawn, Ok(true));
    assert!(
        matches!(plain.calls.as_slice(), [DrawCall::Plain { opacity, .. }] if *opacity == 0.25),
        "{:?}",
        plain.calls
    );

    let mut oriented = FakeDraw::new(true);
    let drawn = try_draw(
        &cache,
        &mut oriented,
        &px,
        destination(),
        ImageInterpolation::Linear,
        0.5,
        6,
    );
    assert_eq!(drawn, Ok(true));
    assert!(
        matches!(oriented.calls.as_slice(), [DrawCall::Oriented { opacity, .. }] if *opacity == 0.5),
        "{:?}",
        oriented.calls
    );
}
