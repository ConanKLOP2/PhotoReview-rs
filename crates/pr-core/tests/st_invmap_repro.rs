//! Lane `st-invmap` (2026-10-31): repro of docs/reviews/ST-INVMAP-2026-10-31.md finding H1 (BACKLOG G25).
//!
//! `RecoveryRetryService` group retry of a *permanent* Recycle member re-checks the file fingerprint only BEFORE the
//! Prepared line is appended (`precheck_group_members`). `run_group_members` (recovery_retry.rs, Recycle branch) then
//! calls `delete_permanently` without a fresh stat, unlike the Move/Copy branch. A different file that takes the path
//! between the pre-check and the mutation is therefore deleted permanently under the old operation's fingerprint.
//!
//! This test is EXPECTED TO FAIL on master until the production code re-stats right before the delete. Fakes only
//! (`ScriptedFs`, `ScriptedBin`): the real Recycle Bin is never touched.
#[path = "recovery_retry_support/mod.rs"]
mod recovery_retry_support;

use pr_core::file_actions::journal_line::{JournalEntry, JournalGroupMember};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use recovery_retry_support::*;
use std::sync::Arc;

const STAMP: i64 = 639_264_384_000_000_000;
const JPG: &str = r"C:\photos\a.jpg";
const RAW: &str = r"C:\photos\a.cr2";

fn failed_permanent_delete_group() -> JournalEntry {
    let members = vec![
        member(JPG, None, 4, ts(STAMP)),
        JournalGroupMember {
            permanent: true,
            ..member(RAW, None, 8, ts(STAMP))
        },
    ];
    JournalEntry {
        permanent: Some(true),
        group_id: Some("capture".to_owned()),
        group_members: Some(members),
        ..entry(
            "del",
            FileOperationType::Recycle,
            JournalState::Failed,
            JPG,
            None,
            4,
            ts(STAMP),
            ts(STAMP),
            Some("x"),
        )
    }
}

// Not a C# port: new test for finding H1 of ST-INVMAP-2026-10-31.
#[test]
#[ignore = "H1/G25 open: red until recovery_retry re-stats before the permanent delete; run with --ignored"]
fn retry_group_permanent_member_replaced_between_precheck_and_mutation_is_not_deleted() {
    let rig = Rig::with_clocks(STAMP + 600_000_000, STAMP + 1_200_000_000);
    rig.fs.seed_text(JPG, "jpeg", ts(STAMP));
    rig.fs.seed_text(RAW, "raw data", ts(STAMP));
    let bin = rig.bin(BinConfig {
        no_bin: vec![RAW.to_owned()],
        ..BinConfig::default()
    });
    let failed = failed_permanent_delete_group();
    rig.append(&failed);

    // The pre-check (size 8, stamp) passes; right when the Prepared line is appended another program puts a
    // DIFFERENT file (other size and write time) at the same path.
    let fs = rig.fs.clone();
    let replaced = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = replaced.clone();
    rig.fs.add_rule(Rule {
        on: "append",
        path: None,
        nth: None,
        f: Arc::new(move |_fs: &ScriptedFs, _call: &Call<'_>| {
            if !flag.swap(true, std::sync::atomic::Ordering::SeqCst) {
                fs.seed_text(RAW, "a brand new, longer replacement photo", ts(STAMP + 5_000_000_000));
            }
            Ok(Next::Proceed)
        }),
    });
    let service = rig
        .service()
        .with_recycle_bin(bin)
        .with_allow_permanent_delete(Rig::allow(true));

    let result = service
        .retry_move_or_copy(&failed, false, &CancelToken::none())
        .expect("not cancelled");

    let deleted: Vec<String> = rig
        .fs
        .ops()
        .iter()
        .filter_map(|o| o.strip_prefix("bin.delete-permanently ").map(str::to_owned))
        .collect();
    assert!(replaced.load(std::sync::atomic::Ordering::SeqCst), "hook ran");
    assert!(
        !result.succeeded,
        "retry reported success after deleting a file it never fingerprinted"
    );
    assert!(
        deleted.is_empty(),
        "the replacement file was permanently deleted: {deleted:?}"
    );
    assert!(rig.fs.has_file(RAW), "the replacement file must still exist");
}
