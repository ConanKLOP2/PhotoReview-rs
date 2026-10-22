//! Hidden native controls used by W8-18f tests.
#![allow(dead_code)]

use pr_platform_win::dialogs::native::template::{DialogControlClass, class_name};
use pr_platform_win::win32::structs::Hwnd;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_CHILD, WS_POPUP};

pub struct ControlHost {
    pub owner: Hwnd,
    controls: Vec<Hwnd>,
}

impl ControlHost {
    pub fn new() -> Self {
        // SAFETY: creates a hidden system STATIC window on the current test thread.
        let owner = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                windows::core::w!("STATIC"),
                windows::core::w!(""),
                WS_POPUP,
                0,
                0,
                400,
                300,
                None,
                None,
                Some(GetModuleHandleW(None).expect("module").into()),
                None,
            )
        }
        .expect("owner");
        Self {
            owner: owner.0 as isize,
            controls: Vec::new(),
        }
    }

    pub fn create(&mut self, class: DialogControlClass, style: u32) -> Hwnd {
        let name = class_name(class).expect("standard control class");
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: creates a child standard control owned by this fixture's hidden window.
        let child = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                windows::core::PCWSTR(wide.as_ptr()),
                windows::core::w!(""),
                WS_CHILD | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(style),
                0,
                0,
                240,
                180,
                Some(HWND(self.owner as *mut _)),
                None,
                Some(GetModuleHandleW(None).expect("module").into()),
                None,
            )
        }
        .expect("child control");
        let hwnd = child.0 as isize;
        self.controls.push(hwnd);
        hwnd
    }
}

impl Drop for ControlHost {
    fn drop(&mut self) {
        for hwnd in self.controls.drain(..).rev() {
            // SAFETY: each child was created by this fixture and is destroyed once.
            let _ = unsafe { DestroyWindow(HWND(hwnd as *mut _)) };
        }
        // SAFETY: owner was created by new and is destroyed after its children.
        let _ = unsafe { DestroyWindow(HWND(self.owner as *mut _)) };
    }
}
