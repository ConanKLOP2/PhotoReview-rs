//! Mutation killers for `pr_core::file_actions::undo` (W3-05 mutation debt, claim PARTIAL-mut-undo). Each test names the
//! cargo-mutants mutant it kills. Fakes only (`undo_support`); the real Recycle Bin is never touched.

mod undo_support;

use pr_core::model::FileOperationType;
use undo_support::*;

fn register_move(undo: &mut pr_core::file_actions::undo::UndoService, source: &str, destination: &str) {
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        13,
        write_time(),
    ));
}

// Kills undo.rs:206:9 `<impl PartialEq for UndoActionIdentity>::eq` -> true and -> false.
// Two reads of the same registered action are equal; the identity of a second registration is a different object.
#[test]
fn undo_action_identity_equals_itself_and_differs_from_a_later_registration() {
    let f = Fixture::new();
    let mut undo = f.undo();
    register_move(&mut undo, r"C:\photos\a.jpg", r"C:\photos\sorted\a.jpg");
    let first = undo.last_undo_action_identity().expect("registered");
    let first_again = undo.last_undo_action_identity().expect("registered");
    assert!(first == first_again, "same action, equal identity (kills eq -> false)");

    register_move(&mut undo, r"C:\photos\a.jpg", r"C:\photos\sorted\a.jpg");
    let second = undo.last_undo_action_identity().expect("registered");
    assert!(
        first != second,
        "equal-value registrations stay distinct (kills eq -> true)"
    );
}
