//! W8-18k regression tests for the native dialog's colour, close and cleanup paths.

#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use fixture::{RecordingContent, create_owner, destroy_owner, on_sta, show_on_owner};
use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::dialog::core::{ID_CANCEL, NativeDialog};
use pr_platform_win::dialogs::native::dialog::draw::{CtlColor, IDC_WAIT, ctl_color_kind};
use pr_platform_win::dialogs::native::template::{DialogControlClass, DialogItem};
use pr_platform_win::win32::{control_constants, dialog_native, messages};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetPixel, HDC, HGDIOBJ, SelectObject,
};
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;

fn colour_content() -> Rc<RecordingContent> {
    let content = Rc::new(RecordingContent::default());
    *content.items.borrow_mut() = vec![
        DialogItem::new(DialogControlClass::Static, 100, "label", 0x5000_0000),
        DialogItem::new(DialogControlClass::Edit, 101, "read only", 0x5000_0800),
        DialogItem::new(DialogControlClass::ListBox, 102, "", 0x5000_0001),
        DialogItem::new(DialogControlClass::Button, 103, "OK", 0x5000_0000),
        DialogItem::new(DialogControlClass::ListView, 104, "", 0x5000_0001),
    ];
    content
}

struct MemoryDc {
    dc: HDC,
    bitmap: HGDIOBJ,
    old: HGDIOBJ,
}
impl MemoryDc {
    fn new() -> Self {
        let screen = dialog_native::user32::get_dc(0);
        // SAFETY: the screen DC is live for these compatible GDI creations.
        let dc = unsafe { CreateCompatibleDC(Some(HDC(screen as *mut _))) };
        // SAFETY: the screen DC is live and 320x32 is a valid bitmap size.
        let bitmap = unsafe { CreateCompatibleBitmap(HDC(screen as *mut _), 320, 32) };
        // SAFETY: bitmap and memory DC are live, compatible GDI objects.
        let old = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
        dialog_native::user32::release_dc(0, screen);
        Self {
            dc,
            bitmap: HGDIOBJ(bitmap.0),
            old,
        }
    }
    fn handle(&self) -> usize {
        self.dc.0 as usize
    }
}
impl Drop for MemoryDc {
    fn drop(&mut self) {
        // SAFETY: restore the original selected object before deleting this DC and bitmap.
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
        }
    }
}

fn colour_message(dialog: &NativeDialog, message: u32, dc: &MemoryDc, control: isize) -> isize {
    // SAFETY: the memory DC and child handle are live during synchronous WM_CTLCOLOR* dispatch.
    unsafe { dialog_native::user32::send_message(dialog.hwnd(), message, dc.handle(), control) }
}

fn brush_colour(handle: isize) -> u32 {
    assert_ne!(handle, 0);
    let mut bytes = [0u8; 16];
    assert!(dialog_native::gdi32::get_object(handle, &mut bytes) >= 8);
    u32::from_ne_bytes(bytes[4..8].try_into().unwrap())
}

// C#: NativeDialog.IdcWait
#[test]
fn idc_wait_matches_the_csharp_golden() {
    let golden = pr_test_support::golden::load("dialog-layout");
    let constants = golden
        .cases
        .iter()
        .find(|case| case["kind"] == "constants")
        .expect("constants golden row");
    assert_eq!(
        pr_test_support::golden::i64_of(constants, "idc_wait"),
        i64::from(IDC_WAIT)
    );
}

// C#: NativeDialog.HandleMessage WM_CTLCOLOR*
#[test]
fn ctl_color_kind_table() {
    assert_eq!(
        ctl_color_kind(dialog_native::WM_CTLCOLORDLG, false),
        Some(CtlColor::DialogBackground)
    );
    for message in [
        dialog_native::WM_CTLCOLORBTN,
        dialog_native::WM_CTLCOLORMSGBOX,
        dialog_native::WM_CTLCOLORSCROLLBAR,
    ] {
        assert_eq!(ctl_color_kind(message, false), Some(CtlColor::Button));
        assert_eq!(ctl_color_kind(message, true), Some(CtlColor::Button));
    }
    assert_eq!(
        ctl_color_kind(dialog_native::WM_CTLCOLORSTATIC, false),
        Some(CtlColor::Static)
    );
    assert_eq!(
        ctl_color_kind(dialog_native::WM_CTLCOLORSTATIC, true),
        Some(CtlColor::ReadOnlyEdit)
    );
    for message in [dialog_native::WM_CTLCOLOREDIT, dialog_native::WM_CTLCOLORLISTBOX] {
        assert_eq!(ctl_color_kind(message, false), Some(CtlColor::Input));
        assert_eq!(ctl_color_kind(message, true), Some(CtlColor::Input));
    }
    assert_eq!(ctl_color_kind(messages::WM_APP, false), None);
}

// C#: NativeDialog.Cleanup: resources start empty before the first show.
#[test]
fn draw_state_starts_empty() {
    let dialog = NativeDialog::new(Rc::new(RecordingContent::default()));
    assert_eq!(dialog.hwnd(), 0);
    assert_eq!(dialog.body_font(), 0);
    assert_eq!(dialog.self_ref_data(), 0);
}

// C#: NativeDialog.HandleMessage WM_CLOSE
#[test]
#[ignore = "native: needs desktop"]
fn wm_close_closes_as_cancel() {
    on_sta(|| {
        let content = Rc::new(RecordingContent::default());
        let dialog = NativeDialog::new(content.clone());
        let result = show_on_owner(&dialog, |d, _| {
            // SAFETY: WM_CLOSE has no pointer arguments and the dialog is live.
            unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_CLOSE, 0, 0) };
            assert!(d.is_closed());
        })
        .unwrap();
        assert_eq!(result, ID_CANCEL);
        assert_eq!(dialog.hwnd(), 0);
        assert_eq!(
            content
                .events
                .borrow()
                .iter()
                .filter(|event| *event == "closed")
                .count(),
            1
        );
    });
}

// C#: NativeDialog.HandleMessage WM_CLOSE and NativeDialog.Close
#[test]
#[ignore = "native: needs desktop"]
fn wm_close_is_ignored_when_the_user_may_not_close_but_close_from_outside_works() {
    on_sta(|| {
        let content = Rc::new(RecordingContent::default());
        content.can_close.set(false);
        let dialog = NativeDialog::new(content);
        let result = show_on_owner(&dialog, |d, _| {
            // SAFETY: WM_CLOSE has no pointer arguments and the dialog is live.
            unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_CLOSE, 0, 0) };
            assert!(!d.is_closed());
            d.close();
        })
        .unwrap();
        assert_eq!(result, ID_CANCEL);
    });
}

// C#: NativeDialog.Cleanup: repeated close invokes OnClosed once.
#[test]
#[ignore = "native: needs desktop"]
fn cleanup_is_idempotent() {
    on_sta(|| {
        let content = Rc::new(RecordingContent::default());
        let dialog = NativeDialog::new(content.clone());
        show_on_owner(&dialog, |d, _| {
            d.end_modal(ID_CANCEL);
            d.close();
        })
        .unwrap();
        assert_eq!(
            content
                .events
                .borrow()
                .iter()
                .filter(|event| *event == "closed")
                .count(),
            1
        );
    });
}

// C#: NativeDialog.Cleanup: the modeless callback follows destruction.
#[test]
#[ignore = "native: needs desktop"]
fn the_closed_callback_of_a_modeless_dialog_runs_once_after_the_window_is_gone() {
    on_sta(|| {
        let dialog = NativeDialog::new(Rc::new(RecordingContent::default()));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let count = Rc::new(Cell::new(0));
        let callback_dialog = dialog.clone();
        let callback_seen = seen.clone();
        let callback_count = count.clone();
        dialog
            .show_modeless(
                0,
                None,
                None,
                Some(Box::new(move || {
                    callback_count.set(callback_count.get() + 1);
                    callback_seen.borrow_mut().push(callback_dialog.hwnd());
                })),
            )
            .unwrap();
        dialog.close();
        dialog.close();
        assert_eq!(count.get(), 1);
        assert_eq!(*seen.borrow(), [0]);
    });
}

// C#: NativeDialog.Cleanup: a captured OnClosed panic is rethrown by ShowModal.
#[test]
#[ignore = "native: needs desktop"]
fn a_panic_in_on_closed_is_resumed_after_show_returns() {
    on_sta(|| {
        let content = Rc::new(RecordingContent::default());
        content.panic_in.set(Some("closed"));
        let dialog = NativeDialog::new(content);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            show_on_owner(&dialog, |d, _| d.close())
        }));
        assert!(result.is_err());
        assert_eq!(dialog.hwnd(), 0);
    });
}

// C#: NativeDialog.HandleMessage WM_SETCURSOR
#[test]
#[ignore = "native: needs desktop"]
fn wait_cursor_shows_the_wait_cursor_everywhere() {
    on_sta(|| {
        let content = Rc::new(RecordingContent::default());
        content.wait_cursor.set(true);
        let dialog = NativeDialog::new(content);
        show_on_owner(&dialog, |d, _| {
            // SAFETY: WM_SETCURSOR carries only window and hit-test integer values.
            let handled =
                unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_SETCURSOR, d.hwnd() as usize, 0) };
            assert_eq!(handled, 1);
            let expected = pr_platform_win::win32::api::user32::load_system_cursor(IDC_WAIT).unwrap();
            // SAFETY: GetCursor reads the calling UI thread's current cursor handle.
            let actual = unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursor() };
            assert_eq!(actual.0 as isize, expected);
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeDialog.HandleMessage: unmatched messages reach OnWindowMessage.
#[test]
#[ignore = "native: needs desktop"]
fn an_unrelated_message_is_not_a_colour_message() {
    on_sta(|| {
        let content = Rc::new(RecordingContent::default());
        content.window_message_result.set(Some(71));
        let dialog = NativeDialog::new(content.clone());
        show_on_owner(&dialog, |d, _| {
            // SAFETY: WM_APP + 19 is a test-only message with no pointer arguments.
            let result = unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_APP + 19, 0, 0) };
            assert_eq!(result, 71);
            d.close();
        })
        .unwrap();
        assert!(content.events.borrow().iter().any(|event| event == "message:8013"));
    });
}

// C#: NativeDialog.Cleanup: owner re-enabled after the modal show.
#[test]
#[ignore = "native: needs desktop"]
fn the_owner_is_enabled_before_the_dialog_is_destroyed_and_after_show() {
    on_sta(|| {
        let owner = create_owner();
        let dialog = NativeDialog::new(Rc::new(RecordingContent::default()));
        let result = dialog
            .show_modal(
                owner,
                Some(&|d| {
                    assert!(!dialog_native::user32::is_window_enabled(owner));
                    d.close();
                }),
                None,
            )
            .unwrap();
        assert_eq!(result, ID_CANCEL);
        assert!(dialog_native::user32::is_window_enabled(owner));
        assert_eq!(dialog.hwnd(), 0);
        destroy_owner(owner);
    });
}

// C#: NativeDialog.EndModal: modeless cleanup is synchronous.
#[test]
#[ignore = "native: needs desktop"]
fn a_modeless_close_cleans_up_at_once() {
    on_sta(|| {
        let dialog = NativeDialog::new(Rc::new(RecordingContent::default()));
        dialog.show_modeless(0, None, None, None).unwrap();
        assert_ne!(dialog.hwnd(), 0);
        dialog.close();
        assert!(dialog.is_closed());
        assert_eq!(dialog.hwnd(), 0);
        assert_eq!(dialog.self_ref_data(), 0);
    });
}

// C#: NativeDialog.Cleanup: the GCHandle equivalent is released.
#[test]
#[ignore = "native: needs desktop"]
fn the_cookie_and_the_content_are_released() {
    on_sta(|| {
        let token = Rc::new(());
        let content = Rc::new(RecordingContent {
            token: token.clone(),
            ..Default::default()
        });
        let dialog = NativeDialog::new(content.clone());
        assert_eq!(Rc::strong_count(&token), 2);
        show_on_owner(&dialog, |d, _| {
            assert_ne!(d.self_ref_data(), 0);
            d.close();
        })
        .unwrap();
        assert_eq!(dialog.self_ref_data(), 0);
        drop(dialog);
        drop(content);
        assert_eq!(Rc::strong_count(&token), 1);

        let modeless_content = Rc::new(RecordingContent {
            token: token.clone(),
            ..Default::default()
        });
        let modeless = NativeDialog::new(modeless_content.clone());
        modeless.show_modeless(0, None, None, None).unwrap();
        assert_ne!(modeless.self_ref_data(), 0);
        modeless.close();
        assert_eq!(modeless.self_ref_data(), 0);
        drop(modeless);
        drop(modeless_content);
        assert_eq!(Rc::strong_count(&token), 1);
    });
}

// C#: NativeDialog.HandleMessage WM_CTLCOLORDLG
#[test]
#[ignore = "native: needs desktop"]
fn wm_ctlcolordlg_returns_the_window_brush() {
    on_sta(|| {
        let dialog = NativeDialog::new(colour_content());
        show_on_owner(&dialog, |d, _| {
            let dc = MemoryDc::new();
            let brush = colour_message(d, dialog_native::WM_CTLCOLORDLG, &dc, 0);
            assert_eq!(brush_colour(brush), DialogColors::window());
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeDialog.HandleMessage WM_CTLCOLORBTN/MSGBOX/SCROLLBAR
#[test]
#[ignore = "native: needs desktop"]
fn button_msgbox_and_scrollbar_colours_use_text_on_window() {
    on_sta(|| {
        let dialog = NativeDialog::new(colour_content());
        show_on_owner(&dialog, |d, _| {
            let dc = MemoryDc::new();
            for message in [
                dialog_native::WM_CTLCOLORBTN,
                dialog_native::WM_CTLCOLORMSGBOX,
                dialog_native::WM_CTLCOLORSCROLLBAR,
            ] {
                assert_eq!(
                    brush_colour(colour_message(d, message, &dc, d.item(103))),
                    DialogColors::window()
                );
                assert_eq!(
                    dialog_native::gdi32::get_text_color(dc.dc.0 as isize),
                    DialogColors::text()
                );
                assert_eq!(
                    dialog_native::gdi32::get_bk_color(dc.dc.0 as isize),
                    DialogColors::window()
                );
            }
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeDialog.HandleMessage WM_CTLCOLORSTATIC
#[test]
#[ignore = "native: needs desktop"]
fn static_colours_use_the_hook_text_colour_on_the_window_background() {
    on_sta(|| {
        let content = colour_content();
        content.static_color.set(Some(0x0012_3456));
        let dialog = NativeDialog::new(content);
        show_on_owner(&dialog, |d, _| {
            let dc = MemoryDc::new();
            assert_eq!(
                brush_colour(colour_message(d, dialog_native::WM_CTLCOLORSTATIC, &dc, d.item(100))),
                DialogColors::window()
            );
            assert_eq!(dialog_native::gdi32::get_text_color(dc.dc.0 as isize), 0x0012_3456);
            assert_eq!(
                dialog_native::gdi32::get_bk_color(dc.dc.0 as isize),
                DialogColors::window()
            );
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeDialog.HandleMessage WM_CTLCOLORSTATIC of read-only EDIT
#[test]
#[ignore = "native: needs desktop"]
fn a_read_only_edit_gets_the_input_background_and_brush() {
    on_sta(|| {
        let content = colour_content();
        content.static_color.set(Some(0x0001_0203));
        let dialog = NativeDialog::new(content);
        show_on_owner(&dialog, |d, _| {
            let dc = MemoryDc::new();
            assert_eq!(
                brush_colour(colour_message(d, dialog_native::WM_CTLCOLORSTATIC, &dc, d.item(101))),
                DialogColors::input()
            );
            assert_eq!(dialog_native::gdi32::get_text_color(dc.dc.0 as isize), 0x0001_0203);
            assert_eq!(
                dialog_native::gdi32::get_bk_color(dc.dc.0 as isize),
                DialogColors::input()
            );
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeDialog.HandleMessage WM_CTLCOLOREDIT/WM_CTLCOLORLISTBOX
#[test]
#[ignore = "native: needs desktop"]
fn edit_and_listbox_colours_use_strong_text_on_input() {
    on_sta(|| {
        let dialog = NativeDialog::new(colour_content());
        show_on_owner(&dialog, |d, _| {
            let dc = MemoryDc::new();
            for (message, id) in [
                (dialog_native::WM_CTLCOLOREDIT, 101),
                (dialog_native::WM_CTLCOLORLISTBOX, 102),
            ] {
                assert_eq!(
                    brush_colour(colour_message(d, message, &dc, d.item(id))),
                    DialogColors::input()
                );
                assert_eq!(
                    dialog_native::gdi32::get_text_color(dc.dc.0 as isize),
                    DialogColors::text_strong()
                );
                assert_eq!(
                    dialog_native::gdi32::get_bk_color(dc.dc.0 as isize),
                    DialogColors::input()
                );
            }
            d.close();
        })
        .unwrap();
    });
}

fn draw_item(dialog: &NativeDialog, dc: &MemoryDc, item_id: u32, state: u32) -> isize {
    let mut draw = DRAWITEMSTRUCT {
        CtlID: 102,
        itemID: item_id,
        hwndItem: windows::Win32::Foundation::HWND(dialog.item(102) as *mut _),
        hDC: dc.dc,
        rcItem: windows::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 320,
            bottom: 32,
        },
        ..Default::default()
    };
    draw.itemState.0 = state;
    // SAFETY: DRAWITEMSTRUCT and its memory bitmap DC live for this synchronous WM_DRAWITEM call.
    unsafe {
        dialog_native::user32::send_message(
            dialog.hwnd(),
            control_constants::WM_DRAWITEM,
            102,
            (&draw as *const DRAWITEMSTRUCT) as isize,
        )
    }
}

// C#: NativeDialog.DrawOwnerItem row hooks
#[test]
#[ignore = "native: needs desktop"]
fn owner_draw_asks_the_hooks_for_text_subtitle_and_badge_colour_per_row() {
    on_sta(|| {
        let content = colour_content();
        *content.list_text.borrow_mut() = Some(Box::new(|_, _| {
            pr_platform_win::dialogs::native::dialog::ListItemText {
                text: "Row".into(),
                badge: Some("Badge".into()),
            }
        }));
        *content.subtitle.borrow_mut() = Some(Box::new(|_, _| Some("Subtitle".into())));
        *content.badge_color.borrow_mut() = Some(Box::new(|_, _| Some(0x0012_3456)));
        let dialog = NativeDialog::new(content.clone());
        show_on_owner(&dialog, |d, _| {
            let dc = MemoryDc::new();
            draw_item(d, &dc, 0, 0);
            assert!(!d.is_closed());
            d.close();
        })
        .unwrap();
        let events = content.events.borrow();
        for event in ["item_text:102:0", "item_subtitle:102:0", "item_badge:102:0"] {
            assert_eq!(
                events.iter().filter(|seen| seen.as_str() == event).count(),
                1,
                "{event}"
            );
        }
    });
}

// C#: NativeDialog.DrawOwnerItem empty area
#[test]
#[ignore = "native: needs desktop"]
fn owner_draw_of_the_empty_area_calls_no_row_hook() {
    on_sta(|| {
        let content = colour_content();
        let dialog = NativeDialog::new(content.clone());
        show_on_owner(&dialog, |d, _| {
            let dc = MemoryDc::new();
            draw_item(d, &dc, u32::MAX, 0);
            assert!(!d.is_closed());
            d.close();
        })
        .unwrap();
        assert!(!content.events.borrow().iter().any(|event| event.starts_with("item_")));
    });
}

// C#: NativeDialog.DrawOwnerItem paints a selected row into the bitmap.
#[test]
#[ignore = "native: needs desktop"]
fn owner_draw_paints_into_the_bitmap() {
    on_sta(|| {
        let content = colour_content();
        *content.list_text.borrow_mut() = Some(Box::new(|_, _| {
            pr_platform_win::dialogs::native::dialog::ListItemText {
                text: "row".into(),
                badge: None,
            }
        }));
        let dialog = NativeDialog::new(content);
        show_on_owner(&dialog, |d, _| {
            pr_platform_win::dialogs::native::controls::basic::ListBoxControl::add(d.item(102), "row");
            let dc = MemoryDc::new();
            draw_item(d, &dc, 0, 1);
            // SAFETY: the memory DC has a selected 320x32 bitmap; (150, 2) lies inside it.
            let selected = unsafe { GetPixel(dc.dc, 150, 2).0 };
            draw_item(d, &dc, 0, 0);
            // SAFETY: the same live bitmap is sampled after the second synchronous draw.
            let plain = unsafe { GetPixel(dc.dc, 150, 2).0 };
            assert_ne!(selected, plain);
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeDialog.HandleMessage WM_SETCURSOR when WaitCursor is false.
#[test]
#[ignore = "native: needs desktop"]
fn no_wait_cursor_leaves_the_message_to_the_default() {
    on_sta(|| {
        let content = colour_content();
        content.wait_cursor.set(false);
        let dialog = NativeDialog::new(content);
        show_on_owner(&dialog, |d, _| {
            // SAFETY: WM_SETCURSOR carries window and hit-test integers, with no pointers.
            let result =
                unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_SETCURSOR, d.hwnd() as usize, 0) };
            assert_eq!(result, 0);
            d.close();
        })
        .unwrap();
    });
}

struct ClosedProbe {
    hwnd_seen: Cell<bool>,
    owner_disabled: Cell<bool>,
    owner: Cell<isize>,
    calls: Cell<usize>,
}
impl pr_platform_win::dialogs::native::dialog::DialogContent for ClosedProbe {
    fn title(&self) -> String {
        "closed probe".into()
    }
    fn items(&self) -> Vec<DialogItem> {
        Vec::new()
    }
    fn preferred_client_size(&self, d: &NativeDialog) -> (i32, i32) {
        (d.dx(100), d.dy(50))
    }
    fn layout_controls(&self, _: &NativeDialog, _: i32, _: i32) {}
    fn on_closed(&self, d: &NativeDialog) {
        self.calls.set(self.calls.get() + 1);
        self.hwnd_seen.set(dialog_native::user32::is_window(d.hwnd()));
        self.owner_disabled
            .set(!dialog_native::user32::is_window_enabled(self.owner.get()));
    }
}

// C#: NativeDialog.Cleanup invokes OnClosed before re-enabling owner and destroying window.
#[test]
#[ignore = "native: needs desktop"]
fn on_closed_runs_once_before_the_window_is_destroyed() {
    on_sta(|| {
        let probe = Rc::new(ClosedProbe {
            hwnd_seen: Cell::new(false),
            owner_disabled: Cell::new(false),
            owner: Cell::new(0),
            calls: Cell::new(0),
        });
        let dialog = NativeDialog::new(probe.clone());
        show_on_owner(&dialog, |d, owner| {
            probe.owner.set(owner);
            d.close();
        })
        .unwrap();
        assert!(probe.hwnd_seen.get());
        assert!(probe.owner_disabled.get());
        assert_eq!(probe.calls.get(), 1);
        assert_eq!(dialog.hwnd(), 0);
    });
}

// C#: NativeDialog.Cleanup invokes the modeless closed callback once, with its panic rethrown.
#[test]
#[ignore = "native: needs desktop"]
fn a_panic_in_the_closed_callback_of_a_modeless_dialog_is_resumed_by_the_loop_owner() {
    on_sta(|| {
        let dialog = NativeDialog::new(colour_content());
        dialog
            .show_modeless(0, None, None, Some(Box::new(|| panic!("closed callback payload"))))
            .unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dialog.close()));
        let payload = outcome.expect_err("callback panic must reach the owning call");
        let message = payload.downcast_ref::<&str>().copied().unwrap_or("");
        assert_eq!(message, "closed callback payload");
        assert_eq!(dialog.hwnd(), 0);
    });
}

// C#: NativeControlsTests.OpeningAndClosingTheGallery_DoesNotLeakGdiOrUserObjects (base cleanup, modal and modeless)
#[test]
#[ignore = "native: needs desktop"]
fn repeated_open_and_close_leaks_no_gdi_or_user_objects() {
    on_sta(|| {
        let modal_cycle = || {
            let content = colour_content();
            let dialog = NativeDialog::new(content);
            show_on_owner(&dialog, |d, _| {
                d.set_tooltip(103, "fixed tip");
                d.set_item_tooltips(102, Rc::new(|index| Some(format!("item {index}"))));
                d.close();
            })
            .unwrap();
        };
        let modeless_cycle = || {
            let dialog = NativeDialog::new(colour_content());
            dialog.show_modeless(0, None, None, None).unwrap();
            dialog.set_tooltip(103, "fixed tip");
            dialog.set_item_tooltips(102, Rc::new(|index| Some(format!("item {index}"))));
            dialog.close();
        };
        modal_cycle();
        modeless_cycle();
        let gdi = fixture::gdi_objects();
        let user = fixture::user_objects();
        for _ in 0..15 {
            modal_cycle();
        }
        for _ in 0..5 {
            modeless_cycle();
        }
        assert!(
            fixture::settles_at_or_below(fixture::gdi_objects, gdi),
            "GDI objects leaked"
        );
        assert!(
            fixture::settles_at_or_below(fixture::user_objects, user),
            "USER objects leaked"
        );
    });
}
