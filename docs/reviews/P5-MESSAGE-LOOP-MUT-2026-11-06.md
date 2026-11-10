# P5 mutation evidence: `host/message_loop.rs` NOT RUN list, bounded `add_frame_clock` killer, `wait_timeout_ms` 170:72 (2026-11-06)

P-396 (2026-11-10): mutant 170:72 no longer exists because the dead `unwrap_or` arm is removed; the historical evidence below is unchanged.

Status: evidence report, task P5, worktree `mut5-message-loop`, branch `test/mut5-message-loop`, base `origin/master` 52f1efb9.
PARITY.md, README, BACKLOG, PROPOSED-DECISIONS, task_on_progress and SYNC-LEDGER are NOT edited. Production code unchanged
(`git diff origin/master --stat -- crates/*/src` is empty). Rule B (P-391, strict): a mutant is closed only if caught, unviable, or proven
equivalent over the whole domain or every input class; a sampled or bounded run, or a reading of the code, does not close one; a timeout is
never caught; an infrastructure failure is NOT RUN. Method as in P3/P4: `tools/dev/mutants.sh`, `-j 1`, one cargo-mutants process, own target
dir outside the worktree.

## Result in one paragraph

All 31 NOT RUN mutants of P3 now have a verdict: **31 caught**, by assertion, with new bounded tests. The two `add_frame_clock` mutants (240:9, 241:12)
are now caught by an assertion in `mut5_message_loop` (`left: Ok(9)`, `right: Err(InvalidOperation { message: "clock failed" })`), not by the
runaway-allocation abort of P3: no log of the run contains an abort, `0xc0000...` status or `memory allocation`. `wait_timeout_ms` 170:72 stays
**one survivor, `- -> /`: equivalent by an arithmetic proof over all three input classes, unkillable** (the `unwrap_or` arm is dead code); the
sibling `- -> +` is unviable (rustc `arithmetic_overflow`, a real compile error in `crates/*/src`). No timeout, no infrastructure failure.
Counting both rounds the file has 84 of 84 tested, 0 NOT RUN (see "Counts").

## New tests (hand written, no golden, no production change, no attended desktop)

| File | Tests | Targets |
|---|---:|---|
| `crates/pr-platform-win/tests/mut5_message_loop.rs` | 17 | the 31 NOT RUN identities except the three `Win32LoopOs` ones, plus 240, 241 and `same_clock` |
| `crates/pr-platform-win/tests/mut5_wait_timeout_domain.rs` | 1 | 170:72 (extremal run of the real function, see the proof) |

`mut5_message_loop.rs` is the bounded harness: every fake (`LoopDispatcher`, `LoopOs`) counts its calls against one shared budget of 600 and
panics with `budget exceeded in <call>: the mutated loop does not end` beyond it. A mutant that makes the loop spin or never block therefore ends
as an ordinary assertion failure after a few hundred passes with a tiny log (the P3 abort needed an unbounded `Vec` of log strings). The
legitimate tests use under 200 calls. Every test is `#[cfg(windows)]` and runs on the test thread with fakes: nothing touches the OS queue. The
three `Win32LoopOs` mutants (144:19, 146:26, 154:9) are killed by the existing `mut3_host_win32_loop_os` (real, non-blocking thread queue and
timeout-0 waits, message-only window; no `#[ignore]`, no `--native` needed). Both new files: `rustfmt --edition 2024 --check` clean.

The killer for 240/241 is `a_registered_failing_clock_ends_run_with_its_error_and_an_unregistered_one_cannot`: a signaled failing clock is
registered twice; the original ends `run` with the clock's error before any wait; with no clock registered the loop blocks once, the wait hook posts
`WM_QUIT(9)` and `run` returns `Ok(9)`, which fails the `assert_eq!`. The same-`Rc`-twice registration also covers `same_clock` (478).

## How the mutation runs were made

`MUT_TIMEOUT=600 CARGO_BUILD_JOBS=2 MUT_OUT=<dir> bash tools/dev/mutants.sh pr-platform-win crates/pr-platform-win/src/host/message_loop.rs --tests mut5_message_loop,mut3_host_win32_loop_os,mut3_host_pure --cargo-arg --test=mut5_message_loop --cargo-arg --test=mut3_host_win32_loop_os --cargo-arg --test=mut3_host_pure --jobs 1 --budget 20000 -F 'message_loop\.rs:<line>:<col>:' ...`
(27 `-F` filters: the 24 NOT RUN positions of P3 plus 240:9, 241:12, 170:72). cargo-mutants 27.1.0, `--baseline skip`, own `CARGO_TARGET_DIR`
(`mut-target-pr-platform-win` under `TEMP`), preflight ran the tests (24 executed, exit 0). `TEMP`/`TMP` = `C:\Users\PC\Projects\target-mut5-tmp`,
`CARGO_TARGET_DIR=C:\Users\PC\Projects\target-mut5`. The log says `Found 38 mutants to test`: 31 NOT RUN plus 7 that P3 had already tested and
the position filters re-selected (two more `<`-replacements of 144:19, the other 327:9 replacement, 240, 241 and both 170:72 mutants); they were
run again, which is harmless.

Harness remark: a first attempt without `--cargo-arg --test=...` rebuilt and relinked every test binary of the crate (hundreds) for each mutant
(about 35 minutes for the first mutant). It was stopped by me (own process tree only: `taskkill /T` on my `timeout` parent) after one caught
verdict; its results are discarded and not used anywhere below. With the `--cargo-arg --test=` form the 38 mutants took 3 minutes.

Verdict check: all 36 caught logs contain a failing test (`test result: FAILED` / `panicked at`), the two others (170:72) do not; no log
contains `paging file`, `LNK1102`, `E0463` or `os error 1455`; 7 logs show the budget assertion, the rest ordinary `assert_eq!` failures.

## Fate of every mutant of the run

| Mutant (`message_loop.rs`) | Fate | Killed by |
|---|---|---|
| 144:19 `< -> ==`, `< -> >`, `< -> <=` (`Win32LoopOs::wait`) | caught (3) | `mut3_host_win32_loop_os::wait_decodes_handle_messages_timeout_and_failure` |
| 146:26 `== -> !=` (`Win32LoopOs::wait`) | caught | same |
| 154:9 `post_quit -> ()` | caught | `mut3_host_win32_loop_os::post_quit_queues_wm_quit_with_the_exit_code` |
| 170:72 `- -> +` | unviable | rustc: `this arithmetic operation will overflow` (`u32::MAX + 1`), error points into `message_loop.rs:170` |
| 170:72 `- -> /` | missed, EQUIVALENT (proof below) | no test can kill it |
| 240:9 `add_frame_clock -> ()` | caught | `a_registered_failing_clock_ends_run_with_its_error_and_an_unregistered_one_cannot` (assertion `Ok(9)` vs `Err`) |
| 241:12 `delete !` | caught | same |
| 327:9 `run_nested -> Ok(true)`, `-> Ok(false)` | caught (2) | `run_nested_pumps_until_is_done_and_returns_true`, `run_nested_meets_wm_quit_returns_false_and_reposts_it` |
| 328:15 `delete !` (`while !is_done()`) | caught | `run_nested_with_is_done_already_true_runs_no_pass`, `run_nested_pumps_until_is_done_and_returns_true` |
| 349:9 `after_pump -> Ok(())` | caught | `background_item_runs_when_the_queue_is_empty` (budget assertion) |
| 353:16 `delete !` (Background only with an empty queue) | caught | `background_item_waits_for_the_pending_message`, `background_item_runs_when_the_queue_is_empty` |
| 358:12 `delete !` (wait only with an empty queue) | caught | `wait_is_skipped_with_a_pending_message_and_taken_with_an_empty_queue` |
| 368:9 `pump_messages -> None`, `Some(0)`, `Some(1)`, `Some(-1)` | caught (4) | `pump_dispatches_in_order_and_returns_the_quit_code` (exit code 5, dispatched `[1, 2]`) |
| 370:16 `delete !` (`!peek_message`) | caught | same |
| 373:28 `== -> !=` (`WM_QUIT`) | caught | same |
| 383:23 `delete !` (`!self.filtered`) | caught | `without_filters_every_message_is_dispatched`, `a_filter_that_handles_a_message_stops_its_dispatch` |
| 397:9 `filtered -> true`, `-> false` | caught (2) | same two tests |
| 426:9 `poll_clocks -> Ok(())` | caught | `a_signaled_clock_raises_its_frame_and_not_its_deadline` |
| 428:16 `delete !` (`!is_armed`, poll) | caught | `an_unarmed_clock_is_skipped_and_an_armed_one_is_polled` |
| 432:23 `!= -> ==` | caught | `a_clock_without_handle_is_never_polled_and_a_passed_deadline_raises_the_deadline` |
| 432:28 `&& -> \|\|` | caught | `an_unsignaled_clock_with_a_future_deadline_raises_nothing` |
| 434:52 `<= -> >` | caught | same two tests |
| 444:9 `wait -> Ok(())` | caught | `wait_gets_the_handles_and_the_nearest_deadline_of_the_armed_clocks` (budget assertion) |
| 449:16 `delete !` (`!is_armed`, wait) | caught | same |
| 454:23 `!= -> ==` | caught | same |
| 454:28 `&& -> \|\|` | caught | same |
| 454:45 `< -> ==`, `< -> >`, `< -> <=` | caught (3) | `wait_takes_at_most_63_handles_in_registration_order` |
| 478:5 `same_clock -> true`, `-> false` | caught (2) | `clocks_are_registered_once_per_identity`, `a_registered_failing_clock_ends_run_with_its_error_and_an_unregistered_one_cannot` |

## 170:72 `- -> /` in `wait_timeout_ms`: equivalent, proof over every input class

```rust
let ms = (i128::from(until_deadline) * 1000 + frequency - 1) / frequency;          // frequency = i128::from(frequency.max(1)) >= 1
u32::try_from(ms.min(i128::from(INFINITE - 1))).unwrap_or(INFINITE - 1)           // 170; the mutant: unwrap_or(INFINITE / 1)
```

The mutant differs from the original only if `try_from` fails, i.e. if `ms.min(0xFFFF_FFFE)` is negative or above `u32::MAX`. The input classes of
`(until_deadline: i64, frequency: i64)`:

* A, `until_deadline == i64::MAX`: returns `INFINITE` before line 170.
* B, `until_deadline <= 0`: returns `0` before line 170.
* C, `1 <= until_deadline < i64::MAX`, every `frequency` (collapsed to `>= 1` by `max(1)`): `until_deadline * 1000 < 2^73` so the `i128` arithmetic
  cannot overflow; the numerator is `>= 1000 + f - 1 > 0` and `f >= 1`, so `ms >= 1`; and `ms.min(0xFFFF_FFFE) <= 0xFFFF_FFFE < u32::MAX`.
  So `try_from` succeeds on every input of C and the `unwrap_or` argument is never evaluated into the result.

A, B, C partition the whole domain, so no input distinguishes the mutant from the original: it cannot be killed by any test. The test
`mut5_wait_timeout_domain` runs the REAL function at the extremes of C (nine `until_deadline` values from 1 to `i64::MAX - 1`, eleven `frequency`
values from `i64::MIN` to `i64::MAX`, the cap boundary `0xFFFF_FFFD..0x1_0000_0001` ms) against an `i128` reference and asserts the `try_from` argument
is in `[1, 0xFFFF_FFFE]`; `ms` is non-decreasing in `until_deadline` and non-increasing in `frequency`, so the extremes bound the class. Run through
`mutants.sh` with `--tests mut5_wait_timeout_domain,mut3_host_pure`: `- -> /` MISSED (the proof passes under the mutant, as it must), `- -> +` unviable.

Honest limit: the proof is an arithmetic argument over the three classes plus an extremal run, not a run over all of `i64 x i64` (2^128 inputs); a
strict reader of "a reading of the code does not close" may keep it as run-supported, open. Nothing else can be done by a test, because the arm is
unreachable. The production simplification P3-2 (replace the `try_from`/`unwrap_or` pair by `ms.min(i128::from(INFINITE - 1)) as u32`, no behaviour
change) would remove the survivor; it is NOT applied here (no production change in this task).

## Counts

This run (`host/message_loop.rs`, 38 current mutants selected by position filters):

| listed | tested | caught | missed | unviable | timeout | NOT RUN |
|---:|---:|---:|---:|---:|---:|---:|
| 38 | 38 | 36 | 1 | 1 | 0 | 0 |

`tested = caught + missed + unviable + timeout` = 36 + 1 + 1 + 0 = 38.

The file as a whole (84 listed by `cargo mutants --list`), P3's verdicts for the 53 it tested taken as recorded there (47 caught, 5 unviable, 1 missed
= 170:72 `/`, 0 timeout) plus this run's 31 newly covered mutants (31 caught, 0 missed, 0 unviable, 0 timeout):

| listed | tested | caught | missed | unviable | timeout | NOT RUN |
|---:|---:|---:|---:|---:|---:|---:|
| 84 | 84 | 78 | 1 | 5 | 0 | 0 |

`84 = 78 + 1 + 5 + 0`. The 53 of P3 were not re-verified one by one here, except the 7 that this run re-selected (all 7 reproduce: 5 caught, 170:72 `-> /`
missed, 170:72 `-> +` unviable). The one missed mutant is the equivalent 170:72 `- -> /` above.

## NOT RUN

None in this file. Nothing was left out for time, memory or an attended desktop.

## Commands run

```
cargo mutants -p pr-platform-win --list --file crates/pr-platform-win/src/host/message_loop.rs        (84 mutants; positions match the P3 NOT RUN list)
cargo test -p pr-platform-win --test mut5_message_loop --test mut5_wait_timeout_domain                    17 + 1 passed
mutants.sh ... (run2, 38 mutants, see above)  and  mutants.sh ... --tests mut5_wait_timeout_domain,mut3_host_pure -F 'message_loop\.rs:170:72:'  (run3)
rustfmt --edition 2024 --check crates/pr-platform-win/tests/mut5_message_loop.rs crates/pr-platform-win/tests/mut5_wait_timeout_domain.rs        clean
cargo clippy -p pr-platform-win --tests -- -D warnings                                                     clean
PY_BIN=$LOCALAPPDATA/Programs/Python/Python312/python.exe bash tools/dev/pre-pr.sh   conflict markers ok, BOM ok, clippy ok, unsafe-audit ok, doc-links ok; the one FAIL is `cargo fmt -p pr-platform-win` "os error 206" (long worktree path, as in N4/P3), covered by the per-file rustfmt check; `cargo test` skipped by the script (no --test), the two new binaries were run alone
```
