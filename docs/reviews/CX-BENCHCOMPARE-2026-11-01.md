Status: Complete - audit only; no code needed, compare and harness already on master.

# CX-BENCHCOMPARE evidence (2026-11-01)

Lane `feat/cx-benchcompare`. Brief: write `pr_bench::compare` and the harness CLI for rows `X-benchmark-compare` (W10-B1) and `X-benchmark-parity-harness` (W10-B1/B3) if still missing.

## Finding: already implemented on master

Checked against `origin/master` (71d9ea76) with `git log` and `git ls-tree`:

| Part | Where | PR |
|---|---|---|
| `pr_bench::compare` (`compare`, `compare_repeats`, `Thresholds`, `Verdict`, `CompareReport`) | `crates/pr-bench/src/compare.rs` (886 lines) | #325 |
| `--bench-compare` CLI mode | `crates/pr-bench/src/cli/bench_compare.rs` | #335 |
| PowerShell driver | `tools/bench-compare.ps1`, `tools/dev/test/bench-compare-selftest.ps1` | #335 |
| Pure tests | `tests/bench_compare.rs` (15 tests), all 10 names proposed in the contract plus repeats, dataset, diagnostic and p50/max-warning cases | #325 |
| Process tests | `tests/bench_compare_cli.rs` (5 tests): exit 0/1/2/3, threshold override, unwritable output | #335 |

No new code was written because nothing in the pure/fake-executable scope was missing.

## Run (this lane)

`cargo test -p pr-bench --test bench_compare --test bench_compare_cli -j 2`: 15 passed and 5 passed, 0 failed. No other build was run.

## Gaps seen (not fixed)

- Contract "Gate 0" (JSON key set/order, number-token grammar of non-timing fields, CRLF) is only partly covered: CRLF/BOM parsing is tested (`compare_crlf_and_bom_input_parses_equally`), but `compare.rs` has no explicit check of key order or .NET `R` token form (the parser only validates that a number token parses as f64). Candidate follow-up, needs a golden C# summary fixture.
- Golden fixtures listed in the contract (metrics-rich, failed, malformed batch) exist only as inline synthetic cases.
- Real timing needs the attended W10-B2 re-measure and a real Windows host in `pr-bench` (cycle pr-platform-win -> pr-app -> pr-bench); not in scope.

## PARITY rows (PARITY.md not edited)

- `X-benchmark-compare`: already `Partial` in the current worktree (the `Todo` text seen on the stale fetch was an older revision); accurate, no change. Not `Done`: Gate 0 byte-format check and attended measurement remain.
- `X-benchmark-parity-harness`: already `Partial` and accurate; no change.
