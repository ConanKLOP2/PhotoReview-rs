//! Hidden native controls used by W8-18f tests.
#![allow(dead_code)]

use pr_platform_win::dialogs::native::controls::basic::CommonControlsInit;
use pr_platform_win::dialogs::native::template::{DialogControlClass, window_class_name};
use pr_platform_win::win32::structs::Hwnd;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_CHILD, WS_POPUP};

pub struct ControlHost {
    pub owner: Hwnd,
    controls: Vec<Hwnd>,
    /// The comctl32 v6 manifest the real exe embeds (`pr-shell/build.rs`); a test exe has none. Declared last so it is deactivated after the windows go.
    _visual_styles: pr_platform_win::win32::activation_context::ActivationScope,
}

impl ControlHost {
    pub fn new() -> Self {
        // The real exe carries the comctl32 v6 manifest; without it a list view has no header until its first column (LVM_GETHEADER is 0),
        // a v5 behaviour. Activate it before any control class is registered or created.
        let visual_styles = pr_platform_win::win32::activation_context::ActivationScope::from_manifest_text(
            pr_platform_win::win32::comctl::SHELL_MANIFEST,
        )
        .expect("comctl32 v6 activation context");
        // The real dialog calls this before creating any control (C# NativeDialog.Show/Create), so a test host must too: the
        // classes are registered once per process.
        CommonControlsInit::ensure();
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
            _visual_styles: visual_styles,
        }
    }

    pub fn create(&mut self, class: DialogControlClass, style: u32) -> Hwnd {
        let name = window_class_name(class);
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
