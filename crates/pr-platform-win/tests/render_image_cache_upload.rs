#![cfg(windows)]
//! Tests of W8-12g (`render::image_cache::upload`, `GpuImageCache.cs` lines 301-414 and the `pr_app` adapter), baseline `d92d6aae`.
//!
//! Pure tests run everywhere (hand-written values, a fake `BitmapDevice` without COM). Native tests are `#[ignore = "native: needs desktop"]`:
//! they use offscreen WARP surfaces (no window is shown), no sleeps; run them with `--include-ignored`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::services::render::RenderError;
use pr_app::services::render::gpu_image::{GpuImageCache, PrefetchStatus};
use pr_app::services::render::tiled::TiledGpuImage;
use pr_app::viewport::render_geometry::ImageInterpolation;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::gfx::com::GfxError;
use pr_platform_win::gfx::d2d;
use pr_platform_win::render::device::DeviceResources;
use pr_platform_win::render::draw_context::{ColorF, D2dDrawContext, RectD};
use pr_platform_win::render::image_cache::policy::{
    CachedImage, DEFAULT_STRIPE_BYTES, GpuImageBackend, StripeScheduler, StripeToken, default_budget_bytes,
};
use pr_platform_win::render::image_cache::upload::{
    BitmapDevice, D2dGpuImageBackend, D2dGpuImageCache, D3D11_MAX_TEXTURE_DIMENSION, DEFAULT_GUTTER,
    ResourcesBitmapDevice, StripeCopy, alpha_mode_for, effective_tile_limit, stripe_copy, tile_byte_offset,
};
use pr_platform_win::render::surface::{D2dRenderSurface, PresentResult};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED,
};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;

const E_FAIL: i32 = 0x8000_4005_u32 as i32;
const E_INVALIDARG: i32 = 0x8007_0057_u32 as i32;

// ---------------------------------------------------------------------------------------------------------------------
// Pure tests
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn effective_tile_limit_clamps_every_device_to_the_d3d11_limit() {
    assert_eq!(effective_tile_limit(8_388_608, None), 16384); // WARP
    assert_eq!(effective_tile_limit(16384, None), 16384);
    assert_eq!(effective_tile_limit(4096, None), 4096); // FL 9.3
    assert_eq!(effective_tile_limit(16384, Some(128)), 128);
    assert_eq!(effective_tile_limit(4096, Some(8192)), 4096);
    assert_eq!(effective_tile_limit(i32::MAX, Some(1)), 1);
    assert_eq!(D3D11_MAX_TEXTURE_DIMENSION, 16384);
    assert_eq!(DEFAULT_GUTTER, 64);
}

#[test]
fn constants_equal_the_pr_app_tile_layout_constants() {
    assert_eq!(
        D3D11_MAX_TEXTURE_DIMENSION,
        pr_app::viewport::render_geometry::D3D11_MAX_TEXTURE_DIMENSION
    );
    assert_eq!(DEFAULT_GUTTER, pr_app::viewport::render_geometry::DEFAULT_GUTTER);
}

#[test]
fn alpha_mode_ignores_the_x_byte_of_bgr32_only() {
    assert_eq!(alpha_mode_for(PixelLayout::Bgr32), D2D1_ALPHA_MODE_IGNORE);
    assert_eq!(alpha_mode_for(PixelLayout::Pbgra32), D2D1_ALPHA_MODE_PREMULTIPLIED);
}

#[test]
fn tile_byte_offset_is_row_times_stride_plus_four_per_pixel_in_i64() {
    assert_eq!(tile_byte_offset(40, 3, 2), 92);
    assert_eq!(tile_byte_offset(28, 0, 0), 0);
    assert_eq!(tile_byte_offset(24_000, 100, 3999), 95_976_400);
    assert_eq!(tile_byte_offset(80_000, 0, 30_000), 2_400_000_000);
    assert_eq!(tile_byte_offset(120_000, 29_999, 1_999), 239_880_000 + 119_996);
}

fn copy(source_top: i32, dest_top: u32, dest_bottom: u32, dest_right: u32) -> StripeCopy {
    StripeCopy {
        source_top,
        dest_top,
        dest_bottom,
        dest_right,
    }
}

#[test]
fn stripe_copy_clips_to_the_tile_rows() {
    // tile: texture_y 100, width 64, height 50
    let clip = |start, end| stripe_copy(100, 64, 50, start, end);
    assert_eq!(clip(0, 120), Some(copy(100, 0, 20, 64)));
    assert_eq!(clip(120, 200), Some(copy(120, 20, 50, 64)));
    assert_eq!(clip(110, 120), Some(copy(110, 10, 20, 64)));
    assert_eq!(clip(0, 1000), Some(copy(100, 0, 50, 64)));
    assert_eq!(clip(0, 100), None); // end equals the tile top
    assert_eq!(clip(150, 200), None); // start equals the tile bottom
    assert_eq!(clip(60, 60), None);
    assert_eq!(clip(130, 120), None); // start after end
}

#[test]
fn stripe_copy_writes_the_gutter_overlap_into_both_tiles() {
    // tile A rows 0..80, tile B rows 48..130 (gutter 16): a stripe inside the overlap lands in both
    let a = stripe_copy(0, 64, 80, 60, 70);
    let b = stripe_copy(48, 64, 82, 60, 70);
    assert_eq!(a, Some(copy(60, 60, 70, 64)));
    assert_eq!(b, Some(copy(60, 12, 22, 64)));
}

#[test]
fn stripe_copy_does_not_overflow_near_i32_max() {
    assert_eq!(
        stripe_copy(i32::MAX - 10, 4, 100, 0, i32::MAX),
        Some(copy(i32::MAX - 10, 0, 10, 4))
    );
}

// C#: GpuImageCacheTests.PixelBuffer_HasNoRowPadding_SoCacheMayAssumeStrideIsWidthTimesFour
#[test]
fn pixel_buffer_has_no_row_padding_so_the_backend_may_assume_stride_is_width_times_four() {
    let buffer = PixelBuffer::allocate(7, 3, PixelLayout::Pbgra32).unwrap();
    assert_eq!(buffer.stride(), 28);
}

type Handler = (u64, Rc<dyn Fn()>);

/// A device that never touches COM: every bitmap request fails with `E_FAIL`.
struct NoDevice {
    max: i32,
    create_calls: Cell<u32>,
    handlers: RefCell<Vec<Handler>>,
}

impl NoDevice {
    fn new(max: i32) -> Rc<Self> {
        Rc::new(Self {
            max,
            create_calls: Cell::new(0),
            handlers: RefCell::new(Vec::new()),
        })
    }
}

impl BitmapDevice for NoDevice {
    fn max_bitmap_size(&self) -> i32 {
        self.max
    }
    fn generation(&self) -> i64 {
        0
    }
    fn create_bitmap(&self, _w: u32, _h: u32, _alpha: D2D1_ALPHA_MODE) -> Result<ID2D1Bitmap1, GfxError> {
        self.create_calls.set(self.create_calls.get() + 1);
        Err(GfxError::new("CreateBitmap", E_FAIL))
    }
    fn release_bitmap(&self, _bitmap: ID2D1Bitmap1) {}
    fn subscribe_device_recreated(&self, handler: Box<dyn Fn()>) -> u64 {
        let id = self.handlers.borrow().len() as u64;
        self.handlers.borrow_mut().push((id, Rc::from(handler)));
        id
    }
    fn unsubscribe_device_recreated(&self, id: u64) {
        self.handlers.borrow_mut().retain(|(existing, _)| *existing != id);
    }
}

#[derive(Default)]
struct ManualScheduler {
    queue: RefCell<VecDeque<StripeToken>>,
}

impl StripeScheduler for ManualScheduler {
    fn schedule_stripe(&self, token: StripeToken) {
        self.queue.borrow_mut().push_back(token);
    }
}

impl ManualScheduler {
    /// C# `ManualDispatcher.RunAll`: runs queued stripes (also the ones they queue) and returns how many ran.
    fn run_all<D: BitmapDevice + 'static>(&self, cache: &D2dGpuImageCache<D>) -> usize {
        let mut count = 0;
        loop {
            let next = self.queue.borrow_mut().pop_front();
            let Some(token) = next else {
                return count;
            };
            cache.run_stripe(token).unwrap();
            count += 1;
        }
    }
}

#[test]
fn backend_new_keeps_its_options() {
    let backend = D2dGpuImageBackend::new(NoDevice::new(16384), Some(128), 16);
    assert_eq!((backend.max_tile_size(), backend.gutter()), (Some(128), 16));
    let backend = D2dGpuImageBackend::new(NoDevice::new(16384), None, DEFAULT_GUTTER);
    assert_eq!((backend.max_tile_size(), backend.gutter()), (None, 64));
    assert!(format!("{backend:?}").contains("D2dGpuImageBackend"));
}

// T10
#[test]
fn upload_of_a_disposed_buffer_is_an_error_not_a_panic() {
    let device = NoDevice::new(16384);
    let backend = D2dGpuImageBackend::new(Rc::clone(&device), None, 16);
    let mut buffer = PixelBuffer::allocate(4, 4, PixelLayout::Pbgra32).unwrap();
    buffer.dispose();
    let error = backend.upload(&buffer, true).unwrap_err();
    assert_eq!(error, GfxError::new("PixelBuffer::bytes", E_INVALIDARG));
    assert_eq!(device.create_calls.get(), 0);
}

#[test]
fn upload_with_an_invalid_tile_layout_is_an_invalid_argument_error() {
    let device = NoDevice::new(16384);
    let backend = D2dGpuImageBackend::new(Rc::clone(&device), None, -1); // negative gutter
    let buffer = PixelBuffer::allocate(4, 4, PixelLayout::Pbgra32).unwrap();
    assert_eq!(
        backend.upload(&buffer, true).unwrap_err(),
        GfxError::new("compute_tiles", E_INVALIDARG)
    );
    // a limit under 4 (device claims 3)
    let backend = D2dGpuImageBackend::new(NoDevice::new(3), None, 0);
    assert_eq!(
        backend.upload(&buffer, false).unwrap_err(),
        GfxError::new("compute_tiles", E_INVALIDARG)
    );
}

#[test]
fn upload_failure_of_the_first_tile_is_returned_with_the_injected_hresult() {
    let device = NoDevice::new(16384);
    let backend = D2dGpuImageBackend::new(Rc::clone(&device), None, 16);
    let buffer = PixelBuffer::allocate(4, 4, PixelLayout::Pbgra32).unwrap();
    assert_eq!(
        backend.upload(&buffer, true).unwrap_err(),
        GfxError::new("CreateBitmap", E_FAIL)
    );
    assert_eq!(device.create_calls.get(), 1);
}

#[test]
fn tiled_gpu_image_is_a_cached_image_with_the_trait_semantics() {
    let buffer = PixelBuffer::allocate(10, 10, PixelLayout::Pbgra32).unwrap();
    let image = TiledGpuImage::<()>::new(&buffer, Vec::new(), 0);
    assert_eq!(CachedImage::pixel_buffer_id(&image), buffer.id());
    assert_eq!(CachedImage::pixel_height(&image), 10);
    assert_eq!(CachedImage::gpu_bytes(&image), 0);
    assert!(!CachedImage::is_complete(&image));
    CachedImage::set_uploaded_rows(&image, 10);
    assert_eq!(CachedImage::uploaded_rows(&image), 10);
    assert!(CachedImage::is_complete(&image));
    assert!(!CachedImage::is_released(&image));
    CachedImage::set_released(&image, true);
    assert!(CachedImage::is_released(&image));
}

#[test]
fn cache_construction_maps_policy_errors_to_render_errors() {
    let sched: Rc<dyn StripeScheduler> = Rc::new(ManualScheduler::default());
    let negative =
        D2dGpuImageCache::with_options(NoDevice::new(16384), Rc::clone(&sched), -5, 4, None, 16).unwrap_err();
    assert_eq!(negative, RenderError::NegativeBudget { value: -5 });
    let small = D2dGpuImageCache::with_options(NoDevice::new(16384), Rc::clone(&sched), 0, 3, None, 16).unwrap_err();
    assert_eq!(small, RenderError::ArgumentOutOfRange { param: "stripeBytes" });
}

// C#: GpuImageCacheTests.Cache_DefaultBudget_IsSixScreensOfAtLeastFullHd (composition passes the PIXEL size)
#[test]
fn default_budget_follows_the_surface_pixel_size_through_the_adapter() {
    let sched: Rc<dyn StripeScheduler> = Rc::new(ManualScheduler::default());
    let small = D2dGpuImageCache::new(NoDevice::new(16384), Rc::clone(&sched), 64, 64).unwrap();
    let large = D2dGpuImageCache::new(NoDevice::new(16384), Rc::clone(&sched), 2560, 1440).unwrap();
    assert_eq!(small.budget_bytes(), 6 * 1920 * 1080 * 4);
    assert_eq!(large.budget_bytes(), 6 * 2560 * 1440 * 4);
    assert_eq!(small.budget_bytes(), default_budget_bytes(64, 64));
}

#[test]
fn adapter_forwards_budget_retain_clear_and_maps_errors() {
    let sched: Rc<dyn StripeScheduler> = Rc::new(ManualScheduler::default());
    let cache =
        D2dGpuImageCache::with_options(NoDevice::new(16384), sched, 1000, DEFAULT_STRIPE_BYTES, None, 16).unwrap();
    assert_eq!(cache.budget_bytes(), 1000);
    cache.set_budget_bytes(2000).unwrap();
    assert_eq!(cache.budget_bytes(), 2000);
    assert_eq!(
        cache.set_budget_bytes(-1).unwrap_err(),
        RenderError::NegativeBudget { value: -1 }
    );
    assert_eq!(cache.budget_bytes(), 2000);
    cache.retain(&[1, 2, 3]).unwrap();
    cache.clear();
    assert!(!cache.is_fully_uploaded(1));
    let id = cache.subscribe_upload_completed(Rc::new(|_| {}));
    cache.unsubscribe_upload_completed(id);
    // a failing backend surfaces as RenderError::Device field for field, the cache stays empty
    let buffer = PixelBuffer::allocate(4, 4, PixelLayout::Pbgra32).unwrap();
    assert_eq!(
        cache.get_or_upload(&buffer).err().unwrap(),
        RenderError::Device {
            hresult: E_FAIL,
            operation: "CreateBitmap"
        }
    );
    assert_eq!(cache.inner().count(), 0);
}

// ---------------------------------------------------------------------------------------------------------------------
// Native helpers (copies of the RenderTestImages helpers of the C# tests)
// ---------------------------------------------------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn byte(&mut self) -> u8 {
        (self.next() >> 32) as u8
    }
    fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n
    }
}

fn to_byte(value: f64) -> u8 {
    value.round_ties_even().clamp(0.0, 255.0) as u8
}

/// C# `CreateNoise`: seeded noise, valid premultiplied Pbgra32, a quarter of the pixels partly transparent.
fn create_noise(width: i32, height: i32, seed: u64) -> PixelBuffer {
    let mut rng = Rng::new(seed);
    let mut buffer = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
    for y in 0..height {
        for pixel in buffer.get_row_mut(y).unwrap().chunks_exact_mut(4) {
            let alpha = if rng.below(4) == 0 { rng.byte() } else { 255 };
            for channel in &mut pixel[..3] {
                *channel = (u32::from(rng.byte()) * u32::from(alpha) / 255) as u8;
            }
            pixel[3] = alpha;
        }
    }
    buffer
}

/// C# `CreateSmooth` (Pbgra32, alpha 255).
fn create_smooth(width: i32, height: i32) -> PixelBuffer {
    let mut buffer = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
    for y in 0..height {
        let row = buffer.get_row_mut(y).unwrap();
        for x in 0..width {
            let fx = f64::from(x) / f64::from(width);
            let fy = f64::from(y) / f64::from(height);
            let mut r = 128.0 + 100.0 * (f64::from(x) / 37.0 + f64::from(y) / 53.0).sin();
            let mut g = 30.0 + 190.0 * fx * (1.0 - 0.5 * fy);
            let mut b = 128.0 + 90.0 * (f64::from(x) / 71.0 - f64::from(y) / 29.0).cos();
            if fx > 0.3 && fx < 0.45 && fy > 0.2 && fy < 0.7 {
                r = 240.0;
                g = 230.0;
                b = 20.0;
            }
            let i = x as usize * 4;
            row[i] = to_byte(b);
            row[i + 1] = to_byte(g);
            row[i + 2] = to_byte(r);
            row[i + 3] = 255;
        }
    }
    buffer
}

fn crop(source: &PixelBuffer, x: i32, y: i32, width: i32, height: i32) -> PixelBuffer {
    let mut result = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
    for row in 0..height {
        let from = &source.get_row(y + row).unwrap()[x as usize * 4..(x + width) as usize * 4];
        result.get_row_mut(row).unwrap().copy_from_slice(from);
    }
    result
}

/// C# `ResizeArea`: area-weighted (box) reference for downscaling.
fn resize_area(source: &PixelBuffer, width: i32, height: i32) -> PixelBuffer {
    let (sw, sh) = (source.width() as usize, source.height() as usize);
    let (w, h) = (width as usize, height as usize);
    let mut horizontal = vec![0.0_f64; w * sh * 4];
    let sx = sw as f64 / w as f64;
    for y in 0..sh {
        let row = source.get_row(y as i32).unwrap();
        for dx in 0..w {
            let start = dx as f64 * sx;
            let end = start + sx;
            let mut acc = [0.0_f64; 4];
            for x in (start as usize)..sw.min(end.ceil() as usize) {
                let weight = end.min(x as f64 + 1.0) - start.max(x as f64);
                for (c, value) in acc.iter_mut().enumerate() {
                    *value += f64::from(row[x * 4 + c]) * weight;
                }
            }
            for c in 0..4 {
                horizontal[(y * w + dx) * 4 + c] = acc[c] / sx;
            }
        }
    }
    let mut result = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap();
    let sy = sh as f64 / h as f64;
    for dy in 0..h {
        let start = dy as f64 * sy;
        let end = start + sy;
        let target = result.get_row_mut(dy as i32).unwrap();
        for dx in 0..w {
            for c in 0..4 {
                let mut sum = 0.0;
                for y in (start as usize)..sh.min(end.ceil() as usize) {
                    let weight = end.min(y as f64 + 1.0) - start.max(y as f64);
                    sum += horizontal[(y * w + dx) * 4 + c] * weight;
                }
                target[dx * 4 + c] = to_byte(sum / sy);
            }
        }
    }
    result
}

/// C# `Psnr` over B, G, R, A (peak 255); identical gives +infinity.
fn psnr(expected: &PixelBuffer, actual: &PixelBuffer) -> f64 {
    assert_eq!((expected.width(), expected.height()), (actual.width(), actual.height()));
    let mut squares = 0.0_f64;
    let mut count = 0_u64;
    for y in 0..expected.height() {
        for (e, a) in expected
            .get_row(y)
            .unwrap()
            .iter()
            .zip(actual.get_row(y).unwrap().iter())
        {
            let d = f64::from(*e) - f64::from(*a);
            squares += d * d;
            count += 1;
        }
    }
    if squares == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0 * 255.0 / (squares / count as f64)).log10()
    }
}

fn assert_identical(expected: &PixelBuffer, actual: &PixelBuffer) {
    assert_eq!((expected.width(), expected.height()), (actual.width(), actual.height()));
    for y in 0..expected.height() {
        let e = expected.get_row(y).unwrap();
        let a = actual.get_row(y).unwrap();
        if let Some(i) = e.iter().zip(a).position(|(x, y)| x != y) {
            panic!(
                "pixel ({}, {y}) channel {}: expected {}, got {}",
                i / 4,
                i % 4,
                e[i],
                a[i]
            );
        }
    }
}

fn pixel(buffer: &PixelBuffer, x: i32, y: i32) -> (u8, u8, u8, u8) {
    let row = buffer.get_row(y).unwrap();
    let i = x as usize * 4;
    (row[i], row[i + 1], row[i + 2], row[i + 3])
}

/// C# `RenderTestImages.Render`: one frame, read back, must present.
fn render(surface: &mut D2dRenderSurface, draw: impl FnOnce(&mut D2dDrawContext)) -> PixelBuffer {
    let context = surface.begin_draw_context().unwrap();
    draw(context);
    let pixels = context
        .try_read_pixels()
        .unwrap()
        .expect("offscreen surfaces read back");
    assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Presented);
    pixels
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

const TRANSPARENT: ColorF = ColorF {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

fn offscreen(width: i32, height: i32) -> D2dRenderSurface {
    D2dRenderSurface::create_offscreen_surface(width, height, 1.0, false).unwrap()
}

type Cache = D2dGpuImageCache<ResourcesBitmapDevice>;

struct Rig {
    surface: D2dRenderSurface,
    device: Rc<ResourcesBitmapDevice>,
    scheduler: Rc<ManualScheduler>,
}

impl Rig {
    fn new(width: i32, height: i32) -> Self {
        let mut surface = offscreen(width, height);
        let device = ResourcesBitmapDevice::new(Rc::clone(surface.resources()));
        device.attach_to_surface(&mut surface);
        Self {
            surface,
            device,
            scheduler: Rc::new(ManualScheduler::default()),
        }
    }

    fn resources(&self) -> Rc<DeviceResources> {
        Rc::clone(self.surface.resources())
    }

    /// C# `CacheFor(surface, maxTile, gutter)`: unlimited budget, default stripe size.
    fn cache(&self, max_tile: Option<i32>, gutter: i32) -> Cache {
        self.cache_with(i64::MAX, DEFAULT_STRIPE_BYTES, max_tile, gutter)
    }

    fn cache_with(&self, budget: i64, stripe_bytes: i32, max_tile: Option<i32>, gutter: i32) -> Cache {
        let scheduler: Rc<dyn StripeScheduler> = Rc::clone(&self.scheduler) as Rc<dyn StripeScheduler>;
        D2dGpuImageCache::with_options(
            Rc::clone(&self.device),
            scheduler,
            budget,
            stripe_bytes,
            max_tile,
            gutter,
        )
        .unwrap()
    }
}

/// The rows of `source` that a tile bitmap must hold: the tile's texture rectangle, gutter included, tight rows.
fn expected_tile_bytes(source: &PixelBuffer, x: i32, y: i32, width: i32, height: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for row in 0..height {
        let line = source.get_row(y + row).unwrap();
        bytes.extend_from_slice(&line[x as usize * 4..(x + width) as usize * 4]);
    }
    bytes
}

fn assert_tiles_equal_source(resources: &DeviceResources, image: &TiledGpuImage<ID2D1Bitmap1>, source: &PixelBuffer) {
    let context = resources.context().unwrap();
    for (index, tile) in image.tiles().iter().enumerate() {
        let texture = tile.spec().texture;
        let actual = tile.with_bitmap(|bitmap| {
            d2d::read_pixels(&context, bitmap.unwrap(), texture.width as u32, texture.height as u32).unwrap()
        });
        let expected = expected_tile_bytes(source, texture.x, texture.y, texture.width, texture.height);
        assert!(
            actual == expected,
            "tile {index} differs from the source rectangle {texture:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Native tests: upload only
// ---------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn upload_with_data_round_trips_pixels() {
    let rig = Rig::new(8, 8);
    let backend = D2dGpuImageBackend::new(Rc::clone(&rig.device), Some(128), 16);
    let source = create_noise(300, 200, 5);
    let baseline = rig.resources().live_bitmap_count();

    let image = backend.upload(&source, true).unwrap();

    assert!(
        image.tiles().len() > 9,
        "test must actually tile: {}",
        image.tiles().len()
    );
    assert_eq!(image.uploaded_rows(), 0);
    assert_eq!(image.generation(), rig.resources().generation());
    assert_eq!(
        rig.resources().live_bitmap_count(),
        baseline + image.tiles().len() as i64
    );
    assert_tiles_equal_source(&rig.resources(), &image, &source);
    backend.release(&image);
    assert_eq!(rig.resources().live_bitmap_count(), baseline);
}

// T3 / T4: a single tile image (the whole picture) and right-hand tiles whose rows continue past the tile
#[test]
#[ignore = "native: needs desktop"]
fn upload_without_forced_tiles_is_one_tile_and_round_trips() {
    let rig = Rig::new(8, 8);
    let backend = D2dGpuImageBackend::new(Rc::clone(&rig.device), None, DEFAULT_GUTTER);
    let source = create_noise(37, 23, 8);
    let image = backend.upload(&source, true).unwrap();
    assert_eq!(image.tiles().len(), 1);
    assert_tiles_equal_source(&rig.resources(), &image, &source);
    backend.release(&image);
}

// C#: D2DWindowSurfaceTests.Prefetch_StripedUpload_OnRealDevice_DrawsIdenticalPixels (data half, without drawing)
#[test]
#[ignore = "native: needs desktop"]
fn striped_upload_matches_a_synchronous_upload() {
    let rig = Rig::new(8, 8);
    let backend = D2dGpuImageBackend::new(Rc::clone(&rig.device), Some(128), 16);
    let source = create_noise(300, 200, 21);
    let synchronous = backend.upload(&source, true).unwrap();
    let striped = backend.upload(&source, false).unwrap();

    let mut stripes = 0;
    let mut start = 0;
    while start < 200 {
        let end = (start + 7).min(200);
        backend.upload_rows(&striped, &source, start, end).unwrap();
        stripes += 1;
        start = end;
    }

    assert_eq!(stripes, (200 + 6) / 7);
    assert_tiles_equal_source(&rig.resources(), &striped, &source);
    assert_tiles_equal_source(&rig.resources(), &synchronous, &source);
    backend.release(&striped);
    backend.release(&synchronous);
}

#[test]
#[ignore = "native: needs desktop"]
fn upload_rows_skips_a_released_image_and_empty_ranges() {
    let rig = Rig::new(8, 8);
    let backend = D2dGpuImageBackend::new(Rc::clone(&rig.device), Some(128), 16);
    let source = create_noise(300, 200, 3);
    let image = backend.upload(&source, false).unwrap();
    backend.upload_rows(&image, &source, 50, 50).unwrap(); // empty range: nothing
    backend.release(&image);
    backend.release(&image); // a second release releases nothing
    assert_eq!(rig.resources().live_bitmap_count(), 0);
    backend.upload_rows(&image, &source, 0, 200).unwrap(); // tiles without bitmap are skipped
    assert!(image.tiles().iter().all(|tile| !tile.has_bitmap()));
}

#[test]
#[ignore = "native: needs desktop"]
fn upload_rows_with_a_disposed_buffer_is_an_error() {
    let rig = Rig::new(8, 8);
    let backend = D2dGpuImageBackend::new(Rc::clone(&rig.device), None, 16);
    let source = create_noise(8, 8, 1);
    let image = backend.upload(&source, false).unwrap();
    let mut other = create_noise(8, 8, 2);
    other.dispose();
    assert_eq!(
        backend.upload_rows(&image, &other, 0, 8).unwrap_err(),
        GfxError::new("PixelBuffer::bytes", E_INVALIDARG)
    );
    backend.release(&image);
}

#[test]
#[ignore = "native: needs desktop"]
fn bgr32_upload_uses_ignore_alpha_and_pbgra32_premultiplied() {
    let rig = Rig::new(8, 8);
    let backend = D2dGpuImageBackend::new(Rc::clone(&rig.device), None, 16);
    let bgr = PixelBuffer::allocate(4, 4, PixelLayout::Bgr32).unwrap();
    let pbgra = PixelBuffer::allocate(4, 4, PixelLayout::Pbgra32).unwrap();
    for (buffer, expected) in [(&bgr, D2D1_ALPHA_MODE_IGNORE), (&pbgra, D2D1_ALPHA_MODE_PREMULTIPLIED)] {
        let image = backend.upload(buffer, true).unwrap();
        let alpha = image.tiles()[0].with_bitmap(|bitmap| {
            // SAFETY: the bitmap is a live owned interface borrowed for the call; `GetPixelFormat` takes no pointers; UI thread.
            unsafe { bitmap.unwrap().GetPixelFormat() }.alphaMode
        });
        assert_eq!(alpha, expected);
        backend.release(&image);
    }
}

// T1 (C#: LiveBitmapCount leak probe): a failure at the 3rd tile releases tiles 0 and 1 through release_bitmap
#[test]
#[ignore = "native: needs desktop"]
fn failed_upload_releases_the_bitmaps_created_so_far() {
    struct FailingDevice {
        inner: Rc<ResourcesBitmapDevice>,
        fail_at: u32,
        calls: Cell<u32>,
    }
    impl BitmapDevice for FailingDevice {
        fn max_bitmap_size(&self) -> i32 {
            self.inner.max_bitmap_size()
        }
        fn generation(&self) -> i64 {
            self.inner.generation()
        }
        fn create_bitmap(&self, w: u32, h: u32, alpha: D2D1_ALPHA_MODE) -> Result<ID2D1Bitmap1, GfxError> {
            self.calls.set(self.calls.get() + 1);
            if self.calls.get() == self.fail_at {
                return Err(GfxError::new("CreateBitmap", 0x887A_0005_u32 as i32));
            }
            self.inner.create_bitmap(w, h, alpha)
        }
        fn release_bitmap(&self, bitmap: ID2D1Bitmap1) {
            self.inner.release_bitmap(bitmap);
        }
        fn subscribe_device_recreated(&self, handler: Box<dyn Fn()>) -> u64 {
            self.inner.subscribe_device_recreated(handler)
        }
        fn unsubscribe_device_recreated(&self, id: u64) {
            self.inner.unsubscribe_device_recreated(id);
        }
    }

    let rig = Rig::new(8, 8);
    let baseline = rig.resources().live_bitmap_count();
    let source = create_noise(300, 200, 4);
    for with_data in [true, false] {
        let device = Rc::new(FailingDevice {
            inner: Rc::clone(&rig.device),
            fail_at: 3,
            calls: Cell::new(0),
        });
        let backend = D2dGpuImageBackend::new(Rc::clone(&device), Some(128), 16);
        let error = backend.upload(&source, with_data).unwrap_err();
        assert_eq!(error, GfxError::new("CreateBitmap", 0x887A_0005_u32 as i32));
        assert_eq!(device.calls.get(), 3);
        assert_eq!(rig.resources().live_bitmap_count(), baseline, "with_data = {with_data}");
    }
}

// T8
#[test]
#[ignore = "native: needs desktop"]
fn release_after_device_recreation_does_not_touch_the_new_device() {
    let mut rig = Rig::new(8, 8);
    let backend = D2dGpuImageBackend::new(Rc::clone(&rig.device), Some(128), 16);
    let source = create_noise(300, 200, 6);
    let baseline = rig.resources().live_bitmap_count();
    let image = backend.upload(&source, true).unwrap();
    let tiles = image.tiles().len() as i64;
    assert_eq!(rig.resources().live_bitmap_count(), baseline + tiles);

    rig.surface.recreate_device().unwrap();
    assert!(!image.is_drawable(rig.resources().generation()));
    backend.release(&image);

    assert_eq!(rig.resources().live_bitmap_count(), baseline);
    assert!(image.tiles().iter().all(|tile| !tile.has_bitmap()));
}

// ---------------------------------------------------------------------------------------------------------------------
// Native tests: the whole cache
// ---------------------------------------------------------------------------------------------------------------------

// C#: GpuImageCacheTests / D2DWindowSurfaceTests.Cache_ThousandUploadsAndEvictions_DoNotLeakBitmaps_AndStayInBudget
#[test]
#[ignore = "native: needs desktop"]
fn thousand_uploads_and_evictions_do_not_leak_bitmaps_and_stay_in_budget() {
    let rig = Rig::new(64, 64);
    let budget = 5 * 64 * 64 * 4;
    let cache = rig.cache_with(budget, DEFAULT_STRIPE_BYTES, None, DEFAULT_GUTTER);
    let baseline = rig.resources().live_bitmap_count();

    for i in 0..1000 {
        let px = Arc::new(PixelBuffer::allocate(64, 64, PixelLayout::Pbgra32).unwrap());
        if i % 2 == 0 {
            cache.prefetch(&px).unwrap();
            rig.scheduler.run_all(&cache);
        }
        cache.get_or_upload(&px).unwrap();
        assert!(cache.inner().total_gpu_bytes() <= cache.budget_bytes());
    }

    assert_eq!(cache.inner().count(), 5);
    assert_eq!(rig.resources().live_bitmap_count(), baseline + 5);
    cache.clear();
    assert_eq!(rig.resources().live_bitmap_count(), baseline);
    assert_eq!(cache.inner().total_gpu_bytes(), 0);
}

// C#: GpuImageCacheTests / D2DWindowSurfaceTests.Cache_DefaultBudget_IsSixScreensOfAtLeastFullHd (real surfaces)
#[test]
#[ignore = "native: needs desktop"]
fn default_budget_follows_the_surface_pixel_size() {
    for (width, height, expected) in [(64, 64, 6 * 1920 * 1080 * 4_i64), (2560, 1440, 6 * 2560 * 1440 * 4_i64)] {
        let mut surface = offscreen(width, height);
        let device = ResourcesBitmapDevice::new(Rc::clone(surface.resources()));
        device.attach_to_surface(&mut surface);
        let scheduler: Rc<dyn StripeScheduler> = Rc::new(ManualScheduler::default());
        let cache = D2dGpuImageCache::new(device, scheduler, surface.pixel_width(), surface.pixel_height()).unwrap();
        assert_eq!(cache.budget_bytes(), expected);
    }
}

// C#: D2DWindowSurfaceTests.Prefetch_StripedUpload_OnRealDevice_DrawsIdenticalPixels
#[test]
#[ignore = "native: needs desktop"]
fn prefetch_striped_upload_on_a_real_device_draws_identical_pixels() {
    let mut rig = Rig::new(300, 200);
    let cache = rig.cache_with(i64::MAX, 300 * 4 * 7, Some(128), 16); // 7 rows per stripe, crossing tile borders
    let source = Arc::new(create_noise(300, 200, 21));

    cache.prefetch(&source).unwrap();
    let stripes = rig.scheduler.run_all(&cache);

    assert_eq!(stripes, 200_usize.div_ceil(7));
    assert!(cache.is_fully_uploaded(source.id()));
    let image = cache.get_or_upload(&source).unwrap();
    let pixels = render(&mut rig.surface, |dc| {
        dc.clear(TRANSPARENT);
        dc.draw_image(
            image.as_any(),
            rect(0.0, 0.0, 300.0, 200.0),
            None,
            ImageInterpolation::HighQualityCubic,
            1.0,
        );
    });
    assert_identical(&source, &pixels);
}

#[test]
#[ignore = "native: needs desktop"]
fn upload_completed_is_raised_once_per_prefetch_after_the_last_stripe() {
    let rig = Rig::new(8, 8);
    let cache = rig.cache_with(i64::MAX, 40 * 3, None, DEFAULT_GUTTER); // 3 rows per stripe
    let source = Arc::new(create_noise(10, 10, 2));
    let completed = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&completed);
    let subscription = cache.subscribe_upload_completed(Rc::new(move |id| log.borrow_mut().push(id)));

    cache.prefetch(&source).unwrap();
    assert!(!cache.is_fully_uploaded(source.id()));
    assert_eq!(rig.scheduler.run_all(&cache), 4); // 3 + 3 + 3 + 1 rows
    assert_eq!(*completed.borrow(), vec![source.id()]);
    assert!(cache.is_fully_uploaded(source.id()));

    cache.unsubscribe_upload_completed(subscription);
    let second = Arc::new(create_noise(10, 10, 3));
    cache.prefetch(&second).unwrap();
    rig.scheduler.run_all(&cache);
    assert_eq!(completed.borrow().len(), 1);
}

// C#: GpuImageCacheTests.Cache_Bgr32_IgnoresUnusedByte
#[test]
#[ignore = "native: needs desktop"]
fn cache_bgr32_ignores_the_unused_byte() {
    let mut rig = Rig::new(4, 4);
    let cache = rig.cache(None, DEFAULT_GUTTER);
    let mut source = PixelBuffer::allocate(4, 4, PixelLayout::Bgr32).unwrap();
    for y in 0..4 {
        for pixel in source.get_row_mut(y).unwrap().chunks_exact_mut(4) {
            pixel.copy_from_slice(&[10, 20, 30, 0]); // X = 0, not alpha
        }
    }
    let image = cache.get_or_upload(&source).unwrap();
    let pixels = render(&mut rig.surface, |dc| {
        dc.clear(TRANSPARENT);
        dc.draw_image(
            image.as_any(),
            rect(0.0, 0.0, 4.0, 4.0),
            None,
            ImageInterpolation::Linear,
            1.0,
        );
    });
    assert_eq!(pixel(&pixels, 3, 3), (10, 20, 30, 255));
}

// C#: D2DRenderSurfaceTests.TiledImage_ForcedSmallTiles_MatchesSingleTexture (the theory rows; at scale 1.0 the result must be
// byte-identical, otherwise PSNR >= 45 dB; the contract counts 8 rows, the C# has 7, the 8th is HighQualityCubic x1.0)
#[test]
#[ignore = "native: needs desktop"]
fn tiled_image_forced_small_tiles_matches_single_texture() {
    let source = create_smooth(500, 300);
    let rows = [
        (ImageInterpolation::NearestNeighbor, 1.0_f64),
        (ImageInterpolation::Linear, 1.0),
        (ImageInterpolation::Linear, 0.5),
        (ImageInterpolation::Linear, 1.7),
        (ImageInterpolation::HighQualityCubic, 0.37),
        (ImageInterpolation::HighQualityCubic, 0.5),
        (ImageInterpolation::HighQualityCubic, 1.0),
        (ImageInterpolation::HighQualityCubic, 1.7),
    ];
    for (interpolation, scale) in rows {
        let width = (500.0 * scale).round_ties_even() as i32;
        let height = (300.0 * scale).round_ties_even() as i32;
        let mut rig = Rig::new(width, height);
        let whole = rig.cache(None, DEFAULT_GUTTER);
        let tiled = rig.cache(Some(128), 16);
        let single = whole.get_or_upload_tiled(&source).unwrap();
        let tiles = tiled.get_or_upload_tiled(&source).unwrap();
        assert!(tiles.tiles().len() > 9, "test must actually tile");
        assert_eq!(single.tiles().len(), 1);

        let dest = rect(0.0, 0.0, f64::from(width), f64::from(height));
        let expected = render(&mut rig.surface, |dc| {
            dc.draw_image(&*single, dest, None, interpolation, 1.0)
        });
        let actual = render(&mut rig.surface, |dc| {
            dc.draw_image(&*tiles, dest, None, interpolation, 1.0)
        });

        let value = psnr(&expected, &actual);
        println!("PSNR tiled vs single {interpolation:?} x{scale}: {value:.2} dB");
        if scale == 1.0 {
            assert_identical(&expected, &actual);
        } else {
            assert!(
                value >= 45.0,
                "PSNR {value:.2} dB: tile seams visible ({interpolation:?} x{scale})"
            );
        }
    }
}

// C#: D2DRenderSurfaceTests.TiledImage_Panorama30000x2000_SplitsAtDeviceLimit_SeamExactAtOneToOne_FitMatchesReference
#[test]
#[ignore = "native: needs desktop (slow: 240 MB panorama)"]
fn tiled_image_panorama_30000x2000_splits_at_device_limit_seam_exact_at_one_to_one() {
    let source = create_smooth(30000, 2000);
    let mut rig = Rig::new(1500, 100);
    let cache = rig.cache(None, DEFAULT_GUTTER);
    let image = cache.get_or_upload_tiled(&source).unwrap();
    println!("max bitmap size {}", rig.resources().max_bitmap_size());
    assert_eq!(image.tiles().len(), 2);
    let core = image.tiles()[0].spec().core;
    let seam = core.x + core.width;
    assert!((16000..=16384).contains(&seam), "seam {seam}");

    // 1:1 across the seam: byte identical
    let one_to_one = render(&mut rig.surface, |dc| {
        dc.clear(TRANSPARENT);
        dc.draw_image(
            &*image,
            rect(0.0, 0.0, 256.0, 100.0),
            Some(rect(f64::from(seam - 128), 900.0, 256.0, 100.0)),
            ImageInterpolation::HighQualityCubic,
            1.0,
        );
    });
    assert_identical(
        &crop(&source, seam - 128, 900, 256, 100),
        &crop(&one_to_one, 0, 0, 256, 100),
    );

    // fit (x0.05) of the whole image against the area reference
    let fit = render(&mut rig.surface, |dc| {
        dc.draw_image(
            &*image,
            rect(0.0, 0.0, 1500.0, 100.0),
            None,
            ImageInterpolation::HighQualityCubic,
            1.0,
        );
    });
    let reference = resize_area(&source, 1500, 100);
    let value = psnr(&reference, &fit);
    println!("panorama 30000x2000 fit HQC PSNR {value:.2} dB");
    assert!(value >= 40.0, "PSNR {value:.2} dB");

    // around the seam (destination column seam * 0.05 +/- 4)
    let seam_column = (f64::from(seam) * 0.05).round_ties_even() as i32;
    let seam_value = psnr(
        &crop(&reference, seam_column - 4, 0, 8, 100),
        &crop(&fit, seam_column - 4, 0, 8, 100),
    );
    println!("panorama seam columns PSNR {seam_value:.2} dB");
    assert!(seam_value >= 40.0, "seam PSNR {seam_value:.2} dB");
}

// C#: D2DRenderSurfaceTests.DeviceLost_RecreateTarget_RecreatesDevice_EmptiesCache_NextFrameDraws (cache half; the EndDraw fault seam is
// pub(crate), so the loss is raised with `recreate_device`, which runs the same handlers)
#[test]
#[ignore = "native: needs desktop"]
fn device_lost_recreate_empties_the_cache_and_the_next_frame_draws() {
    let mut rig = Rig::new(16, 16);
    let cache = rig.cache(None, DEFAULT_GUTTER);
    let source = create_noise(16, 16, 9);
    let before = cache.get_or_upload_tiled(&source).unwrap();
    assert_eq!(rig.resources().live_bitmap_count(), 1);
    let recreated = Rc::new(Cell::new(0));
    let seen = Rc::clone(&recreated);
    rig.surface
        .subscribe_device_recreated(Box::new(move || seen.set(seen.get() + 1)));

    rig.surface.recreate_device().unwrap();

    assert_eq!(recreated.get(), 1);
    assert_eq!(rig.surface.device_generation(), 1);
    assert_eq!(cache.inner().count(), 0);
    assert_eq!(rig.resources().live_bitmap_count(), 0);
    assert!(before.is_released());

    // the old image is skipped, not drawn; uploading again draws
    let pixels = render(&mut rig.surface, |dc| {
        dc.clear(TRANSPARENT);
        dc.draw_image(
            &*before,
            rect(0.0, 0.0, 16.0, 16.0),
            None,
            ImageInterpolation::Linear,
            1.0,
        );
        assert_eq!(dc.skipped_image_draws(), 1);
        let after = cache.get_or_upload_tiled(&source).unwrap();
        assert_eq!(after.generation(), 1);
        dc.draw_image(
            &*after,
            rect(0.0, 0.0, 16.0, 16.0),
            None,
            ImageInterpolation::Linear,
            1.0,
        );
    });
    assert_identical(&source, &pixels);
}

// C#: D2DRenderSurfaceTests.EvictedImage_IsSkippedNotDrawn
#[test]
#[ignore = "native: needs desktop"]
fn evicted_image_is_skipped_not_drawn() {
    let mut rig = Rig::new(4, 4);
    let cache = rig.cache(None, DEFAULT_GUTTER);
    let mut source = PixelBuffer::allocate(4, 4, PixelLayout::Pbgra32).unwrap();
    for y in 0..4 {
        for pixel in source.get_row_mut(y).unwrap().chunks_exact_mut(4) {
            pixel.copy_from_slice(&[255, 0, 0, 255]);
        }
    }
    let image = cache.get_or_upload_tiled(&source).unwrap();
    cache.clear();

    let pixels = render(&mut rig.surface, |dc| {
        dc.clear(TRANSPARENT);
        dc.draw_image(&*image, rect(0.0, 0.0, 4.0, 4.0), None, ImageInterpolation::Linear, 1.0);
        assert_eq!(dc.skipped_image_draws(), 1);
    });
    assert_eq!(pixel(&pixels, 2, 2), (0, 0, 0, 0));
}

#[test]
#[ignore = "native: needs desktop"]
fn get_or_upload_returns_the_same_rc_for_the_same_buffer_through_the_trait() {
    let rig = Rig::new(8, 8);
    let cache = rig.cache(None, DEFAULT_GUTTER);
    let source = create_noise(8, 8, 1);
    let first = cache.get_or_upload(&source).unwrap();
    let second = cache.get_or_upload(&source).unwrap();
    assert!(Rc::ptr_eq(&first, &second));
    assert_eq!(
        (first.pixel_width(), first.pixel_height(), first.gpu_bytes()),
        (8, 8, 8 * 8 * 4)
    );
    assert_eq!(first.pixel_buffer_id(), source.id());
    let mut disposed = create_noise(8, 8, 2);
    disposed.dispose();
    assert_eq!(
        cache.get_or_upload(&disposed).err().unwrap(),
        RenderError::Disposed {
            object: "PhotoReview.Imaging.Pixels.PixelBuffer"
        }
    );
}
