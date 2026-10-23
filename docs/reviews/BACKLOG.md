# BACKLOG tổng hợp từ docs/reviews/

Machine C (cloud), 2026-10-23. Gộp mọi phát hiện của các báo cáo trong `docs/reviews/`: `autopilot`, `dialogs-base`, `native-dialog`, `render-painter`, `settings-ui`, `startup`, `test-quality`, `invariants`, `main-window-input-overlay-viewing`, `w9-contracts`, `pr-164`, `pr-203`. `test-strengthening-plan.md` (80 test đề xuất, PR #208) được trỏ tới từng nhóm, chưa gộp lại từng dòng.

**Mọi mục dưới đây là suy luận từ mã, chưa chạy thử** (đúng như báo cáo nguồn). Mã phát hiện = tên báo cáo + số trong báo cáo (ví dụ `startup H1` = `docs/reviews/startup.md` mục H1). Đã khử trùng lặp: các mục cùng gốc gộp thành một dòng, ghi mọi mã nguồn.

Quy ước prompt Codex: AGENTS.md giao mọi việc triển khai cho Codex. Mỗi prompt có phạm vi file được phép (ngoài phạm vi là cấm sửa), phát hiện cần sửa, test cần thêm, và lệnh kiểm tra theo từng package (CI format theo package: `cargo fmt -p <pkg> --check`). Không chạy mutation trên mã glue/FFI (AGENTS.md).

## 1. Bảng xếp hạng (mức độ, rủi ro dữ liệu/ổn định)

Rủi ro: **D** = có thể mất/sai dữ liệu người dùng, **C** = crash/treo/panic trên luồng UI, **B** = hành vi sai so với C#, **T** = chỉ test/công cụ/tài liệu.

| # | Nhóm | Mã phát hiện | Mức | Rủi ro | Đơn vị chủ | Loại |
|---|---|---|---|---|---|---|
| 1 | G1 Main window: `RefCell` giữ qua hộp thoại modal / controller | b4 H1, b4 H2 | cao | C | W8-13h, 13i, 14a..14g | sửa mã |
| 2 | G2 Zoom do con trỏ/phím không tới presenter/HUD | b4 H3 | cao | B | W8-13i, 14a, 16f | sửa mã |
| 3 | G3 Recovery hiển thị sai thời gian (~1601 năm) | startup H1, startup L1, startup L6 | cao | B | W8-09b (`shell_startup/locale`) | sửa mã |
| 4 | G4 Settings > General mất View mode/Instance mode khi Save | settings-ui H1 | cao | D (cấu hình) | W8-19a..19i | sửa mã |
| 5 | G5 File action: executor panic sau khi file đã đi mất | invariants M3 | trung bình | D | W8-07d | sửa mã |
| 6 | G6 Dọn trùng lặp không kiểm lại bản giữ lại, không Undo | invariants M1 (+Q3) | trung bình | D | W8-07f | quyết định -> sửa mã |
| 7 | G7 Dialog service không chuyển prompt; Recovery không biết cài đặt xoá vĩnh viễn; cổng lazy | startup M1, M2, M3, L3, L4; dialogs-base M1, L1, L2; w9-contracts M1 | trung bình | B/C | W8-17a..17c, 18a..18c, 09b | sửa mã |
| 8 | G8 `OpenInDefaultEditor => todo!()` đi tới được | b4 M1 | trung bình | C | W8-14a..14e | quyết định -> sửa mã |
| 9 | G9 Settings > Files thiếu FEAT-EXTERNAL-EDITORS; texts tiếng Anh; re-entrancy `EN_CHANGE`/`Ref` | settings-ui M1, M2, M3, L1, L2, L3, L4 | trung bình | B | W8-19a..19i, W9-E4 | sửa mã |
| 10 | G10 Render: surface không còn target nhưng vẫn vẽ; pop_transform/clip | render-painter M1, L1, L2, L3 | trung bình | C | W8-12b..12i, 18l..18n | sửa mã |
| 11 | G11 Native dialog: callback chạy khi unwind, tooltip, font, focus | native-dialog M3, L1..L5 | trung bình | C | W8-18h..18k | sửa mã |
| 12 | G12 Ghi journal/Undo/cổng (hardening INV) | invariants L1, L2, L5, L6, L7, L8 | thấp | D | W3-01..06, W8-06e | sửa mã + test |
| 13 | G13 Shell startup (#164): báo cáo lỗi, forwarded-open, StartupTask | pr-164 L1, L5, L6; startup L5 | thấp | C | W8-17a..17c | sửa mã |
| 14 | G14 Main window nhỏ: `UiLocalPoster`, timer, prefetch thread, `FrameLog`, `WM_CONTEXTMENU` | b4 L1..L5 | thấp | C/B | W8-13g..16i | sửa mã |
| 15 | G15 Registry/composition: vòng `Arc`, lỗi tạm bị lưu `OnceLock`, `percent_one_decimal` | startup L2, L3, L6 | thấp | C | W8-09b | sửa mã |
| 16 | G16 Công cụ autopilot: `claim.sh`, `codex-lib.py`, pipeline | autopilot H1, M1..M7, L1..L11 | cao (H1) | T | tools/autopilot | sửa mã (script) |
| 17 | G17 CI không chạy 1157 test `#[ignore]`; ignore stale; test pass giả | test-quality M1..M4, L1..L3; native-dialog M1, M2, L6; settings-ui L5; b4 M2; pr-203 M1, L2 | trung bình | T | nhiều | thêm test |
| 18 | G18 Golden/test của #164 không so sánh; `dpi.rs` không test; forwarded-open | pr-164 M1, M2, M3, L2, L3, L4 | trung bình | T | W8-17a..17b | thêm test |
| 19 | G19 Test INV còn thiếu (mất cache journal, composition gate, dọn trùng lặp) | invariants §5, M1..M3, L1..L8 | trung bình | D | W3, W8-06e/07d/07f | thêm test |
| 20 | G20 Test Rename/StalePreview/Wave 9 chưa có | test-strengthening-plan (tp-pr-core, tp-pr-app) | trung bình | D | W9-R1..R11, W8-06c | thêm test |
| 21 | G21 Contract W9 lỗi thời sau #182 | w9-contracts H1, L6..L10 | cao (H1) | T | contracts/W9-*, docs/WAVE-9-PLAN-edt.md | tài liệu |
| 22 | G22 PARITY/PR evidence mâu thuẫn | pr-164 M4; pr-203 M2 | trung bình | T | docs/PARITY.md | tài liệu |
| 23 | G23 Chú thích/`DIVERGENCE` lạc hậu | test-quality L1; w9-contracts L8; settings-ui L5 | thấp | T | nhiều | tài liệu |
| 25 | G25 Retry Recycle xoá nhầm file thay thế sau Prepared | invariants H1 (tái kiểm #212) | cao | D | W3-06; W8-18v gọi UI | sửa mã + test, cần ADR parity |
| 24 | G24 Cần quyết định sản phẩm | invariants Q1, Q2, Q3; w9-contracts M2..M5, L11; b4 M1; pr-203 M2; test-quality M1 | n/a | n/a | lead | quyết định |

Thứ tự đề xuất sau tái kiểm: G25 (nguy cơ xoá vĩnh viễn), rồi G3, G4, G1, G2 (lỗi người dùng thấy ngay); G5, G7, G6 (an toàn dữ liệu); sau đó G10, G11, G9; test (G17..G20) song song vì không xung đột file; G16 độc lập; G21..G23 tài liệu cuối cùng (chạm PARITY, gộp theo hàng).

## 2. Chi tiết theo nhóm

### G1 sửa mã (cao): `RefCell<MainViewModel>` và `viewer.borrow_mut()` giữ qua OS/controller
- b4 H1: hộp thoại modal chạy khi `update_view_model` (`lifecycle.rs:326`), `with_view_model` (`shell_input/main_window.rs:587`, `shell_menus/main_window.rs:322`) đang `borrow_mut`; vòng lặp lồng gây `BorrowError` ở vẽ, `pump_view_model`, `on_closing`, open chuyển tiếp. Thiết kế 17e (`pr-shell/.../overlay_vm.rs`) đi đúng đường này.
- b4 H2: `run_pointer` (`dispatch.rs:647`) giữ `viewer.borrow_mut()`; `scroll_anchor_to` -> `update_layout` -> `viewer.borrow()` (`lifecycle.rs:359,379`) panic ở thao tác zoom có neo đầu tiên. Fixture `shell_input_dispatch_fixture.rs:9-14` né bằng cache.
- Liên quan (cùng quy tắc D-W8B4I-1): settings-ui M3/L1/L2, render-painter L1, pr-203 M2 (vòng tham chiếu cửa sổ chính, test `closed_disposes_the_bars_once_and_unsubscribes` đỏ trên master).

**PROMPT Codex G1**
```
Việc: sửa lỗi RefCell giữ qua hộp thoại modal / controller trong cửa sổ chính (docs/reviews/main-window-input-overlay-viewing.md H1, H2; docs/reviews/pr-203.md M2).
Đọc trước: AGENTS.md, docs/adr/R0003-*, docs/reviews/main-window-input-overlay-viewing.md (H1, H2), contracts/W8-13h.md, W8-13i.md, W8-14a.md.
Phạm vi file được sửa: crates/pr-platform-win/src/host/main_window/{lifecycle,viewing,mod}.rs, crates/pr-platform-win/src/shell_input/{main_window,dispatch}.rs, crates/pr-platform-win/src/shell_menus/main_window.rs và các test mới tên crates/pr-platform-win/tests/shell_main_window_reentrancy.rs (đường dẫn thật: lấy theo contracts/W8-13g.md, đừng đoán). Cấm sửa file khác.
Yêu cầu: (1) không giữ borrow_mut của view model qua bất kỳ hộp thoại modal/lời gọi OS nào: lấy kết quả ra khỏi borrow trước khi gọi dialog, rồi borrow lại để áp kết quả; (2) run_pointer không giữ viewer.borrow_mut() trong lúc controller gọi update_layout: đổi sang trích dữ liệu cần thiết ra trước hoặc dùng try_borrow có đường dự phòng; (3) sửa vòng tham chiếu của cửa sổ chính để test closed_disposes_the_bars_once_and_unsubscribes xanh.
Test phải thêm: (a) test mở dialog giả (host fake) từ lệnh Open/Settings và trong callback dialog gọi vẽ/pump/on_closing: không panic; (b) test zoom có neo bằng con trỏ trên viewer thật (không dùng cache của fixture); (c) bỏ #[ignore] của closed_disposes_the_bars_once_and_unsubscribes.
CẤM: làm yếu/xoá/ignore test hiện có; thêm #[allow] không kèm lý do; unwrap/expect ở đường production; sửa fixture để né lỗi.
Kiểm tra (chạy và dán kết quả): cargo fmt -p pr-platform-win --check; cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings; cargo test -p pr-platform-win --locked (native chỉ chạy từng test đã duyệt, không chạy blanket --include-ignored; không đụng bin người dùng).
```

### G2 sửa mã (cao): zoom từ con trỏ/phím không tới presenter/HUD
- b4 H3 (`apply_viewer_update` chỉ gọi từ lệnh zoom của view model; C# `MainViewModel.cs:136,140-143` bắt sự kiện trực tiếp từ viewer). Chưa đọc `zoom_detail` tới cùng; Codex phải xác nhận trước khi sửa.

**PROMPT Codex G2**
```
Việc: nối thay đổi zoom do con trỏ/phím tới presenter.set_viewer_zoom, HUD zoom và bộ nạp zoom-detail (docs/reviews/main-window-input-overlay-viewing.md H3).
Bước 0: tái hiện bằng test đỏ trước (không sửa mã): zoom bằng cuộn chuột trên viewer thật, khẳng định presenter nhận set_viewer_zoom đúng giá trị. Nếu test xanh ngay thì dừng và báo.
Phạm vi: crates/pr-platform-win/src/host/main_window/*.rs, crates/pr-platform-win/src/shell_input/*.rs, crates/pr-app/src/viewmodels/main/*.rs (chỉ chỗ đăng ký/nhận sự kiện viewer), tests mới shell_zoom_events.rs (pr-platform-win) và viewmodels_main_zoom_events.rs (pr-app). Cấm sửa file khác.
So với C#: MainViewModel.cs dòng 136, 140-143 (đăng ký sự kiện viewer).
Test: zoom bằng wheel/phím/click-zoom => HUD % đổi, set_viewer_zoom được gọi đúng một lần mỗi thay đổi, zoom-detail loader nhận mức mới.
CẤM: làm yếu test; thêm callback gọi lại vòng tròn (re-entrancy) vào view model đang borrow (xem G1).
Kiểm tra: cargo fmt -p pr-platform-win --check && cargo fmt -p pr-app --check; cargo clippy -p pr-platform-win -p pr-app --all-targets --locked -- -D warnings; cargo test -p pr-platform-win -p pr-app --locked.
```

### G3 sửa mã (cao): thời gian Recovery
- startup H1: `shell_startup/locale.rs` `format_local_general` coi tick .NET là FILETIME (thiếu trừ `504_911_232_000_000_000`); test `shell_startup_locale.rs:78-100` bị `#[ignore]` và chỉ kiểm `contains("20")`. startup L1: `local_long_time_text` dùng `TIME_NOSECONDS` (C# `ToString("T")` có giây). startup L6: `percent_one_decimal` bảng ngôn ngữ cứng.

**PROMPT Codex G3**
```
Việc: sửa các hàm định dạng thời gian của Recovery (docs/reviews/startup.md H1, L1, L6).
Phạm vi: crates/pr-platform-win/src/shell_startup/locale.rs và crates/pr-platform-win/tests/shell_startup_locale.rs. Cấm sửa file khác.
Sửa: (1) format_local_general: filetime = ticks - FILETIME_EPOCH_TICKS (pr_core::time), từ chối (Err/None) khi âm; bỏ dòng .checked_div(1).filter(..) vô nghĩa; (2) local_long_time_text: chỉ nhánh "g" dùng TIME_NOSECONDS, nhánh "T" truyền cờ 0 (có giây); (3) percent_one_decimal: lấy mẫu từ NumberFormatInfo tương đương (GetLocaleInfoEx LOCALE_IPOSITIVEPERCENT hoặc tương đương) nếu làm được trong phạm vi; nếu không, ghi TODO có lý do và KHÔNG đổi hành vi.
Test: ca tất định không phụ thuộc múi giờ máy: tách phần chuyển tick -> SYSTEMTIME ra hàm thuần và kiểm giá trị chính xác (ví dụ tick của 2020-06-15T12:34:56Z phải cho năm 2020, tháng 6, ngày 15, 12:34:56); ca tick âm; bỏ khẳng định contains("20").
CẤM: làm yếu test; giữ #[ignore] cho test thuần.
Kiểm tra: cargo fmt -p pr-platform-win --check; cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings; cargo test -p pr-platform-win --locked --test shell_startup_locale.
```

### G4 sửa mã (cao): Settings > General
- settings-ui H1: `pages/general.rs:258-259` `collect` gọi `set_combo_index` (ghi) thay vì đọc `combo_index` cho View mode và Instance mode; test `settings_page_general.rs:46,122` chỉ kiểm `load`.

**PROMPT Codex G4**
```
Việc: sửa GeneralSettingsPage::collect đọc đúng combo (docs/reviews/settings-ui.md H1). Tham chiếu C#: GeneralSettingsPage.cs Collect (state.ViewModeIndex = Context.ComboIndex(_viewMode), tương tự instance mode).
Phạm vi: crates/pr-platform-win/src/dialogs/settings/pages/general.rs và crates/pr-platform-win/tests/settings_page_general.rs. Cấm sửa file khác.
Test (viết trước, phải đỏ): round-trip thật: load(state A) -> đổi combo bằng context giả -> collect -> state khác A đúng giá trị đã đổi, cho View mode và Instance mode; tên test mirror C# (method_scenario_expected).
CẤM: làm yếu test cũ; chỉ kiểm load rồi đặt tên "round trips".
Kiểm tra: cargo fmt -p pr-platform-win --check; cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings; cargo test -p pr-platform-win --locked --test settings_page_general.
```

### G5 sửa mã (trung bình): executor panic
- invariants M3: `coordinators/file_actions/actions.rs:337-365,427,680-747`; `service.rs:249-251`.

**PROMPT Codex G5**
```
Việc: khi executor panic sau khi file có thể đã đi mất, không trả mục về catalog một cách mù quáng (docs/reviews/invariants.md M3).
Phạm vi: crates/pr-app/src/coordinators/file_actions/actions.rs, crates/pr-app/tests/file_action_core.rs (và fake trong crates/pr-app/tests/file_action_fakes/). Cấm sửa pr-core.
Sửa: ở nhánh ExecOutcome::Panicked của on_executed, hỏi FileSystem (qua dependency đã có) xem nguồn còn tồn tại không (giống service.rs single_failure S7.5): nếu nguồn không còn, KHÔNG catalog.restore; hiện trạng thái "unverified" (key STATUS_MOVE_UNVERIFIED hoặc tương đương) và không đăng ký Undo; nếu nguồn còn, giữ restore như cũ.
Test: executor giả di chuyển file rồi panic -> catalog không chứa lại nguồn đã mất; executor giả panic trước khi chạm đĩa -> catalog restore như cũ. Giữ test two_scripted_executor_panics_in_a_row_both_report_boom.
CẤM: làm yếu test; thêm unwrap/expect.
Kiểm tra: cargo fmt -p pr-app --check; cargo clippy -p pr-app --all-targets --locked -- -D warnings; cargo test -p pr-app --locked.
```

### G6 (trung bình): dọn trùng lặp (Q3 đã chốt ở P-52)
- invariants M1: `duplicate_cleanup.rs:469-611,615-665,741-753`. Đề xuất sau khi chốt Q3: stat lại file đích và ít nhất một bản giữ lại ngay trước mỗi `execute(Recycle)`; bỏ file không còn khớp size/mtime lúc băm.

**PROMPT Codex G6** (Q3 đã chọn kiểm lại, chưa làm Undo; không cần hỏi lại)
```
Việc: trước mỗi recycle trong dọn trùng lặp, kiểm lại danh tính (docs/reviews/invariants.md M1).
Phạm vi: crates/pr-app/src/coordinators/duplicate_cleanup.rs, crates/pr-app/tests/duplicate_cleanup.rs. Cấm sửa file khác.
Sửa: lưu (size, last_write_utc) của mọi file trong danh sách xoá VÀ của bản được giữ lại lúc băm xong; trong submit_recycle, stat lại (FileSystem) mục sắp xoá và ít nhất một bản giữ lại; nếu bản giữ lại không còn hoặc mục sắp xoá đã đổi size/mtime thì bỏ qua mục đó, thêm vào failures với lý do, không gọi execute.
Test: batch_recycle_skips_file_whose_keeper_vanished_during_review (fs giả xoá bản giữ trong callback show_batch_review => 0 lần execute, status BatchCanceled hoặc failures); batch_recycle_skips_file_modified_since_hash.
CẤM: đổi hành vi hộp thoại/Undo (ngoài phạm vi, chờ quyết định); làm yếu test.
Kiểm tra: cargo fmt -p pr-app --check; cargo clippy -p pr-app --all-targets --locked -- -D warnings; cargo test -p pr-app --locked --test duplicate_cleanup.
```

### G7 sửa mã (trung bình): chuyển prompt, cổng lazy, cài đặt xoá vĩnh viễn
- startup M1 = w9-contracts M1: `SharedDialogService` (`pr-shell/src/composition/adapters.rs`) thiếu `prompt_add_external_editor`, `prompt_default_external_editor`, `prompt_rename`. dialogs-base M1: `Win32DialogService` thiếu `prompt_rename`. startup M2: `secondary_dialogs.rs recovery()` đặt `allow_permanent_delete: None`. startup M3, dialogs-base L1/L2: `LazySecondaryPort::get` lưu `Some(None)` vĩnh viễn sau khi factory trả `None`/panic. startup L3, L4: `OnceLock<Result>` lưu lỗi tạm; `panic!` trong adapter lazy.

**PROMPT Codex G7**
```
Việc: sửa chuyển tiếp prompt và độ bền của cổng cửa sổ phụ (docs/reviews/startup.md M1,M2,M3,L3,L4; docs/reviews/dialogs-base.md M1,L1,L2; docs/reviews/w9-contracts.md M1).
Phạm vi: crates/pr-shell/src/composition/adapters.rs, crates/pr-shell/src/startup/secondary_dialogs.rs, crates/pr-shell/src/startup/composition_root.rs, crates/pr-platform-win/src/dialogs/dialog_service.rs; test: crates/pr-shell/tests/composition_adapters.rs (hoặc file hiện có tương ứng), crates/pr-platform-win/tests/dialogs_dialog_service.rs. Cấm sửa pr-app, pr-core.
Sửa: (1) SharedDialogService và Win32DialogService chuyển đủ prompt_add_external_editor, prompt_default_external_editor, prompt_rename; (2) recovery(): allow_permanent_delete đọc settings_store().current().allow_permanent_delete_without_recycle_bin lúc hỏi, như C# Win32SecondaryWindowPort.ShowRecovery; (3) LazySecondaryPort: không lưu None vĩnh viễn khi đồ thị chưa gắn (cho thử lại), bắt panic của factory (catch_unwind) và trả None như C# Port(); (4) thay panic! trong LazyDecoderFactory::resolve / LazyPrewarmPreviews::start_viewer_preview bằng lỗi có log.
Test: fake DialogService ghi lại cuộc gọi, khẳng định MỌI phương thức của trait được chuyển (sinh danh sách từ trait, test đỏ nếu thêm phương thức mà quên chuyển); cổng lazy: lần gọi đầu graph chưa gắn -> None, sau khi gắn -> Some; factory panic không lan ra.
CẤM: làm yếu test; thêm #[allow] không lý do.
Kiểm tra: cargo fmt -p pr-shell --check && cargo fmt -p pr-platform-win --check; cargo clippy -p pr-shell -p pr-platform-win --all-targets --locked -- -D warnings; cargo test -p pr-shell -p pr-platform-win --locked.
```

### G8 (trung bình): `OpenInDefaultEditor => todo!()` (b4 M1)
`shell_input/main_window.rs:266` đi tới được bằng phím tắt người dùng gán. Cần quyết định: nối vào W9-E (external editors) hay trả "không hỗ trợ" an toàn tạm thời. Tạm thời Codex chỉ được thay `todo!()` bằng no-op có log + test.

**PROMPT Codex G8**
```
Việc: thay todo!() ở shell_input/main_window.rs:266 (ReviewCommandType::OpenInDefaultEditor) bằng xử lý an toàn (docs/reviews/main-window-input-overlay-viewing.md M1).
Phạm vi: crates/pr-platform-win/src/shell_input/main_window.rs và test mới crates/pr-platform-win/tests/shell_input_open_default_editor.rs. Cấm sửa file khác.
Sửa: gọi dịch vụ editor nếu đã có trait trong pr-core/pr-app; nếu chưa có, ghi log cảnh báo + set_status_text("not available yet") qua sink, KHÔNG panic. Ghi chú TODO trỏ W9-E.
Test: gán phím tắt tới OpenInDefaultEditor và phát phím: không panic, trạng thái được đặt.
Kiểm tra: cargo fmt -p pr-platform-win --check; cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings; cargo test -p pr-platform-win --locked.
```

### G9 sửa mã (trung bình): Settings pages
- settings-ui M1 (`pages/files.rs` thiếu FEAT-EXTERNAL-EDITORS, `context.rs:376-381` `edit_external_editors` là `todo!()`; thuộc W9-E4/E8), M2 (`actions_for_editor` dùng `EnglishSettingsTexts`), M3/L1/L2/L4 (re-entrancy `EN_CHANGE`, `Ref` giữ qua `open_url`, `apply_update_result` đăng lại), L3 (`to_ascii_lowercase` thay `OrdinalIgnoreCase`). w9-contracts M5 (`load_editor`/`apply_editor` lệch C#).
M1 và M5 thuộc Wave 9, làm theo contract; phần còn lại:

**PROMPT Codex G9**
```
Việc: sửa Settings pages (docs/reviews/settings-ui.md M2, M3, L1, L2, L3, L4).
Phạm vi: crates/pr-platform-win/src/dialogs/settings/pages/{shortcuts,general,performance}.rs, crates/pr-platform-win/src/dialogs/settings/context.rs (chỉ phần texts), test: crates/pr-platform-win/tests/settings_page_{shortcuts,general,performance}.rs. Cấm sửa file khác, cấm đụng pages/files.rs và todo!() của W9.
Sửa: (M2) truyền SettingsTexts đã bản địa hoá cho actions_for_editor; (M3, L4) bảo vệ vào lại: cờ "đang đặt text" để on_command bỏ qua EN_CHANGE do chính mình đặt, refresh() gọi đúng một lần; (L1) không giữ Ref của presenter qua open_url: lấy URL ra trước; (L2) apply_update_result dùng try_borrow_mut và hẹn lại có giới hạn số lần; (L3) so khoá bằng eq_ignore_case của pr_core::text.
Test: ngôn ngữ vi + JSON hành động rỗng -> tên hành động tiếng Việt; EN_CHANGE lồng không panic và refresh một lần; borrow lồng trong open_url không panic.
CẤM: làm yếu test; sửa golden.
Kiểm tra: cargo fmt -p pr-platform-win --check; cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings; cargo test -p pr-platform-win --locked.
```

### G10 sửa mã: render/painter
- render-painter M1 (`render/device/resources.rs resize` ~90-123 + `surface.rs begin_draw_context` ~229-248), L1 (`image_cache/policy.rs` giữ `RefCell` qua `upload`), L2 (`draw_context.rs draw_image_oriented` bỏ kết quả `pop_transform`), L3 (`pop_clip` giảm độ sâu dù không pop). L4 (`dialog_proc` gọi `get_class_name` mỗi `NM_CUSTOMDRAW`) và L5 (số học tràn khác C#) là tối ưu/ghi chú, để mức thấp.

**PROMPT Codex G10**
```
Việc: sửa render/painter (docs/reviews/render-painter.md M1, L1, L2, L3).
Phạm vi: crates/pr-platform-win/src/render/{device/resources.rs,surface.rs,draw_context.rs,image_cache/policy.rs}; test mới/hiện có trong crates/pr-platform-win/tests/render_*.rs. Cấm sửa file khác.
Sửa: (M1) sau resize lỗi không phải mất thiết bị, begin_draw_context trả lỗi rõ ràng thay vì vẽ lên surface không target; (L1) không giữ borrow_mut của cache qua backend.upload/upload_rows (tách khỏi borrow, upload rồi borrow lại để ghi vào cache); (L2) draw_image_oriented không bỏ lỗi pop_transform: nếu push bị bỏ thì không pop, ngược lại truyền lỗi; (L3) pop_clip chỉ giảm độ sâu khi thực sự pop.
Test dùng backend giả đã có trong repo: resize lỗi -> begin_draw_context Err; upload gọi lại cache từ trong backend không panic; push_transform lỗi -> không pop; pop_clip khi context None không làm lệch độ sâu.
CẤM: làm yếu test; mutation testing trên glue D2D (AGENTS.md).
Kiểm tra: cargo fmt -p pr-platform-win --check; cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings; cargo test -p pr-platform-win --locked.
```

### G11 sửa mã: native dialog
- native-dialog M3 (`modal.rs:88-98`, `result.rs:109-112`: `closed`/`on_closed` có thể chạy khi unwind), L1 (`tooltip.rs:230-242` buffer realloc), L2 (`result.rs:48-53` giữ `Rc<MessageLoop>`), L3 (`focus.rs:83-89` `first_tab_stop`), L4 (`dpi.rs:151-183` xoá font cũ khi tạo font mới lỗi), L5 (`modal.rs:357-368` copy MSG).

**PROMPT Codex G11**
```
Việc: sửa native dialog base (docs/reviews/native-dialog.md M3, L1, L2, L3, L4; L5 chỉ ghi chú).
Phạm vi: crates/pr-platform-win/src/dialogs/native/dialog/{modal,result,tooltip,focus,dpi}.rs và test cạnh nó (crates/pr-platform-win/tests/dialogs_native_dialog_*.rs). Cấm sửa file khác.
Sửa: (M3) bọc closed/on_closed trong catch_unwind khi đang unwind (hoặc không gọi lại khi std::thread::panicking()); (L1) giữ buffer tooltip sống tới khi tooltip đổi: dùng Vec cố định dung lượng tối đa hoặc cấp mới và giữ buffer cũ cho tới lần cập nhật sau; (L2) cleanup đặt modal.message_loop = None; (L3) first_tab_stop bỏ qua WS_DISABLED/không hiển thị; (L4) chỉ xoá font cũ khi font mới tạo thành công.
Test thuần (không cần desktop) cho L2, L3, L4, M3 bằng seam hiện có; test native để #[ignore] kèm lý do hợp lệ.
CẤM: làm yếu test; đổi golden.
Kiểm tra: cargo fmt -p pr-platform-win --check; cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings; cargo test -p pr-platform-win --locked.
```

### G12 sửa mã + test (thấp): hardening INV
- invariants L1 (`FileActionService::end` không phân biệt người giữ cổng, `service.rs:229`), L2 (nối dây cổng chung do composition; `recovery_retry.rs:301`, `undo.rs:296`), L5 (Undo Recycle đơn không journal, `undo.rs:754`), L6 (`journal_persisted` không tới người dùng), L7 (dọn Copy dở chỉ khi copy đã trả về), L8 (reconcile Recycle chỉ dựa vào "nguồn không còn"), L3, L4 (thuộc Q2). Chỉ L1, L2, L6 nên sửa; L5, L7, L8 khoá hành vi bằng test (parity C#).

**PROMPT Codex G12**
```
Việc: hardening cổng INV-4 và báo lỗi journal (docs/reviews/invariants.md L1, L2, L6; L5, L7, L8 chỉ thêm test khoá hành vi).
Phạm vi: crates/pr-core/src/file_actions/{service,undo}.rs (chỉ phần end/visibility), crates/pr-shell/src/composition/registry.rs (chỉ đọc), crates/pr-app/src/coordinators/file_actions/actions.rs (chỉ đọc journal_persisted để ghi log), test: crates/pr-core/tests/file_actions_service.rs, file_actions_undo.rs, journal_reconcile.rs, crates/pr-shell/tests/composition_registry*.rs, crates/pr-app/tests/file_action_core.rs. Cấm đổi định dạng journal, cấm đổi hành vi parity C# ngoài những điều liệt kê.
Sửa: (L1) end() chỉ được gọi qua RAII: đổi thành pub(crate) nếu không có người gọi ngoài crate, nếu có thì giữ và thêm test khoá hành vi; (L2) thêm test composition: try_begin trên file_action_service() làm undo_service.is_busy() true và retry trả core_recovery_busy; (L6) khi FileActionResult.journal_persisted == false, ghi log lỗi với journal_error (không đổi UI).
Test khoá hành vi: undo_single_recycle_writes_no_journal_line; copy_fails_mid_write_documents_partial_destination (fake FS); recycle_prepared_with_source_missing_documents_committed (journal_reconcile).
CẤM: làm yếu test; thay đổi mã hoá/định dạng dòng journal.
Kiểm tra: cargo fmt -p pr-core --check && cargo fmt -p pr-shell --check && cargo fmt -p pr-app --check; cargo clippy -p pr-core -p pr-shell -p pr-app --all-targets --locked -- -D warnings; cargo test -p pr-core -p pr-shell -p pr-app --locked.
```

### G13 sửa mã (thấp): shell startup (#164)
- pr-164 L1 (`error_log.rs` `report` dựng backtrace trước khi biết có cấu hình), L5 (forwarded-open bị bỏ im lặng khi router chưa gắn), L6 (`StartupTask` nuốt panic, không test), startup L5 (thread hạn chót cloak không khởi động được).

**PROMPT Codex G13**
```
Việc: sửa các điểm nhỏ của shell startup (docs/reviews/pr-164.md L1, L5, L6; docs/reviews/startup.md L5).
Phạm vi: crates/pr-app/src/startup/{error_log,forwarded_open}.rs, crates/pr-app/src/startup/ (StartupTask), crates/pr-platform-win/src/shell_startup/cloak.rs, test: crates/pr-app/tests/startup_*.rs, crates/pr-platform-win/tests/shell_startup_cloak.rs. Cấm sửa file khác.
Sửa: (L1) chỉ dựng backtrace khi cấu hình ghi log bật; (L5) forwarded open khi router chưa gắn: xếp hàng giữ lại (giới hạn) hoặc log cảnh báo, không bỏ im lặng; (L6) StartupTask ghi log khi panic/khởi động luồng lỗi; (startup L5) cloak: nếu spawn lỗi thì tự lộ cửa sổ (reveal) ngay.
Test: mỗi mục một test đỏ trước rồi xanh.
CẤM: làm yếu test; sửa golden.
Kiểm tra: cargo fmt -p pr-app --check && cargo fmt -p pr-platform-win --check; cargo clippy -p pr-app -p pr-platform-win --all-targets --locked -- -D warnings; cargo test -p pr-app -p pr-platform-win --locked.
```

### G14 (thấp): main window nhỏ, G15 (thấp): registry
- G14: b4 L1..L5 (`UiLocalPoster` tự động `Send`; timer không `catch_unwind`; prefetch một luồng OS mỗi yêu cầu; `FrameLog` ghi file đồng bộ; `WM_CONTEXTMENU` (-1,-1)).
- G15: startup L2 (vòng `Arc` trong `SharedServices`), L3 (lỗi tạm lưu `OnceLock<Result>`), L6.

**PROMPT Codex G14+G15** (hai file tập hợp tách riêng nên một lượt, nhưng hai commit)
```
Việc: (A) docs/reviews/main-window-input-overlay-viewing.md L1..L4; (B) docs/reviews/startup.md L2, L3.
Phạm vi A: crates/pr-platform-win/src/host/main_window/ (UiLocalPoster, TimerQueue, prefetch, FrameLog) và test cạnh. Phạm vi B: crates/pr-shell/src/composition/registry.rs và crates/pr-shell/tests/composition_registry*.rs. Cấm sửa file khác. L5 của b4 giữ nguyên (tương đương C#).
Sửa A: UiLocalPoster thêm PhantomData<*const ()> để !Send; TimerQueue bọc callback bằng catch_unwind và log lỗi tạo luồng; prefetch dùng một luồng dùng chung thay vì một luồng mỗi yêu cầu; FrameLog đệm trong bộ nhớ và ghi ở luồng nền.
Sửa B: giữ Weak thay vì Arc trong closure của preload_scheduler_factory/decoder_decorator; không lưu lỗi tạm trong OnceLock (thử lại lần sau).
Test: mỗi mục một test (compile_fail cho !Send bằng static_assertions nếu repo đã dùng, nếu chưa thì test Rust-only bằng assert_not_impl).
CẤM: làm yếu test; đổi hành vi hiển thị.
Kiểm tra: cargo fmt -p pr-platform-win --check && cargo fmt -p pr-shell --check; cargo clippy -p pr-platform-win -p pr-shell --all-targets --locked -- -D warnings; cargo test -p pr-platform-win -p pr-shell --locked.
```

### G16 sửa mã (script): autopilot
- autopilot H1 (`claim.sh:20` fetch không `--prune`), M1 (`testguard` chỉ kiểm dãy con; thất bại im lặng khi không đọc được `base`), M2 (`cmd_lint` coi mọi thứ sau `#[cfg(test)]` đầu tiên là test), M3 (`extsat` coi nhắc tên unit là đã merge), M4 (`claim.sh gc` dùng `--grep` regex), M5 (khoá merge không tự phục hồi thiếu `pid`), M6 (INT/TERM giữ tiến trình con), M7 (`unit-pr.sh` để worktree bẩn), L1..L11.

**PROMPT Codex G16**
```
Việc: sửa tools/autopilot theo docs/reviews/autopilot.md (H1, M1..M7, L1..L11).
Phạm vi: tools/autopilot/{claim.sh,codex-gate.sh,codex-pipeline.sh,codex-pack.sh,codex-run.sh,codex-usage.sh,status.sh,unit-pr.sh,lib/codex-lib.py,lib/fmtcheck.sh,test/pipeline-selftest.sh}. Cấm sửa mã Rust và tài liệu khác.
Ưu tiên: H1 (git fetch --prune '+refs/claims/*:refs/claims/*' hoặc xoá ref cục bộ không còn trên origin), M4 (git log -F --grep với neo "(unit)" hoặc so tên unit chính xác), M1/M2/M3 (sửa thuật toán, xem kịch bản trong báo cáo), M5/M6/M7 (trap, pid file ghi nguyên tử, dọn worktree), rồi L.
Test: mỗi sửa có một ca trong test/pipeline-selftest.sh (script phải chạy được trên bash của Git for Windows và Linux; không dùng lệnh chỉ có ở một nền).
CẤM: làm yếu selftest; xoá kiểm tra để pass; chạy gh pr merge/push thật trong selftest.
Kiểm tra: bash tools/autopilot/test/pipeline-selftest.sh; shellcheck nếu có; python -I -m py_compile tools/autopilot/lib/codex-lib.py.
```

### G17 thêm test: CI/ignore/pass giả
- test-quality M1 (CI không chạy `--ignored`; 1157 test không chạy), M2 (`#[ignore]` stale), M3 (đạt giả khi thiếu env/dữ liệu), M4 (tên hứa so khớp, chỉ không panic), L1..L3; native-dialog M1 (`run_case` có thể pass mà không chạy kiểm tra), M2 (100 test native ignore), L6; settings-ui L5; b4 M2 (82/120 `host_main_window_*` ignore); pr-203 M1 (test trùng, không giết mutant), L2; test-strengthening-plan §crate pr-bench (4 test chặn ghi đè qua hard link/junction bị ignore), pr-core 22 test thật bị ignore, pr-turbojpeg/pr-libraw đạt giả, pr-perf `divergence_*`.
Q7 đã chốt ở P-52: đề xuất job native nightly ở docs/drafts trước. Phải dùng danh sách test được duyệt; không chạy hàng loạt test Recycle Bin.

**PROMPT Codex G17** (mỗi crate một lượt; ví dụ cho pr-platform-win, thay phạm vi cho crate khác)
```
Việc: biến test giả/ignore stale thành test thật (docs/reviews/test-quality.md M2, M3, M4, L1..L3 + docs/reviews/test-strengthening-plan.md mục crate đang làm).
Phạm vi: chỉ các file test trong crates/<crate>/tests/ (và fixture tests/common/); cấm sửa src/. Nếu một test lộ lỗi sản phẩm thật: giữ nguyên assertion, ghi kết quả đỏ và giao chủ sở hữu sửa trong PR riêng; không thêm ignore để làm xanh.
Với mỗi mục: bỏ #[ignore] khi lý do đã hết hiệu lực; test đạt giả khi thiếu env phải panic/fail (hoặc #[ignore] với lý do native/slow hợp lệ), không return sớm im lặng; test tên "so khớp" phải so giá trị chính xác (lấy từ golden hoặc từ test C# trong báo cáo).
CẤM: làm yếu bất kỳ khẳng định nào; xoá test; thêm sleep/poll cố định.
Kiểm tra: cargo fmt -p <crate> --check; cargo clippy -p <crate> --all-targets --locked -- -D warnings; cargo test -p <crate> --locked (không --include-ignored trừ khi nói rõ).
```

### G18 thêm test: #164
- pr-164 M1 (bốn golden `shell-launch`, `shell-error-log`, `startup-cloak`, `shell-show-command` không được so sánh; `startup_error_log.rs:105-108`, `startup_launch.rs:133-137` không assert; `shell_startup_cloak.rs:285-289` chỉ `contains("hide")`; `shell_startup_placement.rs:319-331` thiếu `Minimized`; đổi `get(..19)` thành `get(..10)` ở `error_log.rs:121` không làm test nào đỏ), M2 (`ensure_per_monitor_v2` không test), M3 (4/11 test `startup_forwarded_open.rs` không assert), L2 (cắt 300 UTF-16 không có surrogate), L3, L4.

**PROMPT Codex G18**
```
Việc: viết test thật cho golden và startup của #164 (docs/reviews/pr-164.md M1, M2, M3, L2, L3, L4).
Phạm vi: crates/pr-app/tests/startup_error_log.rs, startup_launch.rs, startup_forwarded_open.rs; crates/pr-platform-win/tests/shell_startup_cloak.rs, shell_startup_placement.rs, shell_startup_dpi.rs (mới). Cấm sửa src/ và tests/golden-inputs/.
Việc: đọc 4 golden (shell-launch, shell-error-log, startup-cloak, shell-show-command) và so từng ca với kết quả thật, theo contract W8-17a/17b ("every golden case is compared"); thêm shell_startup_dpi.rs cho ensure_per_monitor_v2 qua seam (NativeDpiAwareness giả); thêm assert thật cho 4 test forwarded-open rỗng; test cắt 300 đơn vị UTF-16 với cặp surrogate ở biên.
Kiểm tra mutation tay (không cần cargo-mutants): đổi get(..19) thành get(..10) ở error_log.rs:121 trên bản nháp local phải làm một test đỏ (rồi hoàn tác; KHÔNG commit thay đổi src).
CẤM: làm yếu test; sửa golden.
Kiểm tra: cargo fmt -p pr-app --check && cargo fmt -p pr-platform-win --check; cargo clippy -p pr-app -p pr-platform-win --all-targets --locked -- -D warnings; cargo test -p pr-app -p pr-platform-win --locked.
```

### G19 thêm test: INV
- invariants §5: mất cache journal Fast vs PowerLossSafe (Q1 đã chốt giữ Fast), composition cổng chung (đã trong G12), dọn trùng lặp (trong G6), panic executor (trong G5), Move chéo ổ lỗi nửa chừng (`file_actions_service_native.rs`, 19 test native bị ignore, cần hai ổ hoặc fake). test-strengthening-plan: `tp-pr-core` (12) và `tp-pr-app` (13).

**PROMPT Codex G19**
```
Việc: thêm test INV còn thiếu (docs/reviews/invariants.md §5 + docs/reviews/test-strengthening-plan.md mục pr-core, pr-app).
Phạm vi: crates/pr-core/tests/{file_actions_journal,file_actions_service,file_actions_matrix}.rs, crates/pr-app/tests/{file_action_core,duplicate_cleanup}.rs. Cấm sửa src/.
Test: (1) kiểm đúng open_append(durable) và thứ tự write/flush; stream fake không phải File chỉ plain flush ngay cả PowerLossSafe (journal.rs:675-684), không bịa mô hình fake chứng minh fsync. Mô hình mất cache phải khai báo riêng write-through và volatile/stable storage; đối chiếu test native hiện có, không nhận đó là thử mất điện; (2) Move chéo ổ lỗi nửa chừng bằng fake FS: nguồn còn, đích đầy đủ -> Failed, hành vi hiện tại được khoá; (3) các test tp-pr-core/tp-pr-app trong test-strengthening-plan, mỗi test mirror tên C# ghi trong kế hoạch.
Nếu test lộ lỗi: giữ assertion và ghi kết quả đỏ cùng mã báo cáo; không thêm ignore để làm xanh.
CẤM: làm yếu test; sửa src/.
Kiểm tra: cargo fmt -p pr-core --check && cargo fmt -p pr-app --check; cargo clippy -p pr-core -p pr-app --all-targets --locked -- -D warnings; cargo test -p pr-core -p pr-app --locked.
```

### G20 thêm test: Rename / StalePreview
- test-strengthening-plan: Rename (W9-R2/R4/R5) chưa có test; họ `StalePreviewOverwriteTests` (17 test C#, hoãn ở contract W8-06c) chưa có bản Rust; chỉ có bản ngẫu nhiên 5 seed ở `presenter_present_a.rs:1278`. Làm sau khi stub W9-R tương ứng có thân thật; trước đó chỉ test tên đích bị chiếm/lược đồ journal ở mức contract.

**PROMPT Codex G20**
```
Việc: port StalePreviewOverwriteTests (17 test C#) và test Rename theo docs/reviews/test-strengthening-plan.md (mục pr-app, pr-core, W9-R2/R4/R5).
Phạm vi: crates/pr-app/tests/presenter_stale_preview_overwrite.rs (mới), crates/pr-core/tests/file_actions_rename_*.rs (mới, #[ignore = "pending W9-Rn"] CHỈ khi hàm vẫn todo!()). Cấm sửa src/.
Nguồn: tests/PhotoReview.App.Shared.Tests/... StalePreviewOverwriteTests ở C# baseline 7eaee2a2 (grep tên trong kho C#, không đoán).
CẤM: làm yếu test; bỏ ca khỏi danh sách 17.
Kiểm tra: cargo fmt -p pr-app --check && cargo fmt -p pr-core --check; cargo clippy -p pr-app -p pr-core --all-targets --locked -- -D warnings; cargo test -p pr-app -p pr-core --locked.
```

### G21 tài liệu: contract W9
- w9-contracts H1 (nội dung "Rust đang có/thiếu gì" lỗi thời sau #182; bảng Survey của `docs/WAVE-9-PLAN-edt.md` chưa sửa), L6 (E5 mục 5), L7 (chữ ký stub khác contract), L8 (trạng thái "staged" oracle R1), L9, L10 (thay đổi test C# không có đơn vị chủ), L11. Phần đã sửa nằm trong PR #207.

**PROMPT Codex G21**
```
Việc: sửa tài liệu Wave 9 còn lại (docs/reviews/w9-contracts.md H1, L6..L10).
Phạm vi: contracts/W9-*.md, docs/WAVE-9-PLAN-edt.md, docs/WAVE-9-PLAN-ren.md, docs/WAVE-9-PLAN.md. Cấm sửa mã.
Sửa: cập nhật bảng Survey và các câu "thiếu" theo master hiện tại (grep crates/ để chứng minh từng câu); sửa chữ ký stub theo mã; gán đơn vị chủ cho thay đổi test C# nêu ở L10; mỗi sửa ghi bằng chứng (file:dòng) trong mô tả commit.
CẤM: bịa dòng/tên; sửa phần "cần quyết định" (G24).
Kiểm tra: git grep -n -I '^<<<<<<<' ; file UTF-8 không BOM, LF; không chạy cargo.
```

### G22 tài liệu: PARITY và bằng chứng PR
- pr-164 M4 (thân PR ghi gate FAILED, "manual finish" không nói đã sửa gì; PARITY dòng 632-634 vẫn `Contract`/"planned"), pr-203 M2 (lỗi vòng tham chiếu cửa sổ chính ghi trong PARITY mà ô tự mâu thuẫn, chưa có người nhận). Chạm `docs/PARITY.md`: sửa từng hàng, không lấy một phía của file.

**PROMPT Codex G22**
```
Việc: làm PARITY.md trung thực (docs/reviews/pr-164.md M4, docs/reviews/pr-203.md M2).
Phạm vi: docs/PARITY.md (chỉ các hàng W8-17a/17b/17c và W8-13h..13k), task_on_progress.md (thêm một mục mới theo ngày, không sửa mục máy khác). Cấm sửa mã.
Sửa: trạng thái W8-17a/17b/17c theo mã thực tế trên master (grep crates/ chứng minh); hàng 13h..13k: bỏ mâu thuẫn trong ô, ghi rõ lỗi vòng tham chiếu cửa sổ chính và nhóm G1 là chủ.
Kiểm tra: không có dấu xung đột; UTF-8 không BOM; không chạy cargo.
```

### G23 tài liệu (thấp): chú thích lạc hậu
- test-quality L1 (`DIVERGENCE`/`TODO(lead)` đã lạc hậu), L3 (test khẳng định hằng với chính nó), w9-contracts L8, settings-ui L5. Nên ghép vào G17 cho phần test; phần comment trong src/ do Codex làm trong G17 (chỉ comment).

### G24 quyết định sản phẩm: ĐÃ CHỐT 2026-10-23 theo đề xuất (P-52 trong `docs/PROPOSED-DECISIONS.md`)
Q1 giữ Fast; Q2 giữ cờ Shell + cảnh báo chờ 10 s; Q3 kiểm lại danh tính (mở khoá G6), chưa Undo nhóm; Q4 no-op có thông báo (mở khoá G8); Q5 `PathKeyMap` ở W9-R1; Q6 R11/R9/E4 như bảng; Q7 job `native` hằng đêm, chưa chặn PR; Q8 xác minh `pr_l10n::tr` (đã thấy module `tr` ở `lib.rs:12`). Bảng gốc giữ lại bên dưới.
| Mã | Câu hỏi | Phương án gợi ý | Ghi chú |
|---|---|---|---|
| Q1 invariants M2 | Đổi mặc định `JournalDurability` sang `PowerLossSafe`? | Giữ Fast (parity C#, ADR 0007) hoặc đổi, thêm một fsync mỗi action | Cần ADR; G19 khoá hành vi hiện tại |
| Q2 invariants L3 | Thêm `FOF_NOERRORUI`/hộp thoại có chủ cho Recycle? | Giữ như C# hoặc thêm cờ + báo lỗi bằng UI của app (Different row) | Cổng INV-4 treo khi hộp thoại ẩn |
| Q3 invariants M1 | Dọn trùng lặp có vào Undo/journal nhóm? | (a) kiểm lại danh tính rồi recycle (G6), (b) thêm Undo nhóm (lệch C#) | G6 chờ chọn |
| Q4 b4 M1 | `OpenInDefaultEditor` làm gì trước khi W9-E xong? | no-op có thông báo (G8) hoặc chờ W9-E | |
| Q5 w9-contracts M2 | Kiểu map đường dẫn không phân biệt hoa thường dùng chung | một `PathKeyMap`/chuẩn hoá khoá ở R1 | chặn R2..R8 |
| Q6 w9-contracts M3, M4, M5 | `RenamePromptDialog` thiếu thành viên R11; ai sở hữu bộ bất biến fault-injection nhận biết hoa thường; `load_editor`/`apply_editor` lệch C# | giao đơn vị cụ thể | |
| Q7 test-quality M1 | CI có chạy test native `--include-ignored` trên Windows job riêng? | job `native` nightly hoặc theo nhãn PR | 1157 test hiện không chạy |
| Q8 w9-contracts L11 | Giả định "khoá catalog sinh accessor `tr::`" của E8/E9 | xác nhận hoặc đổi tên | |

## 3. Ghi chú trùng lặp đã gộp
- `SharedDialogService` thiếu 3 prompt: startup M1 = w9-contracts M1; liên quan dialogs-base M1 (`Win32DialogService` thiếu `prompt_rename`). -> G7.
- Cổng lazy lưu None vĩnh viễn: startup M3, L3, dialogs-base L1/L2. -> G7.
- Re-entrancy `RefCell`: b4 H1/H2, settings-ui M3/L1/L2, render-painter L1, pr-203 M2, native-dialog M3 (unwind). -> G1, G9, G10, G11.
- Test rỗng/ignore: test-quality M1..M4, native-dialog M1/M2/L6, settings-ui L5, b4 M2, pr-164 M1..M3/L3/L4, pr-203 M1. -> G17, G18.
- Thời gian Recovery: startup H1/L1/L6 và test-strengthening-plan `tp-pr-platform-win T2`. -> G3.
- Settings General: settings-ui H1 và `tp-pr-platform-win T1`. -> G4.

## 4. Chưa gộp / ngoài phạm vi
- `test-strengthening-plan.md` có 80 test; chỉ gom theo nhóm G17, G19, G20. Các mục crate khác (pr-imaging, pr-turbojpeg, pr-libraw, pr-imaging-raw, pr-perf, pr-l10n, pr-bench, pr-test-support) dùng prompt G17 với phạm vi crate đó.
- INV-1, 2, 8, 10, 11, 12 (pr-imaging, settings store, file_log) chưa được đọc sâu trong invariants.md: chưa có mục trong backlog; cần review riêng.
- Lần tái kiểm này theo đúng thứ tự INVARIANTS -> BACKLOG -> PRS -> B4 -> W9 -> TEST-PLAN. Báo cáo các lần trước giữ làm nguồn, phát hiện sau mốc này cần ghi rõ chưa nằm trong snapshot tổng hợp.
## 5. Tái tổng hợp ngày 2026-10-23 (sau #211 và #212)

**Mọi phát hiện: suy luận từ mã, chưa chạy thử.** Snapshot gồm mọi báo cáo có trong `docs/reviews/` ở `e25f8bf`. Không chạy test để xác nhận lại số liệu lịch sử 10103/1157; chúng thuộc lần quét `test-quality.md`, không phải số hiện tại.

### G25 sửa mã + test (cao, dữ liệu): retry nhóm Recycle sau Prepared

Nguồn duy nhất: `invariants H1` (#212). Không gộp với G6: G6 là lựa chọn file từ quét duplicate trước modal, G25 là recovery của journal Failed/Prepared, gồm permanent-delete. W3-06 sở hữu logic; W8-18v chỉ gọi recovery, sửa tại UI không đủ. Không gộp với G19: G19 thuần thêm test, G25 cần thay hành vi và ADR vì C# cùng thiếu chốt re-stat.

**PROMPT Codex G25**
```
Việc: chặn RecoveryRetryService xoá/recycle file thay thế sau Prepared (docs/reviews/invariants.md H1).
Đọc: AGENTS.md, contracts/W3-06.md, docs/reviews/invariants.md §6 và C# RecoveryRetryService.cs tại 7eaee2a2. Kiểm đường contract thật bằng rg --files; không đoán API.
Phạm vi được sửa: crates/pr-core/src/file_actions/recovery_retry.rs, crates/pr-core/tests/recovery_retry.rs, crates/pr-core/tests/recovery_retry_support/mod.rs; docs/adr/ (một ADR mới cho khác biệt an toàn), docs/PARITY.md (chỉ hàng recovery retry), task_on_progress.md. Không sửa adapter native hay file khác.
Trước mutation mỗi member Recycle non-undo: re-stat qua try_get_file_stat, so size và last_write_utc với member journal; thiếu/đổi/lỗi => Failed, không gọi bin cho member đó. Giữ guard loại ổ và chính sách permanent-delete hiện tại; không tự rollback member đã xong. Kiểm cả sau append Prepared và khi member trước đang chạy. ADR phải nói re-stat thu hẹp khoảng race, không khoá nguyên tử tiến trình ngoài; quyết định sản phẩm/parity phải được ghi rõ, không tự coi khác biệt là parity.
Test đỏ trước: fake journal append hook thay B sau Prepared (khác size, rồi cùng size khác mtime); fake bin xử lý A rồi thay B. Assert B bytes nguyên vẹn, không có delete_permanently/send_to_recycle_bin(B), journal Failed và lỗi SourceChanged. Ca size/mtime không đổi vẫn xử lý đúng. Dùng fake bin toàn bộ, không native bin.
CẤM: xoá/ignore/làm yếu assertion hiện có; thay golden để khớp mã; test chỉ kiểm không panic; nới fingerprint; sửa mã ngoài phạm vi.
Kiểm tra: cargo fmt -p pr-core --check; cargo clippy -p pr-core --all-targets --locked -- -D warnings; cargo test -p pr-core --locked --test recovery_retry; cargo test -p pr-core --locked. Hang guard theo tools/wave.ps1 hoặc timeout ngoài tiến trình; báo đúng lệnh chưa chạy. Mutation chỉ logic owned, sau merge theo quy tắc repo, không mutation bin/native.
```

### Phân loại và các quyết định đã có

- Sửa mã: G1..G16, G25 (G16 là script, không phải Rust). G25 ưu tiên vì có nhánh mất file không phục hồi bằng bin.
- Thêm/siết test: G17..G20; G25 kèm test hồi quy của cùng thay đổi. 80 đề xuất crate còn lại được tham chiếu theo `tp-<crate> Tn`, không tính lại thành phát hiện mới.
- Tài liệu: G21..G23; thêm đính chính INV-5/APP-03 vào phạm vi G22 (catalog/session folder mới không đổi, Undo cho action muộn thành công vẫn được đăng ký).
- Quyết định sản phẩm: Q1..Q8 đã chốt P-52/#211, không yêu cầu người dùng chọn lại. G25 là rủi ro mới, chưa thuộc Q1..Q8; cần ADR parity cho biện pháp chặn.
- ADR-INV Q7 hiện đề nghị `cargo test --workspace --locked -- --include-ignored`. Không chuyển nguyên lệnh đó thành automation: AGENTS.md cấm sweep/empty bin thật. Prompt triển khai job phải chia danh sách test native an toàn, review từng test bin và chỉ cho xoá các mục TempRoot do nó tạo; bin fault/mutation dùng fake. Đây là ràng buộc đã có, không phải yêu cầu quyền mới.

### Ba điểm cần tránh khi giao việc

1. G1/G7/G9 có thể chạm cùng call path: giao tuần tự hoặc tách ownership rõ, không coi chúng độc lập hoàn toàn.
2. Test đỏ vì lỗi thật không được che bằng `#[ignore = "BUG"]`. Bằng chứng phải ghi ca đỏ và giao sửa nguyên nhân; native/slow vẫn theo quy tắc repo.
3. `test-quality` gợi ý đường golden bằng `CARGO_MANIFEST_DIR/../../...` ở ví dụ 14, trái quy tắc test không hard-code repo-relative path. Dùng helper golden đã có của pr-test-support, dữ liệu test qua TempRoot khi cần; không sao chép lời gợi ý đường dẫn cũ vào test mới.
