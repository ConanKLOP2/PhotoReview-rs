# Task on progress

**Updated:** 2026-10-15 (branch `integration/wave-3`)

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

## Wave 3 contracts (branch `wave3/contracts`)
- Wired: 8 contracts (`contracts/W3-01..08.md`), 12 stub files (`file_actions/{journal,transaction,compaction,
  journal_rules,reconcile,startup_recovery,service,undo,recovery_retry,recovery_check,partial_cleanup}.rs`,
  `io/journal_compaction_files.rs`), 8 oracle files in `tools/oracle-cs/Commands/` plus 15 dispatch lines,
  15 inputs in `tests/golden-inputs/`, 15 goldens in `tests/golden/`.
- Run: `tools/oracle.ps1 -Commit d92d6aae` once per command (15 runs, all built and ran); every golden regenerated a
  second time and compared byte for byte: identical. No pre-existing golden changed.
- Run: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean;
  `cargo test --workspace --locked`: 1406 passed, 0 failed, 59 ignored (no new tests: stubs are `todo!()`).
- NOT run: cargo-mutants; Rust tests against the new goldens (none written yet). Open decisions D26 (a)-(f) in
  contracts/README.md are undecided; fixtures `operations-legacy.jsonl` and `journal-older-build/*.jsonl` are not synced.
- Next: lead decides D26, Haiku ports tests per unit (red), then Opus porters in D23/D24 order.

## Wave 3 lead foundation (branch `integration/wave-3`, CARGO_TARGET_DIR=target-wave-3/integ)
- Done (one commit each): P-11 `AppendStream` + `open_append -> Box<dyn AppendStream>`; P-12 `FileSystem::try_get_file_stat`
  (physical, counting, fake overrides; `io::physical::is_stat_missing`); P-13 `RecycleBin::try_restore -> io::Result<bool>`
  (+ `FakeRecycleBin` send/delete/restore hooks); P-14 `pr_test_support::{HookFileSystem, FaultInjectionFileSystem}`;
  P-15 fixtures synced at d92d6aae (sync script also maps the two W2-03 config fixtures, which a sync run used to delete).
- Run: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean;
  `cargo test --workspace --locked`: 1442 passed, 0 failed, 61 ignored; `--ignored` on `io_physical_seams` (2) and
  `io_physical` (47): all passed.
- NOT done: `CrashPointFileSystem`, `FaultableFileSystem`, `FaultInjectionMatrix`/`Rig`/`SecondOrder`/`FaultInvariants` (W3-09,
  P-16); cargo-mutants on the new seams; no PR opened from this step.
- Next: Haiku red-test pass per unit against the landed seams, then porters in D23/D24 order.

## Wave 3 integration (branch `integration/wave-3`, 2026-10-16)
- Merged: W3-02, W3-07, W3-08 (i1); W3-01 (verdict FAIL), W3-03 (i2); W3-04 (i3); W3-05 (verdict FAIL), W3-06 (i4); W3-09 tests (02640a5).
  Only conflicts were in `docs/PARITY.md`; resolved by keeping the Done/Ported rows. Not pushed before this step.
- Run in the integration steps (fmt, clippy `-D warnings`, `cargo test --workspace --locked --no-fail-fast` after each batch):
  all clean; one flake in `diagnostics_file_log::flush_drained_set_while_entry_queued_waits_until_written` (W2 test) under load, green on rerun.
- Open: W3-01 mutation testing not done (228/231 untested); W3-05 verifier mutation re-run unfinished (porter round 3: 198 mutants, 178 caught, 2 missed and killed);
  `#[ignore]`d tests (journal 19, compaction 15, reconcile 2, startup_recovery 6, INV-6 equivalence 5) not run with `--include-ignored`.
- C# moved past `d92d6aae` (external editors): NOT ported; P-17, patch WU W2-S1 after Wave 3.


## Wave 4 integration (branch `integration/wave-4`, 2026-10-16)
- Merged units: see docs/PARITY.md and docs/MULTI-AGENT-PLAN.md section 15. 11 units blocked, not merged.
- Merged origin/master into the branch (docs only, no conflicts).
- Gate (CARGO_TARGET_DIR=target-wave-4/integ): results recorded in the PR body; test failures are `todo!()` stubs of unmerged units.
- Not run: `--include-ignored` native tests, mutation runs beyond those in unit verdicts, nothing for blocked units.
