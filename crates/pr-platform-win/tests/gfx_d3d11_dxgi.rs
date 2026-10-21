//! Tests of `pr_platform_win::gfx::{d3d11, dxgi}` (unit W8-11g). C# twins: `Graphics/D2DSmokeTests.cs`
//! (`Warp_Device_IsCreatedWithFeatureLevelAndAlive`, `SwapChain_ForComposition_PresentsAndResizesWithWaitableObject`,
//! `Dxgi_CreateFactory2_ReturnsFactory2`, `StructLayouts_MatchWin32Sdk`).
#![cfg(windows)]

use pr_platform_win::gfx::com::{
    D2DERR_RECREATE_TARGET_HRESULT, DXGI_ERROR_DEVICE_REMOVED_HRESULT, DXGI_ERROR_DEVICE_RESET_HRESULT,
    DXGI_ERROR_WAS_STILL_DRAWING_HRESULT, DXGI_STATUS_OCCLUDED_HRESULT, hwnd,
};
use pr_platform_win::gfx::d3d11::{
    self, CreatedDevice, DEFAULT_DEVICE_FLAGS, DriverKind, FEATURE_LEVELS, create_device, create_device_on_adapter,
    create_device_with_fallback, device_removed_reason, dxgi_device,
};
use pr_platform_win::gfx::dxgi::{
    self, AdapterInfo, PresentOutcome, SWAP_CHAIN_FLAGS, back_buffer, clamp_frame_latency, classify_present,
    create_factory2, create_window_swap_chain, maximum_frame_latency, present_raw, resize_buffers,
    window_swap_chain_desc,
};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, HWND};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_DEVICE_CONTEXT_IMMEDIATE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_INVALID_CALL, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGISwapChain1,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
use windows::core::w;

// ---------------------------------------------------------------- pure

// C#: Math.Clamp(maxFrameLatency, 1, 16) in the DeviceResources constructor
#[test]
fn clamp_frame_latency_bounds() {
    for (input, expected) in [
        (i32::MIN, 1),
        (-5, 1),
        (0, 1),
        (1, 1),
        (2, 2),
        (16, 16),
        (17, 16),
        (i32::MAX, 16),
    ] {
        assert_eq!(clamp_frame_latency(input), expected, "input {input}");
    }
}

// C#: DeviceResources.CreateSwapChain lines 302-314
#[test]
fn window_swap_chain_desc_matches_the_csharp_production_values() {
    let desc = window_swap_chain_desc(1600, 900);
    assert_eq!((desc.Width, desc.Height), (1600, 900));
    assert_eq!(desc.Format, DXGI_FORMAT_B8G8R8A8_UNORM);
    assert!(!desc.Stereo.as_bool());
    assert_eq!((desc.SampleDesc.Count, desc.SampleDesc.Quality), (1, 0));
    assert_eq!(desc.BufferUsage, DXGI_USAGE_RENDER_TARGET_OUTPUT);
    assert_eq!(desc.BufferUsage.0, 0x20);
    assert_eq!(desc.BufferCount, 2);
    assert_eq!(desc.Scaling, DXGI_SCALING_STRETCH);
    assert_eq!(desc.Scaling.0, 0);
    assert_eq!(desc.SwapEffect, DXGI_SWAP_EFFECT_FLIP_DISCARD);
    assert_eq!(desc.SwapEffect.0, 4);
    assert_eq!(desc.AlphaMode, DXGI_ALPHA_MODE_IGNORE);
    assert_eq!(desc.AlphaMode.0, 3);
    assert_eq!(desc.Flags, 0x40);
    assert_eq!(SWAP_CHAIN_FLAGS.0, 0x40);
    let tiny = window_swap_chain_desc(1, 1);
    assert_eq!((tiny.Width, tiny.Height), (1, 1));
}

#[test]
fn classify_present_table() {
    for presented in [0, 1, 0x087A_0007] {
        assert_eq!(classify_present(presented), PresentOutcome::Presented, "{presented:#x}");
    }
    assert_eq!(classify_present(DXGI_STATUS_OCCLUDED_HRESULT), PresentOutcome::Occluded);
    for lost in [
        D2DERR_RECREATE_TARGET_HRESULT,
        DXGI_ERROR_DEVICE_REMOVED_HRESULT,
        DXGI_ERROR_DEVICE_RESET_HRESULT,
    ] {
        assert_eq!(classify_present(lost), PresentOutcome::DeviceLost(lost));
    }
    for failed in [DXGI_ERROR_WAS_STILL_DRAWING_HRESULT, E_FAIL.0, i32::MIN] {
        assert_eq!(classify_present(failed), PresentOutcome::Failed(failed));
    }
}

#[test]
fn adapter_info_display_formats_vendor_with_four_hex_digits() {
    let warp = AdapterInfo {
        description: "Microsoft Basic Render Driver".to_owned(),
        vendor_id: 0x1414,
    };
    assert_eq!(warp.display(), "Microsoft Basic Render Driver (vendor 0x1414)");
    let one = AdapterInfo {
        description: "X".to_owned(),
        vendor_id: 1,
    };
    assert_eq!(one.display(), "X (vendor 0x0001)");
}

// C#: D2DSmokeTests.StructLayouts_MatchWin32Sdk (D3D/DXGI lines)
#[test]
fn windows_rs_structs_have_the_win32_sdk_sizes() {
    assert_eq!(size_of::<DXGI_SWAP_CHAIN_DESC1>(), 48);
    assert_eq!(size_of::<D3D11_TEXTURE2D_DESC>(), 44);
}

#[test]
fn feature_levels_are_the_four_csharp_levels_in_order() {
    let values: Vec<i32> = FEATURE_LEVELS.iter().map(|level| level.0).collect();
    assert_eq!(values, [0xb000, 0xa100, 0xa000, 0x9300]);
    assert_eq!(D3D11_SDK_VERSION, 7);
    assert_eq!(DEFAULT_DEVICE_FLAGS.0, 0x20);
    assert_eq!(DEFAULT_DEVICE_FLAGS, D3D11_CREATE_DEVICE_BGRA_SUPPORT);
}

// ---------------------------------------------------------------- native helpers

/// A hidden `STATIC` popup window, destroyed on drop.
struct HiddenWindow(HWND);

impl HiddenWindow {
    fn new() -> Self {
        // SAFETY: "STATIC" is a system window class; all strings are static wide literals; no parent, menu, instance or
        // creation parameter. The window is never shown and is destroyed in `Drop` on this thread.
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

fn warp() -> CreatedDevice {
    create_device(DriverKind::Warp, DEFAULT_DEVICE_FLAGS).unwrap()
}

fn desc1(swap_chain: &IDXGISwapChain1) -> DXGI_SWAP_CHAIN_DESC1 {
    // SAFETY: `swap_chain` is a live owned interface; the call returns the description by value.
    unsafe { swap_chain.GetDesc1() }.unwrap()
}

// ---------------------------------------------------------------- native

// C#: D2DSmokeTests.Warp_Device_IsCreatedWithFeatureLevelAndAlive
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn warp_device_is_created_with_feature_level_and_alive() {
    let created = warp();
    assert!(created.is_warp);
    assert!(created.feature_level.0 >= 0x9300);
    // SAFETY: `device` is a live owned interface; no arguments.
    assert_eq!(unsafe { created.device.GetFeatureLevel() }, created.feature_level);
    assert_eq!(device_removed_reason(&created.device), 0);
    assert!(dxgi_device(&created.device).is_ok());
    // SAFETY: as above; the context is an owned interface.
    let context = unsafe { created.device.GetImmediateContext() }.unwrap();
    // SAFETY: `context` is the live immediate context.
    assert_eq!(unsafe { context.GetType() }, D3D11_DEVICE_CONTEXT_IMMEDIATE);
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn create_device_with_fallback_prefers_warp_when_asked_and_always_succeeds_on_this_machine() {
    let warp = create_device_with_fallback(true).unwrap();
    assert!(warp.is_warp);
    let any = create_device_with_fallback(false).unwrap();
    println!("fallback(false) gave warp = {}", any.is_warp);
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn create_device_on_adapter_zero_works_and_a_huge_index_fails_with_not_found() {
    let created = create_device_on_adapter(0, DEFAULT_DEVICE_FLAGS).unwrap();
    assert!(!created.is_warp);
    let error = create_device_on_adapter(99, DEFAULT_DEVICE_FLAGS).unwrap_err();
    assert_eq!(error.hresult, 0x887A_0002u32 as i32, "{error}");
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn clear_state_and_flush_and_flush_do_not_fail_on_a_live_device() {
    let created = warp();
    d3d11::clear_state_and_flush(&created.device);
    d3d11::clear_state_and_flush(&created.device);
    d3d11::flush(&created.device);
    assert_eq!(device_removed_reason(&created.device), 0);
}

// C#: D2DSmokeTests.Dxgi_CreateFactory2_ReturnsFactory2
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn create_factory2_returns_a_current_factory() {
    let factory = create_factory2().unwrap();
    // SAFETY: `factory` is a live owned interface; no arguments.
    assert!(unsafe { factory.IsCurrent() }.as_bool());
}

// C#: D2DSmokeTests.SwapChain_ForComposition_PresentsAndResizesWithWaitableObject (with CreateSwapChainForHwnd)
#[test]
#[ignore = "native: needs desktop"]
fn window_swap_chain_on_a_hidden_window_presents_and_resizes() {
    let created = warp();
    let window = HiddenWindow::new();
    let chain = create_window_swap_chain(&created.device, window.0, 16, 16, 1).unwrap();
    assert_eq!(maximum_frame_latency(&chain.swap_chain).unwrap(), 1);
    assert!(!chain.frame_latency_waitable.is_invalid());
    let desc = desc1(&chain.swap_chain);
    assert_eq!((desc.Width, desc.Height, desc.BufferCount), (16, 16, 2));
    assert_eq!(desc.Flags, 0x40);
    let adapter = chain.adapter.as_ref().expect("adapter info");
    assert!(!adapter.display().is_empty());
    assert_eq!(adapter.vendor_id, 0x1414, "WARP");

    let outcome = classify_present(present_raw(&chain.swap_chain, 0));
    assert!(
        matches!(outcome, PresentOutcome::Presented | PresentOutcome::Occluded),
        "{outcome:?}"
    );

    d3d11::clear_state_and_flush(&created.device);
    resize_buffers(&chain.swap_chain, 32, 8).unwrap();
    let desc = desc1(&chain.swap_chain);
    assert_eq!((desc.Width, desc.Height), (32, 8));
    let surface = back_buffer(&chain.swap_chain).unwrap();
    // SAFETY: `surface` is a live owned interface.
    let surface_desc = unsafe { surface.GetDesc() }.unwrap();
    assert_eq!((surface_desc.Width, surface_desc.Height), (32, 8));
    drop(surface);
    let outcome = classify_present(present_raw(&chain.swap_chain, 0));
    assert!(
        matches!(outcome, PresentOutcome::Presented | PresentOutcome::Occluded),
        "{outcome:?}"
    );

    // Trap T1: flush while the swap chain is alive, then release it, then the device and the window.
    d3d11::clear_state_and_flush(&created.device);
    drop(chain);
}

// Trap T1 (measured in WP-13b): ResizeBuffers needs every back-buffer reference gone.
#[test]
#[ignore = "native: needs desktop"]
fn resize_buffers_fails_while_a_back_buffer_reference_is_alive_and_works_after_it_is_dropped() {
    let created = warp();
    let window = HiddenWindow::new();
    let chain = create_window_swap_chain(&created.device, window.0, 16, 16, 1).unwrap();
    let surface = back_buffer(&chain.swap_chain).unwrap();
    d3d11::clear_state_and_flush(&created.device);
    let error = resize_buffers(&chain.swap_chain, 24, 24).unwrap_err();
    assert_eq!(error.hresult, DXGI_ERROR_INVALID_CALL.0, "{error}");
    drop(surface);
    d3d11::clear_state_and_flush(&created.device);
    resize_buffers(&chain.swap_chain, 24, 24).unwrap();
    d3d11::clear_state_and_flush(&created.device);
    drop(chain);
}

// Trap T2: a waitable swap chain needs the same flags on every ResizeBuffers.
#[test]
#[ignore = "native: needs desktop"]
fn resize_buffers_with_the_wrong_flags_is_rejected() {
    let created = warp();
    let window = HiddenWindow::new();
    let chain = create_window_swap_chain(&created.device, window.0, 16, 16, 1).unwrap();
    d3d11::clear_state_and_flush(&created.device);
    // SAFETY: `swap_chain` is a live owned interface; plain numbers; no back-buffer reference is outstanding.
    let wrong = unsafe {
        chain
            .swap_chain
            .ResizeBuffers(0, 20, 20, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
    };
    assert!(wrong.is_err(), "flags 0 on a waitable swap chain must fail");
    resize_buffers(&chain.swap_chain, 20, 20).unwrap();
    d3d11::clear_state_and_flush(&created.device);
    drop(chain);
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn create_window_swap_chain_rejects_a_null_window_handle() {
    let created = warp();
    let error = create_window_swap_chain(&created.device, hwnd(0), 16, 16, 1).unwrap_err();
    assert_eq!(error.hresult, E_INVALIDARG.0);
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn adapter_info_reads_the_warp_adapter() {
    let created = warp();
    let dxgi = dxgi_device(&created.device).unwrap();
    // SAFETY: `dxgi` is a live owned interface; the adapter is owned.
    let adapter = unsafe { dxgi.GetAdapter() }.unwrap();
    let info = dxgi::adapter_info(&adapter).expect("adapter info");
    assert!(!info.description.is_empty());
    assert_eq!(info.vendor_id, 0x1414);
}
