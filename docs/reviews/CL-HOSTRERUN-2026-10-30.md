# CL-HOSTRERUN 2026-10-31: host mutation rerun with the correct libtest flag form

Status: partial. Only `host/ui_dispatcher.rs` was (partly) rerun; the time-box (heavily loaded machine, about 3 minutes per mutant) was used up on it. Branch `feat/cl-hostrerun`. Brief: [AUDIT-MUTATION-FLAG](AUDIT-MUTATION-FLAG-2026-10-30.md).

## Method

- Native DLLs staged in the mutant target dir with `tools/dev/stage-native.ps1 -Destination <target>\debug[\deps]` (PASS).
- `tools/dev/mutants.sh pr-platform-win crates/pr-platform-win/src/host/ui_dispatcher.rs --native --tests host_ui_dispatcher,host_ui_dispatcher_native --budget 5400` with `MUT_TIMEOUT=300` (a one-line change to `mutants.sh`: the per-mutant `--timeout` was fixed at 90; now `${MUT_TIMEOUT:-90}`).
- Command seen by cargo: `cargo test -p pr-platform-win --test host_ui_dispatcher --test host_ui_dispatcher_native -- --include-ignored --test-threads=1`. Preflight (unmutated): `running 9 tests` + `running 22 tests` = 31 executed, exit 0. A mutant log shows the same `running 9 tests` / `running 22 tests`.

## Result for `ui_dispatcher.rs` (budget exhausted after 5400 s: PARTIAL)

| | Count |
|---|---|
| Listed | 90 |
| Evaluated | 40 (file order, up to line 241) |
| Caught | 26 |
| Missed | 3 |
| Timeout | 2 |
| Unviable | 9 |
| NOT evaluated | 50 |

Old claim (CL-HOST2): 90 tested, 79 caught, 0 missed, 0 timeout, 11 unviable. The old numbers were not reproduced: the old run reported zero missed where the correct run finds 3 missed in the first 40 mutants, so the old run was a false-caught run (it is consistent with the flag-form defect of the audit).

Missed (real):
- `ui_dispatcher.rs:42:62` `WAKE_MESSAGE = WM_APP + 1`: `+ -> -` and `+ -> *` (the tests use the constant symbolically, nothing pinned 0x8001).
- `ui_dispatcher.rs:194:9` `Debug for Win32UiDispatcher::fmt -> Ok(Default::default())` (empty Debug output).

Timeout (treated as NOT detected; the helper-thread test hangs instead of failing): `239:9` `from_current_thread -> None`, `241:34` `delete !`.

## Killers: `crates/pr-platform-win/tests/w8_hostrerun_killers.rs` (4 tests, 3 native-ignored, one message-only window never shown, no sleeps)

`wake_message_is_wm_app_plus_one`, `batch_slice_is_fifty_milliseconds`, `debug_output_names_the_dispatcher_type`, `from_current_thread_follows_the_dispatcher_life_cycle_on_the_test_thread`. Run: `running 4 tests`, 4 passed. Rerun of the survivors with only this binary (`-F "ui_dispatcher.rs:(42:62|194:9|239:9|241:34)"`): 6 mutants, 5 caught, 1 unviable (`from_current_thread -> Some(Default::default())`), 0 missed, 0 timeout.

## NOT run (time-box)

- `ui_dispatcher.rs`: the last 50 mutants (after line 241: `message_window`, `shutdown`, `request_wake`, `execute`, `check_access`, trait impls, `dispatcher_wnd_proc`).
- `host/wndproc.rs` (11), `host/shell_window/dpi.rs` (28), `message_loop.rs` / `swap_chain_clock.rs`, `frame.rs` + `scrollbars.rs` (57), `viewing.rs` (76), `windowing.rs` (175). Their CL-HOST2 / CL-MAINWIN2 numbers stay UNVERIFIED (same author lane, same period, same flag form in the report as the ui_dispatcher run that proved wrong). None is labelled "rerun OK".

## PARITY (docs/PARITY.md)

- `X-host-ui-dispatcher`: Done -> Partial (reason above; 50 mutants unrun).
- `X-host-shell-window-messages`: Done -> Partial (the `dpi.rs` evidence comes from the same unverified CL-HOST2 run).
- `X-host-wndproc-shell-window`, `X-main-window-frame/viewing/windowing`: stay Partial; notes now say the numbers are unverified. No row raised.
