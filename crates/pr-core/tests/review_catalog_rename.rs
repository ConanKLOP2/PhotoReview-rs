//! FEAT-RENAME catalog tests ported from C# baseline 7eaee2a2.
use std::thread;

use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::CatalogEntry;
use pr_core::file_actions::rename::PathKeyMap;
use pr_core::model::RawPairMode;
use pr_core::time::DateTimeUtc;
use pr_test_support::TempRoot;

const TICKS: i64 = 638_712_864_000_000_000;

fn p(root: &TempRoot, name: &str) -> String {
    root.join(name).to_string_lossy().into_owned()
}

fn entry(path: String) -> CatalogEntry {
    CatalogEntry::new(path).expect("valid TempRoot path")
}

fn build(root: &TempRoot, names: &[&str]) -> ReviewCatalog {
    let stamp = DateTimeUtc::from_ticks(TICKS).expect("valid ticks");
    let entries = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            entry(p(root, name)).with_metadata(
                100 + index as i64,
                stamp,
                Some(10 + index as i32),
                Some(20 + index as i32),
            )
        })
        .collect();
    let mut catalog = ReviewCatalog::new();
    catalog.reset(entries, RawPairMode::Separate, None);
    catalog
}

fn build_paired(root: &TempRoot, mode: RawPairMode, sidecars: &[&str]) -> ReviewCatalog {
    let entries = ["a.jpg", "a.cr2", "b.jpg", "c.jpg"]
        .into_iter()
        .map(|name| entry(p(root, name)))
        .collect();
    let sidecars: Vec<String> = sidecars.iter().map(|name| p(root, name)).collect();
    let mut catalog = ReviewCatalog::new();
    catalog.reset(entries, mode, Some(&sidecars));
    catalog
}

fn paths(catalog: &ReviewCatalog) -> Vec<String> {
    catalog.paths().iter().cloned().collect()
}

fn map(root: &TempRoot, pairs: &[(&str, &str)]) -> PathKeyMap {
    pairs.iter().map(|(from, to)| (p(root, from), p(root, to))).collect()
}

#[derive(Clone)]
struct Snapshot {
    paths: Vec<String>,
    version: i32,
    current: i32,
    entries: Vec<CatalogEntry>,
}

fn snapshot(catalog: &ReviewCatalog) -> Snapshot {
    Snapshot {
        paths: paths(catalog),
        version: catalog.structural_version(),
        current: catalog.current_index(),
        entries: catalog.entries_snapshot(),
    }
}

fn assert_unchanged(before: &Snapshot, catalog: &ReviewCatalog) {
    assert_eq!(before.paths, paths(catalog));
    assert_eq!(before.version, catalog.structural_version());
    assert_eq!(before.current, catalog.current_index());
    assert_eq!(before.entries, catalog.entries_snapshot());
}

// C#: ReviewCatalogRenameTests.RenameEntry_FirstMiddleLast_KeepsIndexCurrentAndMetadata
#[test]
fn rename_entry_first_middle_last_keeps_index_current_and_metadata() {
    let root = TempRoot::new("rename-first-middle-last");
    for index in [0usize, 2, 4] {
        let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"]);
        let original = catalog.entries_snapshot()[index].clone();
        catalog.set_current(3);
        let old = p(&root, ["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"][index]);
        let new = p(&root, "renamed.jpg");
        let version = catalog.structural_version();
        assert!(catalog.rename_entry(&old, &new));
        assert_eq!(index as i32, catalog.index_of(&new));
        assert_eq!(-1, catalog.index_of(&old));
        assert_eq!(new, catalog.path_at(index));
        assert_eq!(5, catalog.count());
        assert_eq!(3, catalog.current_index());
        assert_eq!(version + 1, catalog.structural_version());
        let after = catalog.find(&new).expect("renamed entry");
        assert_eq!(original.length, after.length);
        assert_eq!(original.last_write_utc, after.last_write_utc);
        assert_eq!(original.width, after.width);
        assert_eq!(original.height, after.height);
    }
}

// C#: ReviewCatalogRenameTests.RenameEntry_CurrentEntry_StaysCurrentUnderNewName
#[test]
fn rename_entry_current_entry_stays_current_under_new_name() {
    let root = TempRoot::new("rename-current");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    catalog.set_current(1);
    let new = p(&root, "z.jpg");
    assert!(catalog.rename_entry(&p(&root, "b.jpg"), &new));
    assert_eq!(1, catalog.current_index());
    assert_eq!(Some(new.as_str()), catalog.current().map(CatalogEntry::path));
}

// C#: ReviewCatalogRenameTests.RenameEntry_NonCurrentEntry_DoesNotMoveCurrent
#[test]
fn rename_entry_non_current_entry_does_not_move_current() {
    let root = TempRoot::new("rename-non-current");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    catalog.set_current(2);
    assert!(catalog.rename_entry(&p(&root, "a.jpg"), &p(&root, "z.jpg")));
    assert_eq!(2, catalog.current_index());
    assert_eq!(p(&root, "c.jpg"), catalog.current().expect("current").path());
}

// C#: ReviewCatalogRenameTests.RenameEntry_NameSortsElsewhere_KeepsPositionUntilReset
#[test]
fn rename_entry_name_sorts_elsewhere_keeps_position_until_reset() {
    let root = TempRoot::new("rename-order");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    assert!(catalog.rename_entry(&p(&root, "b.jpg"), &p(&root, "z.jpg")));
    assert_eq!(
        vec![p(&root, "a.jpg"), p(&root, "z.jpg"), p(&root, "c.jpg")],
        paths(&catalog)
    );
    let mut sorted = paths(&catalog);
    sorted.sort_by_key(|path| path.to_lowercase());
    catalog.reset(sorted.into_iter().map(entry).collect(), RawPairMode::Separate, None);
    assert_eq!(
        vec![p(&root, "a.jpg"), p(&root, "c.jpg"), p(&root, "z.jpg")],
        paths(&catalog)
    );
    assert_eq!(2, catalog.index_of(&p(&root, "z.jpg")));
}

// C#: ReviewCatalogRenameTests.RenameEntry_CaseOnly_IsValidAndRespellsPath
#[test]
fn rename_entry_case_only_is_valid_and_respells_path() {
    let root = TempRoot::new("rename-case");
    let mut catalog = build(&root, &["a.jpg", "b.jpg"]);
    let new = p(&root, "A.JPG");
    let version = catalog.structural_version();
    assert!(catalog.rename_entry(&p(&root, "a.jpg"), &new));
    assert_eq!(new, catalog.path_at(0));
    assert_eq!(Some(new.as_str()), catalog.paths().first().map(String::as_str));
    assert_eq!(0, catalog.index_of(&p(&root, "a.jpg")));
    assert_eq!(version + 1, catalog.structural_version());
}

// C#: ReviewCatalogRenameTests.RenameEntry_UnknownPath_ReturnsFalseAndLeavesCatalogUntouched
#[test]
fn rename_entry_unknown_path_returns_false_and_leaves_catalog_untouched() {
    let root = TempRoot::new("rename-unknown");
    let mut catalog = build(&root, &["a.jpg", "b.jpg"]);
    let before = snapshot(&catalog);
    assert!(!catalog.rename_entry(&p(&root, "nope.jpg"), &p(&root, "z.jpg")));
    assert_unchanged(&before, &catalog);
}

// C#: ReviewCatalogRenameTests.RenameEntry_NewPathListedByOtherEntry_ReturnsFalseAndLeavesCatalogUntouched
#[test]
fn rename_entry_new_path_listed_by_other_entry_returns_false_and_leaves_catalog_untouched() {
    let root = TempRoot::new("rename-entry-collision");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    catalog.set_current(1);
    let before = snapshot(&catalog);
    assert!(!catalog.rename_entry(&p(&root, "a.jpg"), &p(&root, "C.JPG")));
    assert_unchanged(&before, &catalog);
    assert_eq!(0, catalog.index_of(&p(&root, "a.jpg")));
}

// C#: ReviewCatalogRenameTests.RenameEntries_TwoEntriesToSamePath_ReturnsFalseAndLeavesCatalogUntouched
#[test]
fn rename_entries_two_entries_to_same_path_returns_false_and_leaves_catalog_untouched() {
    let root = TempRoot::new("rename-two-collision");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    let before = snapshot(&catalog);
    assert!(!catalog.rename_entries(&map(&root, &[("a.jpg", "z.jpg"), ("b.jpg", "Z.jpg")])));
    assert_unchanged(&before, &catalog);
}

// C#: ReviewCatalogRenameTests.RenameEntries_SwapTwoNames_IsValidBecauseBothAreBeingRenamed
#[test]
fn rename_entries_swap_two_names_is_valid_because_both_are_being_renamed() {
    let root = TempRoot::new("rename-swap");
    let mut catalog = build(&root, &["a.jpg", "b.jpg"]);
    assert!(catalog.rename_entries(&map(&root, &[("a.jpg", "b.jpg"), ("b.jpg", "a.jpg")])));
    assert_eq!(vec![p(&root, "b.jpg"), p(&root, "a.jpg")], paths(&catalog));
    assert_eq!(Some(100), catalog.entries_snapshot()[0].length);
}

// C#: ReviewCatalogRenameTests.RenameEntries_BlankPath_ReturnsFalseAndLeavesCatalogUntouched
#[test]
fn rename_entries_blank_path_returns_false_and_leaves_catalog_untouched() {
    let root = TempRoot::new("rename-blank");
    for (from, to) in [
        (String::new(), p(&root, "z.jpg")),
        (p(&root, "a.jpg"), String::new()),
        (" ".into(), " ".into()),
    ] {
        let mut catalog = build(&root, &["a.jpg", "b.jpg"]);
        let before = snapshot(&catalog);
        let mut renames = PathKeyMap::new();
        renames.insert(from, to);
        assert!(!catalog.rename_entries(&renames));
        assert_unchanged(&before, &catalog);
    }
}

// C#: ReviewCatalogRenameTests.RenameEntries_EmptyMap_ReturnsFalse
#[test]
fn rename_entries_empty_map_returns_false() {
    let root = TempRoot::new("rename-empty-map");
    let mut catalog = build(&root, &["a.jpg"]);
    let before = snapshot(&catalog);
    assert!(!catalog.rename_entries(&PathKeyMap::new()));
    assert_unchanged(&before, &catalog);
}

// C#: ReviewCatalogRenameTests.RenameEntries_MultipleStandaloneEntries_BumpsStructuralVersionOnce
#[test]
fn rename_entries_multiple_standalone_entries_bumps_structural_version_once() {
    let root = TempRoot::new("rename-multiple");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    let version = catalog.structural_version();
    assert!(catalog.rename_entries(&map(&root, &[("a.jpg", "x.jpg"), ("c.jpg", "y.jpg")])));
    assert_eq!(
        vec![p(&root, "x.jpg"), p(&root, "b.jpg"), p(&root, "y.jpg")],
        paths(&catalog)
    );
    assert_eq!(version + 1, catalog.structural_version());
}

// C#: ReviewCatalogRenameTests.RenameEntries_KeyNoEntryLists_IsIgnoredWhileOthersApply
#[test]
fn rename_entries_key_no_entry_lists_is_ignored_while_others_apply() {
    let root = TempRoot::new("rename-unknown-key");
    let mut catalog = build(&root, &["a.jpg", "b.jpg"]);
    assert!(catalog.rename_entries(&map(&root, &[("a.jpg", "x.jpg"), ("ghost.xmp", "x.xmp")])));
    assert_eq!(vec![p(&root, "x.jpg"), p(&root, "b.jpg")], paths(&catalog));
}

// C#: ReviewCatalogRenameTests.RenameEntries_PairedGroup_RespellsEveryMemberAndRepresentative
#[test]
fn rename_entries_paired_group_respells_every_member_and_representative() {
    let root = TempRoot::new("rename-pair");
    let mut catalog = build_paired(&root, RawPairMode::PreferJpeg, &["a.xmp"]);
    let version = catalog.structural_version();
    assert!(catalog.rename_entries(&map(
        &root,
        &[("a.jpg", "n.jpg"), ("a.cr2", "n.cr2"), ("a.xmp", "n.xmp")]
    )));
    let entry = &catalog.entries_snapshot()[0];
    let group = entry.capture_group.as_ref().expect("group");
    assert_eq!(p(&root, "n.jpg"), entry.path());
    assert_eq!(p(&root, "n.jpg"), group.jpeg_path());
    assert_eq!(p(&root, "n.cr2"), group.raw_path());
    assert_eq!(Some(p(&root, "n.xmp").as_str()), group.xmp_path());
    assert_eq!(0, catalog.index_of(&p(&root, "n.cr2")));
    assert_eq!(-1, catalog.index_of(&p(&root, "a.cr2")));
    assert_eq!(version + 1, catalog.structural_version());
}

// C#: ReviewCatalogRenameTests.RenameEntries_PairedGroupPreferRaw_RepresentativeIsRawPath
#[test]
fn rename_entries_paired_group_prefer_raw_representative_is_raw_path() {
    let root = TempRoot::new("rename-pair-raw");
    let mut catalog = build_paired(&root, RawPairMode::PreferRaw, &["a.xmp"]);
    assert_eq!(p(&root, "a.cr2"), catalog.path_at(0));
    assert!(catalog.rename_entries(&map(
        &root,
        &[("a.jpg", "n.jpg"), ("a.cr2", "n.cr2"), ("a.xmp", "n.xmp")]
    )));
    assert_eq!(p(&root, "n.cr2"), catalog.path_at(0));
    assert_eq!(
        p(&root, "n.jpg"),
        catalog.entries_snapshot()[0]
            .capture_group
            .as_ref()
            .expect("group")
            .jpeg_path()
    );
    assert_eq!(0, catalog.index_of(&p(&root, "n.jpg")));
}

// C#: ReviewCatalogRenameTests.RenameEntries_GroupWithSubsetOfKeys_RespellsOnlyMappedMembers
#[test]
fn rename_entries_group_with_subset_of_keys_respells_only_mapped_members() {
    let root = TempRoot::new("rename-group-subset");
    let mut catalog = build_paired(&root, RawPairMode::PreferJpeg, &["a.xmp"]);
    assert!(catalog.rename_entries(&map(&root, &[("a.cr2", "n.cr2")])));
    let entry = &catalog.entries_snapshot()[0];
    let group = entry.capture_group.as_ref().expect("group");
    assert_eq!(p(&root, "a.jpg"), entry.path());
    assert_eq!(p(&root, "a.jpg"), group.jpeg_path());
    assert_eq!(p(&root, "n.cr2"), group.raw_path());
    assert_eq!(Some(p(&root, "a.xmp").as_str()), group.xmp_path());
    assert_eq!(0, catalog.index_of(&p(&root, "n.cr2")));
    assert_eq!(-1, catalog.index_of(&p(&root, "a.cr2")));
}

// C#: ReviewCatalogRenameTests.RenameEntries_XmpKeyOnly_IsNotIndexedSoNothingChanges
#[test]
fn rename_entries_xmp_key_only_is_not_indexed_so_nothing_changes() {
    let root = TempRoot::new("rename-xmp-only");
    let mut catalog = build_paired(&root, RawPairMode::PreferJpeg, &["a.xmp"]);
    let before = snapshot(&catalog);
    assert!(!catalog.rename_entries(&map(&root, &[("a.xmp", "n.xmp")])));
    assert_unchanged(&before, &catalog);
}

// C#: ReviewCatalogRenameTests.RenameEntries_XmpWithRepresentative_RespellsSidecarOfGroup
#[test]
fn rename_entries_xmp_with_representative_respells_sidecar_of_group() {
    let root = TempRoot::new("rename-xmp-with-representative");
    let mut catalog = build_paired(&root, RawPairMode::PreferJpeg, &["a.xmp"]);
    let version = catalog.structural_version();
    assert!(catalog.rename_entries(&map(&root, &[("a.jpg", "a.jpg"), ("a.xmp", "n.xmp")])));
    let entry = &catalog.entries_snapshot()[0];
    assert_eq!(p(&root, "a.jpg"), entry.path());
    assert_eq!(
        Some(p(&root, "n.xmp").as_str()),
        entry.capture_group.as_ref().expect("group").xmp_path()
    );
    assert_eq!(version + 1, catalog.structural_version());
}

// C#: ReviewCatalogRenameTests.RenameEntries_GroupMemberTakenByOtherEntry_ReturnsFalseAndLeavesCatalogUntouched
#[test]
fn rename_entries_group_member_taken_by_other_entry_returns_false_and_leaves_catalog_untouched() {
    let root = TempRoot::new("rename-group-collision");
    let mut catalog = build_paired(&root, RawPairMode::PreferJpeg, &["a.xmp"]);
    let before = snapshot(&catalog);
    assert!(!catalog.rename_entries(&map(&root, &[("a.jpg", "n.jpg"), ("a.cr2", "b.jpg")])));
    assert_unchanged(&before, &catalog);
    assert_eq!(
        p(&root, "a.cr2"),
        catalog.entries_snapshot()[0]
            .capture_group
            .as_ref()
            .expect("group")
            .raw_path()
    );
}

// C#: ReviewCatalogRenameTests.RenameEntries_XmpOfStandaloneEntry_ReturnsFalse
#[test]
fn rename_entries_xmp_of_standalone_entry_returns_false() {
    let root = TempRoot::new("rename-standalone-xmp");
    let mut catalog = build(&root, &["a.jpg"]);
    let before = snapshot(&catalog);
    assert!(!catalog.rename_entries(&map(&root, &[("a.xmp", "n.xmp")])));
    assert_unchanged(&before, &catalog);
}

// C#: ReviewCatalogRenameTests.RenameEntry_AfterRemoveBurst_UsesLiveIndex
#[test]
fn rename_entry_after_remove_burst_uses_live_index() {
    let root = TempRoot::new("rename-after-remove-burst");
    let names: Vec<String> = (0..40).map(|i| format!("img{i:03}.jpg")).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut catalog = build(&root, &name_refs);
    let _ = catalog.index_of(&p(&root, &names[0]));
    for name in names.iter().skip(5).take(10) {
        catalog.remove(&p(&root, name));
    }
    assert!(catalog.rename_entry(&p(&root, &names[30]), &p(&root, "zz.jpg")));
    assert_eq!(20, catalog.index_of(&p(&root, "zz.jpg")));
    assert_eq!(p(&root, "zz.jpg"), catalog.path_at(20));
    assert_eq!(-1, catalog.index_of(&p(&root, &names[30])));
    assert_eq!(18, catalog.index_of(&p(&root, &names[28])));
    assert_eq!(1, catalog.index_of(&p(&root, &names[1])));
}

// C#: ReviewCatalogRenameTests.ReplaceOrder_AfterRename_AcceptsNewPathSetAndRejectsOld
#[test]
fn replace_order_after_rename_accepts_new_path_set_and_rejects_old() {
    let root = TempRoot::new("rename-then-reorder");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    assert!(catalog.rename_entry(&p(&root, "b.jpg"), &p(&root, "z.jpg")));
    assert!(!catalog.replace_order(&[p(&root, "c.jpg"), p(&root, "b.jpg"), p(&root, "a.jpg")]));
    assert!(catalog.replace_order(&[p(&root, "c.jpg"), p(&root, "z.jpg"), p(&root, "a.jpg")]));
    assert_eq!(
        vec![p(&root, "c.jpg"), p(&root, "z.jpg"), p(&root, "a.jpg")],
        paths(&catalog)
    );
}

// C#: ReviewCatalogRenameTests.RenameEntry_AfterReplaceOrder_RenamesInThePermutedPosition
#[test]
fn rename_entry_after_replace_order_renames_in_the_permuted_position() {
    let root = TempRoot::new("reorder-then-rename");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    assert!(catalog.replace_order(&[p(&root, "c.jpg"), p(&root, "a.jpg"), p(&root, "b.jpg")]));
    assert!(catalog.rename_entry(&p(&root, "a.jpg"), &p(&root, "z.jpg")));
    assert_eq!(
        vec![p(&root, "c.jpg"), p(&root, "z.jpg"), p(&root, "b.jpg")],
        paths(&catalog)
    );
    assert_eq!(1, catalog.index_of(&p(&root, "z.jpg")));
}

// C#: ReviewCatalogRenameTests.Reset_AfterRename_RebuildsFromScratch
#[test]
fn reset_after_rename_rebuilds_from_scratch() {
    let root = TempRoot::new("reset-after-rename");
    let mut catalog = build(&root, &["a.jpg", "b.jpg"]);
    assert!(catalog.rename_entry(&p(&root, "a.jpg"), &p(&root, "z.jpg")));
    catalog.reset(
        ["m.jpg", "n.jpg", "z.jpg"]
            .into_iter()
            .map(|n| entry(p(&root, n)))
            .collect(),
        RawPairMode::Separate,
        None,
    );
    assert_eq!(3, catalog.count());
    assert_eq!(2, catalog.index_of(&p(&root, "z.jpg")));
    assert_eq!(-1, catalog.index_of(&p(&root, "a.jpg")));
    assert_eq!(0, catalog.current_index());
}

// C#: ReviewCatalogRenameTests.Remove_AfterRename_RemovesTheRenamedEntryAndNotAnother
#[test]
fn remove_after_rename_removes_the_renamed_entry_and_not_another() {
    let root = TempRoot::new("remove-after-rename");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    assert!(catalog.rename_entry(&p(&root, "b.jpg"), &p(&root, "z.jpg")));
    catalog.remove(&p(&root, "z.jpg"));
    assert_eq!(vec![p(&root, "a.jpg"), p(&root, "c.jpg")], paths(&catalog));
    assert!(catalog.find(&p(&root, "z.jpg")).is_none());
}

// C#: ReviewCatalogRenameTests.Remove_OldNameAfterRename_IsNoOp
#[test]
fn remove_old_name_after_rename_is_no_op() {
    let root = TempRoot::new("remove-old-name");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    assert!(catalog.rename_entry(&p(&root, "b.jpg"), &p(&root, "z.jpg")));
    let version = catalog.structural_version();
    catalog.remove(&p(&root, "b.jpg"));
    assert_eq!(
        vec![p(&root, "a.jpg"), p(&root, "z.jpg"), p(&root, "c.jpg")],
        paths(&catalog)
    );
    assert_eq!(version, catalog.structural_version());
}

// C#: ReviewCatalogRenameTests.Restore_AfterRename_DoesNotResurrectTheOldName
#[test]
fn restore_after_rename_does_not_resurrect_the_old_name() {
    let root = TempRoot::new("restore-after-rename");
    let mut catalog = build(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    assert!(catalog.rename_entry(&p(&root, "b.jpg"), &p(&root, "z.jpg")));
    catalog.remove(&p(&root, "z.jpg"));
    assert!(catalog.restore(&p(&root, "z.jpg"), 1));
    assert_eq!(
        vec![p(&root, "a.jpg"), p(&root, "z.jpg"), p(&root, "c.jpg")],
        paths(&catalog)
    );
    assert_eq!(-1, catalog.index_of(&p(&root, "b.jpg")));
}

// C#: ReviewCatalogRenameTests.RenameEntry_FromAnotherThreadAfterBind_ThrowsInDebug
#[cfg(debug_assertions)]
#[test]
fn rename_entry_from_another_thread_after_bind_throws_in_debug() {
    let root = TempRoot::new("rename-thread-guard");
    let mut catalog = build(&root, &["a.jpg", "b.jpg"]);
    catalog.bind_to_current_thread();
    let old = p(&root, "a.jpg");
    let new = p(&root, "z.jpg");
    let worker = thread::spawn(move || catalog.rename_entry(&old, &new));
    assert!(worker.join().is_err());
}

// Additional regression for the C# exact-spelling no-op branch in RenameInEntry.
#[test]
fn rename_entries_same_exact_spelling_is_no_op_and_preserves_version() {
    let root = TempRoot::new("rename-exact-noop");
    let mut catalog = build(&root, &["a.jpg", "b.jpg"]);
    let before = snapshot(&catalog);
    assert!(!catalog.rename_entries(&map(&root, &[("a.jpg", "a.jpg")])));
    assert_unchanged(&before, &catalog);
}

// Additional regression for IndexOf first-match staging with pre-existing case-insensitive duplicates.
#[test]
fn rename_entries_first_duplicate_match_changes_without_rejecting_untouched_duplicates() {
    let root = TempRoot::new("rename-existing-duplicates");
    let mut catalog = ReviewCatalog::new();
    catalog.reset(
        vec![
            entry(p(&root, "a.jpg")),
            entry(p(&root, "dup.jpg")),
            entry(p(&root, "DUP.JPG")),
        ],
        RawPairMode::Separate,
        None,
    );
    assert!(catalog.rename_entry(&p(&root, "a.jpg"), &p(&root, "new.jpg")));
    assert_eq!(
        vec![p(&root, "new.jpg"), p(&root, "dup.jpg"), p(&root, "DUP.JPG")],
        paths(&catalog)
    );
    let mut map = PathKeyMap::new();
    map.insert(p(&root, "dup.jpg"), p(&root, "renamed.jpg"));
    assert!(catalog.rename_entries(&map));
    assert_eq!(p(&root, "renamed.jpg"), catalog.path_at(1));
    assert_eq!(p(&root, "DUP.JPG"), catalog.path_at(2));
    assert_eq!(2, catalog.index_of(&p(&root, "dup.jpg")));
}
