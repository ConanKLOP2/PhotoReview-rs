# W9-RENAME-MUTATION-2 rename path mutation evidence (round 2)

Verified 2026-10-29 on master `619f8c73` (Machine A claim `W9-rename-mutation`). Production code is unchanged; only two new test files and this report are added. Complements `W9-R1..R6-MUTATION.md`, which covered only a few functions each.

## What was run (and what was not)

All runs: `cargo mutants -p <crate> --baseline skip --timeout 90 -j 1 --output <dir> --file '<glob>' [-F '<regex>'] --cargo-arg=--test=<binary> ...`, one OWN `CARGO_TARGET_DIR` per run outside the worktree, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, at most three cargo-mutants processes at once, own PIDs only. Tests use `FakeFileSystem`, `FakeRecycleBin`, `FakeClock` and `TempRoot`; no test touches the real Recycle Bin.

Note: `-- --test <binary>` (after the double dash) does NOT restrict the build; cargo-mutants then compiled every `pr-app` test binary per mutant (first attempt, aborted, 2 to 3 minutes per mutant, not used as evidence). `--cargo-arg=--test=<binary>` restricts build and run to that binary (about 10 s per mutant) and is what the table uses.

| Group | Scope | Selected | Result before new tests | Result after new tests |
|---|---|---:|---|---|
| A | `pr-app` `coordinators/file_actions/actions.rs`, `-F 'rename_action\|on_renamed\|register_rename\|finish_rename_tail\|rename_undo\|apply_rename_undo'` (the whole rename and rename-undo coordinator path). Tests: `file_action_rename`, `file_action_undo`, `rename_controller_matrix` (+ `rename_mutation_killers` after) | 48 | 48 tested (2 shards of 24): 30 caught, 13 missed, 5 unviable, 0 timeouts | 48 tested: 39 caught, 4 missed, 5 unviable, 0 timeouts |
| B | `pr-app` `viewmodels/main/rename.rs`, `viewmodels/main/rename_prompt.rs`, `presenters/rename_prompt.rs` (whole files). Tests: `rename_view_model_matrix`, `rename_prompt_presenter`, `rename_presenter_matrix`, `presenter_rename` (+ `rename_mutation_killers` after) | 35 | 35 tested: 28 caught, 4 missed, 3 unviable, 0 timeouts | 35 tested: 32 caught, 0 missed, 3 unviable, 0 timeouts |
| C | `pr-core` `file_actions/rename.rs`, `rename_service.rs`, `case_only_rename.rs` (whole files). Tests: `rename_policy`, `rename_service`, `capture_group_rename_plan`, `rename_file_action_matrix`, `rename_fault_injection`, `rename_file_settings` | 134 | 134 tested: 110 caught, 7 missed, 15 unviable, 2 timeouts | survivors re-run with the new tests alone (see below) |

Not run: `coordinators/file_actions/actions.rs` outside the selected functions; `coordinators/file_actions/undo.rs` (`on_undone` rename-undo branch), `coordinators/presenter/rename.rs` (R6 covered `ordinal_ignore_case_suffix_start`), `input/*` rename shortcut routing, menus, native dialogs and every Win32/COM surface. Group A is a function-filtered selection, not a whole-file verdict. The two C timeouts are infinite loops (see below), reported as timeouts, not as clean catches.

## Survivors and disposition (24 missed in total, before tests)

### Killed by new tests (re-run proves it)

`crates/pr-app/tests/rename_mutation_killers.rs` (13 tests, fakes only; the executor seam `IFileActionExecutor` is wrapped to script worker results and a panic) and `crates/pr-core/tests/rename_mutation_killers.rs` (5 tests).

| Mutant | Killing test |
|---|---|
| actions.rs 325:73 `&&` to `\|\|`, `on_renamed` (panic path, stale folder) | `on_renamed_executor_panic_after_folder_switch_does_not_warm_preload_of_the_new_folder` |
| actions.rs 325:95 `>` to `>=`, `on_renamed` (panic path, empty catalog) | `on_renamed_executor_panic_warms_preload_only_for_a_non_empty_current_catalog` |
| actions.rs 362:26 `&&` to `\|\|` (stale-folder success without renamed paths is registered and announced) | `on_renamed_stale_folder_success_without_renamed_paths_is_no_change_and_not_registered` |
| actions.rs 420:22 `&&` to `\|\|` (a failed rename is registered for undo) | `on_renamed_failed_result_with_a_moved_source_rekeys_the_catalog_but_registers_no_undo` |
| actions.rs 426:13 `\|\|` to `&&`, 425/426 no-change expression | `on_renamed_current_folder_success_flagged_no_change_reports_no_change_and_registers_no_undo` |
| actions.rs 426:98 delete `!` (`&& !conflict`) | `on_renamed_group_success_without_renamed_members_is_no_change` |
| actions.rs 454:31 `>` to `>=` (empty catalog presents or parks) | `on_renamed_with_an_empty_catalog_completes_without_presenting` |
| actions.rs 583:31 `&&` to `\|\|` (`apply_rename_undo` re-keys the catalog for another folder) | `apply_rename_undo_partial_group_undo_in_another_folder_does_not_rekey_this_catalog` |
| actions.rs 597:31 `>` to `>=` (empty catalog presents after undo) | `apply_rename_undo_with_an_empty_catalog_does_not_present` |
| rename_prompt.rs 27:9 and 74:9 `Debug::fmt` to `Ok(Default::default())` | `prepared_rename_prompt_debug_names_the_type_and_the_source`, `rename_prompt_presenter_debug_names_the_type_and_the_source` |
| viewmodels/main/rename.rs 17:9 `on_files_renamed -> Default::default()` | `on_files_renamed_returns_the_catalog_changed_change_set_of_the_rekey` |
| viewmodels/main/rename.rs 60:12 delete `!` | `on_files_renamed_with_an_empty_map_reports_no_catalog_change` |
| pr-core rename.rs 22:9 `PathKeyMap::is_empty -> true` | `path_key_map_is_empty_only_before_the_first_insert` |
| pr-core rename.rs 63:9 `eq -> true`, 63:35 `&&` to `\|\|` | `path_key_map_equality_needs_equal_length_and_equal_entries` |
| pr-core case_only_rename.rs 24:27 `\|\|` to `&&` (`stat_as`) | `stat_as_without_a_case_only_twin_returns_the_stat_without_consulting_the_listing` |
| pr-core case_only_rename.rs 75:25 `>` to `<` (`is_stored_as`) | `stat_as_with_two_stored_spellings_keeps_the_stat_for_either_name` |
| pr-core rename.rs 271:17 `\|\|` to `&&` (`CaptureGroupRenamePlan::build`, sidecar owned by one stem) | `capture_group_plan_maps_a_sidecar_owned_by_either_stem_alone` |

Re-run proof: A (48 mutants, all A tests plus killers) and B (35) are the two re-run rows in the table (B: 0 missed). pr-core: the nine mutants of `PathKeyMap::is_empty`, `PathKeyMap::eq`, `stat_as` `||`-to-`&&` and `is_stored_as` `>`-to-`<` re-run against the killers binary alone: 9 tested, 9 caught; the `CaptureGroupRenamePlan::build` `||`-to-`&&` mutant: 1 tested, 1 caught.

### Remaining survivors (explained, not killed)

| Mutant | Why it is not killed |
|---|---|
| actions.rs 536:64 `>` to `==`, `<`, `>=` in `register_rename` (`renamed_to.len() + renames.len() > 512` reset) | Equivalent through the public API. `renamed_to` is private; it is only read in `on_undone` for the entries of the LAST registered rename, and every `register_rename` inserts the current renames AFTER the reset. Older entries are never consulted, so the 512 bound is a pure memory bound with no observable effect. |
| actions.rs 587:24 delete `!` in `apply_rename_undo` (entries not restored stay remembered) | Dead state at present. After a partial group undo the re-armed undo gets a new action identity, so the second undo is not treated as a rename undo and never reads `renamed_to` (shown while writing the 583 test). Not killable without changing production behaviour. |
| pr-core rename.rs 136:28 `extension_units > 0` to `>= 0` in `check_utf16` | Equivalent: with an empty extension the block strips an empty tail and re-trims whitespace that was already trimmed. |

### Observation (not changed here, not claimed as a bug fix)

While exploring the 587 survivor: after a PARTIAL failed undo of a renamed capture group (first undo restores jpg and cr2 and fails on the xmp), the second, successful undo restores all files on disk but its `Undone` result carries an EMPTY re-key map, so the catalog's capture group keeps the stale renamed sidecar path (`pic.xmp`) although the file is back as `shot.xmp`. Cause: the re-armed record has a new identity, so `on_undone` takes the generic (non-rename) path. I did not check the C# oracle for this; if C# re-keys the sidecar, this is a parity gap worth a PARITY row. No production code was changed.

### Timeouts

`rename.rs` 134:39 and 141:47 (`stem.len() - 1` to `stem.len() / 1` in the whitespace-trimming loops of `check_utf16`) hang in an infinite loop; cargo-mutants reports them as timeouts after 90 s. They are detected (the test run cannot finish) but not by an assertion; no change.

## Gates

`cargo fmt -p pr-app --check` and `cargo fmt -p pr-core --check` clean; `cargo clippy -p pr-app --all-targets --locked -- -D warnings` and `cargo clippy -p pr-core --all-targets --locked -- -D warnings` clean; `cargo test -p pr-app --locked --test rename_mutation_killers` 13 passed; `cargo test -p pr-core --locked --test rename_mutation_killers` 5 passed.

This is not a full mutation verdict for the rename feature: it is whole-file for the five pure-logic files in groups B and C, function-filtered for group A, and nothing else was mutated.
