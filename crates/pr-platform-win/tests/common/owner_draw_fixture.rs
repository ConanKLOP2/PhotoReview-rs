//! Fixture of the W8-14g native tests: a hidden `STATIC` owner window and a memory bitmap canvas to paint into with the real GDI.
#![allow(dead_code)] // each test crate that includes this file uses a different part

use std::sync::{Mutex, MutexGuard};

use pr_platform_win::win32::structs::{Hdc, Hwnd};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetPixel, HBITMAP, HDC, HGDIOBJ,
    ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
use windows::core::w;

/// A hidden (never shown) popup `STATIC` window, destroyed on drop. Must be created and dropped on the thread that uses it.
pub struct OwnerWindow {
    pub hwnd: Hwnd,
}

impl OwnerWindow {
    pub fn new() -> Self {
        // SAFETY: plain arguments; the class is the system `STATIC` class, no parent, the window is never shown.
        let hwnd = unsafe {
            let module = GetModuleHandleW(None).expect("module handle");
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("owner-draw-owner"),
                WS_POPUP,
                0,
                0,
                200,
                100,
                None,
                None,
                Some(module.into()),
                None,
            )
        }
        .expect("create the hidden owner window");
        Self { hwnd: hwnd.0 as isize }
    }
}

impl Drop for OwnerWindow {
    fn drop(&mut self) {
        // SAFETY: the handle was created by `new` on this thread and is destroyed once.
        let _ = unsafe { DestroyWindow(HWND(self.hwnd as *mut core::ffi::c_void)) };
    }
}

/// A memory DC with a compatible bitmap selected; every GDI object is released on drop.
pub struct Canvas {
    pub dc: Hdc,
    pub width: i32,
    pub height: i32,
    mem: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
}

impl Canvas {
    pub fn new(width: i32, height: i32) -> Self {
        // SAFETY: the screen DC is only used to create compatible objects and is released before returning; every created object is owned by `Self`.
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            ReleaseDC(None, screen);
            assert!(!mem.is_invalid() && !bitmap.is_invalid(), "memory canvas");
            let old = SelectObject(mem, bitmap.into());
            Self {
                dc: mem.0 as isize,
                width,
                height,
                mem,
                bitmap,
                old,
            }
        }
    }

    /// The `COLORREF` at `(x, y)` (`0xFFFF_FFFF` outside the bitmap).
    pub fn pixel(&self, x: i32, y: i32) -> u32 {
        // SAFETY: a DC owned by `self`.
        let COLORREF(c) = unsafe { GetPixel(self.mem, x, y) };
        c
    }

    /// All pixels of the columns `x0..x1` of every row.
    pub fn pixels_in(&self, x0: i32, x1: i32) -> Vec<u32> {
        let mut out = Vec::new();
        for y in 0..self.height {
            for x in x0..x1.min(self.width) {
                out.push(self.pixel(x, y));
            }
        }
        out
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: restores the original bitmap, then deletes the objects created in `new`, once.
        unsafe {
            SelectObject(self.mem, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.mem);
        }
    }
}

/// The brightest channel of a `COLORREF` (`0x00BBGGRR`).
pub fn brightness(color: u32) -> u32 {
    (color & 0xFF).max((color >> 8) & 0xFF).max((color >> 16) & 0xFF)
}

static SERIAL: Mutex<()> = Mutex::new(());

/// Native tests count the GDI objects of the whole process, so they must not overlap: every native test holds this guard.
pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
