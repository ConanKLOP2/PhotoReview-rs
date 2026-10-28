//! W9-R10: the `ImagePresenterRetargetTests` of C# 7eaee2a2 that the R6 file `presenter_rename.rs` does not already cover.
//!
//! Already ported there (R6), so NOT repeated here: `RetargetPath_ShownFileRenamed_RekeysPathInfoAndStatusWithoutPresentingStatOrDecode`,
//! `RetargetPath_PresentInFlight_ReturnsFalseSoTheCallerPresentsAgain`,
//! `RetargetPath_CompareVisible_ClearsCompareAndReturnsFalseButRekeysFileIdentity`. `RetargetPath_NullMap_Throws` has no Rust
//! twin: a `&PathKeyMap` cannot be null.
//!
//! The presenter runs over a fake decoder (`SizedDecoder`: a bounded decode answers a 600 px preview, an unbounded one the 6000 x 4000
//! original, gated by file name) and real tiny files under a `TempRoot`; every wait is a bounded pump, never a sleep.

mod support {
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use pr_app::coordinators::zoom_detail::same_platform_image;
use pr_core::file_actions::rename::PathKeyMap;
use support::present_helpers::{ScriptedFs, present};
use support::presenter_fakes::{FixtureOptions, PresenterFixture};
use support::preview_fakes::{SizedDecoder, lock};

/// The rename as the controller leaves it: the file moved on disk (length and time kept) and the catalog entry re-keyed in place.
fn rename_on_disk_and_catalog(f: &mut PresenterFixture, old: &str, new_name: &str) -> String {
    let new = f.path(new_name);
    std::fs::rename(old, &new).unwrap_or_else(|error| panic!("rename {old}: {error}"));
    let map = [(old.to_owned(), new.clone())].into_iter().collect::<PathKeyMap>();
    assert!(f.catalog.rename_entries(&map), "the catalog re-keys the entry");
    new
}

fn retarget(f: &mut PresenterFixture, map: &PathKeyMap) -> bool {
    let ctx = f.format.ctx();
    f.presenter.retarget_path(map, &ctx).0
}

fn map_of(old: &str, new: &str) -> PathKeyMap {
    [(old.to_owned(), new.to_owned())].into_iter().collect()
}

fn shown_images(f: &PresenterFixture) -> usize {
    lock(&f.sink_log).images.len()
}

fn presented_old_path(f: &PresenterFixture) -> String {
    f.presenter.current_presented_path().unwrap_or_default().to_owned()
}

// C#: ImagePresenterTests.RetargetPath_ThenPresentSameEntry_IsNotReportedAsAFileChange
#[test]
fn retarget_path_then_present_same_entry_is_not_reported_as_a_file_change() {
    let mut f = PresenterFixture::new(SizedDecoder::new(), &["a.jpg", "c.jpg"]);
    present(&mut f, 0);
    let old = presented_old_path(&f);
    let new = rename_on_disk_and_catalog(&mut f, &old, "b.jpg");
    assert!(retarget(&mut f, &map_of(&old, &new)));
    let before = shown_images(&f);

    present(&mut f, 0);

    let flags = lock(&f.sink_log).file_change_flags();
    assert!(flags.len() > before, "the present showed an image");
    assert!(flags[before..].iter().all(|flag| !flag), "same file: no cross-fade");
    let before = shown_images(&f);
    present(&mut f, 1);
    let flags = lock(&f.sink_log).file_change_flags();
    assert!(
        flags[before..].iter().any(|flag| *flag),
        "control: a different file IS a file change"
    );
}

// C#: ImagePresenterTests.RetargetPath_AfterStopForAction_ZoomDecodesTheOriginalViaTheNewPath
#[test]
fn retarget_path_after_stop_for_action_zoom_decodes_the_original_via_the_new_path() {
    let decoder = SizedDecoder::new();
    let mut f = PresenterFixture::new(decoder.clone(), &["a.jpg"]);
    present(&mut f, 0);
    let old = presented_old_path(&f);
    f.clock.stop_for_action(); // what RenameAsync does before the I/O
    let new = rename_on_disk_and_catalog(&mut f, &old, "b.jpg");

    assert!(retarget(&mut f, &map_of(&old, &new)));
    f.presenter.set_viewer_zoom(Some(2.0));

    assert!(f.presenter.zoom_detail().pending_load());
    f.pump_until(|f| f.presenter.zoom_detail().held_original().is_some());
    assert_eq!(decoder.original_decodes("b.jpg"), 1);
    assert_eq!(decoder.total_original_decodes(), 1, "only the new name was decoded");
    assert!(f.presenter.zoom_detail().is_showing_original());
}

// C#: ImagePresenterTests.RetargetPath_WithoutRetarget_StopForActionLeavesZoomDetailDead
#[test]
fn retarget_path_without_retarget_stop_for_action_leaves_zoom_detail_dead() {
    let decoder = SizedDecoder::new();
    let mut f = PresenterFixture::new(decoder.clone(), &["a.jpg"]);
    present(&mut f, 0);
    f.clock.stop_for_action();

    f.presenter.set_viewer_zoom(Some(2.0));

    assert!(
        !f.presenter.zoom_detail().pending_load(),
        "the premise the retarget fixes"
    );
    assert_eq!(decoder.total_original_decodes(), 0);
}

// C#: ImagePresenterTests.RetargetPath_EmptyMap_IsAPureReArmThatKeepsNamesAndPresentsNothing
#[test]
fn retarget_path_empty_map_is_a_pure_re_arm_that_keeps_names_and_presents_nothing() {
    let decoder = SizedDecoder::new();
    let file_system = ScriptedFs::new();
    let mut f = PresenterFixture::with_options(
        decoder.clone(),
        &["a.jpg"],
        FixtureOptions {
            file_system: Some(file_system.clone()),
            ..FixtureOptions::default()
        },
    );
    present(&mut f, 0);
    let path = presented_old_path(&f);
    let images = shown_images(&f);
    let stats = file_system.stat_count(&path);
    let status = f.presenter.status_text().to_owned();
    f.clock.stop_for_action();

    let kept = retarget(&mut f, &PathKeyMap::new());
    f.presenter.set_viewer_zoom(Some(2.0));

    assert!(kept);
    assert_eq!(f.presenter.current_presented_path(), Some(path.as_str()));
    assert_eq!(f.presenter.status_text(), status);
    assert_eq!(shown_images(&f), images);
    assert_eq!(file_system.stat_count(&path), stats);
    assert!(
        f.presenter.zoom_detail().pending_load(),
        "zoom detail is alive again under the same path"
    );
    f.pump_until(|f| f.presenter.zoom_detail().held_original().is_some());
    assert_eq!(decoder.original_decodes("a.jpg"), 1);
}

// C#: ImagePresenterTests.RetargetPath_HeldOriginalAndShownState_SurviveWithoutNewDecodeOrImageUpdate
#[test]
fn retarget_path_held_original_and_shown_state_survive_without_new_decode_or_image_update() {
    let decoder = SizedDecoder::new();
    let mut f = PresenterFixture::new(decoder.clone(), &["a.jpg"]);
    present(&mut f, 0);
    f.presenter.set_viewer_zoom(Some(2.0));
    f.pump_until(|f| f.presenter.zoom_detail().held_original().is_some());
    let held = f
        .presenter
        .zoom_detail()
        .held_original()
        .cloned()
        .unwrap_or_else(|| panic!("a held original"));
    let shown = f
        .presenter
        .current_image()
        .cloned()
        .unwrap_or_else(|| panic!("a shown image"));
    let images = shown_images(&f);
    let old = presented_old_path(&f);
    f.clock.stop_for_action();
    let new = rename_on_disk_and_catalog(&mut f, &old, "b.jpg");

    assert!(retarget(&mut f, &map_of(&old, &new)));

    let held_now = f.presenter.zoom_detail().held_original().expect("still held");
    assert!(same_platform_image(held.platform_image(), held_now.platform_image()));
    assert!(f.presenter.zoom_detail().is_showing_original());
    assert!(
        f.presenter
            .current_image()
            .is_some_and(|image| same_platform_image(&shown, image))
    );
    assert_eq!(shown_images(&f), images);
    assert_eq!(decoder.total_original_decodes(), 1);
    assert_eq!(
        f.presenter.current_photo_info().map(|info| info.file_name.as_str()),
        Some("b.jpg")
    );
}

// C#: ImagePresenterTests.RetargetPath_OriginalDecodeInFlightForOldName_IsDroppedAndTheNewNameDecodes
#[test]
fn retarget_path_original_decode_in_flight_for_old_name_is_dropped_and_the_new_name_decodes() {
    let decoder = SizedDecoder::new();
    let gate = decoder.gate("a.jpg");
    let mut f = PresenterFixture::new(decoder.clone(), &["a.jpg"]);
    present(&mut f, 0);
    f.presenter.set_viewer_zoom(Some(2.0));
    assert!(f.presenter.zoom_detail().pending_load());
    assert!(
        decoder.wait_started("a.jpg"),
        "the old-name decode is running, held at its gate"
    );
    let old = presented_old_path(&f);
    f.clock.stop_for_action();
    let new = rename_on_disk_and_catalog(&mut f, &old, "b.jpg");

    assert!(retarget(&mut f, &map_of(&old, &new)));
    gate.open();
    f.pump_until(|f| f.presenter.zoom_detail().held_original().is_some() && !f.presenter.zoom_detail().pending_load());

    assert_eq!(decoder.original_decodes("a.jpg"), 1);
    assert_eq!(decoder.original_decodes("b.jpg"), 1, "the new name decodes");
    assert!(f.presenter.zoom_detail().is_showing_original());
    assert_eq!(
        f.presenter.current_photo_info().map(|info| info.file_name.as_str()),
        Some("b.jpg")
    );
}

// C#: ImagePresenterTests.IsPresentInFlight_AfterPresentCompleted_IsFalseAndRetargetKeeps
#[test]
fn is_present_in_flight_after_present_completed_is_false_and_retarget_keeps() {
    let mut f = PresenterFixture::new(SizedDecoder::new(), &["a.jpg"]);
    assert!(!f.presenter.is_present_in_flight());

    present(&mut f, 0);

    assert!(!f.presenter.is_present_in_flight());
    assert!(retarget(&mut f, &PathKeyMap::new()));
}

// C#: ImagePresenterTests.RetargetPath_NothingPresentedYet_IsANoOpThatKeeps
#[test]
fn retarget_path_nothing_presented_yet_is_a_no_op_that_keeps() {
    let mut f = PresenterFixture::new(SizedDecoder::new(), &["a.jpg"]);

    let kept = retarget(&mut f, &map_of("x.png", "y.png"));

    assert!(kept);
    assert_eq!(f.presenter.current_presented_path(), None);
    assert_eq!(shown_images(&f), 0);
}

// C#: ImagePresenterTests.RetargetPath_MapDoesNotListTheShownFile_KeepsShownNames
#[test]
fn retarget_path_map_does_not_list_the_shown_file_keeps_shown_names() {
    let mut f = PresenterFixture::new(SizedDecoder::new(), &["a.jpg", "c.jpg"]);
    present(&mut f, 0);
    let old = presented_old_path(&f);
    let status = f.presenter.status_text().to_owned();
    let other = f.path("c.jpg");

    let target = f.path("d.jpg");
    let kept = retarget(&mut f, &map_of(&other, &target));

    assert!(kept);
    assert_eq!(f.presenter.current_presented_path(), Some(old.as_str()));
    assert_eq!(
        f.presenter.current_photo_info().map(|info| info.file_name.as_str()),
        Some("a.jpg")
    );
    assert_eq!(f.presenter.status_text(), status);
}
