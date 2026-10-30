# CL-STARTUP: pr-shell startup and shell_startup mutation evidence (2026-10-30, lane `cl-startup`)

Status: evidence report. Branch `feat/cl-startup`. No production change. Rows: X-startup-* (all stay as they were; PARITY rows only gained an evidence note). The earlier CX-STARTUP and MUTATION-ROUTING-STARTUP-REST reports already covered `pr-app` `startup/*` and `pr-shell` `error.rs`, `entry`, `factories`; this lane mutated the `pr-shell` startup files and `pr-platform-win` `shell_startup` files they had not covered.

## Method

`cargo mutants -p <crate> --baseline skip --timeout 120|150 -j 1 --file ... --cargo-arg=--lib --cargo-arg=--test=<binary>... -- --include-ignored --test-threads=1`, `CARGO_TARGET_DIR=<worktree>/target-mut`, outputs `mut-out-{a,b,c}` (not committed), `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, `PHOTOREVIEW_RS_DATA_ROOT` in the worktree, native DLLs staged into `target-mut/debug{,/deps}` with `stage-native.ps1 -Destination`. Instance tests use a nanosecond-stamped fake prefix (`PhotoReviewRsTest...`), never the user's instance. A first run of group a without `--include-ignored` showed 21 MISSED (ScopeGate, GlobalLog, phase_one): a test-selection artifact, because the covering tests are `#[ignore = "native: needs desktop"]`. With `--include-ignored` all were caught. That first run was stopped by me (own PID) and restarted.

| Group | Crate / files | Test binaries | Tested | Caught | Missed | Unviable | Timeout |
|---|---|---|---:|---:|---:|---:|---:|
| a | pr-shell `startup/{options,forward_target,instance,recovery,smoke,state,shutdown,graph,phase_one}.rs` | lib, startup_options, startup_forward_target, startup_instance, startup_phase_one, startup_composition_root, composition_graph, startup_overlay_vm, w8_startup_mutation_killers | 62 | 57 | 0 | 5 | 0 |
| b | pr-shell `startup/{overlay_vm,composition_root,run,window}.rs` | lib, startup_overlay_vm, startup_composition_root, startup_run_native, startup_wiring_native, startup_native, w8_startup_mutation_killers, w8_startup_killers2 | 63 | 56 | 0 | 7 | 0 |
| c | pr-platform-win `shell_startup/{placement,dpi,local_time,cloak}.rs` | lib, shell_startup_cloak, shell_startup_placement, shell_startup_local_time (no native) | 60 | 56 | 0 | 4 | 0 |
| Total | | | 185 | 169 | 0 | 16 | 0 |

## New tests

`crates/pr-shell/tests/w8_startup_killers2.rs` (3 tests, pass): `GlobalLog` forwards `enabled/info/warn/error` to the process-wide `AppLog`; `diag_options()` is the cached process snapshot; `ShellStartupOptions` Debug. They cover mutants graph.rs:111/116/121/126/137 and options.rs:76 WITHOUT needing the native binaries (group a caught them already via native tests). Since nothing survived, no further killer was needed.

## NOT run

- `pr-platform-win` `shell_startup/{locale (64), warmup (14), monitor_probe (17), apartment}.rs`: not mutated (time box).
- `pr-shell` `startup/secondary_dialogs.rs` (72): not mutated.
- `shell_startup_native` (creates hidden real windows for DWM) was deliberately NOT run: no real window or GUI; the `cloak.rs` mutants are caught by the fake-based `shell_startup_cloak` tests only.
- `GraphInputs`/`StartupGraph` Debug mutants were caught by existing tests in group a; no independent test added.
- Real Win32 (window, named mutex to a real process, GUI smoke, monitor/DPI diversity), attended hardware: human-only gaps, unchanged.

## Rows

All X-startup-* rows keep their status: the remaining gaps they name (shell-startup-texts golden, repair-notice UI, unanswered-owner UI, monitor/DPI, host+shared registration golden) are not mutation gaps. X-startup-launch/error-log were already mutated in MUTATION-ROUTING-STARTUP-REST (0 unexplained missed).

## Gate

`cargo fmt -p pr-shell --check`, `cargo clippy -p pr-shell --all-targets --locked -- -D warnings`, `w8_startup_killers2` 3/3, `python -I tools/dev/doc-links.py --quiet`: see PR body.
