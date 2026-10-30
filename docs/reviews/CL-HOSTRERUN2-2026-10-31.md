# CL-HOSTRERUN2 2026-10-31: host mutation rerun, part 2 (correct libtest flag form)

Status: partial. `wndproc.rs` and `shell_window/dpi.rs` are complete; `ui_dispatcher.rs` lines 241+ are only 4 of 53 mutants (time-box). Branch `feat/cl-hostrerun2` (includes `feat/cl-hostrerun` for its killers and `MUT_TIMEOUT`). Brief: [CL-HOSTRERUN](CL-HOSTRERUN-2026-10-30.md), [AUDIT-MUTATION-FLAG](AUDIT-MUTATION-FLAG-2026-10-30.md).

## Method

- Machine heavily loaded (build 60-80 s per mutant), `--timeout 300`, `-j 1`, target dir `%TEMP%\mut-target-pr-platform-win`, native DLLs staged with `tools/dev/stage-native.ps1 -Destination <target>\debug[\deps]` (PASS).
- `tools/dev/mutants.sh pr-platform-win <file> --native --tests <binaries>` (preflight runs the unmutated command; `running N tests` checked). Command seen by cargo: `cargo test -p pr-platform-win --test <b> -- --include-ignored --test-threads=1`.
- Mutant logs show real test runs (11 of 11 `wndproc` logs contain `running 23 tests`).
- The first preflight of `host_shell_window` failed (27 of 32 tests: `CreateWindowEx (shell window) failed`, a transient desktop state, the same binary passed 32/32 minutes later); the mutation run was only started after a passing preflight.

## Results

| File | Test binaries | Preflight | Listed | Caught | Missed | Unviable | Timeout | Not run |
|---|---|---|---|---|---|---|---|---|
| `host/wndproc.rs` | `host_wndproc` | 23 tests | 11 | 10 | 1 | 0 | 0 | 0 |
| `host/shell_window/dpi.rs` | `host_shell_window`, `w8_host2_killers` | 35 tests | 28 | 25 | 2 | 1 | 0 | 0 |
| `host/ui_dispatcher.rs` (lines 241+) | `host_ui_dispatcher`, `host_ui_dispatcher_native`, `w8_hostrerun_killers` | 31 tests (CL-HOSTRERUN) | 53 | 0 | 1 | 0 | 3 | 49 |

Survivors and timeouts, all dealt with:

- `wndproc.rs:117:20` `creation_cookie` `|| -> &&`: missed by `host_wndproc` alone; the existing `w8_host2_killers.rs` tests catch it (rerun with both binaries: 1/1 caught).
- `dpi.rs:58:30` `apply_dpi_change` `SWP_NOZORDER | SWP_NOACTIVATE`: `| -> &` (flags 0, would re-order and activate) real survivor, killed by the new `w8_hostrerun2_killers.rs::dpi_change_set_window_pos_keeps_the_z_order_and_does_not_activate` (a message handler reads `WINDOWPOS.flags` in `WM_WINDOWPOSCHANGING`; hidden window, no sleeps); rerun: caught. `| -> ^` is EQUIVALENT: the two flags are disjoint bits (0x4, 0x10), so `^` equals `|`.
- `ui_dispatcher.rs` timeouts (treated as NOT detected in the main run): `241:34` `from_current_thread` `delete !`, `256:9` `is_shut_down -> true`, `261:9` `has_high_priority_work -> true`; missed: `266:9` `has_background_work -> false`. Killer `w8_hostrerun2_killers.rs::dispatcher_flags_follow_the_queues_and_the_shutdown` (asserts on the test thread, no loop) plus `w8_hostrerun_killers`: rerun of these 4 (7 matching mutants) with the killer binaries only: 7 caught, 0 missed, 0 timeouts. In the full suite these mutants still hang the older helper-thread tests, so only the killers-alone rerun detects them.
- Not run: 49 mutants of `ui_dispatcher.rs` after line 266 (`message_window`, `shutdown`, `request_wake`, `execute`, `check_access`, trait impls, `dispatcher_wnd_proc`); each timeout costs 6 minutes. Not run at all: `swap_chain_clock.rs`, `message_loop.rs`, `shell_window/messages.rs`, `frame.rs`, `scrollbars.rs`, `viewing.rs`, `windowing.rs`.

## PARITY (docs/PARITY.md)

- `X-host-wndproc-shell-window`: stays Partial, `wndproc.rs` 11/11 now "rerun OK 2026-10-31".
- `X-host-shell-window-messages`: stays Partial (`dpi.rs` 28/28 resolved: rerun OK; `messages.rs` has no mutation evidence).
- `X-host-ui-dispatcher`: stays Partial (40 + 4 of 90 evaluated; 49 NOT run).
- No row raised, none lowered further.

## Test files

`crates/pr-platform-win/tests/w8_hostrerun2_killers.rs` (2 tests, `#[ignore = "native: needs desktop"]`, 1 `unsafe` read of `WINDOWPOS.flags` with `// SAFETY:` inside `pr-platform-win`): both pass with `--include-ignored`.
