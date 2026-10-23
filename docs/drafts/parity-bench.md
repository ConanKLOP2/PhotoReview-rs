# PARITY.md row proposals — W10 benchmark

Append proposals only; `docs/PARITY.md` was not edited.

| Row | Status | Rust | Test that will prove it | Notes |
|---|---|---|---|---|
| X-benchmark-compare | Not started / Contract | `pr_bench::compare` | `tests/bench_compare.rs`: `compare_*_expected_result` and `compare_repeats_*_expected_result` pure cases for schema, exact fields, diagnostic deltas, thresholds, paired repeats, and malformed/non-finite values | Requires typed summary parser and `CompareReport`; diagnostic metrics never fail the gate |
| X-benchmark-parity-harness | Not started / Contract | future `tools/bench-compare.ps1` plus `pr-bench --bench-compare` | `tests/bench_compare_cli.rs` process/fake-executable tests for sequential invocation, same args, output override, exit codes, and summary diff | Blocked until engine-backed backend replaces `UnwiredBackend` |
| X-attended-remeasure | Contract | Manual attended session | Filled `docs/adr/R0001-ui-framework.md` Measured table and Reading decision | Production result requires P0 measurement driver; spike-only subset is indicative and cannot decide W8-12j |
