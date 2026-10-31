# CL-MUT-PERFCOM: perf CSV, session writer, Explorer COM, popup host mutation, 2026-10-31

Status: Partial. Lane `cl-mut-perfcom` (branch `cl-mut-perfcom`, from master `9f85ce28`). Production code unchanged. PARITY and task_on_progress.md were NOT edited (proposals at the end).
The machine ran about nine other cargo-mutants processes at the same time: one mutant took 2-6 minutes (build alone 60-180 s), so the time-box (~60 min) allowed only part of the selection. Everything not listed as run is **CHUA CHAY**.

Method: `cargo mutants -p pr-core --baseline skip --timeout 300 -j 1 --file <file> -F <regex> --cargo-test-arg=--test --cargo-test-arg=<bin> ... --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1` (correct libtest form), `target-mut` and `mut-out-*` inside the worktree (deleted afterwards). Preflight without mutation, same arguments: `diagnostics_perf_csv` running 49 tests ok, `session_writer` 48 ok, `w2_perf2_killers` 9 ok, `w2_perf3_killers` 3 ok, `w2_perfcsv_killers` 6 ok. For every caught CSV mutant the log was checked: it contains `running N tests` (N > 0) and `test result: FAILED`, exit 101.

## Results per file

| File | Selection | Tested | Caught | Missed | Timeout | Unviable | Not run |
|---|---|---:|---:|---:|---:|---:|---:|
| `pr-core/src/diagnostics/perf_csv.rs` | the 11 mutants CX-PERF2 never ran (`dropped_count` x3, `writer_faulted` x2, `dispose`, `wait_finished` x3, `Drop::drop`) + `enqueue`(597, started in CX-PERF2 without verdict) + the 4 CX-PERF2 timeouts (273:9, 284:9, 284:12, 410:5) | 15 | 15 | 0 | 0 | 0 | 0 of the selection |
| `pr-core/src/session/writer.rs` | the 30 identities CX-PERF2 never ran | 2 | 2 (166:9 by the existing suite; 167:15 by the new killer) | 0 | 0 (1 raw timeout, see below, then caught) | 0 | 28 |
| `pr-platform-win/src/explorer_com_interop.rs` (P-01) | 253 mutants listed | 0 | - | - | - | - | 253 (10 earlier in CL-P01 stay as they were) |
| `pr-platform-win/src/shell_menus/popup_host.rs` (X-menu-native-popup-host) | 63 listed | 0 | - | - | - | - | 63 |

### perf_csv.rs detail

The four timeouts of CX-PERF2 (`273:9 signal_finished -> ()`, `284:9 enqueue -> ()`, `284:12 delete !`, `410:5 guarded -> Ok(())`) came from the wide suite: a first attempt here with `diagnostics_perf_csv` in the binary list reproduced the timeout (`273:9`: 181 s build + 302 s test; the suite's tests FAILED but a later test blocks without bound). That attempt was stopped. With the three killer binaries alone (`w2_perf2_killers`, `w2_perf3_killers`, `w2_perfcsv_killers`) all four are **caught** by failed assertions in 16 s (not by a timeout), as ST-PERFKILL predicted. The 11 never-run mutants are caught by the same three binaries (`dropped_count` 0/1/-1, `writer_faulted` true/false, `dispose -> ()`, `wait_finished` true/false, `delete !`, `Drop::drop -> ()`), and so is `PerfCsvListener::enqueue -> ()` (597:9). The other mutants of the file keep the verdicts of CX-PERF2 / ST-PERFKILL (the 13 missed there are still class A/C, not re-run here).

### writer.rs detail

- `166:9 Completion::wait -> ()`: caught (existing suite, test exit 101).
- `167:15 delete ! in Completion::wait`: raw result **timeout** with `session_writer` + killers (the mutated `wait` blocks forever on a completed completion), which counts as not detected. New killer `mut_perfcom_killers::wait_on_a_completed_completion_returns_at_once` (bounded `recv_timeout`, no sleep) catches it: re-run on that binary alone, **caught** (exit 101, 59 s build).
- The run was stopped (my own PID only) after these two because each hanging mutant costs 300 s and the machine was loaded. **28 identities were not run**: lines 197, 234, 243 x2, 262, 272, 344, 360, 441, 449, 457 x2, 489, 490 x2, 510, 524, 559, 617, 619, 626 x3, 701, 716, 723, 730, 736. Several of them (`WriteLock::acquire/release`, `ReleaseOnDrop`, `wait_for_in_flight`) are hang-style mutants that need bounded killers through `SessionWriter` fakes before a run is useful.

### P-01 and popup host

Not run (no result claimed). `explorer_com_interop.rs` lists 253 mutants; about 200 are `ShellResult::*` constructor bodies of raw COM slots, the logic ones are `web_browser_location_url` (10), `shell_windows_count`, `item_count`, `get_sort_column_count` (10 each), `get_group_by` (7), `enum_next` (7), `ShellComError::hresult`/`fmt`, `ShellWindowsIter::next`, `check`, `<`/`+=`/`!=` operators. They need the 14 fake-backed `explorer_com_interop` tests (no real Shell, no Recycle Bin) and about 4 hours at today's load. `popup_host.rs` (283 lines, 63 mutants): pure part `command_ids`/`collect_ids` (42-52) and `to_screen_pixels` (95-99) can run with the non-ignored `shell_menus_host` tests; the `HMENU` populate/append/style code needs the 16 native tests, which pop real menus on the desktop (keyboard-driven, watchdog Escape) and therefore **need a person** at the desktop.

Golden check: every caught CSV mutant failed non-golden tests (`failing_sink_counts_the_failed_row...`, `listener_drains_rows...`, `dropping_a_listener_drains...`, literal assertions); `166:9` failed `flush_forgets_the_schedule`, `many_updates_coalesce_into_one_write` and others besides the golden. No "golden only" catches.

## New test

`crates/pr-core/tests/mut_perfcom_killers.rs` (2 tests, pass): `wait_on_a_completed_completion_returns_at_once`, `wait_blocks_until_complete_and_then_returns`. Bounded waits only; no sleep, no clock, no files. The second test is not a deterministic killer of `wait -> ()` (a race: the mutant may win), which is why that mutant relies on the existing `session_writer` suite (it was missed when only this binary ran: 1 missed on 166:9 with the killer binary alone).

## Proposed PARITY changes (not applied)

- X-perf-diagnostics, F-23, F-22 write path: stay Partial. Add: "CL-MUT-PERFCOM 2026-10-31: perf_csv.rs 15/15 selected caught including the 4 former timeouts; writer.rs 2 of the 30 NOT RUN evaluated (2 caught); 28 still NOT RUN". The 13 missed from ST-PERFKILL (class A/C) are unchanged.
- P-01 and X-menu-native-popup-host: no change; mutation still not run (253 and 63 mutants), live Explorer window and desktop menu runs stay human-only.
