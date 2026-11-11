//! D-10 host.rs mutation killers: the real Settings dialog is driven through the `PageHost` adapter (the `SettingsPageHost` impl the pages see) and the
//! effect is observed on real Win32 state or on the dialog's own accessors. Every test names the mutant line:col of `dialog/host.rs` it kills.
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use pr_core::model::enums::DecoderBackend;
use pr_core::settings::external_editors::ExternalEditor;
use pr_imaging::decoder_traits::{ImageDecoder, ImageDecoderFactory};
use pr_platform_win::dialogs::file_dialog::{FileDialogFilter, ShellFileDialog};
use pr_platform_win::dialogs::native::controls::basic::ComboBoxControl;
use pr_platform_win::dialogs::native::external_editors_dialog::ExternalEditorsEdit;
use pr_platform_win::dialogs::settings::context::SettingsPageHost;
use pr_platform_win::dialogs::settings::dialog::frame::SAVE_ID;
use pr_platform_win::dialogs::settings::dialog::host::PageHost;
use pr_platform_win::dialogs::settings::dialog::{SettingsDialog, SettingsDialogArgs};
use pr_platform_win::win32::structs::{Hresult, Hwnd};
use pr_platform_win::win32::{api, dialog_native::user32};

const COMBO: i32 = 1010; // General page, view-mode combo.
const CHECK: i32 = 2002; // Display page, checkbox.
const CHECK2: i32 = 2003;
const SLIDER: i32 = 2014; // Display page, toolbar opacity trackbar.
const KEY_BOX: i32 = 5006; // Shortcuts page, first key box.

type Seen = Vec<(Hwnd, Option<String>)>;

struct NoFiles(Rc<Cell<u32>>);
impl ShellFileDialog for NoFiles {
    fn pick_open_file(
        &self,
        _: Hwnd,
        _: &str,
        _: &[FileDialogFilter],
        _: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        self.0.set(self.0.get() + 1);
        Ok(None)
    }
    fn pick_save_file(
        &self,
        _: Hwnd,
        _: &str,
        _: &[FileDialogFilter],
        _: Option<&str>,
        _: Option<&str>,
        _: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        Ok(None)
    }
}

struct FakeFactory;
impl ImageDecoderFactory for FakeFactory {
    fn create(&self, _: DecoderBackend) -> Arc<dyn ImageDecoder> {
        unreachable!("the host never creates a decoder")
    }
    fn is_registered(&self, _: DecoderBackend) -> bool {
        true
    }
}

/// Builds the dialog on the test thread (args are not `Send`), runs `body` with a `PageHost` over its content inside the real modal loop, then closes it.
fn on_dialog_thread(
    make: impl FnOnce() -> SettingsDialogArgs + Send + 'static,
    body: impl Fn(&SettingsDialog, &PageHost) + Send + 'static,
) {
    desktop::on_sta(move || {
        let _styles = desktop::activate_visual_styles();
        let dialog = SettingsDialog::new(make()).unwrap();
        let ready = |d: &SettingsDialog| {
            d.ensure_all_pages_created();
            let host = PageHost::new(Rc::downgrade(d.content()));
            body(d, &host);
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
    });
}

fn plain(body: impl Fn(&SettingsDialog, &PageHost) + Send + 'static) {
    on_dialog_thread(fixture::args, body);
}

fn rect(hwnd: Hwnd) -> (i32, i32, i32, i32) {
    let r = api::user32::get_window_rect(hwnd).unwrap();
    (r.left, r.top, r.right, r.bottom)
}

// Kills 87:9 Debug::fmt -> Ok(Default::default()): the mutant prints nothing.
#[test]
#[ignore = "native: needs desktop"]
fn debug_names_the_adapter() {
    plain(|_, host| assert_eq!(format!("{host:?}"), "PageHost { .. }"));
}

// Kills 121:9 hwnd -> Default::default() (0): the adapter must answer the real window.
#[test]
#[ignore = "native: needs desktop"]
fn hwnd_is_the_live_dialog_window() {
    plain(|d, host| {
        assert_ne!(d.hwnd(), 0);
        assert_eq!(host.hwnd(), d.hwnd());
        assert!(user32::is_window(host.hwnd()));
    });
}

// Kills 126:9 files -> None: the forwarded object must be the option object itself (its pick call is observed).
#[test]
#[ignore = "native: needs desktop"]
fn files_is_the_option_object() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let picks = Rc::new(Cell::new(0));
        let mut args = fixture::args();
        args.options.files = Some(Rc::new(NoFiles(picks.clone())));
        let dialog = SettingsDialog::new(args).unwrap();
        let ready = |d: &SettingsDialog| {
            let host = PageHost::new(Rc::downgrade(d.content()));
            let files = host.files().expect("files option must be forwarded");
            assert_eq!(files.pick_open_file(0, "t", &[], None), Ok(None));
            assert_eq!(picks.get(), 1);
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
    });
}

// Kills 131:9 decoder_factory -> None.
#[test]
#[ignore = "native: needs desktop"]
fn decoder_factory_is_the_dialog_factory() {
    on_dialog_thread(
        || {
            let mut args = fixture::args();
            args.decoder_factory = Some(Arc::new(FakeFactory));
            args
        },
        |_, host| {
            let factory = host.decoder_factory().expect("decoder factory must be forwarded");
            assert!(factory.is_registered(DecoderBackend::default()));
        },
    );
}

// Kills 136:9 version -> None.
#[test]
#[ignore = "native: needs desktop"]
fn version_is_the_option_text() {
    on_dialog_thread(
        || {
            let mut args = fixture::args();
            args.options.version = Some("9.8.7-d10".to_owned());
            args
        },
        |_, host| assert_eq!(host.version().as_deref(), Some("9.8.7-d10")),
    );
}

// Kills 180:9 show_external_editors -> Ok(None): the callback result and its arguments must pass through.
#[test]
#[ignore = "native: needs desktop"]
fn show_external_editors_forwards_arguments_and_result() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let editor = ExternalEditor {
            id: "0123456789abcdef0123456789abcdef".to_owned(),
            name: "Ed".to_owned(),
            path: "C:/ed.exe".to_owned(),
            arguments: "{file}".to_owned(),
        };
        let edit = ExternalEditorsEdit {
            editors: vec![editor.clone()],
            default_id: editor.id.clone(),
        };
        let seen: Rc<RefCell<Seen>> = Rc::default();
        let (seen_in, edit_in, editor_in) = (seen.clone(), edit.clone(), editor.clone());
        let mut args = fixture::args();
        args.options.show_external_editors = Some(Rc::new(move |owner, editors, default_id| {
            seen_in.borrow_mut().push((owner, default_id.map(str::to_owned)));
            assert_eq!(editors, std::slice::from_ref(&editor_in));
            Ok(Some(edit_in.clone()))
        }));
        let dialog = SettingsDialog::new(args).unwrap();
        let ready = |d: &SettingsDialog| {
            let host = PageHost::new(Rc::downgrade(d.content()));
            let got = host.show_external_editors(77, std::slice::from_ref(&editor), Some("abc"));
            assert_eq!(got.unwrap(), Some(edit.clone()));
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
        assert_eq!(&*seen.borrow(), &[(77, Some("abc".to_owned()))]);
    });
}

// Kills 220:9 set_combo_items -> ().
#[test]
#[ignore = "native: needs desktop"]
fn set_combo_items_replaces_the_list_and_selects() {
    plain(|d, host| {
        d.activate_page(0, false, true);
        let combo = d.native().item(COMBO);
        assert_ne!(combo, 0);
        assert_ne!(ComboBoxControl::count(combo), 3, "precondition: the page list differs");
        host.set_combo_items(COMBO, &["x".to_owned(), "y".to_owned(), "z".to_owned()], 2);
        assert_eq!(ComboBoxControl::count(combo), 3);
        assert_eq!(ComboBoxControl::text_at(combo, 1), "y");
        assert_eq!(host.combo_selected_index(COMBO), 2);
    });
}

// Kills 235:9 set_slider_range -> ().
#[test]
#[ignore = "native: needs desktop"]
fn set_slider_range_reconfigures_the_trackbar() {
    plain(|d, host| {
        d.activate_page(1, false, true);
        let bar = d.native().item(SLIDER);
        assert_ne!(bar, 0);
        // SAFETY: integer-only TBM_GET* messages to this test's own live trackbar.
        let get = |m: u32| unsafe { user32::send_message(bar, m, 0, 0) } as i32;
        assert_ne!((get(0x0401), get(0x0402)), (7, 97), "precondition: range differs");
        host.set_slider_range(SLIDER, 7, 97);
        assert_eq!((get(0x0401), get(0x0402)), (7, 97));
        assert_eq!(get(0x0416), 9, "page size is (max - min) / 10");
        assert!((7..=97).contains(&host.slider_value(SLIDER)));
    });
}

// Kills 240:9 set_control_tooltip -> ().
#[test]
#[ignore = "native: needs desktop"]
fn set_control_tooltip_registers_the_text_provider() {
    plain(|d, host| {
        d.activate_page(1, false, true);
        assert_ne!(d.native().item(CHECK), 0);
        assert_ne!(d.native().item_tooltip_text(CHECK, 0).as_deref(), Some("d10 tip"));
        host.set_control_tooltip(CHECK, Rc::new(|| "d10 tip".to_owned()));
        assert_eq!(d.native().item_tooltip_text(CHECK, 0).as_deref(), Some("d10 tip"));
    });
}

// Kills 245:9 current_shortcut_texts -> BTreeMap::new().
#[test]
#[ignore = "native: needs desktop"]
fn current_shortcut_texts_are_the_collected_texts() {
    plain(|d, host| {
        d.set_control_text(KEY_BOX, "Q");
        let want = d.collect_all().shortcut_texts;
        assert!(!want.is_empty());
        assert!(want.values().any(|v| v == "Q"), "typed key must be collected");
        assert_eq!(host.current_shortcut_texts(), want);
    });
}

// Kills 250:9 set_control_enabled -> () and 255:9 is_control_enabled -> false.
#[test]
#[ignore = "native: needs desktop"]
fn enable_state_round_trips_on_the_real_window() {
    plain(|d, host| {
        d.activate_page(1, false, true);
        let control = d.native().item(CHECK);
        assert_ne!(control, 0);
        assert!(user32::is_window_enabled(control));
        assert!(host.is_control_enabled(CHECK));
        host.set_control_enabled(CHECK, false);
        assert!(!user32::is_window_enabled(control));
        assert!(!host.is_control_enabled(CHECK));
        host.set_control_enabled(CHECK, true);
        assert!(user32::is_window_enabled(control));
        assert!(host.is_control_enabled(CHECK));
    });
}

// Kills 260:9 set_control_visible -> ().
#[test]
#[ignore = "native: needs desktop"]
fn set_control_visible_hides_and_shows_the_control() {
    plain(|d, host| {
        d.activate_page(1, false, true);
        let control = d.native().item(CHECK);
        assert!(d.is_control_visible(CHECK));
        assert!(api::user32::is_window_visible(control));
        host.set_control_visible(CHECK, false);
        assert!(!d.is_control_visible(CHECK));
        assert!(!api::user32::is_window_visible(control));
        host.set_control_visible(CHECK, true);
        assert!(d.is_control_visible(CHECK));
        assert!(api::user32::is_window_visible(control));
    });
}

// Kills 265:9 focus_control -> ().
#[test]
#[ignore = "native: needs desktop"]
fn focus_control_moves_the_thread_focus() {
    plain(|d, host| {
        d.activate_page(1, false, true);
        let (a, b) = (d.native().item(CHECK), d.native().item(CHECK2));
        assert_ne!(a, b);
        user32::set_focus(a);
        assert_eq!(user32::get_focus(), a, "precondition: focus is settable on this thread");
        host.focus_control(CHECK2);
        assert_eq!(user32::get_focus(), b);
    });
}

// Kills 270:9 relayout_active_page -> ().
#[test]
#[ignore = "native: needs desktop"]
fn relayout_active_page_restores_the_layout() {
    plain(|d, host| {
        d.activate_page(0, false, true);
        let save = d.native().item(SAVE_ID);
        let before = rect(save);
        // Park the Save button elsewhere; only a layout pass can put it back.
        user32::move_window(save, 5, 5, 20, 20, true).unwrap();
        assert_ne!(rect(save), before);
        host.relayout_active_page();
        assert_eq!(rect(save), before);
    });
}

// Kills 275:9 warn -> ().
#[test]
#[ignore = "native: needs desktop"]
fn warn_reaches_the_warning_sink_with_title_and_message() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let log: Rc<fixture::Warnings> = Rc::default();
        let mut args = fixture::args();
        args.options.warnings = Some(log.clone());
        let dialog = SettingsDialog::new(args).unwrap();
        let ready = |d: &SettingsDialog| {
            let host = PageHost::new(Rc::downgrade(d.content()));
            assert!(log.0.borrow().is_empty());
            host.warn("d10 title", "d10 message");
            assert_eq!(&*log.0.borrow(), &[("d10 title".to_owned(), "d10 message".to_owned())]);
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
    });
}

// Kills 288:9 actions_json -> String::new(), 293:9 refresh_actions_json_from_pages -> String::new(), 300:9 set_actions_json -> ().
#[test]
#[ignore = "native: needs desktop"]
fn actions_json_accessors_forward_to_the_working_copy() {
    plain(|d, host| {
        let initial = d.actions_json();
        assert!(!initial.is_empty(), "precondition: default actions JSON is not empty");
        assert_eq!(host.actions_json(), initial);
        let refreshed = host.refresh_actions_json_from_pages();
        assert!(!refreshed.is_empty());
        assert_eq!(refreshed, d.collect_all().actions_json);
        let custom = r#"{"d10":"marker"}"#;
        assert_ne!(initial, custom);
        host.set_actions_json(custom, "");
        assert_eq!(d.actions_json(), custom);
        assert_eq!(host.actions_json(), custom);
    });
}
