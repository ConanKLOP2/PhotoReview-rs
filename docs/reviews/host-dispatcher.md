# Review host Win32: dispatcher, message loop và panic qua OS

Machine C (cloud), 2026-10-23. Rust master `a9ed01d`; C# oracle `7eaee2a2`. **Mọi phát hiện: suy luận từ mã, chưa chạy thử.** Chỉ tài liệu; không chạy Cargo, Windows UI hay Recycle Bin.

Tổng mới: **0 cao / 1 trung bình / 0 thấp**. Một khoảng hở xử lý panic chung với C#; thay đổi hành vi cần quyết định parity/ADR. Không chứng minh có crash thực tế hoặc hỏng dữ liệu.

## Phạm vi

Đọc đầy đủ `crates/pr-platform-win/src/host/{ui_dispatcher,message_loop,pending_panic,wndproc}.rs`; đọc các đoạn EventList, destroy/dispose/Drop của `host/shell_window/create.rs`, `dialogs/native/dialog/modal.rs` ModelessFilter và các test pure của message loop/dispatcher. Đối chiếu C# `src/PhotoReview.Shell.Win32/Hosting/MessageLoop.cs` và contract W8-13c/13d. Không kiểm toán toàn bộ clock policy, window/DPI/icons, các API wrapper Win32, renderer hoặc host main window (báo cáo B4 riêng).

## M1 — trung bình: filter xử lý message có thể để panic WndProc nằm chờ vô hạn

**Suy luận từ mã, chưa chạy thử.** Chủ W8-13c, W8-18h..18k; rủi ro trạng thái UI sau panic, không phải chứng cứ mất file. Gộp vào BACKLOG G11, nối câu hỏi còn bỏ ngỏ trong native-dialog.

- `host/message_loop.rs:378-384`: nhánh nested claim trả true gọi `rethrow_pending`; nhánh không filter cũng gọi sau dispatch; nhánh `filtered(&msg) == true` không gọi.
- `dialogs/native/dialog/modal.rs:357-367`: ModelessFilter gọi `IsDialogMessage` và trả thẳng bool, không gọi `rethrow_pending`. Đây là lời gọi OS có thể dispatch vào dialog/subclass và bắt panic vào slot của `pending_panic`.
- `host/pending_panic.rs:22-40` giữ payload đầu tiên; `rethrow_pending:50-55` mới lấy khỏi slot và resume. Vì vậy có pending không tự động đánh thức loop hoặc tự báo lỗi.
- Kịch bản: modeless dialog nhận phím mà IsDialogMessage xử lý; một callback phát sinh panic, guard WndProc giữ payload và trả cho OS; filter trả true. Nếu không còn message, clock hay dispatcher work, loop đi `after_pump:346-359` rồi wait với timeout INFINITE khi không có deadline (`wait_timeout_ms:157-161`). Panic không được resume ngay, UI có thể chờ message mới trong trạng thái callback chưa hoàn tất. Nếu message tiếp là WM_QUIT, `pump_messages:371-374` trả exit code trước mọi rethrow; slot có thể tồn tại qua lúc loop trả. Đây là hai đường có thể suy ra từ control flow, chưa tái hiện native.
- C# cùng khoảng hở: `MessageLoop.cs:242-250` cũng chỉ RethrowPending ở claim/dispatch, không ở Filtered=true. Không trình bày đây là khác biệt Rust–C#.
- Test hiện có `host_message_loop.rs:953`, `a_dispatched_panic_is_resumed_after_dispatch`, kiểm đường dispatch; `nested_claim_resumes_a_captured_panic:716` kiểm claim. `filters_raw_preferred_over_plain_first_true_wins_and_remove_works:805` kiểm filter, nhưng chưa kết hợp filter nhận xử lý + captured panic. Cần test seam riêng, không cần desktop.
- Test nên thêm: filter fake gọi `pending_panic::capture(Box::new("filter callback"))` rồi trả true cho message đầu; queued message thứ hai và WM_QUIT giữ nguyên; bắt unwind ngoài `run`, so payload chính xác, slot trống sau bắt, message thứ hai chưa dispatch. Thêm RawMessageFilter twin và filter trả false có capture để chứng minh không phụ thuộc loại filter. Dọn slot trên mọi nhánh test lỗi, không để nhiễm TLS sang test khác; không sleep.
- Quyết định cần ghi ADR: sau mỗi lần claim/filter/dispatch có thể gọi OS, resume pending ngay khi đã ra khỏi OS, kể cả handled. Giữ quy tắc WndProc không unwind qua extern. Không thêm catch nuốt lỗi trên UI; dispatcher execute có ngoại lệ bắt/log riêng được contract 13d cho phép. Nếu chưa chốt khác C#, chỉ mở PR test mô tả bug với trạng thái đỏ trung thực và xin quyết định trước thay production; không merge một bộ test đỏ hay ignore nó để xanh.

## Các điểm đã kiểm và không ghi thành lỗi

| Đường | Bằng chứng tĩnh | Giới hạn |
|---|---|---|
| Callback có thể post/re-enter | Dispatcher `try_enqueue:374-386` nhả mutex trước request_wake; `execute:413` lấy item ra trước chạy action | Không kiểm tình huống PostMessage lỗi bằng OS thật |
| Shutdown | `ui_dispatcher.rs:330-355` đặt shut_down, lấy abandoned ra ngoài mutex rồi drop, destroy window, xóa CURRENT theo Arc identity | Composition phải gọi shutdown; không tự chứng minh mọi caller làm đúng |
| Một dispatcher mỗi thread | constructor `:203-208` từ chối CURRENT đã có; `from_current_thread:238` trả clone và lọc shut_down | Không suy diễn lỗi nhiều dispatcher từ khả năng Clone |
| Panic của action queued | `execute:413-430` trả Err cho invoke, log cho post; contract W8-13d §6/D-W8B3H-10 cho phép ngoại lệ này | R0003 nói chung UI không catch; ngoại lệ cụ thể không phải vi phạm mới |
| Callback sửa danh sách | MessageLoop `filtered:393-419` snapshot Rc; `poll_clocks:423-436` snapshot; EventList take handler rồi gọi ngoài borrow | Không mở rộng kết luận tới mọi presenter/RefCell |
| Cookie window | `wndproc.rs:96` clone Rc trước callback; `:100-109` clear GWLP_USERDATA và on_native_destroyed sau NCDESTROY kể cả process_message panic | SAFETY comment không thay thế test real OS; không chứng minh mọi SetWindowLongPtr thành công |
| Nested WM_QUIT | `run_nested:332-335` post lại quit cho outer loop | Không gọi đây là lỗi nuốt quit |

Phần lớn test native còn ignore là giới hạn kiểm chứng đã có ở G17, không tính lại thành phát hiện mới. Review này không bật test ignored hay chạy cả workspace.

## PROMPT Codex — G11, xử lý khoảng hở filtered panic

Đọc M1, contracts/W8-13c.md, W8-13d.md và ADR R0003. Phạm vi được sửa sau quyết định parity: `crates/pr-platform-win/src/host/message_loop.rs`, `crates/pr-platform-win/tests/host_message_loop.rs`, `docs/adr/` (ADR mới), `docs/PARITY.md` (chỉ hàng MessageLoop), `task_on_progress.md`. Dùng fake LoopOs/filter có sẵn; không đổi native-dialog hoặc dispatcher nếu không có chứng cứ riêng.

Bổ sung ba biến thể nêu ở M1 và sửa điểm resume sau filter được xử lý theo quyết định đã chốt. CẤM xóa/ignore/làm yếu assertion, catch rồi bỏ panic, thay golden, sleep, chạy bin thật hoặc đổi luồng FIFO/priority. Giữ nguyên test dispatch/claim, quit repost và filter sửa đăng ký.

Kiểm tra với target dir riêng và hang guard: `cargo fmt -p pr-platform-win --check`; `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings`; `cargo test -p pr-platform-win --locked --test host_message_loop`; `cargo test -p pr-platform-win --locked`. Không dùng --include-ignored; không suy diễn kết quả xanh vì 0 test. Ghi lệnh không chạy và chỉ ghi mutation verdict nếu thật sự chạy; không mutation native glue.
