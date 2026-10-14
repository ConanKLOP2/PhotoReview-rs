//! Tests for `pr_core::catalog::review_catalog`, ported from the C# tests at baseline d92d6aae
//! (`ReviewCatalogTests`, `ReviewCatalogMutationGapTests`, `ReviewCatalogRemoveIndexTests`,
//! `ReviewCatalogRobustnessTests`, `ReviewCatalogThreadGuardTests`, the three catalog-using tests of
//! `CaptureGroupBuilderTests`) plus golden `review-catalog` (101 cases, 1877 steps).
//!
//! Dropped (nothing to port in Rust): `Reset_PathsEnumerationThrows_KeepsPreviousState` and
//! `Reset_EntriesEnumerationThrows_KeepsPreviousState` (lazy-enumerable exception safety: `reset` takes a `Vec`);
//! `CatalogEntry_InvalidPath_ThrowsArgumentException` and `CatalogEntry_ValidPath_InitializesCorrectly` are W1-07 tests.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use pr_core::catalog::review_catalog::{MAX_REMOVED_BEFORE_REBUILD, ReviewCatalog};
use pr_core::catalog::types::{CaptureGroup, CatalogEntry};
use pr_core::model::RawPairMode;
use pr_core::text::eq_ignore_case;
use pr_core::time::DateTimeUtc;
use pr_test_support::golden;
use serde_json::Value;

const STAMP_TICKS: i64 = 639_000_000_000_000_000;

fn stamp() -> DateTimeUtc {
    DateTimeUtc::from_ticks(STAMP_TICKS).unwrap()
}

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|x| (*x).to_owned()).collect()
}

fn entry(path: &str) -> CatalogEntry {
    CatalogEntry::new(path).unwrap()
}

fn group2(jpeg: &str, raw: &str) -> CaptureGroup {
    CaptureGroup::new(jpeg, raw, None).unwrap()
}

fn paths_vec(c: &ReviewCatalog) -> Vec<String> {
    c.paths().iter().cloned().collect()
}

/// Test-support `Reset(paths)` overload (`ReviewCatalogTestExtensions`): blank strings dropped, Separate, no sidecars.
fn reset_paths(c: &mut ReviewCatalog, paths: &[&str]) {
    let entries: Vec<CatalogEntry> = paths
        .iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| entry(p))
        .collect();
    c.reset(entries, RawPairMode::Separate, None);
}

fn reset_entries(c: &mut ReviewCatalog, paths: &[&str], mode: RawPairMode) {
    let entries: Vec<CatalogEntry> = paths.iter().map(|p| entry(p)).collect();
    c.reset(entries, mode, None);
}

fn pair_catalog(mode: RawPairMode) -> ReviewCatalog {
    let mut c = ReviewCatalog::new();
    reset_entries(&mut c, &["C:\\photos\\before.jpg", "C:\\photos\\after.jpg"], mode);
    c
}

#[test]
fn restore_members_group_with_non_restorable_missing_member_does_not_reform() {
    // Repair round 1: kills the mutant dropping `is_restorable_image(member, true)` in try_reform_group
    // (C# TryReformGroup requires a missing listed member to be a restorable image).
    let mut c = ReviewCatalog::new();
    reset_entries(&mut c, &["C:\\photos\\p.cr2"], RawPairMode::PreferJpeg);
    let group = group2("C:\\photos\\a.txt", "C:\\photos\\p.cr2");
    let restored = c.restore_members(&s(&["C:\\photos\\a.txt"]), 0, Some(&group), true);
    assert!(!restored);
    assert_eq!(c.count(), 1);
    assert_eq!(paths_vec(&c), s(&["C:\\photos\\p.cr2"]));
    assert!(no_groups(&c));
}

fn no_groups(c: &ReviewCatalog) -> bool {
    c.entries_snapshot().iter().all(|e| e.capture_group.is_none())
}

// ---------------------------------------------------------------------------------------------------------------
// ReviewCatalogTests
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn restore_members_separate_mode_reinserts_only_unique_paths_as_independent_entries() {
    // C#: ReviewCatalogTests.RestoreMembers_SeparateMode_ReinsertsOnlyUniquePathsAsIndependentEntries
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["C:\\photos\\before.jpg", "C:\\photos\\after.jpg"]);
    c.restore_members(
        &s(&["C:\\photos\\raw.cr2", "C:\\photos\\jpeg.jpg", "C:\\photos\\raw.cr2"]),
        1,
        None,
        true,
    );
    assert_eq!(
        paths_vec(&c),
        s(&[
            "C:\\photos\\before.jpg",
            "C:\\photos\\raw.cr2",
            "C:\\photos\\jpeg.jpg",
            "C:\\photos\\after.jpg"
        ])
    );
    assert!(no_groups(&c));
}

#[test]
fn restore_members_raw_support_off_restores_only_the_jpeg_as_a_plain_entry() {
    // C#: ReviewCatalogTests.RestoreMembers_RawSupportOff_RestoresOnlyTheJpegAsAPlainEntry
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["C:\\photos\\before.jpg", "C:\\photos\\after.jpg"]);
    let group = group2("C:\\photos\\p.jpg", "C:\\photos\\p.cr2");
    c.restore_members(&s(&["C:\\photos\\p.jpg", "C:\\photos\\p.cr2"]), 1, Some(&group), false);
    assert_eq!(
        paths_vec(&c),
        s(&["C:\\photos\\before.jpg", "C:\\photos\\p.jpg", "C:\\photos\\after.jpg"])
    );
    assert!(no_groups(&c));
}

#[test]
fn restore_members_webp_and_heic_are_restored_as_plain_entries_whatever_the_raw_switch() {
    // C#: ReviewCatalogTests.RestoreMembers_WebpAndHeic_AreRestoredAsPlainEntriesWhateverTheRawSwitch
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["C:\\photos\\before.jpg"]);
    c.restore_members(
        &s(&[
            "C:\\photos\\web.webp",
            "C:\\photos\\IMG_1.HEIC",
            "C:\\photos\\x.heif",
            "C:\\photos\\y.avif",
        ]),
        1,
        None,
        false,
    );
    assert_eq!(
        paths_vec(&c),
        s(&[
            "C:\\photos\\before.jpg",
            "C:\\photos\\web.webp",
            "C:\\photos\\IMG_1.HEIC",
            "C:\\photos\\x.heif"
        ])
    );
}

#[test]
fn reset_paths_overload_forgets_the_previous_pair_mode() {
    // C#: ReviewCatalogTests.Reset_PathsOverload_ForgetsThePreviousPairMode
    let mut c = pair_catalog(RawPairMode::PreferJpeg);
    let group = group2("C:\\photos\\p.jpg", "C:\\photos\\p.cr2");
    reset_paths(&mut c, &["C:\\photos\\before.jpg", "C:\\photos\\after.jpg"]);
    c.restore_members(&s(&["C:\\photos\\p.jpg", "C:\\photos\\p.cr2"]), 1, Some(&group), true);
    assert_eq!(
        paths_vec(&c),
        s(&[
            "C:\\photos\\before.jpg",
            "C:\\photos\\p.jpg",
            "C:\\photos\\p.cr2",
            "C:\\photos\\after.jpg"
        ])
    );
    assert!(no_groups(&c));
}

#[test]
fn reset_entries_overload_forgets_the_previous_pair_mode() {
    // C#: ReviewCatalogTests.Reset_EntriesOverload_ForgetsThePreviousPairMode
    let mut c = pair_catalog(RawPairMode::PreferJpeg);
    let group = group2("C:\\photos\\p.jpg", "C:\\photos\\p.cr2");
    c.reset(
        vec![entry("C:\\photos\\before.jpg"), entry("C:\\photos\\after.jpg")],
        RawPairMode::Separate,
        None,
    );
    c.restore_members(&s(&["C:\\photos\\p.jpg", "C:\\photos\\p.cr2"]), 1, Some(&group), true);
    assert_eq!(c.count(), 4);
    assert!(no_groups(&c));
}

#[test]
fn update_metadata_path_of_a_non_representative_member_is_a_no_op() {
    // C#: ReviewCatalogTests.UpdateMetadata_PathOfANonRepresentativeMember_IsANoOp
    let mut c = ReviewCatalog::new();
    reset_entries(
        &mut c,
        &["C:\\photos\\p.jpg", "C:\\photos\\p.cr2"],
        RawPairMode::PreferJpeg,
    );
    let applied = c.update_metadata("C:\\photos\\p.cr2", 99, stamp(), Some(6000), Some(4000));
    assert!(!applied);
    let snapshot = c.entries_snapshot();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].length, None);
    assert_eq!(snapshot[0].last_write_utc, None);
    assert_eq!(snapshot[0].width, None);
}

#[test]
fn update_metadata_path_of_the_representative_stamps_the_group_entry() {
    // C#: ReviewCatalogTests.UpdateMetadata_PathOfTheRepresentative_StampsTheGroupEntry
    let mut c = ReviewCatalog::new();
    reset_entries(
        &mut c,
        &["C:\\photos\\p.jpg", "C:\\photos\\p.cr2"],
        RawPairMode::PreferJpeg,
    );
    let applied = c.update_metadata("C:\\photos\\P.JPG", 7, stamp(), None, None);
    assert!(applied);
    assert_eq!(c.entries_snapshot()[0].length, Some(7));
    assert!(c.entries_snapshot()[0].capture_group.is_some());
}

#[test]
fn restore_members_both_image_members_restores_one_grouped_entry_with_representative_for_the_mode() {
    // C#: ReviewCatalogTests.RestoreMembers_BothImageMembers_RestoresOneGroupedEntryWithRepresentativeForTheMode
    for (mode, representative) in [
        (RawPairMode::PreferJpeg, "C:\\photos\\p.jpg"),
        (RawPairMode::PreferRaw, "C:\\photos\\p.cr2"),
    ] {
        let mut c = pair_catalog(mode);
        c.restore_members(&s(&["C:\\photos\\p.jpg", "C:\\photos\\p.cr2"]), 1, None, true);
        assert_eq!(
            paths_vec(&c),
            s(&["C:\\photos\\before.jpg", representative, "C:\\photos\\after.jpg"])
        );
        let e = &c.entries_snapshot()[1];
        let g = e.capture_group.as_ref().unwrap();
        assert_eq!(g.jpeg_path(), "C:\\photos\\p.jpg");
        assert_eq!(g.raw_path(), "C:\\photos\\p.cr2");
        assert_eq!(c.index_of("C:\\photos\\p.jpg"), 1);
        assert_eq!(c.index_of("C:\\photos\\p.cr2"), 1);
    }
}

#[test]
fn restore_members_sidecar_in_paths_never_becomes_an_entry_but_travels_in_the_group() {
    // C#: ReviewCatalogTests.RestoreMembers_SidecarInPaths_NeverBecomesAnEntryButTravelsInTheGroup
    let mut c = pair_catalog(RawPairMode::PreferJpeg);
    c.restore_members(
        &s(&["C:\\photos\\p.jpg", "C:\\photos\\p.cr2", "C:\\photos\\p.xmp"]),
        1,
        None,
        true,
    );
    assert_eq!(c.count(), 3);
    assert!(!c.paths().iter().any(|p| p.to_lowercase().ends_with(".xmp")));
    assert_eq!(
        c.entries_snapshot()[1].capture_group.as_ref().unwrap().xmp_path(),
        Some("C:\\photos\\p.xmp")
    );
    assert_eq!(c.index_of("C:\\photos\\p.xmp"), -1);
}

#[test]
fn restore_members_only_a_sidecar_restores_nothing() {
    // C#: ReviewCatalogTests.RestoreMembers_OnlyASidecar_RestoresNothing
    let mut c = pair_catalog(RawPairMode::PreferJpeg);
    let version = c.structural_version();
    c.restore_members(&s(&["C:\\photos\\p.xmp"]), 0, None, true);
    assert_eq!(c.count(), 2);
    assert_eq!(c.structural_version(), version);
}

#[test]
fn restore_members_explicit_group_is_preserved_including_its_sidecar() {
    // C#: ReviewCatalogTests.RestoreMembers_ExplicitGroup_IsPreservedIncludingItsSidecar
    let mut c = pair_catalog(RawPairMode::PreferRaw);
    let group = CaptureGroup::new(
        "C:\\photos\\p.jpg",
        "C:\\photos\\p.cr2",
        Some("C:\\photos\\p.cr2.xmp".to_owned()),
    )
    .unwrap();
    let members: Vec<String> = group.image_paths().to_vec();
    c.restore_members(&members, 0, Some(&group), true);
    assert_eq!(c.path_at(0), "C:\\photos\\p.cr2");
    assert_eq!(c.entries_snapshot()[0].capture_group.as_ref(), Some(&group));
    assert_eq!(
        c.entries_snapshot()[0].capture_group.as_ref().unwrap().xmp_path(),
        Some("C:\\photos\\p.cr2.xmp")
    );
}

#[test]
fn restore_members_one_member_of_a_pair_restores_an_ungrouped_entry() {
    // C#: ReviewCatalogTests.RestoreMembers_OneMemberOfAPair_RestoresAnUngroupedEntry
    let mut c = pair_catalog(RawPairMode::PreferJpeg);
    c.restore_members(&s(&["C:\\photos\\p.cr2"]), 1, None, true);
    assert_eq!(c.path_at(1), "C:\\photos\\p.cr2");
    assert!(c.entries_snapshot()[1].capture_group.is_none());
}

#[test]
fn restore_members_keeps_current_index_on_the_displayed_image() {
    // C#: ReviewCatalogTests.RestoreMembers_KeepsCurrentIndexOnTheDisplayedImage
    let mut c = pair_catalog(RawPairMode::PreferJpeg);
    c.set_current(1);
    c.restore_members(&s(&["C:\\photos\\p.jpg", "C:\\photos\\p.cr2"]), 1, None, true);
    assert_eq!(c.current().unwrap().path(), "C:\\photos\\after.jpg");
    assert_eq!(c.current_index(), 2);
}

#[test]
fn restore_members_into_empty_catalog_makes_the_restored_entry_current() {
    // C#: ReviewCatalogTests.RestoreMembers_IntoEmptyCatalog_MakesTheRestoredEntryCurrent
    let mut c = ReviewCatalog::new();
    c.reset(Vec::new(), RawPairMode::PreferJpeg, None);
    c.restore_members(&s(&["C:\\photos\\p.jpg", "C:\\photos\\p.cr2"]), 0, None, true);
    assert_eq!(c.count(), 1);
    assert_eq!(c.current_index(), 0);
}

#[test]
fn empty_catalog_properties_and_boundaries() {
    // C#: ReviewCatalogTests.EmptyCatalog_PropertiesAndBoundaries
    let mut c = ReviewCatalog::new();
    assert_eq!(c.count(), 0);
    assert_eq!(c.current_index(), -1);
    assert!(c.current().is_none());
    assert!(c.paths().is_empty());
    assert_eq!(c.index_of("anything.jpg"), -1);
    assert!(!c.set_current(0));
    assert!(c.set_current(-1));
    assert_eq!(c.remove("anything.jpg"), -1);
}

#[test]
fn single_item_removal_empties_catalog() {
    // C#: ReviewCatalogTests.SingleItem_RemovalEmptiesCatalog
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["photo1.jpg"]);
    assert_eq!(c.count(), 1);
    assert_eq!(c.current_index(), 0);
    assert_eq!(c.current().unwrap().path(), "photo1.jpg");
    assert_eq!(c.remove("photo1.jpg"), -1);
    assert_eq!(c.count(), 0);
    assert_eq!(c.current_index(), -1);
    assert!(c.current().is_none());
}

#[test]
fn remove_middle_and_last_calculates_next_index_correctly() {
    // C#: ReviewCatalogTests.Remove_MiddleAndLast_CalculatesNextIndexCorrectly
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["img1.jpg", "img2.jpg", "img3.jpg"]);
    c.set_current(1);
    assert_eq!(c.remove("img2.jpg"), 1);
    assert_eq!(c.current_index(), 1);
    assert_eq!(c.current().unwrap().path(), "img3.jpg");
    assert_eq!(c.count(), 2);
    assert_eq!(c.remove("img3.jpg"), 0);
    assert_eq!(c.current_index(), 0);
    assert_eq!(c.current().unwrap().path(), "img1.jpg");
    assert_eq!(c.count(), 1);
}

#[test]
fn remove_entry_before_current_keeps_same_current_path() {
    // C#: ReviewCatalogTests.Remove_EntryBeforeCurrent_KeepsSameCurrentPath (RV-C09)
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["img1.jpg", "img2.jpg", "img3.jpg", "img4.jpg"]);
    c.set_current(2);
    assert_eq!(c.remove("img1.jpg"), 1);
    assert_eq!(c.current_index(), 1);
    assert_eq!(c.current().unwrap().path(), "img3.jpg");
}

#[test]
fn remove_entry_after_current_keeps_same_current_path() {
    // C#: ReviewCatalogTests.Remove_EntryAfterCurrent_KeepsSameCurrentPath (RV-C09)
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["img1.jpg", "img2.jpg", "img3.jpg", "img4.jpg"]);
    c.set_current(1);
    assert_eq!(c.remove("img4.jpg"), 1);
    assert_eq!(c.current_index(), 1);
    assert_eq!(c.current().unwrap().path(), "img2.jpg");
}

#[test]
fn restore_clamps_index_and_prevents_duplicates() {
    // C#: ReviewCatalogTests.Restore_ClampsIndexAndPreventsDuplicates
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["img1.jpg", "img3.jpg"]);
    assert!(!c.restore("img1.jpg", 0));
    assert_eq!(c.count(), 2);
    assert!(c.restore("img0.jpg", -1));
    assert_eq!(c.count(), 3);
    assert_eq!(c.paths()[0], "img0.jpg");
    assert!(c.restore("img4.jpg", 999));
    assert_eq!(c.count(), 4);
    assert_eq!(c.paths()[3], "img4.jpg");
    assert!(c.restore("img2.jpg", 2));
    assert_eq!(c.count(), 5);
    assert_eq!(c.paths()[2], "img2.jpg");
}

#[test]
fn replace_order_rejects_duplicates_even_when_count_and_members_match() {
    // C#: ReviewCatalogTests.ReplaceOrder_RejectsDuplicatesEvenWhenCountAndMembersMatch
    for (x, y, z) in [
        ("a.jpg", "a.jpg", "c.jpg"),
        ("A.JPG", "a.jpg", "c.jpg"),
        ("a.jpg", "b.jpg", "b.jpg"),
    ] {
        let mut c = ReviewCatalog::new();
        reset_paths(&mut c, &["a.jpg", "b.jpg", "c.jpg"]);
        c.set_current(1);
        let version = c.structural_version();
        assert!(!c.replace_order(&s(&[x, y, z])));
        assert_eq!(paths_vec(&c), s(&["a.jpg", "b.jpg", "c.jpg"]));
        assert_eq!(c.current_index(), 1);
        assert_eq!(c.structural_version(), version);
    }
}

#[test]
fn replace_order_preserves_current_by_path_and_rejects_mismatched_set() {
    // C#: ReviewCatalogTests.ReplaceOrder_PreservesCurrentByPath_AndRejectsMismatchedSet
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["a.jpg", "b.jpg", "c.jpg"]);
    c.set_current(2);
    assert!(!c.replace_order(&s(&["a.jpg", "b.jpg"])));
    assert_eq!(c.current_index(), 2);
    assert!(!c.replace_order(&s(&["a.jpg", "b.jpg", "d.jpg"])));
    assert_eq!(c.current_index(), 2);
    assert!(c.replace_order(&s(&["c.jpg", "a.jpg", "b.jpg"])));
    assert_eq!(c.current_index(), 0);
    assert_eq!(c.current().unwrap().path(), "c.jpg");
    assert_eq!(paths_vec(&c), s(&["c.jpg", "a.jpg", "b.jpg"]));
}

#[test]
fn paths_taken_before_a_change_is_not_mutated_by_it() {
    // C#: ReviewCatalogTests.Paths_TakenBeforeAChange_IsNotMutatedByIt
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["img1.jpg", "img2.jpg"]);
    let snapshot: Arc<[String]> = c.paths();
    assert_eq!(&snapshot[..], &s(&["img1.jpg", "img2.jpg"])[..]);
    c.remove("img1.jpg");
    assert_eq!(c.paths().len(), 1);
    assert_eq!(snapshot.len(), 2);
    assert_eq!(snapshot[0], "img1.jpg");
}

#[test]
fn remove_paths_non_current_keeps_current_by_path() {
    // C#: ReviewCatalogTests.RemovePaths_NonCurrent_KeepsCurrentByPath (AR16)
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"]);
    c.set_current(2);
    let version = c.structural_version();
    let removed = c.remove_paths(&s(&["D.JPG", "a.jpg", "ghost.jpg"]));
    assert_eq!(removed, s(&["a.jpg", "d.jpg"]));
    assert_eq!(paths_vec(&c), s(&["b.jpg", "c.jpg", "e.jpg"]));
    assert_eq!(c.current().unwrap().path(), "c.jpg");
    assert_eq!(c.current_index(), 1);
    assert_eq!(c.index_of("a.jpg"), -1);
    assert_ne!(c.structural_version(), version);
}

#[test]
fn remove_paths_current_advances_like_delete() {
    // C#: ReviewCatalogTests.RemovePaths_Current_AdvancesLikeDelete
    let cases: &[(i32, &[&str], &str)] = &[
        (1, &["b.jpg"], "c.jpg"),
        (1, &["b.jpg", "c.jpg"], "d.jpg"),
        (4, &["e.jpg"], "d.jpg"),
        (3, &["d.jpg", "e.jpg"], "c.jpg"),
        (0, &["a.jpg", "c.jpg"], "b.jpg"),
    ];
    for (current, unreadable, expected) in cases {
        let mut c = ReviewCatalog::new();
        reset_paths(&mut c, &["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"]);
        c.set_current(*current);
        c.remove_paths(&s(unreadable));
        assert_eq!(c.count(), 5 - unreadable.len());
        assert_eq!(c.current().unwrap().path(), *expected);
        assert_eq!(c.index_of(expected), c.current_index());
    }
}

#[test]
fn remove_paths_all_or_none() {
    // C#: ReviewCatalogTests.RemovePaths_AllOrNone
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["a.jpg", "b.jpg"]);
    let version = c.structural_version();
    assert!(c.remove_paths(&s(&["x.jpg"])).is_empty());
    assert_eq!(c.structural_version(), version);
    assert_eq!(c.current_index(), 0);
    assert_eq!(c.remove_paths(&s(&["a.jpg", "b.jpg"])).len(), 2);
    assert_eq!(c.count(), 0);
    assert_eq!(c.current_index(), -1);
}

#[test]
fn restore_members_second_member_comes_back_after_first_was_restored_alone_reforms_the_grouped_entry_in_place() {
    // C#: ReviewCatalogTests.RestoreMembers_SecondMemberComesBackAfterFirstWasRestoredAlone_ReformsTheGroupedEntryInPlace
    let cases = [
        (
            RawPairMode::PreferJpeg,
            "C:\\photos\\p.jpg",
            "C:\\photos\\p.cr2",
            "C:\\photos\\p.jpg",
        ),
        (
            RawPairMode::PreferJpeg,
            "C:\\photos\\p.cr2",
            "C:\\photos\\p.jpg",
            "C:\\photos\\p.cr2",
        ),
        (
            RawPairMode::PreferRaw,
            "C:\\photos\\p.jpg",
            "C:\\photos\\p.cr2",
            "C:\\photos\\p.jpg",
        ),
    ];
    for (mode, first, later, representative) in cases {
        let mut c = pair_catalog(mode);
        let group = CaptureGroup::new(
            "C:\\photos\\p.jpg",
            "C:\\photos\\p.cr2",
            Some("C:\\photos\\p.xmp".to_owned()),
        )
        .unwrap();
        c.restore_members(&s(&[first]), 1, Some(&group), true);
        assert!(c.entries_snapshot()[1].capture_group.is_none());
        c.set_current(1);
        let restored = c.restore_members(&s(&[later]), 1, Some(&group), true);
        assert!(restored);
        assert_eq!(
            paths_vec(&c),
            s(&["C:\\photos\\before.jpg", representative, "C:\\photos\\after.jpg"])
        );
        assert_eq!(c.entries_snapshot()[1].capture_group.as_ref(), Some(&group));
        assert_eq!(c.index_of("C:\\photos\\p.jpg"), 1);
        assert_eq!(c.index_of("C:\\photos\\p.cr2"), 1);
        assert_eq!(c.current_index(), 1);
    }
}

#[test]
fn restore_members_reform_of_a_non_current_entry_follows_the_pair_mode() {
    // C#: ReviewCatalogTests.RestoreMembers_ReformOfANonCurrentEntry_FollowsThePairMode
    for (mode, first, representative) in [
        (RawPairMode::PreferJpeg, "C:\\photos\\p.cr2", "C:\\photos\\p.jpg"),
        (RawPairMode::PreferRaw, "C:\\photos\\p.jpg", "C:\\photos\\p.cr2"),
    ] {
        let mut c = pair_catalog(mode);
        let group = group2("C:\\photos\\p.jpg", "C:\\photos\\p.cr2");
        c.restore_members(&s(&[first]), 1, Some(&group), true);
        c.set_current(2);
        let later = if mode == RawPairMode::PreferJpeg {
            "C:\\photos\\p.jpg"
        } else {
            "C:\\photos\\p.cr2"
        };
        c.restore_members(&s(&[later]), 1, Some(&group), true);
        assert_eq!(c.path_at(1), representative);
        assert_eq!(c.entries_snapshot()[1].capture_group.as_ref(), Some(&group));
        assert_eq!(c.current_index(), 2);
    }
}

#[test]
fn restore_members_reform_when_both_members_were_standalone_and_second_is_current_keeps_displayed_member_and_index() {
    // C#: ReviewCatalogTests.RestoreMembers_ReformWhenBothMembersWereStandaloneAndSecondIsCurrent_KeepsTheDisplayedMemberAndIndex
    let mut c = pair_catalog(RawPairMode::PreferJpeg);
    let group = group2("C:\\photos\\p.jpg", "C:\\photos\\p.cr2");
    c.restore_members(&s(&["C:\\photos\\p.jpg"]), 1, Some(&group), true);
    c.restore("C:\\photos\\p.cr2", 2);
    c.set_current(2);
    c.restore_members(&s(&["C:\\photos\\p.cr2"]), 1, Some(&group), true);
    assert_eq!(c.count(), 3);
    assert_eq!(c.current().unwrap().path(), "C:\\photos\\p.cr2");
    assert_eq!(c.current_index(), 1);
    assert_eq!(c.current().unwrap().capture_group.as_ref(), Some(&group));
}

fn grouped_catalog(mode: RawPairMode, extra_standalone: &[&str]) -> ReviewCatalog {
    let mut c = ReviewCatalog::new();
    let mut paths = vec!["C:\\photos\\p.jpg", "C:\\photos\\p.cr2"];
    paths.extend_from_slice(extra_standalone);
    reset_entries(&mut c, &paths, mode);
    c
}

#[test]
fn remove_paths_unreadable_representative_of_capture_degrades_to_the_readable_partner() {
    // C#: ReviewCatalogTests.RemovePaths_UnreadableRepresentativeOfCapture_DegradesToTheReadablePartner
    for (mode, bad, survivor) in [
        (RawPairMode::PreferRaw, "C:\\photos\\p.cr2", "C:\\photos\\p.jpg"),
        (RawPairMode::PreferJpeg, "C:\\photos\\p.jpg", "C:\\photos\\p.cr2"),
    ] {
        let mut c = grouped_catalog(mode, &["C:\\photos\\z.jpg"]);
        c.set_current(1);
        let removed = c.remove_paths(&s(&[bad]));
        assert_eq!(removed, s(&[bad]));
        assert_eq!(paths_vec(&c), s(&[survivor, "C:\\photos\\z.jpg"]));
        assert!(c.entries_snapshot()[0].capture_group.is_none());
        assert_eq!(c.index_of(bad), -1);
        assert_eq!(c.current_index(), 1);
    }
}

#[test]
fn remove_paths_unreadable_non_representative_member_keeps_representative_and_drops_the_group() {
    // C#: ReviewCatalogTests.RemovePaths_UnreadableNonRepresentativeMember_KeepsRepresentativeAndDropsTheGroup
    let mut c = grouped_catalog(RawPairMode::PreferJpeg, &[]);
    let before = c.entries_snapshot()[0].clone();
    let removed = c.remove_paths(&s(&["C:\\photos\\p.cr2"]));
    assert_eq!(removed, s(&["C:\\photos\\p.cr2"]));
    assert_eq!(paths_vec(&c), s(&["C:\\photos\\p.jpg"]));
    assert!(c.entries_snapshot()[0].capture_group.is_none());
    assert_eq!(c.entries_snapshot()[0].path(), before.path());
    assert_eq!(c.current_index(), 0);
}

#[test]
fn remove_paths_current_capture_degrades_stays_current_at_the_same_position() {
    // C#: ReviewCatalogTests.RemovePaths_CurrentCaptureDegrades_StaysCurrentAtTheSamePosition
    let mut c = grouped_catalog(RawPairMode::PreferRaw, &["C:\\photos\\z.jpg"]);
    assert_eq!(c.current_index(), 0);
    c.remove_paths(&s(&["C:\\photos\\p.cr2"]));
    assert_eq!(c.current_index(), 0);
    assert_eq!(c.current().unwrap().path(), "C:\\photos\\p.jpg");
    assert_eq!(c.count(), 2);
}

#[test]
fn remove_paths_both_image_members_of_capture_removes_the_entry_and_reports_both() {
    // C#: ReviewCatalogTests.RemovePaths_BothImageMembersOfCapture_RemovesTheEntryAndReportsBoth
    let mut c = grouped_catalog(RawPairMode::PreferJpeg, &["C:\\photos\\z.jpg"]);
    let removed = c.remove_paths(&s(&["C:\\photos\\p.cr2", "C:\\photos\\p.jpg"]));
    assert_eq!(removed, s(&["C:\\photos\\p.jpg", "C:\\photos\\p.cr2"]));
    assert_eq!(paths_vec(&c), s(&["C:\\photos\\z.jpg"]));
    assert_eq!(c.current_index(), 0);
}

#[test]
fn restore_members_partner_still_missing_keeps_the_independent_entry_ungrouped() {
    // C#: ReviewCatalogTests.RestoreMembers_PartnerStillMissing_KeepsTheIndependentEntryUngrouped
    let mut c = pair_catalog(RawPairMode::PreferJpeg);
    let group = group2("C:\\photos\\p.jpg", "C:\\photos\\p.cr2");
    c.restore_members(&s(&["C:\\photos\\p.jpg"]), 1, Some(&group), true);
    let restored = c.restore_members(&s(&["C:\\photos\\p.jpg"]), 1, Some(&group), true);
    assert!(!restored);
    assert_eq!(c.count(), 3);
    assert!(c.entries_snapshot()[1].capture_group.is_none());
}

#[test]
fn restore_members_reform_keeps_current_index_on_the_photo_after_the_group() {
    // C#: ReviewCatalogTests.RestoreMembers_ReformKeepsCurrentIndexOnThePhotoAfterTheGroup
    let mut c = pair_catalog(RawPairMode::PreferJpeg);
    let group = group2("C:\\photos\\p.jpg", "C:\\photos\\p.cr2");
    c.restore_members(&s(&["C:\\photos\\p.jpg"]), 1, Some(&group), true);
    c.set_current(2);
    c.restore_members(&s(&["C:\\photos\\p.cr2"]), 1, Some(&group), true);
    assert_eq!(c.count(), 3);
    assert_eq!(c.current_index(), 2);
    assert_eq!(c.current().unwrap().path(), "C:\\photos\\after.jpg");
}

// ---------------------------------------------------------------------------------------------------------------
// ReviewCatalogMutationGapTests
// ---------------------------------------------------------------------------------------------------------------

fn pp(name: &str) -> String {
    format!("C:\\p\\{name}")
}

fn pairing(names: &[&str]) -> ReviewCatalog {
    let mut c = ReviewCatalog::new();
    let paths: Vec<String> = names.iter().map(|n| pp(n)).collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    reset_entries(&mut c, &refs, RawPairMode::PreferJpeg);
    c
}

fn plain(names: &[&str]) -> ReviewCatalog {
    let mut c = ReviewCatalog::new();
    let paths: Vec<String> = names.iter().map(|n| pp(n)).collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    reset_paths(&mut c, &refs);
    c
}

#[test]
fn remove_entry_whose_member_path_is_also_another_entry_keeps_finding_the_survivor_by_that_path() {
    // C#: ReviewCatalogMutationGapTests.Remove_EntryWhoseMemberPathIsAlsoAnotherEntry_KeepsFindingTheSurvivorByThatPath
    let group = group2(&pp("A.jpg"), &pp("A.cr2"));
    let mut c = ReviewCatalog::new();
    let mut grouped = entry(&pp("A.jpg"));
    grouped.capture_group = Some(group.clone());
    c.reset(vec![entry(&pp("A.cr2")), grouped], RawPairMode::Separate, None);
    assert_eq!(c.index_of(&pp("A.cr2")), 0);

    c.remove(&pp("A.cr2"));

    assert_eq!(c.count(), 1);
    assert_eq!(c.index_of(&pp("A.cr2")), 0);
    assert_eq!(c.find(&pp("A.cr2")).unwrap().capture_group.as_ref(), Some(&group));
}

#[test]
fn remove_after_the_maximum_number_of_incremental_removals_forces_an_index_rebuild() {
    // C#: ReviewCatalogMutationGapTests.Remove_AfterTheMaximumNumberOfIncrementalRemovals_ForcesAnIndexRebuild
    let names: Vec<String> = (0..200).map(|i| format!("p{i:03}.jpg")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut c = plain(&refs);
    assert_eq!(c.index_of(&pp(&names[0])), 0);
    let baseline = c.index_rebuild_count_for_tests();

    for name in names.iter().take(MAX_REMOVED_BEFORE_REBUILD) {
        c.remove(&pp(name));
    }
    assert_eq!(c.index_rebuild_count_for_tests(), baseline);

    c.remove(&pp(&names[64]));
    assert_eq!(c.index_of(&pp(&names[199])), 135 - 1);
    assert_eq!(c.index_rebuild_count_for_tests(), baseline + 1);
}

#[test]
fn restore_members_path_already_listed_at_index_zero_is_not_added_again() {
    // C#: ReviewCatalogMutationGapTests.RestoreMembers_PathAlreadyListedAtIndexZero_IsNotAddedAgain
    let mut c = plain(&["a.jpg", "b.jpg"]);
    let restored = c.restore_members(&s(&[&pp("a.jpg")]), 1, None, true);
    assert!(!restored);
    assert_eq!(paths_vec(&c), s(&[&pp("a.jpg"), &pp("b.jpg")]));
}

#[test]
fn restore_members_current_photo_keeps_being_the_displayed_one() {
    // C#: ReviewCatalogMutationGapTests.RestoreMembers_CurrentPhotoKeepsBeingTheDisplayedOne
    for (insert_at, expected_current) in [(0, 1), (1, 0)] {
        let mut c = plain(&["a.jpg", "b.jpg"]);
        assert_eq!(c.current_index(), 0);
        assert!(c.restore_members(&s(&[&pp("c.jpg")]), insert_at, None, true));
        assert_eq!(c.current_index(), expected_current);
        assert_eq!(c.current().unwrap().path(), pp("a.jpg"));
    }
}

#[test]
fn restore_members_into_an_empty_catalog_selects_the_restored_entry() {
    // C#: ReviewCatalogMutationGapTests.RestoreMembers_IntoAnEmptyCatalog_SelectsTheRestoredEntry
    let mut c = ReviewCatalog::new();
    assert!(c.restore_members(&s(&[&pp("c.jpg")]), 0, None, true));
    assert_eq!(c.current_index(), 0);
    assert_eq!(c.current().unwrap().path(), pp("c.jpg"));
}

#[test]
fn restore_members_second_member_returns_while_the_first_is_listed_at_index_zero_folds_them_into_one_grouped_entry() {
    // C#: ReviewCatalogMutationGapTests.RestoreMembers_SecondMemberReturnsWhileTheFirstIsListedAtIndexZero_FoldsThemIntoOneGroupedEntry
    let mut c = pairing(&["a.jpg"]);
    let group = group2(&pp("a.jpg"), &pp("a.cr2"));
    let restored = c.restore_members(&s(&[&pp("a.cr2")]), 1, Some(&group), true);
    assert!(restored);
    let snapshot = c.entries_snapshot();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].capture_group.as_ref(), Some(&group));
    assert_eq!(snapshot[0].path(), pp("a.jpg"));
    assert_eq!(c.current_index(), 0);
}

#[test]
fn restore_members_both_members_listed_separately_folds_them_and_keeps_the_current_photo() {
    // C#: ReviewCatalogMutationGapTests.RestoreMembers_BothMembersListedSeparately_FoldsThemAndKeepsTheCurrentPhoto
    let cases = [
        (0, 0, "x.jpg", false),
        (4, 3, "z.jpg", false),
        (3, 1, "a.cr2", true),
        (1, 1, "a.jpg", true),
    ];
    for (start_current, expected_current, expected_path, grouped) in cases {
        let mut c = pairing(&["x.jpg", "a.jpg", "y.jpg", "z.jpg"]);
        assert!(c.restore_members(&s(&[&pp("a.cr2")]), 3, None, true));
        assert_eq!(
            paths_vec(&c),
            s(&[&pp("x.jpg"), &pp("a.jpg"), &pp("y.jpg"), &pp("a.cr2"), &pp("z.jpg")])
        );
        assert!(c.set_current(start_current));
        let group = group2(&pp("a.jpg"), &pp("a.cr2"));
        let restored = c.restore_members(&[], 0, Some(&group), true);
        assert!(restored);
        assert_eq!(c.count(), 4);
        assert_eq!(c.current_index(), expected_current);
        assert_eq!(c.current().unwrap().path(), pp(expected_path));
        assert_eq!(c.current().unwrap().capture_group.is_some(), grouped);
    }
}

#[test]
fn replace_order_member_order_naming_an_unknown_path_is_rejected_without_throwing() {
    // C#: ReviewCatalogMutationGapTests.ReplaceOrder_MemberOrderNamingAnUnknownPath_IsRejectedWithoutThrowing
    let mut c = pairing(&["a.jpg", "a.cr2", "b.jpg"]);
    assert!(!c.replace_order(&s(&[&pp("a.jpg"), &pp("a.cr2"), &pp("zzz.jpg")])));
    assert_eq!(paths_vec(&c), s(&[&pp("a.jpg"), &pp("b.jpg")]));
}

#[test]
fn replace_order_member_order_repeating_a_member_is_rejected() {
    // C#: ReviewCatalogMutationGapTests.ReplaceOrder_MemberOrderRepeatingAMember_IsRejected
    let mut c = pairing(&["a.jpg", "a.cr2", "b.jpg"]);
    assert!(!c.replace_order(&s(&[&pp("a.jpg"), &pp("a.jpg"), &pp("b.jpg")])));
    assert_eq!(paths_vec(&c), s(&[&pp("a.jpg"), &pp("b.jpg")]));
}

#[test]
fn replace_order_valid_member_order_reorders_the_grouped_entries() {
    // C#: ReviewCatalogMutationGapTests.ReplaceOrder_ValidMemberOrder_ReordersTheGroupedEntries
    let mut c = pairing(&["a.jpg", "a.cr2", "b.jpg"]);
    assert!(c.replace_order(&s(&[&pp("b.jpg"), &pp("a.cr2"), &pp("a.jpg")])));
    assert_eq!(paths_vec(&c), s(&[&pp("b.jpg"), &pp("a.jpg")]));
}

#[test]
fn remove_paths_unrelated_entry_removed_leaves_an_untouched_capture_group_intact() {
    // C#: ReviewCatalogMutationGapTests.RemovePaths_UnrelatedEntryRemoved_LeavesAnUntouchedCaptureGroupIntact
    let mut c = pairing(&["a.jpg", "a.cr2", "b.jpg", "c.jpg"]);
    let removed = c.remove_paths(&s(&[&pp("c.jpg")]));
    assert_eq!(removed, s(&[&pp("c.jpg")]));
    assert_eq!(paths_vec(&c), s(&[&pp("a.jpg"), &pp("b.jpg")]));
    assert!(c.entries_snapshot()[0].capture_group.is_some());
}

// ---------------------------------------------------------------------------------------------------------------
// Deterministic RNG (no `rand` crate)
// ---------------------------------------------------------------------------------------------------------------

struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        // Scramble the seed so small seeds do not start with a run of zeros.
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
        if state == 0 {
            state = 0x2545_F491_4F6C_DD1D;
        }
        Self(state)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// `0..n`.
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() >> 11) as usize % n
    }

    /// `lo..hi` (exclusive).
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo) as usize) as i32
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// ReviewCatalogRemoveIndexTests
// ---------------------------------------------------------------------------------------------------------------

fn img(i: usize) -> String {
    format!("C:\\photos\\img{i:05}.jpg")
}

fn build(count: usize) -> ReviewCatalog {
    let mut c = ReviewCatalog::new();
    let paths: Vec<String> = (0..count).map(img).collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    reset_paths(&mut c, &refs);
    c
}

fn assert_matches_model(c: &ReviewCatalog, model: &[String]) {
    assert_eq!(c.count(), model.len());
    for (i, path) in model.iter().enumerate() {
        assert_eq!(c.index_of(&path.to_uppercase()), i as i32);
    }
}

#[test]
fn remove_burst_does_not_rebuild_index_per_removal() {
    // C#: ReviewCatalogRemoveIndexTests.Remove_Burst_DoesNotRebuildIndexPerRemoval
    let mut c = build(500);
    assert_eq!(c.index_of(&img(0)), 0);
    let rebuilds = c.index_rebuild_count_for_tests();
    for _ in 0..30 {
        let victim = c.path_at(100).to_owned();
        c.remove(&victim);
        let next = c.path_at(100).to_owned();
        assert_eq!(c.index_of(&next), 100);
    }
    assert_eq!(c.index_rebuild_count_for_tests(), rebuilds);
}

#[test]
fn index_stays_correct_against_list_model() {
    // C#: ReviewCatalogRemoveIndexTests.Index_StaysCorrect_AgainstListModel
    let mut c = build(300);
    let mut model: Vec<String> = (0..300).map(img).collect();
    let mut rng = XorShift::new(12345);
    let mut removed: Vec<String> = Vec::new();

    for step in 0..600 {
        match rng.below(6) {
            0..=2 if !model.is_empty() => {
                let at = rng.below(model.len());
                let victim = model.remove(at);
                c.remove(&victim.to_uppercase());
                removed.push(victim.clone());
                assert_eq!(c.index_of(&victim), -1);
            }
            3 if !removed.is_empty() => {
                let back = removed.pop().unwrap();
                let at = rng.below(model.len() + 1);
                assert!(c.restore(&back, at as i32));
                model.insert(at, back);
            }
            4 if model.len() > 1 => {
                let at = rng.below(model.len());
                let front = model.remove(at);
                model.insert(0, front);
                assert!(c.replace_order(&model));
            }
            5 if !removed.is_empty() => {
                let ins = removed.remove(0);
                let idx = rng.below(model.len() + 1);
                assert!(c.restore(&ins, idx as i32));
                model.insert(idx, ins);
            }
            _ => {}
        }
        if step % 25 == 0 {
            assert_matches_model(&c, &model);
        }
    }
    assert_matches_model(&c, &model);
    assert_eq!(paths_vec(&c), model);
}

#[test]
fn remove_past_threshold_still_correct() {
    // C#: ReviewCatalogRemoveIndexTests.Remove_PastThreshold_StillCorrect
    let mut c = build(400);
    let mut model: Vec<String> = (0..400).map(img).collect();
    let _ = c.index_of(&img(0));
    for i in 0..150 {
        let at = (i * 7) % model.len();
        let victim = model.remove(at);
        c.remove(&victim);
    }
    assert_matches_model(&c, &model);
}

// ---------------------------------------------------------------------------------------------------------------
// ReviewCatalogRobustnessTests
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn reset_skips_null_and_blank() {
    // C#: ReviewCatalogRobustnessTests.Reset_SkipsNullAndBlank
    // Null entries do not exist in Rust and a `CatalogEntry` cannot hold a blank path, so only the blank-string half
    // remains: the test-support helper drops blank strings before constructing entries, and `CatalogEntry::new` itself
    // rejects them (W1-07).
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["", "  ", "C:\\a.jpg"]);
    assert_eq!(paths_vec(&c), s(&["C:\\a.jpg"]));
    assert!(CatalogEntry::new("  ").is_err());
    reset_entries(&mut c, &["C:\\b.jpg"], RawPairMode::Separate);
    assert_eq!(paths_vec(&c), s(&["C:\\b.jpg"]));
}

#[test]
fn replace_order_not_a_permutation_returns_false_and_keeps_order() {
    // C#: ReviewCatalogRobustnessTests.ReplaceOrder_NotAPermutation_ReturnsFalseAndKeepsOrder
    for (p0, p1, p2) in [
        ("C:\\a.jpg", "C:\\a.jpg", "C:\\c.jpg"),
        ("C:\\a.jpg", "C:\\b.jpg", "C:\\x.jpg"),
        ("C:\\A.JPG", "C:\\a.jpg", "C:\\c.jpg"),
    ] {
        let mut c = ReviewCatalog::new();
        reset_paths(&mut c, &["C:\\a.jpg", "C:\\b.jpg", "C:\\c.jpg"]);
        c.set_current(1);
        let version = c.structural_version();
        assert!(!c.replace_order(&s(&[p0, p1, p2])));
        assert_eq!(paths_vec(&c), s(&["C:\\a.jpg", "C:\\b.jpg", "C:\\c.jpg"]));
        assert_eq!(c.current_index(), 1);
        assert_eq!(c.structural_version(), version);
    }
}

#[test]
fn replace_order_null_element_returns_false() {
    // C#: ReviewCatalogRobustnessTests.ReplaceOrder_NullElement_ReturnsFalse
    // Not applicable in Rust (a `&[String]` has no null elements). The nearest reachable case, an element that is not a
    // known path (an empty string), is rejected the same way.
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["C:\\a.jpg", "C:\\b.jpg"]);
    assert!(!c.replace_order(&s(&["C:\\a.jpg", ""])));
    assert_eq!(paths_vec(&c), s(&["C:\\a.jpg", "C:\\b.jpg"]));
}

#[test]
fn replace_order_catalog_with_case_variant_duplicates_does_not_throw() {
    // C#: ReviewCatalogRobustnessTests.ReplaceOrder_CatalogWithCaseVariantDuplicates_DoesNotThrow
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["C:\\a.jpg", "C:\\A.JPG"]);
    let accepted = c.replace_order(&s(&["C:\\A.JPG", "C:\\a.jpg"]));
    // The permutation check sees one distinct key for two entries and rejects the order; nothing panics.
    assert!(!accepted);
    assert_eq!(c.count(), 2);
}

#[test]
fn replace_order_keeps_current_by_path() {
    // C#: ReviewCatalogRobustnessTests.ReplaceOrder_KeepsCurrentByPath
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["C:\\a.jpg", "C:\\b.jpg", "C:\\c.jpg"]);
    c.set_current(2);
    assert!(c.replace_order(&s(&["C:\\c.jpg", "C:\\b.jpg", "C:\\a.jpg"])));
    assert_eq!(c.current().unwrap().path(), "C:\\c.jpg");
    assert_eq!(c.current_index(), 0);
}

#[test]
fn restore_blank_path_rejected() {
    // C#: ReviewCatalogRobustnessTests.Restore_BlankPath_Rejected
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["C:\\a.jpg"]);
    let version = c.structural_version();
    assert!(!c.restore(" ", 0));
    assert_eq!(c.count(), 1);
    assert_eq!(c.structural_version(), version);
}

/// Straightforward list model of the catalog used as the oracle.
struct Model {
    paths: Vec<String>,
    current: i32,
}

impl Model {
    fn index_of(&self, p: &str) -> i32 {
        self.paths
            .iter()
            .position(|x| eq_ignore_case(x, p))
            .map_or(-1, |i| i as i32)
    }

    fn reset(&mut self, p: Vec<String>) {
        self.current = if p.is_empty() { -1 } else { 0 };
        self.paths = p;
    }

    fn remove(&mut self, p: &str) {
        let i = self.index_of(p);
        if i < 0 {
            return;
        }
        self.paths.remove(i as usize);
        if self.paths.is_empty() {
            self.current = -1;
        } else if self.current >= 0 && i < self.current {
            self.current -= 1;
        } else if self.current < 0 || i == self.current {
            self.current = i.min(self.paths.len() as i32 - 1);
        }
    }

    fn restore(&mut self, p: &str, index: i32) {
        if self.index_of(p) >= 0 {
            return;
        }
        let at = index.clamp(0, self.paths.len() as i32);
        self.paths.insert(at as usize, p.to_owned());
        if self.current == -1 {
            self.current = at;
        } else if at <= self.current {
            self.current += 1;
        }
    }

    fn replace_order(&mut self, order: Vec<String>) {
        let cur = if self.current >= 0 {
            Some(self.paths[self.current as usize].clone())
        } else {
            None
        };
        self.paths = order;
        self.current = match cur {
            None => {
                if self.paths.is_empty() {
                    -1
                } else {
                    0
                }
            }
            Some(cur) => self.index_of(&cur),
        };
    }
}

#[test]
fn random_operations_match_reference_model() {
    // C#: ReviewCatalogRobustnessTests.RandomOperations_MatchReferenceModel (seeds 1..=8)
    for seed in 1..=8u64 {
        let mut rng = XorShift::new(seed);
        let mut c = ReviewCatalog::new();
        let mut model = Model {
            paths: Vec::new(),
            current: -1,
        };
        let name = |rng: &mut XorShift, n: i32| -> String {
            let prefix = if rng.below(2) == 0 {
                "C:\\Photos\\img"
            } else {
                "C:\\PHOTOS\\IMG"
            };
            format!("{prefix}{n}.jpg")
        };

        for step in 0..400 {
            let before = c.structural_version();
            match rng.below(5) {
                0 => {
                    let mut universe: Vec<i32> = (0..12).collect();
                    rng.shuffle(&mut universe);
                    let take = rng.range(0, 9) as usize;
                    let set: Vec<String> = universe
                        .iter()
                        .take(take)
                        .map(|n| format!("C:\\Photos\\img{n}.jpg"))
                        .collect();
                    let refs: Vec<&str> = set.iter().map(String::as_str).collect();
                    reset_paths(&mut c, &refs);
                    model.reset(set);
                }
                1 => {
                    let n = rng.range(0, 12);
                    let p = name(&mut rng, n);
                    c.remove(&p);
                    model.remove(&p);
                }
                2 => {
                    let n = rng.range(0, 12);
                    let p = name(&mut rng, n);
                    let i = rng.range(-2, 14);
                    c.restore(&p, i);
                    model.restore(&p, i);
                }
                3 => {
                    let mut order = model.paths.clone();
                    rng.shuffle(&mut order);
                    if rng.below(3) == 0 && !order.is_empty() {
                        order[0] = order[0].to_uppercase();
                    }
                    assert!(c.replace_order(&order), "seed {seed} step {step}");
                    model.replace_order(order);
                }
                _ => {
                    if !model.paths.is_empty() {
                        let i = rng.below(model.paths.len()) as i32;
                        assert!(c.set_current(i));
                        model.current = i;
                    }
                }
            }

            let expected: Vec<String> = model.paths.iter().map(|p| p.to_lowercase()).collect();
            let actual: Vec<String> = c.paths().iter().map(|p| p.to_lowercase()).collect();
            assert_eq!(expected, actual, "seed {seed} step {step}");
            assert_eq!(model.current, c.current_index(), "seed {seed} step {step}");
            assert_eq!(model.paths.is_empty(), c.current().is_none());
            assert!(c.structural_version() >= before);
            for i in 0..c.count() {
                assert_eq!(c.index_of(c.path_at(i)), i as i32);
                assert_eq!(c.index_of(&c.path_at(i).to_uppercase()), i as i32);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// ReviewCatalogThreadGuardTests
// ---------------------------------------------------------------------------------------------------------------

const GUARD_PATHS: [&str; 3] = ["C:\\p\\a.jpg", "C:\\p\\b.jpg", "C:\\p\\c.jpg"];

#[test]
fn unbound_mutated_from_other_thread_does_not_throw() {
    // C#: ReviewCatalogThreadGuardTests.Unbound_MutatedFromOtherThread_DoesNotThrow
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &GUARD_PATHS);
    let c = std::thread::spawn(move || {
        c.set_current(1);
        c.remove(GUARD_PATHS[0]);
        reset_paths(&mut c, &GUARD_PATHS);
        c
    })
    .join()
    .unwrap();
    assert_eq!(c.count(), 3);
}

#[test]
fn bound_mutated_on_owner_thread_does_not_throw() {
    // C#: ReviewCatalogThreadGuardTests.Bound_MutatedOnOwnerThread_DoesNotThrow
    let mut c = ReviewCatalog::new();
    c.bind_to_current_thread();
    reset_paths(&mut c, &GUARD_PATHS);
    assert!(c.set_current(2));
    assert!(c.replace_order(&s(&[GUARD_PATHS[2], GUARD_PATHS[1], GUARD_PATHS[0]])));
    assert!(c.update_metadata(GUARD_PATHS[1], 10, stamp(), None, None));
    assert!(c.set_current(0));
    assert_eq!(c.remove(GUARD_PATHS[1]), 0);
    assert!(c.restore(GUARD_PATHS[1], 1));
    assert_eq!(c.count(), 3);
}

#[test]
fn bound_mutated_from_other_thread_throws_only_in_debug() {
    // C#: ReviewCatalogThreadGuardTests.Bound_MutatedFromOtherThread_ThrowsOnlyInDebug
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &GUARD_PATHS);
    c.bind_to_current_thread();

    // The catalog travels to the other thread (it is `Send`, not `Sync`); the thread hands it back with the outcome.
    let (observed, c) = std::thread::spawn(move || {
        let mut c = c;
        let outcome = catch_unwind(AssertUnwindSafe(|| c.set_current(1)));
        let message = outcome.err().map(|payload| {
            payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|m| (*m).to_owned()))
                .unwrap_or_default()
        });
        (message, c)
    })
    .join()
    .unwrap();

    #[cfg(debug_assertions)]
    {
        let message = observed.expect("the guard must panic in debug builds");
        assert!(
            message.contains("ReviewCatalog.SetCurrent called on thread"),
            "unexpected panic message: {message}"
        );
        assert_eq!(c.current_index(), 0);
    }
    #[cfg(not(debug_assertions))]
    {
        assert!(observed.is_none());
        assert_eq!(c.current_index(), 1);
    }

    // The same call without catching: the worker thread panics and `join()` reports it (debug only).
    let mut c2 = ReviewCatalog::new();
    reset_paths(&mut c2, &GUARD_PATHS);
    c2.bind_to_current_thread();
    let joined = std::thread::spawn(move || {
        let mut c2 = c2;
        c2.set_current(1)
    })
    .join();
    #[cfg(debug_assertions)]
    assert!(joined.is_err());
    #[cfg(not(debug_assertions))]
    assert_eq!(joined.ok(), Some(true));
}

#[test]
fn bound_read_from_other_thread_does_not_throw() {
    // C#: ReviewCatalogThreadGuardTests.Bound_ReadFromOtherThread_DoesNotThrow
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &GUARD_PATHS);
    c.bind_to_current_thread();
    let snapshot = std::thread::spawn(move || (c.entries_snapshot().len(), c.index_of(GUARD_PATHS[1])))
        .join()
        .unwrap();
    assert_eq!(snapshot, (3, 1));
}

// ---------------------------------------------------------------------------------------------------------------
// CaptureGroupBuilderTests (the three catalog-using tests)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn group_entries_uses_selected_representative_and_keeps_both_members_addressable() {
    // C#: CaptureGroupBuilderTests.GroupEntries_UsesSelectedRepresentativeAndKeepsBothMembersAddressable
    let mut jpeg = entry("C:\\photos\\a.jpg");
    jpeg.length = Some(10);
    jpeg.width = Some(3000);
    let mut raw = entry("C:\\photos\\a.cr2");
    raw.length = Some(100);
    raw.width = Some(6000);
    let other = entry("C:\\photos\\b.jpg");
    let mut c = ReviewCatalog::new();
    c.reset(
        vec![jpeg.clone(), raw.clone(), other],
        RawPairMode::PreferJpeg,
        Some(&s(&["C:\\photos\\a.xmp"])),
    );
    assert_eq!(c.count(), 2);
    assert_eq!(c.path_at(0), jpeg.path());
    assert_eq!(c.index_of(raw.path()), 0);
    assert_eq!(c.find(raw.path()).unwrap().length, Some(10));
    assert_eq!(
        c.current().unwrap().capture_group.as_ref().unwrap().paths(),
        &s(&[jpeg.path(), raw.path(), "C:\\photos\\a.xmp"])[..]
    );
}

#[test]
fn group_entries_prefer_raw_uses_raw_metadata_and_explorer_order_collapses_members() {
    // C#: CaptureGroupBuilderTests.GroupEntries_PreferRawUsesRawMetadata_AndExplorerOrderCollapsesMembers
    let mut jpeg = entry("C:\\photos\\a.jpg");
    jpeg.length = Some(10);
    let mut raw = entry("C:\\photos\\a.cr2");
    raw.length = Some(100);
    let other = entry("C:\\photos\\b.jpg");
    let mut c = ReviewCatalog::new();
    c.reset(
        vec![jpeg.clone(), raw.clone(), other.clone()],
        RawPairMode::PreferRaw,
        None,
    );
    assert_eq!(c.path_at(0), raw.path());
    assert_eq!(c.find(jpeg.path()).unwrap().length, Some(100));
    assert!(c.replace_order(&s(&[other.path(), raw.path(), jpeg.path()])));
    assert_eq!(paths_vec(&c), s(&[other.path(), raw.path()]));
    assert_eq!(c.index_of(jpeg.path()), 1);
}

#[test]
fn reset_default_mode_keeps_pair_members_separate() {
    // C#: CaptureGroupBuilderTests.Reset_DefaultModeKeepsPairMembersSeparate
    let mut c = ReviewCatalog::new();
    reset_paths(&mut c, &["C:\\photos\\a.jpg", "C:\\photos\\a.cr2"]);
    assert_eq!(c.count(), 2);
    assert!(no_groups(&c));
}

// ---------------------------------------------------------------------------------------------------------------
// Golden `review-catalog`
// ---------------------------------------------------------------------------------------------------------------

fn golden_group(v: &Value) -> Option<CaptureGroup> {
    if !v.is_object() {
        return None;
    }
    let xmp = v["xmp"].as_str().map(str::to_owned);
    Some(CaptureGroup::new(v["jpeg"].as_str().unwrap(), v["raw"].as_str().unwrap(), xmp).unwrap())
}

fn golden_entry(v: &Value) -> CatalogEntry {
    let mut e = CatalogEntry::new(v["path"].as_str().unwrap()).unwrap();
    if let Some(length) = v["length"].as_i64() {
        e.length = Some(length);
    }
    if let Some(width) = v["width"].as_i64() {
        e.width = Some(width as i32);
    }
    e.capture_group = golden_group(&v["group"]);
    e
}

fn golden_strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().map(|x| x.as_str().unwrap().to_owned()).collect())
        .unwrap_or_default()
}

fn group_json(g: &Option<CaptureGroup>) -> Value {
    match g {
        None => Value::Null,
        Some(g) => {
            let mut map = serde_json::Map::new();
            map.insert("jpeg".to_owned(), Value::String(g.jpeg_path().to_owned()));
            map.insert("raw".to_owned(), Value::String(g.raw_path().to_owned()));
            map.insert(
                "xmp".to_owned(),
                g.xmp_path().map_or(Value::Null, |x| Value::String(x.to_owned())),
            );
            Value::Object(map)
        }
    }
}

fn entry_json(e: &CatalogEntry) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("path".to_owned(), Value::String(e.path().to_owned()));
    map.insert("length".to_owned(), e.length.map_or(Value::Null, Value::from));
    map.insert("width".to_owned(), e.width.map_or(Value::Null, Value::from));
    map.insert(
        "last_write_ticks".to_owned(),
        e.last_write_utc.map_or(Value::Null, |t| Value::from(t.ticks())),
    );
    map.insert("group".to_owned(), group_json(&e.capture_group));
    Value::Object(map)
}

fn check_state(c: &ReviewCatalog, probes: &[String], step: &Value, label: &str) {
    assert_eq!(c.count() as i64, step["count"].as_i64().unwrap(), "{label}: count");
    assert_eq!(
        i64::from(c.current_index()),
        step["current_index"].as_i64().unwrap(),
        "{label}: current_index"
    );
    assert_eq!(
        c.current().map(|e| e.path().to_owned()),
        step["current_path"].as_str().map(str::to_owned),
        "{label}: current_path"
    );
    assert_eq!(
        i64::from(c.structural_version()),
        step["structural_version"].as_i64().unwrap(),
        "{label}: structural_version"
    );
    assert_eq!(paths_vec(c), golden_strings(&step["paths"]), "{label}: paths");
    let entries: Vec<Value> = c.entries_snapshot().iter().map(entry_json).collect();
    assert_eq!(Value::Array(entries), step["entries"], "{label}: entries");
    for probe in probes {
        assert_eq!(
            i64::from(c.index_of(probe)),
            step["index_of"][probe.as_str()].as_i64().unwrap(),
            "{label}: index_of({probe})"
        );
    }
}

fn parse_mode(v: &Value) -> RawPairMode {
    match v.as_str().unwrap_or("Separate") {
        "PreferJpeg" => RawPairMode::PreferJpeg,
        "PreferRaw" => RawPairMode::PreferRaw,
        _ => RawPairMode::Separate,
    }
}

#[test]
fn golden_review_catalog_traces_match_csharp() {
    // C#: oracle `review-catalog` (Commands.ReviewCatalogTrace), 101 cases / 1877 steps.
    let g = golden::load("review-catalog");
    assert_eq!(g.cases.len(), 101);
    let mut total_steps = 0usize;
    for case in &g.cases {
        let name = golden::str_of(case, "name");
        let probes = golden_strings(&case["probes"]);
        let ops = case["ops"].as_array().unwrap();
        let steps = case["steps"].as_array().unwrap();
        assert_eq!(steps.len(), ops.len() + 1, "{name}: step count");
        let mut c = ReviewCatalog::new();
        check_state(&c, &probes, &steps[0], &format!("{name}#new"));
        total_steps += 1;
        for (i, op) in ops.iter().enumerate() {
            let kind = op["op"].as_str().unwrap();
            let step = &steps[i + 1];
            let result: Value = match kind {
                "reset" => {
                    let entries: Vec<CatalogEntry> =
                        op["entries"].as_array().unwrap().iter().map(golden_entry).collect();
                    let sidecars = op["sidecars"].as_array().map(|_| golden_strings(&op["sidecars"]));
                    c.reset(entries, parse_mode(&op["mode"]), sidecars.as_deref());
                    Value::Null
                }
                "setCurrent" => Value::Bool(c.set_current(op["index"].as_i64().unwrap() as i32)),
                "remove" => Value::from(c.remove(op["path"].as_str().unwrap())),
                "restore" => Value::Bool(c.restore(op["path"].as_str().unwrap(), op["index"].as_i64().unwrap() as i32)),
                "restoreMembers" => {
                    let group = golden_group(&op["group"]);
                    let raw_enabled = op["rawEnabled"].as_bool().unwrap_or(true);
                    Value::Bool(c.restore_members(
                        &golden_strings(&op["paths"]),
                        op["index"].as_i64().unwrap() as i32,
                        group.as_ref(),
                        raw_enabled,
                    ))
                }
                "replaceOrder" => Value::Bool(c.replace_order(&golden_strings(&op["paths"]))),
                "removePaths" => Value::Array(
                    c.remove_paths(&golden_strings(&op["paths"]))
                        .into_iter()
                        .map(Value::String)
                        .collect(),
                ),
                "updateMetadata" => Value::Bool(c.update_metadata(
                    op["path"].as_str().unwrap(),
                    op["length"].as_i64().unwrap(),
                    DateTimeUtc::from_ticks(op["ticks"].as_i64().unwrap()).unwrap(),
                    op["width"].as_i64().map(|w| w as i32),
                    op["height"].as_i64().map(|h| h as i32),
                )),
                other => panic!("unknown op {other}"),
            };
            let label = format!("{name}#{i}:{kind}");
            assert_eq!(result, step["result"], "{label}: result");
            check_state(&c, &probes, step, &label);
            total_steps += 1;
        }
    }
    assert_eq!(total_steps, 1877);
    // Guard against a vacuous run: the reference really exercised distinct case names.
    let names: HashSet<&str> = g.cases.iter().map(|c| golden::str_of(c, "name")).collect();
    assert_eq!(names.len(), 101);
}
