//! Tests for `pr_core::catalog::{types, explorer_reason}`, ported from the C# catalog tests at baseline e441d67a
//! (`tests/PhotoReview.Core.Tests/Catalog/*`, `tests/PhotoReview.App.Tests/Coordinators/FolderLoadCoordinatorTests.LatestExplorer.cs`).
//! Each test names the C# test it ports in a `// C#:` comment.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use pr_core::abstractions::FileStat;
use pr_core::catalog::explorer_reason::{self, ALL_CODES};
use pr_core::catalog::explorer_types::{ExplorerGroupState, ExplorerOrderStatus, ExplorerViewSnapshot};
use pr_core::catalog::types::{
    ArgumentError, CaptureGroup, CatalogEntry, ExplorerQueryHandle, JPEG_EXTENSIONS, LatestExplorerSnapshot,
    RAW_EXTENSIONS, SUPPORTED_EXTENSIONS, get_extension, is_raw_path, is_supported, is_supported_with_raw,
};
use pr_core::model::RawPairMode;
use pr_core::time::DateTimeUtc;
use pr_test_support::golden::{self, bool_of, str_of};

/// A fixed UTC instant from its tick count (valid ticks, so `from_ticks` always succeeds here).
fn at(ticks: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(ticks).unwrap()
}

fn stamp() -> DateTimeUtc {
    at(630_000_000_000_000_000)
}

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn sample_snapshot(folder: &str) -> ExplorerViewSnapshot {
    ExplorerViewSnapshot {
        folder: folder.to_owned(),
        ordered_paths: vec![format!("{folder}\\a.jpg")],
        sort_columns: Vec::new(),
        group_state: ExplorerGroupState::None,
        status: ExplorerOrderStatus::Available,
        reason: None,
        captured_utc: stamp(),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// ImageFileTypes
// ---------------------------------------------------------------------------------------------------------------

// C#: ImageFileTypesTests.RawExtensions_AreExactlyTheEightApprovedFormats
#[test]
fn raw_extensions_are_exactly_the_eight_approved_formats() {
    let expected: HashSet<&str> = [".cr2", ".cr3", ".nef", ".arw", ".dng", ".raf", ".orf", ".rw2"]
        .into_iter()
        .collect();
    let actual: HashSet<&str> = RAW_EXTENSIONS.iter().copied().collect();
    assert_eq!(RAW_EXTENSIONS.len(), 8);
    assert_eq!(actual, expected);
    assert!(!is_supported_with_raw("photo.nrw", true));
    assert!(!is_supported_with_raw("photo.pef", true));
    assert!(
        !RAW_EXTENSIONS
            .iter()
            .any(|e| e.eq_ignore_ascii_case(".nrw") || e.eq_ignore_ascii_case(".pef"))
    );
}

// The standard and JPEG lists are exact sets too (PARITY I-01): no TIFF or PNG on the JPEG side.
#[test]
fn supported_and_jpeg_extension_lists_are_exact_sets() {
    let supported: HashSet<&str> = SUPPORTED_EXTENSIONS.iter().copied().collect();
    assert_eq!(
        supported,
        [".jpg", ".jpeg", ".png", ".bmp", ".gif", ".tif", ".tiff"]
            .into_iter()
            .collect::<HashSet<&str>>()
    );
    let jpeg: HashSet<&str> = JPEG_EXTENSIONS.iter().copied().collect();
    assert_eq!(jpeg, [".jpg", ".jpeg"].into_iter().collect::<HashSet<&str>>());
}

// C#: ImageFileTypesTests.IsSupportedValidatesExtensions (theory). The `null` row has no Rust twin: an empty string
// is the closest case and is checked explicitly below.
#[test]
fn is_supported_validates_extensions() {
    let rows: [(&str, bool); 8] = [
        ("photo.jpg", true),
        ("photo.JPEG", true),
        ("IMAGE.PNG", true),
        ("bitmap.bmp", true),
        ("anim.gif", true),
        ("document.pdf", false),
        ("executable.exe", false),
        ("", false),
    ];
    for (path, expected) in rows {
        assert_eq!(is_supported(path), expected, "is_supported({path:?})");
    }
    assert!(!is_supported(""));
    assert!(!is_supported("   "));
}

#[test]
fn is_supported_with_raw_gates_raw_on_the_flag() {
    assert!(!is_supported_with_raw("photo.cr2", false));
    assert!(is_supported_with_raw("photo.cr2", true));
    assert!(is_supported_with_raw("photo.jpg", false));
    assert!(!is_supported_with_raw("photo", true));
    assert!(!is_supported_with_raw("   ", true));
}

// C#: CatalogMutationGapTests.IsRawPath_StringOverload_HandlesNullAndRawAndNonRawNames (the `null` line is not
// ported: a Rust `&str` cannot be null, and `is_raw_path("")` covers the "no name" case).
#[test]
fn is_raw_path_handles_raw_and_non_raw_names() {
    assert!(is_raw_path("x.cr2"));
    assert!(is_raw_path(r"C:\shots\IMG_1.DNG"));
    assert!(!is_raw_path("x.jpg"));
    assert!(!is_raw_path(""));
}

#[test]
fn get_extension_follows_dotnet_path_get_extension() {
    assert_eq!(get_extension("a.JPG"), ".JPG");
    assert_eq!(get_extension(".jpg"), ".jpg");
    assert_eq!(get_extension("a."), "");
    assert_eq!(get_extension("a..jpg"), ".jpg");
    assert_eq!(get_extension(r"C:\d.jpg\f"), "");
    assert_eq!(get_extension("C:f.jpg"), ".jpg");
    assert_eq!(get_extension("a.jpg "), ".jpg ");
    assert_eq!(get_extension("noext"), "");
}

// Extensions compare OrdinalIgnoreCase, never with Unicode lowercasing (the Turkish dotless i is not an "i").
#[test]
fn extensions_compare_ordinal_ignore_case_only() {
    assert!(is_supported("PHOTO.JpG"));
    assert!(!is_supported("a.ımg"));
    assert!(!is_supported("x.İpg"));
}

// ---------------------------------------------------------------------------------------------------------------
// CaptureGroup
// ---------------------------------------------------------------------------------------------------------------

// C#: CaptureGroupTests.Equals_SamePathsIgnoringCase_AreEqualWithSameHashCode
#[test]
fn equals_same_paths_ignoring_case_are_equal_with_same_hash_code() {
    let left = CaptureGroup::new(r"C:\photos\a.jpg", r"C:\photos\a.cr2", Some(r"C:\photos\a.xmp".into())).unwrap();
    let right = CaptureGroup::new(r"C:\photos\A.JPG", r"C:\photos\A.CR2", Some(r"C:\photos\A.XMP".into())).unwrap();

    assert_eq!(left, right);
    assert_eq!(hash_of(&left), hash_of(&right));
    let mut set = HashSet::new();
    set.insert(left.clone());
    set.insert(right);
    assert_eq!(set.len(), 1);
}

// C#: CaptureGroupTests.Equals_DifferentRawOrSidecar_AreNotEqual
#[test]
fn equals_different_raw_or_sidecar_are_not_equal() {
    let group = CaptureGroup::new(r"C:\photos\a.jpg", r"C:\photos\a.cr2", Some(r"C:\photos\a.xmp".into())).unwrap();

    assert_ne!(
        group,
        CaptureGroup::new(r"C:\photos\a.jpg", r"C:\photos\a.nef", Some(r"C:\photos\a.xmp".into())).unwrap()
    );
    assert_ne!(
        group,
        CaptureGroup::new(r"C:\photos\a.jpg", r"C:\photos\a.cr2", None).unwrap()
    );
    assert_ne!(
        CaptureGroup::new(r"C:\photos\a.jpg", r"C:\photos\a.cr2", None).unwrap(),
        group
    );
}

// C#: CaptureAndEntryMutationGapTests.CaptureGroup_EqualsNull_IsFalseWithoutThrowing: not ported. A Rust
// `CaptureGroup` is never null, so the null comparison cannot be written; equality with a value is covered above.

// C#: CaptureAndEntryMutationGapTests.CaptureGroup_GroupsDifferingInOnePathOrSidecar_AreNotEqual
#[test]
fn capture_group_groups_differing_in_one_path_or_sidecar_are_not_equal() {
    let group = CaptureGroup::new(r"C:\a.jpg", r"C:\a.cr2", Some(r"C:\a.xmp".into())).unwrap();

    assert_ne!(
        group,
        CaptureGroup::new(r"C:\b.jpg", r"C:\a.cr2", Some(r"C:\a.xmp".into())).unwrap()
    );
    assert_ne!(
        group,
        CaptureGroup::new(r"C:\a.jpg", r"C:\b.cr2", Some(r"C:\a.xmp".into())).unwrap()
    );
    assert_ne!(
        group,
        CaptureGroup::new(r"C:\a.jpg", r"C:\a.cr2", Some(r"C:\b.xmp".into())).unwrap()
    );
    assert_ne!(group, CaptureGroup::new(r"C:\a.jpg", r"C:\a.cr2", None).unwrap());
}

// C#: CaptureAndEntryMutationGapTests.CaptureGroup_SamePathsInDifferentCase_AreEqualWithEqualHashCodes
#[test]
fn capture_group_same_paths_in_different_case_are_equal_with_equal_hash_codes() {
    let lower = CaptureGroup::new(r"C:\photos\a.jpg", r"C:\photos\a.cr2", Some(r"C:\photos\a.xmp".into())).unwrap();
    let upper = CaptureGroup::new(r"C:\PHOTOS\A.JPG", r"C:\PHOTOS\A.CR2", Some(r"C:\PHOTOS\A.XMP".into())).unwrap();

    assert_eq!(lower, upper);
    assert_eq!(hash_of(&lower), hash_of(&upper));
}

// C#: CaptureAndEntryMutationGapTests.CaptureGroup_DifferentGroups_HaveDifferentHashCodes
#[test]
fn capture_group_different_groups_have_different_hash_codes() {
    let a = CaptureGroup::new(r"C:\a.jpg", r"C:\a.cr2", None).unwrap();
    let b = CaptureGroup::new(r"C:\b.jpg", r"C:\b.cr2", None).unwrap();
    let with_sidecar = CaptureGroup::new(r"C:\a.jpg", r"C:\a.cr2", Some(r"C:\a.xmp".into())).unwrap();

    assert_ne!(hash_of(&a), hash_of(&b));
    assert_ne!(hash_of(&a), hash_of(&with_sidecar));

    let other_raw = CaptureGroup::new(r"C:\a.jpg", r"C:\a.nef", None).unwrap();
    assert_ne!(hash_of(&a), hash_of(&other_raw));
}

// A missing sidecar and an empty sidecar are different values (`Some("")` is an error, checked below), so they must
// not be equal either.
#[test]
fn capture_group_none_sidecar_is_not_equal_to_a_sidecar_path() {
    let none = CaptureGroup::new(r"C:\a.jpg", r"C:\a.cr2", None).unwrap();
    let some = CaptureGroup::new(r"C:\a.jpg", r"C:\a.cr2", Some(r"C:\a.xmp".into())).unwrap();
    assert_ne!(none, some);
    assert_eq!(none.clone(), none);
}

#[test]
fn capture_group_new_rejects_blank_paths_in_check_order() {
    assert_eq!(
        CaptureGroup::new("", "r.cr2", None).unwrap_err(),
        ArgumentError::BlankPath { param_name: "jpegPath" }
    );
    assert_eq!(
        CaptureGroup::new("  ", "", None).unwrap_err(),
        ArgumentError::BlankPath { param_name: "jpegPath" }
    );
    assert_eq!(
        CaptureGroup::new("a.jpg", "", None).unwrap_err(),
        ArgumentError::BlankPath { param_name: "rawPath" }
    );
    assert_eq!(
        CaptureGroup::new("a.jpg", "a.cr2", Some(String::new())).unwrap_err(),
        ArgumentError::BlankPath { param_name: "xmpPath" }
    );
    assert_eq!(
        CaptureGroup::new("a.jpg", "a.cr2", Some("   ".into())).unwrap_err(),
        ArgumentError::BlankPath { param_name: "xmpPath" }
    );
    assert!(CaptureGroup::new("a.jpg", "a.cr2", None).is_ok());
}

#[test]
fn capture_group_paths_keep_their_order_and_sidecar() {
    let no_sidecar = CaptureGroup::new("a.jpg", "a.cr2", None).unwrap();
    assert_eq!(no_sidecar.paths(), ["a.jpg".to_owned(), "a.cr2".to_owned()]);
    assert_eq!(no_sidecar.image_paths(), ["a.jpg".to_owned(), "a.cr2".to_owned()]);
    assert_eq!(no_sidecar.xmp_path(), None);

    let with_sidecar = CaptureGroup::new("a.jpg", "a.cr2", Some("a.xmp".into())).unwrap();
    assert_eq!(
        with_sidecar.paths(),
        ["a.jpg".to_owned(), "a.cr2".to_owned(), "a.xmp".to_owned()]
    );
    assert_eq!(with_sidecar.image_paths(), ["a.jpg".to_owned(), "a.cr2".to_owned()]);
    assert_eq!(with_sidecar.xmp_path(), Some("a.xmp"));
    assert_eq!(with_sidecar.jpeg_path(), "a.jpg");
    assert_eq!(with_sidecar.raw_path(), "a.cr2");
}

#[test]
fn capture_group_representative_path_follows_the_mode() {
    let group = CaptureGroup::new("a.jpg", "a.cr2", None).unwrap();
    assert_eq!(group.get_representative_path(RawPairMode::PreferJpeg), Ok("a.jpg"));
    assert_eq!(group.get_representative_path(RawPairMode::PreferRaw), Ok("a.cr2"));
    assert_eq!(
        group.get_representative_path(RawPairMode::Separate),
        Err(ArgumentError::OutOfRange { param_name: "mode" })
    );
}

// ---------------------------------------------------------------------------------------------------------------
// CatalogEntry
// ---------------------------------------------------------------------------------------------------------------

// C#: CatalogEntryTests.Path_RejectsBlank_ViaConstructorAndWith
#[test]
fn path_rejects_blank_via_constructor_and_with() {
    for blank in ["", "   "] {
        assert_eq!(
            CatalogEntry::new(blank).unwrap_err(),
            ArgumentError::BlankPath { param_name: "path" }
        );
        let entry = CatalogEntry::new("a.jpg").unwrap();
        assert_eq!(
            entry.with_path(blank).unwrap_err(),
            ArgumentError::BlankPath { param_name: "path" }
        );
    }
}

#[test]
fn path_is_stored_as_given_and_other_fields_start_empty() {
    let entry = CatalogEntry::new(" a.jpg").unwrap();
    assert_eq!(entry.path(), " a.jpg");
    assert_eq!(entry.length, None);
    assert_eq!(entry.last_write_utc, None);
    assert_eq!(entry.width, None);
    assert_eq!(entry.height, None);
    assert_eq!(entry.capture_group, None);
}

#[test]
fn with_path_keeps_every_other_field() {
    let entry = CatalogEntry::new("a.jpg")
        .unwrap()
        .with_metadata(10, stamp(), Some(4), Some(3));
    let moved = entry.clone().with_path("b.jpg").unwrap();
    assert_eq!(moved.path(), "b.jpg");
    assert_eq!(moved.length, entry.length);
    assert_eq!(moved.last_write_utc, entry.last_write_utc);
    assert_eq!(moved.width, Some(4));
    assert_eq!(moved.height, Some(3));
}

// C#: CatalogEntryTests.WithMetadata_UnchangedStat_KeepsDimensions
#[test]
fn with_metadata_unchanged_stat_keeps_dimensions() {
    let entry = CatalogEntry::new("a.jpg")
        .unwrap()
        .with_metadata(10, stamp(), Some(4000), Some(3000));

    let refreshed = entry.with_metadata(10, stamp(), None, None);

    assert_eq!(refreshed.width, Some(4000));
    assert_eq!(refreshed.height, Some(3000));
}

// C#: CatalogEntryTests.WithMetadata_ChangedStat_DropsOrReplacesDimensions
#[test]
fn with_metadata_changed_stat_drops_or_replaces_dimensions() {
    let entry = CatalogEntry::new("a.jpg")
        .unwrap()
        .with_metadata(10, stamp(), Some(4000), Some(3000));

    let changed = entry.with_metadata(11, stamp(), None, None);
    let replaced = entry.with_metadata(11, stamp(), Some(10), Some(20));

    assert_eq!(changed.width, None);
    assert_eq!(changed.height, None);
    assert_eq!((replaced.width, replaced.height), (Some(10), Some(20)));
}

// C#: CaptureAndEntryMutationGapTests.WithMetadata_SuppliedDimensionsReplaceTheKnownOnesEvenWhenTheStatIsUnchanged
#[test]
fn with_metadata_supplied_dimensions_replace_the_known_ones_even_when_the_stat_is_unchanged() {
    let entry = CatalogEntry::new(r"C:\a.jpg")
        .unwrap()
        .with_metadata(10, stamp(), Some(100), Some(50));

    let updated = entry.with_metadata(10, stamp(), Some(200), Some(80));

    assert_eq!(updated.width, Some(200));
    assert_eq!(updated.height, Some(80));
}

// C#: CaptureAndEntryMutationGapTests.WithMetadata_UnsuppliedDimensions_AreKeptWhileTheStatIsUnchangedAndDroppedWhenItChanged
#[test]
fn with_metadata_unsupplied_dimensions_are_kept_while_unchanged_and_dropped_when_changed() {
    let entry = CatalogEntry::new(r"C:\a.jpg")
        .unwrap()
        .with_metadata(10, stamp(), Some(100), Some(50));

    let same = entry.with_metadata(10, stamp(), None, None);
    let other_length = entry.with_metadata(11, stamp(), None, None);
    let other_stamp = entry.with_metadata(10, at(stamp().ticks() + 10_000_000), None, None);

    assert_eq!((same.width, same.height), (Some(100), Some(50)));
    assert_eq!((other_length.width, other_length.height), (None, None));
    assert_eq!((other_stamp.width, other_stamp.height), (None, None));
}

#[test]
fn with_metadata_keeps_path_and_capture_group() {
    let group = CaptureGroup::new("a.jpg", "a.cr2", None).unwrap();
    let mut entry = CatalogEntry::new("a.jpg").unwrap();
    entry.capture_group = Some(group.clone());
    let refreshed = entry.with_metadata(1, stamp(), None, None);
    assert_eq!(refreshed.path(), "a.jpg");
    assert_eq!(refreshed.capture_group, Some(group));
}

#[test]
fn matches_requires_both_length_and_last_write_to_be_known_and_equal() {
    let stat = FileStat {
        length: 10,
        last_write_utc: stamp(),
    };
    let base = CatalogEntry::new("a.jpg").unwrap();

    assert!(!base.matches(&stat), "None never matches");
    assert!(base.with_metadata(10, stamp(), None, None).matches(&stat));
    assert!(!base.with_metadata(11, stamp(), None, None).matches(&stat));
    assert!(!base.with_metadata(9, stamp(), None, None).matches(&stat));
    assert!(
        !base
            .with_metadata(10, at(stamp().ticks() + 1), None, None)
            .matches(&stat)
    );

    let mut only_length = CatalogEntry::new("a.jpg").unwrap();
    only_length.length = Some(10);
    assert!(!only_length.matches(&stat), "a missing last-write time never matches");
}

#[test]
fn matches_never_matches_a_length_above_i64_max() {
    let entry = CatalogEntry::new("a.jpg")
        .unwrap()
        .with_metadata(i64::MAX, stamp(), None, None);
    let stat = FileStat {
        length: u64::MAX,
        last_write_utc: stamp(),
    };
    assert!(!entry.matches(&stat));
}

#[test]
fn catalog_entry_equality_is_case_sensitive_on_path_but_not_on_capture_group() {
    let a = CatalogEntry::new("A.jpg").unwrap();
    let b = CatalogEntry::new("a.jpg").unwrap();
    assert_ne!(a, b);

    let g1 = CaptureGroup::new("a.jpg", "a.cr2", None).unwrap();
    let g2 = CaptureGroup::new("A.JPG", "A.CR2", None).unwrap();
    let mut x = CatalogEntry::new("a.jpg").unwrap();
    let mut y = CatalogEntry::new("a.jpg").unwrap();
    x.capture_group = Some(g1);
    y.capture_group = Some(g2);
    assert_eq!(x, y);
}

// ---------------------------------------------------------------------------------------------------------------------
// ExplorerReason
// ---------------------------------------------------------------------------------------------------------------------

// C#: ExplorerReasonTests.Format_WithoutDetails_IsTheBareCode
#[test]
fn format_without_details_is_the_bare_code() {
    assert_eq!(explorer_reason::format(explorer_reason::TIMEOUT, &[]), "timeout");
}

// C#: ExplorerReasonTests.FormatThenParse_RoundTripsCodeAndDetails
#[test]
fn format_then_parse_round_trips_code_and_details() {
    let reason = explorer_reason::format(
        explorer_reason::COM_CALL_FAILED,
        &["IFolderView2.GetItem(3)", "0x80004005"],
    );

    let (code, details) = explorer_reason::parse(&reason);

    assert_eq!(code, explorer_reason::COM_CALL_FAILED);
    assert_eq!(details, ["IFolderView2.GetItem(3)".to_owned(), "0x80004005".to_owned()]);
}

// C#: ExplorerReasonTests.Parse_TextWithoutSeparator_IsItsOwnCodeWithNoDetails
#[test]
fn parse_text_without_separator_is_its_own_code_with_no_details() {
    let (code, details) = explorer_reason::parse("Something legacy");

    assert_eq!(code, "Something legacy");
    assert!(details.is_empty());
}

#[test]
fn parse_edge_cases_keep_empty_pieces() {
    let (code, details) = explorer_reason::parse("");
    assert_eq!(code, "");
    assert!(details.is_empty());

    let (code, details) = explorer_reason::parse("a||b");
    assert_eq!(code, "a");
    assert_eq!(details, ["".to_owned(), "b".to_owned()]);
}

// C#: ExplorerReasonTests.Codes_AreLanguageNeutral_NoProse (the C# reflection over the literal fields becomes a
// walk over `ALL_CODES`; the regex `^[a-z]+(-[a-z]+)*$` is checked by hand, not by a regex crate).
#[test]
fn codes_are_language_neutral_no_prose() {
    assert_eq!(ALL_CODES.len(), 12);
    let unique: HashSet<&str> = ALL_CODES.iter().copied().collect();
    assert_eq!(unique.len(), ALL_CODES.len(), "codes must be unique");
    for code in ALL_CODES {
        assert!(!code.is_empty(), "empty code");
        for part in code.split('-') {
            assert!(!part.is_empty(), "{code:?} has an empty segment");
            assert!(
                part.bytes().all(|b| b.is_ascii_lowercase()),
                "{code:?} must be [a-z]+ segments"
            );
        }
    }
}

#[test]
fn code_constants_have_their_persisted_values() {
    assert_eq!(explorer_reason::TIMEOUT, "timeout");
    assert_eq!(explorer_reason::CANCELED, "canceled");
    assert_eq!(explorer_reason::QUERY_FAILED, "query-failed");
    assert_eq!(explorer_reason::SHELL_UNAVAILABLE, "shell-unavailable");
    assert_eq!(explorer_reason::NATIVE_VIEW_FAILED, "native-view-failed");
    assert_eq!(explorer_reason::NO_MATCHING_WINDOW, "no-matching-window");
    assert_eq!(explorer_reason::COM_CALL_FAILED, "com-call-failed");
    assert_eq!(explorer_reason::EMPTY_VIEW, "empty-view");
    assert_eq!(explorer_reason::EMPTY_PATH, "empty-path");
    assert_eq!(explorer_reason::OUTSIDE_FOLDER, "outside-folder");
    assert_eq!(explorer_reason::DUPLICATE_ITEM, "duplicate-item");
    assert_eq!(explorer_reason::INCOMPLETE_SNAPSHOT, "incomplete-snapshot");
}

// C#: ExplorerReasonTests.Format_PipeInDetail_RoundTripsAsOneDetail (W2-CC-07)
#[test]
fn format_pipe_in_detail_round_trips_as_one_detail() {
    let (code, details) = explorer_reason::parse(&explorer_reason::format(
        explorer_reason::QUERY_FAILED,
        &["a|b", "plain"],
    ));

    assert_eq!(code, explorer_reason::QUERY_FAILED);
    assert_eq!(details, ["a/b".to_owned(), "plain".to_owned()]);
}

#[test]
fn format_replaces_a_pipe_in_the_code_too() {
    assert_eq!(explorer_reason::format("a|b", &[]), "a/b");
    assert_eq!(explorer_reason::format("x", &[""]), "x|");
    assert_eq!(explorer_reason::format("x", &["", "y"]), "x||y");
}

// ---------------------------------------------------------------------------------------------------------------------
// LatestExplorerSnapshot and ExplorerQueryHandle
// ---------------------------------------------------------------------------------------------------------------------

// C#: FolderLoadCoordinatorTests.LatestExplorer.Current_QueryStillRunning_IsNull
#[test]
fn current_query_still_running_is_none() {
    let latest = LatestExplorerSnapshot::new();
    let handle = ExplorerQueryHandle::new();
    latest.set(Some(handle.clone()));

    assert!(latest.current().is_none());
}

#[test]
fn current_completed_query_is_some() {
    let latest = LatestExplorerSnapshot::new();
    let handle = ExplorerQueryHandle::new();
    latest.set(Some(handle.clone()));
    assert!(handle.complete(sample_snapshot("C:\\photos")));

    let current = latest.current().unwrap();
    assert_eq!(current.folder, "C:\\photos");
    assert_eq!(*current, sample_snapshot("C:\\photos"));
}

#[test]
fn current_failed_or_canceled_query_is_none() {
    let latest = LatestExplorerSnapshot::new();
    let handle = ExplorerQueryHandle::new();
    latest.set(Some(handle.clone()));
    assert!(handle.fail());

    assert!(latest.current().is_none());
}

#[test]
fn current_set_none_is_none() {
    let latest = LatestExplorerSnapshot::new();
    let handle = ExplorerQueryHandle::new();
    handle.complete(sample_snapshot("C:\\photos"));
    latest.set(Some(handle));
    assert!(latest.current().is_some());

    latest.set(None);
    assert!(latest.current().is_none());
}

#[test]
fn current_a_later_set_replaces_the_earlier_one() {
    let latest = LatestExplorerSnapshot::new();
    let first = ExplorerQueryHandle::new();
    first.complete(sample_snapshot("C:\\first"));
    latest.set(Some(first));

    let second = ExplorerQueryHandle::new();
    latest.set(Some(second.clone()));
    assert!(
        latest.current().is_none(),
        "the new pending query hides the old snapshot"
    );

    assert!(second.complete(sample_snapshot("C:\\second")));
    assert_eq!(latest.current().unwrap().folder, "C:\\second");
}

#[test]
fn explorer_query_handle_completes_or_fails_at_most_once() {
    let handle = ExplorerQueryHandle::new();
    assert!(handle.snapshot().is_none(), "pending");
    assert!(handle.complete(sample_snapshot("C:\\a")));
    assert!(!handle.complete(sample_snapshot("C:\\b")));
    assert!(!handle.fail());
    assert_eq!(handle.snapshot().unwrap().folder, "C:\\a");

    let failed = ExplorerQueryHandle::new();
    assert!(failed.fail());
    assert!(!failed.complete(sample_snapshot("C:\\c")));
    assert!(failed.snapshot().is_none());
}

#[test]
fn explorer_query_handle_clones_share_the_same_cell() {
    let handle = ExplorerQueryHandle::new();
    let clone = handle.clone();
    assert!(clone.complete(sample_snapshot("C:\\shared")));
    assert_eq!(handle.snapshot().unwrap().folder, "C:\\shared");
}

#[test]
fn latest_snapshot_defaults_to_none() {
    assert!(LatestExplorerSnapshot::default().current().is_none());
    let latest = LatestExplorerSnapshot::new();
    latest.set(None);
    assert!(latest.current().is_none());
}

// Shared snapshots are handed out as `Arc`s: two reads of a completed query return the same allocation.
#[test]
fn completed_snapshot_is_shared_not_copied() {
    let latest = LatestExplorerSnapshot::new();
    let handle = ExplorerQueryHandle::new();
    handle.complete(sample_snapshot("C:\\s"));
    latest.set(Some(handle));
    let a: Arc<ExplorerViewSnapshot> = latest.current().unwrap();
    let b = latest.current().unwrap();
    assert!(Arc::ptr_eq(&a, &b));
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/image-file-types.json (oracle-cs image-file-types)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn image_file_types_golden_matches_dotnet() {
    let g = golden::load("image-file-types");
    assert_eq!(g.generator, "oracle-cs image-file-types");
    for case in &g.cases {
        // C# null path (`string?`): the Rust twin is `Option<&str>` with `path.is_some_and(...)`, so a null case
        // expects every answer to be false and no extension.
        if case["path"].is_null() {
            assert!(case["extension"].is_null(), "null path has a null extension");
            assert!(!bool_of(case, "supported") && !bool_of(case, "supported_raw_enabled"));
            assert!(!bool_of(case, "is_raw"));
            let none: Option<&str> = None;
            assert!(!none.is_some_and(is_supported));
            assert!(!none.is_some_and(|p| is_supported_with_raw(p, true)));
            assert!(!none.is_some_and(is_raw_path));
            continue;
        }
        let path = str_of(case, "path");
        assert_eq!(get_extension(path), str_of(case, "extension"), "extension of {path:?}");
        assert_eq!(is_supported(path), bool_of(case, "supported"), "supported {path:?}");
        assert_eq!(
            is_supported_with_raw(path, true),
            bool_of(case, "supported_raw_enabled"),
            "supported_raw_enabled {path:?}"
        );
        assert_eq!(is_raw_path(path), bool_of(case, "is_raw"), "is_raw {path:?}");
    }
}

// Every RAW extension is supported only when RAW is enabled, and is always a RAW path.
#[test]
fn every_raw_extension_is_supported_only_when_raw_is_enabled() {
    for ext in RAW_EXTENSIONS {
        let name = format!("shot{ext}");
        assert!(is_supported_with_raw(&name, true), "{name} with RAW enabled");
        assert!(!is_supported_with_raw(&name, false), "{name} must need the RAW flag");
        assert!(is_raw_path(&name), "{name} is a RAW path");
    }
}
