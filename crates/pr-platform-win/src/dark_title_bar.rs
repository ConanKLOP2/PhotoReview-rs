//! Port of `PhotoReview.Platform.Windows/WindowsDarkTitleBar.cs`; accepts a raw HWND and has no UI dependency.

use std::sync::OnceLock;

use windows::Win32::Foundation::{ERROR_MOD_NOT_FOUND, FreeLibrary, HWND};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::w;

const DWMWA_USE_IMMERSIVE_DARK_MODE_LEGACY: i32 = 19;
const DWMWA_USE_IMMERSIVE_DARK_MODE: i32 = 20;
const DWMWA_CAPTION_COLOR: i32 = 35;

type DwmSetWindowAttributeFn = unsafe extern "system" fn(HWND, u32, *const core::ffi::c_void, u32) -> i32;

static DWM_SET_WINDOW_ATTRIBUTE: OnceLock<Option<DwmSetWindowAttributeFn>> = OnceLock::new();

fn dwm_set_window_attribute() -> Option<DwmSetWindowAttributeFn> {
    *DWM_SET_WINDOW_ATTRIBUTE.get_or_init(|| {
        // SAFETY: the DLL name is a static NUL-terminated UTF-16 string.
        let module = match unsafe { LoadLibraryW(w!("dwmapi.dll")) } {
            Ok(module) => module,
            Err(error) if win32_error_code(&error) == ERROR_MOD_NOT_FOUND.0 => return None,
            Err(error) => panic!("loading dwmapi.dll failed: {error}"),
        };
        // SAFETY: the module handle is valid and the export name is a static NUL-terminated ANSI string.
        let Some(address) = (unsafe { GetProcAddress(module, windows::core::s!("DwmSetWindowAttribute")) }) else {
            // A valid loaded module can return null here only when the requested export is missing; C# catches that.
            // SAFETY: the loaded module is not retained because the required export is absent.
            let _ = unsafe { FreeLibrary(module) };
            return None;
        };
        // SAFETY: dwmapi documents this export as (HWND, DWORD, LPCVOID, DWORD) -> HRESULT.
        Some(unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, DwmSetWindowAttributeFn>(address) })
    })
}

fn win32_error_code(error: &windows::core::Error) -> u32 {
    (error.code().0 as u32) & 0xFFFF
}

/// DWM attributes for the app's native dark title bar and caption color.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsDarkTitleBar;

impl WindowsDarkTitleBar {
    /// Enables immersive dark mode, trying the current and legacy attribute IDs. Unsupported OS versions return false.
    pub fn try_enable_dark_mode(handle: HWND) -> bool {
        if handle.0.is_null() {
            return false;
        }
        let enabled = 1i32;
        Self::try_set(handle, DWMWA_USE_IMMERSIVE_DARK_MODE, &enabled)
            || Self::try_set(handle, DWMWA_USE_IMMERSIVE_DARK_MODE_LEGACY, &enabled)
    }

    /// Sets the DWM caption color, in COLORREF BGR format (`0x00BBGGRR`).
    pub fn try_set_caption_color(handle: HWND, color_bgr: u32) -> bool {
        if handle.0.is_null() {
            return false;
        }
        Self::try_set(handle, DWMWA_CAPTION_COLOR, &color_bgr)
    }

    fn try_set<T>(handle: HWND, attribute: i32, value: &T) -> bool {
        let Some(function) = dwm_set_window_attribute() else {
            return false;
        };
        // SAFETY: `value` points to a live scalar of exactly the size supplied; DWM accepts raw HWND attributes.
        unsafe {
            function(
                handle,
                attribute as u32,
                (value as *const T).cast(),
                std::mem::size_of::<T>() as u32,
            ) == 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_hwnd_is_rejected_without_calling_dwm() {
        // C# twins: LibraryImportInteropTests.DwmCalls_OnNullHandle_ReturnFalse and
        // PlatformPrimitivesTests.WindowsDarkTitleBarEnablesDarkModeOnARealHandle.
        let null = HWND::default();
        assert!(!WindowsDarkTitleBar::try_enable_dark_mode(null));
        assert!(!WindowsDarkTitleBar::try_set_caption_color(null, 0x00171717));
    }

    #[test]
    #[ignore = "native: DWM HWND attribute call"]
    fn bogus_hwnd_fails_closed() {
        // C# twin: LibraryImportInteropTests.DwmCalls_OnNullHandle_ReturnFalse (bogus-handle assertions).
        let invalid = HWND(0x7FFF0usize as *mut _);
        assert!(!WindowsDarkTitleBar::try_enable_dark_mode(invalid));
        assert!(!WindowsDarkTitleBar::try_set_caption_color(invalid, 0x00171717));
    }
}
