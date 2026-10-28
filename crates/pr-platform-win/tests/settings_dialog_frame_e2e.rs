//! Port of SettingsUi/SettingsDialogTests.cs frame tests (W8-19j).
#![cfg(windows)]
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;
use pr_app::presenters::settings::SettingsPresenter;
use pr_platform_win::dialogs::settings::dialog::{
    SettingsDialog,
    frame::{NAV_ID, PAGE_TITLE_ID, viewport_id},
};
use std::rc::Rc;
use std::sync::Mutex;

// The remembered page and GUI resource counters are process-wide.
static SERIAL: Mutex<()> = Mutex::new(());

// C#: ExternalEditorTarget_OpensTheFilesPage_WithoutChangingTheRememberedPage.
#[test]
#[ignore = "native: needs desktop"]
fn external_editor_target_opens_the_files_page_without_changing_the_remembered_page() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        SettingsPresenter::remember_page("Display");
        let mut args = fixture::args();
        args.options.target = pr_core::abstractions::SettingsTarget::ExternalEditor;
        let targeted = SettingsDialog::new(args).unwrap();
        targeted
            .show_modal(
                0,
                Some(&|d| {
                    assert_eq!(d.active_page_key(), "Files");
                    d.close();
                }),
                None,
            )
            .unwrap();
        let next = fixture::dialog();
        next.show_modal(
            0,
            Some(&|d| {
                assert_eq!(d.active_page_key(), "Display");
                d.close();
            }),
            None,
        )
        .unwrap();
        SettingsPresenter::remember_page("General");
    });
}
fn modal(body: impl Fn(&SettingsDialog) + Send + 'static) {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(move || {
        let _styles = desktop::activate_visual_styles();
        let dialog = fixture::dialog();
        assert_eq!(
            dialog
                .show_modal(
                    0,
                    Some(&|d| {
                        body(d);
                        d.close();
                    }),
                    None
                )
                .unwrap(),
            2
        );
        assert!(dialog.is_closed());
    });
}

// C#: Opens_WithEveryPageInTheNavigation_OnTheRememberedPage.
#[test]
#[ignore = "native: needs desktop"]
fn opens_with_every_page_in_the_navigation_on_the_remembered_page() {
    modal(|d| {
        assert_eq!(d.page_count(), 7);
        assert_ne!(d.native().item(NAV_ID), 0);
        assert_eq!(d.active_page_index(), 0);
        assert_eq!(d.active_page_key(), "General");
        assert!(d.content().is_page_created(d.active_page_index() as usize));
    });
}

// C#: SelectingAPage_ShowsItAlone_RetitlesAndRemembersIt.
#[test]
#[ignore = "native: needs desktop"]
fn selecting_a_page_shows_it_alone_retitles_and_remembers_it() {
    modal(|d| {
        for index in 0..d.page_count() {
            d.activate_page(index as i32, true, true);
            assert_eq!(d.active_page_index(), index as i32);
            assert_eq!(d.active_page_key(), d.page_key(index).unwrap());
            assert!(!d.control_text(PAGE_TITLE_ID).is_empty());
            for other in 0..d.page_count() {
                assert_eq!(
                    desktop::is_visible(d.native(), viewport_id(other as i32)),
                    index == other
                );
            }
        }
    });
}

// C#: TheLastPage_IsRememberedForTheNextOpen.
#[test]
#[ignore = "native: needs desktop"]
fn the_last_page_is_remembered_for_the_next_open() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let first = fixture::dialog();
        first
            .show_modal(
                0,
                Some(&|d| {
                    d.activate_page(5, true, true);
                    d.close();
                }),
                None,
            )
            .unwrap();
        let second = fixture::dialog();
        second
            .show_modal(
                0,
                Some(&|d| {
                    assert_eq!(d.active_page_index(), 5);
                    d.close();
                }),
                None,
            )
            .unwrap();
        SettingsPresenter::remember_page("General");
    });
}

// C#: CtrlTab_CyclesThePages_AndWraps.
#[test]
#[ignore = "native: needs desktop"]
fn ctrl_tab_cycles_the_pages_and_wraps() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let mut args = fixture::args();
        args.options.control_key_down = Some(Rc::new(|| true));
        let dialog = SettingsDialog::new(args).unwrap();
        dialog
            .show_modal(
                0,
                Some(&|d| {
                    d.activate_page(0, false, true);
                    let msg = pr_platform_win::win32::structs::Msg {
                        message: 0x100,
                        wParam: windows::Win32::Foundation::WPARAM(9),
                        ..Default::default()
                    };
                    for expected in (1..d.page_count()).chain(std::iter::once(0)) {
                        assert!(d.handle_key_for_test(&msg));
                        assert_eq!(d.active_page_index(), expected as i32);
                    }
                    d.close();
                }),
                None,
            )
            .unwrap();
    });
}

// C#: PlainTab_IsLeftToTheDialogManager.
#[test]
#[ignore = "native: needs desktop"]
fn plain_tab_is_left_to_the_dialog_manager() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let mut args = fixture::args();
        args.options.control_key_down = Some(Rc::new(|| false));
        let dialog = SettingsDialog::new(args).unwrap();
        dialog
            .show_modal(
                0,
                Some(&|d| {
                    let before = d.active_page_index();
                    let msg = pr_platform_win::win32::structs::Msg {
                        message: 0x100,
                        wParam: windows::Win32::Foundation::WPARAM(9),
                        ..Default::default()
                    };
                    assert!(!d.handle_key_for_test(&msg));
                    assert_eq!(d.active_page_index(), before);
                    d.close();
                }),
                None,
            )
            .unwrap();
    });
}

// C#: OnlyTheShowingPagesControlsExist_UntilAnotherPageIsOpened.
#[test]
#[ignore = "native: needs desktop"]
fn only_the_showing_pages_controls_exist_until_another_page_is_opened() {
    modal(|d| {
        let active = d.active_page_index() as usize;
        for index in 0..d.page_count() {
            assert_eq!(d.content().is_page_created(index), index == active);
        }
        let next = (active + 1) % d.page_count();
        d.activate_page(next as i32, false, true);
        for index in 0..d.page_count() {
            assert_eq!(d.content().is_page_created(index), index == active || index == next);
        }
    });
}

// C#: ARealPageIsBuilt_ForEveryNavigationEntry_AndPendingOnesAreAnnounced.
#[test]
#[ignore = "native: needs desktop and all real Settings pages"]
fn a_real_page_is_built_for_every_navigation_entry_and_pending_ones_are_announced() {
    modal(|d| {
        d.ensure_all_pages_created();
        for index in 0..d.page_count() {
            assert!(d.content().is_page_created(index));
            assert!(!d.page_items(index).is_empty());
            d.activate_page(index as i32, false, true);
            assert!(d.content_height(index) > 0);
        }
    });
}

// C#: Chrome_IsApplied_ToTheFrame_AndThenToEachPageWhenItsControlsAreCreated.
#[test]
#[ignore = "native: needs desktop"]
fn chrome_is_applied_to_the_frame_and_then_to_each_page_when_its_controls_are_created() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let chrome = Rc::new(desktop::RecordingChrome::default());
        let mut args = fixture::args();
        args.chrome = Some(chrome.clone());
        let dialog = SettingsDialog::new(args).unwrap();
        dialog
            .show_modal(
                0,
                Some(&|d| {
                    assert_eq!(chrome.apply_calls.borrow().len(), 1);
                    let before = chrome.apply_controls_calls.borrow().len();
                    d.ensure_all_pages_created();
                    assert!(chrome.apply_controls_calls.borrow().len() >= before + d.page_count() - 1);
                    d.close();
                }),
                None,
            )
            .unwrap();
    });
}

// C#: ModalDisablesTheOwner_AndRestoresItOnClose.
#[test]
#[ignore = "native: needs desktop"]
fn modal_disables_the_owner_and_restores_it_on_close() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let owner = desktop::create_owner();
        let dialog = fixture::dialog();
        dialog
            .show_modal(
                owner,
                Some(&|d| {
                    assert!(!pr_platform_win::win32::dialog_native::user32::is_window_enabled(owner));
                    d.close();
                }),
                None,
            )
            .unwrap();
        assert!(pr_platform_win::win32::dialog_native::user32::is_window_enabled(owner));
        desktop::destroy_owner(owner);
    });
}

// C#: OpeningAndClosing_DoesNotLeakGdiOrUserObjects.
#[test]
#[ignore = "native: needs desktop; GUI resource counts require exclusive process use"]
fn opening_and_closing_does_not_leak_gdi_or_user_objects() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let cycle = || {
            let dialog = fixture::dialog();
            dialog
                .show_modal(
                    0,
                    Some(&|d| {
                        d.ensure_all_pages_created();
                        d.close();
                    }),
                    None,
                )
                .unwrap();
        };
        cycle(); // Warm process-wide common-controls/font caches before measuring.
        let before = (desktop::gdi_objects(), desktop::user_objects());
        for _ in 0..5 {
            cycle();
        }
        let after = (desktop::gdi_objects(), desktop::user_objects());
        assert!(
            after.0 <= before.0 && after.1 <= before.1,
            "GUI resources: {before:?} -> {after:?}"
        );
    });
}
