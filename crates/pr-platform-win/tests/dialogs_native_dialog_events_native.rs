//! Desktop event checks for W8-18j. Run explicitly on an interactive Windows desktop.

#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use std::cell::Cell;
use std::rc::Rc;

use fixture::{RecordingChrome, RecordingContent, command, item, on_sta, show_on_owner};
use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::dialog::core::{ID_CANCEL, ID_OK, NativeDialog};
use pr_platform_win::dialogs::native::template::{DIALOG_STYLE_CHILD, DialogControlClass as Class, DialogItem};
use pr_platform_win::win32::structs::{Hwnd, Lparam};
use pr_platform_win::win32::{api, comctl, control_constants as cc, dialog_chrome, dialog_native};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::{NMCUSTOMDRAW, NMHDR, NMTTDISPINFOW};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GW_OWNER, GetWindow, SetCursorPos};
use windows::core::w;

const TAB: u32 = 0x0001_0000;
const BUTTON: i32 = 1;
const CANCEL: i32 = 2;
const EDIT: i32 = 11;
const MULTILINE: i32 = 12;
const LIST: i32 = 13;
const LIST_VIEW: i32 = 14;
const TRACKBAR: i32 = 15;
const COMBO: i32 = 16;
const STATIC: i32 = 17;

fn base_items() -> Vec<DialogItem> {
    vec![
        DialogItem::new(Class::Button, BUTTON, "OK", DIALOG_STYLE_CHILD | TAB),
        DialogItem::new(Class::Button, CANCEL, "Cancel", DIALOG_STYLE_CHILD | TAB),
        DialogItem::new(Class::Edit, EDIT, "one line", DIALOG_STYLE_CHILD | TAB),
        DialogItem::new(
            Class::Edit,
            MULTILINE,
            "two lines",
            DIALOG_STYLE_CHILD | TAB | dialog_native::ES_MULTILINE,
        ),
        DialogItem::new(Class::ListBox, LIST, "", DIALOG_STYLE_CHILD | TAB),
        DialogItem::new(Class::ListView, LIST_VIEW, "", DIALOG_STYLE_CHILD | cc::LVS_REPORT),
        DialogItem::new(Class::Trackbar, TRACKBAR, "", DIALOG_STYLE_CHILD | TAB),
        DialogItem::new(Class::ComboBox, COMBO, "", DIALOG_STYLE_CHILD | TAB),
        DialogItem::new(Class::Static, STATIC, "text", DIALOG_STYLE_CHILD),
    ]
}

fn run_case(
    configure: impl FnOnce(&Rc<RecordingContent>) + Send + 'static,
    check: impl Fn(&NativeDialog, &Rc<RecordingContent>) + Send + 'static,
) -> i32 {
    on_sta(move || {
        let _activation = fixture::activate_visual_styles();
        let content = Rc::new(RecordingContent::default());
        *content.items.borrow_mut() = base_items();
        configure(&content);
        let dialog = NativeDialog::new(content.clone());
        show_on_owner(&dialog, |d, _| {
            check(d, &content);
            if !d.is_closed() {
                d.close();
            }
        })
        .expect("show modal dialog")
    })
}

fn send_notify(dialog: &NativeDialog, header: &mut NMHDR) -> isize {
    // SAFETY: NMHDR remains live throughout synchronous SendMessage.
    unsafe { dialog_native::user32::send_message(dialog.hwnd(), cc::WM_NOTIFY, 0, header as *mut NMHDR as Lparam) }
}

fn tooltip_window(dialog: &NativeDialog) -> Hwnd {
    let mut after = None;
    loop {
        // SAFETY: all arguments are window handles or a static NUL-terminated class name.
        let found = unsafe { FindWindowExW(None, after, w!("tooltips_class32"), None) };
        let Ok(found) = found else { return 0 };
        // SAFETY: found is a live tooltip window; GetWindow only reads its owner handle.
        if unsafe { GetWindow(found, GW_OWNER) }.is_ok_and(|owner| owner.0 as isize == dialog.hwnd()) {
            return found.0 as isize;
        }
        after = Some(found);
    }
}

struct CursorRestore(pr_platform_win::win32::structs::Point);
impl Drop for CursorRestore {
    fn drop(&mut self) {
        // SAFETY: restoring a screen coordinate captured from GetCursorPos.
        let _ = unsafe { SetCursorPos(self.0.x, self.0.y) };
    }
}

fn cursor_over_first_row(dialog: &NativeDialog) -> CursorRestore {
    let old = comctl::user32::get_cursor_pos().expect("cursor position");
    let list = item(dialog, LIST);
    let rect = api::user32::get_window_rect(list).expect("list rect");
    // SAFETY: the target is a screen coordinate within the live list box.
    unsafe { SetCursorPos(rect.left + 8, rect.top + 8) }.expect("move cursor to first row");
    CursorRestore(old)
}

fn tip_text(info: &NMTTDISPINFOW, units: usize) -> String {
    assert!(!info.lpszText.is_null());
    // SAFETY: the dialog's private tip buffer holds the requested number of units through this synchronous request.
    let slice = unsafe { std::slice::from_raw_parts(info.lpszText.0, units) };
    String::from_utf16(slice).expect("UTF-16 tooltip text")
}

struct MemoryDc(Hwnd);
impl MemoryDc {
    fn new() -> Self {
        let dc = dialog_chrome::create_compatible_dc(0);
        assert_ne!(dc, 0);
        Self(dc)
    }
}
impl Drop for MemoryDc {
    fn drop(&mut self) {
        if self.0 != 0 {
            dialog_chrome::delete_dc(self.0);
        }
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn initialize_runs_the_steps_in_order() {
    // C#: NativeDialog.InitializeDialog.
    run_case(
        |_| {},
        |d, c| {
            let events = c.events.borrow();
            let init = events.iter().position(|s| s == "init").expect("init");
            let layout = events.iter().position(|s| s.starts_with("layout:")).expect("layout");
            assert!(init < layout);
            assert_eq!(events.iter().filter(|s| s.starts_with("layout:")).count(), 1);
            assert_ne!(d.body_font(), 0);
            // SAFETY: WM_GETFONT has no pointer parameters.
            let font = unsafe { dialog_native::user32::send_message(item(d, EDIT), dialog_native::WM_GETFONT, 0, 0) };
            assert_eq!(font, d.body_font());
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn chrome_is_applied_exactly_once_with_every_control_in_item_order() {
    // C#: NativeDialog.InitializeDialog / INativeDialogChrome.Apply.
    on_sta(|| {
        let _activation = fixture::activate_visual_styles();
        let content = Rc::new(RecordingContent::default());
        let items = base_items();
        *content.items.borrow_mut() = items.clone();
        let chrome = Rc::new(RecordingChrome::default());
        let dialog = NativeDialog::with_chrome(content, Some(chrome.clone()));
        show_on_owner(&dialog, |d, _| {
            let calls = chrome.apply_calls.borrow();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, d.hwnd());
            assert_eq!(
                calls[0].1.iter().map(|control| control.class).collect::<Vec<_>>(),
                items.iter().map(|x| x.class).collect::<Vec<_>>()
            );
            d.close();
        })
        .expect("show dialog");
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn the_default_button_is_set() {
    // C#: NativeDialog.InitializeDialog DM_SETDEFID.
    run_case(
        |c| c.default_button.set(CANCEL),
        |d, _| {
            // SAFETY: DM_GETDEFID has integer-only parameters.
            let answer = unsafe { dialog_native::user32::send_message(d.hwnd(), dialog_native::DM_GETDEFID, 0, 0) };
            assert_eq!(answer as u32, (0x534b_u32 << 16) | CANCEL as u32);
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn initial_focus_is_the_requested_control() {
    // C#: NativeDialog.InitializeDialog InitialFocusId.
    run_case(
        |c| c.initial_focus.set(EDIT),
        |d, _| assert_eq!(dialog_native::user32::get_focus(), item(d, EDIT)),
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn initial_focus_is_the_first_tab_stop_by_default() {
    // C#: NativeDialog.FirstTabStop.
    run_case(
        |_| {},
        |d, _| assert_eq!(dialog_native::user32::get_focus(), item(d, BUTTON)),
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn ok_command_calls_on_ok_and_closes_with_ok() {
    // C#: NativeDialog.HandleCommand IDOK.
    let result = run_case(
        |_| {},
        |d, c| {
            let _ = command(d, ID_OK, 0);
            assert!(c.events.borrow().iter().any(|e| e == "ok"));
        },
    );
    assert_eq!(result, ID_OK);
}

#[test]
#[ignore = "native: needs desktop"]
fn a_refused_ok_keeps_the_dialog_open() {
    // C#: NativeDialog.HandleCommand OnOk false.
    run_case(
        |c| c.ok_result.set(false),
        |d, c| {
            let _ = command(d, ID_OK, 0);
            assert!(!d.is_closed());
            assert!(c.events.borrow().iter().any(|e| e == "ok"));
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn cancel_command_calls_on_cancel_and_closes_unless_handled() {
    // C#: NativeDialog.HandleCommand IDCANCEL.
    let result = run_case(
        |_| {},
        |d, c| {
            let _ = command(d, ID_CANCEL, 0);
            assert!(c.events.borrow().iter().any(|e| e == "cancel"));
        },
    );
    assert_eq!(result, ID_CANCEL);
    run_case(
        |c| c.cancel_handled.set(true),
        |d, _| {
            let _ = command(d, ID_CANCEL, 0);
            assert!(!d.is_closed());
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn other_commands_reach_on_command_with_the_notification() {
    // C#: NativeDialog.HandleCommand OnCommand.
    run_case(
        |c| c.command_handled.set(true),
        |d, c| {
            let _ = command(d, COMBO, 1);
            assert!(c.events.borrow().iter().any(|e| e == "command:16:1"));
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn a_command_with_a_non_click_notification_for_ok_is_not_ok() {
    // C#: NativeDialog.HandleCommand IDOK with non-click notification.
    run_case(
        |_| {},
        |d, c| {
            assert_eq!(command(d, ID_OK, 1), 0);
            assert!(c.events.borrow().iter().any(|e| e == "command:1:1"));
            assert!(!c.events.borrow().iter().any(|e| e == "ok"));
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn notify_reaches_on_notify_and_its_result_is_the_message_result() {
    // C#: NativeDialog.HandleNotify.
    run_case(
        |c| c.notify_result.set(Some(41)),
        |d, c| {
            let mut header = NMHDR {
                hwndFrom: windows::Win32::Foundation::HWND(item(d, EDIT) as *mut _),
                idFrom: EDIT as usize,
                code: 7,
            };
            assert_eq!(send_notify(d, &mut header), 41);
            assert!(c.events.borrow().iter().any(|e| e == "notify:11:7"));
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn an_unhandled_notify_returns_zero() {
    // C#: NativeDialog.HandleNotify unhandled.
    run_case(
        |_| {},
        |d, _| {
            let mut header = NMHDR {
                hwndFrom: windows::Win32::Foundation::HWND(item(d, EDIT) as *mut _),
                idFrom: EDIT as usize,
                code: 7,
            };
            assert_eq!(send_notify(d, &mut header), 0);
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn scroll_messages_reach_on_scroll() {
    // C#: NativeDialog.HandleMessage WM_HSCROLL / WM_VSCROLL.
    run_case(
        |_| {},
        |d, c| {
            for message in [cc::WM_HSCROLL, cc::WM_VSCROLL] {
                // SAFETY: scroll parameters contain an integer code and a live test trackbar handle.
                unsafe {
                    dialog_native::user32::send_message(d.hwnd(), message, cc::TB_ENDTRACK as usize, item(d, TRACKBAR))
                };
            }
            let before = c.events.borrow().len();
            // SAFETY: null lParam is an intentionally empty scroll notification.
            unsafe { dialog_native::user32::send_message(d.hwnd(), cc::WM_HSCROLL, 0, 0) };
            assert_eq!(c.events.borrow().len(), before);
            assert_eq!(c.events.borrow().iter().filter(|e| *e == "scroll:15:8").count(), 2);
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn is_button_and_is_edit_check_the_class_name() {
    // C#: NativeDialog.IsButton / IsEdit.
    run_case(
        |_| {},
        |d, _| {
            assert!(NativeDialog::is_button(item(d, BUTTON)));
            assert!(!NativeDialog::is_button(item(d, EDIT)));
            assert!(!NativeDialog::is_button(item(d, STATIC)));
            assert!(!NativeDialog::is_button(item(d, COMBO)));
            assert!(!NativeDialog::is_button(0));
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn set_list_view_sort_on_an_absent_control_is_ignored() {
    // C#: NativeDialog.SetListViewSort absent control.
    run_case(|_| {}, |d, _| d.set_list_view_sort(9999, 0, 1));
}

#[test]
#[ignore = "native: needs desktop"]
fn set_tooltip_adds_one_tool() {
    // C#: NativeDialog.SetTooltip / TooltipToolCount.
    run_case(
        |_| {},
        |d, _| {
            assert_eq!(d.tooltip_tool_count(), 0);
            d.set_tooltip(BUTTON, "fixed tip");
            assert_eq!(d.tooltip_tool_count(), 1);
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn item_tooltips_add_one_tool_for_the_whole_list_box_and_a_subclass() {
    // C#: NativeDialog.SetItemTooltips.
    run_case(
        |_| {},
        |d, _| {
            let list = item(d, LIST);
            let before = api::user32::get_window_long_ptr(list, -4); // GWLP_WNDPROC
            d.set_item_tooltips(LIST, Rc::new(|i| Some(format!("row {i}"))));
            assert_eq!(d.tooltip_tool_count(), 1);
            assert_ne!(api::user32::get_window_long_ptr(list, -4), before);
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn item_tooltip_text_comes_from_the_provider_per_row() {
    // C#: NativeDialog.ItemTooltipText.
    run_case(
        |_| {},
        |d, _| {
            d.set_item_tooltips(
                LIST,
                Rc::new(|i| match i {
                    0 => Some("zero".into()),
                    1 => Some("one".into()),
                    _ => None,
                }),
            );
            assert_eq!(d.item_tooltip_text(LIST, 0).as_deref(), Some("zero"));
            assert_eq!(d.item_tooltip_text(LIST, 1).as_deref(), Some("one"));
            assert_eq!(d.item_tooltip_text(LIST, 2), None);
            assert_eq!(d.item_tooltip_text(9999, 0), None);
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn single_line_edits_are_centred_and_multiline_are_not() {
    // C#: NativeDialog.CenterSingleLineEdits.
    run_case(
        |_| {},
        |d, _| {
            assert_ne!(dialog_chrome::get_prop(item(d, EDIT), "PhotoReviewRs.EditCentering"), 0);
            assert_eq!(
                dialog_chrome::get_prop(item(d, MULTILINE), "PhotoReviewRs.EditCentering"),
                0
            );
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn custom_draw_of_a_button_sets_the_text_colour_at_prepaint_only() {
    // C#: NativeDialog.HandleNotify NM_CUSTOMDRAW.
    run_case(
        |_| {},
        |d, c| {
            let dc = MemoryDc::new();
            let original = 0x00ab_1234;
            dialog_native::gdi32::set_text_color(dc.0, original);
            let mut draw = NMCUSTOMDRAW::default();
            draw.hdr.hwndFrom = HWND(item(d, BUTTON) as *mut _);
            draw.hdr.code = cc::NM_CUSTOMDRAW as u32;
            draw.hdc = windows::Win32::Graphics::Gdi::HDC(dc.0 as *mut _);
            draw.dwDrawStage = windows::Win32::UI::Controls::NMCUSTOMDRAW_DRAW_STAGE(cc::CDDS_PREPAINT);
            let notifications = c.events.borrow().iter().filter(|e| e.starts_with("notify:")).count();
            // SAFETY: NMCUSTOMDRAW and its memory DC stay live for synchronous WM_NOTIFY.
            let result = unsafe {
                dialog_native::user32::send_message(
                    d.hwnd(),
                    cc::WM_NOTIFY,
                    0,
                    &mut draw as *mut NMCUSTOMDRAW as Lparam,
                )
            };
            assert_eq!(result, cc::CDRF_DODEFAULT);
            assert_eq!(dialog_native::gdi32::get_text_color(dc.0), DialogColors::text_strong());
            dialog_native::gdi32::set_text_color(dc.0, original);
            draw.dwDrawStage = windows::Win32::UI::Controls::NMCUSTOMDRAW_DRAW_STAGE(2); // CDDS_POSTPAINT
            // SAFETY: same live structure and DC as above.
            unsafe {
                dialog_native::user32::send_message(
                    d.hwnd(),
                    cc::WM_NOTIFY,
                    0,
                    &mut draw as *mut NMCUSTOMDRAW as Lparam,
                )
            };
            assert_eq!(dialog_native::gdi32::get_text_color(dc.0), original);
            assert_eq!(
                c.events.borrow().iter().filter(|e| e.starts_with("notify:")).count(),
                notifications
            );
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn the_list_view_header_hook_is_installed_for_list_view_items() {
    // C#: NativeDialog.HookListView.
    run_case(
        |_| {},
        |d, _| {
            let list = item(d, LIST_VIEW);
            assert_ne!(list, 0);
            assert_ne!(
                pr_platform_win::dialogs::native::controls::list_tree::ListViewControl::header(list),
                0
            );
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn the_tooltip_is_dark_and_unthemed() {
    // C#: NativeDialog.EnsureTooltip.
    run_case(
        |_| {},
        |d, _| {
            d.set_tooltip(BUTTON, "tip");
            let tip = tooltip_window(d);
            assert_ne!(tip, 0);
            // SAFETY: the tooltip is live for this synchronous theme query.
            let theme = unsafe { windows::Win32::UI::Controls::GetWindowTheme(HWND(tip as *mut _)) };
            assert_eq!(theme.0, 0);
            for (message, expected) in [
                (0x0416, DialogColors::from_key("Dark.Tooltip").unwrap_or(0) as isize), // TTM_GETTIPBKCOLOR
                (0x0417, DialogColors::text_strong() as isize),                         // TTM_GETTIPTEXTCOLOR
                (0x0419, d.dx(240) as isize),                                           // TTM_GETMAXTIPWIDTH
            ] {
                // SAFETY: tooltip query messages have integer-only parameters.
                let actual = unsafe { dialog_native::user32::send_message(tip, message, 0, 0) };
                assert_eq!(actual, expected);
            }
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn get_disp_info_writes_the_tip_text_into_the_private_buffer() {
    // C#: NativeDialog.ProvideTipText.
    run_case(
        |_| {},
        |d, _| {
            pr_platform_win::dialogs::native::controls::basic::ListBoxControl::add(item(d, LIST), "row zero");
            d.place(LIST, 8, 8, 180, 100);
            let _cursor = cursor_over_first_row(d);
            let text = Rc::new(Cell::new(false));
            let longer = Rc::clone(&text);
            d.set_item_tooltips(
                LIST,
                Rc::new(move |_| {
                    Some(
                        if longer.get() {
                            "a longer second tip"
                        } else {
                            "first row tip"
                        }
                        .into(),
                    )
                }),
            );
            let mut info = NMTTDISPINFOW::default();
            info.hdr.idFrom = item(d, LIST) as usize;
            info.hdr.code = cc::TTN_GETDISPINFO as u32;
            // SAFETY: NMTTDISPINFOW stays live during synchronous WM_NOTIFY.
            unsafe {
                dialog_native::user32::send_message(
                    d.hwnd(),
                    cc::WM_NOTIFY,
                    0,
                    &mut info as *mut NMTTDISPINFOW as Lparam,
                )
            };
            assert_eq!(tip_text(&info, "first row tip".len()), "first row tip");
            text.set(true);
            // SAFETY: same live structure, list, cursor position, and dialog as the first request.
            unsafe {
                dialog_native::user32::send_message(
                    d.hwnd(),
                    cc::WM_NOTIFY,
                    0,
                    &mut info as *mut NMTTDISPINFOW as Lparam,
                )
            };
            assert_eq!(tip_text(&info, "a longer second tip".len()), "a longer second tip");
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn get_disp_info_for_a_control_without_a_provider_gives_an_empty_text() {
    // C#: NativeDialog.ProvideTipText without provider.
    run_case(
        |_| {},
        |d, _| {
            let mut info = NMTTDISPINFOW::default();
            info.hdr.idFrom = item(d, EDIT) as usize;
            info.hdr.code = cc::TTN_GETDISPINFO as u32;
            // SAFETY: NMTTDISPINFOW stays live during synchronous WM_NOTIFY.
            unsafe {
                dialog_native::user32::send_message(
                    d.hwnd(),
                    cc::WM_NOTIFY,
                    0,
                    &mut info as *mut NMTTDISPINFOW as Lparam,
                )
            };
            assert!(!info.lpszText.is_null());
            // SAFETY: the dialog supplies a live UTF-16 buffer for this request.
            assert_eq!(unsafe { *info.lpszText.0 }, 0);
        },
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn a_long_tip_text_grows_the_buffer_beyond_256_units() {
    // C#: NativeDialog.EnsureTipBuffer.
    run_case(
        |_| {},
        |d, _| {
            pr_platform_win::dialogs::native::controls::basic::ListBoxControl::add(item(d, LIST), "row zero");
            d.place(LIST, 8, 8, 180, 100);
            let _cursor = cursor_over_first_row(d);
            d.set_item_tooltips(LIST, Rc::new(|_| Some("x".repeat(300))));
            let mut info = NMTTDISPINFOW::default();
            info.hdr.idFrom = item(d, LIST) as usize;
            info.hdr.code = cc::TTN_GETDISPINFO as u32;
            // SAFETY: NMTTDISPINFOW stays live during synchronous WM_NOTIFY.
            unsafe {
                dialog_native::user32::send_message(
                    d.hwnd(),
                    cc::WM_NOTIFY,
                    0,
                    &mut info as *mut NMTTDISPINFOW as Lparam,
                )
            };
            assert_eq!(tip_text(&info, 300), "x".repeat(300));
        },
    );
}
