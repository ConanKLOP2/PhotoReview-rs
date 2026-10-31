# ST-DONECHECK 2026-10-31: kiểm toán độc lập các hàng PARITY đang Done dựa trên báo cáo mutation/native

Status: báo cáo kiểm toán tĩnh (lane `st-donecheck`, nhánh `feat/st-donecheck`, từ origin/master `5f26d474`). CHỈ ĐỌC: không build, không chạy
cargo-mutants, không sửa `docs/PARITY.md`. Mọi điều dưới đây là điều tôi đã đọc trong `docs/PARITY.md`, `docs/reviews/*.md` và vài file test;
điều suy luận được ghi rõ "suy luận, chưa chạy". Bối cảnh: [AUDIT-MUTATION-FLAG](AUDIT-MUTATION-FLAG-2026-10-30.md).

## 0. Tóm tắt

| Kết luận | Số hàng | Hàng |
|---|---:|---|
| GIỮ Done | 17 nhóm | X-host-ui-dispatcher, D-05, INV-8, X-input-drop-target, X-input-click-counter, P-04, F-10/INV-4/INV-6 (undo), X-overlay-button, X-viewing-pure, X-startup-options, S-64/S-65/INV-11 (store.rs), I-05, I-11 (order..policy), W4-14d, W4-16a, X-w9r1/r2/r4 |
| NGHI NGỜ (nên sửa chữ hoặc làm thêm một việc nhỏ) | 9 | RV-C05, W6-01b, X-w9r3, X-w9r5, X-host-frame-clock, X-overlay-panels, X-overlay-status-toolbar, X-overlay-autohide, INV-6 matrices (`file_actions_matrix`) |
| NÊN HẠ về Partial | 3 | X-render-surface (L589), X-render-draw-context (L590), AR16 RemovePaths (L61) |

Không hàng nào trong danh sách ưu tiên (U-initial-viewport, X-benchmark-presenter, X-dialogs-*, X-input-native, X-input-message-handlers,
X-input-viewport-surface, X-startup-composition-root, X-host-shell-window-messages) hiện còn là Done: tất cả đã Partial (L572, L640-L649, L617-L620,
L656, L665, L602). Chúng KHÔNG cần hạ thêm; chỉ cần giữ Partial (xem mục 3).

Các phát hiện xuyên suốt:

1. Không tìm thấy hàng Done nào còn dựa trên một lần chạy "void" (cờ libtest sai) mà chưa được chạy lại. Các báo cáo dùng cờ sai
   (W2-FS-EVIDENCE, CL-FSFLUSH, CL-HOST2, CL-STARTUP, CL-REST2, CL-P01, CL-MAINWIN2) đã được chạy lại bằng dạng đúng
   (CL-PHYSRERUN/CX-PHYS2/CL-PHYS3/CL-PHYS4, CL-FSRERUN, CL-HOSTRERUN/2, CL-HOST3, CL-STARTRERUN, CL-REST2RERUN, CL-W6RERUN, CL-W2RERUN) hoặc hàng đã Partial.
2. Chính các báo cáo chạy lại xác nhận dấu vân tay "void" của lần cũ: CL-FSFLUSH 15 caught/0 missed, chạy lại đúng cho 8 caught/3 unviable/6 missed
   (CL-FSRERUN); CL-HOST2 90 mutants/0 missed, chạy lại đúng thấy 3 missed + 2 timeout trong 40 mutant đầu (CL-HOSTRERUN); CL-STARTUP "21 MISSED rồi 0"
   thực ra là 45 MISSED khi thiếu binary native (CL-STARTRERUN).
3. Hai chính sách timeout khác nhau cùng tồn tại. Báo cáo cũ (W6-PLATFORM-EVIDENCE, W9-RENAME-MUTATION-2/3, W3-01) tính hang là "detected";
   báo cáo mới (CX-JRNL, CL-JRNL3, CL-W2RERUN, CL-HOSTRERUN) ghi "Timeouts are NOT counted as caught". AGENTS.md dòng 131 chỉ yêu cầu ghi số timeout. Các
   hàng Done dựa trên báo cáo cũ có "0 missed" nhưng kèm 1-2 hang: P-04, W6-01b, X-w9r1, X-w9r3, X-w9r5.
4. Nhiều killer quan trọng là `#[ignore]` và `native-nightly.yml` chỉ chạy `w2_fs_flush_killers` trong số killer đã kiểm: không có
   `w2_fsrerun_killers`, `host_ui_dispatcher_native`, `w8_host3_killers`, `w8_hostrerun*_killers`, `shell_input_drop` (phần ignore), `w8_inputdone_killers`
   (grep `.github/workflows/*.yml`). Chúng chỉ được bảo vệ khi có người chạy tay `--include-ignored`.
5. Một số ô trong PARITY đã lỗi thời so với báo cáo mới (không phải thiếu bằng chứng): L40 "427 of 427" và "replace_with_retry không chạy lại"; L44 "17 mutants
   ... 0 missed"; L688/L686 "3/199, 70 not run" (CL-RENAME đã ghi là cũ); L597 "not on master when written".

## 1. Hàng ưu tiên đang Done

### X-host-ui-dispatcher (L600): GIỮ Done (kèm 3 ghi chú)

Bằng chứng: [CL-HOST3](CL-HOST3-2026-10-31.md) + [CL-HOSTRERUN](CL-HOSTRERUN-2026-10-30.md) + [CL-HOSTRERUN2](CL-HOSTRERUN2-2026-10-31.md).

- (1) Có thật: file tồn tại, ghi lệnh đầy đủ với `--cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`, preflight
  "running 9/22/2/4 tests" = 37, nhật ký mutant có `running 9 tests`/`22 tests`. `crates/pr-platform-win/tests/w8_host3_killers.rs` có đúng 12 `#[test]`, cả 12
  `#[ignore]`, 1 `unsafe` có `SAFETY`. Số học 90 = 37 + 7 + 7 + 39 khớp trong CL-HOST3; run 2: 39 = 36 caught + 3 unviable + 0 missed + 0 timeout khớp.
- (2) Cờ: dạng đúng, không void. CL-HOSTRERUN đã chủ động phát hiện và hạ hàng khi số cũ của CL-HOST2 không tái lập. Timeout KHÔNG tính là caught
  (CL-HOSTRERUN: "treated as NOT detected"); các mutant `-> true` hang bị bắt bằng killer chạy riêng (killers-alone) chứ không phải bằng timeout.
- (3) Khoảng trống còn lại:
  - Số "37 mutant dòng < 241" trong CL-HOST3 không đối chiếu được với CL-HOSTRERUN, nơi 40 mutant đã chạy (26 caught, 3 missed, 2 timeout, 9 unviable)
    và dòng 239:9/241:34 nằm ở ranh giới. Chênh 2-3 mutant chưa được giải thích; phần này suy luận, chưa chạy.
  - Phán quyết cuối của 46 mutant (dòng 246+) dựa trên "union" của các test: ở run 1 (cả bộ) `284:9` TIMEOUT; chỉ bộ killer mới bắt nhanh. Hợp lệ nhưng
    nghĩa là bộ test cũ vẫn treo khi mutant sống (helper-thread). Không phải lỗi.
  - Killer đều native `#[ignore]`, không có trong CI/nightly (mục 0.4). `messages.rs`, `swap_chain_clock.rs`, `message_loop.rs` chưa chạy mutation; không thuộc hàng này.
  - Văn bản ô trạng thái L600 còn ba lớp lịch sử mâu thuẫn ("stays Partial" của CL-HOSTRERUN2 rồi "raised again"). Nên rút gọn.

### D-05 (L40): GIỮ Done (sửa chữ)

Bằng chứng: [CL-FSRERUN](CL-FSRERUN-2026-10-30.md), [CL-FSFLUSH](CL-FSFLUSH-2026-10-30.md), [CL-PHYSRERUN](CL-PHYSRERUN-2026-10-30.md), [CX-PHYS2](CX-PHYS2-2026-10-31.md).

- (1) CL-FSRERUN: lệnh đầy đủ dùng `-p pr-core --test-package pr-platform-win` và `--cargo-test-arg=-- ...` đúng; preflight "running 9 tests"; log mutant có
  `8 passed; 1 failed`. 17 tested = 8 caught + 3 unviable + 6 missed, khớp. 6 missed đều thuộc `open_read_shared*` (INV-8), không thuộc luồng ghi/flush.
  7 hand mutant (cờ WRITE_THROUGH, nhánh `durable`, SEQUENTIAL_SCAN, `open_append`) được áp dụng lại và đều caught. `w2_fs_flush_killers.rs`: 9 test, cả 9 `#[ignore]`.
- (2) Cờ: lần CL-FSFLUSH cũ (15 caught/0 missed, killers binary riêng, không ghi lệnh) có dấu vân tay void. Lần chạy lại thay thế nó; CL-PHYSRERUN
  (35 mutant: 30 caught, 5 missed đều giải thích) và CX-PHYS2 (`replace_with_retry` 11/11 caught, log có `running 2 tests`) cũng đúng dạng.
- (3) Ô L40 vẫn còn câu "`physical.rs` 427 of 427" và nhận định `replace_with_retry` mutants "all caught" từ W2-FS (void); cuối ô lại nói `replace_with_retry` chưa chạy lại.
  Thực tế CX-PHYS2 đã chạy 11/11 hợp lệ. Nên thay câu 427/427 bằng CX-PHYS2. Giới hạn còn lại (đúng như ô ghi): lệnh `File::sync_all()` chỉ được gián tiếp kiểm qua bộ đếm.
  Cùng giới hạn: ba lỗi `physical.rs` ngoài phạm vi D-05 (listing, matcher, create_directory) vẫn Partial ở X-physical-fs/AR16, đúng.

### INV-8 (L44): GIỮ Done (sửa chữ, 2 ghi chú)

- Ô còn câu "mutation on `open_read_shared*` (17 mutants with D-05 in the same run) 0 missed" — bị CL-FSRERUN bác bỏ (6 missed, 1 thật). Phần sau của cùng ô đã nêu đúng; nên xóa câu cũ.
- Mutant `|` -> `&` ở 1143:48 được `w2_fsrerun_killers.rs` (1 test, `#[ignore]`) giết, chứng minh bằng áp tay (0 passed, 1 failed), KHÔNG bằng một lần chạy cargo-mutants lại.
  Các mutant `buffer_size > 1` (`==`, `<`, `>=`) được xếp "tương đương hành vi" vì chỉ đổi BufReader/File; chấp nhận được (suy luận), nhưng đó là lập luận, không có test.
- Mâu thuẫn trong CL-FSFLUSH: phần Verdicts nói SequentialScan được chứng minh "trên cả hai điểm mở", phần Known limit nói điểm thứ hai (`PhysicalSourceReader`) không có spy. Ô PARITY
  nêu đúng giới hạn; chỉ báo cáo gốc mâu thuẫn.

### RV-C05 (L41): NGHI NGỜ (nhẹ)

- Test `write_all_text_atomic_rename_fails_leaves_no_temp_file` có thật (`crates/pr-core/tests/io_physical.rs:1154`) và `physical_non_durable_write_failed_rename_cleans_up_temp_file`
  (`:1182`); CX-PHYS2 báo toàn bộ io_physical 64 test pass (preflight 95 test).
- Chứng minh "bỏ dọn temp thì test fail" là "manual mutation" ở W2-FS-EVIDENCE (2026-10-29), cùng báo cáo có các lượt native void; không có bản chạy lại. CL-PHYSRERUN chỉ chạy lại `write_all_text_atomic(_observed)`
  `-> Ok(())` và `delete !` (ba mutant), không chứng minh trực tiếp mutant "xóa dòng dọn temp" (cargo-mutants không sinh mutant xóa câu lệnh). CX-PHYS2 nói rõ "atomic-write mutants are not rerun here".
- Đề xuất rẻ: một hand mutant (bỏ dọn temp ở nhánh rename lỗi), chạy `io_physical` đúng 2 test trên; nếu fail thì xóa chữ "manual mutation 2026-10-29" thay bằng bằng chứng mới.

### X-input-drop-target (L622): GIỮ Done (2 ghi chú)

[CL-INPUTDONE](CL-INPUTDONE-2026-10-30.md): file tự loại bỏ 3 giờ chạy đầu vì đúng lỗi cờ ("every mutant is reported caught"), dạng cuối đúng `--cargo-test-arg=-- --cargo-test-arg=--include-ignored ...`.
Số học khớp: 47 = 39 caught + 1 unviable + 7 missed; 7 missed = 6 giết bằng `w8_inputdone_killers.rs` + 1 tương đương (`files` `||` -> `&&`, lập luận `global_lock` không trả null); chạy lại 14 mutant bằng killers riêng: 13 caught, 1 missed
(`register -> Ok(())`, chỉ test native `register_*` giết, đã bị giết ở lượt cả file). Ghi chú: (a) báo cáo không ghi dòng `running N tests`/preflight, chỉ dặn "kiểm một log"; (b) các test đăng ký COM là `#[ignore]` và không có trong CI/nightly;
(c) file test thực có 7 `#[test]` top-level + 1 trong `mod` và 4-5 `#[ignore]` (báo cáo nói 8 test, 3 ignore): chênh nhỏ, không đổi kết luận. Ô L622 vẫn mang đoạn "Mutation debt (machine B ...)" đã bị CL-INPUTDONE sửa; ô đã ghi câu "The earlier ... judgement ... was wrong", chấp nhận được.

### X-input-click-counter (L616): GIỮ Done

[CL-INPUT](CL-INPUT-2026-10-30.md): `click_counter.rs` toàn file 20/20 caught, test thuần (`shell_input_basics`, không `#[ignore]`, không cờ libtest) nên không dính lỗi cờ. Ô PARITY lại trích số 2026-10-23 "machine B"
(18 caught + 2 timeout rồi chạy lại) mà không có file báo cáo; nên thay bằng CL-INPUT (20/20).

### X-input-native, -message-handlers, -viewport-surface, -main-window, -shell-input (L617-L621): đang Partial, giữ Partial

CL-INPUTDONE đã hạ đúng (58/92, 27/42, 34/126); phần Touchpad của `current_wheel_device` không quan sát được. Không đề xuất đổi.

### U-initial-viewport (L665), X-benchmark-presenter (L572), X-dialogs-* (L640-L649), X-startup-composition-root (L656): đang Partial, giữ Partial

[CL-REST2RERUN](CL-REST2RERUN-2026-10-31.md): `initial_viewport.rs` 41/41 caught, preflight 22 test, 41/41 log có `running 22 tests`; `adapters.rs` 34 = 22 + 7 + 5 (CL-REST2 cũ "0 missed" sai, 7 missed thật, đã đóng bằng CX-TEXTS);
`benchmark.rs` 5 = 2 + 1 + 2. Không hàng nào được nâng; đúng. [CL-STARTRERUN](CL-STARTRERUN-2026-10-30.md): 7 survivor của `secondary_dialogs.rs` là thật, X-dialogs-secondary-port giữ Partial.

## 2. Các hàng Done còn lại có trích báo cáo

| Hàng (dòng) | Kết luận | Lý do cụ thể |
|---|---|---|
| P-04 (L392) | GIỮ | `instance_scope.rs` 67 = 54 + 12 unviable + 1 hang (`release_except` `delete !`); `instance_keys.rs` 34 = 22 + 11 + 1 timeout (`:160:20` `+=` -> `*=`). CL-SEC01 liệt kê 11 survivor (session 1 môi trường, nhánh `cfg(not(windows))`, arm chết, `Token::drop`, guard phòng thủ), chạy test thuần (`--lib`) nên không dính cờ. Ghi chú: 2 hang tính "detected" (chính sách cũ); phiên khác 1 chưa thử; chạy mutex native là `#[ignore]` |
| W6-01b (L59) | NGHI NGỜ (nhẹ) | "mutation complete" 30 = 23 + 3 (đã giết) + 2 unviable + 2 hang tính detected (vòng lặp `percent_decode`, W6-PLATFORM-EVIDENCE dòng 47). Không phải hỏng, nhưng 2 mutant chỉ "bị bắt" bởi đồng hồ; theo chính sách mới là chưa bắt. Một killer assert (giới hạn lặp) là đủ |
| F-10/INV-4/INV-6 undo (L86) | GIỮ | [CL-UNDO2](CL-UNDO2-2026-10-30.md): 199 mutant duy nhất = 180 caught + 19 unviable, 0 missed, 0 timeout; đối chiếu: nhóm A 46 caught + nhóm B 157 caught + 19 unviable = 222 lượt, trừ 23 trùng = 199 mutant duy nhất, 180 caught + 19 unviable. Test thuần (fake), không cờ libtest; không ghi dòng `running N` nhưng không có rủi ro void. Chưa chạy: tích hợp Recycle Bin native (không thuộc `undo.rs`) |
| F-18, F-06 retry, INV-4/6 retry (L87), F-01..06 service (L83), F-21 (L89) | GIỮ | L87/L83 không trích mutation, dựa vào golden + test; L89 dùng `partial_cleanup.rs` 14 mutant (13 caught, 1 tương đương) + CORE-LANE. Không có dấu hiệu void |
| INV-6 matrices (L90, `file_actions_matrix`) | NGHI NGỜ (nhẹ) | "Done (tests only)" với 3 hand mutant. Phụ thuộc `journal.rs` mà mutation hiện chỉ 64/179 caught (CL-JRNL3), 105 chưa chạy, 4 timeout; các hàng journal đã Partial (CX-JRNL/CX-JRNL2). Ma trận này chứng minh hành vi đầu-cuối, nhưng ô không nên viết như đóng INV-6 khi phần journal còn Partial. Đề xuất ghi chú tham chiếu CL-JRNL3 |
| S-64/S-65/INV-11/D-02/X-config-import (L33-L39) | GIỮ | CORE-LANE: `store.rs` 150 = 134 caught + 3 tương đương + 13 unviable, 0 missed (khớp: 134 + 3 + 13 = 150); không cờ native |
| I-05 (L94), I-11 order..policy (L95), W4-14d (L482), W4-16a (L484) | GIỮ | CL-APPMUT/CX-IMAGING2: mutation chạy bằng test thường (`--baseline skip --timeout 90`), cờ `--include-ignored` chỉ dùng cho lượt chức năng, không nằm trong lệnh mutation. 33/33, 235/235, 24 mutant (20 caught, 4 unviable) khớp |
| X-overlay-button (L545) | GIỮ | CORE-LANE: 56 + 15 = 71/71 (65 caught, 6 unviable), khớp với L545 |
| X-viewing-pure (L550) | GIỮ | [CL-VIEWING](CL-VIEWING-2026-10-30.md): 33 + 55 + 23 + 10 = 121 khớp con số "92/121" của verifier cũ; không cờ native |
| X-startup-options (L660) | GIỮ | golden `options_match_csharp_golden`; "rerun OK" từ CL-STARTRERUN: lượt a ban đầu 45/62 MISSED khi thiếu binary run native, 46/46 caught khi thêm; chỉ phần "options only" |
| X-w9r1, X-w9r2, X-w9r4 (L684, L685, L687) | GIỮ | CL-RENAME: 88 tested = 77 + 11 unviable, 0 missed; MUTATION-2 `rename.rs`+`rename_service.rs`+`case_only_rename.rs` 134 (110 caught, 7 missed đã giết, 15 unviable, 2 hang). Lưu ý `rename.rs` chỉ chạy lại 21/97; 2 hang `check_utf16` tính detected; L687 viết "0 missed" nhưng MUTATION-3 `catalog/rename.rs` ghi 1 missed (đã giải thích/giết ở `rename_entries_identity_map...`) |
| X-w9r3 (L686), X-w9r5 (L688) | NGHI NGỜ (nhẹ) | ô vẫn viết "10/80, 70 not run" và "3/199, 196 not run", CL-RENAME nói là cũ vì các file đã chạy toàn bộ ở MUTATION-2/3 (undo.rs 92 = 65 + 15 + 11 + 1 hang; actions.rs 100 = 72 + 14 + 14); survivor đã giết bằng `rename_mutation_killers_2` (chạy riêng: 15/17 và 13/21, phần còn lại "neighbour đã bị test cũ bắt", không tự chứng minh). Ô chưa cập nhật, "No full mutation or native UI verdict" ở L688. Sửa chữ, không hạ |
| X-host-frame-clock (L597) | NGHI NGỜ (nhẹ) | "union of 4 runs ... all mutants caught" không có file báo cáo, không có số tested/caught/unviable, ô còn câu "tests need W8-11f WaitableTimer, not on master when written". Không có bằng chứng kiểm chứng được; test có 1 native `#[ignore]` |
| X-overlay-panels (L548), X-overlay-status-toolbar (L549) | NGHI NGỜ | Done với ghi chú "mutation run partial, see the PR": không có số, không có file báo cáo trong repo. Đề xuất hạ nếu người dùng không chấp nhận Done thiếu mutation, hoặc chạy panel/capture_badge/folder_info/status/toolbar |
| X-overlay-autohide (L637) | NGHI NGỜ (nhẹ) | chỉ ghi "mutation-checked", không số. Cần liên kết báo cáo hoặc số |
| X-render-surface (L589) | NÊN HẠ | chính ô ghi "PARTIAL": 49/91 mutant (42 caught, 4 unviable, 3 MISSED và KHÔNG được giết: `:156` `is_warp -> true`, `:203`, `:220` guard mất thiết bị); 42 mutant chưa chạy. Một hàng có 3 survivor thật, một nửa số mutant chưa chạy, không nên là Done. Số từ 2026-10-23 trước khi có cảnh báo cờ nhưng có survivor nên không phải void |
| X-render-draw-context (L590) | NÊN HẠ | ô ghi "not a full mutation verdict": ≥42/109, 4 MISSED không giết (`draw_tile :603:32/:604:34` chưa chứng minh tương đương, `draw_image :363:34/:388:40` sống sau test mới), "about 60 more" chưa chạy; ô cũng ghi chưa chạy test `TiledImage_Panorama30000x2000` |
| X-render-device-create-loss/-resources (L587, L588), X-gpu-cache-policy (L591) | GIỮ (L591 kèm ghi chú) | L587: 100 = 90 + 9 + 1 tương đương, L588: 21 = 17 + 4, khớp. L591: 72 = 61 + 3 timeout + 3 unviable + 5 missed (4 giết, 1 tương đương); 3 timeout là vòng lặp vô hạn chưa được test assert bắt, chưa nêu chấp nhận. Thấp rủi ro |
| AR16 RemovePaths (L61) | NÊN HẠ | ô tự ghi "no evidence file or mutation figures for this row ... the change in the 29-30/10 window is not confirmed" nhưng trạng thái Done. Hàng AR16 khác (L43, listing) đã Partial; hai hàng cùng ID dễ nhầm. Nên hạ Partial (thiếu bằng chứng) cho đến khi có mutation `remove_paths` |
| X-w9r7 (L690) | GIỮ | W9-RENAME-LANE: 26/24/2, 34/28/6, 80/79/1, 68/59/9 khớp từng dòng, 0 missed |
| X-counting-fs (L32) | GIỮ | CL-W2RERUN: không có test `#[ignore]` trong bộ test của `counting.rs` nên kết quả cũ không bị lỗi cờ (suy luận có kiểm grep, mutation chưa chạy lại) |
| X-exe-manifest (L16) | GIỮ | đọc ngược PE, không dựa mutation |

## 3. Các hàng chưa phải Done (chỉ xác nhận không cần hạ thêm)

- X-physical-fs (L31), AR16 (L43), SEC-01/F-05 (L45), X-perf-diagnostics/F-22/F-23 (L48, L74, L75): đã Partial. CL-PHYS3 (43 mutant mới: 26 caught, 17 missed đã phân loại, 16/21 killer xác nhận),
  CL-PHYS4 (115 tested: 92 caught, 20 missed, 3 timeout, timeout KHÔNG tính caught) đều đúng dạng cờ, đều giữ Partial.
- INV-6 (journal), F-16/D-14, F-12/F-15, F-13: đã Partial. CX-JRNL/CX-JRNL2/CL-JRNL3: 64/179 caught, 6 unviable, 4 timeout (không tính detected), 105 chưa chạy. Ví dụ rõ nhất về quy tắc "chỉ nâng khi mọi identity caught/tương đương".
- P-01, P-03, P-05, P-11 (L58, L391, L393, L399): Partial. CL-W6RERUN: P-01/03/05/11 82 mutant = 58 caught + 16 missed + 8 unviable; 68 mutant W6 = 54 + 11 + 3. "0 survivor" cũ sai cho 7 mutant thật (đã giết) + 7 tương đương + 13 `cfg(not(windows))`.
  (Số học: 58 + 16 + 8 = 82 và 54 + 11 + 3 = 68 khớp.)
- X-host-wndproc-shell-window/-shell-window-messages (L601, L602): Partial; CL-HOSTRERUN2: `wndproc.rs` 11 (10 caught + 1), `dpi.rs` 28 = 25 + 2 + 1, đúng; `messages.rs` chưa có mutation.

## 4. Đề xuất cho orchestrator (không tự sửa PARITY)

1. Hạ về Partial: L589 X-render-surface, L590 X-render-draw-context, L61 AR16 (RemovePaths). Bằng chứng: chính các ô đó ghi "PARTIAL"/"not a full mutation verdict"/"no evidence".
2. Chuyển Done -> Done kèm điều kiện hoặc bổ sung bằng chứng: L548, L549, L637 (không có số), L597 (không có báo cáo), L41 RV-C05 (1 hand mutant), L59 (2 hang), L90 (ghi chú journal).
3. Sửa chữ (không đổi trạng thái): L40 (xóa "427 of 427", nêu CX-PHYS2 11/11), L44 (xóa "17 mutants ... 0 missed"), L600 (rút gọn lịch sử), L616 (thay trích 10-23 bằng CL-INPUT 20/20), L686/L688 (cập nhật "70 not run"/"3/199" theo CL-RENAME), L687 ("0 missed" -> nêu 1 missed đã giết).
4. Thống nhất chính sách timeout trong AGENTS.md (mục Mutation testing): "timeout không tính caught; mỗi timeout cần một test có assert hoặc lý do chấp nhận". Ảnh hưởng L59, L392, L684-L688.
5. Nếu native-nightly muốn bảo vệ các hàng Done: thêm `w2_fsrerun_killers` (chỉ cần file system) vào danh sách `pr-platform-win` ở `.github/workflows/native-nightly.yml:66`. Các killer cần desktop (host dispatcher, input) không thể chạy trên runner.

## 5. Giới hạn của kiểm toán này

Không chạy build/test/mutation (theo yêu cầu). Không có log thô `mutants.out/` (đã mất theo các báo cáo), nên "nhật ký có `running N tests`" chỉ dựa vào lời kể của từng báo cáo. Chưa đọc đầy đủ: W3-01-JOURNAL-MUTATION, W2-CONFIG-MUTATION, W8-PURE-MUTATION,
W9-R1..R5 (đọc qua tóm tắt CL-RENAME/MUTATION-2/-3), CX-STARTUP, CX-TEXTS, CX-PAINTER. Các hàng Done không trích báo cáo nào (khoảng 25 hàng X-w9e*, X-keyid-mirror, X-viewer-state, X-main-view-model-*, I-13, RV-P01...) chỉ được xem lướt ô mutation và không kết luận.
