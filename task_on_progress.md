# Task on progress

**Updated:** 2026-10-16 (branch `wave5/contracts`; Wave 4 port is merged, PR #20)

## How to resume (read this first)
1. Read `AGENTS.md`, this file, `docs/PROPOSED-DECISIONS.md` (P-01..P-33: unconfirmed proposals, all acted on),
   `docs/MULTI-AGENT-PLAN.md`, `docs/PARITY.md` ("Wave 4 final status"), `contracts/README.md` ("Wave 5 roster", D45..D58), `tools/autopilot/README.md`.
2. Standing user instruction: run the whole port end to end, auto-fix + auto-merge (squash) every PR, never ask;
   log open questions as a `P-nn` row and act on the recommended option. Replies in Vietnamese.
3. The 15-minute loop was cancelled; restart it with `/loop 15m` + `tools/autopilot/loop-prompt.md`.
4. Permission classifier blocks some `gh pr merge`, `git branch -D`, `git push --delete`, publish commands in
   workflow agents; the user must add the rules from `tools/autopilot/README.md` to `.claude/settings.local.json`
   (agents cannot grant themselves permissions). The main session pushes and opens PRs when a workflow agent refuses.

## State (verified)
- **master** (`985ba99`): Wave 0, 1, 2, 3 merged (PR #8, #17), Wave 4 contracts (#16), Wave 4 plan, docs (#9-#15, #18),
  autopilot kit (#19). C# baseline for all goldens: `d92d6aae`.
- **PR #20 (merged into master as `b3bb2e7`)**: Wave 4 port (`integration/wave-4`, 36 sub-units). Last gate run on the merged tree:
  `cargo fmt` clean, `cargo clippy --workspace --all-targets --locked -- -D warnings` clean,
  `cargo test --workspace --locked --no-fail-fast`: 3619 passed, 0 failed, 304 ignored. First CI run failed on 7
  `pr-imaging` tests (CI temp dir is an 8.3 short name `RUNNER~1`; .NET-style `GetFullPath` expands it); fixed in
  `d113721` (`TempRoot` uses the long `%TEMP%`); CI re-run pending when this was written.
- Wave 4 units Partial (PARITY lists the open items): W4-01c, 02b, 10b, 11c, 11e, 14b, 14c, 14d, 16a + 2 native-test gaps.
  Open: `cargo test -p pr-imaging -p pr-platform-win -- --include-ignored` has 32 failing native tests (30 are `todo!()`
  native seams: `pr-imaging` cannot dev-depend on `pr-platform-win`, a cycle; 1 needs the WIC JPEG codec; 1 blocked on
  W4-11c OQ1). cargo-mutants was only partly run (W4-16a 10/152, W4-01c ~35/43, W4-10b 40 manual mutations).
  Latent bug noted in W4-14a: `PreviewError::is_busy/is_canceled` do not unwrap `WithSourceBytes` (C# still retries).
- Wave 3 Partial: W3-01 (cargo-mutants not done, 228/231 untested), W3-05 (verifier re-run unfinished); ignored native
  journal tests (journal 19, compaction 15, reconcile 2, startup_recovery 6, INV-6 equivalence 5) not run.
- Wave 2 Partial: W2-14 (4 mutants need a privileged symlink), W2-17, W2-01, W2-03.
- Known flaky tests (timing, fail under load): `diagnostics_file_log::*` (W2-16); bounds were raised, more may appear.

## Not done
- **C# sync patch "external editors"** (P-17): C# `master` is past `d92d6aae` (`ExternalEditor.cs`, `AppSettings`,
  `SettingsNormalizer`, `SettingsStore`, `ShortcutMappings`, `IDialogService`, languages). Needs W1-04, W2-01/02/03,
  W2-20 re-ported and goldens regenerated at the new baseline (`docs/SYNC-LEDGER.md` row exists).
- **Phase 0**: UI spike R0001 (P-06: Win32 + Direct2D via `windows-rs`), COM Explorer spike (P-07).
- **Waves 5+6** (RAW/TurboJPEG/LibRaw; Platform Windows), **7** (benchmarking), **8** (App/UI). Schedule the port workflows
  by dependency graph (P-28), not by tiers. Wave 4 lost 11 units to "blocked by a stub of the same tier".
- Rough progress: Waves 0-3 done (59 units), Wave 4 ported and in PR; about 45% of units, about 35-40% by effort.

## Next steps
1. Let PR #20 reach green and merge (auto-fix handles CI events); then delete the Wave 4 worktrees/branches
   (`integ-w4`, `f-W4-*`, `r-W4-*`, `fin/*`, `rep/*`, `wu/W4-03b`, `W4-15a-verify`) and `target-wave-4`/`target-w4ci`.
2. The main checkout may still hold stray edits an agent left (`crates/pr-imaging/src/service/disk.rs`, `service/mod.rs`,
   `docs/PARITY.md`); compare with `integration/wave-4` / master before discarding.
3. P-17 sync patch, then Phase 0 spikes, then Waves 5+6 (contracts workflow, wire + PR, port workflow, PR), 7, 8.
4. Fix the stale last sentence of AGENTS.md "Disk hygiene" (it still mentions a per-loop-tick disk check; the loop no
   longer does that).

## Wave 5 contracts (branch `wave5/contracts`, based on master `bb6a865`)
- Wired: 29 contracts (`contracts/W5-01a..W5-17.md`; W5-01, 02, 10, 11, 12, 13, 14, 15, 16 split into sub-units), stubs in
  `pr-imaging-raw` (`formats`, `contracts`, `tiff_nav`, `jpeg_probe`, `bmff`, `preview_selector`, `raw_exif`, `registry`,
  `header_source`, `decoder` + `decoder/fallback`, `routing`, `readers/{arw,cr2,cr3,dng,nef,orf,raf,rw2}`), `pr-turbojpeg`
  (`native`, `availability`, `area_resampler`, `decoder`) and `pr-libraw` (`native`, `availability`, `full_decode_gate`,
  `memory_guard`, `decoder`, `resampler`); asset `crates/pr-imaging-raw/assets/AdobeCompat-v2.icc`; 27 oracle files plus 45
  dispatch lines; 45 inputs; 45 goldens; 18 TurboJPEG fixtures (136 KB). Cargo: dev-dependencies only (D57), `.gitignore`
  gets `/tests/fixtures/raw-corpus/`. Roster, tiers and lead decisions D45..D58 are in `contracts/README.md`; PARITY has 9 new
  rows; proposals P-30..P-33.
- Run (this machine, C# `d92d6aae`): `tools/oracle.ps1 -Commit d92d6aae -Command <name>` once per command, 45 commands. The first
  pass failed on 7 (4 bad inputs, 2 missing corpus handling, 1 DLL not next to the oracle) and the oracle needed 3 source fixes;
  all were fixed (contracts/README.md D55, D56). Final: all 45 ran; every golden and the fixture folder were produced at least
  twice and compared byte for byte: identical (the three goldens that depend on the native DLLs, `turbojpeg-decode`,
  `turbojpeg-read-info` and `libraw-decoder-native`, differed only between the first pass, which ran without the DLLs, and the
  later passes; the later passes are identical). No pre-existing golden changed.
- Run: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean;
  `cargo test --workspace --locked --no-fail-fast`: 3619 passed, 0 failed, 304 ignored (no new tests: stubs are `todo!()`).
- Wiring fixes found by compiling the drafts together: two incompatible `RawHeaderError`/`EmbeddedPreview`/`ExifBlock` definitions
  (D45) and 15 unresolved imports from differing module and type names in the stubs (D46).
- NOT run: `cargo-mutants`; any Rust test against the new goldens (none written yet); any native test (nothing implemented);
  the RAW corpus (not on this machine, a download needs the user's yes, P-30), so the corpus goldens hold no corpus cases;
  the oracle on another machine or Windows build (goldens that record `windowscodecs_version` or the native DLL hashes may differ).
  The compile check of the stubs against each other is only as good as `cargo clippy`; the `todo!()` bodies were never executed.
- Open for the lead: amendments A8..A11 and proposals P-30..P-33 (contracts/README.md, docs/PROPOSED-DECISIONS.md).
- Next: dependency-graph port workflows (P-28) starting from the Tier A units; `pr_test_support::raw` first (D48).

## Wave 5 port (branch `integration/wave-5`, 2026-10-17)
- Gate run on the merged tree (after merging origin/master, CARGO_TARGET_DIR `target-wave-5/integ`): `cargo fmt --all --check` clean;
  `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked --no-fail-fast`:
  4497 passed, 0 failed, 363 ignored.
- Verifier reports: PASS for W5-02b, 10a, 10b, 11b, 12a, 13a, 13b, 14a, 15ab, 16a, 16b, 17 (only equivalent or native-only mutants
  left, listed in the PR); W5-01ab merged without a verdict; FAIL for W5-16c (10 surviving mutants, killing tests in a scratch
  file not committed). Reported not merged: W5-02a (red tests blocked), W5-04..06, 11a, 11c, 12b1, 12b2, 12c ("dependency not
  merged"), W5-14b (red tests blocked). Git history shows W5-02a/03/07/08/09/11a/11c merges from earlier runs; their verdicts are not in the final reports.
- NOT run: corpus tests (P-30), `#[ignore]` native tests (no pinned DLLs staged in this run), mutation for units without a report.
- Next: land W5-04..06, registry, RawDecoder chain (12b1/12b2/12c), TurboJPEG decoder (14b); commit the W5-16c killing tests.

## Continuation (2026-10-17, `integration/wave-5`)
- Continued from the current remote state in `.claude/worktrees/integ-w5`; the dirty primary `master` checkout was left untouched.
- Integrated W5-04 NEF (`637fc7d`): `cargo fmt --all --check` and `cargo test -p pr-imaging-raw --locked` passed; NEF suite 61 passed, 1 ignored.
- Integrated W5-14b TurboJPEG decoder (`9e0c57f`): `cargo fmt --all --check`, `cargo clippy -p pr-turbojpeg --all-targets --locked -- -D warnings`, and `cargo test -p pr-turbojpeg --locked` passed; crate totals 157 passed, 65 ignored across test binaries (native DLL and slow allocation cases remain ignored by default). This is crate-level evidence, not the full workspace gate.
- Updated `docs/PARITY.md` to remove stale duplicate Todo rows and reflect the current partial W5 state. W5-04 and W5-14b still need verifier reports; W5-14b has the documented oracle gap for file-input rows.
- Delegated W5-05 ORF port and W5-16c mutation-killer closure on isolated unit branches; neither is merged yet. Next: integrate their completed commits, continue W5-06 and W5-11a registry / W5-12 decoder chain, then run full workspace gates before PR.
- Not done in this continuation: cargo-mutants for W5-04/W5-14b, native tests with pinned DLLs, RAW corpus tests, full workspace clippy/test gate, push/PR.

## Wave 5 integration update (2026-10-17)
- Integrated W5-06 DNG at `7652b44` (unit commit `52dbf19`): `cargo fmt --all --check`, `cargo clippy -p pr-imaging-raw --all-targets --locked -- -D warnings`, and `cargo test -p pr-imaging-raw --locked` passed on the integration tree. Crate run included 50 DNG tests plus the 1-case synthetic DNG golden; the DNG implementation report counted 317 passing tests across RAW crate targets.
- Corpus tests and cargo-mutants for DNG remain unrun. Updated the I-13 parity row to mark DNG done while ORF remains pending.
- W5-05 ORF implementation commit arrived, but its golden test is under correction: the existing golden input already includes a repeated zero patch (`times: 40`) mirroring C# `Span.Clear`; likely the Rust test builder must honor the repeat field. Asked the porter to fix the builder and rerun.

## Wave 5 integration update (2026-10-17, ORF)
- Integrated W5-05 at `b95d7b8` (implementation `0eb9713`, test-helper fix `e870990`). ORF unit tests pass 39/39, including all 233 golden cases; the root cause was the Rust golden-input builder not honoring repeated-byte `times` patches, while the input and oracle were already correct.
- On the integration tree, `cargo fmt --all --check`, `cargo clippy -p pr-imaging-raw --all-targets --locked -- -D warnings`, and `cargo test -p pr-imaging-raw --locked` all passed after merging W5-05 and W5-06.
- ORF/DNG corpus checks and cargo-mutants remain open. W5-05/W5-06 still need independent verifier mutation verdicts before final Wave 5 PR gate.

## Wave 5 integration update (2026-10-17, W5-16c)
- Integrated `wu/W5-16c` test commit `0eca1b5` as a test-only change in `decoder_repair.rs`: two boundary cases cover truncated marker after an APP segment and a minimum-length APP segment before EOI.
- Focused mutation run: 8 mutants, 2 caught by the new tests, 6 not killed (5 remove handoff fields outside this subtask; 1 loop-boundary mutant judged equivalent). The prior 277-mutant run was interrupted by a global-gate hang under an unrelated `full_decode_queued_waiters -> 0` mutation; the historical set of 10 survivors is not fully reconstructed. W5-16c stays Partial.
- Mutation run for ORF is still running in this worktree. `mutants.out/` is cargo-mutants output and must be removed after the process completes; do not stage it.

## Wave 5 integration update (2026-10-17, W5-11a)
- Integrated W5-11a registry/ICC at `e541625`; direct integration test `cargo test -p pr-imaging-raw --test registry --locked` passed 30/30, including both full goldens (49 registry cases, 44 ICC cases). Agent also reported full RAW crate tests 729 passed, 0 failed, 8 ignored; fmt/clippy passed. ICC asset SHA-256 matches C# baseline `d92d6aae` (`60fb2adecacf82132db0b1c09b303316f3bdb9e2823e7ba096d01627d12d57c9`).
- This completes the registry dependency for W5-12 RawDecoder. Updated the I-14 parity status; selector/EXIF work remains separately tracked.
- ORF mutation run is ongoing; latest snapshot: 67 caught, 5 missed, 1 unviable, 0 timeout. Review the five survivors once the run completes; likely some are zero-boundary equivalences, but no verdict yet.
