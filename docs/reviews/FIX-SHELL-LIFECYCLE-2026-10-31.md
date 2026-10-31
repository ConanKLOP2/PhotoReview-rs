# FIX-SHELL-LIFECYCLE (2026-10-31)

Trạng thái: đã sửa M1, M3, M4, M5; M6 bỏ (tương đương C#). Nguồn: `ST-SHELL-REVIEW-2026-10-31.md` (M2/T5 đã sửa ở PR #480, L1..L8 để PR dọn dẹp sau).

| Mục | Kết luận | Chi tiết |
|---|---|---|
| M1 forwarded-open mất khi modal giữ view model | SỬA | Kiểm chứng: test mới đỏ với `BorrowMutError` tại `lifecycle.rs:328` (mã cũ). `MainWindowInner::open_path` dùng `try_borrow_mut`; bận thì xếp `pending_opens: VecDeque<String>`; `update_view_model` mở hàng đợi (FIFO) sau khi borrow nhả. Test native `host_main_window_lifecycle::open_path_during_a_modal_command_is_queued_and_opened_after_it_returns` (`#[ignore]`, chạy với `--ignored`). C# không có điểm tương đương (view model không phải RefCell). |
| M3 hộp lỗi khởi động khi khoá+pipe còn | SỬA, DIVERGENCE | C# `FailStartup` cùng thứ tự (log, hộp, exit code, đóng cửa sổ), nên là lệch có chủ đích: `FailureSteps::stop_listening` chạy giữa `log_failure` và `show_failure_box` (idempotent, chạy lại ở `run_shutdown`). Test thứ tự cập nhật ở `pr-app/tests/startup_shutdown.rs` và `pr-shell/tests/startup_run_native.rs`. Chưa thả khoá instance trước hộp (hoãn; chỉ dừng nghe pipe để tiến trình sau không bị "giao" cho tiến trình sắp chết và tự khởi động lại). |
| M4 version Settings luôn None | SỬA | `composition_root::settings_version(Option<String>)`: rỗng/None thì `env!("CARGO_PKG_VERSION")` qua `BuildStamp::version()`; dùng trong `make_host_parts`. DIVERGENCE: nhãn hiện phiên bản trần ("0.1.0"), không phải dòng `BuildInfo.Describe` của C# (cùng trường còn là phiên bản cho kiểm tra cập nhật; tách nhãn cần build time, chưa có). Test `pr-shell/tests/startup_settings_version.rs`. |
| M5 SessionWriter không flush tường minh | SỬA | `StartupGraph.session_writer` (từ `services.session_writer()`); `shutdown::dispose_graph` gọi `dispose()` trước khi thả graph. Test native `composition_graph::dispose_graph_flushes_the_session_writer_while_another_arc_of_it_is_alive` (đỏ khi bỏ `dispose()`: không có file phiên trên đĩa khi còn Arc ngoài). |
| M6 config.json hỏng reset im lặng | BỎ (b) | C# `SettingsStore.cs` nhánh `JsonException` cũng chỉ log + sao lưu + mặc định, không thêm vào `LastLoadRepairs`, không hộp thoại. Tương đương C#; thông báo cho người dùng là quyết định sản phẩm, ghi vào PROPOSED-DECISIONS nếu muốn. |

Hàng PARITY nên đổi (không sửa ở PR này): `ShellStartup` fail-path (thêm ghi chú Different cho stop_listening trước hộp); `BuildInfo`/Settings version (nối vào Settings bằng phiên bản trần, Describe còn thiếu build time).

Xác minh đã chạy: `cargo check` (3 crate), `host_main_window_lifecycle` (native, đỏ rồi xanh), `startup_shutdown` (pr-app), `startup_settings_version`, `startup_run_native` (3 test fail_startup/app_startup), `composition_graph` (native, đỏ rồi xanh), clippy `--all-targets -D warnings` cho pr-shell + pr-app + pr-platform-win, rustfmt từng crate. Chưa chạy: test workspace, mutation (CI chạy phần còn lại).
