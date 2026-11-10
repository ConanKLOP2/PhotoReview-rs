Status: evidence report (2026-11-06), task P10, branch `test/mut10-shell-small` from origin/master cbaf073f. Two new test files and this file; no production change, no edit of PARITY, README, BACKLOG, PROPOSED-DECISIONS, task_on_progress or SYNC-LEDGER.

# P10: `host/icons.rs`, `host/shell_window/messages.rs`, `host/swap_chain_clock.rs`, `host/wndproc.rs`, mutation under OWNER RULE B (strict, P-391)

Rule B: a mutant is closed only if caught, unviable (a real rustc error), or proven equivalent over the whole domain or every input class. A timeout is never caught; infrastructure failures count as NOT RUN.

## Method

* `cargo mutants --list --file ...`: icons 33, messages 34, swap_chain_clock 36, wndproc 11 (same as P3). ALL mutants of the four files were run again (not only the 58 NOT RUN of P3).
* Preflight on the unmutated tree: plain `cargo test -p pr-platform-win --test ... -- --include-ignored --test-threads=1` over 14 binaries: exit 0, 157 tests, nothing skipped. Binaries: `host_icons`, `host_shell_window`, `host_shell_window_class_conflict`, `host_shell_window_fixes`, `host_swap_chain_clock`, `host_wndproc`, `host_message_loop_native`, `host_frame_clock`, `mut3_host_icons_exe`, `mut3_shell_window_lifecycle`, `w8_host2_killers`, `w8_host_mutation_killers`, `w8_hostrerun2_killers`, `w8_hostrest_killers`; from run 2 on also `mut10_shell_window_messages` (159 tests).
* Run A (wndproc.rs alone, 2 min): `bash tools/dev/mutants.sh pr-platform-win crates/pr-platform-win/src/host/wndproc.rs --cargo-arg --test=<binary> ... --budget 3000 --jobs 1 --cargo-test-arg -- --cargo-test-arg --include-ignored --cargo-test-arg --test-threads=1`, `MUT_TIMEOUT=300`, `CARGO_BUILD_JOBS=2`, TEMP/TMP `C:\Users\PC\Projects\target-mut10-tmp`.
* Run B (icons.rs, messages.rs, swap_chain_clock.rs in one process, 103 mutants, 20 min): same command with the 15 binaries and the three files.
* Checked: no log holds an infrastructure marker (paging file, LNK1102, E0463, out of memory, memory allocation); each of the 3 unviable logs holds a real rustc error (a `||` in a let chain at swap_chain_clock 106:13; `FrameClockBase<SwapChainPolicy>: Default` not satisfied at 136:9 and 147:9); the caught logs of the previously open `icons.rs:30:22` and `messages.rs:84:13` hold failing tests. No timeout, no build failure: NOT RUN is 0.

## Counts (tested = caught + missed + unviable + timeout)

| file | listed | tested | caught | missed | unviable | timeout | NOT RUN |
|---|---:|---:|---:|---:|---:|---:|---:|
| `host/icons.rs` | 33 | 33 | 32 | 1 | 0 | 0 | 0 |
| `host/shell_window/messages.rs` | 34 | 34 | 33 | 1 | 0 | 0 | 0 |
| `host/swap_chain_clock.rs` | 36 | 36 | 33 | 0 | 3 | 0 | 0 |
| `host/wndproc.rs` | 11 | 11 | 11 | 0 | 0 | 0 | 0 |
| total | 114 | 114 | 109 | 2 | 3 | 0 | 0 |

The 2 missed: `icons.rs:30:16`, proven equivalent below (closed); `messages.rs:88:13`, OPEN. Closed: 112 of 114. All 58 mutants that P3 left NOT RUN were run and are caught (or unviable: the 3 above).

## The four P3 open mutants

| mutant | fate | reason |
|---|---|---|
| `icons.rs:30:16` `\|\|` to `&&` in `load` | EQUIVALENT, proven over all of i32 x i32 | `load` and `load_from` test the same predicate P(cx, cy) = `cx <= 0 \|\| cy <= 0`. Mutant guard `cx <= 0 && cy <= 0` is true only where P is true. Case 1: P false: both guards false, identical code. Case 2: P true and both sides non-positive: the mutant returns 0, as the original. Case 3: P true, exactly one side non-positive: the mutant continues; `get_module_handle` Err gives 0; Ok calls `load_from`, whose unmutated guard P is true and returns 0 before any OS call (`load_shared_icon` is not reached). The only difference is a `GetModuleHandle(NULL)` call, which has no state. The three cases cover every pair, so no sampling is involved. Test `mut10_icons_guard` records the sign classes (it passes under the mutant). It was MISSED in run B, as expected. |
| `icons.rs:30:22` `<=` to `>` (`cy`) in `load` | CAUGHT (run B) | `mut3_host_icons_exe::the_production_icon_path_loads_a_real_exe_resource` (child probe: `load(16, 16)` must be non-zero). Valid mutated run. |
| `messages.rs:84:13` delete default `WM_SIZE` arm | CAUGHT (run B) | new `mut10_shell_window_messages::default_wm_size_requests_a_frame_and_answers_zero`: after the creation frame is drawn, `WM_SIZE` with no handler must leave `is_render_requested()` true and answer 0 (the mutant goes to `DefWindowProc`, which answers 0 but requests no frame). |
| `messages.rs:88:13` delete default `WM_DPICHANGED => 0` arm | OPEN (MISSED in run B) | The mutant sends the message to `DefWindowProc` after `apply_dpi_change` and the icon update. Nothing the arm does is observable (it only returns 0), so the only possible difference is what `DefWindowProc` does for `WM_DPICHANGED`, an OS-internal behaviour that cannot be proven over the whole domain by reading. `mut10_shell_window_messages::dpi_changed_default_arm_is_indistinguishable_from_def_window_proc` sweeps `lParam` in {null, misaligned, valid RECT} x `wParam` in 7 classes (dpi 0, 96, 144, 192, 0xFFFF, mixed high words, usize::MAX) and every result is 0, but it passes under the mutant: run-supported, not closed (strict rule B, P-395). Killing it needs a seam that observes `DefWindowProc` calls, which this task excluded. |

## New tests

* `crates/pr-platform-win/tests/mut10_shell_window_messages.rs`: 2 tests, both `#[ignore = "native: needs desktop"]` (hidden real shell window on a UI thread, messages sent directly, no sleeps, bounded waits). No `unsafe` in the file.
* `crates/pr-platform-win/tests/mut10_icons_guard.rs`: 1 test, not native (equivalence record).

## Checks

`rustfmt --edition 2024 --check` on both new files (0); `cargo clippy -p pr-platform-win --tests -- -D warnings` (clean); `cargo test -p pr-platform-win --test mut10_icons_guard --test mut10_shell_window_messages -- --include-ignored --test-threads=1` (3 passed); `PY_BIN=... bash tools/dev/pre-pr.sh` (result in the final report of the lane).

## NOT RUN

None (114 of 114 tested). Open: `messages.rs:88:13`.

## Follow-up P-397 (2026-11-10): `messages.rs:88:13` closed

Seam: `ShellWindowInner.default_proc` (`DefaultWindowProc`, default `user32::def_window_proc`; production behaviour unchanged, the fallback arm of `process_message` calls it). New in-crate tests `shell_window::messages::tests::default_wm_dpichanged_answers_zero_without_the_default_window_procedure` (the stand-in default procedure is not reached, result 0) and its control `message_without_a_default_arm_reaches_the_default_window_procedure`. cargo-mutants re-run (`-F messages.rs:88:13`, `-j 1`, `--baseline skip`, `--lib`): 1 of 1 CAUGHT. Open: 0 (114 of 114 closed).
