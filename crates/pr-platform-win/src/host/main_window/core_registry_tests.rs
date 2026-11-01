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

// cl-mut-core3: `TimerEntry::eq` (core.rs:430) is never called by the heap (it orders with `cmp`), so only a direct `==` pins it.
// Kills `eq -> true`, `eq -> false`, `&& -> ||`, and both `== -> !=` (due, seq).
#[test]
fn timer_entries_are_equal_only_when_due_and_arrival_both_match() {
    let now = Instant::now();
    let entry = |offset_ms: u64, seq: u64| TimerEntry {
        due: now + Duration::from_millis(offset_ms),
        seq,
        action: Box::new(|| {}),
    };
    assert!(entry(10, 1) == entry(10, 1), "same due and same seq are equal");
    assert!(
        entry(10, 1) != entry(10, 2),
        "same due, other seq differ (kills `&& -> ||`, `seq == -> !=`)"
    );
    assert!(
        entry(10, 1) != entry(20, 1),
        "other due, same seq differ (kills `&& -> ||`, `due == -> !=`)"
    );
    assert!(
        entry(10, 1) != entry(20, 2),
        "nothing equal differ (kills `eq -> true`)"
    );
}

// cl-mut-core3: core.rs:671 `Debug for DispatcherStripeScheduler -> Ok(Default::default())` (writes nothing): the text names the type and the window id.
#[test]
fn dispatcher_stripe_scheduler_debug_names_the_type_and_the_window() {
    let dispatcher = Win32UiDispatcher::new(Arc::new(pr_core::abstractions::NullLog)).expect("dispatcher");
    let scheduler = DispatcherStripeScheduler {
        dispatcher,
        window: 4242,
    };
    let text = format!("{scheduler:?}");
    assert!(text.contains("DispatcherStripeScheduler"), "{text}");
    assert!(text.contains("4242"), "{text}");
}
