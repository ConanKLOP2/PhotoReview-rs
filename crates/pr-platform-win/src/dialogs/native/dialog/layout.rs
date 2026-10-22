//! Port of `NativeDialog.cs` lines 1147 to 1431 (child registry, `Item`, `Place`, the text and measure helpers, button rows, `WindowSizeForClient`,
//! `ResizeClient`, `CurrentClientSize`, `RunLayout`, `PlaceInitially`) except `SelfRefData` (1148 to 1150, `self_ref_data` of 18h) and `SetListViewSort` (1209 to 1224, 18j), plus the bodies of the `WM_SIZE` and `WM_GETMINMAXINFO` cases of `HandleMessage` (lines 616 to 633),
//! baseline `d92d6aae`. Unit W8-18i. Contract: `contracts/W8-18i.md`.
//!
//! The layout of a dialog is in dialog units (`dx`/`dy`, `dpi.rs`) except multi-line text, whose height is MEASURED (`measure_text_height`), so long Vietnamese
//! strings are never clipped. Every helper is the C# `protected` member of the same name (`pub` here: Rust has no `protected`).
use std::cell::RefCell;

use super::core::NativeDialog;
use crate::dialogs::native::chrome::DialogChromeControl;
use crate::dialogs::native::list_dialog::ListDialogBase;
use crate::win32::structs::MonitorInfo;
use crate::win32::structs::{Hgdi, Hwnd, Lparam, Rect, Wparam};
use crate::win32::{api::user32, control_constants, dialog_native};

/// Child registry (C# `_children`, `_childHosts`).
pub(super) struct ControlsState {
    /// C# `_children`: the template controls and the ones added with `register_child`, in registration order; font changes and `first_tab_stop` walk it.
    pub(super) children: RefCell<Vec<Hwnd>>,
    /// C# `_childHosts`: scrolling container windows that the template controls were re-parented into (the Settings pages); `item` searches them.
    pub(super) child_hosts: RefCell<Vec<Hwnd>>,
}

impl ControlsState {
    /// Both lists empty.
    #[allow(dead_code)] // The 18h constructor still initializes fields inline; a later slice will use this constructor.
    pub(super) fn new() -> Self {
        Self {
            children: RefCell::new(Vec::new()),
            child_hosts: RefCell::new(Vec::new()),
        }
    }
}

/// C# `ButtonWidth` arithmetic (pure): `max(dx50, max over the texts of (text_width + dx16))`; no texts gives `dx50`. `i32` wrapping like the C# unchecked `int`.
pub fn button_width_for(text_widths: &[i32], dx50: i32, dx16: i32) -> i32 {
    text_widths
        .iter()
        .fold(dx50, |width, text| width.max(text.wrapping_add(dx16)))
}

/// C# `ButtonRowWidth` / the return value of `PlaceButtonRow` (pure): `count * width + (count - 1) * gap` with `i32` wrapping (`count == 0` gives `-gap`, as the
/// C# `ids.Length - 1` does).
pub fn button_row_total(count: usize, width: i32, gap: i32) -> i32 {
    let count = count as i32;
    count
        .wrapping_mul(width)
        .wrapping_add(count.wrapping_sub(1).wrapping_mul(gap))
}

/// The left edges `PlaceButtonRow` gives its buttons, left to right (pure): the first is `client_width - right_margin - count * width - (count - 1) * gap`, each
/// next is `width + gap` further right; `count` entries.
pub fn button_row_positions(client_width: i32, right_margin: i32, count: usize, width: i32, gap: i32) -> Vec<i32> {
    let mut x = client_width
        .wrapping_sub(right_margin)
        .wrapping_sub(button_row_total(count, width, gap));
    (0..count)
        .map(|_| {
            let current = x;
            x = x.wrapping_add(width).wrapping_add(gap);
            current
        })
        .collect()
}

/// The work rectangle `PlaceInitially` clamps to (pure): `monitor_work` when the monitor information could be read, else `(0, 0, screen_width, screen_height)`.
pub fn work_area(monitor_work: Option<Rect>, screen_width: i32, screen_height: i32) -> Rect {
    monitor_work.unwrap_or(Rect {
        left: 0,
        top: 0,
        right: screen_width,
        bottom: screen_height,
    })
}

/// The window rectangle `PlaceInitially` computes (pure, lines 1393 to 1431): `(x, y, width, height)`.
/// 1. centre: with an owner rectangle `x = o.left + (o.right - o.left - fw) / 2`, `y = o.top + (o.bottom - o.top - fh) / 2`; without, the same in `work`
///    (truncating `/ 2`, `i32` wrapping).
/// 2. `resizable`: `(fw, fh)` becomes `(min(fw, work.right - work.left), min(fh, work.bottom - work.top))` (a small screen shrinks a big window).
/// 3. `x = max(work.left, min(x, work.right - fw))`, `y = max(work.top, min(y, work.bottom - fh))` in THAT order (min first: a frame wider than the work area
///    ends at `work.left`).
pub fn initial_frame(
    frame_width: i32,
    frame_height: i32,
    owner_rect: Option<Rect>,
    work: Rect,
    resizable: bool,
) -> (i32, i32, i32, i32) {
    let centre = owner_rect.unwrap_or(work);
    let mut width = frame_width;
    let mut height = frame_height;
    let mut x = centre
        .left
        .wrapping_add(centre.right.wrapping_sub(centre.left).wrapping_sub(width) / 2);
    let mut y = centre
        .top
        .wrapping_add(centre.bottom.wrapping_sub(centre.top).wrapping_sub(height) / 2);
    if resizable {
        width = width.min(work.right.wrapping_sub(work.left));
        height = height.min(work.bottom.wrapping_sub(work.top));
    }
    x = x.min(work.right.wrapping_sub(width)).max(work.left);
    y = y.min(work.bottom.wrapping_sub(height)).max(work.top);
    (x, y, width, height)
}

impl NativeDialog {
    /// C# `AddChildHost(host)`: remember a scrolling container whose children `item` must find.
    pub fn add_child_host(&self, host: Hwnd) {
        self.inner.controls.child_hosts.borrow_mut().push(host);
    }

    /// C# `RegisterChild(control)`: a control a derived dialog created AFTER the dialog exists. `0` -> nothing. Else it joins `children` and gets the body font
    /// (`WM_SETFONT`), so later DPI changes update it and `cleanup` forgets it.
    pub fn register_child(&self, control: Hwnd) {
        if control == 0 {
            return;
        }
        self.inner.controls.children.borrow_mut().push(control);
        // SAFETY: WM_SETFONT carries a font handle and an integer flag, with no pointer.
        unsafe {
            dialog_native::user32::send_message(control, 0x0030, self.body_font() as usize, 0);
        }
    }

    /// C# `ApplyChrome(controls)`: the dark look for controls created later: `chrome.apply_controls(hwnd, controls)` (never the title bar).
    pub fn apply_chrome(&self, controls: &[DialogChromeControl]) {
        self.inner.chrome.apply_controls(self.hwnd(), controls);
    }

    /// C# `Item(id)`: `get_dlg_item(hwnd, id)`; while that is `0`, the same in each child host in order. `0` when absent.
    pub fn item(&self, id: i32) -> Hwnd {
        let mut child = dialog_native::user32::get_dlg_item(self.hwnd(), id);
        if child != 0 {
            return child;
        }
        let hosts = self.inner.controls.child_hosts.borrow().clone();
        for host in hosts {
            child = dialog_native::user32::get_dlg_item(host, id);
            if child != 0 {
                break;
            }
        }
        child
    }

    /// C# `ControlHandle(id)` (`internal`): `item(id)`, for tests and composed views.
    pub fn control_handle(&self, id: i32) -> Hwnd {
        self.item(id)
    }

    /// C# `Place(id, x, y, width, height)`: `move_window(item(id), x, y, max(0, width), max(0, height), true)`; an absent control or a failed move is ignored.
    pub fn place(&self, id: i32, x: i32, y: i32, width: i32, height: i32) {
        let child = self.item(id);
        if child != 0 {
            let _ = dialog_native::user32::move_window(child, x, y, width.max(0), height.max(0), true);
        }
    }

    /// C# `SetListItemHeight(id, heightPx)`: `LB_SETITEMHEIGHT` with `lparam = max(1, height_px)`; absent control -> nothing.
    pub fn set_list_item_height(&self, id: i32, height_px: i32) {
        let child = self.item(id);
        if child != 0 {
            // SAFETY: LB_SETITEMHEIGHT carries integer values only; no pointer is passed.
            unsafe {
                dialog_native::user32::send_message(
                    child,
                    control_constants::LB_SETITEMHEIGHT,
                    0,
                    height_px.max(1) as isize,
                );
            }
        }
    }

    /// C# `SetItemText(id, text)`: `set_window_text(item(id), text)`; absent control -> nothing; the error is ignored.
    pub fn set_item_text(&self, id: i32, text: &str) {
        let child = self.item(id);
        if child != 0 {
            let _ = dialog_native::user32::set_window_text(child, text);
        }
    }

    /// C# `GetItemText(id)`: `Self::get_text(item(id))`.
    pub fn get_item_text(&self, id: i32) -> String {
        Self::get_text(self.item(id))
    }

    /// C# `GetText(hwnd)`: `""` for `0` or an empty text; else the window text.
    pub fn get_text(hwnd: Hwnd) -> String {
        if hwnd == 0 || dialog_native::user32::get_window_text_length(hwnd) <= 0 {
            String::new()
        } else {
            dialog_native::user32::get_window_text(hwnd)
        }
    }

    /// C# `ShowItem(id, visible)`: `show_window(item(id), SW_SHOWNA or SW_HIDE)`; absent control -> nothing.
    pub fn show_item(&self, id: i32, visible: bool) {
        let child = self.item(id);
        if child != 0 {
            let _ = user32::show_window(child, if visible { 8 } else { 0 });
        }
    }

    /// C# `MeasureItemHeight(id, widthPx)`: `0` for an absent control; else `measure_text_height(text of the control, width_px, WM_GETFONT of the control)`.
    pub fn measure_item_height(&self, id: i32, width_px: i32) -> i32 {
        let child = self.item(id);
        if child == 0 {
            return 0;
        }
        // SAFETY: WM_GETFONT takes no pointer and returns a font handle.
        let font = unsafe { dialog_native::user32::send_message(child, 0x0031, 0, 0) };
        self.measure_text_height(&Self::get_text(child), width_px, font)
    }

    /// C# `MeasureTextHeight(text, widthPx, font)`: the height of `text` word-wrapped in `width_px`, at least one line. `get_dc(hwnd)` `0` -> `line_height`;
    /// select `font` (or the body font when `font == 0`); `rect = (0, 0, max(1, width_px), 0)`; `draw_text(hdc, text, &mut rect, DT_CALCRECT | DT_WORDBREAK |
    /// DT_NOPREFIX)`; reselect the old font; `release_dc`; `max(line_height, rect.bottom - rect.top)`.
    pub fn measure_text_height(&self, text: &str, width_px: i32, font: Hgdi) -> i32 {
        let hwnd = self.hwnd();
        let hdc = dialog_native::user32::get_dc(hwnd);
        if hdc == 0 {
            return self.line_height();
        }
        let old = dialog_native::gdi32::select_object(hdc, if font == 0 { self.body_font() } else { font });
        let mut rect = Rect {
            left: 0,
            top: 0,
            right: width_px.max(1),
            bottom: 0,
        };
        let _ = dialog_native::user32::draw_text(hdc, text, &mut rect, 0x0400 | 0x0010 | 0x0800);
        let _ = dialog_native::gdi32::select_object(hdc, old);
        let _ = dialog_native::user32::release_dc(hwnd, hdc);
        self.line_height().max(rect.bottom.wrapping_sub(rect.top))
    }

    /// C# `MeasureTextWidth(text, font = 0)`: the width of one line; `0` when there is no DC or the measure fails. `font == 0` = the body font.
    pub fn measure_text_width(&self, text: &str, font: Hgdi) -> i32 {
        let hwnd = self.hwnd();
        let hdc = dialog_native::user32::get_dc(hwnd);
        if hdc == 0 {
            return 0;
        }
        let old = dialog_native::gdi32::select_object(hdc, if font == 0 { self.body_font() } else { font });
        let width = dialog_native::gdi32::get_text_extent_point32(hdc, text).map_or(0, |size| size.x);
        let _ = dialog_native::gdi32::select_object(hdc, old);
        let _ = dialog_native::user32::release_dc(hwnd, hdc);
        width
    }

    /// C# `ButtonWidth(ids)`: `button_width_for(widths of the texts of the controls, dx(50), dx(16))`.
    pub fn button_width(&self, ids: &[i32]) -> i32 {
        let widths: Vec<_> = ids
            .iter()
            .map(|id| self.measure_text_width(&self.get_item_text(*id), 0))
            .collect();
        button_width_for(&widths, self.dx(50), self.dx(16))
    }

    /// C# `PlaceButtonRow(clientWidth, rightMargin, top, height, ids)`: a right-aligned row, `ids[0]` leftmost, equal widths (`button_width(ids)`), gap `dx(6)`;
    /// returns `button_row_total(..)`.
    pub fn place_button_row(&self, client_width: i32, right_margin: i32, top: i32, height: i32, ids: &[i32]) -> i32 {
        let width = self.button_width(ids);
        let gap = self.dx(6);
        for (id, x) in ids
            .iter()
            .zip(button_row_positions(client_width, right_margin, ids.len(), width, gap))
        {
            self.place(*id, x, top, width, height);
        }
        button_row_total(ids.len(), width, gap)
    }

    /// C# `ButtonRowWidth(ids)`: `button_row_total(ids.len(), button_width(ids), dx(6))`.
    pub fn button_row_width(&self, ids: &[i32]) -> i32 {
        button_row_total(ids.len(), self.button_width(ids), self.dx(6))
    }

    /// C# `WindowSizeForClient(clientWidth, clientHeight)`: the window size (frame included) for that client size at the current DPI:
    /// `adjust_window_rect_ex_for_dpi(&mut (0, 0, w, h), GWL_STYLE, false, GWL_EXSTYLE, dpi)` (an `Err` is ignored: the rectangle stays the client rectangle).
    pub fn window_size_for_client(&self, client_width: i32, client_height: i32) -> (i32, i32) {
        let hwnd = self.hwnd();
        let mut rect = Rect {
            left: 0,
            top: 0,
            right: client_width,
            bottom: client_height,
        };
        let style = user32::get_window_long_ptr(hwnd, -16) as u32;
        let ex_style = user32::get_window_long_ptr(hwnd, -20) as u32;
        let _ = user32::adjust_window_rect_ex_for_dpi(&mut rect, style, false, ex_style, self.dpi());
        (rect.right.wrapping_sub(rect.left), rect.bottom.wrapping_sub(rect.top))
    }

    /// C# `ResizeClient(clientWidth, clientHeight)`: `hwnd == 0` -> nothing; else `set_window_pos(hwnd, 0, 0, 0, frame.0, frame.1, SWP_NOMOVE | SWP_NOZORDER
    /// | SWP_NOACTIVATE)` (the top-left corner stays).
    pub fn resize_client(&self, client_width: i32, client_height: i32) {
        let hwnd = self.hwnd();
        if hwnd != 0 {
            let (w, h) = self.window_size_for_client(client_width, client_height);
            let _ = user32::set_window_pos(hwnd, 0, 0, 0, w, h, 0x0002 | 0x0004 | 0x0010);
        }
    }

    /// C# `CurrentClientSize()`: `(right - left, bottom - top)` of `get_client_rect(hwnd)`; `(0, 0)` when the call fails.
    pub fn current_client_size(&self) -> (i32, i32) {
        user32::get_client_rect(self.hwnd())
            .map_or((0, 0), |r| (r.right.wrapping_sub(r.left), r.bottom.wrapping_sub(r.top)))
    }

    /// C# `RunLayout()`: `hwnd == 0` -> nothing; else `content.layout_controls(self, w, h)` for the current client size, then `invalidate_rect(hwnd, None, true)`.
    pub fn run_layout(&self) {
        let hwnd = self.hwnd();
        if hwnd == 0 {
            return;
        }
        let (w, h) = self.current_client_size();
        self.inner.content.layout_controls(self, w, h);
        let _ = dialog_native::user32::invalidate_rect(hwnd, None, true);
    }

    /// C# `PlaceInitially(clientWidth, clientHeight)`: sizes the window for the client size, centres it on the owner (or the work area), clamps it to the work
    /// area of the monitor of the owner (`monitor_from_window(owner, MONITOR_DEFAULTTOPRIMARY)`, `get_monitor_info`; failure -> `work_area(None, ..)` with
    /// `get_system_metrics(SM_CXSCREEN / SM_CYSCREEN)`), and `set_window_pos(hwnd, 0, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE)` using `initial_frame`.
    #[allow(dead_code)] // Initial dialog setup in 18j will call this helper.
    pub(super) fn place_initially(&self, client_width: i32, client_height: i32) {
        let hwnd = self.hwnd();
        let owner = self.inner.modal.owner.get();
        let owner_rect = if owner == 0 {
            None
        } else {
            user32::get_window_rect(owner).ok()
        };
        let monitor = user32::monitor_from_window(owner, 1);
        let mut info = MonitorInfo::default();
        let monitor_work = user32::get_monitor_info(monitor, &mut info).ok().map(|()| info.rcWork);
        let work = work_area(
            monitor_work,
            user32::get_system_metrics(0),
            user32::get_system_metrics(1),
        );
        let (w, h) = self.window_size_for_client(client_width, client_height);
        let (x, y, w, h) = initial_frame(w, h, owner_rect, work, self.inner.content.resizable());
        let _ = user32::set_window_pos(hwnd, 0, x, y, w, h, 0x0004 | 0x0010);
    }

    /// The `WM_SIZE` case: `laid_out && (wparam as u32) != SIZE_MINIMIZED` -> `run_layout()`.
    pub(super) fn on_size(&self, wparam: Wparam) {
        if self.inner.modal.laid_out.get() && (wparam as u32) != 1 {
            self.run_layout();
        }
    }

    /// The `WM_GETMINMAXINFO` case: `content.resizable() && laid_out && lparam != 0` -> `ptMinTrackSize = window_size_for_client(minimum_client_size)` written into
    /// the `MINMAXINFO` that `lparam` points to (SAFETY: valid for the message, written once).
    pub(super) fn on_get_min_max_info(&self, lparam: Lparam) {
        if !self.inner.content.resizable() || !self.inner.modal.laid_out.get() || lparam == 0 {
            return;
        }
        let (w, h) = self.inner.content.minimum_client_size(self);
        let (w, h) = self.window_size_for_client(w, h);
        // SAFETY: WM_GETMINMAXINFO supplies a writable MINMAXINFO valid for this message; update only its minimum track size.
        unsafe {
            let info = &mut *(lparam as *mut windows::Win32::UI::WindowsAndMessaging::MINMAXINFO);
            info.ptMinTrackSize.x = w;
            info.ptMinTrackSize.y = h;
        }
    }
}

/// The base-class members `ListDialog` calls (W8-18e, D-W8B5-208): the real dialog answers with the members above (`measure_text_width(text, 0)`,
/// `place_button_row(..)` without its return value).
impl ListDialogBase for NativeDialog {
    fn item(&self, id: i32) -> Hwnd {
        NativeDialog::item(self, id)
    }
    fn dx(&self, dialog_units: i32) -> i32 {
        NativeDialog::dx(self, dialog_units)
    }
    fn dy(&self, dialog_units: i32) -> i32 {
        NativeDialog::dy(self, dialog_units)
    }
    fn line_height(&self) -> i32 {
        NativeDialog::line_height(self)
    }
    fn measure_text_width(&self, text: &str) -> i32 {
        NativeDialog::measure_text_width(self, text, 0)
    }
    fn measure_item_height(&self, id: i32, width_px: i32) -> i32 {
        NativeDialog::measure_item_height(self, id, width_px)
    }
    fn place(&self, id: i32, x: i32, y: i32, width: i32, height: i32) {
        NativeDialog::place(self, id, x, y, width, height)
    }
    fn place_button_row(&self, client_width: i32, right_margin: i32, top: i32, height: i32, ids: &[i32]) {
        let _ = NativeDialog::place_button_row(self, client_width, right_margin, top, height, ids);
    }
    fn button_row_width(&self, ids: &[i32]) -> i32 {
        NativeDialog::button_row_width(self, ids)
    }
}
