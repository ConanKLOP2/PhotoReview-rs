//! D-10 mutation killers for `dialogs/settings/dialog/frame.rs`, driven on the real Settings dialog inside the real modal loop (no person, no foreground
//! dependence). Every test names the mutant line:col of `frame.rs` it kills (or, for equivalents, documents why none can).
#![cfg(windows)]
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use pr_app::presenters::settings::SettingsPresenter;
use pr_core::abstractions::SettingsTarget;
use pr_platform_win::dialogs::native::dialog::core::{DialogContent, ID_OK};
use pr_platform_win::dialogs::native::template::DIALOG_STYLE_CHILD;
use pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder;
use pr_platform_win::dialogs::settings::dialog::frame::{
    APPLY_ID, CANCEL_ID, DEFAULTS_ID, EXPORT_ID, HEADER_ID, IMPORT_ID, RESET_PAGE_ID, SAVE_ID, VERSION_ID,
    page_id_base, viewport_id,
};
use pr_platform_win::dialogs::settings::dialog::{SettingsDialog, SettingsDialogArgs};
use pr_platform_win::dialogs::settings::page_view::SettingsPageView;
use pr_platform_win::dialogs::settings::pages::files::FilesSettingsPage;
use pr_platform_win::win32::dialog_native::user32;
use pr_platform_win::win32::{api, messages};
use windows::Win32::UI::WindowsAndMessaging::MINMAXINFO;

// The remembered page is process-wide state.
static SERIAL: Mutex<()> = Mutex::new(());
const FILES: i32 = 5;
const GWL_STYLE: i32 = -16;

/// Runs `body` inside the real modal loop with `page` remembered; the remembered page is restored to "General" on every path (also a failed assertion).
fn run_on(
    page: &'static str,
    make: impl FnOnce() -> SettingsDialogArgs + Send + 'static,
    body: impl Fn(&SettingsDialog) + Send + 'static,
) {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            SettingsPresenter::remember_page("General");
        }
    }
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _restore = Restore;
    SettingsPresenter::remember_page(page);
    desktop::on_sta(move || {
        let _styles = desktop::activate_visual_styles();
        let dialog = SettingsDialog::new(make()).unwrap();
        let ready = |d: &SettingsDialog| {
            body(d);
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
    });
}

fn run(make: impl FnOnce() -> SettingsDialogArgs + Send + 'static, body: impl Fn(&SettingsDialog) + Send + 'static) {
    run_on("General", make, body);
}

fn editor_path_id() -> i32 {
    let mut page = FilesSettingsPage::new();
    page.declare(&mut SettingsPageBuilder::new(page_id_base(FILES)));
    page.ids().editor_path
}

fn with_target(target: SettingsTarget) -> SettingsDialogArgs {
    let mut args = fixture::args();
    args.options.target = target;
    args
}

/// Seven probe pages none of which is the Files page.
fn no_files_args() -> SettingsDialogArgs {
    let keys = [
        "General",
        "Display",
        "Mouse",
        "Performance",
        "Shortcuts",
        "NotFiles",
        "Diagnostics",
    ];
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut args = fixture::args();
    args.options.pages = Some(
        keys.iter()
            .map(|&key| {
                Box::new(fixture::ProbePage {
                    key,
                    events: events.clone(),
                }) as Box<dyn SettingsPageView>
            })
            .collect(),
    );
    args
}

// Kills 576:32 `==` -> `!=` (initialize): with the ExternalEditor target the posted action must run and focus the Files page's editor path box; with any other
// target nothing is posted, so the remembered page stays showing.
#[test]
#[ignore = "native: needs desktop"]
fn external_editor_target_posts_the_focus_and_other_targets_do_not() {
    let id = editor_path_id();
    run_on(
        "Display",
        move || with_target(SettingsTarget::ExternalEditor),
        move |d| {
            desktop::drain(d.native());
            assert_eq!(d.active_page_key(), "Files");
            let edit = d.native().item(id);
            assert_ne!(edit, 0, "the Files page controls exist");
            assert_eq!(
                user32::get_focus(),
                edit,
                "ExternalEditor target: focus must be on the editor path box"
            );
        },
    );
    run_on("Display", fixture::args, |d| {
        desktop::drain(d.native());
        assert_eq!(
            d.active_page_key(),
            "Display",
            "default target must not run FocusExternalEditor (it would show Files)"
        );
        assert!(
            !d.is_page_created(FILES as usize),
            "the Files page must not even be created"
        );
    });
}

// Kills 589:9 `-> false`, 589:81 `==` -> `!=`, 593:35 `!=` -> `==` (focus_external_editor): from another showing page it returns true, shows Files and focuses the
// editor path box.
#[test]
#[ignore = "native: needs desktop"]
fn focus_external_editor_shows_the_files_page_and_focuses_the_path_box() {
    let id = editor_path_id();
    run(fixture::args, move |d| {
        assert_eq!(d.active_page_key(), "General", "precondition");
        assert!(!d.is_page_created(FILES as usize), "precondition");
        assert!(d.focus_external_editor(), "the Files page handles it");
        assert_eq!(
            d.active_page_key(),
            "Files",
            "the Files page must become the showing page"
        );
        assert!(d.is_page_created(FILES as usize));
        let edit = d.native().item(id);
        assert_ne!(edit, 0);
        assert_eq!(user32::get_focus(), edit, "editor path box must hold the focus");
        // Already showing: still true, still focused.
        user32::set_focus(
            d.native()
                .item(pr_platform_win::dialogs::settings::dialog::frame::NAV_ID),
        );
        assert!(d.focus_external_editor());
        assert_eq!(user32::get_focus(), edit);
    });
}

// Kills 589:9 `-> true`: with no Files page nothing handles it, the answer is false and the showing page is unchanged.
#[test]
#[ignore = "native: needs desktop"]
fn focus_external_editor_without_a_files_page_is_false_and_changes_nothing() {
    run(no_files_args, |d| {
        let before = d.active_page_index();
        assert!(!d.focus_external_editor());
        assert_eq!(d.active_page_index(), before);
    });
}

// 593:69 and 594:71 (`-` of `unwrap_or(-1)`): EQUIVALENT. `i32::try_from(index)` with `index` a position in a seven-element Vec never fails, so the fallback
// literal is never evaluated; deleting its sign changes only dead code. Nothing to observe; this test pins that the conversion indeed succeeds for every page.
#[test]
#[ignore = "native: needs desktop"]
fn page_index_conversion_never_reaches_the_fallback() {
    run(fixture::args, |d| {
        for index in 0..d.page_count() {
            assert!(i32::try_from(index).is_ok());
        }
        assert_eq!(d.page_count(), 7);
    });
}

// Kills 702:9 `default_button_id -> 1`: EQUIVALENT, SAVE_ID is ID_OK is 1. Pinned so the day SAVE_ID changes the mutant becomes killable.
#[test]
#[ignore = "native: needs desktop"]
fn default_button_is_save_which_is_ok_which_is_one() {
    assert_eq!((SAVE_ID, ID_OK), (1, 1));
    run(fixture::args, |d| {
        assert_eq!(DialogContent::default_button_id(d.content().as_ref()), SAVE_ID);
    });
}

fn dlu_x(d: &SettingsDialog, units: i32) -> i32 {
    // Independent of the dialog's own dx(): avg char width = dx(4).
    (units * d.native().dx(4) + 2) / 4
}
fn dlu_y(d: &SettingsDialog, units: i32) -> i32 {
    // Independent of dy(): line height from the dialog.
    (units * d.native().line_height() + 4) / 8
}

// Kills 707:9 `preferred_client_size -> (0,0)/(0,1)/(0,-1)/(1,0)/(1,1)/(1,-1)/(-1,0)/(-1,1)/(-1,-1)`: exact pair (540 x 330 dialog units), and the live window
// was sized from it.
#[test]
#[ignore = "native: needs desktop"]
fn preferred_client_size_is_540_by_330_dialog_units() {
    run(fixture::args, |d| {
        let got = DialogContent::preferred_client_size(d.content().as_ref(), d.native());
        assert_eq!(
            got,
            (dlu_x(d, 540), dlu_y(d, 330)),
            "540 x 330 DLU from the dialog's own units"
        );
        assert!(got.0 > 1 && got.1 > 1);
        // The window was placed with that client size unless the work area clamped it.
        let (w, h) = d.native().window_size_for_client(got.0, got.1);
        if w <= api::user32::get_system_metrics(0) - 40 && h <= api::user32::get_system_metrics(1) - 120 {
            assert_eq!(d.native().current_client_size(), got, "live client rectangle");
        }
    });
}

// Kills 712:9 `minimum_client_size -> (0,0)/(0,1)/(0,-1)/(1,0)/(1,1)/(1,-1)/(-1,0)/(-1,1)/(-1,-1)`: exact pair re-derived from the real button texts, and the real
// WM_GETMINMAXINFO answer is the frame of that pair.
#[test]
#[ignore = "native: needs desktop"]
fn minimum_client_size_is_derived_from_the_button_rows() {
    run(fixture::args, |d| {
        let n = d.native();
        let button = |id: i32| dlu_x(d, 50).max(n.measure_text_width(&n.get_item_text(id), 0) + dlu_x(d, 16));
        let row = |ids: &[i32]| ids.iter().map(|&id| button(id)).sum::<i32>() + (ids.len() as i32 - 1) * dlu_x(d, 6);
        let left = row(&[DEFAULTS_ID, RESET_PAGE_ID, EXPORT_ID, IMPORT_ID]);
        let right = row(&[CANCEL_ID, APPLY_ID, SAVE_ID]);
        let want = (dlu_x(d, 360).max(left.max(right) + 2 * dlu_x(d, 10)), dlu_y(d, 230));
        let got = DialogContent::minimum_client_size(d.content().as_ref(), n);
        assert_eq!(got, want, "minimum client size");
        assert!(got.0 > 1 && got.1 > 1);
        let mut info = MINMAXINFO::default();
        // SAFETY: WM_GETMINMAXINFO with a writable MINMAXINFO that outlives the synchronous send to this test's own dialog.
        unsafe {
            user32::send_message(
                d.hwnd(),
                messages::WM_GETMINMAXINFO,
                0,
                (&mut info as *mut MINMAXINFO) as isize,
            );
        }
        assert_eq!(
            (info.ptMinTrackSize.x, info.ptMinTrackSize.y),
            n.window_size_for_client(want.0, want.1),
            "min track size is the frame of the minimum client size"
        );
    });
}

// 464:46, 484:26, 484:40, 502:36, 502:42 (`|` -> `^` in build_items): EQUIVALENT. The OR-ed constants are pairwise disjoint bit sets, so `|` and `^` agree:
// CHILD = 0x4000_0000 | 0x1000_0000, 0x80, TAB = 0x0001_0000, WS_CLIPSIBLINGS 0x0200_0000, WS_CLIPCHILDREN 0x0400_0000, default push button 1 (or 0). The one reachable
// constant (DIALOG_STYLE_CHILD) is asserted disjoint, and the real controls carry exactly those bits.
#[test]
#[ignore = "native: needs desktop"]
fn declared_style_bits_are_disjoint_and_present() {
    let (text, clip_sib, clip_child, tab, def_push) =
        (0x80_u32, 0x0200_0000_u32, 0x0400_0000_u32, 0x0001_0000_u32, 1_u32);
    assert_eq!(DIALOG_STYLE_CHILD, 0x5000_0000);
    for bit in [text, clip_sib, clip_child, tab, def_push] {
        assert_eq!(DIALOG_STYLE_CHILD & bit, 0);
    }
    for (a, b) in [
        (text, clip_sib),
        (text, clip_child),
        (clip_sib, clip_child),
        (tab, def_push),
    ] {
        assert_eq!(a & b, 0);
    }
    run(fixture::args, move |d| {
        let style = |id: i32| api::user32::get_window_long_ptr(d.native().item(id), GWL_STYLE) as u32;
        let want_text = DIALOG_STYLE_CHILD | text;
        assert_eq!(style(HEADER_ID) & want_text, want_text);
        assert_eq!(style(VERSION_ID) & want_text, want_text);
        let want_view = want_text | clip_sib | clip_child;
        assert_eq!(style(viewport_id(0)) & want_view, want_view);
        assert_eq!(style(CANCEL_ID) & (DIALOG_STYLE_CHILD | tab), DIALOG_STYLE_CHILD | tab);
        assert_eq!(style(SAVE_ID) & 0xF, def_push, "Save is the default push button");
        assert_eq!(style(CANCEL_ID) & 0xF, 0);
    });
}

// 655:9 forget_subclass_key -> (), 656:16 `!=` -> `==`, 670:9 Drop::drop -> (): NOT KILLED. The only effect is the removal of a dead `Weak` from the private
// thread-local SUBCLASS_REGISTRY; the subclass procedures also require `with_native` (the `link`, taken in the same `on_closed`), a dropped content's `Weak` no
// longer upgrades, and keys are never reused. Observing the registry size needs a test-only seam in src/, which is not allowed here.
