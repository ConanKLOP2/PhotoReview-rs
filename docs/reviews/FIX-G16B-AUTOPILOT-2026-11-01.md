# FIX G16B autopilot (lane fix-g16b-autopilot), 2026-11-01

Phần còn mở của G16 sau PR #542 (`docs/reviews/autopilot.md`). Đã chạy: `bash -n` mọi script đổi; `python -I -m py_compile lib/codex-lib.py`; selftest mới `tools/autopilot/test/lib-selftest.sh` (22 ca, git repo giả cục bộ) và `tools/autopilot/test/proc-selftest.sh` (20 ca, gh giả trên PATH); `claim-selftest.sh` vẫn đạt; `doc-links.py` 0 lỗi. KHÔNG chạy: `claim.sh gc`, pipeline thật, Codex, Cargo, và `pipeline-selftest.sh` (nó chạy cargo thật; các ca extsat/lint/testguard cũ của nó được đối chiếu bằng mắt, chưa chạy: chờ người có cargo chạy một lần).

| Mục | Trạng thái | Việc / lý do |
|---|---|---|
| M1 | ĐÃ SỬA (phần lớn) | `testguard`: base không đọc được thì báo lỗi (hết fail-open); dòng thêm vào test cũ không được là `return`, `if false`, `#[cfg`, `loop {}`; thêm `#[cfg(` mới bị chặn; `_fn_bodies` che chuỗi thô, `/* */`, ký tự `'}'`. Để lại: chế độ `relaxed` chỉ đếm tổng assert; `setdefault` giữ hàm đầu khi trùng tên; bọc `if cond {` bất kỳ không phát hiện được bằng phép so dòng |
| M2 | ĐÃ SỬA | vùng test bắt đầu ở `#[cfg(test)]` đầu tiên mà mục kế tiếp là `mod` thật (`_test_start`); `#[cfg(test)]` trên `use`/`fn` hay trong comment không còn miễn lint. Để lại: vùng test vẫn kéo tới cuối tệp (không khớp ngoặc `mod`) |
| M3 | ĐÃ SỬA | `extsat` chỉ tính id nằm trong scope của commit (`feat(<id>):` hoặc `Complete/Merge <id>`), không tính id nhắc ở thân/phần sau chủ đề; commit `test(...)`, `partial`, `wip`, `red tests` không thoả. Ghi chú: id dạng `W9-E4` chưa bao giờ được `expand_unit_ids` nhận (tồn tại từ trước), để lại |
| M4 | ĐÃ SỬA một phần (PR #542) | không đổi ở lane này (vẫn khớp chuỗi con, vẫn xoá theo tuổi 14 ngày). Để lại: sửa cần quyết định chính sách gc, và không được chạy `gc` ở lane này |
| M5 | ĐÃ SỬA một phần | khoá chính và khoá merge dùng chung `lock_stale`/`lock_reclaim` (`lib/proc.sh`): khoá thiếu pid chỉ cũ sau 1 phút (hết lỗi A xoá khoá B vừa mkdir), gỡ bằng đổi tên nguyên tử rồi kiểm lại. Cửa sổ TOCTOU còn lại rất hẹp (đổi tên đúng lúc người khác vừa tạo khoá mới); xoá hẳn cần `flock`, không có trên Git-Bash |
| M6 | ĐÃ SỬA một phần | `on_exit` gọi `kill_tree` cho từng PID trong `SLOTPID` (con msys trước, rồi chính nó; TERM rồi KILL) trước khi xoá target/khoá. Để lại: tiến trình Windows gốc (codex.exe, cargo.exe) không thấy là con trong `ps` của msys nên không bị giết, chúng tự dừng theo timeout |
| M7 | ĐÃ SỬA một phần | `unit-pr.sh`: `trap EXIT` gọi `unitpr_cleanup` xoá worktree + nhánh cục bộ do chính lần chạy tạo khi thất bại trước push (commit chưa push sẽ mất, có thể sinh lại từ nhánh tích hợp); "already open" chỉ khi `gh pr list` xác nhận có PR mở, nhánh remote không có PR mở thì exit 2 với thông báo rõ. Để lại: nhánh cục bộ có sẵn trùng `origin/master` (đường `worktree add` không `-b`) vẫn không được dọn |
| L1 | ĐỂ LẠI | nhả claim khi unit thất bại là quyết định chính sách (máy giữ claim để triage `needs-claude`); cần chủ quyết định |
| L2 | ĐỂ LẠI | đổi `has_t` thành "đủ mọi tệp test" làm đổi luồng red-tests, không thử được nếu không chạy pipeline-selftest (cargo) |
| L3 | ĐÃ SỬA | `redcheck` từ chối lần chạy không có test nào thất bại và không có lỗi biên dịch |
| L4 | ĐÃ SỬA (phần) | `state_load` giữ bản sao `*.corrupt` khi JSON hỏng, chịu được tệp thiếu `units`/`meta`. Để lại: lỗi đọc tạm thời vẫn trả trạng thái rỗng |
| L5 | ĐỂ LẠI | TOCTOU của `state_lock` cần đổi cơ chế khoá Python; rủi ro thấp (tệp tạm 30 s) |
| L6 | ĐỂ LẠI | `testargs --lib` toàn crate và `TA` rỗng chạy cả crate: đổi hành vi gate, cần cargo để thử |
| L7 | ĐÃ SỬA | bộ lọc owned-files dùng `GATE_IGNORE_RE` (`.codex-*` ở gốc, thư mục `target*/`), tệp gốc tên `target.rs` không còn được miễn. Để lại: commit do Codex tự tạo vẫn làm cây sạch |
| L8 | ĐỂ LẠI | trích dẫn `$LIB` ở mọi chỗ gọi (nhiều chục chỗ, mỗi chỗ cần `bash -n` + chạy thật); đường dẫn có khoảng trắng vẫn lỗi |
| L9 | ĐÃ SỬA | `codex-usage.sh`: ngưỡng không phải số bị từ chối (hết nội suy mã Python), thăm dò lỗi trả exit 2; `wait_quota` dừng (exit 6) sau 5 lần lỗi thăm dò liên tiếp thay vì lặp vô hạn |
| L10 | ĐỂ LẠI | chỉ nghi ngờ, cần cấu hình bảo vệ nhánh thật để kết luận |
| L11 | ĐỂ LẠI | `status.sh` không còn trong cây; `CS_REPO` cố định chỉ đổi khi có cấu hình đa máy |

Khoá chính TOCTOU (M5): đã làm bản gọn (grace + đổi tên), phần còn lại ghi ở trên.

Hàng PARITY: không có (công cụ autopilot, không thuộc cổng parity).
