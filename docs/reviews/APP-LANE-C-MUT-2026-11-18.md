Status: evidence report (2026-11-18). Whole-file mutation run of the `pr-app` lane C files (`input/shortcut_router.rs`, `coordinators/file_action_gate.rs`) against the whole `pr-app` suite, plus a confirmation run of the two killers added. Measured on one machine (Windows 11), `--jobs 1`, `MUT_TIMEOUT=900`.

# `pr-app` lane C: shortcut router and file action gate

## Result (first run, whole suite)

64 identities: 57 caught, 3 unviable, 4 missed, 0 timeout.

| File | Caught | Unviable | Missed |
|---|---|---|---|
| `input/shortcut_router.rs` | 23 | 2 | 1 |
| `coordinators/file_action_gate.rs` | 34 | 1 | 3 |

## Survivors

| Mutant | Verdict | Argument or killer |
|---|---|---|
| `file_action_gate.rs:147:42` `&&` -> `\|\|` in `GateInner::release` | killed | `finishing_queued_turn_with_successor_posts_only_the_successor_not_the_waiters`: a waiter must not be posted while a successor is queued (`scheduler.pending() == 1` after the first finish) |
| `file_action_gate.rs:95:9` `Debug for GateTurn` -> `Ok(Default::default())` | killed | `gate_turn_debug_names_type_kind_and_finished_flag` |
| `file_action_gate.rs:75:9` `GateTurn::finish` -> `()` | equivalent | `finish(mut self)` consumes the turn; the empty body drops it at the end of the function and `Drop` calls the same idempotent `release` |
| `shortcut_router.rs:257:20` match guard `!t.trim().is_empty()` -> `true` in `parse_key` | equivalent | blank or whitespace text reaches `try_parse`, whose `canonicalize` trims it and `chars().next()?` returns `None`, the same result as the guard's fallthrough; valid while `try_parse` rejects blank names |

## Confirmation

Rerun of the two killed positions with `--tests coordinators_file_action_gate`: 2 caught, 0 missed (the test file runs 33 tests).

## New tests

Two tests appended to `crates/pr-app/tests/coordinators_file_action_gate.rs`. No source change, no new seam.
