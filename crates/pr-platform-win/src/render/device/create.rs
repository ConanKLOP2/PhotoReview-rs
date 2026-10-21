//! Device creation of `DeviceResources` (W8-12b): `DeviceResources.cs` lines 14 to 122 (fields, constructor, `CreateForWindow`,
//! `CreateOffscreen`) and 246 to 349 (`CreateAll`, `CreateDevice`, `CreateSwapChain`, `ReadAdapterDescription`), baseline `d92d6aae`.
//! Contract: `contracts/W8-12b.md`.
//!
//! The recipe is the `gfx` helpers in this order: `d3d11::create_device_with_fallback` (hardware, else WARP), `d2d::create_factory`,
//! `d3d11::dxgi_device`, `d2d::create_device_and_context`, `d2d::max_bitmap_size`, `d2d::create_solid_color_brush(white)`; for a window
//! `dxgi::create_window_swap_chain` (adapter name comes with it); last `resources::create_target` (W8-12c).

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

use super::{DeviceResources, DeviceState, RenderFault, object_disposed};
use crate::gfx::{com, d2d, d3d11, dxgi};

/// Environment variable that picks the hardware adapter by index for the attended R0001 re-measure (D-W8B3G-8, D-W8B3R-6). Unset,
/// empty, not a `u32`, or `prefer_warp`: the default adapter is used exactly like the C#.
pub const ADAPTER_ENV: &str = "PHOTOREVIEW_RS_GPU_ADAPTER";

/// `Math.Max(1, value)` (C# constructor lines 39 and 40).
#[must_use]
pub fn clamp_dimension(value: i32) -> i32 {
    value.max(1)
}

/// `dpiScale > 0 ? dpiScale : 1` (C# constructor line 41; NaN and `<= 0` give `1.0`).
#[must_use]
pub fn normalize_dpi_scale(dpi_scale: f64) -> f64 {
    if dpi_scale > 0.0 { dpi_scale } else { 1.0 }
}

/// `dpi == 0 ? 1 : dpi / 96.0` (C# `CreateForWindow` line 110; `GetDpiForWindow` returns 0 for an invalid window).
#[must_use]
pub fn dpi_scale_from_window_dpi(dpi: u32) -> f64 {
    if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
}

/// Pure parse of [`ADAPTER_ENV`]: trimmed, decimal `u32`; anything else is `None`.
#[must_use]
pub fn parse_adapter_index(value: Option<&str>) -> Option<u32> {
    let text = value?.trim();
    // `u32::from_str` also accepts a leading `+`; the variable is a plain decimal index.
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok()
}

impl DeviceResources {
    /// C# `CreateForWindow(hwnd, options)` with the three `RenderSurfaceOptions` fields that matter here (`allow_tearing` is the
    /// surface's). `hwnd == 0` is `Err(InvalidArgument { param: "hwnd", message: "A window handle is required." })`. Size is the client
    /// rectangle of the window (`GetClientRect`; `1 x 1` when the call fails), DPI scale is [`dpi_scale_from_window_dpi`] of
    /// `GetDpiForWindow`. `max_frame_latency` is clamped to 1..=16. Builds everything ([`DeviceResources::create_all`]); a failure
    /// drops what was built (C# leaked it until finalization; Rust `Drop`).
    pub fn create_for_window(hwnd: isize, prefer_warp: bool, max_frame_latency: i32) -> Result<Rc<Self>, RenderFault> {
        if hwnd == 0 {
            return Err(RenderFault::InvalidArgument {
                param: "hwnd",
                message: "A window handle is required.",
            });
        }
        let handle = com::hwnd(hwnd);
        let mut width = 1;
        let mut height = 1;
        let mut client = RECT::default();
        // SAFETY: `GetClientRect` accepts any window handle value and fails cleanly for an invalid one; `client` is a local that
        // outlives the call.
        if unsafe { GetClientRect(handle, &mut client) }.is_ok() {
            width = client.right.saturating_sub(client.left);
            height = client.bottom.saturating_sub(client.top);
        }
        // SAFETY: `GetDpiForWindow` accepts any window handle value and returns 0 for an invalid one; no pointers are passed.
        let dpi = unsafe { GetDpiForWindow(handle) };
        let resources = Rc::new(Self::new_unbuilt(
            hwnd,
            width,
            height,
            dpi_scale_from_window_dpi(dpi),
            prefer_warp,
            max_frame_latency,
        ));
        resources.create_all()?;
        Ok(resources)
    }

    /// C# `CreateOffscreen(pixelWidth, pixelHeight, dpiScale, preferWarp)`. A size below 1 is `Err(ArgumentOutOfRange { param:
    /// "pixelWidth" | "pixelHeight" })` (width is checked first). `dpi_scale` is normalised ([`normalize_dpi_scale`]), latency is 1.
    pub fn create_offscreen(
        pixel_width: i32,
        pixel_height: i32,
        dpi_scale: f64,
        prefer_warp: bool,
    ) -> Result<Rc<Self>, RenderFault> {
        if pixel_width < 1 {
            return Err(RenderFault::ArgumentOutOfRange { param: "pixelWidth" });
        }
        if pixel_height < 1 {
            return Err(RenderFault::ArgumentOutOfRange { param: "pixelHeight" });
        }
        let resources = Rc::new(Self::new_unbuilt(
            0,
            pixel_width,
            pixel_height,
            dpi_scale,
            prefer_warp,
            1,
        ));
        resources.create_all()?;
        Ok(resources)
    }

    /// The private constructor (C# lines 34 to 42): clamps size, DPI and latency; `generation == 0`; nothing is created yet.
    pub(super) fn new_unbuilt(
        hwnd: isize,
        pixel_width: i32,
        pixel_height: i32,
        dpi_scale: f64,
        prefer_warp: bool,
        max_frame_latency: i32,
    ) -> Self {
        Self {
            hwnd,
            prefer_warp,
            max_frame_latency: dxgi::clamp_frame_latency(max_frame_latency),
            state: RefCell::new(DeviceState {
                pixel_width: clamp_dimension(pixel_width),
                pixel_height: clamp_dimension(pixel_height),
                dpi_scale: normalize_dpi_scale(dpi_scale),
                ..DeviceState::default()
            }),
            live_bitmap_count: Cell::new(0),
            end_draw_fault: RefCell::new(None),
            present_fault: RefCell::new(None),
            _not_send: PhantomData,
        }
    }

    /// C# `CreateAll`: `create_device`, then `create_swap_chain` when `hwnd != 0`, then `resources::create_target` (W8-12c).
    /// Also used by `recreate`. On an error the half-built state is dropped (C# left it for finalization).
    pub(super) fn create_all(&self) -> Result<(), RenderFault> {
        let built = self.create_all_steps();
        if built.is_err() {
            self.release_all();
        }
        built
    }

    fn create_all_steps(&self) -> Result<(), RenderFault> {
        self.create_device()?;
        if self.hwnd != 0 {
            self.create_swap_chain(com::hwnd(self.hwnd))?;
        }
        self.create_target()
    }

    /// C# `CreateDevice` (lines 257 to 288): device (hardware unless `prefer_warp` or the optional [`ADAPTER_ENV`] adapter fails the
    /// normal path; WARP fallback), `is_warp`, factory, DXGI device, D2D device and context, `max_bitmap_size`, white brush.
    pub(super) fn create_device(&self) -> Result<(), RenderFault> {
        let adapter_index = if self.prefer_warp {
            None
        } else {
            parse_adapter_index(std::env::var(ADAPTER_ENV).ok().as_deref())
        };
        // The override is tried first; its failure (no such adapter) falls through to the normal path.
        let on_adapter =
            adapter_index.and_then(|index| d3d11::create_device_on_adapter(index, d3d11::DEFAULT_DEVICE_FLAGS).ok());
        let created = match on_adapter {
            Some(created) => created,
            None => d3d11::create_device_with_fallback(self.prefer_warp)?,
        };
        let factory = d2d::create_factory(0)?;
        // Same COM object as the D3D device (a `QueryInterface`).
        let dxgi_device = d3d11::dxgi_device(&created.device)?;
        let (device, context) = d2d::create_device_and_context(&factory, &dxgi_device)?;
        let max_bitmap_size = d2d::max_bitmap_size(&context);
        let white = D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        };
        let brush = d2d::create_solid_color_brush(&context, white)?;

        let mut state = self.state.borrow_mut();
        state.is_warp = created.is_warp;
        state.d3d = Some(created.device);
        state.factory = Some(factory);
        state.device = Some(device);
        state.context = Some(context);
        state.max_bitmap_size = max_bitmap_size;
        state.brush = Some(brush);
        Ok(())
    }

    /// C# `CreateSwapChain` + `ReadAdapterDescription` (lines 290 to 349): `dxgi::create_window_swap_chain` with the clamped latency,
    /// the adapter description from the returned `AdapterInfo::display`. A null waitable handle after success is an error
    /// (D-W8B3G-3, decided in `gfx`).
    pub(super) fn create_swap_chain(&self, hwnd: HWND) -> Result<(), RenderFault> {
        let (d3d, width, height) = {
            let state = self.state.borrow();
            (state.d3d.clone(), state.pixel_width, state.pixel_height)
        };
        let d3d = d3d.ok_or_else(object_disposed)?;
        // The sizes are `>= 1` (`clamp_dimension`), so the conversion cannot fail.
        let width = u32::try_from(width).unwrap_or(1);
        let height = u32::try_from(height).unwrap_or(1);
        let created = dxgi::create_window_swap_chain(&d3d, hwnd, width, height, self.max_frame_latency)?;
        let description = created
            .adapter
            .as_ref()
            .map(dxgi::AdapterInfo::display)
            .unwrap_or_default();
        let mut state = self.state.borrow_mut();
        state.adapter_description = description;
        state.swap_chain = Some(created);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::super::tests::HiddenWindow;
    use super::*;

    /// Serializes the tests that set `ADAPTER_ENV`.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // C#: new DeviceResources(..) Math.Max(1, size)
    #[test]
    fn clamp_dimension_floors_at_one() {
        for value in [0, -1, i32::MIN] {
            assert_eq!(clamp_dimension(value), 1, "{value}");
        }
        for value in [1, 7, i32::MAX] {
            assert_eq!(clamp_dimension(value), value);
        }
    }

    // C#: new DeviceResources(..) dpiScale > 0 ? dpiScale : 1
    #[test]
    fn normalize_dpi_scale_defaults_to_one() {
        for value in [0.0, -1.0, f64::NAN, -0.0, f64::NEG_INFINITY] {
            assert_eq!(normalize_dpi_scale(value), 1.0, "{value}");
        }
        assert_eq!(normalize_dpi_scale(1.25), 1.25);
        assert_eq!(normalize_dpi_scale(0.0001), 0.0001);
    }

    // C#: CreateForWindow dpi == 0 ? 1 : dpi / 96.0
    #[test]
    fn dpi_scale_from_window_dpi_matches_csharp() {
        assert_eq!(dpi_scale_from_window_dpi(0), 1.0);
        assert_eq!(dpi_scale_from_window_dpi(96), 1.0);
        assert_eq!(dpi_scale_from_window_dpi(144), 1.5);
        assert_eq!(dpi_scale_from_window_dpi(120), 1.25);
        assert_eq!(dpi_scale_from_window_dpi(192), 2.0);
    }

    #[test]
    fn parse_adapter_index_accepts_only_u32() {
        assert_eq!(parse_adapter_index(None), None);
        assert_eq!(parse_adapter_index(Some("")), None);
        assert_eq!(parse_adapter_index(Some(" 1 ")), Some(1));
        assert_eq!(parse_adapter_index(Some("0")), Some(0));
        for bad in ["-1", "+1", "1.0", "x", "4294967296", "1 2"] {
            assert_eq!(parse_adapter_index(Some(bad)), None, "{bad}");
        }
        assert_eq!(parse_adapter_index(Some("4294967295")), Some(u32::MAX));
    }

    // C#: D2DWindowSurfaceTests.CreateForWindow_RejectsNullHandle
    #[test]
    fn create_for_window_rejects_a_null_handle_before_any_com_call() {
        let error = DeviceResources::create_for_window(0, true, 1).unwrap_err();
        assert_eq!(
            error,
            RenderFault::InvalidArgument {
                param: "hwnd",
                message: "A window handle is required."
            }
        );
    }

    // C#: ArgumentOutOfRangeException.ThrowIfLessThan(pixelWidth, 1) then pixelHeight
    #[test]
    fn create_offscreen_rejects_sizes_below_one() {
        let width = RenderFault::ArgumentOutOfRange { param: "pixelWidth" };
        let height = RenderFault::ArgumentOutOfRange { param: "pixelHeight" };
        assert_eq!(DeviceResources::create_offscreen(0, 0, 1.0, true).unwrap_err(), width);
        assert_eq!(DeviceResources::create_offscreen(-1, 5, 1.0, true).unwrap_err(), width);
        assert_eq!(DeviceResources::create_offscreen(5, 0, 1.0, true).unwrap_err(), height);
        assert_eq!(
            DeviceResources::create_offscreen(5, i32::MIN, 1.0, true).unwrap_err(),
            height
        );
    }

    // C#: CreateDevice (WARP when asked); no target yet, so this is green before the target exists
    #[test]
    #[ignore = "native: needs desktop"]
    fn create_device_stack_without_target() {
        let resources = DeviceResources::new_unbuilt(0, 16, 16, 1.0, true, 1);
        resources.create_device().unwrap();
        assert!(resources.is_warp(), "prefer_warp must produce a WARP device");
        assert!(resources.max_bitmap_size() >= 4096);
        assert!(resources.brush().is_ok());
        assert!(resources.context().is_ok());
        assert!(resources.d3d_device().is_ok());
        assert_eq!(resources.generation(), 0);
        assert!(resources.target().is_err(), "the target is created by create_target");
    }

    // C#: CreateForWindow GetClientRect / GetDpiForWindow (a bad handle keeps 1 x 1 and fails in CreateSwapChain)
    #[test]
    #[ignore = "native: needs desktop"]
    fn create_for_window_with_a_stale_handle_fails_with_a_device_error() {
        let window = HiddenWindow::new();
        let raw = window.raw();
        drop(window);
        let error = DeviceResources::create_for_window(raw, true, 1).unwrap_err();
        assert!(matches!(error, RenderFault::Device(_)), "{error:?}");
    }

    // New (D-W8B3R-6): the adapter override falls back to the normal path when the index has no adapter
    #[test]
    #[ignore = "native: needs desktop"]
    fn adapter_env_override_falls_back_when_the_index_does_not_exist() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = std::env::var_os(ADAPTER_ENV);
        // SAFETY: the tests that touch this variable are serialized by `ENV_LOCK`; no other thread reads it concurrently
        // (device creation reads it on the test thread of the caller only).
        unsafe { std::env::set_var(ADAPTER_ENV, "99") };
        let created = DeviceResources::create_offscreen(16, 16, 1.0, false);
        // SAFETY: same as above.
        unsafe {
            match &previous {
                Some(value) => std::env::set_var(ADAPTER_ENV, value),
                None => std::env::remove_var(ADAPTER_ENV),
            }
        };
        let resources = created.expect("creation must fall back to the default adapter or WARP");
        assert!(resources.target().is_ok());
    }
}
