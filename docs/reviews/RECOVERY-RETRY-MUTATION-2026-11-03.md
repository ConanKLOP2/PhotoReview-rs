Status: evidence report (2026-11-03), lane mut-recovery-retry. PARTIAL: 54 of the 160 mutants of `recovery_retry.rs` were run (single-retry path and pure helpers only); PARITY status NOT raised.

# Recovery retry: mutation run of the single-retry decisions and pure helpers

File: `crates/pr-core/src/file_actions/recovery_retry.rs` (897 lines, `cargo mutants --list` = 160 mutants). Proving tests: `recovery_retry` (integration), `recovery_retry_golden` (golden `recovery-retry`), `journal_mutation_killers_4` (the only w3 killer file that names `RecoveryRetryService`). Tool: cargo-mutants 27.1.0.

## Result (real numbers, from `mutants.out/outcomes.json` via `tools/dev/mutants.sh`)

| Tested / total (of this run's filter) | Caught | Missed | Unviable | Timeouts | Not run |
|---|---|---|---|---|---|
| 54 / 54 | 49 | 1 | 4 | 0 | 0 of the filter; 106 of the file's 160 are outside the filter |

Preflight (unmutated test command, 3 test binaries): `running` 104 tests, exit 0. The run took 18 minutes (about 20 s build + 0 s test per mutant), budget 1800 s, `-j 1`.

## What was run

Filter (`-F`): `select_pending|same_fingerprint|is_null_or_white_space|is_move_or_copy|length_as_i64|retry_core|precheck_single|failure_message|already_handled`. That is the retry decision of the single (non-group) path: cancelled-needs-confirm, group routing, Move/Copy check, destination check, source pre-checks (`precheck_single`), the `prepared` entry, plus the pure helpers `select_pending`, `same_fingerprint`, `is_null_or_white_space`, `is_move_or_copy`, `length_as_i64`, `failure_message`, `already_handled`.

## What was NOT run (this report says nothing about it)

- Group path with real branching: `retry_group` (about 25 mutants), `precheck_group_members` (about 14), `run_group_members` (about 30), `run_single`, `execute_group_retry`, `execute_retry`, `copy_new_or_keep_leftover`, `is_restored_file`, `stat`, `committed_result`. They did not fit the 30 minute box (one build per mutant, machine shared with other mutation runs).
- Glue: `RecoveryRetryResult::{new, not_journaled}`, the builders `with_*`, `Debug::fmt`, `coded_error`, `plain_error`, and the `EnglishRecoveryRetryTexts` string methods (message text, no branching).
- An older full run exists (PARITY journal rows, CL-JRNL4 era: `recovery_retry.rs` 160 of 160, 142 caught, 2 missed, 16 unviable). This run is a re-measure of a subset on current master (after PR #584, `GateToken`); it does not replace or extend that number.

## Survivor (1): equivalent, no test added

- `:375:13` `delete field size from struct JournalEntry expression in RecoveryRetryService::retry_core`. The struct is `JournalEntry { state, error, error_code, size: length_as_i64(&source_stat).unwrap_or(i64::MAX), ..failed.clone() }`. Without the `size` line the value comes from `failed.size`. `source_stat` is returned by `precheck_single` only when `same_fingerprint(&stat, failed.size, ..)` held, i.e. `length_as_i64(stat) == Some(failed.size)`. So `prepared.size == failed.size` on every path that reaches the struct; the `unwrap_or(i64::MAX)` is dead for the same reason (a length above `i64::MAX` is refused earlier as "source changed"). No input can tell the two apart: equivalent, not killable by a test. The sibling deletions (`state`, `error`, `error_code`, `last_write_utc`, `timestamp_utc`) are caught.

## Unviable (4, do not compile)

`retry_core -> Ok(Default::default())`, `precheck_single -> Ok(Ok(Default::default()))`, `already_handled -> Default::default()`, `select_pending -> vec![Default::default()]` (the types have no `Default`).

## Commands (from the worktree root, `PY_BIN` = Python 3.12, `CARGO_BUILD_JOBS=3`)

```
TEMP=<repo>/.claude/tmp-mut-recovery-retry MUT_OUT=$TEMP/out bash tools/dev/mutants.sh pr-core \
  crates/pr-core/src/file_actions/recovery_retry.rs \
  --tests recovery_retry,recovery_retry_golden,journal_mutation_killers_4 --budget 1800 --jobs 1 \
  -F 'select_pending|same_fingerprint|is_null_or_white_space|is_move_or_copy|length_as_i64|retry_core|precheck_single|failure_message|already_handled'
```

## Verdict

The single-retry decisions and pure helpers of `recovery_retry.rs` are mutation-clean (49 caught, 1 equivalent, 4 unviable). The group path (`retry_group`, `precheck_group_members`, `run_group_members`, `run_single`) was not measured here and is the open gap. Production code and tests unchanged; no P-102/P-103 rows were needed.
