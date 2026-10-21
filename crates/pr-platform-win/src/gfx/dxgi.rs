//! Port of `PhotoReview.Shell.Interop/Graphics/Dxgi.cs` (264 lines, WP-13b), baseline `d92d6aae`. Unit W8-11g (audit; thin helpers).
//!
//! The 13 DXGI interfaces, the enums, `DXGI_SWAP_CHAIN_DESC1`, `DXGI_PRESENT_PARAMETERS` and `CreateDXGIFactory2` come from
//! `windows-rs`; the `Reserved` slots are retired. What stays is the window swap chain recipe of `DeviceResources.CreateSwapChain`
//! and the `Present`/`ResizeBuffers` HRESULT rules of `D2DRenderSurface`. Table: `contracts/W8-11g.md`.

use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, HANDLE, HWND};
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIFactory2, IDXGIOutput, IDXGISurface, IDXGISwapChain1,
    IDXGISwapChain2,
};
use windows::core::{Interface, Owned};

use super::com::{DXGI_STATUS_OCCLUDED_HRESULT, GfxError, ResultExt, is_device_lost};
use super::d3d11::dxgi_device;
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
    // `MIN_FRAME_LATENCY` and `MAX_FRAME_LATENCY` fit in `i32`; the clamped value is within 1..=16, so the conversion cannot fail.
    let clamped = value.clamp(MIN_FRAME_LATENCY as i32, MAX_FRAME_LATENCY as i32);
    u32::try_from(clamped).unwrap_or(MIN_FRAME_LATENCY)
}

/// The swap chain description of `DeviceResources.CreateSwapChain` (pure): `Width`/`Height` as given (the caller already applied
/// `max(1, ..)`), `Format = B8G8R8A8_UNORM`, `Stereo = false`, `SampleDesc = {1, 0}`, `BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT`
/// (0x20), `BufferCount = 2`, `Scaling = DXGI_SCALING_STRETCH`, `SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD`,
/// `AlphaMode = DXGI_ALPHA_MODE_IGNORE`, `Flags = SWAP_CHAIN_FLAGS`. NOT the spike values (FlipSequential, Scaling None, flags 0):
/// the C# production values win (D-W8B3G-3).
#[must_use]
pub fn window_swap_chain_desc(width: u32, height: u32) -> DXGI_SWAP_CHAIN_DESC1 {
    DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: SWAP_CHAIN_BUFFER_COUNT,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: SWAP_CHAIN_FLAGS.0 as u32,
    }
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
        format!("{} (vendor 0x{:04X})", self.description, self.vendor_id)
    }
}

/// Reads `IDXGIAdapter::GetDesc` (C# read the raw `DXGI_ADAPTER_DESC` bytes: name at offset 0, `VendorId` at offset 256). A failing call
/// gives `None` (C# left the string empty).
#[must_use]
pub fn adapter_info(adapter: &IDXGIAdapter) -> Option<AdapterInfo> {
    // SAFETY: `adapter` is a live owned interface; the call takes no pointer arguments and returns the description by value.
    let desc = unsafe { adapter.GetDesc() }.ok()?;
    let end = desc
        .Description
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(desc.Description.len());
    let name = desc.Description.get(..end).unwrap_or(&[]);
    Some(AdapterInfo {
        description: String::from_utf16_lossy(name),
        vendor_id: desc.VendorId,
    })
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
    if hwnd.0.is_null() {
        return Err(GfxError::new("IDXGIFactory2::CreateSwapChainForHwnd", E_INVALIDARG.0));
    }
    let dxgi = dxgi_device(device)?;
    // SAFETY: `dxgi` is a live owned interface; the call returns an owned adapter.
    let adapter = unsafe { dxgi.GetAdapter() }.gfx("IDXGIDevice::GetAdapter")?;
    let info = adapter_info(&adapter);
    // SAFETY: `adapter` is a live owned interface; the parent is the factory that owns the device's adapter (trap T3).
    let factory: IDXGIFactory2 =
        unsafe { adapter.GetParent::<IDXGIFactory2>() }.gfx("IDXGIObject::GetParent(IDXGIFactory2)")?;
    let desc = window_swap_chain_desc(width, height);
    // SAFETY: `desc` is a local that outlives the call; `device` is a live owned device; `hwnd` is non-null (checked above) and the
    // caller guarantees it is a live window of this thread (a stale handle only makes the call fail); the optional fullscreen
    // description and output restriction are None.
    let swap_chain = unsafe { factory.CreateSwapChainForHwnd(device, hwnd, &desc, None, None::<&IDXGIOutput>) }
        .gfx("IDXGIFactory2::CreateSwapChainForHwnd")?;
    drop(factory);
    drop(adapter);
    let swap_chain2: IDXGISwapChain2 = swap_chain.cast().gfx("QueryInterface IDXGISwapChain2")?;
    // SAFETY: `swap_chain2` is a live owned interface; the latency is a plain number.
    unsafe { swap_chain2.SetMaximumFrameLatency(max_frame_latency) }.gfx("IDXGISwapChain2::SetMaximumFrameLatency")?;
    // SAFETY: `swap_chain2` is a live owned interface created with the waitable-object flag; the call returns a handle the caller closes.
    let raw = unsafe { swap_chain2.GetFrameLatencyWaitableObject() };
    if raw.is_invalid() {
        return Err(GfxError::new(
            "IDXGISwapChain2::GetFrameLatencyWaitableObject",
            E_FAIL.0,
        ));
    }
    // SAFETY: `raw` is a valid, non-null handle that DXGI documents as caller-closed (W8-11f trap T4); `Owned` is its only owner.
    let frame_latency_waitable = unsafe { Owned::new(raw) };
    Ok(WindowSwapChain {
        swap_chain,
        frame_latency_waitable,
        adapter: info,
    })
}

/// `IDXGISwapChain::ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, SWAP_CHAIN_FLAGS)`. Precondition (not checkable here): every
/// reference to the back buffer is gone (Direct2D `SetTarget(None)`, target bitmap dropped, then `d3d11::clear_state_and_flush`).
/// The error keeps the HRESULT so the caller can test `is_device_lost` (C# `Resize` returned the raw HRESULT).
pub fn resize_buffers(swap_chain: &IDXGISwapChain1, width: u32, height: u32) -> Result<(), GfxError> {
    // SAFETY: `swap_chain` is a live owned interface; all arguments are plain numbers (buffer count 0 and format UNKNOWN keep the
    // current ones). The caller guarantees no outstanding back-buffer reference, otherwise DXGI fails the call (it does not crash).
    unsafe { swap_chain.ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, SWAP_CHAIN_FLAGS) }
        .gfx("IDXGISwapChain::ResizeBuffers")
}

/// `IDXGISwapChain1::GetBuffer(0)` as the surface Direct2D wraps into the target bitmap.
pub fn back_buffer(swap_chain: &IDXGISwapChain1) -> Result<IDXGISurface, GfxError> {
    // SAFETY: `swap_chain` is a live owned interface; the result is an owned surface interface.
    unsafe { swap_chain.GetBuffer::<IDXGISurface>(0) }.gfx("IDXGISwapChain::GetBuffer")
}

/// `IDXGISwapChain2::GetMaximumFrameLatency` (C# `DeviceResources.QueryMaximumFrameLatency`, test seam).
pub fn maximum_frame_latency(swap_chain: &IDXGISwapChain1) -> Result<u32, GfxError> {
    let sc2: IDXGISwapChain2 = swap_chain.cast().gfx("QueryInterface IDXGISwapChain2")?;
    // SAFETY: `sc2` is a live owned interface; the call takes no arguments.
    unsafe { sc2.GetMaximumFrameLatency() }.gfx("IDXGISwapChain2::GetMaximumFrameLatency")
}

/// `IDXGISwapChain::Present(sync_interval, 0)` and the HRESULT as it came (a SUCCESS code may carry meaning). Seam for the
/// `PresentFault` test hook of `DeviceResources`, which rewrites the HRESULT before [`classify_present`].
#[must_use]
pub fn present_raw(swap_chain: &IDXGISwapChain1, sync_interval: u32) -> i32 {
    // SAFETY: `swap_chain` is a live owned interface used on its owner thread; the arguments are plain numbers. The HRESULT is
    // returned as is (not `.ok()`, which would hide DXGI_STATUS_OCCLUDED).
    unsafe { swap_chain.Present(sync_interval, DXGI_PRESENT(0)) }.0
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
    if hresult == DXGI_STATUS_OCCLUDED_HRESULT {
        PresentOutcome::Occluded
    } else if is_device_lost(hresult) {
        PresentOutcome::DeviceLost(hresult)
    } else if hresult < 0 {
        PresentOutcome::Failed(hresult)
    } else {
        PresentOutcome::Presented
    }
}

/// `CreateDXGIFactory2(0)` as `IDXGIFactory2` (C# `Dxgi.CreateDxgiFactory2(0, IidDxgiFactory2)`; flag 1 = debug is not used).
pub fn create_factory2() -> Result<IDXGIFactory2, GfxError> {
    // SAFETY: the flags are a plain number (0, no debug layer); the result is an owned factory interface.
    unsafe { CreateDXGIFactory2::<IDXGIFactory2>(DXGI_CREATE_FACTORY_FLAGS(0)) }.gfx("CreateDXGIFactory2")
}
