//! Port of `PhotoReview.Shell.Rendering/DeviceResources.cs` (456 lines, WP-15), baseline `d92d6aae`. Units W8-12b (device creation and loss:
//! `create`, `loss`, the state in this file) and W8-12c (target, bitmaps, teardown: `resources`). Contracts: `contracts/W8-12b.md`,
//! `contracts/W8-12c.md`.
//!
//! [`DeviceResources`] owns every device-dependent object of one render surface: the D3D11 device (BGRA), the DXGI device, the Direct2D
//! factory, device and device context, the shared white brush, the flip-model swap chain with its frame-latency waitable object (window
//! surfaces only) and the render target bitmap. A device loss (`D2DERR_RECREATE_TARGET`, `DXGI_ERROR_DEVICE_REMOVED` / `_RESET`) is
//! `recreate`: drop everything, build it again with the same size, bump `generation` (every bitmap of an older generation is dead).
//!
//! Thread affinity: UI thread only. The type is `!Send` and `!Sync` (`PhantomData<*const ()>`) because `windows` 0.62 marks the D3D11,
//! DXGI and Direct2D interfaces `Send` and the single-threaded Direct2D factory must not leave its thread (D-W8B3G-9 d). It is shared as
//! `Rc<DeviceResources>` by the surface, the draw context and the GPU image cache, so every method takes `&self` (`RefCell` / `Cell`).
//!
//! Error type: [`RenderFault`] is the interim twin of `pr_app::services::render::RenderError` (same variants, same fields); it exists only
//! because `pr_platform_win` cannot see `pr_app` yet (D-W8B3R-1). The adapter that implements the `pr_app` traits converts it variant by variant.

#![allow(unused_variables, dead_code)] // W8-12b/12c stub: the porters remove this line when the last `todo!()` of the module tree is gone.

pub mod create;
pub mod loss;
pub mod resources;

use std::cell::{Cell, RefCell};
use std::fmt;
use std::marker::PhantomData;

use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Direct3D11::ID3D11Device;

use crate::gfx::com::GfxError;
use crate::gfx::dxgi::WindowSwapChain;

/// Interim twin of `pr_app::services::render::RenderError` (the five variants the device, surface and draw context can produce).
/// `Device` is a [`GfxError`] one to one (`hresult`, `operation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderFault {
    /// `ObjectDisposedException`; `object` is the C# full type name (for example `"PhotoReview.Shell.Rendering.DeviceResources"`).
    Disposed { object: &'static str },
    /// `InvalidOperationException`, message verbatim.
    InvalidOperation { message: &'static str },
    /// `ArgumentException`, message and parameter name verbatim.
    InvalidArgument { param: &'static str, message: &'static str },
    /// `ArgumentOutOfRangeException`.
    ArgumentOutOfRange { param: &'static str },
    /// A failed COM call (`ComInterop.Check`).
    Device(GfxError),
}

impl fmt::Display for RenderFault {
    /// Same texts as `RenderError::fmt` (contract W8-12a "Error texts").
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W8-12b")
    }
}

impl std::error::Error for RenderFault {}

impl From<GfxError> for RenderFault {
    fn from(error: GfxError) -> Self {
        todo!("W8-12b: RenderFault::Device(error)")
    }
}

/// Test seam (C# `EndDrawFault` / `PresentFault`): receives the real HRESULT and returns the HRESULT to use.
pub type FaultHook = Box<dyn FnMut(i32) -> i32>;

/// The device-dependent objects, replaced as a whole by `recreate` (C# private fields `_d3d` .. `_syncProbe`, `_waitHandle`).
/// Field order is the drop order: target and probe first, then brush, context, device, factory, swap chain (its waitable handle closes
/// with it), D3D device last (D-W8B3G-7; `ReleaseAll` additionally calls `clear_state_and_flush` BEFORE the swap chain is dropped).
#[derive(Debug, Default)]
pub(crate) struct DeviceState {
    pub(crate) target: Option<ID2D1Bitmap1>,
    pub(crate) sync_probe: Option<ID2D1Bitmap1>,
    pub(crate) brush: Option<ID2D1SolidColorBrush>,
    pub(crate) context: Option<ID2D1DeviceContext>,
    pub(crate) device: Option<ID2D1Device>,
    pub(crate) factory: Option<ID2D1Factory1>,
    pub(crate) swap_chain: Option<WindowSwapChain>,
    pub(crate) d3d: Option<ID3D11Device>,
    pub(crate) pixel_width: i32,
    pub(crate) pixel_height: i32,
    pub(crate) dpi_scale: f64,
    pub(crate) is_warp: bool,
    pub(crate) generation: i64,
    pub(crate) max_bitmap_size: i32,
    pub(crate) adapter_description: String,
    pub(crate) disposed: bool,
}

/// All device-dependent resources of one render surface (C# `DeviceResources`). Created by [`DeviceResources::create_for_window`] or
/// [`DeviceResources::create_offscreen`] (`create.rs`), shared as `Rc`. `Drop` is C# `Dispose` (`dispose` is the explicit, idempotent form).
pub struct DeviceResources {
    pub(crate) hwnd: isize,
    pub(crate) prefer_warp: bool,
    /// Already clamped to 1..=16 (`gfx::dxgi::clamp_frame_latency`).
    pub(crate) max_frame_latency: u32,
    pub(crate) state: RefCell<DeviceState>,
    /// C# `LiveBitmapCount` (bitmaps handed out by `create_bitmap` and not yet given back by `release_bitmap`).
    pub(crate) live_bitmap_count: Cell<i64>,
    pub(crate) end_draw_fault: RefCell<Option<FaultHook>>,
    pub(crate) present_fault: RefCell<Option<FaultHook>>,
    pub(crate) _not_send: PhantomData<*const ()>,
}

impl fmt::Debug for DeviceResources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceResources")
            .field("hwnd", &self.hwnd)
            .finish_non_exhaustive()
    }
}

/// Read-only state (W8-12b). All are C# properties of the same name; none can fail except the object getters and
/// `query_maximum_frame_latency`, which are `Err` after `dispose` (C# `ObjectDisposedException`) or when the COM call fails.
impl DeviceResources {
    /// C# `Hwnd`: the window handle, `0` for an offscreen target.
    #[must_use]
    pub fn hwnd(&self) -> isize {
        todo!("W8-12b")
    }

    /// C# `PixelWidth` (always `>= 1`).
    #[must_use]
    pub fn pixel_width(&self) -> i32 {
        todo!("W8-12b")
    }

    /// C# `PixelHeight` (always `>= 1`).
    #[must_use]
    pub fn pixel_height(&self) -> i32 {
        todo!("W8-12b")
    }

    /// C# `DpiScale` (always `> 0`).
    #[must_use]
    pub fn dpi_scale(&self) -> f64 {
        todo!("W8-12b")
    }

    /// C# `IsWarp`.
    #[must_use]
    pub fn is_warp(&self) -> bool {
        todo!("W8-12b")
    }

    /// C# `Generation`: `0` at creation, `+1` per `recreate`.
    #[must_use]
    pub fn generation(&self) -> i64 {
        todo!("W8-12b")
    }

    /// C# `MaxBitmapSize` (`gfx::d2d::max_bitmap_size`).
    #[must_use]
    pub fn max_bitmap_size(&self) -> i32 {
        todo!("W8-12b")
    }

    /// C# `FrameLatencyWaitHandle` as a raw handle value (`0` for an offscreen target). The handle stays owned by the swap chain
    /// (closed when it is dropped by `recreate` / `dispose`); callers never close it.
    #[must_use]
    pub fn frame_latency_wait_handle(&self) -> isize {
        todo!("W8-12b")
    }

    /// C# `LiveBitmapCount` (GPU leak probe for the tests).
    #[must_use]
    pub fn live_bitmap_count(&self) -> i64 {
        todo!("W8-12b")
    }

    /// C# `HasSwapChain`.
    #[must_use]
    pub fn has_swap_chain(&self) -> bool {
        todo!("W8-12b")
    }

    /// C# `AdapterDescription` (`"{name} (vendor 0x{vendor:X4})"`, empty for an offscreen target or when `GetDesc` fails).
    #[must_use]
    pub fn adapter_description(&self) -> String {
        todo!("W8-12b")
    }

    /// C# `QueryMaximumFrameLatency` (`0` without a swap chain). Test seam.
    pub fn query_maximum_frame_latency(&self) -> Result<u32, RenderFault> {
        todo!("W8-12b")
    }

    /// C# `Context`: a clone (an `AddRef`) of the device context. `Err(Disposed)` after `dispose`.
    pub fn context(&self) -> Result<ID2D1DeviceContext, RenderFault> {
        todo!("W8-12b")
    }

    /// C# `Brush`: the shared white solid colour brush (callers set colour and opacity per call).
    pub fn brush(&self) -> Result<ID2D1SolidColorBrush, RenderFault> {
        todo!("W8-12b")
    }

    /// C# `Target`: the current render target bitmap (changes on `resize` and `recreate`; do not keep it across calls).
    pub fn target(&self) -> Result<ID2D1Bitmap1, RenderFault> {
        todo!("W8-12b")
    }

    /// The D3D11 device (the GPU image cache and the keep-alive need it; not in C#, which used the private field).
    pub fn d3d_device(&self) -> Result<ID3D11Device, RenderFault> {
        todo!("W8-12b")
    }

    /// C# `EndDrawFault = fault` (`None` clears it). Test seam; `pub(crate)`.
    pub(crate) fn set_end_draw_fault(&self, fault: Option<FaultHook>) {
        todo!("W8-12b")
    }

    /// C# `PresentFault = fault`. Test seam; `pub(crate)`.
    pub(crate) fn set_present_fault(&self, fault: Option<FaultHook>) {
        todo!("W8-12b")
    }
}
