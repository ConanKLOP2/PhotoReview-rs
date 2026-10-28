# W9-RENAME-MUTATION-3 rename and undo mutation evidence (round 3)

Verified 2026-10-29 on master `cc3e183c` (Machine A claim `W9-rename-mutation-3`). Production code is unchanged; only two new test files and this report are added. Continues `W9-RENAME-MUTATION-2.md`, which left four files not run: the rest of `coordinators/file_actions/actions.rs`, `coordinators/file_actions/undo.rs`, `coordinators/presenter/rename.rs` and `pr-core` `catalog/rename.rs`.

## What was run

All runs: `cargo mutants -p <crate> --baseline skip --timeout 90 -j 1 --output <dir> --file '<glob>' [-F|-E '<regex>'] --cargo-arg=--test=<binary> ...`, one OWN `CARGO_TARGET_DIR` per run outside the worktree, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, at most two cargo-mutants processes at once, own PIDs only. Tests use `Env`/`FakeExecutor`/`InertBin`, `FakeFileSystem`, `FakeRecycleBin`, `FakeClock` and `TempRoot`; no test touches the real Recycle Bin.

| File | Selected | Tests | Tested | Caught | Missed | Unviable | Timeouts |
|---|---:|---|---:|---:|---:|---:|---:|
| `pr-app` `coordinators/file_actions/undo.rs` (whole file) | 92 | `file_action_rename`, `file_action_undo`, `file_action_core`, `file_action_move_copy`, `file_action_recycle`, `rename_controller_matrix`, `rename_partial_undo_rekey`, `rename_mutation_killers` | 92 | 65 | 15 | 11 | 1 |
| `pr-app` `coordinators/presenter/rename.rs` (whole file) | 17 | `presenter_rename`, `rename_view_model_matrix`, `rename_presenter_matrix`, `rename_mutation_killers` | 17 | 10 | 2 | 5 | 0 |
| `pr-core` `catalog/rename.rs` (whole file) | 22 | `review_catalog_rename`, `rename_mutation_killers` | 22 | 20 | 1 | 1 | 0 |
| `pr-app` `coordinators/file_actions/actions.rs`, everything EXCEPT `rename_action\|on_renamed\|register_rename\|finish_rename_tail\|rename_undo\|apply_rename_undo` (`-E`; those were group A of round 2) | 100 | same as `undo.rs` | 100 | 72 | 14 | 14 | 0 |
| Total | 231 | | 231 | 167 | 32 | 31 | 1 |

Nothing selected was left untested. NOT run: `input/*` rename shortcut routing, menus, native dialogs and every Win32/COM surface (out of scope, tested natively by hand), and mutants of the files above under test binaries other than those listed (a survivor of the listed binaries may be caught by an unlisted one; none was checked beyond the lib run below). The 31 unviable mutants do not compile and were not re-run. This is a whole-file verdict for the four files, not for the rename feature as a whole.

## Survivors and disposition (32 missed before the new tests)

New tests: `crates/pr-app/tests/rename_mutation_killers_2.rs` (21 tests) and `crates/pr-core/tests/rename_mutation_killers_2.rs` (1 test). Re-run proof: the survivors were re-run against the new binary alone (`--cargo-arg=--test=rename_mutation_killers_2`): `undo.rs` 17 selected (15 survivors plus two neighbours at 236:56), 15 caught; `actions.rs` 21 selected (14 survivors plus neighbours), 13 caught; `catalog/rename.rs` 12 selected (`rename_in_entry`), 6 caught by the single new test, 5 missed (these are the ones `review_catalog_rename` already killed in the main run, which that proof run did not include), 1 unviable. The mutants still missed in these re-runs are the explained ones below, plus neighbours (236:56 `<`, 755:58 `==`/`<`, 1052:100 `<`, 1297:27 `false`) that the older binaries already catch.

### Killed by new tests

| Mutant | Killing test |
|---|---|
| `undo.rs` 93:26 `+=` to `-=`, `*=` (`undo_last` ticket) | `undo_last_gives_each_undo_its_own_ticket_and_drops_the_event_of_the_older_one` |
| `undo.rs` 183:91 delete `!` (parked rename-undo status, note filter) | `rename_undo_parked_status_shows_a_non_empty_note_and_ignores_an_empty_one` |
| `undo.rs` 197:27 delete `!` (failed rename undo status) | `rename_undo_failed_result_shows_nothing_to_undo_or_its_own_message` |
| `undo.rs` 202:83 delete `!` (succeeded rename undo note filter) | `rename_undo_succeeded_result_shows_a_non_empty_note_and_ignores_an_empty_one` |
| `undo.rs` 236:56 `>` to `>=` (failed partial undo into an empty catalog) | `failed_partial_undo_of_a_non_image_into_an_empty_catalog_does_not_present` |
| `undo.rs` 274:48 `>` to `==`, `<`, `>=` (Recycle undo into an empty catalog) | `recycle_undo_into_an_empty_catalog_presents_the_restored_photo_then_updates_the_session`, `recycle_undo_of_a_non_image_into_an_empty_catalog_does_not_present` |
| `undo.rs` 277:51 `>=` to `<` (present index of the named photo) | `recycle_undo_into_an_empty_catalog_presents_the_named_restored_photo` |
| `undo.rs` 297:13 and 298:13 `&&` to `\|\|` (Move branch) | `undo_of_a_copy_in_the_current_folder_does_not_insert_or_present_anything` |
| `undo.rs` 370:20 delete `!` (`run_undo_tail` RecyclePresented) | `recycle_undo_folder_switch_during_present_does_not_write_session_or_status` |
| `undo.rs` 398:38 `>=` to `<` (`remember_move_position` bound) | `remembered_move_positions_survive_below_the_bound` |
| `actions.rs` 672:17 `\|\|` to `&&`; 672:66 `<` to `==`, `<=` (confirmation re-check) | `confirm_then_folder_switched_alone_abandons_a_move`, `confirm_then_executor_busy_alone_abandons_a_move`, `confirmed_move_goes_ahead_for_index_zero_and_stops_for_an_unlisted_path` |
| `actions.rs` 787:26 `+=` to `*=` (`execute_core` ticket) | `action_tickets_differ_and_the_event_of_an_older_action_is_dropped` |
| `actions.rs` 946:29 `&&` to `\|\|` (failed capture Copy restores nothing) | `failed_group_copy_does_not_restore_or_announce_a_catalog_change` |
| `actions.rs` 959:37 `&&` to `\|\|` (only members still on disk return) | `failed_group_move_restores_only_the_members_still_on_disk` |
| `actions.rs` 1014:61 `&&` to `\|\|` (late capture Copy or Move without destination) | `late_group_copy_and_late_group_move_without_destination_show_no_status` |
| `actions.rs` 1052:100 `>` to `>=` (late failed Delete, zero processed) | `late_failed_group_recycle_with_no_processed_member_shows_no_status` |
| `actions.rs` 1088:29 `&&` to `\|\|` (a Copy remembers no position) | `successful_copy_does_not_remember_a_position_for_a_later_move_undo` |
| `actions.rs` 1204:24 `==` to `!=` (`executor_threw` Recycle dialog) | `executor_throw_shows_the_recycle_failure_dialog_only_for_a_delete` |
| `pr-core` `catalog/rename.rs` 118:29 `!=` to `==` (`rename_in_entry`) | `rename_entries_identity_map_on_a_capture_group_is_a_no_op` |

### Not killed, explained

| Mutant | Why |
|---|---|
| `undo.rs` 150:45 `&&` to `\|\|` (`!new_path.is_empty() && !source.is_empty()`) | Equivalent. The block is only reached for a succeeded undo in the folder, so `source` is never empty; with `\|\|` an empty `new_path` inserts a blank key, `PathKeyMap`/`rename_entries` reject a blank key and `apply_rename_undo` resets the map to empty, the same as no insert. |
| `actions.rs` 755:42 `&&` to `\|\|` (`previous_path` for a non-removing action) | Unobservable: `previous_path` is read only in `finish_success` under `is_remove`. |
| `actions.rs` 755:58 `>` to `>=` | Equivalent: `source_index == 0` gives `usize::try_from(-1)` = `None`, the same as the else branch. |
| `actions.rs` 902:21 `&&` to `\|\|` (group `source_removed`) | Unobservable: for a group result `source_removed` is only forwarded to `report_late`, whose group arm ignores it, and the unverified-Move branch requires a single result. |
| `actions.rs` 1297:27 match guard to `true` | Equivalent: only `PartnerMissing` sets `follow_up` and it always returns `done(true)`. The `false` variant is caught by the older tests. |
| `presenter/rename.rs` 10:5 `ordinal_ignore_case_suffix_start` to `Some(0)`, `Some(1)` | Test-selection artifact: the function is private and pinned by in-file `#[cfg(test)]` unit tests that `--cargo-arg=--test=...` does not run. Re-run with `--cargo-arg=--lib` over the function: 4 tested, 4 caught, 0 missed. |
| `undo.rs` 477:19 `index += 1` to `*= 1` in `sorted_insert_index` (timeout) | Infinite loop, reported as a timeout after 90 s: detected, but by the clock, not an assertion. No change. |

The known unobservable mutants from round 2 (512 bound in `register_rename`, `apply_rename_undo` ~587) were already excluded: they belong to the functions of group A and were not in this selection.

No production defect was found; no C# cross-check was needed (every test pins behavior that existing C#-ported tests already assert for the neighbouring cases).

## Gates

`cargo fmt -p pr-app --check` and `cargo fmt -p pr-core --check` clean; `cargo clippy -p pr-app -p pr-core --all-targets --locked -- -D warnings` clean; `cargo test -p pr-app --locked --test rename_mutation_killers_2` 21 passed; `cargo test -p pr-core --locked --test rename_mutation_killers_2` 1 passed.

Operating note: a normal `cargo test` in a `CARGO_TARGET_DIR` that a cargo-mutants run also used can link a stale mutated library (the new pr-core test failed on unchanged code until `cargo clean -p pr-core` in that private directory). Use separate target directories for mutation and for verification.
