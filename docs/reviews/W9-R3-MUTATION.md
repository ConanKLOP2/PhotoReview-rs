# W9-R3 selected postmerge mutation and group-completion regression

Verified 2026-10-27. Production source is merged PR #282, `614ce83181beba1a11a8836e52a411cce3c8ed58`; this follow-up changes tests and documentation only.

- File: `crates/pr-core/src/file_actions/reconcile.rs`; selected function `is_group_member_completed`; test binaries `rename_recovery` and `file_actions_matrix`.
- Selected10of80file mutants; initial7caught/3missed/0unviable/0timeouts, exit2, about6minutes.
- Added one public startup-reconcile regression with five rows: exact Recycle undo fingerprint, matching timestamp with a different size, matching size with a different timestamp, Move with both paths present, and completed Move with its source absent. The oracle is C# `OperationJournal.IsGroupMemberCompleted` at `7eaee2a2`; journal verdicts are asserted through the public API. This is an added mutation regression, not an invented C# test port.
- Focused baseline: `file_actions_matrix`76passed plus `rename_recovery`15passed/1native ignored; zero failures. All new cases use fake filesystem and fake Recycle Bin, with TempRoot-derived paths.
- Rerun of the same10:10caught/0missed/0unviable/0timeouts, exit0, about6minutes. Total20executions of10unique mutants;70other file mutants NOT RUN. This is not a full-file or full-R3 mutation verdict.
- The three initially missed mutants each failed the new regression on rerun (wrong size accepted, exact timestamp rejected, or Move source still present accepted), with successful builds; catches are not unrelated failures.
- Both runs used baseline skip, timeout90, -j1, an owned external target and900-second outer guards; neither exhausted its budget. No production code, native filesystem behavior, RAW corpus or desktop evidence changed. The existing native test was ignored during these runs; its earlier explicit PASS belongs to PR #282.

Local evidence directory: `%TEMP%/goal-r3-mutation-3824484e`. Hashes identify the completed evidence independently of its temporary location.

| Evidence file | SHA-256 |
|---|---|
| `result.txt` | `FF219FCE09538A868FD79DE6FD9ED36A778863FC2BB00E986ADD2E70FFC60590` |
| `selected.json` | `6958FB121FE9AB9BDA48F2EE01D5F72A44353815BB1C9E45B0F3E3F0055A77A3` |
| `mutants.out/outcomes.json` | `F2F20CC3798AC4FBE19663D902F1AF87F095EB87F702374194890ED0DE291651` |
| `rerun/result.txt` | `25E2AFBD07B8A157E599C8132ACC8EA5E3BF52B8ABBCB508E5442340100237C1` |
| `rerun/mutants.out/outcomes.json` | `BBDE7719A255CD783226DFDDFF67EF9C79DE57842E983556E31682FE0979718C` |
| `rerun/test-regressions.diff` | `1745D02E5FD650E5D26CF9DB1927BA776E9878BA407DCBA09ADA6460A67B8764` |

| Mutant | Initial | Rerun |
|---|---|---|
| `crates/pr-core/src/file_actions/reconcile.rs:455:5: replace is_group_member_completed -> io::Result<bool> with Ok(true)` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/reconcile.rs:455:5: replace is_group_member_completed -> io::Result<bool> with Ok(false)` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/reconcile.rs:457:22: replace == with != in is_group_member_completed` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/reconcile.rs:458:23: replace != with == in is_group_member_completed` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/reconcile.rs:459:23: delete ! in is_group_member_completed` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/reconcile.rs:464:17: replace && with \|\| in is_group_member_completed` | MissedMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/reconcile.rs:464:40: replace == with != in is_group_member_completed` | MissedMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/reconcile.rs:471:31: delete ! in is_group_member_completed` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/reconcile.rs:478:51: replace && with \|\| in is_group_member_completed` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/reconcile.rs:478:36: delete ! in is_group_member_completed` | MissedMutant | CaughtMutant |
