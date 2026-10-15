//! Port of the C# recovery-check tests (W3-07, red phase: the stub bodies of `file_actions::recovery_check` are `todo!()`).
//!
//! C# sources at baseline d92d6aae, `tests/PhotoReview.Core.Tests/FileActions/`:
//! * `RecoveryFileCheckTests.cs`: every method; the theory rows are separate tests (`// C#:` names the method and row),
//! * `RecoveryFileCheckMutationTests.cs`: every method,
//! * `RecoveryRecycleGroupTests.cs`: the six `Check_*` facts only (the `Retry_*` facts belong to W3-06),
//! * `PermanentDeleteTests.cs`: `Recovery_PermanentRecycle_VerdictReflectsPermanence` only (its rest belongs to W3-04/05).
//!
//! Plus the goldens `tests/golden/recovery-decide.json` (720 rows) and `tests/golden/recovery-check.json` (735 cases; the
//! inputs are in `tests/golden-inputs/recovery-check.json`).
//!
//! Documented skips (no assertion made, the case is still listed in the golden test's comments):
//!
//! * `stat-throws-InvalidOperationException-propagates` (`rust_skip`): a C# exception that the Rust seam does not model; the
//!   C# catch filter has no Rust analogue, every `Err` of the seam is `Unreadable`.
//! * `dir-throws-source-folder-unreadable-folder-false` and `dir-throws-destination-folder` (virtual): `directory_exists`
//!   returns `bool` in Rust and the fake has no directory fault, so an injected `Directory.Exists` throw cannot be reproduced
//!   (contract W3-07, open question 3).
//! * The `physical` cases run against the real disk and are `#[ignore = "native: real disk"]`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use pr_core::abstractions::FileSystem;
use pr_core::file_actions::errors::SOURCE_STILL_EXISTS_AFTER_RECOVERY;
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{
    RecoveryCheckResult, RecoveryFileCheck, RecoveryGroupMemberCheck, RecoveryPathCheck, RecoveryPathStatus,
    RecoveryVerdict,
};
use pr_core::io::counting::{CountingFileSystem, StatCounter};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_test_support::golden::{self, str_of};
use pr_test_support::{FakeFileSystem, FsOp, TempRoot};
use serde_json::{Value, json};

/// C# `Src` / `Dst` of `RecoveryFileCheckTests`.
const SRC: &str = r"C:\photos\a.jpg";
const DST: &str = r"D:\keep\a.jpg";
const DST_FOLDER: &str = r"D:\keep";
/// C# `Stamp` of `RecoveryFileCheckTests`: 2026-01-02T03:04:05Z.
const FILE_STAMP: i64 = 639_029_198_450_000_000;
const TICKS_PER_SECOND: i64 = 10_000_000;
const TICKS_PER_DAY: i64 = 864_000_000_000;

/// `DateTimeUtc` of a .NET tick count.
fn at(ticks: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(ticks).expect("ticks in the .NET DateTime range")
}

fn bytes(len: usize) -> Vec<u8> {
    vec![0; len]
}

/// C# `Entry(type, state, destination)` of `RecoveryFileCheckTests`: size 10, both stamps `ticks` (UTC).
fn plain_entry(op: FileOperationType, state: JournalState, destination: Option<&str>, ticks: i64) -> JournalEntry {
    JournalEntry {
        id: "id1".to_owned(),
        op_type: op,
        state,
        source: SRC.to_owned(),
        destination: destination.map(str::to_owned),
        size: 10,
        last_write_utc: JournalDateTime::utc(at(ticks)),
        timestamp_utc: JournalDateTime::utc(at(ticks)),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

/// C# `Fs(source, sourceChanged, dest, destChanged, destDir)`: source 10 bytes (11 when changed), destination 10 bytes (3
/// when changed), both stamped `FILE_STAMP`.
fn fs_for_rows(
    source: bool,
    source_changed: bool,
    destination: bool,
    destination_changed: bool,
    destination_folder: bool,
) -> Arc<FakeFileSystem> {
    let fs = FakeFileSystem::new();
    if source {
        fs.add_file_at(SRC, &bytes(if source_changed { 11 } else { 10 }), at(FILE_STAMP));
    }
    if destination {
        fs.add_file_at(DST, &bytes(if destination_changed { 3 } else { 10 }), at(FILE_STAMP));
    }
    if destination_folder {
        fs.add_dir(DST_FOLDER);
    }
    Arc::new(fs)
}

/// `new RecoveryFileCheck(fs).Check(entry)`.
fn check(fs: Arc<FakeFileSystem>, entry: &JournalEntry) -> RecoveryCheckResult {
    RecoveryFileCheck::new(fs).check(entry)
}

fn operation_of(name: &str) -> FileOperationType {
    match name {
        "Move" => FileOperationType::Move,
        "Copy" => FileOperationType::Copy,
        "Recycle" => FileOperationType::Recycle,
        other => panic!("unknown operation type {other}"),
    }
}

fn state_of(name: &str) -> JournalState {
    match name {
        "Prepared" => JournalState::Prepared,
        "Committed" => JournalState::Committed,
        "Failed" => JournalState::Failed,
        "Dismissed" => JournalState::Dismissed,
        other => panic!("unknown journal state {other}"),
    }
}

fn status_of(name: &str) -> RecoveryPathStatus {
    match name {
        "Exists" => RecoveryPathStatus::Exists,
        "Missing" => RecoveryPathStatus::Missing,
        "Changed" => RecoveryPathStatus::Changed,
        "Unreadable" => RecoveryPathStatus::Unreadable,
        other => panic!("unknown path status {other}"),
    }
}

fn status_name(status: RecoveryPathStatus) -> &'static str {
    match status {
        RecoveryPathStatus::Exists => "Exists",
        RecoveryPathStatus::Missing => "Missing",
        RecoveryPathStatus::Changed => "Changed",
        RecoveryPathStatus::Unreadable => "Unreadable",
    }
}

fn verdict_name(verdict: RecoveryVerdict) -> &'static str {
    match verdict {
        RecoveryVerdict::CanRetry => "CanRetry",
        RecoveryVerdict::AlreadyDone => "AlreadyDone",
        RecoveryVerdict::Conflict => "Conflict",
        RecoveryVerdict::SourceChanged => "SourceChanged",
        RecoveryVerdict::DestinationChanged => "DestinationChanged",
        RecoveryVerdict::Lost => "Lost",
        RecoveryVerdict::Unknown => "Unknown",
        RecoveryVerdict::RecycleUnverifiable => "RecycleUnverifiable",
        RecoveryVerdict::NotRecycled => "NotRecycled",
        RecoveryVerdict::PermanentlyDeleted => "PermanentlyDeleted",
        RecoveryVerdict::PartiallyPermanentlyDeleted => "PartiallyPermanentlyDeleted",
    }
}

/// C# `Exception` type name of a golden `stat_throws` entry, mapped to the `io::ErrorKind` the fake injects.
fn io_kind_of(exception: &str) -> std::io::ErrorKind {
    use std::io::ErrorKind;
    match exception {
        "UnauthorizedAccessException" => ErrorKind::PermissionDenied,
        "IOException" | "PathTooLongException" => ErrorKind::Other,
        "FileNotFoundException" => ErrorKind::NotFound,
        "ArgumentException" => ErrorKind::InvalidInput,
        "NotSupportedException" => ErrorKind::Unsupported,
        other => panic!("no injected fault for C# exception {other}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden JSON helpers (`tests/golden-inputs/*.json` inputs, `tests/golden/*.json` expected outputs).
// ---------------------------------------------------------------------------------------------------------------------

/// `tests/golden-inputs/<name>.json`, keyed by case name.
fn input_cases(name: &str) -> HashMap<String, Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .expect("golden inputs have a cases array")
        .iter()
        .map(|case| (str_of(case, "name").to_owned(), case.clone()))
        .collect()
}

/// Replaces `{root}` in every JSON string (the physical cases name their files relative to a private temp folder).
fn expand(value: &Value, root: &str) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace("{root}", root)),
        Value::Array(items) => Value::Array(items.iter().map(|item| expand(item, root)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), expand(item, root)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `"ticks": "639029198450000000"` (the goldens store ticks as text).
fn ticks_of(value: &Value) -> i64 {
    value
        .as_str()
        .expect("ticks are JSON text")
        .parse()
        .expect("ticks are an integer")
}

/// `{"ticks": ..., "kind": "Utc" | "Unspecified"}` of a journal line.
fn journal_date(value: &Value) -> JournalDateTime {
    let time = at(ticks_of(&value["ticks"]));
    match value["kind"].as_str() {
        Some("Utc") => JournalDateTime::utc(time),
        Some("Unspecified") => JournalDateTime::unspecified(time),
        other => panic!("unknown DateTime kind {other:?}"),
    }
}

fn member_of(value: &Value) -> JournalGroupMember {
    JournalGroupMember {
        source: str_of(value, "source").to_owned(),
        destination: value["destination"].as_str().map(str::to_owned),
        size: value["size"].as_i64().expect("member size"),
        last_write_utc: journal_date(&value["last_write"]),
        permanent: value["permanent"].as_bool().unwrap_or(false),
    }
}

/// The journal entry of a golden input case (`entry` object). An absent `group_members` key is `None`, an empty list is
/// `Some(vec![])`, as in C#.
fn entry_of(value: &Value) -> JournalEntry {
    JournalEntry {
        id: str_of(value, "id").to_owned(),
        op_type: operation_of(str_of(value, "type")),
        state: state_of(str_of(value, "state")),
        source: str_of(value, "source").to_owned(),
        destination: value["destination"].as_str().map(str::to_owned),
        size: value["size"].as_i64().expect("entry size"),
        last_write_utc: journal_date(&value["last_write"]),
        timestamp_utc: journal_date(&value["timestamp"]),
        error: value["error"].as_str().map(str::to_owned),
        error_code: value["error_code"].as_str().map(str::to_owned),
        permanent: value["permanent"].as_bool(),
        undo: value["undo"].as_bool(),
        group_id: value["group_id"].as_str().map(str::to_owned),
        group_members: value["group_members"]
            .as_array()
            .map(|members| members.iter().map(member_of).collect()),
    }
}

/// The virtual fixture of a golden case: files (with their size and last write), folders, and injected stat faults.
/// Parents of files exist implicitly, as in the oracle.
fn virtual_fs(input: &Value) -> Arc<FakeFileSystem> {
    let fs = FakeFileSystem::new();
    for file in input["files"].as_array().into_iter().flatten() {
        let size = usize::try_from(file["size"].as_i64().expect("file size")).expect("file size is not negative");
        fs.add_file_at(
            str_of(file, "path"),
            &bytes(size),
            at(ticks_of(&file["last_write_ticks"])),
        );
    }
    for dir in input["dirs"].as_array().into_iter().flatten() {
        fs.add_dir(dir.as_str().expect("folder path"));
    }
    for (path, exception) in input["stat_throws"].as_object().into_iter().flatten() {
        fs.fail(
            FsOp::Stat,
            path,
            io_kind_of(exception.as_str().expect("exception name")),
        );
    }
    Arc::new(fs)
}

/// `FileStat` times of the physical fixture use the Unix epoch of `SystemTime`.
fn to_system_time(time: DateTimeUtc) -> SystemTime {
    const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;
    let nanos = u64::try_from(time.ticks() - UNIX_EPOCH_TICKS).expect("after 1970") * 100;
    SystemTime::UNIX_EPOCH + Duration::from_nanos(nanos)
}

/// Creates the physical fixture of an (expanded) golden case in the real file system.
fn materialise(input: &Value) {
    for file in input["files"].as_array().into_iter().flatten() {
        let path = PathBuf::from(str_of(file, "path"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create the fixture folder");
        }
        let size = usize::try_from(file["size"].as_i64().expect("file size")).expect("file size is not negative");
        std::fs::write(&path, bytes(size)).expect("write the fixture file");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|handle| handle.set_modified(to_system_time(at(ticks_of(&file["last_write_ticks"])))))
            .expect("set the fixture last write");
    }
    for dir in input["dirs"].as_array().into_iter().flatten() {
        std::fs::create_dir_all(dir.as_str().expect("folder path")).expect("create the fixture folder");
    }
}

/// The golden shape of one [`RecoveryPathCheck`] (`path`, `status`, `size`, `last_write_ticks`, `folder_exists`).
fn path_json(path: &RecoveryPathCheck) -> Value {
    json!({
        "path": path.path,
        "status": status_name(path.status),
        "size": path.current_size,
        "last_write_ticks": path.current_last_write_utc.map(|time| time.ticks().to_string()),
        "folder_exists": path.folder_exists,
    })
}

/// The golden shape of one group member (`member_source`, `verdict`, `source`, `destination`,
/// `entry_is_the_group_entry`, `entry_has_members`).
fn member_json(member: &RecoveryGroupMemberCheck, entry: &JournalEntry) -> Value {
    json!({
        "member_source": member.member.source,
        "verdict": verdict_name(member.verdict()),
        "source": path_json(member.source()),
        "destination": member.destination().map_or(Value::Null, path_json),
        "entry_is_the_group_entry": member.check.entry == *entry,
        "entry_has_members": member.check.entry.group_members.as_ref().is_some_and(|members| !members.is_empty()),
    })
}

/// The golden shape of a whole result, restricted to the keys the oracle writes.
fn result_json(result: &RecoveryCheckResult, entry: &JournalEntry) -> Value {
    json!({
        "verdict": verdict_name(result.verdict),
        "source": path_json(&result.source),
        "destination": result.destination.as_ref().map_or(Value::Null, path_json),
        "destination_folder_missing": result.destination_folder_missing(),
        "is_group": result.is_group(),
        "members": result.group_members.as_ref().map_or(Value::Null, |members| {
            Value::Array(members.iter().map(|member| member_json(member, entry)).collect())
        }),
    })
}

/// One line per compared key whose value differs from the golden `expected` case.
fn differences(result: &RecoveryCheckResult, entry: &JournalEntry, expected: &Value) -> Vec<String> {
    let actual = result_json(result, entry);
    [
        "verdict",
        "source",
        "destination",
        "destination_folder_missing",
        "is_group",
        "members",
    ]
    .into_iter()
    .filter(|key| actual[*key] != expected[*key])
    .map(|key| format!("{key}: got {} want {}", actual[key], expected[key]))
    .collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// RecoveryFileCheckTests
// ---------------------------------------------------------------------------------------------------------------------

mod file_check {
    use super::*;

    /// One row of `Verdict_MoveCopy_FollowsRuleTable`: `(type, state, S, S changed, D, D changed)` with destination `Dst`.
    fn rule_row(
        op: FileOperationType,
        state: JournalState,
        source: bool,
        source_changed: bool,
        destination: bool,
        destination_changed: bool,
    ) -> RecoveryVerdict {
        let fs = fs_for_rows(source, source_changed, destination, destination_changed, false);
        check(fs, &plain_entry(op, state, Some(DST), FILE_STAMP)).verdict
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 1 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_prepared_source_only_can_retry() {
        let verdict = rule_row(
            FileOperationType::Move,
            JournalState::Prepared,
            true,
            false,
            false,
            false,
        );
        assert_eq!(verdict, RecoveryVerdict::CanRetry);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 2 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_failed_source_only_can_retry() {
        let verdict = rule_row(FileOperationType::Move, JournalState::Failed, true, false, false, false);
        assert_eq!(verdict, RecoveryVerdict::CanRetry);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 3 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_copy_failed_source_only_can_retry() {
        let verdict = rule_row(FileOperationType::Copy, JournalState::Failed, true, false, false, false);
        assert_eq!(verdict, RecoveryVerdict::CanRetry);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 4 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_failed_source_changed_is_source_changed() {
        let verdict = rule_row(FileOperationType::Move, JournalState::Failed, true, true, false, false);
        assert_eq!(verdict, RecoveryVerdict::SourceChanged);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 5 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_prepared_destination_only_is_already_done() {
        let verdict = rule_row(
            FileOperationType::Move,
            JournalState::Prepared,
            false,
            false,
            true,
            false,
        );
        assert_eq!(verdict, RecoveryVerdict::AlreadyDone);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 6 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_copy_prepared_destination_only_is_already_done() {
        let verdict = rule_row(
            FileOperationType::Copy,
            JournalState::Prepared,
            false,
            false,
            true,
            false,
        );
        assert_eq!(verdict, RecoveryVerdict::AlreadyDone);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 7 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_failed_destination_changed_is_destination_changed() {
        let verdict = rule_row(FileOperationType::Move, JournalState::Failed, false, false, true, true);
        assert_eq!(verdict, RecoveryVerdict::DestinationChanged);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 8 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_failed_neither_file_is_lost() {
        let verdict = rule_row(
            FileOperationType::Move,
            JournalState::Failed,
            false,
            false,
            false,
            false,
        );
        assert_eq!(verdict, RecoveryVerdict::Lost);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 9 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_copy_prepared_neither_file_is_lost() {
        let verdict = rule_row(
            FileOperationType::Copy,
            JournalState::Prepared,
            false,
            false,
            false,
            false,
        );
        assert_eq!(verdict, RecoveryVerdict::Lost);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 10 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_failed_both_present_is_conflict() {
        let verdict = rule_row(FileOperationType::Move, JournalState::Failed, true, false, true, false);
        assert_eq!(verdict, RecoveryVerdict::Conflict);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 11 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_failed_source_changed_both_present_is_conflict() {
        let verdict = rule_row(FileOperationType::Move, JournalState::Failed, true, true, true, false);
        assert_eq!(verdict, RecoveryVerdict::Conflict);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 12 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_failed_destination_changed_both_present_is_conflict() {
        let verdict = rule_row(FileOperationType::Move, JournalState::Failed, true, false, true, true);
        assert_eq!(verdict, RecoveryVerdict::Conflict);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 13 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_copy_failed_both_exist_is_already_done() {
        let verdict = rule_row(FileOperationType::Copy, JournalState::Failed, true, false, true, false);
        assert_eq!(verdict, RecoveryVerdict::AlreadyDone);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 14 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_copy_failed_destination_changed_is_conflict() {
        let verdict = rule_row(FileOperationType::Copy, JournalState::Failed, true, false, true, true);
        assert_eq!(verdict, RecoveryVerdict::Conflict);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 15 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_copy_failed_source_changed_both_present_is_conflict() {
        let verdict = rule_row(FileOperationType::Copy, JournalState::Failed, true, true, true, false);
        assert_eq!(verdict, RecoveryVerdict::Conflict);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 16 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_committed_destination_only_is_already_done() {
        let verdict = rule_row(
            FileOperationType::Move,
            JournalState::Committed,
            false,
            false,
            true,
            false,
        );
        assert_eq!(verdict, RecoveryVerdict::AlreadyDone);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 17 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_copy_committed_both_exist_is_already_done() {
        let verdict = rule_row(
            FileOperationType::Copy,
            JournalState::Committed,
            true,
            false,
            true,
            false,
        );
        assert_eq!(verdict, RecoveryVerdict::AlreadyDone);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 18 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_committed_neither_file_is_lost() {
        let verdict = rule_row(
            FileOperationType::Move,
            JournalState::Committed,
            false,
            false,
            false,
            false,
        );
        assert_eq!(verdict, RecoveryVerdict::Lost);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 19 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_committed_source_only_is_unknown() {
        let verdict = rule_row(
            FileOperationType::Move,
            JournalState::Committed,
            true,
            false,
            false,
            false,
        );
        assert_eq!(verdict, RecoveryVerdict::Unknown);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 20 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_committed_destination_changed_is_unknown() {
        let verdict = rule_row(
            FileOperationType::Move,
            JournalState::Committed,
            false,
            false,
            true,
            true,
        );
        assert_eq!(verdict, RecoveryVerdict::Unknown);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveCopy_FollowsRuleTable (row 21 of 21)
    #[test]
    fn verdict_move_copy_follows_rule_table_move_dismissed_is_unknown() {
        let verdict = rule_row(
            FileOperationType::Move,
            JournalState::Dismissed,
            true,
            false,
            false,
            false,
        );
        assert_eq!(verdict, RecoveryVerdict::Unknown);
    }

    // C#: RecoveryFileCheckTests.Verdict_SourceLastWriteDiffers_IsSourceChanged
    #[test]
    fn verdict_source_last_write_differs_is_source_changed() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(SRC, &bytes(10), at(FILE_STAMP + 5 * TICKS_PER_SECOND));
        let result = check(
            fs,
            &plain_entry(FileOperationType::Move, JournalState::Failed, Some(DST), FILE_STAMP),
        );
        assert_eq!(result.source.status, RecoveryPathStatus::Changed);
        assert_eq!(result.verdict, RecoveryVerdict::SourceChanged);
    }

    // C#: RecoveryFileCheckTests.Destination_LastWriteDiffers_StillExists
    #[test]
    fn destination_last_write_differs_still_exists() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(DST, &bytes(10), at(FILE_STAMP + TICKS_PER_DAY));
        let result = check(
            fs,
            &plain_entry(FileOperationType::Move, JournalState::Failed, Some(DST), FILE_STAMP),
        );
        let destination = result.destination.expect("the entry has a destination");
        assert_eq!(destination.status, RecoveryPathStatus::Exists);
        assert_eq!(destination.current_last_write_utc, Some(at(FILE_STAMP + TICKS_PER_DAY)));
        assert_eq!(result.verdict, RecoveryVerdict::AlreadyDone);
    }

    // C#: RecoveryFileCheckTests.Verdict_MoveWithoutDestination_IsUnknown
    #[test]
    fn verdict_move_without_destination_is_unknown() {
        let fs = fs_for_rows(true, false, false, false, false);
        let result = check(
            fs,
            &plain_entry(FileOperationType::Move, JournalState::Failed, None, FILE_STAMP),
        );
        assert!(result.destination.is_none());
        assert_eq!(result.verdict, RecoveryVerdict::Unknown);
    }

    /// One row of `Verdict_Recycle`: the source file is present (and changed) or absent; no destination.
    fn recycle_row(source: bool, source_changed: bool) -> RecoveryVerdict {
        let fs = fs_for_rows(source, source_changed, false, false, false);
        check(
            fs,
            &plain_entry(FileOperationType::Recycle, JournalState::Prepared, None, FILE_STAMP),
        )
        .verdict
    }

    // C#: RecoveryFileCheckTests.Verdict_Recycle (row 1 of 3)
    #[test]
    fn verdict_recycle_source_missing_is_recycle_unverifiable() {
        assert_eq!(recycle_row(false, false), RecoveryVerdict::RecycleUnverifiable);
    }

    // C#: RecoveryFileCheckTests.Verdict_Recycle (row 2 of 3)
    #[test]
    fn verdict_recycle_source_present_is_not_recycled() {
        assert_eq!(recycle_row(true, false), RecoveryVerdict::NotRecycled);
    }

    // C#: RecoveryFileCheckTests.Verdict_Recycle (row 3 of 3)
    #[test]
    fn verdict_recycle_source_changed_is_not_recycled() {
        assert_eq!(recycle_row(true, true), RecoveryVerdict::NotRecycled);
    }

    // C#: RecoveryFileCheckTests.Verdict_Recycle_IgnoresDestination
    #[test]
    fn verdict_recycle_ignores_destination() {
        let fs = fs_for_rows(false, false, true, false, false);
        let result = check(
            fs,
            &plain_entry(FileOperationType::Recycle, JournalState::Failed, Some(DST), FILE_STAMP),
        );
        assert_eq!(result.verdict, RecoveryVerdict::RecycleUnverifiable);
    }

    // C#: RecoveryFileCheckTests.DestinationFolderMissing_TrueOnlyWhenDestinationAndFolderAreGone (3 scenarios)
    #[test]
    fn destination_folder_missing_true_only_when_destination_and_folder_are_gone() {
        let entry = plain_entry(FileOperationType::Move, JournalState::Failed, Some(DST), FILE_STAMP);

        let gone = check(fs_for_rows(true, false, false, false, false), &entry);
        assert!(gone.destination_folder_missing());

        let folder_only = check(fs_for_rows(true, false, false, false, true), &entry);
        assert!(!folder_only.destination_folder_missing());
        assert_eq!(folder_only.verdict, RecoveryVerdict::CanRetry);

        let present = check(fs_for_rows(false, false, true, false, false), &entry);
        assert!(!present.destination_folder_missing());
    }

    // C#: RecoveryFileCheckTests.Unreadable_AccessDenied_IsUnknown (row 1 of 2: source denied)
    #[test]
    fn unreadable_access_denied_source_is_unknown() {
        let fs = fs_for_rows(true, false, true, false, false);
        fs.fail(FsOp::Stat, SRC, std::io::ErrorKind::PermissionDenied);
        let result = check(
            fs,
            &plain_entry(FileOperationType::Move, JournalState::Failed, Some(DST), FILE_STAMP),
        );
        assert_eq!(result.verdict, RecoveryVerdict::Unknown);
        assert_eq!(result.source.status, RecoveryPathStatus::Unreadable);
        assert_eq!(result.source.current_size, None);
    }

    // C#: RecoveryFileCheckTests.Unreadable_AccessDenied_IsUnknown (row 2 of 2: destination denied)
    #[test]
    fn unreadable_access_denied_destination_is_unknown() {
        let fs = fs_for_rows(true, false, true, false, false);
        fs.fail(FsOp::Stat, DST, std::io::ErrorKind::PermissionDenied);
        let result = check(
            fs,
            &plain_entry(FileOperationType::Move, JournalState::Failed, Some(DST), FILE_STAMP),
        );
        assert_eq!(result.verdict, RecoveryVerdict::Unknown);
        let destination = result.destination.expect("the entry has a destination");
        assert_eq!(destination.status, RecoveryPathStatus::Unreadable);
        assert_eq!(destination.current_size, None);
    }

    // C#: RecoveryFileCheckTests.Unreadable_RecycleSource_IsUnknown
    #[test]
    fn unreadable_recycle_source_is_unknown() {
        let fs = fs_for_rows(true, false, false, false, false);
        fs.fail(FsOp::Stat, SRC, std::io::ErrorKind::PermissionDenied);
        let result = check(
            fs,
            &plain_entry(FileOperationType::Recycle, JournalState::Prepared, None, FILE_STAMP),
        );
        assert_eq!(result.verdict, RecoveryVerdict::Unknown);
    }

    // C#: RecoveryFileCheckTests.Check_NeverMutates_OnlyStatCalls
    #[test]
    fn check_never_mutates_only_stat_calls() {
        let fs = fs_for_rows(true, false, true, false, false);
        let stat_calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&stat_calls);
        let metrics: Arc<dyn StatCounter> = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let counting = Arc::new(CountingFileSystem::new(fs.clone(), metrics));

        RecoveryFileCheck::new(counting).check(&plain_entry(
            FileOperationType::Move,
            JournalState::Failed,
            Some(DST),
            FILE_STAMP,
        ));

        // CountingFileSystem counts every metadata query (C#: FileExists, DirectoryExists, GetFileStat): per path one
        // folder check and one stat, so 2 + 2. The fake's own Stat counter (below) sees only the two stats.
        assert_eq!(
            stat_calls.load(Ordering::SeqCst),
            4,
            "a folder check and a stat for the source, the same for the destination"
        );
        assert_eq!(fs.count(FsOp::Stat), 2);
        for op in [
            FsOp::Move,
            FsOp::Copy,
            FsOp::Delete,
            FsOp::OpenRead,
            FsOp::OpenAppend,
            FsOp::WriteAtomic,
            FsOp::ReadText,
            FsOp::EnumerateFiles,
            FsOp::EnumerateDirectories,
            FsOp::CreateDirectory,
        ] {
            assert_eq!(fs.count(op), 0, "{op:?} must not be called");
        }
        assert!(fs.file_exists(Path::new(SRC)));
        assert!(fs.file_exists(Path::new(DST)));
        assert_eq!(fs.peek_stat(SRC).map(|stat| stat.length), Some(10));
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// RecoveryFileCheckMutationTests
// ---------------------------------------------------------------------------------------------------------------------

mod mutation {
    use super::*;

    /// C# `Stamp` of `RecoveryFileCheckMutationTests`: 2026-10-01T08:00:00Z.
    const STAMP: i64 = 639_264_384_000_000_000;
    const JPG: &str = r"C:\photos\a.jpg";
    const RAW: &str = r"C:\photos\a.cr2";
    const OUT_JPG: &str = r"C:\out\a.jpg";
    const OUT_RAW: &str = r"C:\out\a.cr2";

    /// C# `JournalGroupMember(source, destination, size, Stamp)` (`permanent` false unless given).
    fn member(source: &str, destination: Option<&str>, size: i64, permanent: bool) -> JournalGroupMember {
        JournalGroupMember {
            source: source.to_owned(),
            destination: destination.map(str::to_owned),
            size,
            last_write_utc: JournalDateTime::utc(at(STAMP)),
            permanent,
        }
    }

    /// C# `UndoRecycleGroup` / `MoveUndoGroup` before `Undo` is set: a Failed group whose source (and destination) come
    /// from the first member, error "x", group id "capture".
    fn group_entry(
        op: FileOperationType,
        destination: Option<String>,
        members: Vec<JournalGroupMember>,
    ) -> JournalEntry {
        let first = members[0].clone();
        JournalEntry {
            id: "group".to_owned(),
            op_type: op,
            state: JournalState::Failed,
            source: first.source,
            destination,
            size: first.size,
            last_write_utc: JournalDateTime::utc(at(STAMP)),
            timestamp_utc: JournalDateTime::utc(at(STAMP)),
            error: Some("x".to_owned()),
            error_code: None,
            permanent: None,
            undo: None,
            group_id: Some("capture".to_owned()),
            group_members: Some(members),
        }
    }

    /// `RecoveryCheckResult` built by hand, as the Recovery presenter does (all fields are `pub`).
    fn result_with_members(members: Option<Vec<RecoveryGroupMemberCheck>>) -> RecoveryCheckResult {
        RecoveryCheckResult {
            entry: plain_entry(FileOperationType::Move, JournalState::Failed, Some(OUT_JPG), STAMP),
            source: RecoveryPathCheck::new(JPG, RecoveryPathStatus::Exists, Some(10), Some(at(STAMP)), true),
            destination: None,
            verdict: RecoveryVerdict::Unknown,
            group_members: members,
        }
    }

    // C#: RecoveryFileCheckMutationTests.Check_UndoRecycleGroupMemberBackAtItsOriginalPath_MemberIsAlreadyDone
    #[test]
    fn check_undo_recycle_group_member_back_at_its_original_path_member_is_already_done() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(JPG, &bytes(10), at(STAMP)); // restored from the bin already; the RAW is still in the bin (missing)
        let mut entry = group_entry(
            FileOperationType::Recycle,
            None,
            vec![member(JPG, None, 10, false), member(RAW, None, 10, false)],
        );
        entry.undo = Some(true);

        let result = check(fs, &entry);
        let members = result.group_members.expect("a group result has members");

        assert_eq!(members[0].verdict(), RecoveryVerdict::AlreadyDone);
        assert_eq!(members[1].verdict(), RecoveryVerdict::RecycleUnverifiable);
        assert_eq!(
            result.verdict,
            RecoveryVerdict::CanRetry,
            "the missing member can be restored by a retry"
        );
    }

    // C#: RecoveryFileCheckMutationTests.Check_UndoRecycleGroupMemberWithDifferentContentAtItsPath_MemberIsNotAlreadyDone
    #[test]
    fn check_undo_recycle_group_member_with_different_content_at_its_path_member_is_not_already_done() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(JPG, &bytes(11), at(STAMP)); // somebody else's file sits where the restore must go
        let mut entry = group_entry(
            FileOperationType::Recycle,
            None,
            vec![member(JPG, None, 10, false), member(RAW, None, 10, false)],
        );
        entry.undo = Some(true);

        let result = check(fs, &entry);
        let members = result.group_members.expect("a group result has members");

        assert_ne!(members[0].verdict(), RecoveryVerdict::AlreadyDone);
        assert_ne!(result.verdict, RecoveryVerdict::CanRetry);
    }

    // C#: RecoveryFileCheckMutationTests.Check_UndoRecycleGroupAllMembersBack_EveryMemberIsAlreadyDone
    #[test]
    fn check_undo_recycle_group_all_members_back_every_member_is_already_done() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(JPG, &bytes(10), at(STAMP));
        fs.add_file_at(RAW, &bytes(10), at(STAMP));
        let mut entry = group_entry(
            FileOperationType::Recycle,
            None,
            vec![member(JPG, None, 10, false), member(RAW, None, 10, false)],
        );
        entry.undo = Some(true);

        let result = check(fs, &entry);
        let members = result.group_members.clone().expect("a group result has members");

        for member in &members {
            assert_eq!(member.verdict(), RecoveryVerdict::AlreadyDone);
        }
        // W2-FA-02: every file is back on disk, so the undo is done: not "probably in the Recycle Bin, cannot verify".
        assert_eq!(result.verdict, RecoveryVerdict::AlreadyDone);
    }

    // C#: RecoveryFileCheckMutationTests.Check_NonUndoRecycleGroupAllMembersMissing_StaysRecycleUnverifiable
    #[test]
    fn check_non_undo_recycle_group_all_members_missing_stays_recycle_unverifiable() {
        let fs = Arc::new(FakeFileSystem::new());
        let mut entry = group_entry(
            FileOperationType::Recycle,
            None,
            vec![member(JPG, None, 10, false), member(RAW, None, 10, false)],
        );
        entry.undo = None;
        entry.state = JournalState::Committed;

        let result = check(fs, &entry);

        assert_eq!(result.verdict, RecoveryVerdict::RecycleUnverifiable);
    }

    // C#: RecoveryFileCheckMutationTests.Check_UndoMoveGroupWithARetryableAndAConflictingMember_IsConflictNotCanRetry
    #[test]
    fn check_undo_move_group_with_a_retryable_and_a_conflicting_member_is_conflict_not_can_retry() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(JPG, &bytes(10), at(STAMP)); // S exists, D missing -> CanRetry
        fs.add_file_at(RAW, &bytes(10), at(STAMP));
        fs.add_file_at(OUT_RAW, &bytes(10), at(STAMP)); // S and D exist on a Move -> Conflict
        let mut entry = group_entry(
            FileOperationType::Move,
            Some(OUT_JPG.to_owned()),
            vec![
                member(JPG, Some(OUT_JPG), 10, false),
                member(RAW, Some(OUT_RAW), 10, false),
            ],
        );
        entry.undo = Some(true);

        let result = check(fs, &entry);
        let members = result.group_members.expect("a group result has members");

        assert_eq!(members[0].verdict(), RecoveryVerdict::CanRetry);
        assert_eq!(members[1].verdict(), RecoveryVerdict::Conflict);
        assert_eq!(result.verdict, RecoveryVerdict::Conflict);
    }

    // C#: RecoveryFileCheckMutationTests.Check_DeleteGroupWithUnreadableAndMissingMembers_IsUnknownNotCanRetry
    #[test]
    fn check_delete_group_with_unreadable_and_missing_members_is_unknown_not_can_retry() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.fail(FsOp::Stat, JPG, std::io::ErrorKind::PermissionDenied); // the JPG cannot be inspected
        let mut entry = group_entry(
            FileOperationType::Recycle,
            None,
            vec![member(JPG, None, 10, false), member(RAW, None, 10, false)],
        );
        entry.undo = None;

        let result = check(fs, &entry);
        let members = result.group_members.expect("a group result has members");

        assert_eq!(members[0].verdict(), RecoveryVerdict::Unknown);
        assert_eq!(members[1].verdict(), RecoveryVerdict::RecycleUnverifiable);
        assert_eq!(result.verdict, RecoveryVerdict::Unknown);
    }

    // C#: RecoveryFileCheckMutationTests.Check_UndoRecycleGroupWithUnreadableAndChangedMembers_IsUnknownNotCanRetry
    #[test]
    fn check_undo_recycle_group_with_unreadable_and_changed_members_is_unknown_not_can_retry() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(RAW, &bytes(11), at(STAMP)); // a different file sits at the restore path -> Unknown
        fs.fail(FsOp::Stat, JPG, std::io::ErrorKind::PermissionDenied); // -> Unknown
        let mut entry = group_entry(
            FileOperationType::Recycle,
            None,
            vec![member(JPG, None, 10, false), member(RAW, None, 10, false)],
        );
        entry.undo = Some(true);

        let result = check(fs, &entry);
        let members = result.group_members.expect("a group result has members");

        assert_eq!(members[0].verdict(), RecoveryVerdict::Unknown);
        assert_eq!(members[1].verdict(), RecoveryVerdict::Unknown);
        assert_eq!(result.verdict, RecoveryVerdict::Unknown);
    }

    // C#: RecoveryFileCheckMutationTests.IsGroup_WithoutMembers_IsFalse
    #[test]
    fn is_group_without_members_is_false() {
        assert!(!result_with_members(None).is_group());
    }

    // C#: RecoveryFileCheckMutationTests.IsGroup_WithAnEmptyMemberList_IsFalse
    #[test]
    fn is_group_with_an_empty_member_list_is_false() {
        assert!(!result_with_members(Some(Vec::new())).is_group());
    }

    // C#: RecoveryFileCheckMutationTests.IsGroup_WithMembers_IsTrue
    #[test]
    fn is_group_with_members_is_true() {
        let group = group_entry(FileOperationType::Recycle, None, vec![member(JPG, None, 10, false)]);
        let member_check = RecoveryGroupMemberCheck {
            member: group.group_members.clone().expect("a group entry has members")[0].clone(),
            check: result_with_members(None),
        };

        assert!(result_with_members(Some(vec![member_check])).is_group());
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// RecoveryRecycleGroupTests (the Check_* facts only)
// ---------------------------------------------------------------------------------------------------------------------

mod recycle_group {
    use super::*;

    /// C# `Stamp` of `RecoveryRecycleGroupTests`: 2026-09-29T00:00:00Z.
    const STAMP: i64 = 639_262_368_000_000_000;
    const JPG: &str = r"C:\photos\a.jpg";
    const RAW: &str = r"C:\photos\a.cr2";

    /// C# `Member(path, size = 10, permanent = false)`.
    fn member(source: &str, permanent: bool) -> JournalGroupMember {
        JournalGroupMember {
            source: source.to_owned(),
            destination: None,
            size: 10,
            last_write_utc: JournalDateTime::utc(at(STAMP)),
            permanent,
        }
    }

    /// C# `FailedGroup(members)`: a failed Recycle group; `Permanent` is `true` when any member is permanent, else `null`.
    fn failed_group(members: Vec<JournalGroupMember>) -> JournalEntry {
        let permanent = members.iter().any(|member| member.permanent).then_some(true);
        JournalEntry {
            id: "delete-group".to_owned(),
            op_type: FileOperationType::Recycle,
            state: JournalState::Failed,
            source: members[0].source.clone(),
            destination: None,
            size: members[0].size,
            last_write_utc: JournalDateTime::utc(at(STAMP)),
            timestamp_utc: JournalDateTime::utc(at(STAMP)),
            error: Some("x".to_owned()),
            error_code: Some(SOURCE_STILL_EXISTS_AFTER_RECOVERY.to_owned()),
            permanent,
            undo: None,
            group_id: Some("capture-a".to_owned()),
            group_members: Some(members),
        }
    }

    // C#: RecoveryRecycleGroupTests.Check_RecycleGroupWithSomeMembersStillPresent_CanRetry
    #[test]
    fn check_recycle_group_with_some_members_still_present_can_retry() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(RAW, &bytes(10), at(STAMP)); // the JPEG already went to the bin, the RAW is still here

        let result = check(fs, &failed_group(vec![member(JPG, false), member(RAW, false)]));

        assert_eq!(result.verdict, RecoveryVerdict::CanRetry);
    }

    // C#: RecoveryRecycleGroupTests.Check_RecycleGroupWithNothingRecycledYet_CanRetry
    #[test]
    fn check_recycle_group_with_nothing_recycled_yet_can_retry() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(JPG, &bytes(10), at(STAMP));
        fs.add_file_at(RAW, &bytes(10), at(STAMP));

        let result = check(fs, &failed_group(vec![member(JPG, false), member(RAW, false)]));

        assert_eq!(result.verdict, RecoveryVerdict::CanRetry);
    }

    // C#: RecoveryRecycleGroupTests.Check_RecycleGroupWherePresentMemberChanged_IsNotRecycledNotRetryable
    #[test]
    fn check_recycle_group_where_present_member_changed_is_not_recycled_not_retryable() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(RAW, &bytes(11), at(STAMP)); // size differs from the journal

        let result = check(fs, &failed_group(vec![member(JPG, false), member(RAW, false)]));

        assert_eq!(result.verdict, RecoveryVerdict::NotRecycled);
    }

    // C#: RecoveryRecycleGroupTests.Check_RecycleGroupAllMembersGone_IsRecycleUnverifiable
    #[test]
    fn check_recycle_group_all_members_gone_is_recycle_unverifiable() {
        let fs = Arc::new(FakeFileSystem::new());

        let result = check(fs, &failed_group(vec![member(JPG, false), member(RAW, false)]));

        assert_eq!(result.verdict, RecoveryVerdict::RecycleUnverifiable);
    }

    // C#: RecoveryRecycleGroupTests.Check_RecycleGroupAllMembersPermanentlyDeleted_IsPermanentlyDeleted
    #[test]
    fn check_recycle_group_all_members_permanently_deleted_is_permanently_deleted() {
        let fs = Arc::new(FakeFileSystem::new());

        let result = check(fs, &failed_group(vec![member(JPG, true), member(RAW, true)]));

        assert_eq!(result.verdict, RecoveryVerdict::PermanentlyDeleted);
    }

    // C#: RecoveryRecycleGroupTests.Check_RecycleGroupMixingPermanentAndRecycledMembers_IsPartiallyPermanentlyDeleted
    #[test]
    fn check_recycle_group_mixing_permanent_and_recycled_members_is_partially_permanently_deleted() {
        // JPEG on a drive without a bin (deleted for good), RAW recycled: the capture is NOT wholly unrecoverable.
        let fs = Arc::new(FakeFileSystem::new());

        let result = check(fs, &failed_group(vec![member(JPG, true), member(RAW, false)]));

        assert_eq!(result.verdict, RecoveryVerdict::PartiallyPermanentlyDeleted);
        let members = result.group_members.expect("a group result has members");
        assert_eq!(members[0].verdict(), RecoveryVerdict::PermanentlyDeleted);
        assert_eq!(members[1].verdict(), RecoveryVerdict::RecycleUnverifiable);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// PermanentDeleteTests: Recovery_PermanentRecycle_VerdictReflectsPermanence only
// ---------------------------------------------------------------------------------------------------------------------

mod permanent_delete {
    use super::*;

    /// C# `Photo` / `Stamp` of `PermanentDeleteTests`: 2026-09-25T10:00:00Z.
    const PHOTO: &str = r"E:\usb\a.jpg";
    const STAMP: i64 = 639_259_272_000_000_000;

    /// `new JournalEntry("id", Recycle, Committed, Photo, null, 10, Stamp, Stamp, Permanent: permanent)`.
    fn committed_recycle(permanent: Option<bool>) -> JournalEntry {
        JournalEntry {
            id: "id".to_owned(),
            op_type: FileOperationType::Recycle,
            state: JournalState::Committed,
            source: PHOTO.to_owned(),
            destination: None,
            size: 10,
            last_write_utc: JournalDateTime::utc(at(STAMP)),
            timestamp_utc: JournalDateTime::utc(at(STAMP)),
            error: None,
            error_code: None,
            permanent,
            undo: None,
            group_id: None,
            group_members: None,
        }
    }

    // C#: PermanentDeleteTests.Recovery_PermanentRecycle_VerdictReflectsPermanence (row 1: permanent, source absent)
    #[test]
    fn recovery_permanent_recycle_verdict_reflects_permanence_permanent_source_absent_is_permanently_deleted() {
        let fs = Arc::new(FakeFileSystem::new());
        assert_eq!(
            check(fs, &committed_recycle(Some(true))).verdict,
            RecoveryVerdict::PermanentlyDeleted
        );
    }

    // C#: PermanentDeleteTests.Recovery_PermanentRecycle_VerdictReflectsPermanence (row 2: not permanent, source absent)
    #[test]
    fn recovery_permanent_recycle_verdict_reflects_permanence_not_permanent_source_absent_is_recycle_unverifiable() {
        let fs = Arc::new(FakeFileSystem::new());
        assert_eq!(
            check(fs, &committed_recycle(None)).verdict,
            RecoveryVerdict::RecycleUnverifiable
        );
    }

    // C#: PermanentDeleteTests.Recovery_PermanentRecycle_VerdictReflectsPermanence (row 3: permanent, source present)
    #[test]
    fn recovery_permanent_recycle_verdict_reflects_permanence_permanent_source_present_is_not_recycled() {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file_at(PHOTO, &bytes(10), at(STAMP));
        assert_eq!(
            check(fs, &committed_recycle(Some(true))).verdict,
            RecoveryVerdict::NotRecycled
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Goldens (tests/golden/recovery-decide.json, tests/golden/recovery-check.json)
// ---------------------------------------------------------------------------------------------------------------------

/// `recovery-decide`: the 720-row table of `RecoveryFileCheck.Decide` (3 types x 4 states x 3 permanent x 4 S x 5 D).
#[test]
fn golden_recovery_decide_matches_oracle() {
    let oracle = golden::load("recovery-decide");
    let mut failures = Vec::new();

    for (index, case) in oracle.cases.iter().enumerate() {
        let destination = case["destination"].as_str().map(status_of);
        let mut entry = plain_entry(
            operation_of(str_of(case, "type")),
            state_of(str_of(case, "state")),
            destination.map(|_| DST),
            FILE_STAMP,
        );
        entry.permanent = case["permanent"].as_bool();
        let source = RecoveryPathCheck::new(SRC, status_of(str_of(case, "source")), None, None, true);
        let destination_check = destination.map(|status| RecoveryPathCheck::new(DST, status, None, None, true));

        let verdict = RecoveryFileCheck::decide(&entry, &source, destination_check.as_ref());

        if verdict_name(verdict) != str_of(case, "verdict") {
            failures.push(format!(
                "row {index}: {} {} permanent={} S={} D={} -> {}, oracle {}",
                str_of(case, "type"),
                str_of(case, "state"),
                case["permanent"],
                str_of(case, "source"),
                case["destination"],
                verdict_name(verdict),
                str_of(case, "verdict"),
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} decide rows differ from the oracle:\n{}",
        failures.len(),
        oracle.cases.len(),
        failures.join("\n")
    );
}

/// `recovery-check`, `mode: virtual` cases (715 in the oracle). Skipped, with the reason in the module docs: the
/// `rust_skip` case and the two `dir_throws` cases.
#[test]
fn golden_recovery_check_virtual_cases_match_oracle() {
    let oracle = golden::load("recovery-check");
    let inputs = input_cases("recovery-check");
    let mut failures = Vec::new();
    let mut ran = 0_usize;

    for expected in oracle.cases.iter().filter(|case| str_of(case, "mode") == "virtual") {
        let name = str_of(expected, "name");
        let input = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no golden input named {name}"));
        if input["rust_skip"].as_bool() == Some(true) || input.get("dir_throws").is_some() {
            continue;
        }

        let fs = virtual_fs(input);
        let entry = entry_of(&input["entry"]);
        let result = RecoveryFileCheck::new(fs.clone()).check(&entry);
        for difference in differences(&result, &entry, expected) {
            failures.push(format!("{name}: {difference}"));
        }
        if let Some(stat_calls) = expected["stat_calls"].as_u64() {
            let got = fs.count(FsOp::Stat) as u64;
            if got != stat_calls {
                failures.push(format!("{name}: stat_calls got {got} want {stat_calls}"));
            }
        }
        ran += 1;
    }

    assert!(
        failures.is_empty(),
        "{} differences in {ran} virtual cases:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// `recovery-check`, `mode: physical` cases (20 in the oracle): real files in a private temp folder, `{root}` substituted.
/// The oracle's `stat_calls` is `null` here, so only the verdict and the path checks are compared.
#[test]
#[ignore = "native: real disk"]
fn golden_recovery_check_physical_cases_match_oracle() {
    let oracle = golden::load("recovery-check");
    let inputs = input_cases("recovery-check");
    let mut failures = Vec::new();
    let mut ran = 0_usize;

    for expected in oracle.cases.iter().filter(|case| str_of(case, "mode") == "physical") {
        let name = str_of(expected, "name");
        let root = TempRoot::new("recovery-check");
        let root_text = root.path().display().to_string();
        let input = expand(
            inputs
                .get(name)
                .unwrap_or_else(|| panic!("no golden input named {name}")),
            &root_text,
        );
        let expected = expand(expected, &root_text);
        materialise(&input);

        let fs: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem);
        let entry = entry_of(&input["entry"]);
        let result = RecoveryFileCheck::new(fs).check(&entry);
        for difference in differences(&result, &entry, &expected) {
            failures.push(format!("{name}: {difference}"));
        }
        ran += 1;
    }

    assert!(
        failures.is_empty(),
        "{} differences in {ran} physical cases:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Rust-only: `RecoveryFileCheck` holds an `Arc<dyn FileSystem>` (no `Debug`), so its `Debug` is written by hand; the
/// Recovery presenter logs it. Kills the `fmt -> Ok(Default::default())` mutant (empty output).
#[test]
fn debug_recovery_file_check_names_the_type() {
    let check = RecoveryFileCheck::new(Arc::new(FakeFileSystem::new()));
    assert_eq!(format!("{check:?}"), "RecoveryFileCheck { .. }");
}

/// A Move group (`g1`) whose own size/last-write are `size`/`last_write`, with ONE member `C:\p\b.jpg` (7 bytes, stamped
/// `MEMBER_STAMP`) moved to `D:\k\b.jpg`.
const MEMBER_STAMP: i64 = 639_264_384_000_000_000;

fn group_with_one_member(size: i64, last_write: i64) -> JournalEntry {
    JournalEntry {
        id: "g1".to_owned(),
        op_type: FileOperationType::Move,
        state: JournalState::Failed,
        source: r"C:\p\a.jpg".to_owned(),
        destination: Some(r"D:\k\a.jpg".to_owned()),
        size,
        last_write_utc: JournalDateTime::utc(at(last_write)),
        timestamp_utc: JournalDateTime::utc(at(FILE_STAMP)),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: Some("g".to_owned()),
        group_members: Some(vec![JournalGroupMember {
            source: r"C:\p\b.jpg".to_owned(),
            destination: Some(r"D:\k\b.jpg".to_owned()),
            size: 7,
            last_write_utc: JournalDateTime::utc(at(MEMBER_STAMP)),
            permanent: false,
        }]),
    }
}

// C#: RecoveryFileCheck.CheckGroupMember builds `entry with { Size = member.Size, LastWriteUtc = member.LastWriteUtc, ... }`,
// so a member is fingerprinted with ITS size and last-write, never the group's. Kills the mutants that drop `size` /
// `last_write_utc` from the synthesized entry (the group's 10 bytes, or the group's stamp, would make the member Changed).
#[test]
fn group_member_uses_member_size_and_last_write_not_the_group_ones() {
    let fs = FakeFileSystem::new();
    fs.add_file_at(r"C:\p\b.jpg", &bytes(7), at(MEMBER_STAMP));
    let fs = Arc::new(fs);
    for (size, last_write) in [(10, MEMBER_STAMP), (7, FILE_STAMP)] {
        let result = check(Arc::clone(&fs), &group_with_one_member(size, last_write));
        let members = result.group_members.as_ref().expect("a group");
        assert_eq!(
            members[0].check.source.status,
            RecoveryPathStatus::Exists,
            "group size {size}, last-write {last_write}"
        );
        assert_eq!(
            result.verdict,
            RecoveryVerdict::CanRetry,
            "group size {size}, last-write {last_write}"
        );
    }
}

// C#: RecoveryFileCheck.CheckGroupMember's synthesized entry has `GroupId = null, GroupMembers = null, Undo = null` and the
// member's `Permanent`; it is visible as `RecoveryGroupMemberCheck.Check.Entry`. Kills the mutants that drop `group_id: None`
// / `undo: None` (an undo group would leak `Some("g")` / `Some(true)` into every member's entry).
#[test]
fn group_member_check_entry_clears_group_id_and_undo() {
    let mut group = group_with_one_member(7, MEMBER_STAMP);
    group.undo = Some(true);
    let result = check(Arc::new(FakeFileSystem::new()), &group);
    let member_entry = &result.group_members.as_ref().expect("a group")[0].check.entry;
    assert_eq!(member_entry.group_id, None);
    assert_eq!(member_entry.undo, None);
    assert_eq!(member_entry.group_members, None);
    assert_eq!(member_entry.permanent, Some(false));
    assert_eq!(member_entry.source, r"C:\p\b.jpg");
    assert_eq!(member_entry.destination.as_deref(), Some(r"D:\k\b.jpg"));
}

/// Removes the deny ACEs this test added (before `TempRoot` deletes the folder).
#[cfg(windows)]
struct DenyAces {
    sid: String,
    paths: Vec<PathBuf>,
}

#[cfg(windows)]
impl Drop for DenyAces {
    fn drop(&mut self) {
        for path in self.paths.iter().rev() {
            let _ = std::process::Command::new("icacls")
                .arg(path)
                .arg("/remove:d")
                .arg(format!("*{}", self.sid))
                .output();
        }
    }
}

#[cfg(windows)]
fn icacls_deny(path: &Path, sid: &str, rights: &str) {
    let output = std::process::Command::new("icacls")
        .arg(path)
        .arg("/deny")
        .arg(format!("*{sid}:({rights})"))
        .output()
        .expect("icacls runs");
    assert!(
        output.status.success(),
        "icacls /deny {rights} on {}: {output:?}",
        path.display()
    );
}

/// The current user's SID (`whoami /user /fo csv /nh` => `"domain\user","S-1-5-..."`).
#[cfg(windows)]
fn current_user_sid() -> String {
    let output = std::process::Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .expect("whoami runs");
    let text = String::from_utf8_lossy(&output.stdout);
    let sid = text
        .trim()
        .rsplit(',')
        .next()
        .unwrap_or_default()
        .trim_matches('"')
        .to_owned();
    assert!(sid.starts_with("S-1-"), "whoami /user: {text}");
    sid
}

// C# oracle (verifier probe on the d92d6aae PhotoReview.Core.dll, same setup): a file whose folder listing (RD) and own
// read-attributes (RA) rights are denied has PhysicalFileSystem.GetFileStat == null (`FileInfo.Exists` swallows the
// error), so RecoveryFileCheck reports the source Missing (folder true) and the verdict Lost, not Unreadable/Unknown.
// Touches only its own TempRoot; the deny ACEs are removed before the folder is deleted.
#[cfg(windows)]
#[test]
#[ignore = "native: real disk (denies rights on its own temp folder)"]
fn physical_access_denied_source_is_missing_and_lost_as_in_csharp() {
    let root = TempRoot::new("recovery-check-denied");
    let folder = root.join("locked");
    std::fs::create_dir_all(&folder).expect("create the test folder");
    let file = folder.join("a.jpg");
    std::fs::write(&file, bytes(10)).expect("write the test file");
    let sid = current_user_sid();
    let _aces = DenyAces {
        sid: sid.clone(),
        paths: vec![file.clone(), folder.clone()],
    };
    icacls_deny(&file, &sid, "RA");
    icacls_deny(&folder, &sid, "RD");
    assert!(
        std::fs::symlink_metadata(&file).is_err(),
        "precondition: the file's metadata must be unreadable (running with backup privileges?)"
    );

    let source = file.display().to_string();
    let destination = format!("{source}.nope-dest");
    let fs: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem);
    let entry = JournalEntry {
        source: source.clone(),
        ..plain_entry(
            FileOperationType::Move,
            JournalState::Prepared,
            Some(&destination),
            FILE_STAMP,
        )
    };
    assert_eq!(
        fs.try_get_file_stat(Path::new(&source)).ok(),
        Some(None),
        "C# GetFileStat returns null"
    );
    let result = RecoveryFileCheck::new(fs).check(&entry);
    assert_eq!(result.source.status, RecoveryPathStatus::Missing);
    assert!(result.source.folder_exists);
    assert_eq!(result.verdict, RecoveryVerdict::Lost);
}
