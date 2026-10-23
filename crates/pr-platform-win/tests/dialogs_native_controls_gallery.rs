//! Native control gallery checks ported from `NativeControlsTests.cs`.

#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;
#[path = "common/dialogs_control_gallery.rs"]
mod gallery;

use fixture::{
    RecordingChrome, activate_visual_styles, child_rect, client_rect, command, gdi_objects, height, item, on_sta,
    settles_at_or_below, show_on_owner, user_objects, width,
};
use gallery::{BADGE_LIST, COMBO, ControlGalleryContent, REPORT, SLIDER, TIP_BUTTON};
use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::controls::basic::ComboBoxControl;
use pr_platform_win::dialogs::native::controls::custom::{ButtonStateControl, TrackbarControl};
use pr_platform_win::dialogs::native::controls::list_tree::ListViewControl;
use pr_platform_win::dialogs::native::dialog::NativeDialog;
use pr_platform_win::dialogs::native::template::DialogControlClass;
use pr_platform_win::win32::{control_constants, dialog_native, messages};
use std::rc::Rc;
use windows::Win32::UI::Controls::{NMCUSTOMDRAW, NMLISTVIEW, NMLVCUSTOMDRAW};

// C#: NativeControlsTests.EveryNewControlClass_IsCreatedFromTheTemplate
#[test]
#[ignore = "native: needs desktop"]
fn every_new_control_class_is_created_from_the_template() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = NativeDialog::new(Rc::new(ControlGalleryContent::default()));
        show_on_owner(&dialog, |d, _| {
            for id in [COMBO, BADGE_LIST, REPORT, SLIDER, TIP_BUTTON] {
                assert_ne!(item(d, id), 0, "control {id}");
            }
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeControlsTests.BadgeList_ItemTooltips_ComeFromTheProvider_PerRow
#[test]
#[ignore = "native: needs desktop"]
fn badge_list_item_tooltips_come_from_the_provider_per_row() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = NativeDialog::new(Rc::new(ControlGalleryContent::default()));
        show_on_owner(&dialog, |d, _| {
            assert_eq!(d.item_tooltip_text(BADGE_LIST, 0).as_deref(), Some("Mode: Fast"));
            assert_eq!(d.item_tooltip_text(BADGE_LIST, 2).as_deref(), Some("Mode: Original"));
            assert_eq!(d.item_tooltip_text(BADGE_LIST, 99), None);
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeControlsTests.Layout_AtOneAndOneAndAHalfScale_NoControlOverlapsAnotherOrLeavesTheClient
#[test]
#[ignore = "native: needs desktop"]
fn layout_at_one_and_one_and_a_half_scale_no_control_overlaps_another_or_leaves_the_client() {
    on_sta(|| {
        let dialog = NativeDialog::new(Rc::new(ControlGalleryContent::default()));
        show_on_owner(&dialog, |d, _| {
            let check = |d: &NativeDialog| {
                let client = client_rect(d.hwnd());
                for control in ControlGalleryContent::build_items() {
                    let rect = child_rect(d, control.id);
                    assert!(
                        rect.left >= 0 && rect.top >= 0,
                        "control {} has negative origin",
                        control.id
                    );
                    assert!(
                        rect.right <= width(client) && rect.bottom <= height(client),
                        "control {} leaves client",
                        control.id
                    );
                }
                assert!(child_rect(d, gallery::NAME_LABEL).bottom <= child_rect(d, COMBO).top);
                assert!(child_rect(d, BADGE_LIST).bottom <= child_rect(d, SLIDER).top);
                assert!(child_rect(d, BADGE_LIST).right <= child_rect(d, REPORT).left);
            };
            check(d);
            let frame = fixture::window_rect(d.hwnd());
            let dpi = d.dpi() * 3 / 2;
            let suggested = windows::Win32::Foundation::RECT {
                left: frame.left,
                top: frame.top,
                right: frame.left + width(frame) * 3 / 2,
                bottom: frame.top + height(frame) * 3 / 2,
            };
            // SAFETY: WM_DPICHANGED receives a live suggested RECT for this synchronous send.
            unsafe {
                dialog_native::user32::send_message(
                    d.hwnd(),
                    messages::WM_DPICHANGED,
                    ((dpi << 16) | dpi) as usize,
                    (&suggested as *const windows::Win32::Foundation::RECT) as isize,
                );
            }
            check(d);
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeControlsTests.Chrome_IsGivenEveryControlWithItsClass_ExactlyOnce
#[test]
fn gallery_template_contains_every_control_class() {
    let items = ControlGalleryContent::build_items();
    for class in [
        DialogControlClass::Static,
        DialogControlClass::ComboBox,
        DialogControlClass::ListBox,
        DialogControlClass::ListView,
        DialogControlClass::Progress,
        DialogControlClass::Trackbar,
        DialogControlClass::Edit,
        DialogControlClass::Button,
    ] {
        assert!(items.iter().any(|item| item.class == class), "missing {class:?}");
    }
    assert_eq!(items.len(), 16);
    assert_eq!(dialog_native::WM_CTLCOLORSTATIC, 0x0138);
}

// C#: NativeControlsTests.Chrome_IsGivenEveryControlWithItsClass_ExactlyOnce
#[test]
#[ignore = "native: needs desktop"]
fn chrome_is_given_every_control_with_its_class_exactly_once() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let chrome = Rc::new(RecordingChrome::default());
        let dialog = NativeDialog::with_chrome(Rc::new(ControlGalleryContent::default()), Some(chrome.clone()));
        show_on_owner(&dialog, |d, _| d.close()).unwrap();
        let calls = chrome.apply_calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1.len(), ControlGalleryContent::build_items().len());
    });
}

// C#: NativeControlsTests.ComboBox_SelectionChange_ReachesTheDialogAsACommand
#[test]
#[ignore = "native: needs desktop"]
fn combo_box_selection_change_reaches_the_dialog_as_a_command() {
    on_sta(|| {
        let content = Rc::new(ControlGalleryContent::default());
        let dialog = NativeDialog::new(content.clone());
        show_on_owner(&dialog, |d, _| {
            ComboBoxControl::select(item(d, COMBO), 2);
            assert_eq!(ComboBoxControl::selected_index(item(d, COMBO)), 2);
            command(d, COMBO, 1);
            assert_eq!(content.last_command_id.get(), COMBO);
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeControlsTests.OpeningAndClosingTheGallery_DoesNotLeakGdiOrUserObjects
#[test]
#[ignore = "native: needs desktop"]
fn opening_and_closing_the_gallery_does_not_leak_gdi_or_user_objects() {
    on_sta(|| {
        let cycle = || {
            let dialog = NativeDialog::new(Rc::new(ControlGalleryContent::default()));
            show_on_owner(&dialog, |d, _| d.close()).unwrap();
        };
        cycle();
        let gdi = gdi_objects();
        let user = user_objects();
        for _ in 0..15 {
            cycle();
        }
        assert!(settles_at_or_below(gdi_objects, gdi), "GDI objects leaked");
        assert!(settles_at_or_below(user_objects, user), "USER objects leaked");
    });
}

// C#: NativeControlsTests.ListView_ColumnClick_SortsAscendingThenDescending_AndDrawsTheArrowState
#[test]
#[ignore = "native: needs desktop"]
fn list_view_column_click_sorts_ascending_then_descending_and_draws_the_arrow_state() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let content = Rc::new(ControlGalleryContent::default());
        let dialog = NativeDialog::new(content.clone());
        show_on_owner(&dialog, |d, _| {
            let mut notice = NMLISTVIEW::default();
            notice.hdr.hwndFrom = windows::Win32::Foundation::HWND(item(d, REPORT) as *mut _);
            notice.hdr.idFrom = REPORT as usize;
            notice.hdr.code = control_constants::LVN_COLUMNCLICK as u32;
            notice.iSubItem = 1;
            for expected in [
                [
                    "Fast Sequential",
                    "Original Correctness",
                    "Preview Light",
                    "Rapid Key Press",
                ],
                [
                    "Rapid Key Press",
                    "Preview Light",
                    "Original Correctness",
                    "Fast Sequential",
                ],
            ] {
                // SAFETY: NMLISTVIEW lives through this synchronous WM_NOTIFY call.
                unsafe {
                    dialog_native::user32::send_message(
                        d.hwnd(),
                        control_constants::WM_NOTIFY,
                        REPORT as usize,
                        (&notice as *const NMLISTVIEW) as isize,
                    )
                };
                let actual: Vec<_> = (0..4)
                    .map(|row| ListViewControl::cell_text(item(d, REPORT), row, 1))
                    .collect();
                assert_eq!(actual, expected);
            }
            assert_eq!(*content.column_clicks.borrow(), [1, 1]);
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeControlsTests.ListView_CustomDraw_ColoursBestFailAndAlternatingRows
#[test]
#[ignore = "native: needs desktop"]
fn list_view_custom_draw_colours_best_fail_and_alternating_rows() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = NativeDialog::new(Rc::new(ControlGalleryContent::default()));
        show_on_owner(&dialog, |d, _| {
            for (row, key) in [
                (1, "Dark.SuccessBackground"),
                (2, "Dark.ErrorBackground"),
                (0, "Dark.Panel"),
                (3, "Dark.RowAlt"),
            ] {
                let mut notice = NMLVCUSTOMDRAW::default();
                notice.nmcd.hdr.hwndFrom = windows::Win32::Foundation::HWND(item(d, REPORT) as *mut _);
                notice.nmcd.hdr.idFrom = REPORT as usize;
                notice.nmcd.hdr.code = control_constants::NM_CUSTOMDRAW as u32;
                notice.nmcd.dwDrawStage.0 = control_constants::CDDS_ITEMPREPAINT;
                notice.nmcd.dwItemSpec = row as usize;
                notice.nmcd.lItemlParam.0 = row as isize;
                // SAFETY: NMLVCUSTOMDRAW lives through synchronous WM_NOTIFY and permits colour writes.
                let result = unsafe {
                    dialog_native::user32::send_message(
                        d.hwnd(),
                        control_constants::WM_NOTIFY,
                        REPORT as usize,
                        (&mut notice as *mut NMLVCUSTOMDRAW) as isize,
                    )
                };
                assert_eq!(result, control_constants::CDRF_NEWFONT);
                assert_eq!(notice.clrTextBk.0, DialogColors::from_key(key).unwrap());
                assert_eq!(notice.clrText.0, DialogColors::text());
            }
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeControlsTests.ListView_HeaderIsDrawnByTheDialog_SkippingTheThemesGreyText
#[test]
#[ignore = "native: needs desktop"]
fn list_view_header_is_drawn_by_the_dialog_skipping_the_themes_grey_text() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = NativeDialog::new(Rc::new(ControlGalleryContent::default()));
        show_on_owner(&dialog, |d, _| {
            let report = item(d, REPORT);
            let header = ListViewControl::header(report);
            let dc = dialog_native::user32::get_dc(header);
            let mut notice = NMCUSTOMDRAW::default();
            notice.hdr.hwndFrom = windows::Win32::Foundation::HWND(header as *mut _);
            notice.hdr.code = control_constants::NM_CUSTOMDRAW as u32;
            notice.dwDrawStage.0 = control_constants::CDDS_ITEMPREPAINT;
            notice.hdc = windows::Win32::Graphics::Gdi::HDC(dc as *mut _);
            notice.rc = windows::Win32::Foundation::RECT {
                left: 0,
                top: 0,
                right: 80,
                bottom: 20,
            };
            notice.dwItemSpec = 1;
            // SAFETY: live header and DC; NMCUSTOMDRAW stays live during synchronous notification.
            let result = unsafe {
                dialog_native::user32::send_message(
                    report,
                    control_constants::WM_NOTIFY,
                    0,
                    (&notice as *const NMCUSTOMDRAW) as isize,
                )
            };
            dialog_native::user32::release_dc(header, dc);
            assert_eq!(result, control_constants::CDRF_SKIPDEFAULT);
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeControlsTests.Trackbar_HasItsRange_AndScrollMessagesReachTheDialog
#[test]
#[ignore = "native: needs desktop"]
fn trackbar_has_its_range_and_scroll_messages_reach_the_dialog() {
    on_sta(|| {
        let content = Rc::new(ControlGalleryContent::default());
        let dialog = NativeDialog::new(content.clone());
        show_on_owner(&dialog, |d, _| {
            let slider = item(d, SLIDER);
            TrackbarControl::set_position(slider, 70);
            // SAFETY: WM_HSCROLL carries the live slider handle and an integer scroll code.
            unsafe {
                dialog_native::user32::send_message(
                    d.hwnd(),
                    control_constants::WM_HSCROLL,
                    control_constants::TB_THUMBTRACK as usize,
                    slider,
                )
            };
            // SAFETY: TBM_GETRANGEMAX carries no pointer.
            let max = unsafe { dialog_native::user32::send_message(slider, control_constants::TBM_GETRANGEMAX, 0, 0) };
            assert_eq!(max, 100);
            assert_eq!(TrackbarControl::position(slider), 70);
            assert_eq!(*content.scrolls.borrow(), [(SLIDER, control_constants::TB_THUMBTRACK)]);
            assert_eq!(fixture::text_of(item(d, gallery::SLIDER_VALUE)), "70");
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeControlsTests.MultilineEdit_KeepsEnterForItself_SoTheDialogDoesNotClose
#[test]
#[ignore = "native: needs desktop"]
fn multiline_edit_keeps_enter_for_itself_so_the_dialog_does_not_close() {
    on_sta(|| {
        let dialog = NativeDialog::new(Rc::new(ControlGalleryContent::default()));
        show_on_owner(&dialog, |d, _| {
            assert!(fixture::key(d, item(d, gallery::NOTES), fixture::VK_RETURN));
            fixture::drain(d);
            assert!(!d.is_closed());
            assert!(fixture::text_of(item(d, gallery::NOTES)).contains("Line two"));
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeControlsTests.NumberEdit_RejectsLetters_AndAcceptsDigits
#[test]
#[ignore = "native: needs desktop"]
fn number_edit_rejects_letters_and_accepts_digits() {
    on_sta(|| {
        let dialog = NativeDialog::new(Rc::new(ControlGalleryContent::default()));
        show_on_owner(&dialog, |d, _| {
            let edit = item(d, gallery::COUNT);
            // SAFETY: EM_SETSEL and WM_CHAR carry only integer values here.
            unsafe {
                dialog_native::user32::send_message(edit, dialog_native::EM_SETSEL, 2, 2);
                dialog_native::user32::send_message(edit, 0x0102, 'x' as usize, 1);
                dialog_native::user32::send_message(edit, 0x0102, '5' as usize, 1);
            }
            assert_eq!(fixture::text_of(edit), "645");
            d.close();
        })
        .unwrap();
    });
}

// C#: NativeControlsTests.Checkbox_ToggleOnClick_AndRadiosAreExclusive
#[test]
#[ignore = "native: needs desktop"]
fn checkbox_toggle_on_click_and_radios_are_exclusive() {
    on_sta(|| {
        let dialog = NativeDialog::new(Rc::new(ControlGalleryContent::default()));
        show_on_owner(&dialog, |d, _| {
            // SAFETY: BM_CLICK carries no pointer arguments and the button windows are live.
            unsafe {
                dialog_native::user32::send_message(item(d, gallery::CHECK), 0x00F5, 0, 0);
                dialog_native::user32::send_message(item(d, gallery::RADIO_B), 0x00F5, 0, 0);
            }
            assert!(!ButtonStateControl::is_checked(item(d, gallery::CHECK)));
            assert!(!ButtonStateControl::is_checked(item(d, gallery::RADIO_A)));
            assert!(ButtonStateControl::is_checked(item(d, gallery::RADIO_B)));
            d.close();
        })
        .unwrap();
    });
}
