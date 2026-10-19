//! Red tests (W8-12a, test porter) for the render contracts `services::render::{error, gpu_image, surface, text, tiled}`.
//! Contract: `contracts/W8-12a.md`. Baseline `d92d6aae`: `Shell.Rendering/Contracts/*.cs` and `TiledGpuImage.cs`.
//!
//! Every test calls a stub that still ends in `todo!()` (or reaches one through the object under test), so the tests fail
//! until the bodies land. Test names mirror the C# test methods; a ported test carries a `// C#: <Class>.<Method>` comment.
//! Tests the contract adds are marked `// new:`. The fakes below are test-local (W8-15/W8-16 copy them, contract Q3).
//!
//! Documented skips (not ported here):
//! - `tests/PhotoReview.Architecture.Tests/ContractSurfaceTests.cs` `Contracts_MatchApprovedSurface` (reflection over .NET
//!   assemblies, not portable). Its Rust twin is `contract_traits_are_object_safe_and_their_signatures_hold` below.
//! - The rest of `GpuImageCacheTests.cs` (cache policy, LRU, prefetch stripes) belongs to W8-12f/12g. The test
//!   `PixelBuffer_HasNoRowPadding_...` belongs to W8-12g (the Rust `PixelBuffer` is already in tree and tested).
//! - Throw-behaviour tests of the implementations (`D2DRenderSurfaceTests`, `D2DWindowSurfaceTests`, `GpuImageCacheTests`
//!   lines 161/306/316, `DWriteTextRendererTests`) belong to W8-12d/12e/12f/12i, which implement these traits.
//! - Real-GPU tests (`GpuImageCacheNativeTests`, `D2DRenderSurfaceTests` on hardware) are native and out of this unit.
//! - Accessor-only methods (`GpuTile` bitmap accessors, `TiledGpuImage` setters) are real code in the stub, so tests that
//!   use only them pass on the stub. Every other test here reaches a `todo!()`.

use std::any::Any;
use std::cell::Cell;
use std::error::Error;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::services::render::error::RenderError;
use pr_app::services::render::gpu_image::{GpuImage, GpuImageCache};
use pr_app::services::render::surface::{
    DEFAULT_IMAGE_OPACITY, DeviceRecreatedHandler, DeviceRecreatedHandlerId, DrawContext, OrientedDrawContext,
    PresentResult, RenderSurface, RenderSurfaceFactory, RenderSurfaceOptions,
};
use pr_app::services::render::text::{TextLayout, TextRenderer, TextStyle, TextTrimming};
use pr_app::services::render::tiled::{GpuTile, TiledGpuImage};
use pr_app::viewport::render_geometry::{ColorF, ImageInterpolation, PixelRect, TileSpec};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};

fn buffer(width: i32, height: i32, layout: PixelLayout) -> PixelBuffer {
    PixelBuffer::allocate(width, height, layout).expect("allocate test buffer")
}

fn spec(texture: PixelRect, core: PixelRect) -> TileSpec {
    TileSpec { texture, core }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

// ---------------------------------------------------------------------------------------------------------------------------
// TiledGpuImage / GpuTile (tiled.rs)
// ---------------------------------------------------------------------------------------------------------------------------

// C#: GpuImageCacheTests.TiledGpuImage_IsDrawable_OnlyForItsOwnDeviceGenerationAndWhenCompleteAndNotReleased
#[test]
fn tiled_gpu_image_is_drawable_only_for_its_own_device_generation_and_when_complete_and_not_released() {
    let px = buffer(4, 4, PixelLayout::Pbgra32);
    let image: TiledGpuImage<()> = TiledGpuImage::new(&px, Vec::new(), 3);
    image.set_uploaded_rows(4);

    assert!(image.is_drawable(3));
    // device recreated: a bitmap of the old generation is not drawn
    assert!(!image.is_drawable(4));

    image.set_uploaded_rows(2);
    // prefetch still running
    assert!(!image.is_drawable(3));

    image.set_uploaded_rows(4);
    image.set_released(true);
    assert!(!image.is_drawable(3));
}

// new: the constructor copies the identity of the buffer and starts empty (uploaded_rows 0, not released, not complete)
#[test]
fn tiled_gpu_image_new_copies_buffer_identity_and_starts_empty() {
    let px = buffer(7, 3, PixelLayout::Bgr32);
    let image: TiledGpuImage<()> = TiledGpuImage::new(&px, Vec::new(), 5);

    assert_eq!(image.pixel_buffer_id(), px.id());
    assert_eq!(image.pixel_width(), 7);
    assert_eq!(image.pixel_height(), 3);
    assert_eq!(image.layout(), PixelLayout::Bgr32);
    assert_eq!(image.uploaded_rows(), 0);
    assert!(!image.is_released());
    assert!(!image.is_complete());
    assert_eq!(image.generation(), 5);
    assert_eq!(image.gpu_bytes(), 0);
}

// new: gpu_bytes sums the TEXTURE rects (gutters included), 4 bytes per pixel; a core-based sum would give 30000
#[test]
fn tiled_gpu_image_gpu_bytes_sums_texture_rects_including_gutters() {
    let px = buffer(190, 50, PixelLayout::Pbgra32);
    let tiles = vec![
        GpuTile::<()>::new(spec(PixelRect::new(0, 0, 100, 50), PixelRect::new(0, 0, 90, 50)), None),
        GpuTile::<()>::new(spec(PixelRect::new(90, 0, 60, 50), PixelRect::new(90, 0, 60, 50)), None),
    ];
    let image: TiledGpuImage<()> = TiledGpuImage::new(&px, tiles, 1);
    assert_eq!(image.gpu_bytes(), 100 * 50 * 4 + 60 * 50 * 4);
    assert_eq!(image.gpu_bytes(), 32_000);

    let empty: TiledGpuImage<()> = TiledGpuImage::new(&px, Vec::new(), 1);
    assert_eq!(empty.gpu_bytes(), 0);
}

// new: three 16384 x 16384 textures are 3 221 225 472 bytes, which does not fit in i32; a 46341 x 46341 texture is 8 589 953 124
#[test]
fn tiled_gpu_image_gpu_bytes_does_not_overflow_i32() {
    // gpu_bytes comes from the tile specs, so the buffer itself stays small
    let px = buffer(4, 4, PixelLayout::Pbgra32);
    let big = PixelRect::new(0, 0, 16384, 16384);
    let tiles = vec![
        GpuTile::<()>::new(spec(big, big), None),
        GpuTile::<()>::new(spec(big, big), None),
        GpuTile::<()>::new(spec(big, big), None),
    ];
    let image: TiledGpuImage<()> = TiledGpuImage::new(&px, tiles, 1);
    assert_eq!(image.gpu_bytes(), 3_221_225_472_i64);

    // 46341 * 46341 = 2 147 488 281 is already above i32::MAX before the factor 4
    let huge = PixelRect::new(0, 0, 46341, 46341);
    let one: TiledGpuImage<()> = TiledGpuImage::new(&px, vec![GpuTile::<()>::new(spec(huge, huge), None)], 1);
    assert_eq!(one.gpu_bytes(), 8_589_953_124_i64);
}

// new: is_complete is uploaded_rows >= pixel_height (not ==): 4 of 5 rows is not complete, 5 and 6 are
#[test]
fn tiled_gpu_image_is_complete_boundaries() {
    let px = buffer(3, 5, PixelLayout::Pbgra32);
    let image: TiledGpuImage<()> = TiledGpuImage::new(&px, Vec::new(), 1);

    for (rows, expected) in [(0, false), (4, false), (5, true), (6, true)] {
        image.set_uploaded_rows(rows);
        assert_eq!(image.is_complete(), expected, "uploaded_rows = {rows}");
    }
}

// new: all 8 combinations of (released, complete, same generation); only (not released, complete, same generation) is drawable
#[test]
fn tiled_gpu_image_is_drawable_truth_table() {
    let px = buffer(2, 2, PixelLayout::Pbgra32);
    for released in [false, true] {
        for complete in [false, true] {
            for same_generation in [false, true] {
                let image: TiledGpuImage<()> = TiledGpuImage::new(&px, Vec::new(), 3);
                image.set_uploaded_rows(if complete { 2 } else { 1 });
                image.set_released(released);
                let current = if same_generation { 3 } else { 4 };
                let expected = !released && complete && same_generation;
                assert_eq!(
                    image.is_drawable(current),
                    expected,
                    "released={released} complete={complete} same_generation={same_generation}"
                );
            }
        }
    }
}

// new: the generation is compared by equality, also for i64::MIN and negative values
#[test]
fn tiled_gpu_image_generation_compares_by_equality_for_extreme_values() {
    let px = buffer(2, 2, PixelLayout::Pbgra32);

    let min: TiledGpuImage<()> = TiledGpuImage::new(&px, Vec::new(), i64::MIN);
    min.set_uploaded_rows(2);
    assert!(min.is_drawable(i64::MIN));
    assert!(!min.is_drawable(-1));
    assert!(!min.is_drawable(i64::MAX));

    let negative: TiledGpuImage<()> = TiledGpuImage::new(&px, Vec::new(), -1);
    negative.set_uploaded_rows(2);
    assert!(negative.is_drawable(-1));
    assert!(!negative.is_drawable(0));
}

// new: through Rc<dyn GpuImage> the four getters return the stored values; as_any downcasts only to the exact B
#[test]
fn tiled_gpu_image_exposes_the_gpu_image_trait() {
    let px = buffer(7, 3, PixelLayout::Pbgra32);
    let tiles = vec![GpuTile::<()>::new(
        spec(PixelRect::new(0, 0, 10, 5), PixelRect::new(0, 0, 10, 5)),
        None,
    )];
    let image: TiledGpuImage<()> = TiledGpuImage::new(&px, tiles, 1);
    image.set_uploaded_rows(3);

    let shared: Rc<dyn GpuImage> = Rc::new(image);
    assert_eq!(shared.pixel_buffer_id(), px.id());
    assert_eq!(shared.pixel_width(), 7);
    assert_eq!(shared.pixel_height(), 3);
    assert_eq!(shared.gpu_bytes(), 10 * 5 * 4);
    assert!(shared.as_any().downcast_ref::<TiledGpuImage<()>>().is_some());
    assert!(shared.as_any().downcast_ref::<TiledGpuImage<u32>>().is_none());
}

// new: two Rc clones share the mutable state (uploaded rows, released flag)
#[test]
fn tiled_gpu_image_state_is_shared_through_rc() {
    let px = buffer(2, 2, PixelLayout::Pbgra32);
    let first: Rc<TiledGpuImage<()>> = Rc::new(TiledGpuImage::new(&px, Vec::new(), 1));
    let second = Rc::clone(&first);

    first.set_uploaded_rows(2);
    assert_eq!(second.uploaded_rows(), 2);
    assert!(second.is_complete());

    second.set_released(true);
    assert!(first.is_released());
    assert!(!first.is_drawable(1));
}

// new: a GpuTile's bitmap can be set, read and taken; the spec does not change
#[test]
fn gpu_tile_bitmap_can_be_set_taken_and_read() {
    let tile_spec = spec(PixelRect::new(1, 2, 3, 4), PixelRect::new(1, 2, 3, 4));
    let tile = GpuTile::<u32>::new(tile_spec, None);
    assert!(!tile.has_bitmap());

    tile.set_bitmap(Some(7));
    assert!(tile.has_bitmap());
    assert_eq!(tile.with_bitmap(|bitmap| bitmap.copied()), Some(7));

    assert_eq!(tile.take_bitmap(), Some(7));
    assert!(!tile.has_bitmap());
    assert_eq!(tile.spec(), tile_spec);
}

// ---------------------------------------------------------------------------------------------------------------------------
// RenderSurfaceOptions / constants (surface.rs)
// ---------------------------------------------------------------------------------------------------------------------------

// new: RenderSurfaceOptions() is (false, false, 1); the derived Default would give latency 0 (trap T4)
#[test]
fn render_surface_options_default_matches_the_csharp_record_defaults() {
    assert_eq!(
        RenderSurfaceOptions::default(),
        RenderSurfaceOptions {
            prefer_warp: false,
            allow_tearing: false,
            max_frame_latency: 1,
        }
    );

    let warp = RenderSurfaceOptions {
        prefer_warp: true,
        ..Default::default()
    };
    assert!(warp.prefer_warp);
    assert_eq!(warp.max_frame_latency, 1);
}

// new: the C# constant `float opacity = 1f` is the Rust const DEFAULT_IMAGE_OPACITY
#[test]
fn default_image_opacity_is_one() {
    assert_eq!(DEFAULT_IMAGE_OPACITY, 1.0_f32);
}

// new: the enum discriminants are the C# values and are not reordered (trap T6)
#[test]
fn present_result_and_text_trimming_values_match_csharp() {
    assert_eq!(PresentResult::Presented as i32, 0);
    assert_eq!(PresentResult::Occluded as i32, 1);
    assert_eq!(PresentResult::DeviceRecreated as i32, 2);
    assert_eq!(PresentResult::Failed as i32, 3);

    assert_eq!(TextTrimming::None as i32, 0);
    assert_eq!(TextTrimming::CharacterEllipsis as i32, 1);
}

// ---------------------------------------------------------------------------------------------------------------------------
// TextStyle (text.rs)
// ---------------------------------------------------------------------------------------------------------------------------

// new: TextStyle() is "Segoe UI", 12 DIP, not bold; the family is never empty (trap T5)
#[test]
fn text_style_default_is_segoe_ui_12_not_bold() {
    let style = TextStyle::default();
    assert_eq!(style.font_family, "Segoe UI");
    assert_eq!(style.font_size_dip, 12.0);
    assert!(!style.bold);
}

// new: TextStyle equality is C# record equality: ordinal family, double.Equals size (NaN equals NaN, 0.0 equals -0.0), bold
#[test]
fn text_style_equality_is_record_equality() {
    let style = |family: &str, size: f64, bold: bool| TextStyle {
        font_family: family.to_string(),
        font_size_dip: size,
        bold,
    };

    assert_eq!(style("Segoe UI", 12.0, false), style("Segoe UI", 12.0, false));
    assert_ne!(style("Segoe UI", 12.0, false), style("segoe ui", 12.0, false));
    assert_eq!(style("Segoe UI", f64::NAN, false), style("Segoe UI", f64::NAN, false));
    assert_eq!(style("Segoe UI", 0.0, false), style("Segoe UI", -0.0, false));
    assert_ne!(style("Segoe UI", 12.0, false), style("Segoe UI", 13.0, false));
    assert_ne!(style("Segoe UI", 12.0, false), style("Segoe UI", 12.0, true));
}

// ---------------------------------------------------------------------------------------------------------------------------
// RenderError (error.rs)
// ---------------------------------------------------------------------------------------------------------------------------

// new: the display texts are the C# messages verbatim (contract "Error texts"); a negative HRESULT prints as u32 hex
#[test]
fn render_error_display_texts() {
    assert_eq!(
        RenderError::Disposed {
            object: "PhotoReview.Shell.Rendering.GpuImageCache"
        }
        .to_string(),
        "Cannot access a disposed object.\r\nObject name: 'PhotoReview.Shell.Rendering.GpuImageCache'."
    );
    assert_eq!(
        RenderError::InvalidOperation {
            message: "EndDrawAndPresent without BeginDraw."
        }
        .to_string(),
        "EndDrawAndPresent without BeginDraw."
    );
    assert_eq!(
        RenderError::InvalidArgument {
            param: "layout",
            message: "The text layout was not created by the Direct2D text renderer.",
        }
        .to_string(),
        "The text layout was not created by the Direct2D text renderer. (Parameter 'layout')"
    );
    assert_eq!(
        RenderError::ArgumentOutOfRange { param: "value" }.to_string(),
        "Specified argument was out of the range of valid values. (Parameter 'value')"
    );
    assert_eq!(
        RenderError::Device {
            hresult: -2_005_270_523,
            operation: "Present",
        }
        .to_string(),
        "Present failed with HRESULT 0x887A0005"
    );
    assert_eq!(
        RenderError::Device {
            hresult: -1,
            operation: "EndDraw",
        }
        .to_string(),
        "EndDraw failed with HRESULT 0xFFFFFFFF"
    );
}

// new: RenderError is a std error (Box<dyn Error> conversion) and clones compare equal
#[test]
fn render_error_is_a_std_error_and_clone_eq() {
    let err = RenderError::ArgumentOutOfRange { param: "value" };
    assert_eq!(err.clone(), err);

    let boxed: Box<dyn Error> = Box::new(err.clone());
    assert_eq!(boxed.downcast_ref::<RenderError>(), Some(&err));
    assert!(!boxed.to_string().is_empty());
}

// ---------------------------------------------------------------------------------------------------------------------------
// Object safety and signatures (the Rust twin of the Architecture test)
// ---------------------------------------------------------------------------------------------------------------------------

/// Shared body of the two fake draw contexts: records the clip depth so that `pop_clip` is balanced, draws nothing.
macro_rules! fake_draw_body {
    () => {
        fn clear(&mut self, _color: ColorF) {}

        fn draw_image(
            &mut self,
            _image: &dyn GpuImage,
            _destination: RectD,
            _source_pixels: Option<RectD>,
            _interpolation: ImageInterpolation,
            _opacity: f32,
        ) {
        }

        fn fill_rectangle(&mut self, _rect: RectD, _color: ColorF) {}

        fn fill_rounded_rectangle(&mut self, _rect: RectD, _radius: f64, _color: ColorF) {}

        fn draw_rectangle(&mut self, _rect: RectD, _color: ColorF, _stroke_width: f64) {}

        fn draw_text(&mut self, _layout: &dyn TextLayout, _origin: PointD, _color: ColorF) -> Result<(), RenderError> {
            Ok(())
        }

        fn push_clip(&mut self, _rect: RectD) {
            self.clip_depth += 1;
        }

        fn pop_clip(&mut self) -> Result<(), RenderError> {
            if self.clip_depth == 0 {
                return Err(RenderError::InvalidOperation {
                    message: "PopClip without PushClip.",
                });
            }
            self.clip_depth -= 1;
            Ok(())
        }

        fn push_opacity(&mut self, _opacity: f32) {}

        fn pop_opacity(&mut self) -> Result<(), RenderError> {
            Ok(())
        }

        fn try_read_pixels(&mut self) -> Result<Option<PixelBuffer>, RenderError> {
            Ok(None)
        }
    };
}

/// A draw context that does not support oriented drawing: keeps the default `as_oriented()` (None).
struct FakeDraw {
    clip_depth: u32,
}

impl DrawContext for FakeDraw {
    fake_draw_body!();
}

/// A draw context that supports oriented drawing (the Rust form of `context is D2DDrawContext oriented`).
struct FakeOrientedDraw {
    clip_depth: u32,
}

impl DrawContext for FakeOrientedDraw {
    fake_draw_body!();

    fn as_oriented(&mut self) -> Option<&mut dyn OrientedDrawContext> {
        Some(self)
    }
}

impl OrientedDrawContext for FakeOrientedDraw {
    fn draw_image_oriented(
        &mut self,
        _image: &dyn GpuImage,
        _destination: RectD,
        _exif_orientation: i32,
        _source_pixels: Option<RectD>,
        _interpolation: ImageInterpolation,
        _opacity: f32,
    ) {
    }
}

struct FakeSurface {
    draw: FakeDraw,
    in_frame: bool,
    next_id: DeviceRecreatedHandlerId,
    handlers: Vec<(DeviceRecreatedHandlerId, DeviceRecreatedHandler)>,
}

impl FakeSurface {
    fn new() -> Self {
        Self {
            draw: FakeDraw { clip_depth: 0 },
            in_frame: false,
            next_id: 1,
            handlers: Vec::new(),
        }
    }
}

impl RenderSurface for FakeSurface {
    fn hwnd(&self) -> isize {
        0
    }

    fn pixel_width(&self) -> i32 {
        10
    }

    fn pixel_height(&self) -> i32 {
        10
    }

    fn dpi_scale(&self) -> f64 {
        1.0
    }

    fn frame_latency_wait_handle(&self) -> isize {
        0
    }

    fn is_warp(&self) -> bool {
        true
    }

    fn resize(&mut self, _pixel_width: i32, _pixel_height: i32, _dpi_scale: f64) -> Result<(), RenderError> {
        if self.in_frame {
            return Err(RenderError::InvalidOperation {
                message: "Resize is not allowed between BeginDraw and EndDrawAndPresent.",
            });
        }
        Ok(())
    }

    fn begin_draw(&mut self) -> Result<&mut dyn DrawContext, RenderError> {
        if self.in_frame {
            return Err(RenderError::InvalidOperation {
                message: "BeginDraw was already called; call EndDrawAndPresent first.",
            });
        }
        self.in_frame = true;
        Ok(&mut self.draw)
    }

    fn end_draw_and_present(&mut self) -> Result<PresentResult, RenderError> {
        if !self.in_frame {
            return Err(RenderError::InvalidOperation {
                message: "EndDrawAndPresent without BeginDraw.",
            });
        }
        self.in_frame = false;
        Ok(PresentResult::Presented)
    }

    fn subscribe_device_recreated(&mut self, handler: DeviceRecreatedHandler) -> DeviceRecreatedHandlerId {
        let id = self.next_id;
        self.next_id += 1;
        self.handlers.push((id, handler));
        id
    }

    fn unsubscribe_device_recreated(&mut self, id: DeviceRecreatedHandlerId) {
        self.handlers.retain(|(existing, _)| *existing != id);
    }
}

struct FakeFactory;

impl RenderSurfaceFactory for FakeFactory {
    fn create_for_window(
        &self,
        _hwnd: isize,
        _options: RenderSurfaceOptions,
    ) -> Result<Box<dyn RenderSurface>, RenderError> {
        Ok(Box::new(FakeSurface::new()))
    }

    fn create_offscreen(
        &self,
        _pixel_width: i32,
        _pixel_height: i32,
        _dpi_scale: f64,
    ) -> Result<Box<dyn RenderSurface>, RenderError> {
        Ok(Box::new(FakeSurface::new()))
    }
}

struct FakeImage {
    id: i64,
}

impl GpuImage for FakeImage {
    fn pixel_buffer_id(&self) -> i64 {
        self.id
    }

    fn pixel_width(&self) -> i32 {
        1
    }

    fn pixel_height(&self) -> i32 {
        1
    }

    fn gpu_bytes(&self) -> i64 {
        4
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct FakeCache {
    budget: Cell<i64>,
}

impl GpuImageCache for FakeCache {
    fn get_or_upload(&self, pixels: &PixelBuffer) -> Result<Rc<dyn GpuImage>, RenderError> {
        let image: Rc<dyn GpuImage> = Rc::new(FakeImage { id: pixels.id() });
        Ok(image)
    }

    fn prefetch(&self, _pixels: &Arc<PixelBuffer>) -> Result<(), RenderError> {
        Ok(())
    }

    fn retain(&self, _pixel_buffer_ids: &[i64]) -> Result<(), RenderError> {
        Ok(())
    }

    fn budget_bytes(&self) -> i64 {
        self.budget.get()
    }

    fn set_budget_bytes(&self, value: i64) -> Result<(), RenderError> {
        if value < 0 {
            return Err(RenderError::ArgumentOutOfRange { param: "value" });
        }
        self.budget.set(value);
        Ok(())
    }

    fn clear(&self) {}
}

struct FakeLayout {
    text: String,
}

impl TextLayout for FakeLayout {
    fn size(&self) -> SizeD {
        SizeD {
            width: 1.0,
            height: 1.0,
        }
    }

    fn text(&self) -> &str {
        &self.text
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct FakeText;

impl TextRenderer for FakeText {
    fn create_layout(
        &self,
        text: &str,
        _style: &TextStyle,
        _max_width: f64,
        _trimming: TextTrimming,
    ) -> Result<Box<dyn TextLayout>, RenderError> {
        Ok(Box::new(FakeLayout { text: text.to_string() }))
    }
}

// new: every contract trait compiles as a trait object and is used through the contract's borrow shapes
// (Box<dyn RenderSurface>, &mut dyn DrawContext, Rc<dyn GpuImage>, &dyn GpuImageCache, &dyn TextRenderer, Box<dyn TextLayout>,
// &dyn RenderSurfaceFactory). Twin of C# ContractSurfaceTests.Contracts_MatchApprovedSurface (documented skip above).
#[test]
fn contract_traits_are_object_safe_and_their_signatures_hold() {
    let factory: &dyn RenderSurfaceFactory = &FakeFactory;
    let mut surface: Box<dyn RenderSurface> = factory.create_offscreen(10, 10, 1.0).expect("offscreen surface");
    assert_eq!(surface.pixel_width(), 10);

    // one frame: begin_draw borrows the surface; a second begin_draw is InvalidOperation
    {
        let ctx: &mut dyn DrawContext = surface.begin_draw().expect("first begin_draw");
        ctx.clear(ColorF {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        ctx.push_clip(rect(0.0, 0.0, 1.0, 1.0));
        ctx.pop_clip().expect("balanced pop_clip");
        assert!(ctx.as_oriented().is_none(), "the default as_oriented is None");
    }
    assert!(matches!(
        surface.begin_draw(),
        Err(RenderError::InvalidOperation { .. })
    ));
    assert_eq!(
        surface.end_draw_and_present().expect("end the open frame"),
        PresentResult::Presented
    );
    assert!(
        matches!(
            surface.end_draw_and_present(),
            Err(RenderError::InvalidOperation { .. })
        ),
        "end without begin is InvalidOperation"
    );

    // DeviceRecreated subscriptions: distinct ids; an unknown id is a no-op
    let first = surface.subscribe_device_recreated(Box::new(|| {}));
    let second = surface.subscribe_device_recreated(Box::new(|| {}));
    assert_ne!(first, second);
    surface.unsubscribe_device_recreated(first);
    surface.unsubscribe_device_recreated(u64::MAX);

    // a draw context that supports oriented drawing returns Some from as_oriented
    let mut oriented = FakeOrientedDraw { clip_depth: 0 };
    let oriented_ctx: &mut dyn DrawContext = &mut oriented;
    let image = FakeImage { id: 9 };
    let oriented_target = oriented_ctx.as_oriented().expect("oriented draw context");
    oriented_target.draw_image_oriented(
        &image,
        rect(0.0, 0.0, 4.0, 4.0),
        6,
        None,
        ImageInterpolation::Linear,
        DEFAULT_IMAGE_OPACITY,
    );

    // the cache, shared through &dyn and Rc<dyn GpuImage>
    let cache: &dyn GpuImageCache = &FakeCache { budget: Cell::new(0) };
    let pixels = buffer(4, 4, PixelLayout::Pbgra32);
    let shared_pixels = Arc::new(buffer(4, 4, PixelLayout::Pbgra32));
    cache.prefetch(&shared_pixels).expect("prefetch");
    let uploaded: Rc<dyn GpuImage> = cache.get_or_upload(&pixels).expect("upload");
    assert_eq!(uploaded.pixel_buffer_id(), pixels.id());
    cache.set_budget_bytes(10).expect("budget");
    assert_eq!(cache.budget_bytes(), 10);
    assert!(cache.set_budget_bytes(-1).is_err());
    cache.retain(&[]).expect("retain");
    cache.clear();

    // the text renderer and a layout (Box<dyn TextLayout>); the default style comes from the stub
    let text: &dyn TextRenderer = &FakeText;
    let layout: Box<dyn TextLayout> = text
        .create_layout("hi", &TextStyle::default(), 100.0, TextTrimming::CharacterEllipsis)
        .expect("layout");
    assert_eq!(layout.text(), "hi");
    assert_eq!(layout.size().width, 1.0);

    // a frame that draws an image and a layout
    let ctx = surface.begin_draw().expect("second frame");
    ctx.draw_image(
        &*uploaded,
        rect(0.0, 0.0, 4.0, 4.0),
        None,
        ImageInterpolation::NearestNeighbor,
        DEFAULT_IMAGE_OPACITY,
    );
    ctx.draw_text(
        &*layout,
        PointD { x: 0.0, y: 0.0 },
        ColorF {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
    )
    .expect("draw_text with a layout of the renderer");
    assert_eq!(
        surface.end_draw_and_present().expect("end second frame"),
        PresentResult::Presented
    );
}

// new: the detailed ArgumentOutOfRange texts of the C# throw sites (GpuImageCache.BudgetBytes, DWriteTextRenderer.CreateLayout)
#[test]
fn argument_out_of_range_details_keep_the_csharp_messages() {
    assert_eq!(
        RenderError::NegativeBudget { value: -1 }.to_string(),
        "value ('-1') must be a non-negative value. (Parameter 'value')\r\nActual value was -1."
    );
    assert_eq!(
        RenderError::InvalidFontSize {
            value_bits: 0.0_f64.to_bits()
        }
        .to_string(),
        "FontSizeDip must be a finite positive number. (Parameter 'style')\r\nActual value was 0."
    );
    assert!(
        RenderError::InvalidFontSize {
            value_bits: (f64::NAN).to_bits()
        }
        .to_string()
        .ends_with("Actual value was NaN.")
    );
    assert!(
        RenderError::InvalidFontSize {
            value_bits: f64::NEG_INFINITY.to_bits()
        }
        .to_string()
        .ends_with("Actual value was -Infinity.")
    );
    assert!(
        RenderError::InvalidFontSize {
            value_bits: (f64::INFINITY).to_bits()
        }
        .to_string()
        .ends_with("Actual value was Infinity.")
    );
    assert!(
        RenderError::InvalidFontSize {
            value_bits: (-2.5_f64).to_bits()
        }
        .to_string()
        .ends_with("Actual value was -2.5.")
    );
}
