//! Port of `NativeDialog.cs` lines 1 to 188 (class declaration, fields, constructor, properties, the virtual hooks) and 535 to 677 (`DialogProc`,
//! `RethrowPending`, `HandleMessage`), baseline `d92d6aae`. Unit W8-18h. Contract: `contracts/W8-18h.md`.
//!
//! The C# `abstract class NativeDialog` becomes a trait ([`DialogContent`], the abstract and virtual members) plus a handle ([`NativeDialog`]) that every hook
//! receives, so a hook calls the C# `protected` helpers (`Place`, `Item`, `SetItemText`...) on the handle (D-W8B5-301). The shared state sits in ONE
//! `DialogInner` (all fields declared here, precedent D-W8B4M-1); each feature file owns a small `*State` struct and adds an `impl NativeDialog` block.
//!
//! Re-entrancy: the dialog procedure is re-entered by every `SendMessage` a hook makes. State is `Cell`/`RefCell`; a `RefCell` borrow is NEVER held across an OS
//! call, a hook or a callback (D-W8B4I-1: decide under the borrow, act after it).
//!
//! `unsafe` here: the cookie (a `Box<Rc<DialogInner>>` pointer given to `CreateDialogIndirectParam` as `lParam` and stored in `DWLP_USER`, the C# `GCHandle`) and
//! the `extern "system"` dialog procedure.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use super::{dpi, draw, events, layout, modal, tooltip};
use crate::dialogs::native::chrome::{ChromePainter, DarkDialogChrome, DialogColors, NativeDialogChrome};
use crate::dialogs::native::template::{DialogItem, TemplateError};
use crate::host::error::HostError;
use crate::host::pending_panic;
use crate::win32::structs::{Hgdi, Hwnd, Lparam, Lresult, Msg, Win32Error, Wparam};
use crate::win32::{api::user32, dialog_native, messages};

/// C# `IdOk`: the id of the default confirm button; `IDOK` of the dialog manager.
pub const ID_OK: i32 = 1;
/// C# `IdCancel`: `IDCANCEL` of the dialog manager (Esc, the Cancel / Close button, the X button).
pub const ID_CANCEL: i32 = 2;
/// C# `SizeMinimized` = `SIZE_MINIMIZED`: the `wParam` of a `WM_SIZE` that must not trigger a layout.
#[allow(dead_code)] // W8-18i's WM_SIZE handler consumes this constant after its stub is ported.
pub(super) const SIZE_MINIMIZED: u32 = 1;
/// C# `WsDefaultDialog` = `WS_POPUP | WS_CAPTION | WS_SYSMENU` (`0x80C80000`).
pub const WS_DEFAULT_DIALOG: u32 = 0x8000_0000 | 0x00C0_0000 | 0x0008_0000;

/// What `show_modal` / `show_modeless` can fail with (the C# exceptions: `InvalidOperationException`, `ArgumentException` of `DialogTemplate.Build`,
/// `Win32Exception` of `CreateDialogIndirectParam`, and what `MessageLoop.RunNested` throws). A panic inside a hook is NOT an error: it is carried across
/// the OS frames by `host::pending_panic` and resumed by the caller of `show_*` (D-W8B5-207).
#[derive(Debug)]
pub enum DialogError {
    /// C# `InvalidOperationException("A NativeDialog instance can be shown once.")`.
    AlreadyShown,
    /// C# `ArgumentException("Too many controls for a dialog template.")`.
    Template(TemplateError),
    /// C# `Win32Exception(GetLastError, "CreateDialogIndirectParam failed.")`.
    CreateFailed(Win32Error),
    /// `MessageLoop::run_nested` failed (the C# `VerifyAccess` `InvalidOperationException`).
    Loop(HostError),
}

impl fmt::Display for DialogError {
    /// `AlreadyShown`: `"A NativeDialog instance can be shown once."`; `Template`: the `TemplateError` text; `CreateFailed`: `"CreateDialogIndirectParam failed
    /// (Win32 error {code})."`; `Loop`: the `HostError` text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyShown => write!(f, "A NativeDialog instance can be shown once."),
            Self::Template(error) => write!(f, "{error}"),
            Self::CreateFailed(error) => write!(f, "CreateDialogIndirectParam failed (Win32 error {}).", error.0),
            Self::Loop(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DialogError {}

impl From<TemplateError> for DialogError {
    fn from(error: TemplateError) -> Self {
        Self::Template(error)
    }
}

impl From<HostError> for DialogError {
    fn from(error: HostError) -> Self {
        Self::Loop(error)
    }
}

/// The C# tuple `(string Text, string? Badge)` of `ListItemText`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListItemText {
    /// The row text.
    pub text: String,
    /// The badge pill drawn at the right of the row; `None` = no badge.
    pub badge: Option<String>,
}

/// The abstract and virtual members of the C# `NativeDialog` (D-W8B5-301). Every method is `&self` (a hook can be re-entered by a nested `SendMessage`), so an
/// implementor keeps its state in `Cell`/`RefCell`; `dialog` is the C# `this` for the `protected` helpers. An implementor must NOT store a [`NativeDialog`]
/// (the handle owns the content: a stored handle would be a cycle); a dialog type that needs external methods is `struct X { native: NativeDialog, content:
/// Rc<XContent> }`. A hook must not panic (R0003); a panic is captured by `pending_panic` and the dialog closes as Cancel.
#[allow(unused_variables)] // Virtual hook defaults keep the C# parameter names for implementors.
pub trait DialogContent {
    /// C# abstract `Title`: the window caption.
    fn title(&self) -> String;

    /// C# abstract `Items`: the child controls in Z order (= Tab order). Called ONCE per `show_*` for the template and ONCE at `WM_INITDIALOG` (the C# called
    /// the getter three times; the list is constant by contract).
    fn items(&self) -> Vec<DialogItem>;

    /// C# virtual `Resizable`: `WS_THICKFRAME`. Default `false`.
    fn resizable(&self) -> bool {
        false
    }

    /// C# virtual `HeaderControlIds`: controls drawn with the larger header font. Default empty.
    fn header_control_ids(&self) -> Vec<i32> {
        Vec::new()
    }

    /// C# virtual `InitialFocusId`: `0` = the first control with `WS_TABSTOP`. Default `0`.
    fn initial_focus_id(&self) -> i32 {
        0
    }

    /// C# virtual `DefaultButtonId`: the button Enter presses, `ID_OK`, or `ID_CANCEL` for a dialog with only a Close button. Default `ID_OK`.
    fn default_button_id(&self) -> i32 {
        ID_OK
    }

    /// C# abstract `PreferredClientSize()`: the wanted client size in px at the current DPI and font (also used on a DPI change of a non-resizable dialog).
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32);

    /// C# virtual `MinimumClientSize()`: the smallest client size when resizing. Default `preferred_client_size`.
    fn minimum_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        self.preferred_client_size(dialog)
    }

    /// C# abstract `LayoutControls(clientWidth, clientHeight)`: place the controls (`dialog.place(..)`) for this client size in px.
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32);

    /// C# virtual `OnInitialized()`: after the controls, fonts and dark chrome exist, before the first layout (set texts, fill lists). Default nothing.
    fn on_initialized(&self, dialog: &NativeDialog) {}

    /// C# virtual `OnOk()`: Enter / `IDOK`. `true` = close with `ID_OK`; `false` = stay open (the hook reported the error). Default `true`.
    fn on_ok(&self, dialog: &NativeDialog) -> bool {
        true
    }

    /// C# virtual `OnCommand(id, notification)`: `WM_COMMAND` of a control other than `IDOK` / `IDCANCEL`; `true` = handled. Default `false`.
    fn on_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        false
    }

    /// C# virtual `OnCancel()`: Esc / `IDCANCEL` from the dialog manager (not the X button or Alt+F4); `true` = handled, do not close. Default `false`.
    fn on_cancel(&self, dialog: &NativeDialog) -> bool {
        false
    }

    /// C# virtual `OnNotify(id, code, header, out result)`: `WM_NOTIFY` of control `id`; `header` is the `lParam` (a `*const NMHDR`, valid for the call only).
    /// `Some(result)` = handled and `result` goes to `DWLP_MSGRESULT`; `None` = not handled. Default `None`.
    fn on_notify(&self, dialog: &NativeDialog, id: i32, code: i32, header: Lparam) -> Option<Lresult> {
        None
    }

    /// C# virtual `OnScroll(id, code)`: `WM_HSCROLL` / `WM_VSCROLL` of trackbar `id` (`code` = `TB_*`). Default nothing.
    fn on_scroll(&self, dialog: &NativeDialog, id: i32, code: i32) {}

    /// C# virtual `ListItemText(id, index)`: text and badge of row `index` of the owner-draw list box `id`. Default
    /// `ListItemText { text: ListBoxControl::text_at(dialog.item(id), index), badge: None }`.
    fn list_item_text(&self, dialog: &NativeDialog, id: i32, index: i32) -> ListItemText {
        ListItemText {
            text: crate::dialogs::native::controls::basic::ListBoxControl::text_at(dialog.item(id), index),
            badge: None,
        }
    }

    /// C# virtual `ListItemSubtitle(id, index)`: the small second line of an owner-draw row; `None` = single line. Default `None`.
    fn list_item_subtitle(&self, dialog: &NativeDialog, id: i32, index: i32) -> Option<String> {
        None
    }

    /// C# virtual `ListItemBadgeColor(id, index)`: the `COLORREF` of the badge text and outline; `None` = the default badge colours. Default `None`.
    fn list_item_badge_color(&self, dialog: &NativeDialog, id: i32, index: i32) -> Option<u32> {
        None
    }

    /// C# virtual `CanCloseByUser`: may the user close now (X, Alt+F4, `WM_CLOSE`)? `Close()` from outside always closes. Default `true`.
    fn can_close_by_user(&self) -> bool {
        true
    }

    /// C# virtual `WaitCursor`: show the wait cursor over the whole dialog. Default `false`.
    fn wait_cursor(&self) -> bool {
        false
    }

    /// C# virtual `OnClosed()`: once, right before the window is destroyed (normal close, close from outside, a captured panic). Default nothing.
    fn on_closed(&self, dialog: &NativeDialog) {}

    /// C# virtual `OnWindowMessage(message, wParam, lParam, out result)`: a message the frame does not handle (`WM_APP`, `WM_MOUSEWHEEL`...). `Some(result)`
    /// = handled, `result` goes to `DWLP_MSGRESULT`; `None` = default processing. Default `None`.
    fn on_window_message(
        &self,
        dialog: &NativeDialog,
        message: u32,
        wparam: Wparam,
        lparam: Lparam,
    ) -> Option<Lresult> {
        None
    }

    /// C# virtual `OnDialogKey(in Msg)`: a key message BEFORE `IsDialogMessage` sees it (Ctrl+Tab); `true` = handled. Default `false`.
    fn on_dialog_key(&self, dialog: &NativeDialog, message: &Msg) -> bool {
        false
    }

    /// C# virtual `StaticTextColor(id)`: the `COLORREF` of the text of a STATIC. Default `DialogColors::text()`.
    fn static_text_color(&self, dialog: &NativeDialog, id: i32) -> u32 {
        DialogColors::text()
    }

    /// C# virtual `ExtraWindowStyle`: extra `WS_*` bits (the minimize and maximize boxes of a modeless window). Default `0`.
    fn extra_window_style(&self) -> u32 {
        0
    }

    /// C# virtual `UsesModalFrame`: `DS_MODALFRAME` and `WS_EX_DLGMODALFRAME`; `false` for a modeless window styled like a normal window. Default `true`.
    fn uses_modal_frame(&self) -> bool {
        true
    }
}

/// C# `DarkDialogChrome.Instance` for the base class: the chrome used when `NativeDialog::new` is given none. Built once per thread
/// (`DarkDialogChrome::new(Rc::new(painter))`); until the painter group (W8-18l..18n) lands this uses the `NoPainter` of this file (D-W8B5-305).
pub fn default_chrome() -> Rc<dyn NativeDialogChrome> {
    thread_local! { static CHROME: Rc<DarkDialogChrome> = Rc::new(DarkDialogChrome::new(Rc::new(NoPainter))); }
    CHROME.with(|chrome| Rc::clone(chrome) as Rc<dyn NativeDialogChrome>)
}

struct NoPainter;
impl ChromePainter for NoPainter {
    fn attach_dialog(&self, _: Hwnd) -> bool {
        false
    }
    fn attach_control(&self, _: Hwnd, _: crate::dialogs::native::template::DialogControlClass) -> bool {
        false
    }
    fn attach_header_filler(&self, _: Hwnd) -> bool {
        false
    }
}

/// A native Win32 dialog (C# `NativeDialog`). The handle is cheap to clone; every clone is the same dialog. Not `Send` (UI thread only). Each object shows
/// once (`show_modal` or `show_modeless`).
#[derive(Clone)]
pub struct NativeDialog {
    pub(super) inner: Rc<DialogInner>,
}

impl fmt::Debug for NativeDialog {
    /// `NativeDialog { hwnd, closed }` (no content, no state dump).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeDialog")
            .field("hwnd", &self.hwnd())
            .field("closed", &self.is_closed())
            .finish()
    }
}

/// All state of one dialog. The `*State` structs are declared by the file that uses them.
pub(super) struct DialogInner {
    /// The abstract and virtual members.
    pub(super) content: Rc<dyn DialogContent>,
    /// C# `_chrome`.
    pub(super) chrome: Rc<dyn NativeDialogChrome>,
    pub(super) modal: modal::ModalState,
    pub(super) metrics: dpi::MetricsState,
    pub(super) controls: layout::ControlsState,
    pub(super) events: events::EventsState,
    pub(super) tips: tooltip::TipState,
    pub(super) draw: draw::DrawState,
}

impl NativeDialog {
    /// C# constructor with `chrome = null`: `with_chrome(content, None)`.
    pub fn new(content: Rc<dyn DialogContent>) -> Self {
        Self::with_chrome(content, None)
    }

    /// C# constructor `NativeDialog(INativeDialogChrome? chrome = null)`: `None` = `default_chrome()`. Nothing is created yet (no window, no handle).
    pub fn with_chrome(content: Rc<dyn DialogContent>, chrome: Option<Rc<dyn NativeDialogChrome>>) -> Self {
        Self {
            inner: Rc::new(DialogInner {
                content,
                chrome: chrome.unwrap_or_else(default_chrome),
                modal: modal::ModalState::new(),
                metrics: dpi::MetricsState {
                    dpi: Cell::new(96),
                    font: Cell::new(0),
                    header_font: Cell::new(0),
                    avg_char_width: Cell::new(6),
                    line_height: Cell::new(15),
                },
                controls: layout::ControlsState {
                    children: RefCell::new(Vec::new()),
                    child_hosts: RefCell::new(Vec::new()),
                },
                events: events::EventsState {
                    list_views: RefCell::new(Vec::new()),
                    sorts: RefCell::new(HashMap::new()),
                },
                tips: tooltip::TipState {
                    tooltip: Cell::new(0),
                    tip_buffer: RefCell::new(Vec::new()),
                    item_tips: RefCell::new(Vec::new()),
                    last_tip_item: RefCell::new(HashMap::new()),
                },
                draw: draw::DrawState {
                    window_brush: Cell::new(0),
                    input_brush: Cell::new(0),
                },
            }),
        }
    }

    /// C# `Hwnd`: the dialog window (`0` before creation and after destruction).
    pub fn hwnd(&self) -> Hwnd {
        self.inner.modal.hwnd.get()
    }

    /// C# `IsClosed`: `show_modal` finished or `close` / `end_modal` was called.
    pub fn is_closed(&self) -> bool {
        self.inner.modal.done.get()
    }

    /// C# `Dpi`: the DPI used for fonts and units (`96` until `WM_INITDIALOG`).
    pub fn dpi(&self) -> u32 {
        self.inner.metrics.dpi.get()
    }

    /// C# `BodyFont`: the current body font (`0` before creation).
    pub fn body_font(&self) -> Hgdi {
        self.inner.metrics.font.get()
    }

    /// C# `SelfRefData`: the cookie as the `ref_data` of a subclass procedure installed by a derived dialog. Valid until the window is destroyed (`0` after).
    pub fn self_ref_data(&self) -> usize {
        self.inner.modal.self_cookie.get() as usize
    }

    /// The content object (for the owner of a composite dialog type and for tests).
    pub fn content(&self) -> &Rc<dyn DialogContent> {
        &self.inner.content
    }

    /// Recovers the dialog from a cookie WITHOUT consuming it (clones the `Rc`): the first thing the dialog procedure and the subclass procedures do.
    ///
    /// # Safety
    /// `cookie` is `0` or a value returned by `make_cookie` that `free_cookie` has not released.
    pub(super) unsafe fn from_cookie(cookie: isize) -> Option<NativeDialog> {
        if cookie == 0 {
            return None;
        }
        Some(Self {
            // SAFETY: cookie is a live pointer produced by make_cookie; cloning preserves ownership of the boxed Rc.
            inner: unsafe { (*(cookie as *const Rc<DialogInner>)).clone() },
        })
    }
}

/// Boxes a clone of `inner` and returns the thin pointer for `lpInitParam` / `DWLP_USER` (never `0`). Pair with exactly one [`free_cookie`] (C# `GCHandle.Alloc`
/// and `Free`).
pub(super) fn make_cookie(inner: &Rc<DialogInner>) -> isize {
    Box::into_raw(Box::new(Rc::clone(inner))) as isize
}

/// Releases a cookie from [`make_cookie`]; `0` is a no-op.
///
/// # Safety
/// `cookie` came from `make_cookie` and was not freed; the dialog procedure clones the `Rc` before it calls anything, so a handler may free its own cookie.
pub(super) unsafe fn free_cookie(cookie: isize) {
    if cookie != 0 {
        // SAFETY: cookie came from make_cookie and is freed exactly once here.
        drop(unsafe { Box::from_raw(cookie as *mut Rc<DialogInner>) });
    }
}

/// The `DLGPROC` of every native dialog (C# `[UnmanagedCallersOnly] DialogProc`, lines 535 to 560). Algorithm: `WM_INITDIALOG`: `SetWindowLongPtr(hwnd,
/// DWLP_USER, lparam)` and the dialog is `from_cookie(lparam)`; any other message: `from_cookie(GetWindowLongPtr(hwnd, DWLP_USER))` (`0` -> `None`). `None` ->
/// `0`. Else `dialog.handle_message(..)` inside `catch_unwind`; a panic is `pending_panic::capture`d, the dialog ends as Cancel (`end_modal(ID_CANCEL)`) and the
/// result is `0` (C# `catch { s_pending ??= ...; dialog?.EndModal(IdCancel); return 0; }`). The `end_modal` of that error arm runs outside the `catch_unwind` (for a modeless
/// dialog it reaches the `closed` callback through `cleanup`), so it is wrapped in `pending_panic::guarded` too. Never unwinds.
///
/// # Safety
/// Only the OS calls it, as a `DLGPROC`, with a window of a dialog created by `show_modal` / `show_modeless` on this thread.
pub(super) unsafe extern "system" fn dialog_proc(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
    let mut dialog = None;
    let answer = catch_unwind(AssertUnwindSafe(|| {
        let cookie = if message == dialog_native::WM_INITDIALOG {
            // SAFETY: lparam is our live cookie and DWLP_USER is this dialog's private slot.
            unsafe { user32::set_window_long_ptr(hwnd, dialog_native::DWLP_USER, lparam) };
            lparam
        } else {
            user32::get_window_long_ptr(hwnd, dialog_native::DWLP_USER)
        };
        // SAFETY: nonzero cookie was stored by this dialog and remains live until cleanup.
        dialog = unsafe { NativeDialog::from_cookie(cookie) };
        dialog
            .as_ref()
            .map_or(0, |d| d.handle_message(hwnd, message, wparam, lparam))
    }));
    match answer {
        Ok(value) => value,
        Err(payload) => {
            pending_panic::capture(payload);
            if let Some(dialog) = dialog {
                pending_panic::guarded((), AssertUnwindSafe(|| dialog.end_modal(ID_CANCEL)));
            }
            0
        }
    }
}

impl NativeDialog {
    /// C# `HandleMessage(hwnd, message, wParam, lParam)` (lines 572 to 677): the router. Cases, in the C# order (each calls the member of the unit that owns it):
    /// `WM_INITDIALOG` -> `initialize_dialog(hwnd)` (18j), result `0` (FALSE: the focus is set by hand); `WM_COMMAND` -> `handle_command(wparam & 0xFFFF,
    /// (wparam >> 16) & 0xFFFF)` (18j), `1`/`0`; `WM_CLOSE` -> `on_close_request()` (18k), `1`; `WM_SETCURSOR` -> `on_set_cursor(hwnd)` (18k); `WM_NOTIFY` ->
    /// `handle_notify(hwnd, lparam)` (18j); `WM_DRAWITEM` -> `draw_owner_item(lparam)` (18k), `1`/`0`; `WM_HSCROLL` and `WM_VSCROLL` -> `on_scroll_message(wparam,
    /// lparam)` (18j), `0`; `WM_SIZE` -> `on_size(wparam)` (18i), `1`; `WM_GETMINMAXINFO` -> `on_get_min_max_info(lparam)` (18i), `1`; `WM_DPICHANGED` ->
    /// `on_dpi_changed_message(wparam, lparam)` (18i; it takes the low word of `wparam`), `1`; the six `WM_CTLCOLOR*` -> `on_ctl_color(message, wparam, lparam)` (18k, `Some` = the brush);
    /// anything else -> `content.on_window_message(..)`: `Some(r)` -> `SetWindowLongPtr(hwnd, DWLP_MSGRESULT, r)` and `1`, `None` -> `0`.
    pub(super) fn handle_message(&self, hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        match message {
            dialog_native::WM_INITDIALOG => {
                self.initialize_dialog(hwnd);
                0
            }
            messages::WM_COMMAND => {
                let (id, notification) = events::split_command_wparam(wparam);
                isize::from(self.handle_command(id, notification))
            }
            messages::WM_CLOSE => self.on_close_request(),
            messages::WM_SETCURSOR => self.on_set_cursor(hwnd),
            crate::win32::control_constants::WM_NOTIFY => self.handle_notify(hwnd, lparam),
            crate::win32::control_constants::WM_DRAWITEM => isize::from(self.draw_owner_item(lparam)),
            crate::win32::control_constants::WM_HSCROLL | crate::win32::control_constants::WM_VSCROLL => {
                self.on_scroll_message(wparam, lparam);
                0
            }
            messages::WM_SIZE => {
                self.on_size(wparam);
                1
            }
            messages::WM_GETMINMAXINFO => {
                self.on_get_min_max_info(lparam);
                1
            }
            messages::WM_DPICHANGED => {
                self.on_dpi_changed_message(wparam, lparam);
                1
            }
            dialog_native::WM_CTLCOLORDLG
            | dialog_native::WM_CTLCOLORBTN
            | dialog_native::WM_CTLCOLORMSGBOX
            | dialog_native::WM_CTLCOLORSCROLLBAR
            | dialog_native::WM_CTLCOLORSTATIC
            | dialog_native::WM_CTLCOLOREDIT
            | dialog_native::WM_CTLCOLORLISTBOX => self.on_ctl_color(message, wparam, lparam).unwrap_or(0),
            _ => match self.inner.content.on_window_message(self, message, wparam, lparam) {
                Some(result) => {
                    // SAFETY: DWLP_MSGRESULT is this dialog's result slot; result is an integer answer from its hook.
                    unsafe { user32::set_window_long_ptr(hwnd, dialog_native::DWLP_MSGRESULT, result) };
                    1
                }
                None => 0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Content;
    impl DialogContent for Content {
        fn title(&self) -> String {
            String::new()
        }
        fn items(&self) -> Vec<DialogItem> {
            Vec::new()
        }
        fn preferred_client_size(&self, _: &NativeDialog) -> (i32, i32) {
            (100, 100)
        }
        fn layout_controls(&self, _: &NativeDialog, _: i32, _: i32) {}
    }

    #[test]
    fn cookie_round_trip_keeps_the_dialog_alive_until_freed() {
        // C#: NativeDialog._self / GCHandle.Alloc and Free.
        let dialog = NativeDialog::new(Rc::new(Content));
        let initial = Rc::strong_count(&dialog.inner);
        let cookie = make_cookie(&dialog.inner);
        assert_ne!(cookie, 0);
        assert_eq!(Rc::strong_count(&dialog.inner), initial + 1);
        // SAFETY: cookie came from make_cookie above and remains live until free_cookie below.
        let recovered = unsafe { NativeDialog::from_cookie(cookie) }.expect("live cookie");
        assert!(Rc::ptr_eq(&dialog.inner, &recovered.inner));
        assert_eq!(Rc::strong_count(&dialog.inner), initial + 2);
        drop(recovered);
        // SAFETY: the cookie came from make_cookie and has not been freed.
        unsafe { free_cookie(cookie) };
        assert_eq!(Rc::strong_count(&dialog.inner), initial);
        // SAFETY: zero is expressly valid for both functions.
        unsafe {
            assert!(NativeDialog::from_cookie(0).is_none());
            free_cookie(0);
        }
    }

    #[test]
    fn size_minimized_matches_the_csharp_golden() {
        // C#: NativeDialog.SizeMinimized.
        let golden = pr_test_support::golden::load("dialog-layout");
        let row = golden
            .cases
            .iter()
            .find(|c| c["kind"] == "constants")
            .expect("constants row");
        assert_eq!(u64::from(SIZE_MINIMIZED), row["size_minimized"]);
    }
}
