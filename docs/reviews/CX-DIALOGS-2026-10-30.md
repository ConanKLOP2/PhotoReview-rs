# Dialogs lane: bounded mutation evidence and killers (2026-10-30, lane `cx-dialogs`)

Status: evidence report. Branch `feat/cx-dialogs`. Started by Codex (quota ran out), finished by Claude. C# reference at `7eaee2a2` (read-only). No PARITY row reaches Done: every row names further gaps beyond mutation (see the table at the end).

## Scope and honesty

The lane inventory (Codex, `cargo mutants --list` over the 51 dialogs files) is 2,993 mutants, including Win32/COM glue that policy excludes from mutation. Nine pure-logic files with 697 mutants were selected, and a bounded, targeted sample of 56 mutants (plus 2 found by filter spill-over) was run. The other mutants of those nine files and all other files were NOT mutated.

## Pass 1: existing covering test binaries (Codex)

Command shape: `cargo mutants -p pr-platform-win --baseline skip --timeout 90 -j 1 --file <file> -F <fn> --cargo-arg=--test=<binary>`; outputs kept in `mut-out-*` (not committed).

| Group | File:function | Binary | Mutants | Caught | Missed | Unviable |
|---|---|---|---|---|---|---|
| painter | `native/painter/geometry.rs` `paint_scale` | `dialogs_painter_geometry` | 5 | 5 | 0 | 0 |
| controls | `native/painter/controls.rs` `frame_rects` | `dialogs_painter_controls` | 9 | 9 | 0 | 0 |
| frame | `native/painter/frame.rs` `border_layers` | `dialogs_painter_frame` | 7 | 5 | 1 | 1 |
| secondary | `secondary_window_port.rs` `try_read_recovery_entries` (+ filter spill-over `prompt_add_external_editor` fields) | `dialogs_secondary_window_port` | 5 | 2 | 2 | 1 |
| benchmark | `benchmark/results.rs` `row_color_key` | `dialogs_benchmark_results` | 5 | 5 | 0 | 0 |
| recovery | `recovery/actions.rs` `command_action` | `dialogs_recovery_actions` | 12 | 11 | 0 | 1 |
| framework | `settings/catalog.rs` `pending_keys_with` | `settings_framework` | 5 | 5 | 0 | 0 |
| pages | `settings/pages/shortcuts.rs` `find_problems` | `settings_page_shortcuts` | 7 | 7 | 0 | 0 |
| window | `settings/dialog/page_layout.rs` `bar_line` (3 mutants via `--in-diff`) | `settings_dialog_layout` | 3 | 3 | 0 | 0 |

Survivors of pass 1 and their disposition:

- `frame.rs:68:28` `edge > 1` -> `>= 1` in `border_layers`: EQUIVALENT. With `edge == 1` the loop `1..1` is empty, so the returned vector is unchanged. Unobservable.
- `secondary_window_port.rs:474:13` and `:475:13` (delete field `name` / `path` in `Win32SecondaryWindowPort::prompt_add_external_editor`): real, observable survivors (picked name or path silently lost). Killed by `editor_result::prompt_add_external_editor_ok_preserves_name_and_path_cancel_returns_none` (native, fake picker and prompt factory, `TempRoot`, ready hook closes the test-owned dialog, no real editor).
- Unviable (3): `BorderLayer` and `CommandAction` have no `Default`; `try_read_recovery_entries -> Some(vec![Default::default()])` cannot build for the same reason. Not counted as caught.

## Pass 2: independent proof with the killers binary alone

New file `crates/pr-shell/tests/w8_dialogs_mutation_killers.rs` (11 tests, 1 native/ignored, no production change; no production bug found). Mutating `pr-platform-win` with `--test-package pr-shell --cargo-arg=--test=w8_dialogs_mutation_killers`, `-- -- --include-ignored --test-threads=1`:

- Run A (nine files, 64 mutants, because `-F bar_line` also matched Win32 `update_scroll_bar` / `scroll_by_bar` glue and `-E` was not honoured): 52 caught, 3 unviable, 9 missed. The 9 missed: 2 editor fields (ignored test not enabled in that run, see run B), the equivalent `frame.rs:68`, and 6 `ScrollInfo` field deletions in Win32 scroll-bar glue (out of scope, not covered by the killers; NOT killed).
- Run B (`prompt_add_external_editor`, 7 mutants, native test enabled): the 2 editor-field mutants are caught. Two further mutants of `SecondaryWindowRouter::prompt_add_external_editor` (`secondary_window_port.rs:635`, body replaced by `None` / `Some(Default)`) were MISSED; the new test `router_editor::router_prompt_add_external_editor_forwards_supported_and_returns_none_otherwise` (pure, fake native port) kills them: re-run = 7/7 caught.
- Net for the 58 intended mutants: every viable one is caught by the killers binary alone except the one equivalent mutant; 3 unviable.

Killers binary: 11 passed; repeated 20x with `--include-ignored --test-threads=1`, 0 failures (no clocks, no sleeps, `TempRoot` and fakes only).

## Gate

`cargo fmt -p pr-shell --check`, `cargo fmt -p pr-platform-win --check`, `cargo clippy -p pr-shell --all-targets --locked -- -D warnings`, `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings`: clean. Codex also reported the native suites of the covering binaries (116 native tests, `--include-ignored --test-threads=1`) passing; Claude did not re-run them. `tools/dev/stage-native.ps1` could not be re-run by Claude (PowerShell execution policy refused the script); the killers' native test needs no staged DLL.

## Rows (all stay Partial)

| Row | Mutation done here | Remaining gap |
|---|---|---|
| X-dialogs-painter | `paint_scale` 5/5 | rest of painter mod/geometry not mutated; pixel results depend on machine font/DPI |
| X-chrome-painter-controls | `frame_rects` 9/9 | 144 dpi test, empty-rect `fill`, rest of controls.rs not mutated |
| X-chrome-painter-frame | `border_layers` 5 caught, 1 equivalent, 1 unviable | `SS_ETCHEDHORZ/VERT`, rest of frame.rs not mutated |
| X-dialogs-secondary-port | `try_read_recovery_entries`, editor prompt fields, router editor prompt | production factories/services, W9 routing, rest of file not mutated |
| X-dialogs-zoom-diagnostics-profiles-benchmark | `row_color_key` 5/5 | rest of benchmark/zoom/diagnostics/profiles not mutated; language/DPI/GDI scenarios; production-codec benchmark |
| X-dialogs-recovery | `command_action` 11 caught, 1 unviable | other recovery files not mutated; P-43, DPI/GDI/cursor/modal, hand-break |
| X-settings-ui-framework | `pending_keys_with` 5/5 | rest of framework/builder not mutated; goldens `settings-layout`, `settings-builder` not loaded |
| X-settings-ui-pages | `find_problems` 7/7 | other pages not mutated; per-page goldens by name |
| X-settings-ui-window | `bar_line` 3/3 | `ScrollInfo` glue survivors, rest of dialog/* not mutated; DPI/GDI/language |

Human-only gaps are unchanged (attended hardware measurements W8-12j, W10-B2, `ui_smoke_perf`; real RAW camera files).
