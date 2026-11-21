//! Killers for the survivors of the whole-file native mutation run of `shell_input/main_window.rs` (lane F, 33 open identities) that sit in the host side of
//! the file: the `MainWindowInputHost` forwarders (queries and actions), `action_profile_name` (P-433 seam), the `PreviousFolder` direction, the `Debug` of the
//! option wrappers and of the feature, the `MainWindowInputFeature` glue and `MainWindowInput::on_settings_changed`. They run on the fake-services `FeatureRig`
//! over a REAL view model, so nothing here needs a desktop. The adapter side is in `nat_main_window_input_adapter_killers.rs`.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/shell_input_dispatch_fixture.rs"]
mod fixture;
#[path = "common/input_feature_fixture.rs"]
mod input_feature_fixture;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use input_feature_fixture::{FakePrompt, FeatureRig};
use pr_app::input::key_id::KeyId;
use pr_app::input::review_command::{ReviewCommand, ReviewCommandType};
use pr_core::abstractions::{BatchReviewItem, DialogService, SettingsTarget, SkippedEntry};
use pr_core::settings::app_settings::AppSettings;
use pr_platform_win::host::contracts::ShellWindowEvent;
use pr_platform_win::host::main_window::core::MainWindowFeature;
use pr_platform_win::shell_input::main_window::{DialogServiceOption, MainWindowInputFeature, ZoomPromptOption};

fn rig_with(names: &[&str]) -> FeatureRig {
    let mut env = support::main_env::MainEnv::new();
    let (folder, _) = env.images("nat-g-host", names);
    env.open(&folder);
    FeatureRig::with_env(env)
}

fn execute(rig: &FeatureRig, kind: ReviewCommandType) {
    rig.input
        .input_host()
        .execute(ReviewCommand::new(kind))
        .expect("execute never fails here");
    rig.settle();
}

fn index(rig: &FeatureRig) -> i32 {
    rig.env.borrow().vm.current_index()
}

// ---- queries --------------------------------------------------------------------------------------------------------------------------------------

// main_window.rs 222:9 (`has_images -> true`): with no folder open there are no images.
#[test]
fn has_images_is_false_without_a_folder_and_true_with_one() {
    let empty = FeatureRig::new();
    assert!(!empty.input.input_host().has_images());
    let opened = rig_with(&["a.jpg"]);
    assert!(opened.input.input_host().has_images());
}

// main_window.rs 230:9 (`is_compare_visible -> false`) and 234:9 (`current_has_compare_pair -> true / false`).
#[test]
fn compare_queries_follow_the_view_model() {
    let single = rig_with(&["a.jpg"]);
    assert!(
        !single.input.input_host().current_has_compare_pair(),
        "one photo has no pair"
    );

    // A pair is an original and its numbered copy (`ComparePairService`).
    let rig = rig_with(&["photo.jpg", "photo (1).jpg"]);
    let host = rig.input.input_host();
    assert!(
        host.current_has_compare_pair(),
        "photo.jpg and photo (1).jpg are a pair"
    );
    // Whatever the initial state is (a pair opens in the compare view), the host reports the view model's state, and both states occur.
    let mut seen = Vec::new();
    for _ in 0..2 {
        let shown = rig.env.borrow().vm.compare().is_visible();
        assert_eq!(
            shown,
            host.is_compare_visible(),
            "the host reports what the view model shows"
        );
        seen.push(shown);
        execute(&rig, ReviewCommandType::ToggleCompare);
    }
    assert!(seen.contains(&true) && seen.contains(&false), "{seen:?}");
}

// ---- actions --------------------------------------------------------------------------------------------------------------------------------------

// main_window.rs 242:9 and 246:9 (`next`, `previous` -> `()`).
#[test]
fn next_and_previous_move_the_selection_through_the_view_model() {
    let rig = rig_with(&["a.jpg", "b.jpg", "c.jpg"]);
    let host = rig.input.input_host();
    assert_eq!(0, index(&rig));
    host.next();
    rig.settle();
    assert_eq!(1, index(&rig), "next");
    host.next();
    rig.settle();
    assert_eq!(2, index(&rig), "next again");
    host.previous();
    rig.settle();
    assert_eq!(1, index(&rig), "previous");
}

// main_window.rs 261:9 (`select_compare_side -> ()`).
#[test]
fn select_compare_side_selects_the_requested_side() {
    let rig = rig_with(&["photo.jpg", "photo (1).jpg"]);
    if !rig.env.borrow().vm.compare().is_visible() {
        execute(&rig, ReviewCommandType::ToggleCompare);
    }
    assert!(rig.env.borrow().vm.compare().is_visible());
    let host = rig.input.input_host();
    host.select_compare_side(false);
    assert!(rig.env.borrow().vm.compare().is_right_selected(), "right");
    assert!(!rig.env.borrow().vm.compare().is_left_selected());
    host.select_compare_side(true);
    assert!(rig.env.borrow().vm.compare().is_left_selected(), "left");
    assert!(!rig.env.borrow().vm.compare().is_right_selected());
}

// main_window.rs 250:9 (`cancel_duplicate_check -> false`): true only while a duplicate check hashes (the Esc key is consumed then).
#[test]
fn cancel_duplicate_check_reports_a_running_check_and_cancels_it() {
    // The two files hold the same bytes (`MainEnv::add_image`), the second one has the numbered-copy name.
    let rig = rig_with(&["photo.png", "photo (1).png"]);
    let host = rig.input.input_host();
    assert!(!host.cancel_duplicate_check(), "idle: nothing to cancel");

    rig.env.borrow_mut().vm.remove_duplicates(true);
    assert_eq!(1, rig.env.borrow().runner.count_of("Hashing"), "the check is hashing");
    assert!(host.cancel_duplicate_check(), "the host reports the running check");
    rig.settle();
    assert!(!host.cancel_duplicate_check(), "the check is over");
}

// main_window.rs 279:112 (`delete -` of the `-1` of `PreviousFolder`): the folder is the only one, so each direction ends in its own boundary status.
#[test]
fn previous_folder_asks_for_the_previous_sibling_not_the_next() {
    let mut env = support::main_env::MainEnv::new();
    let parent = env.folder("nat-g-siblings");
    let only = Path::new(&parent).join("only_one");
    std::fs::create_dir_all(&only).expect("create the folder");
    env.add_image(&only.to_string_lossy(), "pic.jpg");
    env.open(&only.to_string_lossy());
    let rig = FeatureRig::with_env(env);

    execute(&rig, ReviewCommandType::NextFolder);
    let next_status = rig.env.borrow().vm.status_text();
    assert!(!next_status.is_empty(), "the boundary of the next folder is reported");
    rig.env.borrow_mut().vm.set_status_text("");
    execute(&rig, ReviewCommandType::PreviousFolder);
    let previous_status = rig.env.borrow().vm.status_text();
    assert!(
        !previous_status.is_empty(),
        "the boundary of the previous folder is reported"
    );
    assert_ne!(next_status, previous_status, "the two directions have their own text");
    assert!(rig.statuses().is_empty(), "no action failed");
}

// ---- action_profile_name (P-433) -------------------------------------------------------------------------------------------------------------------

// main_window.rs 211:9 (`action_profile_name -> String::new() / "xyzzy"`).
#[test]
fn action_profile_name_is_the_name_of_the_profile_or_empty_outside_the_list() {
    let rig = FeatureRig::new();
    let actions = rig.env.borrow().settings.current().actions.clone();
    assert!(actions.len() >= 2, "the default profiles");
    assert!(!actions[0].name.is_empty() && actions[0].name != actions[1].name);
    assert_eq!(actions[0].name, rig.input.action_profile_name_for_test(0));
    assert_eq!(actions[1].name, rig.input.action_profile_name_for_test(1));
    let last = i32::try_from(actions.len() - 1).expect("small list");
    assert_eq!(
        actions[actions.len() - 1].name,
        rig.input.action_profile_name_for_test(last)
    );
    assert_eq!("", rig.input.action_profile_name_for_test(last + 1), "past the end");
    assert_eq!("", rig.input.action_profile_name_for_test(-1), "negative");
}

// ---- Debug ----------------------------------------------------------------------------------------------------------------------------------------

struct SilentDialog;

impl DialogService for SilentDialog {
    fn show_confirmation(&self, _title: &str, _message: &str) -> bool {
        false
    }
    fn show_message(&self, _title: &str, _message: &str) {}
    fn show_error(&self, _title: &str, _message: &str) {}
    fn pick_folder(&self, _initial_folder: Option<&Path>) -> Option<PathBuf> {
        None
    }
    fn show_batch_review(&self, _items: &[BatchReviewItem]) -> bool {
        false
    }
    fn show_recovery(&self) {}
    fn show_diagnostics(&self) {}
    fn show_settings(&self) -> bool {
        false
    }
    fn show_benchmark(&self, _folder: Option<&Path>) {}
    fn show_skipped_files(&self, _entries: &[SkippedEntry]) {}
}

// main_window.rs 613:9, 623:9, 772:9: the `Debug` of the two option wrappers and of the feature names the type and elides the fields.
#[test]
fn debug_output_names_the_option_wrappers_and_the_feature() {
    let zoom = ZoomPromptOption(FakePrompt::new(None) as Rc<dyn pr_app::services::contracts::ZoomPromptService>);
    let debug = format!("{zoom:?}");
    assert!(debug.contains("ZoomPromptOption"), "{debug:?}");
    let dialogs = DialogServiceOption(Rc::new(SilentDialog));
    let debug = format!("{dialogs:?}");
    assert!(debug.contains("DialogServiceOption"), "{debug:?}");
    let rig = FeatureRig::new();
    let feature = MainWindowInputFeature::new(Rc::clone(&rig.input));
    let debug = format!("{feature:?}");
    assert!(debug.contains("MainWindowInputFeature"), "{debug:?}");
    let _ = SettingsTarget::ExternalEditor;
}

// ---- the feature glue -----------------------------------------------------------------------------------------------------------------------------

fn kinetic_rig() -> FeatureRig {
    let rig = rig_with(&["a.jpg", "b.jpg"]);
    rig.env.borrow().update_settings(|s| s.kinetic_pan_enabled = true);
    let send = |key: KeyId| {
        let vk = pr_app::input::key_mapping::to_virtual_key(key);
        rig.window
            .dispatch(pr_platform_win::win32::messages::WM_KEYDOWN, vk as usize, 1);
        rig.drain();
    };
    send(KeyId::D1); // actual size, so an arrow key pans
    send(KeyId::RIGHT); // starts a glide
    rig
}

// main_window.rs 778:9 (`name -> "xyzzy"` and `-> ""`).
#[test]
fn the_feature_is_called_input() {
    let rig = FeatureRig::new();
    assert_eq!("input", MainWindowInputFeature::new(Rc::clone(&rig.input)).name());
}

// main_window.rs 782:9 (`dispose -> ()`): disposing the feature runs the window-closed path (the `Closed` subscription goes, the input is disposed).
#[test]
fn disposing_the_feature_runs_the_window_closed_path() {
    let rig = FeatureRig::new();
    let feature = MainWindowInputFeature::new(Rc::clone(&rig.input));
    assert_eq!(1, rig.window.subscriber_count(ShellWindowEvent::Closed));
    feature.dispose();
    assert_eq!(0, rig.window.subscriber_count(ShellWindowEvent::Closed));
    feature.dispose(); // idempotent
}

// main_window.rs 788:9 (`on_current_index_changed -> ()`): navigation stops a glide, the repeated index of a full-resolution swap does not.
#[test]
fn the_feature_forwards_a_new_index_to_the_pointer() {
    let rig = kinetic_rig();
    let feature = MainWindowInputFeature::new(Rc::clone(&rig.input));
    let running = rig.clock.subscriber_count();
    assert!(running >= 1, "a glide runs");
    feature.on_current_index_changed(7);
    assert!(
        rig.clock.subscriber_count() < running,
        "a new index stops the glide: {running} -> {}",
        rig.clock.subscriber_count()
    );
}

// main_window.rs 792:9 (`on_scroll_bar_press -> ()`): the press tunnel of a scroll bar stops a glide.
#[test]
fn the_feature_forwards_a_scroll_bar_press_to_the_pointer() {
    let rig = kinetic_rig();
    let feature = MainWindowInputFeature::new(Rc::clone(&rig.input));
    let running = rig.clock.subscriber_count();
    assert!(running >= 1, "a glide runs");
    feature.on_scroll_bar_press();
    assert!(
        rig.clock.subscriber_count() < running,
        "the press stops the glide: {running} -> {}",
        rig.clock.subscriber_count()
    );
}

// ---- on_settings_changed --------------------------------------------------------------------------------------------------------------------------

// main_window.rs 512:9 (`on_settings_changed -> ()`): the table is rebuilt from the settings handed in even when the store still holds the object the
// input saw last (so the lazy rebuild at a key press does not run). `Refresh` saves no setting, so the store's object stays the same.
#[test]
fn on_settings_changed_rebuilds_the_shortcut_table_from_the_given_settings() {
    let rig = rig_with(&["a.jpg"]);
    let send = |key: KeyId| {
        let vk = pr_app::input::key_mapping::to_virtual_key(key);
        rig.window
            .dispatch(pr_platform_win::win32::messages::WM_KEYDOWN, vk as usize, 1);
        rig.drain();
        rig.settle();
    };
    // `R` is Refresh by default; this key press makes the input remember the store's current settings object.
    let before = rig.services.invalidations.get();
    send(KeyId::R);
    assert!(rig.services.invalidations.get() > before, "R refreshes by default");

    let mut other = (*rig.env.borrow().settings.current()).clone();
    other.shortcuts.refresh = "F9".to_owned();
    let other: Arc<AppSettings> = Arc::new(other);
    rig.input.on_settings_changed(&other);

    let before = rig.services.invalidations.get();
    send(KeyId::F9);
    assert!(
        rig.services.invalidations.get() > before,
        "F9, bound by the settings handed in, refreshes"
    );
    let before = rig.services.invalidations.get();
    send(KeyId::R);
    assert_eq!(before, rig.services.invalidations.get(), "R is no longer bound");
}
