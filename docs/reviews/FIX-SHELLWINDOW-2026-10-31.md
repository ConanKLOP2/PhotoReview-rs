# Sửa ShellWindow / forward_server từ review ST-WIN32 (M1, M2, L1)

Status: báo cáo lane `fix-shellwindow` (nhánh `feat/fix-shellwindow`, cắt từ master `2fdb32fd`). Nguồn: [ST-WIN32-2026-10-31.md](ST-WIN32-2026-10-31.md). Mỗi mục được đọc lại trong mã thật trước khi sửa.

| Mục | Kết luận | Sửa |
|---|---|---|
| M1 | Đúng (use-after-free tiềm ẩn) | `create.rs` `dispose`: khi `hwnd != 0 && destroyed` (đã qua `WM_DESTROY`, chưa qua `WM_NCDESTROY`) không gọi `free_self`; `on_native_destroyed` (WM_NCDESTROY) giải phóng cookie |
| M2 | Đúng | `create.rs` thêm `closing_in_progress: Cell<bool>`; `messages.rs` `raise_closing` bỏ qua `WM_CLOSE` lồng (trả 0, không huỷ), cờ được xoá bằng guard `ResetFlag` kể cả khi handler panic |
| L1 | Đúng | `forward_server.rs` `wait_operation`: chỉ `WAIT_OBJECT_0 + 1` mới là "I/O xong"; mọi kết quả khác (`WAIT_FAILED`, `WAIT_ABANDONED_*`) -> `CancelIoEx` + `GetOverlappedResult(.., true)` rồi `Failed`, giống `forward_client.rs` |

Không thêm `unsafe` mới trong mã production; hai khối `unsafe` mới ở L1 có `// SAFETY:` (sao từ nhánh timeout). `unsafe-audit.baseline` không đổi.

## Test (`crates/pr-platform-win/tests/host_shell_window_fixes.rs`, cả hai `#[ignore = "native: needs desktop"]`)

- `owner_dropped_from_closed_keeps_the_cookie_alive_until_wm_ncdestroy` (M1): owner bị drop trong subscriber `Closed`. Một hook `WH_CALLWNDPROC` thấy `WM_NCDESTROY` TRƯỚC window procedure và đếm strong ref của frame clock (chỉ đối tượng cửa sổ giữ). Khi đối tượng đã chết (lỗi), hook tự xoá `GWLP_USERDATA` nên test không bao giờ đọc cookie đã free. Kiểm thêm sau `WM_NCDESTROY` đối tượng được giải phóng (không rò).
- `nested_wm_close_during_a_closing_handler_does_not_destroy_the_window` (M2): handler `Closing` tự `SendMessage(WM_CLOSE)` rồi huỷ; kiểm cửa sổ còn sống, trả 0, handler không bị vào lại, và lần `close()` sau vẫn tới handler (cờ đã xoá).
- L1: không có test (đường `WAIT_FAILED` không kích hoạt được an toàn); chờ CI cho biên dịch/clippy, hành vi theo `forward_client.rs`.

Kết quả: TRƯỚC sửa cả hai test ĐỎ (M1: đếm 1 so với 2; M2: cửa sổ bị huỷ bởi `WM_CLOSE` lồng); SAU sửa cả hai XANH; 28 test `host_shell_window` (ignored) vẫn xanh. Chạy native không cần `stage-native.ps1` (không dùng DLL C#).

## Đề xuất cho orchestrator

- PARITY: không nâng hàng nào. M2 là khác biệt có chủ đích với C# multicast (C# cho gọi lồng, Rust bỏ qua `WM_CLOSE` lồng); nên ghi vào hàng ShellWindow nếu muốn.
- Còn lại từ ST-WIN32: M3, L2-L14 chưa làm trong lane này.
