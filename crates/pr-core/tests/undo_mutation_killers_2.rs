//! Mutation killers for `pr_core::file_actions::errors` and `service` (claim PARTIAL-mut-undo). Each test names the cargo-mutants
//! mutant it kills. Pure functions and fakes only, no real disk, no Recycle Bin.

use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::FileSystem;
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::errors::{UnknownJournalCode, get_path_root};
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::service::FileActionService;
use pr_test_support::{FakeClock, FakeFileSystem, FakeRecycleBin};

// Kills errors.rs:78:9 `<impl fmt::Display for UnknownJournalCode>::fmt` -> `Ok(Default::default())` (empty text).
#[test]
fn unknown_journal_code_display_names_the_code_in_the_english_text() {
    let error = UnknownJournalCode("bogus".to_owned());
    assert_eq!(error.to_string(), "Unknown journal error code 'bogus'.");
}

// Characterisation pin of the device branch of get_path_root (NOT a killer: the survivors errors.rs:310:14 `<` -> `<=`, 310:20
// and 310:29 `&&` -> `||`, 310:25 `>` -> `>=` and 315:16 `>` -> `>=` are equivalent. `sep_at(i)` already implies `i < len`, and
// including the separator directly after the device prefix gives root_len 5 whose normalisation drops the doubled separator, so the
// text is the same as for root_len 4; at 315 `len > 2` vs `len >= 2` only differs for len == 2, where `sep_at(2)` is false).
// .NET 10 `Path.GetPathRoot` of a device prefix + separator + name is the bare prefix (checked by hand with a console project).
#[test]
fn get_path_root_device_prefix_followed_by_a_separator_stops_after_the_prefix() {
    assert_eq!(get_path_root(r"\\?\\foo").as_deref(), Some(r"\\?\"));
    assert_eq!(get_path_root(r"\\.\\a").as_deref(), Some(r"\\.\"));
    // The same rule with a first component keeps the component and its separator (`i > 4` holds).
    assert_eq!(get_path_root(r"\\?\x\foo").as_deref(), Some(r"\\?\x\"));
    // Prefix only, and prefix + one separator.
    assert_eq!(get_path_root(r"\\?\").as_deref(), Some(r"\\?\"));
}

// Kills service.rs:183:9 `<impl fmt::Debug for FileActionService>::fmt` -> `Ok(Default::default())` (empty text).
#[test]
fn file_action_service_debug_names_the_type_and_reports_the_gate_and_override() {
    let disk = Arc::new(FakeFileSystem::new());
    let fs: Arc<dyn FileSystem> = disk.clone();
    let clock = Arc::new(FakeClock::new());
    let journal = Arc::new(OperationJournal::new(
        &AppPaths::new(Path::new(r"C:\data"), None, false),
        fs.clone(),
        clock.clone(),
    ));
    let service = FileActionService::new(journal, fs, clock, Arc::new(FakeRecycleBin::new(disk)));

    let text = format!("{service:?}");

    assert!(text.starts_with("FileActionService {"), "{text}");
    assert!(text.contains("move_override: false"), "{text}");
    assert!(text.contains("in_progress:"), "{text}");
}

// Pins service.rs:800 `if already || target == *source` in run_group (800:28 `||` -> `&&` and 800:38 `==` -> `!=` were already
// caught by the rename matrix; this test is a second, local pin). A member whose spelling in the
// request differs from the stored one but whose target name IS the stored name (`already` true, `target != source`) is skipped
// as already named; with `&&` it would be renamed again (a case-only Move) and become a second journaled member.
// C#: FileActionService.ExecuteGroup rename branch (`if (already || target == source)`).
#[test]
fn execute_group_rename_member_whose_request_spelling_differs_but_is_already_named_is_skipped() {
    use pr_core::catalog::types::CaptureGroup;
    use pr_core::file_actions::rename::PathKeyMap;
    use pr_core::file_actions::types::CaptureGroupActionRequest;
    use pr_core::model::FileOperationType;
    use pr_core::sync::CancelToken;

    let disk = Arc::new(FakeFileSystem::new());
    disk.add_file(r"C:\photos\A.jpg", b"jpeg");
    disk.add_file(r"C:\photos\a.cr2", b"raw");
    let fs: Arc<dyn FileSystem> = disk.clone();
    let clock = Arc::new(FakeClock::new());
    let journal = Arc::new(OperationJournal::new(
        &AppPaths::new(Path::new(r"C:\data"), None, false),
        fs.clone(),
        clock.clone(),
    ));
    let service = FileActionService::new(journal.clone(), fs, clock, Arc::new(FakeRecycleBin::new(disk.clone())));
    let group = CaptureGroup::new(r"C:\photos\a.jpg".to_owned(), r"C:\photos\a.cr2".to_owned(), None).unwrap();
    let mut names = PathKeyMap::new();
    names.insert(r"C:\photos\a.jpg".to_owned(), "A.jpg".to_owned()); // already stored as A.jpg
    names.insert(r"C:\photos\a.cr2".to_owned(), "B.cr2".to_owned());
    let request = CaptureGroupActionRequest {
        group,
        operation: FileOperationType::Move,
        destination: None,
        allow_permanent_delete: false,
        destination_file_names: Some(names),
    };

    let result = service.execute_group(&request, &CancelToken::new());

    assert!(result.succeeded, "{:?}", result.error);
    let mut members = 0;
    journal
        .read_entries(&mut |entry| members = members.max(entry.group_members.map_or(0, |m| m.len())))
        .expect("read journal");
    assert_eq!(members, 1, "only the cr2 is renamed; the jpg was already named");
    assert!(disk.file_exists(Path::new(r"C:\photos\B.cr2")));
    assert_eq!(
        disk.get_stored_file_names(Path::new(r"C:\photos\A.jpg")).unwrap(),
        ["A.jpg"]
    );
}

// Kills service.rs:790:21 `||` -> `&&` in run_group (`check_file_name(name) != None || get_file_name(name) != name`): a target name
// the rename policy rejects although it has no path part (`*` is an invalid file name character) must still fail the whole
// group before anything is journaled or moved.
// C#: FileActionService.ExecuteGroupAsync rename branch (`RenameNamePolicy.Check(name) != None || Path.GetFileName(name) != name`).
#[test]
fn execute_group_rename_with_an_invalid_name_without_a_path_part_is_refused_and_changes_nothing() {
    use pr_core::catalog::types::CaptureGroup;
    use pr_core::file_actions::rename::PathKeyMap;
    use pr_core::file_actions::types::CaptureGroupActionRequest;
    use pr_core::model::FileOperationType;
    use pr_core::sync::CancelToken;

    let disk = Arc::new(FakeFileSystem::new());
    disk.add_file(r"C:\photos.jpg", b"jpeg");
    disk.add_file(r"C:\photos.cr2", b"raw");
    let fs: Arc<dyn FileSystem> = disk.clone();
    let clock = Arc::new(FakeClock::new());
    let journal = Arc::new(OperationJournal::new(
        &AppPaths::new(Path::new(r"C:\data"), None, false),
        fs.clone(),
        clock.clone(),
    ));
    let service = FileActionService::new(journal.clone(), fs, clock, Arc::new(FakeRecycleBin::new(disk.clone())));
    let group = CaptureGroup::new(r"C:\photos.jpg".to_owned(), r"C:\photos.cr2".to_owned(), None).unwrap();
    let mut names = PathKeyMap::new();
    names.insert(r"C:\photos.jpg".to_owned(), "bad*name.jpg".to_owned());
    names.insert(r"C:\photos.cr2".to_owned(), "bad*name.cr2".to_owned());
    let request = CaptureGroupActionRequest {
        group,
        operation: FileOperationType::Move,
        destination: None,
        allow_permanent_delete: false,
        destination_file_names: Some(names),
    };

    let result = service.execute_group(&request, &CancelToken::new());

    assert!(!result.succeeded);
    let mut lines = 0;
    journal.read_entries(&mut |_| lines += 1).expect("read journal");
    assert_eq!(lines, 0, "nothing is journaled");
    assert!(disk.file_exists(Path::new(r"C:\photos.jpg")));
    assert!(disk.file_exists(Path::new(r"C:\photos.cr2")));
}
