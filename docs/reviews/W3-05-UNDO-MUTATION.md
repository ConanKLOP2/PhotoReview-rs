# W3-05 undo and file-action logic: mutation evidence (claim PARTIAL-mut-undo, Machine A, 2026-10-29)

> **Status: evidence of one run on `origin/master` 40066357 plus the killers added by this PR.** It is measured, not inferred.
> It does not replace the W3-05 verdict (see `docs/PARITY.md` row `F-10 / INV-4 (undo) / INV-6 (undo)`); it closes the
> mutation part of that row's gap.

## How it was run

`cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 --file '**/file_actions/<file>.rs' --cargo-arg=--test=<binary> ...`,
one process per file, `-j 1`, own `CARGO_TARGET_DIR` outside the worktree, at most two processes at once, cargo-mutants 27.1.0.
No `#[cfg(test)]` unit tests exist in these files, so `--cargo-arg=--lib` was not needed. Only fakes were involved: every
test binary used here was grepped first and uses `FakeRecycleBin` / `ScriptedBin` / `ScenarioBin` / `FaultBin` over
`FakeFileSystem`; the real Recycle Bin was never touched. `#[ignore]` native tests are skipped by cargo-mutants (they were
not run), and the native binary `file_actions_service_native` was not used at all.

| File | Test binaries used | Tested / total | Caught | Missed | Unviable | Timeouts |
|---|---|---:|---:|---:|---:|---:|
| `undo.rs` | `file_actions_undo`, `file_actions_undo_golden` | 199 / 199 | 177 | 3 | 19 | 0 |
| `service.rs` (1st pass) | `file_actions_service`, `file_actions_service_group`, `file_actions_service_golden` | 277 / 277 | 220 | 38 | 19 | 0 |
| `service.rs` (2nd pass, `--iterate` over the 38 missed) | `rename_*` (service, mutation_killers 1+2, file_action_matrix, recovery, fault_injection, policy, file_settings), `review_catalog_rename`, `capture_group_rename_plan`, `file_actions_matrix`, `file_actions_transaction` | 38 / 38 | 35 | 3 | 0 | 0 |
| `types.rs` | `file_action_types` | 6 / 6 | 6 | 0 | 0 | 0 |
| `destination_policy.rs` | `file_actions_destination_policy` | 15 / 15 | 12 | 0 | 3 | 0 |
| `duplicates.rs` | `file_actions_duplicates` | 21 / 21 | 20 | 1 | 0 | 0 |
| `partial_cleanup.rs` | `file_actions_partial_cleanup` | 14 / 14 | 13 | 1 | 0 | 0 |
| `recovery_check.rs` | `file_actions_recovery_check` | 83 / 83 | 72 | 3 | 8 | 0 |
| `errors.rs` | `file_action_errors` | 118 / 118 | 104 | 6 | 5 | 3 |

Every listed file was run to completion (no mutant was skipped by a time box). `service.rs` after both passes: 277 mutants,
255 caught (220 + 35), 19 unviable, 3 missed (below). The 3 `errors.rs` timeouts are `+=` -> `*=` mutants of `get_path_root`
that loop forever (`i *= 1`); a timeout is a detected mutant.

## Missed mutants and what became of them

Killers are in `crates/pr-core/tests/undo_mutation_killers.rs` and `undo_mutation_killers_2.rs`. After they were written, each
killed mutant was re-run against the killers binary alone (`--cargo-arg=--test=undo_mutation_killers[_2]`) and reported CAUGHT.

| Mutant | Verdict | Test / reason |
|---|---|---|
| `undo.rs:206` `UndoActionIdentity::eq` -> `true` and -> `false` | killed | `undo_action_identity_equals_itself_and_differs_from_a_later_registration` |
| `undo.rs:349` `last_undo_action_identity` -> `None` | killed | same test |
| `errors.rs:78` `Display for UnknownJournalCode` -> `Ok(Default::default())` | killed | `unknown_journal_code_display_names_the_code_in_the_english_text` |
| `service.rs:183` `Debug for FileActionService` -> `Ok(Default::default())` | killed | `file_action_service_debug_names_the_type_and_reports_the_gate_and_override` |
| `service.rs:790:21` `\|\|` -> `&&` in `run_group` (rename name check) | killed | `execute_group_rename_with_an_invalid_name_without_a_path_part_is_refused_and_changes_nothing` |
| `service.rs` 35 rename and group-rename mutants (lines 115, 193, 326, 328, 406, 407, 461, 522, 642, 643, 721, 789, 790:44, 800, 830-841, 1335-1342) | caught by the rename / matrix binaries | they were missed only because the first pass did not include `rename_*` and `file_actions_matrix` |
| `service.rs:684` delete field `permanently_deleted` in `single_failure` | equivalent (unreachable) | `permanently_deleted = run.permanent && mutation_completed`: `mark_mutation_completed` is called for a Recycle only at line 596, directly before `commit()` and the success return, so no failure can follow it; Move and rename have `permanent == false` |
| `duplicates.rs:110` `>` -> `>=` (`g.len() > 1`) | equivalent | a one-member group has `matching.len() == group.len()` when it matches, so `matching.remove(0)` leaves nothing to remove either way |
| `partial_cleanup.rs:32` `>` -> `>=` (`source_size > 0`) | equivalent | for `source_size == 0`, `stat.length < 0` is never true for a `u64` |
| `recovery_check.rs:327` guard -> `true`, `:328` `&&` -> `\|\|` | equivalent | the arm is reached only for `source.status == Exists`; `check_path` compares size and write time exactly for the source (`compare_last_write: true`), so `Exists` already implies both equalities |
| `recovery_check.rs:412` `==` -> `!=` (rule 1 of `aggregate_group_verdict`) | equivalent | rule 1 (undo Move, only CanRetry/AlreadyDone, contains CanRetry) gives CanRetry, and rule 5 gives the same verdict for every input rule 1 accepts, whatever the operation |
| `errors.rs:310:14` `<` -> `<=`, `310:20` and `310:29` `&&` -> `\|\|`, `310:25` `>` -> `>=`, `315:16` `>` -> `>=` (`get_path_root`) | equivalent | `sep_at(i)` already implies `i < len`; taking the separator directly after a device prefix gives `root_len` 5 whose normalisation drops the doubled separator, so the text equals the `root_len` 4 result (a killer attempt on `\\?\\foo` still reported them missed); at 315 `len > 2` vs `len >= 2` differs only for `len == 2`, where `sep_at(2)` is false |

`get_path_root(@"\\?\\foo")` = `\\?\` was checked against .NET 10 `Path.GetPathRoot` with a throw-away console project; the pin test
`get_path_root_device_prefix_followed_by_a_separator_stops_after_the_prefix` records it (it is a characterisation test, not a killer).
`execute_group_rename_member_whose_request_spelling_differs_but_is_already_named_is_skipped` is a second local pin of the
`already || target == *source` skip (its mutants at line 800 were already caught by the rename matrix).

No mutant exposed a production bug; no production code was changed.

## What was NOT run

- `file_actions_service_native` (real disk, `#[ignore]`) and the `#[ignore]` native cases of `file_actions_recovery_check` and
  `file_actions_partial_cleanup`: not part of any mutation run.
- Out of scope by the task: `rename*`, `journal*`, compaction, transaction, reconcile.
- `service.rs` was measured by three plus twelve test binaries; a mutant caught only by a binary outside that list would still
  show as missed, so the 3 remaining `service.rs` survivors (2 killed, 1 equivalent above) are the complete list for these binaries.
- Rows `F-23`, `D-05`, `RV-C05`, `AR16`: their gaps are about `SessionWriter` and `PhysicalFileSystem` (W2-13/W2-14), not about these
  files, so they were not touched. `F-21 / R01 (partial cleanup)` was already Done; its row got the counts above.
- Row `F-10 / INV-4 (undo) / INV-6 (undo)` stays Partial: `undo.rs` does not use the P-12 seam `try_get_file_stat`, and the
  `RecoveryRetryService` asserts of the `TODO(lead)` test are not ported.
