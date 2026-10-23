# Task on progress

**Updated:** 2026-10-23 (older entries moved to `docs/archive/`; the sections below are the latest ones; where two sections disagree the later date wins, not the later position)

## How to resume (read this first)
1. Read `AGENTS.md`, this file, `docs/PROPOSED-DECISIONS.md` (P-01..P-33: unconfirmed proposals, all acted on),
   `docs/MULTI-AGENT-PLAN.md`, `docs/PARITY.md` ("Wave 4 final status"), `contracts/README.md` ("Wave 5 roster", D45..D58), `tools/autopilot/README.md`.
2. Standing user instruction: run the whole port end to end, auto-fix + auto-merge (squash) every PR, never ask;
   log open questions as a `P-nn` row and act on the recommended option. Replies in Vietnamese.
3. The 15-minute loop was cancelled; restart it with `/loop 15m` + `tools/autopilot/loop-prompt.md`.
4. Permission classifier blocks some `gh pr merge`, `git branch -D`, `git push --delete`, publish commands in
   workflow agents; the user must add the rules from `tools/autopilot/README.md` to `.claude/settings.local.json`
   (agents cannot grant themselves permissions). The main session pushes and opens PRs when a workflow agent refuses.
5. Older entries (Wave 5, Wave 6, Wave 7, Wave 8 B1/B2, with their old "State" and "Next steps") were moved verbatim to `docs/archive/task_on_progress-<wave>.md`; they are history, not current state.

## Open debts (index; details in the archives and the sections below)
- Wave 5: native LibRaw/TurboJPEG DLL tests, RAW corpus tests and the historical mutation survivor set stay unverified (`docs/archive/task_on_progress-wave5.md`; contracts W10-N1..N4; PARITY rows `X-raw-*`).
- Wave 6: mutation verdicts and native evidence for the platform units were not recorded (`docs/archive/task_on_progress-wave6.md`; PARITY section 7).
- Wave 7: `cli-process` golden not run, `cargo-mutants` on the merged tree not run (`docs/archive/task_on_progress-wave7.md`).
- Wave 8: B2 open items D-W8B2-4/5/6 (`docs/archive/task_on_progress-wave8-b1-b2.md`); B3/B4 debts in the two "Wave 8" status sections below.
- Wave 9 (E and R lanes): contracts only, nothing ported (last two sections).

## Wave 8 B4 status (2026-10-22): 23 of 24 units on master, 14f in progress; 106 of 151 units
- **Done on `master` (PRs #107-#141):** B4 contracts + amendments, 13g, 13h, 13i, 13j, 13k (main window core, lifecycle, frame and scroll bar wiring, viewing, windowing), 14a, 14b, 14c, 14d, 14e, 14g (input, drop target, context menu controller, owner-draw session), 15f-15j (overlay), 16b, 16c, 16d, 16f, 16g, 16h, 16i (viewing layers, composer, scroll bars, presentation sink). Also CI shards: `test (core-a|core-b|crates-win|crates-rest|app-unit|0..3)` (#126, #129).
- **In progress:** 14f (context menu feature, `NativePopupMenuHost`, also the two `popup_capture_*` tests 14g could not write); branch `fix/w8-b4-integration` (native tests of the main window on the real tree, 14b/13h RefCell risk, temporary `allow`s). Neither is verified yet.
- **Known gaps (honest):** the native tests of 13g-13k and the 14c E2E tests were each run only against temporary stand-ins; the real-tree run is the integration task above. Mutation evidence is partial for most B4 units (see each PR body); B3 host units: 514 mutants, 0 missed.
- **Open product decisions (contract defaults applied):** 16i cross-thread guard (not ported, hub/sink are `!Send`, PARITY row stays `Different (compile-time)`), 14g panic swallow vs resume, 14d drop effect and inline open, 14f SetForegroundWindow/WM_NULL, 15j dialogs owner (B5/17e), `config.json` lenient vs Rust.
- **Next:** B5 (35 dialog/settings units) and B6 (8 startup/composition/integration units) have no contracts yet; same process as B3/B4 (contracts and compiling stubs first).

## Wave 8 status (2026-10-21): B1, B2 and B3 done; 83 of 151 units
- **Done on `master`:** B1 (35 units), B2 (26 units, PRs #46-#80), B3 (22 of 23 units, PRs #85-#106): Win32 foundation 11a-11h, rendering 12b-12i (device, surface, draw context, cache policy and D2D upload, keep-alive, DirectWrite text), host 13a-13f (frame clocks, swap-chain clock, message loop, dispatcher, wndproc, shell window). The layering PR (#91) reversed the crate edge: `pr-platform-win` depends on `pr-app`; the new thin package `crates/pr-shell` holds `main.rs`, the R0001 spike and `composition`.
- **Not done:** W8-12j (swap-chain latency experiment; starts only if an attended re-measure of the production surface with the 12h keep-warm still shows windowed p95 above 10 ms, someone has to measure on a real screen); B4 (24 units: main window 13g-13k, input/menus 14a-14g, overlay 15f-15j, viewing 16b-16d, 16f-16i), B5 (35 units: dialogs and settings UI), B6 (8 units: startup, composition, integration). No `PhotoReviewRs.exe` window exists yet (W8-20b closes the wave).
- **Verified on master after #106:** `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked --no-fail-fast` 8383 passed, 0 failed, 754 ignored over 420 result lines. CI (`build-test`, `repo-checks`) green on every merged PR. The ignored tests include the native Win32/Direct2D tests (`#[ignore = "native: needs desktop"]`): CI does not run them; they were run by hand on a desktop with `--include-ignored` (WARP, hidden or message-only windows only) for each B3 PR.
- **Mutation evidence:** W8-05g 117 mutants (0 missed), W8-06c 24/24, W8-08a `state.rs` 220 mutants with 3 judged equivalent survivors, 12f 72 mutants (61 caught, 3 timeouts, 3 unviable, 5 missed of which 4 were then covered by a constants test and 1 is equivalent), 13a (one equivalent survivor). Measure with `-j 1`: `-j 3` with one shared `CARGO_TARGET_DIR` reports false MISSED results. NOT done for most other B3 units (11a-11h, 12b-12e, 12g-12i, 13b-13f: only a few hand-applied mutants).
- **Open debts:** PARITY rows still `Partial`: 13b, 13c, 13d (7 C# tests relying on `await`/`SynchronizationContext` retired), 13e (`show` on a visible window not exercised); the separate `unsafe` review of 11b/11c is written (docs/UNSAFE-REVIEW-W8-11.md, by reading only, no defects found, no native verification); no hardware (non-WARP) GPU run; `DeviceResources::synchronize` on a window (swap-chain) target fails with `E_INVALIDARG` (PREMULTIPLIED probe vs IGNORE back buffer; the C# has the same latent problem, ported as is, product decision); the exe icon resource and the manifest embedding belong to W8-20a (`task_dialog`/`ICC_STANDARD_CLASSES` and `icons::load` return 0/false in test executables); the benchmark presenter has no OS memory probe until the shell passes `WindowsMemoryProbe` (`BenchmarkSessionDeps::memory_probe`, W8-09b/W8-17).
- **C# parity:** C# PR #533 (unknown `Operation` in a typed or imported action list is an error) is merged and logged in `docs/SYNC-LEDGER.md`; one `Different` row remains (`D-actions-unknown-operation`: `config.json` load stays lenient in C#, Rust drops the whole actions list).
- **Tooling:** `tools/autopilot/codex-pipeline.sh` (Codex gpt-6-sol medium) and `unit-pr.sh`; B3 was ported with Sonnet subagents in worktrees (one agent per unit group, a lead check of clippy and the native tests on the merged master before each PR). CI formats per package (#101: `cargo fmt --all` fails with os error 206 on Windows). Do not run two pipelines at once (lock: `tools/autopilot/state/pipeline.lock`).
- **Next:** decide with the user whether to write the B4 contracts (same process as B3: contracts and compiling stubs first, port after review).

## Wave 8 batch B5 contracts (2026-10-22, branch `contracts/w8-b5`, based on master `9ba127a`)
- Wired: 35 contracts (`contracts/W8-18a..18v.md`, `W8-19a..19m.md`), the stubs under `crates/pr-platform-win/src/dialogs/` (all bodies `todo!()`), the roster and decisions D-W8B5-1..1011 in `contracts/README.md`, 25 oracle commands (dispatch lines in `Program.cs`, `FolderPicker.cs` in `OracleCs.csproj`), 25 new goldens at C# `d92d6aae` (run twice, byte-identical, no old golden changed), `pr-bench` dependency of `pr-platform-win`, PARITY rows `X-dialogs-*` / `X-settings-ui-*`, proposals P-43..P-45.
- NOT done / NOT run: no unit is ported; no Rust test reads the new goldens yet; no native test was run; the hand tables of W8-18t/18u/18m/18n are not oracle-verified; unplanned post-baseline dialogs (external editors, rename, text prompt) are deferred (P-44) and the production `SecondaryWindowDialogs` is folded into W8-17c (P-43).
- Next: port B5 in the order of D-W8B5-1006 (18d..18g, 18a..18c, 18h..18k, 18l..18n, 18p..18v, 19a..19m, 18o), then W8-17c / 17e.

## Wave 8 batch B6 contracts (2026-10-22, branch `contracts/w8-b6`, based on master `267124c`)
- Wired: 7 contracts (`contracts/W8-17a..17e.md`, `W8-20a.md`, `W8-20b.md`), stubs in `pr-app::startup`, `pr-platform-win::shell_startup`, `pr-shell::startup`, the exe files and UI smoke test stubs of `pr-shell` (all bodies `todo!()`), the roster and decisions D-W8B6-1..54 and 101..107 in `contracts/README.md`, 7 oracle commands (inputs in `tests/golden-inputs/`, goldens in `tests/golden/`, each generated twice and byte-identical), PARITY rows `X-startup-*`, `X-exe-*`, `X-ui-smoke`, proposals P-46 and P-47.
- NOT done / NOT run: no unit is ported; no Rust test reads the new goldens yet; no native test was run; the icon sync (`tools/sync-from-csharp.ps1`), the plan edge corrections in `docs/WAVE-8-PLAN.md` and the 09b `production_graph_builder_with` are follow-ups (D-W8B6-106); W8-09b is not contracted, so the 17d/17e graph is a `todo!()` seam.
- Next: contract W8-09b, then port B6 in the order 17a, 17b, 17c (after B5 dialogs), 17d, 17e, 20a, 20b (S0 last).

## Wave 9 sync contracts (EXTERNAL-EDITORS UI), machine A, 2026-10-22 (branch `contracts/w9-edt`)
- Wired: 9 contracts (`contracts/W9-E1..E9.md`), `docs/WAVE-9-PLAN-edt.md`, stubs in `pr-core` settings, `pr-app` menus/input/presenters/viewmodels and `pr-platform-win` dialogs/shell, oracle commands `external-editor-rules` (77), `external-editor-menu` (18), `open-in-default-editor-shortcut` (13) with goldens at `7eaee2a2` (each run twice, byte-identical), roster and decisions D-W9E-1..12 in `contracts/README.md`, PARITY rows `X-w9e1..9`, P-48.
- NOT done / NOT run: no unit is ported; no oracle for E2..E5, E8, E9; no native test, mutation run or desktop run; the spec baseline was not moved and existing goldens were not regenerated (the new commit changes `settings-*`, `shortcut-canonical`, `secondary-window-supports`, `settings-page-*`: logged, not touched); the en/vi catalog (34 keys) is not synced, so E2, E4, E6, E8 text tests are blocked; E2, E3, E4, E8 wait for W8-18o/18p/18q/19f..19m.
- Existing golden left untouched: `shortcut-canonical` (test `shortcut_defaults_match_golden` in `crates/pr-core/tests/settings_shortcuts.rs`) lacks `OpenInDefaultEditor`, so that one test is `#[ignore]`d until the baseline move regenerates it.
- Next: lead decides the baseline move, the catalog sync and the order against the rename lane; port E1 first.

## Wave 9 sync contracts (FEAT-RENAME), machine A, 2026-10-23 (branch `contracts/w9-ren`)
- Wired: 11 contracts (`contracts/W9-R1..R11.md`), `docs/WAVE-9-PLAN-ren.md`, stubs in `pr-core` (file_actions rename/case_only_rename/rename_service, catalog rename, FileSystem and dialog abstractions), `pr-imaging` (cache key), `pr-app` (controller, presenter, zoom detail, view-model rename and prompt) and `pr-platform-win` (rename prompt dialog, secondary-window port, shell input), oracle command `w9-r1-rename-policy` (44 cases, golden at `7eaee2a2`, two runs byte-identical; the input stays in `tests/golden-inputs/staged/` and the C# command is compiled only with `-p:W9RenameBaseline=true`, which `tools/oracle.ps1` now passes by itself when the C# source has `RenameNamePolicy`), roster and decisions D-W9R-1..14 in `contracts/README.md`, PARITY rows `X-w9r1..8`, P-49. Rebased on master after lane E merged (`RenameFile` = 31 follows `OpenInDefaultEditor` = 30).
- Gates (CARGO_TARGET_DIR in the worktree): rustfmt per touched file clean, `cargo clippy --workspace --all-targets --locked -- -D warnings` clean, `cargo test --workspace --locked --no-fail-fast`: 9494 passed, 0 failed, 1084 ignored.
- NOT done / NOT run: no unit is ported; R2..R11 have no oracle (fake or native tests); no native test, mutation run or desktop run; no Rust test reads the `w9-r1-rename-policy` golden yet; the spec baseline in `docs/SYNC-LEDGER.md` was not moved and no existing golden was regenerated (settings goldens stay at `d92d6aae`, R7 updates the rename expectations by hand); the en/vi catalog keys of the rename dialog and menu are not synced; `FileSystem` fake in `pr-test-support` is not yet changed for case-only moves (owned by R2).
- Next: lead decides the ledger advance and the catalog sync; port R1 first, then R2, R4, R7; the golden generator moves `staged/w9-r1-rename-policy.json` up when the baseline moves.
## Review INVARIANTS, machine C, 2026-10-23 (tái kiểm)
- Rust reviewed: 85b8bf0; C# oracle read-only: 7eaee2a2. Claim REVIEW-INVARIANTS held by C. Docs-only, không sửa Rust.
- Bổ sung H1: retry nhóm Recycle thiếu kiểm lại fingerprint sau Prepared, có nhánh permanent-delete; sửa diễn giải INV-5/APP-03 và live marker của M3.
- Kết quả: suy luận từ mã, chưa chạy thử. Không chạy fmt/clippy/test/oracle/mutation; không chạm Recycle Bin. PR docs sẽ bật squash auto-merge; nhả claim sau khi xác nhận merged.
- Tiếp: REVIEW-BACKLOG, rồi REVIEW-PRS, B4, W9-CONTRACTS và TEST-PLAN theo lệnh người dùng.
## Review BACKLOG, machine C, 2026-10-23 (tái kiểm)
- REVIEW-INVARIANTS #212 đã merged tại e25f8bf; content diff đã xác nhận, claim nhả; worktree và branch local đã dọn, remote branch đã được GitHub xoá tự động (lệnh delete báo ref không tồn tại; không thử lại).
- Claim REVIEW-BACKLOG bởi C. Bổ sung G25 rủi ro permanent-delete từ invariants H1, prompt/test/phạm vi riêng; cập nhật Q1..Q8 theo P-52/#211; bỏ đề nghị che test đỏ bằng ignore và cảnh báo blanket native run trái quy tắc bin.
- Docs-only; suy luận từ mã, chưa chạy thử. Không chạy Cargo/oracle/mutation. Tiếp theo REVIEW-PRS; nhả claim sau merge PR docs này.
## Review PRS, machine C, 2026-10-23 (tái kiểm)
- REVIEW-BACKLOG #213 đã merged tại 45fdb48; content diff sạch, claim nhả, branch local/worktree đã dọn. REVIEW-PRS claim bởi C.
- PR mở có mã: #164 head 6e45777 không đổi; đọc lại diff/contract/C# 7eaee2a2. Báo cáo giữ 0 cao / 4 trung bình / 6 thấp trên head, phân biệt master đã có #177/#179, không đăng lại toàn bộ phát hiện cũ như lỗi mới.
- Docs-only; mọi phát hiện là suy luận từ mã, chưa chạy thử. Không chạy Cargo/oracle/mutation, không sửa mã. Tiếp REVIEW-B4 sau merge PR docs này.
## Review B4, machine C, 2026-10-23 (tái kiểm)
- REVIEW-PRS #214 merged tại 48f96aa; content diff sạch, claim nhả, branch local/worktree dọn. Nhận xét đã đăng #164; head PR không đổi. REVIEW-B4 claim bởi C.
- Giữ 3 cao / 2 trung bình / 5 thấp, không lỗi mới. Bổ sung đường H1/H2/H3, đọc vùng layout/hit-test, quét chú thích unsafe (54 file/38 dòng block), test inventory 121/82 ignore; sửa đường file sai trong prompt G1/G2 của BACKLOG.
- Mọi phát hiện là suy luận từ mã, chưa chạy thử. Không Cargo/oracle/mutation/native UI. Tiếp W9-CONTRACTS sau merge.
## Review W9-CONTRACTS, machine C, 2026-10-23 (tái kiểm)
- REVIEW-B4 #215 merged tại f795e18; content diff sạch, claim nhả, branch local/worktree dọn. REVIEW-W9-CONTRACTS claim bởi C.
- Đối chiếu E1..E9/R1..R11 với C# 7eaee2a2 và crates. 254 tên test inventory tìm thấy, không phải test runner. Sửa docs E1/E3/E5/R1/R6/R7 và hai plan: baseline/golden, Option/Localizer, no-save khi full, empty-map rearm, trạng thái pages. Không sửa Rust/assets/goldens.
- Tổng lịch sử báo cáo 1 cao / 5 trung bình / 11 thấp gồm mục đã sửa; không phát hiện mới trùng. Mọi phát hiện là suy luận từ mã, chưa chạy thử. Không Cargo/oracle/mutation/native. Tiếp REVIEW-TEST-PLAN sau merge.
## Review TEST-PLAN và bàn giao 6 việc, machine C, 2026-10-23
- REVIEW-W9-CONTRACTS #216 merged tại 65b1597; content diff sạch, claim nhả, branch local/worktree dọn. REVIEW-TEST-PLAN claim bởi C cho PR docs cuối.
- Các việc 1-5 đã merge theo đúng thứ tự: INVARIANTS #212, BACKLOG #213, PRS #214 (comment #164 issuecomment-6362445502), B4 #215, W9-CONTRACTS #216. TEST-PLAN: nội dung review hoàn tất, PR docs cuối dùng auto squash; trạng thái merge được xác minh trực tiếp qua GitHub khi bàn giao.
- 81 mục test/12 crate, mỗi crate tối đa 14 mục (core T4 hai matrix vẫn tối đa 15 test); thêm regression G25/durability, sửa kỳ vọng stale-preview/LibRaw248/overflow và constructor retry; tách perf1e400 chờ oracle. Không biến đề xuất thành tuyên bố test đã chạy.
- Mọi phát hiện: suy luận từ mã, chưa chạy thử. Không sửa Rust, assets, fixture/golden, dependency, hay repo C#. Không chạy cargo fmt/clippy/test/oracle/mutation/native bin. git diff --check, tên/dòng nguồn, inventory và UTF-8 không BOM/LF được kiểm. Không có tool ccd_pr/set_monitor trong phiên; auto-merge dùng gh như yêu cầu.
- Công việc tiếp theo là implementation theo BACKLOG/P-52/G25 của máy sở hữu, ngoài phạm vi session chỉ đọc. Claim cuối nhả và worktree/branch của session dọn sau merge; không có target build của session này.
## Review INVARIANTS-STORAGE, machine C, 2026-10-23
- Sáu PR review trước #212..#217 đều merged; master khởi điểm 809759d sạch. PR mã đang mở chỉ #164 head 6e45777 không đổi, nên không đăng lại nhận xét. Claim REVIEW-INVARIANTS-STORAGE bởi C cho phần INV-1/2/8/10/11/12 trước đây chưa đọc sâu.
- Báo cáo mới docs/reviews/invariants-storage.md: 0 cao / 3 trung bình / 1 thấp (suy luận từ mã, chưa chạy thử). G26 cache stale rename sau writer mới; G27 best-effort newer config backup có thể mất unknown fields; G29 cache đọc/cấp phát trước guard và phân loại I/O OOM; G28 log hai lần partial-write failure.
- Đã đối chiếu C# 7eaee2a2 và test hiện có; M1/M2 thừa kế oracle, M3 contract từng cho phép whole-file nhưng guard resource chưa tương đương, L1 test staging chưa bảo vệ short I/O. Không sửa Rust/assets/goldens, không chạy Cargo/test/oracle/mutation/native bin, không tạo file lớn/ép OOM.
- Bổ sung BACKLOG G26..G29 với prompt/phạm vi/test/lệnh theo package; contracts/W4-08b.md ghi rõ khác biệt guard trước allocation; invariants.md dẫn báo cáo mới. Quyết định mới của G27 chưa thuộc P-52; giữ cần quyết định, không tự sửa production.
- Kiểm docs diff/encoding trước PR; bật squash auto-merge qua gh, nhả claim và dọn branch/worktree sau merge. Không có target build của session.

## 2026-10-23 — Machine C REVIEW-RECOVERY-FLOW

Review tĩnh tại bb5c02a, oracle 7eaee2a2: 0 cao / 1 trung bình (test) / 0 thấp. Báo cáo recovery-flow.md; nối M1 vào G17, không đếm lại G25/G7/G3. Phân biệt presenter đã port với 96 todo native; không yêu cầu catch panic UI trái R0003. Không sửa Rust, không chạy Cargo/native; suy luận từ mã, chưa chạy thử. Tiếp theo: PR docs, auto squash, nhả claim sau merge; tiếp tục vùng host/dialog khác.
