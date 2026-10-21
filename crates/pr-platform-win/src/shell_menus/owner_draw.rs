//! Port of `PhotoReview.Shell.Win32/Menus/OwnerDrawMenuSession.cs` (336 lines, baseline `d92d6aae`): `OwnerDrawItem` and `OwnerDrawMenuSession`. Unit W8-14g.
//! Contract: `contracts/W8-14g.md`.
//!
//! Paints the native menu itself while `TrackPopupMenuEx` runs: the owner window is subclassed for the duration (`SetWindowSubclass`), because the menu
//! sends `WM_MEASUREITEM` and `WM_DRAWITEM` to the owner of the menu; the subclass measures and draws background, text, hover, check mark and submenu
//! arrow with GDI according to `owner_draw_style` and the dark palette. No uxtheme ordinal is used. Keyboard, submenus and multi-monitor stay the
//! system menu's. Order of use (W8-14f): `new`, `attach` BEFORE the menu is built, `register` per item, `dispose` (or drop) after the menu closed.
//!
//! Rust shape: the session is an `Rc` core (`SessionInner`) whose raw pointer is the `dwRefData` of the subclass (the C# `GCHandle`); `dispose` removes
//! the subclass first and only then releases that reference. The subclass procedure never lets a panic out (it is `extern "system"`): it catches,
//! logs and lets the menu draw its default (the C# `catch (Exception)` comment), see D-W8B4I-9.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;

use pr_core::abstractions::Log;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{HDC, LineTo, MoveToEx};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT};
use windows::Win32::UI::WindowsAndMessaging::{
    NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

use crate::shell_menus::owner_draw_style::{
    ODS_CHECKED, ODS_DISABLED, ODS_GRAYED, background, color_ref, item_background, item_height, item_text, item_width,
    left_gutter, right_pad, scale, separator, separator_height,
};
use crate::win32::api::gdi32::get_stock_object;
use crate::win32::comctl::common_controls::{def_subclass_proc, remove_window_subclass, set_window_subclass};
use crate::win32::comctl::gdi32::{create_pen, set_bk_mode};
use crate::win32::comctl::user32::fill_rect;
use crate::win32::dialog_native::gdi32::{
    create_font_indirect, create_solid_brush, delete_object, get_text_extent_point32, select_object, set_text_color,
};
use crate::win32::dialog_native::user32::{draw_text, get_dc, non_client_metrics_for_dpi, release_dc};
use crate::win32::structs::{Hdc, Hgdi, Hwnd, Lparam, Lresult, Rect, Wparam};

/// `WM_DRAWITEM`.
pub const WM_DRAWITEM: u32 = 0x002B;
/// `WM_MEASUREITEM`.
pub const WM_MEASUREITEM: u32 = 0x002C;
/// `ODT_MENU`: the `CtlType` of a menu item.
pub const ODT_MENU: u32 = 1;
/// The subclass id (C# `SubclassId` = `0x50524D4E`, "PRMN").
pub const SUBCLASS_ID: usize = 0x5052_4D4E;
/// `TRANSPARENT` background mode of `SetBkMode`.
pub const TRANSPARENT_BK: i32 = 1;

const DT_LEFT: u32 = 0x0000;
const DT_RIGHT: u32 = 0x0002;
const DT_VCENTER: u32 = 0x0004;
const DT_SINGLELINE: u32 = 0x0020;
const DT_NOPREFIX: u32 = 0x0800;
const DT_ENDELLIPSIS: u32 = 0x8000;
/// `DEFAULT_GUI_FONT`.
const DEFAULT_GUI_FONT: i32 = 17;
/// `PS_SOLID`.
const PS_SOLID: i32 = 0;
/// `sizeof(NONCLIENTMETRICSW)` since Vista.
const NONCLIENT_METRICS_SIZE: u32 = 504;

/// C# `OwnerDrawItem(Text, Shortcut, IsSeparator, IsSubmenu)`: one menu item to paint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerDrawItem {
    /// The item text (already localized, no `&` doubling: owner-draw text is drawn with `DT_NOPREFIX`).
    pub text: String,
    /// The accelerator text drawn right-aligned (`None` or empty = none).
    pub shortcut: Option<String>,
    /// A separator line.
    pub is_separator: bool,
    /// An item that opens a submenu (draws the arrow).
    pub is_submenu: bool,
}

/// The state behind a session; shared with the subclass procedure through its `dwRefData`.
struct SessionInner {
    items: RefCell<Vec<OwnerDrawItem>>,
    owner: Hwnd,
    dpi: u32,
    font: Cell<Hgdi>,
    /// The font is the stock `DEFAULT_GUI_FONT`: never deleted by us.
    font_is_stock: Cell<bool>,
    background_brush: Cell<Hgdi>,
    attached: Cell<bool>,
    /// `Rc::into_raw` of the strong reference owned by the subclass registration (0 while detached).
    ref_data: Cell<usize>,
    log: Option<Arc<dyn Log>>,
}

/// C# `OwnerDrawMenuSession(owner, dpi)`.
pub struct OwnerDrawMenuSession {
    inner: Rc<SessionInner>,
}

impl OwnerDrawMenuSession {
    /// C# constructor: creates the menu font (`create_menu_font(dpi)`) and the solid background brush (`owner_draw_style::background()`).
    pub fn new(owner: Hwnd, dpi: u32, log: Option<Arc<dyn Log>>) -> Self {
        let (font, font_is_stock) = create_menu_font_with(dpi, read_metrics);
        let brush = create_solid_brush(background());
        Self {
            inner: Rc::new(SessionInner {
                items: RefCell::new(Vec::new()),
                owner,
                dpi,
                font: Cell::new(font),
                font_is_stock: Cell::new(font_is_stock),
                background_brush: Cell::new(brush),
                attached: Cell::new(false),
                ref_data: Cell::new(0),
                log,
            }),
        }
    }

    /// C# `BackgroundBrush`: the brush for `MENUINFO.hbrBack` of every menu (border and padding in the menu colour).
    pub fn background_brush(&self) -> Hgdi {
        self.inner.background_brush.get()
    }

    /// C# `IsAttached`: false when `SetWindowSubclass` failed (the menu then keeps the system look).
    pub fn is_attached(&self) -> bool {
        self.inner.attached.get()
    }

    /// C# `Attach()`: `SetWindowSubclass(owner, subclass_proc, SUBCLASS_ID, ref_data)`.
    pub fn attach(&self) -> bool {
        if self.inner.attached.get() {
            return true;
        }
        let ref_data = Rc::into_raw(Rc::clone(&self.inner)) as usize;
        // SAFETY: `subclass_proc` is a `'static` function and stays valid until `remove_window_subclass` (`dispose`); `ref_data` is the pointer of a strong
        // count of this `SessionInner` that the registration owns until removal (reclaimed below on failure, or in `dispose`).
        let ok = unsafe { set_window_subclass(self.inner.owner, subclass_proc, SUBCLASS_ID, ref_data) };
        if ok {
            self.inner.ref_data.set(ref_data);
        } else {
            // SAFETY: `ref_data` came from `Rc::into_raw` just above and the registration failed, so nobody else holds it.
            drop(unsafe { Rc::from_raw(ref_data as *const SessionInner) });
        }
        self.inner.attached.set(ok);
        ok
    }

    /// C# `Register(item)`: stores the item and returns its `itemData` (the 1-based index) for `AppendMenu(MF_OWNERDRAW)`.
    pub fn register(&self, item: OwnerDrawItem) -> isize {
        let mut items = self.inner.items.borrow_mut();
        items.push(item);
        items.len() as isize
    }

    /// The measuring half of `WM_MEASUREITEM` for one item (C# `Measure` after the item lookup): `(width, height)` in pixels. A separator is
    /// `(scale(48), separator_height)`; any other item is measured with the menu font on the owner's DC (`GetDC`/`SelectObject`/
    /// `GetTextExtentPoint32`/`ReleaseDC`) as `(item_width(text, shortcut), item_height(text height))`. Public so tests measure without a menu.
    pub fn measure_item(&self, item: &OwnerDrawItem) -> (i32, i32) {
        self.inner.measure_item(item)
    }

    /// The painting half of `WM_DRAWITEM` for one item (C# `Draw` after the item lookup) into `hdc` over `rect` with the `ODS_*` `state`. Public so tests
    /// paint into a memory bitmap without a menu.
    pub fn draw_item(&self, hdc: Hdc, rect: Rect, state: u32, item: &OwnerDrawItem) {
        self.inner.draw_item(hdc, rect, state, item);
    }

    /// C# `Dispose()`: `RemoveWindowSubclass`, then releases the reference of the registration, deletes the font (unless it is the stock font) and the
    /// brush. Idempotent. No GDI object is leaked on any path.
    pub fn dispose(&self) {
        let inner = &self.inner;
        if inner.attached.get() {
            remove_window_subclass(inner.owner, subclass_proc, SUBCLASS_ID);
            let ref_data = inner.ref_data.replace(0);
            if ref_data != 0 {
                // SAFETY: `ref_data` is the `Rc::into_raw` pointer stored by `attach`, released exactly once (the cell was reset); the subclass was
                // removed first, so no further message can reach the procedure with it.
                drop(unsafe { Rc::from_raw(ref_data as *const SessionInner) });
            }
            inner.attached.set(false);
        }
        let brush = inner.background_brush.replace(0);
        if brush != 0 {
            delete_object(brush);
        }
        let font = inner.font.replace(0);
        if font != 0 && !inner.font_is_stock.get() {
            delete_object(font);
        }
    }
}

impl Drop for OwnerDrawMenuSession {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl fmt::Debug for OwnerDrawMenuSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnerDrawMenuSession").finish_non_exhaustive()
    }
}

/// Runs a measure/draw step and never lets a panic out: a panic is logged and counts as "not handled" (the menu draws its default). The C#
/// `catch (Exception)` comment: it must not throw across the unmanaged boundary (D-W8B4I-9).
fn run_guarded(log: &Option<Arc<dyn Log>>, step: impl FnOnce() -> bool) -> bool {
    match catch_unwind(AssertUnwindSafe(step)) {
        Ok(handled) => handled,
        Err(payload) => {
            if let Some(log) = log {
                let detail = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                let error = std::io::Error::other(detail);
                log.error("Owner-draw menu failed", Some(&error));
            }
            false
        }
    }
}

/// C# `SubclassProc` (`[UnmanagedCallersOnly]`): `WM_MEASUREITEM` and `WM_DRAWITEM` of a menu item go to the session (result 1 = handled); every
/// other message, and any case the session does not handle, goes to `DefSubclassProc`. Matches `win32::comctl::SubclassProc`.
unsafe extern "system" fn subclass_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    _id: usize,
    ref_data: usize,
) -> Lresult {
    if (message == WM_MEASUREITEM || message == WM_DRAWITEM) && ref_data != 0 {
        // SAFETY: the registration holds a strong count of this `SessionInner` (`Rc::into_raw` in `attach`) until `remove_window_subclass` in `dispose`,
        // and nothing else is ever stored with `SUBCLASS_ID`.
        let inner = unsafe { &*(ref_data as *const SessionInner) };
        let handled = run_guarded(&inner.log, || {
            if message == WM_MEASUREITEM {
                inner.measure(lparam)
            } else {
                inner.draw(lparam)
            }
        });
        if handled {
            return 1;
        }
    }
    // SAFETY: called from a subclass procedure with its own message parameters.
    unsafe { def_subclass_proc(hwnd, message, wparam, lparam) }
}

impl SessionInner {
    /// C# `Measure(lParam)`: fills `MEASUREITEMSTRUCT` (`itemWidth`, `itemHeight`) for a menu item; a separator gets `scale(48)` x `separator_height`;
    /// others `item_width(text, shortcut)` x `item_height(text height)` measured with the menu font on the owner's DC. False when it is not a menu item
    /// or the `itemData` is unknown.
    fn measure(&self, lparam: Lparam) -> bool {
        let ptr = lparam as *mut MEASUREITEMSTRUCT;
        if ptr.is_null() || !ptr.is_aligned() {
            return false;
        }
        // SAFETY: for `WM_MEASUREITEM` `lparam` points to a `MEASUREITEMSTRUCT` valid for the duration of the message (Win32 contract); non-null and
        // aligned checked above.
        let m = unsafe { &mut *ptr };
        if m.CtlType.0 != ODT_MENU {
            return false;
        }
        let Some(item) = self.item_for(m.itemData) else {
            return false;
        };
        let (w, h) = self.measure_item(&item);
        m.itemWidth = w as u32;
        m.itemHeight = h as u32;
        true
    }

    /// C# `Draw(lParam)`: paints one `DRAWITEMSTRUCT`: background by state, separator line, shortcut (`Dark.TextSecondary` when enabled) right-aligned,
    /// text left with end ellipsis, check mark (two segments) in the left gutter, submenu arrow (two segments) at the right. Per-draw pens and brushes
    /// are created and deleted inside the call; the DC's font is restored.
    fn draw(&self, lparam: Lparam) -> bool {
        let ptr = lparam as *const DRAWITEMSTRUCT;
        if ptr.is_null() || !ptr.is_aligned() {
            return false;
        }
        // SAFETY: for `WM_DRAWITEM` `lparam` points to a `DRAWITEMSTRUCT` valid for the duration of the message (Win32 contract); non-null and aligned
        // checked above. It is copied out so no reference outlives the read.
        let d = unsafe { *ptr };
        if d.CtlType.0 != ODT_MENU {
            return false;
        }
        let Some(item) = self.item_for(d.itemData) else {
            return false;
        };
        self.draw_item(d.hDC.0 as Hdc, d.rcItem, d.itemState.0, &item);
        true
    }

    /// C# `TryItem(itemData)`: `itemData` is a 1-based index.
    fn item_for(&self, item_data: usize) -> Option<OwnerDrawItem> {
        let index = item_data.checked_sub(1)?;
        self.items.borrow().get(index).cloned()
    }

    fn measure_item(&self, item: &OwnerDrawItem) -> (i32, i32) {
        if item.is_separator {
            return (scale(48, self.dpi), separator_height(self.dpi));
        }
        let dc = get_dc(self.owner);
        let _release = DcRelease { owner: self.owner, dc };
        let old = select_object(dc, self.font.get());
        let (text_w, text_h) = text_extent(dc, &item.text);
        let shortcut_w = match item.shortcut.as_deref() {
            Some(s) if !s.is_empty() => text_extent(dc, s).0,
            _ => 0,
        };
        select_object(dc, old);
        (item_width(text_w, shortcut_w, self.dpi), item_height(text_h, self.dpi))
    }

    fn draw_item(&self, dc: Hdc, rc: Rect, state: u32, item: &OwnerDrawItem) {
        let dpi = self.dpi;
        fill_solid(
            dc,
            &rc,
            if item.is_separator {
                background()
            } else {
                item_background(state)
            },
        );
        let mid_y = rc.top + (rc.bottom - rc.top) / 2;
        if item.is_separator {
            let line = Rect {
                left: rc.left + scale(8, dpi),
                top: mid_y,
                right: rc.right - scale(8, dpi),
                bottom: mid_y + 1,
            };
            fill_solid(dc, &line, separator());
            return;
        }

        let enabled = state & (ODS_GRAYED | ODS_DISABLED) == 0;
        let color = item_text(state);
        let old = select_object(dc, self.font.get());
        set_bk_mode(dc, TRANSPARENT_BK);
        set_text_color(dc, color);

        let mut text_rect = Rect {
            left: rc.left + left_gutter(dpi),
            top: rc.top,
            right: rc.right - right_pad(dpi),
            bottom: rc.bottom,
        };
        let shortcut_right = text_rect.right - if item.is_submenu { scale(16, dpi) } else { 0 };
        if let Some(shortcut) = item.shortcut.as_deref().filter(|s| !s.is_empty()) {
            let mut shortcut_rect = text_rect;
            shortcut_rect.right = shortcut_right;
            set_text_color(
                dc,
                if enabled {
                    color_ref("Dark.TextSecondary")
                } else {
                    color
                },
            );
            draw_text(
                dc,
                shortcut,
                &mut shortcut_rect,
                DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            set_text_color(dc, color);
        }
        draw_text(
            dc,
            &item.text,
            &mut text_rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_ENDELLIPSIS,
        );
        select_object(dc, old);

        if state & ODS_CHECKED != 0 {
            let cx = rc.left + scale(14, dpi);
            self.stroke_lines(
                dc,
                color,
                [
                    (cx - scale(4, dpi), mid_y),
                    (cx - scale(1, dpi), mid_y + scale(3, dpi)),
                    (cx + scale(4, dpi), mid_y - scale(4, dpi)),
                ],
            );
        }
        if item.is_submenu {
            let ax = rc.right - scale(12, dpi);
            let h = scale(4, dpi);
            // The system still draws its own black arrow after WM_DRAWITEM (cannot be disabled): black on dark, almost invisible.
            self.stroke_lines(dc, color, [(ax - h, mid_y - h), (ax, mid_y), (ax - h, mid_y + h)]);
        }
    }

    /// Two segments a-b-c with a GDI pen as thick as `scale(1)`.
    fn stroke_lines(&self, dc: Hdc, color: u32, points: [(i32, i32); 3]) {
        let pen = create_pen(PS_SOLID, scale(1, self.dpi).max(1), color);
        let old = select_object(dc, pen);
        move_to_ex(dc, points[0].0, points[0].1);
        line_to(dc, points[1].0, points[1].1);
        line_to(dc, points[2].0, points[2].1);
        select_object(dc, old);
        delete_object(pen);
    }
}

/// Releases the DC of `get_dc` on every path.
struct DcRelease {
    owner: Hwnd,
    dc: Hdc,
}

impl Drop for DcRelease {
    fn drop(&mut self) {
        if self.dc != 0 {
            release_dc(self.owner, self.dc);
        }
    }
}

/// C# `FillSolid`: a per-call brush, deleted again.
fn fill_solid(dc: Hdc, rect: &Rect, color: u32) {
    let brush = create_solid_brush(color);
    fill_rect(dc, rect, brush);
    delete_object(brush);
}

/// C# `TextWidth`: empty text is `(0, 0)`; a failed call too.
fn text_extent(dc: Hdc, text: &str) -> (i32, i32) {
    if text.is_empty() {
        return (0, 0);
    }
    get_text_extent_point32(dc, text).map_or((0, 0), |p| (p.x, p.y))
}

fn move_to_ex(dc: Hdc, x: i32, y: i32) {
    // SAFETY: a DC handle value the OS validates and plain integers; the previous-point pointer is null (not wanted).
    let _ = unsafe { MoveToEx(HDC(dc as *mut core::ffi::c_void), x, y, None::<*mut POINT>) };
}

fn line_to(dc: Hdc, x: i32, y: i32) {
    // SAFETY: a DC handle value the OS validates and plain integers.
    let _ = unsafe { LineTo(HDC(dc as *mut core::ffi::c_void), x, y) };
}

/// C# `CreateMenuFont(dpi)` metrics source: `dpi != 0` -> `non_client_metrics_for_dpi`, else the plain `SystemParametersInfoW`.
fn read_metrics(dpi: u32) -> Option<NONCLIENTMETRICSW> {
    if dpi != 0 {
        return non_client_metrics_for_dpi(dpi).ok();
    }
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: NONCLIENT_METRICS_SIZE,
        ..Default::default()
    };
    // SAFETY: the pointer is to a local `NONCLIENTMETRICSW` whose `cbSize` is its exact size (504); the OS fills it and does not keep the pointer.
    unsafe {
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            NONCLIENT_METRICS_SIZE,
            Some((&mut metrics as *mut NONCLIENTMETRICSW).cast::<core::ffi::c_void>()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .ok()
    .map(|()| metrics)
}

/// C# `CreateMenuFont(dpi)`: `NONCLIENTMETRICSW.lfMenuFont` for the DPI (`non_client_metrics_for_dpi`; the plain `SystemParametersInfo` when `dpi` is 0),
/// created with `CreateFontIndirect`; any failure falls back to the stock `DEFAULT_GUI_FONT` (17), which must never be deleted.
#[cfg(test)]
fn create_menu_font(dpi: u32) -> Hgdi {
    create_menu_font_with(dpi, read_metrics).0
}

/// The body of `create_menu_font` with the metrics source injected; the flag is true for the stock font.
fn create_menu_font_with(dpi: u32, metrics: impl FnOnce(u32) -> Option<NONCLIENTMETRICSW>) -> (Hgdi, bool) {
    if let Some(m) = metrics(dpi) {
        let font = create_font_indirect(&m.lfMenuFont);
        if font != 0 {
            return (font, false);
        }
    }
    (get_stock_object(DEFAULT_GUI_FONT), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingLog(Mutex<Vec<String>>);

    impl Log for RecordingLog {
        fn enabled(&self) -> bool {
            true
        }
        fn info(&self, _message: &str) {}
        fn warn(&self, _message: &str) {}
        fn error(&self, message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {
            self.0.lock().unwrap().push(message.to_owned());
        }
    }

    #[test]
    fn nonclientmetrics_lfmenufont_offset_is_224() {
        assert_eq!(core::mem::offset_of!(NONCLIENTMETRICSW, lfMenuFont), 224);
        assert_eq!(size_of::<NONCLIENTMETRICSW>(), 504);
    }

    #[test]
    fn a_panic_in_a_step_is_swallowed_logged_and_not_handled() {
        let log = Arc::new(RecordingLog::default());
        let dyn_log: Option<Arc<dyn Log>> = Some(log.clone());
        assert!(!run_guarded(&dyn_log, || panic!("boom")));
        assert_eq!(*log.0.lock().unwrap(), vec!["Owner-draw menu failed".to_owned()]);
        assert!(run_guarded(&dyn_log, || true));
        assert!(!run_guarded(&None, || false));
        assert!(!run_guarded(&None, || panic!("no log")));
    }

    #[test]
    fn create_menu_font_falls_back_to_the_stock_font_when_the_metrics_call_fails() {
        let (font, stock) = create_menu_font_with(96, |_| None);
        assert!(stock);
        assert_eq!(font, get_stock_object(DEFAULT_GUI_FONT));
        assert_ne!(font, 0);
    }

    #[test]
    fn create_menu_font_creates_an_owned_font_for_real_metrics() {
        let (font, stock) = create_menu_font_with(96, |_| Some(NONCLIENTMETRICSW::default()));
        assert!(!stock);
        assert_ne!(font, 0);
        assert!(delete_object(font));
        let real = create_menu_font(0);
        assert_ne!(real, 0);
    }

    #[test]
    fn register_is_one_based_and_item_for_rejects_zero_and_out_of_range() {
        let s = OwnerDrawMenuSession::new(0, 96, None);
        let item = |t: &str| OwnerDrawItem {
            text: t.to_owned(),
            shortcut: None,
            is_separator: false,
            is_submenu: false,
        };
        assert_eq!(s.register(item("a")), 1);
        assert_eq!(s.register(item("b")), 2);
        assert_eq!(s.inner.item_for(1).unwrap().text, "a");
        assert_eq!(s.inner.item_for(2).unwrap().text, "b");
        assert!(s.inner.item_for(0).is_none());
        assert!(s.inner.item_for(3).is_none());
    }
}
