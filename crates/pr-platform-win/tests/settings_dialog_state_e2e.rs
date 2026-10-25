//! Real Settings controls and lazy-page lifetime (C# SettingsDialogTests; W8-19j/k).
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

#[test]
#[ignore = "native: needs desktop"]
fn every_page_creates_controls_and_closing_releases_the_content() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let dialog = fixture::dialog();
        let weak = std::rc::Rc::downgrade(dialog.content());
        let ready = |d: &pr_platform_win::dialogs::settings::dialog::SettingsDialog| {
            assert_eq!(d.page_count(), 7);
            assert!(d.active_page_index() >= 0);
            d.ensure_all_pages_created();
            for i in 0..7 {
                assert!(d.content().is_page_created(i));
                d.activate_page(i as i32, false, true);
                assert_eq!(d.active_page_index(), i as i32);
                assert!(d.viewport_height() > 0);
                assert!(d.content_height(i) > 0, "page {i} has no layout");
            }
            assert_ne!(d.native().item(5006), 0, "shortcut key box was not created");
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
        assert!(dialog.is_closed());
        drop(dialog);
        assert!(
            weak.upgrade().is_none(),
            "Settings content still owns a native-dialog cycle"
        );
    });
}

// C#: APressedKey_IsWrittenAsItsCanonicalName_ThroughTheBoxSubclass.
#[test]
#[ignore = "native: needs desktop"]
fn pressed_keys_reach_the_real_subclass_and_ime_characters_cannot_replace_them() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let dialog = fixture::dialog();
        let ready = |d: &pr_platform_win::dialogs::settings::dialog::SettingsDialog| {
            d.activate_page(4, false, true);
            let control = d.native().item(5006);
            assert_ne!(control, 0);
            let mut captured = 0;
            for extended in [false, true] {
                for vk in 0..=255 {
                    let Some(expected) = pr_platform_win::dialogs::settings::dialog::keybox::key_box_text(vk, extended)
                    else {
                        continue;
                    };
                    // SAFETY: synchronous key message uses integer virtual-key and extended-bit arguments to this test's own control.
                    unsafe {
                        pr_platform_win::win32::dialog_native::user32::send_message(
                            control,
                            0x0100,
                            vk as usize,
                            if extended { 0x0100_0000 } else { 0 },
                        );
                    }
                    assert_eq!(d.control_text(5006), expected, "vk={vk}, extended={extended}");
                    captured += 1;
                }
            }
            assert!(captured >= 300);
            for message in [
                0x102, 0x106, 0x103, 0x107, 0x286, 0x10d, 0x10f, 0x302, 0x300, 0x303, 0x304,
            ] {
                d.set_control_text(5006, "A");
                // SAFETY: only integer character/edit messages; no clipboard data is requested because the subclass must swallow them.
                unsafe {
                    pr_platform_win::win32::dialog_native::user32::send_message(control, message, 66, 0);
                }
                assert_eq!(d.control_text(5006), "A", "message={message:#x}");
            }
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
    });
}
