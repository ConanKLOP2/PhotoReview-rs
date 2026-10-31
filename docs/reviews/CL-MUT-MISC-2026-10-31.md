# CL-MUT-MISC: mutation of P-09 checker scanner, startup secondary dialogs and rename presenter state (2026-10-31, lane `cl-mut-misc`)

Status: evidence report. No production change. Method per `AUDIT-MUTATION-FLAG-2026-10-30.md`: `cargo mutants --baseline skip --timeout 300 -j 1`, own `target-mut` and `--output` (deleted afterwards), `CARGO_PROFILE_{DEV,TEST}_DEBUG=0`. The unmutated binaries were run first (`running N tests`, N>0, exit 0) and mutant logs show `running N tests`. Test binaries were selected with `--cargo-arg=--test --cargo-arg=<bin>` (build and run only those binaries); the first attempt with `--cargo-test-arg=--test` compiled all 88 pr-core test binaries per mutant and was stopped (own PID only). A mutant caught by `golden_update_checker_cases` alone is reported as "golden only".

## Results

| File / scope | Tested | Caught | Missed | Timeout | Unviable | Not run |
|---|---:|---:|---:|---:|---:|---|
| `pr-core` `updates/checker.rs` `Scanner::{value,array,object}` + `scan_root_object` (binaries `updates_checker`, `w10_release_killers2`) | 39 | 31 (13 golden only: lines 775, 776 x5, 779 x2, 780 x2, 785, 792 x2) | 6 (768:26 x2, 768:30 x2 incl. `>`->`==`/`>=`, 772:35 x2) | 0 | 2 | 0 of 39 (the other 316 identities of the 355 were not part of this lane) |
| same 6 survivors, rerun with `mut_misc_killers` alone | 7 (the 6 plus 768:30 `>`->`<`) | 7 | 0 | 0 | 0 | - |
| the old cursor timeouts (`skip_ws`, `hex4`, `string`, `digits` `+=` -> `-=`/`*=`; lines 648, 676, 687, 692, 694, 720, 730 in this tree) with `mut_misc_killers` alone | 14 | 12 after fixing the killer (first pass: 11 caught, 1 missed 692:30, 2 timeouts 720:30 and 730:22 because two killers ran the scanner on the test thread) | 0 | 0 | 0 | the final 6 (692, 720, 730) rerun: 6/6 caught in 55 s |
| `pr-shell` `startup/secondary_dialogs.rs` (`startup_secondary_dialogs`, `w8_dialogs_killers3`, `w8_dialogs_mutation_killers`, `dialogs_recovery_composition`) | 73 | 15 | 7 | 0 | 51 | 0 |
| `pr-app` `coordinators/presenter/rename.rs` (`--lib`, `presenter_rename`) | 17 | 12 | 0 | 0 | 5 | 0 |
| `pr-app` `coordinators/presenter/state.rs` (`--lib` + `presenter_rename`, `presenter_state`, `presenter_missing`, `presenter_present_a/b`, `presenter_compare_pair`, `w8_presenter_killers2`, `w8_presenter_mutation_killers`, `rename_presenter_matrix`) | 123 | 85 | 25 | 1 | 12 | 0 |

## P-09 (checker scanner)

The 32 previously unrun identities of CX-REL (value, array, object, scan_root_object) were run: 39 identities, 6 real survivors, all in the container depth guard of `Scanner::value` (`depth + 1 > MAX_JSON_DEPTH`, 64 levels, and `self.object(depth + 1, ..)`). Killer `nested_object_depth_limit_is_64_inclusive` (64 levels accepted, 65 and 200 rejected) kills all 6 plus the `>`->`<` mutant (7/7 caught alone). `>`->`==` is also caught by it. The five old timeouts of CX-REL (detected only by the 300 s limit) are now caught in seconds: `scanner_cursor_always_advances_and_terminates` runs `Interpret` on a worker thread with a 10 s `recv_timeout`; one more survivor (`string` escape cursor 692 `+=`->`*=`, which decodes `_` in a property name as a backslash) is killed by `escaped_property_name_is_decoded_by_the_scanner`. No production bug found. Open as before: 316 of the 355 checker identities were not rerun here, Settings close/ShellExecute/TLS/PAC and .NET runtime oracle need a person.

## X-startup-composition-root / X-dialogs-secondary-port (`secondary_dialogs.rs`)

Rerun with the correct form and real tests: 73 identities (the file drifted by one since CX-DLGS), 15 caught, 51 unviable, 7 MISSED, 0 timeout. Same 7 as CL-STARTRERUN: `SettingsDialogOptions` fields files/warnings/target/version/show_action_profiles (216-220), recovery `auto_check: !recovery_no_auto_check` (266), custom_zoom `id == ID_OK` (378). They are observable only with a shown native modal dialog (OnInit, edit control, auto-check): NEED A PERSON, not rerun with native UI. The Recovery `allow_permanent_delete` part (line 392 `recovery_window_options`, fixed in #500) is caught. Locale/warmup/monitor (13 environment misses) were not touched.

## X-w9r6-rename-view-retarget

`rename.rs`: 17/17 run, 12 caught, 5 unviable, 0 missed. `state.rs`: 123 run (118 were unrun before): 85 caught, 12 unviable, 25 MISSED and 1 TIMEOUT. These survive every presenter test binary listed above, so they are real gaps (no killer written in this lane; time-box):

- Debug/Display impls: 131, 194, 506, 509:56 (`Ok(Default::default())` and `||`->`&&`); `clock()` 604; `PresenterChanges::bitor` 91:21 (`|`->`^`).
- `is_same_source_swap` 634 (false); `clear_presentation` 696:12 (`delete !`); `pump` 724:65 (`>`->`>=`, timeout, an infinite loop with the old test list).
- `wait_for_event` 751:39, 753:39, 753:75, 756:45, 759:31; `dispatch` 803:58, 803:67, 821:12; `has_wait_for` 827:9 (true and false), 827:62, 828:13, 831:46, 831:55, 832:13; `retire_child_waiters` 836, `track_child_present` 840.

## Killers added

`crates/pr-core/tests/mut_misc_killers.rs`: 3 tests (depth limit, scanner termination under deadline, escaped property name), pass, no sleeps, no network, inline strings (no golden).

## Suggested PARITY changes (PARITY.md and task_on_progress.md NOT edited)

- P-09: keep Partial; add "CL-MUT-MISC 2026-10-31: scanner value/array/object/scan_root_object 39 run, 6 survivors killed by `mut_misc_killers`, 5 old timeouts now caught"; 13 of the 31 first-pass catches are golden only.
- X-startup-composition-root and X-dialogs-secondary-port: keep Partial; the 7 survivors are confirmed (73 identities, 15 caught, 51 unviable) and need native dialog observation.
- X-w9r6-rename-view-retarget: keep Partial; `state.rs` is now fully run and shows 25 MISSED + 1 timeout, so the old "118 not run" becomes "123 run, 26 survivors open"; no "rerun OK".
