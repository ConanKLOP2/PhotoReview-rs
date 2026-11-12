//! D-10 mutation killers (part two) for `dialogs/settings/dialog/frame.rs`: the thread-local subclass registry, observed through the test seam
//! `frame::subclass_registry_len()` on the real Settings dialog.
#![cfg(windows)]
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

use std::sync::Mutex;

use pr_platform_win::dialogs::settings::dialog::SettingsDialog;
use pr_platform_win::dialogs::settings::dialog::frame::subclass_registry_len;

static SERIAL: Mutex<()> = Mutex::new(());

// Kills 670:9 `Drop::drop -> ()` (also 655:9 and 656:16): a content that is created and dropped without ever being shown never reaches `on_closed`, so the Drop impl is
// the only thing that can remove its entry.
#[test]
#[ignore = "native: needs desktop"]
fn dropping_a_dialog_that_was_never_shown_removes_its_registry_entry() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let baseline = subclass_registry_len();
        let dialog = SettingsDialog::new(fixture::args()).unwrap();
        assert_eq!(subclass_registry_len(), baseline + 1, "creating registers one entry");
        let second = SettingsDialog::new(fixture::args()).unwrap();
        assert_eq!(
            subclass_registry_len(),
            baseline + 2,
            "a second dialog adds a second entry"
        );
        drop(dialog);
        assert_eq!(
            subclass_registry_len(),
            baseline + 1,
            "dropping removes exactly its own entry"
        );
        drop(second);
        assert_eq!(
            subclass_registry_len(),
            baseline,
            "back to the baseline once every Rc is dropped"
        );
    });
}

// Kills 655:9 `forget_subclass_key -> ()` and 656:16 `!=` -> `==`: after the modal loop ended `on_closed` has run, so the entry is gone while the dialog (and its
// content `Rc`) is STILL ALIVE; the Drop impl cannot be what removed it.
#[test]
#[ignore = "native: needs desktop"]
fn closing_the_dialog_removes_the_registry_entry_while_the_content_is_alive() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let baseline = subclass_registry_len();
        let dialog = SettingsDialog::new(fixture::args()).unwrap();
        assert_eq!(subclass_registry_len(), baseline + 1, "creating registers one entry");
        let ready = |d: &SettingsDialog| {
            assert_eq!(subclass_registry_len(), baseline + 1, "still registered while showing");
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
        assert_eq!(
            subclass_registry_len(),
            baseline,
            "on_closed forgot the key although the content is still alive"
        );
        drop(dialog);
        assert_eq!(subclass_registry_len(), baseline, "dropping afterwards changes nothing");
    });
}
