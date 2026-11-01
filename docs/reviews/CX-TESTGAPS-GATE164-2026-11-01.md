# CX-TESTGAPS-GATE164 (2026-11-01): G18/G19 phần còn thiếu

Lane `cx-testgaps-gate164`. Nguồn: `CX-TESTGAPS-G19-2026-11-01.md`, `ST-BACKLOG-NOW-2026-11-01.md` (G18, G19), `pr-164.md` (M1..M3, L2..L4), `invariants.md` mục 5. Chỉ so golden đã có (không chạy oracle C#, không sửa `tests/golden`). Fakes only, không sleep.

## Kết quả grep master trước khi viết

| Mục | Kết luận |
|---|---|
| (1) Composition gate dùng chung (INV-4) | ĐÃ CÓ: `pr-shell/tests/composition_registry.rs` `recovery_retry_takes_the_file_action_gate` và `undo_takes_the_same_file_action_gate_as_retry`. Lazy: `RetryCell` có test trong `registry.rs`, `LazySecondaryPort` có test ở `pr-platform-win/tests/dialogs_service.rs`. Không thêm. (Phần "`end()` sai người" là sửa production G12, không phải test.) |
| (2a) Forwarded-open M3 / `ui_open_callback...` | ĐÃ CÓ trên master: các test `startup_forwarded_open.rs` đã có assert chuỗi gọi và log; test UiScheduler không còn `#[ignore]`. Không thêm. |
| (2b) 4 golden không được so sánh (M1) | THIẾU: `shell-launch`, `shell-error-log`, `startup-cloak`, `shell-show-command` chỉ có test rỗng/`contains`. Đã thêm. |
| (3) Mất cache journal (Fast vs PowerLossSafe) | THIẾU: không test nào mô hình hoá mất cache. Đã thêm. |

## Test đã thêm (12 test, 4 tệp)

1. `crates/pr-app/tests/g19_golden_launch.rs` (chỉ `cfg(windows)`): `shell_launch_matches_every_golden_case`, so đủ 21 ca (arguments, initial_file/folder, launch_folder, initial_path, smoke, existing_paths) và hằng số; ca `cwd` đặt cwd có guard khôi phục.
2. `crates/pr-app/tests/g19_golden_error_log.rs`: `..._format_matches_every_golden_case` (14 ca, so byte với `text`), `..._constants_match_the_golden`, `..._truncate_matches_every_golden_case` (5 ca: text_length, first/last_old, kept_old_lines...), `..._unusable_paths_never_throw_like_the_golden` (2), `..._report_matches_every_golden_case` (3; chỉ so được dấu thời gian, ngữ cảnh, thông điệp vì kiểu lỗi Rust khác C#).
3. `crates/pr-platform-win/tests/g19_golden_startup.rs`: `shell_show_command_matches_every_golden_case` (8 ca, gồm Minimized), `startup_cloak_replays_every_golden_script` (15 kịch bản: trạng thái sau từng op, `calls`, `delays_ms`, `timer_disposed`, `fire_ran`).
4. `crates/pr-core/tests/g19_journal_cache_loss.rs` (4 test): mô hình "mất điện" bằng cờ `durable` của `HookFileSystem` (chỉ dòng ghi write-through còn lại): Fast mất dòng Prepared, PowerLossSafe giữ; provider được đọc mỗi lần append; Prepared bền + Committed Fast bị mất thì khởi động lại thấy Prepared.

Đã chạy (pass): `cargo test -p pr-core --test g19_journal_cache_loss` (4/4), `-p pr-app --test g19_golden_launch --test g19_golden_error_log` (1/1, 5/5), `-p pr-platform-win --test g19_golden_startup` (2/2). rustfmt đã áp dụng cho 4 tệp. Chưa chạy clippy/workspace (chờ CI).

## Không phát hiện lỗi production

Mọi ca golden khớp. Hai chỗ lệch do fake, không phải lỗi: `FakeFileSystem` không giải `..` như OS (ca `folder-with-dot-dot-segment` đăng ký sẵn cách viết đó); ca `report` không so được kiểu lỗi/HRESULT.

## Hạn chế

- Ca `thrown` (số khung stack, 3 ca) không có bản Rust tương đương (khác cách đếm backtrace, xem L1 của pr-164): chưa so.
- Mô hình mất cache dựa vào cờ `durable` của stream, không kiểm tra `sync_all` thật (đã có test native `#[ignore]` ở `file_actions_journal.rs`).
- Chưa mutation-test (bị cấm).
- Còn mở của G19: Move chéo ổ lỗi nửa chừng bằng fake (không thuộc lane này).

## Hàng PARITY nên đổi (không sửa trong PR này)

- Ba hàng `X-startup-launch`, `X-startup-error-log`, `X-startup-cloak-placement`: thêm bằng chứng "mọi ca golden được so" (21 + 14/5/2/3 + 15 + 8); phần `thrown` còn thiếu nên ghi Partial cho error-log, không nâng Done chỉ vì golden.
- Hàng journal/INV-6 durability: ghi có test mô hình mất cache (Fast vs PowerLossSafe); quyết định Q1 (đổi mặc định) vẫn mở.
- Backlog G18: đổi "chưa xác minh" thành "đã so từng ca 4 golden (PR này)", G19: thêm "đã có test mất cache journal".
