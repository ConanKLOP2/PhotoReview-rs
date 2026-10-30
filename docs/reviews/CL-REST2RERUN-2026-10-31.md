# CL-REST2RERUN: correct-flag rerun of the CL-REST2 groups (2026-10-31, lane `cl-rest2rerun`)

Status: evidence report. Branch `feat/cl-rest2rerun`. No production change. Redoes [CL-REST2](CL-REST2-2026-10-30.md) (its `secondary_dialogs.rs` "22 caught, 0 missed" was a void run, see [AUDIT-MUTATION-FLAG-2026-10-30](AUDIT-MUTATION-FLAG-2026-10-30.md) and [CL-STARTRERUN](CL-STARTRERUN-2026-10-30.md)).

Method: `cargo mutants -p <crate> --baseline skip --timeout 300 -j 1 --file ... --cargo-test-arg=--test --cargo-test-arg=<binary> ... --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`, `CARGO_TARGET_DIR=<worktree>/target-mut`, `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, lane-local data root, native DLLs staged by `stage-native.ps1 -Destination`. Preflight (unmutated, same arguments): `services_initial_viewport` `running 22 tests` ok; pr-shell lib + composition_texts + composition_adapters + composition_graph + w8_startup_mutation_killers `running 5/4/4/13/7 tests`, all ok, exit 0 (`composition_graph` includes its `#[ignore]` native test).

## Results

| Group | Binaries | Tested | Caught | Missed | Unviable | Timeout | CL-REST2 said |
|---|---|---:|---:|---:|---:|---:|---|
| pr-app `services/initial_viewport.rs` | `services_initial_viewport` | 41 | 41 | 0 | 0 | 0 | 41/41 (confirmed) |
| pr-shell `composition/adapters.rs` | lib, composition_texts, composition_adapters, composition_graph, w8_startup_mutation_killers | 34 | 22 | 7 | 5 | 0 | 29 caught, 0 missed (WRONG) |
| pr-app `presenters/benchmark.rs` `ProductionBenchmarkSession` | `w8_rest2_killers` | 5 | 2 | 1 | 2 | 0 | 3 (2 caught, 1 unviable) (confirmed) |
| pr-shell `composition/texts.rs` | not rerun | - | - | - | - | - | 5 of 161; superseded by CX-TEXTS (38/161, 37 caught, 1 unviable) |

Every viewport mutant log shows `running 22 tests` and `FAILED` (41 of 41, 0 logs without the line). Every adapters log shows tests running (4-13 per binary).

`adapters.rs` 7 MISSED (all real, with the CL-REST2 binary list): `Debug::fmt -> Ok(Default::default())` of `SharedDialogService` (43), `SharedClipboard` (100), `Win32SinkFactoryAdapter` (118), `LazyPrewarmPreviews` (153), `LazyDecoderFactory` (203), `LocalizedFormat` (247) and `LocalizedFormat::to_local_time -> Default::default()` (285). CL-REST2's "0 missed" was wrong. Closure: CX-TEXTS (#420, `w8_texts_killers.rs`: six Debug names, LocalizedFormat time conversion) already holds the killers and records adapters 34/34 with them. This lane independently wrote a 6-test file and reran just those 7 mutants with it added: 7 tested, 7 caught (`running 6 tests` in each log). The file was dropped from the PR because `w8_texts_killers` duplicates it.

`benchmark.rs` (the `-F ProductionBenchmarkSession` filter also matched two mutants outside the killers' scope): `dispose -> ()` caught, delete `!` caught, `scratch_directory` unviable, `run -> Ok(Default)` unviable; the `Debug::fmt` mutant (line 930) is MISSED by `w8_rest2_killers` alone because it is killed by `w8_presenter_killers2` (PARITY: re-run 2 caught), not a new gap.

## Hand check of the 2 `dispose` mutants (`w8_rest2_killers`, 2 tests)

Baseline: `running 2 tests`, both ok. Mutant 1, body of `dispose` emptied: both tests FAILED (panics at `w8_rest2_killers.rs:71` and `:91`, `finished in 40.01s` = the bounded poll expiring). Mutant 2, `if !self.disposed` -> `if self.disposed`: both FAILED at the same lines. Source restored with `git checkout`; the killers do kill both.

## Rows

No row raised. X-benchmark-presenter and U-initial-viewport: `rerun OK 2026-10-31`, stay Partial (human desktop run, monitor/DPI diversity). X-composition-texts-adapters stays Partial (already corrected by CX-TEXTS; this adds the adapters 7-survivor note). X-startup-composition-root: the CL-REST2 `secondary_dialogs.rs` sentence is marked INVALID; the 7 real survivors stay with X-dialogs-secondary-port (native observation gap, not repeated here). X-composition-factories: not rerun here, CX-TEXTS already has 7/7 (1 caught, 6 unviable). Not done: `texts.rs` beyond CX-TEXTS's 38 of 161.
