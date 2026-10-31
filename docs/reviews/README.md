# docs/reviews index

Static review reports written by Machine C (cloud session) on 2026-10-23, plus the consolidated [`BACKLOG.md`](BACKLOG.md). The report files are evidence and are not edited; this index only points to them.

How to read this page:

- **Date**: the date written in the file's own header where there is one; otherwise "2026-10-23 (git)" = the date the file was first committed (every file here was added on 2026-10-23). The commit hashes are the ones the file itself states; "commit not stated" means the file gives none.
- **Findings**: copied from the file's own counts line (H = high, M = medium, L = low). Where a file has a re-check section, that is noted.
- **Status of findings**: derived ONLY from the status table "Trạng thái xử lý tại master b5d1327 (2026-10-23)" and the Q1..Q8 decision table (G24) in `BACKLOG.md`. If `BACKLOG.md` does not say, the cell says "see file". `BACKLOG.md` is a 2026-10-23 snapshot and may be stale; check `task_on_progress.md` and `docs/PARITY.md` for newer state.
- Every finding in every report is inferred from the code and was NOT run (each file says so). `BACKLOG.md` marks no group as closed (G21 is "partly fixed", G24 decisions are "decided"), so no row below says closed.
- G1..G31 are the backlog groups in `BACKLOG.md` section 1; Q1..Q8 are the product decisions in G24 (accepted as P-52 in `docs/PROPOSED-DECISIONS.md`).

There are 18 files here: 17 reports and `BACKLOG.md`. There is no separate report for Wave 10 contracts; the only contract review is the Wave 9 one. No two reports are exact duplicates and none supersedes another (see "Relations between files").

## Consolidated backlog

| File | Date | What it covers | Status |
|---|---|---|---|
| [`BACKLOG.md`](BACKLOG.md) | 2026-10-23 | Merges the findings of `autopilot`, `dialogs-base`, `native-dialog`, `render-painter`, `settings-ui`, `startup`, `test-quality`, `invariants`, `main-window-input-overlay-viewing`, `w9-contracts`, `pr-164`, `pr-203` into groups G1..G24; section 5 (re-synthesis) adds G25; section 6 adds `invariants-storage` (G26..G29); appended sections fold in `recovery-flow` (into G17), `host-dispatcher` (into G11) and `recycle-policy` (G30, G31); the last section is the status table at master b5d1327. Holds the Codex prompts per group. `test-strengthening-plan.md` is pointed to, not merged line by line. | The source of the Status column below |
| [`ST-BACKLOG-NOW-2026-11-01.md`](ST-BACKLOG-NOW-2026-11-01.md) | 2026-11-01 | Current status of G1..G31 at master `71d9ea76` (8 fixed, 14 partial, 7 open, 1 needs decision) and the top 8 open items by data risk with file scope and tests to add. | Supersedes the 2026-10-23 status table in `BACKLOG.md` (static, not run) |

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
| [`settings-ui.md`](settings-ui.md) | 2026-10-23 (git) | 1 H / 3 M / 5 L | `pr-platform-win/src/dialogs/settings/` and `tests/settings_*.rs` (W8-19a..19i), vs C# `7eaee2a2` | G4 (H1), G9 (M1..M3, L1..L4): open; L5 is in G17 / G23: open |
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
