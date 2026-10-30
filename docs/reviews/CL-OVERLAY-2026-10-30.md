# Lane cl-overlay: overlay button, pointer, presenters, composition (2026-10-30)

Worktree `cl-overlay`, branch `feat/cl-overlay`. C# oracle read-only at `7eaee2a2`. No production change. `CARGO_TARGET_DIR=<worktree>/target` for tests, `<worktree>/target-mut` with `--output mut-out-bench` for cargo-mutants (1 process at a time, `-j 1`, `--timeout 90`). `PHOTOREVIEW_RS_ISOLATE_CONFIG=1` and a lane-local `PHOTOREVIEW_RS_DATA_ROOT`; no real Recycle Bin or user config touched. DLLs checked with `stage-native.ps1 -Check` (match the C# baseline). All native binaries ran with `--include-ignored --test-threads=1`. The sandbox desktop ran them; this is not an attended-hardware verdict.

## Native re-verification

| Binary | Passed | Failed |
|---|---:|---:|
| `pr-platform-win` `shell_overlay_input` | 35 | 0 |
| `shell_overlay_main_window` | 32 (incl. `real_window_mouse_messages_click_fit_and_the_press_is_swallowed`) | 0 |
| `shell_overlay_host` | 25 | 0 |
| `shell_overlay_behavior` | 50 | 0 |
| `shell_overlay_native` | 5 | 0 |
| `shell_overlay_paint` | 14 | 0 |
| `shell_overlay_tooltips` | 18 | 0 |
| `shell_input_dispatch` | 18 | 0 |
| `shell_input_dispatch_pointer` | 32 | 0 |
| `shell_input_e2e` | 16 | 0 |
| `shell_input_surface` | 29 | 0 |
| `pr-shell` `ui_smoke` | 6 | 0 |
| `ui_smoke_overlay` | 6 | 0 |
| `ui_smoke_viewing` | 9 (second run) | 0 |

First `ui_smoke_viewing` run (started while a compile was running in the same target dir): 8 passed, 1 failed, `dragging_the_horizontal_thumb_past_both_ends_clamps_at_the_maximum_offset` hit its 60 s fixture timeout. Rerun alone 3 of 3 passed and the whole binary 9/9 passed. Treated as load-induced; not a code change.

## Mutation (benchmark.rs, `ProductionBenchmarkSession` and `BenchmarkSessionDeps`)

`cargo mutants -p pr-app --file '**/presenters/benchmark.rs' -F 'BenchmarkSessionDeps|ProductionBenchmarkSession' --cargo-arg=--test=w8_presenter_killers2`: Found 5 (matches `--list`), 5 tested, 2 caught (the two `Debug::fmt` at `:900`, `:927`), 1 unviable (`run -> Ok(Default::default())`), 2 MISSED, 0 timeouts.

The 2 MISSED are `dispose -> ()` (`:1004:9`) and delete `!` (`:1004:12`). `executor.dispose()` only cancels the preload scheduler and starts a background thread that deletes a random scratch directory under the temp dir; the session exposes no scratch path, teardown handle or executor accessor, and `BenchmarkExecutorOptions::scratch_parent` is not reachable through `BenchmarkSessionDeps`. They stay unkilled; closing them needs a production seam (a `scratch_parent` field in the deps), not done here (no production change without a real bug).

## What was NOT run

- `settings` presenter: no new mutation run. The 3 `try_start_explorer` mutants spawn `explorer.exe` (untestable) and 2 clamp comparisons are equivalent, per `W8-PRESENTER-MUTATION.md`; the native settings dialog run belongs to W8-19.
- `recovery_window` presenter: nothing left to mutate (96 tested, 0 survivors); the desktop Restore/Dismiss run (W8-18t..18v) is human-only.
- `benchmark` presenter: the desktop run (W8-18r/18s) is human-only; `ProductionBenchmarkSession` end-to-end needs the composition-root codecs (W8-17c).
- `pr-shell` composition: the 13 decoder candidates outside the selected pure-policy scope are native FFI glue and were skipped; the ORF/camera corpus needs real RAW files (human-only).
- `click_zoom.rs`/`pan.rs`/`button.rs`: no re-run (already whole-file clean at cl-core).

## Row verdicts

- X-overlay-button: Partial -> Done (mutation 71/71 clean, native and pointer wiring re-verified above, stale "until W8-15d/15e/14c" note replaced: those rows are Done).
- X-pointer-click-pan: stays Partial; remaining gap named in the row (drag-pan release starting a kinetic glide through real messages is not covered by a native test; no attended hardware run).
- X-recovery-window-presenter, X-benchmark-presenter, X-settings-presenter, X-composition-factories: stay Partial (human desktop runs, corpus, native dialog).

## Killers

`crates/pr-app/tests/w8_presenter_killers2.rs`: 2 tests (`benchmark_session_deps_debug_names_the_type_and_the_memory_field`, `production_session_debug_names_the_type_and_the_profile_id`), fakes only, no sleeps.
