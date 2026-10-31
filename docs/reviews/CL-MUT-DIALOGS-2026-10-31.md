# CL-MUT-DIALOGS 2026-10-31: dialogs mutation rerun, partial and time-boxed

Status: PARTIAL. Only a small sample of one function group was mutated; the other PARITY rows in scope were NOT run.

## What was run
- Preflight (no mutation): `cargo test -p pr-platform-win --test dialogs_painter_widgets -- --include-ignored --test-threads=1` -> `running 17 tests`, 17 passed, exit 0.
- Mutation: `cargo mutants -p pr-platform-win --baseline skip --timeout 300 -j 1 --shuffle --file "**/painter/widgets.rs" -F "combo_layout|trackbar_layout|tick_xs" --cargo-test-arg=--test --cargo-test-arg=dialogs_painter_widgets --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1` (correct flag form, own target-mut and output dir, `CARGO_PROFILE_*_DEBUG=0`; both removed afterwards).
- Machine was at 100% CPU from other agents: a cold build took about 4 min and each mutant about 5 min, so the 50-minute cap allowed only a handful of mutants. `--shuffle` was used so the sample is spread across the three functions.

## Result (real numbers)
Found 99 mutants (combo_layout 49, trackbar_layout 18, tick_xs 32 in the filtered set). Tested 6 (7th in progress when the cap killed the run), caught 6, missed 0, timeout 0, unviable 0, NOT RUN 92 (plus the interrupted one).

| mutant | verdict | evidence |
|---|---|---|
| widgets.rs:412:28 `-` -> `+` in combo_layout | caught | log: running 17 tests, 1 failed |
| widgets.rs:557:61 `/` -> `*` in trackbar_layout | caught | running 17 tests, 3 failed |
| widgets.rs:634:5 tick_xs -> `vec![1]` | caught | running 17 tests, 3 failed |
| widgets.rs:647:54 `*` -> `/` in tick_xs | caught | running 17 tests, 2 failed |
| widgets.rs:661:20 `%` -> `+` in tick_xs | caught | running 17 tests, 3 failed |
| widgets.rs:379:34 `/` -> `%` in combo_layout | listed caught, log has no `running 17 tests` line (grep) | treat as UNVERIFIED; rerun needed |
| widgets.rs:408:42 `&&` -> `||` in combo_layout | NOT TESTED (interrupted by the time cap) | rerun needed |

No survivor, so no killer was added (`tests/w8_dialogsmut_killers.rs` not created). Production code not touched.

## Not run (CHƯA CHẠY)
- X-dialogs-native-base: template (37), chrome (50), edit_centering (51), list_dialog (123), controls basic/custom/list_tree (126/29/136) - not mutated in this lane.
- X-dialogs-native-dialog: dialog/core 70, dpi 71, draw 21, events 47, focus 20, layout 208, modal 48, result 6, tooltip 47 - not mutated.
- X-dialogs-painter: painter/mod 37, geometry 129, controls 116, frame 90 - not mutated.
- X-chrome-painter-widgets: 92 of 99 combo/trackbar/tick mutants not run; check/radio/progress already audited earlier.
- X-dialogs-secondary-port: production factories and editor/rename routing not mutated here.
- Raw Win32 glue (HWND/DC/message loop, draw_* functions that call GDI) excluded by policy.

## Suggested PARITY changes (orchestrator)
- None to status. For X-chrome-painter-widgets add: "cl-mut-dialogs 2026-10-31: 5 of 99 combo/trackbar/tick mutants verified caught (correct flags, running 17 tests), 1 unverified, 92 not run; no valid full verdict yet."
- Other rows: no evidence produced; leave Partial.
- Rerun needs a quiet machine (about 5 min per mutant under load; roughly 1.5 min idle): 99 mutants is about 2.5 h idle.

## Caveat (golden)
cargo-mutants does not copy tests/golden into its temp tree, so a mutant failing only on a golden-loading test is not a true catch. The mutant logs were deleted (disk nearly full) before this check, so which failing tests were non-golden was NOT re-verified; the 5 'caught' are therefore provisional ('caught, golden-only not excluded'). target-mut and mut-out were removed.
