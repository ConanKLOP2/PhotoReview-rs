# Task on progress

**Updated:** 2026-10-16 (branch `wave4/contracts`)

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

## Wave 4 contracts (branch `wave4/contracts`)
- Wired: 35 contracts (`contracts/W4-01a..W4-16b.md`, W4-14 split into a/b/c, W4-15 into a/b), 37 stub files in
  `pr-imaging` (`cache_key`, `decode_types`, `decoder_traits`, `orientation`, `embedded_thumbnail`, `tiff`, `exif_parser`,
  `exif_summary`, `exif_query`, `adaptive_preview`, `preload_order`, `navigation_pace`, `preload_backoff`, `ram_budget`,
  `memory_headroom`, `pixels`, `pixel_memory`, `factory`, `fallback`, `webp_heic`, `cache/*` x5, `service/*` x5,
  `preload/*` x7) and 6 in `pr-platform-win::wic`; 35 oracle files plus 70 dispatch lines; 70 inputs; 70 goldens; 2 fixture
  folders (235 KB). Cargo: `windows` 0.62 and `windows-core` (workspace, `pr-platform-win` only), `pr-imaging` dev-dependencies
  `pr-test-support` and `serde_json`, `pr-platform-win` depends on `pr-imaging`.
- Run (this machine, C# `d92d6aae`): `tools/oracle.ps1 -Commit d92d6aae -Command <name>` once per command, 70 commands, all
  built and ran. Every golden and fixture produced a second time and compared byte for byte: identical (after fixing two
  nondeterministic outputs, D40). No pre-existing golden changed.
- Run: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean;
  `cargo test --workspace --locked`: 1407 passed, 0 failed (no new tests: stubs are `todo!()`).
- Wiring fixes found by compiling the drafts together: W4-01c used the trait name `DecodedImage` and elided the
  `PixelLease` lifetime (D29); `ExifRational` was declared twice (D30); 10 placeholder blocks and 3 stand-in modules replaced
  by real types (D31); 6 public types lacked `Debug`; `ram_budget` stub test used a placeholder `image_count`.
- NOT run: `cargo-mutants`; Rust tests against the new goldens (none written yet); any native WIC test (nothing
  implemented); the oracle on another machine or Windows build (pixel-hash goldens of real WIC output may differ, D39).
  The oracle for `wic-direct-*` and `embedded-thumbnail-*` needs WPF (`UseWPF`) and so runs only on Windows.
- Open for the lead: amendments A1..A7 and the D32 seam merge in contracts/README.md; proposals P-24..P-27.
- Next: Haiku/Sonnet port tests per unit (red), then porters in the merge order of contracts/README.md (Wave 4 roster).
- Flake seen once in the gate: `diagnostics_file_log::failed_rotation_keeps_the_old_log_content` failed in one full run (known timing-sensitive suite, see Wave 2 notes) and passed twice alone and in the next full run (1407 passed, 0 failed).
