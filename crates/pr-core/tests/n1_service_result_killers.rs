//! N1 killers (mutation close-out 2026-11-05) for survivors of the F-21 service side in `file_actions/service.rs`
//! (`execute_rename`, `single_failure`, `group_failure`, `inspect_group_member`): result fields that no earlier test read, and
//! the rename Dismissed/Failed rule. Each test names the mutants it kills. Hand-written, no golden file, no sleep; everything
//! runs on the in-memory disk of the shared fixture (nothing touches the real Recycle Bin).
//!
//! * `318:17` delete `rejected` in `execute_rename`
//! * `641:21`, `641:24` in `single_failure` (the rename Dismissed rule: prepared, not completed, untouched)
//! * `682:13` delete `journal_error` in `single_failure`
//! * `683:48` `&&` -> `||` in `permanently_deleted: run.permanent && mutation_completed`
//! * `1036:13` delete `journal_error`, `1038:13` delete `skipped_missing` in `group_failure`
//! * `1286:30` `==` -> `!=` (Move compares the restored source with the tolerant fingerprint), `1294:17` delete `error`,
//!   `1294:25` delete `!` in `inspect_group_member`

mod common_file_actions;

use std::io;
use std::path::Path;
use std::sync::Arc;

use common_file_actions::{
    Fixture, LAST_WRITE_TICKS, english_texts, fail_journal_append_nth, group, last_write_plus, pattern_bytes, single,
    stamp,
};
use pr_core::abstractions::FileSystem;
use pr_core::file_actions::rename::RenameRequest;
use pr_core::file_actions::undo::MoveOverride;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;

use FileOperationType::{Move, Recycle};

const SOURCE: &str = r"C:\photos\a.jpg";
const JPEG: &str = r"C:\photos\IMG_0001.JPG";
const RAW: &str = r"C:\photos\IMG_0001.CR3";
const XMP: &str = r"C:\photos\IMG_0001.xmp";
const SEL: &str = r"C:\photos\sel";

fn rename_request() -> RenameRequest {
    RenameRequest {
        source: SOURCE.to_owned(),
        new_file_name: "b.jpg".to_owned(),
    }
}

fn is(path: &Path, expected: &str) -> bool {
    path.to_string_lossy().eq_ignore_ascii_case(expected)
}

// 318:17: a rename refused because the gate is busy is flagged `rejected` (a plain failure is not).
#[test]
fn rename_while_the_gate_is_busy_is_rejected_and_touches_nothing() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let held = fx.service.try_begin().expect("the gate is free");
    let result = fx.service.rename(&rename_request(), &CancelToken::new());
    assert!(result.rejected);
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_texts_busy().as_str()),
        "the busy message of the catalog"
    );
    assert!(fx.file_exists(SOURCE) && !fx.file_exists(r"C:\photos\b.jpg"));
    assert!(fx.journal_lines().is_empty());
    drop(held);
}

fn english_texts_busy() -> String {
    use pr_core::file_actions::service::FileActionTexts;
    english_texts().core_file_action_busy()
}

// 641:24 (and the whole rename rule): a rename that fails after Prepared while the file is untouched is Dismissed.
#[test]
fn rename_that_fails_before_touching_the_file_is_dismissed() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks
        .set_move_hook(Some(Arc::new(|_: &Path, _: &Path| Some(io::Error::other("no")))));
    let result = fx.service.rename(&rename_request(), &CancelToken::new());
    assert!(!result.succeeded);
    assert!(fx.file_exists(SOURCE));
    let states: Vec<JournalState> = fx.journal_entries().into_iter().map(|e| e.state).collect();
    assert_eq!(states, vec![JournalState::Prepared, JournalState::Dismissed]);
}

// 641:21: a rename whose move happened and then failed (source gone, destination there) is NOT untouched: Failed.
#[test]
fn rename_that_fails_after_the_file_moved_is_failed_not_dismissed() {
    let fx = Fixture::with_move_override(|disk| -> MoveOverride {
        Arc::new(move |source: &Path, destination: &Path| {
            disk.move_file(source, destination)?;
            Err(io::Error::other("late failure"))
        })
    });
    fx.add_file(SOURCE, 11);
    let result = fx.service.rename(&rename_request(), &CancelToken::new());
    assert!(!result.succeeded);
    assert!(result.source_removed);
    let states: Vec<JournalState> = fx.journal_entries().into_iter().map(|e| e.state).collect();
    assert_eq!(states, vec![JournalState::Prepared, JournalState::Failed]);
}

// 682:13: the journal error of the Failed line's own append failure reaches the result.
#[test]
fn single_failure_reports_the_journal_error_of_the_failed_line() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_move_hook(Some(Arc::new(|_: &Path, _: &Path| {
        Some(io::Error::other("disk on fire"))
    })));
    // Prepared is append 1, the Failed line is append 2.
    fail_journal_append_nth(&fx, 2, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(!result.journal_persisted);
    assert_eq!(result.journal_error.as_deref(), Some("journal unavailable"));
}

// 683:48: a permanent delete that failed did not delete permanently (`run.permanent && mutation_completed`).
#[test]
fn failed_permanent_delete_does_not_report_permanently_deleted() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    fx.bin.fail_permanent(SOURCE, "access denied");
    let mut request = single(SOURCE, Recycle, None);
    request.allow_permanent_delete = true;
    let result = fx.run(&request);
    assert!(!result.succeeded);
    assert!(!result.permanently_deleted);
    assert!(fx.file_exists(SOURCE));
}

fn add_triple(fx: &Fixture) {
    fx.add_file(JPEG, 40);
    fx.add_file(RAW, 60);
    fx.add_file(XMP, 5);
}

// 1036:13: the journal error of a group's Failed line reaches the result (the Recycle path is never rolled back, so the
// outcome line is Failed here, not Dismissed).
#[test]
fn group_failure_reports_the_journal_error_of_the_outcome_line() {
    let fx = Fixture::new();
    fx.add_file(JPEG, 40);
    fx.add_file(RAW, 60);
    fx.bin.fail_send(RAW, "shell said no");
    fail_journal_append_nth(&fx, 2, "journal unavailable");
    let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
    assert!(!result.succeeded);
    assert!(!result.journal_persisted);
    assert_eq!(result.journal_error.as_deref(), Some("journal unavailable"));
}

// 1038:13: a partner that vanished before the action is listed in `skipped_missing` of a FAILED group too.
#[test]
fn group_failure_lists_the_partner_that_was_missing() {
    let fx = Fixture::new();
    add_triple(&fx);
    fx.disk.delete(Path::new(XMP)).expect("the sidecar vanished");
    fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
        if is(source, RAW) {
            Some(io::Error::other("boom"))
        } else {
            None
        }
    })));
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(result.skipped_missing, Some(vec![XMP.to_owned()]));
}

// 1294:17 and 1294:25: after a failed Move group whose rollback could not put the first member back, the member that
// stayed moved is `completed` with NO error, the member that never moved carries the original failure message.
#[test]
fn group_failure_member_error_is_set_only_for_members_that_did_not_complete() {
    let fx = Fixture::new();
    fx.add_file(JPEG, 40);
    fx.add_file(RAW, 60);
    fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
        if is(source, RAW) {
            Some(io::Error::other("boom"))
        } else if source.to_string_lossy().to_ascii_lowercase().contains(r"\sel\") {
            Some(io::Error::other("cannot put it back"))
        } else {
            None
        }
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    let jpeg = result
        .members
        .iter()
        .find(|m| is(Path::new(&m.member.source), JPEG))
        .expect("JPEG member");
    let raw = result
        .members
        .iter()
        .find(|m| is(Path::new(&m.member.source), RAW))
        .expect("RAW member");
    assert!(jpeg.completed, "the JPEG stayed in the destination: {jpeg:?}");
    assert_eq!(jpeg.error, None);
    assert!(!raw.completed);
    assert_eq!(raw.error.as_deref(), Some("boom"));
    let _ = SEL;
}

// 1286:30: for a Move group the source of a member that never left is compared with the TOLERANT fingerprint (two seconds), so
// a source whose stamp moved by one second is still "unchanged": no conflict, the group is rolled back (Dismissed).
#[test]
fn group_move_failure_with_a_source_stamp_one_second_off_is_still_rolled_back() {
    let fx = Fixture::new();
    fx.add_file(JPEG, 40);
    fx.add_file(RAW, 60);
    let disk = fx.disk.clone();
    fx.hooks.set_move_hook(Some(Arc::new(move |source: &Path, _: &Path| {
        if is(source, JPEG) {
            // The first move fails, and the not yet moved RAW gets a stamp one second off (round-trip precision loss).
            disk.add_file_at(RAW, &pattern_bytes(60), last_write_plus(1));
            Some(io::Error::other("boom"))
        } else {
            None
        }
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    let raw = result
        .members
        .iter()
        .find(|m| is(Path::new(&m.member.source), RAW))
        .expect("RAW member");
    assert!(
        !raw.conflict,
        "a Move source within the tolerance is unchanged: {raw:?}"
    );
    assert!(raw.source_exists);
    let last = fx.journal_entries().pop().expect("an outcome line");
    assert_eq!(last.state, JournalState::Dismissed);
    let _ = (stamp(LAST_WRITE_TICKS), SEL);
}
