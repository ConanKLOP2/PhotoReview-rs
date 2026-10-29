# W2 settings/config verification and selected mutation

Date: 2026-10-29. Worktree: `codex-w2cfg`, branch `feat/codex-w2cfg`, starting
HEAD `1b8abe6372e78c719cd90b62cf30be4892f3b250`. No commit or push is part of this task.

## Scope and oracle

Owned rows: S-64, S-65, INV-11, D-02, D-10 (store only), C-28
(W2-03 text/store and W8-17d boundary), X-config-import.
Production files inspected: `crates/pr-core/src/settings/store.rs` and
`crates/pr-core/src/settings/normalizer.rs`, with their settings dependencies.
The C# repository was read only through `git show 7eaee2a2:<path>` (with a
command-local `safe.directory` override for the sandbox identity).
Oracle sources are `src/PhotoReview.Core/Settings/SettingsStore.cs` and
`tests/PhotoReview.Core.Tests/Settings/SettingsStoreLoadRepairPersistenceTests.cs`.
Existing golden tests assert their C# baseline is `7eaee2a2`.

S-64's claim that repair tests await W2-02 is stale. All 15 C# load/persistence
tests in that test class have active Rust counterparts in `settings_store.rs`
(lines 1299–1559 at the starting HEAD). They cover the second load reporting
no repairs, failed writes retaining repairs in memory, throwing Changed handlers,
exactly one write/event, newer-build and explicit-path protection, original-byte
backups, one salvage backup, and backup failure. None is ignored.
The four C# `Build_*` repair-text cases are exercised by the 22-case
`settings-repair-text` golden in `settings_normalizer`. No repair test needed
porting or un-ignoring.

## Functional baseline

Discovery: filenames matching `config*`, `settings*`, `normalizer*` under
`crates/pr-core/tests`, then content searches for `SettingsStore`, `normalizer`
and the settings golden names. Six relevant binaries were run together:

| Binary | Passed | Failed | Ignored |
|---|---:|---:|---:|
| settings_app_settings | 17 | 0 | 0 |
| settings_context_menu | 27 | 0 | 0 |
| settings_normalizer | 31 | 0 | 0 |
| settings_perf_options | 4 | 0 | 0 |
| settings_shortcuts | 16 | 0 | 1 |
| settings_store | 119 | 0 | 0 |
| Total | 214 | 0 | 1 |

The ignored test is `serialize_review_action_uses_pascal_case_names`, with an
existing W1-02 serde dependency reason; it is not a store repair test and was
not enabled by this task. Golden case counts are normalize 294, validate 84,
repair-text 22, parse-text 66 and store 43 (cases are inside the binary counts).

Normal commands use `CARGO_TARGET_DIR=<worktree>/target`,
`CARGO_PROFILE_DEV_DEBUG=line-tables-only`, `CARGO_INCREMENTAL=0`, and `-j 2`:

```text
cargo test -p pr-core --locked -j 2 --test settings_app_settings --test settings_context_menu --test settings_normalizer --test settings_perf_options --test settings_shortcuts --test settings_store -- --test-threads=2
```

## Mutation method

`cargo-mutants 27.1.0`; exactly one owned mutation process at a time.
Mutation uses a separate `CARGO_TARGET_DIR=<worktree>/target-mut`, never for
normal tests. Scratch directories are inside an ignored `.codex-*` directory
in this worktree. `--gitignore true` and temporary scratch exclusions prevent
copying build/output directories into the mutation source copy.
Two initial copy-stage attempts were interrupted before testing any mutant;
they are excluded from all verdict counts. Only this task's runner sessions
were interrupted. No other process was stopped.

Each run uses:

```text
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 --gitignore true --file "**/settings/<file>.rs" -F "<selection>" --cargo-arg=--test=<binary> --output <worktree>/mut-out-<group>
```

Neither owned source file contains `#[cfg(test)]` tests requiring `--lib`.
This is selected pure-logic mutation, not a full-file or workspace verdict.

The independently run `pr-app::startup_settings_load` binary also passed all
7 tests, 0 failed/ignored, with the same normal-build environment and `-j 2`.
This covers W8-17d settings/language sequencing and repair notice text; it is
not a desktop dialog run.

## Completed normalizer mutation

| Group | Selection (`-F`) | Binary | Tested / selected | Caught | Missed | Unviable | Timeout |
|---|---|---|---:|---:|---:|---:|---:|
| normalizer | `clamp_i32` or `reset_if_none` or `disable_conflicting_optional_shortcuts` or `build_repair_text` or `SettingsParseResult::repairs` | settings_normalizer | 16 / 16 | 16 | 0 | 0 | 0 |
| normalizer-rules | `in.normalize$` or `replace.normalize.with` | settings_normalizer | 33 / 33 | 33 | 0 | 0 | 0 |

The actual regex uses `|` alternation (the word "or" above avoids table
delimiter ambiguity). Completed summaries: `16 mutants tested in 5m: 16 caught`
and `33 mutants tested in 9m: 33 caught`. No new killer was needed for these
49 mutants. File inventory: 67; tested 49; not run 18. Exclusions are
`SettingsValidator::validate_shortcuts` (14), its Debug implementation (1),
and the three whole-return replacement mutants in `normalize` (the second
regex selects its branch operators). This is not full normalizer mutation.

Raw results are under `mut-out-normalizer/mutants.out` and
`mut-out-normalizer-rules/mutants.out` in this worktree. Each contains the exact
selected inventory (`mutants.json`), outcomes, caught/missed/unviable/timeout
lists, per-mutant logs and diffs. These build artifacts must not be staged.

## Store continuation and standalone killers

The first store run selected 42/150 file mutants with
`SettingsStore::(parse_text|try_salvage|persist|load_first_run_with_import|unique_backup_path|migrate_core)`
and binary `settings_store`. Its last durable outcome was **24 completed:
20 caught, 1 missed, 3 unviable, 0 timeouts**. The previous session ended
while the 25th mutant was building; there is no successful terminal summary.
On resumption its output lock was available, so that run was not restarted or
overwritten. Its 18 uncompleted mutants include that interrupted build.

Of the three unviable outcomes, `store.rs:443:9` replaces `parse_text` with
`Ok(Default::default())`, which does not compile because `SettingsParseResult`
has no Default implementation. The other two (`544:73` delete `!`, `558:22`
guard -> true) failed during environmental build errors (`can't find crate
for std` / LLVM out of memory); these are not accepted as clean mutation
evidence and are selected again in the continuation.

The continuation uses only `w2_config_mutation_killers` with
`-F ':776:|:778:|migrate_core|load_first_run_with_import|:467:(16|45):'`
(25 selected). It adds `CARGO_BUILD_JOBS=2` and `--build-timeout 90` to bound
builds as well as test execution. The six new tests use TempRoot/fakes only:

- Salvage plus normalize reports repairs in order, without repeating the
  salvaged `ExternalEditors` name when reconciliation also repairs it.
- Zero memory reserve is valid; negative reserve is reset and a second
  normalize reports no further repair.
- Version 2/3/4/99 boundaries preserve declared language, migrate silently
  only before version 4, and report later editor repairs.
- Read-only import persists repairs once and an existing Rust config wins
  over a different valid C# source on the next call.
- JSON null is a skipped import, with defaults and no imported log entry.
- Finite fake backup collisions (0, 1, 2, 98 and 99 occupied names) preserve
  every existing backup; exhaustion keeps the original config and prevents
  a later save from overwriting it.

One initial standalone fixture let `store.rs:544:70` (`||` -> `&&` in import)
survive: its second C# source was garbage, so both paths eventually loaded
the Rust config. The fixture was strengthened to a valid source with different
language/zoom values, without changing production code. The original
`settings_store` binary had already caught this mutant; the corrected
standalone test is independently rechecked below.

## Equivalent survivor rationale

`store.rs:467:42`, `&&` -> `||` in `parse_text`, is equivalent for this
pipeline. `reset_settings` starts as a clone of `salvaged`. For a normalized
name already in `salvaged`, both negated membership tests are false. For a
name absent from `salvaged`, both are true because `normalize` emits each
repair name at most once (each field is checked once; `Shortcuts` and `Actions`
are aggregated). Thus AND and OR agree for every reachable iteration. The
new salvage/normalize overlap test also pins the observable single-entry
repair result. This is an explained equivalent, not a killed mutant.

`store.rs:834:36`, `<` -> `<=` when stamping `CURRENT_CONFIG_VERSION`, is
also equivalent: the added branch only assigns version 4 to an existing
version 4, with no side effect. Lower/newer versions take the same branches.

## Final mutation results

| Run / binary | Completed / selected | Caught | Missed | Unviable | Timeout | Terminal result |
|---|---:|---:|---:|---:|---:|---|
| store / settings_store | 24 / 42 | 20 | 1 | 3 | 0 | Interrupted; no terminal summary |
| store-killers / w2_config_mutation_killers | 25 / 25 | 22 | 2 | 0 | 1 | `25 mutants tested in 7m: 2 missed, 22 caught, 1 timeouts` |
| store-reproof / w2_config_mutation_killers | 3 / 3 | 3 | 0 | 0 | 0 | `3 mutants tested in 46s: 3 caught` |
| normalizer-killers / w2_config_mutation_killers | 3 / 3 | 3 | 0 | 0 | 0 | `3 mutants tested in 50s: 3 caught` |

The store reproof selects `:544:70:` and `:778:15:`: import OR -> AND and
both backup increment replacements. The timeout (`+=` -> `*=`) repeatedly
multiplies the counter by 1. The new `BackupProbeGuard` fails immediately if
the fake sees the same corrupt-backup candidate twice; no delay or thread
polling is involved. The corrected fixture catches both the former import
survivor and the former timeout, using only the new binary. The third mutant
(`+=` -> `-=`) is caught again. Raw failures are preserved, not relabeled.
The normalizer standalone reproof selects `:83:38:` (reserve `<` -> `==`,
`>` and `<=`); all three are caught by the zero/negative boundary test.

The two environmental import build failures from the interrupted run were
both caught in `store-killers`. One fixture compile error (incorrect tuple
access on RecordingLog entries) was fixed before the successful six-test
baseline. No production source was changed and no real C# parity bug was
found.

Deduplicating by full mutant name and taking the final reproof outcome:

| Source file | File inventory | Distinct tested | Final caught | Final missed (equivalent) | Intrinsic unviable | Unresolved timeout | Not run |
|---|---:|---:|---:|---:|---:|---:|---:|
| normalizer.rs | 67 | 49 | 49 | 0 | 0 | 0 | 18 |
| store.rs | 150 | 42 | 39 | 2 | 1 | 0 | 108 |
| Total | 217 | 91 | 88 | 2 | 1 | 0 | 126 |

There were **104 completed mutant executions**, including repeated proofs;
these are **91 distinct mutants**, not 104 distinct mutants. The raw history
includes one timeout and two environmental build failures subsequently
resolved. Active mutation time was approximately 15 minutes per owned file;
the session interruption increased wall time without adding verdicts.
No selected mutant remains uncompleted after combining the continuation
with the interrupted run. This does not cover the 126 unselected mutants.

Raw continuation results are in `mut-out-store-killers/mutants.out`,
`mut-out-store-reproof/mutants.out` and
`mut-out-normalizer-killers/mutants.out`; the interrupted store results
remain in `mut-out-store/mutants.out`. The original normalizer directories
are listed above. All are local, untracked build evidence, not deliverables
to stage. Temporary copy exclusions in `.gitignore` were restored exactly.

## Row decisions

| Row | Result | Evidence and remaining limit |
|---|---|---|
| S-64 | Partial; functional pipeline PASS | 119 store tests, parse/store goldens, active repair persistence tests, 49 normalizer and 42 store selected mutants. Stale W2-02 wait removed. Load/apply/retry guards were not mutated; one intrinsic unviable parse replacement and two equivalents are explicitly retained. |
| S-65 | Partial; functional save PASS | Save round trips, failed-write preservation, canonicalization, durable atomic-write and Changed assertions pass; the persist replacement is caught. Newer-backup/pruning mutations were not selected. The stale version-3 note is corrected to current schema version 4 (W9-E1). |
| INV-11 | Partial; fake failure contracts PASS | Locked/corrupt config and later-save protection tests pass. Load and keep-corrupt failure-path guards were not selected for mutation. |
| D-02 | Done | All 7 mutants in unique_backup_path are caught after reproof; existing injected UTC stamp test passes. Six operator mutants are proved using the standalone killers (one repeated after the timeout); the whole-return replacement was caught by settings_store. Collision/exhaustion tests preserve every fake original. |
| D-10 (store) | PASS for parse/store; row remains Partial | 66 parse-text golden cases and new pipeline regressions pass. The import/export dialog is not exercised. |
| C-28 | Partial | Repair text/store goldens and 7 freshly rerun W8-17d startup tests pass; W8-17e information-dialog display and desktop integration were not run. |
| X-config-import | Done (R0002) | All 5 load_first_run_with_import mutants caught with the standalone binary after reproof. Existing Rust config wins, null/failure/default paths and read-only fake source preservation are covered. Path composition is covered by existing tests; its whole-return mutation was not selected. |

`parity.py set` updated the five unambiguous status rows. Its S-64 dry-run
reported two matches (the individual row and the combined W2-02 row at
line 119). C-28 has the same combined-row ambiguity, so only those two
individual rows were edited directly; the combined W2-02 row was untouched.

## Gates and commands not run

Final scoped test evidence: **227 passed, 0 failed, 1 ignored** across the
six original core binaries (214 pass), the new killers (6 pass), and
startup_settings_load (7 pass). Repeated baseline runs of the new binary
are not added to this count.

- `cargo fmt -p pr-core --check`: exit 0, no output.
- `cargo clippy -p pr-core --all-targets --locked -- -D warnings`:
  exit 0, last line `Finished dev profile [unoptimized + debuginfo] target(s) in 40.91s`.
- `cargo test -p pr-core --locked -j 2 --test w2_config_mutation_killers`:
  `test result: ok. 6 passed; 0 failed; 0 ignored`.
- `python -I tools/dev/doc-links.py --quiet`: exit 0; last line:
  `doc-links: files=457 errors=0 proposed=170 archive_unchecked=14 csharp_refs=505 runtime_paths=13 placeholders=105`.
- `git diff --check`: exit 0, no output. `.gitignore` has no remaining diff.

Command transcripts and initial inventories are retained under
`target/w2-evidence/`; mutation inventories/outcomes remain in the six
`mut-out-*` directories above. Nothing was staged.

Not run: workspace tests/Clippy, release/performance/RAW/corpus checks,
native filesystem or desktop tests, the ignored shortcut serializer test,
126 unselected mutants, and mutation of app_settings.rs, JSON parser,
shortcut/context-menu/editor modules or startup composition glue. No
dependency added, no C# repo edits, no real Recycle Bin/user-data access,
no task_on_progress.md edit, commit, push, PR or staging operation.
