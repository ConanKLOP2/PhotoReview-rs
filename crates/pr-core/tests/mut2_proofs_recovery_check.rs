//! N3 run-proofs for the three `file_actions/recovery_check.rs` mutants that M4 only argued equivalent by reading the code:
//! 327:24 (guard of the `Exists` arm to `true`), 328:25 (`&&` to `||` in that guard) and 412:26 (`Move` `==` to `!=` in rule 1 of
//! `aggregate_group_verdict`). Each test is a differential test against an independent model over the complete reachable
//! domain; it passes on the unmutated code AND under the named mutant. Hand-written, no golden data, no production change.
use std::collections::BTreeSet;
use std::sync::Arc;

use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{RecoveryFileCheck, RecoveryPathStatus, RecoveryVerdict};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_test_support::FakeFileSystem;

const STAMP: i64 = 639_264_384_000_000_000;
const SIZE: i64 = 10;

fn at(ticks: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(ticks).expect("ticks in the .NET DateTime range")
}

fn group_entry(
    op: FileOperationType,
    state: JournalState,
    undo: Option<bool>,
    members: Vec<JournalGroupMember>,
) -> JournalEntry {
    JournalEntry {
        id: "group".to_owned(),
        op_type: op,
        state,
        source: r"C:\photos\group.jpg".to_owned(),
        destination: None,
        size: 0,
        last_write_utc: JournalDateTime::utc(at(STAMP)),
        timestamp_utc: JournalDateTime::utc(at(STAMP)),
        error: None,
        error_code: None,
        permanent: None,
        undo,
        group_id: Some("capture".to_owned()),
        group_members: Some(members),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Src {
    Exists,
    OtherSize,
    OtherTime,
    Missing,
    Unreadable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Dst {
    NoDestination,
    Exists,
    OtherSize,
    Missing,
    Unreadable,
}

const SRCS: [Src; 5] = [
    Src::Exists,
    Src::OtherSize,
    Src::OtherTime,
    Src::Missing,
    Src::Unreadable,
];
const DSTS: [Dst; 5] = [
    Dst::NoDestination,
    Dst::Exists,
    Dst::OtherSize,
    Dst::Missing,
    Dst::Unreadable,
];

#[derive(Clone, Copy, Debug)]
struct World {
    src: Src,
    dst: Dst,
    permanent: bool,
}

/// Builds member `index` of the world into `fs` and returns the journal member (recorded size 10, recorded write time STAMP).
fn add_member(fs: &FakeFileSystem, index: usize, w: World) -> JournalGroupMember {
    let source = match w.src {
        Src::Unreadable => format!(r"  \m{index}.jpg"),
        _ => format!(r"C:\photos\m{index}.jpg"),
    };
    match w.src {
        Src::Exists => fs.add_file_at(&source, &[0u8; 10], at(STAMP)),
        Src::OtherSize => fs.add_file_at(&source, &[0u8; 11], at(STAMP)),
        Src::OtherTime => fs.add_file_at(&source, &[0u8; 10], at(STAMP + 1)),
        Src::Missing | Src::Unreadable => {}
    }
    let destination = match w.dst {
        Dst::NoDestination => None,
        Dst::Unreadable => Some(format!(r"  \d{index}.jpg")),
        _ => Some(format!(r"D:\keep\d{index}.jpg")),
    };
    match (&destination, w.dst) {
        (Some(path), Dst::Exists) => fs.add_file_at(path, &[0u8; 10], at(STAMP)),
        (Some(path), Dst::OtherSize) => fs.add_file_at(path, &[0u8; 11], at(STAMP)),
        _ => {}
    }
    JournalGroupMember {
        source,
        destination,
        size: SIZE,
        last_write_utc: JournalDateTime::utc(at(STAMP)),
        permanent: w.permanent,
    }
}

type Observed = (RecoveryVerdict, RecoveryPathStatus);

/// Checks the group of `worlds` and returns the group verdict and, per member, its verdict and source status.
fn check_group(
    op: FileOperationType,
    state: JournalState,
    undo: Option<bool>,
    worlds: &[World],
) -> (RecoveryVerdict, Vec<Observed>) {
    let fs = Arc::new(FakeFileSystem::new());
    let members: Vec<JournalGroupMember> = worlds.iter().enumerate().map(|(i, w)| add_member(&fs, i, *w)).collect();
    let result = RecoveryFileCheck::new(fs).check(&group_entry(op, state, undo, members));
    let observed = result
        .group_members
        .expect("a group result has members")
        .iter()
        .map(|m| (m.verdict(), m.source().status))
        .collect();
    (result.verdict, observed)
}

/// Independent model of `AggregateGroupVerdict` from its documentation, WITHOUT rule 1 (rule 5 subsumes it): the group
/// verdict from the op, the undo flag and the member verdicts / source statuses.
fn model(op: FileOperationType, undo: Option<bool>, members: &[Observed]) -> RecoveryVerdict {
    use RecoveryVerdict as V;
    let undo = undo == Some(true);
    let all = |set: &[V]| members.iter().all(|(v, _)| set.contains(v));
    let has = |v: V| members.iter().any(|(m, _)| *m == v);
    if undo
        && op == FileOperationType::Recycle
        && all(&[V::RecycleUnverifiable, V::AlreadyDone])
        && has(V::RecycleUnverifiable)
    {
        return V::CanRetry;
    }
    if op == FileOperationType::Recycle
        && !undo
        && all(&[V::NotRecycled, V::RecycleUnverifiable, V::PermanentlyDeleted])
        && has(V::NotRecycled)
    {
        let every_present_one_unchanged = members
            .iter()
            .filter(|(v, _)| *v == V::NotRecycled)
            .all(|(_, s)| *s == RecoveryPathStatus::Exists);
        return if every_present_one_unchanged {
            V::CanRetry
        } else {
            V::NotRecycled
        };
    }
    if all(&[V::AlreadyDone, V::RecycleUnverifiable, V::PermanentlyDeleted]) {
        return if all(&[V::PermanentlyDeleted]) {
            V::PermanentlyDeleted
        } else if has(V::PermanentlyDeleted) {
            V::PartiallyPermanentlyDeleted
        } else if op == FileOperationType::Recycle && !undo {
            V::RecycleUnverifiable
        } else {
            V::AlreadyDone
        };
    }
    if all(&[V::AlreadyDone, V::CanRetry]) && has(V::CanRetry) {
        return V::CanRetry;
    }
    [
        V::Conflict,
        V::SourceChanged,
        V::DestinationChanged,
        V::Lost,
        V::NotRecycled,
    ]
    .into_iter()
    .find(|v| has(*v))
    .unwrap_or(V::Unknown)
}

#[test]
fn group_verdict_equals_the_model_without_rule_1_for_every_op_state_undo_and_ordered_member_triple() {
    // 412:26 (`op_type == Move` to `!=` in rule 1): rule 1 and rule 5 return the same CanRetry for an undo group of
    // {CanRetry, AlreadyDone} members, whatever the op, so a model with no rule 1 must agree with the function. Domain: the 3 ops x
    // 4 journal states x 3 undo values; for each, every DISTINCT (verdict, source status) a member can reach (from 50 member worlds:
    // 5 source x 5 destination states x permanent) and every ordered group of 1, 2 or 3 such members.
    let mut groups = 0usize;
    let mut rule_1_domain_move = 0usize;
    let mut rule_1_domain_other = 0usize;
    let mut verdicts_seen = BTreeSet::new();
    for op in [
        FileOperationType::Move,
        FileOperationType::Copy,
        FileOperationType::Recycle,
    ] {
        for state in [
            JournalState::Prepared,
            JournalState::Failed,
            JournalState::Committed,
            JournalState::Dismissed,
        ] {
            for undo in [None, Some(false), Some(true)] {
                // One representative world per distinct member observation.
                let mut reps: Vec<(Observed, World)> = Vec::new();
                for src in SRCS {
                    for dst in DSTS {
                        for permanent in [false, true] {
                            let world = World { src, dst, permanent };
                            let (_, observed) = check_group(op, state, undo, &[world]);
                            if !reps.iter().any(|(o, _)| *o == observed[0]) {
                                reps.push((observed[0], world));
                            }
                        }
                    }
                }
                for (o, _) in &reps {
                    verdicts_seen.insert(format!("{:?}", o.0));
                }
                let worlds: Vec<World> = reps.iter().map(|(_, w)| *w).collect();
                let mut check = |picked: &[World]| {
                    let (actual, observed) = check_group(op, state, undo, picked);
                    let expected = model(op, undo, &observed);
                    assert_eq!(
                        actual, expected,
                        "{op:?} {state:?} undo {undo:?} members {observed:?} worlds {picked:?}"
                    );
                    groups += 1;
                    let all_retry_or_done = observed
                        .iter()
                        .all(|(v, _)| matches!(v, RecoveryVerdict::CanRetry | RecoveryVerdict::AlreadyDone));
                    if undo == Some(true)
                        && all_retry_or_done
                        && observed.iter().any(|(v, _)| *v == RecoveryVerdict::CanRetry)
                    {
                        if op == FileOperationType::Move {
                            rule_1_domain_move += 1;
                        } else {
                            rule_1_domain_other += 1;
                        }
                        assert_eq!(actual, RecoveryVerdict::CanRetry);
                    }
                };
                for a in &worlds {
                    check(&[*a]);
                    for b in &worlds {
                        check(&[*a, *b]);
                        for c in &worlds {
                            check(&[*a, *b, *c]);
                        }
                    }
                }
            }
        }
    }
    assert!(groups > 20_000, "{groups}");
    // The domain where rule 1 fires (undo, all CanRetry/AlreadyDone, one CanRetry) is reached for Move; for the other ops it is the
    // domain where the mutant `!=` would fire instead. The second count is what the run shows about it (see the evidence file).
    assert!(
        rule_1_domain_move > 0,
        "the Move undo group of CanRetry/AlreadyDone members was never reached"
    );
    println!(
        "groups {groups}; undo CanRetry/AlreadyDone groups: Move {rule_1_domain_move}, other ops {rule_1_domain_other}; verdicts {verdicts_seen:?}"
    );
}

#[test]
fn undo_recycle_member_verdict_is_decided_by_status_alone_and_never_conflict() {
    // 327:24 (guard to `true`) and 328:25 (`&&` to `||`): for an undo Recycle group the member verdict is AlreadyDone for source
    // `Exists`, `Unknown` for `Changed`/`Unreadable`, `RecycleUnverifiable` for `Missing`; `Exists` already implies equal size
    // and equal write time (the source check compares both), so the `Conflict` arm is dead. Domain: every source state, size
    // {9, 10, 11} and write time {STAMP-1, STAMP, STAMP+1} of the file, permanent on/off, one and two members.
    let mut exists_cases = 0usize;
    let mut changed_cases = 0usize;
    for size in [9usize, 10, 11] {
        for time in [STAMP - 1, STAMP, STAMP + 1] {
            for present in [true, false] {
                for permanent in [false, true] {
                    for unreadable in [false, true] {
                        for second in [false, true] {
                            let fs = Arc::new(FakeFileSystem::new());
                            let path = if unreadable { r"  \a.jpg" } else { r"C:\photos\a.jpg" };
                            if present && !unreadable {
                                fs.add_file_at(path, &vec![0u8; size], at(time));
                            }
                            let member = |source: &str| JournalGroupMember {
                                source: source.to_owned(),
                                destination: None,
                                size: SIZE,
                                last_write_utc: JournalDateTime::utc(at(STAMP)),
                                permanent,
                            };
                            let mut members = vec![member(path)];
                            if second {
                                fs.add_file_at(r"C:\photos\b.jpg", &[0u8; 10], at(STAMP));
                                members.push(member(r"C:\photos\b.jpg"));
                            }
                            let entry =
                                group_entry(FileOperationType::Recycle, JournalState::Prepared, Some(true), members);
                            let result = RecoveryFileCheck::new(fs).check(&entry);
                            let checks = result.group_members.expect("members");
                            let first = &checks[0];
                            let expected = if unreadable {
                                RecoveryVerdict::Unknown
                            } else if !present {
                                RecoveryVerdict::RecycleUnverifiable
                            } else if size == 10 && time == STAMP {
                                exists_cases += 1;
                                RecoveryVerdict::AlreadyDone
                            } else {
                                changed_cases += 1;
                                RecoveryVerdict::Unknown
                            };
                            assert_eq!(
                                first.verdict(),
                                expected,
                                "size {size} time {time} present {present} unreadable {unreadable} permanent {permanent}"
                            );
                            for check in &checks {
                                assert_ne!(check.verdict(), RecoveryVerdict::Conflict);
                            }
                            if present && !unreadable {
                                let status = first.source().status;
                                assert_eq!(
                                    status == RecoveryPathStatus::Exists,
                                    size == 10 && time == STAMP,
                                    "Exists must mean an exact size and write time match"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(exists_cases > 0 && changed_cases > 0);
}
