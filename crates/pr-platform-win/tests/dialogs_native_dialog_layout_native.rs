//! Desktop contract tests for W8-18i. Run explicitly on an interactive desktop.
#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use std::rc::Rc;

use fixture::{RecordingChrome, RecordingContent, on_sta, show_on_owner};
use pr_platform_win::dialogs::native::chrome::DialogChromeControl;
use pr_platform_win::dialogs::native::dialog::core::NativeDialog;
use pr_platform_win::dialogs::native::dialog::dpi::ALPHABET;
use pr_platform_win::dialogs::native::list_dialog::ListDialogBase;
use pr_platform_win::dialogs::native::template::{DialogControlClass, DialogItem};
use pr_platform_win::win32::structs::{Hwnd, Rect};
use pr_platform_win::win32::{api, dialog_native, messages};
use windows::Win32::UI::WindowsAndMessaging::MINMAXINFO;

const HEADER: i32 = 100;
const BODY: i32 = 101;
const THIRD: i32 = 102;

fn content() -> Rc<RecordingContent> {
    let c = Rc::new(RecordingContent::default());
    c.items.borrow_mut().extend([
        DialogItem::new(DialogControlClass::Static, HEADER, "Header", 0),
        DialogItem::new(DialogControlClass::Button, BODY, "Body", 0),
        DialogItem::new(DialogControlClass::Button, THIRD, "Third", 0),
    ]);
    c
}

fn run(test: impl Fn(&NativeDialog, &Rc<RecordingContent>, Hwnd) + Send + 'static) {
    on_sta(move || {
        let c = content();
        let d = NativeDialog::new(c.clone());
        show_on_owner(&d, |d, owner| {
            test(d, &c, owner);
            d.close();
        })
        .unwrap();
    });
}

fn send(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: callers supply a valid payload for the synchronous message.
    unsafe { dialog_native::user32::send_message(hwnd, message, wparam, lparam) }
}

fn font(hwnd: Hwnd) -> isize {
    send(hwnd, dialog_native::WM_GETFONT, 0, 0)
}

fn font_height(hwnd: Hwnd, selected: isize) -> i32 {
    let hdc = dialog_native::user32::get_dc(hwnd);
    assert_ne!(hdc, 0);
    let old = dialog_native::gdi32::select_object(hdc, selected);
    let extent = dialog_native::gdi32::get_text_extent_point32(hdc, "Hg").unwrap();
    dialog_native::gdi32::select_object(hdc, old);
    dialog_native::user32::release_dc(hwnd, hdc);
    extent.y
}

fn size(rect: Rect) -> (i32, i32) {
    (rect.right - rect.left, rect.bottom - rect.top)
}

#[test]
#[ignore = "native: needs desktop"]
fn the_dialog_units_follow_the_measured_font() {
    // C#: NativeDialog.MeasureUnits / Dx / Dy.
    run(|d, _, _| {
        let hdc = dialog_native::user32::get_dc(d.hwnd());
        assert_ne!(hdc, 0);
        let old = dialog_native::gdi32::select_object(hdc, d.body_font());
        let extent = dialog_native::gdi32::get_text_extent_point32(hdc, ALPHABET).unwrap();
        dialog_native::gdi32::select_object(hdc, old);
        dialog_native::user32::release_dc(d.hwnd(), hdc);
        assert_eq!(d.dx(4), ((extent.x / 26 + 1) / 2).max(1));
        assert_eq!(d.dy(8), d.line_height());
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn the_initial_client_size_is_the_preferred_size() {
    // C#: NativeDialog.PlaceInitially.
    run(|d, c, _| assert_eq!(d.current_client_size(), (c.preferred.borrow())(d)));
}

#[test]
#[ignore = "native: needs desktop"]
fn the_dialog_is_centred_on_the_owner_inside_the_work_area() {
    // C#: NativeDialog.PlaceInitially.
    run(|d, _, owner| {
        let a = api::user32::get_window_rect(d.hwnd()).unwrap();
        let b = api::user32::get_window_rect(owner).unwrap();
        assert!(((a.left + a.right) - (b.left + b.right)).abs() <= 1);
        assert!(((a.top + a.bottom) - (b.top + b.bottom)).abs() <= 1);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn a_big_resizable_dialog_is_shrunk_to_the_work_area() {
    // C#: NativeDialog.PlaceInitially.
    on_sta(|| {
        let c = content();
        c.resizable.set(true);
        *c.preferred.borrow_mut() = Box::new(|_| (20_000, 20_000));
        let d = NativeDialog::new(c);
        show_on_owner(&d, |d, owner| {
            let monitor = api::user32::monitor_from_window(owner, 1);
            let mut info = Default::default();
            api::user32::get_monitor_info(monitor, &mut info).unwrap();
            let frame = api::user32::get_window_rect(d.hwnd()).unwrap();
            assert!(size(frame).0 <= size(info.rcWork).0);
            assert!(size(frame).1 <= size(info.rcWork).1);
            d.close();
        })
        .unwrap();
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn layout_runs_on_wm_size_but_not_when_minimised() {
    // C#: NativeDialog.HandleMessage WM_SIZE.
    run(|d, c, _| {
        let before = c.events.borrow().iter().filter(|s| s.starts_with("layout:")).count();
        send(d.hwnd(), messages::WM_SIZE, 1, 0);
        assert_eq!(
            c.events.borrow().iter().filter(|s| s.starts_with("layout:")).count(),
            before
        );
        send(d.hwnd(), messages::WM_SIZE, 0, 0);
        assert_eq!(
            c.events.borrow().iter().filter(|s| s.starts_with("layout:")).count(),
            before + 1
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn min_track_size_is_the_frame_of_the_minimum_client_size() {
    // C#: NativeDialog.HandleMessage WM_GETMINMAXINFO.
    on_sta(|| {
        for resizable in [false, true] {
            let c = content();
            c.resizable.set(resizable);
            *c.minimum.borrow_mut() = Some(Box::new(|_| (180, 90)));
            let d = NativeDialog::new(c);
            show_on_owner(&d, |d, _| {
                let mut info = MINMAXINFO::default();
                send(
                    d.hwnd(),
                    messages::WM_GETMINMAXINFO,
                    0,
                    (&mut info as *mut MINMAXINFO) as isize,
                );
                let expected = if resizable {
                    d.window_size_for_client(180, 90)
                } else {
                    (0, 0)
                };
                assert_eq!((info.ptMinTrackSize.x, info.ptMinTrackSize.y), expected);
                d.close();
            })
            .unwrap();
        }
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn dpi_change_rescales_the_body_and_header_fonts() {
    // C#: DiagnosticsAndListDialogTests.ListAndDiagnostics_DpiChange_RescalesTheFontsIncludingTheHeaderFont.
    on_sta(|| {
        let c = content();
        c.header_ids.borrow_mut().push(HEADER);
        let d = NativeDialog::new(c);
        show_on_owner(&d, |d, _| {
            let before = (
                font_height(d.hwnd(), d.body_font()),
                font_height(d.hwnd(), font(d.item(HEADER))),
            );
            let next = d.dpi() * 2;
            let mut rect = api::user32::get_window_rect(d.hwnd()).unwrap();
            send(
                d.hwnd(),
                messages::WM_DPICHANGED,
                next as usize,
                (&mut rect as *mut Rect) as isize,
            );
            assert_eq!(d.dpi(), next);
            let after = (
                font_height(d.hwnd(), d.body_font()),
                font_height(d.hwnd(), font(d.item(HEADER))),
            );
            for (original, scaled) in [(before.0, after.0), (before.1, after.1)] {
                let ratio = f64::from(scaled) / f64::from(original);
                assert!((1.8..=2.2).contains(&ratio), "font scale ratio {ratio}");
            }
            d.close();
        })
        .unwrap();
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn dpi_change_to_the_same_dpi_is_ignored() {
    // C#: NativeDialog.OnDpiChanged guard.
    run(|d, c, _| {
        let before = (d.body_font(), c.events.borrow().len());
        let mut rect = api::user32::get_window_rect(d.hwnd()).unwrap();
        send(
            d.hwnd(),
            messages::WM_DPICHANGED,
            d.dpi() as usize,
            (&mut rect as *mut Rect) as isize,
        );
        assert_eq!((d.body_font(), c.events.borrow().len()), before);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn dpi_change_of_a_non_resizable_dialog_resizes_to_the_preferred_frame() {
    // C#: NativeDialog.OnDpiChanged.
    run(|d, c, _| {
        let mut suggested = Rect {
            left: 200,
            top: 200,
            right: 900,
            bottom: 900,
        };
        send(
            d.hwnd(),
            messages::WM_DPICHANGED,
            (d.dpi() * 2) as usize,
            (&mut suggested as *mut Rect) as isize,
        );
        assert_eq!(
            size(api::user32::get_window_rect(d.hwnd()).unwrap()),
            d.window_size_for_client((c.preferred.borrow())(d).0, (c.preferred.borrow())(d).1)
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn dpi_change_of_a_resizable_dialog_takes_the_suggested_rectangle() {
    // C#: NativeDialog.OnDpiChanged.
    on_sta(|| {
        let c = content();
        c.resizable.set(true);
        let d = NativeDialog::new(c);
        show_on_owner(&d, |d, _| {
            let mut suggested = Rect {
                left: 200,
                top: 200,
                right: 700,
                bottom: 600,
            };
            send(
                d.hwnd(),
                messages::WM_DPICHANGED,
                (d.dpi() * 2) as usize,
                (&mut suggested as *mut Rect) as isize,
            );
            assert_eq!(api::user32::get_window_rect(d.hwnd()).unwrap(), suggested);
            d.close();
        })
        .unwrap();
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn the_header_font_goes_only_to_the_header_controls() {
    // C#: NativeDialog.ApplyFonts.
    on_sta(|| {
        for has_header in [false, true] {
            let c = content();
            if has_header {
                c.header_ids.borrow_mut().push(HEADER);
            }
            let d = NativeDialog::new(c);
            show_on_owner(&d, |d, _| {
                assert_eq!(font(d.item(BODY)), d.body_font());
                assert_eq!(font(d.item(HEADER)) != d.body_font(), has_header);
                d.close();
            })
            .unwrap();
        }
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn measure_text_height_wraps_long_text() {
    // C#: NativeDialog.MeasureTextHeight.
    run(|d, _, _| {
        let long = "Một đoạn văn tiếng Việt đủ dài để được xuống dòng trong khung rất hẹp";
        assert!(d.measure_text_height(long, 40, 0) >= d.line_height() * 2);
        assert!(d.measure_text_height(long, 0, 0) >= d.line_height());
        assert!(d.measure_text_height(long, -1, 0) >= d.line_height());
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn measure_item_height_of_an_absent_control_is_zero() {
    // C#: NativeDialog.MeasureItemHeight.
    run(|d, _, _| assert_eq!(d.measure_item_height(9999, 100), 0));
}

#[test]
#[ignore = "native: needs desktop"]
fn place_clamps_negative_sizes_to_zero() {
    // C#: NativeDialog.Place.
    run(|d, _, _| {
        d.place(BODY, 10, 20, -1, -5);
        assert_eq!(size(api::user32::get_window_rect(d.item(BODY)).unwrap()), (0, 0));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn show_item_hides_and_shows() {
    // C#: NativeDialog.ShowItem.
    run(|d, _, _| {
        d.show_item(BODY, false);
        assert!(!api::user32::is_window_visible(d.item(BODY)));
        d.show_item(BODY, true);
        assert!(api::user32::is_window_visible(d.item(BODY)));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn set_item_text_and_get_item_text_round_trip() {
    // C#: NativeDialog.SetItemText / GetItemText.
    run(|d, _, _| {
        d.set_item_text(BODY, "Ảnh 🐈");
        assert_eq!(d.get_item_text(BODY), "Ảnh 🐈");
        assert_eq!(d.get_item_text(9999), "");
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn place_button_row_aligns_the_buttons_right_with_equal_widths() {
    // C#: NativeDialog.PlaceButtonRow.
    run(|d, _, _| {
        let total = d.place_button_row(600, 10, 40, 30, &[BODY, THIRD]);
        let a = api::user32::get_window_rect(d.item(BODY)).unwrap();
        let b = api::user32::get_window_rect(d.item(THIRD)).unwrap();
        assert_eq!(size(a), size(b));
        assert_eq!(total, d.button_row_width(&[BODY, THIRD]));
        assert_eq!(b.left - a.right, d.dx(6));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn resize_client_keeps_the_top_left_corner() {
    // C#: NativeDialog.ResizeClient.
    run(|d, _, _| {
        let before = api::user32::get_window_rect(d.hwnd()).unwrap();
        d.resize_client(300, 200);
        let after = api::user32::get_window_rect(d.hwnd()).unwrap();
        assert_eq!((after.left, after.top), (before.left, before.top));
        assert_eq!(d.current_client_size(), (300, 200));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn window_size_for_client_matches_the_nonclient_difference() {
    // C#: NativeDialog.WindowSizeForClient.
    run(|d, _, _| {
        let frame = size(api::user32::get_window_rect(d.hwnd()).unwrap());
        let client = d.current_client_size();
        assert_eq!(
            d.window_size_for_client(300, 200),
            (300 + frame.0 - client.0, 200 + frame.1 - client.1)
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn current_client_size_matches_get_client_rect() {
    // C#: NativeDialog.CurrentClientSize.
    run(|d, _, _| {
        assert_eq!(
            d.current_client_size(),
            size(api::user32::get_client_rect(d.hwnd()).unwrap())
        )
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn wm_dpichanged_with_a_null_rect_changes_the_dpi_but_not_the_position() {
    // Rust guard for NativeDialog.OnDpiChangedMessage.
    run(|d, _, _| {
        let before = api::user32::get_window_rect(d.hwnd()).unwrap();
        let next = d.dpi() * 2;
        send(d.hwnd(), messages::WM_DPICHANGED, next as usize, 0);
        assert_eq!(d.dpi(), next);
        assert_eq!(api::user32::get_window_rect(d.hwnd()).unwrap(), before);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn apply_chrome_calls_apply_controls_only() {
    // C#: NativeDialog.ApplyChrome.
    on_sta(|| {
        let c = content();
        let chrome = Rc::new(RecordingChrome::default());
        let d = NativeDialog::with_chrome(c, Some(chrome.clone()));
        show_on_owner(&d, |d, _| {
            let controls = [DialogChromeControl {
                hwnd: d.item(BODY),
                class: DialogControlClass::Button,
            }];
            d.apply_chrome(&controls);
            assert_eq!(chrome.apply_calls.borrow().len(), 1);
            assert_eq!(chrome.apply_controls_calls.borrow().len(), 1);
            d.close();
        })
        .unwrap();
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn list_dialog_base_delegates() {
    // C#: ListDialogBase forwarding to NativeDialog.
    run(|d, _, _| {
        let base: &dyn ListDialogBase = d;
        assert_eq!(base.item(BODY), d.item(BODY));
        assert_eq!(base.dx(10), d.dx(10));
        assert_eq!(base.dy(10), d.dy(10));
        assert_eq!(base.line_height(), d.line_height());
        assert_eq!(base.measure_text_width("sample"), d.measure_text_width("sample", 0));
        assert_eq!(base.measure_item_height(BODY, 100), d.measure_item_height(BODY, 100));
        assert_eq!(base.button_row_width(&[BODY]), d.button_row_width(&[BODY]));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn the_old_fonts_are_deleted_after_a_dpi_change() {
    // C#: NativeDialog.ApplyFonts deletes old fonts only after assigning replacements.
    on_sta(|| {
        let baseline = dialog_native::user32::get_gui_resources(-1, 0);
        let c = content();
        c.header_ids.borrow_mut().push(HEADER);
        let d = NativeDialog::new(c);
        show_on_owner(&d, |d, _| {
            let previous = [d.body_font(), font(d.item(HEADER))];
            let mut rect = api::user32::get_window_rect(d.hwnd()).unwrap();
            send(
                d.hwnd(),
                messages::WM_DPICHANGED,
                (d.dpi() * 2) as usize,
                (&mut rect as *mut Rect) as isize,
            );
            assert_ne!(d.body_font(), previous[0]);
            assert_ne!(font(d.item(HEADER)), previous[1]);
            for current in [d.body_font(), font(d.item(HEADER))] {
                // SAFETY: GetObjectType reads the live font handle and does not modify it.
                let object_type = unsafe {
                    windows::Win32::Graphics::Gdi::GetObjectType(windows::Win32::Graphics::Gdi::HGDIOBJ(
                        current as *mut _,
                    ))
                };
                assert_ne!(object_type, 0);
            }
            for old in previous {
                assert_eq!(
                    // SAFETY: GetObjectType reads a GDI handle value and returns zero for deleted handles.
                    unsafe {
                        windows::Win32::Graphics::Gdi::GetObjectType(windows::Win32::Graphics::Gdi::HGDIOBJ(
                            old as *mut _,
                        ))
                    },
                    0
                );
            }
            d.close();
        })
        .unwrap();
        assert_eq!(dialog_native::user32::get_gui_resources(-1, 0), baseline);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn item_finds_controls_in_child_hosts() {
    // C#: NativeDialog.Item searches registered child hosts.
    run(|d, _, _| {
        let instance = api::kernel32::get_module_handle().unwrap_or(0);
        // SAFETY: STATIC is a system class; the parent is the live test dialog and the creation parameter is null.
        let host = unsafe {
            api::user32::create_window_ex(
                0,
                "STATIC",
                "host",
                messages::WS_CHILD,
                0,
                0,
                100,
                100,
                d.hwnd(),
                0,
                instance,
                0,
            )
        }
        .unwrap();
        // SAFETY: SetParent reparents this live template control to a live child of the same dialog.
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetParent(
                windows::Win32::Foundation::HWND(d.item(BODY) as *mut _),
                Some(windows::Win32::Foundation::HWND(host as *mut _)),
            )
        }
        .unwrap();
        d.add_child_host(host);
        assert_eq!(d.item(BODY), dialog_native::user32::get_dlg_item(host, BODY));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn register_child_gets_the_body_font_and_follows_dpi() {
    // C#: NativeDialog.RegisterChild / ApplyFonts.
    run(|d, _, _| {
        let instance = api::kernel32::get_module_handle().unwrap_or(0);
        // SAFETY: STATIC is a system class; the parent is the live test dialog and the creation parameter is null.
        let child = unsafe {
            api::user32::create_window_ex(
                0,
                "STATIC",
                "added",
                messages::WS_CHILD,
                0,
                0,
                100,
                20,
                d.hwnd(),
                0,
                instance,
                0,
            )
        }
        .unwrap();
        d.register_child(child);
        assert_eq!(font(child), d.body_font());
        let old = font(child);
        let mut rect = api::user32::get_window_rect(d.hwnd()).unwrap();
        send(
            d.hwnd(),
            messages::WM_DPICHANGED,
            (d.dpi() * 2) as usize,
            (&mut rect as *mut Rect) as isize,
        );
        assert_eq!(font(child), d.body_font());
        assert_ne!(font(child), old);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn set_list_item_height_is_applied() {
    // C#: NativeDialog.SetListItemHeight.
    on_sta(|| {
        let c = content();
        c.items
            .borrow_mut()
            .push(DialogItem::new(DialogControlClass::ListBox, 103, "", 0x0010)); // LBS_OWNERDRAWFIXED
        let d = NativeDialog::new(c);
        show_on_owner(&d, |d, _| {
            d.set_list_item_height(103, 31);
            assert_eq!(send(d.item(103), 0x01A1, 0, 0), 31); // LB_GETITEMHEIGHT
            d.set_list_item_height(103, 0);
            assert_eq!(send(d.item(103), 0x01A1, 0, 0), 1);
            d.close();
        })
        .unwrap();
    });
}
