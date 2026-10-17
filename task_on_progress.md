# Task on progress

**Updated:** 2026-10-17 (branch `integration/wave-5`; Wave 5 integration continues)

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

## Wave 5 integration update (2026-10-17, ORF mutation)
- Added `read_zero_ifd0_offset_does_not_check_cancellation`, covering C#'s zero-IFD0 behavior. `cargo test -p pr-imaging-raw --test readers_orf --locked` passed 40/40; `cargo fmt --all --check` and RAW crate clippy passed.
- Targeted cargo-mutants over ORF source and only the `readers_orf` test binary completed: 109 mutants, 101 caught, 7 missed, 1 unviable. A focused rerun confirmed the zero-offset `current > 0` -> `>= 0` mutant is caught. Six other missed variants were reviewed against navigator/caller bounds and judged equivalent by source inspection; they were not rerun individually. This is targeted evidence, not a package-wide mutation run.

## Wave 5 integration update (2026-10-17, W5-12b1 core)
- Cherry-picked W5-12b1 core implementation `a8d8d94` as `26546df` on `integration/wave-5`. It adds decoder construction/text defaults, identity-keyed container cache and TOCTOU checks, `read_info`, decode composition, preview range/cache handling, and error mapping; it does not implement the W5-12b2 fallback chain.
- `cargo fmt --all --check` passed; `cargo test -p pr-imaging-raw --test raw_decoder_core --locked` passed 2/2 blank-path checks. The ORF regression target still passes 40/40.
- The core's 50-case golden harness and most contract tests are still missing. Clippy is not yet clean because core seams are unused until 12b2 calls them; do not treat package tests as parity proof yet.

## Wave 5 integration update (2026-10-17, W5-12c routing)
- Implemented `FormatRoutingDecoder` routing, RAW-off localized `NotSupported`, and the `read_info` blank-path guard. Added `raw_routing.rs` with a golden runner covering all 116 `raw-routing` cases plus routing/cancellation/localization checks.
- `cargo fmt --all --check` passed and `cargo test -p pr-imaging-raw --test raw_routing --locked` passed 5/5 (including all 116 golden cases).
- `cargo clippy -p pr-imaging-raw --all-targets --locked -- -D warnings` is still blocked by three dead-code warnings in W5-12b1 core (`inner_decoder`, `source_bytes_cache`, preview range methods/error mapper), because W5-12b2 fallback remains stubbed. Routing itself emitted no clippy warning.
- Added the W5-12b1 golden harness (`raw_decoder_core.rs`): the current run passes 12 independent `read_info` golden steps and explicitly gates 40 end-to-end cases until fallback/physical hooks are available. This is partial core evidence, not a 50-case full pass.
- Re-ran the full `cargo test -p pr-imaging-raw --locked` on the integrated tree; all test binaries completed with passing results. The 8.3k-token test log was truncated by the terminal view, so no aggregate count is recorded here. W5-12b1's two blank-path tests and W5-12c's 116-case routing golden were visible and passed.

## Wave 5 integration update (2026-10-17, W5-12b2 fallback; historical checkpoint, status superseded by later entries)
- Cherry-picked fallback implementation `42503cd` as `dd56406` on `integration/wave-5` and pushed the branch (`71422c0..dd56406`). The change implements preview retry, ORF thumbnail, no-preview and full-decode paths plus classification/candidate helpers.
- Validation on the integration worktree: `cargo fmt --all --check`, `cargo test -p pr-imaging-raw --locked`, and `cargo clippy -p pr-imaging-raw --all-targets --locked -- -D warnings` all passed. This confirms the current crate test suite and lint, not the W5-12b parity contract: the 75-case fallback golden harness and complete fallback integration twins are still being added.
- Two independent agents are preparing the fallback golden/integration harness and a read-only semantic review; a third is expanding W5-12b1 core golden coverage. Their changes remain isolated pending lead review and integration.
- At this checkpoint A8 was unresolved; the later A8 update records the user's option A, implementation decision D59, and all three enabled golden rows.
- Added and integrated `raw_decoder_fallback.rs` (`376ff08` as `792ae15`): 71/75 golden cases now execute against `RawDecoder`; 3 A8 cases and the physical identity-rewrite hook case remain gated. Integration reruns of the fallback target and RAW crate clippy passed.
- Expanded `raw_decoder_core.rs` (`321dce6` as `b8bcc38`): 21/67 steps pass across 16/50 cases (13 `read_info`, 8 early-error decode); 34 cases/46 steps remain gated and are enumerated by the test. A negative preview offset case remains gated because the source-open count differs from the C# oracle.
- At this checkpoint core decode/identity cases and A8 were still open; later entries below record all core/fallback golden steps enabled and the current mutation work still underway.

## Wave 5 integration update (2026-10-17, A8 inner HRESULT)
- User confirmed A8 option A. Added `DecodeError::NotSupported.inner_hresult`, preservation through `with_user_text`, `WithSourceBytes`, and PreviewImageService error reconstruction; WIC ICC failures keep the HRESULT only when the cause is COM. Recorded D59 and marked P-32 confirmed.
- Enabled all 3 `requires:[inner_com_hresult]` rows in `raw-decoder-fallback`; integration now executes 74/75 cases, with only the physical identity-rewrite hook case gated.
- Focused verification: `cargo test -p pr-imaging --test decoder_traits --locked` passed 33/33; `cargo test -p pr-platform-win --test wic_decoder --locked` passed 41, 1 ignored (unrelated W4-11c localization seam); `cargo test -p pr-imaging-raw --test raw_decoder_fallback --locked` passed including all 74 runnable goldens.
- Mutation follow-up: the first fallback run exposed 3 surviving assignments; source tracing showed all were overwritten before reaching the inner decoder. Removed the redundant `request_with_empty_bytes` intermediary and expanded the golden fake to compare the full inner `DecodeRequest` trace. Focused rerun caught both required-field deletion mutants (2/2); fallback golden remains green.
- Core parity follow-up: compared `read_preview_range` with C# `RawDecoder.ReadPreviewRange` at baseline `d92d6aae`. Moved the negative count/offset guard after opening the source and checking its length, matching C# call order while still rejecting before allocation/seek. Focused core golden rerun is pending; workspace gates will follow the core harness integration.
- Core golden follow-up: added a scripted `DecodedImageView`/decoder and test-owned physical files in `raw_decoder_core.rs`; now 51 steps pass (13 `read_info`, 29 image decode, 9 early errors), including `range-negative-offset`. The four physical read/cache cases use RAII-cleaned temp files and match every golden field. Seven stateful identity/rewrite cases (16 steps) remain gated because they require deterministic mutation hooks or metadata timing.
- Integration gate checkpoint before the physical golden extension: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4,803 passed, 0 failed, 413 ignored; 206 test-result suites). `cargo mutants` focused on thumbnail `DecodeRequest` fields caught 2/2 required-field deletion mutants. Native tests and RAW corpus tests were not run; native dependencies/corpus remain unavailable in this worktree. Rerun the workspace gates after the current test expansion below.

## Wave 5 integration update (2026-10-17, core physical goldens and focused mutations)
- Extended `raw_decoder_core.rs` to use RAII-cleaned test files and deterministic hooks for indexed append/touch-before-open, rewrite timestamps, and move-during-read. The harness compares per-step source-open deltas (the C# golden does not record a cumulative count). All cache/read/error fields match without weakening assertions: 63 steps pass (16 `read_info`, 40 scripted image decode, 7 early errors), covering all 50 cases with 0 gates. Focused core test passed 3/3.
- Mutation verification: `read_preview_range` guard set, 4/4 targeted `||` mutants caught by the core golden runner. W5-16c targeted handoff field mutations caught 4/5 (`downscaled`, `actual_backend`, `original_width`, `original_height`); omitting explicit `orientation` is equivalent under default value 1. The structural `<` to `<=` boundary mutant is also equivalent. This does not reconstruct the historical complete 10-survivor run.
- NEF/DNG mutation follow-up added focused killers only in their test files. NEF original run: 126 mutants (117 caught, 8 missed, 1 unviable); two zero-offset survivors are equivalent after downstream guards, while six distinct survivors now have targeted tests, and a focused rerun caught 6/6. DNG original run: 94 (80 caught, 10 missed, 4 unviable); three disjoint-mask XORs are equivalent, six targeted behavior gaps were caught 6/6 (zero SubIFD/next pointers use a deterministic cancellation boundary), and `current > 0` to `>= 0` remains a narrow zero-IFD cancellation-check survivor with no stable hook. The test whose name sounded like a cap boundary does catch the cap mutant; it was distinct from these offset guard mutations. Focused suites now pass: `raw_nef` 67 passed/1 ignored, `dng_reader` 55/55. Targeted mutation output preserved in `target-wave-5/nef-killers-mut`; NEF/DNG original outcome data in `target-wave-5/agent-nef-dng`.
- Latest full integration gate after NEF/DNG killer additions and before applying the TurboJPEG helper tests: `cargo fmt --all --check`, workspace clippy, and workspace tests passed (4,814 passed, 0 failed, 413 ignored across 206 suites). Focused `raw_nef` passed 67/68 (one ignored), `dng_reader` 55/55. NEF's six behavioral survivors and DNG's six behavior gaps were each caught by targeted reruns (6/6); two NEF zero-offset operators are equivalent, three DNG mask XORs are equivalent, and DNG's zero IFD0 chain guard remains a narrow cancellation-check survivor. Core runner passes all 63 steps, asserts every expected step is consumed, and moves the file in `GoldenReader::read` to match the C# callback boundary; uninstrumented call-count fields are no longer treated as Rust evidence. `8ecdfcc` contains the core audit fix.
- W5-14b pure mutation verification is bounded/partial: broad run stopped at 72 outcomes (42 caught, 22 missed, 7 unviable, baseline passed); focused run matched 162 candidates but evaluated 77 (68 caught, 5 missed, 4 unviable), leaving 85 unevaluated. Evaluated functions: `scaled_size`, `select_scaling_factor`, `calculate_output_buffer`, `output_fits`, `source_fits`, and `try_read_segment`. Five targeted follow-up mutations: 3 caught (`calculate_output_buffer` `>`→`==` and `>`→`<`, plus `try_read_segment` `+`→`-`), 2 equivalent (`>`→`>=` threshold mathematically unreachable for i32 inputs; `+`→`*` equivalent under the outer offset bound). Two public-helper Rust-only tests were committed/pushed on `wu/W5-14b` as `0f19229`; patch applied locally to integration and `cargo test -p pr-turbojpeg --test decoder_pure --locked` passed 90/90 runnable, 2 ignored. Final integration workspace gates remain to be rerun after this patch/test-only wave.
- Final combined integration gate after applying the TurboJPEG helper tests: `git diff --check`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` all passed. Exact test total: 4,816 passed, 0 failed, 413 ignored across 206 suites. Commit `7cba449` pushed this slice. The fallback identity-rewrite golden now runs; W5-14b remaining pure-helper mutation candidates are under bounded verification. Native tests still need pinned DLLs and RAW corpus tests need the corpus.
- Fallback golden completion: `raw_decoder_fallback.rs` now executes 75/75 cases with no gates. The physical identity-rewrite case uses a unique RAII-cleaned test directory, a fixed initial mtime, and a deterministic timestamp update inside the first preview decode; no sleeps or user files. Source-open and stream-byte counters reset at each operation to match per-operation oracle fields, while container read totals remain cumulative. Focused integration rerun passed 1/1; fmt and diff checks passed.
- Re-ran the combined workspace gate after the fallback golden change: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked --quiet` all exited 0. Test binaries reported no failures; the suite footprint remains 206 suites / 4,816 passed / 413 ignored.
- W5-14b remaining-helper mutation continuation completed on the separate `W5-14b` worktree: 51 candidates, 42 caught, 8 missed, 1 unviable; baseline `decoder_pure` passed 90 with 2 ignored. Survivors: two `scan_header` early-stop conditions, four `grow_header_area` loop-bound conditions, and two reserve-size arithmetic mutations. The loop-bound survivors are output-equivalent under the clamped `limit` and no-growth guard; early-stop survivors only lose the optimization when the corresponding output was not requested. Reserve arithmetic survivors remain open because they can affect allocation behavior and are not proven equivalent. The `finish_with_codec` body replacement was unviable. Artifacts are preserved in the W5-14b worktree; no production/test source was edited in that lane.
