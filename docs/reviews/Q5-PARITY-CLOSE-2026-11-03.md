# Q5: PARITY close attempt for the Settings/config rows (2026-11-03, lane `pc-settings-config`)

Status: evidence report. Branch `parity/settings-config`, base `origin/master` 84d2c151. Rows: D-10, C-28, X-settings-presenter, X-startup-settings-load. All four STAY Partial (reasons per row below). No production code changed. Two native test files added; no PARITY/README/BACKLOG edits (the lead does those).

## What ran (this run, this desktop; Windows 10, interactive desktop, WIC decode because the pinned `libraw.dll`/`turbojpeg.dll` are NOT on this machine)

Environment: `CARGO_TARGET_DIR=C:\Users\PC\Projects\target-pc-settings-config`, `CARGO_BUILD_JOBS=2`, `--include-ignored --test-threads=1`. The pinned DLLs could not be staged (`tools/dev/stage-native.ps1`: "Missing source DLL ...\PhotoReview\native\x64\libraw.dll"; the C# checkout has no `native/x64`); only the unrelated failure 2 under "Found" depends on them.

| Crate / binary | Result |
|---|---|
| pr-platform-win `settings_dialog_apply_e2e` | 2 passed |
| `settings_dialog_frame_e2e` | 10 passed, 1 FAILED (`opening_and_closing_does_not_leak_gdi_or_user_objects`, see Found 1) |
| `settings_dialog_state_e2e` | 2 passed |
| `settings_dialog_layout_e2e` | 1 passed |
| `settings_reentrancy_native` | 1 passed |
| `settings_reset_page_native` | 2 passed |
| `w8_settingswin4_killers` | 29 passed (5 native included) |
| `dialogs_task_dialog_native` | 2 passed |
| NEW `settings_dialog_import_export_e2e` | 1 passed |
| pr-shell `startup_phase_one` | 2 passed |
| pr-shell `composition_graph` | 5 passed |
| pr-shell `ui_smoke_dialogs` | 7 tests (isolated one-per-process); output was cut at 6 result lines, all "1 passed", no FAILED line seen; Settings Save window among them |
| NEW pr-shell `startup_repair_notice_native` | 2 passed |
| pr-shell `startup_native` | 1 passed, 1 FAILED (`startup_with_logging_off_writes_no_log_file`: missing DLLs, see Found 2) |
| pr-app `startup_settings_load` 7, `startup_ui_hop` 4, `settings_presenter_load` 18, `_save` 36, `_apply` 31, `_shortcuts` 4, `settings_field_map` 32, `presenters_settings_form_state` 5 | all passed |

## Tests added (tests only)

1. `crates/pr-shell/tests/startup_repair_notice_native.rs` (native, 2 tests). Real shell rig (`UiSmokeRun`, production graph, `task_dialogs: true` activates comctl32 v6 so `TaskDialogIndirect` is real):
   - `startup_with_a_repaired_config_shows_the_notice_once_through_the_production_dialog_service`: config `{"ImageCacheRamPercent":999}`; an owned dialog appears after startup with title "Photo Review"; OK closes it; it does not return; first image renders; exit 0.
   - `startup_with_a_valid_config_shows_no_repair_notice`: valid config, no owned window.
   - Proven non-vacuous: `phase_two` step 6 mutated by hand (`dialogs.show_message(..)` replaced by a no-op): the repaired test FAILED (the wait for the dialog timed out after 60 s), the valid test passed; production restored (git diff empty).
2. `crates/pr-platform-win/tests/settings_dialog_import_export_e2e.rs` (native, 1 test) inside the REAL modal loop (`show_modal`) with a faked Shell picker: Export writes the file with the dialog's values; a cancelled Export writes nothing new; Import of a valid file reloads already-created page controls (shortcut box 5006 shows `F9`, not only the presenter state); Import of a repaired file shows exactly ONE warning titled "Import settings"; Import of non-JSON shows the "invalid settings" title and leaves the values; neither command saves. Proven non-vacuous with two hand mutants of `dialog/apply.rs::import_settings_from` (drop the repairs-notice `warn`; skip `load_all`): both made the test FAIL; production restored.
3. Style: `rustfmt --edition 2024 --check` clean on both new files (`cargo fmt -p` fails with os error 206 on long paths); `cargo clippy -p pr-shell --test startup_repair_notice_native -- -D warnings` and `cargo clippy -p pr-platform-win --test settings_dialog_import_export_e2e -- -D warnings` clean.

## Mutation (time-boxed, about 60 minutes in total; the machine was shared with about 30 other cargo processes, so 2 to 5 minutes per mutant)

`bash tools/dev/mutants.sh pr-platform-win <file> --tests <non-native binaries> --budget N` (`-j 1`, own target dir, preflight ok). Counts are `tested` from the outcomes, not `Found`.

| Batch | Files | Found | Tested | Caught | Missed | Unviable | Timeout |
|---|---|---:|---:|---:|---:|---:|---:|
| A (2400 s budget, exhausted) | `builder/groups.rs` + `builder/rows.rs` | 153 | 21 | 18 | 0 | 3 | 0 |
| B (1200 s budget, exhausted) | `dialog/surface.rs` with `-F 'static_text_kind\|combo_drop_height\|placement_change'` (the filter also matched other names; known overlap, see tools/dev/mutants.sh) | 92 | 19 | 17 | 0 | 2 | 0 |

Test binaries A (168 tests): settings_builder_inputs, settings_builder_layout, settings_framework, settings_page_{display,files,files_editors,general,mouse,shortcuts}, settings_pages_diag_perf, settings_pages_golden_by_name, w8_settingsui_killers, w8_settingswin{,2,3}_killers. B (40 tests): settings_dialog_{layout,state,frame}, w8_settingswin{,2,3}_killers.

No survivor, so no killer test was needed. Batch A reached `groups.rs` lines 10 to 66 (helpers `add/sub/mul`, `title_id`, `children(_mut)`, `visible`, `set_visible`, `control_ids`, Debug); batch B reached `static_text_kind` (lines 36, 38) and `combo_drop_height` (line 54).

NOT RUN (`cargo mutants --list` counts; no mutation verdict from this run):
- `builder/groups.rs` + `rows.rs`: 132 of 153.
- `builder/inputs.rs`: 199 of 199.
- `context.rs`: 94 of 94.
- `dialog/host.rs`: 80 of 80 (window host glue; policy says native tests, which passed above).
- `dialog/frame.rs`: 190 of 190.
- `dialog/surface.rs`: 111 of 130.
- Total NOT RUN: 806 of 846. At the measured rate this is 25 to 60 hours on a loaded machine; run it on an idle one, `-j 1`, per file, the native e2e binaries via `--native` (and skip `opening_and_closing_does_not_leak_gdi_or_user_objects`, see Found 1).

## Found

1. FLAKY native test, not a production leak: `settings_dialog_frame_e2e::opening_and_closing_does_not_leak_gdi_or_user_objects` fails reproducibly here (4 of 4 runs) with `GUI resources: (18, 4) -> (18, 5)`. A 20 cycle probe showed the USER object count oscillating between 4 and 5 and never growing, so one object is freed lazily; the single-sample assertion `after <= before` is too tight. PROPOSAL (not done): compare the minimum of several settled samples, or assert the series stays bounded. Not a parity gap in itself.
2. `pr-shell startup_native::startup_with_logging_off_writes_no_log_file` fails only because the pinned DLLs are missing (the app writes a forced error log when the probes fail, as the test comment documents). Not one of these rows.
3. First draft of the import/export test passed with `load_all` disabled: the default Next shortcut is already `Right` and re-activating a page re-reads state. Fixed in the final test (imported value `F9`, pages created and activated before the import, no re-activation after).

## Per row

### D-10 (Settings import/export)

Gaps before: `builder/{groups,rows,inputs}`, `context`, `host`, `frame`, `surface` not mutated; the complete modal import/export flow with a person at the machine.
Ran: tables above. The modal flow now has a native test through the real modal loop with a faked picker. Mutation: 40 more mutants, 0 missed.
Gaps after: 806 mutants NOT RUN; the real `IFileDialog` Open/Save UI and the flow driven by a person (needs a person at the machine).
Proposed status: stays Partial. Proposed note: "2026-11-03 Q5 ([Q5](reviews/Q5-PARITY-CLOSE-2026-11-03.md)): the modal import/export flow has a native test through the real modal loop (`settings_dialog_import_export_e2e`, faked Shell picker; 2 hand mutants of `import_settings_from` killed); Settings native tests rerun (apply_e2e 2, frame_e2e 10 of 11 (the 11th is a flaky GUI-count test), state_e2e 2, layout_e2e 1, reentrancy 1, reset_page 2, w8_settingswin4 29); mutation: groups/rows 21 of 153 tested (18 caught, 3 unviable), surface 19 tested (17 caught, 2 unviable), 0 missed; NOT RUN: inputs 199, context 94, host 80, frame 190, surface 111, groups/rows 132. The real file picker UI needs a person."

### C-28 (Settings repair notice)

Gaps before: the startup repair-notice-to-dialog chain (`phase_two` step 6) never exercised.
Ran: new native pair `startup_repair_notice_native` (2 passed) plus `startup_settings_load` 7, `startup_phase_one` 2, `composition_graph` 5, `dialogs_task_dialog_native` 2. A hand mutant of step 6 is killed.
Gaps after: the text of the box is not read back (a TaskDialog has no cheap text read; title and presence are asserted, the text is covered by `repair_notice`/`LocalizerSettingsTexts` tests and goldens). The real exe with an invalid `config.json`, looked at by a person, is not run.
Proposed status: Partial. Proposed note: "2026-11-03 Q5: repair notice shown once through the production dialog service in a real shell startup (`startup_repair_notice_native`, 2 native tests, hand mutant of step 6 killed); a valid config shows none. Not done: attended run of the real exe with an invalid `config.json`; box text not read back." The lead may judge the in-process production-graph run sufficient for Done; I did not promote because the real-exe look was not done in this run.

### X-settings-presenter

Gaps before: row text still says the Save tests use "Move" until #444 (stale: `settings_presenter_save` has 36 on master); `try_start_explorer` mutants unobservable; update check is a documented Different (D-W8-2).
Ran: pr-app `settings_presenter_load` 18, `_save` 36, `_apply` 31, `_shortcuts` 4, `settings_field_map` 32, `presenters_settings_form_state` 5: all passed. No new presenter mutation (earlier verdicts stand: 0 real survivors, 3 `try_start_explorer` spawn mutants unobservable by design, 2 equivalent clamp comparisons).
Gaps after: no machine-closable gap is left in the presenter. Remaining: the unobservable spawn mutants and the Different update-check split, both documented.
Proposed status: Partial stays only because of those two documented items; if the owner accepts them as final the row can move to Done. Proposed note: "2026-11-03 Q5: presenter tests rerun (load 18, save 36, apply 31, shortcuts 4, field_map 32, form_state 5, all passed); the #444 remark is stale."

### X-startup-settings-load

Gap before: "explicit repaired-settings notice displayed once through the production dialog service".
Ran: new `startup_repair_notice_native` 2/2; `startup_settings_load` 7/7, `startup_ui_hop` 4/4, `startup_phase_one` 2/2, `composition_graph` 5/5.
Gap after: the named gap is closed by the in-process rig (`production_graph_builder`, real `DialogService`, real `TaskDialogIndirect`, no fake). Remaining risk: the real exe, an attended run (listed under X-exe-entry).
Proposed status: Done is plausible once the lead accepts the rig run as "through the production dialog service"; proposed text "Done (2026-11-03 Q5): repaired-settings notice shown once through the production dialog service by `startup_repair_notice_native`; valid config shows none." I kept it Partial in my result because the real-exe run was not done.

## humanNeeded (exact steps)

1. Attended real-exe check (C-28, X-startup-settings-load): build `PhotoReviewRs.exe`, copy `%LOCALAPPDATA%\PhotoReviewRs\config.json` aside, put `{"ImageCacheRamPercent":999}` into it, start the exe, confirm exactly one information box titled "Photo Review" naming the reset setting appears after the main window, click OK, restart and confirm it does not appear again, restore the copy.
2. Attended import/export (D-10): open Settings, press Export, check the real Save dialog suggests `PhotoReview-settings.json` with the JSON filter, save; change a value; press Import, pick the file, confirm the value returns; import a copy with an invalid value and confirm one notice.
3. Stage the pinned `libraw.dll` and `turbojpeg.dll` (`tools/dev/stage-native.ps1`; the C# checkout here has no `native/x64`) and rerun `pr-shell startup_native` (Found 2).
4. Run the 806 NOT RUN mutants on an idle machine.
