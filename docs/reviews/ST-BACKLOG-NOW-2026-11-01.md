# ST-BACKLOG-NOW 2026-11-01: trạng thái hiện tại của nhóm G1..G31 trong BACKLOG.md

Status: report (lane tĩnh `st-backlog-now`). Cập nhật trạng thái cho `BACKLOG.md` (snapshot 2026-10-23, master `b5d1327`) theo master `71d9ea76` (#526). Không đổi mã, không chạy cargo.

Phương pháp và trung thực: đối chiếu 300 PR merged gần nhất (`gh pr list --state merged --limit 300`, #230..#526), `git log`/`git show --stat` của PR liên quan, và `grep`/đọc mã tại master. "ĐÃ SỬA" chỉ khi thấy thay đổi tương ứng trong mã hoặc PR có tiêu đề/diff khớp. Điều chưa kiểm sâu ghi "chưa xác minh". Không chạy test nên không có kết luận về test xanh/đỏ. `BACKLOG.md` giữ nguyên làm nguồn lịch sử (không sửa).

Ký hiệu: ĐÃ SỬA, MỘT PHẦN (còn phần mở), CÒN MỞ, QUYẾT ĐỊNH (cần người). Đường dẫn dưới `crates/`.

## 1. Bảng trạng thái hiện tại

| Nhóm | Trạng thái | Số PR / bằng chứng |
|---|---|---|
| G1 RefCell qua modal/controller | MỘT PHẦN | H2 ĐÃ SỬA: #255 (`pr-platform-win/src/shell_input/dispatch.rs` `run_pointer` ~718, `pr-app/src/input/pointer/viewer.rs`). H1 giảm nhẹ: `host/main_window/lifecycle.rs:326-337` nhả borrow trước `apply_changes`, `shell_input/main_window.rs:708` và `shell_menus/main_window.rs:338` dùng `try_borrow_mut` và bỏ lệnh khi bận, `lifecycle.rs:341` xếp hàng open; liên quan #318. Còn mở: lệnh modal vẫn giữ borrow suốt vòng lặp lồng (thiết kế), test `closed_disposes_the_bars_once_and_unsubscribes` vẫn `#[ignore = "native: needs desktop"]` (`tests/host_main_window_frame.rs:507`) |
| G2 Zoom không tới presenter/HUD | ĐÃ SỬA | #338 "shell input zoom changes reach the presenter, D1 loads the full-resolution original" |
| G3 Recovery thời gian | MỘT PHẦN | H1 ĐÃ SỬA #339 (`shell_startup/locale.rs:16,132` trừ `FILETIME_EPOCH_TICKS`, trả chuỗi rỗng khi âm). L1 CÒN MỞ: `locale.rs:163-189` `local_long_time_text` gọi `format_system_time` với cờ `TIME_NOSECONDS` (C# "T" có giây). L6 CÒN MỞ: `locale.rs:104-117` bảng ngôn ngữ cứng cho `percent_one_decimal` |
| G4 Settings > General mất View/Instance mode | ĐÃ SỬA (PR #531) | `dialogs/settings/pages/general.rs:258-259`: `collect` vẫn gọi `set_combo_index` (ghi) thay vì `combo_index` (đọc); không PR nào chạm file sau #162 |
| G5 Executor panic | ĐÃ SỬA | #462 "ask the disk before restoring after an executor panic (M3)" |
| G6 Dọn trùng lặp | ĐÃ SỬA | #464 "re-check keeper and copies after the review dialog (ST-INVMAP M1)". Undo nhóm vẫn không có (đúng quyết định Q3) |
| G7 Prompt/cổng lazy/cài đặt xoá vĩnh viễn | MỘT PHẦN | ĐÃ SỬA: chuyển 3 prompt (#254; `dialogs/dialog_service.rs:296-310`), cảnh báo xoá vĩnh viễn của Recovery (#500; `pr-shell/src/startup/secondary_dialogs.rs:405-413`). CÒN MỞ: `LazySecondaryPort::get` lưu `None` vĩnh viễn (`dialog_service.rs:132-134`, `OnceCell<Option<..>>`, ghi là parity C# `Lazy`), `panic!("{error}")` ở adapter lazy (`pr-shell/src/composition/adapters.rs:189`, bị bắt bởi `catch_unwind` của worker) |
| G8 `OpenInDefaultEditor => todo!()` | ĐÃ SỬA | W9-E (#236, #256): `shell_input/main_window.rs:328` gọi `vm.open_in_external_editor()`; không còn `todo!()` ở đường production. Q4 (no-op) bị thay bởi triển khai thật |
| G9 Settings pages | MỘT PHẦN | M1 (FEAT-EXTERNAL-EDITORS) đã làm bởi W9-E (#237, #235, #234; `pages/files.rs` có editor, `context.rs:376` `edit_external_editors` có thân). CÒN MỞ: M2 `pages/shortcuts.rs:449` vẫn `EnglishSettingsTexts`; L3 `shortcuts.rs:186` `to_ascii_lowercase`; re-entrancy `EN_CHANGE` (`shortcuts.rs:327-328`, không thấy cờ chống vào lại; chưa xác minh sâu) |
| G10 Render/painter | MỘT PHẦN | L3 ĐÃ SỬA (`render/draw_context.rs:145-152` `pop_clip` trả `Err` khi độ sâu 0, không giảm). Còn: L2 `draw_context.rs:414` `let _ = self.pop_transform()` (có chú thích "finally", tương đương C#); L1 `render/image_cache/policy.rs:76-77` quy tắc "backend không gọi lại cache" chỉ là tài liệu, cache vẫn giữ borrow qua `upload`; M1 (resize lỗi rồi vẽ) chưa xác minh |
| G11 Native dialog | CÒN MỞ | Không có dấu vết sửa: `dialogs/native/dialog/` không có `thread::panicking`, không lọc `WS_DISABLED` ở `focus.rs`, không `message_loop = None` ở cleanup; `modal.rs:238` chỉ bọc `ready` bằng `catch_unwind` |
| G12 Hardening INV | MỘT PHẦN (parity với C#, cần quyết định nếu muốn hơn; xem PR #532) | L3 (Recycle đơn re-stat) ĐÃ SỬA #463. CÒN MỞ: L1 `pr-core/src/file_actions/service.rs:239` `end()` vẫn `pub` và ghi 0 vô điều kiện; L6 `journal_persisted` chỉ dùng ở `pr-app/src/presenters/recovery_window.rs:766`, không có log ở `coordinators/file_actions`; L5/L7/L8 chưa có test khoá hành vi mới (chưa xác minh) |
| G13 Shell startup nhỏ | MỘT PHẦN | Lô "low findings of the shell review" #492 và #487 đã sửa một số (chưa đối chiếu từng mục). CÒN MỞ chắc chắn: L1 `pr-app/src/startup/error_log.rs:75` `Backtrace::force_capture()` không phụ thuộc cấu hình log. Forwarded-open có hàng chờ `forwarded_open.rs:114` (parity C#) |
| G14 Main window nhỏ | CÒN MỞ | `host/main_window/core.rs:511` `UiLocalPoster` không có `PhantomData<*const ()>`; `shell_viewing/frame_log.rs:25` `FrameLog` giữ `File` và ghi đồng bộ |
| G15 Registry/composition | CÒN MỞ | `pr-shell/src/composition/registry.rs:106` `session_writer: OnceLock<Result<..>>` lưu lỗi tạm |
| G16 Autopilot tooling | CÒN MỞ | H1: `tools/autopilot/claim.sh:6` `git fetch -q origin '+refs/claims/*:refs/claims/*'` không `--prune`. Các PR autopilot đã merge (#183, #178, #253) không phải nhóm này |
| G17 Test ignore/pass giả | MỘT PHẦN | Q7 đã làm: `.github/workflows/native-nightly.yml` (chạy `--include-ignored --test-threads=1` theo bin đã duyệt, cập nhật ở #526); #479 audit, FIX-TEST-HYGIENE. Còn 1630 thuộc tính `#[ignore` trong `crates/` (đếm bằng grep, gồm lý do hợp lệ native/slow); chưa rà từng test |
| G18 Test #164 | MỘT PHẦN | #449 golden shell-startup-texts; có `tests/win32_startup_dpi.rs`. Chưa xác minh bốn golden `shell-launch`/`shell-error-log`/`startup-cloak`/`shell-show-command` đã được so sánh từng ca |
| G19 Test INV | MỘT PHẦN | #441 (repro H1), #446 (JournalDurability), #457/#429/#425/#419 (journal killers), #369 (flush). Chưa thấy test Move chéo ổ lỗi nửa chừng bằng fake (chưa xác minh) |
| G20 Rename / StalePreview | MỘT PHẦN | Rename ĐÃ CÓ: W9-R1..R11 (#275..#311) với `pr-core/tests/rename_*`, `pr-app/tests/rename_*`. CÒN MỞ: không tìm thấy `StalePreviewOverwriteTests` (17 test C#) bằng grep tên `StalePreviewOverwrite`/`stale_preview_overwrite` trong `crates/` |
| G21 Contract W9 | MỘT PHẦN | Như BACKLOG (#207/#216); W9-E/R đã triển khai nên nhiều câu "thiếu" tự lỗi thời. Không kiểm lại trong lane này (chưa xác minh) |
| G22 PARITY/PR evidence | MỘT PHẦN | PARITY được đồng bộ nhiều lần (#394, #440, #526). Chưa kiểm hàng W8-17a/b/c, W8-13h..13k (chưa xác minh) |
| G23 Chú thích lạc hậu | CÒN MỞ (chưa kiểm) | Không có PR nhắm riêng |
| G24 Quyết định Q1..Q8 | ĐÃ CHỐT | P-52 (Q1..Q8), P-57 (Q1 giữ `Fast`, `PowerLossSafe` là lựa chọn người dùng), P-58/P-59 (khác biệt chủ ý). Q4 và Q7 đã được hiện thực hoá (G8, G17) |
| G25 Retry Recycle sau Prepared | ĐÃ SỬA | #448 "retry nhóm Recycle không xoá vĩnh viễn file bị thay thế (H1/G25)", bổ sung #463, #460 |
| G26 Writer cache cũ | ĐÃ SỬA | #474 (`pr-imaging/src/service/disk.rs` +105, `cache/atomic_file.rs` +35) |
| G27 Backup config newer lỗi | ĐÃ SỬA (PR #530, P-66: Save bị từ chối khi backup lỗi, khác C# best effort) | `pr-core/src/settings/store.rs:665-668`: `backup_newer_config` trả `()`, rồi `newer_file_on_disk = false` bất kể backup lỗi (`store.rs:695-720` chỉ warn). P-52..P-59 không có quyết định về mục này; cần mở P mới (đề xuất: giữ cờ `newer` khi copy lỗi hoặc hoãn downgrade) |
| G28 Log ghi dở | MỘT PHẦN | Mã đã tránh lặp byte: `pr-core/src/diagnostics/file_log.rs:439-449` `write_remaining` tăng `offset` theo số byte đã ghi. Còn thiếu test seam short-write (chưa xác minh) |
| G29 Cache read toàn file | ĐÃ SỬA | #474 (`pr-imaging/src/cache/preview_file.rs` +114, `OutOfMemory` ở `preview_file.rs:73`) |
| G30 Output lần registry read thứ hai | ĐÃ SỬA (PR #532) | `pr-platform-win/src/recycle_bin_settings.rs:118-125,162-202` chỉ kiểm kiểu DWORD của từng lần đọc; không thấy seam đổi output giữa probe/read |
| G31 Restore verb helper | ĐÃ SỬA (phần mã) | #480 "restore_verb in crate": `recycle_bin_shell.rs:34` dùng `crate::restore_verb::CANONICAL_VERB_Z`. Hàng PARITY P-03 giữ Partial (không đổi ở lane này) |

Tổng (31 nhóm): ĐÃ SỬA 11 (G2, G4, G5, G6, G8, G25, G26, G27, G29, G30, G31; cập nhật 2026-11-01 sau #530..#532), MỘT PHẦN 14, CÒN MỞ 5 (G11, G14, G15, G16, G23), QUYẾT ĐỊNH 0, G24 đã chốt. "Đã sửa" không đồng nghĩa test xanh vì không chạy test.

## 2. Top 8 mục CÒN MỞ theo rủi ro dữ liệu

| # | Mục | Rủi ro | Phạm vi file | Test cần thêm |
|---|---|---|---|---|
| 1 | G4 Settings > General: `collect` ghi thay vì đọc combo | D (cấu hình): bấm Save làm mất lựa chọn View mode/Instance mode | `pr-platform-win/src/dialogs/settings/pages/general.rs:256-270`, `tests/settings_page_general.rs` | Round-trip `load(A)` -> đổi combo qua context giả -> `collect` -> state mang giá trị mới cho View mode và Instance mode (test đỏ trước) |
| 2 | G27 Backup config bản newer lỗi vẫn downgrade | D (cấu hình): mất field chưa biết khi backup lỗi | `pr-core/src/settings/store.rs:660-720`, `pr-core/tests/settings_store.rs`, ADR mới | Characterization bằng fake FS: version>4 có FutureField, `copy_file` lỗi + `WriteAtomic` ok; rồi lỗi cả hai ở Save1, ok ở Save2; kiểm cờ `newer` và bytes backup. Sửa chỉ sau quyết định (P mới) |
| 3 | G30 Kiểm output của lần registry read thứ hai | D/B (fail-closed, chưa chứng minh mất file) | `pr-platform-win/src/recycle_bin_settings.rs:118-202`, `tests/recycle_bin_settings_capacity.rs` | Seam thấp cho fake đổi output giữa probe và read; kiểu/độ dài sai -> Unknown chặn xoá; theo `recycle-policy.md` M1 |
| 4 | G12 hardening cổng INV-4 và journal | D (thấp): `end()` giải phóng cổng của người khác; lỗi journal không tới log điều phối | `pr-core/src/file_actions/service.rs:239`, `pr-app/src/coordinators/file_actions/actions.rs` (đọc `journal_persisted`), `pr-core/tests/file_actions_service.rs`, `pr-shell/tests/composition_registry*.rs` | `try_begin` trên service chung làm `undo_service.is_busy()` true và retry trả `core_recovery_busy`; `journal_persisted == false` có log lỗi; test khoá hành vi Undo Recycle đơn không journal |
| 5 | G1 phần dư: modal giữ borrow, vòng tham chiếu cửa sổ chính | C (lệnh bị bỏ im lặng hoặc rò) | `pr-platform-win/src/host/main_window/lifecycle.rs:326-348`, `shell_input/main_window.rs:705-718`, `shell_menus/main_window.rs:335-345` | Fake dialog host: trong callback dialog gọi vẽ/pump/`on_closing` không panic và lệnh bị bỏ phải có trạng thái; bỏ `#[ignore]` của `closed_disposes_the_bars_once_and_unsubscribes` khi chạy được (`tests/host_main_window_frame.rs:507`) |
| 6 | G7 phần dư: cổng lazy lưu `None`, `panic!` trong adapter | B/C: cửa sổ phụ không bao giờ mở lại sau khi đồ thị chưa gắn | `pr-platform-win/src/dialogs/dialog_service.rs:100-140`, `pr-shell/src/composition/adapters.rs:147-190`, `tests/dialogs_service.rs` | Lần `get` đầu khi graph chưa gắn -> `None`, sau khi gắn -> `Some` (cần quyết định lệch C# `Lazy`); factory panic không lan; test sinh danh sách phương thức trait phải được chuyển |
| 7 | G11 native dialog: callback chạy khi unwind, focus, font | C: panic kép/treo | `pr-platform-win/src/dialogs/native/dialog/{modal,result,focus,tooltip,dpi}.rs`, `tests/dialogs_native_dialog_*.rs` | Test thuần cho `closed`/`on_closed` không gọi lại khi `thread::panicking()`, `first_tab_stop` bỏ qua control disabled, font mới lỗi không xoá font cũ |
| 8 | G9 Settings Shortcuts: texts tiếng Anh, so khoá ASCII, `EN_CHANGE` lồng | B: tên hành động sai ngôn ngữ, khoá trùng sai | `pr-platform-win/src/dialogs/settings/pages/shortcuts.rs:186,327-328,449`, `tests/settings_page_shortcuts.rs` | Ngôn ngữ vi + JSON hành động rỗng -> tên tiếng Việt; khoá khác hoa/thường Unicode gộp đúng như `OrdinalIgnoreCase`; `EN_CHANGE` lồng chỉ `refresh` một lần |

Ghi chú xếp hạng: các mục 1 và 2 là dữ liệu cấu hình của người dùng; mục 1 sửa nhỏ, có thể giao ngay. Nhóm không vào top 8 nhưng còn mở: G3 L1/L6 (hiển thị giờ), G10, G13 L1, G14, G15, G16 H1, G20 (StalePreview 17 test), G23.

## 3. Việc cần người quyết định (P-nn)

- G27 (đã chốt bằng P-66, PR #530; dòng này là lịch sử): chưa có P-nn; đề xuất mở P mới (backup config newer: giữ cờ khi copy lỗi hay chấp nhận best effort như C#).
- G7 lazy `None`: hiện là parity C# `Lazy`; nếu muốn thử lại thì ghi vào ADR lệch C# (liên quan P-58/P-59).
- G24 Q1..Q8 đã chốt (P-52, P-57); không hỏi lại.
