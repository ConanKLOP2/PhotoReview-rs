//! Phase 0 UI spike (docs/adr/R0001, P-06): a minimal Win32 window with a Direct2D render target and DirectWrite text.
//!
//! Not a port of C# code: it exists to measure whether "Win32 + Direct2D through `windows-rs`" meets the review-speed
//! priority before the real windows are built. All `unsafe` of the spike lives here; the app side (`pr_app::spike`) is
//! safe code that implements [`SpikeApp`].
//!
//! Model: single UI thread, blocking `GetMessageW` loop (no polling, so an idle window uses no CPU), painting only on
//! `WM_PAINT` after `InvalidateRect`. Other threads wake the UI thread with [`WakeHandle::wake`].

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{E_FAIL, HMODULE};
use windows::Win32::Foundation::{FILETIME, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES, D2D1_BITMAP_PROPERTIES1,
    D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_IMMEDIATELY,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_TYPE_HARDWARE, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1CreateFactory, ID2D1Bitmap,
    ID2D1DeviceContext, ID2D1Factory1, ID2D1HwndRenderTarget, ID2D1Image, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_PRESENT, DXGI_SCALING_NONE, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIDevice, IDXGIFactory1,
    IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT, ScreenToClient};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE, VK_LEFT, VK_RIGHT};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW, IDC_ARROW,
    KillTimer, LoadCursorW, MSG, PostMessageW, PostQuitMessage, RegisterClassW, SW_SHOW, SW_SHOWNA, SetTimer,
    SetWindowLongPtrW, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT,
    WM_SIZE, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{GUID, Interface, PCWSTR, w};

/// Private "something happened on another thread" message (see [`WakeHandle`]).
const WM_WAKE: u32 = WM_APP + 1;
/// The one timer the app can arm ([`Ctx::set_timer`]).
const APP_TIMER_ID: usize = 1;

/// Keys the spike reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    /// `F`: back to Fit.
    Fit,
    /// `1`: 100% (source pixels).
    Actual,
    Escape,
}

/// Input events delivered to the app.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Key(Key),
    /// Wheel notches (positive = up/away) at client position `(x, y)`.
    Wheel {
        notches: f32,
        x: f32,
        y: f32,
    },
    /// Left-button drag delta in client pixels.
    Drag {
        dx: f32,
        dy: f32,
    },
    /// Client area changed size.
    Resized,
}

/// What the app implements. All callbacks run on the UI thread.
pub trait SpikeApp {
    /// Called once, after the window exists and before it is shown.
    fn on_create(&mut self, ctx: &mut Ctx<'_>);
    fn on_input(&mut self, input: Input, ctx: &mut Ctx<'_>);
    /// Paints into `canvas` (called on `WM_PAINT` and by [`Ctx::render_now`]).
    fn paint(&mut self, canvas: &mut Canvas<'_>);
    /// Another thread called [`WakeHandle::wake`].
    fn on_wake(&mut self, ctx: &mut Ctx<'_>);
    /// The timer armed with [`Ctx::set_timer`] fired.
    fn on_timer(&mut self, ctx: &mut Ctx<'_>);
    /// A `WM_PAINT` frame finished (after `EndDraw`), used for the first-frame time.
    fn after_paint(&mut self);
}

/// Window options.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    /// Wanted client size in physical pixels.
    pub client_width: i32,
    pub client_height: i32,
    /// Show without taking focus (used by the automated measurement).
    pub no_activate: bool,
}

/// Thread-safe handle that posts a wake-up to the UI thread.
#[derive(Debug, Clone, Copy)]
pub struct WakeHandle(isize);

impl WakeHandle {
    /// Asks the UI thread to run [`SpikeApp::on_wake`]. Never blocks. A closed window just ignores it.
    pub fn wake(self) {
        // SAFETY: PostMessageW is safe to call from any thread with any HWND value: for a stale or invalid handle it
        // fails and returns an error, which is ignored on purpose (the window is gone, there is nobody to wake).
        let _ = unsafe { PostMessageW(Some(HWND(self.0 as *mut c_void)), WM_WAKE, WPARAM(0), LPARAM(0)) };
    }
}

/// Direct2D + DirectWrite resources of one window.
struct Gfx {
    hwnd: HWND,
    factory: ID2D1Factory1,
    /// Kept alive for the lifetime of the text format created from it.
    _dwrite: IDWriteFactory,
    text_format: IDWriteTextFormat,
    /// The drawing surface: the HWND render target (legacy) or the device context over a DXGI flip swap chain.
    target: Option<ID2D1RenderTarget>,
    hwnd_rt: Option<ID2D1HwndRenderTarget>,
    dc: Option<ID2D1DeviceContext>,
    swap: Option<IDXGISwapChain1>,
    brush: Option<ID2D1SolidColorBrush>,
    bitmaps: HashMap<u64, ID2D1Bitmap>,
    quit: bool,
    /// Duration of the last `IDXGISwapChain::Present` call in milliseconds (0 on the HWND render target path).
    last_present_ms: f64,
}

/// Everything the window procedure needs.
struct State {
    gfx: Option<Gfx>,
    app: Option<Box<dyn SpikeApp>>,
    drag_last: Option<(i32, i32)>,
}

/// UI-thread context handed to the callbacks that may invalidate, render or quit.
#[derive(Debug)]
pub struct Ctx<'a> {
    gfx: &'a mut Gfx,
}

/// Drawing surface handed to [`SpikeApp::paint`].
#[derive(Debug)]
pub struct Canvas<'a> {
    gfx: &'a mut Gfx,
    size: (f32, f32),
}

impl std::fmt::Debug for Gfx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gfx")
            .field("bitmaps", &self.bitmaps.len())
            .finish_non_exhaustive()
    }
}

fn client_size_of(hwnd: HWND) -> (u32, u32) {
    let mut rc = RECT::default();
    // SAFETY: `rc` is a valid out pointer for the call; a failure leaves it zeroed, which maps to a 1x1 client below.
    let _ = unsafe { GetClientRect(hwnd, &mut rc) };
    (
        u32::try_from((rc.right - rc.left).max(1)).unwrap_or(1),
        u32::try_from((rc.bottom - rc.top).max(1)).unwrap_or(1),
    )
}

impl Gfx {
    fn ensure_target(&mut self) -> windows::core::Result<()> {
        if self.target.is_some() {
            return Ok(());
        }
        let (w, h) = client_size_of(self.hwnd);
        let wants_hwnd = std::env::var("PHOTOREVIEW_RS_SPIKE_BACKEND").as_deref() == Ok("hwnd");
        if !wants_hwnd && self.create_dxgi(w, h).is_ok() {
            return self.create_brush();
        }
        self.swap = None;
        self.dc = None;
        let kind = match std::env::var("PHOTOREVIEW_RS_SPIKE_RT").as_deref() {
            Ok("hardware") => D2D1_RENDER_TARGET_TYPE_HARDWARE,
            Ok("software") => D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            _ => D2D1_RENDER_TARGET_TYPE_DEFAULT,
        };
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: kind,
            // Pixels, not DIPs: 1 source pixel = 1 device pixel at 100% zoom regardless of the monitor DPI.
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: self.hwnd,
            pixelSize: D2D_SIZE_U { width: w, height: h },
            // Present without waiting for vblank so the measurement shows work done, not the refresh interval.
            presentOptions: if std::env::var_os("PHOTOREVIEW_RS_SPIKE_VSYNC").is_some() {
                D2D1_PRESENT_OPTIONS_NONE
            } else {
                D2D1_PRESENT_OPTIONS_IMMEDIATELY
            },
        };
        // SAFETY: both property structs outlive the call; the HWND belongs to this thread's live window.
        let hwnd_rt = unsafe { self.factory.CreateHwndRenderTarget(&props, &hwnd_props)? };
        self.target = Some(hwnd_rt.cast::<ID2D1RenderTarget>()?);
        self.hwnd_rt = Some(hwnd_rt);
        self.create_brush()
    }

    fn create_brush(&mut self) -> windows::core::Result<()> {
        let Some(target) = self.target.as_ref() else {
            return Ok(());
        };
        let color = D2D1_COLOR_F {
            r: 0.92,
            g: 0.92,
            b: 0.92,
            a: 1.0,
        };
        // SAFETY: `color` outlives the call; brush properties are optional.
        let brush = unsafe { target.CreateSolidColorBrush(&color, None)? };
        self.brush = Some(brush);
        Ok(())
    }

    /// Direct3D 11 device + DXGI flip-model swap chain + Direct2D device context drawing into its back buffer.
    fn create_dxgi(&mut self, w: u32, h: u32) -> windows::core::Result<()> {
        // SAFETY: plain device/swap-chain creation with locals that outlive each call; every interface is owned (COM
        // reference counted) by this struct or dropped at the end of the function.
        unsafe {
            let mut device = None;
            let choice = std::env::var("PHOTOREVIEW_RS_SPIKE_ADAPTER").unwrap_or_default();
            let picked: Option<IDXGIAdapter> = match choice.parse::<u32>() {
                Ok(index) => CreateDXGIFactory1::<IDXGIFactory1>()?
                    .EnumAdapters1(index)
                    .ok()
                    .map(|a| a.cast())
                    .transpose()?,
                Err(_) => None,
            };
            let driver = if picked.is_some() {
                D3D_DRIVER_TYPE_UNKNOWN
            } else if choice == "warp" {
                D3D_DRIVER_TYPE_WARP
            } else {
                D3D_DRIVER_TYPE_HARDWARE
            };
            D3D11CreateDevice(
                picked.as_ref(),
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
            let device = device.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            let dxgi_device: IDXGIDevice = device.cast()?;
            let adapter = dxgi_device.GetAdapter()?;
            if std::env::var_os("PHOTOREVIEW_RS_SPIKE_TRACE").is_some() {
                if let Ok(d) = adapter.GetDesc() {
                    let name = String::from_utf16_lossy(&d.Description);
                    eprintln!("dxgi adapter: {}", name.trim_end_matches(' '));
                }
            }
            let dxgi_factory: IDXGIFactory2 = adapter.GetParent()?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: w,
                Height: h,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_NONE,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                Flags: 0,
            };
            let swap = dxgi_factory.CreateSwapChainForHwnd(&device, self.hwnd, &desc, None, None)?;
            let d2d_device = self.factory.CreateDevice(&dxgi_device)?;
            let dc = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            dc.SetDpi(96.0, 96.0);
            self.swap = Some(swap);
            self.dc = Some(dc.clone());
            self.attach_backbuffer()?;
            self.target = Some(dc.cast::<ID2D1RenderTarget>()?);
        }
        Ok(())
    }

    /// Makes the swap chain's current back buffer the device context's target.
    fn attach_backbuffer(&mut self) -> windows::core::Result<()> {
        let (Some(swap), Some(dc)) = (self.swap.as_ref(), self.dc.as_ref()) else {
            return Ok(());
        };
        // SAFETY: the surface and the property struct outlive the call; the bitmap keeps its own reference to the surface.
        unsafe {
            let offscreen = std::env::var("PHOTOREVIEW_RS_SPIKE_BACKEND").as_deref() == Ok("offscreen");
            let surface: IDXGISurface = swap.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: std::mem::ManuallyDrop::new(None),
            };
            let bitmap = if offscreen {
                // Measurement only: an ordinary Direct2D target bitmap, never presented, so the compositor is out of the loop.
                let (w, h) = client_size_of(self.hwnd);
                dc.CreateBitmap(D2D_SIZE_U { width: w, height: h }, None, 0, &props)?
            } else {
                dc.CreateBitmapFromDxgiSurface(&surface, Some(&props))?
            };
            dc.SetTarget(&bitmap);
        }
        Ok(())
    }

    /// Client area changed: resize whichever surface is in use. A failure drops the target; the next paint rebuilds it.
    fn resize(&mut self, w: u32, h: u32) {
        let ok = if let (Some(swap), Some(dc)) = (self.swap.clone(), self.dc.clone()) {
            // SAFETY: the target bitmap (the only other reference to the back buffer) is released before ResizeBuffers.
            let resized = unsafe {
                dc.SetTarget(None::<&ID2D1Image>);
                swap.ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
            };
            resized.is_ok() && self.attach_backbuffer().is_ok()
        } else if let Some(rt) = self.hwnd_rt.as_ref() {
            // SAFETY: the size struct is a local; the target belongs to this window.
            unsafe { rt.Resize(&D2D_SIZE_U { width: w, height: h }) }.is_ok()
        } else {
            true
        };
        if !ok {
            self.drop_target();
        }
    }

    fn drop_target(&mut self) {
        self.bitmaps.clear();
        self.brush = None;
        self.target = None;
        self.hwnd_rt = None;
        self.dc = None;
        self.swap = None;
    }

    /// BeginDraw / paint / EndDraw. A lost device (`D2DERR_RECREATE_TARGET`) drops the target; the next paint rebuilds it.
    fn render(&mut self, paint: impl FnOnce(&mut Canvas<'_>)) {
        if self.ensure_target().is_err() {
            return;
        }
        let Some(target) = self.target.clone() else { return };
        let (w, h) = client_size_of(self.hwnd);
        // SAFETY: BeginDraw/Clear on a render target owned by this thread; paired with EndDraw below.
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&D2D1_COLOR_F {
                r: 0.08,
                g: 0.08,
                b: 0.09,
                a: 1.0,
            }));
        }
        let mut canvas = Canvas {
            gfx: self,
            size: (w as f32, h as f32),
        };
        paint(&mut canvas);
        self.last_present_ms = 0.0;
        // SAFETY: matches the BeginDraw above on the same target.
        let mut result = unsafe { target.EndDraw(None, None) };
        if result.is_ok() {
            if let Some(swap) = self
                .swap
                .as_ref()
                .filter(|_| std::env::var("PHOTOREVIEW_RS_SPIKE_BACKEND").as_deref() != Ok("offscreen"))
            {
                let interval = u32::from(std::env::var_os("PHOTOREVIEW_RS_SPIKE_VSYNC").is_some());
                let t0 = std::time::Instant::now();
                // SAFETY: presents the back buffer of the swap chain owned by this struct.
                let hr = unsafe { swap.Present(interval, DXGI_PRESENT(0)) };
                self.last_present_ms = t0.elapsed().as_secs_f64() * 1000.0;
                if hr.is_err() {
                    result = Err(windows::core::Error::from(hr));
                }
            }
        }
        if result.is_err() {
            self.drop_target();
        }
    }
}

impl Ctx<'_> {
    /// Client size in physical pixels.
    #[must_use]
    pub fn client_size(&self) -> (f32, f32) {
        let (w, h) = client_size_of(self.gfx.hwnd);
        (w as f32, h as f32)
    }

    /// How long the last `Present` call took (it can block when the compositor throttles an occluded or idle display).
    #[must_use]
    pub fn last_present_ms(&self) -> f64 {
        self.gfx.last_present_ms
    }

    /// Marks the window dirty; `WM_PAINT` follows once the message queue is empty.
    pub fn invalidate(&mut self) {
        // SAFETY: valid HWND of this thread's window; a null rect invalidates the whole client area.
        let _ = unsafe { InvalidateRect(Some(self.gfx.hwnd), None, false) };
    }

    /// Runs `f` with a canvas outside `BeginDraw`/`EndDraw`, to upload bitmaps ahead of the frame that needs them.
    pub fn prepare(&mut self, f: impl FnOnce(&mut Canvas<'_>)) {
        if self.gfx.ensure_target().is_err() {
            return;
        }
        let (w, h) = client_size_of(self.gfx.hwnd);
        f(&mut Canvas {
            gfx: self.gfx,
            size: (w as f32, h as f32),
        });
    }

    /// Paints synchronously (used by the measurement to time key -> frame).
    pub fn render_now(&mut self, paint: impl FnOnce(&mut Canvas<'_>)) {
        self.gfx.render(paint);
    }

    /// Arms (or re-arms) the single app timer; `0` cancels it.
    pub fn set_timer(&mut self, millis: u32) {
        // SAFETY: valid HWND of this thread; timer id is private to this window.
        unsafe {
            if millis == 0 {
                let _ = KillTimer(Some(self.gfx.hwnd), APP_TIMER_ID);
            } else {
                SetTimer(Some(self.gfx.hwnd), APP_TIMER_ID, millis, None);
            }
        }
    }

    /// Sets the window title.
    pub fn set_title(&mut self, title: &str) {
        let wide = to_wide(title);
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        let _ = unsafe { SetWindowTextW(self.gfx.hwnd, PCWSTR(wide.as_ptr())) };
    }

    /// Ends the message loop.
    pub fn quit(&mut self) {
        self.gfx.quit = true;
        // SAFETY: valid HWND of this thread; WM_CLOSE leads to DestroyWindow and PostQuitMessage in the window proc.
        let _ = unsafe { PostMessageW(Some(self.gfx.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }

    /// Whether [`Self::quit`] was requested.
    #[must_use]
    pub fn quitting(&self) -> bool {
        self.gfx.quit
    }
}

impl Canvas<'_> {
    /// Client size in pixels.
    #[must_use]
    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    /// Whether a bitmap is already uploaded under `key`.
    #[must_use]
    pub fn has_bitmap(&self, key: u64) -> bool {
        self.gfx.bitmaps.contains_key(&key)
    }

    /// Uploads BGRA32 pixels as a Direct2D bitmap (no-op when `key` exists). `opaque` = the 4th byte is unused.
    ///
    /// # Errors
    /// The Direct2D error text.
    pub fn upload(
        &mut self,
        key: u64,
        width: u32,
        height: u32,
        stride: u32,
        bgra: &[u8],
        opaque: bool,
    ) -> Result<(), String> {
        if self.gfx.bitmaps.contains_key(&key) {
            return Ok(());
        }
        let needed = u64::from(stride) * u64::from(height);
        if (bgra.len() as u64) < needed || stride < width.saturating_mul(4) {
            return Err("pixel buffer smaller than stride x height".to_owned());
        }
        let target = self.gfx.target.as_ref().ok_or("no render target")?;
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: if opaque {
                    D2D1_ALPHA_MODE_IGNORE
                } else {
                    D2D1_ALPHA_MODE_PREMULTIPLIED
                },
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        // SAFETY: `bgra` holds at least stride * height bytes (checked above) and stays alive for the call, which copies
        // the data into the bitmap before returning.
        let bitmap = unsafe {
            target.CreateBitmap(
                D2D_SIZE_U { width, height },
                Some(bgra.as_ptr().cast::<c_void>()),
                stride,
                &props,
            )
        }
        .map_err(|e| e.to_string())?;
        self.gfx.bitmaps.insert(key, bitmap);
        Ok(())
    }

    /// Forces the GPU upload of bitmap `key` now by drawing it, downscaled, into a tiny off-screen target (nothing
    /// reaches the window). Moves the first-use cost of a big bitmap off the key -> frame path.
    pub fn warm(&mut self, key: u64) {
        let (Some(target), Some(bitmap)) = (self.gfx.target.as_ref(), self.gfx.bitmaps.get(&key)) else {
            return;
        };
        let size = D2D_SIZE_F {
            width: 64.0,
            height: 36.0,
        };
        // SAFETY: the size struct outlives the call; the off-screen target is used and dropped inside this block, and
        // BeginDraw/EndDraw are paired on it.
        unsafe {
            let Ok(offscreen) = target.CreateCompatibleRenderTarget(
                Some(&size),
                None,
                None,
                D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
            ) else {
                return;
            };
            offscreen.BeginDraw();
            let dest = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: 64.0,
                bottom: 36.0,
            };
            offscreen.DrawBitmap(bitmap, Some(&dest), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            let _ = offscreen.EndDraw(None, None);
        }
    }

    /// Releases every uploaded bitmap whose key is not in `keep`.
    pub fn retain_bitmaps(&mut self, keep: &[u64]) {
        self.gfx.bitmaps.retain(|k, _| keep.contains(k));
    }

    /// Draws `src` (source pixels `[left, top, right, bottom]`) of bitmap `key` into `dest` (client pixels).
    /// `nearest` = no smoothing (pixel zoom above 100%).
    pub fn draw_bitmap(&mut self, key: u64, dest: [f32; 4], src: [f32; 4], nearest: bool) {
        let (Some(target), Some(bitmap)) = (self.gfx.target.as_ref(), self.gfx.bitmaps.get(&key)) else {
            return;
        };
        let d = D2D_RECT_F {
            left: dest[0],
            top: dest[1],
            right: dest[2],
            bottom: dest[3],
        };
        let s = D2D_RECT_F {
            left: src[0],
            top: src[1],
            right: src[2],
            bottom: src[3],
        };
        let mode = if nearest {
            D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR
        } else {
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR
        };
        // SAFETY: both rects are locals that outlive the call; the bitmap belongs to this render target.
        unsafe { target.DrawBitmap(bitmap, Some(&d), 1.0, mode, Some(&s)) };
    }

    /// DirectWrite text (UTF-8 in, UTF-16 to DirectWrite) in a box at `(x, y)`.
    pub fn draw_text(&mut self, text: &str, x: f32, y: f32, width: f32, height: f32) {
        let (Some(target), Some(brush)) = (self.gfx.target.as_ref(), self.gfx.brush.as_ref()) else {
            return;
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        let rect = D2D_RECT_F {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        };
        // SAFETY: `wide`, `rect`, the format and the brush outlive the call; DrawText copies what it needs.
        unsafe {
            target.DrawText(
                &wide,
                &self.gfx.text_format,
                &rect,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn lparam_xy(lparam: LPARAM) -> (i32, i32) {
    // Low and high words are SIGNED 16-bit client coordinates.
    let raw = lparam.0 as usize;
    (
        i32::from((raw & 0xFFFF) as u16 as i16),
        i32::from(((raw >> 16) & 0xFFFF) as u16 as i16),
    )
}

/// Runs `f` with the state behind the window's `GWLP_USERDATA` pointer.
fn with_state<R>(hwnd: HWND, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    // SAFETY: GWLP_USERDATA holds either 0 or the `*mut State` leaked by `run`, which stays valid until the loop ends
    // and is only touched on this (the window's) thread.
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    if ptr.is_null() {
        return None;
    }
    // SAFETY: see above; callbacks never re-enter the window procedure while holding this reference (no message is
    // pumped inside them), so there is no second `&mut State`.
    Some(f(unsafe { &mut *ptr }))
}

fn dispatch(hwnd: HWND, f: impl FnOnce(&mut dyn SpikeApp, &mut Ctx<'_>)) {
    with_state(hwnd, |state| {
        if let (Some(gfx), Some(app)) = (state.gfx.as_mut(), state.app.as_mut()) {
            f(app.as_mut(), &mut Ctx { gfx });
        }
    });
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            // SAFETY: for WM_NCCREATE `lparam` points to the CREATESTRUCTW that CreateWindowExW is filling in.
            let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
            // SAFETY: stores the `*mut State` passed as the creation parameter; read back by `with_state`.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize) };
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: `ps` is a valid out struct; paired with EndPaint below.
            unsafe { BeginPaint(hwnd, &mut ps) };
            with_state(hwnd, |state| {
                if let (Some(gfx), Some(app)) = (state.gfx.as_mut(), state.app.as_mut()) {
                    gfx.render(|canvas| app.paint(canvas));
                    app.after_paint();
                }
            });
            // SAFETY: matches BeginPaint with the same struct.
            let _ = unsafe { EndPaint(hwnd, &ps) };
            return LRESULT(0);
        }
        WM_SIZE => {
            with_state(hwnd, |state| {
                if let Some(gfx) = state.gfx.as_mut() {
                    let (w, h) = client_size_of(hwnd);
                    gfx.resize(w, h);
                }
            });
            dispatch(hwnd, |app, ctx| {
                app.on_input(Input::Resized, ctx);
                ctx.invalidate();
            });
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            let key = match wparam.0 as u16 {
                x if x == VK_LEFT.0 => Some(Key::Left),
                x if x == VK_RIGHT.0 => Some(Key::Right),
                x if x == VK_ESCAPE.0 => Some(Key::Escape),
                0x46 => Some(Key::Fit),    // 'F'
                0x31 => Some(Key::Actual), // '1'
                _ => None,
            };
            if let Some(key) = key {
                dispatch(hwnd, |app, ctx| app.on_input(Input::Key(key), ctx));
                return LRESULT(0);
            }
        }
        WM_MOUSEWHEEL => {
            // High word of wparam = signed wheel delta (120 per notch); lparam = SCREEN coordinates.
            let delta = i32::from((wparam.0 >> 16) as u16 as i16);
            let (sx, sy) = lparam_xy(lparam);
            let mut pt = POINT { x: sx, y: sy };
            // SAFETY: `pt` is a valid in/out point for this window.
            let _ = unsafe { ScreenToClient(hwnd, &mut pt) };
            dispatch(hwnd, |app, ctx| {
                app.on_input(
                    Input::Wheel {
                        notches: delta as f32 / 120.0,
                        x: pt.x as f32,
                        y: pt.y as f32,
                    },
                    ctx,
                );
            });
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let xy = lparam_xy(lparam);
            with_state(hwnd, |s| s.drag_last = Some(xy));
            // SAFETY: capture the mouse for this window so the drag continues outside it; released on button up.
            unsafe { SetCapture(hwnd) };
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let (x, y) = lparam_xy(lparam);
            let last = with_state(hwnd, |s| {
                let last = s.drag_last;
                if last.is_some() {
                    s.drag_last = Some((x, y));
                }
                last
            })
            .flatten();
            if let Some((lx, ly)) = last {
                dispatch(hwnd, |app, ctx| {
                    app.on_input(
                        Input::Drag {
                            dx: (x - lx) as f32,
                            dy: (y - ly) as f32,
                        },
                        ctx,
                    );
                });
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            with_state(hwnd, |s| s.drag_last = None);
            // SAFETY: releases the capture taken on button down.
            let _ = unsafe { ReleaseCapture() };
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == APP_TIMER_ID => {
            dispatch(hwnd, |app, ctx| app.on_timer(ctx));
            return LRESULT(0);
        }
        WM_WAKE => {
            dispatch(hwnd, |app, ctx| app.on_wake(ctx));
            return LRESULT(0);
        }
        WM_CLOSE => {
            // SAFETY: destroys this thread's own window; WM_DESTROY then ends the loop.
            let _ = unsafe { DestroyWindow(hwnd) };
            return LRESULT(0);
        }
        WM_DESTROY => {
            // SAFETY: posts WM_QUIT to this thread's queue.
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        _ => {}
    }
    // SAFETY: default processing of a message this procedure received with its own arguments.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Creates the window, builds the app with a [`WakeHandle`], and runs the message loop until the window closes.
///
/// # Errors
/// A text describing the failing Win32/Direct2D call.
pub fn run(config: &WindowConfig, make_app: impl FnOnce(WakeHandle) -> Box<dyn SpikeApp>) -> Result<(), String> {
    // SAFETY: first call on this thread; a repeated call with the same awareness returns an error that is irrelevant.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let class_name = w!("PhotoReviewRsSpike");
    if !CLASS_REGISTERED.swap(true, Ordering::SeqCst) {
        // SAFETY: standard cursor lookup with a predefined id.
        let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hCursor: cursor,
            lpszClassName: class_name,
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised and the class name is a static wide string.
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err("RegisterClassW failed".to_owned());
        }
    }

    // SAFETY: plain factory creation with no shared state.
    let factory: ID2D1Factory1 = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
        .map_err(|e| format!("D2D1CreateFactory: {e}"))?;
    // SAFETY: plain factory creation.
    let dwrite: IDWriteFactory =
        unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.map_err(|e| format!("DWriteCreateFactory: {e}"))?;
    // SAFETY: static wide strings; the format object owns copies.
    let text_format = unsafe {
        dwrite.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            16.0,
            w!("vi-VN"),
        )
    }
    .map_err(|e| format!("CreateTextFormat: {e}"))?;

    let state = Box::into_raw(Box::new(State {
        gfx: None,
        app: None,
        drag_last: None,
    }));

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: config.client_width,
        bottom: config.client_height,
    };
    // SAFETY: `rect` is a valid in/out struct.
    let _ = unsafe { AdjustWindowRectEx(&mut rect, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0)) };
    let title = to_wide(&config.title);
    // SAFETY: class registered above; `state` is passed as the creation parameter and stays alive until freed below,
    // after the loop has ended and the window is destroyed.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            None,
            Some(state.cast::<c_void>()),
        )
    };
    let hwnd = match hwnd {
        Ok(h) => h,
        Err(e) => {
            // SAFETY: reclaims the box leaked above; no window refers to it.
            drop(unsafe { Box::from_raw(state) });
            return Err(format!("CreateWindowExW: {e}"));
        }
    };

    // SAFETY: `state` is valid and only this thread touches it; no callback is running.
    unsafe {
        (*state).gfx = Some(Gfx {
            hwnd,
            factory,
            _dwrite: dwrite,
            text_format,
            target: None,
            hwnd_rt: None,
            dc: None,
            swap: None,
            brush: None,
            bitmaps: HashMap::new(),
            quit: false,
            last_present_ms: 0.0,
        });
        (*state).app = Some(make_app(WakeHandle(hwnd.0 as isize)));
    }
    dispatch(hwnd, |app, ctx| app.on_create(ctx));

    // SAFETY: valid HWND of this thread.
    unsafe {
        let _ = ShowWindow(hwnd, if config.no_activate { SW_SHOWNA } else { SW_SHOW });
    }
    dispatch(hwnd, |_, ctx| ctx.invalidate());

    let mut msg = MSG::default();
    // SAFETY: standard message loop; `msg` is a valid out struct, GetMessageW blocks while there is nothing to do.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // SAFETY: the window is destroyed (the loop ended on WM_QUIT), so nothing refers to `state` any more.
    drop(unsafe { Box::from_raw(state) });
    Ok(())
}

/// Total CPU time (user + kernel) used by this process so far, in 100 ns units.
#[must_use]
pub fn process_cpu_time_100ns() -> u64 {
    let (mut creation, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: the pseudo handle from GetCurrentProcess is always valid; the four out structs are locals.
    let ok = unsafe { GetProcessTimes(GetCurrentProcess(), &mut creation, &mut exit, &mut kernel, &mut user) };
    if ok.is_err() {
        return 0;
    }
    let to_u64 = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    to_u64(kernel) + to_u64(user)
}

const CLSID_WIC_IMAGING_FACTORY: GUID = GUID::from_u128(0xcacaf262_9370_4615_a13b_9f5539da4c0a);

/// Writes a synthetic photo-like JPEG (smooth gradient plus noise so the file is not tiny) for the measurement.
/// Deterministic for a given `seed`.
///
/// # Errors
/// A text describing the failing WIC call.
pub fn write_test_jpeg(path: &Path, width: u32, height: u32, seed: u32) -> Result<(), String> {
    use windows::Win32::Foundation::GENERIC_WRITE;
    use windows::Win32::Graphics::Imaging::{
        GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, IWICImagingFactory, WICBitmapEncoderNoCache,
    };

    let wide = to_wide(&path.to_string_lossy());
    let err = |what: &str, e: windows::core::Error| format!("{what}: {e}");
    // SAFETY: COM calls in sequence on interface pointers owned by this function (released on drop). The row buffer
    // outlives each WritePixels call, which copies it. `wide` is NUL-terminated.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WIC_IMAGING_FACTORY, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| err("WIC factory", e))?;
        let stream = factory.CreateStream().map_err(|e| err("CreateStream", e))?;
        stream
            .InitializeFromFilename(PCWSTR(wide.as_ptr()), GENERIC_WRITE.0)
            .map_err(|e| err("InitializeFromFilename", e))?;
        let encoder = factory
            .CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())
            .map_err(|e| err("CreateEncoder", e))?;
        encoder
            .Initialize(&stream, WICBitmapEncoderNoCache)
            .map_err(|e| err("Encoder.Initialize", e))?;
        let mut frame = None;
        encoder
            .CreateNewFrame(&mut frame, std::ptr::null_mut())
            .map_err(|e| err("CreateNewFrame", e))?;
        let frame = frame.ok_or("CreateNewFrame returned no frame")?;
        frame.Initialize(None).map_err(|e| err("Frame.Initialize", e))?;
        frame.SetSize(width, height).map_err(|e| err("SetSize", e))?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        frame
            .SetPixelFormat(&mut format)
            .map_err(|e| err("SetPixelFormat", e))?;

        let stride = width as usize * 3;
        let mut row = vec![0u8; stride];
        let mut rng = seed.wrapping_mul(2_654_435_761).wrapping_add(0x9E37_79B9) | 1;
        for y in 0..height {
            for x in 0..width {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let noise = (rng & 0x1F) as i32 - 16;
                let base = [
                    (x * 255 / width.max(1)) as i32,
                    (y * 255 / height.max(1)) as i32,
                    (((x + y) * 127 / (width + height).max(1)) + (seed % 100)) as i32,
                ];
                let o = x as usize * 3;
                for c in 0..3 {
                    row[o + c] = (base[c] + noise).clamp(0, 255) as u8;
                }
            }
            frame
                .WritePixels(1, stride as u32, &row)
                .map_err(|e| err("WritePixels", e))?;
        }
        frame.Commit().map_err(|e| err("Frame.Commit", e))?;
        encoder.Commit().map_err(|e| err("Encoder.Commit", e))?;
    }
    Ok(())
}
