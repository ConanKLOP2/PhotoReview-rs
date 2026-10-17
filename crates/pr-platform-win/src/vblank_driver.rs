//! Port of `IVBlankDriver` and `NativeVBlankDriver` from
//! `PhotoReview.Platform.Windows/WindowsDisplayClock.cs` (C# baseline `d92d6aae`).
//!
//! This module owns the narrow Win32, D3DKMT, and DWM boundary used by the Windows display clock.

use pr_core::abstractions::DisplayTiming;

/// Monitor nearest to a window, adapter open/wait/close, and compositor timing operations for the display clock.
pub(crate) trait IVBlankDriver: Send + Sync {
    /// Returns the nearest monitor for a nonzero HWND, or zero when lookup fails.
    fn monitor_from_window(&self, window: isize) -> isize;

    /// Opens the adapter associated with `monitor` and returns its adapter/source identifiers.
    fn try_open(&self, monitor: isize) -> Option<(u32, u32)>;

    /// Waits for a vblank and returns the D3DKMT NTSTATUS (zero is success).
    fn wait_for_vblank(&self, adapter: u32, source: u32) -> i32;

    /// Closes an opened adapter.
    fn close(&self, adapter: u32);

    /// Returns the primary compositor timing, if available.
    fn dwm_timing(&self) -> Option<DisplayTiming>;
}

/// Stateless native driver. A static instance mirrors C# `NativeVBlankDriver.Instance`.
pub(crate) struct NativeVBlankDriver;

impl NativeVBlankDriver {
    /// Process-wide singleton matching the C# static instance.
    pub(crate) const INSTANCE: Self = Self;

    /// Size of the Windows `MONITORINFOEXW` ABI structure.
    pub const fn monitor_info_ex_size() -> usize {
        core::mem::size_of::<MonitorInfoExW>()
    }

    /// Size of the packed Windows `DWM_TIMING_INFO` ABI structure.
    pub const fn timing_info_size() -> usize {
        core::mem::size_of::<DwmTimingInfo>()
    }

    /// Applies the former `ByValTStr` conversion: stop at NUL and inspect at most 32 UTF-16 code units.
    pub fn device_name_from(buffer: &[u16]) -> String {
        let bounded = &buffer[..buffer.len().min(32)];
        let end = bounded.iter().position(|unit| *unit == 0).unwrap_or(bounded.len());
        String::from_utf16_lossy(&bounded[..end])
    }

    #[cfg(windows)]
    fn dwm_timing_impl(&self) -> Option<DisplayTiming> {
        let mut info = DwmTimingInfo {
            size: Self::timing_info_size() as u32,
            ..DwmTimingInfo::default()
        };
        // SAFETY: the dynamic resolver checks for the optional DLL and entry point before calling it; `info` is a
        // writable stack value with the packed DWM_TIMING_INFO ABI and exact size, and stays live for the call.
        let status = (unsafe { with_optional_dwm(|get_timing| get_timing(0, &mut info)) })?;
        if status != 0 || info.qpc_refresh_period == 0 || info.qpc_vblank == 0 {
            return None;
        }
        Some(DisplayTiming {
            last_vblank: info.qpc_vblank as i64,
            refresh_period: info.qpc_refresh_period as i64,
        })
    }
}

#[cfg(windows)]
impl IVBlankDriver for NativeVBlankDriver {
    fn monitor_from_window(&self, window: isize) -> isize {
        // SAFETY: the HWND is an opaque handle supplied by the caller; the API does not retain or dereference it
        // after returning. MONITOR_DEFAULTTONEAREST is the C# behavior.
        unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) }
    }

    fn try_open(&self, monitor: isize) -> Option<(u32, u32)> {
        let mut info = MonitorInfoExW {
            size: Self::monitor_info_ex_size() as u32,
            ..MonitorInfoExW::default()
        };
        // SAFETY: `info` is initialized with the ABI size and has the exact MONITORINFOEXW layout; it is writable
        // for the duration of this call and `monitor` is an opaque HMONITOR returned by MonitorFromWindow.
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return None;
        }

        let device = Self::device_name_from(&info.device);
        let mut device_wide: Vec<u16> = device.encode_utf16().collect();
        device_wide.push(0);
        // SAFETY: `device_wide` is NUL terminated and remains alive through CreateDCW; null driver/output/init-data
        // pointers match the C# call. The returned HDC is owned here until DeleteDC below.
        let dc = unsafe {
            CreateDCW(
                core::ptr::null(),
                device_wide.as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
            )
        };
        if dc == 0 {
            return None;
        }

        let mut opened = OpenAdapterFromHdc {
            hdc: dc,
            ..OpenAdapterFromHdc::default()
        };
        // SAFETY: `opened` matches D3DKMT_OPENADAPTERFROMHDC and is writable for the call; `dc` remains a valid
        // HDC because DeleteDC is deferred until the cleanup immediately below. A missing entry point maps to
        // failure just as the C# display-clock catch path does.
        let status = unsafe { with_optional_gdi_d3dkmt_open(&mut opened) }.unwrap_or(-1);
        // SAFETY: `dc` was returned by CreateDCW above and has not been deleted; this call releases its single owner
        // regardless of whether the D3DKMT open succeeded.
        let _ = unsafe { DeleteDC(dc) };
        (status == 0).then_some((opened.adapter, opened.vid_pn_source_id))
    }

    fn wait_for_vblank(&self, adapter: u32, source: u32) -> i32 {
        let mut wait = WaitForVerticalBlankEvent {
            adapter,
            device: 0,
            vid_pn_source_id: source,
        };
        // SAFETY: `wait` is a writable stack value matching D3DKMT_WAITFORVERTICALBLANKEVENT and remains valid
        // through the synchronous native call; adapter/source came from a successful open.
        unsafe { with_optional_gdi_d3dkmt_wait(&mut wait) }.unwrap_or(-1)
    }

    fn close(&self, adapter: u32) {
        let mut close = CloseAdapter { adapter };
        // SAFETY: `close` is a writable stack value matching D3DKMT_CLOSEADAPTER and remains valid for this call;
        // the identifier is owned by the caller from a successful adapter open.
        let _ = unsafe { with_optional_gdi_d3dkmt_close(&mut close) };
    }

    fn dwm_timing(&self) -> Option<DisplayTiming> {
        self.dwm_timing_impl()
    }
}

#[cfg(not(windows))]
impl IVBlankDriver for NativeVBlankDriver {
    fn monitor_from_window(&self, _window: isize) -> isize {
        0
    }

    fn try_open(&self, _monitor: isize) -> Option<(u32, u32)> {
        None
    }

    fn wait_for_vblank(&self, _adapter: u32, _source: u32) -> i32 {
        -1
    }

    fn close(&self, _adapter: u32) {}

    fn dwm_timing(&self) -> Option<DisplayTiming> {
        None
    }
}

#[cfg(windows)]
const MONITOR_DEFAULTTONEAREST: u32 = 2;

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[cfg(windows)]
#[repr(C)]
struct MonitorInfoExW {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
    device: [u16; 32],
}

#[cfg(windows)]
impl Default for MonitorInfoExW {
    fn default() -> Self {
        Self {
            size: 0,
            monitor: Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            work: Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            flags: 0,
            device: [0; 32],
        }
    }
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct Luid {
    low_part: u32,
    high_part: i32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct OpenAdapterFromHdc {
    hdc: isize,
    adapter: u32,
    adapter_luid: Luid,
    vid_pn_source_id: u32,
}

#[cfg(windows)]
#[repr(C)]
struct WaitForVerticalBlankEvent {
    adapter: u32,
    device: u32,
    vid_pn_source_id: u32,
}

#[cfg(windows)]
#[repr(C)]
struct CloseAdapter {
    adapter: u32,
}

#[cfg(windows)]
#[repr(C, packed)]
#[derive(Default)]
struct UnsignedRatio {
    numerator: u32,
    denominator: u32,
}

#[cfg(windows)]
#[repr(C, packed)]
#[derive(Default)]
struct DwmTimingInfo {
    size: u32,
    rate_refresh: UnsignedRatio,
    qpc_refresh_period: u64,
    rate_compose: UnsignedRatio,
    qpc_vblank: u64,
    c_refresh: u64,
    c_dx_refresh: u32,
    qpc_compose: u64,
    c_frame: u64,
    c_dx_present: u32,
    c_refresh_frame: u64,
    c_frame_submitted: u64,
    c_dx_present_submitted: u32,
    c_frame_confirmed: u64,
    c_dx_present_confirmed: u32,
    c_refresh_confirmed: u64,
    c_dx_refresh_confirmed: u32,
    c_frames_late: u64,
    c_frames_outstanding: u32,
    c_frame_displayed: u64,
    qpc_frame_displayed: u64,
    c_refresh_frame_displayed: u64,
    c_frame_complete: u64,
    qpc_frame_complete: u64,
    c_frame_pending: u64,
    qpc_frame_pending: u64,
    c_frames_displayed: u64,
    c_frames_complete: u64,
    c_frames_pending: u64,
    c_frames_available: u64,
    c_frames_dropped: u64,
    c_frames_missed: u64,
    c_refresh_next_displayed: u64,
    c_refresh_next_presented: u64,
    c_refreshes_displayed: u64,
    c_refreshes_presented: u64,
    c_refresh_started: u64,
    c_pixels_received: u64,
    c_pixels_drawn: u64,
    c_buffers_empty: u64,
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MonitorFromWindow(window: isize, flags: u32) -> isize;
    fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfoExW) -> i32;
}

#[cfg(windows)]
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateDCW(
        driver: *const u16,
        device: *const u16,
        output: *const u16,
        init_data: *const core::ffi::c_void,
    ) -> isize;
    fn DeleteDC(dc: isize) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut core::ffi::c_void;
    fn FreeLibrary(module: *mut core::ffi::c_void) -> i32;
    fn GetProcAddress(module: *mut core::ffi::c_void, name: *const u8) -> *mut core::ffi::c_void;
}

#[cfg(windows)]
unsafe fn with_optional_dwm<T>(
    call: impl FnOnce(unsafe extern "system" fn(isize, *mut DwmTimingInfo) -> i32) -> T,
) -> Option<T> {
    const MODULE_NAME: [u16; 11] = [100, 119, 109, 97, 112, 105, 46, 100, 108, 108, 0];
    // SAFETY: MODULE_NAME is a static, NUL-terminated UTF-16 string.
    let module = unsafe { LoadLibraryW(MODULE_NAME.as_ptr()) };
    if module.is_null() {
        return None;
    }
    // SAFETY: module is a live LoadLibraryW reference and the symbol name is static and NUL terminated.
    let address = unsafe { GetProcAddress(module, c"DwmGetCompositionTimingInfo".as_ptr().cast()) };
    let result = if address.is_null() {
        None
    } else {
        // SAFETY: GetProcAddress found the documented DWM function; this signature matches dwmapi.h.
        let function: unsafe extern "system" fn(isize, *mut DwmTimingInfo) -> i32 =
            unsafe { core::mem::transmute(address) };
        Some(call(function))
    };
    // SAFETY: module is the reference acquired above and is no longer used after this call.
    let _ = unsafe { FreeLibrary(module) };
    result
}

#[cfg(windows)]
unsafe fn with_optional_gdi_d3dkmt_open(data: *mut OpenAdapterFromHdc) -> Option<i32> {
    // SAFETY: the symbol helper checks the module and export before invoking this documented ABI.
    unsafe { with_optional_gdi_symbol(c"D3DKMTOpenAdapterFromHdc", |function| function(data)) }
}

#[cfg(windows)]
unsafe fn with_optional_gdi_d3dkmt_wait(data: *mut WaitForVerticalBlankEvent) -> Option<i32> {
    // SAFETY: the symbol helper checks the module and export before invoking this documented ABI.
    unsafe { with_optional_gdi_symbol(c"D3DKMTWaitForVerticalBlankEvent", |function| function(data)) }
}

#[cfg(windows)]
unsafe fn with_optional_gdi_d3dkmt_close(data: *mut CloseAdapter) -> Option<i32> {
    // SAFETY: the symbol helper checks the module and export before invoking this documented ABI.
    unsafe { with_optional_gdi_symbol(c"D3DKMTCloseAdapter", |function| function(data)) }
}

#[cfg(windows)]
unsafe fn with_optional_gdi_symbol<T>(
    name: &'static core::ffi::CStr,
    call: impl FnOnce(unsafe extern "system" fn(*mut T) -> i32) -> i32,
) -> Option<i32> {
    const MODULE_NAME: [u16; 10] = [103, 100, 105, 51, 50, 46, 100, 108, 108, 0];
    // SAFETY: MODULE_NAME is a static, NUL-terminated UTF-16 string.
    let module = unsafe { LoadLibraryW(MODULE_NAME.as_ptr()) };
    if module.is_null() {
        return None;
    }
    // SAFETY: module is a live LoadLibraryW reference and name points to a static NUL-terminated export name.
    let address = unsafe { GetProcAddress(module, name.as_ptr().cast()) };
    let result = if address.is_null() {
        None
    } else {
        // SAFETY: callers request documented D3DKMT exports with the same one-pointer NTSTATUS ABI.
        let function: unsafe extern "system" fn(*mut T) -> i32 = unsafe { core::mem::transmute(address) };
        Some(call(function))
    };
    // SAFETY: module is the reference acquired above and is no longer used after this call.
    let _ = unsafe { FreeLibrary(module) };
    result
}
