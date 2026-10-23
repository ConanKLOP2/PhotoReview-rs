//! Port of `PhotoReview.Shell.Rendering/D2DRenderSurface.cs` (236 lines, WP-15 / C-09) plus the 13-line `D2DRenderSurfaceFactory.cs`,
//! baseline `d92d6aae`. Unit W8-12d. Contract: `contracts/W8-12d.md`.
//!
//! One Direct2D 1.1 render surface on D3D11 (BGRA). Window: flip-model `FLIP_DISCARD` swap chain, 2 buffers, frame-latency waitable
//! object, `SetMaximumFrameLatency(max_frame_latency)` (default 1). Offscreen: premultiplied target bitmap on WARP (tests, G-PIX).
//! A device loss in `resize`, `end_draw_and_present` (`EndDraw` or `Present`) recreates the whole device stack, calls the subscribed
//! handlers and reports `PresentResult::DeviceRecreated` (the caller redraws the frame).
//!
//! Frame protocol: `begin_draw_context` borrows the surface mutably and returns the draw context, so the surface cannot be touched while
//! the context is alive. The context reference can still be dropped WITHOUT `end_draw_and_present`; the frame stays open (`drawing`), and
//! the C# runtime checks stay: a second `begin_draw_context` and `resize` while `drawing` are `InvalidOperation`, and
//! `end_draw_and_present` without a frame is `InvalidOperation`.
//!
//! Thread affinity: UI thread only; `!Send` (`PhantomData<*const ()>`), so the C# owner-thread check has no Rust twin.
//!
//! Types (the layering PR landed, D-W8B3R-1): [`SurfaceOptions`] is `pr_app::services::render::RenderSurfaceOptions` and
//! [`PresentResult`] / [`DeviceRecreatedHandlerId`] are the `pr_app` types, re-exported under the contract's names. The inherent methods
//! keep the contract's signatures (`RenderFault`, `D2dDrawContext`); `impl RenderSurface` / `RenderSurfaceFactory` at the end of the
//! file are the adapter to the `pr_app` traits (`RenderError`, `dyn DrawContext`).

use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::{Duration, Instant};

use pr_app::services::render::{DrawContext, RenderError, RenderSurface, RenderSurfaceFactory};
use windows::Win32::Foundation::HANDLE;

pub use pr_app::services::render::surface::DeviceRecreatedHandlerId;
pub use pr_app::services::render::{PresentResult, RenderSurfaceOptions as SurfaceOptions};

use super::device::loss::{FrameEnd, classify_end_draw};
use super::device::resources::ResizeOutcome;
use super::device::{DeviceResources, RenderFault};
use super::draw_context::{D2dDrawContext, render_error};
use crate::gfx::com::{self, GfxError};
use crate::gfx::dxgi::{self, PresentOutcome};
use crate::gfx::extras;

/// `ObjectDisposedException.ObjectName` of the surface (`ThrowIfInvalidThreadOrDisposed`).
const OBJECT_NAME: &str = "PhotoReview.Shell.Rendering.D2DRenderSurface";

/// `AllowTearing || ImmediatePresent ? 0u : 1u` (C# `Present`, line 212; pure): the sync interval handed to `IDXGISwapChain::Present`.
#[must_use]
pub fn sync_interval(allow_tearing: bool, immediate_present: bool) -> u32 {
    if allow_tearing || immediate_present { 0 } else { 1 }
}

/// Direct2D render surface (C# `D2DRenderSurface`). `Drop` is C# `Dispose` (`DeviceResources::dispose`).
pub struct D2dRenderSurface {
    resources: Rc<DeviceResources>,
    options: SurfaceOptions,
    draw_context: D2dDrawContext,
    drawing: bool,
    disposed: bool,
    immediate_present: bool,
    handlers: Vec<(DeviceRecreatedHandlerId, Box<dyn FnMut()>)>,
    next_handler_id: DeviceRecreatedHandlerId,
    last_end_draw: Duration,
    last_present: Duration,
    _not_send: PhantomData<*const ()>,
}

impl fmt::Debug for D2dRenderSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("D2dRenderSurface")
            .field("drawing", &self.drawing)
            .finish_non_exhaustive()
    }
}

/// `ThrowIfInvalidThreadOrDisposed`, the disposed half (the thread half has no Rust twin: the type is `!Send`).
fn surface_disposed() -> RenderFault {
    RenderFault::Disposed { object: OBJECT_NAME }
}

impl D2dRenderSurface {
    /// The private C# constructor: one draw context for the life of the surface.
    fn new(resources: Rc<DeviceResources>, options: SurfaceOptions) -> Self {
        let draw_context = D2dDrawContext::new(Rc::clone(&resources));
        Self {
            resources,
            options,
            draw_context,
            drawing: false,
            disposed: false,
            immediate_present: false,
            handlers: Vec::new(),
            next_handler_id: 1,
            last_end_draw: Duration::ZERO,
            last_present: Duration::ZERO,
            _not_send: PhantomData,
        }
    }

    fn ensure_not_disposed(&self) -> Result<(), RenderFault> {
        if self.disposed { Err(surface_disposed()) } else { Ok(()) }
    }

    /// C# `CreateWindowSurface(hwnd, options)`: `DeviceResources::create_for_window` then the surface. `hwnd == 0` is
    /// `Err(InvalidArgument { param: "hwnd", message: "A window handle is required." })`.
    pub fn create_window_surface(hwnd: isize, options: SurfaceOptions) -> Result<Self, RenderFault> {
        let resources = DeviceResources::create_for_window(hwnd, options.prefer_warp, options.max_frame_latency)?;
        Ok(Self::new(resources, options))
    }

    /// C# `CreateOffscreenSurface(w, h, dpi, useHardware)`: WARP unless `use_hardware`. A size below 1 is `ArgumentOutOfRange`.
    pub fn create_offscreen_surface(
        pixel_width: i32,
        pixel_height: i32,
        dpi_scale: f64,
        use_hardware: bool,
    ) -> Result<Self, RenderFault> {
        let resources = DeviceResources::create_offscreen(pixel_width, pixel_height, dpi_scale, !use_hardware)?;
        let options = SurfaceOptions {
            prefer_warp: !use_hardware,
            allow_tearing: false,
            max_frame_latency: 1,
        };
        Ok(Self::new(resources, options))
    }

    /// C# `Hwnd` (0 offscreen).
    #[must_use]
    pub fn hwnd(&self) -> isize {
        self.resources.hwnd()
    }

    /// C# `PixelWidth`.
    #[must_use]
    pub fn pixel_width(&self) -> i32 {
        self.resources.pixel_width()
    }

    /// C# `PixelHeight`.
    #[must_use]
    pub fn pixel_height(&self) -> i32 {
        self.resources.pixel_height()
    }

    /// C# `DpiScale`.
    #[must_use]
    pub fn dpi_scale(&self) -> f64 {
        self.resources.dpi_scale()
    }

    /// C# `FrameLatencyWaitHandle` (raw handle value, 0 offscreen). Owned by the swap chain; never close it.
    #[must_use]
    pub fn frame_latency_wait_handle(&self) -> isize {
        self.resources.frame_latency_wait_handle()
    }

    /// C# `IsWarp`.
    #[must_use]
    pub fn is_warp(&self) -> bool {
        self.resources.is_warp()
    }

    /// C# `AdapterDescription` (diagnostics; empty offscreen).
    #[must_use]
    pub fn adapter_description(&self) -> String {
        self.resources.adapter_description()
    }

    /// C# `DeviceGeneration`.
    #[must_use]
    pub fn device_generation(&self) -> i64 {
        self.resources.generation()
    }

    /// C# `MaxBitmapSize`.
    #[must_use]
    pub fn max_bitmap_size(&self) -> i32 {
        self.resources.max_bitmap_size()
    }

    /// C# `ImmediatePresent` getter (WP-20: sync interval 0 until the first image frame is presented).
    #[must_use]
    pub fn immediate_present(&self) -> bool {
        self.immediate_present
    }

    /// C# `ImmediatePresent` setter.
    pub fn set_immediate_present(&mut self, value: bool) {
        self.immediate_present = value;
    }

    /// C# `Resources` (the same object across every recreate; the GPU image cache keeps a clone to create and release bitmaps).
    #[must_use]
    pub fn resources(&self) -> &Rc<DeviceResources> {
        &self.resources
    }

    /// C# `LastEndDrawTicks` as a duration (WP-21 diagnostics: time inside `ID2D1DeviceContext::EndDraw` of the last frame).
    #[must_use]
    pub fn last_end_draw(&self) -> Duration {
        self.last_end_draw
    }

    /// C# `LastPresentTicks` as a duration (`Duration::ZERO` when the last frame had no `Present` call).
    #[must_use]
    pub fn last_present(&self) -> Duration {
        self.last_present
    }

    /// C# `Resize`: `Err(Disposed)` after dispose; `DeviceResources::resize`; `BuffersFailed(hr)` with a device-loss `hr` is
    /// `recreate_device`, any other `hr < 0` is `Err(Device { hresult, operation: "IDXGISwapChain::ResizeBuffers" })`. While a frame is
    /// open it is `Err(InvalidOperation "Resize is not allowed between BeginDraw and EndDrawAndPresent.")`, checked before the
    /// device is touched.
    pub fn resize(&mut self, pixel_width: i32, pixel_height: i32, dpi_scale: f64) -> Result<(), RenderFault> {
        self.ensure_not_disposed()?;
        if self.drawing {
            return Err(RenderFault::InvalidOperation {
                message: "Resize is not allowed between BeginDraw and EndDrawAndPresent.",
            });
        }
        match self.resources.resize(pixel_width, pixel_height, dpi_scale)? {
            ResizeOutcome::Resized => Ok(()),
            // The size and DPI were stored before `ResizeBuffers` ran, so the new device is built at the NEW size.
            ResizeOutcome::BuffersFailed(hresult) if com::is_device_lost(hresult) => self.recreate_device(),
            ResizeOutcome::BuffersFailed(hresult) => Err(RenderFault::Device(GfxError::new(
                "IDXGISwapChain::ResizeBuffers",
                hresult,
            ))),
        }
    }

    /// C# `BeginDrawContext`: `Err(Disposed)`; sets `drawing`, `ID2D1DeviceContext::BeginDraw`, `D2dDrawContext::begin`; returns the
    /// context (it borrows the surface). A second call while a frame is open (the first reference was dropped without
    /// `end_draw_and_present`) is `Err(InvalidOperation "BeginDraw was already called; call EndDrawAndPresent first.")`.
    pub fn begin_draw_context(&mut self) -> Result<&mut D2dDrawContext, RenderFault> {
        self.ensure_not_disposed()?;
        if self.drawing {
            return Err(RenderFault::InvalidOperation {
                message: "BeginDraw was already called; call EndDrawAndPresent first.",
            });
        }
        // Fetched before `drawing` is set: if the device failed to rebuild, the surface does not get stuck in a frame.
        let context = self.resources.context()?;
        self.drawing = true;
        // SAFETY: the context is owned by the shared DeviceResources and alive; the previous frame was ended (drawing == false);
        // the target is attached.
        unsafe { context.BeginDraw() };
        self.draw_context.begin();
        Ok(&mut self.draw_context)
    }

    /// C# `EndDrawAndPresent`. Order: `drawing = false`; `D2dDrawContext::end` (pops dangling clips, returns `balanced`); time
    /// `DeviceResources::end_draw`; `classify_end_draw`: `DeviceLost` is `recreate_device` and `DeviceRecreated`, `Failed` is
    /// `Ok(Failed)`, `Continue` is `present`. THEN, if the stacks were not balanced,
    /// `Err(InvalidOperation "Unbalanced PushClip/PopClip or PushOpacity/PopOpacity in the frame.")` (the frame was still shown and
    /// the surface is usable). Without `begin_draw_context`: `Err(InvalidOperation "EndDrawAndPresent without BeginDraw.")`.
    pub fn end_draw_and_present(&mut self) -> Result<PresentResult, RenderFault> {
        self.ensure_not_disposed()?;
        if !self.drawing {
            return Err(RenderFault::InvalidOperation {
                message: "EndDrawAndPresent without BeginDraw.",
            });
        }
        self.drawing = false;
        let balanced = self.draw_context.end();
        let started = Instant::now();
        let hresult = self.resources.end_draw();
        self.last_end_draw = started.elapsed();
        let result = match classify_end_draw(hresult?) {
            FrameEnd::DeviceLost => {
                self.recreate_device()?;
                PresentResult::DeviceRecreated
            }
            FrameEnd::Failed => PresentResult::Failed,
            FrameEnd::Continue => self.present()?,
        };
        if !balanced {
            return Err(RenderFault::InvalidOperation {
                message: "Unbalanced PushClip/PopClip or PushOpacity/PopOpacity in the frame.",
            });
        }
        Ok(result)
    }

    /// C# `Present` (private): no swap chain gives `Presented` (and `last_present` zero); sync interval is 0 when `allow_tearing` or
    /// `immediate_present`, else 1; `gfx::dxgi::classify_present` maps `Occluded`, `DeviceLost` (recreate, `DeviceRecreated`),
    /// `Failed`, `Presented`. `Err` only when the device could not be rebuilt after a loss.
    fn present(&mut self) -> Result<PresentResult, RenderFault> {
        self.last_present = Duration::ZERO;
        if !self.resources.has_swap_chain() {
            return Ok(PresentResult::Presented);
        }
        // `DXGI_PRESENT_ALLOW_TEARING` is not used (C# line 210: needs `IDXGIFactory5`, "v1"): `allow_tearing` only means sync 0.
        let started = Instant::now();
        let hresult = self
            .resources
            .present(sync_interval(self.options.allow_tearing, self.immediate_present));
        self.last_present = started.elapsed();
        match dxgi::classify_present(hresult) {
            PresentOutcome::Occluded => Ok(PresentResult::Occluded),
            PresentOutcome::DeviceLost(_) => {
                self.recreate_device()?;
                Ok(PresentResult::DeviceRecreated)
            }
            PresentOutcome::Failed(_) => Ok(PresentResult::Failed),
            PresentOutcome::Presented => Ok(PresentResult::Presented),
        }
    }

    /// C# `WaitForNextFrame(timeoutMilliseconds)`: offscreen is `Ok(true)`; else `WaitForSingleObjectEx(handle, max(0, timeout),
    /// alertable = true)` through `gfx::extras::wait_for_handle`; true only for `WAIT_OBJECT_0`.
    pub fn wait_for_next_frame(&self, timeout_milliseconds: i32) -> Result<bool, RenderFault> {
        self.ensure_not_disposed()?;
        let handle = self.resources.frame_latency_wait_handle();
        if handle == 0 {
            return Ok(true);
        }
        let timeout = u32::try_from(timeout_milliseconds.max(0)).unwrap_or(0);
        // The raw value was read from the swap chain's own waitable object just now; the wait only reads it.
        let handle = HANDLE(handle as *mut core::ffi::c_void);
        Ok(extras::wait_for_handle(handle, timeout, true).is_signaled())
    }

    /// C# `WaitForGpu`: `DeviceResources::synchronize`.
    pub fn wait_for_gpu(&self) -> Result<(), RenderFault> {
        self.ensure_not_disposed()?;
        self.resources.synchronize()
    }

    /// C# `RecreateDevice`: `DeviceResources::recreate` then every subscribed handler in subscription order (synchronously).
    pub fn recreate_device(&mut self) -> Result<(), RenderFault> {
        self.resources.recreate()?;
        for (_, handler) in &mut self.handlers {
            handler();
        }
        Ok(())
    }

    /// C# `DeviceRecreated +=`. Ids are never reused.
    pub fn subscribe_device_recreated(&mut self, handler: Box<dyn FnMut()>) -> DeviceRecreatedHandlerId {
        let id = self.next_handler_id;
        self.next_handler_id += 1;
        self.handlers.push((id, handler));
        id
    }

    /// C# `DeviceRecreated -=`; an unknown id is a no-op.
    pub fn unsubscribe_device_recreated(&mut self, id: DeviceRecreatedHandlerId) {
        self.handlers.retain(|(existing, _)| *existing != id);
    }

    /// C# `Dispose`: idempotent; marks the surface disposed and calls `DeviceResources::dispose` (the cache's clone of the `Rc` stays
    /// valid but every device getter then gives `Err(Disposed)`).
    pub fn dispose(&mut self) {
        if self.disposed {
            return;
        }
        self.disposed = true;
        self.resources.dispose();
    }
}

impl Drop for D2dRenderSurface {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl RenderSurface for D2dRenderSurface {
    fn hwnd(&self) -> isize {
        D2dRenderSurface::hwnd(self)
    }

    fn pixel_width(&self) -> i32 {
        D2dRenderSurface::pixel_width(self)
    }

    fn pixel_height(&self) -> i32 {
        D2dRenderSurface::pixel_height(self)
    }

    fn dpi_scale(&self) -> f64 {
        D2dRenderSurface::dpi_scale(self)
    }

    fn frame_latency_wait_handle(&self) -> isize {
        D2dRenderSurface::frame_latency_wait_handle(self)
    }

    fn is_warp(&self) -> bool {
        D2dRenderSurface::is_warp(self)
    }

    fn resize(&mut self, pixel_width: i32, pixel_height: i32, dpi_scale: f64) -> Result<(), RenderError> {
        D2dRenderSurface::resize(self, pixel_width, pixel_height, dpi_scale).map_err(render_error)
    }

    fn begin_draw(&mut self) -> Result<&mut dyn DrawContext, RenderError> {
        let context = self.begin_draw_context().map_err(render_error)?;
        Ok(context)
    }

    fn end_draw_and_present(&mut self) -> Result<PresentResult, RenderError> {
        D2dRenderSurface::end_draw_and_present(self).map_err(render_error)
    }

    fn subscribe_device_recreated(&mut self, handler: Box<dyn FnMut()>) -> DeviceRecreatedHandlerId {
        D2dRenderSurface::subscribe_device_recreated(self, handler)
    }

    fn unsubscribe_device_recreated(&mut self, id: DeviceRecreatedHandlerId) {
        D2dRenderSurface::unsubscribe_device_recreated(self, id);
    }
}

/// C# `D2DRenderSurfaceFactory` (13 lines, `IRenderSurfaceFactory`): stateless.
#[derive(Debug, Clone, Copy, Default)]
pub struct D2dRenderSurfaceFactory;

impl D2dRenderSurfaceFactory {
    /// C# `CreateForWindow` = `D2DRenderSurface.CreateWindowSurface`.
    pub fn create_for_window(&self, hwnd: isize, options: SurfaceOptions) -> Result<D2dRenderSurface, RenderFault> {
        D2dRenderSurface::create_window_surface(hwnd, options)
    }

    /// C# `CreateOffscreen` = `CreateOffscreenSurface(w, h, dpi)` (WARP).
    pub fn create_offscreen(
        &self,
        pixel_width: i32,
        pixel_height: i32,
        dpi_scale: f64,
    ) -> Result<D2dRenderSurface, RenderFault> {
        D2dRenderSurface::create_offscreen_surface(pixel_width, pixel_height, dpi_scale, false)
    }
}

impl RenderSurfaceFactory for D2dRenderSurfaceFactory {
    fn create_for_window(&self, hwnd: isize, options: SurfaceOptions) -> Result<Box<dyn RenderSurface>, RenderError> {
        let surface = D2dRenderSurfaceFactory::create_for_window(self, hwnd, options).map_err(render_error)?;
        Ok(Box::new(surface))
    }

    fn create_offscreen(
        &self,
        pixel_width: i32,
        pixel_height: i32,
        dpi_scale: f64,
    ) -> Result<Box<dyn RenderSurface>, RenderError> {
        let surface = D2dRenderSurfaceFactory::create_offscreen(self, pixel_width, pixel_height, dpi_scale)
            .map_err(render_error)?;
        Ok(Box::new(surface))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use pr_app::viewport::render_geometry::ImageInterpolation;
    use windows::Win32::Graphics::Dxgi::DXGI_ERROR_INVALID_CALL;

    use super::*;
    use crate::gfx::com::{
        D2DERR_RECREATE_TARGET_HRESULT, DXGI_ERROR_DEVICE_REMOVED_HRESULT, DXGI_STATUS_OCCLUDED_HRESULT,
    };
    use crate::render::device::tests::HiddenWindow;
    use crate::render::draw_context::test_kit::{
        assert_identical, color, create_noise, pixel, release, render, upload_single,
    };
    use crate::render::draw_context::{ColorF, RectD};

    const BLACK: ColorF = ColorF {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    const WHITE: ColorF = ColorF {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    fn offscreen(width: i32, height: i32, dpi: f64) -> D2dRenderSurface {
        D2dRenderSurface::create_offscreen_surface(width, height, dpi, false).unwrap()
    }

    fn warp_options() -> SurfaceOptions {
        SurfaceOptions {
            prefer_warp: true,
            ..SurfaceOptions::default()
        }
    }

    fn invalid(message: &'static str) -> RenderFault {
        RenderFault::InvalidOperation { message }
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
        RectD { x, y, width, height }
    }

    fn disposed() -> RenderFault {
        RenderFault::Disposed {
            object: "PhotoReview.Shell.Rendering.D2DRenderSurface",
        }
    }

    /// A counter handler for `subscribe_device_recreated`.
    fn counter() -> (Rc<Cell<i32>>, Box<dyn FnMut()>) {
        let count = Rc::new(Cell::new(0));
        let seen = Rc::clone(&count);
        (count, Box::new(move || seen.set(seen.get() + 1)))
    }

    // ------------------------------------------------------------------ pure

    // C#: `record RenderSurfaceOptions(bool PreferWarp = false, bool AllowTearing = false, int MaxFrameLatency = 1)`
    #[test]
    fn surface_options_default_matches_the_csharp_record() {
        let options = SurfaceOptions::default();
        assert_eq!(
            (options.prefer_warp, options.allow_tearing, options.max_frame_latency),
            (false, false, 1)
        );
        // `with { PreferWarp = true }` keeps the latency of 1
        let updated = SurfaceOptions {
            prefer_warp: true,
            ..SurfaceOptions::default()
        };
        assert_eq!(updated.max_frame_latency, 1);
    }

    // C#: enum PresentResult { Presented, Occluded, DeviceRecreated, Failed }
    #[test]
    fn present_result_discriminants_match_csharp() {
        assert_eq!(
            [
                PresentResult::Presented as i32,
                PresentResult::Occluded as i32,
                PresentResult::DeviceRecreated as i32,
                PresentResult::Failed as i32
            ],
            [0, 1, 2, 3]
        );
    }

    // C#: D2DWindowSurfaceTests.CreateForWindow_RejectsNullHandle (no COM is touched)
    #[test]
    fn create_window_surface_rejects_a_null_handle() {
        let expected = RenderFault::InvalidArgument {
            param: "hwnd",
            message: "A window handle is required.",
        };
        assert_eq!(
            D2dRenderSurface::create_window_surface(0, SurfaceOptions::default()).unwrap_err(),
            expected
        );
        // C#: D2DRenderSurfaceTests.Factory_CreateForWindow_RejectsNullHandle (Rust: the concrete factory and the trait)
        let factory = D2dRenderSurfaceFactory;
        assert_eq!(
            factory.create_for_window(0, SurfaceOptions::default()).unwrap_err(),
            expected
        );
        let via_trait: &dyn RenderSurfaceFactory = &factory;
        assert_eq!(
            via_trait.create_for_window(0, SurfaceOptions::default()).err(),
            Some(RenderError::InvalidArgument {
                param: "hwnd",
                message: "A window handle is required.",
            })
        );
    }

    // C#: DeviceResources.CreateOffscreen `ArgumentOutOfRangeException.ThrowIfLessThan` (width first)
    #[test]
    fn create_offscreen_surface_rejects_sizes_below_one() {
        let width = RenderFault::ArgumentOutOfRange { param: "pixelWidth" };
        let height = RenderFault::ArgumentOutOfRange { param: "pixelHeight" };
        assert_eq!(
            D2dRenderSurface::create_offscreen_surface(0, 4, 1.0, false).unwrap_err(),
            width
        );
        assert_eq!(
            D2dRenderSurface::create_offscreen_surface(0, 0, 1.0, false).unwrap_err(),
            width
        );
        assert_eq!(
            D2dRenderSurface::create_offscreen_surface(4, -1, 1.0, false).unwrap_err(),
            height
        );
        assert_eq!(D2dRenderSurfaceFactory.create_offscreen(-1, 4, 1.0).unwrap_err(), width);
    }

    // C#: D2DRenderSurface.Present `_options.AllowTearing || ImmediatePresent ? 0u : 1u` (trap T5)
    #[test]
    fn sync_interval_selection() {
        assert_eq!(sync_interval(false, false), 1);
        assert_eq!(sync_interval(true, false), 0);
        assert_eq!(sync_interval(false, true), 0);
        assert_eq!(sync_interval(true, true), 0);
    }

    // ---------------------------------------------------------------- native

    // C#: D2DRenderSurfaceTests.Offscreen_IsWarpWithoutWindowOrWaitHandle
    #[test]
    #[ignore = "native: needs desktop"]
    fn offscreen_surface_is_warp_without_window_or_wait_handle() {
        let surface = offscreen(64, 32, 1.25);
        assert!(surface.is_warp());
        assert!(
            surface.options.prefer_warp,
            "use_hardware=false is retained as PreferWarp=true"
        );
        assert_eq!(
            (surface.pixel_width(), surface.pixel_height(), surface.dpi_scale()),
            (64, 32, 1.25)
        );
        assert_eq!(surface.hwnd(), 0);
        assert_eq!(surface.frame_latency_wait_handle(), 0);
        assert!(surface.max_bitmap_size() >= 4096);
        assert_eq!(surface.device_generation(), 0);
        assert!(surface.adapter_description().is_empty());
        assert!(!surface.immediate_present());
        assert_eq!(surface.last_end_draw(), Duration::ZERO);
        assert_eq!(surface.last_present(), Duration::ZERO);
    }

    // C#: D2DRenderSurfaceTests.Factory_CreatesOffscreenSurfaceThroughTheInterface
    #[test]
    #[ignore = "native: needs desktop"]
    fn factory_creates_offscreen_surface() {
        let factory = D2dRenderSurfaceFactory;
        let surface = factory.create_offscreen(16, 8, 1.0).unwrap();
        assert!(surface.is_warp());
        assert_eq!((surface.pixel_width(), surface.pixel_height()), (16, 8));
        // through the trait object, as a consumer holding `dyn RenderSurfaceFactory` sees it
        let via_trait: &dyn RenderSurfaceFactory = &factory;
        let boxed = via_trait.create_offscreen(16, 8, 1.0).unwrap();
        assert!(boxed.is_warp());
        assert_eq!((boxed.pixel_width(), boxed.pixel_height(), boxed.hwnd()), (16, 8, 0));
        assert_eq!(boxed.frame_latency_wait_handle(), 0);
    }

    // C#: Factory_CreateForWindow_RejectsNullHandle is the pure test above; this one adds a real window through the factory
    #[test]
    #[ignore = "native: needs desktop"]
    fn factory_creates_a_window_surface() {
        let window = HiddenWindow::new();
        let surface = D2dRenderSurfaceFactory
            .create_for_window(window.raw(), warp_options())
            .unwrap();
        assert_eq!(surface.hwnd(), window.raw());
        assert!(surface.is_warp());
    }

    // C#: D2DRenderSurface.EndDrawAndPresent on an offscreen surface (no swap chain: Present returns Presented, no hook call)
    #[test]
    #[ignore = "native: needs desktop"]
    fn end_draw_and_present_offscreen_presents() {
        let mut surface = offscreen(8, 8, 1.0);
        let present_calls = Rc::new(Cell::new(0));
        let seen = Rc::clone(&present_calls);
        surface.resources().set_present_fault(Some(Box::new(move |hr| {
            seen.set(seen.get() + 1);
            hr
        })));
        surface.begin_draw_context().unwrap().clear(WHITE);
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Presented);
        assert_eq!(surface.last_present(), Duration::ZERO);
        assert_eq!(present_calls.get(), 0, "no swap chain: the present seam is not called");
    }

    // C#: D2DRenderSurfaceTests.EndDraw_UnbalancedClip_ThrowsButSurfaceStaysUsable
    #[test]
    #[ignore = "native: needs desktop"]
    fn end_draw_unbalanced_clip_is_an_error_after_the_frame_and_surface_stays_usable() {
        let mut surface = offscreen(8, 8, 1.0);
        surface
            .begin_draw_context()
            .unwrap()
            .push_clip(rect(0.0, 0.0, 4.0, 4.0));
        assert_eq!(
            surface.end_draw_and_present().unwrap_err(),
            invalid("Unbalanced PushClip/PopClip or PushOpacity/PopOpacity in the frame.")
        );
        let pixels = render(&mut surface, |dc| dc.clear(color(0.0, 1.0, 0.0, 1.0)));
        assert_eq!(pixel(&pixels, 7, 7).1, 255);
    }

    // T2: the unbalanced error is raised AFTER the present (window surface: the present seam saw the frame)
    #[test]
    #[ignore = "native: needs desktop"]
    fn unbalanced_frame_is_presented_before_the_error_is_raised() {
        let window = HiddenWindow::new();
        let mut surface = D2dRenderSurface::create_window_surface(window.raw(), warp_options()).unwrap();
        let present_calls = Rc::new(Cell::new(0));
        let seen = Rc::clone(&present_calls);
        surface.resources().set_present_fault(Some(Box::new(move |hr| {
            seen.set(seen.get() + 1);
            hr
        })));
        let context = surface.begin_draw_context().unwrap();
        context.clear(BLACK);
        context.push_opacity(0.5);
        assert!(surface.end_draw_and_present().is_err());
        assert_eq!(present_calls.get(), 1, "the frame was presented before the error");
        // the surface still works
        surface.begin_draw_context().unwrap().clear(BLACK);
        assert!(matches!(
            surface.end_draw_and_present().unwrap(),
            PresentResult::Presented | PresentResult::Occluded
        ));
        assert_eq!(present_calls.get(), 2);
    }

    // C#: D2DRenderSurfaceTests.Misuse_IsRejected (the half reachable in Rust; the cross-thread half cannot compile, note 8)
    #[test]
    #[ignore = "native: needs desktop"]
    fn misuse_is_rejected() {
        let mut surface = offscreen(8, 8, 1.0);
        assert_eq!(
            surface.end_draw_and_present().unwrap_err(),
            invalid("EndDrawAndPresent without BeginDraw.")
        );
        {
            let dc = surface.begin_draw_context().unwrap();
            assert_eq!(dc.pop_clip().unwrap_err(), invalid("PopClip without PushClip."));
            assert_eq!(
                dc.pop_opacity().unwrap_err(),
                invalid("PopOpacity without PushOpacity.")
            );
            assert_eq!(
                dc.pop_transform().unwrap_err(),
                invalid("PopTransform without PushTransform.")
            );
        }
        assert_eq!(
            surface.begin_draw_context().unwrap_err(),
            invalid("BeginDraw was already called; call EndDrawAndPresent first.")
        );
        assert_eq!(
            surface.resize(4, 4, 1.0).unwrap_err(),
            invalid("Resize is not allowed between BeginDraw and EndDrawAndPresent.")
        );
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Presented);

        surface.dispose();
        surface.dispose();
        assert_eq!(surface.begin_draw_context().unwrap_err(), disposed());
        assert_eq!(surface.end_draw_and_present().unwrap_err(), disposed());
        assert_eq!(surface.resize(4, 4, 1.0).unwrap_err(), disposed());
        assert_eq!(surface.wait_for_next_frame(0).unwrap_err(), disposed());
        assert_eq!(surface.wait_for_gpu().unwrap_err(), disposed());
        // the numeric getters keep working after dispose
        assert_eq!((surface.pixel_width(), surface.pixel_height()), (8, 8));
    }

    // C#: D2DRenderSurfaceTests.Resize_Offscreen_ChangesTargetSizeAndDpi
    #[test]
    #[ignore = "native: needs desktop"]
    fn resize_offscreen_changes_target_size_and_dpi() {
        let mut surface = offscreen(8, 8, 1.0);
        surface.resize(30, 12, 2.0).unwrap();
        assert_eq!(
            (surface.pixel_width(), surface.pixel_height(), surface.dpi_scale()),
            (30, 12, 2.0)
        );
        let pixels = render(&mut surface, |dc| {
            dc.clear(BLACK);
            dc.fill_rectangle(rect(0.0, 0.0, 5.0, 5.0), WHITE); // 5 DIP = 10 px
        });
        assert_eq!((pixels.width(), pixels.height()), (30, 12));
        assert_eq!(pixel(&pixels, 9, 9).0, 255);
        assert_eq!(pixel(&pixels, 10, 10).0, 0);
    }

    // C#: D2DRenderSurfaceTests.DeviceLost_RecreateTarget_RecreatesDevice_EmptiesCache_NextFrameDraws (surface half; the cache half is W8-12g)
    #[test]
    #[ignore = "native: needs desktop"]
    fn end_draw_device_lost_recreates_and_raises_the_event_once() {
        let mut surface = offscreen(16, 16, 1.0);
        let source = create_noise(16, 16, 9, true);
        let before = Rc::new(upload_single(surface.resources(), &source));
        assert_eq!(surface.resources().live_bitmap_count(), 1);
        let (recreated, handler) = counter();
        surface.subscribe_device_recreated(handler);
        // What the GPU image cache does in its handler (W8-12g): give every bitmap back.
        let resources = Rc::clone(surface.resources());
        let image = Rc::clone(&before);
        surface.subscribe_device_recreated(Box::new(move || release(&resources, &image)));
        let mut injected = false;
        surface.resources().set_end_draw_fault(Some(Box::new(move |hr| {
            if injected {
                return hr;
            }
            injected = true;
            D2DERR_RECREATE_TARGET_HRESULT
        })));

        let context = surface.begin_draw_context().unwrap();
        context.draw_image(
            &*before,
            rect(0.0, 0.0, 16.0, 16.0),
            None,
            ImageInterpolation::Linear,
            1.0,
        );
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::DeviceRecreated);

        assert_eq!(recreated.get(), 1);
        assert_eq!(surface.device_generation(), 1);
        assert_eq!(surface.resources().live_bitmap_count(), 0);
        // The old image is skipped, not drawn; uploading again on the new device draws.
        let after = upload_single(surface.resources(), &source);
        let pixels = render(&mut surface, |dc| {
            dc.clear(color(0.0, 0.0, 0.0, 0.0));
            dc.draw_image(
                &*before,
                rect(0.0, 0.0, 16.0, 16.0),
                None,
                ImageInterpolation::Linear,
                1.0,
            );
            assert_eq!(dc.skipped_image_draws(), 1);
            dc.draw_image(
                &after,
                rect(0.0, 0.0, 16.0, 16.0),
                None,
                ImageInterpolation::Linear,
                1.0,
            );
        });
        assert_identical(&source, &pixels);
        // the hook injected once: the second frame did not recreate
        assert_eq!(recreated.get(), 1);
        assert_eq!(surface.device_generation(), 1);
    }

    // C#: D2DRenderSurfaceTests.EndDraw_OtherFailure_ReturnsFailedWithoutRecreating (T1: a result, not an error)
    #[test]
    #[ignore = "native: needs desktop"]
    fn end_draw_other_failure_returns_failed_without_recreating() {
        let mut surface = offscreen(4, 4, 1.0);
        let (recreated, handler) = counter();
        surface.subscribe_device_recreated(handler);
        surface
            .resources()
            .set_end_draw_fault(Some(Box::new(|_| 0x8000_4005_u32 as i32))); // E_FAIL
        surface.begin_draw_context().unwrap().clear(color(0.0, 0.0, 0.0, 0.0));
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Failed);
        assert_eq!(surface.device_generation(), 0);
        assert_eq!(recreated.get(), 0);
        // the surface is usable afterwards
        surface.resources().set_end_draw_fault(None);
        surface.begin_draw_context().unwrap().clear(BLACK);
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Presented);
    }

    // C#: D2DRenderSurface.RecreateDevice / DeviceRecreated event (trap T6)
    #[test]
    #[ignore = "native: needs desktop"]
    fn subscribe_unsubscribe_and_order() {
        let mut surface = offscreen(4, 4, 1.0);
        let log = Rc::new(RefCell::new(Vec::new()));
        let push = |tag: &'static str| {
            let log = Rc::clone(&log);
            Box::new(move || log.borrow_mut().push(tag)) as Box<dyn FnMut()>
        };
        let first = surface.subscribe_device_recreated(push("first"));
        let second = surface.subscribe_device_recreated(push("second"));
        let third = surface.subscribe_device_recreated(push("third"));
        assert!(first < second && second < third, "ids increase");
        surface.unsubscribe_device_recreated(second);
        surface.unsubscribe_device_recreated(9999); // unknown: no-op
        surface.unsubscribe_device_recreated(second); // twice: no-op
        surface.recreate_device().unwrap();
        assert_eq!(
            *log.borrow(),
            ["first", "third"],
            "subscription order, the unsubscribed one is silent"
        );
        // ids are never reused, even after an unsubscribe
        let fourth = surface.subscribe_device_recreated(push("fourth"));
        assert!(fourth > third);
        // a handler runs after the new device exists
        let probe = Rc::clone(surface.resources());
        let generation_seen = Rc::new(Cell::new(-1));
        let seen = Rc::clone(&generation_seen);
        surface.subscribe_device_recreated(Box::new(move || seen.set(probe.generation())));
        surface.recreate_device().unwrap();
        assert_eq!(generation_seen.get(), 2);
        assert_eq!(surface.device_generation(), 2);
        // after a failed recreate (disposed) the handlers are NOT called
        let before = log.borrow().len();
        surface.dispose();
        assert!(surface.recreate_device().unwrap_err().to_string().contains("disposed"));
        assert_eq!(log.borrow().len(), before);
    }

    // Trait adapter must preserve handler identity and subscription lifecycle.
    #[test]
    #[ignore = "native: needs desktop"]
    fn render_surface_trait_device_recreated_subscription_is_forwarded() {
        let mut surface = offscreen(4, 4, 1.0);
        let (first_count, first_handler) = counter();
        let (second_count, second_handler) = counter();
        let via_trait: &mut dyn RenderSurface = &mut surface;
        let first = via_trait.subscribe_device_recreated(first_handler);
        let second = via_trait.subscribe_device_recreated(second_handler);
        assert_ne!(first, second);
        via_trait.unsubscribe_device_recreated(first);
        surface.recreate_device().unwrap();
        assert_eq!(first_count.get(), 0);
        assert_eq!(second_count.get(), 1);
        let via_trait: &mut dyn RenderSurface = &mut surface;
        via_trait.unsubscribe_device_recreated(second);
        surface.recreate_device().unwrap();
        assert_eq!(first_count.get(), 0);
        assert_eq!(second_count.get(), 1);
    }

    // C#: D2DRenderSurface.ImmediatePresent property
    #[test]
    #[ignore = "native: needs desktop"]
    fn immediate_present_flag_round_trips() {
        let mut surface = offscreen(4, 4, 1.0);
        assert!(!surface.immediate_present());
        surface.set_immediate_present(true);
        assert!(surface.immediate_present());
        surface.set_immediate_present(false);
        assert!(!surface.immediate_present());
    }

    // C#: D2DWindowSurfaceTests.WindowSurface_FlipModelWaitableSwapChain_LatencyOne_PresentsAndResizes (surface half)
    #[test]
    #[ignore = "native: needs desktop"]
    fn window_surface_presents_three_frames_and_resizes() {
        let window = HiddenWindow::new();
        let mut surface = D2dRenderSurface::create_window_surface(window.raw(), warp_options()).unwrap();
        assert_eq!(surface.hwnd(), window.raw());
        assert!(surface.is_warp());
        assert_eq!((surface.pixel_width(), surface.pixel_height()), (160, 90));
        assert_ne!(surface.frame_latency_wait_handle(), 0);
        assert_eq!(surface.resources().query_maximum_frame_latency().unwrap(), 1);
        assert!(surface.wait_for_next_frame(1000).unwrap());
        assert!(surface.adapter_description().contains("vendor 0x"));

        let presented = |result: PresentResult| matches!(result, PresentResult::Presented | PresentResult::Occluded);
        for _ in 0..3 {
            let dc = surface.begin_draw_context().unwrap();
            dc.clear(color(0.0627, 0.0627, 0.0627, 1.0));
            dc.fill_rectangle(rect(10.0, 10.0, 20.0, 20.0), WHITE);
            assert!(
                dc.try_read_pixels().unwrap().is_none(),
                "a window surface has no read-back"
            );
            assert!(presented(surface.end_draw_and_present().unwrap()));
        }

        surface.resize(320, 200, 1.0).unwrap();
        assert_eq!((surface.pixel_width(), surface.pixel_height()), (320, 200));
        surface.begin_draw_context().unwrap().clear(BLACK);
        assert!(presented(surface.end_draw_and_present().unwrap()));
        assert_ne!(surface.frame_latency_wait_handle(), 0);
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn window_surface_records_present_interval() {
        let window = HiddenWindow::new();
        let mut surface = D2dRenderSurface::create_window_surface(window.raw(), warp_options()).unwrap();
        surface.begin_draw_context().unwrap().clear(BLACK);
        assert!(matches!(
            surface.end_draw_and_present().unwrap(),
            PresentResult::Presented | PresentResult::Occluded
        ));
        // The interval may be clock-resolution zero. A Present fault hook proves the interval was measured
        // around the swap-chain call, while last_present remains the elapsed duration (including zero).
        assert!(surface.last_present() <= Duration::from_secs(60));
    }

    // Immediate present and the tearing option only change the sync interval; the frame still presents
    #[test]
    #[ignore = "native: needs desktop"]
    fn window_surface_presents_with_immediate_and_tearing_sync_zero() {
        let window = HiddenWindow::new();
        let options = SurfaceOptions {
            allow_tearing: true,
            ..warp_options()
        };
        let mut surface = D2dRenderSurface::create_window_surface(window.raw(), options).unwrap();
        for immediate in [false, true] {
            surface.set_immediate_present(immediate);
            surface.begin_draw_context().unwrap().clear(BLACK);
            assert!(matches!(
                surface.end_draw_and_present().unwrap(),
                PresentResult::Presented | PresentResult::Occluded
            ));
        }
    }

    // Equivalent/unobservable: source lines 291-294 time a real swap-chain Present with Instant::elapsed;
    // there is no clock seam, and elapsed can legitimately be Duration::ZERO at clock resolution, so
    // replacing the getter with ZERO cannot be distinguished by a deterministic native test.

    // C#: D2DWindowSurfaceTests.WindowSurface_DeviceRemovedOnPresent_RecreatesSwapChainAndRaisesEvent
    #[test]
    #[ignore = "native: needs desktop"]
    fn window_surface_device_removed_on_present_recreates_swap_chain_and_raises_event() {
        let window = HiddenWindow::new();
        let mut surface = D2dRenderSurface::create_window_surface(window.raw(), warp_options()).unwrap();
        let (recreated, handler) = counter();
        surface.subscribe_device_recreated(handler);
        let mut injected = false;
        surface.resources().set_present_fault(Some(Box::new(move |hr| {
            if injected {
                return hr;
            }
            injected = true;
            DXGI_ERROR_DEVICE_REMOVED_HRESULT
        })));

        surface.begin_draw_context().unwrap().clear(BLACK);
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::DeviceRecreated);

        assert_eq!(recreated.get(), 1);
        assert_eq!(surface.device_generation(), 1);
        assert_ne!(surface.frame_latency_wait_handle(), 0);
        assert_eq!(surface.resources().query_maximum_frame_latency().unwrap(), 1);
        assert_eq!((surface.pixel_width(), surface.pixel_height()), (160, 90));
        surface.begin_draw_context().unwrap().clear(BLACK);
        assert!(matches!(
            surface.end_draw_and_present().unwrap(),
            PresentResult::Presented | PresentResult::Occluded
        ));
        assert_eq!(recreated.get(), 1);
    }

    // C#: D2DWindowSurfaceTests.WindowSurface_OccludedStatus_IsReported (T4: a success code)
    #[test]
    #[ignore = "native: needs desktop"]
    fn window_surface_occluded_status_is_reported() {
        let window = HiddenWindow::new();
        let mut surface = D2dRenderSurface::create_window_surface(window.raw(), warp_options()).unwrap();
        surface
            .resources()
            .set_present_fault(Some(Box::new(|_| DXGI_STATUS_OCCLUDED_HRESULT)));
        surface.begin_draw_context().unwrap().clear(BLACK);
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Occluded);
        assert_eq!(surface.device_generation(), 0);
    }

    // C#: D2DRenderSurface.Present `hr < 0` that is not a loss (T1)
    #[test]
    #[ignore = "native: needs desktop"]
    fn window_surface_present_failure_returns_failed_without_recreating() {
        let window = HiddenWindow::new();
        let mut surface = D2dRenderSurface::create_window_surface(window.raw(), warp_options()).unwrap();
        surface
            .resources()
            .set_present_fault(Some(Box::new(|_| 0x8000_4005_u32 as i32)));
        surface.begin_draw_context().unwrap().clear(BLACK);
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Failed);
        assert_eq!(surface.device_generation(), 0);
    }

    // C#: D2DRenderSurface.Resize `ComInterop.Check(hr)` for a ResizeBuffers failure that is not a device loss (T3: the non-loss branch)
    #[test]
    #[ignore = "native: needs desktop"]
    fn window_surface_resize_failure_that_is_not_a_device_loss_is_a_device_error() {
        let window = HiddenWindow::new();
        let mut surface = D2dRenderSurface::create_window_surface(window.raw(), warp_options()).unwrap();
        let (recreated, handler) = counter();
        surface.subscribe_device_recreated(handler);
        let keep = surface.resources().target().unwrap(); // a back-buffer reference makes ResizeBuffers fail
        let error = surface.resize(200, 120, 1.0).unwrap_err();
        assert_eq!(
            error,
            RenderFault::Device(GfxError::new(
                "IDXGISwapChain::ResizeBuffers",
                DXGI_ERROR_INVALID_CALL.0
            ))
        );
        assert_eq!(recreated.get(), 0, "no device loss: no recreate");
        assert_eq!(surface.device_generation(), 0);
        drop(keep);
        surface.resize(200, 120, 1.0).unwrap();
        assert_eq!((surface.pixel_width(), surface.pixel_height()), (200, 120));
        surface.begin_draw_context().unwrap().clear(BLACK);
        assert!(surface.end_draw_and_present().is_ok());
    }

    // C#: D2DRenderSurface.WaitForNextFrame (the waitable object is signaled after creation)
    #[test]
    #[ignore = "native: needs desktop"]
    fn wait_for_next_frame_offscreen_is_true_and_window_is_true_within_a_second() {
        let offscreen_surface = offscreen(4, 4, 1.0);
        assert!(offscreen_surface.wait_for_next_frame(0).unwrap());
        assert!(
            offscreen_surface.wait_for_next_frame(-5).unwrap(),
            "a negative timeout is 0"
        );
        let window = HiddenWindow::new();
        let surface = D2dRenderSurface::create_window_surface(window.raw(), warp_options()).unwrap();
        assert!(surface.wait_for_next_frame(1000).unwrap());
    }

    // C#: D2DRenderSurface.WaitForGpu = DeviceResources.Synchronize (the perf tests use offscreen surfaces; on a window target the
    // C# code path copies the swap-chain back buffer into a premultiplied probe, which WARP rejects with E_INVALIDARG: not asserted)
    #[test]
    #[ignore = "native: needs desktop"]
    fn wait_for_gpu_synchronizes_an_offscreen_target() {
        let mut surface = offscreen(8, 8, 1.0);
        surface.begin_draw_context().unwrap().clear(WHITE);
        surface.end_draw_and_present().unwrap();
        surface.wait_for_gpu().unwrap();
        surface.wait_for_gpu().unwrap();
        assert_eq!(
            surface.resources().live_bitmap_count(),
            1,
            "one probe bitmap, created once"
        );
    }

    // C#: D2DRenderSurface.Dispose -> DeviceResources.Dispose; the cache's clone of the resources stays valid but reports Disposed
    #[test]
    #[ignore = "native: needs desktop"]
    fn dropping_the_surface_disposes_the_shared_resources() {
        let surface = offscreen(8, 8, 1.0);
        let resources = Rc::clone(surface.resources());
        assert!(resources.context().is_ok());
        drop(surface);
        assert_eq!(
            resources.context().unwrap_err(),
            RenderFault::Disposed {
                object: "DeviceResources"
            }
        );
        // an explicit dispose does the same and is idempotent
        let mut surface = offscreen(8, 8, 1.0);
        let resources = Rc::clone(surface.resources());
        surface.dispose();
        surface.dispose();
        assert!(resources.target().is_err());
    }

    // The RenderSurface adapter: begin_draw / end_draw_and_present / resize through the trait object
    #[test]
    #[ignore = "native: needs desktop"]
    fn render_surface_trait_adapter_maps_errors_and_results() {
        let mut boxed = D2dRenderSurfaceFactory
            .create_offscreen(8, 8, 1.0)
            .map(Box::new)
            .unwrap();
        let surface: &mut dyn RenderSurface = &mut *boxed;
        assert_eq!(
            surface.end_draw_and_present().unwrap_err(),
            RenderError::InvalidOperation {
                message: "EndDrawAndPresent without BeginDraw."
            }
        );
        surface.begin_draw().unwrap().clear(BLACK);
        assert_eq!(
            surface.begin_draw().err(),
            Some(RenderError::InvalidOperation {
                message: "BeginDraw was already called; call EndDrawAndPresent first."
            })
        );
        assert_eq!(
            surface.resize(4, 4, 1.0).unwrap_err(),
            RenderError::InvalidOperation {
                message: "Resize is not allowed between BeginDraw and EndDrawAndPresent."
            }
        );
        assert_eq!(surface.end_draw_and_present().unwrap(), PresentResult::Presented);
        surface.resize(4, 4, 1.0).unwrap();
        let (count, handler) = counter();
        let id = surface.subscribe_device_recreated(handler);
        surface.unsubscribe_device_recreated(id);
        assert_eq!(count.get(), 0);
    }
}
