# ST-PARITY-SYNC6 (2026-10-31)

Status: docs + `native-nightly.yml` only. No build, no test run. Sources: the six reports of PRs #497..#502 (read in full or in their tables).

## Done

PARITY synced to #497..#502 (rows checked against the current text, nothing duplicated). No status word changed: `python tools/dev/parity.py stats` still gives 361 rows, Done 183, Partial 102, Ported 38 (PARITY-CLOSEOUT and TRIAL-PLAN already carry these numbers, not edited).

| Row | Change |
|---|---|
| X-benchmark-presenter | `st_bench_presenter_session` (5 F tests, no new seam) added; "not run by a test" replaced; W10-B2 attended run still missing; Partial |
| X-startup-early-decode | process scenario now in `startup_early_decode_native` (1/1 desktop); Partial until a green nightly; the logging-off note is only that `error_forced` is C# behaviour and the DLLs were unstaged ([FIX-LOGGING-OFF](FIX-LOGGING-OFF-2026-10-31.md)) |
| X-stj-reader | 8 `*= 1` timeouts killed by hand-applied mutants through `w3_stj_timeout_killers` (4 tests, 10 s bounded wait); no cargo-mutants; Partial |
| X-pointer-click-pan | `shell_input_pan_glide_native` 3 -> 9; real mouse/pen hardware remains a human job; Partial |
| X-dialogs-recovery | real bug (permanent-delete warning lost, `allow_permanent_delete: None`) fixed by #500; 8 headless composition tests; the not-yet-ported C# tests listed; Partial |
| X-dialogs-zoom-diagnostics-profiles-benchmark | 19 new native tests (7 + 9 + 3); Partial |
| X-main-window-windowing, X-main-window-lifecycle, X-host-shell-window-messages | 11 tests in `w8_mainwin_seams2`; equivalent mutants recorded (close waiter, `update_fit_size_if_alive` delete `!`, `WM_DPICHANGED => 0`); 72 and 21 mutants still NOT run; Partial |

Nine rows changed. The report of lane `mut-newfixes` is not on master, so none of its numbers is cited.

## native-nightly

Five binaries added to the `pr-platform-win` list: `shell_input_pan_glide_native`, `dialogs_action_profiles_scenarios_native`, `dialogs_zoom_diagnostics_scenarios_native`, `dialogs_benchmark_scenarios_native`, `w8_mainwin_seams2`. All are `#[ignore]` native tests with hidden windows and fakes, no C# DLL, no Recycle Bin. `startup_early_decode_native` was already added by #498. `startup_native` is NOT added (needs staged DLLs). YAML parses. One night must confirm them; a binary that is red only because windows-latest has no interactive desktop must be removed.

## P-nn / DV-nn

P-60..P-64 each have one ADR and a README row, no duplicate id in `docs/PROPOSED-DECISIONS.md`. DV-22 and DV-24 are `##` headings in their ADRs, DV-23 is the "Decision (DV-23)" heading of the recycle ADR; no duplicate DV heading, each is referenced from PARITY. Nothing to fix.

## Việc còn lại

- Mutation: `windowing.rs` 72 mutants, `messages.rs` 21 mutants NOT run; cargo-mutants rerun of `app_settings.rs` json after the timing guards; mutation of the Recovery composition and the new dialog code.
- Unported C# dialog tests (list in [ST-DIALOG-SCENARIOS](ST-DIALOG-SCENARIOS-2026-10-31.md) section 2): `Port_ShowRecovery_WithAnUnreadableJournal_TellsTheUser`, `Port_Dispose_ClosesAnOpenRecovery...`, `Port_Copy_UsesTheShellsClipboardService`, benchmark `Escape_WhenIdle`, Tab key, language switch, GDI leak, two-scale layout.
- `native-nightly` list: confirm the five new binaries after one night.
- `MUT-NEWFIXES` lane: merge, then sync the numbers.

## Cần người

- Attended recycle test (real Recycle Bin, Shell outcome).
- Decision on 404 (see `docs/reviews/ST-PARITY-SYNC4-2026-10-31.md` owner questions).
- Real RAW corpus run (RAW tests are data-gated; CI skips are false green).
- W10-B2 attended benchmark run (real iteration, real report, `PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN=1`).
- 144 DPI machine run and language/DPI/GDI/modal dialog scenarios; hand-break checks.
- WPF pixel comparison.
- Real mouse and pen hardware (drag feel, pen, OS double click); real multi-monitor and maximised/minimised F11.
- One native-nightly night to confirm the new binaries.
- Mutation runs left open (above).
