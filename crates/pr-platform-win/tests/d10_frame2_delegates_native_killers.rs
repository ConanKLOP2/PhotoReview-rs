//! D-10 mutation killers for the `SettingsDialog` delegating methods of `dialogs/settings/dialog/frame.rs` (warn .. relayout_active_page), driven on the real
//! Settings dialog inside the real modal loop. Every test names the mutants it kills.
#![cfg(windows)]
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_app::presenters::settings::SettingsPresenter;
use pr_core::settings::app_settings::{MAX_TOOLBAR_OPACITY_PERCENT, MIN_TOOLBAR_OPACITY_PERCENT};
use pr_platform_win::dialogs::action_profiles::DialogWarningSink;
use pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder;
use pr_platform_win::dialogs::settings::dialog::SettingsDialog;
use pr_platform_win::dialogs::settings::dialog::frame::{HEADER_ID, NAV_ID, SAVE_ID, VERSION_ID, page_id_base};
use pr_platform_win::dialogs::settings::page_view::SettingsPageView;
use pr_platform_win::dialogs::settings::pages::display::{DisplayIds, DisplaySettingsPage};
use pr_platform_win::win32::dialog_native::user32;

const DISPLAY: i32 = 1;
const TBM_GETRANGEMIN: u32 = 0x0401;
const TBM_GETRANGEMAX: u32 = 0x0402;

// The remembered page is process-wide state.
static SERIAL: Mutex<()> = Mutex::new(());
static WARNINGS: Mutex<Vec<(isize, String, String)>> = Mutex::new(Vec::new());

struct Sink;
impl DialogWarningSink for Sink {
    fn show_warning(&self, owner: isize, title: &str, message: &str) {
        WARNINGS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((owner, title.to_owned(), message.to_owned()));
    }
}

/// Runs `body` inside the real modal loop with the Display page remembered; the remembered page is restored to "General" on every path.
fn run(body: impl Fn(&SettingsDialog) + Send + 'static) {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            SettingsPresenter::remember_page("General");
        }
    }
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _restore = Restore;
    SettingsPresenter::remember_page("Display");
    desktop::on_sta(move || {
        let _styles = desktop::activate_visual_styles();
        let mut args = fixture::args();
        args.options.warnings = Some(Rc::new(Sink));
        let dialog = SettingsDialog::new(args).unwrap();
        let ready = |d: &SettingsDialog| {
            body(d);
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
    });
}

fn display_ids() -> DisplayIds {
    let mut page = DisplaySettingsPage::new();
    page.declare(&mut SettingsPageBuilder::new(page_id_base(DISPLAY)));
    page.ids()
}

fn trackbar_range(d: &SettingsDialog, id: i32) -> (isize, isize) {
    let slider = d.native().item(id);
    // SAFETY: integer-only trackbar queries on a live control of this test's own dialog.
    unsafe {
        (
            user32::send_message(slider, TBM_GETRANGEMIN, 0, 0),
            user32::send_message(slider, TBM_GETRANGEMAX, 0, 0),
        )
    }
}

// Kills 991:9 `warn -> ()`: the injected warning sink receives the title and message, owned by this dialog's window.
#[test]
#[ignore = "native: needs desktop"]
fn warn_reaches_the_warning_sink_with_the_dialog_window() {
    run(|d| {
        WARNINGS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        d.warn("the title", "the message");
        let got = WARNINGS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(got, vec![(d.hwnd(), "the title".to_owned(), "the message".to_owned())]);
    });
}

// Kills 996:9 `post_to_ui -> ()`: the posted action runs on the UI thread once the queue is drained.
#[test]
#[ignore = "native: needs desktop"]
fn post_to_ui_runs_the_action_on_the_window_thread() {
    run(|d| {
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        d.post_to_ui(Box::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        desktop::drain(d.native());
        assert_eq!(count.load(Ordering::SeqCst), 1, "action posted from the UI thread ran");
    });
}

// Kills 1001:9 `refresh_localized_texts -> ()`: texts overwritten by the test are rebuilt from the localisation tables.
#[test]
#[ignore = "native: needs desktop"]
fn refresh_localized_texts_rebuilds_header_and_window_title() {
    run(|d| {
        let header = d.control_text(HEADER_ID);
        assert!(!header.is_empty());
        d.set_control_text(HEADER_ID, "zzz scribbled");
        assert_eq!(d.control_text(HEADER_ID), "zzz scribbled");
        user32::set_window_text(d.hwnd(), "zzz title").unwrap();
        d.refresh_localized_texts();
        assert_eq!(d.control_text(HEADER_ID), header, "header text restored");
        assert_ne!(user32::get_window_text(d.hwnd()), "zzz title", "window title restored");
    });
}

// Kills 1006:9 `set_control_tooltip_fn -> ()`: the navigation list gets a per-row provider that is asked on every query.
#[test]
#[ignore = "native: needs desktop"]
fn set_control_tooltip_fn_installs_a_provider_asked_each_time() {
    run(|d| {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        d.set_control_tooltip_fn(
            NAV_ID,
            Rc::new(move || {
                let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                format!("tip {n}")
            }),
        );
        assert_eq!(d.native().item_tooltip_text(NAV_ID, 0).as_deref(), Some("tip 1"));
        assert_eq!(d.native().item_tooltip_text(NAV_ID, 0).as_deref(), Some("tip 2"));
    });
}

// Kills 1011:9 `set_control_tooltip_text -> ()`: a fixed tip registers one more tool with the dialog's tooltip window.
#[test]
#[ignore = "native: needs desktop"]
fn set_control_tooltip_text_registers_a_tool() {
    run(|d| {
        let before = d.native().tooltip_tool_count();
        d.set_control_tooltip_text(VERSION_ID, "fixed tip text");
        assert_eq!(d.native().tooltip_tool_count(), before + 1);
    });
}

// Kills 1031:9 `set_control_checked -> ()`: both directions are observed on the real check box.
#[test]
#[ignore = "native: needs desktop"]
fn set_control_checked_changes_the_real_check_box() {
    let id = display_ids().show_info_overlay;
    run(move |d| {
        assert_ne!(d.native().item(id), 0);
        d.set_control_checked(id, true);
        assert!(d.is_control_checked(id));
        d.set_control_checked(id, false);
        assert!(!d.is_control_checked(id));
    });
}

// Kills 1041:9 `set_combo_selected_index -> ()`.
#[test]
#[ignore = "native: needs desktop"]
fn set_combo_selected_index_changes_the_real_selection() {
    let id = display_ids().loading_mode;
    run(move |d| {
        d.set_combo_selected_index(id, 2);
        assert_eq!(d.combo_selected_index(id), 2);
        d.set_combo_selected_index(id, 1);
        assert_eq!(d.combo_selected_index(id), 1);
    });
}

// Kills 1051:9 `slider_value -> 0 / 1 / -1`, 1056:9 `set_slider_value -> ()`: two distinct in-range values (neither 0, 1 nor -1) are written and read back.
#[test]
#[ignore = "native: needs desktop"]
fn slider_value_round_trips_two_distinct_values() {
    let id = display_ids().opacity;
    let (a, b) = (MIN_TOOLBAR_OPACITY_PERCENT + 7, MIN_TOOLBAR_OPACITY_PERCENT + 13);
    assert!(b <= MAX_TOOLBAR_OPACITY_PERCENT && a > 1);
    run(move |d| {
        d.set_slider_value(id, a);
        assert_eq!(d.slider_value(id), a);
        d.set_slider_value(id, b);
        assert_eq!(d.slider_value(id), b);
    });
}

// Kills 1061:9 `set_slider_range -> ()`: a narrower range moves the real trackbar limits and clamps the position.
#[test]
#[ignore = "native: needs desktop"]
fn set_slider_range_changes_limits_and_clamps_the_position() {
    let id = display_ids().opacity;
    run(move |d| {
        d.set_slider_range(id, 0, 100);
        d.set_slider_value(id, 70);
        assert_eq!(trackbar_range(d, id), (0, 100));
        d.set_slider_range(id, 10, 20);
        assert_eq!(trackbar_range(d, id), (10, 20));
        assert_eq!(d.slider_value(id), 20, "position clamped to the new maximum");
    });
}

// Kills 1066:9 `set_control_enabled -> ()` and 1071:9 `is_control_enabled -> true`: disable then enable, observed through a direct window query too.
#[test]
#[ignore = "native: needs desktop"]
fn control_enabled_state_round_trips_on_the_real_window() {
    let id = display_ids().font_size;
    run(move |d| {
        let edit = d.native().item(id);
        assert_ne!(edit, 0);
        d.set_control_enabled(id, false);
        assert!(!user32::is_window_enabled(edit));
        assert!(!d.is_control_enabled(id));
        d.set_control_enabled(id, true);
        assert!(user32::is_window_enabled(edit));
        assert!(d.is_control_enabled(id));
    });
}

// Kills 1086:9 `focus_control -> ()`: focus moves from the navigation list to the edit box.
#[test]
#[ignore = "native: needs desktop"]
fn focus_control_moves_the_focus() {
    let id = display_ids().font_size;
    run(move |d| {
        let edit = d.native().item(id);
        assert_ne!(edit, 0);
        user32::set_focus(d.native().item(NAV_ID));
        assert_ne!(user32::get_focus(), edit, "precondition");
        d.focus_control(id);
        assert_eq!(user32::get_focus(), edit);
    });
}

// Kills 1091:9 `relayout_active_page -> ()`: a button displaced by hand is put back by the layout pass.
#[test]
#[ignore = "native: needs desktop"]
fn relayout_active_page_restores_displaced_controls() {
    run(|d| {
        assert!(d.active_page_index() >= 0);
        let rect = |d: &SettingsDialog| {
            let r = desktop::child_rect(d.native(), SAVE_ID);
            (r.left, r.top, r.right, r.bottom)
        };
        let want = rect(d);
        let (w, h) = (want.2 - want.0, want.3 - want.1);
        user32::move_window(d.native().item(SAVE_ID), 3, 3, w, h, true).unwrap();
        assert_ne!(rect(d), want, "precondition: displaced");
        d.relayout_active_page();
        assert_eq!(rect(d), want);
    });
}
