//! Unit W8-18l, native part (contract `contracts/W8-18l.md`): `attach`, `is_attached`, `attach_dialog`, the cleanup at `WM_NCDESTROY` and the dialog subclass.
//! Every test is `#[ignore = "native: needs desktop"]`, runs on its own STA thread through the fixture and creates only hidden windows: no message loop is pumped
//! and nothing is painted (the drawing of 18m and 18n is not reached, trap T6).
//!
//! `live_state_count` and `GetGuiResources` are process-wide, so every test of this file takes `SERIAL` for its whole body.

mod common {
    pub mod dialogs_painter_fixture;
}

use std::sync::Mutex;
use std::sync::mpsc;

use common::dialogs_painter_fixture::{OwnerWindow, child, on_sta};
use pr_platform_win::dialogs::native::painter::geometry::{ChromePaintKind as K, kind_for_style};
use pr_platform_win::dialogs::native::painter::{
    CONTROL_PROP, DIALOG_PROP, NativeDialogChromePainter as Painter, is_list_view, live_state_count, production_painter,
};
use pr_platform_win::dialogs::native::template::DialogControlClass as C;
use pr_platform_win::win32::api::user32::{destroy_window, get_window_long_ptr};
use pr_platform_win::win32::dialog_chrome::get_prop;
use pr_platform_win::win32::dialog_native::user32::{get_gui_resources, send_message};
use pr_platform_win::win32::structs::Hwnd;
use windows::Win32::UI::Controls::NMHDR;

/// Serialises the tests of this file (the state counter and the handle counts are per process).
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

const WM_NOTIFY: u32 = 0x004E;
const NM_CLICK: i32 = -2;
const NM_CUSTOMDRAW: i32 = -12;
const LVS_REPORT: u32 = 1;
const GR_USEROBJECTS: u32 = 1;

/// One representative child per attachable kind: (class, style, expected kind).
fn gallery() -> Vec<(C, u32, K)> {
    vec![
        (C::Button, 0, K::PushButton),
        (C::Button, 3, K::CheckBox),
        (C::Button, 9, K::RadioButton),
        (C::Button, 7, K::GroupBox),
        (C::ComboBox, 3, K::ComboBox),
        (C::Progress, 0, K::Progress),
        (C::Trackbar, 0, K::Trackbar),
        (C::Static, 0x10, K::EtchedStatic),
        (C::Edit, 0, K::EditBorder),
        (C::ListBox, 0, K::ListBoxBorder),
        (C::ListView, LVS_REPORT, K::ListViewBorder),
    ]
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_marks_the_control_and_is_idempotent() {
    on_sta(|| {
        let _serial = serial();
        let before = live_state_count();
        let owner = OwnerWindow::hidden();
        let button = child(&owner, C::Button, 0, "OK", 80, 24);
        assert!(!Painter::is_attached(button));
        assert_eq!(get_prop(button, CONTROL_PROP), 0);

        assert!(Painter::attach(button, K::PushButton));
        assert!(Painter::is_attached(button));
        assert_ne!(get_prop(button, CONTROL_PROP), 0);
        assert_eq!(live_state_count(), before + 1);

        // Idempotent: the second attach answers true and allocates nothing.
        let marker = get_prop(button, CONTROL_PROP);
        assert!(Painter::attach(button, K::PushButton));
        assert_eq!(get_prop(button, CONTROL_PROP), marker);
        assert_eq!(live_state_count(), before + 1);

        // The trait entry points choose the kind from the class and the current style.
        let painter = production_painter();
        let check = child(&owner, C::Button, 3, "Check", 80, 24);
        assert!(painter.attach_control(check, C::Button));
        assert!(Painter::is_attached(check));
        let label = child(&owner, C::Static, 0, "label", 80, 24);
        assert!(
            !painter.attach_control(label, C::Static),
            "a plain label has no hand painter"
        );
        assert!(!Painter::is_attached(label));

        drop(owner);
        assert_eq!(live_state_count(), before, "WM_NCDESTROY frees every state");
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_none_kind_dead_window_and_zero_are_refused() {
    on_sta(|| {
        let _serial = serial();
        let before = live_state_count();
        let owner = OwnerWindow::hidden();
        let button = child(&owner, C::Button, 0, "OK", 80, 24);
        assert!(!Painter::attach(0, K::PushButton));
        assert!(!Painter::attach(button, K::None));
        assert!(!Painter::is_attached(button));
        assert!(!Painter::is_attached(0));
        destroy_window(button).expect("destroy the button");
        assert!(!Painter::attach(button, K::PushButton), "a dead window is refused");
        assert!(!Painter::is_attached(button));
        assert!(!Painter::attach_dialog(0));
        assert!(!Painter::attach_dialog(button));
        assert_eq!(live_state_count(), before);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_survives_destroy_without_leaking() {
    on_sta(|| {
        let _serial = serial();
        let process = pr_platform_win::win32::dialog_native::kernel32::get_current_process();
        let cycle = || {
            let owner = OwnerWindow::hidden();
            let mut attached = 0;
            for (class, style, kind) in gallery() {
                let hwnd = child(&owner, class, style, "x", 60, 24);
                assert!(Painter::attach(hwnd, kind), "{class:?}");
                attached += 1;
            }
            assert_eq!(attached, 11);
        };
        // Warm-up: the first cycle may create lazily allocated system objects (class registration of the common controls).
        cycle();
        let states = live_state_count();
        let user_objects = get_gui_resources(process, GR_USEROBJECTS);
        for _ in 0..50 {
            cycle();
        }
        assert_eq!(
            live_state_count(),
            states,
            "every attached state is freed at WM_NCDESTROY"
        );
        assert_eq!(
            get_gui_resources(process, GR_USEROBJECTS),
            user_objects,
            "no USER object leaks"
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn failed_attach_to_a_window_of_another_thread_frees_the_state() {
    let _serial = serial();
    let (hwnd_tx, hwnd_rx) = mpsc::channel::<Hwnd>();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let foreign = std::thread::spawn(move || {
        let owner = OwnerWindow::hidden();
        hwnd_tx.send(owner.hwnd).expect("send the handle");
        let _ = done_rx.recv();
        drop(owner);
    });
    let foreign_hwnd = hwnd_rx.recv().expect("foreign window");
    let before = live_state_count();
    // SetWindowSubclass only works for windows of the calling thread.
    assert!(!Painter::attach(foreign_hwnd, K::PushButton));
    assert!(!Painter::is_attached(foreign_hwnd));
    assert!(!Painter::attach_dialog(foreign_hwnd));
    assert_eq!(live_state_count(), before, "the refused state is freed at once");
    done_tx.send(()).expect("release the foreign thread");
    foreign.join().expect("join the foreign thread");
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_dialog_is_idempotent_and_marks_the_dialog() {
    on_sta(|| {
        let _serial = serial();
        let owner = OwnerWindow::hidden();
        assert_eq!(get_prop(owner.hwnd, DIALOG_PROP), 0);
        assert!(Painter::attach_dialog(owner.hwnd));
        assert_eq!(get_prop(owner.hwnd, DIALOG_PROP), 1);
        assert!(
            Painter::attach_dialog(owner.hwnd),
            "second attach is a no-op that answers true"
        );
        assert_eq!(get_prop(owner.hwnd, DIALOG_PROP), 1);
        assert!(production_painter().attach_dialog(owner.hwnd));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn dialog_subclass_ignores_other_notifications() {
    on_sta(|| {
        let _serial = serial();
        let subclassed = OwnerWindow::hidden();
        let plain = OwnerWindow::hidden();
        assert!(Painter::attach_dialog(subclassed.hwnd));
        let list_view = child(&subclassed, C::ListView, LVS_REPORT, "", 100, 60);
        let button = child(&subclassed, C::Button, 0, "b", 60, 24);
        let plain_list_view = child(&plain, C::ListView, LVS_REPORT, "", 100, 60);
        let plain_button = child(&plain, C::Button, 0, "b", 60, 24);
        assert!(is_list_view(list_view));
        assert!(!is_list_view(button));
        assert!(!is_list_view(0));

        let send = |owner: Hwnd, from: Hwnd, code: i32| -> isize {
            let header = NMHDR {
                hwndFrom: windows::Win32::Foundation::HWND(from as *mut core::ffi::c_void),
                idFrom: 1,
                code: code as u32,
            };
            // SAFETY: `header` is a live local NMHDR for the whole synchronous call; the code is not NM_CUSTOMDRAW from a list view, so the dialog
            // subclass reads nothing beyond the NMHDR.
            unsafe { send_message(owner, WM_NOTIFY, 1, std::ptr::from_ref(&header) as isize) }
        };
        // A click from a real list view, and a custom draw from a button: the answers are those of the plain owner (the default procedure).
        assert_eq!(
            send(subclassed.hwnd, list_view, NM_CLICK),
            send(plain.hwnd, plain_list_view, NM_CLICK)
        );
        assert_eq!(
            send(subclassed.hwnd, button, NM_CUSTOMDRAW),
            send(plain.hwnd, plain_button, NM_CUSTOMDRAW)
        );
        // A WM_NOTIFY with a null lParam is passed on, not read.
        // SAFETY: a null lParam, which the default procedure of the owner ignores.
        let null_answer = unsafe { send_message(subclassed.hwnd, WM_NOTIFY, 1, 0) };
        // SAFETY: as above.
        assert_eq!(null_answer, unsafe { send_message(plain.hwnd, WM_NOTIFY, 1, 0) });
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn kind_for_reads_the_style_of_real_windows() {
    // The native twin of the C# theory KindFor_FollowsTheClassAndTheStyle: the kind comes from the style the window REALLY has.
    on_sta(|| {
        let _serial = serial();
        let owner = OwnerWindow::hidden();
        for (class, style, expected) in gallery() {
            let hwnd = child(&owner, class, style, "x", 60, 24);
            assert_eq!(Painter::kind_for(hwnd, class), expected, "{class:?} {style:#x}");
            let read = get_window_long_ptr(hwnd, -16) as u32;
            assert_eq!(
                kind_for_style(class, read),
                expected,
                "{class:?} {style:#x} (style read {read:#x})"
            );
        }
        // Styles that remove the hand painter.
        let icon = child(&owner, C::Button, 0x40, "", 24, 24);
        assert_eq!(Painter::kind_for(icon, C::Button), K::None);
        let marquee = child(&owner, C::Progress, 8, "", 60, 12);
        assert_eq!(Painter::kind_for(marquee, C::Progress), K::None);
        let vertical = child(&owner, C::Trackbar, 2, "", 30, 60);
        assert_eq!(Painter::kind_for(vertical, C::Trackbar), K::None);
        let label = child(&owner, C::Static, 0, "label", 60, 24);
        assert_eq!(Painter::kind_for(label, C::Static), K::None);
        let scroll = child(&owner, C::ScrollBar, 0, "", 16, 60);
        assert_eq!(Painter::kind_for(scroll, C::ScrollBar), K::None);
    });
}
