//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/NativeDialogChromePainter.cs` (991 lines, baseline `d92d6aae`): the hand-painted dark look of the dialog
//! controls. Units W8-18l, W8-18m, W8-18n. Contracts: `contracts/W8-18l.md`, `W8-18m.md`, `W8-18n.md`; decisions D-W8B5-401 .. D-W8B5-416.
//!
//! The control theme is removed by `DarkDialogChrome` (`SetWindowTheme("", "")`, classic look); this painter then draws each control itself with the `Dark.*`
//! palette through a window subclass (`SetWindowSubclass`): buttons, check boxes, radio buttons, group boxes, the closed part of a combo box, progress bars,
//! trackbars and etched lines are painted whole on `WM_PAINT` (double buffered), edit boxes, list boxes and list views only get a flat border
//! (`WM_NCPAINT`, `WM_PRINT`), the header of a list view gets its right-hand filler repainted, and a subclass of the DIALOG corrects the selected-row colours
//! of the list views (`NM_CUSTOMDRAW`). Nothing here depends on the system theme, nor on an undocumented uxtheme ordinal.
//!
//! The C# class is one static class; the Rust twin is split by C# line ranges (D-W8B5-401, D-W8B5-402):
//!
//! | file | unit | C# members |
//! |---|---|---|
//! | `geometry` | 18l | `ChromePaintKind`, `KindFor`, the predicates, `RepaintsAfterSystemPaint`, `S`, `Key`, the decision of `AdjustListViewCustomDraw`, the `switch` of `ControlProc` as a pure function |
//! | `mod` (this file) | 18l | `Attach`, `IsAttached`, `AttachDialog`, `ControlProc`, `DialogProc`, `IsListView`, the `ChromePainter` implementation |
//! | `controls` | 18m | `AdjustListViewCustomDraw` (memory part), `RepaintNow`, `PaintWindow`, `PaintTo`, `Draw`, `Fill`, `Frame`, `WindowText`, `DrawText`, `TextWidth`, `TextHeight`, `UiState`, `DrawPushButton` |
//! | `widgets` | 18n | `DrawCheckOrRadio`, `DrawComboBox`, `DrawProgress`, `DrawTrackbar` |
//! | `frame` | 18n | `DrawGroupBox`, `DrawEtched`, `PaintHeaderFiller`, `BorderThickness`, `PaintBorder` |
//!
//! Lead-owned index: this file declares the submodules (the porter of 18l fills the bodies below it; 18m and 18n never edit it).

use std::cell::Cell;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows::Win32::UI::Controls::{NMHDR, NMLVCUSTOMDRAW};

pub mod controls;
pub mod frame;
pub mod geometry;
pub mod widgets;

pub use geometry::ChromePaintKind;

use super::chrome::ChromePainter;
use super::template::DialogControlClass;
use crate::host::pending_panic;
use crate::win32::comctl::common_controls::{def_subclass_proc, remove_window_subclass, set_window_subclass};
use crate::win32::comctl::user32::get_class_name;
use crate::win32::control_constants::{NM_CUSTOMDRAW, WC_LISTVIEW, WM_NOTIFY};
use crate::win32::dialog_chrome::{
    TME_LEAVE, WM_NCDESTROY, get_prop, get_window_dc, remove_prop, set_prop, track_mouse_event,
};
use crate::win32::dialog_native::user32::{get_dc, invalidate_rect, is_window, release_dc};
use crate::win32::structs::{Hdc, Hwnd};
use geometry::{ControlMessageAction, control_message_action};

/// C# `ControlSubclassId` = `0x43484D50` ("CHMP").
pub const CONTROL_SUBCLASS_ID: usize = 0x4348_4D50;
/// C# `DialogSubclassId` = `0x43484D44` ("CHMD").
pub const DIALOG_SUBCLASS_ID: usize = 0x4348_4D44;
/// C# `ControlProp` = `"PhotoReview.ChromePaint"` with the Rust prefix (D-W8B5-206): the window property that marks a control as attached; its value is the
/// address of the `PaintState`.
pub const CONTROL_PROP: &str = "PhotoReviewRs.ChromePaint";
/// C# `DialogProp` = `"PhotoReview.ChromeDialog"` with the Rust prefix: marks a dialog as subclassed (value `1`).
pub const DIALOG_PROP: &str = "PhotoReviewRs.ChromeDialog";

/// C# `PaintState` (a native struct `Kind, Hot, Tracking, Reserved`; `Tracking` and `Reserved` were never read, they are not ported). One per attached
/// control: boxed by `attach`, its address is the `ref_data` of the subclass and the value of [`CONTROL_PROP`], freed by `WM_NCDESTROY`. `hot` is a `Cell`
/// because the control procedure is re-entered by the `SendMessage` calls of the drawing code.
///
/// `non_exhaustive` + `new` + `Drop`: the number of live states is counted (D-W8B5-419) so a native test can prove that `WM_NCDESTROY` frees the box exactly
/// once (a heap box is invisible to `GetGuiResources`); a struct literal outside this module would unbalance the counter.
#[derive(Debug)]
#[non_exhaustive]
pub struct PaintState {
    /// How the control is drawn (never `None`: `attach` refuses it).
    pub kind: ChromePaintKind,
    /// C# `Hot`: the mouse is over the control (`WM_MOUSEMOVE` until `WM_MOUSELEAVE`).
    pub hot: Cell<bool>,
}

/// The number of `PaintState` values alive (test hook, D-W8B5-419).
static LIVE_STATES: AtomicUsize = AtomicUsize::new(0);

impl PaintState {
    /// A state for a control of this kind, not hot. Counts one live state.
    pub fn new(kind: ChromePaintKind) -> Self {
        LIVE_STATES.fetch_add(1, Ordering::Relaxed);
        Self {
            kind,
            hot: Cell::new(false),
        }
    }
}

impl Drop for PaintState {
    /// Counts one live state less.
    fn drop(&mut self) {
        LIVE_STATES.fetch_sub(1, Ordering::Relaxed);
    }
}

/// How many `PaintState` boxes are alive now: `attach` adds one, the `WM_NCDESTROY` of the control (or a refused `set_window_subclass`) removes it. Test hook
/// (D-W8B5-419).
#[doc(hidden)]
pub fn live_state_count() -> usize {
    LIVE_STATES.load(Ordering::Relaxed)
}

/// C# static class `NativeDialogChromePainter`: a unit struct so it can be handed to `DarkDialogChrome::new(Rc::new(NativeDialogChromePainter))` as the
/// `ChromePainter`. All state lives in the windows (property, subclass), none in the struct.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeDialogChromePainter;

impl NativeDialogChromePainter {
    /// C# `KindFor(hwnd, controlClass)`: `geometry::kind_for`.
    pub fn kind_for(hwnd: Hwnd, class: DialogControlClass) -> ChromePaintKind {
        geometry::kind_for(hwnd, class)
    }

    /// C# `Attach(hwnd, kind)` (lines 129-153): `false` when `hwnd == 0`, `kind == None`, or `!is_window(hwnd)`. Already attached (`get_prop(hwnd,
    /// CONTROL_PROP) != 0`) -> `true` without doing anything (idempotent). Else: box `PaintState::new(kind)`, `set_window_subclass(hwnd,
    /// control_proc, CONTROL_SUBCLASS_ID, ptr)`; on failure drop the box and answer `false`; else `set_prop(hwnd, CONTROL_PROP, ptr)`, `invalidate_rect(hwnd,
    /// None, true)` (erase: the first paint is ours) and `true`.
    pub fn attach(hwnd: Hwnd, kind: ChromePaintKind) -> bool {
        if hwnd == 0 || kind == ChromePaintKind::None || !is_window(hwnd) {
            return false;
        }
        if get_prop(hwnd, CONTROL_PROP) != 0 {
            return true;
        }
        let state = Box::into_raw(Box::new(PaintState::new(kind)));
        // SAFETY: `control_proc` is a `static` function that stays valid for the process; `ref_data` is the address of the box leaked just above, which
        // `WM_NCDESTROY` frees exactly once (or this function, when the subclass is refused).
        let attached = unsafe { set_window_subclass(hwnd, control_proc, CONTROL_SUBCLASS_ID, state as usize) };
        if !attached {
            // SAFETY: the box was leaked by `Box::into_raw` above and no subclass holds it (the call failed), so it is freed once here.
            drop(unsafe { Box::from_raw(state) });
            return false;
        }
        set_prop(hwnd, CONTROL_PROP, state as isize);
        invalidate_rect(hwnd, None, true);
        true
    }

    /// C# `IsAttached(hwnd)` (test hook): `hwnd != 0 && get_prop(hwnd, CONTROL_PROP) != 0`.
    pub fn is_attached(hwnd: Hwnd) -> bool {
        hwnd != 0 && get_prop(hwnd, CONTROL_PROP) != 0
    }

    /// C# `AttachDialog(dialog)` (lines 162-181): `false` for `0` or a dead window; already marked (`DIALOG_PROP`) -> `true`; else `set_window_subclass(dialog,
    /// dialog_proc, DIALOG_SUBCLASS_ID, 0)` (failure -> `false`), `set_prop(dialog, DIALOG_PROP, 1)`, `true`.
    pub fn attach_dialog(dialog: Hwnd) -> bool {
        if dialog == 0 || !is_window(dialog) {
            return false;
        }
        if get_prop(dialog, DIALOG_PROP) != 0 {
            return true;
        }
        // SAFETY: `dialog_proc` is a `static` function that stays valid for the process; it uses no `ref_data` (0).
        if !unsafe { set_window_subclass(dialog, dialog_proc, DIALOG_SUBCLASS_ID, 0) } {
            return false;
        }
        set_prop(dialog, DIALOG_PROP, 1);
        true
    }
}

impl ChromePainter for NativeDialogChromePainter {
    /// `NativeDialogChromePainter::attach_dialog`.
    fn attach_dialog(&self, dialog: Hwnd) -> bool {
        Self::attach_dialog(dialog)
    }

    /// C# `Attach(hwnd, KindFor(hwnd, class))`: the kind is chosen at attach time from the CURRENT window style.
    fn attach_control(&self, hwnd: Hwnd, class: DialogControlClass) -> bool {
        Self::attach(hwnd, Self::kind_for(hwnd, class))
    }

    /// C# `Attach(header, ChromePaintKind.HeaderFiller)`.
    fn attach_header_filler(&self, header: Hwnd) -> bool {
        Self::attach(header, ChromePaintKind::HeaderFiller)
    }
}

/// The shared painter for the production chrome: `Rc<dyn ChromePainter>` of the unit struct (`DarkDialogChrome::new(production_painter())`).
pub fn production_painter() -> Rc<dyn ChromePainter> {
    Rc::new(NativeDialogChromePainter)
}

/// C# `[UnmanagedCallersOnly] ControlProc` (lines 186-295) as a `SubclassProc`. Algorithm: `state = &*(ref_data as *const PaintState)`; the body runs inside
/// `pending_panic::guarded` (D-W8B5-207 and D-W8B5-410: a panic is captured and resumed by the owner of the native call; the message still goes to
/// `def_subclass_proc`, as the C# `catch (Exception) {}` followed by `DefSubclassProc`); `match geometry::control_message_action(state.kind, message, lparam)`:
/// `EraseBackgroundHandled` -> `1`; `PaintWindow` -> `controls::paint_window(hwnd, state)`, `0`; `PrintClient` -> `controls::paint_to(hwnd, wparam as Hdc, state)`,
/// `0`; `TrackHover` -> when `!state.hot`: `hot = true`, `track_mouse_event(TME_LEAVE, hwnd, 0)` (result ignored), `invalidate_rect(hwnd, None, false)`; then
/// default; `LeaveHover` -> `hot = false`, `invalidate_rect(.., false)`; default; `PaintNonClientBorder` -> `r = def`, `dc = get_window_dc(hwnd)`, when `dc != 0`
/// `frame::paint_border(hwnd, dc, state.kind)` and `release_dc(hwnd, dc)`; `r`; `PrintNonClientBorder` -> `r = def`, `frame::paint_border(hwnd, wparam as Hdc,
/// kind)`, `r`; `PaintHeaderFiller` -> `r = def`, `dc = get_dc(hwnd)`, `frame::paint_header_filler(hwnd, dc)`, `release_dc`, `r`; `PrintHeaderFiller` -> `r =
/// def`, `frame::paint_header_filler(hwnd, wparam as Hdc)`, `r`; `InvalidateAfterDefault` -> `r = def`, `invalidate_rect(.., false)`, `r`; `RepaintAfterDefault`
/// -> `r = def`, `controls::repaint_now(hwnd, state)`, `r`; `Destroy` -> `remove_window_subclass(hwnd, control_proc, id)`, `remove_prop(hwnd, CONTROL_PROP)`,
/// `drop(Box::from_raw(ref_data as *mut PaintState))` and NOTHING touches `state` afterwards, then default; `PassThrough` -> default. `def` is
/// `def_subclass_proc(hwnd, message, wparam, lparam)`.
///
/// # Safety
/// Only the OS calls it, as the subclass procedure installed by [`NativeDialogChromePainter::attach`], with `ref_data` the address of that call's `PaintState`.
pub(crate) unsafe extern "system" fn control_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    id: usize,
    ref_data: usize,
) -> isize {
    if ref_data == 0 {
        // SAFETY: called from a subclass procedure with its own message parameters.
        return unsafe { def_subclass_proc(hwnd, message, wparam, lparam) };
    }
    let state_ptr = ref_data as *const PaintState;
    // SAFETY: `ref_data` is the address of the `PaintState` boxed by `attach` for this subclass, alive until `WM_NCDESTROY` frees it. The kind is copied
    // before any message is handled (T3: nothing reads the state after `Destroy` freed it).
    let kind = unsafe { (*state_ptr).kind };
    let action = control_message_action(kind, message, lparam);
    // `None` = the message goes to `def_subclass_proc` (also after a captured panic, D-W8B5-410); `Some(r)` = answer `r`.
    let answer = pending_panic::guarded(
        None,
        AssertUnwindSafe(|| {
            // SAFETY: as above; the reference is read only while this message is processed, and not created for `Destroy`.
            let state = || unsafe { &*state_ptr };
            let default = || {
                // SAFETY: called from a subclass procedure with its own message parameters.
                unsafe { def_subclass_proc(hwnd, message, wparam, lparam) }
            };
            match action {
                ControlMessageAction::PassThrough => None,
                ControlMessageAction::EraseBackgroundHandled => Some(1),
                ControlMessageAction::PaintWindow => {
                    controls::paint_window(hwnd, state());
                    Some(0)
                }
                ControlMessageAction::PrintClient => {
                    controls::paint_to(hwnd, wparam as Hdc, state());
                    Some(0)
                }
                ControlMessageAction::TrackHover => {
                    let state = state();
                    if !state.hot.get() {
                        state.hot.set(true);
                        let _ = track_mouse_event(TME_LEAVE, hwnd, 0);
                        invalidate_rect(hwnd, None, false);
                    }
                    None
                }
                ControlMessageAction::LeaveHover => {
                    state().hot.set(false);
                    invalidate_rect(hwnd, None, false);
                    None
                }
                ControlMessageAction::PaintNonClientBorder => {
                    let result = default();
                    let hdc = get_window_dc(hwnd);
                    if hdc != 0 {
                        let _release = ReleaseDcOnDrop { hwnd, hdc };
                        frame::paint_border(hwnd, hdc, kind);
                    }
                    Some(result)
                }
                ControlMessageAction::PrintNonClientBorder => {
                    let result = default();
                    frame::paint_border(hwnd, wparam as Hdc, kind);
                    Some(result)
                }
                ControlMessageAction::PaintHeaderFiller => {
                    let result = default();
                    let hdc = get_dc(hwnd);
                    if hdc != 0 {
                        let _release = ReleaseDcOnDrop { hwnd, hdc };
                        frame::paint_header_filler(hwnd, hdc);
                    }
                    Some(result)
                }
                ControlMessageAction::PrintHeaderFiller => {
                    let result = default();
                    frame::paint_header_filler(hwnd, wparam as Hdc);
                    Some(result)
                }
                ControlMessageAction::InvalidateAfterDefault => {
                    let result = default();
                    invalidate_rect(hwnd, None, false);
                    Some(result)
                }
                ControlMessageAction::RepaintAfterDefault => {
                    let result = default();
                    controls::repaint_now(hwnd, state());
                    Some(result)
                }
                ControlMessageAction::Destroy => {
                    remove_window_subclass(hwnd, control_proc, id);
                    remove_prop(hwnd, CONTROL_PROP);
                    // SAFETY: the box was leaked by `attach` for this window and is freed once here; nothing reads the state afterwards.
                    drop(unsafe { Box::from_raw(ref_data as *mut PaintState) });
                    None
                }
            }
        }),
    );
    match answer {
        Some(result) => result,
        // SAFETY: called from a subclass procedure with its own message parameters.
        None => unsafe { def_subclass_proc(hwnd, message, wparam, lparam) },
    }
}

/// C# `[UnmanagedCallersOnly] DialogProc` (lines 298-321) as a `SubclassProc`. Inside `pending_panic::guarded` (the same rule as `control_proc`):
/// `WM_NOTIFY` with `lparam != 0`, the `NMHDR` at `lparam` having `code == NM_CUSTOMDRAW (-12)` and `is_list_view(hwndFrom)` -> `r = def_subclass_proc(..)`, then
/// `controls::adjust_list_view_custom_draw(&mut *(lparam as *mut NMLVCUSTOMDRAW), r)`; `WM_NCDESTROY` -> `remove_window_subclass(hwnd, dialog_proc, id)`,
/// `remove_prop(hwnd, DIALOG_PROP)`, then default; everything else default.
///
/// # Safety
/// Only the OS calls it, as the subclass procedure installed by [`NativeDialogChromePainter::attach_dialog`]. For `WM_NOTIFY` the `lparam` is a pointer to a
/// notification structure valid for the duration of the call (a list view sends an `NMLVCUSTOMDRAW`).
pub(crate) unsafe extern "system" fn dialog_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    id: usize,
    _ref_data: usize,
) -> isize {
    let answer = pending_panic::guarded(
        None,
        AssertUnwindSafe(|| {
            if message == WM_NOTIFY && lparam != 0 {
                // SAFETY: for `WM_NOTIFY` the `lparam` points to a notification structure (it starts with an `NMHDR`) valid for the duration of the message.
                let header = unsafe { &*(lparam as *const NMHDR) };
                if header.code as i32 == NM_CUSTOMDRAW && is_list_view(header.hwndFrom.0 as isize) {
                    // SAFETY: called from a subclass procedure with its own message parameters.
                    let result = unsafe { def_subclass_proc(hwnd, message, wparam, lparam) };
                    // SAFETY: a list view sends `NM_CUSTOMDRAW` with an `NMLVCUSTOMDRAW` (`is_list_view(hwndFrom)` was checked) valid and writable for the message.
                    let custom_draw = unsafe { &mut *(lparam as *mut NMLVCUSTOMDRAW) };
                    return Some(controls::adjust_list_view_custom_draw(custom_draw, result));
                }
            }
            if message == WM_NCDESTROY {
                remove_window_subclass(hwnd, dialog_proc, id);
                remove_prop(hwnd, DIALOG_PROP);
            }
            None
        }),
    );
    match answer {
        Some(result) => result,
        // SAFETY: called from a subclass procedure with its own message parameters.
        None => unsafe { def_subclass_proc(hwnd, message, wparam, lparam) },
    }
}

/// C# `IsListView(hwnd)`: `hwnd != 0` and the class name is `SysListView32` ignoring ASCII case (the C# compares a 13 unit buffer, so a longer name never
/// matches: `eq_ignore_ascii_case` on the whole name has the same result).
pub fn is_list_view(hwnd: Hwnd) -> bool {
    hwnd != 0 && get_class_name(hwnd).is_some_and(|name| name.eq_ignore_ascii_case(WC_LISTVIEW))
}

/// Releases a window DC on every exit of a drawing arm, also when the drawing panics.
struct ReleaseDcOnDrop {
    hwnd: Hwnd,
    hdc: Hdc,
}

impl Drop for ReleaseDcOnDrop {
    fn drop(&mut self) {
        release_dc(self.hwnd, self.hdc);
    }
}
