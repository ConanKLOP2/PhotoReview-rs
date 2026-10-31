# FIX-RAW-SHARE-DELETE 2026-10-31: xoá/di chuyển file RAW trong lúc full decode (INV-8, M4 của FIX-IMAGING-CACHE)

Status: report (lane `fix-raw-share-delete`). Quyết định: `P-60` (`docs/PROPOSED-DECISIONS.md`) và `DV-22` (`docs/adr/ADR-D-2026-10-31-raw-share-delete.md`); đây là phương án khuyến nghị của câu hỏi 3 trong `ST-PARITY-SYNC4-2026-10-31.md`, chưa có xác nhận người dùng, đã làm theo.

## Kết quả

| Câu hỏi | Kết quả (đã chạy thật) |
|---|---|
| Rename file RAW trong lúc decode, đường cũ (`libraw_open_wfile`) | Bị chặn: `native_path_open_blocks_rename_while_libraw_decodes` xanh, rename tại stage `opened` trả lỗi OS 32 (`ERROR_SHARING_VIOLATION`) qua đường Rust `LibRawDecoder` thật với `libraw.dll` thật (khác M4 cũ chỉ thử bằng Python ctypes) |
| Rename/xoá khi dùng đọc chia sẻ | Không bị chặn: `native_shared_file_read_lets_the_file_be_renamed_and_deleted_while_libraw_decodes` xanh (rename tại `opened`, xoá tại `unpacked`, decode vẫn trả ảnh 160x96) |
| Pixel so với đường cũ | Giống hệt: `native_shared_file_read_decodes_the_same_pixels_as_the_path_open`, 7 DNG tổng hợp (plain, preview, subifd, swap, orient6, orient3, pair) x 2 hộp (không giới hạn, 64x64) = 14/14 trùng BGRA, kích thước, `downscaled`, `original_*`, orientation |
| Lỗi so với đường cũ | Cùng kind và cùng text cho file rác và file không tồn tại (`native_shared_file_read_reports_the_same_errors_as_the_path_open` xanh) |
| Golden/oracle | Không đổi: `LibRawDecoder::new` vẫn đi đường C#; `decoder_logic` + `decoder_golden` 86 xanh |

## Cài đặt (nhỏ nhất)

- `crates/pr-libraw/src/decoder.rs`: `read_file_shared` (mở bằng `PhysicalFileSystem::open_read_shared` = Read|Write|Delete, đọc theo khối 4 MiB, kiểm huỷ trước mỗi khối, kiểm headroom `2 x length` trước khi cấp phát, `try_reserve_exact`, đóng handle trước khi trả) và `LibRawDecoder::with_shared_file_read` (mặc định tắt). `decode_core` dùng `open_buffer` khi cờ bật và request không có `bytes`. Lỗi đọc ánh xạ về `LIBRAW_IO_ERROR` qua `open_file_with` nên giữ nguyên thử lại đường dẫn dài, văn bản lỗi và phân loại fallback; cấp phát thất bại là `LIBRAW_UNSUFFICIENT_MEMORY`; huỷ thắng mã lỗi (`check_result`).
- `crates/pr-shell/src/composition/{decoders,factories,registry}.rs`: ba chỗ dựng `LibRawDecoder` gọi `.with_shared_file_read()`.
- Không thêm `unsafe` (dùng API an toàn của pr-core). `tools/dev/unsafe-audit.sh`: "no new violations".
- Không dùng lại `SourceBytesCache`/đường pre-read của pr-imaging: C# cố ý không pre-read RAW (tránh đẩy cache), và cache giữ bytes lâu hơn một lần decode. Đọc trong pr-libraw giữ thay đổi trong một crate.

## Đánh giá chi phí

- RAM: kích thước file (25-100 MB) nằm trong bộ nhớ suốt lúc decode, thêm một bản sao tạm lúc `Vec` thành `Arc<[u8]>`; gate chỉ cho một full decode tại một thời điểm. Thăm dò bộ nhớ của `admit_decode` chạy sau khi đọc nên phần buffer đã được trừ khỏi bộ nhớ khả dụng; công thức ước lượng không đổi.
- Đĩa/tốc độ: vẫn một lần đọc tuần tự dùng chia sẻ (quy tắc "đọc một lần" giữ); độ trễ chuyển lên trước `opened` (đọc cả file rồi mới decode) thay vì đọc theo nhu cầu. Chưa đo trên đĩa chậm/mạng, không có số liệu hiệu năng.
- Hash/fingerprint INV-1: decode không băm; khoá cache vẫn lấy từ `stat` trước đó; cuộc đua "file đổi giữa stat và đọc" y như trước.

## Chưa làm / rủi ro còn lại

- Chưa chạy bộ RAW thật của máy ảnh (corpus `PHOTOREVIEW_RAW_CORPUS_DIR`); `libraw_open_buffer` có thể khác `open_wfile` với định dạng hãng cần file phụ. Việc tiếp: chạy `decoder_native` có cờ bật trên máy có corpus.
- `read_info` và `read_jpeg_thumbnail` vẫn dùng `libraw_open_wfile` (giữ file rất ngắn, không phải trong lúc decode); chuyển sẽ đọc cả file chỉ để lấy vài KB nên không làm.
- File lớn hơn nửa bộ nhớ trống bị từ chối rõ ràng ("Not enough memory"), trước đây có thể decode streaming.
- Hoàn tác: bỏ ba lời gọi `.with_shared_file_read()`, hoặc revert PR.

## Cho orchestrator (không sửa PARITY ở đây)

- Hàng RAW/LibRaw (`I-15b / INV-8`) và `LibRawDecoder`: thêm `Different` DV-22, test `crates/pr-libraw/tests/raw_share_delete.rs`; KHÔNG nâng Done vì chưa chạy corpus thật.
- Nên thêm 4 test `#[ignore]` native của `raw_share_delete` vào danh sách nightly nếu có quy ước (cần `libraw.dll` + python; không tự thêm vì brief không yêu cầu).

## Đã chạy (worktree `fix-raw-share-delete`, `CARGO_TARGET_DIR` riêng, `libraw.dll` chép tạm cạnh binary test rồi đã xoá)

- `cargo test -p pr-libraw --test raw_share_delete`: 11 xanh, 4 ignored; `-- --ignored --test-threads=1` với `PHOTOREVIEW_RS_SYNTHETIC_STRICT=1` (không cho skip): 4/4 xanh.
- `cargo test -p pr-libraw --test decoder_logic --test decoder_golden`: 86 xanh.
- `cargo fmt -p pr-libraw -p pr-shell --check`; `cargo clippy -p pr-libraw --all-targets -- -D warnings` và `-p pr-shell` sạch.
- Không chạy: workspace test/clippy, mutation, `decoder_native` với corpus (CI/máy có corpus).
