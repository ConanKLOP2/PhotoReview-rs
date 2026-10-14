//! Tests for `pr_core::file_actions::errors`: port of `JournalErrorCatalogTests`, the `Describe` semantics of
//! `JournalErrorCodeTests`, `IsDestinationExists` from `PhysicalFileSystemTryCopyNewTests`, and the group Recycle
//! capacity rule from `CaptureGroupActionGapTests` (C# baseline e441d67a). Pure logic only: no real Recycle Bin and no
//! file-system access.

use std::collections::{HashMap, HashSet};
use std::io::{self, ErrorKind};
use std::path::Path;

use pr_core::abstractions::RecycleBin;
use pr_core::file_actions::errors::{
    ALL_CODES, CANCELLED_BY_USER, CODE_KEYS, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, JournalCodedError,
    UnknownJournalCode, VERIFY_SIZE_CHANGED, catalog_key, describe, first_overflow, for_journal, get_path_root,
    hresult_is_destination_exists, is_destination_exists, is_known, text,
};
use pr_core::file_actions::journal_line::JournalDateTime;
use pr_core::file_actions::types::JournalGroupMember;
use pr_core::sync::Canceled;
use pr_core::text::ignore_case_key;
use pr_core::time::DateTimeUtc;
use pr_test_support::golden;

/// The English catalog for the journal codes, built from the golden `journal-error-text` (its known cases are the
/// English texts). Stands in for `en.json` until `pr-l10n` exists.
fn english_catalog() -> HashMap<String, String> {
    let golden = golden::load("journal-error-text");
    let mut map = HashMap::new();
    for case in &golden.cases {
        if case["known"].as_bool() == Some(true) {
            let code = golden::str_of(case, "code");
            let key = catalog_key(code).expect("golden code is a known code");
            map.insert(key.to_owned(), golden::str_of(case, "english").to_owned());
        }
    }
    map
}

fn english_for(catalog: &HashMap<String, String>, key: &str) -> String {
    catalog
        .get(key)
        .cloned()
        .unwrap_or_else(|| panic!("no English text for key {key}"))
}

/// Fake UI catalog used by the tests that must show that the UI text (not the English text) reaches the error.
fn ui_key(key: &str) -> String {
    format!("UI:{key}")
}

fn member(source: &str, size: i64) -> JournalGroupMember {
    JournalGroupMember {
        source: source.to_owned(),
        destination: None,
        size,
        last_write_utc: JournalDateTime::utc(DateTimeUtc::default()),
        permanent: false,
    }
}

fn permanent_member(source: &str, size: i64) -> JournalGroupMember {
    JournalGroupMember {
        permanent: true,
        ..member(source, size)
    }
}

/// Fake bin with a capacity per volume. The volume key is the OrdinalIgnoreCase key of `get_path_root`, so `c:\` and
/// `C:\` are the same volume. C# `PerVolumeBin`: `FitsInRecycleBin` = `fileSize <= capacityByRoot[root]`.
struct PerVolumeBin {
    capacity: HashMap<String, u64>,
}

impl PerVolumeBin {
    fn new(capacities: &[(&str, u64)]) -> Self {
        let capacity = capacities
            .iter()
            .map(|&(root, cap)| (ignore_case_key(&get_path_root(root).unwrap_or_default()), cap))
            .collect();
        Self { capacity }
    }
}

impl RecycleBin for PerVolumeBin {
    fn send_to_recycle_bin(&self, _path: &Path) -> io::Result<()> {
        Err(io::Error::from(ErrorKind::Unsupported))
    }

    fn try_restore(
        &self,
        _original_path: &Path,
        _expected_size: u64,
        _expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        Ok(false)
    }

    fn fits_in_recycle_bin(&self, path: &Path, file_size: u64) -> bool {
        let text = path.to_string_lossy();
        let root = get_path_root(&text).unwrap_or_default();
        match self.capacity.get(&ignore_case_key(&root)) {
            Some(capacity) => file_size <= *capacity,
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// JournalErrorCatalogTests
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalErrorCatalogTests.AllCodes_IsNotEmpty
#[test]
fn all_codes_is_not_empty_unique_and_equal_to_code_keys() {
    assert_eq!(ALL_CODES.len(), 8);
    let mut seen = HashSet::new();
    for code in ALL_CODES {
        assert!(!code.is_empty(), "empty code in ALL_CODES");
        assert!(seen.insert(code), "duplicate code {code}");
    }
    let keys: Vec<&str> = CODE_KEYS.iter().map(|(code, _)| *code).collect();
    assert_eq!(keys, ALL_CODES.to_vec());
}

// C#: JournalErrorCatalogTests.Code_IsDescribedInBothLanguages (the Vietnamese half waits for pr-l10n)
#[test]
fn code_is_described_in_english_with_text_that_does_not_repeat_the_code() {
    let catalog = english_catalog();
    let english = |key: &str| english_for(&catalog, key);
    for code in ALL_CODES {
        assert!(is_known(Some(code)), "{code} must be known");
        let english_text = text(code, &english).expect("known code has text");
        assert!(!english_text.trim().is_empty(), "{code} has blank English text");
        assert!(!english_text.contains(code), "{code} English text repeats the code");
    }
}

// C#: JournalErrorCatalogTests.CodedException_IsJournaledInEnglish_UiShowsVietnamese
#[test]
fn coded_exception_is_journaled_in_english_and_shows_ui_text() {
    let catalog = english_catalog();
    let english = |key: &str| english_for(&catalog, key);
    for code in ALL_CODES {
        let ui_text = text(code, &ui_key).expect("known code has text");
        let err = JournalCodedError::new(code, ui_text.clone());
        assert_eq!(err.to_string(), ui_text, "UI text is the exception message");
        let (journal_code, journal_text) = for_journal(&err, &english);
        assert_eq!(journal_code, Some(code));
        assert_eq!(journal_text, text(code, &english).expect("known code has text"));
    }
}

// C#: JournalErrorCatalogTests.Cancellation_IsJournaledWithTheCancelledByUserCode
#[test]
fn cancellation_is_journaled_with_the_cancelled_by_user_code() {
    let catalog = english_catalog();
    let english = |key: &str| english_for(&catalog, key);
    let expected_text = text(CANCELLED_BY_USER, &english).expect("known code has text");

    assert_eq!(
        for_journal(&Canceled, &english),
        (Some(CANCELLED_BY_USER), expected_text.clone())
    );
    let wrapped = io::Error::other(Canceled);
    assert_eq!(
        for_journal(&wrapped, &english),
        (Some(CANCELLED_BY_USER), expected_text)
    );
}

// C#: JournalErrorCatalogTests.NonCodedFailures_KeepTheirMessage_WithoutCode
#[test]
fn non_coded_failures_keep_their_message_without_code() {
    let catalog = english_catalog();
    let english = |key: &str| english_for(&catalog, key);

    let no_space = io::Error::other("There is not enough space on the disk.");
    assert_eq!(
        for_journal(&no_space, &english),
        (None, "There is not enough space on the disk.".to_owned())
    );

    let denied = io::Error::new(ErrorKind::PermissionDenied, "Access to the path is denied.");
    assert_eq!(
        for_journal(&denied, &english),
        (None, "Access to the path is denied.".to_owned())
    );

    let os_error = io::Error::from_raw_os_error(5);
    assert_eq!(for_journal(&os_error, &english), (None, os_error.to_string()));
}

// C#: JournalErrorCatalogTests (ForJournal order: a coded error with an UNKNOWN code falls through to the message)
#[test]
fn coded_error_with_unknown_code_falls_through_and_keeps_its_message() {
    let catalog = english_catalog();
    let english = |key: &str| english_for(&catalog, key);
    let err = JournalCodedError::new("FutureCode", "Something from a newer build.");
    assert_eq!(
        for_journal(&err, &english),
        (None, "Something from a newer build.".to_owned())
    );
}

// C#: JournalErrorCatalogTests (ForJournal look-through: a known coded error wrapped in an io::Error)
#[test]
fn known_coded_error_wrapped_in_io_error_is_journaled_by_code() {
    let catalog = english_catalog();
    let english = |key: &str| english_for(&catalog, key);
    let wrapped = JournalCodedError::new(VERIFY_SIZE_CHANGED, "UI text").into_io_error();
    assert_eq!(
        for_journal(&wrapped, &english),
        (
            Some(VERIFY_SIZE_CHANGED),
            text(VERIFY_SIZE_CHANGED, &english).expect("known code has text")
        )
    );
}

// C#: JournalCodedException(string message, Exception innerException) (code is empty, inner is kept as source)
#[test]
fn with_inner_has_empty_code_and_keeps_the_inner_error_as_source() {
    let catalog = english_catalog();
    let english = |key: &str| english_for(&catalog, key);
    let err = JournalCodedError::with_inner("outer message", Box::new(io::Error::other("inner message")));
    assert!(err.code.is_empty());
    assert_eq!(err.to_string(), "outer message");
    let source = std::error::Error::source(&err).expect("inner error is the source");
    assert_eq!(source.to_string(), "inner message");
    assert_eq!(for_journal(&err, &english), (None, "outer message".to_owned()));
}

// C#: JournalCodedException (into_io_error is an IOException of kind Other whose inner value is recoverable)
#[test]
fn into_io_error_is_other_kind_and_keeps_the_coded_error_recoverable() {
    let io_err = JournalCodedError::new(CANCELLED_BY_USER, "cancelled text").into_io_error();
    assert_eq!(io_err.kind(), ErrorKind::Other);
    assert_eq!(io_err.raw_os_error(), None);
    let inner = io_err
        .get_ref()
        .and_then(|e| e.downcast_ref::<JournalCodedError>())
        .expect("coded error is recoverable from the io::Error");
    assert_eq!(inner.code, CANCELLED_BY_USER);
    assert_eq!(inner.message, "cancelled text");
}

// C#: JournalErrorCatalogTests (Text: unknown code is an ArgumentException in C#; here UnknownJournalCode)
#[test]
fn text_of_unknown_code_is_an_error_naming_the_code() {
    let catalog = english_catalog();
    let english = |key: &str| english_for(&catalog, key);
    assert_eq!(text("Nope", &english), Err(UnknownJournalCode("Nope".to_owned())));
    assert!(text("cancelledbyuser", &english).is_err());
}

// ---------------------------------------------------------------------------------------------------------------
// JournalErrorCodeTests: Describe semantics only
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalErrorCodeTests.OldRecordWithoutCode_Loads_AndDescribeShowsStoredText
#[test]
fn describe_old_record_without_code_shows_stored_text() {
    assert_eq!(describe(None, Some("old"), &ui_key), Some("old".to_owned()));
    assert_eq!(describe(None, None, &ui_key), None);
}

// C#: JournalErrorCodeTests.UnknownCode_FromNewerBuild_ShowsStoredText
#[test]
fn describe_unknown_code_from_newer_build_shows_stored_text() {
    assert_eq!(
        describe(Some("FutureCode"), Some("stored"), &ui_key),
        Some("stored".to_owned())
    );
}

// C#: JournalErrorCodeTests (Describe semantics for a known code: localized text, stored text ignored)
#[test]
fn describe_known_code_shows_the_localized_text() {
    for (code, key) in CODE_KEYS {
        assert_eq!(describe(Some(code), Some("stored"), &ui_key), Some(ui_key(key)));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// PhysicalFileSystemTryCopyNewTests: IsDestinationExists
// ---------------------------------------------------------------------------------------------------------------

// C#: PhysicalFileSystemTryCopyNewTests.IsDestinationExists_OnlyWin32FileExistsAndAlreadyExists_AreRecognized
#[test]
fn is_destination_exists_only_win32_file_exists_and_already_exists_are_recognized() {
    let rows: [(i32, bool); 8] = [
        (0x8007_0050_u32 as i32, true),
        (0x8007_00B7_u32 as i32, true),
        (0x8007_0005_u32 as i32, false),
        (0x8007_0020_u32 as i32, false),
        (0x8007_0070_u32 as i32, false),
        (0x0000_0050_u32 as i32, false),
        (0x800B_0050_u32 as i32, false),
        (0x0007_00B7_u32 as i32, true),
    ];
    for (hresult, expected) in rows {
        assert_eq!(
            hresult_is_destination_exists(hresult),
            expected,
            "HRESULT 0x{:08X}",
            hresult as u32
        );
    }
}

#[test]
fn is_destination_exists_recognizes_raw_os_codes_80_and_183() {
    assert_eq!(ERROR_FILE_EXISTS, 80);
    assert_eq!(ERROR_ALREADY_EXISTS, 183);
    assert!(is_destination_exists(&io::Error::from_raw_os_error(80)));
    assert!(is_destination_exists(&io::Error::from_raw_os_error(183)));
}

#[test]
fn is_destination_exists_rejects_access_sharing_and_disk_full_codes() {
    for raw in [5, 32, 112] {
        assert!(!is_destination_exists(&io::Error::from_raw_os_error(raw)), "raw {raw}");
    }
}

#[test]
fn is_destination_exists_recognizes_already_exists_kind_without_os_code() {
    assert!(is_destination_exists(&io::Error::from(ErrorKind::AlreadyExists)));
}

#[test]
fn is_destination_exists_rejects_other_kind_without_os_code() {
    assert!(!is_destination_exists(&io::Error::from(ErrorKind::Other)));
}

// ---------------------------------------------------------------------------------------------------------------
// get_path_root: golden `path-root`
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn golden_path_root_matches_every_case() {
    let golden = golden::load("path-root");
    for case in &golden.cases {
        // A JSON null path is C# null; `""` gives the same `None` result in Rust.
        let path = case["path"].as_str().unwrap_or("");
        let expected = case["root"].as_str();
        assert_eq!(
            get_path_root(path).as_deref(),
            expected,
            "get_path_root({:?}) (case {case})",
            path
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Golden `journal-error-text`
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn golden_journal_error_text_matches_every_case() {
    let golden = golden::load("journal-error-text");
    let catalog = english_catalog();
    let english = |key: &str| english_for(&catalog, key);
    for case in &golden.cases {
        let code = case["code"].as_str();
        let known = golden::bool_of(case, "known");
        assert_eq!(is_known(code), known, "is_known({code:?}) (case {case})");
        match code {
            Some(code) if known => {
                assert_eq!(
                    text(code, &english).expect("known code has text"),
                    golden::str_of(case, "english"),
                    "English text of {code}"
                );
            }
            Some(code) => {
                assert!(
                    text(code, &english).is_err(),
                    "unknown code {code:?} must not have text"
                );
                assert_eq!(
                    describe(Some(code), Some("stored"), &english),
                    Some("stored".to_owned()),
                    "describe of unknown code {code:?}"
                );
            }
            None => {
                assert_eq!(
                    describe(None, Some("stored"), &english),
                    Some("stored".to_owned()),
                    "describe of missing code"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// get_path_root: own cases
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn get_path_root_empty_or_blank_is_none_and_relative_is_empty_root() {
    assert_eq!(get_path_root(""), None);
    assert_eq!(get_path_root("   "), None);
    assert_eq!(get_path_root("a\\b"), Some(String::new()));
}

// ---------------------------------------------------------------------------------------------------------------
// RecycleBinCapacity.FirstOverflow: C# CaptureGroupActionGapTests.ExecuteGroupAsync_RecycleBin* scenarios
// ---------------------------------------------------------------------------------------------------------------

// C#: CaptureGroupActionGapTests.ExecuteGroupAsync_RecycleBinFitsEachVolumeButNotTheSumAcrossVolumes_Succeeds
#[test]
fn first_overflow_each_volume_fits_on_its_own_and_together_returns_none() {
    let bin = PerVolumeBin::new(&[("C:\\", 10), ("D:\\", 10)]);
    let members = [
        member("C:\\photos\\a.jpg", 4),
        member("C:\\photos\\a.xmp", 3),
        member("D:\\photos\\a.cr2", 8),
    ];
    assert_eq!(first_overflow(&bin, &members), None);
}

// C#: CaptureGroupActionGapTests.ExecuteGroupAsync_RecycleBinOverflowsOnTheSecondVolume_IsRefusedBeforeAnyMutation
#[test]
fn first_overflow_second_volume_overflows_returns_its_first_member() {
    let bin = PerVolumeBin::new(&[("C:\\", 100), ("D:\\", 5)]);
    let members = [member("C:\\photos\\a.jpg", 4), member("D:\\photos\\a.cr2", 8)];
    assert_eq!(
        first_overflow(&bin, &members).map(|m| m.source.as_str()),
        Some("D:\\photos\\a.cr2")
    );
}

// C#: CaptureGroupActionGapTests.ExecuteGroupAsync_RecycleBinOverflowsOnTheFirstVolumeOnly_IsRefusedBeforeAnyMutation
#[test]
fn first_overflow_first_volume_overflows_returns_first_member_of_that_volume() {
    let bin = PerVolumeBin::new(&[("C:\\", 6), ("D:\\", 100)]);
    let members = [
        member("C:\\photos\\a.jpg", 4),
        member("D:\\photos\\a.cr2", 8),
        member("C:\\photos\\a.xmp", 3),
    ];
    assert_eq!(
        first_overflow(&bin, &members).map(|m| m.source.as_str()),
        Some("C:\\photos\\a.jpg")
    );
}

#[test]
fn first_overflow_returns_first_member_of_the_group_not_the_biggest() {
    let bin = PerVolumeBin::new(&[("C:\\", 6)]);
    let members = [member("C:\\photos\\a.xmp", 3), member("C:\\photos\\a.jpg", 4)];
    assert_eq!(
        first_overflow(&bin, &members).map(|m| m.source.as_str()),
        Some("C:\\photos\\a.xmp")
    );
}

#[test]
fn first_overflow_permanent_members_are_ignored() {
    let bin = PerVolumeBin::new(&[("C:\\", 5), ("D:\\", 5)]);
    let members = [
        permanent_member("C:\\photos\\big.bin", 100),
        member("C:\\photos\\a.jpg", 4),
        permanent_member("D:\\photos\\huge.bin", 500),
    ];
    assert_eq!(first_overflow(&bin, &members), None);
}

#[test]
fn first_overflow_volume_roots_compare_ignoring_case() {
    let bin = PerVolumeBin::new(&[("C:\\", 7)]);
    let members = [member("c:\\x", 4), member("C:\\y", 4)];
    assert_eq!(first_overflow(&bin, &members).map(|m| m.source.as_str()), Some("c:\\x"));
}

#[test]
fn first_overflow_when_several_volumes_overflow_the_first_appearing_one_wins() {
    let bin = PerVolumeBin::new(&[("C:\\", 5), ("D:\\", 5)]);
    let members = [member("D:\\a", 8), member("C:\\b", 8)];
    assert_eq!(first_overflow(&bin, &members).map(|m| m.source.as_str()), Some("D:\\a"));
}

#[test]
fn first_overflow_empty_slice_is_none() {
    let bin = PerVolumeBin::new(&[]);
    assert_eq!(first_overflow(&bin, &[]), None);
}

#[test]
fn get_path_root_explicit_nt_device_prefix_requires_backslashes_at_0_and_3() {
    // C# IsExtended: path[0] and path[3] must be backslashes (forward slashes do not make a NT device prefix).
    assert_eq!(get_path_root(r"\??/").as_deref(), Some(r"\"));
    assert_eq!(get_path_root(r"/??\").as_deref(), Some(r"\"));
    assert_eq!(get_path_root("/??/").as_deref(), Some(r"\"));
    assert_eq!(get_path_root(r"\??/a").as_deref(), Some(r"\"));
    assert_eq!(get_path_root("/??/C:/a").as_deref(), Some(r"\"));
    assert_eq!(get_path_root(r"\??\C:\a").as_deref(), Some(r"\??\C:\"));
}

/// Bin that records every size it is asked about and rejects totals above `limit`.
struct RecordingBin {
    limit: u64,
    asked: std::sync::Mutex<Vec<u64>>,
}

impl RecordingBin {
    fn asked(&self) -> Vec<u64> {
        self.asked.lock().map(|a| a.clone()).unwrap_or_default()
    }
}

impl RecycleBin for RecordingBin {
    fn send_to_recycle_bin(&self, _path: &Path) -> io::Result<()> {
        Err(io::Error::from(ErrorKind::Unsupported))
    }

    fn try_restore(
        &self,
        _original_path: &Path,
        _expected_size: u64,
        _expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        Ok(false)
    }

    fn fits_in_recycle_bin(&self, _path: &Path, file_size: u64) -> bool {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(file_size);
        }
        file_size <= self.limit
    }
}

#[test]
fn first_overflow_size_sums_saturate_instead_of_wrapping() {
    let bin = RecordingBin {
        limit: 1000,
        asked: std::sync::Mutex::new(Vec::new()),
    };
    let members = [member(r"C:\a", i64::MAX), member(r"C:\b", i64::MAX)];
    assert_eq!(first_overflow(&bin, &members).map(|m| m.source.as_str()), Some(r"C:\a"));
    assert_eq!(bin.asked(), vec![i64::MAX as u64]);
}

#[test]
fn first_overflow_negative_size_sum_is_asked_as_zero() {
    let bin = RecordingBin {
        limit: u64::MAX,
        asked: std::sync::Mutex::new(Vec::new()),
    };
    let members = [member(r"C:\a", -1)];
    assert_eq!(first_overflow(&bin, &members), None);
    assert_eq!(bin.asked(), vec![0]);
}
