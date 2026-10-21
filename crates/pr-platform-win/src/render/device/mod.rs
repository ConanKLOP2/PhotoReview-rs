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
//! Error type: [`RenderFault`] mirrors the device-level variants of `pr_app::services::render::RenderError` (same fields, same
//! `Display` texts, pinned by a test). It stays a separate type because the contract fixes it as the public error of this layer (other
//! render units import it); the adapter that implements the `pr_app` traits converts it variant by variant (D-W8B3R-1).

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
use crate::gfx::dxgi::{self, WindowSwapChain};

/// `ObjectDisposedException.ThrowIf(_disposed, this)`: the C# full type name (`Recreate`, `Resize`, `TouchGpu`).
pub(crate) const OBJECT_NAME: &str = "PhotoReview.Shell.Rendering.DeviceResources";

/// `ObjectDisposedException(nameof(DeviceResources))`: the short name the `Context`, `Brush` and `Target` getters used.
const OBJECT_NAME_SHORT: &str = "DeviceResources";

/// The error of the object getters after `dispose` (or after a failed `recreate`): `ObjectName` is `nameof(DeviceResources)`.
pub(crate) fn object_disposed_by_name() -> RenderFault {
    RenderFault::Disposed {
        object: OBJECT_NAME_SHORT,
    }
}

/// The error of `ObjectDisposedException.ThrowIf(_disposed, this)`: `ObjectName` is the full type name.
pub(crate) fn object_disposed() -> RenderFault {
    RenderFault::Disposed { object: OBJECT_NAME }
}

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
        match self {
            RenderFault::Disposed { object } => {
                write!(f, "Cannot access a disposed object.\r\nObject name: '{object}'.")
            }
            RenderFault::InvalidOperation { message } => f.write_str(message),
            RenderFault::InvalidArgument { param, message } => write!(f, "{message} (Parameter '{param}')"),
            RenderFault::ArgumentOutOfRange { param } => write!(
                f,
                "Specified argument was out of the range of valid values. (Parameter '{param}')"
            ),
            RenderFault::Device(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RenderFault {}

impl From<GfxError> for RenderFault {
    fn from(error: GfxError) -> Self {
        RenderFault::Device(error)
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
///
/// The type is `!Send` (one thread owns it and the single-threaded Direct2D factory):
///
/// ```compile_fail
/// fn needs_send<T: Send>() {}
/// needs_send::<pr_platform_win::render::device::DeviceResources>();
/// ```
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
        self.hwnd
    }

    /// C# `PixelWidth` (always `>= 1`).
    #[must_use]
    pub fn pixel_width(&self) -> i32 {
        self.state.borrow().pixel_width
    }

    /// C# `PixelHeight` (always `>= 1`).
    #[must_use]
    pub fn pixel_height(&self) -> i32 {
        self.state.borrow().pixel_height
    }

    /// C# `DpiScale` (always `> 0`).
    #[must_use]
    pub fn dpi_scale(&self) -> f64 {
        self.state.borrow().dpi_scale
    }

    /// C# `IsWarp`.
    #[must_use]
    pub fn is_warp(&self) -> bool {
        self.state.borrow().is_warp
    }

    /// C# `Generation`: `0` at creation, `+1` per `recreate`.
    #[must_use]
    pub fn generation(&self) -> i64 {
        self.state.borrow().generation
    }

    /// C# `MaxBitmapSize` (`gfx::d2d::max_bitmap_size`).
    #[must_use]
    pub fn max_bitmap_size(&self) -> i32 {
        self.state.borrow().max_bitmap_size
    }

    /// C# `FrameLatencyWaitHandle` as a raw handle value (`0` for an offscreen target). The handle stays owned by the swap chain
    /// (closed when it is dropped by `recreate` / `dispose`); callers never close it.
    #[must_use]
    pub fn frame_latency_wait_handle(&self) -> isize {
        self.state
            .borrow()
            .swap_chain
            .as_ref()
            .map_or(0, |swap_chain| swap_chain.frame_latency_waitable.0 as isize)
    }

    /// C# `LiveBitmapCount` (GPU leak probe for the tests).
    #[must_use]
    pub fn live_bitmap_count(&self) -> i64 {
        self.live_bitmap_count.get()
    }

    /// C# `HasSwapChain`.
    #[must_use]
    pub fn has_swap_chain(&self) -> bool {
        self.state.borrow().swap_chain.is_some()
    }

    /// C# `AdapterDescription` (`"{name} (vendor 0x{vendor:X4})"`, empty for an offscreen target or when `GetDesc` fails).
    #[must_use]
    pub fn adapter_description(&self) -> String {
        self.state.borrow().adapter_description.clone()
    }

    /// C# `QueryMaximumFrameLatency` (`0` without a swap chain). Test seam.
    pub fn query_maximum_frame_latency(&self) -> Result<u32, RenderFault> {
        let swap_chain = self.state.borrow().swap_chain.as_ref().map(|s| s.swap_chain.clone());
        match swap_chain {
            None => Ok(0),
            Some(swap_chain) => Ok(dxgi::maximum_frame_latency(&swap_chain)?),
        }
    }

    /// C# `Context`: a clone (an `AddRef`) of the device context. `Err(Disposed)` after `dispose`.
    pub fn context(&self) -> Result<ID2D1DeviceContext, RenderFault> {
        self.state.borrow().context.clone().ok_or_else(object_disposed_by_name)
    }

    /// C# `Brush`: the shared white solid colour brush (callers set colour and opacity per call).
    pub fn brush(&self) -> Result<ID2D1SolidColorBrush, RenderFault> {
        self.state.borrow().brush.clone().ok_or_else(object_disposed_by_name)
    }

    /// C# `Target`: the current render target bitmap (changes on `resize` and `recreate`; do not keep it across calls).
    pub fn target(&self) -> Result<ID2D1Bitmap1, RenderFault> {
        self.state.borrow().target.clone().ok_or_else(object_disposed_by_name)
    }

    /// The D3D11 device (the GPU image cache and the keep-alive need it; not in C#, which used the private field).
    pub fn d3d_device(&self) -> Result<ID3D11Device, RenderFault> {
        self.state.borrow().d3d.clone().ok_or_else(object_disposed_by_name)
    }

    /// C# `EndDrawFault = fault` (`None` clears it). Test seam; `pub` so the window-level tests (W8-13i, integration tests) can inject device loss and occlusion.
    #[cfg_attr(not(test), allow(dead_code))] // first production user is the surface (W8-12d); tests use it today
    pub fn set_end_draw_fault(&self, fault: Option<FaultHook>) {
        *self.end_draw_fault.borrow_mut() = fault;
    }

    /// C# `PresentFault = fault`. Test seam; `pub` so the window-level tests (W8-13i, integration tests) can inject device loss and occlusion.
    #[cfg_attr(not(test), allow(dead_code))] // first production user is the surface (W8-12d); tests use it today
    pub fn set_present_fault(&self, fault: Option<FaultHook>) {
        *self.present_fault.borrow_mut() = fault;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    //! Whole-object tests of `DeviceResources` (W8-12b/12c). Native tests use WARP and a hidden `STATIC` window; each says what to
    //! change if Windows disagrees.

    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::Duration;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
    use windows::core::w;

    use super::*;
    use crate::gfx::com::{D2DERR_RECREATE_TARGET_HRESULT, GfxError};

    /// A hidden `STATIC` popup window of 160 x 90 client pixels, destroyed on drop.
    pub(crate) struct HiddenWindow(HWND);

    impl HiddenWindow {
        pub(crate) fn new() -> Self {
            // SAFETY: "STATIC" is a system window class; static wide literals; no parent, menu, instance or creation parameter.
            // The window is never shown and is destroyed in `Drop` on this thread.
            let window = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("device test"),
                    WS_POPUP,
                    0,
                    0,
                    160,
                    90,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap();
            Self(window)
        }

        pub(crate) fn raw(&self) -> isize {
            self.0.0 as isize
        }
    }

    impl Drop for HiddenWindow {
        fn drop(&mut self) {
            // SAFETY: the handle was created by this thread in `new` and is destroyed once.
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    // C#: RenderError.Display texts (contract W8-12a "Error texts")
    #[test]
    fn render_fault_display_matches_render_error_texts() {
        let disposed = RenderFault::Disposed { object: "X.Y" };
        assert_eq!(
            disposed.to_string(),
            "Cannot access a disposed object.\r\nObject name: 'X.Y'."
        );
        assert_eq!(RenderFault::InvalidOperation { message: "boom" }.to_string(), "boom");
        let invalid = RenderFault::InvalidArgument {
            param: "hwnd",
            message: "A window handle is required.",
        };
        assert_eq!(invalid.to_string(), "A window handle is required. (Parameter 'hwnd')");
        assert_eq!(
            RenderFault::ArgumentOutOfRange { param: "pixelWidth" }.to_string(),
            "Specified argument was out of the range of valid values. (Parameter 'pixelWidth')"
        );
        let device = RenderFault::Device(GfxError::new("op", 0x887A_0005_u32 as i32));
        assert_eq!(device.to_string(), "op failed with HRESULT 0x887A0005");
    }

    #[test]
    fn render_fault_display_equals_the_pr_app_render_error() {
        use pr_app::services::render::RenderError;
        let pairs = [
            (
                RenderFault::Disposed { object: "A.B" },
                RenderError::Disposed { object: "A.B" },
            ),
            (
                RenderFault::InvalidOperation { message: "m" },
                RenderError::InvalidOperation { message: "m" },
            ),
            (
                RenderFault::InvalidArgument {
                    param: "p",
                    message: "m",
                },
                RenderError::InvalidArgument {
                    param: "p",
                    message: "m",
                },
            ),
            (
                RenderFault::ArgumentOutOfRange { param: "p" },
                RenderError::ArgumentOutOfRange { param: "p" },
            ),
            (
                RenderFault::Device(GfxError::new("op", -5)),
                RenderError::Device {
                    hresult: -5,
                    operation: "op",
                },
            ),
        ];
        for (fault, error) in pairs {
            assert_eq!(fault.to_string(), error.to_string());
        }
    }

    #[test]
    fn render_fault_from_gfx_error_keeps_hresult_and_operation() {
        let error = GfxError::new("ID2D1Factory1::CreateDevice", -2_147_467_259);
        assert_eq!(RenderFault::from(error), RenderFault::Device(error));
    }

    #[test]
    fn render_fault_is_a_std_error() {
        fn takes_error(_: &dyn std::error::Error) {}
        takes_error(&RenderFault::ArgumentOutOfRange { param: "p" });
    }

    // C#: ObjectDisposedException(nameof(DeviceResources)) in the getters versus ThrowIf(_disposed, this) in Recreate/Resize
    #[test]
    fn getters_on_an_unbuilt_object_report_the_short_name() {
        let resources = DeviceResources::new_unbuilt(0, 8, 8, 1.0, true, 1);
        let expected = RenderFault::Disposed {
            object: "DeviceResources",
        };
        assert_eq!(resources.context().unwrap_err(), expected);
        assert_eq!(resources.brush().unwrap_err(), expected);
        assert_eq!(resources.target().unwrap_err(), expected);
        assert_eq!(resources.d3d_device().unwrap_err(), expected);
        assert_eq!(resources.end_draw().unwrap_err(), expected);
        assert_eq!(resources.present(1), 0);
        assert_eq!(resources.frame_latency_wait_handle(), 0);
        assert_eq!(resources.adapter_description(), "");
        assert_eq!(resources.query_maximum_frame_latency().unwrap(), 0);
    }

    // C#: the private DeviceResources constructor clamps
    #[test]
    fn new_unbuilt_clamps_size_dpi_and_latency() {
        let resources = DeviceResources::new_unbuilt(7, -5, 0, f64::NAN, false, 99);
        assert_eq!((resources.pixel_width(), resources.pixel_height()), (1, 1));
        assert_eq!(resources.dpi_scale(), 1.0);
        assert_eq!(resources.max_frame_latency, 16);
        assert_eq!(resources.hwnd(), 7);
        assert_eq!(resources.generation(), 0);
        assert!(!resources.is_warp());
        assert_eq!(resources.live_bitmap_count(), 0);
    }

    // C#: D2DRenderSurfaceTests.Offscreen_IsWarpWithoutWindowOrWaitHandle
    #[test]
    #[ignore = "native: needs desktop"]
    fn offscreen_is_warp_without_window_or_wait_handle() {
        let r = DeviceResources::create_offscreen(64, 32, 1.25, true).unwrap();
        assert!(r.is_warp());
        assert_eq!((r.pixel_width(), r.pixel_height()), (64, 32));
        assert_eq!(r.dpi_scale(), 1.25);
        assert_eq!(r.hwnd(), 0);
        assert_eq!(r.frame_latency_wait_handle(), 0);
        assert!(!r.has_swap_chain());
        assert!(r.max_bitmap_size() >= 4096);
        assert!(r.adapter_description().is_empty());
        assert_eq!(r.query_maximum_frame_latency().unwrap(), 0);
        assert!(r.target().is_ok());
    }

    // C#: D2DWindowSurfaceTests.WindowSurface_FlipModelWaitableSwapChain_LatencyOne_PresentsAndResizes (first assertions)
    #[test]
    #[ignore = "native: needs desktop"]
    fn window_resources_have_a_waitable_flip_swap_chain_with_latency_one() {
        let window = HiddenWindow::new();
        let r = DeviceResources::create_for_window(window.raw(), true, 1).unwrap();
        assert_eq!(r.hwnd(), window.raw());
        assert!(r.is_warp());
        assert_eq!((r.pixel_width(), r.pixel_height()), (160, 90));
        assert_ne!(r.frame_latency_wait_handle(), 0);
        assert_eq!(r.query_maximum_frame_latency().unwrap(), 1);
        assert!(r.has_swap_chain());
        assert!(
            r.adapter_description().contains("vendor 0x"),
            "{}",
            r.adapter_description()
        );
    }

    // C#: D2DWindowSurfaceTests.WindowSurface_MaxFrameLatencyOption_IsApplied
    #[test]
    #[ignore = "native: needs desktop"]
    fn window_resources_apply_max_frame_latency_option() {
        let window = HiddenWindow::new();
        for (asked, expected) in [(2, 2), (0, 1), (-3, 1), (99, 16)] {
            let r = DeviceResources::create_for_window(window.raw(), true, asked).unwrap();
            assert_eq!(r.query_maximum_frame_latency().unwrap(), expected, "asked {asked}");
        }
    }

    // C#: DeviceResources.Recreate (generation, same hwnd/size/DPI, seams survive)
    #[test]
    #[ignore = "native: needs desktop"]
    fn recreate_bumps_generation_keeps_size_and_hooks() {
        let window = HiddenWindow::new();
        let r = DeviceResources::create_for_window(window.raw(), true, 1).unwrap();
        let dpi = r.dpi_scale();
        r.set_end_draw_fault(Some(Box::new(|_| 4242)));
        assert_eq!(r.generation(), 0);
        for expected in [1, 2] {
            r.recreate().unwrap();
            assert_eq!(r.generation(), expected);
            assert_eq!((r.pixel_width(), r.pixel_height()), (160, 90));
            assert_eq!(r.dpi_scale(), dpi);
            assert_eq!(r.hwnd(), window.raw());
            assert_ne!(r.frame_latency_wait_handle(), 0);
            assert_eq!(r.live_bitmap_count(), 0);
            assert!(r.target().is_ok());
            // the hook set before the recreate is still installed
            // SAFETY: the context was created by `recreate` and no draw is open.
            unsafe { r.context().unwrap().BeginDraw() };
            assert_eq!(r.end_draw().unwrap(), 4242);
        }
    }

    // C#: D2DRenderSurfaceTests EndDrawFault / PresentFault seams
    #[test]
    #[ignore = "native: needs desktop"]
    fn end_draw_hook_rewrites_the_hresult_and_present_hook_is_skipped_offscreen() {
        let r = DeviceResources::create_offscreen(8, 8, 1.0, true).unwrap();
        r.set_end_draw_fault(Some(Box::new(|real| {
            assert_eq!(real, 0, "the hook sees the real HRESULT");
            D2DERR_RECREATE_TARGET_HRESULT
        })));
        let present_calls = Rc::new(std::cell::Cell::new(0));
        let seen = Rc::clone(&present_calls);
        r.set_present_fault(Some(Box::new(move |hr| {
            seen.set(seen.get() + 1);
            hr
        })));
        // SAFETY: the context is live and its target is set; the draw is ended by `end_draw` below.
        unsafe { r.context().unwrap().BeginDraw() };
        assert_eq!(r.end_draw().unwrap(), D2DERR_RECREATE_TARGET_HRESULT);
        assert_eq!(r.present(1), 0);
        assert_eq!(present_calls.get(), 0);
    }

    // C#: DeviceResources.Present with a swap chain goes through PresentFault
    #[test]
    #[ignore = "native: needs desktop"]
    fn present_hook_replaces_the_hresult_with_a_swap_chain() {
        let window = HiddenWindow::new();
        let r = DeviceResources::create_for_window(window.raw(), true, 1).unwrap();
        r.set_present_fault(Some(Box::new(|_| 77)));
        assert_eq!(r.present(0), 77);
    }

    // C#: DeviceResources.Dispose idempotent; getters throw ObjectDisposedException
    #[test]
    #[ignore = "native: needs desktop"]
    fn dispose_is_idempotent_and_getters_report_disposed() {
        let window = HiddenWindow::new();
        let r = DeviceResources::create_for_window(window.raw(), true, 1).unwrap();
        r.dispose();
        r.dispose();
        let short = RenderFault::Disposed {
            object: "DeviceResources",
        };
        assert_eq!(r.context().unwrap_err(), short);
        assert_eq!(r.brush().unwrap_err(), short);
        assert_eq!(r.target().unwrap_err(), short);
        assert_eq!(r.d3d_device().unwrap_err(), short);
        assert_eq!(r.end_draw().unwrap_err(), short);
        assert_eq!(r.recreate().unwrap_err(), object_disposed());
        assert_eq!(r.present(1), 0);
        assert_eq!(r.frame_latency_wait_handle(), 0);
        assert!(!r.has_swap_chain());
        assert_eq!((r.pixel_width(), r.pixel_height()), (160, 90));
        drop(r);
    }

    // C#: DeviceResources.ReleaseAll comment (flush while the swap chain is alive, else the CI runner hangs)
    #[test]
    #[ignore = "native: needs desktop"]
    fn drop_releases_everything_in_order() {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let window = HiddenWindow::new();
            for cycle in 0..50 {
                let r = DeviceResources::create_for_window(window.raw(), true, 1).unwrap();
                drop(r);
                if tx.send(cycle).is_err() {
                    return;
                }
            }
        });
        for cycle in 0..50 {
            let got = rx.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|_| {
                panic!("create/drop cycle {cycle} hung for 10 s (ClearStateAndFlush order, trap T1)")
            });
            assert_eq!(got, cycle);
        }
    }
}
