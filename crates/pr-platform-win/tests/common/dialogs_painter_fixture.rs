//! Fixture of the W8-18l / 18m / 18n native tests (contract `contracts/W8-18l.md`, section "Fixture"): an STA thread runner, an owner window (plain, hidden or
//! "dark", i.e. answering `WM_CTLCOLOR*` like a `NativeDialog` does), control creation, pixel capture with `PrintWindow`, and the C# "dark and readable" rule
//! (`DarkAndReadableFindings`). Read-only for 18m and 18n.
//!
//! Differences from the contract text, on purpose:
//! - `child` initialises the common controls with `init_common_controls_ex` directly instead of `CommonControlsInit::ensure()` (W8-18f is ported in parallel and its
//!   body may still be a stub when these tests run); the call is idempotent, so the result is the same.
//! - `OwnerWindow::dark()` implements the `WM_CTLCOLOR*` decision itself (the contract names `dialog::draw::ctl_color_kind` of W8-18h, not merged yet): dialog and button
//!   colours use the window brush, a static label the window brush with `Dark.Text`, an edit box and a list box the input brush with `Dark.TextStrong`.
//! - `PrintWindow` returns black for a window that is not shown, so `new()` and `dark()` show the owner WITHOUT activation far outside every monitor
//!   (-20000, -20000): the pixels are real and nothing flashes on the user's desktop. `hidden()` is never shown (the attach tests need no pixels).
#![allow(dead_code)] // each test crate that includes this file uses a different part

use std::any::Any;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::mpsc;
use std::time::Duration;

use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::template::{DialogControlClass, window_class_name};
use pr_platform_win::win32::api::user32::{get_client_rect, get_window_long_ptr, get_window_rect};
use pr_platform_win::win32::comctl::common_controls::{
    def_subclass_proc, init_common_controls_ex, set_window_subclass,
};
use pr_platform_win::win32::comctl::user32::{get_class_name, get_parent};
use pr_platform_win::win32::dialog_native::gdi32::{create_solid_brush, delete_object, set_bk_color, set_text_color};
use pr_platform_win::win32::dialog_native::user32::{get_dlg_ctrl_id, get_window, get_window_text, screen_to_client};
use pr_platform_win::win32::structs::{Hdc, Hgdi, Hwnd, Point, Rect, to_utf16z};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDC, GetDIBits, HBITMAP, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, SW_SHOWNOACTIVATE, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WS_CHILD, WS_POPUP, WS_VISIBLE,
};
use windows::core::PCWSTR;

pub const MIN_TEXT_CONTRAST: f64 = 4.5;
pub const MIN_GRAPHIC_CONTRAST: f64 = 3.0;
pub const MAX_BACKGROUND_LUMINANCE: f64 = 0.12;

#[link(name = "user32")]
unsafe extern "system" {
    fn PrintWindow(hwnd: isize, hdc: isize, flags: u32) -> i32;
}

// ---- STA thread -----------------------------------------------------------------------------------------------------------------------------------------

/// Runs `f` on its own thread in a COM single-threaded apartment and waits at most 60 s for it; a panic of `f` is resumed on the calling thread.
pub fn on_sta<F: FnOnce() + Send + 'static>(f: F) {
    let (sender, receiver) = mpsc::channel::<Result<(), Box<dyn Any + Send>>>();
    let worker = std::thread::Builder::new()
        .name("painter-native-sta".into())
        .spawn(move || {
            // SAFETY: plain arguments; paired with `CoUninitialize` below on the same thread.
            let initialised = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
            let outcome = catch_unwind(AssertUnwindSafe(f));
            if initialised {
                // SAFETY: balances the successful `CoInitializeEx` above, on the same thread.
                unsafe { CoUninitialize() };
            }
            let _ = sender.send(outcome);
        })
        .expect("spawn the STA thread");
    match receiver.recv_timeout(Duration::from_secs(60)) {
        Ok(Ok(())) => {
            let _ = worker.join();
        }
        Ok(Err(payload)) => {
            let _ = worker.join();
            resume_unwind(payload);
        }
        Err(_) => panic!("the native test did not finish within 60 s"),
    }
}

// ---- windows --------------------------------------------------------------------------------------------------------------------------------------------

fn to_hwnd(hwnd: Hwnd) -> HWND {
    HWND(hwnd as *mut core::ffi::c_void)
}

#[allow(clippy::too_many_arguments)] // a thin CreateWindowEx shape
fn create(
    class: &str,
    text: &str,
    style: u32,
    ex_style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    parent: Option<Hwnd>,
) -> Hwnd {
    let class = to_utf16z(class);
    let text = to_utf16z(text);
    // SAFETY: both strings are NUL-terminated UTF-16 locals that outlive the call; the parent is a live window of this thread or none.
    let hwnd = unsafe {
        let module = GetModuleHandleW(None).expect("module handle");
        CreateWindowExW(
            WINDOW_EX_STYLE(ex_style),
            PCWSTR(class.as_ptr()),
            PCWSTR(text.as_ptr()),
            WINDOW_STYLE(style),
            x,
            y,
            w,
            h,
            parent.map(to_hwnd),
            None,
            Some(module.into()),
            None,
        )
    }
    .expect("create the window");
    hwnd.0 as isize
}

struct DarkBrushes {
    window: Hgdi,
    input: Hgdi,
}

/// The `WM_CTLCOLOR*` answers of a `NativeDialog` (see the module doc).
unsafe extern "system" fn dark_owner_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    _id: usize,
    ref_data: usize,
) -> isize {
    // SAFETY: `ref_data` is the address of the `DarkBrushes` boxed by `OwnerWindow::dark`, alive until the owner is dropped (after `DestroyWindow`).
    let brushes = unsafe { &*(ref_data as *const DarkBrushes) };
    let hdc = wparam as Hdc;
    match message {
        0x0136 => return brushes.window,
        0x0135 | 0x0132 | 0x0137 => {
            set_text_color(hdc, DialogColors::text());
            set_bk_color(hdc, DialogColors::window());
            return brushes.window;
        }
        0x0138 => {
            set_text_color(hdc, DialogColors::text());
            set_bk_color(hdc, DialogColors::window());
            return brushes.window;
        }
        0x0133 | 0x0134 => {
            set_text_color(hdc, DialogColors::text_strong());
            set_bk_color(hdc, DialogColors::input());
            return brushes.input;
        }
        _ => {}
    }
    // SAFETY: called from a subclass procedure with its own message parameters.
    unsafe { def_subclass_proc(hwnd, message, wparam, lparam) }
}

/// A `WS_POPUP` `STATIC` owner of 640 x 2000 (at (-20000, -20000) when shown, see the module doc), destroyed on drop. Create and drop it on the thread that uses
/// it. `child` and `child_ex` stack the children vertically so that none overlaps another (the pixels of one are never those of its neighbour).
pub struct OwnerWindow {
    pub hwnd: Hwnd,
    brushes: Option<Box<DarkBrushes>>,
    next_y: std::cell::Cell<i32>,
}

impl OwnerWindow {
    fn make(show: bool, dark: bool) -> Self {
        let hwnd = create(
            "STATIC",
            "painter-owner",
            WS_POPUP.0,
            0,
            -20000,
            -20000,
            640,
            2000,
            None,
        );
        let mut owner = Self {
            hwnd,
            brushes: None,
            next_y: std::cell::Cell::new(8),
        };
        if dark {
            let brushes = Box::new(DarkBrushes {
                window: create_solid_brush(DialogColors::window()),
                input: create_solid_brush(DialogColors::input()),
            });
            let address = std::ptr::from_ref::<DarkBrushes>(&brushes) as usize;
            // SAFETY: `dark_owner_proc` lives for the process; `ref_data` is the heap address of the boxed brushes, which `owner` keeps alive past the window.
            let attached = unsafe { set_window_subclass(hwnd, dark_owner_proc, 0x5041_4E54, address) };
            assert!(attached, "subclass the dark owner");
            owner.brushes = Some(brushes);
        }
        if show {
            // SAFETY: a live window of this thread.
            let _ = unsafe { ShowWindow(to_hwnd(hwnd), SW_SHOWNOACTIVATE) };
        }
        owner
    }

    /// The plain owner, shown without activation (offscreen).
    pub fn new() -> Self {
        Self::make(true, false)
    }

    /// The plain owner, never shown.
    pub fn hidden() -> Self {
        Self::make(false, false)
    }

    /// The owner that answers `WM_CTLCOLORDLG / BTN / STATIC / EDIT / LISTBOX` like a dark `NativeDialog` (shown without activation, offscreen).
    pub fn dark() -> Self {
        Self::make(true, true)
    }
}

impl Drop for OwnerWindow {
    fn drop(&mut self) {
        // SAFETY: the handle was created by `make` on this thread and is destroyed once.
        let _ = unsafe { DestroyWindow(to_hwnd(self.hwnd)) };
        if let Some(brushes) = self.brushes.take() {
            delete_object(brushes.window);
            delete_object(brushes.input);
        }
    }
}

/// A child of `owner` at (8, next free row): the common controls are initialised, the class name is `template::window_class_name(class)`, the style is `WS_CHILD | WS_VISIBLE | style`.
pub fn child(owner: &OwnerWindow, class: DialogControlClass, style: u32, text: &str, width: i32, height: i32) -> Hwnd {
    let y = owner.next_y.get();
    owner.next_y.set(y + height + 8);
    child_at(owner, class, style, text, 8, y, width, height)
}

/// Like [`child`] at an explicit position.
#[allow(clippy::too_many_arguments)] // position and size of a window
pub fn child_at(
    owner: &OwnerWindow,
    class: DialogControlClass,
    style: u32,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Hwnd {
    // ICC_WIN95_CLASSES: list view, header, tool bar, trackbar, progress... (idempotent).
    let _ = init_common_controls_ex(0x0000_00FF);
    create(
        window_class_name(class),
        text,
        WS_CHILD.0 | WS_VISIBLE.0 | style,
        0,
        x,
        y,
        width,
        height,
        Some(owner.hwnd),
    )
}

/// Like [`child`] with extended window styles (`WS_EX_CLIENTEDGE`, ...).
pub fn child_ex(
    owner: &OwnerWindow,
    class: DialogControlClass,
    style: u32,
    ex_style: u32,
    text: &str,
    width: i32,
    height: i32,
) -> Hwnd {
    let _ = init_common_controls_ex(0x0000_00FF);
    let y = owner.next_y.get();
    owner.next_y.set(y + height + 8);
    create(
        window_class_name(class),
        text,
        WS_CHILD.0 | WS_VISIBLE.0 | style,
        ex_style,
        8,
        y,
        width,
        height,
        Some(owner.hwnd),
    )
}

/// Pumps the messages of this thread until the queue is empty (bounded to 10 000 messages).
pub fn drain(_hwnd: Hwnd) {
    let mut msg = MSG::default();
    for _ in 0..10_000 {
        // SAFETY: `msg` is a live local; the loop only dispatches messages of this thread.
        unsafe {
            if !PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

// ---- pixels ---------------------------------------------------------------------------------------------------------------------------------------------

/// A screenshot of one window (`0xRRGGBB` per pixel) and the screen position of its top-left corner.
pub struct Shot {
    pub width: i32,
    pub height: i32,
    pub origin: (i32, i32),
    pub rgb: Vec<u32>,
}

impl Shot {
    /// `PrintWindow` of the top-level window of `hwnd` (like the C# test, which printed the dialog and read its children by screen position: `PrintWindow`
    /// of a child window alone answers black) into a memory bitmap of the window size.
    pub fn capture(hwnd: Hwnd) -> Shot {
        let mut hwnd = hwnd;
        loop {
            let parent = get_parent(hwnd);
            if parent == 0 {
                break;
            }
            hwnd = parent;
        }
        let rect = window_rect(hwnd);
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        // SAFETY: the screen DC is released at the end; the memory DC and the bitmap are owned here and deleted before returning; `BITMAPINFO` is a live local
        // and the pixel buffer has `width * height * 4` bytes, the size `GetDIBits` is told (32 bits per pixel, top-down).
        unsafe {
            let screen = GetDC(None);
            let memory = CreateCompatibleDC(Some(screen));
            let bitmap: HBITMAP = CreateCompatibleBitmap(screen, width, height);
            let old = SelectObject(memory, HGDIOBJ(bitmap.0));
            let _ = PrintWindow(hwnd, memory.0 as isize, 0);
            SelectObject(memory, old);
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = vec![0_u8; usize::try_from(width).unwrap_or(0) * usize::try_from(height).unwrap_or(0) * 4];
            let _ = GetDIBits(
                memory,
                bitmap,
                0,
                height as u32,
                Some(pixels.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            );
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            let rgb = pixels
                .chunks_exact(4)
                .map(|p| (u32::from(p[2]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[0]))
                .collect();
            Shot {
                width,
                height,
                origin: (rect.left, rect.top),
                rgb,
            }
        }
    }

    /// The colour at screen position (x, y); `0xFF00FF` outside the shot.
    pub fn at(&self, screen_x: i32, screen_y: i32) -> u32 {
        let x = screen_x - self.origin.0;
        let y = screen_y - self.origin.1;
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return 0xFF00FF;
        }
        usize::try_from(y * self.width + x)
            .ok()
            .and_then(|i| self.rgb.get(i))
            .copied()
            .unwrap_or(0xFF00FF)
    }
}

/// The `0xRRGGBB` of a palette key (`0xFF00FF` when unknown).
pub fn rgb(key: &str) -> u32 {
    let c = DialogColors::from_key(key).unwrap_or(0x00FF_00FF);
    ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

/// WCAG relative luminance of `0xRRGGBB`.
pub fn luminance(rgb: u32) -> f64 {
    fn channel(v: u32) -> f64 {
        let c = f64::from(v) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel((rgb >> 16) & 0xFF) + 0.7152 * channel((rgb >> 8) & 0xFF) + 0.0722 * channel(rgb & 0xFF)
}

/// WCAG contrast ratio of two colours.
pub fn contrast(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

pub fn window_rect(hwnd: Hwnd) -> Rect {
    get_window_rect(hwnd).expect("window rect")
}

/// The client rectangle of `hwnd` in screen coordinates.
pub fn client_on_screen(hwnd: Hwnd) -> Rect {
    let window = window_rect(hwnd);
    let client = get_client_rect(hwnd).expect("client rect");
    let mut origin = Point {
        x: window.left,
        y: window.top,
    };
    let _ = screen_to_client(hwnd, &mut origin); // = -(offset of the client area inside the window)
    let left = window.left - origin.x;
    let top = window.top - origin.y;
    Rect {
        left,
        top,
        right: left + (client.right - client.left),
        bottom: top + (client.bottom - client.top),
    }
}

/// (background = the most frequent colour, ink = the colour with the best contrast against it, that contrast, the number of non-background pixels) in the
/// client area of `control`.
pub fn analyse(shot: &Shot, control: Hwnd) -> (u32, u32, f64, usize) {
    let rect = client_on_screen(control);
    let mut histogram: HashMap<u32, usize> = HashMap::new();
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            *histogram.entry(shot.at(x, y)).or_default() += 1;
        }
    }
    let background = histogram
        .iter()
        .max_by_key(|(color, count)| (**count, **color))
        .map_or(0, |(color, _)| *color);
    let mut ink = background;
    let mut best = 1.0;
    for color in histogram.keys() {
        let c = contrast(*color, background);
        if c > best {
            best = c;
            ink = *color;
        }
    }
    let ink_pixels = histogram
        .iter()
        .filter(|(color, _)| **color != background)
        .map(|(_, n)| *n)
        .sum();
    (background, ink, best, ink_pixels)
}

/// One violation of the "dark and readable" rule.
#[derive(Debug, Clone)]
pub struct Finding {
    pub control: String,
    pub class: String,
    pub problem: String,
}

/// C# `DarkAndReadableFindings`: every visible child of `dialog` must have a background luminance of at most 0.12 and an ink contrast of at least 4.5 for
/// text, 3.0 for progress bars, trackbars and group frames (empty static labels and group frames are skipped).
pub fn dark_and_readable_findings(dialog: Hwnd) -> Vec<Finding> {
    let shot = Shot::capture(dialog);
    let mut findings = Vec::new();
    // GW_CHILD = 5, GW_HWNDNEXT = 2.
    let mut child = get_window(dialog, 5);
    while child != 0 {
        let current = child;
        child = get_window(current, 2);
        let text = get_window_text(current);
        let name = format!(
            "id {} '{}'",
            get_dlg_ctrl_id(current),
            text.chars().take(24).collect::<String>()
        );
        let class = get_class_name(current).unwrap_or_default();
        let rect = window_rect(current);
        let style = get_window_long_ptr(current, -16) as u32;
        // The "visible" test of the C#: the WS_VISIBLE flag of the child (the owner may be offscreen).
        if style & 0x1000_0000 == 0 || rect.right - rect.left < 8 || rect.bottom - rect.top < 6 {
            continue;
        }
        let (background, ink, contrast_ratio, ink_pixels) = analyse(&shot, current);
        let is_graphic = class == "msctls_progress32" || class == "msctls_trackbar32";
        let is_frame = class == "Button" && style & 0xF == 7;
        if class == "Static" && (is_frame || text.is_empty()) {
            continue;
        }
        if luminance(background) > MAX_BACKGROUND_LUMINANCE {
            findings.push(Finding {
                control: name.clone(),
                class: class.clone(),
                problem: format!(
                    "light background #{background:06X} (luminance {:.2})",
                    luminance(background)
                ),
            });
        }
        if ink_pixels > 0 {
            let needed = if is_graphic || is_frame {
                MIN_GRAPHIC_CONTRAST
            } else {
                MIN_TEXT_CONTRAST
            };
            if contrast_ratio < needed {
                findings.push(Finding {
                    control: name,
                    class,
                    problem: format!("contrast {contrast_ratio:.2}:1 < {needed} (ink #{ink:06X} on #{background:06X})"),
                });
            }
        }
    }
    findings
}

/// Used by the unused-import lints of test crates that include this file but not every helper.
pub fn send_for_tests(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: the callers pass integer parameters only (or pointers to live locals that match the message).
    unsafe { pr_platform_win::win32::dialog_native::user32::send_message(hwnd, message, wparam, lparam) }
}
