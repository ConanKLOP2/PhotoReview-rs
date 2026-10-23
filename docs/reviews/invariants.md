# Kiểm toán an toàn dữ liệu: INV-1..INV-12

Machine C (cloud). Review CHỈ ĐỌC, không sửa mã, không chạy cargo (container Linux, crate Windows).

**Mọi phát hiện dưới đây: suy luận từ mã, chưa chạy thử.** Số dòng lấy từ `master` tại `c40b59d`; C# đối chiếu tại `7eaee2a2` (clone `ConanKLOP2/photoreview`, HEAD = baseline).

## Phạm vi đã đọc
- `crates/pr-core/src/file_actions/{service,transaction,journal,reconcile,startup_recovery,undo,recovery_retry,compaction,journal_rules,duplicates,partial_cleanup}.rs` (đọc kỹ `service`, `transaction`, `journal`, `reconcile`, `undo`, `startup_recovery`, `compaction`; `recovery_retry` đọc phần cổng/precheck/group, không đọc hết `execute_group_retry`; `recovery_check.rs` chỉ đọc lướt).
- `crates/pr-core/src/session/{store,writer}.rs` (chỉ đường lưu/đọc), `crates/pr-core/src/io/{physical,journal_compaction_files}.rs` (các hàm move/copy/delete/open_read_shared/replace), `crates/pr-core/src/catalog/review_catalog.rs` (`remove`, `restore`, `restore_members`, `replace_order`).
- `crates/pr-app/src/coordinators/{file_action_gate,file_actions/{actions,undo},duplicate_cleanup,sibling_navigator}.rs`, `folder_load/*` (chỉ cổng thứ tự và `skipped`, không đọc hết `load.rs`).
- `crates/pr-platform-win/src/{recycle_bin,recycle_restore,live_operation_registry}.rs` (`recycle_bin_shell/capacity/settings/candidate/eligibility` chỉ liệt kê, không đọc sâu).
- Không đọc sâu: `pr-imaging` (INV-1, 2, 8 phía giải mã/cache), `pr-core/src/settings/store.rs` (INV-11), `pr-core/src/diagnostics/file_log.rs` (INV-10). Các hàng đó trong bảng dưới chỉ dựa vào tên test và tài liệu, ghi rõ.

Tổng sau tái kiểm ngày 2026-10-23: 1 cao / 3 trung bình / 8 thấp / 3 câu hỏi sản phẩm (H1 mới bên dưới).

## 1. Bảng INV

Nguồn định nghĩa: `docs/architecture.md` của C# (mục "Ánh xạ bất biến"), `docs/PARITY.md`, `docs/MULTI-AGENT-PLAN.md`.

| INV | Ý nghĩa | Nơi Rust bảo đảm | Test chứng minh |
|---|---|---|---|
| INV-1 | Cache định danh đầy đủ; không present pixel cũ | `pr-imaging/src/cache_key.rs:103` (`ImageCacheKey`), `PreviewImageService`, presenter token (chưa đọc sâu) | `pr-imaging/tests/preview_service_decode.rs` (nhắc INV-1 ở dòng 2 và 2067), `pr-app/tests/presenter_present_a.rs:245` |
| INV-2 | Clear/evict vô hiệu hoá in-flight/persist cũ | epoch trong `pr-imaging/src/service/mod.rs` (~959 có test white-box), `DiskCacheStore`, `ThumbnailCache` (chưa đọc sâu) | `pr-imaging/tests/cache_preview_file.rs`, `thumbnail_cache.rs:1228` |
| INV-3 | Advance một lần TRƯỚC I/O; không present lại sau action | `pr-app/.../file_actions/actions.rs:299-320` (`execute_core`: `catalog.remove` -> `sink.present` -> mới submit job ở 365) | `pr-app/tests/viewmodels_main_commands.rs:811` `recycle_failing_during_the_file_operation_keeps_the_advance_restores_the_photo_and_shows_a_dialog`; `pr-core/tests/file_actions_service.rs:324` `execute_async_move_size_changed_source_gone_failed_journal_and_source_removed` (sự kiện `source_removed`, F3) |
| INV-4 | Chỉ một file action chạy cùng lúc | hai lớp: `FileActionService::try_begin` `service.rs:222` (CAS `in_progress`) + `GateGuard` `service.rs:1174`; lớp UI `coordinators/file_action_gate.rs:129-166` (`release`), `:202` (`try_enter`); `UndoService::begin_hold` `undo.rs:539`; retry `recovery_retry.rs:316` | `file_actions_service.rs:963` `file_action_gate_trait_forwards_to_the_one_gate`; `file_actions_matrix.rs:346` (một journal, một service, undo dùng chung gate); `pr-app/tests/coordinators_file_action_gate.rs` |
| INV-5 | Không sửa catalog/session của folder mới; APP-03 vẫn đăng ký Undo cho action cũ thành công; thất bại chỉ trả ảnh vào folder gốc nếu còn hiện hành | `actions.rs:444-448` (stale-folder guard), `:479-507` (restore), `:680-747` (`executor_threw`); `review_catalog.rs:299` (`restore`) | `viewmodels_main_commands.rs:811` (ở trên); `file_actions_matrix.rs:1095` (undo không báo khôi phục thứ chưa đủ); `pr-app/tests/file_action_core.rs` |
| INV-6 | Recycle; journal Prepared -> Committed/Failed; reconcile khi khởi động | `service.rs:393/473/717` (`tx.begin()` TRƯỚC mọi mutation); `transaction.rs:137-160,212-223,249-293`; `journal.rs:653-684`; `reconcile.rs:99,286,411`; `startup_recovery.rs:44-67` | `file_actions_matrix.rs`: `move_matrix:1445`, `copy_matrix`, `recycle_matrix`, `group_*_matrix`, `undo_*_matrix`, `reconcile_interrupted_*` (1628-1732), `action_crashed_before_each_call_restart_is_consistent_move:2615`, `torn_journal_append_restart_is_consistent_move:2674`, `torn_append_after_successful_appends_next_append_starts_on_new_line:2690`; `journal_reconcile.rs`, `journal_startup_recovery.rs` |
| INV-7 | Bỏ snapshot Explorer trễ khi catalog đã tương tác/đổi | `folder_load/load.rs` (~456-487 cổng thứ tự), `review_catalog.rs:511` (`replace_order` chỉ nhận hoán vị), `snapshot_validator` | `pr-app/tests/folder_load_order.rs:21` `late_order_is_ignored_after_user_interaction` |
| INV-8 | Handle decode share ReadWrite/Delete, SequentialScan, đóng sớm | `io/physical.rs:1217-1231` (`open_read_shared`: `FILE_SHARE_READ|WRITE|DELETE` + `SEQUENTIAL_SCAN`); phía decoder chưa đọc | ops `openread-then-rename` (PARITY); `pixel_memory.rs:650` |
| INV-9 | Mở file: present ngay, điều hướng/action chờ `PendingOrder` | `folder_load/state.rs:269,280`, `reload.rs:29,139-153`; `actions.rs` không gọi cổng này (view model giữ) | `folder_load_load.rs:1067` `direct_file_open_with_pending_snapshot_presents_the_file_and_holds_the_order_gate`; `viewmodels_main_navigation.rs:68` |
| INV-10 | Logging mặc định tắt; tắt thì không tạo log | `diagnostics/file_log.rs:281` (`write_entry` thoát sớm khi `enabled` false) (chưa đọc sâu) | `diagnostics_file_log.rs:304` `disabled_logging_creates_no_directory_or_file` |
| INV-11 | Config hỏng backup/reset; config khoá dùng default RAM | `settings/store.rs:291` (`load`), cờ `keep_corrupt_file` `:92,273` (chưa đọc sâu) | `settings_store.rs:2654` `salvage_backup_fails_once_config_is_still_never_overwritten`, `:2671` `explicit_unreadable_file_does_not_block_default_save` |
| INV-12 | Backend lỗi fallback WPF/WIC; cache không lẫn backend | `pr-imaging/src/fallback.rs:65` (`is_fallbackable`), `ImageCacheKey` (chưa đọc sâu) | `pr-imaging/tests/fallback.rs` (INV-12 whitelist, 998 busy decoder), `pr-shell/tests/composition_factories.rs:556` |

## 2. Đã đọc và thấy đúng
- **Thứ tự Prepared trước mutation** (`service.rs`): Move/Copy 393 (trước 400/414), Recycle 473 (trước 475-480), group 717 (trước vòng 720). Prepared lỗi -> `Err` trước khi chạm đĩa; thư mục đích do lần gọi tạo được dọn ở 515-517.
- **Move không ghi đè**: `physical.rs:799-805` dùng `MoveFileExW` không có `MOVEFILE_REPLACE_EXISTING` (`overwrite=false`, `move_file` 1158-1163); Copy dùng `COPY_FILE_FAIL_IF_EXISTS` (`physical.rs:809-823`, bằng chứng tạo `mark_created` chỉ sau khi copy trả về, 1195-1198). Hai lớp kiểm tra tồn tại trước đó (service.rs 366, 633) chỉ là thông báo sớm.
- **Recycle không xoá vĩnh viễn nhầm**: `can_recycle` + `fits_in_recycle_bin` trước Prepared (service.rs 444-459, group 647-685); `delete_permanently` chỉ chạy khi `allow_permanent_delete` và ổ được nhận diện là không có thùng (`recycle_bin.rs:192-204`); retry đọc lại cài đặt mỗi lần (`factories.rs:36-58`); batch trùng lặp từ chối nếu thiếu thùng (`duplicate_cleanup.rs:561-573`).
- **Xác minh sau thao tác**: `verify_moved`/`verify_destination_stat` (`transaction.rs:177-200`), B-06 (`service.rs:482`, 761) coi shell báo thành công mà nguồn còn là lỗi.
- **Bù trừ group**: Copy xoá chỉ bản mình tạo và còn nguyên dấu vết (`remove_created_copies` 948-1000, R01b), Move đưa về chỉ khi destination khớp vân tay (1011-1044); Recycle không bù trừ nhưng đăng ký Undo cho phần đã vào thùng (`actions.rs:474-477`, `undo.rs:408-466`).
- **Journal**: sửa đuôi dòng cụt (`journal.rs:671-673`, 703-716), một lần `write_all` cho cả batch (674), ghi bền + flush đĩa ở PowerLossSafe (675-682), thử lại khi vi phạm chia sẻ (688-700), cổng reentrant theo từng instance (285-334).
- **Reconcile**: chỉ xét dòng cuối của từng Id (FA-01), bỏ Id đang có marker sống (`reconcile.rs:290`), marker là sự kiện kernel đặt tên theo SID nên thấy được giữa các process (`live_operation_registry.rs:60-82`), thêm kết quả chỉ khi dòng cuối vẫn Prepared (`append_if_still_pending`), mỗi mục bọc `catch_unwind` (reconcile 113-123). Không `unwrap`/`expect` trong mã production của các file đã đọc (grep `unwrap()|expect(` chỉ ra test).
- **Nén journal** (`compaction.rs`): chỉ bỏ dòng cũ của Id mà dòng hiệu lực cuối là Committed/Dismissed; giữ mọi dòng của Id còn Prepared/Failed, 200 Move committed gần nhất, bằng chứng sửa group; thay thế nguyên tử khi còn giữ handle deny-writers (`journal.rs:760-791`).
- **Cổng UI** (`file_action_gate.rs`): Drop của `GateTurn` nhả cổng cả khi panic; không chạy mã người dùng dưới khoá nội bộ.
- **Session** (`session/store.rs:238-301`): lưu nguyên tử không fsync (ADR 0007 §2), file rỗng/hỏng khi đọc cho ra "không có session" kèm cảnh báo.
- `sibling_navigator.rs`: không có đường ghi/xoá/di chuyển file (grep không có `move_file|delete|rename`), chỉ liệt kê thư mục.

## 3. Phát hiện

### M1 (trung bình, xác nhận bằng đọc mã): dọn trùng lặp không kiểm lại bản "giữ lại" và không đăng ký Undo
- File: `pr-app/src/coordinators/duplicate_cleanup.rs:469-611` (`on_hash_finished`), `:615-665` (`start_batch`, `submit_recycle`), `:741-753` (`finish_batch`: "never registered for Undo").
- `duplicates::find` (`pr-core/.../duplicates.rs`, bước 3) chọn danh sách xoá dựa trên băm SHA-256 lúc quét. Từ đó tới lúc recycle có hộp thoại duyệt (`show_batch_review`, modal, có thể hàng phút, 590) rồi từng `executor.execute(Recycle)` tuần tự. Sau hộp thoại chỉ có kiểm tra "folder không đổi", `executor.is_busy()` và `catalog.index_of(path) >= 0` (595-606). Không có bước nào xác nhận bản được giữ vẫn tồn tại hoặc bản bị xoá vẫn có đúng nội dung/size/mtime lúc băm.
- Kịch bản A: thư mục có `IMG_1.jpg` (giữ) và `IMG_1 (1).jpg`, `IMG_1 (2).jpg` (cùng nội dung). Trong lúc hộp thoại mở, người dùng (hoặc Lightroom/đồng bộ đám mây) xoá hoặc đổi tên `IMG_1.jpg` ngoài ứng dụng. Danh sách xoá vẫn còn 2 bản kia, catalog vẫn liệt kê chúng -> cả hai vào Thùng rác -> mất bản duy nhất trong thư mục, Undo không có (`finish_batch` ghi rõ không đăng ký Undo, RV-A10/D5).
- Kịch bản B: `IMG_1 (1).jpg` vừa được sửa trong Photoshop trong lúc hộp thoại mở (nội dung khác bản gốc nhưng vẫn trong danh sách xoá); recycle thẳng bản đã sửa.
- Mức: dữ liệu còn trong Thùng rác nên khôi phục được bằng Explorer; thành "trung bình" vì người dùng không được báo và Ctrl+Z không hoạt động. Tương đương C# (`DuplicateCleanupController.cs`, đã xem cấu trúc, parity) nên đây là lỗ hổng thừa kế, không phải hồi quy.
- Gợi ý: trước mỗi `execute(Recycle)` trong `submit_recycle`, stat lại file đích và ít nhất một bản giữ lại (size + mtime khớp snapshot lúc băm); đưa các lần recycle thành các mục Undo (nhóm). Cần quyết định sản phẩm vì lệch C# (Q3 bên dưới).
- Test nên thêm (`pr-app/tests/duplicate_cleanup.rs`): `batch_recycle_skips_file_whose_keeper_vanished_during_review` (fake file system xoá bản giữ trong callback `show_batch_review`; kỳ vọng 0 lần `execute`, status BatchCanceled); `batch_recycle_skips_file_modified_since_hash`. Test cũ `find_*` ở `pr-core/tests/file_actions_duplicates.rs` chỉ chứng minh lúc quét.

### M2 (trung bình, thiết kế): `JournalDurability::Fast` là mặc định, dòng Prepared không bền khi mất điện
- File: `pr-core/src/settings/app_settings.rs:433` (`Fast`), `journal.rs:667-684` (chỉ khi `PowerLossSafe` mới mở `WRITE_THROUGH` + `sync_all`; Fast chỉ `flush()`), `pr-shell/src/composition/registry.rs:303` (provider đọc cài đặt).
- Kịch bản: Fast, người dùng Delete/Move ảnh; Prepared nằm trong cache hệ thống, NTFS ghi metadata (đổi tên/đưa vào thùng) ra đĩa trước khi cache journal được ghi; mất điện -> sau khởi động thư mục mất ảnh nhưng journal không có dòng nào (không Prepared, không Committed). Reconcile không có gì để xét, cửa sổ Recovery không hiển thị, Undo (chỉ trong RAM, quyết định P03) cũng không có -> ảnh nằm ở đích không ai biết. Với Move cùng ổ đây là rename nguyên tử nên không hỏng file, chỉ mất dấu vết. Điều này trái với câu "journal Prepared TRƯỚC mutation" của INV-6 khi nhìn ở mức mất điện.
- C# giống hệt (ADR 0007, IO03: Fast là mặc định có chủ ý để giữ tốc độ duyệt), nên không phải lỗi port. Ghi lại để chủ sản phẩm quyết định (Q1).
- Test nên thêm: `pr-core/tests/file_actions_journal.rs`: mô hình hoá "mất cache" bằng fake `AppendStream` bỏ phần chưa `flush_to_disk`, rồi `restart` và khẳng định (Fast) dòng Prepared có thể mất, (PowerLossSafe) không mất. Hiện chỉ có `journal_entries_are_durably_written_as_jsonl:713` và `read_skips_corrupt_trailing_line` (1579), không mô hình hoá mất cache.

### M3 (trung bình, xác nhận bằng đọc mã): executor panic -> trả lại ảnh và không đăng ký Undo dù file có thể đã di chuyển/xoá
- File: `actions.rs:337-365` (job bọc `catch_unwind`, panic thành `ExecOutcome::Panicked`), `:427` -> `executor_threw(.., bookkeeping=false)` `:680-747`; `service.rs:249-251` (doc: panic trong `FileSystem`/`RecycleBin` "NOT caught", dòng Prepared ở lại cho reconcile).
- Kịch bản: Move đã hoàn tất `move_file` rồi một lời gọi sau đó panic (ví dụ `verify_moved` -> `try_get_file_stat` của một `FileSystem` có `unwrap` hoặc lỗi chỉ-mục) hoặc `RecycleBin::send_to_recycle_bin` panic sau khi Shell đã chuyển file. `executor_threw` thấy `flight.is_remove && source_index >= 0 && folder_current` nên `catalog.restore(source, index)` (718-721) đưa lại một mục mà file đã đi mất, không gọi `with_undo(register)` (Undo không có), trạng thái "ActionFailed". Dòng Prepared chỉ được xử lý ở lần khởi động sau (marker nhả bởi `Drop`, `transaction.rs:310-314`; reconcile loại Id có marker còn sống nên trong phiên này cũng không xử lý).
- Hiện trạng tốt: `GateGuard` vẫn nhả cổng, nên không treo. Panic ở đây là sự cố hiếm, nhưng quy tắc dự án cấm `unwrap` ở đường này chính để loại trường hợp này; cần một chốt chặn kiểm tra thực tế.
- Gợi ý: trong nhánh `Panicked` của `on_executed`, hỏi đĩa như `single_failure` (S7.5, `service.rs:545-548`): nếu nguồn không còn thì không `restore`, coi như unverified và hiện trạng thái; hoặc chuyển `catch_unwind` vào chính `FileActionService::execute` để nó tự journal Failed.
- Test nên thêm (`pr-app/tests/file_action_core.rs`): `executor_panic_after_move_completed_does_not_restore_catalog_entry` (executor giả di chuyển file rồi panic; kỳ vọng catalog không chứa lại nguồn đã mất). Test hiện có `two_scripted_executor_panics_in_a_row_both_report_boom:606` chỉ kiểm tra thông báo "boom".

### L1 (thấp): `end()` không phân biệt người giữ cổng
- File: `service.rs:229-231` (`self.in_progress.store(0)` vô điều kiện), cũng `UndoService::end` `undo.rs:356-361`, trait `FileActionGate::end` `undo.rs:56`.
- Kịch bản: mã gọi `end()` mà không giữ cổng (hoặc gọi hai lần) nhả cổng của một action đang chạy -> action thứ hai bắt đầu song song (vi phạm INV-4). Hiện tất cả đường nội bộ dùng RAII (`GateGuard`, `GateHold`, `GateGuard` của retry) nên chưa có đường thực tế; là API công khai dễ dùng sai. Parity C# (`Volatile.Write(0)`).
- Test nên thêm: `file_actions_service.rs`: `end_called_by_non_holder_while_action_runs_documents_current_behaviour` (khoá hành vi hiện tại bằng test, hoặc chuyển `end` thành `pub(crate)`).

### L2 (thấp): INV-4 phụ thuộc cách nối dây của composition root
- File: `recovery_retry.rs:301-305` (`with_file_action_gate` tuỳ chọn), `undo.rs:296` (`with_gate` tuỳ chọn, mặc định `own_gate` riêng), `pr-shell/src/composition/factories.rs:36-58` (tham số `Option`), `registry.rs:340,359` (đang truyền cổng đúng).
- Kịch bản: một đường dựng service mới (test, công cụ, hoặc sửa registry sau này) quên truyền cổng -> Undo/Retry chạy song song với Delete/Move mà không thông báo lỗi; INV-4 vỡ âm thầm.
- Test nên thêm (`pr-shell/tests/composition_*.rs`): `registry_undo_and_retry_share_the_file_action_service_gate` (lấy `try_begin` trên `file_action_service()`, khẳng định `undo_service.is_busy()` và retry trả `core_recovery_busy`).

### L3 (thấp, quyết định sản phẩm?): hộp thoại lỗi của Shell không chủ có thể giữ cổng vô hạn
- File: `pr-platform-win/src/recycle_bin.rs:343` (`FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOCONFIRMMKDIR | FOF_SILENT | FOFX_RECYCLEONDELETE`, KHÔNG có `FOF_NOERRORUI`), `:307-314` (chạy trên luồng STA riêng, `join()` chờ không giới hạn).
- Kịch bản: file đang bị chương trình khác khoá chia sẻ; Shell hiện hộp thoại lỗi không có cửa sổ cha, có thể nằm sau cửa sổ chính. Luồng `PhotoReviewRs-RecycleShell` chờ người dùng bấm; trong thời gian đó `FileActionService::in_progress` = 1 và mọi action/undo/retry trả "A previous operation is still running". Dòng Prepared vẫn "sống" (marker) nên reconcile bỏ qua.
- C# cố ý không dùng `FOF_NOERRORUI` ("lỗi vẫn hiện hộp thoại như bản VB", `ShellFileRecycler.cs:20`) nên đây là hành vi parity; ghi lại vì hệ quả với cổng. Gợi ý: thêm cờ owner hoặc timeout/cảnh báo; cần quyết định (Q2).
- Test nên thêm: native `#[ignore]` hoặc seam: `shell_recycle` giả không bao giờ trả -> khẳng định UI vẫn hiện trạng thái "đang chạy" và hủy được (hiện không có).

### L4 (thấp): Move chéo ổ để lại bản sao ở đích khi lỗi/mất điện, không có đường dọn
- File: `physical.rs:799-805` (`MOVEFILE_COPY_ALLOWED`: chéo ổ là copy rồi xoá), `service.rs:409-417` (không bù trừ Move đơn khi `move_file` lỗi giữa chừng), `reconcile.rs:286-330` (Move: nguồn còn -> Failed `PendingUnconfirmed`, không dọn đích), `recovery_retry.rs:~397-412` (retry từ chối nếu đích đã tồn tại: `core_recovery_destination_exists`).
- Kịch bản: Move sang ổ USB; copy xong nhưng xoá nguồn bị từ chối (file chỉ-đọc/đang mở) -> `MoveFileExW` trả lỗi, đích có bản đầy đủ, nguồn còn nguyên. Journal Failed; catalog trả ảnh lại; thư mục đích giữ một bản thừa; Recovery "thử lại" bị chặn vì đích tồn tại, người dùng phải tự xoá bản thừa. Mất điện giữa chừng: đích có file dở, journal Prepared -> Failed.
- Không mất dữ liệu (nguồn còn) nhưng để lại rác không được liệt kê ở đâu ngoài Recovery. C# giống nhau (cùng API).
- Test nên thêm: native `#[ignore]` (cần hai ổ) hoặc fake `FileSystem` mô phỏng "đích có đầy đủ, nguồn còn": `service.rs` Move trả `succeeded=false`, journal Failed, và kiểm tra hành vi mong muốn (dọn bản mình tạo nếu có bằng chứng, hoặc ghi chú trong lỗi) để khoá quyết định.

### L5 (thấp): hoàn tác Recycle đơn không được journal
- File: `undo.rs:754-812` (`undo_single_recycle`: `try_restore` + kiểm chứng R18, không `JournalTransaction`), trong khi Move đơn (`undo.rs:684-710`) và nhóm (`undo.rs:930-947`, `1119-1134`) đều journal.
- Kịch bản: mất điện/crash giữa lúc Shell khôi phục file: không có dòng Prepared để reconcile phát hiện; Recovery không hiện gì. Bản thân khôi phục dùng thùng rác nên không mất dữ liệu; thiếu dấu vết. C# `UndoLastAsync` (364) cũng không journal nhánh này (parity).
- Test nên thêm: ghi nhận hiện trạng `undo_single_recycle_writes_no_journal_line` (khoá hành vi, bắt buộc suy nghĩ lại nếu đổi).

### L6 (thấp): `journal_persisted`/`journal_error` của action không bao giờ tới người dùng
- File: `service.rs:421-433` (`journal_persisted: journal_error.is_none()`), nhưng `grep journal_persisted` chỉ khớp `pr-app/src/presenters/recovery_window.rs:766`; không có trong `coordinators/file_actions/*`.
- Kịch bản: đĩa đầy/journal bị khoá khi ghi Committed: file đã đổi nhưng dòng cuối còn Prepared; UI báo thành công. Tự lành ở lần khởi động sau (reconcile phân xử theo đĩa), nhưng nếu ghi cả Failed cũng lỗi thì không ai biết trong phiên. Parity C# (`RecoveryWindowPresenter.cs:325` là nơi duy nhất).
- Test nên thêm (`pr-app/tests/file_action_core.rs`): `committed_line_not_persisted_is_logged_or_shown` sau khi quyết định hành vi (hiện chưa có).

### L7 (thấp): dọn dẹp bản Copy dở chỉ có tác dụng khi copy đã trả về
- File: `service.rs:406` (`run.copied_stat` chỉ được gán sau khi `try_copy_new_with_proof` thành công), `:519-523` (cần `proof.destination_created()` và `copied_stat`), group: `:737-741` (`created_copies.insert` sau thành công), `:980-997` (nhánh in-flight cần `created_copies.get`), `physical.rs:1194-1198` (`mark_created` chỉ sau `Ok`).
- Hệ quả: `partial_cleanup::remove_if_partial` (`partial_cleanup.rs:24`) chỉ chạy được cho file copy ĐÃ HOÀN TẤT rồi mới `verify` hỏng; một Copy lỗi giữa chừng không bao giờ được dọn bởi mã này. Phần còn lại do `CopyFileExW` tự xoá file dở khi lỗi thường (Windows làm vậy). Process bị giết/mất điện: file dở ở lại, Prepared -> reconcile Failed, không dọn. Trong group, nhánh in-flight đếm "stuck", `rolled_back=false` nên journal là Failed thay vì Dismissed. Hành vi giống C# (`copiedStat` 693/806).
- Test nên thêm: `file_actions_service.rs`: `copy_fails_mid_write_leaves_no_partial_destination_or_reports_it` (fake `try_copy_new_with_proof` tạo file ngắn rồi trả `Err`): khoá hành vi mong muốn.

### L8 (thấp): reconcile coi Recycle đơn là xong chỉ vì nguồn không còn
- File: `reconcile.rs:305-317` (Recycle đơn: `verdict(!source_exists)`), nhóm `is_group_member_completed` `:411-421` (`!file_exists(source)`).
- Kịch bản: crash trước khi Shell chuyển file; người dùng tự xoá/đổi tên file bằng Explorer trước khi mở lại ứng dụng -> journal ghi Committed (đã xoá vào thùng) dù file không có trong thùng. Chỉ ảnh hưởng nhật ký/Recovery, không đổi file. Parity C# (R15 chỉ làm chặt cho nhánh undo).
- Test nên thêm: `journal_reconcile.rs`: `recycle_prepared_with_source_missing_but_not_in_bin_documents_committed` (khoá hành vi; bin giả cho biết file không có).

### Q1 (câu hỏi sản phẩm): đổi mặc định `JournalDurability` thành PowerLossSafe?
Xem M2. Chi phí: mỗi action thêm một fsync (journal nhỏ nên vài ms trên SSD, nhiều chục ms trên HDD); lợi: không mất dấu vết khi mất điện. Cần chủ sản phẩm quyết định, ghi ADR.

### Q2 (câu hỏi sản phẩm): `FOF_NOERRORUI` hoặc hộp thoại có chủ cho Recycle
Xem L3. Hai phương án: giữ như C# (parity), hoặc thêm `FOF_NOERRORUI` và báo lỗi bằng cơ chế của ứng dụng (lệch C# nên cần `Different` row trong PARITY).

### Q3 (câu hỏi sản phẩm): dọn trùng lặp có nên nằm trong Undo và journal nhóm?
Xem M1. Hiện mỗi bản được recycle riêng với dòng journal riêng; không có Ctrl+Z cho cả lô. Lệch C# nếu sửa.

## 4. Các điểm đã loại trừ (đã nghĩ tới, không phải lỗi)
- `FileActionGate::release` trừ `queue_length` (`file_action_gate.rs:138`): không tràn vì `GateTurn::release` có cờ `finished`, và `TurnKind::Queued` chỉ được tạo ở 158-162 sau khi tăng.
- Hash trùng lặp dùng SHA-256 (`misc/file_hash.rs:4`) nên va chạm không đáng kể.
- `undo_last` kiểm `is_busy()` rồi mới `begin_hold()` (không nguyên tử) nhưng `begin_hold` dùng CAS và trả `rejected` nếu thua (`undo.rs:539-547`).
- Session ghi không fsync: chủ ý (ADR 0007) và đường đọc xử lý file hỏng.

## 5. Test thiếu so với yêu cầu của đề bài (tổng hợp)
1. Mất cache journal (Fast vs PowerLossSafe): chưa có.
2. Panic của `FileSystem`/`RecycleBin` giữa action: chỉ có test thông báo "boom", chưa có test trạng thái catalog/Undo/journal.
3. Chống `end()` sai người, cổng dùng chung trong composition: chưa có.
4. Dọn trùng lặp: chưa có test bản giữ lại biến mất/bị sửa trong lúc xem lại.
5. Move chéo ổ lỗi nửa chừng: chỉ có native `#[ignore]` (`file_actions_service_native.rs` 19 test bị bỏ qua), chưa chạy trên CI Linux.
## 6. Tái kiểm Machine C ngày 2026-10-23 (Rust `85b8bf0`, C# `7eaee2a2`)

**Mọi phát hiện bổ sung: suy luận từ mã, chưa chạy thử.** Không chạy Cargo, oracle, mutation hoặc thao tác Recycle Bin. Đây là bổ sung cho lần đọc cũ, không phải xác nhận runtime cho toàn bộ bảng INV. Không thay đổi mã Rust.

### H1 (cao, thừa kế C#): Recovery retry nhóm Recycle không kiểm lại fingerprint sau Prepared

- Chủ sở hữu: W3-06 (`pr-core` recovery retry); W8-18v chỉ là bên gọi UI, không nên sửa riêng tại dialog.
- Rust: `crates/pr-core/src/file_actions/recovery_retry.rs:505` (`precheck_group_members`) kiểm size/mtime trước transaction; `:596` ghi Prepared qua `tx.try_begin`; `:609-648` nhánh Recycle chỉ kiểm path trống khi undo, hoặc loại ổ khi permanent, rồi gọi bin. Khác nhánh Move/Copy `:651-657`, không stat lại nguồn ngay trước mutation.
- Kịch bản cụ thể: journal Failed có member B được đánh dấu `permanent=true`, cho phép permanent-delete đang bật, B trên ổ được xác nhận không có bin. Precheck thấy B đúng size/mtime. Trong lúc append Prepared bị chậm hoặc đang xử lý member A, chương trình khác thay B bằng ảnh khác size/mtime. Tới B, `can_recycle` vẫn false -> `delete_permanently(B)` xoá vĩnh viễn ảnh thay thế, trong khi journal lưu fingerprint ảnh cũ. Với member không permanent, ảnh thay thế bị recycle nhầm. Gate INV-4 chỉ khoá thao tác của app, không khoá chương trình ngoài.
- Adapter `crates/pr-platform-win/src/recycle_bin.rs:192-210` kiểm loại ổ và nguồn đã mất sau delete, không kiểm fingerprint; vì vậy adapter không chặn kịch bản trên. Không cần chạy thùng rác thật để chứng minh nhánh logic này.
- C# baseline: `src/PhotoReview.Core/FileActions/RecoveryRetryService.cs`, `ExecuteGroupRetry` có cùng nhánh `else if (member.Permanent)` -> `DeletePermanently(member.Source)` mà không re-stat; đây là rủi ro thừa kế, thay đổi hành vi cần ADR và hàng Different theo quy tắc parity.
- Test đã đọc: `crates/pr-core/tests/recovery_retry.rs:960` `retry_move_or_copy_delete_group_member_resized_after_the_check_is_refused_and_nothing_recycled` chỉ bảo vệ khoảng trước transaction; `:1084` `retry_move_or_copy_move_group_source_resized_after_prepared_fails_as_source_changed_without_moving` bảo vệ Move, không Recycle; `:2748` `retry_group_permanent_member_whose_drive_gained_a_recycle_bin_after_the_pre_check_is_not_deleted` bảo vệ loại ổ, không fingerprint.
- Test nên thêm (đề xuất, chưa tồn tại): `retry_group_permanent_source_replaced_after_prepared_is_not_deleted`, fake append hook thay B sau Prepared; kỳ vọng 0 lần `delete_permanently(B)`, file thay thế còn nguyên bytes, kết quả thất bại SourceChanged và dòng Failed. Thêm biến thể non-permanent và thay B trong callback recycle A. Không làm yếu các test cũ; mọi bin đều fake. Re-stat thu hẹp khoảng race, chưa tạo bảo đảm nguyên tử với tiến trình ngoài; cần ghi giới hạn đó trong ADR.

### Đính chính INV-5: ngoại lệ APP-03 là chủ ý của oracle

`docs/architecture.md` C# vẫn tóm tắt INV-5 là không sửa catalog/undo sau đổi folder, nhưng test baseline mới hơn giải nghĩa ngoại lệ: `tests/PhotoReview.Integration.Tests/MainWindowBehaviorTests.FolderSwitch.cs:123`, tên `MoveCompletingAfterFolderSwitchDoesNotMutateNewCatalogOrRegisterUndo`, có DisplayName và assert yêu cầu đăng ký Undo. Tên test cũ không phản ánh assertion hiện tại. Rust `actions.rs:442-448,545,844` thực hiện ngoại lệ này. Các test Rust `move_finishing_after_folder_switch_is_registered_and_leaves_new_folder_untouched` (`file_action_core.rs:914`) và `recycle_finishing_after_folder_switch_is_undoable` (`:937`) là bằng chứng mã test, chưa được chạy trong lần review này. Không coi việc đăng ký Undo muộn là vi phạm INV-5.

### Đính chính M3: live marker không chặn mãi trong phiên

Lập luận cũ “trong phiên này cũng không xử lý vì marker sống” quá mạnh: marker được nhả khi transaction Drop. Sau panic, một lần reconcile mới có thể xét Prepared. Giới hạn thực tế là UI không tự động reconcile ngay trong nhánh `executor_threw`; nguồn được restore vào catalog mà không chứng minh lại. M3 vẫn còn, nhưng không nên mô tả reconcile là bất khả thi trước lần khởi động kế tiếp.

### Bằng chứng durability và giới hạn

`journal.rs:667-684` dùng `sync_all` hoặc seam flush khi PowerLossSafe và stream là File; stream khác vẫn plain flush theo parity. Các test `append_power_loss_safe_on_a_real_file_flushes_to_disk_once_per_append_after_the_bytes_were_written` (`file_actions_journal.rs:1680`, native ignored), `append_fast_mode_never_flushes_to_disk` (`:1729`, native ignored), `append_power_loss_safe_on_a_stream_that_is_not_a_file_stream_falls_back_to_a_plain_flush` (`:1762`) có kiểm đường flush. Test native đầu dùng seam recorder, không thực sự cắt điện; không gọi đó là chứng minh chịu mất điện. Đề xuất mô hình mất cache ở M2 vẫn cần.

### Phạm vi bổ sung đã đọc

Đọc `recovery_retry.rs:555-810` (transaction và toàn bộ vòng retry), `recovery_check.rs` phần type/check, `recycle_candidate.rs`, `recycle_bin_capacity.rs`, `recycle_bin_shell.rs` phần COM apartment/enumeration/candidate/restore và `recycle_restore.rs` phần chọn đúng một candidate. Candidate không rõ/không duy nhất bị từ chối; lỗi per-item được log rồi bỏ qua. Không kết luận bộ COM/native này an toàn runtime chỉ từ việc đọc mã. `folder_load/load.rs:456-487` vẫn giữ PendingOrder khi mở file và Explorer pending; `actions.rs:444` vẫn chặn completion chạm catalog folder mới.
