# FIX-IMAGING-CACHE 2026-10-31: sửa M1/M2/M3 của ST-IMAGING-INV, kiểm chứng M4

Status: report (lane `fix-imaging-cache`). Nguồn: `ST-IMAGING-INV-2026-10-31.md` (PR #468). Mỗi mục được đọc lại trên mã tại `origin/master` `cbdf3c45` và đối chiếu C# (`7eaee2a2`) trước khi sửa.

| Mục | Kết luận | Test đỏ trước | Test sau |
|---|---|---|---|
| M1 (G26) | Sửa (a) | `stale_persist_writer_finishing_after_new_epoch_does_not_replace_newer_payload` đỏ (assert byte file bị đè bởi payload cũ) khi tắt cổng commit | xanh |
| M2 (G29) | Sửa (a) | 2 test mới ở `cache/preview_file.rs` (đỏ theo nghĩa không biên dịch: seam kiểm kích thước chưa tồn tại; hành vi cũ `Vec::reserve` không thể test mà không abort) | xanh |
| M3 | Sửa (a) | `folder_estimate_cache_raw_entry_with_absurd_dimension_means_do_not_preload_instead_of_panicking` đỏ (panic `arithmetic operation overflowed` tại `ram_budget.rs:201`) | xanh |
| M4 | Không sửa; đã kiểm chứng bằng thực nghiệm (xem dưới) | | |

## M1: writer persist epoch cũ đè payload mới
- C#: `PreviewImageService.PersistOne` cũng ghi với `CancellationToken.None`, kiểm epoch sau khi ghi xong rồi `DeleteStalePersistFile` (không xoá khi có claim mới hơn). Cùng thứ tự, nên lỗ hổng là hành vi thừa kế; sửa là cải thiện có chủ đích (cần hàng `Different`, xem mục PARITY dưới).
- Sửa: `atomic_file::write_atomic_gated` + `preview_file::write_atomically_gated` nhận `CommitGate`: gọi ngay trước `rename_over`, trả `None` thì huỷ (`Canceled`, temp bị `TempCleanup` xoá), trả `Some(hold)` thì giữ `hold` suốt lúc rename. `persist_one` truyền cổng khoá `owner_epochs`, kiểm `owner_epoch > request.epoch` và `epoch != current_epoch`, giữ khoá qua rename (kiểm và rename nguyên tử so với `claim_persist_path` của worker mới). Đường thường (`write_atomic`, `write_atomically`, mọi chữ ký cũ) không đổi; bước 7 (xoá file stale) giữ nguyên làm lưới an toàn cho epoch bump xảy ra ngay sau cổng.
- Ngoại lệ R0003 (I/O dưới khoá) giống `delete_stale_persist_file`: chỉ hai persist worker lấy khoá này, một lần rename.
- Test dùng seam mới trong `RawPayloadCodec` (đỗ lần encode đầu bằng `Gate`), không sleep.

## M2: `preview_file::read` cấp phát theo kích thước file
- C#: `Read` đọc header từ stream rồi mới thuê `ArrayPool` theo `payloadLength` đã kiểm `<= int.MaxValue`; Rust đảo thứ tự. Sửa đưa Rust về đúng ý C#.
- Sửa: `check_entry_file_len` (file dài hơn header + EXIF tối đa + `i32::MAX` là `InvalidData` "implausibly large", quyết định từ độ dài, trước mọi cấp phát) và `try_entry_buffer` (`try_reserve_exact`, lỗi là `PreviewCacheError::OutOfMemory`, entry được giữ, không abort). Giới hạn vẫn có thể cấp phát tới ~2 GiB cho file hợp lệ tối đa; khi không đủ bộ nhớ trả lỗi thay vì abort. Chưa làm: đọc header trước rồi mới cấp phát theo payload (thiết kế lớn hơn, không cần cho an toàn tiến trình).
- Test chạy trên hàm thuần, không cấp phát lớn thật.

## M3: `estimate_folder_preview_bytes` panic
- C#: `checked(width * 2)` ném `OverflowException`, và golden oracle ghi đúng điều đó. Giữ `estimate_folder_preview_bytes` nguyên (panic, golden không đổi).
- Sửa: lõi trả `Option<i64>`; thêm `estimate_folder_preview_bytes_saturating` (tràn thì `i64::MAX`, nghĩa "không preload cả thư mục", là đáp án đúng cho thư mục có entry vô lý); `FolderEstimateCache::get_or_compute` (đường duy nhất của preload queue) dùng bản này, nên `refresh` không còn thất bại mỗi lượt. Không thêm test mức scheduler (không có seam sẵn; chỉ có `get_or_compute` là điểm gọi).

## M4: RAW full decode và share Delete (INV-8): ĐÃ KIỂM CHỨNG, có vấn đề, chưa sửa
- Thực nghiệm (ngoài repo, thư mục tạm; không ghi gì vào repo): Python `ctypes` nạp `libraw.dll` (bản `C:\MyProjects\PhotoReview\native\x64\libraw.dll`, chỉ đọc/copy), `libraw_init`, `libraw_open_wfile` trên DNG tổng hợp (`tools/dev/make-synthetic-raw.py`, 160x96) trả `0`, rồi `os.rename` file đang mở: **thất bại `WinError 32` (file đang được tiến trình khác dùng)**; sau `libraw_close` thì xoá được. Vậy `libraw_open_wfile` mở file KHÔNG kèm `FILE_SHARE_DELETE` và giữ handle tới khi đóng/recycle.
- Hệ quả: trong lúc RAW full decode đang chạy (đường mặc định theo path, `pr-libraw/src/native.rs:836`), Delete/Move chính file RAW đó có thể thất bại. C# dùng cùng API nên là hành vi thừa kế. Phạm vi thực nghiệm: một DLL, một DNG tổng hợp, chỉ rename (xoá/move cùng cần quyền DELETE nên cùng cơ chế, nhưng chưa thử riêng); chưa chạy qua đường Rust `LibRawDecoder` và chưa thử RAW máy ảnh thật.
- Việc tiếp (thiết kế lớn, hoãn): đọc RAW bằng `open_read_shared` (share Read|Write|Delete) vào bộ nhớ rồi `open_buffer` (vài chục MB), đổi đường mở của `pr-libraw::decoder::decode_core`; cần cân nhắc RAM budget và quy tắc "đọc mỗi file một lần". Cần ADR/hàng `Different`.

## Cho orchestrator (không sửa PARITY ở đây)
- Hàng `INV-2` (persist worker/`DiskCacheStore`): M1 đã đóng bằng cổng commit; ghi `Different` (thứ tự kiểm/rename khác C#), test `stale_persist_writer_finishing_after_new_epoch_does_not_replace_newer_payload`.
- Hàng preview file/`PreviewCacheFile` (G29): ghi cải thiện M2, không đổi hành vi hợp lệ.
- Hàng `RamBudgetPolicy`/preload queue: `Different` cho M3 (C# ném, Rust bão hoà `i64::MAX` ở `FolderEstimateCache`).
- Hàng RAW/LibRaw (`I-15b / INV-8`): ghi M4 đã kiểm chứng (rename bị chặn khi `libraw_open_wfile` giữ file); mức rủi ro: trung bình, hoãn.

## Đã chạy (worktree `fix-imaging-cache`, `CARGO_TARGET_DIR` riêng)
`cargo test -p pr-imaging -j 2 --lib --test ram_budget --test cache_preview_file --test cache_atomic_file --test preview_service_disk`: 154 + 24 + 45 + 53 + 51 xanh; `cargo fmt -p pr-imaging`; `cargo clippy -p pr-imaging --all-targets -- -D warnings` sạch. Không chạy: workspace test/clippy, mutation, native test (CI chạy phần còn lại).
