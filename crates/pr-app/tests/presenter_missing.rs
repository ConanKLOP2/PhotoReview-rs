//! Tests of the missing-item removal (W8-07k): `RemoveOrDegrade` and `RemoveMissingCatalogItemAsync` (C# `ImagePresenter.cs` lines 714 to 770, baseline d92d6aae), reached
//! through `present` (a vanished file) and directly. Contract: `contracts/W8-07k.md`, kind F (fakes, no oracle, no golden).
//!
//! The C# loop awaits the stat of the NEXT photo each round; the Rust form resumes it from `pump`, so the skip stays iterative (R2-F-07). Every wait is bounded.

mod support {
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::io;
use std::sync::Arc;

use pr_app::coordinators::presenter::missing::remove_or_degrade;
use pr_app::coordinators::presenter::state::PresentRequest;
use pr_app::viewmodels::format::status;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::{CaptureGroup, CatalogEntry};
use pr_core::model::enums::RawPairMode;
use support::present_helpers::{MemberScriptDecoder, ScriptedFs, present, present_with, presented_paths, statuses};
use support::presenter_fakes::{FixtureOptions, PresenterFixture, env};
use support::preview_fakes::{SizedDecoder, lock, sized_image};

// ---- helpers ---------------------------------------------------------------------------------------------------------------

fn fixture(names: &[&str]) -> PresenterFixture {
    PresenterFixture::new(SizedDecoder::new(), names)
}

fn fixture_with_fs(names: &[&str], fs: &Arc<ScriptedFs>) -> PresenterFixture {
    PresenterFixture::with_options(
        SizedDecoder::new(),
        names,
        FixtureOptions {
            file_system: Some(Arc::clone(fs) as _),
            ..FixtureOptions::default()
        },
    )
}

fn delete(path: &str) {
    std::fs::remove_file(path).unwrap_or_else(|error| panic!("delete {path}: {error}"));
}

fn group_entry(jpeg: &str, member: &str) -> CatalogEntry {
    let group = CaptureGroup::new(jpeg, member, None).unwrap_or_else(|error| panic!("group: {error:?}"));
    let mut entry = CatalogEntry::new(jpeg).unwrap_or_else(|error| panic!("entry: {error:?}"));
    entry.capture_group = Some(group);
    entry
}

fn catalog_of(paths: &[&str]) -> ReviewCatalog {
    let mut catalog = ReviewCatalog::new();
    let entries = paths
        .iter()
        .map(|path| CatalogEntry::new(*path).unwrap_or_else(|error| panic!("entry: {error:?}")))
        .collect();
    catalog.reset(entries, RawPairMode::Separate, None);
    catalog
}

// ---- [self]: RemoveMissingCatalogItem -------------------------------------------------------------------------------------------

// C#: Adversarial.RemoveMissingCatalogItemAsync_StaleToken_LeavesCatalogUntouched
#[test]
fn remove_missing_stale_token_leaves_catalog_untouched() {
    let mut f = fixture(&["gone.png", "here.png"]);
    let (gone, here) = (f.path("gone.png"), f.path("here.png"));
    delete(&gone);
    let stale_token = f.clock.next_navigation();
    f.clock.next_navigation();

    let changes = {
        let mut env = env(&mut f.catalog, &f.settings, &f.format);
        f.presenter.remove_missing_catalog_item(&mut env, &gone, stale_token)
    };
    f.pump_until_idle();

    assert_eq!(&*f.catalog.paths(), &[gone, here][..]);
    assert!(changes.is_empty());
    assert_eq!(lock(&f.sink_log).call_count(), 0);
}

// C#: StatErrors.RemoveMissingCatalogItemAsync_NextPhotoStatFailsWithIoError_DoesNotSkipIt
#[test]
fn remove_missing_next_photo_stat_io_error_does_not_skip_it() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["missing.png", "flaky.png"], &fs);
    let (missing, flaky) = (f.path("missing.png"), f.path("flaky.png"));
    delete(&missing);
    fs.fail_with(&flaky, || io::Error::other("share hiccup"));
    let token = f.clock.next_navigation();

    {
        let mut env = env(&mut f.catalog, &f.settings, &f.format);
        f.presenter.remove_missing_catalog_item(&mut env, &missing, token);
    }
    f.pump_until_idle();

    assert_eq!(&*f.catalog.paths(), &[flaky][..]);
    // The unreadable photo is presented, so its error is reported instead of being dropped.
    assert!(f.presenter.status_needs_attention());
    assert!(
        statuses(&f)
            .last()
            .is_some_and(|status| status.contains("share hiccup"))
    );
}

// C#: ImagePresenterTests.PresentAsync_WhenFileMissing_RemovesFromCatalogAndAdvances
#[test]
fn present_when_file_missing_removes_from_catalog_and_advances() {
    let mut f = fixture(&["missing1.jpg", "exists2.jpg", "exists3.jpg"]);
    let (f1, f2, f3) = (f.path("missing1.jpg"), f.path("exists2.jpg"), f.path("exists3.jpg"));
    delete(&f1);

    present(&mut f, 0);

    assert_eq!(f.catalog.count(), 2);
    assert!(!f.catalog.paths().contains(&f1));
    assert_eq!(&*f.catalog.paths(), &[f2.clone(), f3][..]);
    assert_eq!(f.catalog.current_index(), 0);
    assert_eq!(presented_paths(&f), vec![f2]);
}

// C#: ImagePresenterTests.PresentAsync_WhenManyConsecutiveFilesMissing_SkipsThemWithoutNestedPresents (R2-F-07)
#[test]
fn present_when_many_consecutive_files_missing_skips_them_without_nested_presents() {
    let names: Vec<String> = (0..200)
        .map(|i| format!("gone{i}.jpg"))
        .chain(["survivor.jpg".to_owned()])
        .collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut f = fixture(&name_refs);
    for name in &names[..200] {
        delete(&f.path(name));
    }
    let survivor = f.path("survivor.jpg");

    present(&mut f, 0);

    assert_eq!(&*f.catalog.paths(), &[survivor][..]);
    assert_eq!(f.catalog.current_index(), 0);
    // The first present plus exactly one for the survivor; a recursive skip would notify once per missing file.
    assert_eq!(f.preload.notified().len(), 2);
}

// C#: ImagePresenterTests.PresentAsync_WhenAllFilesMissing_EmitsNoImagesRemaining
#[test]
fn present_when_all_files_missing_emits_no_images_remaining() {
    let mut f = fixture(&["missing1.jpg"]);
    delete(&f.path("missing1.jpg"));

    present(&mut f, 0);

    assert_eq!(f.catalog.count(), 0);
    assert_eq!(f.presenter.status_text(), status::no_images_remaining(&f.format.ctx()));
    assert!(
        f.presenter.status_needs_attention(),
        "the empty state is never faded out"
    );
    assert!(f.presenter.current_image().is_none());
    assert!(f.presenter.current_photo_info().is_none());
}

// C#: ImagePresenterTests.PresentAsync_MissingCaptureGroupMember_KeepsTheRemainingMemberAsAStandaloneEntry
#[test]
fn present_missing_capture_group_member_keeps_the_remaining_member_as_a_standalone_entry() {
    let mut f = fixture(&["pair-survivor.jpg"]);
    let jpeg = f.path("pair-survivor.jpg");
    let missing_raw = f.path("pair-survivor.cr3"); // never created
    f.catalog
        .reset(vec![group_entry(&jpeg, &missing_raw)], RawPairMode::PreferJpeg, None);

    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: false,
            path_override: Some(missing_raw.clone()),
            include_capture_group_in_compare: false,
        },
    );

    assert_eq!(f.catalog.count(), 1);
    let entry = f
        .catalog
        .find(&jpeg)
        .cloned()
        .unwrap_or_else(|| panic!("the survivor stays"));
    assert!(entry.capture_group.is_none());
    assert_eq!(f.catalog.index_of(&missing_raw), -1);
    assert_eq!(f.presenter.current_presented_path(), Some(jpeg.as_str()));
    assert_eq!(presented_paths(&f), vec![jpeg]);
}

// C#: ImagePresenterTests.PresentAsync_MissingRepresentativeOfCaptureGroup_PresentsTheRemainingMember
#[test]
fn present_missing_representative_of_capture_group_presents_the_remaining_member() {
    let mut f = fixture(&["rep-gone.png"]);
    let raw = f.path("rep-gone.png");
    let missing_jpeg = f.path("rep-gone.jpg"); // never created
    f.catalog
        .reset(vec![group_entry(&missing_jpeg, &raw)], RawPairMode::PreferJpeg, None);

    present(&mut f, 0);

    assert_eq!(f.catalog.count(), 1);
    assert_eq!(f.catalog.path_at(0), raw);
    assert!(f.catalog.find(&raw).is_some_and(|entry| entry.capture_group.is_none()));
    assert_eq!(presented_paths(&f), vec![raw]);
}

// C#: Adversarial.PresentAsync_SingleImageDeletedExternally_EndsEmpty
#[test]
fn present_single_image_deleted_externally_ends_empty() {
    let mut f = fixture(&["single.png"]);
    let path = f.path("single.png");
    present(&mut f, 0);
    delete(&path);

    present(&mut f, 0); // e.g. the user presses Home on the only image

    assert_eq!(f.catalog.count(), 0);
    assert!(f.presenter.current_image().is_none());
    assert!(f.presenter.current_photo_info().is_none());
    assert_eq!(f.presenter.current_original_width(), 0);
}

// C#: Adversarial.PresentAsync_FileRenamedExternally_IsSkippedWhenReached
#[test]
fn present_file_renamed_externally_is_skipped_when_reached() {
    let mut f = fixture(&["first.png", "second.png", "third.png"]);
    let (f1, f2, f3) = (f.path("first.png"), f.path("second.png"), f.path("third.png"));
    present(&mut f, 0);
    std::fs::rename(&f2, f.path("renamed.png")).unwrap_or_else(|error| panic!("rename: {error}"));

    present(&mut f, 1);

    assert_eq!(&*f.catalog.paths(), &[f1, f3][..]);
    assert_eq!(f.catalog.current_index(), 1);
    assert_eq!(
        f.presenter.current_photo_info().map(|info| info.file_name.clone()),
        Some("third.png".to_owned())
    );
}

// C#: StatErrors.PresentAsync_WhenStatThrowsFileNotFound_StillTreatsThePhotoAsMissing
#[test]
fn present_stat_not_found_error_is_missing() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["gone.png", "other.png"], &fs);
    let (gone, other) = (f.path("gone.png"), f.path("other.png"));
    fs.fail_with(&gone, || io::Error::new(io::ErrorKind::NotFound, "gone"));

    present(&mut f, 0);

    assert_eq!(&*f.catalog.paths(), &[other][..]);
}

// C#: StatErrors.PresentAsync_WhenPathCanNeverExist_TreatsThePhotoAsMissingAndMovesOn(Type): ArgumentException, NotSupportedException, PathTooLongException
fn path_that_can_never_exist(make: impl Fn() -> io::Error + Send + Sync + 'static, label: &str) {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["bad.png", "other.png"], &fs);
    let (bad, other) = (f.path("bad.png"), f.path("other.png"));
    fs.fail_with(&bad, make);

    present(&mut f, 0);

    // Not a permanent Error item that the skip loop would land on forever.
    assert_eq!(&*f.catalog.paths(), &[other][..], "{label}");
}

#[test]
fn present_stat_path_that_can_never_exist_is_missing_invalid_input() {
    path_that_can_never_exist(
        || io::Error::new(io::ErrorKind::InvalidInput, "invalid path"),
        "InvalidInput",
    );
}

#[test]
fn present_stat_path_that_can_never_exist_is_missing_unsupported() {
    path_that_can_never_exist(
        || io::Error::new(io::ErrorKind::Unsupported, "invalid path"),
        "Unsupported",
    );
}

#[test]
fn present_stat_path_that_can_never_exist_is_missing_path_too_long() {
    path_that_can_never_exist(|| io::Error::from_raw_os_error(206), "raw OS error 206");
}

// ---- [self]: the stale FileNotFound recheck ends in the removal ---------------------------------------------------------------------

// New test (W8-07j contract note 6 (2), T6): a FileNotFound whose recheck stat says the file is really gone removes it, like a missing file.
#[test]
fn present_stale_not_found_with_the_file_gone_removes_it() {
    let fs = ScriptedFs::new();
    let decoder = MemberScriptDecoder::new("vanishing.png", sized_image(100, 100, 100, 100, false), |_| {
        Err(pr_imaging::decoder_traits::DecodeError::FileNotFound {
            message: "gone".to_owned(),
        })
    });
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &["first.png", "vanishing.png"],
        FixtureOptions {
            file_system: Some(Arc::clone(&fs) as _),
            ..FixtureOptions::default()
        },
    );
    let (first, vanishing) = (f.path("first.png"), f.path("vanishing.png"));
    let real = real_stat(&vanishing);
    // The initial stat finds the file; by the time the recheck runs it is gone.
    fs.script(&vanishing, move |call| Ok((call == 1).then_some(real)));

    present(&mut f, 1);

    assert_eq!(&*f.catalog.paths(), &[first.clone()][..]);
    assert_eq!(presented_paths(&f), vec![first]);
    assert_eq!(decoder.member_calls(), 1, "no re-present of a file that is gone");
}

// New test (T6): a recheck that cannot tell (`Error`, not `Found`) also goes to the removal -- the C# code only keeps the photo on `Found`.
#[test]
fn present_stale_not_found_with_an_unreadable_recheck_removes_it_like_csharp() {
    let fs = ScriptedFs::new();
    let decoder = MemberScriptDecoder::new("vanishing.png", sized_image(100, 100, 100, 100, false), |_| {
        Err(pr_imaging::decoder_traits::DecodeError::FileNotFound {
            message: "gone".to_owned(),
        })
    });
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &["first.png", "vanishing.png"],
        FixtureOptions {
            file_system: Some(Arc::clone(&fs) as _),
            ..FixtureOptions::default()
        },
    );
    let (first, vanishing) = (f.path("first.png"), f.path("vanishing.png"));
    let real = real_stat(&vanishing);
    fs.script(&vanishing, move |call| {
        if call == 1 {
            Ok(Some(real))
        } else {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
        }
    });

    present(&mut f, 1);

    assert_eq!(&*f.catalog.paths(), &[first][..]);
}

fn real_stat(path: &str) -> pr_core::abstractions::FileStat {
    use pr_core::abstractions::FileSystem;
    pr_core::io::physical::PhysicalFileSystem
        .get_file_stat(std::path::Path::new(path))
        .unwrap_or_else(|| panic!("{path} has no stat"))
}

// ---- [self]: RemoveOrDegrade -----------------------------------------------------------------------------------------------------

// New test (contract note 1): a plain entry is removed and the catalog's own new current index is returned.
#[test]
fn remove_or_degrade_plain_entry_is_removed_and_returns_the_catalog_answer() {
    let mut catalog = catalog_of(&["C:\\p\\a.jpg", "C:\\p\\b.jpg", "C:\\p\\c.jpg"]);
    catalog.set_current(1);

    let next = remove_or_degrade(&mut catalog, "C:\\p\\b.jpg");

    assert_eq!(
        &*catalog.paths(),
        &["C:\\p\\a.jpg".to_owned(), "C:\\p\\c.jpg".to_owned()][..]
    );
    assert_eq!(next, catalog.current_index());
    assert_eq!(next, 1);
}

// New test (contract note 1): an unknown path changes nothing.
#[test]
fn remove_or_degrade_unknown_path_changes_nothing() {
    let mut catalog = catalog_of(&["C:\\p\\a.jpg", "C:\\p\\b.jpg"]);
    let version = catalog.structural_version();

    remove_or_degrade(&mut catalog, "C:\\p\\zzz.jpg");

    assert_eq!(catalog.count(), 2);
    assert_eq!(catalog.structural_version(), version);
}

// New test (contract note 1, T2): a missing member of a capture group degrades the entry to the surviving member -- a STANDALONE entry at the SAME position, selected.
#[test]
fn remove_or_degrade_capture_group_member_leaves_the_survivor_standalone_at_the_same_position() {
    let mut catalog = ReviewCatalog::new();
    let group = group_entry("C:\\p\\pair.jpg", "C:\\p\\pair.cr3");
    let entries = vec![
        CatalogEntry::new("C:\\p\\a.jpg").unwrap_or_else(|error| panic!("{error:?}")),
        group,
        CatalogEntry::new("C:\\p\\z.jpg").unwrap_or_else(|error| panic!("{error:?}")),
    ];
    catalog.reset(entries, RawPairMode::PreferJpeg, None);
    catalog.set_current(2);

    // The RAW member vanished (the path of a member, in another case: the comparison is ordinal ignore case).
    let next = remove_or_degrade(&mut catalog, "C:\\P\\PAIR.CR3");

    assert_eq!(next, 1);
    assert_eq!(catalog.current_index(), 1, "the survivor is selected");
    assert_eq!(
        &*catalog.paths(),
        &[
            "C:\\p\\a.jpg".to_owned(),
            "C:\\p\\pair.jpg".to_owned(),
            "C:\\p\\z.jpg".to_owned()
        ][..]
    );
    assert!(
        catalog
            .find("C:\\p\\pair.jpg")
            .is_some_and(|entry| entry.capture_group.is_none())
    );
    assert_eq!(catalog.index_of("C:\\p\\pair.cr3"), -1);
}

// New test (contract note 1): the survivor is the member that is NOT the missing path under ordinal ignore case -- a path that differs only in case is the
// missing member itself, so the OTHER one survives.
#[test]
fn remove_or_degrade_survivor_is_chosen_ignoring_the_case_of_the_missing_path() {
    let mut catalog = ReviewCatalog::new();
    catalog.reset(
        vec![group_entry("C:\\p\\pair.jpg", "C:\\p\\pair.cr3")],
        RawPairMode::PreferJpeg,
        None,
    );

    remove_or_degrade(&mut catalog, "C:\\P\\PAIR.JPG");

    assert_eq!(&*catalog.paths(), &["C:\\p\\pair.cr3".to_owned()][..]);
}

// New test (contract note 1, T2): the representative itself missing keeps the other member.
#[test]
fn remove_or_degrade_missing_representative_keeps_the_other_member() {
    let mut catalog = ReviewCatalog::new();
    catalog.reset(
        vec![group_entry("C:\\p\\pair.jpg", "C:\\p\\pair.cr3")],
        RawPairMode::PreferJpeg,
        None,
    );

    let next = remove_or_degrade(&mut catalog, "C:\\p\\pair.jpg");

    assert_eq!(next, 0);
    assert_eq!(&*catalog.paths(), &["C:\\p\\pair.cr3".to_owned()][..]);
    assert!(
        catalog
            .find("C:\\p\\pair.cr3")
            .is_some_and(|entry| entry.capture_group.is_none())
    );
}

// ---- [self]: the loop ---------------------------------------------------------------------------------------------------------------

// New test (contract note 3): a run whose navigation is superseded while the next photo's stat is pending ends without presenting anything.
#[test]
fn remove_missing_superseded_while_the_next_stat_is_pending_does_not_present() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["missing.png", "next.png"], &fs);
    let (missing, next) = (f.path("missing.png"), f.path("next.png"));
    delete(&missing);
    fs.hold(&next);
    let token = f.clock.next_navigation();

    {
        let mut env = env(&mut f.catalog, &f.settings, &f.format);
        f.presenter.remove_missing_catalog_item(&mut env, &missing, token);
    }
    assert!(fs.wait_started(&next));
    assert_eq!(
        &*f.catalog.paths(),
        &[next.clone()][..],
        "the missing entry was removed before the stat"
    );
    f.clock.next_navigation(); // another navigation took over
    fs.release(&next);
    f.pump_until_idle();

    assert!(presented_paths(&f).is_empty());
    assert_eq!(f.preload.notified(), Vec::<i32>::new(), "no present for the next photo");
}

// New test: kills pending_next.
#[test]
fn remove_missing_waits_for_pending_next_then_presents_it() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["missing.png", "next.png"], &fs);
    let (missing, next) = (f.path("missing.png"), f.path("next.png"));
    delete(&missing);
    fs.hold(&next);
    let token = f.clock.next_navigation();

    {
        let mut env = env(&mut f.catalog, &f.settings, &f.format);
        f.presenter.remove_missing_catalog_item(&mut env, &missing, token);
    }
    assert!(fs.wait_started(&next));
    assert!(!f.presenter.is_idle(), "the next stat is pending");
    assert_eq!(&*f.catalog.paths(), &[next.clone()][..]);
    assert!(presented_paths(&f).is_empty());

    fs.release(&next);
    f.pump_until_idle();
    assert!(f.presenter.is_idle(), "the pending stat was consumed");
    assert_eq!(presented_paths(&f), vec![next]);
}
