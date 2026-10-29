//! Mutation killers (part 4) for `recovery_retry.rs` (W3-01 journal mutation debt,
//! `docs/reviews/W3-01-JOURNAL-MUTATION.md`). Scripted fakes only (no real disk, no Recycle Bin, no sleeps).

mod recovery_retry_support;

use recovery_retry_support::*;

// recovery_retry.rs:258:9 replace <impl fmt::Debug for RecoveryRetryService>::fmt -> fmt::Result with Ok(Default::default())
#[test]
fn recovery_retry_service_debug_names_the_type_and_reports_which_collaborators_are_set() {
    let rig = Rig::new();
    let bare = format!("{:?}", rig.service());
    assert!(bare.starts_with("RecoveryRetryService"), "{bare}");
    assert!(bare.contains("has_recycle_bin: false"), "{bare}");
    assert!(bare.contains("has_allow_permanent_delete: false"), "{bare}");
    assert!(bare.contains("has_file_action_gate: false"), "{bare}");

    let with_bin = format!("{:?}", rig.service().with_recycle_bin(rig.bin(BinConfig::default())));
    assert!(with_bin.contains("has_recycle_bin: true"), "{with_bin}");
    assert!(with_bin.contains("has_allow_permanent_delete: false"), "{with_bin}");
}

// recovery_retry.rs:385:13 delete field `size` from the Prepared JournalEntry of retry_core: UNOBSERVABLE (equivalent).
// precheck_single only returns the stat when `same_fingerprint` holds, i.e. `stat.length == failed.size`, so the
// `..failed.clone()` fallback carries the same size. No test can tell the two apart; no killer is written for it.
