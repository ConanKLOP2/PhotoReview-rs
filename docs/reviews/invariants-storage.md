# Review bổ sung: cache, handle đọc, config và logging (INV-1/2/8/10/11/12)

Machine C (cloud), 2026-10-23. Rust `809759d`; C# `ConanKLOP2/PhotoReview` tại `7eaee2a2b071c7ca05291c86fb78a347f9e6a15f`, chỉ đọc bằng git show. Claim `REVIEW-INVARIANTS-STORAGE`.

**Mọi phát hiện: suy luận từ mã, chưa chạy thử.** Không chạy Cargo/test/oracle/mutation, không tạo ảnh/corpus/sparse file, không mở cửa sổ hoặc thao tác Recycle Bin. Không sửa Rust. Báo cáo này bổ sung những vùng `invariants.md` ghi chưa đọc sâu; không thay bằng chứng runtime còn thiếu bằng kết luận review.

Tổng bổ sung: **0 cao / 3 trung bình / 1 thấp**. M1/M2 là rủi ro có cùng đường xử lý trong oracle; M3 là khác biệt về thứ tự kiểm/cấp phát được contract cũ cho phép; L1 cần test ghi dở thật. Không đếm lại H1 retry-Recycle, B4 RefCell, General collect hay các stub Wave 9 đã có trong BACKLOG.

## Phạm vi đã đọc và giới hạn

- `crates/pr-imaging/src/cache_key.rs`: identity, create/create_for_entry/create_original, matches_current_source; `with_path` W9-R4 còn todo, đã có trong báo cáo W9 nên không đếm mới.
- `service/decode.rs`: in-flight registration, source/disk decode, verify, publish/dimensions/persist, source-byte handoff; `service/mod.rs`: evict/clear/publish; `service/disk.rs`: persist queue, hai worker, epoch/owner bookkeeping, Clear Disk và test hai worker.
- `cache/source_bytes.rs`: RangeKey, in-flight join, read/stat/publish, generation/path version; `cache/thumbnails.rs`: get/observe/clear/load, shared disk read; `cache/preview_file.rs`: read/parse/payload limits và codec errors; `cache/atomic_file.rs`: create-new temp, close-before-rename và rename overwrite. `disk_store.rs` chỉ đọc clear/access/prune entry points, không kiểm toán đầy đủ mọi pass quota/mtime.
- `pr-core/src/settings/store.rs`: load, salvage/repair backups, keep-corrupt guard, persist/newer backup; `diagnostics/file_log.rs`: enqueue, enable/disable/shutdown, batch drain/retry và formatting/error rendering.
- `pr-core/src/abstractions/source_reader.rs`, `io/physical.rs` nhánh open_read_shared; `pr-imaging/src/embedded_thumbnail.rs` nhánh đọc/giải phóng; `pr-platform-win/src/wic/decoder.rs` nhánh mở stream/decode/drop. Không kiểm chứng native COM reference count hoặc mọi backend LibRaw/TurboJPEG trong lần này.

## Bảng invariant: nơi chặn và test hiện có

Tên test dưới đây là bằng chứng về nội dung source test, **chưa chạy thử**; không có nghĩa toàn bộ INV đã được chứng minh runtime.

| INV | Chốt trong Rust | Test đã đọc/đối chiếu |
|---|---|---|
| INV-1 | `cache_key.rs:103` chứa path/size/ticks/mode/box/orientation/backend/source kind; `:304` kiểm source; `service/decode.rs` verify sau decode/disk hit trước publish | `tests/cache_key.rs:270` `key_equality_matches_identity_fields`; `tests/preview_service_decode.rs:1140,1146` `source_replaced_during_decode_is_never_cached` và bản disk |
| INV-2 | `service/mod.rs:862` compare epoch + RAM set chung lifecycle gate; source bytes `cache/source_bytes.rs:528` read rồi stat và publish dưới gate với generation/path version; thumbnail `observe` kiểm generation dưới lifecycle lock | `service/mod.rs:1067` `clear_cache_bumps_the_epoch_so_publish_if_current_refuses_the_old_epoch`; `tests/cache_source_bytes.rs:955,961` hai test `evict_between_check_and_set_does_not_republish_*`; M1 cho khoảng trống persist |
| INV-8 | `pr-core/src/abstractions/source_reader.rs:48` mở ReadWrite/Delete + SequentialScan; `io/physical.rs:1217` cùng cờ; `embedded_thumbnail.rs:146` frame/stream scoped; WIC decode drop stream trước return | `pr-imaging/tests/embedded_thumbnail.rs:671,693` `try_read_opens_the_file_shared_exactly_once`, `try_read_releases_the_stream_before_returning`; test native sharing không được chạy |
| INV-10 | `file_log.rs:280` thoát trước enqueue khi disabled/stopping; `:693` cũng thoát trước render_error; writer không được khởi động từ constructor | `pr-core/tests/diagnostics_file_log.rs:304` `disabled_logging_creates_no_directory_or_file`; `:352` `disabling_logging_stops_writes` |
| INV-11 | `settings/store.rs:291` load, `:377-417` backup trước ghi repair; `:619-630` keep_corrupt_file giữ file trên đĩa và cập nhật RAM | `settings_store.rs:489` `load_inv11_when_corrupt_config_backs_up_and_resets_to_default`; `:545` `load_inv11_when_file_locked_uses_in_memory_defaults_and_leaves_file_intact`; `:1404` `load_repair_backup_fails_config_untouched_and_repaired_in_memory` |
| INV-12 | `fallback.rs:65` whitelist loại lỗi, không fallback OOM/cancel/busy; source bytes được chuyển cho fallback; requested backend nằm trong key, actual backend nằm trong decoded/cache entry | `tests/fallback.rs:1239,1266` `fallbackable_types_use_the_fallback`, `non_fallbackable_types_propagate`; `:1320` `fallbackable_failure_bytes_are_handed_to_the_fallback`; M3 cho allocation ngoài codec |

## M1 — trung bình: writer epoch cũ có thể rename đè file writer mới, rồi cleanup giữ chính file cũ đó

- Chủ: W4-14c (`pr-imaging::service::disk`), liên quan W4-08a atomic publication. BACKLOG G26.
- Rust: `service/disk.rs:491` claim path, `:497-507` write_atomically không truyền token huỷ, `:517-518` chỉ kiểm epoch sau khi rename đã xong; owner table `:530-548` giữ epoch lớn nhất đến khi writer cuối ra; `:554-565` không xoá khi owner_epoch > request.epoch. `cache/atomic_file.rs:141-142` kiểm token rồi rename overwrite; token ở persist là `CancelToken::none()`.
- Lịch cụ thể (cùng cache path K): A epoch e claim rồi kẹt trong encode trước rename; Clear Cache/Clear Disk tăng epoch lên n; B epoch n claim K, encode/rename B xong rồi release, table còn `(n,1)` vì A vẫn chạy; A được nhả và rename A đè B; A thấy stale nhưng delete_stale_persist_file thấy n>e nên không xoá. A release, table trống; K còn bytes của A. Check-and-delete có khoá nhưng thứ tự publication không được khoá cùng epoch/owner.
- Hệ quả chắc từ đường mã: một persisted entry thuộc epoch trước clear có thể tồn tại thay entry mới. Cùng K đồng nghĩa cùng source fingerprint/decode identity; không suy diễn rằng A/B nhất thiết có pixel khác trong mọi decoder thật, hay ảnh nguồn bị sửa. Test cần dùng payload A/B phân biệt để kiểm quyền publication theo epoch.
- C# cùng thứ tự: `src/PhotoReview.Imaging/Caching/PreviewImageService.cs:304-322` write rồi stale cleanup, `:339-368` owner table. Đây là rủi ro thừa kế, không kết luận lỗi dịch Rust. Biện pháp thay đổi thứ tự cạnh tranh cần ghi ADR/parity theo quy tắc repo.
- Test hiện có `service/disk.rs:1182` `stale_worker_cleanup_after_newer_worker_wrote_same_path_keeps_newer_file`, nguồn `PreviewImageServicePersistTwoWorkerTests.StaleWorkerCleanup_AfterNewerWorkerWroteSamePath_KeepsNewerFile`, dừng A **sau write**, nên chỉ bảo vệ thứ tự A-write -> B-write -> A-cleanup. Nó kiểm file tồn tại, không xác nhận bytes thuộc B. Test owner table `release_keeps_the_newest_epoch_while_an_older_writer_is_in_flight` cũng không bảo vệ thứ tự rename.
- Test nên thêm (tên đề xuất, chưa tồn tại): `stale_persist_writer_finishing_after_new_epoch_does_not_replace_newer_payload`. Fake PreviewPayloadCodec chặn A trong encode trước return; B chạy xong; nhả A; đọc entry qua fake payload codec và assert payload B, không chỉ exists. Gate/completion có timeout ngoài, cleanup luôn nhả gate trong finally/Drop, không sleep. Không dùng thread timing để đoán đã tới rename.

## M2 — trung bình, cần quyết định sản phẩm: mất trường config từ bản mới hơn khi backup thất bại

- Chủ: W2-03 (`pr-core::settings::store`); BACKLOG G27. Đây là giới hạn ngoài nhánh corrupt/locked chính của INV-11, không gắn nhầm thành vi phạm mọi trường hợp INV-11.
- Rust `settings/store.rs:634-641`: gọi backup_newer_config, xoá newer_file_on_disk rồi serialise version hiện tại và atomic write; `:662-680` backup lỗi chỉ warn rồi trả (), không chặn persist.
- Input: config version >4 chứa trường chưa biết `FutureField` và field hiện có; load giữ file không sửa; lần Save sau, Copy backup bị lỗi nhưng atomic replace thành công -> config version4 mới không chứa FutureField, không có bản backup của nguyên văn. File vẫn là JSON hoàn chỉnh; atomic write không bảo vệ khỏi mất trường do downgrade serialization.
- C# cố ý giống vậy: `src/PhotoReview.Core/Settings/SettingsStore.cs:227-252,262...`, comment nói failure backup không chặn Save. Rust test `settings_store.rs:1762` `save_when_backup_fails_still_writes_the_config` port `SettingsStoreSaveAfterNewerVersionLoadTests.Save_WhenBackupFails_StillWritesTheConfig`; nó assert version4 và warning, không assert FutureField được giữ. Test trước đó còn assert FutureField biến mất khi save; không nên sửa assertion để giả vờ có round-trip trường chưa biết.
- Biến thể lỗi hai bước cần kiểm: backup và atomic write đều lỗi ở lần Save1, sau đó hết lỗi, Save2 chạy khi cờ newer đã false -> không thử backup lại dù file newer vẫn trên đĩa.
- Test nên thêm: `save_backup_and_write_fail_then_next_save_retries_preserving_newer_original` với fake FS ghi call log, config version cao + FutureField; lỗi Copy và WriteAtomic lần đầu, lần sau hết lỗi. Kỳ vọng sau quyết định an toàn: Save2 backup đúng nguyên văn trước write, unknown fields còn trong backup. Nếu giữ chính sách C# hiện tại, viết characterization tên rõ `...documents_no_second_backup_attempt`, assert call count hiện tại, ghi rủi ro và ADR; không gọi đó là test bảo vệ toàn vẹn.
- Cần quyết định: giữ best-effort backup như C# hoặc khi backup newer thất bại chỉ áp RAM/hoãn downgrade. P-52/Q1..Q8 chưa chốt vấn đề này. Review không tự chọn hay sửa production.

## M3 — trung bình: đọc toàn bộ preview cache trước khi kiểm header/payload length, allocation có thể đi ngoài đường lỗi OOM

- Chủ: W4-08b (`pr-imaging::cache::preview_file`), caller W4-14c; BACKLOG G29.
- `cache/preview_file.rs:323` reserve theo metadata.len, `:325` đọc cả file, `:328` mới parse; kiểm payload >i32::MAX nằm ở `:239-240`, quá muộn để ngăn allocation/read. `Vec::reserve` là allocation không trả Result: khi allocator hết RAM, đường mặc định có thể abort thay vì trả PreviewCacheError. `service/decode.rs` catch_unwind không biến allocator abort thành PreviewError. Chưa gây OOM hay chạy thử trên máy.
- Input cụ thể: một `.pv4` ở đúng hash path, header hợp lệ nhưng payload khai theo file length >i32::MAX (hoặc header hỏng + file kéo dài nhiều GiB). Rust cố reserve/read toàn bộ trước khi từ chối; trên máy thiếu commit RAM có thể kết thúc tiến trình trong khi cache lẽ ra chỉ là miss/lỗi đọc. Không cần giả định ảnh nguồn bị ghi/xoá.
- C# kiểm sớm hơn: `src/PhotoReview.Imaging/Caching/PreviewCacheFile.cs:174-...` đọc fixed header, `:209-224` đọc EXIF có giới hạn, `:231-232` từ chối payload >int.MaxValue **trước** ArrayPool.Rent ở 237. Header hỏng cũng từ chối trước đọc payload. Với payload hợp lệ nhưng hết RAM, lỗi runtime vẫn phải được phân biệt với corrupt entry.
- `contracts/W4-08b.md:99-101,198-200` cho phép whole-file read và mô tả khác biệt như vấn đề benchmark. Đó không phải chứng cứ tương đương ở allocation failure; PR này bổ sung giới hạn vào contract, không thay API Rust. Không cần cấp quyền mới để sửa đường kiểm giống oracle.
- Điểm cùng đường cần test: `impl From<io::Error>` ở preview_file.rs:97 biến mọi I/O thành Io, còn is_cache_entry_failure (:78) chỉ trừ Canceled/OutOfMemory. Vì thế lỗi io::ErrorKind::OutOfMemory từ read có thể bị coi là cache hỏng, caller xoá cache và thử decode nguồn. Test OOM hiện có (`tests/cache_preview_file.rs:1781` `to_pixels_out_of_memory_is_not_a_cache_entry_failure`, `:1805` `from_pixels_out_of_memory_is_not_a_cache_entry_failure`) dùng lỗi từ platform codec, chưa bảo vệ allocation của read.
- Test nên thêm: seam Read+Seek/length hoặc helper pure kiểm header/total length: payload len=i32::MAX+1 -> đúng InvalidData, 0 decode call, không xin buffer toàn file; bad magic + huge declared length -> InvalidData sau prefix; inject reserve/read OOM -> OutOfMemory không bị xoá như corrupt. Dùng fake stream/counter, không tạo file nhiều GiB hoặc cố ép allocator máy hết RAM. Ca hợp lệ v6/v7 vẫn byte/pixel như golden.

## L1 — thấp: lỗi ghi dở hai lần có thể làm log lặp hoặc dính dòng ở lượt drain kế tiếp

- Chủ: W2-16 (`pr-core::diagnostics::file_log`); BACKLOG G28. Đây là chất lượng log chẩn đoán, không phải journal operations.jsonl.
- `file_log.rs:401` offset bắt đầu 0 cho mỗi batch; `:402-404` ghi rồi retry phần còn lại một lần. Chỉ khi lần retry hoàn tất mới mark flushed (`:405-410`); nếu cả hai lỗi, queue giữ batch nhưng offset cục bộ mất khi return (`:412`). Drain sau dựng lại toàn buffer và append từ 0.
- Lịch: buffer chứa dòng A và B; write1 ghi xong A và nửa B, rồi lỗi; write2 lỗi ngay; hết lỗi trước drain sau -> append lại A+B vào sau nửa B, tạo dòng dính hoặc lặp. Nếu lỗi ở flush sau khi mọi bytes đã ghi, retry flush thất bại cũng có thể khiến cả batch được append lại lần sau.
- Test `diagnostics_file_log.rs:445` `partial_batch_failure_does_not_duplicate_flushed_entries` không mô phỏng short write: hook trước entry thứ hai panic trong staging, phần staging xong được ghi đầy đủ nên cờ flushed hoạt động. Test này hợp lệ cho staging failure, chưa chứng minh arbitrary partial I/O.
- C# `FileLog.cs:276-290` cũng thử Flush lần hai và chỉ mark Flushed sau khi thành công; chưa có test short-write thật được xác minh. Không khẳng định byte-level hành vi C#/Rust giống nhau trên mọi lỗi StreamWriter.
- Test nên thêm `log_partial_write_and_retry_failure_does_not_repeat_a_completed_line`: scripted Write nhận N bytes, lỗi lần tiếp và lần retry, rồi cho drain sau thành công; assert đúng chuỗi log theo chính sách được chọn, A đúng một lần, không ghép nửa B vào A. Cần seam writer của unit, không làm đầy disk thật, không sleep. Nếu giữ best-effort log, ghi rõ dữ liệu có thể lặp sau persistent I/O và đổi tên phạm vi lời hứa của test cũ; không bỏ assertion cũ.

## Những điểm đã loại trừ hoặc không tính mới

- SourceBytesCache cố ý có RangeKey path ordinal, nhưng eviction path version dùng OrdinalIgnoreCase; đây là parity có chú thích, không tự coi hai cách so là bug.
- RAM preview key do caller cung cấp có thể dùng metadata catalog để tránh stat lại. Không gán mọi RAM hit thiếu stat thành INV-1 lỗi; presenter/generation và hợp đồng source key là phần của bảo đảm.
- Original-dimensions cache có đường seed không epoch-check (chú thích nêu C#); chưa chứng minh nó làm hiện pixel sai nên không thêm phát hiện chỉ từ khác biệt đó.
- Preview disk entry mở share Read/Delete, không share Write; đây là file cache thay nguyên tử, khác file nguồn cần ReadWrite/Delete. Không áp INV-8 nguồn ảnh máy móc cho mọi cache stream.
- Log đã từng bật có thể còn drain queued entries lúc tắt (set_enabled(false) gọi flush); INV-10 “disabled mặc định không tạo log” không đồng nghĩa xoá lịch sử/huỷ ngay mọi I/O đang chạy.
- Các giới hạn W9-R4 with_path, dữ liệu stale-preview và native tests ignored đã có ở báo cáo trước, không đếm lại.

## Thứ tự bàn giao

1. G29: chặn allocation/read không cần thiết trước khi kiểm cache, test fake/pure trước; giữ OOM khác corrupt.
2. G26: test lịch rename A sau B bằng codec gates rồi chốt serialization/epoch publication theo ADR.
3. G27: chốt chính sách newer-config backup, sau đó test hai Save lỗi/phục hồi; không làm yếu test parity hiện có trước quyết định.
4. G28: bổ sung short-write seam và xác định mức bảo đảm log; ưu tiên thấp hơn journal/config.
