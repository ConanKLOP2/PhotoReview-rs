//! Real page scrolling and combo-wheel ownership, SettingsDialogTests (W8-19m).
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;
use pr_platform_win::dialogs::settings::dialog::{
    SettingsDialog,
    frame::{SCROLL_ID, viewport_id},
    page_layout::{clamp_scroll, wheel_target},
};

#[test]
#[ignore = "native: needs desktop"]
fn page_combobox_wheel_scrolls_the_page_until_its_dropdown_is_open() {
    desktop::on_sta(|| {
        use pr_platform_win::dialogs::native::{controls::basic::ComboBoxControl, template::DialogControlClass};
        let _styles = desktop::activate_visual_styles();
        let dialog = fixture::dialog();
        let ready = |d: &SettingsDialog| {
            d.activate_page(1, false, true);
            assert!(
                d.content_height(1) > d.viewport_height(),
                "Display page must exercise real scrolling"
            );
            let combo_id = d
                .page_items(1)
                .iter()
                .find(|item| item.class == DialogControlClass::ComboBox)
                .unwrap()
                .id;
            let combo = d.native().item(combo_id);
            assert_ne!(combo, 0);
            let current = d.scroll_offset(1);
            let expected = clamp_scroll(
                wheel_target(current, -120, d.native().line_height()),
                d.content_height(1),
                d.viewport_height(),
            );
            assert_ne!(current, expected);
            // SAFETY: integer-only wheel message sent synchronously to the test-owned combo subclass.
            unsafe {
                pr_platform_win::win32::dialog_native::user32::send_message(
                    combo,
                    0x020a,
                    usize::from((-120_i16) as u16) << 16,
                    0,
                );
            }
            assert_eq!(d.scroll_offset(1), expected);
            let mut info = pr_platform_win::win32::structs::ScrollInfo {
                fMask: windows::Win32::UI::WindowsAndMessaging::SIF_POS,
                ..Default::default()
            };
            assert!(pr_platform_win::win32::settings_native::get_scroll_info(
                d.native().item(SCROLL_ID),
                2,
                &mut info
            ));
            assert_eq!(info.nPos, expected);
            d.ensure_visible(combo_id);
            ComboBoxControl::drop(combo, true);
            assert!(ComboBoxControl::is_dropped(combo));
            let current = d.scroll_offset(1);
            // SAFETY: same integer-only wheel message; the expanded combo must keep it instead of forwarding to the page.
            unsafe {
                pr_platform_win::win32::dialog_native::user32::send_message(
                    combo,
                    0x020a,
                    usize::from((-120_i16) as u16) << 16,
                    0,
                );
            }
            assert_eq!(d.scroll_offset(1), current);
            ComboBoxControl::drop(combo, false);
            let last = d.page_items(1).last().unwrap().id;
            d.ensure_visible(last);
            let rect = pr_platform_win::win32::api::user32::get_window_rect(d.native().item(last)).unwrap();
            let mut point = pr_platform_win::win32::structs::Point {
                x: rect.left,
                y: rect.top,
            };
            assert!(pr_platform_win::win32::dialog_native::user32::screen_to_client(
                d.native().item(viewport_id(1)),
                &mut point
            ));
            assert!(point.y >= 0, "last control above viewport: {}", point.y);
            assert!(
                point.y + rect.bottom - rect.top <= d.viewport_height(),
                "last control below viewport"
            );
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
    });
}
