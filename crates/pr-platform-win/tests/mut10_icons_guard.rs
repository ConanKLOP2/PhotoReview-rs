//! MUT10 (2026-11-06; REMOVED by refactor 2026-11-10: `load` no longer has its own guard, only `load_from` does): `host/icons.rs:30:16` (`||` to `&&` in `load`) is EQUIVALENT, see `docs/reviews/P10-HOST-SMALL-MUT-2026-11-06.md`.
//! Proof (over all of i32 x i32, by the sign classes of the guard, no sampling): `load` and `load_from` guard the same predicate `P(cx, cy) = cx <= 0 || cy <= 0`.
//! With `&&` in `load`: if P holds with both sides non-positive, `load` returns 0 itself; if P holds for one side only, `load` falls through to
//! `GetModuleHandle` and then to `load_from`, whose own (unmutated) guard P returns 0, or `GetModuleHandle` fails and `load` returns 0. If P does not hold
//! the mutated and the original guard are both false and the code is identical. `GetModuleHandle(NULL)` has no observable effect. So the result is
//! the same for every input. This test only records the four sign classes (it passes under the mutant, by design).
#![cfg(windows)]

use pr_platform_win::host::icons::{load, load_from};

#[test]
fn load_gives_zero_in_all_three_non_positive_classes() {
    for (cx, cy) in [
        (0, 0),
        (0, 16),
        (16, 0),
        (-1, 16),
        (16, -1),
        (i32::MIN, i32::MAX),
        (i32::MAX, i32::MIN),
    ] {
        assert_eq!(load(cx, cy), 0, "load({cx}, {cy})");
        assert_eq!(load_from(0, cx, cy), 0, "load_from(0, {cx}, {cy})");
    }
}
