//! Selected desktop checks for W8-18r. Other C# native scenarios remain open in PARITY.md.
#![cfg(windows)]

use std::sync::Arc;

#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use fixture::{activate_visual_styles, create_owner, destroy_owner, item, on_sta, text_of};
use pr_app::presenters::benchmark::BenchmarkSessionFactory;
use pr_bench::profiles;
use pr_l10n::localizer::Localizer;
use pr_platform_win::dialogs::benchmark::layout::{FOLDER_ID, LIMIT_ID, PROFILES_ID, RUN_ID};
use pr_platform_win::dialogs::benchmark::{BenchmarkDialog, BenchmarkOptions};
use pr_platform_win::dialogs::native::controls::basic::ListBoxControl;
use pr_platform_win::win32::{api, dialog_native};

fn factory() -> BenchmarkSessionFactory {
    Arc::new(|_, _| panic!("benchmark session is not started by these desktop checks"))
}

// C#: BenchmarkDialogTests.Opens_WithoutBlocking_AndWithTheInitialValues
#[test]
#[ignore = "native: needs desktop"]
fn opens_without_blocking_and_with_the_initial_values() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let owner = create_owner();
        let dialog = BenchmarkDialog::new(
            Some("bench-fixture"),
            factory(),
            BenchmarkOptions {
                pictures_folder: Some("pictures-fixture".to_owned()),
                ..Default::default()
            },
        );
        dialog
            .native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        assert_ne!(dialog.native().hwnd(), 0);
        assert!(api::user32::is_window_visible(dialog.native().hwnd()));
        assert!(dialog_native::user32::is_window_enabled(owner));
        assert_eq!(text_of(item(dialog.native(), FOLDER_ID)), "bench-fixture");
        assert_eq!(text_of(item(dialog.native(), LIMIT_ID)), "64");
        let list = item(dialog.native(), PROFILES_ID);
        assert_eq!(ListBoxControl::count(list), profiles::all().len() as i32);
        let default_index = profiles::all().iter().position(|p| p.id == "fast-sequential").unwrap() as i32;
        assert_eq!(ListBoxControl::selected_indices(list), vec![default_index]);
        assert!(dialog_native::user32::is_window_enabled(item(dialog.native(), RUN_ID)));
        dialog.native().close();
        assert_eq!(dialog.native().hwnd(), 0);
        destroy_owner(owner);
    });
}

// C#: BenchmarkDialogTests.WithoutAFolder_TheFolderBoxStartsAtPictures (explicit folder seam)
#[test]
#[ignore = "native: needs desktop"]
fn pictures_folder_override_wins() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let owner = create_owner();
        let dialog = BenchmarkDialog::new(
            None,
            factory(),
            BenchmarkOptions {
                pictures_folder: Some("test-pictures-fixture".to_owned()),
                ..Default::default()
            },
        );
        dialog
            .native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        assert_eq!(text_of(item(dialog.native(), FOLDER_ID)), "test-pictures-fixture");
        dialog.native().close();
        destroy_owner(owner);
    });
}

// C#: BenchmarkDialogTests.EveryProfileHasATooltipWithItsDescription_AndABadgeWithItsLoadingMode
#[test]
#[ignore = "native: needs desktop"]
fn every_profile_has_a_tooltip_with_its_description() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let owner = create_owner();
        let dialog = BenchmarkDialog::new(
            None,
            factory(),
            BenchmarkOptions {
                pictures_folder: Some("pictures-fixture".to_owned()),
                ..Default::default()
            },
        );
        dialog
            .native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        let texts = Localizer::current();
        for (index, item) in dialog.presenter().borrow().profile_items().iter().enumerate() {
            assert_eq!(
                dialog.native().item_tooltip_text(PROFILES_ID, index as i32),
                Some(item.description(&texts))
            );
        }
        dialog.native().close();
        destroy_owner(owner);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn closing_twice_and_dispose_twice_are_harmless() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let owner = create_owner();
        let dialog = BenchmarkDialog::new(
            None,
            factory(),
            BenchmarkOptions {
                pictures_folder: Some("pictures-fixture".to_owned()),
                ..Default::default()
            },
        );
        dialog
            .native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        dialog.native().close();
        dialog.native().close();
        dialog.dispose();
        dialog.dispose();
        assert!(dialog.native().is_closed());
        assert_eq!(dialog.native().hwnd(), 0);
        destroy_owner(owner);
    });
}
