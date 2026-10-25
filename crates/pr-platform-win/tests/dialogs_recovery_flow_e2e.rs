//! Selected native Retry/Dismiss flows ported from RecoveryDialogTests and RecoveryDialogGapTests.
use pr_app::presenters::recovery_window::RecoveryWindowOptions;
use pr_core::file_actions::journal::DismissOutcome;
use pr_core::file_actions::recovery_retry::RecoveryRetryResult;
use pr_platform_win::dialogs::native::controls::basic::ComboBoxControl;
use pr_platform_win::dialogs::recovery::{RecoveryDialog, RecoveryDialogSeams, layout::*};
use pr_platform_win::dialogs::task_dialog::TaskDialogKind;
use pr_platform_win::win32::messages::{IDCANCEL, IDNO};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/dialogs_recovery_fixture.rs"]
mod fixture;

fn dialog(
    options: RecoveryWindowOptions,
    messages: Rc<fixture::FakeMessages>,
    runner: Arc<fixture::QueuedRunner>,
) -> Rc<RecoveryDialog> {
    let mut services = fixture::services();
    services.runner = runner;
    Rc::new(RecoveryDialog::new(
        vec![fixture::entry("e1"), fixture::entry("e2")],
        options,
        services,
        RecoveryDialogSeams {
            messages: Some(messages),
            auto_check: false,
            ..Default::default()
        },
        None,
    ))
}

// C#: Retry_DeclinedAtTheConfirmation_NeverCallsTheDelegate and AConfirmationClosedWithoutAnswering_IsNotAYes_ForRetryOrClear
#[test]
#[ignore = "native: needs desktop"]
fn declined_or_unanswered_retry_and_clear_never_call_delegates() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        for answer in [IDNO, IDCANCEL] {
            let owner = desktop::create_owner();
            let messages = fixture::FakeMessages::yes();
            messages.answer.set(answer);
            let runner = Arc::new(fixture::QueuedRunner::default());
            let mut options = fixture::options();
            options.retry = Some(Arc::new(|_| panic!("declined retry must not run")));
            options.dismiss = Some(Box::new(|_| panic!("declined clear must not run")));
            let d = dialog(options, messages.clone(), runner.clone());
            d.show_modeless(owner, None, None).unwrap();
            d.select_rows(&[1]);
            for id in [RETRY_ID, CLEAR_SELECTED_ID, CLEAR_ALL_ID] {
                desktop::command(d.native(), id, 0);
            }
            assert_eq!(runner.count(), 0);
            assert_eq!(d.retry_runs(), 0);
            assert!(!d.is_busy());
            assert_eq!(d.with_presenter(|p| p.rows().len()), 2);
            assert_eq!(messages.calls.borrow().len(), 3);
            assert!(
                messages
                    .calls
                    .borrow()
                    .iter()
                    .all(|m| m.2 == TaskDialogKind::Confirmation)
            );
            d.close();
            desktop::destroy_owner(owner);
        }
    });
}

// C#: Retry_Confirmed_CallsTheDelegateOnceForTheSelectedEntry_ThenReportsAndCloses / WhileARetryRuns_RetryAndClearAreLocked_AndTheWindowRefusesEveryWayOfClosing
#[test]
#[ignore = "native: needs desktop"]
fn confirmed_retry_runs_selected_entry_once_locks_user_close_and_reports_then_closes() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let owner = desktop::create_owner();
        let messages = fixture::FakeMessages::yes();
        let runner = Arc::new(fixture::QueuedRunner::default());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let mut options = fixture::options();
        options.retry = Some(Arc::new(move |e| {
            recorded.lock().unwrap().push(e.id.clone());
            Ok(RecoveryRetryResult::new(true, "done", None))
        }));
        options.dismiss = Some(Box::new(|_| panic!("retry must not dismiss")));
        let d = dialog(options, messages.clone(), runner.clone());
        d.show_modeless(owner, None, None).unwrap();
        d.select_rows(&[1]);
        desktop::command(d.native(), RETRY_ID, 0);
        assert!(d.is_busy());
        assert!(!d.is_enabled(RETRY_ID));
        assert!(!d.is_enabled(CLEAR_SELECTED_ID));
        assert!(!d.is_enabled(CLEAR_ALL_ID));
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(messages.calls.borrow().len(), 1);
        assert_eq!(runner.count(), 1);
        desktop::command(d.native(), IDCANCEL, 0);
        assert_ne!(d.native().hwnd(), 0);
        desktop::command(d.native(), RETRY_ID, 0);
        assert_eq!(runner.count(), 1);
        runner.run_all();
        d.pump_completions();
        assert_eq!(&*calls.lock().unwrap(), &["e2"]);
        assert_eq!(d.retry_runs(), 1);
        assert!(!d.is_busy());
        assert_eq!(d.native().hwnd(), 0);
        assert_eq!(
            messages.calls.borrow().iter().map(|m| m.2).collect::<Vec<_>>(),
            vec![TaskDialogKind::Confirmation, TaskDialogKind::Information]
        );
        desktop::destroy_owner(owner);
    });
}

// C#: Retry_Rejected / Retry_WhenTheDelegateThrows / Retry_Superseded
#[test]
#[ignore = "native: needs desktop"]
fn rejected_failed_and_superseded_retry_release_busy_with_the_correct_follow_up() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        for mode in 0..3 {
            let owner = desktop::create_owner();
            let messages = fixture::FakeMessages::yes();
            let runner = Arc::new(fixture::QueuedRunner::default());
            let mut options = fixture::options();
            options.retry = Some(Arc::new(move |_| match mode {
                0 => Err("delegate failed".to_owned()),
                1 => Ok(RecoveryRetryResult::new(false, "rejected", None)),
                _ => {
                    let mut r = RecoveryRetryResult::new(false, "stale", None);
                    r.superseded = true;
                    Ok(r)
                }
            }));
            let d = dialog(options, messages.clone(), runner.clone());
            d.show_modeless(owner, None, None).unwrap();
            d.select_rows(&[1]);
            desktop::command(d.native(), RETRY_ID, 0);
            runner.run_all();
            d.pump_completions();
            assert!(!d.is_busy());
            assert_ne!(d.native().hwnd(), 0);
            assert_eq!(d.with_presenter(|p| p.rows().len()), if mode == 2 { 1 } else { 2 });
            assert_eq!(d.check_runs(), if mode == 1 { 1 } else { 0 });
            assert_eq!(messages.calls.borrow().len(), 2);
            assert_eq!(
                messages.calls.borrow()[1].2,
                if mode == 2 {
                    TaskDialogKind::Information
                } else {
                    TaskDialogKind::Error
                }
            );
            runner.run_all();
            d.pump_completions();
            d.close();
            desktop::destroy_owner(owner);
        }
    });
}

// C#: Retry_WhenTheWindowCloseWhileTheNoticeIsUp_DoesNotCheckAgainOnADeadWindow
#[test]
#[ignore = "native: needs desktop"]
fn closing_during_notice_prevents_a_recheck_on_the_destroyed_window() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let owner = desktop::create_owner();
        let messages = fixture::FakeMessages::yes();
        let runner = Arc::new(fixture::QueuedRunner::default());
        let mut options = fixture::options();
        options.retry = Some(Arc::new(|_| Ok(RecoveryRetryResult::new(false, "rejected", None))));
        let d = dialog(options, messages.clone(), runner.clone());
        let weak = Rc::downgrade(&d);
        *messages.on_show.borrow_mut() = Some(Rc::new(move |kind| {
            if kind != TaskDialogKind::Confirmation {
                if let Some(d) = weak.upgrade() {
                    d.close();
                }
            }
        }));
        d.show_modeless(owner, None, None).unwrap();
        d.select_rows(&[0]);
        desktop::command(d.native(), RETRY_ID, 0);
        runner.run_all();
        d.pump_completions();
        assert_eq!(d.native().hwnd(), 0);
        assert_eq!(d.check_runs(), 0);
        assert_eq!(runner.count(), 0);
        assert_eq!(messages.calls.borrow().len(), 2);
        desktop::destroy_owner(owner);
    });
}

// C#: ClosingTheShellMidRetry_ClosesTheWindowWithoutHanging_AndTheLateResultTouchesNoView
#[test]
#[ignore = "native: needs desktop"]
fn closing_mid_retry_keeps_the_file_job_and_suppresses_late_ui() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let owner = desktop::create_owner();
        let messages = fixture::FakeMessages::yes();
        let runner = Arc::new(fixture::QueuedRunner::default());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let mut options = fixture::options();
        options.retry = Some(Arc::new(move |e| {
            recorded.lock().unwrap().push(e.id.clone());
            Ok(RecoveryRetryResult::new(true, "finished after close", None))
        }));
        let d = dialog(options, messages.clone(), runner.clone());
        d.show_modeless(owner, None, None).unwrap();
        d.select_rows(&[0]);
        desktop::command(d.native(), RETRY_ID, 0);
        d.close();
        assert_eq!(d.native().hwnd(), 0);
        runner.run_all();
        d.pump_completions();
        assert_eq!(&*calls.lock().unwrap(), &["e1"]);
        assert_eq!(messages.calls.borrow().len(), 1);
        assert_eq!(d.check_runs(), 0);
        desktop::destroy_owner(owner);
    });
}

// C#: ClearAll_Confirmed_DismissesEveryRow_EvenOnesHiddenByTheFilter
#[test]
#[ignore = "native: needs desktop"]
fn clear_all_includes_rows_hidden_by_the_filter() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let owner = desktop::create_owner();
        let messages = fixture::FakeMessages::yes();
        let runner = Arc::new(fixture::QueuedRunner::default());
        let calls = Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = calls.clone();
        let mut options = fixture::options();
        options.dismiss = Some(Box::new(move |entries| {
            recorded
                .borrow_mut()
                .push(entries.iter().map(|e| e.id.clone()).collect::<Vec<_>>());
            Ok(DismissOutcome {
                dismissed: entries.to_vec(),
                skipped: vec![],
            })
        }));
        let d = dialog(options, messages, runner);
        d.show_modeless(owner, None, None).unwrap();
        ComboBoxControl::select(d.native().item(FILTER_ID), 1);
        desktop::command(d.native(), FILTER_ID, 1);
        assert!(d.visible_rows().is_empty());
        desktop::command(d.native(), CLEAR_ALL_ID, 0);
        assert_eq!(&*calls.borrow(), &[vec!["e1", "e2"]]);
        assert_eq!(d.with_presenter(|p| p.rows().len()), 0);
        assert!(!d.is_enabled(CLEAR_ALL_ID));
        d.close();
        desktop::destroy_owner(owner);
    });
}
