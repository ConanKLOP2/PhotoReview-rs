# Q7 PARITY close: rename rows (2026-11-03)

Branch `parity/rename` on `origin/master` 84d2c151. Machine: Windows 10, 12 logical CPUs, 6.4 GB free RAM, `CARGO_BUILD_JOBS=2`, own target dir. Native runs used `PHOTOREVIEW_RS_ISOLATE_CONFIG=1` and a temp `PHOTOREVIEW_RS_DATA_ROOT`. No production code changed. `cargo fmt -p` fails with os error 206 on long paths, so `rustfmt --edition 2024 --check` on the changed file was used.

## What ran in this run

| Run | Result |
|---|---|
| `pr-platform-win --test rename_prompt_dialog -- --include-ignored --test-threads=1` | 11 passed, 0 failed |
| `pr-platform-win --test rename_shell -- --include-ignored --test-threads=1` | 18 passed, 0 failed |
| `pr-platform-win --test rename_shell_e2e -- --include-ignored --test-threads=1` | 2 passed, 0 failed |
| NEW `pr-shell --test ui_smoke_rename -- --include-ignored --test-threads=1` | 1 passed, 0 failed |
| `pr-app` `presenter_rename` 20, `rename_presenter_matrix` 9, `rename_controller_matrix` 16, `rename_view_model_matrix` 18, `rename_mutation_killers` 13, `rename_mutation_killers_2` 21, `rename_partial_undo_rekey` 1 | all passed |
| `cargo clippy -p pr-shell --all-targets --locked -- -D warnings` | clean |

The new test `ui_smoke_rename::f2_on_the_real_shell_refuses_invalid_cancel_changes_nothing_and_a_vietnamese_name_renames_the_real_file` drives the REAL `ShellStartup` process through the `UiSmokeRun` rig: F2 opens the owned modal rename box (main window disabled), the invalid name `a:b` disables OK in place, Cancel leaves the folder untouched (3 entries), F2 again, the name `Ảnh đẹp` (Unicode through `SetDlgItemTextW`, `EN_CHANGE`, policy and the real file move) is accepted, the real file `Ảnh đẹp.jpg` exists, the old name is gone, the presented path follows, and the shell exits 0. It closes the gap "E2E drives a fixture window, not the real ShellStartup" and half of "no IME/Unicode input coverage" (Unicode via the control; real IME composition still needs a person).

## Mutation

Not run in this run (time box spent on native evidence; nothing in the rows has an open mutation gap that a machine can close: `X-w9r6` mutation figures stand from W9-R6-MUTATION, W9-RENAME-LANE and CL-MUT-STATE2; native glue `dialogs::rename_prompt` is excluded by policy). NOT RUN: 0 new mutants, 0 of 0.

## Rows

### X-w9r6-rename-view-retarget
- Gaps before: only the native UI / corpus / performance claim.
- Ran: the seven `pr-app` rename binaries above (98 tests pass) plus the real-shell F2 test (the view follows the rename on the real shell).
- Gaps after: attended GUI check that the displayed pixels and zoom are preserved across a rename on a real corpus, and a performance claim. No corpus on this machine, needs a person.
- Proposed status: stays Partial; add note "2026-11-03 Q7: pr-app rename binaries 98/98 and the real-shell F2 rename test pass; pixels/zoom keep on a corpus and performance need an attended run".

### X-w9r8-native-rename-dialog-shell
- Gaps before: 144 DPI / vi ink measurement is a structural proxy; `DialogServiceOption` wiring (already asserted by `startup_composition_root`).
- Ran: `rename_shell` 18/18 native (this run), plus `ui_smoke_rename` proves the production wiring (dialog service reaches the real box over the real port) end to end.
- Gaps after: the 144 DPI / vi ink measurement stays a structural proxy (a human visual check of the ink, no pixel oracle exists in the C# repo for this).
- Proposed status: stays Partial; note "2026-11-03 Q7: rename_shell 18/18 desktop run; production wiring now proven by the real-shell F2 test".

### X-w9r11-rename-dialog-e2e
- Gaps before: (a) not the real `ShellStartup`, (b) 144 DPI / vi ink structural only, (c) no IME/Unicode, no handle-leak counter.
- Ran: `rename_prompt_dialog` 11/11, `rename_shell_e2e` 2/2, new `ui_smoke_rename` 1/1.
- Gaps after: (a) closed by the new test for the F2 path (the menu id and context-menu paths remain in the fixture-window E2E; both reach the same command); (b) open; (c) Unicode through the control closed, real IME composition and handle-leak counter open.
- Proposed status: stays Partial; note "2026-11-03 Q7: real-ShellStartup F2 rename E2E added (`pr-shell/tests/ui_smoke_rename.rs`, 1/1, Vietnamese name); remaining: IME composition, 144 DPI vi ink, handle-leak count".

## Human needed (exact steps)
1. Attended run of the release exe on a real corpus: open a folder, zoom to 200 percent, pan, press F2, rename, check the pixels and zoom stay and the status text updates; Ctrl+Z restores. Time a rename on a large folder.
2. IME: with a Vietnamese Telex / VNI IME type `Ảnh đẹp` into the rename box, press Enter, check the file name.
3. 144 DPI and vi: set display scaling to 150 percent (144 DPI), set language vi, open the box with an error line shown, check by eye that the text is vertically centred and not clipped (C# `DialogEditCenteringTests`).
4. Handle count: note `Get-Process PhotoReviewRs | % HandleCount` before and after 50 open/Cancel cycles of F2.
