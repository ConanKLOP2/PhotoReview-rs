# Task on progress

**Updated:** 2026-10-14 (branch `integration/wave-2`)

## State
- master: Wave 0 + Wave 1 + sync to C# baseline d92d6aae.
- `integration/wave-2`: all 21 Wave 2 units (W2-00..W2-20) merged in three batches; PR open to master.
- Gate as run on this branch (CARGO_TARGET_DIR=target-wave-2/integ):
  - `cargo fmt --all`: no changes.
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
  - `cargo test --workspace --locked --no-fail-fast`: 76 test binaries ok, 1 failed; 1375 passed, 1 failed, 54 ignored.
    The failure is `pr-core settings_context_menu::numeric_path_trailing_whitespace_rules` (W2-20): the test asserts
    `"3 "` does not parse, but .NET `Enum.TryParse` accepts trailing whitespace and the Rust code does too. The test is
    wrong, not the code.
  - 54 tests are `#[ignore]`d (42 of them `io_physical`, which per the W2-14 verdict pass with `--include-ignored`;
    that run was not repeated here).

## Verdicts (from the verifier reports)
- PASS: W2-15, W2-16.
- FAIL, merged with open issues (listed in PARITY.md as Partial):
  - W2-00: 3 surviving mutants (`directory_name_end` second trim after UNC root, `root_length` NT `\??` prefix, device UNC scan start).
  - W2-01: 3 surviving mutants (surrogate guard widening, Infinity sign in `json::to_f64`), no behaviour diff.
  - W2-03: 7 surviving mutants in settings store tests, no behaviour diff.
  - W2-12: mutant M20 (`nests_too_deep` escape handling untested).
  - W2-13: mutant M22 (`key_of` fallback for two uncanonicalisable folders).
  - W2-14: 3 behaviour diffs on trailing space/dot directory segments (`directory_exists`, `get_file_stat`) plus 2 surviving mutants.
  - W2-17: `PerfCsvListener::try_start/trace_dir_path` differs for trace dir segments ending in space or dot; many surviving mutants.
  - W2-18: 3 surviving mutants in `is_rate_limited` / `is_leap`.
  - W2-19: surviving mutant (`disposed` check in `flush_after_window`).
  - W2-20: its own test is wrong (see above).
- Other units (W2-02, 04..11) have no open issue recorded in the reports given to the integrator.

## Next
1. Fix the W2-20 test (trailing ASCII whitespace is accepted) so the workspace gate is green.
2. Repair round for the FAIL units above (tests for surviving mutants; fix the W2-14 and W2-17 trailing space/dot diffs).
3. Re-run the gate with `--include-ignored` for io_physical once W2-00 is final.
4. Phase 0 still open: UI spikes (R0001; C# moved to Win32 + Direct2D, ADR 0010), COM Explorer spike.
5. Then Wave 3 (data safety) per docs/MULTI-AGENT-PLAN.md, with lessons from section 11.
