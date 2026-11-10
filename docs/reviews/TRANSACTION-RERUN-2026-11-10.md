Status: evidence report (2026-11-10), task T6 (F-12 / F-15 transaction row: rerun journal_rules, recovery_retry, compaction, transaction with the correct flag form). Production code unchanged, no test added. PARITY.md, task_on_progress.md NOT edited (the docs agent folds this in). Base: origin/master f804cedf.

# F-12 / F-15 transaction rerun

Setup: worktree `.claude/worktrees/transaction-rerun`, branch `test/transaction-rerun`, `tools/dev/mutants.sh pr-core <file> --tests ... --jobs 1` (its preflight runs the unmutated command first and needs `running N tests`, N > 0; `--baseline skip`, `--timeout 90`, own target dir per run under `%TEMP%\trrerun\<name>`, `CARGO_BUILD_JOBS=2`, budget 3600 s each). Three runs in parallel on a loaded shared machine. No `-F`: the filter does not narrow (P-111), so each file was run whole. Recycle Bin code is not in these files (fakes only in the tests; `journal_reconcile_recycle_bin` was not part of any run).

## Result per file

| file | tests passed to cargo (preflight) | decided of total | caught | missed | unviable | timeout | NOT RUN |
|---|---|---|---|---|---|---|---|
| `file_actions/journal_rules.rs` | `file_actions_journal_rules`, `journal_mutation_killers`, `journal_mutation_killers_2`, `w3_jrnl_killers5` (25 tests) | 22 of 22 | 21 | 0 | 1 | 0 | 0 |
| `file_actions/transaction.rs` | `file_actions_transaction`, `journal_mutation_killers`, `file_actions_matrix`, `journal_reconcile` (162 tests) | 52 of 52 | 37 | 1 | 14 | 0 | 0 |
| `io/journal_compaction_files.rs` | `io_journal_compaction_files`, `file_actions_journal_compaction`, `journal_startup_recovery`, `--native` (75 tests, `--include-ignored --test-threads=1`) | NOT decided (see below) | 0 | 0 | see below | see below | most of 100 |
| `file_actions/recovery_retry.rs` | NOT rerun here | - | - | - | - | - | all 158 (current verdict is the one of RECOVERY-RETRY-MUTATION*, GLUE-MUTANTS, 2026-11-03) |

journal_rules: identical to M5 (21 caught, 1 unviable). transaction: identical to M5 (37 caught, 1 missed, 14 unviable).

Survivor `transaction.rs:315:9` `impl Drop for JournalTransaction::drop` -> `()`: the same single survivor as M5. Its run proof is `n1_proof_transaction_drop` (N1, 2026-11-05, rule B); on today's master that test file passes unmutated (7 tests). I did not re-apply the hand mutant in this task.

## `io/journal_compaction_files.rs`: not a verdict

The run reached 24 of 100 mutants when this note was written: 0 caught, 9 unviable, 15 TIMEOUT. A timeout is never counted as caught, and these are not real signals: the unmutated compaction binaries take about 150 s on this machine (`file_actions_journal_compaction` 152 s in CL-JRNL3), so a 90 s `--timeout` expires on every mutant that gets as far as running them. The 15 timeouts (all of `PhysicalStagedReplacement` and friends in the first third of the file) therefore say nothing about the tests. To get a verdict the file needs `MUT_TIMEOUT` of at least 600 and about 100 x 3 to 6 minutes (5 to 10 hours), outside this task's 60 minute box. The raw run output is `%TEMP%\trrerun\comp\out` (not committed); the cargo-mutants process 18348 was still running when this was written and ends by its own `timeout 3600`. I tried to stop it by process id and the permission classifier denied it, so it was left alone.

Not run in this task (list): all 158 `recovery_retry.rs` identities (already decided on 2026-11-03), 76+ of 100 `journal_compaction_files.rs` identities (and no valid verdict for the 24 that ran), `journal.rs` and `journal_line.rs` (decided by CL-JRNL4 and JOURNAL-LINE-MUTATION*).

## F-15 live-marker part: the old preflight failure is gone

CX-JRNL2 reported the native reconcile concurrency preflight as failed; CL-JRNL3 recorded it green after PR #426. Today: `cargo test -p pr-core --locked --test journal_reconcile -- --include-ignored --test-threads=1` = 56 passed, 0 failed, 0 ignored (includes `reconcile_concurrent_commit_not_overwritten_by_failed_copy` and `..._by_failed_move`). So that blocker no longer applies on this machine.

## Suggested PARITY wording (not applied)

F-12 / F-15 (transaction): `journal.rs` 179/179 decided (CL-JRNL4), `transaction.rs` 52/52 (37 caught, 14 unviable, 1 survivor `Drop` proven equivalent by run in N1), `journal_rules.rs` 22/22 (21 caught, 1 unviable), `journal_line.rs` 141/141, `recovery_retry.rs` 158 decided in the 2026-11-03 reports; native `journal_reconcile` 56/56. Still open for Done: a valid mutation verdict for `io/journal_compaction_files.rs` (100 mutants, needs `MUT_TIMEOUT` >= 600). The interim CX-JRNL, CX-JRNL2 and CL-JRNL3 counts in the cell can be dropped.
