//! Tests of `pr_platform_win::gfx::{d2d, dwrite}` (unit W8-11h). C# twins: `Graphics/D2DSmokeTests.cs` (WARP, no window except the
//! swap chain test) and `Rendering/D2DRenderSurfaceTests.Primitives_FillDrawRoundedClipAndOpacity`.
//! Pure argument checks of `copy_from_memory` are unit tests inside `gfx/d2d.rs` (`copy_from_memory_argument_checks`).
#![cfg(windows)]
#![allow(clippy::cast_possible_truncation)] // test pixel arithmetic on tiny values

use pr_platform_win::gfx::com::GfxError;
use pr_platform_win::gfx::d2d::{self, BASE_DPI};
use pr_platform_win::gfx::d3d11::{self, DEFAULT_DEVICE_FLAGS, DriverKind};
use pr_platform_win::gfx::dwrite::{self, UNBOUNDED};
use pr_platform_win::gfx::dxgi;
use pr_platform_win::gfx::extras::set_character_ellipsis_trimming;
use std::sync::{Mutex, MutexGuard, PoisonError};
use windows::Win32::Foundation::{E_INVALIDARG, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_RECT_U, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_PRIMITIVE_BLEND_COPY, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1_UNIT_MODE_PIXELS, ID2D1Bitmap1, ID2D1DeviceContext,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_REGULAR,
    DWRITE_HIT_TEST_METRICS, DWRITE_LINE_METRICS, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFontCollection, IDWriteInlineObject,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
use windows::core::{BOOL, Interface, w};
use windows_numerics::{Matrix3x2, Vector2};

// ---------------------------------------------------------------- pure

// C#: DeviceResources.CreateTarget lines 353-364
#[test]
fn target_bitmap_properties_matches_create_target() {
    let off = d2d::target_bitmap_properties(false, 1.0);
    assert_eq!(off.pixelFormat.alphaMode, D2D1_ALPHA_MODE_PREMULTIPLIED);
    assert_eq!(off.bitmapOptions, D2D1_BITMAP_OPTIONS_TARGET);
    assert_eq!(off.bitmapOptions.0, 1);
    assert_eq!((off.dpiX, off.dpiY), (96.0, 96.0));
    assert_eq!(off.pixelFormat.format, DXGI_FORMAT_B8G8R8A8_UNORM);
    assert_eq!(off.pixelFormat.format.0, 87);
    assert!(off.colorContext.is_none());

    let swap = d2d::target_bitmap_properties(true, 1.0);
    assert_eq!(swap.pixelFormat.alphaMode, D2D1_ALPHA_MODE_IGNORE);
    assert_eq!(swap.bitmapOptions.0, 3, "TARGET | CANNOT_DRAW");
    assert_eq!(swap.pixelFormat.format, DXGI_FORMAT_B8G8R8A8_UNORM);

    let scaled = d2d::target_bitmap_properties(false, 1.5);
    assert_eq!((scaled.dpiX, scaled.dpiY), (144.0, 144.0));
    let scaled = d2d::target_bitmap_properties(true, 2.0);
    assert_eq!((scaled.dpiX, scaled.dpiY), (192.0, 192.0));
}

#[test]
fn image_bitmap_properties_is_96_dpi_with_the_given_alpha_and_options() {
    let upload = d2d::image_bitmap_properties(D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BITMAP_OPTIONS_NONE);
    assert_eq!(upload.pixelFormat.alphaMode, D2D1_ALPHA_MODE_PREMULTIPLIED);
    assert_eq!(upload.bitmapOptions, D2D1_BITMAP_OPTIONS_NONE);
    assert_eq!((upload.dpiX, upload.dpiY), (96.0, 96.0));
    assert_eq!(upload.pixelFormat.format, DXGI_FORMAT_B8G8R8A8_UNORM);
    assert!(upload.colorContext.is_none());

    let read = d2d::image_bitmap_properties(
        D2D1_ALPHA_MODE_IGNORE,
        windows::Win32::Graphics::Direct2D::D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    );
    assert_eq!(read.pixelFormat.alphaMode, D2D1_ALPHA_MODE_IGNORE);
    assert_eq!(read.bitmapOptions.0, 6, "CPU_READ | CANNOT_DRAW");
    assert_eq!((read.dpiX, read.dpiY), (BASE_DPI, BASE_DPI));
}

// C#: D2DSmokeTests.StructLayouts_MatchWin32Sdk (D2D/DWrite lines)
#[test]
fn windows_rs_structs_have_the_win32_sdk_sizes() {
    use windows::Win32::Graphics::Direct2D::D2D1_BITMAP_PROPERTIES1;
    assert_eq!(size_of::<D2D1_BITMAP_PROPERTIES1>(), 32);
    assert_eq!(size_of::<D2D1_COLOR_F>(), 16);
    assert_eq!(size_of::<DWRITE_TEXT_METRICS>(), 36);
    assert_eq!(size_of::<DWRITE_LINE_METRICS>(), 24);
    assert_eq!(size_of::<DWRITE_HIT_TEST_METRICS>(), 36);
}

#[test]
fn unbounded_is_the_csharp_constant() {
    assert_eq!(UNBOUNDED, 1_000_000.0);
}

// ---------------------------------------------------------------- native helpers

/// Reference rig of the C# `D2DSmokeTests`: WARP device, DXGI device, single-threaded factory, D2D device and context.
/// Fields drop in declaration order (context first, D3D device last).
struct Rig {
    context: ID2D1DeviceContext,
    _d2d_device: windows::Win32::Graphics::Direct2D::ID2D1Device,
    _factory: windows::Win32::Graphics::Direct2D::ID2D1Factory1,
    _dxgi: IDXGIDevice,
    d3d: windows::Win32::Graphics::Direct3D11::ID3D11Device,
    /// Serialises the native tests of this file (the handle-count smoke test reads a process-wide counter). Dropped last.
    _serial: Option<MutexGuard<'static, ()>>,
}

static NATIVE_LOCK: Mutex<()> = Mutex::new(());

impl Rig {
    fn new() -> Self {
        let guard = NATIVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        Self::build(Some(guard))
    }

    fn build(serial: Option<MutexGuard<'static, ()>>) -> Self {
        let created = d3d11::create_device(DriverKind::Warp, DEFAULT_DEVICE_FLAGS).unwrap();
        let dxgi_device = d3d11::dxgi_device(&created.device).unwrap();
        let factory = d2d::create_factory(0).unwrap();
        let (d2d_device, context) = d2d::create_device_and_context(&factory, &dxgi_device).unwrap();
        Self {
            context,
            _d2d_device: d2d_device,
            _factory: factory,
            _dxgi: dxgi_device,
            d3d: created.device,
            _serial: serial,
        }
    }

    fn target(&self, width: u32, height: u32) -> ID2D1Bitmap1 {
        d2d::create_target_bitmap(&self.context, None, width, height, 1.0).unwrap()
    }

    /// `attach_target`, `BeginDraw`, the closure, `EndDraw` (must be S_OK), `detach_target`.
    fn draw(&self, target: &ID2D1Bitmap1, paint: impl FnOnce(&ID2D1DeviceContext)) {
        d2d::attach_target(&self.context, target, 1.0);
        // SAFETY: the context is live and its target is set; BeginDraw/EndDraw are balanced below on this thread.
        unsafe { self.context.BeginDraw() };
        paint(&self.context);
        assert_eq!(d2d::end_draw_hresult(&self.context), 0);
        d2d::detach_target(&self.context);
    }

    fn brush(&self, r: f32, g: f32, b: f32, a: f32) -> windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush {
        d2d::create_solid_color_brush(&self.context, D2D1_COLOR_F { r, g, b, a }).unwrap()
    }

    fn pixels(&self, target: &ID2D1Bitmap1, width: u32, height: u32) -> Vec<u8> {
        d2d::read_pixels(&self.context, target, width, height).unwrap()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        d3d11::clear_state_and_flush(&self.d3d);
    }
}

fn clear(context: &ID2D1DeviceContext, r: f32, g: f32, b: f32, a: f32) {
    let color = D2D1_COLOR_F { r, g, b, a };
    // SAFETY: `color` is a local that outlives the call; the context is inside BeginDraw/EndDraw.
    unsafe { context.Clear(Some(&color)) };
}

fn fill(
    context: &ID2D1DeviceContext,
    rect: (f32, f32, f32, f32),
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
) {
    let rect = D2D_RECT_F {
        left: rect.0,
        top: rect.1,
        right: rect.2,
        bottom: rect.3,
    };
    // SAFETY: `rect` is a local that outlives the call; `brush` is a live brush of the same device; inside BeginDraw/EndDraw.
    unsafe { context.FillRectangle(&rect, brush) };
}

fn px(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn bitmap_from(rig: &Rig, width: u32, height: u32, bgra: &[u8], pitch: u32) -> ID2D1Bitmap1 {
    let bitmap = d2d::create_bitmap_uninitialized(
        &rig.context,
        width,
        height,
        D2D1_ALPHA_MODE_PREMULTIPLIED,
        D2D1_BITMAP_OPTIONS_NONE,
    )
    .unwrap();
    d2d::copy_from_memory(&bitmap, None, bgra, pitch).unwrap();
    bitmap
}

fn draw_bitmap_1to1(rig: &Rig, target: &ID2D1Bitmap1, bitmap: &ID2D1Bitmap1, width: f32, height: f32) {
    rig.draw(target, |context| {
        clear(context, 0.0, 0.0, 0.0, 0.0);
        let rect = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: width,
            bottom: height,
        };
        // SAFETY: `rect` is a local that outlives the call; `bitmap` belongs to the same device; inside BeginDraw/EndDraw.
        unsafe {
            context.DrawBitmap(
                bitmap,
                Some(&rect),
                1.0,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                Some(&rect),
                None,
            )
        };
    });
}

/// A hidden `STATIC` popup window, destroyed on drop.
struct HiddenWindow(HWND);

impl HiddenWindow {
    fn new() -> Self {
        // SAFETY: "STATIC" is a system window class; static wide literals; no parent, menu, instance or creation parameter. Never
        // shown; destroyed in `Drop` on this thread.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("gfx test"),
                WS_POPUP,
                0,
                0,
                64,
                64,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        Self(window)
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        // SAFETY: the handle was created by this thread in `new` and is destroyed once.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

// ---------------------------------------------------------------- native: Direct2D

// C#: D2DSmokeTests.Offscreen_Bitmap_DrawAndReadBackPixels
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn offscreen_bitmap_draw_and_read_back_pixels() {
    let rig = Rig::new();
    let target = rig.target(4, 4);
    let green = rig.brush(0.0, 1.0, 0.0, 1.0);
    rig.draw(&target, |context| {
        clear(context, 0.0, 0.0, 1.0, 1.0);
        fill(context, (0.0, 0.0, 2.0, 2.0), &green);
    });
    let pixels = rig.pixels(&target, 4, 4);
    assert_eq!(pixels.len(), 64);
    assert_eq!(px(&pixels, 4, 0, 0), [0, 255, 0, 255]);
    assert_eq!(px(&pixels, 4, 1, 1), [0, 255, 0, 255]);
    assert_eq!(px(&pixels, 4, 2, 2), [255, 0, 0, 255]);
    assert_eq!(px(&pixels, 4, 3, 3), [255, 0, 0, 255]);
    assert_eq!(px(&pixels, 4, 3, 0), [255, 0, 0, 255]);
}

// C#: D2DSmokeTests.Bitmap_StructReturningMethods_ReportSizeAndFormat
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn bitmap_reports_size_format_dpi_and_options() {
    let rig = Rig::new();
    let target = rig.target(6, 3);
    // SAFETY: `target` is a live owned bitmap; the out pointers are locals that outlive the calls.
    unsafe {
        let pixel = target.GetPixelSize();
        assert_eq!((pixel.width, pixel.height), (6, 3));
        let size = target.GetSize();
        assert_eq!((size.width, size.height), (6.0, 3.0));
        let format = target.GetPixelFormat();
        assert_eq!(format.format, DXGI_FORMAT_B8G8R8A8_UNORM);
        assert_eq!(format.alphaMode, D2D1_ALPHA_MODE_PREMULTIPLIED);
        let (mut dpi_x, mut dpi_y) = (0.0_f32, 0.0_f32);
        target.GetDpi(&mut dpi_x, &mut dpi_y);
        assert_eq!((dpi_x, dpi_y), (96.0, 96.0));
        assert_eq!(target.GetOptions(), D2D1_BITMAP_OPTIONS_TARGET);
    }
}

// C#: D2DSmokeTests.DeviceContext_StateSetters_RoundTrip (proves the retired members still exist with the same semantics)
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn device_context_state_setters_round_trip() {
    let rig = Rig::new();
    let context = &rig.context;
    // SAFETY: the context is a live owned interface; every pointer argument is a local that outlives its call.
    unsafe {
        let matrix = Matrix3x2::translation(3.0, 4.0);
        context.SetTransform(&matrix);
        let mut back = Matrix3x2::default();
        context.GetTransform(&mut back);
        assert_eq!(back, matrix);
        context.SetTransform(&Matrix3x2::identity());

        context.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
        assert_eq!(context.GetAntialiasMode(), D2D1_ANTIALIAS_MODE_ALIASED);
        context.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        context.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        assert_eq!(context.GetTextAntialiasMode(), D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        context.SetUnitMode(D2D1_UNIT_MODE_PIXELS);
        assert_eq!(context.GetUnitMode(), D2D1_UNIT_MODE_PIXELS);
        context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
        assert_eq!(context.GetPrimitiveBlend(), D2D1_PRIMITIVE_BLEND_COPY);
        context.SetDpi(120.0, 144.0);
        let (mut dpi_x, mut dpi_y) = (0.0_f32, 0.0_f32);
        context.GetDpi(&mut dpi_x, &mut dpi_y);
        assert_eq!((dpi_x, dpi_y), (120.0, 144.0));
    }
    assert!(d2d::max_bitmap_size(context) >= 4096);
}

// C#: D2DSmokeTests.DrawBitmap_Scales_FromSourceToDestination
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn draw_bitmap_scales_from_source_to_destination() {
    let rig = Rig::new();
    let red: Vec<u8> = [0u8, 0, 255, 255].repeat(4);
    let source = bitmap_from(&rig, 2, 2, &red, 8);
    let target = rig.target(4, 4);
    rig.draw(&target, |context| {
        clear(context, 0.0, 0.0, 0.0, 0.0);
        let dest = D2D_RECT_F {
            left: 1.0,
            top: 1.0,
            right: 3.0,
            bottom: 3.0,
        };
        let src = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: 2.0,
            bottom: 2.0,
        };
        // SAFETY: the rectangles are locals that outlive the call; `source` belongs to the same device; inside BeginDraw/EndDraw.
        unsafe {
            context.DrawBitmap(
                &source,
                Some(&dest),
                1.0,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                Some(&src),
                None,
            )
        };
    });
    let pixels = rig.pixels(&target, 4, 4);
    assert_eq!(px(&pixels, 4, 1, 1), [0, 0, 255, 255]);
    assert_eq!(px(&pixels, 4, 2, 2), [0, 0, 255, 255]);
    assert_eq!(px(&pixels, 4, 0, 0), [0, 0, 0, 0]);
    assert_eq!(px(&pixels, 4, 3, 3), [0, 0, 0, 0]);
}

// C#: D2DSmokeTests.BitmapFromD3DTexture_ViaDxgiSurface_DrawAndReadBack
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn bitmap_from_d3d_texture_via_dxgi_surface_draw_and_read_back() {
    let rig = Rig::new();
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 4,
        Height: 4,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture: Option<ID3D11Texture2D> = None;
    // SAFETY: `desc` and `texture` are locals that outlive the call; no initial data.
    unsafe { rig.d3d.CreateTexture2D(&desc, None, Some(&mut texture)) }.unwrap();
    let surface: IDXGISurface = texture.unwrap().cast().unwrap();
    let mut props = d2d::target_bitmap_properties(false, 1.0);
    props.bitmapOptions |= D2D1_BITMAP_OPTIONS_CANNOT_DRAW;
    // SAFETY: `surface` and `props` are locals that outlive the call; the colour context is None.
    let bitmap = unsafe { rig.context.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }.unwrap();
    // SAFETY: `surface` is a live owned interface.
    let surface_desc = unsafe { surface.GetDesc() }.unwrap();
    assert_eq!(
        (surface_desc.Width, surface_desc.Height, surface_desc.Format),
        (4, 4, DXGI_FORMAT_B8G8R8A8_UNORM)
    );
    let green = rig.brush(0.0, 1.0, 0.0, 1.0);
    rig.draw(&bitmap, |context| {
        clear(context, 0.0, 0.0, 1.0, 1.0);
        fill(context, (0.0, 0.0, 2.0, 2.0), &green);
    });
    let pixels = rig.pixels(&bitmap, 4, 4);
    assert_eq!(px(&pixels, 4, 0, 0), [0, 255, 0, 255]);
    assert_eq!(px(&pixels, 4, 3, 3), [255, 0, 0, 255]);
}

// C#: D2DSmokeTests.SwapChain_ForComposition_... (the Direct2D half; HWND swap chain)
#[test]
#[ignore = "native: needs desktop"]
fn swap_chain_back_buffer_target_draws_presents_and_survives_resize() {
    let rig = Rig::new();
    let window = HiddenWindow::new();
    let chain = dxgi::create_window_swap_chain(&rig.d3d, window.0, 16, 16, 1).unwrap();
    let yellow = rig.brush(1.0, 1.0, 0.0, 1.0);

    let target = d2d::create_target_bitmap(&rig.context, Some(&chain.swap_chain), 16, 16, 1.0).unwrap();
    // SAFETY: `target` is a live owned bitmap.
    unsafe {
        assert_eq!(target.GetPixelSize().width, 16);
        assert_eq!(target.GetOptions().0, 3, "TARGET | CANNOT_DRAW");
    }
    rig.draw(&target, |context| clear(context, 1.0, 1.0, 0.0, 1.0));
    assert!(dxgi::present_raw(&chain.swap_chain, 0) >= 0);

    // Trap T1/T2: detach, drop the bitmap, flush, then resize.
    d2d::detach_target(&rig.context);
    drop(target);
    d3d11::clear_state_and_flush(&rig.d3d);
    dxgi::resize_buffers(&chain.swap_chain, 32, 8).unwrap();
    let target = d2d::create_target_bitmap(&rig.context, Some(&chain.swap_chain), 32, 8, 1.0).unwrap();
    // SAFETY: `target` is a live owned bitmap.
    assert_eq!(unsafe { target.GetPixelSize() }.width, 32);
    rig.draw(&target, |context| fill(context, (0.0, 0.0, 32.0, 8.0), &yellow));
    assert!(dxgi::present_raw(&chain.swap_chain, 0) >= 0);

    d2d::detach_target(&rig.context);
    drop(target);
    d3d11::clear_state_and_flush(&rig.d3d);
    drop(chain);
}

// C#: D2DSmokeTests.Brush_Primitives_ClipAndCopyFromMemory_Work
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn copy_from_memory_and_clip_and_brush_primitives_work() {
    let rig = Rig::new();
    let black = rig.brush(0.0, 0.0, 0.0, 1.0);
    // SAFETY: the brush is a live owned interface.
    unsafe {
        black.SetOpacity(0.5);
        assert_eq!(black.GetOpacity(), 0.5);
        black.SetOpacity(1.0);
        let color = D2D1_COLOR_F {
            r: 0.25,
            g: 0.5,
            b: 0.75,
            a: 1.0,
        };
        black.SetColor(&color);
        let back = black.GetColor();
        assert_eq!((back.r, back.g, back.b, back.a), (0.25, 0.5, 0.75, 1.0));
        black.SetColor(&D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
    }

    // rectangle outline and line
    let target = rig.target(8, 8);
    rig.draw(&target, |context| {
        clear(context, 1.0, 1.0, 1.0, 1.0);
        let rect = D2D_RECT_F {
            left: 0.5,
            top: 0.5,
            right: 7.5,
            bottom: 7.5,
        };
        // SAFETY: `rect` is a local that outlives the call; the brush is live; inside BeginDraw/EndDraw.
        unsafe {
            context.DrawRectangle(&rect, &black, 1.0, None);
            context.DrawLine(Vector2::new(2.5, 4.5), Vector2::new(5.5, 4.5), &black, 1.0, None);
        }
    });
    let pixels = rig.pixels(&target, 8, 8);
    assert_eq!(px(&pixels, 8, 0, 0), [0, 0, 0, 255], "corner of the outline");
    assert_eq!(px(&pixels, 8, 3, 2), [255, 255, 255, 255], "inside");
    assert_eq!(px(&pixels, 8, 3, 4), [0, 0, 0, 255], "line");

    // axis aligned clip
    let red = rig.brush(1.0, 0.0, 0.0, 1.0);
    rig.draw(&target, |context| {
        clear(context, 0.0, 0.0, 0.0, 1.0);
        let clip = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: 2.0,
            bottom: 2.0,
        };
        // SAFETY: `clip` is a local that outlives the call; Push and Pop are balanced inside BeginDraw/EndDraw.
        unsafe {
            context.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_ALIASED);
            fill(context, (0.0, 0.0, 8.0, 8.0), &red);
            context.PopAxisAlignedClip();
        }
    });
    let pixels = rig.pixels(&target, 8, 8);
    assert_eq!(px(&pixels, 8, 1, 1), [0, 0, 255, 255]);
    assert_eq!(px(&pixels, 8, 2, 2), [0, 0, 0, 255]);

    // CopyFromMemory then draw 1:1
    let data = [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255];
    let bitmap = bitmap_from(&rig, 2, 2, &data, 8);
    let small = rig.target(2, 2);
    draw_bitmap_1to1(&rig, &small, &bitmap, 2.0, 2.0);
    let pixels = rig.pixels(&small, 2, 2);
    assert_eq!(px(&pixels, 2, 0, 0), [1, 2, 3, 255]);
    assert_eq!(px(&pixels, 2, 1, 0), [4, 5, 6, 255]);
    assert_eq!(px(&pixels, 2, 0, 1), [7, 8, 9, 255]);
    assert_eq!(px(&pixels, 2, 1, 1), [10, 11, 12, 255]);
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn copy_from_memory_into_a_sub_rect_and_with_a_wide_pitch() {
    let rig = Rig::new();
    let bitmap = d2d::create_bitmap_uninitialized(
        &rig.context,
        4,
        4,
        D2D1_ALPHA_MODE_PREMULTIPLIED,
        D2D1_BITMAP_OPTIONS_NONE,
    )
    .unwrap();
    // contents are undefined until written: zero the whole bitmap first
    d2d::copy_from_memory(&bitmap, None, &[0u8; 64], 16).unwrap();
    let mut source = [0u8; 32];
    source[..8].copy_from_slice(&[10, 20, 30, 255, 40, 50, 60, 255]);
    source[16..24].copy_from_slice(&[70, 80, 90, 255, 100, 110, 120, 255]);
    let rect = D2D_RECT_U {
        left: 1,
        top: 1,
        right: 3,
        bottom: 3,
    };
    d2d::copy_from_memory(&bitmap, Some(rect), &source, 16).unwrap();

    let target = rig.target(4, 4);
    draw_bitmap_1to1(&rig, &target, &bitmap, 4.0, 4.0);
    let pixels = rig.pixels(&target, 4, 4);
    assert_eq!(px(&pixels, 4, 1, 1), [10, 20, 30, 255]);
    assert_eq!(px(&pixels, 4, 2, 1), [40, 50, 60, 255]);
    assert_eq!(px(&pixels, 4, 1, 2), [70, 80, 90, 255]);
    assert_eq!(px(&pixels, 4, 2, 2), [100, 110, 120, 255]);
    for (x, y) in [(0, 0), (3, 3), (0, 3), (3, 0), (0, 1), (3, 2)] {
        assert_eq!(px(&pixels, 4, x, y), [0, 0, 0, 0], "({x},{y}) outside the block");
    }
    // argument errors keep their name
    let error = d2d::copy_from_memory(&bitmap, None, &[0u8; 10], 16).unwrap_err();
    assert_eq!(error, GfxError::new("CopyFromMemory", E_INVALIDARG.0));
}

// Trap T3
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn read_pixels_uses_the_mapped_pitch_not_the_row_length() {
    let rig = Rig::new();
    let target = rig.target(5, 3);
    let mut expected = Vec::new();
    rig.draw(&target, |context| {
        clear(context, 0.0, 0.0, 0.0, 1.0);
        for y in 0..3u32 {
            for x in 0..5u32 {
                let b = (x * 40 + 10) as u8;
                let g = (y * 80 + 5) as u8;
                let r = (100 + x + y * 3) as u8;
                let brush = rig.brush(f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, 1.0);
                fill(context, (x as f32, y as f32, x as f32 + 1.0, y as f32 + 1.0), &brush);
            }
        }
    });
    for y in 0..3u32 {
        for x in 0..5u32 {
            expected.extend_from_slice(&[(x * 40 + 10) as u8, (y * 80 + 5) as u8, (100 + x + y * 3) as u8, 255]);
        }
    }
    let pixels = rig.pixels(&target, 5, 3);
    assert_eq!(pixels.len(), 5 * 3 * 4);
    assert_eq!(pixels, expected);
}

// C#: D2DRenderSurfaceTests.Primitives_FillDrawRoundedClipAndOpacity (the retired raw vtable call)
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn fill_rounded_rectangle_native_call_replaces_the_raw_vtable_call() {
    let rig = Rig::new();
    let target = rig.target(20, 40);
    let white = rig.brush(1.0, 1.0, 1.0, 1.0);
    rig.draw(&target, |context| {
        clear(context, 0.0, 0.0, 0.0, 1.0);
        let rounded = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: 0.0,
                top: 20.0,
                right: 20.0,
                bottom: 40.0,
            },
            radiusX: 8.0,
            radiusY: 8.0,
        };
        // SAFETY: `rounded` is a local that outlives the call; the brush is live; inside BeginDraw/EndDraw.
        unsafe { context.FillRoundedRectangle(&rounded, &white) };
    });
    let pixels = rig.pixels(&target, 20, 40);
    assert_eq!(px(&pixels, 20, 10, 30), [255, 255, 255, 255], "centre");
    assert_eq!(px(&pixels, 20, 0, 20), [0, 0, 0, 255], "corner cut by the radius");
}

// ---------------------------------------------------------------- native: DirectWrite

// C#: D2DSmokeTests.DirectWrite_VietnameseLayout_MeasuresAndHitTests
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn directwrite_vietnamese_layout_measures_and_hit_tests() {
    let factory = dwrite::create_factory().unwrap();
    let format = dwrite::create_text_format(&factory, "Segoe UI", false, 16.0, "vi-VN").unwrap();
    let text = "Tiếng Việt";
    let layout = dwrite::create_text_layout(&factory, text, &format, 1000.0).unwrap();
    // SAFETY: all objects are live owned interfaces; every out pointer is a local that outlives its call.
    unsafe {
        assert_eq!(format.GetFontSize(), 16.0);
        assert_eq!(format.GetFontWeight(), DWRITE_FONT_WEIGHT_REGULAR);
        format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER).unwrap();
        assert_eq!(format.GetTextAlignment(), DWRITE_TEXT_ALIGNMENT_CENTER);

        let metrics = dwrite::layout_metrics(&layout).unwrap();
        assert!(metrics.width > 0.0 && metrics.height > 0.0);
        assert_eq!(metrics.line_count, 1);
        assert_eq!(layout.GetMaxWidth(), 1000.0);

        let mut lines = [DWRITE_LINE_METRICS::default(); 1];
        let mut count = 0;
        layout.GetLineMetrics(Some(&mut lines), &mut count).unwrap();
        assert_eq!(lines[0].length, 10);
        assert!(lines[0].baseline > 0.0);

        let (mut x, mut y) = (0.0_f32, 0.0_f32);
        let mut hit = DWRITE_HIT_TEST_METRICS::default();
        layout.HitTestTextPosition(10, false, &mut x, &mut y, &mut hit).unwrap();
        assert!(x > 0.0);
        let (mut trailing, mut inside) = (BOOL(0), BOOL(0));
        layout
            .HitTestPoint(x + 500.0, 1.0, &mut trailing, &mut inside, &mut hit)
            .unwrap();
        assert!(!inside.as_bool());

        layout.SetMaxWidth(metrics.width / 3.0).unwrap();
        assert!(dwrite::layout_metrics(&layout).unwrap().line_count > 1);
        layout.SetMaxWidth(1000.0).unwrap();
        layout
            .SetFontSize(
                32.0,
                DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length: 10,
                },
            )
            .unwrap();
        assert!(dwrite::layout_metrics(&layout).unwrap().width > metrics.width * 1.5);
    }
}

// C#: D2DSmokeTests.DirectWrite_TextDrawnWithD2D_ProducesInk
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn directwrite_text_drawn_with_d2d_produces_ink() {
    let rig = Rig::new();
    let factory = dwrite::create_factory().unwrap();
    let format = dwrite::create_text_format(&factory, "Segoe UI", true, 18.0, "vi-VN").unwrap();
    let layout = dwrite::create_text_layout(&factory, "Tiếng Việt", &format, 96.0).unwrap();
    let target = rig.target(96, 32);
    let black = rig.brush(0.0, 0.0, 0.0, 1.0);
    rig.draw(&target, |context| {
        clear(context, 1.0, 1.0, 1.0, 1.0);
        // SAFETY: layout and brush are live and belong to compatible factories; inside BeginDraw/EndDraw.
        unsafe { context.DrawTextLayout(Vector2::new(0.0, 0.0), &layout, &black, D2D1_DRAW_TEXT_OPTIONS_NONE) };
    });
    let pixels = rig.pixels(&target, 96, 32);
    let dark = pixels
        .chunks_exact(4)
        .filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128)
        .count();
    assert!(dark > 30, "dark pixels: {dark}");
}

// C#: D2DSmokeTests.DirectWrite_TextFormat_WrappingTrimmingAndParagraphAlignment_RoundTrip
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn directwrite_text_format_wrapping_trimming_and_paragraph_alignment_round_trip() {
    let factory = dwrite::create_factory().unwrap();
    // SAFETY: static wide strings; the factory is live; the format owns copies.
    let format = unsafe {
        factory.CreateTextFormat(
            w!("Segoe UI"),
            None::<&IDWriteFontCollection>,
            DWRITE_FONT_WEIGHT_REGULAR,
            DWRITE_FONT_STYLE_ITALIC,
            DWRITE_FONT_STRETCH_NORMAL,
            14.0,
            w!("en-US"),
        )
    }
    .unwrap();
    // SAFETY: `format` is a live owned interface; the trimming struct and out pointers are locals.
    unsafe {
        assert_eq!(format.GetFontStyle(), DWRITE_FONT_STYLE_ITALIC);
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).unwrap();
        assert_eq!(format.GetWordWrapping(), DWRITE_WORD_WRAPPING_NO_WRAP);
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER).unwrap();
        assert_eq!(format.GetParagraphAlignment(), DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        let trimming = DWRITE_TRIMMING {
            granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            delimiter: 0,
            delimiterCount: 0,
        };
        format.SetTrimming(&trimming, None::<&IDWriteInlineObject>).unwrap();
        let mut back = DWRITE_TRIMMING::default();
        let mut sign: Option<IDWriteInlineObject> = None;
        format.GetTrimming(&mut back, &mut sign).unwrap();
        assert_eq!(back.granularity, DWRITE_TRIMMING_GRANULARITY_CHARACTER);
    }
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn create_text_format_bold_flag_and_empty_text_layout() {
    let factory = dwrite::create_factory().unwrap();
    let bold = dwrite::create_text_format(&factory, "Segoe UI", true, 12.0, "en-US").unwrap();
    // SAFETY: `bold` is a live owned interface.
    assert_eq!(unsafe { bold.GetFontWeight() }, DWRITE_FONT_WEIGHT_BOLD);
    let layout = dwrite::create_text_layout(&factory, "", &bold, 100.0).unwrap();
    assert_eq!(dwrite::layout_metrics(&layout).unwrap().width, 0.0);
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn first_line_is_trimmed_follows_the_ellipsis_helper() {
    let factory = dwrite::create_factory().unwrap();
    let format = dwrite::create_text_format(&factory, "Segoe UI", false, 12.0, "en-US").unwrap();
    let long = "A very long line of text that cannot possibly fit in a hundred and twenty device independent pixels";
    let narrow = dwrite::create_text_layout(&factory, long, &format, 120.0).unwrap();
    // SAFETY: `narrow` is a live owned interface.
    unsafe { narrow.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }.unwrap();
    assert!(!dwrite::first_line_is_trimmed(&narrow));
    set_character_ellipsis_trimming(&factory, &narrow).unwrap();
    assert!(dwrite::first_line_is_trimmed(&narrow));

    let short = dwrite::create_text_layout(&factory, "short", &format, 500.0).unwrap();
    // SAFETY: `short` is a live owned interface.
    unsafe { short.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }.unwrap();
    assert!(!dwrite::first_line_is_trimmed(&short));
}

// Smoke leak guard (weak by design, contract open question 3): graphics objects must not pile up process handles.
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn create_device_and_context_leaves_no_references_after_drop() {
    fn handles() -> Option<u32> {
        let mut count = 0_u32;
        // SAFETY: the pseudo handle of the current process is always valid; `count` is a local that outlives the call.
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) }.ok()?;
        Some(count)
    }
    let _lock = NATIVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let cycle = || {
        let rig = Rig::build(None);
        let target = rig.target(8, 8);
        drop(target);
        drop(rig);
    };
    // The first rigs create process-wide driver state once (WARP worker threads, loader handles): warm up, then measure.
    for _ in 0..5 {
        cycle();
    }
    let Some(before) = handles() else {
        println!("skipped: GetProcessHandleCount unavailable");
        return;
    };
    for _ in 0..50 {
        cycle();
    }
    let after = handles().unwrap_or(before);
    assert!(after <= before + 20, "handles grew from {before} to {after}");
}
