//! RecoveryDialogDataSafetyTests: real journal and Move/Copy service inside TempRoot. No Recycle Bin is configured.
use pr_core::abstractions::FileSystem;
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::RecoveryFileCheck;
use pr_core::file_actions::recovery_retry::{EnglishRecoveryRetryTexts, RecoveryRetryService};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use pr_platform_win::dialogs::recovery::{RecoveryDialog, RecoveryDialogSeams, layout::*};
use pr_platform_win::dialogs::task_dialog::TaskDialogKind;
use pr_platform_win::win32::messages::{IDCANCEL, IDNO, IDYES};
use pr_test_support::{FakeClock, TempRoot};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::SystemTime;
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/dialogs_recovery_fixture.rs"]
mod fixture;

struct RealRig {
    root: TempRoot,
    fs: Arc<PhysicalFileSystem>,
    journal: Arc<OperationJournal>,
    service: Arc<RecoveryRetryService>,
    journal_path: PathBuf,
}
impl RealRig {
    fn new() -> Self {
        let root = TempRoot::new("recovery-data-safety");
        let fs = Arc::new(PhysicalFileSystem);
        let clock = Arc::new(FakeClock::new());
        std::fs::create_dir_all(root.join("user/source")).unwrap();
        std::fs::create_dir_all(root.join("user/destination")).unwrap();
        let paths = AppPaths {
            journal_file: root.join("data/operations.jsonl"),
            ..AppPaths::new(root.path(), None, false)
        };
        let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
        let service = Arc::new(RecoveryRetryService::new(
            journal.clone(),
            fs.clone(),
            clock,
            Arc::new(EnglishRecoveryRetryTexts),
        ));
        Self {
            root,
            fs,
            journal,
            service,
            journal_path: paths.journal_file,
        }
    }
    fn seed(&self, id: &str, operation: FileOperationType, state: JournalState) -> JournalEntry {
        let source = self.root.join(format!("user/source/{id}.jpg"));
        std::fs::write(&source, format!("original bytes for {id}")).unwrap();
        let stat = self.fs.get_file_stat(&source).unwrap();
        let mut entry = fixture::entry(id);
        entry.op_type = operation;
        entry.state = state;
        entry.source = source.to_string_lossy().into_owned();
        entry.destination = Some(
            self.root
                .join(format!("user/destination/{id}.jpg"))
                .to_string_lossy()
                .into_owned(),
        );
        entry.size = stat.length as i64;
        entry.last_write_utc = JournalDateTime::utc(stat.last_write_utc);
        self.journal.append(&entry).unwrap();
        entry
    }
    fn journal_bytes(&self) -> Vec<u8> {
        std::fs::read(&self.journal_path).unwrap()
    }
    fn tree(&self) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, (Vec<u8>, SystemTime)>) {
            for item in std::fs::read_dir(dir).unwrap() {
                let p = item.unwrap().path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    out.insert(
                        p.strip_prefix(root).unwrap().to_path_buf(),
                        (
                            std::fs::read(&p).unwrap(),
                            std::fs::metadata(&p).unwrap().modified().unwrap(),
                        ),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(self.root.path(), &self.root.join("user"), &mut out);
        out
    }
    fn open(
        &self,
        entries: Vec<JournalEntry>,
        messages: Rc<fixture::FakeMessages>,
        runner: Arc<fixture::QueuedRunner>,
    ) -> RecoveryDialog {
        let mut options = fixture::options();
        let service = self.service.clone();
        options.retry = Some(Arc::new(move |e| {
            service
                .retry_move_or_copy(e, true, &CancelToken::new())
                .map_err(|e| e.to_string())
        }));
        let journal = self.journal.clone();
        options.dismiss = Some(Box::new(move |entries| {
            journal.dismiss(entries).map_err(|e| e.to_string())
        }));
        options.checker = Arc::new(RecoveryFileCheck::new(self.fs.clone()));
        let mut services = fixture::services();
        services.runner = runner;
        RecoveryDialog::new(
            entries,
            options,
            services,
            RecoveryDialogSeams {
                messages: Some(messages),
                auto_check: false,
                ..Default::default()
            },
            None,
        )
    }
}

// C#: Retry_Move_Succeeds_MovesTheFile_CommitsTheJournal_AndClosesTheWindow / Retry_Copy_KeepsTheSource_AndCreatesTheDestination
#[test]
#[ignore = "native: needs desktop and isolated real disk"]
fn confirmed_move_and_copy_use_the_real_service_and_commit_the_journal() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        for operation in [FileOperationType::Move, FileOperationType::Copy] {
            let rig = RealRig::new();
            let e = rig.seed("e1", operation, JournalState::Failed);
            let original = std::fs::read(&e.source).unwrap();
            let messages = fixture::FakeMessages::yes();
            let runner = Arc::new(fixture::QueuedRunner::default());
            let owner = desktop::create_owner();
            let d = rig.open(vec![e.clone()], messages.clone(), runner.clone());
            d.show_modeless(owner, None, None).unwrap();
            d.select_rows(&[0]);
            desktop::command(d.native(), RETRY_ID, 0);
            assert_eq!(std::fs::read(&e.source).unwrap(), original);
            assert!(!Path::new(e.destination.as_ref().unwrap()).exists());
            runner.run_all();
            d.pump_completions();
            assert_eq!(d.native().hwnd(), 0);
            assert_eq!(std::fs::read(e.destination.as_ref().unwrap()).unwrap(), original);
            assert_eq!(Path::new(&e.source).exists(), operation == FileOperationType::Copy);
            assert_eq!(
                rig.journal.read_latest_entry("e1").unwrap().unwrap().state,
                JournalState::Committed
            );
            assert_eq!(messages.calls.borrow().len(), 2);
            desktop::destroy_owner(owner);
        }
    });
}

// C#: Retry_Declined_ChangesNothing_NeitherFilesNorJournal / ClearAll_Declined_LeavesTheJournalAndTheFilesAlone
#[test]
#[ignore = "native: needs desktop and isolated real disk"]
fn declined_and_unanswered_actions_preserve_file_bytes_times_and_journal_bytes() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        for answer in [IDNO, IDCANCEL] {
            let rig = RealRig::new();
            let e = rig.seed("e1", FileOperationType::Move, JournalState::Failed);
            let before = rig.tree();
            let journal = rig.journal_bytes();
            let messages = fixture::FakeMessages::yes();
            messages.answer.set(answer);
            let runner = Arc::new(fixture::QueuedRunner::default());
            let owner = desktop::create_owner();
            let d = rig.open(vec![e], messages, runner.clone());
            d.show_modeless(owner, None, None).unwrap();
            d.select_rows(&[0]);
            for id in [RETRY_ID, CLEAR_SELECTED_ID, CLEAR_ALL_ID] {
                desktop::command(d.native(), id, 0);
            }
            assert_eq!(runner.count(), 0);
            assert_eq!(rig.tree(), before);
            assert_eq!(rig.journal_bytes(), journal);
            d.close();
            desktop::destroy_owner(owner);
        }
    });
}

// C#: Retry_WhenTheSourceChangedAfterTheCheck_IsRefusedByTheService_AndNothingMoves / Retry_OfAnEntryAnotherWindowAlreadyDismissed_IsSuperseded_AndTouchesNoFile
#[test]
#[ignore = "native: needs desktop and isolated real disk"]
fn changed_or_superseded_entries_do_not_move_files_or_append_a_retry() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        for stale in [false, true] {
            let rig = RealRig::new();
            let e = rig.seed("e1", FileOperationType::Move, JournalState::Failed);
            let messages = fixture::FakeMessages::yes();
            let runner = Arc::new(fixture::QueuedRunner::default());
            let owner = desktop::create_owner();
            let d = rig.open(vec![e.clone()], messages, runner.clone());
            d.show_modeless(owner, None, None).unwrap();
            d.select_rows(&[0]);
            d.start_checks();
            runner.run_all();
            d.pump_completions();
            if stale {
                rig.journal.dismiss(std::slice::from_ref(&e)).unwrap();
            } else {
                std::fs::write(&e.source, "changed after check, different length").unwrap();
            }
            let before = rig.tree();
            let journal = rig.journal_bytes();
            desktop::command(d.native(), RETRY_ID, 0);
            runner.run_all();
            d.pump_completions();
            runner.run_all();
            d.pump_completions();
            assert_eq!(rig.tree(), before);
            assert_eq!(rig.journal_bytes(), journal);
            assert_ne!(d.native().hwnd(), 0);
            assert_eq!(d.with_presenter(|p| p.rows().len()), if stale { 0 } else { 1 });
            d.close();
            desktop::destroy_owner(owner);
        }
    });
}

// C#: Retry_OfAnEntryTheUserCancelled_NeedsThePointedConfirmation_AndFinishesItOnlyOnYes
#[test]
#[ignore = "native: needs desktop and isolated real disk"]
fn cancelled_entry_is_only_finished_after_the_pointed_confirmation_is_accepted() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let rig = RealRig::new();
        let mut e = rig.seed("e1", FileOperationType::Move, JournalState::Failed);
        e.error_code = Some(pr_core::file_actions::errors::CANCELLED_BY_USER.to_owned());
        rig.journal.append(&e).unwrap();
        let before = rig.tree();
        let journal = rig.journal_bytes();
        let messages = fixture::FakeMessages::yes();
        messages.answer.set(IDNO);
        let runner = Arc::new(fixture::QueuedRunner::default());
        let owner = desktop::create_owner();
        let d = rig.open(vec![e.clone()], messages.clone(), runner.clone());
        d.show_modeless(owner, None, None).unwrap();
        d.select_rows(&[0]);
        desktop::command(d.native(), RETRY_ID, 0);
        assert!(messages.calls.borrow()[0].1.to_lowercase().contains("cancel"));
        assert_eq!(rig.tree(), before);
        assert_eq!(rig.journal_bytes(), journal);
        messages.answer.set(IDYES);
        desktop::command(d.native(), RETRY_ID, 0);
        runner.run_all();
        d.pump_completions();
        assert_eq!(d.native().hwnd(), 0);
        assert!(!Path::new(&e.source).exists());
        assert!(Path::new(e.destination.as_ref().unwrap()).exists());
        assert_eq!(
            rig.journal.read_latest_entry("e1").unwrap().unwrap().state,
            JournalState::Committed
        );
        desktop::destroy_owner(owner);
    });
}

// C#: ClearSelected_WritesOnlyJournalLines_AcrossFailedPreparedAndDeleteFailedEntries / ClearAll_DismissesEverything_AndLeavesEveryFileExactlyAsItWas
#[test]
#[ignore = "native: needs desktop and isolated real disk"]
fn clearing_selected_or_all_changes_only_journal_lines_for_every_entry_kind() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        for clear_all in [false, true] {
            let rig = RealRig::new();
            let entries = vec![
                rig.seed("failed", FileOperationType::Move, JournalState::Failed),
                rig.seed("prepared", FileOperationType::Copy, JournalState::Prepared),
                rig.seed("delete", FileOperationType::Recycle, JournalState::Failed),
            ];
            let before = rig.tree();
            let messages = fixture::FakeMessages::yes();
            let runner = Arc::new(fixture::QueuedRunner::default());
            let owner = desktop::create_owner();
            let d = rig.open(entries.clone(), messages, runner);
            d.show_modeless(owner, None, None).unwrap();
            if clear_all {
                desktop::command(d.native(), CLEAR_ALL_ID, 0);
            } else {
                for _ in 0..3 {
                    d.select_rows(&[0]);
                    desktop::command(d.native(), CLEAR_SELECTED_ID, 0);
                }
            }
            assert_eq!(rig.tree(), before);
            assert_eq!(d.with_presenter(|p| p.rows().len()), 0);
            for e in entries {
                assert_eq!(
                    rig.journal.read_latest_entry(&e.id).unwrap().unwrap().state,
                    JournalState::Dismissed
                );
            }
            d.close();
            desktop::destroy_owner(owner);
        }
    });
}

// C#: Clear_OfAnEntryResolvedElsewhere_IsReportedAsStale_NotClaimedAsCleared
#[test]
#[ignore = "native: needs desktop and isolated real disk"]
fn clearing_an_already_resolved_entry_reports_stale_without_another_journal_line() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let rig = RealRig::new();
        let e = rig.seed("e1", FileOperationType::Move, JournalState::Failed);
        let messages = fixture::FakeMessages::yes();
        let owner = desktop::create_owner();
        let d = rig.open(
            vec![e.clone()],
            messages.clone(),
            Arc::new(fixture::QueuedRunner::default()),
        );
        d.show_modeless(owner, None, None).unwrap();
        d.select_rows(&[0]);
        rig.journal.dismiss(&[e]).unwrap();
        let before = rig.tree();
        let journal = rig.journal_bytes();
        desktop::command(d.native(), CLEAR_SELECTED_ID, 0);
        assert_eq!(rig.tree(), before);
        assert_eq!(rig.journal_bytes(), journal);
        assert_eq!(d.with_presenter(|p| p.rows().len()), 0);
        assert_eq!(messages.calls.borrow().len(), 2);
        assert_eq!(messages.calls.borrow()[1].2, TaskDialogKind::Information);
        assert!(messages.calls.borrow()[1].1.contains("already resolved"));
        d.close();
        desktop::destroy_owner(owner);
    });
}

// C#: GroupDeleteRetry_WarnsAboutPermanentDeletion_UsingTheSettingReadWhenAsked (no file operation is accepted).
#[test]
#[ignore = "native: needs desktop and isolated real disk"]
fn group_delete_question_reads_the_current_permanent_delete_setting() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let rig = RealRig::new();
        let mut e = rig.seed("group", FileOperationType::Recycle, JournalState::Failed);
        e.group_members = Some(vec![JournalGroupMember {
            source: e.source.clone(),
            destination: None,
            size: e.size,
            last_write_utc: e.last_write_utc,
            permanent: true,
        }]);
        rig.journal.append(&e).unwrap();
        let before = rig.tree();
        let journal = rig.journal_bytes();
        let allow = Arc::new(AtomicBool::new(false));
        let setting = allow.clone();
        let mut options = fixture::options();
        options.allow_permanent_delete = Some(Arc::new(move || setting.load(Ordering::SeqCst)));
        options.retry = Some(Arc::new(|_| {
            panic!("question is always declined; no bin or file action is allowed")
        }));
        let messages = fixture::FakeMessages::yes();
        messages.answer.set(IDNO);
        let owner = desktop::create_owner();
        let d = RecoveryDialog::new(
            vec![e],
            options,
            fixture::services(),
            RecoveryDialogSeams {
                messages: Some(messages.clone()),
                auto_check: false,
                ..Default::default()
            },
            None,
        );
        d.show_modeless(owner, None, None).unwrap();
        d.select_rows(&[0]);
        desktop::command(d.native(), RETRY_ID, 0);
        allow.store(true, Ordering::SeqCst);
        desktop::command(d.native(), RETRY_ID, 0);
        assert_ne!(messages.calls.borrow()[0].1, messages.calls.borrow()[1].1);
        assert!(messages.calls.borrow()[1].1.to_lowercase().contains("permanent"));
        assert_eq!(rig.tree(), before);
        assert_eq!(rig.journal_bytes(), journal);
        assert_eq!(d.retry_runs(), 0);
        d.close();
        desktop::destroy_owner(owner);
    });
}
