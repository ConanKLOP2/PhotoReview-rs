//! Port of `PhotoReview.Shell.Interop/Graphics/ComInterop.cs` (70 lines, WP-13b), baseline `d92d6aae`. Unit W8-11f.
//!
//! The C# class existed to turn raw COM pointers into `[GeneratedComInterface]` wrappers and to release a wrapper's COM
//! reference immediately (`ComInterop.Release`), because the .NET wrapper cache kept references alive and `ResizeBuffers` then failed.
//! `windows-rs` interfaces are reference counted Rust values: `Drop` releases at once, `Interface::cast` is `QueryInterface`,
//! `Clone` is `AddRef`. So `Wrap`, `QueryInterface` and `Release` are retired; what remains is the HRESULT side (`Check`,
//! `DeviceLoss`, `Describe`), the error type shared by all `gfx` helpers and the `isize` to `HWND` conversion.

#![allow(unused_variables, dead_code)] // W8-11f stub: the porter removes this line when the last `todo!()` is gone.

use core::fmt;

use windows::Win32::Foundation::HWND;

/// `D2DERR_RECREATE_TARGET` (C# `D2dConstants.ErrorRecreateTarget`): the render target / device is gone, rebuild everything.
pub const D2DERR_RECREATE_TARGET_HRESULT: i32 = 0x8899_000Cu32 as i32;
/// `DXGI_ERROR_DEVICE_REMOVED` (C# `DxgiConstants.ErrorDeviceRemoved`).
pub const DXGI_ERROR_DEVICE_REMOVED_HRESULT: i32 = 0x887A_0005u32 as i32;
/// `DXGI_ERROR_DEVICE_RESET` (C# `DxgiConstants.ErrorDeviceReset`).
pub const DXGI_ERROR_DEVICE_RESET_HRESULT: i32 = 0x887A_0007u32 as i32;
/// `DXGI_ERROR_WAS_STILL_DRAWING` (C# `DxgiConstants.ErrorWasStillDrawing`; unused by the C# renderer, kept for the audit).
pub const DXGI_ERROR_WAS_STILL_DRAWING_HRESULT: i32 = 0x887A_000Au32 as i32;
/// `DXGI_STATUS_OCCLUDED` (C# `DxgiConstants.StatusOccluded`), a SUCCESS code: `Present` returns it when the window is not visible.
pub const DXGI_STATUS_OCCLUDED_HRESULT: i32 = 0x087A_0001;

/// A failed graphics call: the HRESULT plus the name of the call (the first argument of every `gfx` helper). Replaces the
/// `COMException` that `ComInterop.Check` threw. `W8-12*` convert it one to one into `RenderError::Device { hresult, operation }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GfxError {
    /// The failing HRESULT (negative).
    pub hresult: i32,
    /// The API that failed, for example `"D3D11CreateDevice(WARP)"`.
    pub operation: &'static str,
}

impl GfxError {
    /// Builds an error from an operation name and an HRESULT.
    #[must_use]
    pub const fn new(operation: &'static str, hresult: i32) -> Self {
        Self { hresult, operation }
    }

    /// Wraps a `windows` error, keeping its HRESULT.
    #[must_use]
    pub fn from_windows(operation: &'static str, error: &windows::core::Error) -> Self {
        todo!("W8-11f: Self::new(operation, error.code().0)")
    }

    /// `true` for the three device-loss HRESULTs ([`is_device_lost`]).
    #[must_use]
    pub fn is_device_lost(&self) -> bool {
        todo!("W8-11f: is_device_lost(self.hresult)")
    }
}

impl fmt::Display for GfxError {
    /// `"{operation} failed with HRESULT 0x{hresult as u32:08X}"` (the same text as `RenderError::Device`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W8-11f")
    }
}

impl std::error::Error for GfxError {}

/// C# `ComInterop.Check(hr)`: `Ok(())` for any HRESULT >= 0 (S_FALSE and `DXGI_STATUS_OCCLUDED` are success), else `Err`.
pub fn check(operation: &'static str, hresult: i32) -> Result<(), GfxError> {
    todo!("W8-11f: Err(GfxError::new(operation, hresult)) when hresult < 0, else Ok")
}

/// C# `DeviceLoss.IsDeviceLost`: `D2DERR_RECREATE_TARGET`, `DXGI_ERROR_DEVICE_REMOVED` or `DXGI_ERROR_DEVICE_RESET`. Nothing else
/// (`DXGI_ERROR_WAS_STILL_DRAWING` and `DXGI_STATUS_OCCLUDED` are not losses).
#[must_use]
pub fn is_device_lost(hresult: i32) -> bool {
    todo!("W8-11f")
}

/// C# `DeviceLoss.Describe`: `"0x"` plus the HRESULT as `u32` in 8 upper-case hex digits (`-2005270523` gives `0x887A0005`).
#[must_use]
pub fn describe_hresult(hresult: i32) -> String {
    todo!("W8-11f")
}

/// Maps a `windows` result to `GfxError`, naming the call. Use as `unsafe { factory.CreateDevice(&dxgi) }.gfx("ID2D1Factory1::CreateDevice")?`.
pub trait ResultExt<T> {
    /// `Err(e)` becomes `Err(GfxError { hresult: e.code().0, operation })`.
    fn gfx(self, operation: &'static str) -> Result<T, GfxError>;
}

impl<T> ResultExt<T> for windows::core::Result<T> {
    fn gfx(self, operation: &'static str) -> Result<T, GfxError> {
        todo!("W8-11f")
    }
}

/// The `isize` window handle of the `pr_app` contracts (`RenderSurface::hwnd`, C# `nint`) as a `windows` `HWND` (D-W8-16e-2 precedent).
#[must_use]
pub fn hwnd(raw: isize) -> HWND {
    todo!("W8-11f: HWND(raw as *mut core::ffi::c_void)")
}
