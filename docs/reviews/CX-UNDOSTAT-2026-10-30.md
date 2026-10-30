# CX-UNDOSTAT: Recycle undo stat error seam (2026-10-30)

Worktree `cx-undostat`, branch `feat/cx-undostat`, starting Rust HEAD
`bf0aed9d3e7358d80cb8e5fe040079b1ba3edf10`. Changes are left uncommitted for the orchestrator.
Only row `F-10 / INV-4 (undo) / INV-6 (undo)` is owned by this lane.

## Oracle and scope

C# baseline `7eaee2a2`, read-only via `git show`:
`src/PhotoReview.Core/FileActions/UndoService.cs`.
Single Recycle `UndoLastAsync`, lines 351-387, calls `GetFileStat` at 373 and has
only `finally { End(); }`: a stat exception escapes; the saved bin exception does
not replace it. Group Recycle `UndoGroupRecycleAsync` calls `GetFileStat` before
restoring and after each successful restore; its catch calls `tx?.Fail`, remembers
partial undo, settles already verified restored members and returns `CoreUndoFailed(ex.Message)`.

**API limitation:** Rust's existing `undo_last() -> UndoResult` cannot return an
`io::Error` separately. This focused fix reports the single-Recycle stat error as
`CoreUndoFailed`, retains the action and releases the gate. That closes the silent
Err-to-None seam, but does not reproduce C# single-Recycle exception propagation.
This remains an explicit parity gap, not an accepted intentional difference or a
claim of Done. No public API or caller was changed.

## Test-first fix and safety

New binary `crates/pr-core/tests/w3_undo_stat_parity.rs`: a local `FileSystem`
wrapper delegates to `FakeFileSystem` and injects an error on a selected stat call.
It deliberately folds errors into None for the legacy `get_file_stat`, while
`try_get_file_stat` preserves them. The three site tests check PermissionDenied and
InvalidInput with deterministic fixture clocks and metadata (no wall clock or sleeps).

Before production edits, all three tests failed:

- single post-restore returned `Could not restore from the Recycle Bin: fault.raw`;
- group pre-check went on to restore and reported success;
- group post-restore returned the restore-failed text instead of the inspection error.

After the fix all three passed. A fourth test pins stat-error precedence when the
fake bin restores the file and then returns its own error; it also passes. Production changes only the `stat` helper to return
`io::Result<Option<FileStat>>`, handles the single-Recycle error, and adds `?` at
the two group sites. The production bug was supplied by the seam audit and reproduced
before the fix; no mutation survivor prompted an unrelated production change.

The tests pin INV-4 gate release and action identity retention; a subsequent call
proves the gate is usable. Single Recycle still writes no journal and protects the
file already restored from a second shell restore. Group pre-check error makes no
bin calls and writes no undo journal. Group post-check error returns only verified
restored paths, writes Prepared then Failed for the same ID with Undo=true, never
Committed, and resolves the remembered partial undo on retry once inspection works.
Existing undo/golden tests cover the other journal settlement and fingerprint rules.
All bin calls use `ScriptedBin`; all disk/journal access uses in-memory fakes.
No real Recycle Bin, user config or C# application data was accessed.

## Validation

Final source and all four new tests passed these gates, with `CARGO_TARGET_DIR=<worktree>/target`,
`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`:

- `cargo fmt -p pr-core --check` ? pass.
- `cargo clippy -p pr-core --all-targets --locked -- -D warnings` ? pass, zero warnings.
- `cargo test -p pr-core --locked --test file_actions_undo --test file_actions_undo_golden --test undo_mutation_killers --test undo_mutation_killers_2 --test w3_undo_stat_parity -- --include-ignored --test-threads=1` ? 131 passed, 0 failed, 0 ignored (120 + 1 + 1 + 5 + 4). The golden binary covers 100 oracle scenarios within its one test.
- `powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev/stage-native.ps1` ? staged and verified both pinned DLLs in this worktree. The initial attempt needed both read-only Git ownership exceptions; command-local `GIT_CONFIG_*` entries fixed this without global config edits. These undo binaries still use fakes, not native bin integration.
- `python -I tools/dev/doc-links.py --quiet` ? 0 errors, 171 PROPOSED (not errors).
- `git diff --check` ? pass.

No timing-dependent tests were added, so a 20x timing rerun was unnecessary.

## Mutation evidence

Cargo-mutants 27.1.0, one process at a time, own `<worktree>/target-mut`,
`--baseline skip --timeout 90 -j 1`, output `<worktree>/mut-out-undostat`.
No normal cargo test was run in `target-mut`. The run completed inside the 20-minute budget:

```text
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1
  --file "**/file_actions/undo.rs"
  -F "UndoService::(stat|undo_single_recycle|try_undo_group_recycle)"
  --output mut-out-undostat
  --cargo-arg=--test=file_actions_undo
  --cargo-arg=--test=file_actions_undo_golden
  --cargo-arg=--test=undo_mutation_killers
  --cargo-arg=--test=undo_mutation_killers_2
  --cargo-arg=--test=w3_undo_stat_parity
```

`--list --json` inventories: 199 current file mutants; 42 selected, matching
`Found 42 mutants to test`. This version also includes struct-field mutants from
other functions despite the function regex; these are separated below.

| Scope | Tested / selected | Caught | Missed | Unviable | Timeouts |
|---|---:|---:|---:|---:|---:|
| Changed functions: stat, undo_single_recycle, try_undo_group_recycle | 20 / 20 | 17 | 0 | 3 | 0 |
| Additional struct-field mutants selected by the tool | 22 / 22 | 22 | 0 | 0 | 0 |
| Unique combined run | 42 / 42 (of 199 file total) | 39 | 0 | 3 | 0 |
| Helper rerun against new binary alone (duplicate identities) | 2 / 2 | 1 | 0 | 1 | 0 |

The three unviable mutants require missing `Default` implementations: stat at
`:577` -> `Ok(Some(Default::default()))`, single Recycle at `:777` ->
`Default::default()`, group Recycle at `:1117` -> `Ok(Default::default())`.
They did not compile; they are not caught-test claims. No MISSED mutant required
an equivalent explanation or another killer.

The helper-only rerun used the same mutation options plus `-F "UndoService::stat"`,
`--in-diff mut-out-undostat/production.diff`, output `mut-out-undostat-stat-alone`,
and **only** `--cargo-arg=--test=w3_undo_stat_parity`. Its `--list` and `Found` both
reported 2. `stat -> Ok(None)` was caught by the new binary alone. No `--lib`
mutation pass was needed: `undo.rs` has no cfg(test) unit tests.

Cargo-mutants does not generate the exact method-call regressions being repaired.
Three additional manual mutants independently replaced each fallible call with
legacy `get_file_stat` (which folds the injected Err into None):

| Current site | Manual operator | New binary alone |
|---|---|---|
| undo.rs:810, single post-restore | `self.stat(source)` -> `Ok(get_file_stat(source))` | CAUGHT |
| undo.rs:1125, group pre-restore | `self.stat(source)?` -> `get_file_stat(source)` | CAUGHT |
| undo.rs:1190, group post-restore | `self.stat(source)?` -> `get_file_stat(source)` | CAUGHT |

Each ran `cargo test -p pr-core --locked --test w3_undo_stat_parity` alone and
failed test assertions (not compilation). Production bytes were restored in
`finally`, then all final gates above ran successfully. These three manual
mutants are separate from the 199 generated identities and are not added to that total.

Local logs and inventories (ignored build evidence, retained for the orchestrator):
`mut-out-undostat/mutants.out/outcomes.json`, `mut-out-undostat/undostat-mutants-all.json`,
`mut-out-undostat/undostat-mutants-selected.json`, `mut-out-undostat/undostat-mutation-command.json`,
`mut-out-undostat/manual-seam/results.json` and its three logs,
`mut-out-undostat/*-final.log`, `mut-out-undostat-stat-alone/mutants.out/outcomes.json`.

**Not rerun:** 157 of 199 current file mutants, outside the selected set. This is
not a replacement for the historical complete W3-05 run or a claim that all 189
identities left unverified by CL-UNDO have now been reverified. The main mutation
run copied the three site tests; the fourth bin-error precedence test was added
later and is included in the helper-only rerun and all final normal gates.

## Remaining limits

The single-Recycle C# exception/API difference above remains open. The complete
199-mutant file is not being rerun: only the changed functions and the helper are
selected within a 20-minute budget. No native bin integration or attended tests
are needed to exercise these injected stat errors. `file_actions_undo` has no
ignored/native tests; `--include-ignored` does not turn its fakes into native proof.
No workspace-wide fmt, Clippy or test claim is made.
