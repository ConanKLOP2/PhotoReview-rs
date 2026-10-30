//! cl-mainwin: in-crate killers for the crate-private UI-thread registry of `core.rs` (D-W8B4M-6).
//! A real `MainWindowInner` needs a window, so these tests drive the `LIVE` list with dead `Weak`s and read it back directly; the
//! live-window cases of `with_current_window` stay a native gap (no seam builds a `MainWindowInner` without Win32).

use super::*;

fn ids() -> Vec<MainWindowId> {
    LIVE.with(|live| live.borrow().iter().map(|(id, _)| *id).collect())
}

fn reset() {
    LIVE.with(|live| live.borrow_mut().clear());
}

fn push_dead(id: MainWindowId) {
    LIVE.with(|live| live.borrow_mut().push((id, Weak::new())));
}

// core.rs `register_window` push: the id lands in the registry (kills `register_window -> ()`).
#[test]
fn register_window_adds_the_entry() {
    reset();
    register_window(7, Weak::new());
    assert_eq!(ids(), [7]);
    reset();
}

// core.rs `register_window` `retain(|..| strong_count() > 0)`: a dead entry is dropped when the next window registers
// (kills `> -> ==`, `> -> >=`, `> -> <` and `retain -> keep all`); dead entries never survive, so only the new id remains.
#[test]
fn register_window_drops_dead_entries_on_the_way() {
    reset();
    push_dead(1);
    push_dead(2);
    register_window(3, Weak::new());
    assert_eq!(ids(), [3]);
    reset();
}

// core.rs `unregister_window` `retain(|(known, _)| *known != id)`: only the named id leaves (kills `!= -> ==`, `-> ()`).
#[test]
fn unregister_window_removes_only_that_id() {
    reset();
    push_dead(1);
    push_dead(2);
    push_dead(3);
    unregister_window(2);
    assert_eq!(ids(), [1, 3]);
    unregister_window(99);
    assert_eq!(ids(), [1, 3], "an unknown id is a no-op");
    reset();
}

// core.rs `with_window` / `with_current_window` with no live window: `None` and the closure never runs.
#[test]
fn lookups_without_a_live_window_return_none() {
    reset();
    push_dead(5);
    assert!(with_window(5, |_| ()).is_none());
    assert!(with_current_window(|_| ()).is_none());
    reset();
}
