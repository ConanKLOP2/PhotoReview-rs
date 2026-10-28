//! W9-R9 rename fault-injection tests, C# baseline 7eaee2a2.
//!
//! Inventory: 10 C# matrix methods have dedicated tests here: Rename_Matrix, Rename_ToTakenName_Matrix,
//! CaseOnlyRename_Matrix, GroupRename_Matrix, RenameThenUndo_Matrix,
//! CaseOnlyRenameThenUndo_Matrix, GroupRenameThenUndo_Matrix, Rename_ReconcileInterrupted,
//! GroupRename_ReconcileInterrupted, and TwoRenames_Matrix.
//! The two R3 scenarios GroupCaseOnlyRename_Matrix and CaseOnlyRename_ReconcileInterrupted remain in
//! `file_actions_matrix.rs` and are deliberately excluded here. Same-folder moves use `rename` faults, excluding
//! cross-volume partial-copy fault kinds.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_core::abstractions::{FileSystem, NullLog};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::journal::{OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::JournalEntry;
use pr_core::file_actions::recovery_retry::{EnglishRecoveryRetryTexts, RecoveryRetryService};
use pr_core::file_actions::rename::RenameRequest;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::types::{CaptureGroupActionResult, FileActionResult, UndoResult};
use pr_core::file_actions::undo::{EnglishUndoTexts, UndoService};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use pr_test_support::{FakeClock, FakeFileSystem, FaultInjectionFileSystem, FaultKind, FaultPlan, TempRoot};

const NAMES: [&str; 3] = ["a.jpg", "a.cr2", "a.xmp"];
const BYTES: [&[u8]; 3] = [b"unique-jpeg-data", b"unique-raw-data", b"unique-sidecar-data"];
const KINDS: [FaultKind; 7] = [
    FaultKind::CrashBefore,
    FaultKind::CrashAfter,
    FaultKind::CrashPartial,
    FaultKind::CrashMoveCopiedNotDeleted,
    FaultKind::FailOnce,
    FaultKind::FailPartial,
    FaultKind::FailUnauthorized,
];

struct Rig {
    _root: TempRoot,
    folder: PathBuf,
    member_count: usize,
    disk: Arc<FakeFileSystem>,
    fs: Arc<FaultInjectionFileSystem>,
    bin: Arc<pr_test_support::FakeRecycleBin>,
    clock: Arc<FakeClock>,
    journal: Arc<OperationJournal>,
    service: Arc<FileActionService>,
    undo: UndoService,
    outcomes: Vec<Outcome>,
}

#[derive(Debug)]
enum Outcome {
    Action(FileActionResult),
    Group(CaptureGroupActionResult),
    Undo(UndoResult),
}

impl Rig {
    fn new(group: bool) -> Self {
        let root = TempRoot::new("rename-fault-matrix");
        let folder = root.path().join("photos");
        std::fs::create_dir_all(&folder).expect("create test-owned folder");
        let disk = Arc::new(FakeFileSystem::new());
        let fs = Arc::new(FaultInjectionFileSystem::new(disk.clone()));
        for i in 0..if group { 3 } else { 1 } {
            disk.add_file(folder.join(NAMES[i]), BYTES[i]);
        }
        disk.add_file(folder.join("b.jpg"), b"foreign-file");
        let clock = Arc::new(FakeClock::new());
        let paths = AppPaths {
            journal_file: root.path().join("journal.jsonl"),
            ..AppPaths::new(root.path(), None, false)
        };
        let journal = Arc::new(OperationJournal::with_options(
            &paths,
            fs.clone(),
            clock.clone(),
            OperationJournalOptions {
                append_retry_delay: Some(Arc::new(|_| {})),
                log: Some(Arc::new(NullLog)),
                ..OperationJournalOptions::default()
            },
        ));
        let bin = Arc::new(pr_test_support::FakeRecycleBin::new(disk.clone()));
        let service = Arc::new(FileActionService::new(
            journal.clone(),
            fs.clone(),
            clock.clone(),
            bin.clone(),
        ));
        let undo = UndoService::new(journal.clone(), fs.clone(), bin.clone(), Arc::new(EnglishUndoTexts))
            .with_gate(service.clone())
            .with_clock(clock.clone());
        Self {
            _root: root,
            folder,
            member_count: if group { 3 } else { 1 },
            disk,
            fs,
            bin,
            clock,
            journal,
            service,
            undo,
            outcomes: Vec::new(),
        }
    }

    fn path(&self, name: &str) -> String {
        self.folder.join(name).to_string_lossy().into_owned()
    }
    fn capture(&self) -> CaptureGroup {
        CaptureGroup::new(self.path("a.jpg"), self.path("a.cr2"), Some(self.path("a.xmp"))).unwrap()
    }
    fn entries(&self) -> Vec<JournalEntry> {
        let path = self._root.path().join("journal.jsonl");
        let Some(bytes) = self.disk.file_content(path) else {
            return Vec::new();
        };
        String::from_utf8_lossy(&bytes)
            .split('\n')
            .filter(|line| !line.is_empty())
            .filter_map(|line| pr_core::file_actions::journal_line::try_parse(line.trim_end_matches('\r')))
            .collect()
    }

    fn restart(&mut self) {
        self.fs.arm(None);
        self.journal = Arc::new(OperationJournal::with_options(
            &AppPaths {
                journal_file: self._root.path().join("journal.jsonl"),
                ..AppPaths::new(self._root.path(), None, false)
            },
            self.fs.clone(),
            self.clock.clone(),
            OperationJournalOptions {
                append_retry_delay: Some(Arc::new(|_| {})),
                log: Some(Arc::new(NullLog)),
                ..OperationJournalOptions::default()
            },
        ));
        self.service = Arc::new(FileActionService::new(
            self.journal.clone(),
            self.fs.clone(),
            self.clock.clone(),
            self.bin.clone(),
        ));
        self.undo = UndoService::new(
            self.journal.clone(),
            self.fs.clone(),
            self.bin.clone(),
            Arc::new(EnglishUndoTexts),
        )
        .with_gate(self.service.clone())
        .with_clock(self.clock.clone());
        self.outcomes.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Rename,
    Taken,
    CaseOnly,
    Group,
    RenameUndo,
    CaseUndo,
    GroupUndo,
    TwoRenames,
}

fn action(rig: &mut Rig, case: Scenario) {
    let run_one = |rig: &Rig, from: &str, to: &str| {
        rig.service.rename(
            &RenameRequest {
                source: rig.path(from),
                new_file_name: to.into(),
            },
            &CancelToken::none(),
        )
    };
    match case {
        Scenario::Rename | Scenario::RenameUndo => {
            let out = run_one(rig, "a.jpg", "x.jpg");
            if case == Scenario::RenameUndo {
                rig.undo.register(&out);
                let history_before = rig.undo.move_history().len();
                let undo = rig.undo.undo_move();
                assert!(
                    rig.undo.move_history().len() <= history_before,
                    "UndoMove duplicated an action on the replay stack"
                );
                rig.outcomes.push(Outcome::Undo(undo));
            } else {
                rig.outcomes.push(Outcome::Action(out));
            }
        }
        Scenario::Taken => {
            let out = run_one(rig, "a.jpg", "b.jpg");
            rig.outcomes.push(Outcome::Action(out));
        }
        Scenario::CaseOnly | Scenario::CaseUndo => {
            let out = run_one(rig, "a.jpg", "A.jpg");
            if case == Scenario::CaseUndo {
                rig.undo.register(&out);
                let history_before = rig.undo.move_history().len();
                let undo = rig.undo.undo_move();
                assert!(
                    rig.undo.move_history().len() <= history_before,
                    "case-only UndoMove duplicated an action on the replay stack"
                );
                rig.outcomes.push(Outcome::Undo(undo));
            } else {
                rig.outcomes.push(Outcome::Action(out));
            }
        }
        Scenario::Group | Scenario::GroupUndo => {
            let out = rig.service.rename_group(&rig.capture(), "x", &CancelToken::none());
            if case == Scenario::GroupUndo {
                rig.undo.register_group(&out);
                let undo = rig.undo.undo_last();
                rig.outcomes.push(Outcome::Undo(undo));
            } else {
                rig.outcomes.push(Outcome::Group(out));
            }
        }
        Scenario::TwoRenames => {
            let first = run_one(rig, "a.jpg", "x.jpg");
            if first.succeeded {
                let second = run_one(rig, "x.jpg", "A.jpg");
                rig.outcomes.push(Outcome::Action(second));
            } else {
                rig.outcomes.push(Outcome::Action(first));
            }
        }
    }
}

type JournalSummary = Vec<(
    FileOperationType,
    JournalState,
    Option<bool>,
    Vec<(String, Option<String>)>,
)>;

fn summarize(entries: &[JournalEntry]) -> JournalSummary {
    let mut last = Vec::<&JournalEntry>::new();
    for e in entries {
        if let Some(i) = last.iter().position(|old| old.id == e.id) {
            last[i] = e;
        } else {
            last.push(e);
        }
    }
    let mut summary: JournalSummary = last
        .into_iter()
        .map(|e| {
            let members = e
                .group_members
                .as_ref()
                .map(|g| {
                    g.iter()
                        .map(|m| (leaf(&m.source), m.destination.as_deref().map(leaf)))
                        .collect()
                })
                .unwrap_or_else(|| vec![(leaf(&e.source), e.destination.as_deref().map(leaf))]);
            (e.op_type, e.state, e.undo, members)
        })
        .filter(|(_, state, _, _)| *state != JournalState::Dismissed)
        .collect();
    // C# Summary compares a sorted set of live verdicts; append/Id insertion order is irrelevant.
    summary.sort_by_cached_key(|entry| format!("{entry:?}"));
    summary
}

fn latest_entries(entries: &[JournalEntry]) -> Vec<JournalEntry> {
    let mut latest = Vec::<JournalEntry>::new();
    for entry in entries {
        if let Some(index) = latest.iter().position(|old| old.id == entry.id) {
            latest[index] = entry.clone();
        } else {
            latest.push(entry.clone());
        }
    }
    latest
}

fn leaf(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_owned()
}

const B_CONTENT: &[u8] = b"foreign-file";
const FOLLOWUP_CONTENT: &[u8] = b"FOREIGN-PHOTO-THAT-MUST-SURVIVE";

fn expected_content(path: &str) -> Option<&'static [u8]> {
    match leaf(path).to_ascii_lowercase().as_str() {
        "a.jpg" | "x.jpg" => Some(BYTES[0]),
        "a.cr2" | "x.cr2" => Some(BYTES[1]),
        "a.xmp" | "x.xmp" => Some(BYTES[2]),
        "b.jpg" => Some(B_CONTENT),
        _ => None,
    }
}

fn has_full(rig: &Rig, path: &str) -> bool {
    expected_content(path).is_some_and(|bytes| rig.disk.file_content(path).as_deref() == Some(bytes))
}

fn source_still_there(rig: &Rig, source: &str, destination: Option<&str>) -> bool {
    if !rig.disk.file_exists(Path::new(source)) {
        return false;
    }
    let Some(destination) = destination else { return true };
    if source == destination || !pr_core::text::eq_ignore_case(source, destination) {
        return true;
    }
    let stored = rig
        .disk
        .get_stored_file_names(Path::new(source))
        .expect("stored spelling probe");
    stored.len() != 1 || stored.first().map(String::as_str) != Some(leaf(destination).as_str())
}

fn check_success(rig: &Rig, source: &str, destination: Option<&str>, context: &str) {
    assert!(
        !source_still_there(rig, source, destination),
        "{context}: Move reported success but source remains under its stored spelling"
    );
    let Some(destination) = destination else {
        panic!("{context}: successful Move has no destination")
    };
    assert!(
        has_full(rig, destination),
        "{context}: successful Move destination is absent or contains foreign/truncated content"
    );
}

fn check_outcomes(rig: &Rig, context: &str) {
    for outcome in &rig.outcomes {
        match outcome {
            Outcome::Action(result) if result.no_change => {}
            Outcome::Action(result) if result.succeeded && !result.rejected => {
                check_success(rig, &result.source, result.destination_path.as_deref(), context);
            }
            Outcome::Action(result)
                if !result.succeeded && result.operation == FileOperationType::Move && !rig.fs.dead() =>
            {
                let present = source_still_there(rig, &result.source, result.destination_path.as_deref());
                assert_ne!(
                    result.source_removed, present,
                    "{context}: failed Move SourceRemoved={} disagrees with source presence={present}",
                    result.source_removed
                );
            }
            Outcome::Action(_) => {}
            Outcome::Undo(result) => {
                let paths = result.restored_paths.clone().unwrap_or_else(|| {
                    if result.succeeded {
                        vec![result.source.clone()]
                    } else {
                        Vec::new()
                    }
                });
                for path in paths {
                    assert!(
                        has_full(rig, &path),
                        "{context}: Undo reported {path} restored but it is not the complete original payload"
                    );
                }
            }
            Outcome::Group(result) => {
                for member in result
                    .members
                    .iter()
                    .filter(|m| m.state_known && (m.completed || result.succeeded))
                {
                    check_success(
                        rig,
                        &member.member.source,
                        member.member.destination.as_deref(),
                        context,
                    );
                }
            }
        }
    }
}

fn journal_members(entry: &JournalEntry) -> Vec<(&str, Option<&str>, i64, pr_core::time::DateTimeUtc)> {
    entry
        .group_members
        .as_ref()
        .filter(|items| !items.is_empty())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    (
                        item.source.as_str(),
                        item.destination.as_deref(),
                        item.size,
                        item.last_write_utc.value,
                    )
                })
                .collect()
        })
        .unwrap_or_else(|| {
            vec![(
                entry.source.as_str(),
                entry.destination.as_deref(),
                entry.size,
                entry.last_write_utc.value,
            )]
        })
}

fn later_move_moved_on(
    rig: &Rig,
    lines: &[JournalEntry],
    latest: &[JournalEntry],
    entry: &JournalEntry,
    destination: &str,
) -> bool {
    let Some(entry_at) = lines.iter().rposition(|line| line.id == entry.id) else {
        return false;
    };
    latest.iter().any(|other| {
        other.id != entry.id
            && other.op_type == FileOperationType::Move
            && matches!(
                other.state,
                JournalState::Committed | JournalState::Prepared | JournalState::Failed
            )
            && lines
                .iter()
                .rposition(|line| line.id == other.id)
                .is_some_and(|at| at > entry_at)
            && journal_members(other).iter().any(|(source, later_destination, _, _)| {
                pr_core::text::eq_ignore_case(source, destination)
                    && later_destination.is_some_and(|later_destination| *source != later_destination)
            })
            && (other.state == JournalState::Committed || !rig.disk.file_exists(Path::new(destination)))
    })
}

fn check_state(rig: &Rig, context: &str, after_recovery: bool) {
    let photo_files = rig
        .disk
        .all_files()
        .into_iter()
        .filter(|path| path.starts_with(&rig.folder))
        .collect::<Vec<_>>();
    let mut payloads = BYTES.iter().take(rig.member_count).copied().collect::<Vec<_>>();
    payloads.push(B_CONTENT);
    for bytes in payloads {
        let copies = photo_files
            .iter()
            .filter(|path| rig.disk.file_content(path).as_deref() == Some(bytes))
            .count();
        assert_eq!(
            copies, 1,
            "{context}: each original payload must exist exactly once (no loss, duplication, or overwrite)"
        );
    }
    let lines = rig.entries();
    let latest = latest_entries(&lines);
    if after_recovery {
        assert!(
            latest.iter().all(|entry| entry.state != JournalState::Prepared),
            "{context}: latest journal contains Prepared: {:?}",
            summarize(&lines)
        );
    }
    let mut named = std::collections::HashSet::new();
    for entry in &lines {
        for (source, destination, _, _) in journal_members(entry) {
            named.insert(pr_core::text::ignore_case_key(source));
            if let Some(destination) = destination {
                named.insert(pr_core::text::ignore_case_key(destination));
            }
        }
    }
    for name in NAMES.iter().take(rig.member_count).chain(std::iter::once(&"b.jpg")) {
        let seed = rig.path(name);
        if !has_full(rig, &seed) {
            assert!(
                named.contains(&pr_core::text::ignore_case_key(&seed)),
                "{context}: seed {seed} left without a journal record"
            );
        }
    }
    for path in &photo_files {
        let text = path.to_string_lossy().into_owned();
        let seed = (0..rig.member_count).any(|i| pr_core::text::eq_ignore_case(&text, &rig.path(NAMES[i])))
            || pr_core::text::eq_ignore_case(&text, &rig.path("b.jpg"));
        if !seed {
            assert!(
                named.contains(&pr_core::text::ignore_case_key(&text)),
                "{context}: unjournaled destination {text}"
            );
        }
    }
    for entry in latest
        .iter()
        .filter(|entry| entry.state != JournalState::Dismissed && entry.op_type == FileOperationType::Move)
    {
        for (source, destination, size, last_write) in journal_members(entry) {
            let Some(destination) = destination else { continue };
            let undo_supersedes = entry.undo != Some(true)
                && entry.op_type != FileOperationType::Copy
                && lines.iter().any(|undo| {
                    undo.undo == Some(true)
                        && undo.id != entry.id
                        && undo.op_type == entry.op_type
                        && journal_members(undo)
                            .iter()
                            .any(|(undo_source, _, _, _)| pr_core::text::eq_ignore_case(undo_source, destination))
                });
            let moved_on = later_move_moved_on(rig, &lines, &latest, entry, destination);
            if entry.state == JournalState::Committed && !undo_supersedes && !moved_on {
                assert!(
                    !source_still_there(rig, source, Some(destination)),
                    "{context}: committed source {source} remains (destination={destination}, moved_on={moved_on})"
                );
                assert!(
                    has_full(rig, destination),
                    "{context}: committed destination {destination} is not the original payload"
                );
                let stat = rig
                    .disk
                    .get_file_stat(Path::new(destination))
                    .expect("committed destination stat");
                assert_eq!(
                    i64::try_from(stat.length).ok(),
                    Some(size),
                    "{context}: committed length fingerprint differs"
                );
                assert_eq!(
                    stat.last_write_utc, last_write,
                    "{context}: committed LastWriteUtc differs"
                );
            }
        }
    }
}

fn retry_all(rig: &mut Rig) {
    let failed = latest_entries(&rig.entries())
        .into_iter()
        .filter(|entry| entry.state == JournalState::Failed)
        .collect::<Vec<_>>();
    let retry = RecoveryRetryService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        rig.clock.clone(),
        Arc::new(EnglishRecoveryRetryTexts),
    )
    .with_recycle_bin(rig.bin.clone());
    for entry in failed {
        let outcome = retry
            .retry_move_or_copy(&entry, false, &CancelToken::none())
            .expect("retry recovery item");
        if outcome.succeeded {
            let committed = outcome
                .entry
                .as_ref()
                .expect("completed retry returns its outcome entry");
            assert_eq!(
                committed.state,
                JournalState::Committed,
                "successful retry result is not Committed"
            );
            if outcome.journal_persisted {
                assert!(
                    latest_entries(&rig.entries())
                        .iter()
                        .any(|latest| { latest.id == committed.id && latest.state == JournalState::Committed }),
                    "persisted successful retry is not Committed in the latest journal"
                );
            } else {
                assert!(
                    outcome.journal_error.is_some(),
                    "unpersisted successful retry omitted its journal error"
                );
            }
        }
    }
}

fn foreign_followups(rig: &mut Rig, context: &str) {
    let foreign_dir = rig._root.path().join("foreign");
    let selected = rig.folder.join("sel");
    for name in ["a.jpg", "a.cr2", "a.xmp", "b.jpg"] {
        let before = rig
            .disk
            .all_files()
            .into_iter()
            .filter(|path| path.starts_with(&rig.folder))
            .map(|path| (path.clone(), rig.disk.file_content(&path).unwrap_or_default()))
            .collect::<Vec<_>>();
        let foreign = foreign_dir.join(name);
        rig.disk.add_file(&foreign, FOLLOWUP_CONTENT);
        let result = rig.service.execute(
            &pr_core::file_actions::types::FileActionRequest {
                source: foreign.to_string_lossy().into_owned(),
                operation: FileOperationType::Move,
                destination: Some(selected.to_string_lossy().into_owned()),
                allow_permanent_delete: false,
            },
            &CancelToken::none(),
        );
        let after = rig
            .disk
            .all_files()
            .into_iter()
            .filter(|path| path.starts_with(&rig.folder))
            .map(|path| (path.clone(), rig.disk.file_content(&path).unwrap_or_default()))
            .collect::<Vec<_>>();
        for (path, bytes) in before {
            assert_eq!(
                after
                    .iter()
                    .find(|(candidate, _)| candidate == &path)
                    .map(|(_, bytes)| bytes),
                Some(&bytes),
                "{context}: foreign same-name Move changed or removed {}",
                path.display()
            );
        }
        let copies = rig
            .disk
            .all_files()
            .iter()
            .filter(|path| rig.disk.file_content(path).as_deref() == Some(FOLLOWUP_CONTENT))
            .count();
        assert_eq!(copies, 1, "{context}: foreign same-name payload was lost or duplicated");
        assert!(
            result.succeeded || rig.disk.file_exists(&foreign),
            "{context}: foreign payload disappeared after its own Move"
        );
        rig.disk.delete(&foreign).expect("remove test-owned foreign source");
        let moved = selected.join(name);
        if rig.disk.file_content(&moved).as_deref() == Some(FOLLOWUP_CONTENT) {
            rig.disk.delete(&moved).expect("remove test-owned foreign destination");
        }
    }
}

fn verify_and_follow(rig: &mut Rig, context: &str, check_reported_outcomes: bool) {
    if check_reported_outcomes {
        check_outcomes(rig, context);
    }
    check_state(rig, context, false);
    rig.restart();
    let _ = rig
        .journal
        .reconcile_pending_operations(None)
        .expect("startup reconcile");
    check_state(rig, context, true);
    let settled = rig.disk.file_content(rig._root.path().join("journal.jsonl"));
    let _ = rig
        .journal
        .reconcile_pending_operations(None)
        .expect("idempotent reconcile");
    assert_eq!(
        rig.disk.file_content(rig._root.path().join("journal.jsonl")),
        settled,
        "{context}: restart reconcile appended lines"
    );
    rig.restart();
    let _ = rig
        .journal
        .reconcile_pending_operations(None)
        .expect("fresh-instance idempotent reconcile");
    assert_eq!(
        rig.disk.file_content(rig._root.path().join("journal.jsonl")),
        settled,
        "{context}: a fresh journal instance appended lines"
    );
    retry_all(rig);
    check_state(rig, &format!("{context} after retry"), true);
    let after_retry = rig.disk.file_content(rig._root.path().join("journal.jsonl"));
    let _ = rig
        .journal
        .reconcile_pending_operations(None)
        .expect("reconcile after retry");
    assert_eq!(
        rig.disk.file_content(rig._root.path().join("journal.jsonl")),
        after_retry,
        "{context}: reconcile after retry appended lines"
    );
    check_state(rig, &format!("{context} after retry reconcile"), true);
    foreign_followups(rig, context);
}

fn assert_fault_hit(rig: &Rig, at: usize, expected_kind: &str, context: &str) {
    let actual = rig.fs.kinds();
    assert!(
        actual.len() >= at,
        "{context}: planned fault at call #{at} was not reached; only {} mutating calls ran ({actual:?})",
        actual.len()
    );
    assert_eq!(
        actual.get(at - 1).map(String::as_str),
        Some(expected_kind),
        "{context}: planned mutation point changed"
    );
}

fn matrix(case: Scenario) {
    let group = matches!(case, Scenario::Group | Scenario::GroupUndo);
    let mut probe = Rig::new(group);
    probe.fs.arm(None);
    action(&mut probe, case);
    let calls = probe.fs.kinds();
    if case == Scenario::Taken {
        assert!(calls.is_empty(), "taken-name preflight unexpectedly mutated: {calls:?}");
        verify_and_follow(&mut probe, "taken-name preflight", true);
        eprintln!("W9-R9 {case:?}: mutation_points=0 fault_runs=0 nested_runs=0 (expected preflight no-op)");
        return;
    }
    assert!(
        calls.len() >= 2,
        "{case:?}: expected at least two numbered mutation points, got {calls:?}"
    );
    verify_and_follow(&mut probe, &format!("{case:?} healthy"), true);
    let mut runs = 0usize;
    for (index, mutation) in calls.iter().enumerate() {
        for kind in KINDS {
            if !kind.applies(mutation) {
                continue;
            }
            let mut rig = Rig::new(group);
            rig.fs.arm(Some(FaultPlan { at: index + 1, kind }));
            action(&mut rig, case);
            assert_fault_hit(&rig, index + 1, mutation, &format!("{case:?} {kind:?}"));
            runs += 1;
            verify_and_follow(
                &mut rig,
                &format!("{case:?} {kind:?} call #{}/{} ({mutation})", index + 1, calls.len()),
                true,
            );
        }
    }
    assert!(runs > 0, "{case:?}: no applicable fault plans ran");
    eprintln!(
        "W9-R9 {case:?}: mutation_points={} fault_runs={runs} nested_runs=0",
        calls.len()
    );
}

fn interrupted_reconcile(case: Scenario) {
    let group = matches!(case, Scenario::Group | Scenario::GroupUndo);
    let mut probe = Rig::new(group);
    probe.fs.arm(None);
    action(&mut probe, case);
    let outer_calls = probe.fs.kinds();
    let mut runs = 0usize;
    for (point, call) in outer_calls.iter().enumerate() {
        for outer in [FaultKind::CrashBefore, FaultKind::CrashAfter] {
            if !outer.applies(call) {
                continue;
            }
            let mut baseline = Rig::new(group);
            baseline.fs.arm(Some(FaultPlan {
                at: point + 1,
                kind: outer,
            }));
            action(&mut baseline, case);
            assert_fault_hit(&baseline, point + 1, call, &format!("{case:?} outer {outer:?}"));
            baseline.restart();
            let _ = baseline.journal.reconcile_pending_operations(None);
            let expected = summarize(&baseline.entries());
            let inner_calls = baseline.fs.kinds();
            for (inner_point, inner_call) in inner_calls.iter().enumerate() {
                for inner in [FaultKind::CrashBefore, FaultKind::CrashPartial, FaultKind::FailOnce] {
                    if !inner.applies(inner_call) {
                        continue;
                    }
                    let mut rig = Rig::new(group);
                    rig.fs.arm(Some(FaultPlan {
                        at: point + 1,
                        kind: outer,
                    }));
                    action(&mut rig, case);
                    assert_fault_hit(&rig, point + 1, call, &format!("{case:?} outer {outer:?}"));
                    rig.restart();
                    rig.fs.arm(Some(FaultPlan {
                        at: inner_point + 1,
                        kind: inner,
                    }));
                    let _ = rig.journal.reconcile_pending_operations(None);
                    assert_fault_hit(
                        &rig,
                        inner_point + 1,
                        inner_call,
                        &format!("{case:?} reconcile {inner:?}"),
                    );
                    runs += 1;
                    rig.restart();
                    let _ = rig.journal.reconcile_pending_operations(None);
                    assert_eq!(
                        summarize(&rig.entries()),
                        expected,
                        "{case:?}: nested interrupted reconcile {outer:?} call {} / {inner:?} call {}",
                        point + 1,
                        inner_point + 1
                    );
                    verify_and_follow(&mut rig, "nested interrupted reconcile", false);
                }
            }
        }
    }
    assert!(runs > 0, "{case:?}: no nested reconcile fault plans ran");
    eprintln!(
        "W9-R9 {case:?} reconcile: mutation_points={} nested_runs={runs}",
        outer_calls.len()
    );
}

const PROCESS_CRASH_KINDS: [FaultKind; 4] = [
    FaultKind::CrashBefore,
    FaultKind::CrashAfter,
    FaultKind::CrashPartial,
    FaultKind::CrashMoveCopiedNotDeleted,
];
const RETRY_FOLLOWUP_KINDS: [FaultKind; 6] = [
    FaultKind::CrashBefore,
    FaultKind::CrashPartial,
    FaultKind::CrashMoveCopiedNotDeleted,
    FaultKind::FailOnce,
    FaultKind::FailPartial,
    FaultKind::FailUnauthorized,
];

fn assert_retry_interrupted(case: Scenario) {
    let group = matches!(case, Scenario::Group | Scenario::GroupUndo);
    let mut probe = Rig::new(group);
    probe.fs.arm(None);
    action(&mut probe, case);
    let action_calls = probe.fs.kinds();
    let mut outer_runs = 0usize;
    let mut retryable_crash_states = 0usize;
    let mut nested_runs = 0usize;
    for (action_point, action_kind) in action_calls.iter().enumerate() {
        for outer_kind in PROCESS_CRASH_KINDS {
            if !outer_kind.applies(action_kind) {
                continue;
            }
            let mut clean = Rig::new(group);
            clean.fs.arm(Some(FaultPlan {
                at: action_point + 1,
                kind: outer_kind,
            }));
            action(&mut clean, case);
            assert_fault_hit(
                &clean,
                action_point + 1,
                action_kind,
                &format!("{case:?} initial {outer_kind:?}"),
            );
            check_outcomes(&clean, &format!("{case:?} initial {outer_kind:?}"));
            check_state(&clean, &format!("{case:?} initial {outer_kind:?}"), false);
            clean.restart();
            let _ = clean.journal.reconcile_pending_operations(None);
            clean.fs.arm(None);
            retry_all(&mut clean);
            let retry_calls = clean.fs.kinds();
            outer_runs += 1;
            if retry_calls.is_empty() {
                continue;
            }
            retryable_crash_states += 1;
            for (retry_point, retry_kind_name) in retry_calls.iter().enumerate() {
                for injected_kind in RETRY_FOLLOWUP_KINDS {
                    if !injected_kind.applies(retry_kind_name) {
                        continue;
                    }
                    let mut rig = Rig::new(group);
                    rig.fs.arm(Some(FaultPlan {
                        at: action_point + 1,
                        kind: outer_kind,
                    }));
                    action(&mut rig, case);
                    assert_fault_hit(
                        &rig,
                        action_point + 1,
                        action_kind,
                        &format!("{case:?} initial {outer_kind:?}"),
                    );
                    check_outcomes(&rig, &format!("{case:?} initial {outer_kind:?}"));
                    check_state(&rig, &format!("{case:?} initial {outer_kind:?}"), false);
                    rig.restart();
                    let _ = rig.journal.reconcile_pending_operations(None);
                    rig.fs.arm(Some(FaultPlan {
                        at: retry_point + 1,
                        kind: injected_kind,
                    }));
                    retry_all(&mut rig);
                    assert_fault_hit(
                        &rig,
                        retry_point + 1,
                        retry_kind_name,
                        &format!("{case:?} retry {injected_kind:?}"),
                    );
                    nested_runs += 1;
                    verify_and_follow(
                        &mut rig,
                        &format!("{case:?} retry interrupted {outer_kind:?}/{injected_kind:?}"),
                        false,
                    );
                }
            }
        }
    }
    assert!(
        retryable_crash_states > 0,
        "{case:?}: no action crash left a recovery retry to interrupt"
    );
    assert!(nested_runs > 0, "{case:?}: no retry fault points ran");
    eprintln!(
        "W9-R9 {case:?} second-order retry: mutation_points={} fault_runs={outer_runs} nested_runs={nested_runs}",
        action_calls.len()
    );
}

// C#: RenameFaultInjectionTests.Rename_Matrix.
#[test]
fn rename_matrix() {
    matrix(Scenario::Rename);
}
// C#: RenameFaultInjectionTests.Rename_ToTakenName_Matrix.
#[test]
fn rename_to_taken_name_matrix() {
    matrix(Scenario::Taken);
}
// C#: RenameFaultInjectionTests.CaseOnlyRename_Matrix.
#[test]
fn case_only_rename_matrix() {
    matrix(Scenario::CaseOnly);
}
// C#: RenameFaultInjectionTests.GroupRename_Matrix.
#[test]
fn group_rename_matrix() {
    matrix(Scenario::Group);
}
// C#: RenameFaultInjectionTests.GroupCaseOnlyRename_Matrix is owned by file_actions_matrix.rs (R3); no duplicate test.
// C#: RenameFaultInjectionTests.RenameThenUndo_Matrix.
#[test]
fn rename_then_undo_matrix() {
    matrix(Scenario::RenameUndo);
}
// C#: RenameFaultInjectionTests.CaseOnlyRenameThenUndo_Matrix.
#[test]
fn case_only_rename_then_undo_matrix() {
    matrix(Scenario::CaseUndo);
}
// C#: RenameFaultInjectionTests.GroupRenameThenUndo_Matrix.
#[test]
fn group_rename_then_undo_matrix() {
    matrix(Scenario::GroupUndo);
}
// C#: RenameFaultInjectionTests.TwoRenames_Matrix.
#[test]
fn two_renames_matrix() {
    matrix(Scenario::TwoRenames);
}
// C#: RenameFaultInjectionTests.Rename_ReconcileInterrupted.
#[test]
fn rename_reconcile_interrupted() {
    interrupted_reconcile(Scenario::Rename);
}
// C#: RenameFaultInjectionTests.CaseOnlyRename_ReconcileInterrupted is owned by file_actions_matrix.rs (R3); no duplicate test.
// C#: RenameFaultInjectionTests.GroupRename_ReconcileInterrupted.
#[test]
fn group_rename_reconcile_interrupted() {
    interrupted_reconcile(Scenario::Group);
}

#[test]
fn rename_retry_interruption_second_order() {
    // C#: FaultInjectionSecondOrder.AssertRetryInterruptedAsync, applied to the owned single, case-only, and group rename scenarios.
    for scenario in [
        Scenario::Rename,
        Scenario::CaseOnly,
        Scenario::Group,
        Scenario::RenameUndo,
        Scenario::CaseUndo,
        Scenario::GroupUndo,
        Scenario::TwoRenames,
    ] {
        assert_retry_interrupted(scenario);
    }
}

// C#: FaultInjectionFileSystem.Move classifies same-folder atomic rename as "rename", excluding partial-copy faults.
#[test]
fn same_folder_move_uses_rename_fault_kind() {
    let rig = Rig::new(false);
    rig.fs.arm(None);
    rig.fs
        .move_file(Path::new(&rig.path("a.jpg")), Path::new(&rig.path("A.jpg")))
        .unwrap();
    assert_eq!(rig.fs.kinds(), ["rename"]);
    for kind in [
        FaultKind::CrashPartial,
        FaultKind::FailPartial,
        FaultKind::CrashMoveCopiedNotDeleted,
    ] {
        assert!(!kind.applies("rename"));
    }
    for kind in [
        FaultKind::CrashBefore,
        FaultKind::CrashAfter,
        FaultKind::FailOnce,
        FaultKind::FailUnauthorized,
    ] {
        assert!(kind.applies("rename"));
    }
}
