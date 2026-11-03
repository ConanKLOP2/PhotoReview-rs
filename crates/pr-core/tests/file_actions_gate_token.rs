//! P-71 option C: the INV-4 gate hands out a release token; only the holder of a successful `try_begin` can release it.
//! Before this change `FileActionService::end()` was `pub` and stored 0 unconditionally, so a stale or duplicate `end()`
//! cleared the in-progress flag of a LATER action. Fakes only (in-memory disk); nothing touches the real Recycle Bin.

mod common_file_actions;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common_file_actions::{Fixture, single};
use pr_core::file_actions::undo::{GateFlag, GateToken};
use pr_core::model::FileOperationType::Move;

const SOURCE: &str = r"C:\photos\a.jpg";

// The old hazard: action A finished, action B runs, a second release of A must not free B's gate. Now A's token is
// consumed by its first release, and B's gate can only be freed by B's token.
#[test]
fn a_finished_actions_token_never_frees_a_later_actions_gate() {
    let fx = Fixture::new();
    let a = fx.service.try_begin().expect("A takes the free gate");
    a.release();
    let b = fx.service.try_begin().expect("B takes the gate A released");
    assert!(fx.service.is_busy(), "B is running");
    assert!(fx.service.try_begin().is_none(), "nobody else gets in while B runs");
    assert!(fx.service.is_busy(), "the refused begin released nothing");
    drop(b);
    assert!(!fx.service.is_busy(), "only B's own token frees B's gate");
}

// The flag remembers WHICH token holds it (compare-exchange id -> 0), so a release only ever frees its own hold.
#[test]
fn gate_flag_hands_the_gate_to_the_next_holder_after_a_release() {
    let flag = Arc::new(GateFlag::new());
    let first = flag.try_begin().expect("free");
    first.release();
    let second = flag.try_begin().expect("free again");
    assert!(flag.is_busy());
    drop(second);
    assert!(!flag.is_busy());
}

#[test]
fn failed_begin_yields_no_token_and_the_holder_keeps_the_gate() {
    let fx = Fixture::new();
    let held = fx.service.try_begin().expect("free");
    let refused: Option<GateToken> = fx.service.try_begin();
    assert!(refused.is_none());
    drop(refused);
    assert!(fx.service.is_busy(), "the loser held no token, so nothing was released");
    drop(held);
    assert!(!fx.service.is_busy());
}

#[test]
fn dropping_or_releasing_the_token_runs_the_release_exactly_once() {
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&runs);
    let token = GateToken::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(runs.load(Ordering::SeqCst), 0, "creating a token releases nothing");
    token.release();
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    let counted = Arc::clone(&runs);
    drop(GateToken::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[test]
fn a_panic_while_holding_the_token_still_frees_the_gate() {
    let fx = Fixture::new();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let _held = fx.service.try_begin().expect("free");
        panic!("simulated failure while the gate is held");
    }));
    assert!(outcome.is_err());
    assert!(!fx.service.is_busy());
}

#[test]
fn execute_while_held_is_rejected_without_releasing_the_holder_then_runs_and_frees_the_gate() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let held = fx.service.try_begin().expect("free");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(result.rejected);
    assert!(fx.service.is_busy(), "the rejected call did not own the gate");
    assert!(fx.journal_lines().is_empty());
    assert!(fx.file_exists(SOURCE));
    drop(held);

    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(result.succeeded && !result.rejected, "{:?}", result.error);
    assert!(!fx.service.is_busy(), "the execute path released its own token");
}
