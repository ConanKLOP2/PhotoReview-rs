//! G19 (docs/reviews/invariants.md section 5 item 1 / M2): the journal in `Fast` mode only flushes to the OS cache, so a
//! power loss can drop the last lines; `PowerLossSafe` opens the stream write-through (`durable`) and keeps them.
//! The "power loss" is modelled without a real disk: every append is recorded by the `HookFileSystem` with its `durable`
//! flag, and the journal after the loss is rebuilt from the lines whose append was durable only. Fakes only, no sleeping.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::model::{FileOperationType, JournalDurability, JournalState};
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeClock, FakeFileSystem, HookFileSystem};

fn paths() -> AppPaths {
    AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false)
}

fn stamp() -> JournalDateTime {
    JournalDateTime::utc(DateTimeUtc::parse_json("2026-09-25T00:00:00Z").unwrap().0)
}

fn entry(id: &str, state: JournalState) -> JournalEntry {
    JournalEntry {
        id: id.to_owned(),
        op_type: FileOperationType::Move,
        state,
        source: r"C:\photos\a.jpg".to_owned(),
        destination: Some(r"C:\photos\b.jpg".to_owned()),
        size: 1,
        last_write_utc: stamp(),
        timestamp_utc: stamp(),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

struct Rig {
    clock: Arc<FakeClock>,
    disk: Arc<FakeFileSystem>,
    hooks: Arc<HookFileSystem>,
    journal: OperationJournal,
    journal_file: PathBuf,
}

fn rig(durability: JournalDurability) -> Rig {
    let clock = Arc::new(FakeClock::at(
        DateTimeUtc::parse_json("2026-09-25T00:00:00Z").unwrap().0,
    ));
    let disk = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    let hooks = Arc::new(HookFileSystem::new(disk.clone()));
    let journal = OperationJournal::with_options(
        &paths(),
        hooks.clone(),
        clock.clone(),
        OperationJournalOptions {
            durability: Some(Arc::new(move || durability)),
            ..OperationJournalOptions::default()
        },
    );
    Rig {
        clock,
        disk,
        hooks,
        journal,
        journal_file: paths().journal_file,
    }
}

impl Rig {
    /// The journal as the next start sees it after a power loss: only lines whose append was write-through survive.
    fn after_power_loss(&self) -> OperationJournal {
        let text = String::from_utf8(self.disk.file_content(&self.journal_file).unwrap_or_default()).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        let durable: Vec<bool> = self
            .hooks
            .events()
            .iter()
            .filter(|e| e.kind == "append")
            .map(|e| e.durable)
            .collect();
        assert_eq!(lines.len(), durable.len(), "one line per append event");
        let survivors: Vec<&str> = lines
            .iter()
            .zip(&durable)
            .filter(|(_, d)| **d)
            .map(|(l, _)| *l)
            .collect();
        let restarted = Arc::new(FakeFileSystem::with_clock(self.clock.clone()));
        if !survivors.is_empty() {
            restarted.add_file(&self.journal_file, format!("{}\r\n", survivors.join("\r\n")).as_bytes());
        }
        OperationJournal::new(&paths(), restarted, self.clock.clone())
    }
}

#[test]
fn fast_mode_append_is_not_write_through_so_a_power_loss_can_drop_the_prepared_line() {
    let rig = rig(JournalDurability::Fast);
    rig.journal.append(&entry("op-1", JournalState::Prepared)).unwrap();
    let events = rig.hooks.events();
    assert_eq!(events.len(), 1);
    assert!(!events[0].durable, "Fast opens the stream without write-through");
    // The line is visible to this process, but after the power loss nothing is left to reconcile.
    assert_eq!(rig.journal.read_pending_and_failed_operations().unwrap().len(), 1);
    assert!(
        rig.after_power_loss()
            .read_pending_and_failed_operations()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn power_loss_safe_append_is_write_through_so_the_prepared_line_survives_a_power_loss() {
    let rig = rig(JournalDurability::PowerLossSafe);
    rig.journal.append(&entry("op-1", JournalState::Prepared)).unwrap();
    assert!(rig.hooks.events().iter().all(|e| e.durable));
    let pending = rig.after_power_loss().read_pending_and_failed_operations().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "op-1");
    assert_eq!(pending[0].state, JournalState::Prepared);
}

#[test]
fn the_durability_provider_is_read_for_every_append_so_a_settings_change_applies_to_the_next_line() {
    let mode = Arc::new(std::sync::Mutex::new(JournalDurability::Fast));
    let clock = Arc::new(FakeClock::at(
        DateTimeUtc::parse_json("2026-09-25T00:00:00Z").unwrap().0,
    ));
    let disk = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    let hooks = Arc::new(HookFileSystem::new(disk));
    let provider = mode.clone();
    let journal = OperationJournal::with_options(
        &paths(),
        hooks.clone(),
        clock,
        OperationJournalOptions {
            durability: Some(Arc::new(move || *provider.lock().unwrap())),
            ..OperationJournalOptions::default()
        },
    );
    journal.append(&entry("op-1", JournalState::Prepared)).unwrap();
    *mode.lock().unwrap() = JournalDurability::PowerLossSafe;
    journal.append(&entry("op-1", JournalState::Committed)).unwrap();
    let flags: Vec<bool> = hooks.events().iter().map(|e| e.durable).collect();
    assert_eq!(flags, [false, true]);
}

#[test]
fn a_committed_line_in_fast_mode_can_be_lost_while_the_earlier_durable_prepared_line_survives() {
    // Prepared written while PowerLossSafe, Committed after the user switched to Fast: the loss leaves the action
    // Prepared, which the next start reconciles from the disk (INV-6), instead of silently forgetting it.
    let mode = Arc::new(std::sync::Mutex::new(JournalDurability::PowerLossSafe));
    let clock = Arc::new(FakeClock::at(
        DateTimeUtc::parse_json("2026-09-25T00:00:00Z").unwrap().0,
    ));
    let disk = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    let hooks = Arc::new(HookFileSystem::new(disk.clone()));
    let provider = mode.clone();
    let journal = OperationJournal::with_options(
        &paths(),
        hooks.clone(),
        clock.clone(),
        OperationJournalOptions {
            durability: Some(Arc::new(move || *provider.lock().unwrap())),
            ..OperationJournalOptions::default()
        },
    );
    journal.append(&entry("op-1", JournalState::Prepared)).unwrap();
    *mode.lock().unwrap() = JournalDurability::Fast;
    journal.append(&entry("op-1", JournalState::Committed)).unwrap();
    let rig = Rig {
        clock,
        disk,
        hooks,
        journal,
        journal_file: paths().journal_file,
    };
    let pending = rig.after_power_loss().read_pending_and_failed_operations().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].state, JournalState::Prepared);
}
