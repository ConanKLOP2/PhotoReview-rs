# ST-FFI-LIBS 2026-10-31: ranh giới FFI của `pr-libraw`, `pr-turbojpeg` và các chỗ gọi DLL/OS từ `pr-imaging` (tĩnh)

Status: report (lane tĩnh). Kết quả: 0 cao, 4 trung bình (M1-M4), 7 thấp (L1-L7). Không đổi mã, không thêm test, không build, không chạy gì.

Phạm vi và trung thực: đọc toàn văn `crates/pr-libraw/src/{native,availability,decoder,memory_guard}.rs`, phần liên quan của `resampler.rs` (`validate_source_length`, `validate_target_length`) và `full_decode_gate.rs`; toàn văn `crates/pr-turbojpeg/src/native.rs`, `availability.rs` và phần đọc/giải mã của `decoder.rs`; mọi `unsafe` của `crates/pr-imaging/src` (`pixels.rs`, `pixel_memory.rs`, `cache/atomic_file.rs`); chỗ lắp ráp ở `crates/pr-shell/src/composition/{decoders,factories,registry}.rs`; C# tại `7eaee2a2` (`LibRawNativeMethods.cs`, `Native/TurboJpegNative.cs`, `AssemblyInfo.cs` của TurboJpeg); danh sách tên test trong `pr-libraw/tests/native.rs` và `pr-turbojpeg/tests/native.rs`. Worktree `st-ffi-libs` từ `origin/master` `a61cb3aa`. **Mọi điều dưới đây là suy luận từ mã (và từ hiểu biết về API LibRaw/libjpeg-turbo), chưa chạy** trừ khi ghi khác. Tôi KHÔNG đọc mã nguồn LibRaw/libjpeg-turbo và KHÔNG có DLL; những chỗ phụ thuộc hành vi nội bộ của DLL được ghi rõ "suy luận về DLL". Không chạy cargo, không chạy test native. Báo cáo [ST-IMAGING-INV-2026-10-31.md](ST-IMAGING-INV-2026-10-31.md) đã nêu M4 của nó (LibRaw mở file bằng `libraw_open_wfile` nên share-mode do DLL quyết định); không lặp lại ở đây.

## 1. Tóm tắt các câu hỏi của lane

| Câu hỏi | Kết luận (suy luận từ mã) |
|---|---|
| SAFETY comment đúng không | Phần lớn đúng và cụ thể (xem mục 3). Có 3 chỗ comment nói quá hoặc lỗi thời: `pr-libraw/src/native.rs:10-11`, `:640-642`, `:1050-1052` (mục 3.2). |
| Kích thước/alignment buffer | `pr-turbojpeg`: `decompress8` tự parse lại header của đúng slice rồi kiểm `pitch >= w*bpp` và `dst.len() >= pitch*h` bằng i64 bão hoà trước khi gọi (`native.rs:768-862`); đúng và còn chặt hơn C#. `pr-libraw`: Rust không cấp buffer cho LibRaw (LibRaw tự cấp `libraw_processed_image_t`), Rust chỉ đọc `data_size` byte; độ dài bị chặn bởi `validate_source_length` (`resampler.rs:270-280`). Không có đòi hỏi alignment nào vượt `u8`; header đọc bằng `read_unaligned` (`native.rs:819`). |
| Thời gian sống con trỏ | `libraw_open_buffer` giữ con trỏ vào buffer: thể hiện bằng `'buf` (`native.rs:846`) hoặc `Arc<[u8]>` giữ trong handle đến sau `libraw_close` nhờ thứ tự field (`native.rs:734-743`). Token huỷ boxed, giải phóng sau `libraw_close` (`:968-979`). Đúng. |
| Double-free / use-after-close | `NativeOwner::close` idempotent, đặt `closed` trước khi release, `release` là `FnOnce` bị `take()` (`native.rs:334-343`). `TurboJpegHandle::close` đặt `closed` trước khi release (`turbojpeg native.rs:649-671`). Mọi phương thức kiểm `closed`. `LibRawImage` độc lập với handle (malloc riêng, `libraw_dcraw_clear_mem`). Đúng. Ngoại lệ về tính vững của API: M4. |
| Đa luồng (`libraw_data_t` dùng chung giữa thread?) | Không. Mỗi lần giải mã `NativeLibRaw::init` tạo handle riêng (`decoder.rs:362`, `:600`); `LibRawHandle` có `NativeOwner` chứa `Box<dyn FnOnce + Send>` nên `Send` và không `Sync` (suy ra tự động, không có `unsafe impl`). Chỉ `LibraryHandle` có `unsafe impl Send/Sync` (`native.rs:545-548`), hợp lý (HMODULE). Thumbnail không qua gate (`decoder.rs:589-594`) nên chạy song song với full decode trên handle khác; an toàn nếu LibRaw không có trạng thái toàn cục (suy luận về DLL). |
| Mã lỗi trả về | Mọi lời gọi trả `c_int` đều qua `check_result`/`create_failure` (`decoder.rs:370,381,384,392`); `libraw_init` null, `make_mem_*` null, `tj3Init` null, `tj3DecompressHeader`/`tj3Decompress8` khác 0 đều được xử lý. `tj3Set(ScanLimit)` bỏ qua kết quả có chủ đích (`turbojpeg decoder.rs:1218`). `tj3SetScalingFactor != 0` quay về ONE (`turbojpeg decoder.rs:1099-1102`). `try_set_use_camera_wb` bỏ qua kết quả có chủ đích (`decoder.rs:476`). Đúng. |
| Tràn số khi cấp buffer | Không thấy tràn. `calculate_output_buffer` (`turbojpeg decoder.rs:375-406`) dùng i64 bão hoà rồi `i32::try_from`; `PixelBuffer::allocate` kiểm `width <= i32::MAX/4`; `validate_source_length` nhân i64 (u16*u16*3 không tràn); `estimate_peak_bytes` dùng u128. |
| Dữ liệu RAW/JPEG độc hại | JPEG: phía Rust an toàn (mọi chỉ số dùng `get`, vòng lặp có tiến triển, tiền kiểm kích thước, `ScanLimit=500`), rủi ro còn lại nằm trong libjpeg-turbo (thời gian CPU). RAW: toàn bộ parser nằm trong `libraw.dll` chạy cùng tiến trình; Rust chỉ tiền kiểm kích thước sau khi mở (M2, M3). |
| DLL load path | TurboJPEG tốt (`LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | DEFAULT_DIRS`, đường tuyệt đối từ `current_exe()`). LibRaw yếu hơn (M1). Tên ngắn không bao giờ được truyền cho `LoadLibraryExW` ở đường chạy thật (`library_path_next_to` trả `None` thay vì quay về tên trần, `libraw native.rs:288-294`). |
| Version DLL không khớp ABI | LibRaw: probe kiểm đủ 23 export, version `0.22.x`, init/close; ghi thô vào `libraw_data_t` chỉ khi đúng `0.22.2` và qua phép thử sentinel (`native.rs:182-209`). TurboJPEG: không có kiểm phiên bản nào (L2). Có khe giữa probe và lần nạp thứ hai (M3). |
| Chuỗi wchar NUL | TurboJPEG chặn NUL (`turbojpeg native.rs:369-375`). LibRaw không chặn ở `load_from` và `open_wfile` (L1). |
| Panic xuyên FFI | Callback duy nhất `cancellation_progress_callback` (`libraw native.rs:270-283`) chỉ đọc một `AtomicBool` và kiểm null; không thể panic; nếu panic thì `extern "C"` abort chứ không unwind vào C. Không có callback nào khác. |
| Rò rỉ khi lỗi giữa chừng | Không thấy. `LibRawHandle` và `LibRawImage` giải phóng bằng Drop; panic giữa `decode_core` vẫn unwind qua Drop (profile release không đặt `panic = "abort"`, `Cargo.toml:45-48`). `compress8` giải phóng `tj3Free` trên mọi đường (nếu export thiếu thì rò rỉ có chủ đích, `turbojpeg native.rs:954-962`). |

## 2. Phát hiện

### M1 (trung bình): LibRaw nạp DLL bằng `LOAD_WITH_ALTERED_SEARCH_PATH`, yếu hơn TurboJPEG và yếu hơn lời hứa trong comment
- File: `crates/pr-libraw/src/native.rs:72-74` (hằng), `:409-417` (`load_from`), `:399-403`; so với `crates/pr-turbojpeg/src/native.rs:299-304` (`DLL_LOAD_DIR | DEFAULT_DIRS`).
- Kịch bản: comment `native.rs:72-73` nói cờ `0x8` là "what .NET does for `DllImportSearchPath.AssemblyDirectory`" và `:5-6` nói "never from the working directory or PATH". Với đường tuyệt đối, cờ này làm DLL chính được nạp đúng chỗ, nhưng các DLL PHỤ THUỘC của `libraw.dll` (runtime VC, nếu bản build có OpenMP thì cả runtime OpenMP; tôi không biết bản build 0.22.2 phụ thuộc gì) được tìm theo thứ tự tìm kiếm chuẩn bắt đầu từ thư mục của DLL, rồi System32, Windows, thư mục làm việc hiện tại, rồi PATH. Nếu một DLL phụ thuộc không có ở thư mục app/System32 (máy thiếu VC++ redistributable là trường hợp thật), kẻ có quyền ghi vào thư mục làm việc hoặc một mục PATH đặt DLL cùng tên sẽ được nạp. Với `LOAD_LIBRARY_SEARCH_*` của TurboJPEG thì thư mục làm việc và PATH bị loại. C# dùng `DefaultDllImportSearchPaths(AssemblyDirectory)` (`LibRawNativeMethods.cs:5`, `Native/TurboJpegNative.cs:5`) cho cả hai, tức là hai crate Rust không đồng nhất nhau và LibRaw lệch khỏi C# (suy luận về cách .NET dịch cờ, chưa chạy).
- Mức khai thác: cần quyền ghi vào CWD/PATH của người dùng và máy thiếu DLL phụ thuộc; vì vậy trung bình, không cao.
- Test hiện có: `pr-libraw/tests/native.rs:123-132` khẳng định `LOAD_WITH_ALTERED_SEARCH_PATH == 0x8` (khoá luôn giá trị yếu) và rằng đường tương đối bị từ chối với lỗi 87. Không có test nào cho tập cờ an toàn như `pr-turbojpeg/src/native.rs:1003-1009` (test `load_flags_...` khẳng định `0x1100`).
- Đề xuất: đổi `load_from` sang `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS` (0x1100) như TurboJPEG, đổi test hằng, thêm test cùng dạng `load_flags_restrict_...`. Sửa comment `:72-73`. Native-test: nạp `libraw.dll` đã pin với cờ mới (cần máy có DLL).

### M2 (trung bình): file RAW độc hại có thể treo một lần giải mã LibRaw không huỷ được, và gate chỉ có MỘT chỗ
- File: `crates/pr-libraw/src/full_decode_gate.rs:205-215` (`state.busy` một slot; người đợi chỉ kiểm `cancel` mỗi 10 ms `:41`, không có timeout), `decoder.rs:362-388` (các lệnh `open_*`, `adjust_sizes_info_only`, `make_mem_image` không có điểm huỷ; callback chỉ chạy trong `unpack`/`dcraw_process` do LibRaw gọi), `native.rs:968-979`.
- Kịch bản: một file RAW (hoặc file có đuôi RAW) khiến `libraw_open_*`/identify hoặc một decoder nội bộ lặp rất lâu hoặc vô hạn (parser C++ lịch sử có lỗi kiểu này; suy luận về DLL). Luồng giải mã giữ `FullDecodeLease` mãi, token huỷ không có tác dụng vì LibRaw không gọi callback ở pha mở. Mọi lần giải mã RAW đầy đủ sau đó xếp hàng sau slot đó; preload bị `Busy`, viewer chờ vô hạn (chỉ thoát được nếu người dùng huỷ chính request của mình). Thumbnail ORF (`read_jpeg_thumbnail`) không qua gate nên vẫn chạy, nhưng chính nó cũng có thể treo một luồng.
- Phụ: ở đường thumbnail, `MAX_THUMBNAIL_BYTES` = 32 MiB (`decoder.rs:44`, `:637-647`) chỉ kiểm SAU khi LibRaw đã cấp bộ nhớ trong `unpack_thumb` + `make_mem_thumb`; kích thước đó do header file quyết định. Đường thumbnail cũng không qua `memory_guard` (chỉ `decode_core` gọi `admit_decode`, `decoder.rs:380`). Giới hạn bộ nhớ ở đó là của LibRaw (suy luận về DLL: LibRaw có `max_raw_memory_mb` mặc định, không đặt trong Rust).
- Test hiện có: gate được test kỹ về logic (`tests/full_decode_gate.rs`, kể cả huỷ khi đợi); không có test nào cho "decode treo" vì cần DLL và corpus (PARITY dòng 138-139: corpus RAW chưa có, các ca corpus return sớm).
- Đề xuất: (a) watchdog theo thời gian cho lease (ví dụ sau N giây ghi log và đánh dấu "wedged", không cấp thêm slot mới nhưng cho phép preload trả `Busy` thay vì treo); chấp nhận rằng không thể giết luồng trong LibRaw, nên cân nhắc đưa LibRaw sang tiến trình con (M3); (b) đặt giới hạn LibRaw nếu API có (`max_raw_memory_mb`) qua một setter đã resolve; (c) test native bằng file RAW cắt/hỏng khi có corpus, ít nhất khẳng định thời gian trần. Đây là quyết định kiến trúc, để orchestrator chọn.

### M3 (trung bình): parser RAW chạy trong tiến trình; DLL được nạp hai lần độc lập và không kiểm lại khi giải mã
- File: `crates/pr-libraw/src/availability.rs:80-92` (`probe()` nạp rồi giải phóng `LibraryHandle`), `native.rs:698-704` (`api()` nạp lại, không kiểm version/export riêng), `registry.rs:671` (chỉ `probe().available` quyết định đăng ký), `factories.rs:128` (cùng cổng cho thumbnail); `crates/pr-turbojpeg/src/availability.rs:66-77` so với `native.rs:485-494` (`shared()` nạp lần thứ hai).
- Kịch bản 1 (cửa sổ TOCTOU, cần quyền ghi vào thư mục app): probe duyệt `libraw.dll` hợp lệ 0.22.x rồi giải phóng; một tiến trình khác thay file; `api()` nạp bản mới không qua probe. Xác thực mật mã chỉ có ở khâu đóng gói (`native/libraw.sha256`, `native/README.md`: "Stage-local native verification"), không có lúc chạy. Kẻ có quyền ghi vào thư mục app đã chiếm được quyền chạy mã sẵn (thay chính exe), nên đây là rủi ro thấp trong mô hình đe doạ thông thường; ghi lại vì hai crate nạp hai lần thay vì giữ handle đã probe.
- Kịch bản 2 (quan trọng hơn): toàn bộ phân tích file RAW không tin cậy chạy trong DLL C++ cùng tiến trình với đầy đủ quyền người dùng. Một lỗi hỏng bộ nhớ trong LibRaw (nhóm lỗi thường gặp ở decoder RAW; suy luận về DLL, không có CVE cụ thể nào được kiểm ở đây) làm sập cả ứng dụng, và về nguyên tắc có thể bị khai thác. Phía Rust không có cách bảo vệ nào (không tiến trình con, không job object, không CFG/ACG bật riêng cho DLL).
- Test hiện có: `tests/availability.rs` (logic probe), `tests/native.rs` 38 test (PARITY dòng 138 ghi chạy xanh với DLL 0.22.2 ngày 2026-10-29; 7 ca cần corpus return sớm). Không có test/fuzz với file hỏng ở mức DLL: grep `proptest|quickcheck|cargo-fuzz|arbitrary` trong hai crate chỉ ra không có.
- Đề xuất: (a) giữ `LibraryHandle` đã probe và dùng lại làm `api()` (một lần nạp, một lần kiểm); hoặc kiểm lại `version_string()` và export trong `api()`; (b) về dài hạn đánh giá chạy giải mã RAW trong tiến trình phụ có giới hạn (job object: giới hạn bộ nhớ/thời gian), gắn với M2; (c) harness fuzz/mutate-file (cắt, lật byte header) chạy tay với DLL và corpus khi có, khẳng định không crash và có thời gian trần; (d) giữ bản LibRaw luôn vá (theo dõi thông báo bảo mật của LibRaw khi bump pin).

### M4 (trung bình, tính vững của API; không khai thác được từ dữ liệu ngoài): hàm `safe` nhận số nguyên tuỳ ý rồi dùng làm con trỏ native
- File: `crates/pr-turbojpeg/src/native.rs:543-550` (`wrap_init_result(ptr: isize)`), `:603-612` (`TurboJpegHandle::from_raw(raw: isize, owns: bool)`), `:616-624` (`with_destroy`); gọi tới `tj3Destroy`/`tj3Get`/`tj3DecompressHeader`/`tj3Decompress8` với `self.ptr()` (`:669`, `:715`, `:726`, `:737`, `:753`, `:796`, `:851`).
- Kịch bản: mã an toàn có thể viết `TurboJpegHandle::from_raw(0x1234, true).close()` hoặc `.get(..)`; kết quả là giải phóng/dereference địa chỉ tuỳ ý trong DLL: hành vi không xác định từ mã không-`unsafe`. Tài liệu chỉ ghi "fake pointers" cho test (`set_handle_as_invalid` dùng cho test). Hiện chỉ test dùng các hàm này, và composition không gọi (grep `from_raw|wrap_init_result` ngoài `native.rs`/`tests` không thấy cho `pr-turbojpeg`). Nên rủi ro hiện tại bằng không, nhưng quy ước Rust coi đây là API không vững.
- Cùng loại, nhẹ hơn, ở `pr-libraw`: `LibRawHandle::try_set_use_camera_wb_with(.., version_pinned, layout)` (`native.rs:1009-1018`) cùng `write_i32` (`:1056-1063`) cho phép ghi một `i32` vào bất kỳ offset trong 64 KiB đầu của `libraw_data_t` (cửa sổ này chứa các con trỏ nội bộ như `image` ở đầu struct, suy luận về bố cục LibRaw); `raw_structure_with(version_pinned)` (`:949-963`) bỏ được cổng version bằng closure `|| true`; `LibraryHandle::load_from` (`:409`) chạy `DllMain` của DLL tuỳ ý. Các hàm gọi hàm C có tiền điều kiện (`call_version`, `call_init_close`, `resolve_export`) đã đúng là `unsafe fn` (`:461`, `:478`, `:679`).
- Test hiện có: nhiều test dùng các điểm vào này với con trỏ giả (`pr-turbojpeg/tests/native.rs:122-193`), chúng cần mở nên đề xuất dưới đây phải giữ khả năng test.
- Đề xuất: đổi `from_raw`/`wrap_init_result`/`with_destroy` thành `pub unsafe fn` hoặc `#[doc(hidden)] pub(crate)` + feature `test-support`; với libraw, đưa `try_set_use_camera_wb_with`, `write_i32`, `raw_structure_with` thành `pub(crate)` hoặc thêm `# Safety`. Thay đổi cơ học nhưng chạm nhiều test, nên cần một lượt build/test đầy đủ.

### L1 (thấp): `pr-libraw` không chặn NUL trong đường dẫn nạp DLL và đường dẫn mở file, `pr-turbojpeg` thì có
- File: `crates/pr-libraw/src/native.rs:514-525` (`wide_nul`), `:409-417`, `:836-843`; đối chiếu `crates/pr-turbojpeg/src/native.rs:368-376`.
- Kịch bản: đường dẫn có NUL bị cắt tại NUL khi LibRaw/loader đọc; ở `load_from` có thể nạp file khác với file mà cờ kiểm tra thấy. `current_exe()` không chứa NUL và `open_wfile` nhận đường dẫn file ảnh (Windows không cho NUL trong tên file), nên chỉ đạt được qua API tổng quát. Comment `:834-835` nói rõ "an interior NUL ends the path" nên đây là hành vi biết trước.
- Test: `tests/native.rs:123-132` kiểm đường tương đối. Không có test NUL cho LibRaw (TurboJPEG: kiểm chặn NUL, có `invalid_argument` ở `:371`).
- Đề xuất: trả `LoadFailed`/`-1` khi có NUL, thêm test thuần (không cần DLL).

### L2 (thấp): TurboJPEG không kiểm phiên bản DLL; probe chỉ chứng minh `tj3Init` chạy
- File: `crates/pr-turbojpeg/src/availability.rs:66-77`; `native.rs:276-278` (export thiếu là `None`, lỗi chỉ ở lần gọi đầu, "trap T4"), `:414-425`.
- Kịch bản: DLL 2.x thiếu `tj3Init` bị phát hiện ngay; một DLL có `tj3Init` nhưng thiếu `tj3Decompress8`/`tj3SetScalingFactor` (build tuỳ biến) được probe báo "available", rồi mỗi lần giải mã trả `EntryPointNotFound` (có fallback WIC nên chỉ chậm). Một bản 3.x khác nhưng cùng ABI (số `TJPARAM`) thì chạy; libjpeg-turbo không có hàm lấy version để so, nên cần dựa vào hash pin lúc đóng gói. Tập `TJPARAM` hard-code theo 3.0 (`native.rs:59-83`): nếu bản sau đổi số thứ tự, `tj3Set(ScanLimit=13)` có thể thành tham số khác (suy luận, các bản 3.0/3.1 giữ số).
- Test: `native.rs` test "load of a dll without tj3 exports succeeds and each call reports entry point not found" (`tests/native.rs:531`); không có test probe-báo-không-sẵn-sàng khi thiếu `tj3Decompress8`.
- Đề xuất: probe resolve đủ các export dùng trong đường giải mã (`decompress_header`, `get`, `set`, `set_scaling_factor`, `decompress8`, `get_error_str`, `destroy`) và trả không sẵn sàng nếu thiếu; thêm test cạnh `probe_with`.

### L3 (thấp): `Arc::from(bytes)` khi lỗi có thể fallback sao chép toàn bộ file và dùng cấp phát không thể thất bại
- File: `crates/pr-turbojpeg/src/decoder.rs:1271`.
- Kịch bản: `decode` đọc file vào `Vec<u8>` (có `try_reserve_exact`, `:643-652`), khi `decode_bytes` trả lỗi fallbackable (gồm `ICC_FALLBACK`: JPEG có hồ sơ ICC là trường hợp phổ biến) thì `Arc::from(Vec<u8>)` cấp một khối mới và sao chép; peak gấp đôi kích thước file, và cấp phát này không dùng `try_reserve`, nên khi thiếu bộ nhớ sẽ abort tiến trình thay vì lỗi. PARITY dòng 137 ghi D-4 "copied once" là chủ ý, nhưng không nêu hệ quả bộ nhớ. File lớn gần `MAX_SOURCE_BYTES` (~2 GiB) hiếm.
- Test: có test D-4 (`tests/decoder_pure.rs`), không có test bộ nhớ.
- Đề xuất: giữ nguồn là `Arc<[u8]>` ngay từ `read_all_bytes` khi có thể (đọc thẳng vào `Arc`), hoặc cấp `Arc<[u8]>` bằng `try_reserve`. Mức thấp.

### L4 (thấp): ảnh lớn libjpeg-turbo tốn CPU không huỷ được
- File: `crates/pr-turbojpeg/src/decoder.rs:1139-1170` (một lời gọi `decompress8` đồng bộ, không có token huỷ), `:1214-1218` (`ScanLimit = 500`); `output_fits` ở `:1134`.
- Kịch bản: JPEG progressive hợp lệ nhưng 500 scan, kích thước ~20000x20000 (≤ i32 sau chia tỉ lệ; `output_fits` chặn chỉ theo bộ nhớ, `:416-422`) giải mã nhiều giây đến vài chục giây trên một luồng nền, không dừng được. Không phải lỗi bộ nhớ; ảnh hưởng độ phản hồi và làm hao luồng.
- Test: `ffi_wrapper_never_panics_on_mutated_jpegs` (`tests/native.rs:727`) kiểm không panic, không kiểm thời gian.
- Đề xuất: giới hạn tổng số điểm ảnh nguồn (ví dụ trần theo cài đặt), hoặc chấp nhận và ghi vào tài liệu.

### L5 (thấp): `ProcessedImageHeader` dùng `u16` cho `width`/`height`, `data_size` do DLL tính
- File: `crates/pr-libraw/src/native.rs:564-573`, `:1089-1099`, `decoder.rs:405-415`.
- Kịch bản: nếu LibRaw cắt chiều rộng/cao vượt 65535 về `ushort` (suy luận về `dcraw_make_mem_image`, chưa xác nhận) trong khi `data_size` tính theo giá trị đầy đủ, `validate_source_length` chỉ yêu cầu `data_size >= w*h*c` nên vẫn qua và ảnh ra sai hình học; không mất an toàn bộ nhớ vì `image.data().get(..rgb_length)` chặn `rgb_length <= data_size`. Ảnh RAW rộng hơn 65535 px không có trên thực tế hiện nay.
- Đề xuất (tuỳ chọn): so khớp `data_size == w*h*c` chặt (hiện là `>=`) để lộ sai lệch. Test cạnh `validate_source_length` goldens (phải giữ golden C#; có thể chỉ kiểm thêm ở `decode_core`).

### L6 (thấp): phép thử bố cục `libraw_data_t` không xác minh chính trường `use_camera_wb`
- File: `crates/pr-libraw/src/native.rs:182-209`, `:105-112`.
- Kịch bản: sentinel kiểm `output_color`, `output_bps`, `no_auto_bright` đúng offset; sau đó ghi `1` vào `use_camera_wb` (offset 5384) và đọc lại chính nó (luôn khớp). Nếu bản DLL "cùng chuỗi version 0.22.2" nhưng chèn/đổi trường giữa +152 và +160 (khác cờ biên dịch), ba kiểm tra đầu vẫn có thể khớp trong khi +152 là trường khác. Cổng version là chuỗi do DLL tự khai (`availability.rs:311-314`), không phải hash. Rủi ro thực tế rất nhỏ vì DLL được pin hash lúc đóng gói.
- Test: có test layout đúng/sai (`tests/native.rs:362-445`, native `:564-632`), không có test cho "trường giữa các offset đã kiểm".
- Đề xuất: ghi chú giới hạn vào comment; tuỳ chọn thêm đọc `use_camera_wb` qua sentinel phụ nếu LibRaw có setter (hiện chưa resolve setter này).

### L7 (thấp, chính sách): `unsafe` ngoài ba crate được phép nằm trong `pr-imaging`
- File: `crates/pr-imaging/src/pixel_memory.rs:115,182`, `pixels.rs:174,185`, `cache/atomic_file.rs:320-341`.
- Kịch bản: `tools/dev/unsafe-audit.py:14` chỉ cho phép `pr-platform-win,pr-turbojpeg,pr-libraw`; quy tắc (b) của nó sẽ gắn cờ các dòng này nếu chưa có trong baseline (tôi không đọc file baseline). Nội dung các khối `unsafe` đúng: `alloc_zeroed` với layout hợp lệ, `dealloc` đúng layout đúng một lần nhờ `AtomicPtr::swap` (`pixel_memory.rs:166-183`), slice dựng từ con trỏ sống trong `&`/`&mut` của `PixelBuffer` (`pixels.rs:166-186`), `MoveFileExW` nhận chuỗi UTF-16 có NUL cuối và đã chặn NUL giữa chuỗi (`atomic_file.rs:324-341`). Cửa sổ nhỏ: `NativePixelMemory::dispose(&self)` là hàm an toàn công khai giải phóng khối, trong khi `as_mut_ptr(&self)` trả con trỏ thô; chỉ `PixelBuffer` (field `memory` private) dựng slice nên an toàn miễn `memory` giữ private.
- Đề xuất: ghi nhận ngoại lệ trong `AGENTS.md`/baseline hoặc chuyển ba khối vào crate được phép; không có lỗi vững bộ nhớ.

## 3. Đánh giá SAFETY comment

### 3.1 Đúng và đủ cụ thể (đã đối chiếu với mã xung quanh)
- `pr-libraw/src/native.rs:226-229, 240-243, 254-256` (`libraw_strerror`/`libraw_version` trả chuỗi tĩnh hoặc null; `c_string` kiểm null).
- `:279-281` (callback: `data` là hộp token của chính handle, sống đến sau `libraw_close`; điều này được bảo đảm bởi thứ tự field `owner` rồi `cancel`, `:734-743`, và `close()` xoá sau `owner.close()` `:1022-1027`).
- `:428-435, 499-500, 531-535` (`LoadLibraryExW`/`GetProcAddress`/`FreeLibrary`: chuỗi UTF-16 có NUL sống suốt lời gọi, HMODULE giải phóng đúng một lần).
- `:749-757, 791-792, 798-799, 812-826, 841-842, 854-856, 933-938, 954-962, 974-978` (mỗi lời gọi có handle sống, con trỏ out hợp lệ; `open_raw_buffer` đúng vì buffer sống lâu hơn `libraw_close`).
- `:818-825` và `:1094-1098` (khối `libraw_processed_image_t` do `malloc` của DLL, header 16 byte, `data_size` byte tiếp theo; giải phóng chỉ qua `libraw_dcraw_clear_mem` nên không lẫn CRT).
- `:959-961` (`libraw_get_iparams` trả `&imgdata.idata`; đọc 425 byte nằm trong struct lớn hơn).
- `pr-turbojpeg/src/native.rs:379-381, 396-413, 438-440, 519-522, 666-669, 711-715, 724-726, 735-737, 750-753, 794-809, 844-860, 872-881, 923-939, 950-959`. Đặc biệt `:844-860` lập luận đúng về `pitch * height` bao phủ vùng `tj3Decompress8` ghi và về nhánh lossless bị bỏ qua tỉ lệ (`:810-817`); `:878-880` sao chép chuỗi lỗi ngay trước mọi lời gọi tiếp theo.
- `pr-imaging`: `pixel_memory.rs:114-115, 178-182`, `pixels.rs:166-186`, `atomic_file.rs:339-340`.

### 3.2 Chưa chính xác hoặc lỗi thời
- `pr-libraw/src/native.rs:10-11`: nói "the W5-15b probe adapter and, unless the lead removes it, W5-16c's progress callback are the only other `unsafe` of `pr-libraw`". Thực tế callback đã nằm trong chính `native.rs` (`:270-283`) và `decoder.rs:14-16` ghi "review decision: one callback, one owner"; chỉ `availability.rs` còn hai `unsafe` (`:209`, `:218`). Sửa câu cho khớp.
- `native.rs:640-642`: SAFETY của `resolve_all!` nói "the library is a LibRaw build (`api` loads only `libraw.dll` from the app folder; W5-15b rejects anything else first)". `api()` (`:698-704`) không gọi probe và không kiểm gì; chỉ composition (`registry.rs:671`, `factories.rs:128`) làm việc đó. Lập luận đúng ở mức hệ thống nhưng sai ở mức hàm; liên quan M3.
- `native.rs:1050-1053` (`read_i32`) và `:1060`: lý lẽ "`libraw_data_t` lớn hơn 65 536 byte" đúng cho 0.22.x, nhưng hàm không có cổng version riêng (cổng nằm ở `try_set_use_camera_wb_core`) và là `pub` qua `read_data_i32` (`:830`). Với DLL khác bố cục, đọc 4 byte trong cửa sổ vẫn nằm trong khối (struct luôn lớn), nên không gây UB nhưng comment nên nói điều kiện là "mọi bản LibRaw ≥ 0.17 có `color.curve[0x10000]` (suy luận)".

## 4. So sánh với P/Invoke C# (`7eaee2a2`)

| Điểm | C# | Rust | Nhận xét |
|---|---|---|---|
| Đường tìm DLL | `[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.AssemblyDirectory)]` cho cả LibRaw (`LibRawNativeMethods.cs:5`) và TurboJpeg (`AssemblyInfo`/`TurboJpegNative.cs:5`) | TurboJPEG `0x1100` (rộng hơn C# một chút: thêm System32/AddDllDirectory, hợp lý để tìm runtime VC); LibRaw `0x8` | M1 |
| Kích thước đích `tj3Decompress8` | `CalculateOutputBuffer` tin giá trị header; lossless bị cấp thiếu (PARITY dòng 134 ghi rõ) | Parse lại header đúng slice, kiểm pitch và độ dài, ONE cho lossless | Rust tốt hơn C# (sửa một lớp ghi tràn trong DLL) |
| Handle 0/-1 | C# truyền thẳng vào P/Invoke | Rust từ chối trước lời gọi (`check_usable`, `turbojpeg native.rs:674-685`) | Tốt hơn |
| Pin buffer cho `libraw_open_buffer` | `MemoryHandle` pin khai báo trước handle | `'buf` hoặc `Arc` giữ đến sau `libraw_close` | Tương đương |
| Callback huỷ | delegate cdecl, `GCHandle` | `unsafe extern "C" fn` + `Box<CancelToken>`; không có thể bị GC/di chuyển | Tương đương, đơn giản hơn |
| Thử lại nạp DLL thất bại | P/Invoke thử lại mỗi lần gọi | `OnceLock` cache cả thất bại (`libraw native.rs:17-18`, `turbojpeg native.rs:485-494`) | Đã ghi DIVERGENCE; DLL chép vào sau khi khởi động không được nhận (chỉ cần khởi động lại) |
| Kiểm phiên bản | `LibRawAvailability` (0.22.x) + `TrySetUseCameraWb` chỉ trên 0.22.2 | Cùng quy tắc, thêm giới hạn cửa sổ 64 KiB | Tương đương |
| Chuỗi NUL | marshaller C# cắt tại NUL | LibRaw cắt tại NUL (L1); TurboJPEG từ chối | L1 |

## 5. Khoảng trống test (tổng hợp)

- Không có test nạp DLL kiểm tập cờ `LoadLibraryExW` cho LibRaw (chỉ giá trị `0x8`).
- Không có test/fuzz nào đưa file RAW hỏng/cắt/giả header vào `libraw.dll`; corpus RAW chưa có (PARITY dòng 138-139, P-30); các test `decoder_native` có điều kiện corpus return sớm (`return` thành xanh giả, khó phân biệt "chạy thật" với "bỏ qua" trong kết quả `cargo test`).
- Không có test thời gian trần cho giải mã (M2, L4).
- Không có khẳng định biên dịch `LibRawHandle: Send`, `!Sync` và `LibRawImage: Send` (chỉ TurboJPEG có `turbojpeg_handle_is_send` / `turbojpeg_library_is_send_and_sync`, `tests/native.rs:274-285`). Hiện chúng `Send` + `!Sync` nhờ suy luận tự động; chỉ cần ai bỏ `+ Send` khỏi `NativeOwner::release` là đổi, không có test báo.
- Không có test probe-thiếu-export cho TurboJPEG (L2), không có test NUL cho LibRaw (L1).
- Có và tốt: `ffi_wrapper_never_panics_on_mutated_jpegs` (`pr-turbojpeg/tests/native.rs:727`), các test từ chối đích thiếu một byte / pitch sai / lossless (`:550-640`, `:788`), `native_owner_drop_alone_releases_once` và các test `safe_lib_raw_*_dispose_*` (`pr-libraw/tests/native.rs:506-560`), `cancelled_token_aborts_unpack` (`:731`, native), `open_shared_buffer_keeps_the_buffer_alive_after_the_callers_arc_is_dropped` (`:711`, native), `probe_window_accepts_exactly_the_in_window_offsets` (`native.rs:1126`).

## 6. Hàng PARITY cần cân nhắc cho orchestrator (không sửa ở đây)

- `docs/PARITY.md` dòng 138 (I-15b LibRaw native): câu "`libraw.dll` loads only from the app folder" nên kèm giới hạn M1 (DLL chính từ thư mục app, phụ thuộc theo thứ tự tìm kiếm chuẩn). Bằng chứng `pr-libraw/src/native.rs:72-74,409-417`.
- Dòng 134 (I-02b TurboJPEG native): đúng với mã (`0x1100`, guard giải mã). Có thể thêm "probe chỉ chạy `tj3Init`" (L2).
- Dòng 137 (X-turbojpeg-decoder): D-4 nên ghi hệ quả bộ nhớ của L3.

## 7. Thứ tự sửa đề xuất

1. M1 (một dòng hằng số, một test, sửa comment) và L1 (hai kiểm NUL): thay đổi nhỏ, rủi ro thấp, nhưng cần ít nhất `cargo test -p pr-libraw --test native` và một lần chạy native với DLL đã pin để chắc chắn `LoadLibraryExW` với cờ mới vẫn nạp được `libraw.dll` kèm phụ thuộc.
2. L2 (probe resolve đủ export) và khẳng định biên dịch `Send`/`!Sync` cho `LibRawHandle`/`LibRawImage`.
3. M4 (thu hẹp bề mặt API), gộp vào một lượt có build đầy đủ.
4. M3(a) (một lần nạp, dùng lại handle đã probe) khi có cơ hội sửa `availability.rs`.
5. M2 và M3(b): quyết định kiến trúc (watchdog / tiến trình con), cần ý kiến chủ dự án; hiện chỉ ghi nhận.
