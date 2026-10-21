//! Tests of `pr_platform_win::gfx::{com, extras}` (unit W8-11f; the IID table test lives in `gfx/guids.rs`).
//! C# twins: `D2DSmokeTests.ComInterop_Release_ReturnsEveryWrapperReferenceImmediately`,
//! `DWriteTextRendererTests.LongText_WithEllipsis_FitsMaxWidthAndIsMarkedTrimmed` (helper level).
#![cfg(windows)]

use pr_platform_win::gfx::com::{
    D2DERR_RECREATE_TARGET_HRESULT, DXGI_ERROR_DEVICE_REMOVED_HRESULT, DXGI_ERROR_DEVICE_RESET_HRESULT,
    DXGI_ERROR_WAS_STILL_DRAWING_HRESULT, DXGI_STATUS_OCCLUDED_HRESULT, GfxError, ResultExt, check, describe_hresult,
    hwnd, is_device_lost,
};
use pr_platform_win::gfx::extras::{
    WaitResult, WaitableTimer, create_ellipsis_trimming_sign, relative_due_time_100ns, set_character_ellipsis_trimming,
    wait_for_handle,
};
use pr_platform_win::gfx::{dwrite, extras};
use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, DXGI_STATUS_OCCLUDED, HANDLE};
use windows::Win32::Graphics::DirectWrite::{DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET,
    DXGI_ERROR_WAS_STILL_DRAWING, IDXGIFactory2,
};
use windows::core::{HRESULT, IUnknown_Vtbl, Interface};

// C#: ComInterop.Check
#[test]
fn check_negative_is_err_and_non_negative_is_ok() {
    assert_eq!(check("op", -1), Err(GfxError::new("op", -1)));
    assert_eq!(check("other", i32::MIN), Err(GfxError::new("other", i32::MIN)));
    assert_eq!(check("op", 0), Ok(()));
    assert_eq!(check("op", 1), Ok(()), "S_FALSE is success");
    assert_eq!(check("op", DXGI_STATUS_OCCLUDED_HRESULT), Ok(()), "OCCLUDED is success");
}

#[test]
fn gfx_error_display_text() {
    let error = GfxError::new("D3D11CreateDevice(WARP)", 0x887A_0005u32 as i32);
    assert_eq!(
        error.to_string(),
        "D3D11CreateDevice(WARP) failed with HRESULT 0x887A0005"
    );
    let copy = error; // Copy: `error` stays usable
    assert_eq!(copy, error);
}

// C#: DeviceLoss.Describe
#[test]
fn describe_hresult_matches_csharp_format() {
    assert_eq!(describe_hresult(-2_005_270_523), "0x887A0005");
    assert_eq!(describe_hresult(0), "0x00000000");
    assert_eq!(describe_hresult(-1), "0xFFFFFFFF");
    assert_eq!(describe_hresult(0x087A_0001), "0x087A0001");
    assert_eq!(describe_hresult(i32::MIN), "0x80000000");
}

// C#: DeviceLoss.IsDeviceLost
#[test]
fn is_device_lost_is_exactly_the_three_codes() {
    for lost in [
        D2DERR_RECREATE_TARGET_HRESULT,
        DXGI_ERROR_DEVICE_REMOVED_HRESULT,
        DXGI_ERROR_DEVICE_RESET_HRESULT,
    ] {
        assert!(is_device_lost(lost), "{}", describe_hresult(lost));
        assert!(GfxError::new("x", lost).is_device_lost());
    }
    for not_lost in [
        DXGI_ERROR_WAS_STILL_DRAWING_HRESULT,
        DXGI_STATUS_OCCLUDED_HRESULT,
        0,
        0x8000_4005u32 as i32,
        0x887A_0001u32 as i32,
        0x887A_0004u32 as i32,
        0x887A_0006u32 as i32,
        0x8899_000Bu32 as i32,
        0x8899_000Du32 as i32,
    ] {
        assert!(!is_device_lost(not_lost), "{}", describe_hresult(not_lost));
        assert!(!GfxError::new("x", not_lost).is_device_lost());
    }
}

#[test]
fn hresult_constants_equal_windows_rs_and_the_csharp_literals() {
    assert_eq!(D2DERR_RECREATE_TARGET_HRESULT, D2DERR_RECREATE_TARGET.0);
    assert_eq!(DXGI_ERROR_DEVICE_REMOVED_HRESULT, DXGI_ERROR_DEVICE_REMOVED.0);
    assert_eq!(DXGI_ERROR_DEVICE_RESET_HRESULT, DXGI_ERROR_DEVICE_RESET.0);
    assert_eq!(DXGI_ERROR_WAS_STILL_DRAWING_HRESULT, DXGI_ERROR_WAS_STILL_DRAWING.0);
    assert_eq!(DXGI_STATUS_OCCLUDED_HRESULT, DXGI_STATUS_OCCLUDED.0);
    assert_eq!(D2DERR_RECREATE_TARGET_HRESULT, 0x8899_000Cu32 as i32);
    assert_eq!(DXGI_ERROR_DEVICE_REMOVED_HRESULT, 0x887A_0005u32 as i32);
    assert_eq!(DXGI_ERROR_DEVICE_RESET_HRESULT, 0x887A_0007u32 as i32);
    assert_eq!(DXGI_ERROR_WAS_STILL_DRAWING_HRESULT, 0x887A_000Au32 as i32);
    assert_eq!(DXGI_STATUS_OCCLUDED_HRESULT, 0x087A_0001);
}

#[test]
fn result_ext_keeps_hresult_and_operation() {
    let failed: windows::core::Result<i32> = Err(windows::core::Error::from(HRESULT(0x887A_0007u32 as i32)));
    assert_eq!(
        failed.gfx("x"),
        Err(GfxError {
            hresult: 0x887A_0007u32 as i32,
            operation: "x"
        })
    );
    let ok: windows::core::Result<i32> = Ok(5);
    assert_eq!(ok.gfx("x"), Ok(5));
}

#[test]
fn hwnd_round_trips_the_isize() {
    assert!(hwnd(0).0.is_null());
    assert_eq!(hwnd(0x1234).0 as isize, 0x1234);
    assert_eq!(hwnd(-1).0 as isize, -1);
}

#[test]
fn wait_result_from_raw_maps_every_documented_value() {
    assert_eq!(WaitResult::from_raw(0), WaitResult::Signaled);
    assert_eq!(WaitResult::from_raw(0x102), WaitResult::Timeout);
    assert_eq!(WaitResult::from_raw(0x80), WaitResult::Abandoned);
    assert_eq!(WaitResult::from_raw(0xC0), WaitResult::IoCompletion);
    assert_eq!(WaitResult::from_raw(0xFFFF_FFFF), WaitResult::Failed);
    assert_eq!(WaitResult::from_raw(7), WaitResult::Failed);
    assert!(WaitResult::Signaled.is_signaled());
    for other in [
        WaitResult::Timeout,
        WaitResult::Abandoned,
        WaitResult::IoCompletion,
        WaitResult::Failed,
    ] {
        assert!(!other.is_signaled());
    }
}

// C#: GpuKeepAlive.SleepInterval
#[test]
fn relative_due_time_is_negative_100ns_without_overflow() {
    assert_eq!(relative_due_time_100ns(16), -160_000);
    assert_eq!(relative_due_time_100ns(0), 0);
    assert_eq!(relative_due_time_100ns(-5), 0);
    assert_eq!(relative_due_time_100ns(300_000), -3_000_000_000);
    assert_eq!(
        relative_due_time_100ns(i64::MAX / 10_000),
        -(i64::MAX / 10_000) * 10_000
    );
    let _ = relative_due_time_100ns(i64::MAX);
    let _ = relative_due_time_100ns(i64::MIN);
}

// C#: D2DSmokeTests.ComInterop_Release_ReturnsEveryWrapperReferenceImmediately
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn com_drop_releases_the_reference_immediately() {
    /// COM reference count of a live object, read through `AddRef`/`Release` (returns the count before the probe).
    fn count(raw: *mut core::ffi::c_void) -> u32 {
        // SAFETY: `raw` points at a live COM object (the caller holds a reference); every COM object starts with a pointer to
        // an IUnknown-compatible vtable, and AddRef followed by Release leaves the count unchanged.
        unsafe {
            let vtable = &**(raw as *const *const IUnknown_Vtbl);
            let after_add = (vtable.AddRef)(raw);
            (vtable.Release)(raw);
            after_add - 1
        }
    }
    // SAFETY: no pointer arguments; the factory is an owned interface.
    let factory = unsafe { CreateDXGIFactory2::<IDXGIFactory2>(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
    let raw = factory.as_raw();
    let baseline = count(raw);
    assert!(baseline >= 1);

    let clone = factory.clone();
    assert_eq!(count(raw), baseline + 1, "clone is AddRef");
    drop(clone);
    assert_eq!(count(raw), baseline, "drop is Release, at once");

    // An observer takes one extra reference; dropping the factory must give back exactly its own one.
    // SAFETY: `raw` is alive (the factory still holds a reference) for this AddRef; the vtable pointer is the first field of the object.
    let vtable = unsafe { &**(raw as *const *const IUnknown_Vtbl) };
    // SAFETY: as above.
    unsafe { (vtable.AddRef)(raw) };
    drop(factory);
    assert_eq!(count(raw), baseline, "drop released exactly the factory's reference");
    // SAFETY: the observer's own reference keeps the object alive; this Release gives it back (and may free the object).
    let rest = unsafe { (vtable.Release)(raw) };
    assert_eq!(rest, baseline - 1);
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn waitable_timer_unset_times_out_at_once_and_set_signals() {
    let timer = WaitableTimer::new().unwrap();
    assert_eq!(timer.wait(0, false), WaitResult::Timeout, "not signaled yet");
    timer.set_relative_ms(1).unwrap();
    // 5000 ms is only a bound for a hung machine, never reached.
    assert_eq!(timer.wait(5000, false), WaitResult::Signaled);
    timer.cancel().unwrap();
    timer.set_relative_ms(60_000).unwrap();
    timer.cancel().unwrap();
    assert_eq!(timer.wait(0, false), WaitResult::Timeout);
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn waitable_timer_reports_high_resolution_flag() {
    let timer = WaitableTimer::new().unwrap();
    println!("high resolution timer: {}", timer.is_high_resolution());
    assert!(!timer.handle().is_invalid());
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn wait_for_handle_null_handle_is_failed() {
    assert_eq!(wait_for_handle(HANDLE::default(), 0, false), WaitResult::Failed);
    assert_eq!(extras::wait_for_handle(HANDLE::default(), 0, true), WaitResult::Failed);
}

fn long_vietnamese() -> String {
    "Đây là một dòng văn bản tiếng Việt rất dài để kiểm tra việc cắt bớt bằng dấu ba chấm".repeat(2)
}

fn layout_width(layout: &windows::Win32::Graphics::DirectWrite::IDWriteTextLayout) -> f32 {
    let mut metrics = DWRITE_TEXT_METRICS::default();
    // SAFETY: `metrics` is a local that outlives the call; `layout` is a live owned interface.
    unsafe { layout.GetMetrics(&mut metrics) }.unwrap();
    metrics.width
}

// C#: DWriteTextRendererTests.LongText_WithEllipsis_FitsMaxWidthAndIsMarkedTrimmed / ShortText_WithEllipsis_IsNotTrimmed...
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn set_character_ellipsis_trimming_marks_a_narrow_layout_trimmed() {
    let factory = dwrite::create_factory().unwrap();
    let format = dwrite::create_text_format(&factory, "Segoe UI", false, 12.0, "vi-VN").unwrap();

    let long = dwrite::create_text_layout(&factory, &long_vietnamese(), &format, 120.0).unwrap();
    // SAFETY: `long` is a live owned interface.
    unsafe { long.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }.unwrap();
    assert!(!dwrite::first_line_is_trimmed(&long), "not trimmed before the helper");
    set_character_ellipsis_trimming(&factory, &long).unwrap();
    assert!(dwrite::first_line_is_trimmed(&long), "trimmed after the helper");
    assert!(layout_width(&long) <= 120.0);

    let short = dwrite::create_text_layout(&factory, "100 %", &format, 500.0).unwrap();
    // SAFETY: `short` is a live owned interface.
    unsafe { short.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }.unwrap();
    set_character_ellipsis_trimming(&factory, &short).unwrap();
    assert!(!dwrite::first_line_is_trimmed(&short));
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn create_ellipsis_trimming_sign_accepts_a_text_layout_as_format() {
    let factory = dwrite::create_factory().unwrap();
    let format = dwrite::create_text_format(&factory, "Segoe UI", false, 12.0, "en-US").unwrap();
    let layout = dwrite::create_text_layout(&factory, "abc", &format, 100.0).unwrap();
    assert!(create_ellipsis_trimming_sign(&factory, &layout).is_ok());
    assert!(create_ellipsis_trimming_sign(&factory, &format).is_ok());
}
