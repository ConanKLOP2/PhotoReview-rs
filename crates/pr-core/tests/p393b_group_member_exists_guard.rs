//! P-393b (2026-11-06): proof that the size / last-write guard on the `Exists` arm of the undo-Recycle branch of
//! `RecoveryFileCheck::check_group_member` is redundant. A source status of `Exists` is only produced by `check_path` when the
//! stat length equals the recorded size and the stat last-write equals the recorded one, so the `Conflict` arm below the guard
//! can never be taken and the verdict is a function of the source status alone.
//!
//! The test drives the public `RecoveryFileCheck::check` of an undo Recycle group with a stub file system that returns a chosen
//! stat (any `u64` length, any valid last-write ticks), over the cross product of recorded size x recorded write time x stat
//! length x stat write time, and compares the member verdict with an independent model that has no guard
//! (`Exists` => `AlreadyDone`, `Missing` => `RecycleUnverifiable`, `Changed`/`Unreadable` => `Unknown`) while the model of the
//! status itself is the spec of `check_path` (equal size AND equal write time). It must pass before and after the guard is removed.
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_core::abstractions::{AppendStream, FileStat, FileSystem, ReadSeek};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{RecoveryFileCheck, RecoveryPathStatus, RecoveryVerdict};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;

const MAX_TICKS: i64 = 3_155_378_975_999_999_999;
const STAMP: i64 = 639_264_384_000_000_000;

/// What the stub reports for every stat call.
#[derive(Clone, Copy)]
enum Stat {
    Missing,
    Unreadable,
    File(FileStat),
}

struct StubFs {
    stat: Stat,
}

impl FileSystem for StubFs {
    fn file_exists(&self, _: &Path) -> bool {
        unimplemented!("not used by the recovery check")
    }
    fn directory_exists(&self, _: &Path) -> bool {
        true
    }
    fn get_file_stat(&self, _: &Path) -> Option<FileStat> {
        match self.stat {
            Stat::File(stat) => Some(stat),
            Stat::Missing | Stat::Unreadable => None,
        }
    }
    fn try_get_file_stat(&self, _: &Path) -> io::Result<Option<FileStat>> {
        match self.stat {
            Stat::File(stat) => Ok(Some(stat)),
            Stat::Missing => Ok(None),
            Stat::Unreadable => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        }
    }
    fn move_file(&self, _: &Path, _: &Path) -> io::Result<()> {
        unimplemented!("not used by the recovery check")
    }
    fn copy_file(&self, _: &Path, _: &Path) -> io::Result<()> {
        unimplemented!("not used by the recovery check")
    }
    fn delete(&self, _: &Path) -> io::Result<()> {
        unimplemented!("not used by the recovery check")
    }
    fn open_read_shared(&self, _: &Path, _: usize) -> io::Result<Box<dyn ReadSeek>> {
        unimplemented!("not used by the recovery check")
    }
    fn open_append(&self, _: &Path, _: bool) -> io::Result<Box<dyn AppendStream>> {
        unimplemented!("not used by the recovery check")
    }
    fn write_all_text_atomic(&self, _: &Path, _: &str, _: bool) -> io::Result<()> {
        unimplemented!("not used by the recovery check")
    }
    fn read_all_text(&self, _: &Path) -> io::Result<String> {
        unimplemented!("not used by the recovery check")
    }
    fn enumerate_files(&self, _: &Path, _: &str) -> io::Result<Vec<PathBuf>> {
        unimplemented!("not used by the recovery check")
    }
    fn enumerate_directories(&self, _: &Path) -> io::Result<Vec<PathBuf>> {
        unimplemented!("not used by the recovery check")
    }
    fn create_directory(&self, _: &Path) -> io::Result<()> {
        unimplemented!("not used by the recovery check")
    }
}

fn at(ticks: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(ticks).expect("ticks in the .NET DateTime range")
}

fn group(recorded_size: i64, recorded_ticks: i64, permanent: bool) -> JournalEntry {
    JournalEntry {
        id: "group".to_owned(),
        op_type: FileOperationType::Recycle,
        state: JournalState::Prepared,
        source: r"C:\photos\group.jpg".to_owned(),
        destination: None,
        size: 0,
        last_write_utc: JournalDateTime::utc(at(STAMP)),
        timestamp_utc: JournalDateTime::utc(at(STAMP)),
        error: None,
        error_code: None,
        permanent: None,
        undo: Some(true),
        group_id: Some("capture".to_owned()),
        group_members: Some(vec![JournalGroupMember {
            source: r"C:\photos\a.jpg".to_owned(),
            destination: None,
            size: recorded_size,
            last_write_utc: JournalDateTime::utc(at(recorded_ticks)),
            permanent,
        }]),
    }
}

#[test]
fn undo_recycle_member_verdict_depends_only_on_the_source_status_for_every_size_and_write_time() {
    let sizes: [i64; 9] = [i64::MIN, -1, 0, 1, 10, 11, i64::MAX - 1, i64::MAX, 4096];
    let ticks: [i64; 7] = [0, 1, STAMP - 1, STAMP, STAMP + 1, MAX_TICKS - 1, MAX_TICKS];
    let lengths: [u64; 11] = [
        0,
        1,
        9,
        10,
        11,
        4096,
        i64::MAX as u64 - 1,
        i64::MAX as u64,
        i64::MAX as u64 + 1,
        u64::MAX - 1,
        u64::MAX,
    ];
    let mut cases = 0usize;
    let (mut exists, mut changed, mut missing, mut unreadable) = (0usize, 0usize, 0usize, 0usize);
    for recorded_size in sizes {
        for recorded_ticks in ticks {
            for permanent in [false, true] {
                // Stat outcomes: nothing, an inspection error, and every (length, write time) pair.
                let mut stats = vec![Stat::Missing, Stat::Unreadable];
                for length in lengths {
                    for stat_ticks in ticks {
                        stats.push(Stat::File(FileStat {
                            length,
                            last_write_utc: at(stat_ticks),
                        }));
                    }
                }
                // Exact matches for this recorded size, when it is a valid `u64` length.
                if let Ok(length) = u64::try_from(recorded_size) {
                    stats.push(Stat::File(FileStat {
                        length,
                        last_write_utc: at(recorded_ticks),
                    }));
                }
                for stat in stats {
                    let entry = group(recorded_size, recorded_ticks, permanent);
                    let result = RecoveryFileCheck::new(Arc::new(StubFs { stat })).check(&entry);
                    let members = result.group_members.expect("a group result has members");
                    assert_eq!(members.len(), 1);
                    let status = members[0].source().status;
                    // Spec of `check_path` for the source (size AND last-write compared).
                    let expected_status = match stat {
                        Stat::Missing => RecoveryPathStatus::Missing,
                        Stat::Unreadable => RecoveryPathStatus::Unreadable,
                        Stat::File(f) => {
                            if i64::try_from(f.length) == Ok(recorded_size) && f.last_write_utc == at(recorded_ticks) {
                                RecoveryPathStatus::Exists
                            } else {
                                RecoveryPathStatus::Changed
                            }
                        }
                    };
                    assert_eq!(status, expected_status, "size {recorded_size} ticks {recorded_ticks}");
                    // The model with no guard: the verdict is a function of the status alone.
                    let expected_verdict = match status {
                        RecoveryPathStatus::Exists => {
                            exists += 1;
                            RecoveryVerdict::AlreadyDone
                        }
                        RecoveryPathStatus::Missing => {
                            missing += 1;
                            RecoveryVerdict::RecycleUnverifiable
                        }
                        RecoveryPathStatus::Changed => {
                            changed += 1;
                            RecoveryVerdict::Unknown
                        }
                        RecoveryPathStatus::Unreadable => {
                            unreadable += 1;
                            RecoveryVerdict::Unknown
                        }
                    };
                    assert_eq!(
                        members[0].verdict(),
                        expected_verdict,
                        "size {recorded_size} ticks {recorded_ticks} permanent {permanent} status {status:?}"
                    );
                    assert_ne!(members[0].verdict(), RecoveryVerdict::Conflict);
                    // The group entry replaces the member entry; the group verdict is the plain aggregate of one member.
                    assert_eq!(members[0].check.entry, entry);
                    cases += 1;
                }
            }
        }
    }
    assert!(cases > 10_000, "{cases}");
    assert!(exists > 100 && changed > 1000 && missing > 100 && unreadable > 100);
}
