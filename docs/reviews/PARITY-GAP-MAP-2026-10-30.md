# PARITY gap map (cl-audit2, 2026-10-30)

Trạng thái: bản kiểm toán tài liệu. Không đổi mã nguồn, không đổi trạng thái dòng nào trong `docs/PARITY.md` (chỉ sửa chữ cho khớp bằng chứng đã có trong `docs/reviews`).

Phạm vi: mọi dòng của [`PARITY.md`](../PARITY.md) có cột trạng thái bắt đầu bằng `Partial` (84 dòng; `X-exe-entry` và `X-exe-versioninfo` mỗi cái xuất hiện 2 lần nên bảng gộp thành 83 mục). Mỗi mục được xếp vào MỘT nhóm theo khoảng trống chính của nó; nhiều dòng còn khoảng trống phụ thuộc nhóm khác, ghi trong cột mô tả.

| Nhóm | Số mục |
|---|---|
| [mâu thuẫn] ghi chú lỗi thời, đã sửa chữ trong PR này | 13 |
| [người] chỉ chủ dự án/người ngồi máy làm được | 34 |
| [máy] mutation hoặc test còn chạy được | 36 |

## 1. [mâu thuẫn]: đã sửa chữ

Sửa theo bằng chứng đã có (CL-ENTRY, CL-INPUT2, CL-MAINWIN, CL-PRAPP, W9-RENAME-*, CX-STARTUP và các dòng chị em trong PARITY). Không dòng nào đổi trạng thái: mỗi dòng vẫn còn khoảng trống thật, nêu ở cột phải.

| Dòng PARITY | Khoảng trống chính xác | Lane đề xuất |
|---|---|---|
| X-exe-entry (dòng 2, `Program.Main`) | Ghi chú "entry mutation not rerun" lỗi thời: CL-ENTRY đã mutate `entry.rs` (9 mutant, 6 caught, 3 `production_run` không quan sát được nếu không mở cửa sổ thật). Đã sửa chữ. |  |
| X-w9r8-native-rename-dialog-shell | Ghi chú nói "chưa nối `DialogServiceOption`", "chưa có test E2E", "mutation not run": thực tế đã nối trong `startup/composition_root.rs`, E2E `rename_shell_e2e` có, presenter rename đã mutate sạch. Đã sửa chữ. Còn thật: đo mực 144 DPI/vi chỉ là proxy cấu trúc. |  |
| X-w9r11-rename-dialog-e2e | Ghi chú "`UiSmokeRun` còn là stub", "nối ở PR #304", "mutation not run": lỗi thời. Đã sửa chữ. Còn thật: E2E chạy trên `main_window_fixture`, không qua `ShellStartup`; không có IME/Unicode, handle-leak. |  |
| X-recovery-window-presenter | "Desktop run pending W8-18t..18v" trong khi `X-dialogs-recovery` đã ghi chạy native 2026-10-29. Đã sửa chữ; giữ Partial vì chưa có lần chạy có người ngồi máy. |  |
| X-benchmark-presenter | "Done once W8-18r/18s record a desktop run" trong khi dòng 18r/18s đã ghi chạy native. Đã sửa chữ; Partial còn vì 2 mutant `dispose` thiếu seam. | ml-bench-dispose-seam (cần seam sản xuất) |
| X-input-shell-input | "mutation debt of dispatch.rs below" lỗi thời: CL-INPUT2 `dispatch.rs` 173 tested, 0 missed. Đã sửa chữ. Còn thật: replay không đi qua window procedure của OS, không có RAW thật. |  |
| X-input-main-window | "mutation debts below" lỗi thời: CL-INPUT2 `main_window.rs` (input) 98 tested, 0 missed. Đã sửa chữ. Còn thật: chưa đi qua `ShellStartup`. |  |
| X-menu-native-popup-host | "mutation debt below" chỉ là 13/44 ngày 2026-10-23; còn 31 mutant `popup_host.rs` chưa chạy (CL-INPUT2). Đã sửa chữ. | ml-menus-popup-host (31 mutant) |
| X-menu-main-window | "mutation debt below" lỗi thời: `shell_menus/main_window.rs` 58 tested, 0 missed (CL-INPUT2). Đã sửa chữ. Còn thật: pixel menu theo font/DPI của máy này. |  |
| X-main-window-core | "mutation debts below" là vòng machine B cũ; CL-MAINWIN đã chạy lại registry, còn 5 MISSED cần `MainWindowInner` sống. Đã sửa chữ. `core.rs` (134 mutant) ngoài registry chưa chạy lại. | ml-mainwin-core-seam (cần seam hoặc test trong crate) |
| X-main-window-lifecycle | Danh sách survivor cũ; CL-MAINWIN đã giết các `InnerSteps`, còn MISSED: `on_navigation_changed`, `on_compare_changed` (không có observer), minimise/restore x4 (hiện cửa sổ), close waiter `&&`. Đã sửa chữ. | ml-mainwin-core-seam |
| X-settings-presenter | Số mutation cũ (apply 7 missed, load 7 missed) chưa nêu cl-prapp đã chạy nốt 126 mutant, 0 survivor thật. Đã thêm con trỏ. Còn thật: seam `SettingsTexts`/`ShellIntegration`/HTTP, `try_start_explorer` không quan sát được. |  |
| X-settings-ui-pages | Ghi chú "other pages not mutated" lỗi thời: CL-DLG đã mutate `pages/*` (244 tested, 0 missed). Đã sửa chữ. Còn thật: golden từng trang chưa nạp theo tên; Shortcuts dùng tên hành động tiếng Anh (D-W8B5-806); chỉ trang baseline. |  |

Ngoài 84 dòng Partial, 3 dòng `Done` cũng mang ghi chú "chưa chạy" đã lỗi thời và đã được thêm con trỏ (trạng thái giữ nguyên): `X-w9r2-case-only-file-action-rename` ("268 not run"), `X-w9r3-rename-journal-undo-recovery` ("70notrun") và `X-w9r4-rename-catalog-cache`; cả ba đã được W9-RENAME-MUTATION-2/3 chạy cả file.

Dòng `Done` còn mang khoảng trống mutation THẬT, KHÔNG sửa (chỉ báo cáo, chủ dự án quyết hạ về Partial hay giao lane): `X-input-native` (`native.rs` 69/92, 18 missed lịch sử), `X-input-message-handlers` (`handlers.rs` 13/42), `X-input-viewport-surface` (`surface.rs` 13/126), `X-input-drop-target` (`drop_target.rs` 17/23, 3 missed coi như tương đương/native). CL-INPUT/CL-INPUT2 không phủ các file này.

## 2. [máy]: việc còn chạy được, kèm lane đề xuất

Quy ước chung của lane đề xuất: `cargo mutants --baseline skip --timeout 90 -j 1` trên file thuần logic, killers trong file test mới, không mở cửa sổ/GUI thật; glue Win32 vẫn loại theo chính sách.

| Dòng PARITY | Khoảng trống chính xác | Lane đề xuất |
|---|---|---|
| P-03 | 32 mutant adapter/normalization còn sống ở `recycle_bin_settings` + chưa có test native với thùng rác thật (phần này là người). | ml-recycle-settings (mutation + killers `recycle_bin_settings.rs`) |
| X-stj-reader | 8 timeout chưa giải quyết ở các vòng `*=1`; không chứng minh lại được nếu không có progress hook sản xuất hoặc timing guard. | ml-stj-hook (thêm hook rồi chứng minh lại) |
| X-release-dependencies | `cargo deny check` chưa chạy được (tool vắng, cần cài + mạng). | ml-deny-run (cài cargo-deny 0.19.9 rồi chạy audit) |
| P-11 | 19 mutant `display_clock.rs` còn sống (41 caught/6 unviable). | ml-display-clock |
| W4-11c WIC direct decode | 54 định danh pipeline/COM/helper chưa chạy; `wic_decoder` 41/42 với lỗi OQ1 (câu địa phương). | ml-w4-wic (54 mutant + OQ1) |
| W4-11e WIC cache codec | 33 định danh adapter/native chưa chạy; 2 giới hạn seam COM; (và pixel WPF là việc người). | ml-w4-wic |
| W4-14b service decode path | Quan sát disk I/O race, phân loại Busy/Canceled của `WithSourceBytes`, joiner retry. | ml-w4-service |
| W4-14c service disk | Mutant bounded-wait (`286 > >=`) thiếu quan sát không dùng đồng hồ; seam thứ tự shutdown; `path_id`. | ml-w4-service (cần seam) |
| W5 final status (integration/wave-5) | Survivor lịch sử không tái dựng được ("exact historical diff unavailable"); nên đề xuất chủ dự án đóng bằng phán quyết cuối. | chủ dự án chốt (không chạy thêm) |
| X-raw-mutation-w5 | Danh tính 277 mutant lịch sử chưa tái dựng. | như trên |
| X-bench-slowlink-fs | Phần kho mutant còn lại + DIVERGENCE đã ghi. | ml-bench-rest |
| X-bench-executor | Chuyển tiếp `DefaultFileLog` riêng + kho mutant còn lại. | ml-bench-rest |
| X-bench-workload-runner | Kho mutant còn lại + DIVERGENCE (D67). | ml-bench-rest |
| X-bench-cli-guard | Kho mutant còn lại; lỗi sandbox UNC; khác biệt replace. | ml-bench-rest |
| X-pointer-click-pan | Chưa có test native lái kéo chuột -> thả bắt đầu quán tính qua message thật (còn lại: chuột/bút có người). | ml-pan-glide-native |
| X-composition-factories | 13 ứng viên decoder ngoài phạm vi chính sách thuần chưa chạy (ORF corpus là việc người). | ml-startup-rest |
| X-zoom-detail-loader / I-16 | 44 mutant zoom-detail chưa chạy, native twin `ExifRotatedSource_*`, stale completion chưa ép trong exe. | ml-zoom-detail |
| X-host-swap-chain-clock, X-host-shell-contracts, X-host-icon | `host::contracts::tests` inline chưa chạy lại; test cửa sổ-surface 12d; wrapper `host::native` là glue. | ml-host-rest |
| X-viewing-composer | 3 bản sinh đôi native chưa port: `LargeImage_SplitIntoTiles_*`, `DeviceLostDuringACrossfade_*`, `Orientation_*`. | ml-composer-twins |
| X-main-window-frame | Cửa sổ minimised (client = 0), frame trễ (`wait_for_next_frame` không seam), 7 test bar end-to-end, ảnh đầu tiên. | ml-mainwin-native-tests |
| X-main-window-viewing | `update_retained` đầu ra, `PrefetchHost`, `draw_presentation`/`render_now` ảnh đầu, crossfade mất thiết bị, 100 bước điều hướng (W8-17e). | ml-mainwin-native-tests |
| X-main-window-windowing | 85/175 mutant đuôi `windowing.rs` chưa chạy; `ShellFullscreenFrameGeometryTests`; F11 maximised thật. | ml-mainwin-windowing-tail |
| X-dialogs-painter | Chỉ `paint_scale` đã mutate; phần còn lại của painter chưa. | ml-dialog-painter-mut |
| X-chrome-painter-controls | Chỉ `frame_rects`; bản 144 dpi chưa port; `fill` rect rỗng. | ml-dialog-painter-mut |
| X-chrome-painter-frame | Chỉ `border_layers`; `SS_ETCHEDHORZ/VERT` chưa đo. | ml-dialog-painter-mut |
| X-dialogs-secondary-port | Chỉ vài hàm đã mutate; factories sản xuất và route editor/rename chưa kiểm bằng các binary này. | ml-dialog-painter-mut |
| X-dialogs-zoom-diagnostics-profiles-benchmark | Chỉ `row_color_key`; kịch bản native (ngôn ngữ/DPI/GDI, sort, lặp) chưa port; không có benchmark codec sản xuất. | ml-dialog-painter-mut |
| X-dialogs-recovery | Chỉ `command_action`; closure Retry/Dismiss từ composition (P-43), kịch bản DPI/GDI/modal, hand-break (người). | ml-dialog-painter-mut |
| X-settings-ui-framework | CL-DLG đã mutate sạch catalog/page_view/builder-layout; ghi chú "rest not mutated" đã sửa. Còn thật: `builder/{groups,rows,inputs}.rs` (352) và `context.rs` (94) chưa mutate; golden `settings-layout`/`settings-builder` chưa nạp. | ml-settings-ui-builder-mut |
| X-settings-ui-window | CL-DLG đã mutate sạch phần logic `dialog/*`; chữ đã sửa. Còn thật: `dialog/host.rs` 68, `frame.rs` 190, `surface.rs` 130 chưa mutate; nhãn kiểu trang runtime C#, DPI/GDI/ngôn ngữ; `ScrollInfo` glue. | ml-settings-ui-builder-mut |
| X-startup-composition-root | `secondary_dialogs.rs` 72 mutant chưa chạy; đối chiếu golden đăng ký host+shared; locale. | ml-startup-rest |
| X-startup-options-run | Golden `shell-startup-texts`, tổ hợp option/env đầy đủ. | ml-startup-rest |
| X-startup-early-decode | Kịch bản process `Startup_EarlyDecode_StartsOnlyAfterTheInstanceLockIsHeld` chưa chạy. | ml-startup-rest |
| X-startup-pure-17e | Golden `shell-startup-texts` (phần phần cứng là người). | ml-startup-rest |
| X-composition-texts-adapters | Adapter `LocalizedFormat::new/to_local_time`, lazy preview success/error forwarding. | ml-startup-rest |
| X-w9r10-rename-view-model-prompt-app-inventories | Chỉ là kiểm kê; nên đóng sau khi chủ dự án chấp nhận (không có khoảng trống chạy được). | chủ dự án chốt |

Gộp lane theo thứ tự ưu tiên giá trị/chi phí: (1) `ml-mainwin-windowing-tail` + `ml-menus-popup-host` (116 mutant xác định, thuần thông số); (2) `ml-startup-rest` (secondary_dialogs 72, locale 64, warmup 14, monitor_probe 17, decoder 13, golden `shell-startup-texts`); (3) `ml-dialog-painter-mut` (painter/controls/frame/secondary/recovery/zoom-diagnostics: phần lớn file chưa mutate); (4) `ml-w4-wic` + `ml-w4-service`; (5) `ml-input-handlers-surface` cho 4 dòng Done ở mục 1; (6) `ml-deny-run` (cài cargo-deny); (7) `ml-recycle-settings`, `ml-display-clock`, `ml-bench-rest`, `ml-zoom-detail`, `ml-composer-twins`, `ml-pan-glide-native`. Các mục cần seam sản xuất (`ml-stj-hook`, `ml-bench-dispose-seam`, `ml-mainwin-core-seam`) cần quyết định thêm production code trước.

## 3. [người]: bảng chi tiết

| Dòng PARITY | Khoảng trống chính xác |
|---|---|
| X-exe-entry (dòng 1) | `production_run`/`main_entry` chỉ kiểm được bằng lần chạy có người ngồi máy (`exe_process` smoke ignored) + 3 mutant `production_run` cần cửa sổ thật. |
| X-exe-versioninfo (2 dòng) | Chuỗi công ty/bản quyền, kiểm Properties và SmartScreen (P-55); chỉ làm được bằng người + bản release. |
| X-benchmark-compare | Đo có người ngồi máy W10-B2; backend dùng engine chưa nối (cần code, sau khi có số đo). |
| X-benchmark-parity-harness | Như trên: W10-B2 có người ngồi máy; backend engine chưa nối. |
| D-10 | Hộp thoại Settings chạy tay (không phải mutation: mutation toàn cửa sổ + presenter đã sạch). |
| C-28 | Luồng thông báo sửa cấu hình lúc khởi động -> hộp thoại thật (`phase_two` bước 6) cần chạy có người ngồi máy. |
| P-01 (Explorer COM boundary) | Cửa sổ Explorer thật có thư mục khớp chưa được lái; COM glue không mutate theo chính sách. |
| P-09 | TLS downgrade, PAC/proxy/trust, runtime mặc định .NET, Settings close/ShellExecute thật: oracle runtime + rig có người. |
| X-http-transport | Như P-09: PAC/proxy/trust/TLS, rig attended; helper wide/os_error là việc nhỏ cho lane máy. |
| X-http-composition | Settings close / late-poster / ShellExecute thật chưa kiểm (glue bị loại khỏi mutation). |
| I-15b / INV-8 / INV-12 (LibRaw native layer) | Cần bộ ảnh RAW thật (P-30, ~590 MB CC0) cho 7 ca corpus-gated. |
| I-15 / I-14 / I-12 (LibRaw decoder, W5-16c) | 17 ca corpus-gated + 3 ca "ported but not executed" cần RAW thật. |
| X-release-workflow | Dispatch/publish trên GitHub thật và P-55; actionlint chưa có (việc nhỏ cho máy). |
| X-release-native | Fetch gói native thật, gói/nguồn release thật, rà soát P-55 (TurboJPEG pin, notices, LICENSE). |
| X-raw-native-libraw | Chạy strict-mode với corpus RAW thật. |
| X-raw-native-turbojpeg | So sánh pixel với WPF thật (Rust không có WPF, slot là `WicDirectDecoder`); DIVERGENCE đã ghi. |
| X-raw-corpus | Chủ dự án đồng ý (P-30), cung cấp corpus + manifest, chạy strict, sinh/so golden theo tên. |
| F-21 | Quyết định sản phẩm COPY-PARTIAL-01: giữ file dở khi Copy hỏng giữa chừng. |
| P-05 | Hành vi liên máy chủ (cross-host) của pipe; glue I/O native loại khỏi mutation. |
| W4-02b embedded thumbnail | So pixel độc lập với WPF cũ + UI có người ngồi máy. |
| W4-10b thumbnail cache | So PNG với WPF cũ; hai khác biệt (RAM-observation, abandoned-load) giữ nguyên. |
| X-host-message-loop | Còn lại chỉ là glue `Win32LoopOs::translate_and_dispatch` (loại theo chính sách) + 1 mutant tương đương; chủ dự án chấp nhận hoặc chạy tay. |
| X-host-wndproc-shell-window | 6 mutant field `WNDCLASSEXW` (glue, không seam); nhánh icon exe của W8-20b (X-exe-icon cần ánh xạ sync, việc của lead). |
| X-viewing-compare-layer | So scene/pixel golden C# (chưa nạp), GPU phần cứng (hiện chỉ WARP). |
| X-viewing-crossfade | Như trên: golden pixel C#, GPU thật; 2 test C# chưa có bản sinh đôi. |
| X-menu-owner-draw-session | Pixel phụ thuộc font/DPI của máy; `owner_draw.rs` là glue GDI chưa mutate. |
| X-startup-main-window | Đa dạng monitor/DPI và phần cứng desktop. |
| X-startup-overlay-viewmodel | Đan xen modal/lệnh thật đầy đủ + phần cứng. |
| X-startup-settings-load | Thông báo "đã sửa cấu hình" hiện đúng một lần qua dialog service thật. |
| X-startup-instance | Thông báo chủ-sở-hữu-không-trả-lời theo từng chế độ trong UI thật. |
| U-initial-viewport | Kỳ vọng chỉ tính từ monitor/DPI của máy này; chạy khởi động desktop đầy đủ. |
| X-ui-smoke | `ui_smoke_perf` (first_frame_switch_p50_p95_and_idle_cpu_report) có người ngồi máy, W8-12j. |
| X-w9r6-rename-view-retarget | Chỉ còn khẳng định UI native / RAW corpus / hiệu năng. |
| X-w9r9-rename-test-matrices | Không có verdict Recycle Bin thật / desktop / RAW (đúng theo thiết kế, chỉ test với fake). |

## 4. Việc cần người (danh sách NGẮN cho chủ dự án)

1. RAW thật (P-30): đồng ý + cung cấp corpus CC0 ~590 MB, chạy strict-mode; mở khóa `X-raw-corpus`, `I-15b`, `I-15`, `X-raw-native-libraw`, ORF của `X-composition-factories`, `X-zoom-detail-loader`.
2. Đo có người ngồi máy: `ui_smoke_perf` (`X-ui-smoke`), W8-12j và W10-B2 (`X-attended-remeasure`, `X-benchmark-compare`, `X-benchmark-parity-harness`).
3. Chạy tay có người: Settings (D-10), thông báo sửa cấu hình lúc khởi động (C-28, `X-startup-settings-load`), thông báo chủ-sở-hữu-không-trả-lời (`X-startup-instance`), `production_run` smoke (`X-exe-entry`), các kịch bản hand-break của hộp thoại recovery.
4. So pixel với WPF/C#: thumbnail và cache (W4-02b, W4-10b), golden scene của compare/crossfade, menu/painter theo font và DPI, TurboJPEG (`X-raw-native-turbojpeg`); GPU phần cứng (hiện chỉ WARP).
5. Phần cứng/hệ thống thật: Explorer thật (P-01), thùng rác thật (P-03), pipe liên máy (P-05), đa monitor/DPI (`X-startup-main-window`, `U-initial-viewport`).
6. Release và mạng (P-55, P-09): fetch native thật, dispatch/publish thật, chuỗi công ty/bản quyền, SmartScreen, TLS/PAC/trust.
7. Quyết định: COPY-PARTIAL-01 (`F-21`); ánh xạ icon trong `tools/sync-from-csharp.ps1` (`X-exe-icon`, việc của lead); chốt hay đóng `W5 final status`/`X-raw-mutation-w5` (survivor lịch sử không tái dựng được); chấp nhận chính sách loại glue Win32 khỏi mutation.
