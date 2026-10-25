//! W9-E2 real window-DC capture. BitBlt only; never PrintWindow.
use pr_platform_win::win32::{
    api, dialog_chrome as dc,
    dialog_native::{gdi32, user32},
    structs::{Hdc, Hgdi, Hwnd, Rect},
};
use std::collections::HashMap;
use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDIBits, HBITMAP, HDC};

struct WindowDc(Hwnd, Hdc);
impl Drop for WindowDc {
    fn drop(&mut self) {
        user32::release_dc(self.0, self.1);
    }
}
struct Canvas {
    dc: Hdc,
    bitmap: Hgdi,
    old: Hgdi,
}
impl Drop for Canvas {
    fn drop(&mut self) {
        if self.old != 0 {
            gdi32::select_object(self.dc, self.old);
        }
        if self.bitmap != 0 {
            gdi32::delete_object(self.bitmap);
        }
        if self.dc != 0 {
            dc::delete_dc(self.dc);
        }
    }
}
pub struct Shot {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    origin: Rect,
}
impl Shot {
    pub fn capture(hwnd: Hwnd) -> Self {
        let origin = api::user32::get_window_rect(hwnd).unwrap();
        let width = origin.right - origin.left;
        let height = origin.bottom - origin.top;
        assert!(width > 0 && height > 0);
        let source = WindowDc(hwnd, dc::get_window_dc(hwnd));
        assert_ne!(source.1, 0);
        let mut canvas = Canvas {
            dc: dc::create_compatible_dc(source.1),
            bitmap: 0,
            old: 0,
        };
        assert_ne!(canvas.dc, 0);
        canvas.bitmap = dc::create_compatible_bitmap(source.1, width, height);
        assert_ne!(canvas.bitmap, 0);
        canvas.old = gdi32::select_object(canvas.dc, canvas.bitmap);
        assert_ne!(canvas.old, 0);
        assert!(dc::bit_blt(canvas.dc, 0, 0, width, height, source.1, 0, 0, dc::SRCCOPY));
        // GetDIBits requires the bitmap to be deselected from its memory DC.
        gdi32::select_object(canvas.dc, canvas.old);
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: all GDI handles are live and owned above, the bitmap is deselected,
        // info describes the full top-down 32-bit buffer, allocated for width*height pixels.
        let lines = unsafe {
            GetDIBits(
                HDC(canvas.dc as *mut _),
                HBITMAP(canvas.bitmap as *mut _),
                0,
                height as u32,
                Some(pixels.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        assert_eq!(lines, height);
        Self {
            width: width as usize,
            height: height as usize,
            pixels,
            origin,
        }
    }
    pub fn assert_not_flat(&self, hwnd: Hwnd, label: &str) {
        let rect = api::user32::get_window_rect(hwnd).unwrap();
        let (left, top, right, bottom) = (
            rect.left - self.origin.left,
            rect.top - self.origin.top,
            rect.right - self.origin.left,
            rect.bottom - self.origin.top,
        );
        assert!(
            left >= 0
                && top >= 0
                && right > left
                && bottom > top
                && right as usize <= self.width
                && bottom as usize <= self.height,
            "{label}: crop outside window"
        );
        let mut counts = HashMap::<u32, usize>::new();
        for y in top..bottom {
            for x in left..right {
                let index = (y as usize * self.width + x as usize) * 4;
                let pixel = &self.pixels[index..index + 3];
                let colour = u32::from(pixel[0]) | u32::from(pixel[1]) << 8 | u32::from(pixel[2]) << 16;
                *counts.entry(colour).or_default() += 1;
            }
        }
        let total = (right - left) as usize * (bottom - top) as usize;
        let dominant = counts.values().copied().max().unwrap_or(0);
        // C#: ExternalEditorsUiTests.AssertNotFlat: >=3 colours, dominant <99%.
        assert!(
            counts.len() >= 3 && dominant * 100 < total * 99,
            "{label}: {} colours, dominant {dominant}/{total}",
            counts.len()
        );
    }
}
