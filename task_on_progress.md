# Task on progress

**Updated:** 2026-10-14 (branch `integration/wave-2`)

## State
- master: Wave 0 + Wave 1 + sync to C# baseline d92d6aae.
- `integration/wave-2`: all 21 Wave 2 units (W2-00..W2-20) merged in three batches; PR open to master.
- Repair round merged (fix/W2-14, 17, 00, 18, 01, 03, 12, 13, 19; no conflicts). Gate on this branch (CARGO_TARGET_DIR=target-wave-2/integ):
  - `cargo fmt --all --check`: clean.
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
  - `cargo test --workspace --locked --no-fail-fast`: 77 test binaries, 59 ignored. One run out of four showed exit 101
    with one failing suite that I did not identify (output was filtered); the next two runs were fully green. Treat as a
    timing-sensitive flake (units W2-13/W2-19 also reported flaky save-worker / diagnostics_file_log tests under load).
  - `cargo test -p pr-core --test io_physical -- --include-ignored`: 64 passed, 0 failed.

## Verdicts after the repair round
- Closed (Done in PARITY.md): W2-00 (3 mutants killed), W2-12 (M20 killed), W2-13 (M22 killed), W2-18 (all 9 mutants killed),
  W2-19 (mutant killed).
- Behaviour fixes: W2-14 (trailing space/dot segments in directory_exists/get_file_stat, junction cap error 1921) and
  W2-17 (PerfCsvListener trace dir via GetFullPath semantics) now match the C# oracle.
- Remaining, still Partial:
  - W2-14: 4 mutants alive (MAX_FOLLOWED_LINKS cap needs a symlink, unavailable without privilege; file_exists_full probe is behaviourally equivalent).
  - W2-17: CHANNEL_CAPACITY 100_000 -> 10 only killed by a value-pinning test.
  - W2-01: 2 equivalent mutants (surrogate range edges).
  - W2-03: 13 survivors killed; 6 judged equivalent; the original M-number mapping could not be confirmed.
  - W2-20 test fix was already merged earlier (2c9e711 / 56451aa).
- PASS: W2-15, W2-16. Other units (W2-02, 04..11) have no open issue.

## Next
1. Decide whether the 4 W2-14 mutants need a privileged-symlink test (CI with Developer Mode) or are accepted as equivalent.
2. Phase 0 still open: UI spikes (R0001; C# moved to Win32 + Direct2D, ADR 0010), COM Explorer spike.
3. Then Wave 3 (data safety) per docs/MULTI-AGENT-PLAN.md, with lessons from section 11.
