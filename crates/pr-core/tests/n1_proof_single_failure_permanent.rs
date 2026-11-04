//! N1 run-proof (mutation close-out 2026-11-05) for the `file_actions/service.rs:683:13` survivor in `single_failure`:
//! `delete field permanently_deleted` (the field is `run.permanent && mutation_completed`; its default is `false`).
//!
//! `mutation_completed` is set by `mark_mutation_completed()` as the LAST step before `commit()`, and `commit()` cannot
//! fail, so a request that reaches `single_failure` never has it set; the field is always `false` there. This file runs
//! every exit of the permanent-delete arm of `run_single` that returns an error AFTER `run.permanent` was decided:
//!
//! 1. permanent delete not opted in (refused before the Prepared line);
//! 2. the Prepared append fails;
//! 3. cancelled at the hand-off after Prepared;
//! 4. the source changed between the first stat and the re-stat (size);
//! 5. the source vanished between the first stat and the re-stat;
//! 6. `delete_permanently` fails;
//! 7. `delete_permanently` reports success while the file is still there.
//!
//! and asserts `permanently_deleted == false` for each, and `true` for the successful permanent delete. It is meant to PASS
//! with the original code and with the hand-applied mutant. The completeness of the list of exits is a reading of
//! `run_single`; the run proves the field for each exit.

mod common_file_actions;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common_file_actions::{Fixture, fail_journal_append_nth, single};
use pr_core::file_actions::types::FileActionResult;
use pr_core::model::FileOperationType::Recycle;
use pr_core::sync::CancelToken;

const SOURCE: &str = r"C:\photos\a.jpg";

fn permanent_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    fx
}

fn permanent_request(allow: bool) -> pr_core::file_actions::types::FileActionRequest {
    let mut request = single(SOURCE, Recycle, None);
    request.allow_permanent_delete = allow;
    request
}

fn assert_failed_without_permanent_delete(result: &FileActionResult, what: &str) {
    assert!(!result.succeeded, "{what}");
    assert!(!result.permanently_deleted, "{what}");
}

#[test]
fn every_failing_exit_of_the_permanent_delete_arm_reports_permanently_deleted_false() {
    // 1. not opted in
    let fx = permanent_fixture();
    assert_failed_without_permanent_delete(&fx.run(&permanent_request(false)), "not opted in");

    // 2. the Prepared append fails (append 1)
    let fx = permanent_fixture();
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    assert_failed_without_permanent_delete(&fx.run(&permanent_request(true)), "Prepared append fails");

    // 3. cancelled at the hand-off
    let fx = permanent_fixture();
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_failed_without_permanent_delete(&fx.run_with(&permanent_request(true), &cancel), "cancelled");

    // 4. the source changed between the first stat and the re-stat
    let fx = permanent_fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    fx.faulty
        .set_stat_filter(Some(Arc::new(move |path: &std::path::Path, stat| {
            if path.to_string_lossy().eq_ignore_ascii_case(SOURCE) && seen.fetch_add(1, Ordering::SeqCst) >= 1 {
                return stat.map(|mut s| {
                    s.length += 1;
                    s
                });
            }
            stat
        })));
    assert_failed_without_permanent_delete(&fx.run(&permanent_request(true)), "source changed at the re-stat");
    assert!(calls.load(Ordering::SeqCst) >= 2, "the re-stat happened");
    assert!(fx.file_exists(SOURCE));

    // 5. the source vanished between the first stat and the re-stat
    let fx = permanent_fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    fx.faulty
        .set_stat_filter(Some(Arc::new(move |path: &std::path::Path, stat| {
            if path.to_string_lossy().eq_ignore_ascii_case(SOURCE) && seen.fetch_add(1, Ordering::SeqCst) >= 1 {
                return None;
            }
            stat
        })));
    assert_failed_without_permanent_delete(&fx.run(&permanent_request(true)), "source vanished at the re-stat");

    // 6. delete_permanently fails
    let fx = permanent_fixture();
    fx.bin.fail_permanent(SOURCE, "access denied");
    assert_failed_without_permanent_delete(&fx.run(&permanent_request(true)), "delete fails");

    // 7. delete_permanently reports success, the file stays
    let fx = permanent_fixture();
    fx.bin.leave_on_permanent(SOURCE);
    assert_failed_without_permanent_delete(&fx.run(&permanent_request(true)), "file still there");
}

#[test]
fn the_successful_permanent_delete_reports_permanently_deleted_true() {
    let fx = permanent_fixture();
    let result = fx.run(&permanent_request(true));
    assert!(result.succeeded);
    assert!(result.permanently_deleted);
}
