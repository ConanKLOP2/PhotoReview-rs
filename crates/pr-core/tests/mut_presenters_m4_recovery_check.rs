//! M4 mutation killers for `file_actions/recovery_check.rs` (F-17). Hand-written, no golden data.
//! C# sources: `RecoveryFileCheck.Check` / `CheckPath` / `RecoveryGroupMemberCheck.Destination` at the baseline of
//! docs/SYNC-LEDGER.md.

use std::sync::Arc;

use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{RecoveryFileCheck, RecoveryPathStatus};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_test_support::FakeFileSystem;

const STAMP: i64 = 639_264_384_000_000_000;

fn at(ticks: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(ticks).expect("ticks in the .NET DateTime range")
}

fn entry(destination: Option<&str>) -> JournalEntry {
    JournalEntry {
        id: "id1".to_owned(),
        op_type: FileOperationType::Move,
        state: JournalState::Failed,
        source: r"C:\photos\a.jpg".to_owned(),
        destination: destination.map(str::to_owned),
        size: 10,
        last_write_utc: JournalDateTime::utc(at(STAMP)),
        timestamp_utc: JournalDateTime::utc(at(STAMP)),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

// C# `RecoveryGroupMemberCheck.Destination` is `Check.Destination`: the member's destination check, not null.
#[test]
fn group_member_destination_is_the_destination_check_of_the_member() {
    let fs = Arc::new(FakeFileSystem::new());
    fs.add_file_at(r"C:\photos\a.jpg", &[0u8; 10], at(STAMP));
    fs.add_dir(r"D:\keep");
    let mut group = entry(None);
    group.group_id = Some("capture".to_owned());
    group.group_members = Some(vec![JournalGroupMember {
        source: r"C:\photos\a.jpg".to_owned(),
        destination: Some(r"D:\keep\a.jpg".to_owned()),
        size: 10,
        last_write_utc: JournalDateTime::utc(at(STAMP)),
        permanent: false,
    }]);

    let result = RecoveryFileCheck::new(fs).check(&group);
    let members = result.group_members.expect("a group result has members");
    let destination = members[0].destination().expect("the member has a destination");

    assert_eq!(destination.path, r"D:\keep\a.jpg");
    assert_eq!(destination.status, RecoveryPathStatus::Missing);
    assert!(destination.folder_exists);
    // A member without a destination reports none.
    group.group_members.as_mut().unwrap()[0].destination = None;
    let fs = Arc::new(FakeFileSystem::new());
    let result = RecoveryFileCheck::new(fs).check(&group);
    assert!(result.group_members.unwrap()[0].destination().is_none());
}

// C# `string.IsNullOrWhiteSpace(Destination)`: a blank destination gives no destination check (None), not a path check.
#[test]
fn whitespace_only_destination_gives_no_destination_check() {
    let fs = Arc::new(FakeFileSystem::new());
    fs.add_file_at(r"C:\photos\a.jpg", &[0u8; 10], at(STAMP));
    let result = RecoveryFileCheck::new(fs).check(&entry(Some("   ")));
    assert!(result.destination.is_none(), "a blank destination is no destination");
}

// C# `CheckPath`: a whitespace-only folder part is Unreadable before any stat (`Directory.Exists` throws for it).
#[test]
fn whitespace_only_folder_part_is_unreadable_before_any_stat() {
    let fs = Arc::new(FakeFileSystem::new());
    let mut e = entry(None);
    e.source = r"  \a.jpg".to_owned();
    let result = RecoveryFileCheck::new(fs).check(&e);
    assert_eq!(result.source.status, RecoveryPathStatus::Unreadable);
    assert!(!result.source.folder_exists);
    assert!(result.source.current_size.is_none());
}
