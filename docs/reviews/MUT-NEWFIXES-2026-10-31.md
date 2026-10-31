# MUT-NEWFIXES: mutation check of the recent data-safety fixes (2026-10-31)

Status: done. One mutant survived and is now killed by a new test; no production code changed.

Scope: only the production code added by the data-safety fixes #448, #462, #463, #464, #473, #477. Tooling: cargo-mutants 27.1.0,
`--baseline skip --timeout 300 -j 1`, `CARGO_PROFILE_DEV_DEBUG=0`, own `target-mut` (deleted afterwards), one cargo-mutants at a time.

## Flags and preflight (AUDIT-MUTATION-FLAG)

Correct form: `--cargo-test-arg=--test --cargo-test-arg=<binary> ... --cargo-test-arg=-- --cargo-test-arg=--test-threads=1`.
Preflight `cargo test -p pr-core --test recovery_retry --test file_actions_service --test settings_store` passed (100 and 122 tests plus
the service binary, exit 0). The per-mutant logs show `running 89 tests` / `running 100 tests` (N > 0) for pr-core and
`running 46 tests` / `running 68 tests` for pr-app, and one log shows `test result: FAILED`, so the tests really ran.
No timeouts occurred, so nothing has to be counted as "not detected".

Selection note: cargo-mutants `--re`/`--exclude-re` do not filter the "delete field ..." mutants (they are always listed). Later runs
therefore select with `--in-diff` and a synthetic diff that marks only the target line ranges (scratch script, not committed). The first
run (`precheck`) and the killer rerun also executed unrelated `delete field` mutants of other functions; they are listed separately and
are NOT in the numbers below.

## Results per function

| Function (file lines) | Fix | tested | caught | missed | timeout | unviable | not run |
|---|---|---|---|---|---|---|---|
| `RecoveryRetryService::precheck_group_members` (recovery_retry.rs 512-549) | #448/#463 | 15 | 15 | 0 | 0 | 0 | 0 |
| `RecoveryRetryService::run_group_members`, re-stat branches (recovery_retry.rs 600, 648, 658) | #448/#463 | 4 | 4 | 0 | 0 | 0 | 0 |
| `FileActionService::run_single`, re-stat of a single Recycle (service.rs 369, 592) | #463 | 6 | 4 | 1 (592:67 `\|\|` to `&&`) | 0 | 1 (369) | 0 |
| `SettingsStore::load`, probe read on stat failure (store.rs 297-309) | #477 | 6 | 6 | 0 | 0 | 0 | 0 |
| `SettingsStore::own_config_may_exist` (store.rs 555-557) | #477 | 4 | 4 | 0 | 0 | 0 | 0 |
| `verify_content_groups` (duplicate_cleanup.rs 673-686) | #464 | 12 | 12 | 0 | 0 | 0 | 0 |
| `remove_duplicates`, `on_hash_finished` (duplicate_cleanup.rs 273, 329, 514, 640) | #464/#473 | 4 | 2 | 0 | 0 | 2 | 0 |
| `FileActionController::executor_threw`, disk check (actions.rs 1153, 1183) | #462 | 4 | 3 | 0 | 0 | 1 | 0 |
| `clear_cache`, `submit_recycle` (duplicate_cleanup.rs 446, 732), `execute_core`, `on_executed` (actions.rs 730, 881, 910) | #473 | 5 | 1 (910) | 0 | 0 | 4 | 0 |

Total of the targeted set: 60 tested, 51 caught by the existing tests, 1 missed, 0 timeout, 8 unviable (the unviable ones are
`Default::default()` replacements that do not compile). After the killer below the survivor is caught: 52 caught, 0 missed.

Not provable with cargo-mutants: the `catch_unwind` calls themselves (#473: duplicate_cleanup.rs 322/483/741, actions.rs 252/802/918)
carry no operator mutants and their enclosing functions' whole-body replacements are unviable. The panic-to-failure conversion is covered
only by existing tests (for example `a_panicking_hasher_ends_the_run_with_a_failure_and_recycles_nothing`, `file_hash_worker_panic`); no mutant
proves it. This is "not provable", not "run and passed".

## The survivor and its killer

`service.rs:592:67`, `||` to `&&` in the re-stat guard
`size_of(&now) != run.source_size || now.last_write_utc != run.source_last_write`. The existing L3 tests replace the file with one that
differs in BOTH length and write time, so a replacement that differs in only one of them would have been deleted or recycled. The
production code is correct; the test was too weak.

Killers: `crates/pr-core/tests/mut_newfixes_killers.rs` (same length with another write time; another length with the same write time;
each for the bin and the permanent branch; in-memory disk and bin, no sleeps). Rerun: baseline passes (2 tests); with the mutant both
tests FAIL, so `service.rs:592:67` is caught.

## Incidental, out of scope

Struct-field mutants of other functions ran by accident and were missed: `delete field error/error_code/size/timestamp_utc` in
`retry_core` (recovery_retry.rs 383-387) and `error/error_code/timestamp_utc` in `retry_group` (493-495); the killer rerun also listed
several `delete field` mutants in service.rs (`CaptureGroupMemberResult`, `CaptureGroupActionResult`, `FileActionResult`) as missed
because only the killer binary ran. This is old code not touched by the fixes and was not investigated further.

## PARITY

No row changes: the result confirms the existing evidence of the re-stat rows (rerun OK 2026-10-31); nothing lowered or raised.
