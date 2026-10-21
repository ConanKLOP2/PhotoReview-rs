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
//! Interim types (D-W8B3R-1): [`SurfaceOptions`] and [`PresentResult`] mirror `pr_app::services::render::surface::{RenderSurfaceOptions,
//! PresentResult}` and are replaced by imports when the layering PR lands; the `RenderSurface` / `RenderSurfaceFactory` impls are the
//! adapter written then.

#![allow(unused_variables, dead_code)] // W8-12d stub: the porter removes this line when the last `todo!()` is gone.

use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Duration;

use super::device::{DeviceResources, RenderFault};
use super::draw_context::D2dDrawContext;

/// Interim twin of `RenderSurfaceOptions` (C# record `(bool PreferWarp = false, bool AllowTearing = false, int MaxFrameLatency = 1)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceOptions {
    /// Skip the hardware device and use WARP.
    pub prefer_warp: bool,
    /// Present with sync interval 0 (no vblank wait). `DXGI_PRESENT_ALLOW_TEARING` is NOT used (C# v1, needs `IDXGIFactory5`).
    pub allow_tearing: bool,
    /// `SetMaximumFrameLatency` value, clamped to 1..=16 by `DeviceResources`.
    pub max_frame_latency: i32,
}

impl Default for SurfaceOptions {
    /// `(false, false, 1)`: NOT a derived `Default` (latency would be 0).
    fn default() -> Self {
        todo!("W8-12d")
    }
}

/// Interim twin of `PresentResult` (discriminants are the C# values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentResult {
    /// The frame was handed to DXGI (or the surface has no swap chain).
    Presented = 0,
    /// `DXGI_STATUS_OCCLUDED`.
    Occluded = 1,
    /// A device loss happened during `EndDraw` or `Present`; the device was rebuilt, the frame is lost.
    DeviceRecreated = 2,
    /// `EndDraw` or `Present` failed with a negative HRESULT that is not a device loss.
    Failed = 3,
}

/// `AllowTearing || ImmediatePresent ? 0u : 1u` (C# `Present`, line 212; pure): the sync interval handed to `IDXGISwapChain::Present`.
#[must_use]
pub fn sync_interval(allow_tearing: bool, immediate_present: bool) -> u32 {
    todo!("W8-12d")
}

/// Handle returned by [`D2dRenderSurface::subscribe_device_recreated`] (same pattern as `FrameClock`).
pub type DeviceRecreatedHandlerId = u64;

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

impl D2dRenderSurface {
    /// C# `CreateWindowSurface(hwnd, options)`: `DeviceResources::create_for_window` then the surface. `hwnd == 0` is
    /// `Err(InvalidArgument { param: "hwnd", message: "A window handle is required." })`.
    pub fn create_window_surface(hwnd: isize, options: SurfaceOptions) -> Result<Self, RenderFault> {
        todo!("W8-12d")
    }

    /// C# `CreateOffscreenSurface(w, h, dpi, useHardware)`: WARP unless `use_hardware`. A size below 1 is `ArgumentOutOfRange`.
    pub fn create_offscreen_surface(
        pixel_width: i32,
        pixel_height: i32,
        dpi_scale: f64,
        use_hardware: bool,
    ) -> Result<Self, RenderFault> {
        todo!("W8-12d")
    }

    /// C# `Hwnd` (0 offscreen).
    #[must_use]
    pub fn hwnd(&self) -> isize {
        todo!("W8-12d")
    }

    /// C# `PixelWidth`.
    #[must_use]
    pub fn pixel_width(&self) -> i32 {
        todo!("W8-12d")
    }

    /// C# `PixelHeight`.
    #[must_use]
    pub fn pixel_height(&self) -> i32 {
        todo!("W8-12d")
    }

    /// C# `DpiScale`.
    #[must_use]
    pub fn dpi_scale(&self) -> f64 {
        todo!("W8-12d")
    }

    /// C# `FrameLatencyWaitHandle` (raw handle value, 0 offscreen). Owned by the swap chain; never close it.
    #[must_use]
    pub fn frame_latency_wait_handle(&self) -> isize {
        todo!("W8-12d")
    }

    /// C# `IsWarp`.
    #[must_use]
    pub fn is_warp(&self) -> bool {
        todo!("W8-12d")
    }

    /// C# `AdapterDescription` (diagnostics; empty offscreen).
    #[must_use]
    pub fn adapter_description(&self) -> String {
        todo!("W8-12d")
    }

    /// C# `DeviceGeneration`.
    #[must_use]
    pub fn device_generation(&self) -> i64 {
        todo!("W8-12d")
    }

    /// C# `MaxBitmapSize`.
    #[must_use]
    pub fn max_bitmap_size(&self) -> i32 {
        todo!("W8-12d")
    }

    /// C# `ImmediatePresent` getter (WP-20: sync interval 0 until the first image frame is presented).
    #[must_use]
    pub fn immediate_present(&self) -> bool {
        todo!("W8-12d")
    }

    /// C# `ImmediatePresent` setter.
    pub fn set_immediate_present(&mut self, value: bool) {
        todo!("W8-12d")
    }

    /// C# `Resources` (the same object across every recreate; the GPU image cache keeps a clone to create and release bitmaps).
    #[must_use]
    pub fn resources(&self) -> &Rc<DeviceResources> {
        todo!("W8-12d")
    }

    /// C# `LastEndDrawTicks` as a duration (WP-21 diagnostics: time inside `ID2D1DeviceContext::EndDraw` of the last frame).
    #[must_use]
    pub fn last_end_draw(&self) -> Duration {
        todo!("W8-12d")
    }

    /// C# `LastPresentTicks` as a duration (`Duration::ZERO` when the last frame had no `Present` call).
    #[must_use]
    pub fn last_present(&self) -> Duration {
        todo!("W8-12d")
    }

    /// C# `Resize`: `Err(Disposed)` after dispose; `DeviceResources::resize`; `BuffersFailed(hr)` with a device-loss `hr` is
    /// `recreate_device`, any other `hr < 0` is `Err(Device { hresult, operation: "IDXGISwapChain::ResizeBuffers" })`. While a frame is
    /// open it is `Err(InvalidOperation "Resize is not allowed between BeginDraw and EndDrawAndPresent.")`, checked before the
    /// device is touched.
    pub fn resize(&mut self, pixel_width: i32, pixel_height: i32, dpi_scale: f64) -> Result<(), RenderFault> {
        todo!("W8-12d")
    }

    /// C# `BeginDrawContext`: `Err(Disposed)`; sets `drawing`, `ID2D1DeviceContext::BeginDraw`, `D2dDrawContext::begin`; returns the
    /// context (it borrows the surface). A second call while a frame is open (the first reference was dropped without
    /// `end_draw_and_present`) is `Err(InvalidOperation "BeginDraw was already called; call EndDrawAndPresent first.")`.
    pub fn begin_draw_context(&mut self) -> Result<&mut D2dDrawContext, RenderFault> {
        todo!("W8-12d")
    }

    /// C# `EndDrawAndPresent`. Order: `drawing = false`; `D2dDrawContext::end` (pops dangling clips, returns `balanced`); time
    /// `DeviceResources::end_draw`; `classify_end_draw`: `DeviceLost` is `recreate_device` and `DeviceRecreated`, `Failed` is
    /// `Ok(Failed)`, `Continue` is `present`. THEN, if the stacks were not balanced,
    /// `Err(InvalidOperation "Unbalanced PushClip/PopClip or PushOpacity/PopOpacity in the frame.")` (the frame was still shown and
    /// the surface is usable). Without `begin_draw_context`: `Err(InvalidOperation "EndDrawAndPresent without BeginDraw.")`.
    pub fn end_draw_and_present(&mut self) -> Result<PresentResult, RenderFault> {
        todo!("W8-12d")
    }

    /// C# `Present` (private): no swap chain gives `Presented` (and `last_present` zero); sync interval is 0 when `allow_tearing` or
    /// `immediate_present`, else 1; `gfx::dxgi::classify_present` maps `Occluded`, `DeviceLost` (recreate, `DeviceRecreated`),
    /// `Failed`, `Presented`.
    fn present(&mut self) -> PresentResult {
        todo!("W8-12d")
    }

    /// C# `WaitForNextFrame(timeoutMilliseconds)`: offscreen is `Ok(true)`; else `WaitForSingleObjectEx(handle, max(0, timeout),
    /// alertable = true)` through `gfx::extras::wait_for_handle`; true only for `WAIT_OBJECT_0`.
    pub fn wait_for_next_frame(&self, timeout_milliseconds: i32) -> Result<bool, RenderFault> {
        todo!("W8-12d")
    }

    /// C# `WaitForGpu`: `DeviceResources::synchronize`.
    pub fn wait_for_gpu(&self) -> Result<(), RenderFault> {
        todo!("W8-12d")
    }

    /// C# `RecreateDevice`: `DeviceResources::recreate` then every subscribed handler in subscription order (synchronously).
    pub fn recreate_device(&mut self) -> Result<(), RenderFault> {
        todo!("W8-12d")
    }

    /// C# `DeviceRecreated +=`. Ids are never reused.
    pub fn subscribe_device_recreated(&mut self, handler: Box<dyn FnMut()>) -> DeviceRecreatedHandlerId {
        todo!("W8-12d")
    }

    /// C# `DeviceRecreated -=`; an unknown id is a no-op.
    pub fn unsubscribe_device_recreated(&mut self, id: DeviceRecreatedHandlerId) {
        todo!("W8-12d")
    }

    /// C# `Dispose`: idempotent; marks the surface disposed and calls `DeviceResources::dispose` (the cache's clone of the `Rc` stays
    /// valid but every device getter then gives `Err(Disposed)`).
    pub fn dispose(&mut self) {
        todo!("W8-12d")
    }
}

impl Drop for D2dRenderSurface {
    fn drop(&mut self) {
        // The porter calls `self.dispose()`.
    }
}

/// C# `D2DRenderSurfaceFactory` (13 lines, `IRenderSurfaceFactory`): stateless; the `RenderSurfaceFactory` trait impl is the adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct D2dRenderSurfaceFactory;

impl D2dRenderSurfaceFactory {
    /// C# `CreateForWindow` = `D2DRenderSurface.CreateWindowSurface`.
    pub fn create_for_window(&self, hwnd: isize, options: SurfaceOptions) -> Result<D2dRenderSurface, RenderFault> {
        todo!("W8-12d")
    }

    /// C# `CreateOffscreen` = `CreateOffscreenSurface(w, h, dpi)` (WARP).
    pub fn create_offscreen(
        &self,
        pixel_width: i32,
        pixel_height: i32,
        dpi_scale: f64,
    ) -> Result<D2dRenderSurface, RenderFault> {
        todo!("W8-12d")
    }
}
