# CL-STARTRERUN: correct-flag rerun of pr-shell startup mutation (2026-10-30/31, lane `cl-startrerun`)

Status: evidence report. Branch `feat/cl-startrerun`. No production change. Redoes CL-STARTUP groups a and b and the CX-DLGS 7 MISSED, per AUDIT-MUTATION-FLAG-2026-10-30.md.

## Method

`cargo mutants -p pr-shell --baseline skip --timeout 300 -j 1 --file ... --cargo-test-arg=--lib --cargo-test-arg=--test --cargo-test-arg=<binary> ... --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`, `CARGO_TARGET_DIR=<worktree>/target-mut`, outputs `mut-out-{a,a2,b,s,k}` (not committed), `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, data root inside the worktree, fake instance prefix `PhotoReviewRsTest...`. Native DLLs staged. Before the runs the unmutated command was executed (`running N tests`, exit 0), and the mutant logs show `running N tests` for every binary. Only the existing hidden-window native tests ran (the `ui_smoke` rig, hidden `MainWindow`, hidden dispatcher); no new GUI.

## Results

| Run | Files / selection | Binaries | Tested | Caught | Missed | Unviable | Timeout |
|---|---|---|---:|---:|---:|---:|---:|
| a (first) | startup/{options,forward_target,instance,recovery,smoke,state,shutdown,graph,phase_one}.rs | lib + the 8 CL-STARTUP group-a binaries | 62 | 12 | 45 | 5 | 0 |
| a2 (the 45 survivors) | same files, `-F` on the 45 line:col | group-a binaries + w8_startup_killers2, startup_run_native, startup_native, startup_wiring_native | 46 | 46 | 0 | 0 | 0 |
| b | startup/{overlay_vm,composition_root,run,window}.rs | lib, startup_overlay_vm, startup_composition_root, startup_run_native, startup_wiring_native, startup_native, w8_startup_mutation_killers, w8_startup_killers2 | 63 | 56 | 0 | 7 | 0 |
| s | secondary_dialogs.rs, the 7 CX-DLGS survivors (216-220, 283, 395) | startup_secondary_dialogs, w8_dialogs_killers3, w8_dialogs_mutation_killers | 7 | 0 | 7 | 0 | 0 |
| k | killers alone vs ScopeGate/Debug survivors (instance.rs 48/62/67/75, graph.rs 48/85, state.rs 101) | w8_startrerun_killers | 9+5 | 13 | 1 then 0 | 0 | 0 |

Group a (62): 57 caught, 5 unviable, 0 missed once the native run binaries and `w8_startup_killers2` are in the list. The CL-STARTUP "21 MISSED then 0" was therefore not a flag artefact alone: with only the group-a binaries 45 of 62 are MISSED (phase_one 117/120/208/241/246, recovery 27/46, shutdown StateSteps steps, smoke 31/45/55/85, ScopeGate, GlobalLog, Debug impls); they are caught only by `startup_run_native`/`startup_native`/`startup_wiring_native` and `w8_startup_killers2`. Group b: 56 caught, 7 unviable, 0 missed, 0 timeout (confirms CL-STARTUP).

## Secondary dialogs (CX-DLGS vs CL-REST2)

The 7 CX-DLGS survivors are real: all 7 MISSED with the correct form (settings fields `files`, `warnings`, `target`, `version`, `show_action_profiles` at lines 216-220; recovery `!` at 283; custom_zoom `==` -> `!=` at 395). CL-REST2's 0 MISSED came from a void run. They are observable only after the native dialog initialises (OnInit, edit control, Settings buttons), which needs a shown modal dialog; not done (no real GUI). They stay open native-observation gaps.

## New killers

`crates/pr-shell/tests/w8_startrerun_killers.rs` (5 tests, pass, hidden dispatcher and fake prefix, no sleeps): ScopeGate `mode` (PerFolder), `try_acquire` true then false, `forward_to_owner` false without owner and true with an owner, ScopeGate/StartupState/GraphInputs/StartupGraph Debug. They kill the instance.rs and graph/state Debug survivors without the native run binaries (run k: 14 of 14 identities caught after adding the owner test).

## Not run (time box)

CL-MAINWIN2 (host/main_window frame, viewing, windowing) and CL-HOST2 (ui_dispatcher, dpi) were NOT rerun; their evidence stays suspect (see AUDIT-MUTATION-FLAG-2026-10-30.md).

## Rows

No row changes status. X-startup-{composition-root, main-window, overlay-viewmodel, options-run, options, settings-load, instance} get `rerun OK 2026-10-31`; X-dialogs-secondary-port keeps Partial with the 7 MISSED named.
