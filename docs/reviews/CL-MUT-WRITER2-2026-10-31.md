# CL-MUT-WRITER2: session writer mutation, 28 remaining mutants, 2026-10-31

Status: Done for `pr-core/src/session/writer.rs` (28 of 28 caught); `popup_host.rs` NOT RUN (time-box used up). Lane `cl-mut-writer2`, from master `1ba2dacf`. Production code unchanged. PARITY and task_on_progress.md were NOT edited (proposal below). Predecessor: `CL-MUT-PERFCOM-2026-10-31.md` (28 identities NOT RUN).

Method: `cargo mutants -p pr-core --baseline skip --timeout 300 -j 1 --file "**/session/writer.rs" -F 'writer\.rs:(<lines>):'` with `--cargo-test-arg=--test --cargo-test-arg=mut_writer2_killers --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`, own `target-mut` and `mut-out-*` (deleted afterwards). Preflight without mutation: `mut_writer2_killers` running 11 tests, ok. The killers binary alone is used, so no golden test is involved (`no tests/golden above` found in 0 logs). Run in two parts: the first process (25 mutants) was lost before it printed a summary (the 25 verdicts were already in `caught.txt`), the remaining 3 were run again (`3 mutants tested in 5m: 3 caught`).

| File | Tested | Caught | Missed | Timeout | Unviable | Not run |
|---|---:|---:|---:|---:|---:|---:|
| `pr-core/src/session/writer.rs` (the 28 never-run identities) | 28 | 28 | 0 | 0 | 0 | 0 |
| `pr-platform-win/src/shell_menus/popup_host.rs` (63 listed) | 0 | - | - | - | - | 63 |

Caught: 197, 234, 243 (true and false), 262, 272, 344, 360, 441, 449, 457 (both), 489, 490 (`-=`, `*=`), 510, 524, 559, 617, 619, 626 (`==`, `<`, `>=`), 701, 716, 723, 730, 736. Hang-style mutants (lock never released/acquired, completion never completed, `wait_for_in_flight`, `when_idle`) fail by a bounded `recv_timeout`/`wait_timeout` assertion, none by timeout.

## New tests

`crates/pr-core/tests/mut_writer2_killers.rs` (11 tests, a hand-fired timer, an in-memory fake file system with a write hook, all waits bounded, no sleeps). Each test has a comment naming the line and operator it kills.

`crates/pr-core/tests/mut_perfcom_killers.rs`: the racy test `wait_blocks_until_complete_and_then_returns` is fixed: it first asserts that the waiter is still blocked (300 ms negative `recv_timeout`) before `complete()`, so `Completion::wait -> ()` (166:9) is now killed by this binary alone. It was not re-run under cargo-mutants in this lane (166:9 stays caught by the existing `session_writer` suite).

Known soft spot: `when_idle_blocks_until_the_timer_write_finished` and the fixed wait test use a 200-300 ms negative wait; a mutant could only slip through if the helper thread were starved for that long, correct code never fails because of it.

## popup_host.rs: NOT RUN

The 70-minute time-box went on writer.rs under the load of the other lanes (about 3-7 min per mutant). `command_ids`/`collect_ids` (lines 42-52) and `to_screen_pixels` (95-99) can run with the non-ignored `shell_menus_host` tests (do NOT pass `--include-ignored`: the native tests pop real menus). No killers written, no result claimed; `mut_popup2_killers.rs` does not exist.

## Proposed PARITY changes (not applied)

- X-perf-diagnostics, F-23, F-22 write path: add "CL-MUT-WRITER2 2026-10-31: session/writer.rs the 28 identities never run in CL-MUT-PERFCOM are all caught (28/28, killers binary alone, non-golden); with CL-MUT-PERFCOM and ST-PERFKILL the file has no known survivor". Status of the rows otherwise unchanged (the 13 perf_csv survivors of ST-PERFKILL stay).
- X-menu-native-popup-host: no change; mutation still not run.
