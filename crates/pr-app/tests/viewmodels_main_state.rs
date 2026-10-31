//! Red tests for W8-08a. Each assertion describes the C# behavior or a contract decision.

mod file_action_fakes;
#[path = "support/main_vm.rs"]
mod main_vm;
mod support {
    pub mod main_env;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use file_action_fakes::EnglishTexts;
use pr_app::coordinators::folder_load::state::{WorkerLane, WorkerPort};
use pr_app::coordinators::presenter::state::PresenterChanges;
use pr_app::coordinators::traits::IFolderLoadSink;
use pr_app::coordinators::zoom_detail::CompletionInbox;
use pr_app::viewmodels::compare::CompareProperty;
use pr_app::viewmodels::info_overlay::FolderInfoLabels;
use pr_app::viewmodels::info_overlay::InfoOverlayProperty;
use pr_app::viewmodels::main::MainFolderInfoLabels;
use pr_app::viewmodels::main::{
    ChangeSet, ImageChangingEventArgs, MainChanges, MainEvent, MainProperty, MainWorkerPort,
};
use pr_app::viewmodels::viewer_state::ViewerProperty;
use pr_core::abstractions::{SkippedEntry, SkippedKind};
use pr_core::model::enums::InitialViewMode;
use pr_core::model::{LoadingMode, RawFullDecode, RawPairMode};
use pr_core::settings::app_settings::AppSettings;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_l10n::template::LocArg;
use support::main_env::{EnvOptions, MainEnv};
use support::presenter_fakes::FormatFixture;
use support::preview_fakes::{Gate, sized_image};

#[test]
fn main_changes_union_preserves_members_shared_by_both_inputs() {
    // New test: kills MainChanges::union replace | with ^.
    let mut left = MainChanges::EMPTY;
    left.insert(MainProperty::StatusText);
    left.insert(MainProperty::FolderTitle);
    let mut right = MainChanges::EMPTY;
    right.insert(MainProperty::StatusText);
    right.insert(MainProperty::FolderText);
    assert_eq!(
        left.union(right).iter().collect::<Vec<_>>(),
        vec![
            MainProperty::StatusText,
            MainProperty::FolderTitle,
            MainProperty::FolderText,
        ]
    );
}

#[test]
fn main_changes_iter_exposes_empty_singleton_and_all_properties() {
    // New test: kills all three MainChanges::iter replacement iterators.
    assert_eq!(MainChanges::EMPTY.iter().count(), 0);
    for property in MainProperty::ALL {
        let mut one = MainChanges::EMPTY;
        one.insert(property);
        assert_eq!(one.iter().collect::<Vec<_>>(), vec![property]);
    }
    let mut all = MainChanges::EMPTY;
    for property in MainProperty::ALL {
        all.insert(property);
    }
    assert_eq!(all.iter().collect::<Vec<_>>(), MainProperty::ALL);
}

#[test]
fn change_set_is_empty_checks_each_component_independently() {
    // New test: kills ChangeSet::is_empty true, false, and each of the six && -> || mutants.
    assert!(ChangeSet::default().is_empty());
    let mut cases = Vec::new();
    cases.push(ChangeSet::of(MainProperty::StatusText));
    let mut viewer = ChangeSet::default();
    viewer.viewer.insert(ViewerProperty::Zoom);
    cases.push(viewer);
    let mut overlay = ChangeSet::default();
    overlay.overlay.insert(InfoOverlayProperty::FontSize);
    cases.push(overlay);
    let mut compare = ChangeSet::default();
    compare.compare.insert(CompareProperty::LeftPath);
    cases.push(compare);
    let presenter = ChangeSet {
        presenter: PresenterChanges::STATUS,
        ..ChangeSet::default()
    };
    cases.push(presenter);
    let image = ChangeSet {
        image_changing: Some(ImageChangingEventArgs { is_file_change: true }),
        ..ChangeSet::default()
    };
    cases.push(image);
    let catalog = ChangeSet {
        catalog_changed: true,
        ..ChangeSet::default()
    };
    cases.push(catalog);
    for (index, changes) in cases.iter().enumerate() {
        assert!(
            !changes.is_empty(),
            "component {index} alone must make the set nonempty"
        );
    }
}

#[test]
fn bridge_viewer_returns_the_shared_live_handle() {
    // New test: kills PresentationBridge::viewer replace ViewerHandle with Default::default().
    let (bridge, viewer) = main_vm::bridge();
    viewer.borrow_mut().set_source_size(1234, 567, true);
    assert_eq!(bridge.viewer().borrow().source_pixel_width(), 1234);
    bridge.viewer().borrow_mut().set_source_size(77, 88, true);
    assert_eq!(viewer.borrow().source_pixel_height(), 88);
}

#[test]
fn bridge_source_size_swap_hook_can_be_replaced_and_cleared() {
    // New test: kills PresentationBridge::set_source_size_swapping_hook with ().
    let (bridge, viewer) = main_vm::bridge();
    bridge.on_set_current_image(&main_vm::image_change(Some("a.jpg"), 640, 480, false), true);
    viewer.borrow_mut().zoom_to_actual_size();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let first = calls.clone();
    bridge.set_source_size_swapping_hook(Some(Box::new(move |state| {
        first
            .borrow_mut()
            .push((state.source_pixel_width(), state.source_pixel_height()));
    })));
    bridge.on_set_current_image(&main_vm::image_change(Some("a.jpg"), 1280, 960, true), false);
    assert_eq!(&*calls.borrow(), &[(640, 480)]);
    bridge.set_source_size_swapping_hook(None);
    bridge.on_set_current_image(&main_vm::image_change(Some("a.jpg"), 1920, 1080, true), false);
    assert_eq!(&*calls.borrow(), &[(640, 480)]);
    assert_eq!(viewer.borrow().source_pixel_width(), 1920);
}

#[test]
fn main_view_model_initial_accessors_return_the_owned_collaborators() {
    // New test: kills MainViewModel::metrics, compare, viewer, bridge, catalog, session and preload_controller replacement returns.
    let env = MainEnv::new();
    assert!(std::sync::Arc::ptr_eq(env.vm.metrics(), &env.metrics));
    assert!(std::ptr::eq(env.vm.compare(), env.vm.presenter().compare()));
    env.viewer.borrow_mut().set_source_size(345, 234, true);
    assert_eq!(env.vm.viewer().borrow().source_pixel_width(), 345);
    env.vm.bridge().viewer().borrow_mut().set_source_size(231, 232, true);
    assert_eq!(env.viewer.borrow().source_pixel_width(), 231);
    assert_eq!(env.vm.catalog().count(), 0);
    assert!(env.vm.session().is_none());
    assert!(env.vm.preload_controller().is_some());
}

#[test]
fn main_view_model_default_status_and_settings_are_exact() {
    // New test: kills MainViewModel::settings, is_status_panel_visible, folder_title, folder_text, is_explorer_order_applied, status_text, status_needs_attention, skipped_entries, has_skipped_entries and skipped_warning_text replacement returns.
    let env = MainEnv::new();
    assert_eq!(env.vm.settings(), env.store.current());
    assert_eq!(env.vm.folder_title(), env.text("app.title"));
    assert_eq!(env.vm.folder_text(), "");
    assert!(!env.vm.is_explorer_order_applied());
    assert_eq!(env.vm.status_text(), env.vm.presenter().status_text());
    assert!(!env.vm.status_needs_attention());
    assert!(env.vm.skipped_entries().is_empty());
    assert!(!env.vm.has_skipped_entries());
    assert_eq!(env.vm.skipped_warning_text(), "");
    assert_eq!(env.vm.is_status_panel_visible(), env.vm.settings().show_info_overlay);
}

#[test]
fn status_text_setter_reports_only_actual_changes_and_preserves_the_value() {
    // New test: kills MainViewModel::set_status_text replacement and == -> !=.
    // New test: kills MainViewModel::status_text constant returns, delete !, and status_needs_attention constant/! mutants.
    let mut env = MainEnv::new();
    assert!(
        env.vm
            .set_status_text("Operation completed")
            .main
            .contains(MainProperty::StatusText)
    );
    assert_eq!(env.vm.status_text(), "Operation completed");
    assert!(env.vm.status_needs_attention());
    assert!(env.vm.set_status_text("Operation completed").is_empty());
    assert!(env.vm.set_status_text("").main.contains(MainProperty::StatusText));
    assert!(!env.vm.status_needs_attention());
}

#[test]
fn main_changes_iterate_in_declaration_order() {
    let mut changes = MainChanges::EMPTY;
    changes.insert(MainProperty::FolderText);
    changes.insert(MainProperty::StatusText);
    changes.insert(MainProperty::CapturePairBadge);
    changes.insert(MainProperty::StatusText);
    assert_eq!(
        changes.iter().collect::<Vec<_>>(),
        vec![
            MainProperty::StatusText,
            MainProperty::FolderText,
            MainProperty::CapturePairBadge
        ]
    );
    assert!(!changes.is_empty());
    assert_eq!(MainChanges::EMPTY.union(changes), changes);
}

#[test]
fn change_set_merge_unions_every_part_and_keeps_the_first_image_changing() {
    let mut first = ChangeSet::of(MainProperty::StatusText);
    first.viewer.insert(ViewerProperty::Zoom);
    first.overlay.insert(InfoOverlayProperty::IsFileInfoVisible);
    first.compare.insert(CompareProperty::LeftPath);
    first.presenter = PresenterChanges::STATUS;
    first.image_changing = Some(ImageChangingEventArgs { is_file_change: true });

    let mut second = ChangeSet::of(MainProperty::FolderTitle);
    second.viewer.insert(ViewerProperty::SourcePixelWidth);
    second.overlay.insert(InfoOverlayProperty::ZoomIndicatorText);
    second.compare.insert(CompareProperty::RightPath);
    second.presenter = PresenterChanges::CURRENT_IMAGE;
    second.image_changing = Some(ImageChangingEventArgs { is_file_change: false });
    second.catalog_changed = true;
    first.merge(second);

    assert!(first.main.contains(MainProperty::StatusText));
    assert!(first.main.contains(MainProperty::FolderTitle));
    assert!(first.viewer.contains(ViewerProperty::Zoom));
    assert!(first.viewer.contains(ViewerProperty::SourcePixelWidth));
    assert!(first.overlay.contains(InfoOverlayProperty::IsFileInfoVisible));
    assert!(first.overlay.contains(InfoOverlayProperty::ZoomIndicatorText));
    assert!(first.compare.contains(CompareProperty::LeftPath));
    assert!(first.compare.contains(CompareProperty::RightPath));
    assert!(first.presenter.contains(PresenterChanges::STATUS));
    assert!(first.presenter.contains(PresenterChanges::CURRENT_IMAGE));
    assert_eq!(
        first.image_changing,
        Some(ImageChangingEventArgs { is_file_change: true })
    );
    assert!(first.catalog_changed);
    assert!(!first.is_empty());
    assert!(ChangeSet::default().is_empty());
}

#[test]
fn main_property_names_match_csharp_notifications() {
    assert_eq!(MainProperty::ALL.len(), 21);
    assert_eq!(MainProperty::StatusText.csharp_name(), "StatusText");
    assert_eq!(MainProperty::IsStatusPanelVisible.csharp_name(), "IsStatusPanelVisible");
    assert_eq!(
        MainProperty::CapturePairBadgeToolTip.csharp_name(),
        "CapturePairBadgeToolTip"
    );
}

#[test]
fn bridge_new_image_resets_force_high_quality_and_sets_the_source_size() {
    // C#: MainViewModel.NotifyCurrentImageChanged
    let (bridge, viewer) = main_vm::bridge();
    viewer.borrow_mut().set_force_high_quality_scaling(true);
    bridge.on_set_current_image(&main_vm::image_change(Some("A.JPG"), 640, 480, false), true);
    assert_eq!(
        (
            viewer.borrow().source_pixel_width(),
            viewer.borrow().source_pixel_height()
        ),
        (640, 480)
    );
    assert!(!viewer.borrow().force_high_quality_scaling());
}

#[test]
fn bridge_same_path_refresh_is_not_a_new_image() {
    // C#: MainViewModel.NotifyCurrentImageChanged
    let (bridge, viewer) = main_vm::bridge();
    bridge.on_set_current_image(&main_vm::image_change(Some("A.JPG"), 640, 480, false), true);
    viewer.borrow_mut().set_force_high_quality_scaling(true);
    bridge.on_set_current_image(&main_vm::image_change(Some("a.jpg"), 640, 480, false), false);
    assert!(viewer.borrow().force_high_quality_scaling());
}

#[test]
fn bridge_same_source_swap_calls_the_swapping_hook_before_the_size_changes() {
    // C#: MainViewModel.NotifyCurrentImageChanged
    let (bridge, viewer) = main_vm::bridge();
    bridge.on_set_current_image(&main_vm::image_change(Some("A.JPG"), 640, 480, false), true);
    viewer.borrow_mut().zoom_to_actual_size();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&seen);
    bridge.set_source_size_swapping_hook(Some(Box::new(move |before| {
        captured
            .borrow_mut()
            .push((before.source_pixel_width(), before.source_pixel_height()));
    })));
    bridge.on_set_current_image(&main_vm::image_change(Some("A.JPG"), 1280, 960, true), false);
    assert_eq!(&*seen.borrow(), &[(640, 480)]);
    assert_eq!(
        (
            viewer.borrow().source_pixel_width(),
            viewer.borrow().source_pixel_height()
        ),
        (1280, 960)
    );
}

#[test]
fn bridge_zoom_mode_change_is_returned_once_by_take_viewer_zoom_change() {
    // C#: MainViewModel.NotifyCurrentImageChanged
    let (bridge, viewer) = main_vm::bridge();
    viewer.borrow_mut().set_source_size(640, 480, true);
    let settings = AppSettings {
        initial_view_mode: InitialViewMode::ClickZoomLevel,
        click_zoom_percent: 175,
        ..AppSettings::default()
    };
    assert!(bridge.apply_initial_view((800.0, 600.0), &settings));
    assert_eq!(bridge.take_viewer_zoom_change(), Some(Some(1.75)));
    assert_eq!(bridge.take_viewer_zoom_change(), None);
}

#[test]
fn apply_initial_view_applies_the_configured_mode_and_click_zoom_percent() {
    // C#: MainViewModelAdvancedTests.ApplyInitialView_AppliesTheConfiguredModeAndClickZoomPercent
    let (bridge, viewer) = main_vm::bridge();
    viewer.borrow_mut().set_source_size(1600, 1200, true);
    let settings = AppSettings {
        initial_view_mode: InitialViewMode::ClickZoomLevel,
        click_zoom_percent: 175,
        ..AppSettings::default()
    };
    assert!(bridge.apply_initial_view((800.0, 600.0), &settings));
    assert_eq!(viewer.borrow().zoom(), 1.75);
}

#[test]
fn apply_initial_view_when_zoom_is_kept_across_images_leaves_the_zoom_alone() {
    // C#: MainViewModelAdvancedTests.ApplyInitialView_WhenZoomIsKeptAcrossImages_LeavesTheZoomAlone
    let (bridge, viewer) = main_vm::bridge();
    viewer.borrow_mut().set_zoom(2.5);
    let settings = AppSettings {
        initial_view_mode: InitialViewMode::Fit,
        keep_zoom_across_images: true,
        ..AppSettings::default()
    };
    assert!(!bridge.apply_initial_view((800.0, 600.0), &settings));
    assert_eq!(viewer.borrow().zoom(), 2.5);
}

#[test]
fn settings_source_override_edit_and_commit_follow_the_store() {
    // New test: kills SettingsSource::set_override with (), has_override with true/false, begin_edit with Default::default(), and commit_edit with Ok(()).
    let env = MainEnv::new();
    assert!(!env.settings.has_override());
    let mut override_settings = (*env.store.current()).clone();
    override_settings.keyboard_zoom_step_percent = 37;
    env.settings.set_override(Some(std::sync::Arc::new(override_settings)));
    assert!(env.settings.has_override());
    assert_eq!(env.settings.current().keyboard_zoom_step_percent, 37);
    assert_ne!(env.settings.store_current().keyboard_zoom_step_percent, 37);
    let edit = env
        .settings
        .begin_edit(|settings| settings.keyboard_zoom_step_percent = 41);
    assert_eq!(edit.settings().keyboard_zoom_step_percent, 41);
    assert_eq!(env.settings.current().keyboard_zoom_step_percent, 41);
    assert!(env.settings.commit_edit(edit).is_ok());
    assert_eq!(env.store.current().keyboard_zoom_step_percent, 41);
    assert!(
        env.settings.has_override(),
        "the preexisting explicit override remains after save"
    );
    env.settings.set_override(None);
    assert!(!env.settings.has_override());
    assert_eq!(env.settings.current().keyboard_zoom_step_percent, 41);
}

#[test]
fn folder_info_labels_use_the_localized_previous_next_and_pending_keys() {
    // New test: kills both empty/xyzzy replacements of MainFolderInfoLabels::previous, next and pending.
    let labels = MainFolderInfoLabels::new(std::sync::Arc::new(EnglishTexts::new()));
    let env = MainEnv::new();
    let args = [LocArg::new("key", "PageUp"), LocArg::new("folderName", "Yesterday")];
    assert_eq!(
        labels.previous("PageUp", "Yesterday"),
        env.text_with("main.folderInfo.previous", &args)
    );
    assert_eq!(
        labels.next("PageUp", "Yesterday"),
        env.text_with("main.folderInfo.next", &args)
    );
    assert_eq!(labels.pending(), env.text("main.folderInfo.pending"));
}

#[test]
fn settings_override_updates_the_view_model_and_the_visibility() {
    // New test: kills MainViewModel::set_settings with Default::default(), settings with default and is_status_panel_visible || -> &&.
    let mut env = MainEnv::new();
    let mut settings = (*env.vm.settings()).clone();
    settings.show_info_overlay = false;
    let changes = env.vm.set_settings(std::sync::Arc::new(settings));
    assert!(!env.vm.is_status_panel_visible());
    assert!(!env.vm.settings().show_info_overlay);
    assert!(changes.overlay.contains(InfoOverlayProperty::IsFileInfoVisible));
    assert!(changes.main.contains(MainProperty::IsStatusPanelVisible));
    let mut settings = (*env.vm.settings()).clone();
    settings.show_info_overlay = true;
    env.vm.set_settings(std::sync::Arc::new(settings));
    assert!(env.vm.is_status_panel_visible());
}

#[test]
fn skipped_warning_alone_keeps_attention_and_status_panel_visible() {
    // New test: kills MainViewModel::skipped_entries empty/nonempty replacements, has_skipped_entries true/false/delete !, skipped_warning_text replacements, status_needs_attention || -> &&, and is_status_panel_visible || -> &&.
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(|settings| settings.show_info_overlay = false),
        ..EnvOptions::default()
    });
    assert!(!env.vm.is_status_panel_visible());
    assert!(!env.vm.status_needs_attention());
    let skipped = SkippedEntry {
        path: env.photos.join("unreadable.jpg"),
        reason: "locked".to_owned(),
        kind: SkippedKind::FileUnreadable,
    };
    let changes = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.on_files_skipped("folder", std::slice::from_ref(&skipped)));
    for property in [
        MainProperty::SkippedEntries,
        MainProperty::HasSkippedEntries,
        MainProperty::SkippedWarningText,
        MainProperty::IsStatusPanelVisible,
    ] {
        assert!(changes.main.contains(property), "{property:?}");
    }
    assert_eq!(env.vm.skipped_entries(), &[skipped]);
    assert!(env.vm.has_skipped_entries());
    assert!(env.vm.is_status_panel_visible());
    assert!(env.vm.status_needs_attention());
    let fixture = FormatFixture::new();
    let ctx = fixture.ctx();
    assert_eq!(
        env.vm.skipped_warning_text(),
        ctx.texts
            .format_plural_with("main.skipped.warning", 1, &[LocArg::new("count", 1_i64)], ctx.numbers)
    );
}

#[test]
fn catalog_accessors_follow_empty_and_populated_folders() {
    // New test: kills MainViewModel::catalog/default, current_image None, and the derived navigation/state accessor replacements.
    let mut env = MainEnv::new();
    assert_eq!(env.vm.current_index(), -1);
    assert_eq!(env.vm.total_files(), 0);
    assert!(!env.vm.has_images());
    assert!(!env.vm.can_navigate_next());
    assert!(!env.vm.can_navigate_previous());
    let (folder, _) = env.images("accessors", &["1.png", "2.png"]);
    env.open(&folder);
    assert_eq!(env.vm.catalog().count(), 2);
    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(env.vm.current_index(), 0);
    assert!(env.vm.has_images());
    assert!(env.vm.can_navigate_next());
    assert!(!env.vm.can_navigate_previous());
    assert!(env.vm.current_image().is_some());
    env.vm.next();
    env.settle();
    assert_eq!(env.vm.current_index(), 1);
    assert!(!env.vm.can_navigate_next());
    assert!(env.vm.can_navigate_previous());
}

#[test]
fn main_worker_port_submits_wraps_and_wakes_once() {
    // New test: kills MainWorkerPort::submit with ().
    use std::sync::atomic::{AtomicUsize, Ordering};
    let wakes = std::sync::Arc::new(AtomicUsize::new(0));
    let wake_count = wakes.clone();
    let inbox = CompletionInbox::new(std::sync::Arc::new(move || {
        wake_count.fetch_add(1, Ordering::SeqCst);
    }));
    let runner = support::main_env::ManualRunner::new();
    let port = MainWorkerPort::new(runner.clone(), inbox.clone(), |_: u8| MainEvent::GateReleased);
    port.submit(WorkerLane::Scan, Box::new(|| 7));
    assert_eq!(runner.count_of("main-view-model-worker"), 1);
    assert!(inbox.is_empty());
    assert!(runner.run_tagged("main-view-model-worker"));
    assert!(matches!(inbox.drain().as_slice(), [MainEvent::GateReleased]));
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
}

#[test]
fn settings_failed_commit_keeps_provisional_value_until_cleared() {
    // New test: kills SettingsSource::commit_edit with Ok(()) and clear_provisional_override with ().
    let env = MainEnv::new();
    let original = env.store.current().keyboard_zoom_step_percent;
    let edit = env.settings.begin_edit(|settings| {
        settings.keyboard_zoom_step_percent = original + 3;
        settings.preload_memory_load_limit = f64::NAN;
    });
    assert_eq!(env.settings.current().keyboard_zoom_step_percent, original + 3);
    assert!(env.settings.commit_edit(edit).is_err());
    assert!(env.settings.has_override());
    assert_eq!(env.store.current().keyboard_zoom_step_percent, original);
    env.settings.clear_provisional_override();
    assert!(!env.settings.has_override());
    assert_eq!(env.settings.current().keyboard_zoom_step_percent, original);
}

#[test]
fn opened_folder_exposes_title_text_session_and_explorer_order() {
    // New test: kills MainViewModel::folder_title/ folder_text empty and xyzzy returns, is_explorer_order_applied true/false, and session None/default returns.
    let mut env = MainEnv::new();
    let (folder, files) = env.images("state_fields", &["1.png", "2.png"]);
    env.explorer.set_order(files);
    env.open(&folder);
    assert!(env.vm.folder_title().contains("state_fields"));
    assert!(env.vm.folder_text().starts_with(&folder));
    assert!(env.vm.is_explorer_order_applied());
    let session = env.vm.session().expect("opening the folder creates a session");
    assert_eq!(session.folder, folder);
}

#[test]
fn presenter_line_is_shown_when_the_event_status_is_empty() {
    // New test: kills MainViewModel::status_text delete !, && -> || and delete ! in the RAW/presenter branch.
    let mut env = MainEnv::new();
    let (folder, _) = env.images("status_line", &["one.png"]);
    env.open(&folder);
    let presenter_line = env.vm.presenter().status_text().to_owned();
    assert!(!presenter_line.is_empty());
    assert_eq!(env.vm.status_text(), presenter_line);
    env.vm.set_status_text("saved");
    assert_eq!(env.vm.status_text(), "saved");
    env.vm.set_status_text("");
    assert_eq!(env.vm.status_text(), presenter_line);
}

#[test]
fn defer_next_preload_kick_is_forwarded_to_the_presenter() {
    // New test: kills MainViewModel::set_defer_next_preload_kick with ().
    let mut env = MainEnv::new();
    assert!(!env.vm.presenter().defer_next_preload_kick());
    env.vm.set_defer_next_preload_kick(true);
    assert!(env.vm.presenter().defer_next_preload_kick());
    env.vm.set_defer_next_preload_kick(false);
    assert!(!env.vm.presenter().defer_next_preload_kick());
}

#[test]
fn prewarm_initial_image_starts_for_a_supported_file() {
    // New test: kills MainViewModel::prewarm_initial_image with None or Some(Default::default()).
    let env = MainEnv::new();
    let (_, paths) = env.images("prewarm", &["first.png"]);
    let task = env
        .vm
        .prewarm_initial_image(&paths[0])
        .expect("supported image starts a prewarm task");
    assert!(task.wait().is_ok());
    assert!(env.vm.prewarm_initial_image(" ").is_none());
}

#[test]
fn prewarm_initial_image_uses_store_settings_even_with_an_override() {
    // New test: kills MainViewModel::prewarm_initial_image with None and checks the C# store-current rule.
    let env = MainEnv::new();
    let (_, paths) = env.images("prewarm_store", &["first.webp"]);
    env.update_settings(|settings| settings.webp_heic_support_enabled = false);
    let mut override_settings = (*env.store.current()).clone();
    override_settings.webp_heic_support_enabled = true;
    env.settings.set_override(Some(Arc::new(override_settings)));
    assert!(env.vm.prewarm_initial_image(&paths[0]).is_none());
    env.update_settings(|settings| settings.webp_heic_support_enabled = true);
    let mut override_settings = (*env.store.current()).clone();
    override_settings.webp_heic_support_enabled = false;
    env.settings.set_override(Some(Arc::new(override_settings)));
    let task = env.vm.prewarm_initial_image(&paths[0]).expect("store allows WebP");
    assert!(task.wait().is_ok());
}

#[test]
fn explicit_override_is_not_cleared_as_if_it_were_provisional() {
    // New test: kills SettingsSource::clear_provisional_override with () in its provisional case while preserving explicit overrides.
    let env = MainEnv::new();
    let mut settings = (*env.store.current()).clone();
    settings.keyboard_zoom_step_percent = 73;
    env.settings.set_override(Some(std::sync::Arc::new(settings)));
    env.settings.clear_provisional_override();
    assert!(env.settings.has_override());
    assert_eq!(env.settings.current().keyboard_zoom_step_percent, 73);
}

struct EmbeddedRaw;

impl ImageDecoder for EmbeddedRaw {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Ok(sized_image(2000, 1333, 6000, 4000, true))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(6000, 4000))
    }
}

struct GatedRaw {
    gate: Arc<Gate>,
}

impl ImageDecoder for GatedRaw {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if !self.gate.wait() {
            return Err(DecodeError::Io(std::io::Error::other("the full decode gate timed out")));
        }
        Ok(sized_image(6016, 4016, 6016, 4016, false))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(6000, 4000))
    }
}

#[test]
fn raw_indicator_shows_decoding_line_until_an_action_status_takes_precedence() {
    // New test: kills MainViewModel::status_text delete ! in the RAW indicator branch, and the RAW-only status_needs_attention || mutants.
    let full = Arc::new(GatedRaw { gate: Gate::new() });
    let mut env = MainEnv::with_options(EnvOptions {
        decoder: Arc::new(EmbeddedRaw),
        raw_full_decoder: Some(full.clone()),
        settings: Box::new(|settings| {
            settings.loading_mode = Some(LoadingMode::Preview);
            settings.raw_full_decode = RawFullDecode::OnZoom;
        }),
        catalog_builder: Some(Box::new(|photos: &Path| {
            let folder = photos.join("raw_indicator");
            std::fs::create_dir_all(&folder).unwrap_or_else(|error| panic!("create RAW folder: {error}"));
            let path = folder.join("raw.cr2");
            std::fs::write(&path, [0_u8; 64]).unwrap_or_else(|error| panic!("write RAW: {error}"));
            let mut catalog = pr_core::catalog::review_catalog::ReviewCatalog::new();
            catalog.reset(
                vec![
                    pr_core::catalog::types::CatalogEntry::new(path.to_string_lossy().into_owned())
                        .unwrap_or_else(|error| panic!("{error:?}")),
                ],
                RawPairMode::Separate,
                None,
            );
            catalog
        })),
        ..EnvOptions::default()
    });
    env.vm.first();
    env.settle();
    env.vm.zoom_actual_size();
    env.settle_until("the RAW indicator", |env| {
        env.vm.presenter().zoom_detail().is_raw_decode_indicator_visible()
    });
    assert!(env.vm.presenter().zoom_detail().is_raw_decode_indicator_visible());
    let expected = pr_app::viewmodels::format::status::decoding_raw(&FormatFixture::new().ctx());
    assert_eq!(env.vm.status_text(), expected);
    assert!(env.vm.status_needs_attention());
    env.vm.set_status_text("Action failed");
    assert_eq!(env.vm.status_text(), "Action failed");
    full.gate.open();
    env.settle();
}

#[test]
fn debug_views_name_their_stateful_types_and_fields() {
    // New test: kills the empty Debug implementations for SettingsSource, PresentationBridge, MainFolderInfoLabels, and MainViewModel.
    let env = MainEnv::new();
    let settings_debug = format!("{:?}", env.settings);
    assert!(settings_debug.contains("SettingsSource"));
    assert!(settings_debug.contains("has_override: false"));
    assert!(format!("{:?}", env.vm.bridge()).contains("PresentationBridge"));
    let labels = MainFolderInfoLabels::new(Arc::new(EnglishTexts::new()));
    assert!(format!("{labels:?}").contains("MainFolderInfoLabels"));
    // New test: kills the empty Debug implementation for MainViewModelDeps.
    assert!(env.deps_debug.contains("MainViewModelDeps"));
    let vm_debug = format!("{:?}", env.vm);
    assert!(vm_debug.contains("MainViewModel"));
    assert!(vm_debug.contains("folder_title"));
    assert!(vm_debug.contains("status_text"));
    assert!(vm_debug.contains("is_closed: false"));
}

#[test]
fn metrics_accessor_returns_the_injected_metrics_arc() {
    // New test: kills MainViewModel::metrics replacement with a leaked default Arc.
    let env = MainEnv::new();
    assert!(Arc::ptr_eq(env.vm.metrics(), &env.metrics));
    assert!(Arc::ptr_eq(env.vm.metrics(), env.vm.metrics()));
}

#[test]
fn viewer_handle_borrow_reads_the_same_live_viewer_after_updates() {
    // New test: kills ViewerHandle::borrow replacements with an empty or default Ref.
    let (_, viewer) = main_vm::bridge();
    viewer.borrow_mut().set_source_size(918, 527, true);
    assert_eq!(viewer.borrow().source_pixel_width(), 918);
    assert_eq!(viewer.borrow().source_pixel_height(), 527);
    viewer.borrow_mut().set_source_size(42, 71, true);
    assert_eq!(viewer.borrow().source_pixel_width(), 42);
}

#[test]
fn bridge_path_identity_resets_quality_only_when_the_path_changes() {
    // New test: kills the (None, None) and (Some, Some) match-arm deletions and the negated path comparison.
    let cases = [
        (None, None, true),
        (Some("A.JPG"), Some("a.jpg"), true),
        (Some("A.JPG"), Some("B.JPG"), false),
        (None, Some("A.JPG"), false),
    ];
    for (first, second, quality_survives) in cases {
        let (bridge, viewer) = main_vm::bridge();
        bridge.on_set_current_image(&main_vm::image_change(first, 640, 480, false), false);
        viewer.borrow_mut().set_force_high_quality_scaling(true);
        bridge.on_set_current_image(&main_vm::image_change(second, 640, 480, false), false);
        assert_eq!(
            viewer.borrow().force_high_quality_scaling(),
            quality_survives,
            "{first:?} -> {second:?}"
        );
    }
}

#[test]
fn bridge_image_changing_is_only_raised_for_the_first_file_change() {
    // New test: kills PresentationBridge::on_set_current_image && -> ||.
    let mut env = MainEnv::new();
    env.vm
        .bridge()
        .on_set_current_image(&main_vm::image_change(Some("a.jpg"), 640, 480, false), false);
    assert_eq!(env.vm.pump().image_changing, None);

    env.vm
        .bridge()
        .on_set_current_image(&main_vm::image_change(Some("b.jpg"), 640, 480, false), true);
    env.vm
        .bridge()
        .on_set_current_image(&main_vm::image_change(Some("c.jpg"), 640, 480, false), true);
    assert_eq!(
        env.vm.pump().image_changing,
        Some(ImageChangingEventArgs { is_file_change: true })
    );
}

#[test]
fn bridge_status_callback_marks_navigation_dirty() {
    // New test: kills PresentationBridge::on_set_status_text replacement with ().
    let mut env = MainEnv::new();
    env.vm.bridge().on_set_status_text();
    let changes = env.vm.pump();
    assert!(changes.main.contains(MainProperty::CurrentIndex));
    assert!(changes.main.contains(MainProperty::TotalFiles));
}

#[test]
fn presenter_photo_info_only_still_notifies_navigation_state() {
    // New test: kills MainViewModel::finish_presenter_call PHOTO_INFO || STATUS -> &&.
    let mut env = MainEnv::new();
    let (folder, _) = env.images("photo_info_start", &["a.jpg", "b.jpg"]);
    env.open(&folder);
    let changes = env.vm.next();
    assert!(changes.presenter.contains(PresenterChanges::PHOTO_INFO));
    assert!(!changes.presenter.contains(PresenterChanges::STATUS));
    assert!(!changes.presenter.contains(PresenterChanges::CURRENT_IMAGE));
    assert!(changes.main.contains(MainProperty::CurrentIndex));
    assert!(changes.main.contains(MainProperty::TotalFiles));
    env.settle();
}

#[test]
fn initial_view_return_reports_whether_the_viewer_applied_it() {
    // New test: kills PresentationBridge::apply_initial_view replacements with true and false.
    let (bridge, viewer) = main_vm::bridge();
    viewer.borrow_mut().set_source_size(1600, 1200, true);
    let applied = AppSettings {
        initial_view_mode: InitialViewMode::ClickZoomLevel,
        click_zoom_percent: 160,
        ..AppSettings::default()
    };
    assert!(bridge.apply_initial_view((800.0, 600.0), &applied));
    assert_eq!(viewer.borrow().zoom(), 1.6);
    let kept = AppSettings {
        initial_view_mode: InitialViewMode::Fit,
        keep_zoom_across_images: true,
        ..AppSettings::default()
    };
    assert!(!bridge.apply_initial_view((800.0, 600.0), &kept));
    assert_eq!(viewer.borrow().zoom(), 1.6);
}

#[test]
fn current_folder_label_includes_the_folder_name() {
    // New test: kills MainFolderInfoLabels::current replacement with String::new().
    let labels = MainFolderInfoLabels::new(Arc::new(EnglishTexts::new()));
    let env = MainEnv::new();
    assert_eq!(
        labels.current("October"),
        env.text_with("main.folderInfo.current", &[LocArg::new("folderName", "October")])
    );
    assert!(!labels.current("October").is_empty());
}

#[test]
fn capture_pair_state_reports_empty_and_both_presented_members() {
    // New test: kills current_has_capture_pair true/false and capture_pair_badge empty/xyzzy replacements.
    let mut empty = MainEnv::new();
    assert!(!empty.vm.current_has_capture_pair());
    assert_eq!(empty.vm.capture_pair_badge(), "");
    let (folder, _) = empty.images("single_capture", &["single.jpg"]);
    empty.open(&folder);
    assert!(!empty.vm.current_has_capture_pair());
    assert_eq!(empty.vm.capture_pair_badge(), "");

    let mut paired = MainEnv::with_options(EnvOptions {
        settings: Box::new(|settings| {
            settings.raw_support_enabled = true;
            settings.raw_pair_mode = RawPairMode::PreferJpeg;
        }),
        ..EnvOptions::default()
    });
    let (folder, paths) = paired.images("capture_members", &["a.jpg", "a.cr2"]);
    paired.open(&folder);
    assert_eq!(paired.vm.total_files(), 1);
    assert!(paired.vm.current_has_capture_pair());
    assert_eq!(paired.vm.presenter().current_presented_path(), Some(paths[0].as_str()));
    assert_eq!(
        paired.vm.capture_pair_badge(),
        paired.text("main.capturePair.badge.jpeg")
    );
    paired.vm.toggle_capture_group_member();
    paired.settle();
    assert_eq!(paired.vm.presenter().current_presented_path(), Some(paths[1].as_str()));
    assert_eq!(
        paired.vm.capture_pair_badge(),
        paired.text("main.capturePair.badge.raw")
    );
}

#[test]
fn capture_pair_tooltip_uses_only_a_valid_nonblank_binding() {
    // New test: kills capture_pair_badge_tool_tip empty/xyzzy and deleted ! mutants.
    let env = MainEnv::new();
    let unbound = env.text("main.capturePair.badge.tooltipUnbound");
    assert_eq!(env.vm.capture_pair_badge_tool_tip(), unbound);
    for (configured, expected) in [
        (
            "F4",
            env.text_with("main.capturePair.badge.tooltipBound", &[LocArg::new("key", "F4")]),
        ),
        ("", unbound.clone()),
        (" ", unbound.clone()),
    ] {
        let mut settings = (*env.store.current()).clone();
        settings.shortcuts.toggle_capture_member = configured.to_owned();
        env.settings.set_override(Some(Arc::new(settings)));
        assert_eq!(env.vm.capture_pair_badge_tool_tip(), expected, "binding {configured:?}");
    }
}

#[test]
fn compare_pair_requires_a_current_entry_and_finds_numbered_neighbors() {
    // New test: kills current_has_compare_pair false and >= -> < mutants.
    let mut env = MainEnv::new();
    assert!(!env.vm.current_has_compare_pair());
    let (folder, _) = env.images("numbered_compare", &["a.jpg", "a (1).jpg", "solo.png"]);
    env.open(&folder);
    assert!(env.vm.current_has_compare_pair());
    env.vm.next();
    env.settle();
    assert!(env.vm.current_has_compare_pair());
    env.vm.next();
    env.settle();
    assert!(!env.vm.current_has_compare_pair());
}

#[test]
fn close_session_changes_the_lifetime_state() {
    // New test: kills is_closed true and false replacements.
    let mut env = MainEnv::new();
    assert!(!env.vm.is_closed());
    env.vm.close_session();
    assert!(env.vm.is_closed());
}

#[test]
fn viewer_commands_forward_exact_viewer_changes() {
    // New test: kills apply_viewer_update default return and omitted viewer field.
    let mut env = MainEnv::new();
    let changes = env.vm.toggle_fullscreen();
    assert!(changes.viewer.contains(ViewerProperty::IsFullscreen));
    assert!(env.viewer.borrow().is_fullscreen());
    let changes = env.vm.exit_fullscreen();
    assert!(changes.viewer.contains(ViewerProperty::IsFullscreen));
    assert!(!env.viewer.borrow().is_fullscreen());
}

#[test]
fn toggling_compare_keeps_the_presented_raw_member() {
    // New test: kills presented_path_of_current_entry None/empty/xyzzy and == -> != mutants.
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(|settings| {
            settings.raw_support_enabled = true;
            settings.raw_pair_mode = RawPairMode::PreferJpeg;
        }),
        ..EnvOptions::default()
    });
    let (folder, paths) = env.images("presented_raw", &["a.jpg", "a.cr2"]);
    env.open(&folder);
    env.vm.toggle_capture_group_member();
    env.settle();
    assert_eq!(env.vm.presenter().current_presented_path(), Some(paths[1].as_str()));
    env.vm.toggle_compare();
    env.settle();
    assert_eq!(env.vm.presenter().current_presented_path(), Some(paths[1].as_str()));
    assert_eq!(env.vm.capture_pair_badge(), env.text("main.capturePair.badge.raw"));
    assert!(
        env.log
            .entries()
            .iter()
            .all(|(level, _)| *level != pr_test_support::LogLevel::Error),
        "a current RAW member must be accepted as the override: {:?}",
        env.log.entries()
    );
}

#[test]
fn zoom_and_overlay_refresh_return_the_exact_changed_properties() {
    // New test: kills refresh_overlay default return and apply_viewer_update default/omitted-viewer mutants.
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(|settings| settings.show_zoom_indicator = true),
        ..EnvOptions::default()
    });
    let (folder, _) = env.images("zoom_overlay_state", &["a.png"]);
    env.open(&folder);
    let zoom = env.vm.zoom_actual_size();
    assert!(zoom.viewer.contains(ViewerProperty::DisplayZoomPercent));
    assert!(zoom.overlay.contains(InfoOverlayProperty::ZoomIndicatorText));
    let expected = format!("{}%", env.viewer.borrow().display_zoom_percent());
    assert_eq!(
        pr_app::viewmodels::info_overlay::InfoOverlayViewModel::<MainFolderInfoLabels>::zoom_indicator_text(
            pr_app::viewmodels::info_overlay::ViewInputs {
                has_image: true,
                zoom_percent: env.viewer.borrow().display_zoom_percent(),
            },
        ),
        expected,
    );
    let toggled = env.vm.toggle_info_overlay();
    assert!(toggled.overlay.contains(InfoOverlayProperty::IsFileInfoVisible));
    assert!(toggled.main.contains(MainProperty::IsStatusPanelVisible));
}

/// A sibling-search environment whose RAW getter panics: three folders a < b < c with one image each, `b` is current.
fn panicking_sibling_env() -> (MainEnv, String) {
    let env = MainEnv::with_options(EnvOptions {
        sibling_raw_getter: Some(Arc::new(|| panic!("raw getter blew up"))),
        ..EnvOptions::default()
    });
    let parent = env.folder("sibling_panic");
    let mut current = String::new();
    for name in ["2024-05-01", "2024-05-02", "2024-05-03"] {
        let folder = Path::new(&parent).join(name);
        std::fs::create_dir_all(&folder).unwrap_or_else(|error| panic!("create sibling: {error}"));
        env.add_image(&folder.to_string_lossy(), "a.jpg");
        if name == "2024-05-02" {
            current = folder.to_string_lossy().into_owned();
        }
    }
    (env, current)
}

// New test (FIX-PANIC-REST M3): a panic in the overlay sibling-search job must still deliver `OverlaySiblings` (as `Failed`), so
// `pending_worker_jobs` drops and the view model becomes idle again.
#[test]
fn overlay_sibling_search_job_panic_still_releases_the_pending_count() {
    let (mut env, current) = panicking_sibling_env();
    env.update_settings(|settings| settings.show_folder_info = true);
    env.vm.open_folder(&current, None).expect("open current");
    env.settle_holding("overlay sibling search queued", &["overlay-siblings"], |_| false);
    assert_eq!(env.runner.count_of("overlay-siblings"), 1);
    assert!(!env.vm.is_idle());

    assert!(env.runner.run_tagged("overlay-siblings"));
    env.pump();

    assert!(
        env.vm.is_idle(),
        "the panicked job must have released its pending count"
    );
}

// New test (FIX-PANIC-REST M3): a panic in the sibling-folder-search job must still deliver `SiblingFound` (an error), so
// `pending_worker_jobs` drops and the view model becomes idle again.
#[test]
fn sibling_folder_search_job_panic_still_releases_the_pending_count() {
    let (mut env, current) = panicking_sibling_env();
    env.open(&current);
    env.vm.navigate_sibling_folder(1).expect("navigate");
    assert!(!env.vm.is_idle());

    assert!(env.runner.run_tagged("sibling-folder-search"));
    env.pump();

    assert!(
        env.vm.is_idle(),
        "the panicked job must have released its pending count"
    );
}

#[test]
fn sibling_search_completion_returns_the_folder_line_change() {
    // New test: kills apply_overlay_refresh += -> *= and on_overlay_siblings default return.
    let mut env = MainEnv::new();
    let parent = env.folder("overlay_siblings_state");
    let before = Path::new(&parent).join("2024-05-01");
    let current = Path::new(&parent).join("2024-05-02");
    for folder in [&before, &current] {
        std::fs::create_dir_all(folder).unwrap_or_else(|error| panic!("create sibling: {error}"));
    }
    env.add_image(&before.to_string_lossy(), "a.jpg");
    env.add_image(&current.to_string_lossy(), "b.jpg");
    env.update_settings(|settings| settings.show_folder_info = true);
    env.vm
        .open_folder(&current.to_string_lossy(), None)
        .expect("open sibling");
    env.settle_holding("overlay sibling search queued", &["overlay-siblings"], |_| false);
    assert_eq!(env.runner.count_of("overlay-siblings"), 1);
    assert!(!env.vm.is_idle(), "the pending sibling worker must be counted");
    assert!(env.runner.run_tagged("overlay-siblings"));
    let changes = env.pump();
    assert!(changes.overlay.contains(InfoOverlayProperty::FolderInfoText));
    assert_eq!(
        env.vm.info_overlay().folder_info_text(),
        format!(
            "{}     {}",
            env.text_with(
                "main.folderInfo.previous",
                &[LocArg::new("key", "PageUp"), LocArg::new("folderName", "2024-05-01")]
            ),
            env.text_with("main.folderInfo.current", &[LocArg::new("folderName", "2024-05-02")])
        )
    );
}

#[test]
fn skip_persists_the_new_session_state_after_an_initial_flush() {
    // New test: kills persist_session replacement with ().
    let mut env = MainEnv::new();
    let (folder, files) = env.images("persist_skip_state", &["a.jpg", "b.jpg"]);
    env.open(&folder);
    env.session_writer.flush();
    env.session_writer.when_idle();
    let before = env.sessions.load(&folder).expect("initial session");
    assert!(!before.skipped.contains(&files[0]));
    env.vm.skip();
    env.session_writer.flush();
    env.session_writer.when_idle();
    let saved = env.sessions.load(&folder).expect("saved session");
    assert_eq!(saved.skipped, vec![files[0].clone()]);
    env.settle();
}
