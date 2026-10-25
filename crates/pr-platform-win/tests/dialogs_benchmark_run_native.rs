//! Selected W8-18s desktop command checks. Other C# run scenarios remain open in PARITY.md.
#![cfg(windows)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use fixture::{activate_visual_styles, command, create_owner, destroy_owner, item, on_sta, text_of};
use pr_app::presenters::benchmark::{BenchmarkProfileSession, BenchmarkSessionFactory};
use pr_bench::engine::{BenchmarkError, BenchmarkProgress, BenchmarkProgressSink};
use pr_bench::models::{
    BenchmarkPhaseResult, BenchmarkProfile, BenchmarkReport, BenchmarkResultStatus, BenchmarkTimestamp,
};
use pr_core::sync::CancelToken;
use pr_l10n::localizer::Localizer;
use pr_platform_win::dialogs::benchmark::layout::{
    BROWSE_ID, CANCEL_RUN_ID, FOLDER_ID, OPEN_FOLDER_ID, OPEN_REPORT_ID, PROFILES_ID, RESULTS_ID, RUN_ID,
    SELECT_ALL_ID, SELECT_NONE_ID, STATUS_ID,
};
use pr_platform_win::dialogs::benchmark::{BenchmarkDialog, BenchmarkOptions};
use pr_platform_win::dialogs::native::controls::basic::ListBoxControl;
use pr_platform_win::dialogs::native::controls::list_tree::ListViewControl;
use pr_platform_win::dialogs::native::dialog::ID_CANCEL;
use pr_platform_win::win32::dialog_native;
use pr_test_support::TempRoot;

fn factory() -> BenchmarkSessionFactory {
    Arc::new(|_, _| panic!("these command checks must not create a benchmark session"))
}

struct SuccessfulSession {
    profile: BenchmarkProfile,
    disposed: Arc<AtomicUsize>,
}

struct CancellableSession {
    entered: Arc<AtomicBool>,
    disposed: Arc<AtomicUsize>,
}

impl BenchmarkProfileSession for CancellableSession {
    fn run(
        &mut self,
        _: &str,
        _: &dyn BenchmarkProgressSink,
        cancel: &CancelToken,
    ) -> Result<BenchmarkReport, BenchmarkError> {
        self.entered.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(20);
        while !cancel.is_canceled() {
            if Instant::now() >= deadline {
                return Err(BenchmarkError::Operation("test did not cancel the session".into()));
            }
            std::thread::yield_now();
        }
        Err(BenchmarkError::Canceled)
    }

    fn dispose(&mut self) {
        self.disposed.fetch_add(1, Ordering::SeqCst);
    }
}

// C#: BenchmarkDialogTests.CancelButton_StopsTheRun_AndTheWindowStaysOpen
// C#: BenchmarkDialogTests.Escape_WhileRunning_CancelsTheRun_ButDoesNotCloseTheWindow
// C#: BenchmarkDialogTests.Closing_MidRun_CancelsTheRun_DestroysTheWindow_AndLateProgressIsIgnored (close route)
#[test]
#[ignore = "native: needs desktop"]
fn cancel_escape_and_close_cancel_the_session_and_dispose_it() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        for action in [CANCEL_RUN_ID, ID_CANCEL, -1] {
            let entered = Arc::new(AtomicBool::new(false));
            let disposed = Arc::new(AtomicUsize::new(0));
            let session_entered = entered.clone();
            let session_disposed = disposed.clone();
            let session_factory: BenchmarkSessionFactory = Arc::new(move |_, _| {
                Ok(Box::new(CancellableSession {
                    entered: session_entered.clone(),
                    disposed: session_disposed.clone(),
                }))
            });
            let owner = create_owner();
            let dialog = BenchmarkDialog::new(
                Some("fake-folder"),
                session_factory,
                BenchmarkOptions {
                    pictures_folder: Some("fake-pictures".to_owned()),
                    scan: Some(Arc::new(|_, _, _| Ok(vec!["fake.jpg".to_owned()]))),
                    save: Some(Arc::new(|_| panic!("a canceled run must not save a summary"))),
                    ..Default::default()
                },
            );
            dialog
                .native()
                .show_modeless(owner, None, None, None)
                .expect("show benchmark");
            command(dialog.native(), RUN_ID, 0);
            let deadline = Instant::now() + Duration::from_secs(20);
            while !entered.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline, "session did not enter for action {action}");
                fixture::drain(dialog.native());
                std::thread::yield_now();
            }
            if action == -1 {
                dialog.native().close();
                assert_eq!(dialog.native().hwnd(), 0);
            } else {
                command(dialog.native(), action, 0);
                assert_ne!(dialog.native().hwnd(), 0, "cancel action must leave the window open");
            }
            while disposed.load(Ordering::SeqCst) == 0 || (action != -1 && dialog.presenter().borrow().is_running()) {
                assert!(
                    Instant::now() < deadline,
                    "canceled session did not finish for action {action}"
                );
                fixture::drain(dialog.native());
                std::thread::yield_now();
            }
            assert_eq!(disposed.load(Ordering::SeqCst), 1);
            assert!(dialog.presenter().borrow().rows().is_empty());
            assert!(dialog.presenter().borrow().last_report_path().is_none());
            dialog.native().close();
            destroy_owner(owner);
        }
    });
}

impl BenchmarkProfileSession for SuccessfulSession {
    fn run(
        &mut self,
        folder: &str,
        progress: &dyn BenchmarkProgressSink,
        _: &CancelToken,
    ) -> Result<BenchmarkReport, BenchmarkError> {
        progress.report(BenchmarkProgress {
            profile_id: self.profile.id.clone(),
            workload: self.profile.workload,
            completed: 3,
            total: 4,
            message: "OK".to_owned(),
        });
        Ok(BenchmarkReport {
            run_id: "desktop-fake".to_owned(),
            started_utc: BenchmarkTimestamp::unix_epoch(),
            folder: folder.to_owned(),
            phases: vec![BenchmarkPhaseResult::new(
                self.profile.id.clone(),
                self.profile.workload,
                vec![10.0, 20.0, 30.0],
                BenchmarkResultStatus::Pass,
                None,
                None,
            )],
            metrics: None,
            machine: None,
        })
    }

    fn dispose(&mut self) {
        self.disposed.fetch_add(1, Ordering::SeqCst);
    }
}

// C#: BenchmarkDialogTests.RunButton_RunsTheSelectedProfiles_ShowsProgress_LocksTheControls_ThenListsRows
// C#: BenchmarkDialogTests.OpenReportAndOpenFolder_UseTheLastSummary
#[test]
#[ignore = "native: needs desktop"]
fn run_selected_profile_lists_result_and_opens_its_saved_report() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let temp = TempRoot::new("benchmark-native-run");
        let summary = temp.join("summary.json").to_string_lossy().into_owned();
        let saved_path = summary.clone();
        let saved = Arc::new(AtomicUsize::new(0));
        let save_calls = saved.clone();
        let disposed = Arc::new(AtomicUsize::new(0));
        let session_disposed = disposed.clone();
        let sessions = Arc::new(AtomicUsize::new(0));
        let session_calls = sessions.clone();
        let session_factory: BenchmarkSessionFactory = Arc::new(move |profile, files| {
            assert_eq!(files, &["fake.jpg"]);
            session_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(SuccessfulSession {
                profile: profile.clone(),
                disposed: session_disposed.clone(),
            }))
        });
        let explorer = Rc::new(RefCell::new(Vec::new()));
        let explorer_calls = explorer.clone();
        let owner = create_owner();
        let dialog = BenchmarkDialog::new(
            Some(&temp.path().to_string_lossy()),
            session_factory,
            BenchmarkOptions {
                pictures_folder: Some(temp.path().to_string_lossy().into_owned()),
                scan: Some(Arc::new(|_, _, _| Ok(vec!["fake.jpg".to_owned()]))),
                save: Some(Arc::new(move |reports| {
                    assert_eq!(reports.len(), 1);
                    save_calls.fetch_add(1, Ordering::SeqCst);
                    std::fs::write(&saved_path, "[]").map_err(|e| e.to_string())?;
                    Ok(saved_path.clone())
                })),
                open_in_explorer: Some(Rc::new(move |args| explorer_calls.borrow_mut().push(args.to_owned()))),
                ..Default::default()
            },
        );
        dialog
            .native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        command(dialog.native(), RUN_ID, 0);
        assert!(dialog.presenter().borrow().is_running());
        assert!(!dialog_native::user32::is_window_enabled(item(dialog.native(), RUN_ID)));
        assert!(dialog_native::user32::is_window_enabled(item(
            dialog.native(),
            CANCEL_RUN_ID
        )));
        let deadline = Instant::now() + Duration::from_secs(20);
        while dialog.presenter().borrow().is_running() {
            assert!(Instant::now() < deadline, "benchmark run did not complete");
            fixture::drain(dialog.native());
            std::thread::yield_now();
        }
        assert_eq!(sessions.load(Ordering::SeqCst), 1);
        assert_eq!(disposed.load(Ordering::SeqCst), 1);
        assert_eq!(saved.load(Ordering::SeqCst), 1);
        assert_eq!(dialog.presenter().borrow().rows().len(), 1);
        let grid = item(dialog.native(), RESULTS_ID);
        assert_eq!(ListViewControl::count(grid), 1);
        assert_eq!(ListViewControl::cell_text(grid, 0, 1), "Fast Sequential");
        assert!(!ListViewControl::cell_text(grid, 0, 0).is_empty());
        assert!(dialog_native::user32::is_window_enabled(item(dialog.native(), RUN_ID)));
        assert!(!dialog_native::user32::is_window_enabled(item(
            dialog.native(),
            CANCEL_RUN_ID
        )));
        assert_eq!(dialog.presenter().borrow().last_report_path(), Some(summary.as_str()));
        command(dialog.native(), OPEN_REPORT_ID, 0);
        command(dialog.native(), OPEN_FOLDER_ID, 0);
        assert_eq!(
            &*explorer.borrow(),
            &[
                format!("/select,\"{summary}\""),
                temp.path().to_string_lossy().into_owned()
            ]
        );
        dialog.native().close();
        destroy_owner(owner);
    });
}

// C#: BenchmarkDialogTests.SelectAllAndSelectNone_ToggleEveryProfile
#[test]
#[ignore = "native: needs desktop"]
fn select_all_and_select_none_toggle_every_profile() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let owner = create_owner();
        let dialog = BenchmarkDialog::new(
            None,
            factory(),
            BenchmarkOptions {
                pictures_folder: Some("pictures-fixture".to_owned()),
                ..Default::default()
            },
        );
        dialog
            .native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        let list = item(dialog.native(), PROFILES_ID);
        command(dialog.native(), SELECT_NONE_ID, 0);
        assert!(ListBoxControl::selected_indices(list).is_empty());
        command(dialog.native(), SELECT_ALL_ID, 0);
        assert_eq!(
            ListBoxControl::selected_indices(list),
            (0..ListBoxControl::count(list)).collect::<Vec<_>>()
        );
        dialog.native().close();
        destroy_owner(owner);
    });
}

// C#: BenchmarkDialogTests.Browse_AsksTheWindowsOwnPicker_AndFillsTheFolderBox
#[test]
#[ignore = "native: needs desktop"]
fn browse_picker_fills_the_folder_box() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let temp = TempRoot::new("benchmark-browse");
        let missing = temp.join("missing").to_string_lossy().into_owned();
        let chosen = temp.join("chosen").to_string_lossy().into_owned();
        let picked = chosen.clone();
        let owner = create_owner();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let recorded = calls.clone();
        let dialog = BenchmarkDialog::new(
            Some(&missing),
            factory(),
            BenchmarkOptions {
                pictures_folder: Some("pictures-fixture".to_owned()),
                pick_folder: Some(Rc::new(move |hwnd, title, initial| {
                    recorded
                        .borrow_mut()
                        .push((hwnd, title.to_owned(), initial.map(str::to_owned)));
                    Some(picked.clone())
                })),
                ..Default::default()
            },
        );
        dialog
            .native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        command(dialog.native(), BROWSE_ID, 0);
        assert_eq!(text_of(item(dialog.native(), FOLDER_ID)), chosen);
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(calls.borrow()[0].0, dialog.native().hwnd());
        assert_eq!(calls.borrow()[0].1, pr_l10n::tr::benchmark_pick_folder_title());
        assert_eq!(calls.borrow()[0].2, None);
        dialog.native().close();
        destroy_owner(owner);
    });
}

// C#: BenchmarkDialogTests.Browse_Cancelled_LeavesTheFolderBoxAlone
#[test]
#[ignore = "native: needs desktop"]
fn browse_cancelled_leaves_folder_box_alone() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let owner = create_owner();
        let dialog = BenchmarkDialog::new(
            Some("start-fixture"),
            factory(),
            BenchmarkOptions {
                pictures_folder: Some("pictures-fixture".to_owned()),
                pick_folder: Some(Rc::new(|_, _, _| None)),
                ..Default::default()
            },
        );
        dialog
            .native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        command(dialog.native(), BROWSE_ID, 0);
        assert_eq!(text_of(item(dialog.native(), FOLDER_ID)), "start-fixture");
        dialog.native().close();
        destroy_owner(owner);
    });
}

// C#: BenchmarkDialogTests.Run_WithNothingSelected_SaysSo_AndStartsNothing
#[test]
#[ignore = "native: needs desktop"]
fn run_with_nothing_selected_says_so_and_starts_nothing() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let owner = create_owner();
        let dialog = BenchmarkDialog::new(
            None,
            factory(),
            BenchmarkOptions {
                pictures_folder: Some("pictures-fixture".to_owned()),
                ..Default::default()
            },
        );
        dialog
            .native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        command(dialog.native(), SELECT_NONE_ID, 0);
        command(dialog.native(), RUN_ID, 0);
        assert!(!dialog.presenter().borrow().is_running());
        assert!(!text_of(item(dialog.native(), STATUS_ID)).is_empty());
        dialog.native().close();
        destroy_owner(owner);
    });
}

// C#: BenchmarkDialogTests.EveryProfileHasATooltipWithItsDescription_AndABadgeWithItsLoadingMode
#[test]
#[ignore = "native: needs desktop"]
fn every_profile_has_badge_with_its_loading_mode() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let owner = create_owner();
        let dialog = BenchmarkDialog::new(
            None,
            factory(),
            BenchmarkOptions {
                pictures_folder: Some("pictures-fixture".to_owned()),
                ..Default::default()
            },
        );
        dialog
            .native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        let items = dialog.presenter().borrow().profile_items().to_vec();
        for (index, item) in items.iter().enumerate() {
            let expected = pr_app::presenters::benchmark_presentation::loading_mode_text(
                item.profile.loading_mode,
                &Localizer::current(),
            );
            assert_eq!(dialog.profile_badge(index as i32), Some(expected));
        }
        assert_eq!(dialog.profile_badge(-1), None);
        dialog.native().close();
        destroy_owner(owner);
    });
}
