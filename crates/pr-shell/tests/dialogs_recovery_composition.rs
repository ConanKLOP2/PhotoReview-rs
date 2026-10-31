//! Lane st-dialog-scenarios: the Retry / Clear / permanent-delete closures that the production composition hands to the Recovery window
//! (C# `Win32SecondaryWindowPort.ShowRecovery` 148-160, P-43). Headless: `FakeFileSystem`, a fixed clock, no Recycle Bin, no window.
//! C# twins: `RecoveryDialogGapTests.Port_Retry_ConfirmsAUserCancelledEntryExplicitly_SoTheServiceFinishesIt`,
//! `Port_Clear_WritesTheDismissalToTheRealJournal`, `Port_RetryQuestion_WarnsAboutPermanentDeletion_OnlyWhenTheSettingAllowsIt`,
//! and the closure side of `RecoveryDialogDataSafetyTests.Retry_*`.
use pr_app::coordinators::zoom_detail::ThreadRunner;
use pr_app::presenters::benchmark::BenchmarkSessionFactory;
use pr_app::presenters::recovery_window::RecoveryWindowOptions;
use pr_core::abstractions::{FileSystem, NullLog, ShellIntegration, ShellMenuState};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::errors::CANCELLED_BY_USER;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::file_actions::recovery_retry::{EnglishRecoveryRetryTexts, RecoveryRetryService};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::store::SettingsStore;
use pr_core::time::DateTimeUtc;
use pr_shell::startup::secondary_dialogs::{DialogServices, recovery_window_options};
use pr_test_support::{FakeClock, FakeFileSystem};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

const STAMP_TICKS: i64 = 639_253_296_000_000_000;

fn stamp() -> DateTimeUtc {
    DateTimeUtc::from_ticks(STAMP_TICKS).expect("fixture ticks are in range")
}

#[derive(Debug)]
struct NoIntegration;
impl ShellIntegration for NoIntegration {
    fn get_state(&self, _: &str) -> ShellMenuState {
        ShellMenuState::NotRegistered
    }
    fn register(&self, _: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn unregister(&self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Rig {
    fs: Arc<FakeFileSystem>,
    journal: Arc<OperationJournal>,
    settings: Arc<SettingsStore>,
    services: Rc<DialogServices>,
}

impl Rig {
    /// `with_retry` / `with_settings` = whether the composition registered `RecoveryRetryService` / `SettingsStore` (C# `GetService<T>()` not null).
    fn new(with_retry: bool, with_settings: bool) -> Self {
        let fs = Arc::new(FakeFileSystem::new());
        let fs_dyn: Arc<dyn FileSystem> = fs.clone();
        let paths = AppPaths::new(Path::new(r"C:\data"), None, false);
        let clock = Arc::new(FakeClock::at(stamp()));
        let journal = Arc::new(OperationJournal::new(&paths, fs_dyn.clone(), clock.clone()));
        let settings = Arc::new(SettingsStore::new(
            paths,
            fs_dyn.clone(),
            Arc::new(NullLog),
            clock.clone(),
            Arc::new(EnglishSettingsTexts),
        ));
        let retry = with_retry.then(|| {
            Arc::new(RecoveryRetryService::new(
                journal.clone(),
                fs_dyn.clone(),
                clock.clone(),
                Arc::new(EnglishRecoveryRetryTexts),
            ))
        });
        let sessions: BenchmarkSessionFactory = Arc::new(|_, _| unreachable!("benchmark is not used here"));
        let services = Rc::new(DialogServices {
            settings_store: with_settings.then(|| settings.clone()),
            localization: None,
            update_checker: None,
            decoder_factory: None,
            journal: Some(journal.clone()),
            recovery_retry: retry,
            file_system: fs_dyn,
            metrics: None,
            explorer_snapshot: None,
            settings_texts: Arc::new(EnglishSettingsTexts),
            shell_integration: Arc::new(NoIntegration),
            physical_memory_bytes: 0,
            runner: Arc::new(ThreadRunner),
            benchmark_sessions: sessions,
        });
        Self {
            fs,
            journal,
            settings,
            services,
        }
    }

    fn options(&self) -> RecoveryWindowOptions {
        recovery_window_options(&self.services, &self.journal, Arc::new(|| "12:00:00".to_owned()))
    }

    /// A journaled entry whose source exists and still matches the journaled fingerprint.
    fn seed(&self, id: &str, operation: FileOperationType, state: JournalState) -> JournalEntry {
        let source = format!(r"C:\photos\{id}.jpg");
        self.fs.add_file_at(&source, b"original bytes", stamp());
        self.fs.add_dir(r"C:\dest");
        let entry = JournalEntry {
            id: id.to_owned(),
            op_type: operation,
            state,
            source,
            destination: Some(format!(r"C:\dest\{id}.jpg")),
            size: 14,
            last_write_utc: JournalDateTime::utc(stamp()),
            timestamp_utc: JournalDateTime::utc(stamp()),
            error: None,
            error_code: None,
            permanent: None,
            undo: None,
            group_id: None,
            group_members: None,
        };
        self.journal.append(&entry).expect("the fake journal accepts the line");
        entry
    }

    fn latest(&self, id: &str) -> JournalEntry {
        self.journal.read_latest_entry(id).unwrap().expect("entry is journaled")
    }

    fn exists(&self, path: &str) -> bool {
        self.fs.file_exists(Path::new(path))
    }
}

#[test]
fn retry_closure_moves_or_copies_and_commits_the_journal() {
    for operation in [FileOperationType::Move, FileOperationType::Copy] {
        let rig = Rig::new(true, true);
        let entry = rig.seed("e1", operation, JournalState::Failed);
        let retry = rig.options().retry.expect("the retry service is registered");
        let result = retry(&entry).expect("not cancelled");
        assert!(result.succeeded, "{operation:?}: {}", result.message);
        assert!(!result.superseded);
        assert_eq!(rig.fs.file_content(r"C:\dest\e1.jpg").unwrap(), b"original bytes");
        assert_eq!(rig.exists(&entry.source), operation == FileOperationType::Copy);
        assert_eq!(rig.latest("e1").state, JournalState::Committed);
    }
}

#[test]
fn retry_closure_finishes_a_user_cancelled_entry_without_asking_again() {
    // The window confirms a cancelled entry explicitly BEFORE it calls the delegate, so the delegate passes confirmedFinishCancelled: true.
    let rig = Rig::new(true, true);
    let mut entry = rig.seed("cancelled", FileOperationType::Move, JournalState::Failed);
    entry.error_code = Some(CANCELLED_BY_USER.to_owned());
    rig.journal.append(&entry).unwrap();
    let result = (rig.options().retry.unwrap())(&entry).unwrap();
    assert!(result.succeeded, "{}", result.message);
    assert_eq!(rig.latest("cancelled").state, JournalState::Committed);
    assert!(rig.exists(r"C:\dest\cancelled.jpg"));
}

#[test]
fn retry_closure_is_refused_and_touches_nothing_when_the_source_changed() {
    let rig = Rig::new(true, true);
    let entry = rig.seed("changed", FileOperationType::Move, JournalState::Failed);
    rig.fs.add_file_at(
        &entry.source,
        b"edited elsewhere!",
        DateTimeUtc::from_ticks(STAMP_TICKS + 10_000_000).unwrap(),
    );
    let result = (rig.options().retry.unwrap())(&entry).unwrap();
    assert!(!result.succeeded);
    assert!(rig.exists(&entry.source));
    assert!(!rig.exists(r"C:\dest\changed.jpg"));
    assert_eq!(rig.latest("changed").state, JournalState::Failed);
}

#[test]
fn retry_closure_reports_superseded_for_an_entry_another_window_dismissed() {
    let rig = Rig::new(true, true);
    let entry = rig.seed("gone", FileOperationType::Move, JournalState::Failed);
    rig.journal.dismiss(std::slice::from_ref(&entry)).unwrap();
    let result = (rig.options().retry.unwrap())(&entry).unwrap();
    assert!(result.superseded);
    assert!(!result.succeeded);
    assert!(rig.exists(&entry.source));
    assert!(!rig.exists(r"C:\dest\gone.jpg"));
    assert_eq!(rig.latest("gone").state, JournalState::Dismissed);
}

#[test]
fn without_the_retry_service_the_retry_delegate_is_absent_but_clear_still_works() {
    let options = Rig::new(false, false).options();
    assert!(options.retry.is_none());
    assert!(options.dismiss.is_some());
}

#[test]
fn dismiss_closure_writes_only_journal_lines_and_reports_resolved_entries_as_skipped() {
    let rig = Rig::new(true, true);
    let a = rig.seed("a", FileOperationType::Move, JournalState::Failed);
    let b = rig.seed("b", FileOperationType::Copy, JournalState::Prepared);
    let c = rig.seed("c", FileOperationType::Recycle, JournalState::Failed);
    rig.journal.dismiss(std::slice::from_ref(&c)).unwrap(); // resolved elsewhere (another window)
    let photos_before: Vec<_> = rig
        .fs
        .all_files()
        .into_iter()
        .filter(|p| p.starts_with(r"C:\photos"))
        .collect();
    let outcome =
        (rig.options().dismiss.unwrap())(&[a.clone(), b.clone(), c.clone()]).expect("the journal accepts the lines");
    assert_eq!(
        outcome.dismissed.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(outcome.skipped.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["c"]);
    assert_eq!(rig.latest("a").state, JournalState::Dismissed);
    assert_eq!(rig.latest("b").state, JournalState::Dismissed);
    let photos_after: Vec<_> = rig
        .fs
        .all_files()
        .into_iter()
        .filter(|p| p.starts_with(r"C:\photos"))
        .collect();
    assert_eq!(photos_after, photos_before);
    assert_eq!(photos_after.len(), 3);
    assert!(!rig.exists(r"C:\dest\a.jpg"));
}

#[test]
fn permanent_delete_warning_reads_the_current_setting_each_time_it_is_asked() {
    let rig = Rig::new(true, true);
    let allow = rig
        .options()
        .allow_permanent_delete
        .expect("a settings store is registered");
    assert!(!allow());
    let mut on = AppSettings::new(&EnglishSettingsTexts);
    on.allow_permanent_delete_without_recycle_bin = true;
    rig.settings.save(on).expect("the fake disk accepts the settings");
    assert!(allow(), "the closure must read store.current() when asked, not a copy");
    let mut off = AppSettings::new(&EnglishSettingsTexts);
    off.allow_permanent_delete_without_recycle_bin = false;
    rig.settings.save(off).unwrap();
    assert!(!allow());
}

#[test]
fn without_a_settings_store_no_permanent_delete_warning_closure_exists() {
    // C# `settings?.Current.AllowPermanentDeleteWithoutRecycleBin == true`: no store = false; the window treats `None` as false.
    assert!(Rig::new(true, false).options().allow_permanent_delete.is_none());
}
