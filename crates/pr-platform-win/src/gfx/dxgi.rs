//! Port of `PhotoReview.Shell.Interop/Graphics/Dxgi.cs` (264 lines, WP-13b), baseline `d92d6aae`. Unit W8-11g (audit; thin helpers).
//!
//! The 13 DXGI interfaces, the enums, `DXGI_SWAP_CHAIN_DESC1`, `DXGI_PRESENT_PARAMETERS` and `CreateDXGIFactory2` come from
//! `windows-rs`; the `Reserved` slots are retired. What stays is the window swap chain recipe of `DeviceResources.CreateSwapChain`
//! and the `Present`/`ResizeBuffers` HRESULT rules of `D2DRenderSurface`. Table: `contracts/W8-11g.md`.

#![allow(unused_variables, dead_code)] // W8-11g stub: the porter removes this line when the last `todo!()` is gone.

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Dxgi::{
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT, IDXGIAdapter,
    IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
};
use windows::core::Owned;

use super::com::GfxError;

/// `DeviceResources.SwapChainBufferCount`.
pub const SWAP_CHAIN_BUFFER_COUNT: u32 = 2;
/// `DeviceResources.SwapChainFlags` = `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT` (0x40). The SAME flags must be passed to
/// every `ResizeBuffers`, or the call fails with `DXGI_ERROR_INVALID_CALL`.
pub const SWAP_CHAIN_FLAGS: DXGI_SWAP_CHAIN_FLAG = DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT;
/// Lower bound of `Math.Clamp(maxFrameLatency, 1, 16)` in the `DeviceResources` constructor.
pub const MIN_FRAME_LATENCY: u32 = 1;
/// Upper bound of the same clamp.
pub const MAX_FRAME_LATENCY: u32 = 16;

/// `Math.Clamp(value, 1, 16)` as `u32` (negative and 0 give 1).
#[must_use]
pub fn clamp_frame_latency(value: i32) -> u32 {
    todo!("W8-11g")
}

/// The swap chain description of `DeviceResources.CreateSwapChain` (pure): `Width`/`Height` as given (the caller already applied
/// `max(1, ..)`), `Format = B8G8R8A8_UNORM`, `Stereo = false`, `SampleDesc = {1, 0}`, `BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT`
/// (0x20), `BufferCount = 2`, `Scaling = DXGI_SCALING_STRETCH`, `SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD`,
/// `AlphaMode = DXGI_ALPHA_MODE_IGNORE`, `Flags = SWAP_CHAIN_FLAGS`. NOT the spike values (FlipSequential, Scaling None, flags 0):
/// the C# production values win (D-W8B3G-3).
#[must_use]
pub fn window_swap_chain_desc(width: u32, height: u32) -> DXGI_SWAP_CHAIN_DESC1 {
    todo!("W8-11g")
}

/// Name and vendor of the adapter the device runs on (diagnostics; C# `DeviceResources.AdapterDescription`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfo {
    /// `DXGI_ADAPTER_DESC::Description` up to the first NUL.
    pub description: String,
    /// `DXGI_ADAPTER_DESC::VendorId`.
    pub vendor_id: u32,
}

impl AdapterInfo {
    /// `"{description} (vendor 0x{vendor_id:X4})"` (C# `$"{name} (vendor 0x{vendor:X4})"`).
    #[must_use]
    pub fn display(&self) -> String {
        todo!("W8-11g")
    }
}

/// Reads `IDXGIAdapter::GetDesc` (C# read the raw `DXGI_ADAPTER_DESC` bytes: name at offset 0, `VendorId` at offset 256). A failing call
/// gives `None` (C# left the string empty).
#[must_use]
pub fn adapter_info(adapter: &IDXGIAdapter) -> Option<AdapterInfo> {
    todo!("W8-11g")
}

/// A flip-model swap chain for a window with its frame-latency waitable object.
#[derive(Debug)]
pub struct WindowSwapChain {
    /// The swap chain (`IDXGISwapChain1`; cast to `IDXGISwapChain2` when needed).
    pub swap_chain: IDXGISwapChain1,
    /// `IDXGISwapChain2::GetFrameLatencyWaitableObject`. Owned: closed on drop (C# `DeviceResources` called `CloseHandle`; the comment
    /// in `Dxgi.cs` saying "do not close" contradicts the code that ran in production).
    pub frame_latency_waitable: Owned<HANDLE>,
    /// The adapter the device is on, when readable.
    pub adapter: Option<AdapterInfo>,
}

/// `DeviceResources.CreateSwapChain`: `device.cast::<IDXGIDevice>().GetAdapter()`, `adapter.GetParent::<IDXGIFactory2>()`,
/// `CreateSwapChainForHwnd(device, hwnd, &window_swap_chain_desc(w, h), None, None)`, then `cast::<IDXGISwapChain2>()`,
/// `SetMaximumFrameLatency(max_frame_latency)` and `GetFrameLatencyWaitableObject()`. The adapter and the factory are dropped
/// before returning. `max_frame_latency` must already be clamped ([`clamp_frame_latency`]).
pub fn create_window_swap_chain(
    device: &ID3D11Device,
    hwnd: HWND,
    width: u32,
    height: u32,
    max_frame_latency: u32,
) -> Result<WindowSwapChain, GfxError> {
    todo!("W8-11g")
}

/// `IDXGISwapChain::ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, SWAP_CHAIN_FLAGS)`. Precondition (not checkable here): every
/// reference to the back buffer is gone (Direct2D `SetTarget(None)`, target bitmap dropped, then `d3d11::clear_state_and_flush`).
/// The error keeps the HRESULT so the caller can test `is_device_lost` (C# `Resize` returned the raw HRESULT).
pub fn resize_buffers(swap_chain: &IDXGISwapChain1, width: u32, height: u32) -> Result<(), GfxError> {
    todo!("W8-11g")
}

/// `IDXGISwapChain1::GetBuffer(0)` as the surface Direct2D wraps into the target bitmap.
pub fn back_buffer(swap_chain: &IDXGISwapChain1) -> Result<IDXGISurface, GfxError> {
    todo!("W8-11g")
}

/// `IDXGISwapChain2::GetMaximumFrameLatency` (C# `DeviceResources.QueryMaximumFrameLatency`, test seam).
pub fn maximum_frame_latency(swap_chain: &IDXGISwapChain1) -> Result<u32, GfxError> {
    todo!("W8-11g")
}

/// `IDXGISwapChain::Present(sync_interval, 0)` and the HRESULT as it came (a SUCCESS code may carry meaning). Seam for the
/// `PresentFault` test hook of `DeviceResources`, which rewrites the HRESULT before [`classify_present`].
#[must_use]
pub fn present_raw(swap_chain: &IDXGISwapChain1, sync_interval: u32) -> i32 {
    todo!("W8-11g")
}

/// What a `Present` HRESULT means (C# `D2DRenderSurface.Present`): `DXGI_STATUS_OCCLUDED` is `Occluded` (checked FIRST, it is a
/// success code); a device-loss code is `DeviceLost`; any other negative value is `Failed`; the rest is `Presented`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentOutcome {
    /// HRESULT >= 0 other than `DXGI_STATUS_OCCLUDED`.
    Presented,
    /// `DXGI_STATUS_OCCLUDED` (0x087A0001).
    Occluded,
    /// `D2DERR_RECREATE_TARGET`, `DXGI_ERROR_DEVICE_REMOVED` or `DXGI_ERROR_DEVICE_RESET`: the caller recreates the device.
    DeviceLost(i32),
    /// Any other HRESULT < 0.
    Failed(i32),
}

/// Pure classification of a `Present` HRESULT (see [`PresentOutcome`]).
#[must_use]
pub fn classify_present(hresult: i32) -> PresentOutcome {
    todo!("W8-11g")
}

/// `CreateDXGIFactory2(0)` as `IDXGIFactory2` (C# `Dxgi.CreateDxgiFactory2(0, IidDxgiFactory2)`; flag 1 = debug is not used).
pub fn create_factory2() -> Result<IDXGIFactory2, GfxError> {
    todo!("W8-11g")
}
