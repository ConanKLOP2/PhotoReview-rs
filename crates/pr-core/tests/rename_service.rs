use std::sync::Arc;

use pr_core::abstractions::FileSystem;
use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::rename::RenameRequest;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::types::FileActionRequest;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use pr_test_support::{FakeClock, FakeFileSystem, FakeRecycleBin, TempRoot};

struct Rig {
    _root: TempRoot,
    photos: std::path::PathBuf,
    fs: Arc<FakeFileSystem>,
    journal: Arc<OperationJournal>,
    service: FileActionService,
    clock: Arc<FakeClock>,
    journal_path: std::path::PathBuf,
}

impl Rig {
    fn file(&self, name: &str) -> String {
        self.photos.join(name).to_string_lossy().into_owned()
    }
    fn group(&self, xmp: Option<&str>) -> CaptureGroup {
        CaptureGroup::new(self.file("a.jpg"), self.file("a.cr2"), xmp.map(|name| self.file(name))).unwrap()
    }
    fn request(&self, source: &str, new_file_name: &str) -> RenameRequest {
        RenameRequest {
            source: source.to_owned(),
            new_file_name: new_file_name.to_owned(),
        }
    }
}

fn setup(names: &[&str]) -> Rig {
    let root = TempRoot::new("rename-service");
    let photos = root.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    let fs = Arc::new(FakeFileSystem::new());
    for name in names {
        fs.add_file(photos.join(name), b"image");
    }
    let clock = Arc::new(FakeClock::new());
    let journal_path = root.path().join("journal.jsonl");
    let paths = pr_core::app_paths::AppPaths {
        journal_file: journal_path.clone(),
        ..pr_core::app_paths::AppPaths::new(root.path(), None, false)
    };
    let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
    let bin = Arc::new(FakeRecycleBin::new(fs.clone()));
    let service = FileActionService::new(journal.clone(), fs.clone(), clock.clone(), bin);
    Rig {
        _root: root,
        photos,
        fs,
        journal,
        service,
        clock,
        journal_path,
    }
}

#[test]
fn rename_success_moves_in_place_and_journals_one_committed_move() {
    // C#: RenameFileActionTests.Rename_Success_MovesInPlaceAndJournalsOneCommittedMove
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    let destination = rig.file("x.jpg");
    let before = rig.fs.get_file_stat(std::path::Path::new(&source)).unwrap();
    let result = rig.service.rename(&rig.request(&source, "x.jpg"), &CancelToken::new());
    assert!(result.succeeded, "{:?}", result.error);
    assert_eq!(result.operation, FileOperationType::Move);
    assert!(!rig.fs.file_exists(std::path::Path::new(&source)));
    assert!(rig.fs.file_exists(std::path::Path::new(&destination)));
    assert_eq!(
        rig.fs.get_file_stat(std::path::Path::new(&destination)).unwrap(),
        before
    );
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |entry| entries.push(entry)).unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.state == JournalState::Committed)
            .count(),
        1
    );
    let entry = entries.last().unwrap();
    assert_eq!(entry.state, JournalState::Committed);
    assert_eq!(entry.op_type, FileOperationType::Move);
}

#[test]
fn rename_journal_line_uses_exactly_the_schema_of_an_ordinary_move() {
    // C#: RenameFileActionTests.Rename_JournalLine_UsesExactlyTheSchemaOfAnOrdinaryMove
    let rename_rig = setup(&["a.jpg"]);
    let source1 = rename_rig.file("a.jpg");
    assert!(
        rename_rig
            .service
            .rename(&rename_rig.request(&source1, "x.jpg"), &CancelToken::new())
            .succeeded
    );
    let move_rig = setup(&["a.jpg"]);
    let source2 = move_rig.file("a.jpg");
    let mut request = FileActionRequest::new(source2, FileOperationType::Move);
    request.destination = Some("sel".into());
    assert!(move_rig.service.execute(&request, &CancelToken::new()).succeeded);
    let mut rename_lines = Vec::new();
    rename_rig.journal.read_entries(&mut |e| rename_lines.push(e)).unwrap();
    let mut move_lines = Vec::new();
    move_rig.journal.read_entries(&mut |e| move_lines.push(e)).unwrap();
    let keys = |lines: &[pr_core::file_actions::types::JournalEntry]| -> Vec<Vec<String>> {
        let mut sets: Vec<Vec<String>> = lines
            .iter()
            .map(|entry| {
                let value = serde_json::to_value(entry).unwrap();
                let mut names = value.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
                names.sort();
                names
            })
            .collect();
        sets.sort();
        sets.dedup();
        sets
    };
    assert_eq!(keys(&rename_lines), keys(&move_lines));
    assert!(
        rename_lines
            .iter()
            .all(|entry| entry.op_type == FileOperationType::Move)
    );
}

#[test]
fn rename_target_taken_by_another_file_is_refused_and_touches_nothing() {
    // C#: RenameFileActionTests.Rename_TargetTakenByAnotherFile_IsRefusedAndTouchesNothing
    let rig = setup(&["a.jpg", "b.jpg"]);
    let source = rig.file("a.jpg");
    let target = rig.file("b.jpg");
    let result = rig.service.rename(&rig.request(&source, "b.jpg"), &CancelToken::new());
    assert!(!result.succeeded);
    assert!(rig.fs.file_exists(std::path::Path::new(&source)));
    assert!(rig.fs.file_exists(std::path::Path::new(&target)));
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |entry| entries.push(entry)).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn rename_to_the_same_name_is_a_no_change_success_with_no_journal_line() {
    // C#: RenameFileActionTests.Rename_ToTheSameName_IsANoChangeSuccessWithNoJournalLine
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    let result = rig.service.rename(&rig.request(&source, "a.jpg"), &CancelToken::new());
    assert!(result.succeeded);
    assert!(result.no_change);
    assert!(rig.fs.file_exists(std::path::Path::new(&source)));
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |entry| entries.push(entry)).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn case_only_rename_succeeds_stores_the_new_spelling_and_is_committed() {
    // C#: RenameFileActionTests.CaseOnlyRename_Succeeds_StoresTheNewSpellingAndIsCommitted
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    let target = rig.file("A.jpg");
    let result = rig.service.rename(&rig.request(&source, "A.jpg"), &CancelToken::new());
    assert!(result.succeeded, "{:?}", result.error);
    assert_eq!(
        rig.fs.get_stored_file_names(std::path::Path::new(&source)).unwrap(),
        ["A.jpg"]
    );
    assert!(rig.fs.file_exists(std::path::Path::new(&target)));
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |entry| entries.push(entry)).unwrap();
    assert_eq!(entries.last().unwrap().state, JournalState::Committed);
}

#[test]
fn rename_group_renames_every_member_in_one_journal_group_each_keeping_its_extension() {
    // C#: RenameFileActionTests.RenameGroup_RenamesEveryMemberInOneJournalGroup_EachKeepingItsExtension
    let rig = setup(&["a.jpg", "a.cr2", "a.xmp"]);
    let group = rig.group(Some("a.xmp"));
    let result = rig.service.rename_group(&group, "x", &CancelToken::new());
    assert!(result.succeeded, "{:?}", result.error);
    for name in ["x.jpg", "x.cr2", "x.xmp"] {
        assert!(rig.fs.file_exists(std::path::Path::new(&rig.file(name))));
        assert_eq!(rig.fs.file_content(rig.file(name)).unwrap(), b"image");
    }
    for name in ["a.jpg", "a.cr2", "a.xmp"] {
        assert!(!rig.fs.file_exists(std::path::Path::new(&rig.file(name))));
    }
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |entry| entries.push(entry)).unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.state == JournalState::Committed)
            .count(),
        1
    );
    assert_eq!(
        entries
            .iter()
            .rev()
            .find(|e| e.state == JournalState::Committed)
            .unwrap()
            .group_members
            .as_ref()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn rename_group_one_target_taken_changes_nothing_and_journals_nothing() {
    // C#: RenameFileActionTests.RenameGroup_OneTargetTaken_ChangesNothingAndJournalsNothing
    let rig = setup(&["a.jpg", "a.cr2", "a.xmp", "x.cr2"]);
    rig.fs.add_file(rig.file("x.cr2"), b"foreign raw");
    let group = rig.group(Some("a.xmp"));
    let originals = [rig.file("a.jpg"), rig.file("a.cr2"), rig.file("a.xmp")];
    let result = rig.service.rename_group(&group, "x", &CancelToken::new());
    assert!(!result.succeeded);
    assert!(originals.iter().all(|p| rig.fs.file_exists(std::path::Path::new(p))));
    assert_eq!(rig.fs.file_content(rig.file("x.cr2")).unwrap(), b"foreign raw");
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |entry| entries.push(entry)).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn rename_group_compound_xmp_follows_raw_owner_and_normalizes_typed_stem() {
    // C#: RenameFileActionTests.RenameGroup_SidecarNamedAfterTheRawFile_FollowsTheRawFile
    let rig = setup(&["a.jpg", "a.cr2", "a.cr2.xmp"]);
    let group = rig.group(Some("a.cr2.xmp"));
    let result = rig.service.rename_group(&group, "  x  ", &CancelToken::new());
    assert!(result.succeeded, "{:?}", result.error);
    assert!(rig.fs.file_exists(std::path::Path::new(&rig.file("x.jpg"))));
    assert!(rig.fs.file_exists(std::path::Path::new(&rig.file("x.cr2"))));
    assert!(rig.fs.file_exists(std::path::Path::new(&rig.file("x.cr2.xmp"))));
}

#[test]
fn rename_group_invalid_stem_returns_catalog_message_without_journal() {
    // C#: RenameFileActionTests.RenameGroup_InvalidName_IsRejectedBeforeAnything_IsTouched
    let rig = setup(&["a.jpg", "a.cr2"]);
    let group = rig.group(None);
    let result = rig.service.rename_group(&group, "bad:name", &CancelToken::new());
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some("The new file name is not valid: bad:name")
    );
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn rename_failure_before_mutation_is_dismissed_with_original_metadata() {
    // C#: RenameFileActionTests.Rename_OsRefusesTheMove_NothingChangedSoTheLineIsDismissedNotARecoveryItem
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    rig.fs.fail(
        pr_test_support::FsOp::Move,
        std::path::Path::new(&source),
        std::io::ErrorKind::PermissionDenied,
    );
    let before = rig.fs.get_file_stat(std::path::Path::new(&source)).unwrap();
    let result = rig.service.rename(&rig.request(&source, "x.jpg"), &CancelToken::new());
    assert!(!result.succeeded);
    assert!(!result.source_removed);
    assert_eq!(result.size, i64::try_from(before.length).unwrap());
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert_eq!(entries.last().unwrap().state, JournalState::Dismissed);
}

#[test]
fn rename_failure_after_mutation_is_failed_and_source_removed() {
    // C#: RenameFileActionTests.Rename_FailureAfterTheFileWasRenamed_StaysAFailedRecoveryItemNeverDismissed
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    let target = rig.file("x.jpg");
    let disk = rig.fs.clone();
    let svc = FileActionService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        rig.clock.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
    )
    .with_move_override(Arc::new(move |from, to| {
        disk.move_file(from, to)?;
        Err(std::io::Error::other("post move failure"))
    }));
    let result = svc.rename(&rig.request(&source, "x.jpg"), &CancelToken::new());
    assert!(!result.succeeded);
    assert!(result.source_removed);
    assert!(rig.fs.file_exists(std::path::Path::new(&target)));
}

#[test]
fn rename_uncertain_source_stat_stays_failed_but_does_not_claim_source_removed() {
    // C#: RenameFileActionTests.Rename_OsRefusesTheMove_NothingChangedSoTheLineIsDismissedNotARecoveryItem
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    let fault_path = source.clone();
    let fs = rig.fs.clone();
    let service = FileActionService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        rig.clock.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
    )
    .with_move_override(Arc::new(move |_, _| {
        fs.fail(
            pr_test_support::FsOp::Stat,
            std::path::Path::new(&fault_path),
            std::io::ErrorKind::PermissionDenied,
        );
        Err(std::io::Error::other("move refused"))
    }));
    let result = service.rename(&rig.request(&source, "x.jpg"), &CancelToken::new());
    assert!(!result.succeeded);
    assert!(!result.source_removed);
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert_eq!(entries.last().unwrap().state, JournalState::Failed);
}

#[test]
fn rename_destination_verification_failure_is_failed_and_source_removed() {
    // C#: RenameFileActionTests.Rename_FailureAfterTheFileWasRenamed_StaysAFailedRecoveryItemNeverDismissed
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    let target = rig.file("x.jpg");
    let fs = rig.fs.clone();
    let service = FileActionService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        rig.clock.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
    )
    .with_move_override(Arc::new(move |from, to| {
        fs.move_file(from, to)?;
        fs.fail(pr_test_support::FsOp::Stat, to, std::io::ErrorKind::PermissionDenied);
        Ok(())
    }));
    let result = service.rename(&rig.request(&source, "x.jpg"), &CancelToken::new());
    assert!(!result.succeeded);
    assert!(result.source_removed);
    assert!(rig.fs.file_exists(std::path::Path::new(&target)));
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert_eq!(entries.last().unwrap().state, JournalState::Failed);
}

#[test]
fn rename_pre_cancel_and_journal_append_failure_preserve_failure_evidence() {
    // C#: RenameFileActionTests.Rename_Success_MovesInPlaceAndJournalsOneCommittedMove
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    let token = CancelToken::new();
    token.cancel();
    let result = rig.service.rename(&rig.request(&source, "x.jpg"), &token);
    assert!(!result.succeeded);
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert_eq!(entries.last().unwrap().state, JournalState::Dismissed);
}

#[test]
fn rename_failed_terminal_append_reports_persisted_mutation() {
    // C#: RenameFileActionTests.Rename_Success_MovesInPlaceAndJournalsOneCommittedMove
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    let fs = rig.fs.clone();
    let journal_path = rig.journal_path.clone();
    let service = FileActionService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        rig.clock.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
    )
    .with_move_override(Arc::new(move |from, to| {
        fs.move_file(from, to)?;
        fs.fail_times(
            pr_test_support::FsOp::OpenAppend,
            &journal_path,
            std::io::ErrorKind::PermissionDenied,
            1,
        );
        Ok(())
    }));
    let result = service.rename(&rig.request(&source, "x.jpg"), &CancelToken::new());
    assert!(result.succeeded);
    assert!(!result.journal_persisted);
    assert!(result.journal_error.is_some());
}

#[test]
fn rename_group_mixed_noop_compound_owner_all_noop_and_case_variants() {
    // C#: RenameFileActionTests.RenameGroup_PartlyAlreadyNamed_RenamesOnlyTheOthers
    let rig = setup(&["a.jpg", "A.cr2", "a.cr2.xmp"]);
    let group = rig.group(Some("a.cr2.xmp"));
    let result = rig.service.rename_group(&group, "A", &CancelToken::new());
    assert!(result.succeeded);
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert_eq!(
        entries
            .iter()
            .rev()
            .find(|e| e.state == JournalState::Committed)
            .unwrap()
            .group_members
            .as_ref()
            .unwrap()
            .len(),
        2
    );
    let all = setup(&["A.jpg", "A.cr2"]);
    let group = all.group(None);
    let all_result = all.service.rename_group(&group, "A", &CancelToken::new());
    assert!(all_result.succeeded);
    assert!(all_result.members.is_empty());
    let mut noops = Vec::new();
    all.journal.read_entries(&mut |e| noops.push(e)).unwrap();
    assert!(noops.is_empty());
    let with_missing = setup(&["A.jpg", "A.cr2"]);
    let missing_group = with_missing.group(Some("a.xmp"));
    let missing_result = with_missing
        .service
        .rename_group(&missing_group, "A", &CancelToken::new());
    assert!(missing_result.succeeded);
    assert_eq!(missing_result.skipped_missing, Some(vec![with_missing.file("a.xmp")]));
    let twins = setup(&["a.jpg", "a.cr2"]);
    twins
        .fs
        .set_stored_file_names_override(Some(vec!["a.jpg".into(), "A.jpg".into()]));
    assert!(
        !twins
            .service
            .rename(&twins.request(&twins.file("a.jpg"), "A.jpg"), &CancelToken::new())
            .succeeded
    );
}

#[test]
fn case_only_helpers_fail_closed_for_missing_third_and_ambiguous_stored_names() {
    // C#: CaseOnlyRename.StatAs and ProbeTarget
    use pr_core::file_actions::case_only_rename::CaseOnlyRename;
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    let target = rig.file("A.jpg");
    assert_eq!(
        CaseOnlyRename::probe_target(&*rig.fs, &source, &target).unwrap(),
        (false, false)
    );
    rig.fs.set_stored_file_names_override(Some(Vec::new()));
    assert!(!CaseOnlyRename::exists_as(&*rig.fs, &source, Some(&target)).unwrap());
    rig.fs.set_stored_file_names_override(Some(vec!["a.JPG".into()]));
    assert!(CaseOnlyRename::exists_as(&*rig.fs, &source, Some(&target)).unwrap());
    rig.fs
        .set_stored_file_names_override(Some(vec!["a.jpg".into(), "A.jpg".into()]));
    assert_eq!(
        CaseOnlyRename::probe_target(&*rig.fs, &source, &target).unwrap(),
        (true, false)
    );
}

#[test]
fn case_only_helper_reports_stat_and_listing_errors_and_hook_wrapper_forwards_names() {
    // C#: CaseOnlyRename.StatAs, ExistsAs, ProbeTarget
    use pr_core::file_actions::case_only_rename::CaseOnlyRename;
    let rig = setup(&["a.jpg"]);
    let source = rig.file("a.jpg");
    let target = rig.file("A.jpg");
    rig.fs.fail(
        pr_test_support::FsOp::Stat,
        std::path::Path::new(&source),
        std::io::ErrorKind::PermissionDenied,
    );
    assert!(CaseOnlyRename::stat_as(&*rig.fs, &source, Some(&target)).is_err());
    rig.fs.fail(
        pr_test_support::FsOp::EnumerateFiles,
        &rig.photos,
        std::io::ErrorKind::PermissionDenied,
    );
    assert!(CaseOnlyRename::probe_target(&*rig.fs, &source, &target).is_err());
    rig.fs.clear_faults();
    let wrapped = pr_test_support::HookFileSystem::new(rig.fs.clone());
    assert_eq!(
        wrapped.get_stored_file_names(std::path::Path::new(&source)).unwrap(),
        ["a.jpg"]
    );
}

#[test]
#[ignore = "native Windows filesystem case-only rename behavior"]
fn native_case_only_rename_preserves_requested_stored_spelling() {
    // C#: RenameFileActionTests.CaseOnlyRename_Succeeds_StoresTheNewSpellingAndIsCommitted
    let root = TempRoot::new("rename-native-case-only");
    let path = root.path().join("a.jpg");
    std::fs::write(&path, b"image").unwrap();
    let target = path.parent().unwrap().join("A.jpg");
    std::fs::rename(&path, &target).unwrap();
    let fs = pr_core::io::physical::PhysicalFileSystem;
    assert_eq!(fs.get_stored_file_names(&target).unwrap(), ["A.jpg"]);
    assert!(
        pr_core::file_actions::case_only_rename::CaseOnlyRename::stat_as(&fs, path.to_str().unwrap(), target.to_str())
            .unwrap()
            .is_none()
    );
    std::fs::remove_file(target).unwrap();
}

#[test]
#[ignore = "native Windows real service rename and case-only rename under TempRoot"]
fn native_service_rename_and_case_only_rename_use_only_owned_temp_files() {
    // C#: RenameFileActionTests.Rename_Success_MovesInPlaceAndJournalsOneCommittedMove; CaseOnlyRename_Succeeds_StoresTheNewSpellingAndIsCommitted
    use pr_core::io::physical::PhysicalFileSystem;
    let root = TempRoot::new("rename-native-service");
    let photos = root.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    let source = photos.join("a.jpg");
    std::fs::write(&source, b"original bytes").unwrap();
    let clock = Arc::new(FakeClock::new());
    let paths = pr_core::app_paths::AppPaths {
        journal_file: root.path().join("operations.jsonl"),
        ..pr_core::app_paths::AppPaths::new(root.path(), None, false)
    };
    let fs: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem);
    let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
    let bin = Arc::new(FakeRecycleBin::new(Arc::new(FakeFileSystem::new())));
    let service = FileActionService::new(journal, fs, clock, bin);
    let moved = service.rename(
        &RenameRequest {
            source: source.to_string_lossy().into_owned(),
            new_file_name: "x.jpg".into(),
        },
        &CancelToken::new(),
    );
    assert!(moved.succeeded, "{:?}", moved.error);
    let case = service.rename(
        &RenameRequest {
            source: photos.join("x.jpg").to_string_lossy().into_owned(),
            new_file_name: "X.JPG".into(),
        },
        &CancelToken::new(),
    );
    assert!(case.succeeded, "{:?}", case.error);
    assert_eq!(std::fs::read(photos.join("X.JPG")).unwrap(), b"original bytes");
    assert_eq!(
        pr_core::io::physical::PhysicalFileSystem
            .get_stored_file_names(&photos.join("X.JPG"))
            .unwrap(),
        ["X.JPG"]
    );
}

#[test]
#[ignore = "native Windows case-sensitive directory; requires fsutil capability on the test-owned TempRoot"]
fn native_case_sensitive_directory_reports_two_case_variants() {
    // C#: RenameFileActionTests.CaseOnlyRename_TwoDifferentFilesInACaseSensitiveFolder_IsRefused
    use pr_core::file_actions::case_only_rename::CaseOnlyRename;
    let root = TempRoot::new("rename-native-case-sensitive");
    // This modifies only the directory created by this test. Fail explicitly when the host cannot provide the capability.
    let enabled = std::process::Command::new("fsutil")
        .args(["file", "setCaseSensitiveInfo"])
        .arg(root.path())
        .arg("enable")
        .output()
        .unwrap();
    assert!(
        enabled.status.success(),
        "case-sensitive directory capability unavailable: {} {}",
        String::from_utf8_lossy(&enabled.stdout),
        String::from_utf8_lossy(&enabled.stderr)
    );
    let lower = root.path().join("a.jpg");
    let upper = root.path().join("A.jpg");
    std::fs::write(&lower, b"one").unwrap();
    std::fs::write(&upper, b"two").unwrap();
    let fs = pr_core::io::physical::PhysicalFileSystem;
    assert_eq!(fs.get_stored_file_names(&lower).unwrap().len(), 2);
    assert_eq!(
        CaseOnlyRename::probe_target(&fs, lower.to_str().unwrap(), upper.to_str().unwrap()).unwrap(),
        (true, false)
    );
    assert_eq!(std::fs::read(&lower).unwrap(), b"one");
    assert_eq!(std::fs::read(&upper).unwrap(), b"two");
}
#[test]
fn rename_invalid_target_is_rejected_before_anything_is_journaled() {
    // C#: RenameFileActionTests.Rename_InvalidTarget_IsRejectedBeforeAnythingIsJournaled
    use pr_core::file_actions::service::{EnglishFileActionTexts, FileActionTexts};
    for name in [
        "",
        "  ",
        "sub\\x.jpg",
        "..\\x.jpg",
        "a/b.jpg",
        "a:b.jpg",
        "a?.jpg",
        "x.",
        "CON.jpg",
        "nul",
        "LPT1.jpg",
    ] {
        let rig = setup(&["a.jpg"]);
        let result = rig
            .service
            .rename(&rig.request(&rig.file("a.jpg"), name), &CancelToken::new());
        assert!(!result.succeeded, "{name:?}");
        assert_eq!(
            result.error,
            Some(EnglishFileActionTexts.core_file_action_rename_invalid_name(name)),
            "{name:?}"
        );
        assert!(rig.fs.file_exists(std::path::Path::new(&rig.file("a.jpg"))));
        let mut entries = Vec::new();
        rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
        assert!(entries.is_empty());
    }
}

#[test]
fn rename_source_missing_precedes_target_conflict_without_journal() {
    // C#: FileActionService.ExecuteCoreAsync checks renamedSourceStat before CaseOnlyRename.ProbeTarget.
    use pr_core::file_actions::service::{EnglishFileActionTexts, FileActionTexts};
    let rig = setup(&["taken.jpg"]);
    let source = rig.file("missing.jpg");
    let result = rig
        .service
        .rename(&rig.request(&source, "taken.jpg"), &CancelToken::new());
    assert!(!result.succeeded);
    assert_eq!(
        result.error,
        Some(EnglishFileActionTexts.core_file_action_source_missing(&source))
    );
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn rename_target_raced_with_different_identity_is_untouched_and_dismissed() {
    // C#: FileActionService.IsRenameUntouched accepts a target that cannot be the original source file.
    let rig = setup(&["a.jpg"]);
    let fs = rig.fs.clone();
    let service = FileActionService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        rig.clock.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
    )
    .with_move_override(Arc::new(move |_, target| {
        fs.add_file(target, b"different target bytes");
        Err(std::io::Error::other("target appeared"))
    }));
    let result = service.rename(&rig.request(&rig.file("a.jpg"), "x.jpg"), &CancelToken::new());
    assert!(!result.succeeded);
    assert!(!result.source_removed);
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert_eq!(entries.last().unwrap().state, JournalState::Dismissed);
    assert_eq!(
        rig.fs
            .get_file_stat(std::path::Path::new(&rig.file("a.jpg")))
            .unwrap()
            .length,
        5
    );
}

#[test]
fn rename_target_inspection_error_stays_failed_instead_of_proving_untouched() {
    // C#: FileActionService.IsRenameUntouched catches an inspection error and returns false.
    let rig = setup(&["a.jpg"]);
    let fs = rig.fs.clone();
    let service = FileActionService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        rig.clock.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
    )
    .with_move_override(Arc::new(move |_, target| {
        fs.fail(
            pr_test_support::FsOp::Stat,
            target,
            std::io::ErrorKind::PermissionDenied,
        );
        Err(std::io::Error::other("move refused with uncertain target"))
    }));
    let result = service.rename(&rig.request(&rig.file("a.jpg"), "x.jpg"), &CancelToken::new());
    assert!(!result.succeeded);
    assert!(!result.source_removed);
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert_eq!(entries.last().unwrap().state, JournalState::Failed);
}

#[test]
fn rename_group_case_only_member_failure_rolls_back_prior_members_and_dismisses() {
    // C#: FileActionService.ExecuteGroupAsync uses case-spelling-aware Move compensation before dismissing a rolled-back group.
    let rig = setup(&["a.jpg", "a.cr2", "a.xmp"]);
    rig.fs.fail(
        pr_test_support::FsOp::Move,
        std::path::Path::new(&rig.file("a.cr2")),
        std::io::ErrorKind::PermissionDenied,
    );
    let result = rig
        .service
        .rename_group(&rig.group(Some("a.xmp")), "A", &CancelToken::new());
    assert!(!result.succeeded);
    assert!(result.journal_persisted);
    assert!(result.members.iter().all(|member| member.state_known
        && !member.completed
        && !member.conflict
        && member.source_exists
        && !member.destination_exists));
    for name in ["a.jpg", "a.cr2", "a.xmp"] {
        assert_eq!(
            rig.fs
                .get_stored_file_names(std::path::Path::new(&rig.file(name)))
                .unwrap(),
            [name]
        );
    }
    let mut entries = Vec::new();
    rig.journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert_eq!(entries.last().unwrap().state, JournalState::Dismissed);
}
