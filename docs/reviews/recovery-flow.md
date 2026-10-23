# Review luồng Recovery: presenter, worker và điểm nối cửa sổ

Machine C (cloud), 2026-10-23. Nguồn Rust: master `bb5c02a`; oracle C#: `7eaee2a2`. **Mọi nhận định và phát hiện: suy luận từ mã, chưa chạy thử.** Không sửa Rust, không chạy Cargo, native UI hoặc Recycle Bin.

Tổng phát hiện mới: **0 cao / 1 trung bình / 0 thấp**. M1 là lỗ hổng test, chưa phải bằng chứng lỗi production. Các nguy cơ đã biết giữ mã cũ, không cộng lại.

## Phạm vi và giới hạn

Đọc `crates/pr-app/src/presenters/recovery_window.rs`, các đường mapping trong `recovery_presentation.rs`, `recovery_window_presenter.rs`; đối chiếu `contracts/W8-10g.md`, ADR R0003, C# `src/PhotoReview.App.Shared/Presenters/RecoveryWindowPresenter.cs` và test cùng tên ở `tests/PhotoReview.App.Tests/Presenters/RecoveryWindowPresenterTests.cs`. Kiểm điểm nối `crates/pr-shell/src/startup/secondary_dialogs.rs:251-267` và inventory stub `crates/pr-platform-win/src/dialogs/recovery/`.

Cửa sổ native 18t–18v còn 96 biểu thức `todo!()` qua grep trong thư mục recovery tại head này. Ví dụ constructor `recovery/mod.rs:183-189`, `show_modal:318-319`, `pump_completions:383-384`. Không thể kết luận host đã bảo đảm FIFO, close khi busy hay vòng đời post chỉ từ presenter. Đây là phạm vi chưa port, không phải 96 lỗi mới; không gọi các đường stub để kiểm chứng.

## M1 — trung bình, test: ca xóa dòng giữa live check chưa chứng minh kết quả gắn đúng Id

**Suy luận từ mã, chưa chạy thử.** Đơn vị chủ W8-10g; liên quan bẫy T1 của contract và W8-18t khi nối host.

- `crates/pr-app/tests/recovery_window_presenter.rs:997-1020`, `result_for_a_cleared_row_still_counts_progress`: tạo hai entry, gom message, dismiss entry đầu rồi giao message. Assertion kiểm Id còn lại, `check().is_some()`, tiến độ 2/2 và completion; không so kết quả với entry thứ hai, không kiểm snapshot ngay sau message của entry đã xóa.
- FakeChecker tại cùng file `:182-208` trả cùng verdict cho mọi entry. `check(entry, verdict)` vẫn có thể chứa dữ liệu riêng của entry; test không kiểm dữ liệu đó.
- Kịch bản: snapshot [A,B], xóa A, message A rồi B tới. Một implementation sai dùng vị trí gốc 0/1 để cập nhật vector hiện tại có thể gán check(A) cho B, bỏ qua B vì index 1 không còn. Test này vẫn thấy B có `Some`, progress 2/2, completed 1. Đây là phản ví dụ cho sức mạnh assertion, không khẳng định mã hiện tại làm thế và chưa chạy mutant.
- Mã đúng hiện tại `recovery_window.rs:595-597` tìm theo `row.entry().id == id`, sau đó mới cập nhật. Giữ nguyên mã này khi chỉ siết test.
- Test cần siết: A/B có source, size và verdict khác nhau; sau message A, B vẫn unchecked, không có `row_changed` mới nhưng tiến độ tăng 1; sau message B, so toàn bộ result(B), index callback mới bằng 0; cuối cùng Finished một lần và tiến độ 2/2. Thêm biến thể xóa dòng giữa [A,B,C], nhận C trước B nếu seam cho phép, để kiểm mapping theo Id độc lập vị trí; production worker FIFO vẫn phải giữ thứ tự của nó.
- Nguồn oracle: C# presenter `:207-211` giữ chính đối tượng Row trong progress; việc chuyển sang vector index là nguy cơ riêng Rust, vì vậy đánh dấu ca thêm là Rust-only, không gán tên test C# bịa.

## Những bảo đảm đã thấy trong mã và test hiện có

| Đường | Bằng chứng tĩnh | Giới hạn |
|---|---|---|
| Bỏ kết quả từ run cũ | `recovery_window.rs:553-555` cancel token cũ; `:589-590` kiểm token của message; test `late_result_of_an_earlier_run_is_dropped_during_a_new_run`, `run_checks_late_result_of_cancelled_run_is_ignored` | Chưa chạy; host thực tế chưa nối |
| Lỗi checker / retry worker | `:270-273`, `:317-320` catch_unwind; test `checker_panic_becomes_failed_and_reenables_recheck`, `retry_job_panic_becomes_err_and_busy_is_released` | Không chứng minh launcher/post thất bại được xử lý |
| Busy / stale retry | `finish_retry:734` nhả busy trước `outcome?`, superseded xóa theo Id; test `retry_throws_releases_busy_state`, `retry_superseded_drops_the_row` | Phải gọi finish đúng một lần trên UI; chưa có bảo đảm native ở stub |
| Dismiss lỗi / stale | `dismiss:661-697`: Err giữ dòng, Ok xóa cả dismissed và skipped; test `dismiss_throws_removes_nothing`, `dismiss_removes_dismissed_and_stale` | Không chứng minh journal ghi bền bằng test presenter |
| Kiểm tra chỉ đọc | RecoveryCheckRun chỉ giữ checker, không nhận retry/dismiss; test `run_checks_reports_every_row_and_writes_nothing` | Fake checker là seam được tin cậy; không suy rộng sang retry |

Không đánh dấu thiếu catch panic quanh DismissFn là lỗi parity: C# `Dismiss:269-272` bắt exception; Rust chuyển lỗi thường thành Result, production closure tại `secondary_dialogs.rs:260-262` map journal error sang Err. ADR R0003 quy định UI không bắt panic. Test tên `dismiss_throws_removes_nothing` dùng Err là phù hợp phép ánh xạ; việc checker/retry worker bắt panic không buộc UI làm tương tự.

## Nguy cơ đã có, không lặp phát hiện

1. `invariants H1` / BACKLOG G25: retry Recycle thiếu chốt fingerprint sau Prepared. Presenter không thể sửa lỗi ở core bằng việc hỏi xác nhận thêm.
2. `startup M2` / G7: `secondary_dialogs.rs:267` truyền `allow_permanent_delete: None`, trong khi retry service có thể dùng cấu hình cho phép xóa vĩnh viễn. Cần nối setting lúc hỏi như báo cáo cũ.
3. `startup H1` / G3: định dạng thời gian Recovery sai epoch ở adapter Windows; presenter dùng formatter seam không tự chứng minh timestamp production đúng.

## PROMPT Codex — chỉ siết test W8-10g

Phạm vi được sửa: `crates/pr-app/tests/recovery_window_presenter.rs`, `task_on_progress.md`. Đọc M1 trong báo cáo này và contract W8-10g T1. Siết `result_for_a_cleared_row_still_counts_progress` bằng kết quả phân biệt A/B, assert sau từng message và so toàn bộ check B. Không sửa production, không xóa/ignore/làm yếu assertion, không đổi golden để khớp mã, không dùng sleep hay bin thật. Không coi chỉ `is_some()` hoặc không panic là đủ.

Kiểm tra có hang guard: `cargo fmt -p pr-app --check`; `cargo clippy -p pr-app --all-targets --locked -- -D warnings`; `cargo test -p pr-app --locked --test recovery_window_presenter`; `cargo test -p pr-app --locked`. Dùng target dir riêng theo AGENTS.md. Báo đúng lệnh không chạy. Khi nối W8-18t–v sau này, thêm test launcher lỗi, close trong retry, late post sau close và callback tái nhập; không ghi các test host đó là đã có.
