//! W8-18v: C# dismiss golden and retry/dismiss decisions without a native window.
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

#[path = "common/dialogs_recovery_fixture.rs"]
mod fixture;

use pr_app::presenters::recovery_window::{DismissFn, RecoveryDismissResult, RecoveryRetryNotice};
use pr_core::file_actions::journal::DismissOutcome;
use pr_platform_win::dialogs::recovery::{RecoveryDialog, RecoveryDialogSeams, dismiss, restore};
use pr_platform_win::win32::messages::{IDNO, IDYES};

// C#: RecoveryDialog.Dismiss oracle (12 cases, no window).
#[test]
fn dismiss_matches_every_csharp_golden_case() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/recovery-dialog-dismiss.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 12);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let entries = case["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| fixture::entry(v.as_str().unwrap()))
            .collect();
        let rows = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as usize)
            .collect();
        let skip: HashSet<String> = case["skip"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        let throws = case["mode"] == "throws";
        let calls = Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
        let recorded = calls.clone();
        let delegate: Option<DismissFn> = case["has_delegate"].as_bool().unwrap().then(|| {
            Box::new(move |entries: &[pr_core::file_actions::journal_line::JournalEntry]| {
                recorded
                    .borrow_mut()
                    .push(entries.iter().map(|e| e.id.clone()).collect());
                if throws {
                    return Err("journal locked".to_owned());
                }
                let (skipped, dismissed) = entries.iter().cloned().partition(|e| skip.contains(&e.id));
                Ok(DismissOutcome { dismissed, skipped })
            }) as DismissFn
        });
        let mut options = fixture::options();
        options.dismiss = delegate;
        let messages = fixture::FakeMessages::yes();
        messages
            .answer
            .set(if case["answer"].as_bool().unwrap() { IDYES } else { IDNO });
        let dialog = RecoveryDialog::new(
            entries,
            options,
            fixture::services(),
            RecoveryDialogSeams {
                messages: Some(messages.clone()),
                auto_check: false,
                ..Default::default()
            },
            None,
        );
        dismiss::dismiss(dialog.content(), dialog.native(), rows);
        let actual_messages:Vec<_>=messages.calls.borrow().iter().map(|(title,message,kind)|serde_json::json!({"title":title,"message":message,"kind":format!("{kind:?}")})).collect();
        assert_eq!(serde_json::json!(actual_messages), case["messages"], "{name}");
        assert_eq!(serde_json::json!(&*calls.borrow()), case["delegate_calls"], "{name}");
        let left = dialog.with_presenter(|p| p.rows().iter().map(|r| r.entry().id.clone()).collect::<Vec<_>>());
        assert_eq!(serde_json::json!(left), case["rows_left"], "{name}");
    }
}

// C#: RecoveryDialog.RetryAsync / SetBusy and Dismiss follow-up branches.
#[test]
fn retry_notice_and_dismiss_follow_up_keep_csharp_precedence() {
    for (close, recheck, expected) in [
        (true, true, restore::AfterNotice::Close),
        (true, false, restore::AfterNotice::Close),
        (false, true, restore::AfterNotice::Recheck),
        (false, false, restore::AfterNotice::Nothing),
    ] {
        assert_eq!(
            restore::after_notice(&RecoveryRetryNotice {
                title: String::new(),
                message: String::new(),
                is_warning: false,
                close_window: close,
                recheck
            }),
            expected
        );
    }
    assert_eq!(restore::cursor_id(true), restore::CURSOR_WAIT);
    assert_eq!(restore::cursor_id(false), restore::CURSOR_ARROW);
    for has_window in [false, true] {
        assert_eq!(
            dismiss::follow_up(
                &RecoveryDismissResult {
                    error: Some("failed".to_owned()),
                    skipped_count: 2
                },
                has_window
            ),
            dismiss::DismissFollowUp::ShowError("failed".to_owned())
        );
    }
    assert_eq!(
        dismiss::follow_up(
            &RecoveryDismissResult {
                error: None,
                skipped_count: 2
            },
            false
        ),
        dismiss::DismissFollowUp::Stop
    );
    for skipped in [0, 2] {
        assert_eq!(
            dismiss::follow_up(
                &RecoveryDismissResult {
                    error: None,
                    skipped_count: skipped
                },
                true
            ),
            dismiss::DismissFollowUp::Refresh { skipped }
        );
    }
}

// C#: RecoveryDialogGapTests.Retry_WithNothingSelected_AsksNothing.
#[test]
fn retry_without_selection_asks_nothing_and_busy_without_window_is_only_state() {
    let messages = fixture::FakeMessages::yes();
    let dialog = RecoveryDialog::new(
        vec![fixture::entry("e1")],
        fixture::options(),
        fixture::services(),
        RecoveryDialogSeams {
            messages: Some(messages.clone()),
            auto_check: false,
            ..Default::default()
        },
        None,
    );
    restore::retry(dialog.content(), dialog.native());
    assert!(messages.calls.borrow().is_empty());
    assert_eq!(dialog.retry_runs(), 0);
    restore::apply_busy(dialog.content(), dialog.native(), true);
    assert!(dialog.is_busy());
    restore::apply_busy(dialog.content(), dialog.native(), false);
    assert!(!dialog.is_busy());
}
