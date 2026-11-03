Status: evidence report (2026-11-03), lane mut-recovery-retry-2. Continuation of [RECOVERY-RETRY-MUTATION-2026-11-03.md](RECOVERY-RETRY-MUTATION-2026-11-03.md): the group path of `recovery_retry.rs` is now measured (72 new mutants: 67 caught, 0 missed, 5 unviable). The 34 glue mutants stay NOT RUN; PARITY status not changed by this lane.

# Recovery retry: mutation run of the group path

File: `crates/pr-core/src/file_actions/recovery_retry.rs` (`cargo mutants --list` = 160 mutants). Proving tests: `recovery_retry` (integration, many group tests), `recovery_retry_golden`, `journal_mutation_killers_4`. Tool: cargo-mutants 27.1.0, `tools/dev/mutants.sh`, `-j 1`, `CARGO_BUILD_JOBS=2`.

## Result (real numbers, from `mutants.out/outcomes.json` of three batches)

| Batch (filter) | Tested / total | Caught | Missed | Unviable | Timeouts | Time |
|---|---|---|---|---|---|---|
| A: `retry_group`, `precheck_group_members` | 42 / 42 | 40 | 1 (known equivalent) | 1 | 0 | 14 min |
| B: `run_group_members` | 38 / 38 | 37 | 1 (known equivalent) | 0 | 0 | 10 min |
| C: `run_single`, `execute_group_retry`, `execute_retry`, `copy_new_or_keep_leftover`, `is_restored_file`, `stat`, `committed_result` | 27 / 27 | 22 | 1 (known equivalent) | 4 | 0 | 5 min |

The `-F` regex of `cargo-mutants` did not narrow the batches as intended: each batch also contained `retry_core`, `already_handled` and `RecoveryRetryResult::not_journaled` mutants, so the batches overlap (107 runs, 81 distinct mutants). Distinct mutants, deduplicated by name and position:

| Distinct | Caught | Missed | Unviable | Timeouts | Not run |
|---|---|---|---|---|---|
| 81 | 75 | 1 | 5 | 0 | 0 |

Of the 81, 9 were already measured by the first report (`retry_core` 6, `already_handled` 1, `not_journaled` 2; their 8 caught and 1 missed re-confirm it, nothing new). The NEW group-path mutants are 72: **67 caught, 0 missed, 5 unviable, 0 timeouts**. Per function (all mutants of that function, distinct): `retry_group` 17 caught + 1 unviable, `precheck_group_members` 12 caught, `run_group_members` 23 caught, `run_single` 4 caught, plus `execute_group_retry`, `execute_retry`, `copy_new_or_keep_leftover`, `is_restored_file`, `stat`, `committed_result` (caught except the unviable ones below).

Preflight of every batch (unmutated test command, 3 test binaries): `running` 104 tests, exit 0. No `no tests/golden above` warning.

## Survivors

None new. The only missed mutant in every batch is `:375:13 delete field size ... in retry_core`, already classified as equivalent in the first report (`prepared.size == failed.size` on every path that reaches the struct). No killer test was needed, so `crates/pr-core/tests/recovery_retry_group_killers.rs` was not created (an empty test file would only add noise).

## Unviable (5, do not compile: the types have no `Default`)

`retry_group -> Ok(Default::default())` (:408), `execute_group_retry -> Default::default()` (:556), `execute_retry -> Default::default()` (:704), `stat -> Ok(Some(Default::default()))` (:796), `committed_result -> Default::default()` (:810).

## What was NOT run

34 of the 160 mutants (160 minus the 54 of the first report minus the 72 new): glue without branching, i.e. `RecoveryRetryResult::new`, the builders `with_recycle_bin`, `with_allow_permanent_delete`, `with_file_action_gate`, `Debug::fmt`, `coded_error`, `plain_error`, `failure_message`-adjacent string plumbing and the `EnglishRecoveryRetryTexts` string methods. This report says nothing about them. The exact count of 34 is arithmetic from the two reports, not a fresh `--list` diff.

## Commands (worktree root, `PY_BIN` = Python 3.12, `TEMP=<worktree>/t`)

```
bash tools/dev/mutants.sh pr-core crates/pr-core/src/file_actions/recovery_retry.rs \
  --tests recovery_retry,recovery_retry_golden,journal_mutation_killers_4 --budget 3000 --jobs 1 \
  --skip 'retry_core|not_journaled|already_handled' \
  -F 'RecoveryRetryService::(retry_group|precheck_group_members)( |$)'      # batch A; B and C: other function lists
```

## Verdict

With the existing tests, the group path of `recovery_retry.rs` (`retry_group`, `precheck_group_members`, `run_group_members`, `execute_group_retry`, `execute_retry`, `run_single`, helpers) is mutation-clean: 0 missed among 72 mutants. Together with the first report, 126 of 160 mutants are measured on current master (116 caught, 1 equivalent, 9 unviable); the remaining 34 are glue. Production code and tests unchanged.
