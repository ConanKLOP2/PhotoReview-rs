# CX-TESTGAPS-G19 (2026-11-01): test còn thiếu G17..G20

Lane `cx-testgaps-g19`. Nguồn: `docs/reviews/ST-BACKLOG-NOW-2026-11-01.md` (G17..G20). Đối chiếu C# `7eaee2a2`. Fakes only, không sleep, không chạm Recycle Bin thật.

## Kết quả grep trước khi viết (master)

| Ứng viên | Kết luận |
|---|---|
| Dọn trùng lặp (`DuplicateCleanupController*`, `DuplicateCheckCancel`) | Đã phủ: `pr-app/tests/duplicate_cleanup.rs` (51 test) khớp các test C# của 4 lớp; không thêm. |
| Rename | Đã phủ (W9-R1..R11, `rename_*`); không thêm. |
| Move/Copy/Undo/Retry fault-injection (INV) | Đã có `file_actions_matrix.rs`, `file_actions_service_group.rs`, `rename_fault_injection.rs`, `common_file_actions` (fake FS); không thêm. |
| `StalePreviewOverwriteTests` (17 test C#) | Phủ một phần ở `presenter_present_a/b.rs` (supersede từng điểm); THIẾU: vét cạn thứ tự hoàn tất, revisit 0-1-0, thumbnail cũ về muộn. Thêm 3 test. |
| `dpi.rs` | `shell_startup/dpi.rs::ensure_per_monitor_v2` không có test nào (chỉ `win32_startup_dpi.rs` kiểm API thật). Thêm 4 test. |
| Composition gate, golden #164 (`shell-launch` v.v.), INV mất cache journal | Chưa làm trong lane này (hết phạm vi 8 test; còn mở). |

## Test đã thêm (7)

1. `crates/pr-platform-win/tests/g19_dpi_bootstrap.rs` (C# `DpiAwarenessBootstrapTests`, 4 test):
   `already_per_monitor_v2_from_the_manifest_is_left_alone`, `not_per_monitor_v2_is_set_once`,
   `when_windows_refuses_reports_false_without_panicking`, `on_an_os_without_the_api_reports_false_without_panicking`.
2. `crates/pr-app/tests/g19_stale_preview.rs` (C# `StalePreviewOverwriteTests`, 3 test):
   `burst_three_files_every_decode_completion_order_never_shows_an_older_image` (6 thứ tự mở cổng),
   `burst_revisiting_the_first_file_every_decode_completion_order_never_shows_the_other_image` (0,1,0),
   `late_thumbnail_of_an_older_navigation_is_dropped_not_flashed_over_the_new_image`.

Đã chạy: `cargo test -p pr-platform-win --test g19_dpi_bootstrap` (4/4), `cargo test -p pr-app --test g19_stale_preview` (3/3, chạy lại 5 lần không flake), rustfmt --check sạch. Chưa chạy clippy/workspace (chờ CI).

## Hạn chế trung thực

- Thứ tự hoàn tất decode được lái bằng thứ tự mở cổng + `pump()`; không có tín hiệu "decode xong" nên thứ tự thật không bị ép tuyệt đối (bất biến kiểm tra đúng với mọi thứ tự). Bản C# vét cạn 15 thứ tự có kiểm soát từng bước.
- Test thumbnail muộn: khẳng định phủ định sau khi reader trả về và presenter idle; sự kiện thumbnail có thể chưa kịp pump trong cửa sổ hiếm (không xảy ra trong 5 lần chạy). Chưa mutation-test (đã cấm cargo-mutants).
- Không phát hiện lỗi production.
- Còn thiếu của StalePreview: ClearCache giữa decode, zoom original muộn, compare pair A bị B đè, stat sau present, release zoom original ngay.

## Hàng PARITY nên đổi (không sửa trong PR này)

- Hàng DPI bootstrap (W8-17a / `shell_startup/dpi.rs`): thêm bằng chứng test `g19_dpi_bootstrap.rs` (4/4 test C# đã port).
- Hàng presenter INV-1 / StalePreview: ghi "một phần" (3/17 test C# mới + test supersede sẵn có); không nâng Done.
- G20 trong backlog: đổi "không tìm thấy StalePreviewOverwrite" thành "đã có 3/17 ở `g19_stale_preview.rs`".
