# CL-RECONCILE (2026-10-31): `reconcile_while_other_process_commits_latest_state_stays_committed`

Verdict: **(a) test defect, no production bug.** The test asserted something the C# test (and FA-01) never promised.

## Reproduction (before the fix)

`journal_reconcile` native test, `--include-ignored --test-threads=1`, same binary looped, isolated data root:

| Condition | Runs | Failed |
|---|---:|---:|
| idle machine | 100 | 2 (`c7` twice: raw last line `Failed`) |
| 12 busy PowerShell loops | 60 | 1 (`c5`) |
| after fixing assertion only, idle | 250 | 1 (different failure, below) |

## Cause 1: raw last line instead of the effective latest state

The two `OperationJournal` instances stand for two processes and share no gate. The reconciler can read the Id as
`Prepared`, then the owner writes the destination and appends `Committed`, then the reconciler's `append_if` (read,
compare, append is atomic only per instance) appends its stale `Failed` after the `Committed`. That is the documented
FA-01 cross-process residue: every reader (`read_latest_entry`, `read_pending_and_failed_operations`, `read_committed_moves`)
ignores a reconcile-coded `Failed` that follows `Committed`, so no undo or UI state is affected and `Committed` is never lost
or overwritten. The C# test asserts through `ReadFailedOperations`/`ReadPendingOperations`; the Rust port folded the RAW last
line per Id, which sees the residue. The loop of up to 10 000 reconciles in the Rust test (C#: one) makes the window likely.
Fix: assert the effective latest state with `read_latest_entry` (every line still has to parse, no glued or torn line).

## Cause 2: unbounded-by-scheduling append refusal

The owner's `append` has a bounded count of sharing-violation retries (Os error 32); under a hammering reconciler it can run
out. C# retries in the test like the production caller does. Fix: retry a sharing/lock violation with a 120 s deadline (no sleep).

## After the fix

120 idle runs and 60 runs under load (12 busy loops): 180/180 pass; the whole `journal_reconcile` file passes; clippy `-D warnings`
and fmt clean. Production code untouched. No PARITY row changes.
