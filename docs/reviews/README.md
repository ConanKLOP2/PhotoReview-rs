# docs/reviews index

Static review reports written by Machine C (cloud session) on 2026-10-23, plus the consolidated [`BACKLOG.md`](BACKLOG.md). The report files are evidence and are not edited; this index only points to them.

How to read this page:

- **Date**: the date written in the file's own header where there is one; otherwise "2026-10-23 (git)" = the date the file was first committed (every file here was added on 2026-10-23). The commit hashes are the ones the file itself states; "commit not stated" means the file gives none.
- **Findings**: copied from the file's own counts line (H = high, M = medium, L = low). Where a file has a re-check section, that is noted.
- **Status of findings**: derived ONLY from the status table "Trạng thái xử lý tại master b5d1327 (2026-10-23)" and the Q1..Q8 decision table (G24) in `BACKLOG.md`. If `BACKLOG.md` does not say, the cell says "see file". `BACKLOG.md` is a 2026-10-23 snapshot and may be stale; check `task_on_progress.md` and `docs/PARITY.md` for newer state.
- Every finding in every report is inferred from the code and was NOT run (each file says so). `BACKLOG.md` marks no group as closed (G21 is "partly fixed", G24 decisions are "decided"), so no row below says closed.
- G1..G31 are the backlog groups in `BACKLOG.md` section 1; Q1..Q8 are the product decisions in G24 (accepted as P-52 in `docs/PROPOSED-DECISIONS.md`).
- 2026-11-11 update: the mutation reports of 2026-11-04..11 are indexed at the end of the first table; the current state of the groups is "Update 2026-11-11" at the end of [`BACKLOG.md`](BACKLOG.md), which supersedes the 2026-11-03 section where they differ.
- 2026-11-03 update: the "Status of findings" cells in the tables below are the 2026-10-23 snapshot and are stale for many groups. The current state of G1..G31 is in the section "Update 2026-11-03" at the end of [`BACKLOG.md`](BACKLOG.md); the rows above (re-check evidence and owner-decision implementations) point to the PRs, and the decision rows are P-300..P-390 in [`PROPOSED-DECISIONS.md`](../PROPOSED-DECISIONS.md).

There were 18 files here on 2026-10-23: 17 reports and `BACKLOG.md` (the first table now also indexes later reports). There is no separate report for Wave 10 contracts; the only contract review is the Wave 9 one. No two reports are exact duplicates and none supersedes another (see "Relations between files").

## Consolidated backlog

| File | Date | What it covers | Status |
|---|---|---|---|
| [`BACKLOG.md`](BACKLOG.md) | 2026-10-23 | Merges the findings of `autopilot`, `dialogs-base`, `native-dialog`, `render-painter`, `settings-ui`, `startup`, `test-quality`, `invariants`, `main-window-input-overlay-viewing`, `w9-contracts`, `pr-164`, `pr-203` into groups G1..G24; section 5 (re-synthesis) adds G25; section 6 adds `invariants-storage` (G26..G29); appended sections fold in `recovery-flow` (into G17), `host-dispatcher` (into G11) and `recycle-policy` (G30, G31); the last section is the status table at master b5d1327. Holds the Codex prompts per group. `test-strengthening-plan.md` is pointed to, not merged line by line. | The source of the Status column below |
| [`ST-BACKLOG-NOW-2026-11-01.md`](ST-BACKLOG-NOW-2026-11-01.md) | 2026-11-01 | Current status of G1..G31 at master `71d9ea76` (8 fixed, 14 partial, 7 open, 1 needs decision) and the top 8 open items by data risk with file scope and tests to add. | Supersedes the 2026-10-23 status table in `BACKLOG.md` (static, not run) |
| [`G17-IGNORED-TESTS-2026-11-03.md`](G17-IGNORED-TESTS-2026-11-03.md) | 2026-11-03 | Audit of `#[ignore]` tests at master `82da9f7`: 1548 attributes per crate and reason category, 0 without a reason, 276 run by `native-nightly.yml`, 1272 run nowhere. Commands included; nothing was run with `cargo`. | Counts for BACKLOG G17 (static audit); P-80 asks the owner about the never-run blocks; section 8 (2026-11-10) is the P-370 per-family owner table |
| [`AR16-REMOVEPATHS-MUTATION-2026-11-03.md`](AR16-REMOVEPATHS-MUTATION-2026-11-03.md) | 2026-11-03 | Mutation run of `ReviewCatalog::remove_paths` (AR16): 24 of 24 tested, 20 caught, 3 missed (all equivalent), 1 unviable, 0 timeouts. | Measured; PARITY row AR16 stays Partial |
| [`STJ-READER-TIMEOUTS-2026-11-03.md`](STJ-READER-TIMEOUTS-2026-11-03.md) | 2026-11-03 | cargo-mutants rerun of the 8 former `*= 1` timeouts of `mod json` (X-stj-reader): 8 of 8 tested, 8 caught (bounded-wait test), 0 missed, 0 timeouts. | Measured; PARITY row X-stj-reader stays Partial |
| [`SEC01-MUTATION-2026-11-03.md`](SEC01-MUTATION-2026-11-03.md) | 2026-11-03 | Mutation run of the SEC-01 link-walk functions of `physical.rs`: 27 of 27 tested, 17 caught by the pure tests, 10 missed (8 killed by hand-run native tests, 2 equivalent), 0 unviable, 0 timeouts. | Measured; PARITY row SEC-01 stays Partial |
| [`JOURNAL-LINE-MUTATION-2026-11-03.md`](JOURNAL-LINE-MUTATION-2026-11-03.md) | 2026-11-03 | Partial mutation run of `journal_line.rs`: 92 of 136 tested, 78 caught, 5 missed (4 killed by new tests, 1 equivalent), 9 unviable, 44 NOT RUN. | Partial; PARITY journal rows stay Partial |
| [`JOURNAL-LINE-MUTATION-2-2026-11-03.md`](JOURNAL-LINE-MUTATION-2-2026-11-03.md) | 2026-11-03 | Completes `journal_line.rs`: the remaining 44 mutants, 35 caught, 1 equivalent missed, 8 unviable, 0 timeouts; 5 glue mutants skipped on purpose. | Measured |
| [`RECOVERY-RETRY-MUTATION-2026-11-03.md`](RECOVERY-RETRY-MUTATION-2026-11-03.md) | 2026-11-03 | Mutation run of the `recovery_retry.rs` single-retry path: 54 of 160 run, 49 caught, 1 equivalent, 4 unviable; group path NOT run. | Partial |
| [`RECOVERY-RETRY-MUTATION-2-2026-11-03.md`](RECOVERY-RETRY-MUTATION-2-2026-11-03.md) | 2026-11-03 | `recovery_retry.rs` group path: 74 new mutants, 69 caught, 0 missed, 5 unviable; 32 glue mutants NOT run (128 of 160 measured). | Partial |
| [`G28-LOG-SHORT-WRITE-2026-11-03.md`](G28-LOG-SHORT-WRITE-2026-11-03.md) | 2026-11-03 | G28 log short-write: 4 more seam tests in `file_log.rs`; duplication across drains after two failures is open as P-140 (needs a behaviour change). | Partial |
| [`G29-CACHE-READ-2026-11-03.md`](G29-CACHE-READ-2026-11-03.md) | 2026-11-03 | G29 re-checked: the size check before allocation and the fallible buffer were fixed in #474; only the read-time `OutOfMemory` classification is left (P-150). | Mostly closed |
| [`G30-RECYCLE-REGISTRY-TESTS-2026-11-03.md`](G30-RECYCLE-REGISTRY-TESTS-2026-11-03.md) | 2026-11-03 | G30 is stale: the second registry read is checked since #532 (`read_dword_with_query`) and `recycle_bin_settings_capacity.rs` covers the cases; no test added. | Done |
| [`G31-RESTORE-VERB-2026-11-03.md`](G31-RESTORE-VERB-2026-11-03.md) | 2026-11-03 | G31 re-checked: the restore verb helper is still unwired (Rust restores with the canonical `undelete` only, C# builds an HMENU and uses `QueryContextMenu`); decision recorded as P-160. | Open, P-160 |
| [`G21-W9-CONTRACTS-2026-11-03.md`](G21-W9-CONTRACTS-2026-11-03.md) | 2026-11-03 | G21 re-checked: w9-contracts H1 and L6..L10 are all already corrected on master; no contract text changed. | Done |
| [`G18-PR164-TESTS-2026-11-03.md`](G18-PR164-TESTS-2026-11-03.md) | 2026-11-03 | G18 re-checked per finding: M1 to M3 were closed earlier; L2, L3, L4 now covered by `g18_error_log_cut.rs`, `g18_launch_log.rs`, `g18_startup_native.rs` (4 native tests `#[ignore]`d); the spawn-failure branch is P-170. | Mostly closed |
| [`TIFF-NAV-MUTATION-2026-11-03.md`](TIFF-NAV-MUTATION-2026-11-03.md) | 2026-11-03 | Mutation run of the 52 `tiff_nav.rs` mutants left unrun: 49 caught, 3 missed (2 equivalent, 1 killed by a new short-read test). | Measured |
| [`PERF-DIAGNOSTICS-MUTATION-2026-11-03.md`](PERF-DIAGNOSTICS-MUTATION-2026-11-03.md) | 2026-11-03 | Re-run of the 11 X-perf-diagnostics identities that had no verdict: 11 of 11 caught. | Measured |
| [`PHYSICAL-LISTING-MUTATION-2026-11-03.md`](PHYSICAL-LISTING-MUTATION-2026-11-03.md) | 2026-11-03 | Mutation run of the physical listing functions (X-physical-fs, AR16): 0 survivors. | Measured |
| [`G1-G14-2026-11-03.md`](G1-G14-2026-11-03.md) | 2026-11-03 | Re-check of G1 and G14 at master `4c3e41b`: G1 already fixed (#544, `SharedViewerAdapter`); G14 L1, L2, L5 closed, one timer-queue panic test added; L3 became P-74, L4 became P-301, spawn-error log became P-300. | Done (PR #612); G14 L3/L4/spawn log decided, see [PROPOSED-DECISIONS](../PROPOSED-DECISIONS.md) |
| [`G5-G6-G12-2026-11-03.md`](G5-G6-G12-2026-11-03.md) | 2026-11-03 | Re-check of G5, G6 and G12: G5 fixed in #462, G6 in #610 (D2), G12 L1 by P-71 C; 3 tests added; open items became P-330 and P-331. | Done (PR #613); P-330 partly done, P-331 Rust done |
| [`G7-G15-2026-11-03.md`](G7-G15-2026-11-03.md) | 2026-11-03 | Re-check of G7, G8, G13 and G15: dialog, input and startup tests added; open items became P-340..P-343; G15 L6 is the percent text (P-92). | Done (PR #614); P-340..P-343 done in #635 |
| [`G11-2026-11-03.md`](G11-2026-11-03.md) | 2026-11-03 | Re-check of G11: native-dialog M3, L3, L4 closed by #539 (P-67); L1, L2, L5 stay as deliberate C# parity; 2 invariant tests added. | Done (PR #615) |
| [`G16-G22-G23-2026-11-03.md`](G16-G22-G23-2026-11-03.md) | 2026-11-03 | Re-check of G16, G22 and G23: G22 closed, G23 closed except P-363..P-365, G16 open items became P-88, P-89, P-360, P-361; `claim-selftest.sh` gained release and gc cases. | Done (PR #616); P-88/P-89/P-360/P-361/P-363/P-365 decided |
| [`G17-G19-2026-11-03.md`](G17-G19-2026-11-03.md) | 2026-11-03 | Re-check of G17 and G19: M2 stale ignores closed, M3 closed for `pr-shell` host registrations and 5 `pr-app` oracle tests; G19 has no test left to add; open items became P-370..P-374. | Done (PR #617); P-370 rule part, P-372..P-374 done, P-370 per-family owners drafted 2026-11-10 (unconfirmed proposal, G17-IGNORED-TESTS section 8), P-371 deferred |
| [`G26-G27-2026-11-03.md`](G26-G27-2026-11-03.md) | 2026-11-03 | Re-check of G26 and G27: G26 fixed in #474 (DV-9), one newer-claim gate test added; G27 fixed by P-66 A (#530). | Done (PR #618); both closed |
| [`GLUE-MUTANTS-2026-11-03.md`](GLUE-MUTANTS-2026-11-03.md) | 2026-11-03 | Glue mutants of `recovery_retry.rs` (77 selectable, all measured) and `journal_line.rs` (5 skipped, now measured); P-390 for the 3 equivalent `expecting` mutants. | Measured (PR #619); P-110 done, P-390 closed |
| [`G4-G9-2026-11-03.md`](G4-G9-2026-11-03.md) | 2026-11-03 | Re-check of G4 and G9: G4 already fixed; G9 M1 fixed in code (W9-E4) and now covered by 7 tests; open items became P-320..P-322. | Done (PR #620); P-320, P-322 done in #634, P-321 in PR #628 (open at the time of writing) |
| [`G2-2026-11-03.md`](G2-2026-11-03.md) | 2026-11-03 | Re-check of G2 and G10: wheel, key and click zoom reach the presenter (4 tests); the HUD refresh is P-310; G10 open items became P-311..P-313. | Done (PR #621); P-310, P-312, P-313 done, P-311 open |
| [`D1-IMPL-2026-11-03.md`](D1-IMPL-2026-11-03.md) | 2026-11-03 | Owner decision D1 (P-310): refresh the HUD zoom text after an input zoom; restores C# `MainViewModel.cs:140-143`. | Done (PR #624, merged) |
| [`D2-IMPL-2026-11-03.md`](D2-IMPL-2026-11-03.md) | 2026-11-03 | Owner decision D2 (P-74 C): bounded pool of prefetch lookup lanes with `catch_unwind`. | Done (PR #625, merged) |
| [`D3-IMPL-2026-11-03.md`](D3-IMPL-2026-11-03.md) | 2026-11-03 | Owner decision D3 (P-191): restore the D-07f-5 pre-check as a Rust-only extra when every kept copy was modified. | Done (PR #626, merged) |
| [`D4-IMPL-2026-11-03.md`](D4-IMPL-2026-11-03.md) | 2026-11-03 | Owner decision D4 (P-92 A): percent pattern read from the OS, `-0.0` sign fixed, known NLS-vs-ICU differences listed. | Done (PR #627, merged; the file is on master) |
| [`D5-IMPL-2026-11-03.md`](D5-IMPL-2026-11-03.md) | 2026-11-03 | Owner decision D5 (P-321): update-result retry every 20 ms, 50 tries at most. | Done (PR #628, merged) |
| [`D6-IMPL-2026-11-03.md`](D6-IMPL-2026-11-03.md) | 2026-11-03 | Owner decision D6 (P-330): evidence only; the C# branch `feat/executor-throw-unverified-status` (`a3dc30cc`, local) adds `status.actionOutcomeUnknown`; the Rust part waited for the catalog sync. | Closed 2026-11-06: C# #539, Rust #671 (PR #629 was evidence only) |
| [`D7-IMPL-2026-11-03.md`](D7-IMPL-2026-11-03.md) | 2026-11-03 | Owner decision D7 (P-331): reconcile of a single Recycle asks the Recycle Bin; C# branch `feat/reconcile-recycle-bin-check` (`396d5ac2`, local at the time). | Closed 2026-11-06: Rust #630 and #671, C# #540 (`f57061ef`); matches baseline `5e5a9ce5` |
| [`D8-IMPL-2026-11-03.md`](D8-IMPL-2026-11-03.md) | 2026-11-03 | Owner decision D8 (P-150 A): a read-time `OutOfMemory` keeps the preview cache entry. | Done (PR #631, merged); deviation from C# |
| [`D9-IMPL-2026-11-03.md`](D9-IMPL-2026-11-03.md) | 2026-11-03 | Owner decision D9 (P-301): `FrameLog` writes on a bounded writer thread. | Done (PR #632, merged); deviation from C# `AutoFlush` |
| [`E1-IMPL-2026-11-03.md`](E1-IMPL-2026-11-03.md) | 2026-11-03 | Group B items P-300, P-312, P-313: timer spawn log, image cache borrow, released-device pops. | Done (PR #633, merged) |
| [`E2-IMPL-2026-11-03.md`](E2-IMPL-2026-11-03.md) | 2026-11-03 | Group B items P-320, P-322: nested `EN_CHANGE` test, ordinal ignore-case shortcuts. | Done (PR #634, merged) |
| [`E3-IMPL-2026-11-03.md`](E3-IMPL-2026-11-03.md) | 2026-11-03 | Group B items P-340, P-341, P-342, P-170, P-343: dialog forwarding, panicking port factory, cloak spawn failure, startup join log. | Done (PR #635, merged) |
| [`F1-IMPL-2026-11-03.md`](F1-IMPL-2026-11-03.md) | 2026-11-03 | Section C decisions: P-86 A, P-374, P-88 A, P-89 A, P-360 B, P-361 B, P-111 B, P-370 rule part, P-390 wording. | Done (PR #622, merged) |
| [`F2-IMPL-2026-11-03.md`](F2-IMPL-2026-11-03.md) | 2026-11-03 | Section C test cleanups: P-363, P-365, P-372, P-373 (P-364 was already done by #617). | Done (PR #623, merged) |
| [`PARITY-WORDING-OWNER-DECISIONS-2026-11-03.md`](PARITY-WORDING-OWNER-DECISIONS-2026-11-03.md) | 2026-11-03 | Proposed PARITY `Different` rows, notes and ADR wording for the owner decisions of 2026-11-03 (P-92, P-150, P-301, P-331, P-191, P-74, P-321 and the Rust-only hardening notes). | For the lead; not applied to `PARITY.md` |
| [`M1-MUT-2026-11-04.md`](M1-MUT-2026-11-04.md) | 2026-11-04 | M1: mutation of `viewport/types.rs`, `input/primitives.rs` and `viewport/layout.rs` (the batches Q1 left NOT RUN), hand-written tests only. | Measured; row status in `PARITY.md` |
| [`M2-MUT-2026-11-04.md`](M2-MUT-2026-11-04.md) | 2026-11-04 | M2: mutation of `pr-app` `viewport/render_geometry.rs` (X-render-geometry), closes the Q1 NOT RUN list for that file. | Measured |
| [`M3-MUT-2026-11-04.md`](M3-MUT-2026-11-04.md) | 2026-11-04 | M3: `viewer_state.rs` and `kinetic_pan.rs` (X-viewer-state, X-kinetic-pan), 20 new tests, no production change. | Measured |
| [`M4-MUT-2026-11-04.md`](M4-MUT-2026-11-04.md) | 2026-11-04 | M4: presenters, scrollbar geometry, zoom detail and recovery check, 10 files decided with non-golden tests. | Measured |
| [`M5-MUT-2026-11-04.md`](M5-MUT-2026-11-04.md) | 2026-11-04 | M5: the NOT RUN batches of the Q3..Q11 close-outs that a machine can run (`perf.rs`, F-13 files, resampler, checker, `journal_line.rs`). | Measured |
| [`N1-MUT-2026-11-05.md`](N1-MUT-2026-11-05.md) | 2026-11-05 | N1: `pr-core` io mutation close-out under rule B (P-391): updates checker, `app_settings.rs` JSON reader, F-21 service side, run-proofs for the M5 survivors. | Measured |
| [`N2-MUT-2026-11-05.md`](N2-MUT-2026-11-05.md) | 2026-11-05 | N2: run-proofs and killers for the 19 equivalents of M1, M2, M3 that were argued only by reading; 3 were not equivalent and are killed. | Measured |
| [`N3-MUT-2026-11-05.md`](N3-MUT-2026-11-05.md) | 2026-11-05 | N3: run-proofs for the 21 equivalents of M4 and the 25 NOT RUN `zoom_detail.rs` mutants. | Measured |
| [`N4-MUT-2026-11-05.md`](N4-MUT-2026-11-05.md) | 2026-11-05 | N4: native decoders (libraw, turbojpeg), RAW readers, bench presenter and WinHTTP transport under rule B. | Measured; survivors continue in P2 |
| [`P1-MUT-2026-11-06.md`](P1-MUT-2026-11-06.md) | 2026-11-06 | P1: RAW selector shard 0/2, `tiff_nav.rs` and the N4 reader survivors under rule B (strict). | Measured |
| [`P2-MUT-2026-11-06.md`](P2-MUT-2026-11-06.md) | 2026-11-06 | P2: second pass over the open N4 survivors (libraw and turbojpeg `decoder.rs`, `tool_path_guard.rs`, presenter `benchmark.rs`, `http/transport.rs`). | Measured; later work in P393, P-398, P-399 |
| [`P3-MUT-2026-11-06.md`](P3-MUT-2026-11-06.md) | 2026-11-06 | P3: pr-platform-win host, WIC decode and cache codec, painter widgets, main-window core under rule B; lists what a Windows session can run. | Measured; see P4..P10 |
| [`P4-MUT-2026-11-06.md`](P4-MUT-2026-11-06.md) | 2026-11-06 | P4: whole-domain attempt on the viewport rows held at Ported. | Measured |
| [`P4-WIC-SCALER-MUT-2026-11-06.md`](P4-WIC-SCALER-MUT-2026-11-06.md) | 2026-11-06 | P4: `wic/scaler.rs` and `host/native.rs` (33 mutants, NOT RUN in P3). | Measured |
| [`P5-MESSAGE-LOOP-MUT-2026-11-06.md`](P5-MESSAGE-LOOP-MUT-2026-11-06.md) | 2026-11-06 | P5: `host/message_loop.rs`, bounded `add_frame_clock` killer; mutant 170:72 no longer exists since P-396. | Measured |
| [`P6-CACHE-IMAGE-MUT-2026-11-06.md`](P6-CACHE-IMAGE-MUT-2026-11-06.md) | 2026-11-06 | P6: `wic/cache_image.rs` (W4-11e) under rule B; 679:13/680:13 stay open (P-402). | Measured; two mutants open by owner decision |
| [`P7-WIC-DECODER-MUT-2026-11-06.md`](P7-WIC-DECODER-MUT-2026-11-06.md) | 2026-11-06 | P7: `wic/decoder.rs`, all 218 mutants run. | Measured |
| [`P8-MAIN-WINDOW-CORE-MUT-2026-11-06.md`](P8-MAIN-WINDOW-CORE-MUT-2026-11-06.md) | 2026-11-06 | P8: `host/main_window/core.rs` (X-main-window-core) under rule B. | Measured; see the file for survivors |
| [`P9-PAINTER-WIDGETS-MUT-2026-11-06.md`](P9-PAINTER-WIDGETS-MUT-2026-11-06.md) | 2026-11-06 | P9: `painter/widgets.rs`, all 266 mutants run. | Measured |
| [`P10-HOST-SMALL-MUT-2026-11-06.md`](P10-HOST-SMALL-MUT-2026-11-06.md) | 2026-11-06 | P10: `host/icons.rs`, `shell_window/messages.rs`, `swap_chain_clock.rs`, `wndproc.rs` under rule B. | Measured |
| [`P393-MUT-2026-11-06.md`](P393-MUT-2026-11-06.md) | 2026-11-06 | P-393 seams for the N1/N4 survivors (turbojpeg 1204:27, WinHTTP transport 443/501, service 683:13). | Done (seams); tool_path_guard 299 and LibRaw were closed later by #709 and #713 |
| [`P393B-APP-2026-11-06.md`](P393B-APP-2026-11-06.md) | 2026-11-06 | P-393 option 1: dead `e` branch of `dotnet_double_text` removed, no behaviour change. | Done |
| [`P393B-CORE-2026-11-06.md`](P393B-CORE-2026-11-06.md) | 2026-11-06 | P-393B-CORE: redundant rule 1 of `aggregate_group_verdict` and redundant `Exists` guard in `check_group_member` removed after exhaustive tests. | Done |
| [`P394-MUT-2026-11-06.md`](P394-MUT-2026-11-06.md) | 2026-11-06 | P-394 seams: mutation evidence, no behaviour change versus the C# baseline. | Done |
| [`TRANSACTION-RERUN-2026-11-10.md`](TRANSACTION-RERUN-2026-11-10.md) | 2026-11-10 | F-12 / F-15 transaction rerun: `journal_rules.rs`, `transaction.rs` and the first 24 identities of `journal_compaction_files.rs` (no valid verdict for that file yet). | Measured; `journal_compaction_files.rs` NOT decided |
| [`COMPACTION-FILES-MUT-2026-11-11.md`](COMPACTION-FILES-MUT-2026-11-11.md) | 2026-11-11 | `io/journal_compaction_files.rs`: 100 mutants decided after a flake analysis (79 caught, 13 unviable, 3 equivalent, 4 not compiled on Windows, 1 removed by refactor); test-helper flake fixed and the 19 affected catches rerun; const-mask refactor tried and reverted. | Measured |
| [`D10-GLUE-MUT-2026-11-11.md`](D10-GLUE-MUT-2026-11-11.md) | 2026-11-11 | Settings glue `builder/{inputs,groups,rows}` and `context`: 446 mutants run completely (372 caught, 38 unviable, 36 equivalent by argument, 0 NOT RUN); 10 survivors killed by new tests; `dialog/{host,surface,frame}` still open. | Measured |
| [`PHYSICAL-TAIL-MUTATION-2026-11-10.md`](PHYSICAL-TAIL-MUTATION-2026-11-10.md) | 2026-11-10 | `physical.rs` tail: 76 identities of the groups Q3 listed as never run; the rest of the 440 identities is not decided here. | Measured; remainder open |
| [`P393-GUARD-2026-11-10.md`](P393-GUARD-2026-11-10.md) | 2026-11-10 | P-393 / P-398: the non-Windows twin of `tool_path_guard` `leaf_refuses_replacement` removed; replace refusal is Windows only. | Done (PR #709, merged) |
| [`D10-SETTINGS-GLUE-2026-11-10.md`](D10-SETTINGS-GLUE-2026-11-10.md) | 2026-11-10 | P-401: PARTIAL mutation run of the Settings window glue, 79 of 846 identities; not a verdict for any file. | Partial (PR #712, merged); 767 NOT RUN |
| [`P399-MUT-2026-11-11.md`](P399-MUT-2026-11-11.md) | 2026-11-11 | P-399: LibRaw `try_set_use_camera_wb` mutant closed with a test-only version-gate seam, no DLL downloaded. | Done (PR #713, merged) |

## Autopilot tooling

| File | Date | Findings | Reviewed code | Status of findings |
|---|---|---|---|---|
| [`csharp-defects.md`](csharp-defects.md) | 2026-10-26 | 8 lỗi của app C# (D1..D8), 2 lỗi mất dữ liệu | C# `7eaee2a2`: RecoveryRetryService, DuplicateCleanupController, MessageLoop, UndoService, OperationJournal | cần sửa bên repo C#; suy luận từ mã, chưa chạy |
| [`autopilot.md`](autopilot.md) | 2026-10-23 (git) | 1 H / 7 M / 11 L | `tools/autopilot/{claim.sh,codex-gate.sh,codex-pipeline.sh,codex-pack.sh,codex-run.sh,codex-usage.sh,status.sh,unit-pr.sh,lib/codex-lib.py,lib/fmtcheck.sh,test/pipeline-selftest.sh}`; commit not stated | G16: open (script/tooling still open; re-read findings against the current version before fixing) |

## Platform dialogs and Win32 host

| File | Date | Findings | Reviewed code | Status of findings |
|---|---|---|---|---|
| [`dialogs-base.md`](dialogs-base.md) | 2026-10-23 (git) | 0 H / 1 M / 3 L | `pr-platform-win/src/dialogs/{file_dialog,folder_dialog,clipboard,task_dialog,dialog_service,zoom_prompt_service}.rs` and their tests (W8-18a..18c), vs C# `7eaee2a2`; Rust commit not stated | G7 (M1, L1, L2): open |
| [`native-dialog.md`](native-dialog.md) | 2026-10-23 | 0 H / 3 M / 6 L (9 findings) | `pr-platform-win/src/dialogs/native/dialog/` (W8-18h..18k, PR #184); commit not stated | G11 (M3, L1..L5): open; G17 (M1, M2, L6): open |
| [`render-painter.md`](render-painter.md) | 2026-10-23 (git) | 0 H / 1 M / 5 L | `pr-platform-win/src/dialogs/native/painter/` and `src/render/` (W8-18l..18n, W8-12b..12i); commit not stated; the file lists which files were read line by line and which were only skimmed | G10 (M1, L1..L3): open |
| [`host-dispatcher.md`](host-dispatcher.md) | 2026-10-23 | 0 H / 1 M / 0 L (new) | `pr-platform-win/src/host/{ui_dispatcher,message_loop,pending_panic,wndproc}.rs`, parts of `shell_window/create.rs` and `dialogs/native/dialog/modal.rs`; Rust `a9ed01d`, C# `7eaee2a2` | G11 (M1 added): open; C# has the same gap, a change needs a parity/ADR decision |
| [`recovery-flow.md`](recovery-flow.md) | 2026-10-23 | 0 H / 1 M / 0 L (new) | `pr-app/src/presenters/recovery_window.rs`, `recovery_presentation.rs`, `recovery_window_presenter.rs` (W8-10g); Rust `bb5c02a`, C# `7eaee2a2` | G17 (M1 added): open; a test gap, not evidence of a production bug |
| [`recycle-policy.md`](recycle-policy.md) | 2026-10-23 | 0 H / 1 M / 1 L | `pr-platform-win/src/{recycle_bin_capacity,recycle_bin_settings,recycle_candidate,recycle_eligibility,recycle_bin_shell,restore_verb}.rs`, `recycle_restore.rs:115-168`; Rust `8042d67`, C# `7eaee2a2` | G30 (M1): open; G31 (L1): open, parity stays Partial |

## Main window, settings UI, startup, PR reviews

| File | Date | Findings | Reviewed code | Status of findings |
|---|---|---|---|---|
| [`main-window-input-overlay-viewing.md`](main-window-input-overlay-viewing.md) (B4) | 2026-10-23 (git); re-check section at Rust `48f96aa` | 3 H / 2 M / 5 L (the re-check keeps 3 H / 2 M / 5 L, 0 new) | `pr-platform-win/src/host/main_window/*`, `shell_input/*`, `shell_menus/*`, `shell_overlay/*`, `shell_viewing/*` and tests (W8-13g..13k, 14a..14g, 15f..15j, 16b..16i); first-pass commit not stated | G1, G2, G14: open; M1 `OpenInDefaultEditor` is G8 (decision Q4 accepted, implementation open); M2 is in G17: open |
| [`settings-ui.md`](settings-ui.md) | 2026-10-23 (git) | 1 H / 3 M / 5 L | `pr-platform-win/src/dialogs/settings/` and `crates/pr-platform-win/tests/settings_*.rs` (W8-19a..19i), vs C# `7eaee2a2` | G4 (H1), G9 (M1..M3, L1..L4): open; L5 is in G17 / G23: open |
| [`startup.md`](startup.md) | 2026-10-23 (git) | 1 H / 4 M / 6 L | `pr-platform-win/src/shell_startup/*`, `pr-app/src/startup/*`, `pr-shell/src/startup/*`, `pr-shell/src/composition/*` and tests (W8-17a..17c, W8-09b), vs C# `7eaee2a2`/`d92d6aae` | G3 (H1, L1, L6), G7 (M1..M3, L3, L4), G13 (L5), G15 (L2, L3, L6): open |
| [`pr-164.md`](pr-164.md) | 2026-10-23 (git) | 0 H / 4 M / 6 L | PR #164 `integration/wave8-b6`, head `6e45777` vs master `c40b59d` (14 files; W8-17a, W8-17b: startup, cloak, dpi, placement and tests) | G13 (L1, L5, L6), G18 (M1..M3, L2..L4): open; G22 (M4): still to review against master; findings apply to the PR head, which BACKLOG says was still open |
| [`pr-203.md`](pr-203.md) | 2026-10-23 (git) | 0 H / 2 M / 2 L | PR #203 `codex/mut-13h-13k-r2`, head `c905ed0` vs master `c40b59d` (3 files; mutation debt W8-13h..13k; tests and PARITY rows only) | M2 is in G1 / G22, M1 and L2 are in G17: open |

## Data safety and invariants

| File | Date | Findings | Reviewed code | Status of findings |
|---|---|---|---|---|
| [`invariants.md`](invariants.md) | 2026-10-23 (git); re-check 2026-10-23 | 1 H / 3 M / 8 L / 3 product questions (total after the re-check; the H is the new H1) | INV-1..INV-12: `pr-core/src/file_actions/*`, `session/*`, `io/*`, `catalog/review_catalog.rs`, `pr-app/src/coordinators/*`, `pr-platform-win/src/{recycle_bin,recycle_restore,live_operation_registry}.rs`; line numbers at `c40b59d`, C# `7eaee2a2`. The file says INV-1, 2, 8, 10, 11, 12 were not read deeply (see `invariants-storage.md`) | G25 (H1): open, needs an ADR; G5 (M3): open; G6 (M1): decision Q3 accepted, implementation open; G12, G19: open; Q1, Q2, Q3 accepted (G24) |
| [`invariants-storage.md`](invariants-storage.md) | 2026-10-23 | 0 H / 3 M / 1 L (supplement) | `pr-imaging/src/{cache_key.rs,service/decode.rs,service/mod.rs,service/disk.rs}`, preview cache read, config, logging (INV-1/2/8/10/11/12); Rust `809759d`, C# `7eaee2a2` | G26 (M1), G29 (M3), G28 (L1): open; G27 (M2): needs a product decision, not covered by Q1..Q8 |

## Tests

| File | Date | Findings | Reviewed code | Status of findings |
|---|---|---|---|---|
| [`test-quality.md`](test-quality.md) | 2026-10-23 (git) | 0 H / 4 M / 3 L, plus a per-crate table and 30 examples | every `crates/*/tests/**/*.rs` (regex scan, not run; counts may be off by a few units); commit not stated | G17 (M1..M4, L1..L3): open; G23 (L1, L3): open; Q7 accepted (nightly `native` job, not blocking PRs) |
| [`test-strengthening-plan.md`](test-strengthening-plan.md) | 2026-10-23 (git); re-check 2026-10-23 at `65b1597` | 81 proposals over 12 crates after the re-check (a plan, not findings by severity) | reads `test-quality.md` and the other reports; code at `c40b59d`, re-check at `65b1597` | G17, G19, G20: open; the 81 items are a plan, not 81 tests that were run |

## Contracts

| File | Date | Findings | Reviewed code | Status of findings |
|---|---|---|---|---|
| [`w9-contracts.md`](w9-contracts.md) | 2026-10-23 (git) | 1 H / 5 M / 11 L; the file says it fixed 27 places in 13 files | `contracts/W9-E1..E9.md`, `contracts/W9-R1..R11.md`, `docs/WAVE-9-PLAN*.md` vs C# `7eaee2a2` and Rust master `c40b59d` | G21 (H1, L6..L10): partly fixed in #207/#216, scaffold/generator L8 still open; G7 (M1): open; Q5, Q6, Q8 accepted (G24) |

No Wave 10 contract review exists in this folder.

## Relations between files

- `invariants-storage.md` supplements `invariants.md` for the areas the older report marked as not read deeply; they are counted separately. Both are kept.
- `recycle-policy.md`, `recovery-flow.md` and `host-dispatcher.md` are supplements that `BACKLOG.md` folds into groups (G30/G31 new; G17; G11), not replacements.
- `invariants.md` and `main-window-input-overlay-viewing.md` each contain a later re-check section; the numbers above are the ones in the header line of each file.
- `pr-164.md` and `pr-203.md` review PR heads; their findings apply to the PR head, not necessarily to master.
- No file in this folder is an exact duplicate of another, and no later report states that it supersedes an earlier one.
