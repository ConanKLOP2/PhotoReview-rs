# ST-PARITY-SYNC3 (2026-10-31): PARITY, ADR and nightly sync for PRs #461..#474

Status: report of lane `st-parity-sync3`. Docs, one workflow list and one test; no production code. Every PARITY note was checked against the current row first (sync2 had already written the H1, F-13, ShellWindow M1/L1, restore-bound, read-only and worker-panic notes; those were not duplicated, only linked to the ADR).

## PARITY rows changed (29, no status change; `parity.py stats` before and after: Done 183, Partial 102, Ported 38, Superseded 17, Different 14, Todo 4, Implementation 3)

| Row | Change |
|---|---|
| F-01..F-06 / F-12 (service side) | #463 L3 re-stat before the single Recycle mutation, INV-4/INV-6 service side closed; L1/L4/L5 deliberately kept; L2 covered by the new test |
| F-18 / F-06 (group retry) | stale sentence "send_to_recycle_bin branch does not re-stat" replaced: #463 added it |
| P-03 | stale "open product question" sentence replaced (fixed by #460); ST-COM-RECYCLE T1..T6 listed, not Done for P-03 / `recycle_bin_shell` / `restore_verb`; C1 not reproduced for junctions, C2 deferred |
| X-benchmark-compare, X-benchmark-parity-harness, X-bench-cli-batch | #466 EngineBackend wired over `EngineHost`; real Windows host not wired (cycle pr-platform-win -> pr-app -> pr-bench) |
| X-file-action-controller | ST-APP-RACES M4 (mutex invariant only in a comment, no test) |
| X-main-view-model | ST-APP-RACES M5 (`MainWorkerPort` ignores `WorkerLane`) |
| W4-14c service disk, W4-08a atomic file | #474 M1 `CommitGate` (INV-2) |
| I-07 | #474 M3 saturating estimate (Different from C# `checked` throw) |
| I-08 / D-06 | #474 M2 bounded preview-file read |
| I-15b / INV-8 | M4 verified by experiment: `libraw_open_wfile` has no share-delete (inherited from C#), deferred |
| X-main-window-frame / -lifecycle / -viewing | #469 15 native tests; "not exercised" lists narrowed; stay Partial |
| P-05 | #471 L2 reply wait (DV-12), L5 |
| P-11 | #471 L6 System32 module cache |
| X-settings-ui-pages | #471 L11 http/https only (DV-10) |
| X-settings-ui-window | #471 L13 `requeue_front` |
| X-host-shell-window-messages | #456 M2 nested `WM_CLOSE` ignored (DV-4) |
| X-release-workflow | #461/#470 advisory actionlint; step results success on master run 41176962097 |
| X-http-composition | "1 placeholder failure" comes from a local `--include-ignored` run, not CI |
| X-release-dependencies | `deny` job concluded success with install and audit steps success on master run 41176962097 and on PR runs (checked with `gh run view`); the "never run" wording applies to local runs only |
| X-duplicate-cleanup, X-folder-load, F-13, F-04 | ADR links (DV-7, DV-8, DV-5, DV-11) |

## ADR

One consolidated file `docs/adr/ADR-D-2026-10-31-intentional-differences-from-fix-lanes.md`, 12 items, each with context, C# file:line, Rust, reason, remaining risk, tests: DV-1 #448, DV-2 #463, DV-3 #454, DV-4 #456, DV-5 #455, DV-6 #462, DV-7 #464, DV-8 #473, DV-9 #474 (M1, M2, M3, plus the M4 note), DV-10 #471 L11, DV-11 #460, DV-12 #471 L2. Linked from `docs/adr/README.md`, `docs/INDEX.md` and `docs/PROPOSED-DECISIONS.md` (row P-58). Repo convention is one ADR per Different row for drafts, but the accepted records (R0002, R0003, ADR-INV) already bundle several items, so one file was used. C# line numbers were read at `7eaee2a2`.

## native-nightly (`.github/workflows/native-nightly.yml`, YAML parses; additions only)

Added to the pr-platform-win list: `w8_mainwin_frame_tests` (15 tests, hidden window and WARP, one shows a window minimised), `host_shell_window_fixes` (2, creates windows, needs a desktop), `dialogs_painter_dpi_empty` (3 native tests, hidden window and PrintWindow), `recycle_final_path` (1 native test, junction in a TempRoot, no desktop). `recycle_restore_timeout` has no `#[ignore]` (fakes, already in ci.yml), so it was not added. None needs a C# DLL or stage-native. A night must confirm them; the three window binaries are the risk if the runner has no interactive desktop (remove those if red for that reason only).

## Test

`crates/pr-shell/tests/composition_registry.rs::undo_takes_the_same_file_action_gate_as_retry`: real move through the registry's `FileActionService`, register in the registry's `UndoService`, take the shared gate, `undo_move` is `rejected` and the file stays at the destination, release the gate, `undo_move` succeeds. Run: `cargo test -p pr-shell --test composition_registry -j 2 undo_takes` 1 passed (only that test was run, not the whole binary); `cargo fmt -p pr-shell --check` clean. Clippy not run (only a test was added).

## Not done

No status raised; M4/M5, RAW share-delete, COM sink C2, atomic export (L4) remain open and are named in the ADR and in the rows.
