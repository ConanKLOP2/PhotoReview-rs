//! Mutation killers (part 2) for `recovery_retry.rs` (W3-01 journal mutation debt,
//! `docs/reviews/W3-01-JOURNAL-MUTATION.md`). Scripted fakes only (no real disk, no Recycle Bin, no sleeps).

mod recovery_retry_support;

use pr_core::file_actions::errors::RETRY_VERIFY_FAILED;
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_retry::is_null_or_white_space;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use recovery_retry_support::*;

const SOURCE: &str = r"C:\photos\a.jpg";
const DEST: &str = r"C:\photos\sub\a.jpg";
const OLD_STAMP: i64 = CSHARP_CLOCK_TICKS - 10_000_000 * 3600;

// recovery_retry.rs:383:13 delete field error / 384:13 delete field error_code from the Prepared JournalEntry of retry_core
#[test]
fn single_retry_prepared_line_clears_the_old_error_and_code_and_takes_the_service_time() {
    let rig = Rig::new();
    rig.fs.seed_text(SOURCE, "12345", ts(CSHARP_CLOCK_TICKS));
    let stat = stat_of(&rig.fs, SOURCE);
    let failed = JournalEntry {
        error_code: Some(RETRY_VERIFY_FAILED.to_owned()),
        ..failed_entry(
            "op-1",
            FileOperationType::Move,
            SOURCE,
            DEST,
            stat.length as i64,
            stat.last_write_utc,
            ts(OLD_STAMP),
        )
    };
    rig.append(&failed);

    let result = rig
        .service()
        .retry_move_or_copy(&failed, false, &CancelToken::none())
        .expect("not cancelled");
    assert!(result.succeeded, "{}", result.message);

    let prepared: Vec<JournalEntry> = rig
        .entries()
        .into_iter()
        .filter(|e| e.state == JournalState::Prepared)
        .collect();
    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0].error, None);
    assert_eq!(prepared[0].error_code, None);
    assert_eq!(prepared[0].timestamp_utc, JournalDateTime::utc(ts(CSHARP_CLOCK_TICKS)));
    let committed = result.entry.expect("committed entry");
    assert_eq!(committed.error, None);
    assert_eq!(committed.error_code, None);
}

fn group_member(source: &str, destination: &str, size: i64) -> JournalGroupMember {
    member(source, Some(destination), size, ts(OLD_STAMP))
}

// recovery_retry.rs:494:13 delete field error_code / 495:13 delete field timestamp_utc from the Prepared JournalEntry of
// retry_group (error: the sibling mutant at :493 is caught by the existing tests)
#[test]
fn group_retry_prepared_line_clears_the_old_code_and_takes_the_service_time() {
    let jpg = r"C:\photos\g.jpg";
    let raw = r"C:\photos\g.cr2";
    let out_jpg = r"C:\photos\out\g.jpg";
    let out_raw = r"C:\photos\out\g.cr2";
    let service_ticks = CSHARP_CLOCK_TICKS + 120 * 10_000_000;
    let rig = Rig::with_clocks(CSHARP_CLOCK_TICKS + 60 * 10_000_000, service_ticks);
    rig.fs.seed_text(jpg, "jpeg", ts(OLD_STAMP));
    rig.fs.seed_text(raw, "raw data", ts(OLD_STAMP));
    let failed = JournalEntry {
        group_id: Some("capture".to_owned()),
        group_members: Some(vec![group_member(jpg, out_jpg, 4), group_member(raw, out_raw, 8)]),
        error_code: Some(RETRY_VERIFY_FAILED.to_owned()),
        ..entry(
            "grp",
            FileOperationType::Move,
            JournalState::Failed,
            jpg,
            Some(out_jpg),
            4,
            ts(OLD_STAMP),
            ts(OLD_STAMP),
            Some("earlier failure"),
        )
    };
    rig.append(&failed);

    let service = rig.service().with_recycle_bin(rig.bin(BinConfig::default()));
    let result = service
        .retry_move_or_copy(&failed, false, &CancelToken::none())
        .expect("not cancelled");
    assert!(result.succeeded, "{}", result.message);

    let prepared: Vec<JournalEntry> = rig
        .entries()
        .into_iter()
        .filter(|e| e.state == JournalState::Prepared)
        .collect();
    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0].error, None);
    assert_eq!(prepared[0].error_code, None);
    assert_eq!(prepared[0].timestamp_utc, JournalDateTime::utc(ts(service_ticks)));
}

// recovery_retry.rs:364:34 replace match guard !is_null_or_white_space(Some(destination)) with true
#[test]
fn retry_with_a_blank_destination_is_refused_as_no_destination() {
    for blank in ["", " ", "\t \r\n"] {
        let rig = Rig::new();
        rig.fs.seed_text(SOURCE, "12345", ts(CSHARP_CLOCK_TICKS));
        let failed = failed_entry(
            "op-blank",
            FileOperationType::Move,
            SOURCE,
            blank,
            5,
            ts(CSHARP_CLOCK_TICKS),
            ts(OLD_STAMP),
        );
        rig.append(&failed);
        let result = rig
            .service()
            .retry_move_or_copy(&failed, false, &CancelToken::none())
            .expect("not cancelled");
        assert!(!result.succeeded);
        assert_eq!(
            result.message,
            english().core_recovery_no_destination(),
            "destination {blank:?}"
        );
        assert!(rig.fs.has_file(SOURCE));
    }
}

// recovery_retry.rs:879:5 replace is_null_or_white_space -> bool with false
#[test]
fn is_null_or_white_space_follows_dotnet() {
    assert!(is_null_or_white_space(None));
    assert!(is_null_or_white_space(Some("")));
    assert!(is_null_or_white_space(Some(" \t\r\n")));
    assert!(!is_null_or_white_space(Some("a")));
    assert!(!is_null_or_white_space(Some(" a ")));
}
