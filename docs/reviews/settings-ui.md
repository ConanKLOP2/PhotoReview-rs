# Review tĩnh: Settings UI (W8-19a..19i)

Machine C (cloud). Phạm vi: `crates/pr-platform-win/src/dialogs/settings/` (builder, context, page_view, catalog, pages/*) và `crates/pr-platform-win/tests/settings_*.rs`.
So với C# tại `7eaee2a2` (`SettingsUi/Pages/*.cs`, `SettingsPageBuilder.cs`).

**Mọi phát hiện dưới đây: suy luận từ mã, chưa chạy thử.** Không chạy cargo, không chạy test.

Tổng: 1 cao / 3 trung bình / 5 thấp.

## Đã đọc và thấy đúng (không có phát hiện)
- `builder/inputs.rs`, `rows.rs`, `groups.rs`, `layout.rs`: công thức Row/Slider/Text/Buttons/Check/Radio/CheckFlow/Group/KeyGrid, `add()`, `_breakGroup`, thay `&` bằng `&&` ở nút khớp C#.
- `context.rs`, `page_view.rs`, `catalog.rs` (factory, `PendingSettingsPage`).
- Trang `diagnostics`, `performance` (kể cả `round_ties_even` = `Math.Round`), `display` (thứ tự EXIF/TITLE, `update_dependents`), `mouse` (declare/load/collect khớp từng dòng).

## Phát hiện

### H1 (cao, xác nhận bằng cách đọc): `GeneralSettingsPage::collect` không đọc View mode và Instance mode
- File: `pages/general.rs:258-259` (C#: `GeneralSettingsPage.cs` `Collect`: `state.ViewModeIndex = Context.ComboIndex(_viewMode)`).
- Mô tả: Rust gọi `c.set_combo_index(view_mode, state.view_mode_index)` và `set_combo_index(instance_mode, ...)` (GHI từ state vào control) thay vì đọc `combo_index` vào state.
- Kịch bản: mở Settings, đổi "View mode" từ 0 sang 1, bấm Save → `collect` ghi lại 0 vào combo và `state.view_mode_index` giữ 0 → cài đặt không đổi, lựa chọn của người dùng bị bỏ. Tương tự Instance mode.
- Vì sao test không bắt được: `settings_page_general.rs` chỉ kiểm `load` cho hai combo này (dòng 46-59, 122-135); `collect` chỉ được kiểm cho `sort_mode_tag` và `selected_language_code`.
- Gợi ý sửa: `state.view_mode_index = c.combo_index(ids.view_mode)`; thêm test load → đổi combo → collect cho hai trường.

### M1 (trung bình, xác nhận): Rust thiếu phần FEAT-EXTERNAL-EDITORS của trang Files so với `7eaee2a2`
- File: `pages/files.rs` (declare 108-139, load 150-165, collect 167-179); `context.rs:376-381` `edit_external_editors` là `todo!()`.
- C# `FilesSettingsPage.cs` có nút "Manage editors…", ô "send RAW", dòng tóm tắt, danh sách `_editors`/`_defaultEditorId` (load/collect).
- Kịch bản: mở Settings → không có nút Manage editors; `ExternalEditors` trong state không đổi bởi trang. Là khoảng trống đã biết (W9-E4), không phải hồi quy; nhưng `todo!()` ở context.rs:376-381 sẽ panic trên UI thread nếu một trang nào sau này gọi nó (hiện grep không thấy chỗ gọi).
- Gợi ý: ghi rõ ở PARITY; khi làm W9-E4 thay `todo!()`.

### M2 (trung bình, xác nhận): `actions_for_editor` dùng `EnglishSettingsTexts`, C# dùng bản địa hoá
- File: `pages/shortcuts.rs` `edit_actions` (gọi `SettingsPresenter::actions_for_editor(&actions_json, &EnglishSettingsTexts)`); C#: `ShortcutsSettingsPage.cs:218` → `ActionsForEditor` → `ReviewAction.Defaults()` dùng `Tr.ActionDefaultName(n)`/`Tr.ActionDefaultBackup`.
- Kịch bản: ngôn ngữ UI là Tiếng Việt, JSON hành động rỗng/null → trình sửa mở với tên hành động tiếng Anh ("Group-2", "Backup" kiểu English) thay vì tên đã dịch; `default_name` cho action thiếu tên cũng tiếng Anh. Lưu lại sẽ ghi tên tiếng Anh vào config.
- Chưa chắc mức độ hiển thị thực vì không biết `action_default_name` của bản English trả gì so với `Tr`; điểm chắc chắn là tham số khác nguồn.
- Gợi ý: truyền bản texts theo `Localizer` hiện tại (cùng cách `with_field_context` đang dùng).

### M3 (trung bình, chỉ nghi ngờ): vào lại (re-entrancy) qua `EN_CHANGE` khi trang đang `&mut self`
- File: `pages/shortcuts.rs` `on_command` nhánh nút Clear (`set_text(box, "")` rồi `refresh()`), `load` (`set_text` từng ô rồi `refresh()`), `reset_keys`; tương tự `performance.rs` `load` (`set_text(forward)` kích `EN_CHANGE`).
- C# dựa vào việc `SetText` phát `EN_CHANGE` đồng bộ → `OnCommand` → `Refresh` (comment "EN_CHANGE of the box refreshes the warning"). Rust vừa `set_text` (có thể phát `EN_CHANGE` đồng bộ vào chính `on_command` của trang đang giữ `&mut self`) vừa gọi `refresh()` tường minh.
- Kịch bản: host thật (khung dialog 19j+, hiện là `todo!()` nên không kiểm được) giữ trang trong `RefCell`/con trỏ thô; `SetWindowTextW` → `WM_COMMAND/EN_CHANGE` → `on_command` lồng nhau → hoặc `BorrowMutError` panic trên UI thread (vi phạm R0003) hoặc aliasing `&mut`. Phụ thuộc hoàn toàn vào cách host chặn/hoãn thông báo khi set text.
- Chưa chắc: cần xem 19j-19m khi có. Gợi ý: host đặt cờ "đang set text lập trình" để nuốt `EN_CHANGE`, hoặc trang tự quản và bỏ gọi `refresh()` trùng.

### L1 (thấp, xác nhận): `Ref` của presenter giữ qua `open_url`
- File: `pages/general.rs` `on_command` nhánh `open_update_page`: `c.presenter().borrow().open_update_page(&mut |url| c.open_url(url), f)`.
- Kịch bản: `open_url` (ShellExecute) có thể bơm thông điệp; một handler lồng gọi `presenter().borrow_mut()` (ví dụ `shortcuts.rs` `refresh` dùng `borrow_mut()` không `try_`) → panic trên UI thread. `apply_update_result` đã dùng `try_borrow_mut` nên an toàn. Quy tắc D-W8B4I-1 "không giữ borrow qua gọi OS". Chưa chắc ShellExecute có bơm trong trường hợp này.
- Gợi ý: lấy URL từ presenter (borrow ngắn) rồi gọi `open_url` sau khi thả borrow.

### L2 (thấp, xác nhận): `apply_update_result` tự đăng lại nếu presenter đang mượn
- File: `pages/general.rs` cuối file. Nếu borrow kéo dài (ví dụ trong lúc hộp thoại modal con chạy) thì hành động được đăng lại liên tục (vòng bận) cho tới khi nhả. Không mất dữ liệu; chỉ tốn CPU và chưa chắc xảy ra. Gợi ý: đăng lại có trễ/giới hạn số lần.

### L3 (thấp, xác nhận): `ShortcutsSettingsPage::find_problems` so khoá bằng `to_ascii_lowercase`, C# dùng `OrdinalIgnoreCase`
- File: `pages/shortcuts.rs` `find_problems`. Tên phím hợp lệ chỉ ASCII nên không khác biệt thực tế; ghi lại vì khác ngữ nghĩa cho chuỗi không ASCII (đã bị `try_parse` loại trước đó).

### L4 (thấp, xác nhận): ID tooltip/clear cell
- File: `pages/shortcuts.rs` `on_command`: nhánh Clear gọi `refresh()` sau `set_text`; nếu host phát `EN_CHANGE` thì `refresh()` chạy hai lần (vô hại, tốn thêm). Liên quan M3.

### L5 (thấp, xác nhận): test stale `#[ignore]` ghi lý do đã hết hiệu lực
- `tests/settings_framework.rs:672` ("SettingsPageBuilder is a stub (todo!) until then") và `:766` ("page constructors are stubs until then") — builder và 7 trang đã được cài đặt; hai test (`pending_page_declares_one_unaligned_note_with_the_pending_text`, `real_create_pages_returns_the_seven_pages_in_nav_order`) bị bỏ qua mà không còn lý do, nên không có test nào xác nhận catalog thật. Gợi ý: bỏ `#[ignore]`, chạy lại.
- `tests/settings_builder_layout.rs:209` test rỗng `placed_rectangles_are_the_window_rectangles_and_siblings_do_not_overlap() {}` ignore "needs desktop": không có thân hàm, không kiểm gì.

## Test yếu / rỗng (suy luận từ mã)
- `settings_page_mouse.rs:4-7` `mouse_page_constructor_is_implemented`: chỉ `let _ = MouseSettingsPage::new();`, tên chú thích C# `Mouse_LoadsEveryProperty_AndSaveWritesThemBackUnchanged` nhưng không load/collect gì. Trang Mouse (19 trường + 18 ô menu) không có test hành vi nào.
- `settings_pages_diag_perf.rs:14-23` `diagnostics_logging_round_trips`: gọi `host.put_text(id, "x")` rồi kết thúc, 0 assert, không load/collect; tên nói "round trips".
- `settings_pages_diag_perf.rs:36-47`, `:55-` : `golden_*_declare_matches_the_csharp_dump` gọi `let _ = dump(&b)` và `let _ = run_ops(...)` rồi bỏ kết quả — không so với golden dù tên nói vậy; chỉ `assert_eq!` vài id.
- `settings_pages_diag_perf.rs:61-66` `focus_target_maps_the_two_preload_fields`: `let _ = p.focus_target(...)`, 0 assert.
- `settings_builder_layout.rs:99-130, 132-175` (`group_place_honours_a_hidden_child...`, `check_flow_does_not_skip_hidden_items`, `radio_hidden_hint_alone_is_still_placed`, `label_width_does_not_panic...`, `layout_arithmetic_saturates_at_i32_extremes`, `layout_elements_are_object_safe`): `let _ = e.place(...)`, 0 assert — chỉ kiểm không panic, dù tên (ví dụ "does_not_skip_hidden_items") hứa kết quả cụ thể.
- Độ phủ trang: không có test `load`/`collect` cho Display, Files (chỉ `initial_folder`), Mouse, Performance, Diagnostics; General chỉ phủ một phần (nguyên nhân H1 lọt qua).
- Không thấy test nào sửa kỳ vọng cho khớp mã thay vì C# trong phạm vi đã đọc (chưa so toàn bộ golden).

## Không kiểm tra được
- Mọi hành vi thật trên Windows: tạo control, `EN_CHANGE` đồng bộ (M3), ShellExecute (L1), tab order, DPI/layout thật.
- Khung dialog Settings (19j-19m, `todo!()`) — cách host giữ trang, gửi `EN_CHANGE`, `PostToUi`.
- Luồng kiểm tra cập nhật thật (worker, mạng) và `change_shell_registration` (registry HKCU).
- Chưa đối chiếu từng giá trị golden JSON (`settings-*`) với C#; chưa đọc toàn bộ `settings_framework.rs` (783 dòng) và `settings_page_shortcuts.rs` (415 dòng) từng dòng.
