# task_on_progress archive: Wave 7 (benchmarking)

Moved verbatim from `task_on_progress.md` (DOC-PROGRESS-TRIM, machine C). Wave 7 readiness inventory, contracts PR and integration, 2026-10-18.
The text below is historical: states, paths and next steps are those of the date in each heading, not current.
Current state: `../../task_on_progress.md`.

## Wave 7 readiness inventory (2026-10-18, read-only)
- Independent read-only audit found W7-01..03 have no contracts or parity rows and no implementations beyond the empty `pr-bench` crate shell. C# sources are pinned by `docs/SYNC-LEDGER.md` at `d92d6aae`: models/profiles/metrics; slow-link filesystem/source-reader and benchmark Recycle Bin; engine/executor/workload runner and CLI output parity.
- After Wave 6 has passed gates and merged, add W7 contracts first. W7-01 provides shared types; W7-02 depends on filesystem/source abstractions and must use fake/test-owned files only; W7-03 depends on W7-01/02 plus imaging/preload/performance APIs and needs pinned JSON/summary/console output behavior. No Wave 7 files were changed while Wave 6 is unmerged.

## Wave 7 contracts PR (2026-10-18, branch `wave7/contracts`, based on master `9ffb51b`)
- Wired: 8 contracts (`contracts/W7-01a..W7-03d.md`), stubs in `crates/pr-bench/src/` (`models`, `profiles`, `metrics`, `slow_link_fs`, `slow_link_reader`, `bench_recycle_bin`, `engine`, `image_executor`, `workload_runner`, `cli/`, `main.rs`), 8 oracle files (`tools/oracle-cs/Commands/W7-*.cs`), 20 goldens (`tests/golden/`, inputs in `tests/golden-inputs/`), Wave 7 roster and decisions D60..D70 in `contracts/README.md`, PARITY rows `X-bench-*`, proposals P-35..P-37.
- Oracle: `tools/oracle.ps1 -Commit d92d6aae` now builds `PhotoReview.Benchmarking` from the baseline archive; `OracleCs.csproj` references it and compiles the two baseline CLI classes via `-p:CSharpSrcDir`. All 20 commands ran twice; the two runs are byte-identical and no pre-existing golden changed. Spot checks: `ticks-long-max-clamped` = `922337203685477.0`, `ticks-minus-1` = `-0.0`, `delete-read-only-file-throws-and-keeps-it` = UnauthorizedAccessException.
- Verified on the final head: `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked -- -D warnings` clean, `cargo test --workspace --locked` exit 0 (5162 passed, 0 failed, 426 ignored).
- NOT run: the `cli-process` golden (needs the baseline WPF-bound Benchmark.Cli exe; input dropped, see D70); no Rust test exercises the stubs (all bodies are `todo!()`); `cargo-mutants` (nothing ported yet); the native/real-clock behaviour of `slow-link-fs` is only pinned as lower-bound booleans; CI result pending at PR time.
- Next: port W7-01a/01b/02a first (see the roster dependency order), then 02b, 03a, 03b, 03c, 03d; the lead writes `EngineBackend` (D69).

## Wave 7 integration (2026-10-18, branch `integration/wave-7`)
- Merged W7-01a, 01b, 02a, 02b, 03a, 03b, 03c, 03d (verdicts in `docs/PARITY.md`); one integration fix (`ed47436`, workload_runner test uses wired `BenchmarkProfile`).
- Full gate (fmt, clippy -D warnings, test --workspace --locked --no-fail-fast) is run before the PR; results are in the PR body.
- NOT run: `cli-process` golden; re-verification of `wu/W7-02a` fix `2d602dd`; cargo-mutants on the merged tree.
