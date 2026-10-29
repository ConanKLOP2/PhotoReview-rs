# W7 benchmark parity and focused mutation follow-up

Date: 2026-10-29. Worktree: `codex-w7bench`, branch `feat/codex-w7bench`, starting HEAD
`1b8abe6372e78c719cd90b62cf30be4892f3b250`. No commit or push. No new dependency.

## Source and verdict recheck

Read `AGENTS.md` (especially mutation/parallel-agent rules), `docs/LESSONS.md`, and contracts
W7-02a / W7-03b / W7-03c / W7-03d by ranges. The verdict text available here is in the four
[PARITY rows and Wave 7 open items](../PARITY.md). Original verifier logs identifying the historical
eight executor and seven workload survivors were not present in the checked local state/review files;
this follow-up does not silently close those historical identities.

C# was read only with `git -c safe.directory=C:/MyProjects/PhotoReview -C C:/MyProjects/PhotoReview show 7eaee2a2:<path>`.
The command-local ownership exception does not change Git configuration. Contracts name the older
`d92d6aae` baseline; the recheck below uses the owner's requested `7eaee2a2` source.

| Row | C# source at `7eaee2a2` and relevant ranges | Result / remaining boundary |
|---|---|---|
| X-bench-slowlink-fs | `src/PhotoReview.Benchmarking/SlowLinkFileSystem.cs`, 16-91 decorator, 98-151 limiter | Whole-ms sleep truncation and 1-ms floor already ported. Lazy enumeration versus collected vectors, monotonic versus wall-clock ticks, and negative/null constructor inputs with no Rust type representation remain documented differences. No production change. |
| X-bench-executor | `src/PhotoReview.Benchmarking/BenchmarkImageExecutor.cs`, constructor 30-53, decode 65-81, teardown 105-165, logging 175-193, window 202-239 | Confirmed real FAIL gap: C# uses `Directory.Exists`, Rust used `Path::exists` before removing scratch. Fixed test-first to `is_dir`. Selected counter/window/logging/options checks have no surviving behavioral mutant after follow-up. Historical Display/Debug/DefaultFileLog scope remains unverified. |
| X-bench-workload-runner | `src/PhotoReview.Benchmarking/BenchmarkWorkloadRunner.cs`, 28-46 stamping/counts, 49-112 preparation/parallel decoding, 115-207 file actions, 210-243 seed/selection | Count, ordering/wrapping and action mapping agree with source/contract in the selected scope. New tests close registry lookup coverage gaps. Lowest-index parallel error and empty-list InvalidArgument remain D67; historical zero-dimension survivor identities were not rerun. No production change. |
| X-bench-cli-guard | `tools/PhotoReview.Benchmark.Cli/ToolPathGuard.cs`, 16-65 path/output/cache rules, 81-93 report replacement | Selected path and ownership rules agree. Synchronous report writing, probe plus rename replacement, and protection of both app-data identities remain documented. Existing UNC test fails under sandbox permissions; native replacement behavior is not proved by this follow-up. No production change. |

All four rows stay **Partial**. The scratch fix alone is not a clean verdict for every executor behavior.

## Production regression and new tests

Only production change: [image_executor.rs](../../crates/pr-bench/src/image_executor.rs), scratch cleanup
guard and its doc comment, `exists()` to `is_dir()`.

The new [w7_mutation_killers.rs](../../crates/pr-bench/tests/w7_mutation_killers.rs) has 11 tests.
`teardown_scratch_leaf_file_is_preserved_without_error` failed before the fix with an unexpected
`Benchmark scratch directory could not be removed: ... The directory name is invalid. (os error 267)`
recorded in `RecordingLog`; after the fix it preserves the file bytes and records no error.
Evidence: `w7-red.log`, `w7-green.log`. No assertion was weakened.

Other tests cover fractional-ms truncation and leftover tokens; workload counts and wrapping index order;
exact/interleaved action IDs; path overlap/output safety; cache marker ownership; RAM hits versus preload hits;
inclusive window boundaries and `i32::MIN`; logging counter sampling and external same-value writes;
full-folder scheduler options; case-insensitive registry lookup and unknown IDs. Test comments name mutant
source locations/operators. All new I/O is in `TempRoot`, using fake decoders/clocks or `RecordingLog`.
Action mapping tests do not invoke the Recycle Bin. No new sleeps or polling.

## Mutation method and exact scope

Normal commands: `CARGO_TARGET_DIR=<worktree>/target`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`,
`CARGO_INCREMENTAL=0`, builds/tests `-j 2`. Mutation commands: separate
`CARGO_TARGET_DIR=<worktree>/target-mut`, same profile/incremental settings, one process at a time, `-j 1`.
No normal test command used `target-mut`.

Command template (PowerShell resolves `cargo` from `%USERPROFILE%/.cargo/bin/cargo.exe`):

```text
cargo mutants -p pr-bench --gitignore true --baseline skip --timeout 90 -j 1
  --file "**/<file>.rs" -F "<filter>"
  --cargo-arg=--test=<binary> [--cargo-arg=--lib]
  --output <worktree>/mut-out-<group>
```

| Group / filter | Test binary (plus lib when noted) | Tested / selected | Caught | MISSED | Unviable | Timeouts | Artifact directory |
|---|---|---:|---:|---:|---:|---:|---|
| slow_link_fs: `consume_chunk` initial | `slow_link_fs` | 10/10 | 9 | 0 | 0 | 1 | `mut-out-slowlink-v2` |
| image_executor: `outside_window\|decode_async\|apply_logging`, `-E build_preload` | `w7_mutation_killers` + lib | 14/14 | 11 | 1 | 2 | 0 | `mut-out-executor` |
| workload_runner: `images_per_sample\|select_parallel_indices\|file_action_op` | `w7_mutation_killers` + lib | 20/20 | 15 | 4 | 1 | 0 | `mut-out-workload` |
| tool_path_guard: `is_same_or_under\|overlaps\|ensure_output_directory\|ensure_output_file` | `w7_mutation_killers` + lib | 7/7 | 7 | 0 | 0 | 0 | `mut-out-guard` |
| tool_path_guard: `ensure_cache_directory` | `w7_mutation_killers` + lib | 4/4 | 4 | 0 | 0 | 0 | `mut-out-guard-cache` |
| image_executor MISSED: `delete.field.options` | **new binary alone** | 1/1 | 1 | 0 | 0 | 0 | `mut-out-executor-killed` |
| workload_runner MISSED: `registry_images_per_sample`, first replay | **new binary alone** | 4/4 attempts | 0 | 0 | 4* | 0 | `mut-out-workload-killed` |
| workload_runner MISSED: `registry_images_per_sample`, successful replay | **new binary alone** | 4/4 | 4 | 0 | 0 | 0 | `mut-out-workload-killed-v2` |
| slow_link_fs complete selected replay: `consume_chunk` | **new binary alone** | 10/10 | 10 | 0 | 0 | 0 | `mut-out-slowlink-killers` |

The executor listing included the `PreloadSchedulerDeps.options` field deletion despite the function exclusion;
it is included in the 14 identities and was tested and killed, rather than omitted from the tally.
The workload filter includes `registry_images_per_sample`; all four constants were real MISSED coverage gaps.

`*` The first registry replay suffered compiler memory-allocation failure (`memory allocation of 1179664 bytes failed`,
rustc exit `0xc0000409`). These four raw unviable outcomes are infrastructure failures, not mutant classifications
or kill evidence. The isolated replay after the other groups ended caught all four.

Unique selected identities after successful replays: **55/55 tested, 52 caught, 0 outstanding MISSED,
3 compile-unviable, 0 outstanding timeouts**. Initial outcomes are preserved above: five MISSED and one
90-second timeout were not hidden. The three intrinsic unviables replace `LoggingScope`, `PreviewHandle`, or
`FileActionOp` with `Default::default()` where those types have no Default implementation. No selected survivor
was declared equivalent. No new production defect was exposed by these mutants; they exposed test coverage gaps.

Full four-file candidate inventory, without function filters, is saved in `w7-all-candidates.log`:
executor 87, workload 127, slow-link 91, guard 57 = **362** candidates. This run selected **55**;
**307 were not run**. This is not a full-file or whole-crate mutation verdict. In particular, Random internals,
other selection/preparation/parallel-error branches, historical zero-dimension guards, limiter refill/consume,
decorator forwarding/metadata delay, Display/Debug/default logging, teardown/native and report replacement
branches outside these filters were not mutated here.

Each completed group took about five minutes or less, below the requested 15-20 minute per-file budget even with
the focused replays. An earlier `mut-out-slowlink` attempt was stopped during source copy with zero mutant
outcomes: temp had been placed under the source tree. Subsequent source copies use `TEMP`/`TMP` under
`target/mutants-tmp`, which copying excludes. Temporary `.gitignore` exclusions for `target-mut`, `mut-out-*`
and task logs were restored byte-for-byte after mutation. The aborted copy was preserved under
`target/w7-aborted-mutation-copy` so doc-links does not recursively index it. Raw local artifacts are not intended for Git staging.

## Validation and limits

| Command | Result / last meaningful output |
|---|---|
| `cargo fmt -p pr-bench --check` | exit 0, no output |
| `cargo clippy -p pr-bench --all-targets --locked -j 2 -- -D warnings` | exit 0; final confirmation `Finished dev profile ... in 1.78s` (PowerShell wraps native stderr as NativeCommandError text in the log; no Rust warning/error) |
| `cargo test -p pr-bench --locked -j 2 --lib --test w7_mutation_killers --test image_executor --test workload_runner --test slow_link_fs` | 124 passed, 0 failed, 1 ignored: lib 14, executor 28, slow-link 37, killers 11, workload 34 + 1 ignored. Last binary: `test result: ok. 34 passed; 0 failed; 1 ignored` |
| Existing `cli_tool_path_guard` binary (initial focused run) | **12 passed, 1 failed, 4 ignored**. `missing_share_is_an_io_exception_like_dotnet` expected IOException for a nonexistent localhost UNC share; sandbox returned UnauthorizedAccessException. Assertion unchanged; not used as a mutation gate. |
| `python -I tools/dev/doc-links.py --quiet` | exit 0; `doc-links: files=457 errors=0 proposed=170 archive_unchecked=14 csharp_refs=505 runtime_paths=13 placeholders=105` |

Normal-test logs: `w7-final-tests.log`, `w7-killers.log`, `w7-clippy-final.log`.
Initial mixed-run failure: `w7-focused.log`. Mutation counts come from each directory's
`mutants.out/outcomes.json`; `missed.txt`, `timeout.txt`, and per-mutant logs preserve identities and diagnostics.

NOT RUN: workspace-wide fmt/clippy/tests; ignored native tests; GUI/WIC/FFI measurements; real benchmark profiles
against user photos; C# execution/oracle regeneration; real NAS/slow-link timing; native MoveFileEx/reparse-point
equivalence; historical survivor identities outside the selected scope. No app-data files or real Recycle Bin
were used, no C# files edited, and `task_on_progress.md` was not edited.
