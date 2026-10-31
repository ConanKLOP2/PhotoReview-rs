# ST-IMAGING-INV 2026-10-31: INV-1/2/8/10/11/12 phía `pr-imaging`, cache đĩa, decoder, FileLog, SettingsStore (tĩnh)

Status: report (lane tĩnh). Kết quả: 0 cao, 4 trung bình (M1, M2 còn từ báo cáo cũ; M3, M4 mới), 6 thấp (L1-L3, L5, L6 mới; L4 còn từ cũ). Không đổi mã, không thêm test, không build/chạy gì.

Phạm vi và trung thực: đọc mã tại `origin/master` `93e44a1d` (worktree `st-imaging-inv`), bổ sung phần [ST-INVMAP-2026-10-31.md](ST-INVMAP-2026-10-31.md) mục 5 ghi "chưa đọc sâu" (`pr-imaging` INV-1/2/8/12, INV-10, INV-11). Đối chiếu [invariants-storage.md](invariants-storage.md) (G26..G29). **Mọi điều dưới đây là suy luận từ mã, chưa chạy** trừ khi ghi khác; không chạy test nào. Ở đâu viết "test có" nghĩa là đã đọc thân test trong nguồn, chưa chạy lại.

## 1. Định nghĩa INV dùng ở đây (nguồn: `docs/reviews/invariants.md:22-33`, `docs/PARITY.md` hàng INV-8/10/11)

| INV | Nội dung |
|---|---|
| 1 | Khoá cache chứa đủ định danh nguồn (path/size/mtime/box/orientation/backend/kind); không bao giờ trình bày pixel của nguồn cũ dưới định danh mới |
| 2 | Clear/evict vô hiệu hoá việc publish/persist đang bay của epoch cũ; store không bao giờ kẹt |
| 8 | Mở ảnh nguồn với share Read\|Write\|Delete + SequentialScan, đóng sớm (không giữ handle qua decode dài) |
| 10 | Log chẩn đoán mặc định tắt; tắt thì không tạo thread/thư mục/file |
| 11 | Config hỏng thì backup rồi reset; config bị khoá thì dùng default trong RAM và không ghi đè |
| 12 | Lỗi backend thì chỉ fallback với loại lỗi trong whitelist (không OOM/cancel/busy); cache không lẫn backend |

## 2. Bản đồ INV tới mã và test thật (file:line tại `93e44a1d`)

| INV | Mã bảo đảm | Test (đã đọc) | Đánh giá |
|---|---|---|---|
| 1 | `crates/pr-imaging/src/cache_key.rs:306` `matches_current_source` (stat sau decode); `service/decode.rs:436` kiểm sau decode/disk hit, trước publish `:448`; `service/decode.rs:156` kiểm lại cho `get_original_dimensions`; `:625` cho `decode_original` | `tests/preview_service_decode.rs:1140`, `:1146` (`source_replaced_during_decode_is_never_cached[_or_persisted_disk]`), `:1709` | Đúng. Giới hạn cố hữu: so size+mtime, ghi lại cùng size/mtime thì không phát hiện (L6 dưới) |
| 2 | `service/mod.rs:808` `clear_cache` (bump+clear dưới `lifecycle_gate`), `:869` `publish_if_current`; `service/disk.rs:330` `clear_disk` (bump trước xoá), `:454` `queue_persist`, `:489` `persist_one` (kiểm epoch `:491`, `:536`); `cache/source_bytes.rs:422,434` `clear`/`evict`, publish dưới `publish_gate` `:577` | `preview_service_decode.rs:3024`; `cache_source_bytes.rs:955,961`; `service/disk.rs:1201` | Phía RAM và source-bytes đúng. Phía ghi file persist còn lỗ hổng (M1) |
| 8 | `embedded_thumbnail.rs:152-153` mở share, frame/stream bị drop trước khi trả; `cache/source_bytes.rs:528-584` mở qua `SourceReader` rồi đóng ở cuối hàm; `pr-imaging-raw/src/decoder.rs:620-660` `read_preview_range` đóng trước khi trả; `pr-turbojpeg/src/decoder.rs:672-684` `open_shared` (SEQUENTIAL_SCAN, share mặc định std = R\|W\|D) | `tests/embedded_thumbnail.rs:555,577` | Đúng cho JPEG/WIC/preview RAW. RAW full decode qua `libraw_open_wfile` không do Rust mở (M4) |
| 10 | `pr-core/src/diagnostics/file_log.rs:500-520` constructor không chạm đĩa; `:553-590` `set_enabled(true)` mới spawn; `:280-282` `write_entry` thoát khi tắt; thư mục/file chỉ tạo ở `:357,366` trong `drain_to_disk` | `pr-core/tests/diagnostics_file_log.rs:304,352` | Đúng. Một lưu ý nhỏ L4 |
| 11 | `pr-core/src/settings/store.rs:296` `load`; backup trước reset `:311-322`; `persist` chặn ghi khi `keep_corrupt_file` `:626-633`; `unique_backup_path` `:771` + `copy_file` không ghi đè (`io/physical.rs:807` `COPY_FILE_FAIL_IF_EXISTS`) | `pr-core/tests/settings_store.rs:609,629,649,665` | Đúng cho nhánh corrupt/locked. Một điểm tựa vào `file_exists` (L2) |
| 12 | `pr-imaging/src/fallback.rs:65` `is_fallbackable` (match đầy đủ, không `_ =>`); `:112-135` decode có relabel backend (`:163`); `pr-imaging/src/pixel_memory.rs:53` OOM => `DecodeError::OutOfMemory` (không fallback) | `tests/fallback.rs:1239,1266` | Đúng. `Io(_)` không fallback (lưu ý L5) |

## 3. Phát hiện

### M1 (trung bình, còn từ G26, mã chưa đổi): writer persist epoch cũ có thể rename đè file của writer epoch mới
- File: `crates/pr-imaging/src/service/disk.rs:489-545` (`persist_one`): ghi `preview_file::write_atomically(.. &CancelToken::none())` ở `:520`, kiểm epoch chỉ sau khi rename xong `:536`; `delete_stale_persist_file` `:573-583` không xoá khi `owner_epoch > epoch`. `cache/atomic_file.rs:141-142` kiểm token trước rename, nhưng token là `none`.
- Kịch bản (giữ nguyên từ invariants-storage M1, tôi đọc lại và thấy đúng với mã hiện tại): A (epoch e) kẹt trong encode; Clear Cache; B (epoch n) cùng đường dẫn K ghi xong; A thoát encode, rename đè B; A thấy stale nhưng không xoá vì n > e. K giữ byte của A. Cùng K nghĩa là cùng định danh nguồn, nên hậu quả thực tế là một entry của epoch cũ còn sống sau Clear Cache; chưa chứng minh pixel khác.
- Test hiện có: `service/disk.rs:1201` / `tests/preview_service_disk.rs:1246` (dừng A SAU write, kiểm tra tồn tại chứ không kiểm byte). Test đề xuất chưa có (đã grep `newer_payload` không có kết quả): `stale_persist_writer_finishing_after_new_epoch_does_not_replace_newer_payload`.
- Đề xuất: truyền token epoch vào `write_atomic` (kiểm lại ngay trước `rename_over`, `atomic_file.rs:142`), hoặc rename dưới `owner_epochs` lock. Đổi thứ tự so với C# cần hàng `Different` trong PARITY.

### M2 (trung bình, còn từ G29, mã chưa đổi): `preview_file::read` cấp phát theo kích thước file trước khi kiểm header
- File: `crates/pr-imaging/src/cache/preview_file.rs:322-328`: `bytes.reserve(meta.len())` (không `try_reserve`) rồi `read_to_end`, chỉ sau đó `parse_entry` (`:180`) kiểm `payload_len > i32::MAX` (`:236`).
- Kịch bản: một `.pv4` hỏng/khổng lồ ở đúng đường hash trong thư mục cache (ví dụ tạo bởi hỏng đĩa hoặc tiến trình khác). `reserve` không trả `Result`: hết bộ nhớ thì cả tiến trình abort thay vì coi là cache miss. Ảnh nguồn không liên quan.
- Test hiện có: `tests/cache_preview_file.rs` có test OOM của codec (`:1781,:1805`) nhưng không có test độ dài khai báo quá lớn / giới hạn đọc. Thiếu.
- Đề xuất: đọc header 24 byte + khối EXIF trước, so `file_len - header` với `i32::MAX` rồi mới cấp phát bằng `try_reserve_exact`; map lỗi cấp phát sang `OutOfMemory` (không coi là entry hỏng).

### M3 (trung bình, MỚI, suy luận): `estimate_folder_preview_bytes` panic với một RAW có kích thước lưu trong catalog quá lớn, làm hỏng cả lượt preload
- File: `crates/pr-imaging/src/ram_budget.rs:196-205` (`checked_mul(2).expect("arithmetic operation overflowed")`, đã ghi "# Panics" ở `:163-165` vì C# ném `OverflowException`). Gọi từ `FolderEstimateCache::get_or_compute` `:290`, từ `preload/queue.rs:233` (`refresh`), bắt bởi `catch` ở `preload/scheduler.rs:307`.
- Kịch bản: thư mục có một file RAW hỏng/độc hại mà WIC/`read_info` báo chiều rộng hoặc cao > 1 073 741 823 (i32::MAX/2) và giá trị đó được lưu vào `CatalogEntry`. Mỗi lần `refresh` dựng lại thứ tự, `estimate` panic, scheduler bắt thành lỗi callback; không có bước nào bỏ qua riêng entry đó, nên preload của cả thư mục lặp lại thất bại (suy luận về vòng lặp, chưa chạy). Dữ liệu người dùng không bị đụng, nhưng tính năng preload chết cho thư mục đó và log đầy lỗi.
- Test hiện có: `tests/ram_budget.rs:1102-1120` chỉ coi panic là "overflow được kỳ vọng" (khớp C#). Không có test ở mức scheduler: "một entry có kích thước phi lý không làm hỏng lượt preload".
- Đề xuất: không dùng `expect`; dùng `saturating_mul` hoặc tính bằng `f64`, hoặc bỏ qua entry (coi như không biết kích thước, rơi về nhánh dùng `length`). Cần hàng `Different` vì C# ném.

### M4 (trung bình, MỚI, chưa kiểm chứng): RAW full decode mở file bằng `libraw_open_wfile` bên trong DLL, INV-8 (share Delete) không được Rust bảo đảm
- File: `crates/pr-libraw/src/native.rs:836-842` (`open_wfile`), gọi từ `pr-libraw/src/decoder.rs:373-378` trong `decode_core`; RAW không bao giờ được pre-read thành byte (`pr-imaging/src/service/decode.rs:488,497`: `!is_raw_source` mới pre-read), nên đường mặc định cho RAW full decode là mở theo path. Nhánh `open_buffer` (`decoder.rs:367`) chỉ dùng khi caller đã có byte.
- Kịch bản: người dùng bấm Xoá/Chuyển ảnh RAW trong lúc viewer đang giải mã đầy đủ (chỉ báo 300 ms, vài trăm ms đến vài giây). Nếu `libraw.dll` mở file không kèm `FILE_SHARE_DELETE` (mặc định của CRT `fopen`/`_wfopen`), thao tác di chuyển/xoá thất bại "đang được dùng". Tôi KHÔNG đọc nguồn `libraw.dll` và không chạy nên không biết chắc cờ share; C# cũng dùng cùng API nên nếu có thì là hành vi thừa kế.
- Test hiện có: không có test nào khẳng định hành vi share của handle LibRaw (PARITY dòng 44 cũng ghi decoder handle thuộc hàng decoder, native test với `libraw.dll` bị skip khi thiếu corpus).
- Đề xuất: test native (chỉ khi có DLL) giữ decode tại progress handler rồi thử `rename` file từ handle khác; nếu thất bại, mở file bằng `open_read_shared` ở Rust, nạp byte (RAW tối đa vài chục MB) và dùng `open_buffer`.

### L1 (thấp, MỚI, suy luận): đọc source-bytes cấp phát zero-fill không có đường lỗi OOM
- File: `crates/pr-imaging/src/cache/source_bytes.rs:550` (`std::iter::repeat_n(0u8, count).collect()`), trong khi `service/decode.rs:556` (`diag_pre_read`) dùng `try_reserve_exact` đúng. `count` bị chặn bởi `can_cache` (`:412`: <= capacity và <= ARRAY_MAX_LENGTH) nên chỉ xảy ra khi áp lực bộ nhớ gần capacity.
- Hậu quả: allocation thất bại gọi `handle_alloc_error` (abort) thay vì trả `SourceBytesError`. Test: không có (cần reader giả báo kích thước lớn hơn bộ nhớ khả dụng, khó; chấp nhận đổi sang `try_reserve_exact` kèm test đơn vị đường lỗi).

### L2 (thấp, MỚI, suy luận): INV-11 phụ thuộc `file_exists`, trả false khi stat thất bại
- File: `crates/pr-core/src/settings/store.rs:302` (`if self.file_system.file_exists(&file_path)`), `crates/pr-core/src/io/physical.rs:1203-1212`, `:658-660` (`any failure is false`, lỗi quyền/sharing của stat cũng thành false).
- Kịch bản: `config.json` có thật nhưng `GetFileAttributes` lỗi tạm (AV, ổ mạng, lỗi quyền chỉ đọc thuộc tính). `load` coi như chưa có config, đi nhánh `AppSettings::new` rồi `persist` (store.rs:327-333, ghi nguyên tử) mà không backup. Nếu rename-đè thành công thì mất cấu hình cũ. Nhánh "khoá khi đọc" (INV-11) có test nhưng nhánh "khoá khi stat" thì không. Hiếm; C# `File.Exists` cùng hành vi.
- Test thiếu: `FakeFileSystem` có `file_exists` false nhưng `read_all_text` thành công và ghi đang được cho phép, kỳ vọng `load` không ghi đè (hoặc ghi sau backup).

### L3 (thấp, MỚI): thư mục cache là thư mục con cố định nhưng `clear`/`prune`/`legacy cleanup` xoá theo pattern `*.pv4` / `*.png` / `*.tmp` không có dấu hiệu nhận diện
- File: `cache/disk_store.rs:323-335` (`clear_directory`), `:454-533` (`prune_directory`), `service/disk.rs:646-679` (`cleanup_legacy_cache_files`: `*.png`, `*.png.meta`, tối đa 5000 file), `cache/thumbnails.rs:84` (`*.png`). Thư mục lấy từ `pr-core/src/app_paths.rs:50-51` (`<user_root>/cache`, `<user_root>/thumbnails`); `user_root` bằng biến môi trường data-root khi isolate (`:44-47`, `resolve_override :71`).
- Kịch bản: đặt biến môi trường data-root trỏ vào thư mục có sẵn một thư mục con `thumbnails` chứa PNG của công cụ khác (hoặc `cache` chứa `*.png`); Clear Cache/prune/start-up cleanup xoá các file này. Cần người dùng tự đặt biến môi trường; không phải đường thường. Mọi xoá đều qua `try_delete` => `PhysicalFileSystem::delete` (DeleteFileW, không vào Recycle Bin), nên không phục hồi được.
- Đề xuất (nhỏ): ghi file đánh dấu `.photoreview-cache` khi tạo thư mục và chỉ xoá/prune khi có dấu, hoặc từ chối thư mục có file lạ. Test: `TempRoot` có `foreign.png` không dấu, `clear_directory` không xoá.

### L4 (thấp, còn từ L1/G28, đã đọc lại): FileLog ghi dở rồi lỗi hai lần có thể lặp dòng ở lượt drain kế
- File: `diagnostics/file_log.rs:403-412` (`offset` cục bộ cho mỗi batch; chỉ đánh dấu `flushed` khi `on_disk`; khi cả hai lần ghi lỗi thì `return Err` mất `offset`). Lượt drain kế dựng lại cả buffer từ đầu. Test `tests/diagnostics_file_log.rs:445` không mô phỏng short write. Không ảnh hưởng journal. Mã giống mô tả invariants-storage L1, chưa đổi.

### L5 (thấp, MỚI, chưa đọc đủ): `DecodeError::Io(_)` không nằm trong whitelist fallback
- File: `pr-imaging/src/fallback.rs:90`. Một JPEG/RAW bị cắt cụt mà decoder báo bằng `Io(UnexpectedEof)` (ví dụ `pr-imaging-raw/src/decoder.rs:653-656` "Truncated embedded preview" là `Io(UnexpectedEof)`) sẽ không thử backend dự phòng, hiện lỗi thay vì ảnh một phần. Đây là bảo thủ có chủ đích (khớp C#?) nhưng tôi chưa kiểm C#; ghi để orchestrator quyết, không coi là vi phạm INV-12. Test có: `tests/fallback.rs:1266` bao quát biến thể được liệt kê.

### L6 (thấp, MỚI, suy luận): khoá dùng đường dẫn IN HOA cho cả định danh lẫn `stat` xác minh INV-1
- File: `cache_key.rs:306-314` (`get_file_stat(Path::new(&self.path))` với `self.path` đã `ToUpperInvariant`, chuẩn hoá ở `:322`), `session/store.rs:361-365` (`to_upper_invariant`, có ngoại lệ U+017F). Thư mục NTFS phân biệt hoa thường (WSL/`fsutil file setCaseSensitiveInfo`) có `a.jpg` và `A.JPG`: hai file khác nhau cùng một khoá path nếu cùng size/mtime (copy giữ mtime) => RAM/disk cache dùng chung pixel. Tên file chứa ký tự mà bảng NTFS `$UpCase` không coi bằng chữ hoa .NET (ví dụ `ſ` -> `S`) thì `matches_current_source` có thể false vĩnh viễn => `SourceChanged`. Cả hai cực hiếm và khớp C#. Test: `FakeFileSystem` với tên `ſ.jpg` (nếu muốn khoá lại), không đề xuất sửa production.

## 4. Các mục người dùng yêu cầu: kết quả kiểm

| Mục | Kết quả |
|---|---|
| Ảnh gốc bị ghi đè/sửa khi decode/cache/preview/export | Không thấy. Toàn bộ ghi trong `pr-imaging`, `pr-imaging-raw`, `pr-libraw`, `pr-turbojpeg` (grep `write(`, `OpenOptions`, `File::create`, `remove_file`, `rename`, `set_len`, `set_times`, `delete(` trong `src` bỏ test): chỉ `cache/atomic_file.rs` (temp `create_new` + `MoveFileExW` vào thư mục cache), `cache/disk_store.rs:887` (`FILE_WRITE_ATTRIBUTES` đặt access time cho file entry cache), `try_delete` trên đường entry cache. Mở nguồn đều read-only (`turbojpeg/decoder.rs:672-684`, `source_bytes.rs`, `pr-imaging-raw/header_source.rs:58-65`). Không có "export" trong các crate này |
| Đường ghi vào thư mục ảnh người dùng ngoài journal | Không có trong phạm vi này. Đường dẫn ghi cache luôn `disk_directory.join(<sha256>.pv4)` (`service/disk.rs:97-117`) hoặc `<dir>\<sha256>.png` (`thumbnails.rs:685`), không ghép chuỗi từ tên file người dùng, không thể thoát thư mục (không có traversal). Xem L3 cho rủi ro thư mục cấu hình |
| Cache `disk.rs` xoá/ghi nhầm ngoài thư mục cache | Chỉ qua cấu hình thư mục (L3). Mọi xoá dùng `try_delete` trên đường dẫn do store tự dựng hoặc liệt kê trong chính thư mục đó (`disk_store.rs:536-549`, `service/disk.rs:652-669`) |
| Race nhiều thread | M1 (hai writer persist). Phần còn lại đã đọc: in-flight `(key, epoch)` dưới khoá, tái kiểm RAM khi đăng ký (`decode.rs:261-268`); `source_bytes` generation + path version dưới `publish_gate`; `thumbnails.rs` generation dưới `lifecycle`; `file_log` có `catch_unwind` quanh hook và drain. Không thấy khoá giữ qua I/O dài ngoài `delete_stale_persist_file` (có chú thích R0003) |
| panic/unwrap với file hỏng (RAW/JPEG) | Đếm `unwrap/expect/panic!/unreachable!` ngoài `#[cfg(test)]` trong `pr-imaging`, `pr-imaging-raw`, `pr-libraw`, `pr-turbojpeg`: 2 chỗ — `ram_budget.rs:201,204` (M3) và `factory.rs:172` (`unreachable!`, bất biến của constructor luôn điền slot Wpf). Quét tự động theo ranh giới `#[cfg(test)]` đầu tiên của từng file; `pr-libraw` không có khối test trong file nên 0 ở đó cũng nghĩa là 0 trong toàn file. TIFF/EXIF (`tiff.rs`, `exif_parser.rs`, `exif_summary.rs:144-171`) dùng `get(..)`/`checked_add` và chỉ mục sau kiểm độ dài. Mọi job decode chạy trong `run_catching` (`decode.rs:183-187`, dùng ở `:335-355`) nên panic thành `PreviewError::Panicked`, không treo `LoadCell` |
| Tràn số w*h*4 | `pixels.rs:195-205` (`width <= i32::MAX/4`, nhân trong i64, không tràn); `pixel_memory.rs:99-120` (`Layout` lỗi/null => `OutOfMemory`, không abort); `pr-turbojpeg/src/decoder.rs:375` `calculate_output_buffer` (bão hòa về `i64::MAX`) + `try_reserve_exact` (`:647`); `pr-platform-win/src/wic/decoder.rs:254-255` `checked_mul`; `preview_file.rs:236` giới hạn payload. Không thấy tràn. Ngoại lệ về cấp phát vô điều kiện: M2, L1 |
| Rò tài nguyên khi decode lỗi | Stream/frame bị drop trước khi trả lỗi (`embedded_thumbnail.rs:150-205`; `source_bytes.rs` stream cục bộ; `InFlightRemoval` `source_bytes.rs:632-646` xoá entry cả khi panic; `TempCleanup` `atomic_file.rs:147-159` xoá temp khi lỗi/panic; `ActiveViewerDecode` `decode.rs:200-212`; `libraw` `Drop` đóng handle). Không thấy rò. Lưu ý nhỏ: `read_and_cache` giữ stream đến sau `cache.set` (`source_bytes.rs:577-584`), share có Delete nên không chặn thao tác file |
| Đọc file đang bị tiến trình khác ghi | Nguồn: so độ dài lúc mở (`source_bytes.rs:540-543`), stat lại sau đọc (`:567-571`) => `file_changed_while_reading`; INV-1 verify sau decode. Không thể loại bỏ trường hợp file pre-allocate đúng kích thước nhưng ghi dở (cố hữu). Cache: ghi qua temp exclusive (`share_mode(0)`, `atomic_file.rs:326`) rồi rename nên người đọc không thấy file dở |

## 5. Test thiếu (tóm tắt, theo thứ tự ưu tiên)
1. M1: `stale_persist_writer_finishing_after_new_epoch_does_not_replace_newer_payload` (payload A/B phân biệt, chặn A trong encode).
2. M2: `preview_file::read` với header khai payload > i32::MAX và file lớn giả (reader/seam), khẳng định không cấp phát toàn file và lỗi `InvalidData`; OOM không bị coi là entry hỏng.
3. M3: test mức scheduler/queue: một entry RAW có `width = i32::MAX` không làm `refresh` lỗi.
4. M4: test native (cần `libraw.dll`) rename/xoá file khi decode đang chạy.
5. L2: `load` khi `file_exists` false nhưng file có thật.
6. L3: `clear_directory`/`prune_directory` bỏ qua file lạ khi không có dấu cache (nếu chấp nhận đề xuất).

## 6. Cho orchestrator (không sửa PARITY ở đây)
- Hàng INV-2 (store `DiskCacheStore`, persist worker, `docs/PARITY.md`): không nên coi hoàn toàn "Done" khi M1 còn mở (G26). Bằng chứng: `service/disk.rs:520,536,573-583`.
- Hàng `I-04 / I-07 / I-08 / I-16 / INV-1 / INV-2 (service)` và `P-08 / INV-10` và `INV-11`: không có phát hiện làm đổi trạng thái; chỉ L2 liên quan INV-11 (hiếm).
- Hàng RAW/LibRaw (`I-15b / INV-8 / INV-12`, PARITY-CLOSEOUT dòng 89): thêm ghi chú M4 chưa kiểm chứng share-Delete của `libraw_open_wfile`.
- Hàng cần `Different` nếu sửa: M1 (thứ tự rename), M2 (kiểm sớm hơn C# là cải thiện, không đổi hành vi hợp lệ), M3 (C# ném `OverflowException`).
