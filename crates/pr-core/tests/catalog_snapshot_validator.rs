//! pr_core::catalog::snapshot_validator: ports of the C# ExplorerSnapshotValidator* tests plus golden `snapshot-validator`.

use pr_core::catalog::explorer_reason::{DUPLICATE_ITEM, EMPTY_PATH, INCOMPLETE_SNAPSHOT, OUTSIDE_FOLDER};
use pr_core::catalog::explorer_types::{ExplorerGroupState, ExplorerOrderStatus, ExplorerViewSnapshot};
use pr_core::catalog::snapshot_validator::{canonicalize_folder, normalize_path, same_path, try_validate};
use pr_core::text::dotnet_path::{PathError, get_directory_name, get_full_path, trim_ending_directory_separator};
use pr_core::text::{eq_ignore_case, ignore_case_key};
use pr_core::time::DateTimeUtc;
use pr_test_support::golden::{self, str_of};
use std::collections::HashSet;

const FOLDER: &str = r"C:\photos";

fn snap_with(
    folder: &str,
    ordered: &[&str],
    status: ExplorerOrderStatus,
    reason: Option<&str>,
) -> ExplorerViewSnapshot {
    ExplorerViewSnapshot {
        folder: folder.to_owned(),
        ordered_paths: ordered.iter().map(|s| (*s).to_owned()).collect(),
        sort_columns: Vec::new(),
        group_state: ExplorerGroupState::None,
        status,
        reason: reason.map(str::to_owned),
        captured_utc: DateTimeUtc::MIN,
    }
}

fn snap(folder: &str, ordered: &[&str]) -> ExplorerViewSnapshot {
    snap_with(folder, ordered, ExplorerOrderStatus::Available, None)
}

fn owned(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

fn run(folder: &str, ordered: &[&str], scanned: &[&str]) -> Result<Vec<String>, String> {
    try_validate(&snap(folder, ordered), &owned(scanned))
}

/// Deterministic xorshift64 (the C# tests use `System.Random` with a seed; only determinism matters).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x >> 11
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

// C#: ExplorerSnapshotValidatorTests.TryValidateUnavailableStatusReturnsFalseWithReason
#[test]
fn try_validate_unavailable_status_returns_false_with_reason() {
    let a = r"C:\photos\a.jpg";
    let s = snap_with(FOLDER, &[a], ExplorerOrderStatus::TimedOut, Some("Timeout occurred"));
    assert_eq!(try_validate(&s, &owned(&[a])), Err("Timeout occurred".to_owned()));
}

// C#: ExplorerSnapshotValidatorTests.TryValidateValidSnapshotReturnsTrueAndOrderedList
#[test]
fn try_validate_valid_snapshot_returns_true_and_ordered_list() {
    let (a, b) = (r"C:\photos\a.jpg", r"C:\photos\b.jpg");
    assert_eq!(run(FOLDER, &[b, a], &[a, b]), Ok(owned(&[b, a])));
}

// C#: ExplorerSnapshotValidatorTests.TryValidateMissingExpectedFileReturnsFalse
#[test]
fn try_validate_missing_expected_file_returns_false() {
    let (a, b) = (r"C:\photos\a.jpg", r"C:\photos\b.jpg");
    assert_eq!(run(FOLDER, &[a], &[a, b]), Err(INCOMPLETE_SNAPSHOT.to_owned()));
}

// C#: ExplorerSnapshotValidatorTests.TryValidateDuplicateItemReturnsFalse
#[test]
fn try_validate_duplicate_item_returns_false() {
    let (a, b) = (r"C:\photos\a.jpg", r"C:\photos\b.jpg");
    assert_eq!(run(FOLDER, &[a, a, b], &[a, b]), Err(DUPLICATE_ITEM.to_owned()));
}

// C#: ExplorerSnapshotValidatorTests.TryValidatePathOutsideFolderReturnsFalse
#[test]
fn try_validate_path_outside_folder_returns_false() {
    let a = r"C:\photos\a.jpg";
    assert_eq!(
        run(FOLDER, &[a, r"C:\other\c.jpg"], &[a]),
        Err(OUTSIDE_FOLDER.to_owned())
    );
}

// C#: ExplorerSnapshotValidatorTests.TryValidateEmptyCandidatePathReturnsFalse
#[test]
fn try_validate_empty_candidate_path_returns_false() {
    let a = r"C:\photos\a.jpg";
    assert_eq!(run(FOLDER, &["", a], &[a]), Err(EMPTY_PATH.to_owned()));
}

// C#: ExplorerSnapshotValidatorTests.CanonicalizeFolderAndSamePathWorkCorrectly
#[test]
fn canonicalize_folder_and_same_path_work_correctly() {
    let canonical = canonicalize_folder(FOLDER).expect("canonical");
    assert!(!canonical.ends_with('\\'));
    assert_eq!(same_path(FOLDER, r"C:\photos\"), Ok(true));
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.NonAvailableStatusIsRejectedWithItsReasonOrStatusName
#[test]
fn non_available_status_is_rejected_with_its_reason_or_status_name() {
    use ExplorerOrderStatus::*;
    let rows: [(ExplorerOrderStatus, Option<&str>, &str); 6] = [
        (NoMatchingWindow, None, "NoMatchingWindow"),
        (NativeViewUnavailable, None, "NativeViewUnavailable"),
        (InvalidSnapshot, None, "InvalidSnapshot"),
        (TimedOut, Some("timeout"), "timeout"),
        (Canceled, Some("canceled"), "canceled"),
        (Failed, Some("query-failed|IOException"), "query-failed|IOException"),
    ];
    let a = r"C:\photos\a.jpg";
    for (status, reason, expected) in rows {
        let s = snap_with(FOLDER, &[a], status, reason);
        assert_eq!(try_validate(&s, &owned(&[a])), Err(expected.to_owned()), "{status:?}");
    }
}

// Not in C#: an empty-string reason is kept as is (only `None` falls back to the status name).
#[test]
fn empty_string_reason_is_kept() {
    let s = snap_with(FOLDER, &[], ExplorerOrderStatus::Failed, Some(""));
    assert_eq!(try_validate(&s, &[]), Err(String::new()));
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.EquivalentSpellingsOfTheSameFileAreAccepted
#[test]
fn equivalent_spellings_of_the_same_file_are_accepted() {
    let rows = [
        (r"C:\photos\a.jpg", r"C:\photos\a.jpg"),
        (r"C:\PHOTOS\A.JPG", r"C:\photos\a.jpg"),
        (r"C:\photos\.\a.jpg", r"C:\photos\a.jpg"),
        (r"C:\photos\x\..\a.jpg", r"C:\photos\a.jpg"),
        ("C:/photos/a.jpg", r"C:\photos\a.jpg"),
        (r"\\?\C:\photos\a.jpg", r"C:\photos\a.jpg"),
        (r"C:\photos\Ảnh đẹp 01.jpg", r"C:\photos\ẢNH ĐẸP 01.jpg"),
    ];
    for (reported, scanned) in rows {
        let ordered = run(FOLDER, &[reported], &[scanned]).unwrap_or_else(|e| panic!("{reported}: {e}"));
        assert_eq!(ordered.len(), 1);
        assert!(
            eq_ignore_case(&get_full_path(scanned).expect("full"), &ordered[0]),
            "{reported}"
        );
    }
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.PathsThatAreNotDirectChildrenAreRejectedWithTheirReasonAndNeverThrow
#[test]
fn paths_that_are_not_direct_children_are_rejected_with_their_reason_and_never_throw() {
    let rows = [
        (r"C:\photos\sub\a.jpg", "outside-folder"),
        (r"C:\photos2\a.jpg", "outside-folder"),
        (r"C:\photo\a.jpg", "outside-folder"),
        (r"C:\photos\..\other\a.jpg", "outside-folder"),
        (r"D:\photos\a.jpg", "outside-folder"),
        (r"\\server\share\photos\a.jpg", "outside-folder"),
        ("C:\\photos\\a\0.jpg", "outside-folder"),
        ("", "empty-path"),
        ("   ", "empty-path"),
    ];
    for (candidate, reason) in rows {
        let r = run(FOLDER, &[r"C:\photos\a.jpg", candidate], &[r"C:\photos\a.jpg"]);
        assert_eq!(r, Err(reason.to_owned()), "{candidate:?}");
    }
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.ScannedFileWithInvalidCharacterIsRejectedInsteadOfThrowing
#[test]
fn scanned_file_with_invalid_character_is_rejected_instead_of_throwing() {
    assert_eq!(
        run(FOLDER, &[r"C:\photos\a.jpg"], &["C:\\photos\\bad\0.jpg"]),
        Err(OUTSIDE_FOLDER.to_owned())
    );
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.DuplicatesDifferingOnlyByCaseAreRejected
#[test]
fn duplicates_differing_only_by_case_are_rejected() {
    let r = run(
        FOLDER,
        &[r"C:\photos\a.jpg", r"C:\photos\A.JPG", r"C:\photos\b.jpg"],
        &[r"C:\photos\a.jpg", r"C:\photos\b.jpg"],
    );
    assert_eq!(r, Err(DUPLICATE_ITEM.to_owned()));
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.DuplicateOfAnUnscannedFileIsStillRejected
#[test]
fn duplicate_of_an_unscanned_file_is_still_rejected() {
    let r = run(
        FOLDER,
        &[r"C:\photos\x.txt", r"C:\photos\x.txt", r"C:\photos\a.jpg"],
        &[r"C:\photos\a.jpg"],
    );
    assert_eq!(r, Err(DUPLICATE_ITEM.to_owned()));
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.UnscannedNonImageEntriesAreIgnoredButOrderOfTheRestIsKept
#[test]
fn unscanned_non_image_entries_are_ignored_but_order_of_the_rest_is_kept() {
    let r = run(
        FOLDER,
        &[
            r"C:\photos\readme.txt",
            r"C:\photos\b.jpg",
            r"C:\photos\notes.doc",
            r"C:\photos\a.jpg",
        ],
        &[r"C:\photos\a.jpg", r"C:\photos\b.jpg"],
    );
    assert_eq!(r, Ok(owned(&[r"C:\photos\b.jpg", r"C:\photos\a.jpg"])));
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.ScannedFileMissingFromSnapshotIsIncomplete
#[test]
fn scanned_file_missing_from_snapshot_is_incomplete() {
    let r = run(
        FOLDER,
        &[r"C:\photos\a.jpg", r"C:\photos\readme.txt"],
        &[r"C:\photos\a.jpg", r"C:\photos\b.jpg"],
    );
    assert_eq!(r, Err(INCOMPLETE_SNAPSHOT.to_owned()));
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.EmptySnapshotIsValidOnlyForAnEmptyScan
#[test]
fn empty_snapshot_is_valid_only_for_an_empty_scan() {
    assert_eq!(run(FOLDER, &[], &[]), Ok(Vec::new()));
    assert_eq!(
        run(FOLDER, &[], &[r"C:\photos\a.jpg"]),
        Err(INCOMPLETE_SNAPSHOT.to_owned())
    );
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.SnapshotFolderSpellingDoesNotChangeTheVerdict
#[test]
fn snapshot_folder_spelling_does_not_change_the_verdict() {
    for folder in [r"C:\photos\", r"c:\PHOTOS", r"C:\photos\.", r"\\?\C:\photos"] {
        let r = run(
            folder,
            &[r"C:\photos\b.jpg", r"C:\photos\a.jpg"],
            &[r"C:\photos\a.jpg", r"C:\photos\b.jpg"],
        );
        assert_eq!(r.map(|v| v.len()), Ok(2), "{folder}");
    }
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.DriveRootFolderIsSupported
#[test]
fn drive_root_folder_is_supported() {
    assert_eq!(run(r"C:\", &[r"C:\a.jpg"], &[r"C:\a.jpg"]), Ok(owned(&[r"C:\a.jpg"])));
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.UncFolderIsSupportedInBothSpellings
#[test]
fn unc_folder_is_supported_in_both_spellings() {
    let r = run(
        r"\\srv\share\photos",
        &[r"\\?\UNC\srv\share\photos\a.jpg"],
        &[r"\\srv\share\photos\a.jpg"],
    );
    assert_eq!(r, Ok(owned(&[r"\\srv\share\photos\a.jpg"])));
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.LongPathsBeyondMaxPathAreSupported
#[test]
fn long_paths_beyond_max_path_are_supported() {
    let deep = format!(r"{FOLDER}\{}\{}", "d".repeat(120), "e".repeat(120));
    let file = format!(r"{deep}\{}.jpg", "f".repeat(60));
    let reported = format!(r"\\?\{file}");
    assert_eq!(run(&deep, &[&reported], &[&file]), Ok(vec![file.clone()]));
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.NormalizePathStripsExtendedPrefixOnly
#[test]
fn normalize_path_strips_extended_prefix_only() {
    let rows = [
        (r"\\?\C:\a", r"C:\a"),
        (r"\\?\UNC\srv\share\a", r"\\srv\share\a"),
        (r"C:\a\..\b", r"C:\b"),
        (r"\\srv\share\a", r"\\srv\share\a"),
    ];
    for (input, expected) in rows {
        assert_eq!(normalize_path(input), Ok(expected.to_owned()), "{input}");
    }
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.AnyPermutationValidatesToItself
#[test]
fn any_permutation_validates_to_itself() {
    let mut rng = Rng::new(20_260_926);
    let files: Vec<String> = (0..50).map(|i| format!(r"C:\photos\IMG_{i:04} ảnh.jpg")).collect();
    for _ in 0..40 {
        let mut permuted = files.clone();
        rng.shuffle(&mut permuted);
        let spelled: Vec<String> = permuted
            .iter()
            .map(|p| match rng.below(3) {
                0 => p.to_uppercase(),
                1 => p.replace('\\', "/"),
                _ => p.clone(),
            })
            .collect();
        let spelled_ref: Vec<&str> = spelled.iter().map(String::as_str).collect();
        let files_ref: Vec<&str> = files.iter().map(String::as_str).collect();
        let ordered = run(FOLDER, &spelled_ref, &files_ref).expect("valid");
        assert_eq!(ordered.len(), permuted.len());
        for (o, p) in ordered.iter().zip(&permuted) {
            assert!(eq_ignore_case(o, p));
        }
    }
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.AnySingleMutationIsDetected
#[test]
fn any_single_mutation_is_detected() {
    let files: Vec<String> = (0..12).map(|i| format!(r"C:\photos\{i}.jpg")).collect();
    let files_ref: Vec<&str> = files.iter().map(String::as_str).collect();
    for (i, file) in files.iter().enumerate() {
        let without: Vec<&str> = files_ref
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != i)
            .map(|(_, f)| *f)
            .collect();
        assert_eq!(run(FOLDER, &without, &files_ref), Err(INCOMPLETE_SNAPSHOT.to_owned()));

        let upper = file.to_uppercase();
        let mut duplicated = files_ref.clone();
        duplicated.push(&upper);
        assert_eq!(run(FOLDER, &duplicated, &files_ref), Err(DUPLICATE_ITEM.to_owned()));
    }
}

// C#: ExplorerSnapshotValidatorEdgeCaseTests.CanonicalizeFolderIsIdempotent
#[test]
fn canonicalize_folder_is_idempotent() {
    let mut rng = Rng::new(7);
    let folders = [
        r"C:\a",
        r"C:\a\b\",
        r"C:\a\.\b",
        r"C:\a\x\..\b",
        r"\\srv\share\a\",
        r"C:\",
        r"\\?\D:\deep\x\",
    ];
    for folder in folders {
        let once = canonicalize_folder(folder).expect("once");
        assert_eq!(canonicalize_folder(&once), Ok(once.clone()));
        let with_sep = if once.ends_with('\\') {
            once.clone()
        } else {
            format!(r"{once}\")
        };
        assert_eq!(same_path(folder, &with_sep), Ok(true));
        let cased = if rng.below(2) == 0 {
            once.to_uppercase()
        } else {
            once.to_lowercase()
        };
        assert_eq!(same_path(folder, &cased), Ok(true));
    }
}

// C#: ExplorerSnapshotValidatorRobustnessTests.InvalidCandidatePath_ReturnsFalse
#[test]
fn invalid_candidate_path_returns_false() {
    for bad in ["C:\\photos\\a\0.jpg", "C:\\photos\\<>|.jpg", "\0"] {
        let r = run(FOLDER, &[bad], &[r"C:\photos\a.jpg"]);
        assert!(r.is_err(), "{bad:?}");
    }
}

// C#: ExplorerSnapshotValidatorRobustnessTests.InvalidScannedPath_ReturnsFalse
#[test]
fn invalid_scanned_path_returns_false() {
    assert!(run(FOLDER, &[r"C:\photos\a.jpg"], &["C:\\photos\\a\0.jpg"]).is_err());
}

// C#: ExplorerSnapshotValidatorRobustnessTests.NullPieces_ReturnFalse (rows "" and "   "; the null folder and the null
// path list have no Rust twin: `String` and `Vec` cannot be null)
#[test]
fn null_pieces_return_false() {
    for folder in ["", "   "] {
        assert!(
            run(folder, &[r"C:\photos\a.jpg"], &[r"C:\photos\a.jpg"]).is_err(),
            "{folder:?}"
        );
    }
}

// C#: ExplorerSnapshotValidatorRobustnessTests.EquivalentSpellings_Validate
#[test]
fn equivalent_spellings_validate() {
    let r = run(
        "c:/PHOTOS/",
        &[r"C:\photos\sub\..\B.JPG", "c:/photos/a.jpg"],
        &[r"C:\photos\a.jpg", r"C:\photos\b.jpg"],
    )
    .expect("valid");
    let lower: Vec<String> = r.iter().map(|p| p.to_lowercase()).collect();
    assert_eq!(lower, owned(&[r"c:\photos\b.jpg", r"c:\photos\a.jpg"]));
}

// C#: ExplorerSnapshotValidatorRobustnessTests.ExtraFiles_Ignored
#[test]
fn extra_files_ignored() {
    let r = run(
        FOLDER,
        &[
            r"C:\photos\z.txt",
            r"C:\photos\b.jpg",
            r"C:\photos\notes.docx",
            r"C:\photos\a.jpg",
        ],
        &[r"C:\photos\a.jpg", r"C:\photos\b.jpg"],
    );
    assert_eq!(r, Ok(owned(&[r"C:\photos\b.jpg", r"C:\photos\a.jpg"])));
}

// C#: ExplorerSnapshotValidatorRobustnessTests.Fuzz_PermutationOracle
#[test]
fn fuzz_permutation_oracle() {
    for seed in 1..=3_u64 {
        let mut r = Rng::new(seed);
        for _ in 0..300 {
            let n = 1 + r.below(24);
            let scanned: Vec<String> = (0..n)
                .map(|i| format!(r"C:\photos\img{i}{}", if r.below(2) == 0 { ".jpg" } else { ".png" }))
                .collect();
            let mut order = scanned.clone();
            r.shuffle(&mut order);
            let order: Vec<String> = order
                .into_iter()
                .map(|p| if r.below(3) == 0 { p.to_uppercase() } else { p })
                .collect();
            let order_ref: Vec<&str> = order.iter().map(String::as_str).collect();
            let scanned_ref: Vec<&str> = scanned.iter().map(String::as_str).collect();

            let ordered = run(FOLDER, &order_ref, &scanned_ref).expect("permutation validates");
            let up_order: Vec<String> = order.iter().map(|p| p.to_uppercase()).collect();
            let up_ordered: Vec<String> = ordered.iter().map(|p| p.to_uppercase()).collect();
            assert_eq!(up_order, up_ordered);

            let mut broken = order.clone();
            match r.below(4) {
                0 => {
                    let i = r.below(broken.len());
                    broken.remove(i);
                }
                1 => {
                    let i = r.below(broken.len());
                    let dup = broken[i].clone();
                    broken.push(dup);
                }
                2 => broken.push(r"C:\elsewhere\x.jpg".to_owned()),
                _ => {
                    let i = r.below(broken.len());
                    broken[i] = r"C:\photos\sub\deep.jpg".to_owned();
                }
            }
            let broken_ref: Vec<&str> = broken.iter().map(String::as_str).collect();
            assert!(run(FOLDER, &broken_ref, &scanned_ref).is_err());
        }
    }
}

// C#: CatalogMutationGapTests.NormalizePath_StripsTheExtendedPrefixOnlyFromDriveLetterPaths
#[test]
fn normalize_path_strips_the_extended_prefix_only_from_drive_letter_paths() {
    let rows = [
        (r"\\?\C:", "C:"),
        (r"\\?\C:\photos\a.jpg", r"C:\photos\a.jpg"),
        (r"\\?\UNC\srv\share\a.jpg", r"\\srv\share\a.jpg"),
        (r"\\?\x", r"\\?\x"),
        (r"\\?\Volume{1}\a.jpg", r"\\?\Volume{1}\a.jpg"),
        (r"C:\ab:c", r"C:\ab:c"),
        (r"C:\photos\a.jpg", r"C:\photos\a.jpg"),
    ];
    for (input, expected) in rows {
        assert_eq!(normalize_path(input), Ok(expected.to_owned()), "{input}");
    }
}

// C#: CatalogMutationGapTests.ExplorerSnapshotValidator_NullSnapshot_IsAnIncompleteSnapshotNotACrash and
// ..._NullScannedFiles_IsAnIncompleteSnapshotNotACrash: no Rust twin (`&ExplorerViewSnapshot` and `&[String]` are
// non-optional); the contract documents this as a non-port.

// ---- C# tests/PhotoReview.App.Tests/CacheExplorerRegressionTests.cs ----

fn cache_snapshot(ordered: &[&str]) -> ExplorerViewSnapshot {
    snap(r"C:\root\folder", ordered)
}

const CA: &str = r"C:\root\folder\a.jpg";
const CB: &str = r"C:\root\folder\b.jpg";
const CC: &str = r"C:\root\folder\c.jpg";

// C#: CacheExplorerRegressionTests.ExplorerValidatorPreservesCompleteNativeOrder
#[test]
fn explorer_validator_preserves_complete_native_order() {
    let r = try_validate(&cache_snapshot(&[CC, CA, CB]), &owned(&[CA, CB, CC]));
    assert_eq!(r, Ok(owned(&[CC, CA, CB])));
}

// C#: CacheExplorerRegressionTests.ExplorerValidatorRejectsDuplicateNativeItems
#[test]
fn explorer_validator_rejects_duplicate_native_items() {
    let r = try_validate(&cache_snapshot(&[CC, CC, CA]), &owned(&[CA, CB, CC]));
    assert!(r.expect_err("rejected").to_lowercase().contains("duplicate"));
}

// C#: CacheExplorerRegressionTests.ExplorerValidatorRejectsItemOutsideFolder
#[test]
fn explorer_validator_rejects_item_outside_folder() {
    let r = try_validate(
        &cache_snapshot(&[CC, CA, r"C:\root\outside.jpg"]),
        &owned(&[CA, CB, CC]),
    );
    assert!(r.expect_err("rejected").to_lowercase().contains("outside"));
}

// C#: CacheExplorerRegressionTests.ExplorerValidatorRejectsIncompleteSnapshots
#[test]
fn explorer_validator_rejects_incomplete_snapshots() {
    let r = try_validate(&cache_snapshot(&[CC, CA]), &owned(&[CA, CB, CC]));
    assert!(r.expect_err("rejected").to_lowercase().contains("complete"));
}

// C#: CacheExplorerRegressionTests.ExplorerValidatorFallsBackOnMalformedNativePaths
#[test]
fn explorer_validator_falls_back_on_malformed_native_paths() {
    // "not-a-path" resolves against the process directory => outside the folder.
    assert!(try_validate(&cache_snapshot(&["not-a-path"]), &owned(&[CA, CB, CC])).is_err());
}

// ---- C# ExplorerSnapshotValidatorReference (verbatim pre-optimisation algorithm) ----

fn ref_normalize(path: &str) -> Result<String, PathError> {
    let full = get_full_path(path)?;
    let chars: Vec<char> = full.chars().collect();
    if chars.len() >= 8 && eq_ignore_case(&chars[..8].iter().collect::<String>(), r"\\?\UNC\") {
        return Ok(format!(r"\\{}", chars[8..].iter().collect::<String>()));
    }
    if chars.len() >= 6 && full.starts_with(r"\\?\") && chars[5] == ':' {
        return Ok(chars[4..].iter().collect());
    }
    Ok(full)
}

fn ref_canonicalize(folder: &str) -> Result<String, PathError> {
    Ok(trim_ending_directory_separator(&ref_normalize(folder)?).to_owned())
}

fn ref_same_path(a: &str, b: &str) -> Result<bool, PathError> {
    Ok(eq_ignore_case(&ref_canonicalize(a)?, &ref_canonicalize(b)?))
}

/// Returns the normalized spellings, like the C# oracle.
fn reference_try_validate(snapshot: &ExplorerViewSnapshot, scanned: &[String]) -> Result<Vec<String>, String> {
    if snapshot.status != ExplorerOrderStatus::Available {
        return Err(snapshot
            .reason
            .clone()
            .unwrap_or_else(|| format!("{:?}", snapshot.status)));
    }
    let setup = (|| -> Result<(String, HashSet<String>, Vec<String>), PathError> {
        let folder = ref_canonicalize(&snapshot.folder)?;
        let mut keys = HashSet::new();
        let mut list = Vec::new();
        for s in scanned {
            let n = ref_normalize(s)?;
            if keys.insert(ignore_case_key(&n)) {
                list.push(n);
            }
        }
        Ok((folder, keys, list))
    })();
    let Ok((folder, expected, expected_list)) = setup else {
        return Err(OUTSIDE_FOLDER.to_owned());
    };
    let mut result: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for candidate in &snapshot.ordered_paths {
        if candidate.trim().is_empty() {
            return Err(EMPTY_PATH.to_owned());
        }
        let Ok(path) = ref_normalize(candidate) else {
            return Err(OUTSIDE_FOLDER.to_owned());
        };
        let dir = get_directory_name(&path).unwrap_or_default();
        match ref_same_path(&dir, &folder) {
            Ok(true) => {}
            Ok(false) => return Err(OUTSIDE_FOLDER.to_owned()),
            Err(_) => return Err(INCOMPLETE_SNAPSHOT.to_owned()),
        }
        let key = ignore_case_key(&path);
        if !seen.insert(key.clone()) {
            return Err(DUPLICATE_ITEM.to_owned());
        }
        if expected.contains(&key) {
            result.push(path);
        }
    }
    let result_keys: HashSet<String> = result.iter().map(|p| ignore_case_key(p)).collect();
    if result.len() != expected_list.len() || result_keys != expected {
        return Err(INCOMPLETE_SNAPSHOT.to_owned());
    }
    Ok(result)
}

// C#: ExplorerSnapshotValidatorEquivalenceTests.MatchesOriginal_OnMutatedSnapshots
#[test]
fn matches_original_on_mutated_snapshots() {
    let folder = r"C:\Photos\Set A";
    let scanned: Vec<String> = (0..8).map(|i| format!(r"{folder}\img{i}.jpg")).collect();
    // The odd one-backslash `\?\` spelling is kept on purpose, like the C# test.
    let spellings = [
        folder.to_owned(),
        format!(r"{folder}\"),
        folder.to_uppercase(),
        format!(r"\?\{folder}"),
        r"C:\Photos\Set A\..\Set A".to_owned(),
        r"C:\Photos".to_owned(),
    ];
    let mut r = Rng::new(7);
    for iteration in 0..3000 {
        let mut order = scanned.clone();
        r.shuffle(&mut order);
        match r.below(9) {
            1 => {
                let i = r.below(order.len());
                order.remove(i);
            }
            2 => {
                let i = r.below(order.len());
                let d = order[i].clone();
                order.push(d);
            }
            3 => order.push(format!(r"{folder}\extra.jpg")),
            4 => {
                let i = r.below(order.len());
                order[i] = format!(r"C:\Other\img{}.jpg", r.below(8));
            }
            5 => {
                let i = r.below(order.len());
                order[i] = order[0].to_uppercase();
            }
            6 => {
                let i = r.below(order.len());
                order[i] = format!(r"\?\{}", order[0]);
            }
            7 => {
                let i = r.below(order.len());
                order[i] = String::new();
            }
            8 => {
                let i = r.below(order.len());
                order[i] = format!(r"{folder}\sub\..\img{}.jpg", r.below(8));
            }
            _ => {}
        }
        let spelling = &spellings[r.below(spellings.len())];
        let order_ref: Vec<&str> = order.iter().map(String::as_str).collect();
        let snapshot = snap(spelling, &order_ref);
        let expected = reference_try_validate(&snapshot, &scanned);
        let actual = try_validate(&snapshot, &scanned);
        match (expected, actual) {
            (Ok(exp), Ok(act)) => {
                // The oracle returns normalized spellings; the validator returns the caller's scanned strings.
                let mapped: Vec<String> = exp
                    .iter()
                    .map(|p| {
                        scanned
                            .iter()
                            .find(|s| normalize_path(s).is_ok_and(|n| eq_ignore_case(&n, p)))
                            .cloned()
                            .unwrap_or_default()
                    })
                    .collect();
                assert_eq!(mapped, act, "iteration {iteration}");
            }
            (Err(e), Err(a)) => assert_eq!(e, a, "iteration {iteration}"),
            (e, a) => panic!("iteration {iteration}: verdict differs: {e:?} vs {a:?}"),
        }
    }
}

// ---- golden `snapshot-validator` ----

fn status_of(name: &str) -> ExplorerOrderStatus {
    match name {
        "Available" => ExplorerOrderStatus::Available,
        "NoMatchingWindow" => ExplorerOrderStatus::NoMatchingWindow,
        "NativeViewUnavailable" => ExplorerOrderStatus::NativeViewUnavailable,
        "InvalidSnapshot" => ExplorerOrderStatus::InvalidSnapshot,
        "TimedOut" => ExplorerOrderStatus::TimedOut,
        "Canceled" => ExplorerOrderStatus::Canceled,
        "Failed" => ExplorerOrderStatus::Failed,
        other => panic!("unknown status {other}"),
    }
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|s| s.as_str().expect("string").to_owned())
        .collect()
}

fn check_throws<T: std::fmt::Debug>(expected: &serde_json::Value, got: &Result<T, PathError>, what: &str) {
    assert_eq!(expected["throws"], "ArgumentException", "{what}");
    assert_eq!(got.as_ref().err(), Some(&PathError::Argument), "{what}: {got:?}");
}

// C#: oracle command snapshot-validator (ExplorerSnapshotValidator.TryValidate / NormalizePath / CanonicalizeFolder / SamePath).
#[test]
fn snapshot_validator_matches_golden() {
    let g = golden::load("snapshot-validator");
    let (mut validate, mut normalize, mut same) = (0, 0, 0);
    for case in &g.cases {
        match str_of(case, "kind") {
            "validate" => {
                let name = str_of(case, "name");
                let ordered_paths = strings(&case["ordered_paths"]);
                let snapshot = ExplorerViewSnapshot {
                    folder: str_of(case, "folder").to_owned(),
                    ordered_paths,
                    sort_columns: Vec::new(),
                    group_state: ExplorerGroupState::None,
                    status: status_of(str_of(case, "status")),
                    reason: case["snapshot_reason"].as_str().map(str::to_owned),
                    captured_utc: DateTimeUtc::MIN,
                };
                let got = try_validate(&snapshot, &strings(&case["scanned"]));
                if case["valid"].as_bool().expect("valid") {
                    assert!(case["reason"].is_null(), "{name}");
                    assert_eq!(got, Ok(strings(&case["ordered"])), "{name}");
                } else {
                    assert_eq!(got, Err(str_of(case, "reason").to_owned()), "{name}");
                }
                validate += 1;
            }
            "normalize" => {
                let path = str_of(case, "path");
                let n = normalize_path(path);
                let c = canonicalize_folder(path);
                match case["normalized"].as_str() {
                    Some(e) => assert_eq!(n.as_deref(), Ok(e), "normalize {path:?}"),
                    None => check_throws(&case["normalized"], &n, path),
                }
                match case["canonical_folder"].as_str() {
                    Some(e) => assert_eq!(c.as_deref(), Ok(e), "canonical {path:?}"),
                    None => check_throws(&case["canonical_folder"], &c, path),
                }
                normalize += 1;
            }
            "same_path" => {
                let (a, b) = (str_of(case, "a"), str_of(case, "b"));
                let got = same_path(a, b);
                match case["same"].as_bool() {
                    Some(e) => assert_eq!(got, Ok(e), "same_path {a:?} {b:?}"),
                    None => check_throws(&case["same"], &got, a),
                }
                same += 1;
            }
            other => panic!("unknown kind {other}"),
        }
    }
    assert_eq!((validate, normalize, same), (69, 23, 13));
}

#[test]
fn extended_prefix_followed_by_astral_char_then_colon_is_unchanged() {
    // C# indexes UTF-16 units: full[5] is a low surrogate for an astral char at index 4, so no prefix strip.
    let p = "\\?\\u{1F600}:x";
    assert_eq!(normalize_path(p).as_deref(), Ok(p));
    assert_eq!(normalize_path("\\?\C:\a").as_deref(), Ok("C:\a"));
}
