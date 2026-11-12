//! D-10 mutation killers for `dialogs/settings/dialog/frame.rs`, part two: the `DialogContent` forwarders of `SettingsContent`, the `Debug` impl and the thin
//! `SettingsDialog` wrappers (page creation, paging, key boxes, close). Real Settings window on a desktop, driven synchronously from the `ready` callback; every
//! test ends the modal loop itself, so nothing can block.
#![cfg(windows)]
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Mutex;

use pr_app::presenters::settings::SettingsPresenter;
use pr_platform_win::dialogs::native::dialog::core::DialogContent;
use pr_platform_win::dialogs::settings::dialog::frame::{NAV_ID, SCROLL_ID};
use pr_platform_win::dialogs::settings::dialog::keybox::key_box_text;
use pr_platform_win::dialogs::settings::dialog::page_layout::clamp_scroll;
use pr_platform_win::dialogs::settings::dialog::{SettingsDialog, SettingsDialogArgs};
use pr_platform_win::win32::dialog_native::user32;
use pr_platform_win::win32::structs::Msg;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

// The remembered page is process-wide state.
static SERIAL: Mutex<()> = Mutex::new(());
const DISPLAY: usize = 1;
const SHORTCUTS: i32 = 4;
const FILES: usize = 5;
const FIRST_KEY_BOX: i32 = 5006;
const SB_TOP: i32 = 6;
const SB_BOTTOM: i32 = 7;

/// Runs `body` in the real modal loop, then closes the dialog; the remembered page is restored to "General" on every path.
fn run(make: impl FnOnce() -> SettingsDialogArgs + Send + 'static, body: impl Fn(&SettingsDialog) + Send + 'static) {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            SettingsPresenter::remember_page("General");
        }
    }
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _restore = Restore;
    SettingsPresenter::remember_page("General");
    desktop::on_sta(move || {
        let _styles = desktop::activate_visual_styles();
        let dialog = SettingsDialog::new(make()).unwrap();
        let ready = |d: &SettingsDialog| {
            body(d);
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
    });
}

fn key_msg(message: u32, wparam: usize) -> Msg {
    Msg {
        hwnd: HWND(std::ptr::null_mut()),
        message,
        wParam: WPARAM(wparam),
        lParam: LPARAM(0),
        ..Default::default()
    }
}

// Kills 727:9 `on_ok -> true` and `-> false`: the trait entry answers what try_save_all answers: true for valid settings, false (and a refused Save) when a mandatory
// shortcut box was emptied.
#[test]
#[ignore = "native: needs desktop"]
fn on_ok_forwards_the_save_verdict() {
    run(fixture::args, |d| {
        let content = d.content().as_ref();
        assert!(DialogContent::on_ok(content, d.native()), "valid settings are saved");
        assert!(d.saved());
        d.activate_page(SHORTCUTS, false, true);
        d.set_control_text(FIRST_KEY_BOX, "");
        assert!(
            !DialogContent::on_ok(content, d.native()),
            "an emptied mandatory shortcut refuses Save"
        );
    });
}

// Kills 737:9 `on_scroll -> ()`: the scroll bar id moves the showing page to the bottom and back to the top.
#[test]
#[ignore = "native: needs desktop"]
fn on_scroll_on_the_bar_moves_the_showing_page() {
    run(fixture::args, |d| {
        d.activate_page(DISPLAY as i32, false, true);
        assert!(
            d.content_height(DISPLAY) > d.viewport_height(),
            "Display page can scroll"
        );
        assert_eq!(d.scroll_offset(DISPLAY), 0);
        let content = d.content().as_ref();
        DialogContent::on_scroll(content, d.native(), SCROLL_ID, SB_BOTTOM);
        let bottom = clamp_scroll(i32::MAX, d.content_height(DISPLAY), d.viewport_height());
        assert!(bottom > 0);
        assert_eq!(d.scroll_offset(DISPLAY), bottom);
        DialogContent::on_scroll(content, d.native(), SCROLL_ID, SB_TOP);
        assert_eq!(d.scroll_offset(DISPLAY), 0);
    });
}

// Kills 753:9 `on_dialog_key -> true` and `-> false`: Ctrl+Tab is consumed and cycles the page; a plain Tab is left to the dialog manager.
#[test]
#[ignore = "native: needs desktop"]
fn on_dialog_key_consumes_ctrl_tab_and_leaves_plain_tab() {
    run(
        || {
            let mut args = fixture::args();
            args.options.control_key_down = Some(Rc::new(|| true));
            args
        },
        |d| {
            d.activate_page(0, false, true);
            let tab = key_msg(0x0100, 9);
            // The Control check is the option (true here), Shift is the real keyboard (either direction leaves page 0).
            let content = d.content().as_ref();
            assert!(DialogContent::on_dialog_key(content, d.native(), &tab));
            assert_ne!(d.active_page_index(), 0, "Ctrl+Tab cycled the page");
            // A message that is no key message is never claimed.
            assert!(!DialogContent::on_dialog_key(content, d.native(), &key_msg(0x0200, 9)));
        },
    );
    run(
        || {
            let mut args = fixture::args();
            args.options.control_key_down = Some(Rc::new(|| false));
            args
        },
        |d| {
            d.activate_page(0, false, true);
            assert!(
                !DialogContent::on_dialog_key(d.content().as_ref(), d.native(), &key_msg(0x0100, 9)),
                "plain Tab stays with the dialog manager"
            );
            assert_eq!(d.active_page_index(), 0);
        },
    );
}

// Kills 778:9 `Debug::fmt -> Ok(Default::default())`: the output names the type.
#[test]
fn debug_names_the_dialog_type() {
    let dialog = fixture::dialog();
    assert_eq!(format!("{dialog:?}"), "SettingsDialog { .. }");
}

// Kills 906:9 `is_page_created -> true`/`-> false` and 916:9 `ensure_page_created -> ()`: pages are created lazily; the showing page exists, the others do not until
// ensured, and ensuring creates the real controls.
#[test]
#[ignore = "native: needs desktop"]
fn pages_are_created_lazily_and_ensure_creates_them() {
    run(fixture::args, |d| {
        assert!(d.is_page_created(0), "the showing page exists");
        assert!(!d.is_page_created(FILES), "a page nobody opened does not");
        assert!(!d.is_page_created(99), "out of range is not created");
        d.ensure_page_created(FILES);
        assert!(d.is_page_created(FILES), "ensure creates it");
        let first = d.page_items(FILES)[0].id;
        assert_ne!(d.native().item(first), 0, "with real controls");
        assert!(!d.is_page_created(3), "other pages stay uncreated");
    });
}

// Kills 911:9 `focus_external_editor -> true`/`-> false`: from another showing page the Files page is shown and focused (true); the answer is not a constant: after the
// Files page was shown the editor box still holds the focus when asked again.
#[test]
#[ignore = "native: needs desktop"]
fn focus_external_editor_answers_true_and_focuses() {
    run(fixture::args, |d| {
        assert_eq!(d.active_page_key(), "General");
        assert!(d.focus_external_editor());
        assert_eq!(d.active_page_key(), "Files");
        let focused = user32::get_focus();
        assert_ne!(focused, 0);
        assert_eq!(user32::get_dlg_ctrl_id(focused) / 1000, FILES as i32 + 1);
        assert!(d.is_page_created(FILES));
    });
}

// Kills 911:9 `-> true`: with no Files page the wrapper answers false.
#[test]
#[ignore = "native: needs desktop"]
fn focus_external_editor_is_false_without_a_files_page() {
    use pr_platform_win::dialogs::settings::page_view::SettingsPageView;
    run(
        || {
            let events = Rc::new(std::cell::RefCell::new(Vec::new()));
            let mut args = fixture::args();
            args.options.pages = Some(
                [
                    "General",
                    "Display",
                    "Mouse",
                    "Performance",
                    "Shortcuts",
                    "NotFiles",
                    "Diagnostics",
                ]
                .iter()
                .map(|&key| {
                    Box::new(fixture::ProbePage {
                        key,
                        events: events.clone(),
                    }) as Box<dyn SettingsPageView>
                })
                .collect(),
            );
            args
        },
        |d| {
            assert!(!d.focus_external_editor());
        },
    );
}

// Kills 931:9 `cycle_page -> ()`: forward moves to the next page, backward from the first wraps to the last, and the nav list follows.
#[test]
#[ignore = "native: needs desktop"]
fn cycle_page_moves_forward_and_wraps_backward() {
    run(fixture::args, |d| {
        d.activate_page(0, false, true);
        d.cycle_page(true);
        assert_eq!(d.active_page_index(), 1);
        d.cycle_page(false);
        assert_eq!(d.active_page_index(), 0);
        d.cycle_page(false);
        assert_eq!(d.active_page_index(), d.page_count() as i32 - 1);
        assert_eq!(
            pr_platform_win::dialogs::native::controls::basic::ListBoxControl::selected_index(d.native().item(NAV_ID)),
            d.page_count() as i32 - 1
        );
    });
}

// Kills 941:9 `is_key_box -> false`, 946:9 `is_key_box_id -> true`/`-> false`, 951:9 `capture_key -> true`/`-> false`.
#[test]
#[ignore = "native: needs desktop"]
fn key_box_queries_and_capture() {
    run(fixture::args, |d| {
        d.activate_page(SHORTCUTS, false, true);
        let boxed = d.native().item(FIRST_KEY_BOX);
        assert_ne!(boxed, 0);
        // is_key_box_id: the declared key box ids only.
        assert!(d.is_key_box_id(FIRST_KEY_BOX));
        assert!(!d.is_key_box_id(NAV_ID));
        assert!(!d.is_key_box_id(0));
        // is_key_box: the real key box window yes; 0 and a non-key-box control no.
        assert!(d.is_key_box(boxed));
        assert!(!d.is_key_box(0));
        assert!(!d.is_key_box(d.native().item(NAV_ID)));
        // capture_key: 'B' is swallowed and written into the box; Tab and a non-key-box window are not.
        let before = d.control_text(FIRST_KEY_BOX);
        assert!(d.capture_key(boxed, 0x42, 0));
        let text = key_box_text(0x42, false).unwrap();
        assert_eq!(d.control_text(FIRST_KEY_BOX), text);
        assert_ne!(text, before);
        assert!(!d.capture_key(boxed, 0x09, 0), "Tab is not swallowed");
        assert_eq!(d.control_text(FIRST_KEY_BOX), text);
        assert!(!d.capture_key(d.native().item(NAV_ID), 0x42, 0), "not a key box");
        assert_eq!(d.control_text(FIRST_KEY_BOX), text);
    });
}

// Kills 818:9 `close -> ()` (a TIMEOUT before: the modal loop never ends). Detected without waiting: after close() the dialog reports closed at once; the test then ends
// the loop itself so even the mutant cannot hang it.
#[test]
#[ignore = "native: needs desktop"]
fn close_marks_the_dialog_closed_immediately() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let dialog = SettingsDialog::new(fixture::args()).unwrap();
        let observed = Cell::new(None);
        let ready = |d: &SettingsDialog| {
            let before = d.is_closed();
            d.close();
            observed.set(Some((before, d.is_closed())));
            d.native().end_modal(2); // guarantees the exit even when close() did nothing
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
        assert_eq!(
            observed.get(),
            Some((false, true)),
            "(closed before, closed after close())"
        );
    });
}
