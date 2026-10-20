//! Red tests for W8-08a. Each assertion describes the C# behavior or a contract decision.

#[path = "support/main_vm.rs"]
mod main_vm;

use std::cell::RefCell;
use std::rc::Rc;

use pr_app::coordinators::presenter::state::PresenterChanges;
use pr_app::viewmodels::compare::CompareProperty;
use pr_app::viewmodels::info_overlay::InfoOverlayProperty;
use pr_app::viewmodels::main::{ChangeSet, ImageChangingEventArgs, MainChanges, MainProperty};
use pr_app::viewmodels::viewer_state::ViewerProperty;
use pr_core::model::enums::InitialViewMode;
use pr_core::settings::app_settings::AppSettings;

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
