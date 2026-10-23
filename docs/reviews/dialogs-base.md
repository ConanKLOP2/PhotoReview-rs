# Review tĩnh: dialogs nền (W8-18a..18c)

Machine C (cloud). Phạm vi: `crates/pr-platform-win/src/dialogs/{file_dialog,folder_dialog,clipboard,task_dialog,dialog_service,zoom_prompt_service}.rs` và các test `dialogs_clipboard.rs`, `dialogs_folder_picker.rs`, `dialogs_native_pickers.rs`, `dialogs_service.rs`, `dialogs_task_dialog_native.rs`.
So với C# tại `7eaee2a2` (`NativeShellFileDialog.cs`, `NativeShellFolderDialog.cs`, `Win32ClipboardService.cs`, `Win32DialogService.cs`).

**Mọi phát hiện dưới đây: suy luận từ mã, chưa chạy thử.** Không chạy cargo, không chạy test.

Tổng: 0 cao / 1 trung bình / 3 thấp.

## Đã đọc và thấy đúng
- `file_dialog.rs`, `folder_dialog.rs`: thứ tự `GetOptions/SetOptions/SetTitle/SetFileTypes/SetFileTypeIndex(1)/SetDefaultExtension/SetFileName/SetFolder` khớp C#. Bộ đệm bộ lọc (`FilterBuffers`) sống tới hết `run` nên con trỏ trong `COMDLG_FILTERSPEC` hợp lệ suốt `Show`; `Vec<u16>` không dời khi struct di chuyển. Huỷ = `-2147023673` = `0x800704C7`. Hằng `FOLDER_OPTIONS` 0x860 (2144), `OPEN_OPTIONS` 0x1840 (6208), `SAVE_OPTIONS` 0x8842 (34882) khớp golden. `take_co_task_string` giải phóng đúng một lần, kể cả chuỗi UTF-16 sai.
- `clipboard.rs`: `HGLOBAL` được giải phóng trên mọi đường lỗi (lock hỏng, `EmptyClipboard` hỏng, `SetClipboardData` hỏng) và chỉ "nhường" khi thành công; `CloseClipboard` qua `CloseGuard` kể cả khi lỗi; kích thước khối `(n+1)*2` có kiểm tràn; thử lại 5 lần, ngủ 4 lần giữa các lần khớp C#.
- `task_dialog.rs`: `cbSize` do `new_task_dialog_config`; con trỏ trong `config` (tiêu đề, nội dung, dữ liệu callback) trỏ vào biến cục bộ/`self` còn sống tới khi `TaskDialogIndirect` trả; callback bọc `pending_panic::guarded` nên panic không đi xuyên khung OS, rồi `rethrow_pending` sau khi hộp thoại đóng; icon là giá trị 16-bit.
- `zoom_prompt_service.rs`, `LazySecondaryPort`: gọi lại (re-entrant) trả `None`, không chạy factory hai lần.

## Phát hiện

### M1 (trung bình, xác nhận bằng cách đọc): `Win32DialogService` không chuyển `prompt_rename` sang cổng cửa sổ phụ
- File: `dialogs/dialog_service.rs` — khối `impl DialogService for Win32DialogService` (khoảng dòng 215-290) có `prompt_add_external_editor`, `prompt_default_external_editor` nhưng KHÔNG có `prompt_rename`. Trait `DialogService` (`pr-core/src/abstractions/dialog.rs:49`) có cài mặc định trả `None`. `SecondaryWindowPort::prompt_rename` (dialog_service.rs:90) tồn tại nhưng không có nơi nào gọi tới trong crate (grep `prompt_rename` chỉ ra định nghĩa trait, mặc định trait và lời gọi `vm.prompt_rename_current()` ở tầng ViewModel).
- C# (`Win32DialogService.cs`): `PromptRename(request) => Port()?.PromptRename(_ownerProvider(), request)`.
- Kịch bản: người dùng nhấn F2 / "Rename…" trên shell Win32; ViewModel gọi `dialogs.prompt_rename(..)` → dùng bản mặc định `None` → coi như huỷ, không hiện hộp thoại, kể cả khi cổng thật (W9-R8) đã cài `prompt_rename`.
- Chưa chắc: FEAT-RENAME được ghi là chưa thuộc Wave 8 (D-W8B5-4), có thể đây là chỗ chờ W9-R. Nhưng khi cổng cài xong mà quên nối ở đây, tính năng vẫn chết im lặng; không có test nào bắt (không test nào trong `dialogs_service.rs` nhắc rename).
- Gợi ý: thêm `fn prompt_rename(&self, request) -> Option<String> { self.secondary.get()?.prompt_rename((self.owner)(), request) }` kèm test với `FakePort`.

### L1 (thấp, chỉ nghi ngờ): factory của `LazySecondaryPort` panic
- File: `dialogs/dialog_service.rs` `LazySecondaryPort::get` (khoảng dòng 118-130). C# `Port()` bắt mọi ngoại lệ của `Lazy.Value` (trừ OOM) và trả `null`. Rust không bắt: panic trong `factory()` lan ra mọi `Win32DialogService::show_*`, `Win32ZoomPromptService::prompt_custom_zoom`.
- Kịch bản: việc dựng router lỗi (panic) khi người dùng bấm "Settings" từ WM_COMMAND/menu → panic đi xuyên khung OS (R0003 cấm; thường abort tiến trình) thay vì "không có cổng → false".
- Chưa chắc có đường panic thật trong factory; có thể R0003 chủ ý chấp nhận. Gợi ý: `catch_unwind` quanh factory (một ranh giới job hợp lệ) và coi là `None`.

### L2 (thấp, xác nhận bằng cách đọc): sau khi factory panic, cổng vĩnh viễn là `None`
- File: `dialogs/dialog_service.rs` `LazySecondaryPort::get`: `self.factory.borrow_mut().take()` lấy factory ra TRƯỚC khi chạy; nếu `factory()` panic (và panic bị bắt ở nơi cao hơn) thì `value` chưa được đặt và factory đã mất → mọi `get()` sau đó trả `None` mãi, `is_value_created()` = false, nên `SecondaryWindowPortHost::dispose` không đóng gì. C# `Lazy` (chế độ mặc định ExecutionAndPublication) lưu và ném lại cùng ngoại lệ.
- Kịch bản: một lần dựng cổng lỗi → Settings/Recovery/... im lặng không bao giờ mở lại trong phiên. Phụ thuộc L1.

### L3 (thấp, xác nhận): `Win32ClipboardService::try_set_text` nuốt lỗi
- File: `dialogs/clipboard.rs` `try_set_text`: `set_unicode_text(..).unwrap_or(false)`; `Err(Win32Error)` bị bỏ không ghi nhật ký. Khớp hành vi C# (bắt `ExternalException`…); chỉ ghi lại vì không có dấu vết khi sao chép thất bại.

## Test yếu / rỗng (suy luận từ mã)
- `dialogs_task_dialog_native.rs:34-36` `assert_eq!(IDCANCEL, 2)`: so một hằng với chính nó (không liên quan hộp thoại); toàn bộ file (2 test) `#[ignore]`.
- `dialogs_native_pickers.rs`: 9/10 test `#[ignore = "native: needs desktop"]`; test không ignore (`debug_output_names_the_native_dialogs`, dòng 60-66) chỉ kiểm tên `Debug`. Helper `show_is_cancelled` / `folder_show_returns_none...` (dòng 83-98, 144-166): nếu hộp thoại không bao giờ hiện thì `Show` chặn (treo) chứ không cho kết quả sai; nếu `close_visible_windows` đóng nhầm cửa sổ khác, test vẫn xanh.
- `dialogs_clipboard.rs:63-72` `win32_clipboard_service_is_the_shell_implementation...`: kiểm biên dịch (`assert_impl`) cộng 1 assert; `:168` Debug; 2 test native `#[ignore]` (dòng 245, 277) là hai test duy nhất chạm clipboard thật.
- `dialogs_folder_picker.rs:92-123`: `unwired_dialog_answers_cancelled`, `folder_options_equal_the_golden_file_dialog_options` (`== 2144`), `open_and_save_options...`, `hresult_cancelled_is_0x800704c7`: so hằng với số cứng, hữu ích như bẫy hồi quy nhưng không kiểm hành vi.
- `dialogs_service.rs`: `zoom_prompt_cancelled_returns_none` (dòng 157) khẳng định `None` từ FakePort mặc định — dễ đúng giả (fake trả `None` mặc định, không cho thấy đường "huỷ" thật); `port_host_dispose_*` (381, 393) 1 assert; không có test cho `prompt_rename`, factory panic, hay `pick_folder` lỗi COM ghi log.
- Không thấy test nào bị sửa kỳ vọng cho khớp mã thay vì C# trong phạm vi đã đọc.

## Không kiểm tra được
- Hành vi thật của `IFileDialog`/`IFileOpenDialog` (STA, `CoInitialize` đã gọi chưa, hộp thoại nhóm theo owner, `SetFolder` với đường dẫn không tồn tại → `Err`), `OpenClipboard` tranh chấp thật, `TaskDialogIndirect` với manifest comctl32 v6, `HWND` đã bị huỷ — cần chạy trên Windows có desktop.
- `win32::api::{user32,kernel32}` wrapper (`open_clipboard`, `global_alloc`, `global_lock`, …) và `win32::comctl::task_dialog` chỉ đọc chỗ gọi, chưa kiểm từng wrapper (thuộc phạm vi khác).
- `SecondaryWindowRouter`/`secondary_window_port` (W8-18o) không thuộc phạm vi.
