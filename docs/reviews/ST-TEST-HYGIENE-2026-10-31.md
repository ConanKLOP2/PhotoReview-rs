# ST-TEST-HYGIENE 2026-10-31: review tĩnh vệ sinh bộ test (`crates/*/tests` và `#[cfg(test)]`)

Status: report (lane tĩnh). Kết quả: 0 cao, 8 trung bình, 15 thấp (23 phát hiện; F6 ghi ở nhóm trung bình nhưng mức thật là thấp, dưới mức trần 40). Không đổi mã production, không thêm test, không sửa PARITY.

Phạm vi và trung thực: chỉ đọc mã, `grep` và một script Python đếm `#[ignore]` (không build, không chạy test, không chạy cargo) tại master `a61cb3aa` (worktree `st-test-hygiene`). Mọi điều dưới đây là **suy luận từ mã, chưa chạy**; số đếm là kết quả regex (`^\s*#\[ignore`, bỏ các dòng chú thích `//`), có thể lệch vài đơn vị. Không đọc nguồn C#. Báo cáo này không lặp lại `docs/reviews/test-quality.md` (test không có assert, 2026-10-23); chỉ đối chiếu khi cần.

Quy ước mức: Cao = test có thể làm hỏng dữ liệu/cấu hình thật của người dùng hoặc che một lỗi nghiêm trọng; Trung bình = khoảng trống chạy thật, flaky có khả năng xảy ra, hoặc test xanh giả trên CI; Thấp = rủi ro hiếm, chỉ ảnh hưởng độ tin cậy cục bộ.

## 1. Tóm tắt

- **Khoảng trống lớn nhất**: khoảng 1 483 `#[ignore]` thật (không tính chú thích); `native-nightly.yml` chạy `--include-ignored` cho đúng 25 binary, tức khoảng 171 test (11,5%). Khoảng 1 100 test cần desktop (`pr-platform-win`, `pr-shell`) và 106 test cần DLL (`pr-libraw`, `pr-turbojpeg`) **không chạy ở đâu trên CI**, chỉ chạy tay qua `tools/dev/native-tests.sh` (xem mục 3).
- Mọi `#[ignore]` thật đều có chuỗi lý do (không có `#[ignore]` trần ngoài chú thích). Hai lý do cũ/sai: `settings_shortcuts.rs:204` ("sau khi W1-02 merge") và comment đầu `native-nightly.yml` nói `pr-imaging cache_key/...` cần DLL trong khi chúng chỉ cần file system (P-1, P-2).
- Test "luôn xanh" kiểu `assert!(true)` không tồn tại (grep 0 kết quả). Golden đọc file đều `panic!` khi thiếu file. **Ngoại lệ có thật**: nhóm test RAW corpus (`raw_exif`, `raw_reader_arw`, `readers_cr2/rw2`, `cr3_reader`, `w5_18_integration`, `raw_nef`) trả về sớm và báo xanh khi `tests/fixtures/raw-corpus` vắng, mà CI không đặt `PHOTOREVIEW_RAW_CORPUS_STRICT` (F1).
- Không thấy test đụng Recycle Bin thật (mọi test dùng `FakeRecycleBin` hoặc fake shell; `recycle_bin_facade.rs:480` dùng hàm fake xoá file test-owned). Registry thật chỉ ở `shell_integration.rs` (khoá riêng `Software\PhotoReviewRsTests\<pid>-<n>-<nanos>`, đã `#[ignore]`, có chặn xoá ngoài tiền tố) và `w6_rerun_killers.rs` (khoá `PhotoReviewRsW6RerunScratch-<pid>`, `#[ignore]`). Không thấy test ghi vào `%LOCALAPPDATA%\PhotoReview` khi chạy mặc định (xem F8 cho ca ngoại lệ khi chạy tay).
- `unsafe` ngoài ba crate cho phép: trong `crates/*/tests` chỉ có `pr-imaging/tests/pixel_memory.rs:39-60` (allocator đếm, có `// SAFETY:`); trong **production** có `unsafe` ở `pr-core` và `pr-imaging` (F18, ngoài phạm vi test nhưng vi phạm AGENTS.md:41 nếu đọc nghiêm).

## 2. Phát hiện (ưu tiên theo rủi ro)

### Trung bình

**F1. Test RAW corpus xanh giả trên CI (trả sớm khi thiếu corpus).**
- File:dòng: `pr-imaging-raw/tests/raw_exif.rs:16,416-418`; `raw_reader_arw.rs:15`; `readers_cr2.rs:1030-1048`; `readers_rw2.rs` (cùng mẫu); `cr3_reader.rs:1122-1146,2110-2130` (`eprintln!("skipped: ...")`); `w5_18_integration.rs`; `raw_nef.rs:444`. Đếm ước lượng (regex `return;|skipped|None => return`): raw_exif 10, raw_reader_arw 12, readers_cr2 5, readers_rw2 5, w5_18_integration 6, cr3_reader 17, raw_nef 3.
- Kịch bản: `tests/fixtures/raw-corpus` không có trong repo (`ls tests/fixtures` không có thư mục này; P-30: corpus không tải). Trên CI hàng chục test "corpus_*" chạy, thoát sớm, đếm là pass. Một hồi quy trong đọc EXIF CR3/CR2/RW2/ORF/ARW không bao giờ bị CI thấy; PARITY có thể ghi "có test" cho các hàng này. `PHOTOREVIEW_RAW_CORPUS_STRICT` không xuất hiện trong `.github`, `tools` hay `AGENTS.md` (grep 0 kết quả), nên chế độ nghiêm không được dùng ở đâu.
- Đề xuất: (a) đổi các test corpus sang `#[ignore = "needs RAW corpus"]` thay vì thoát sớm (như `raf_reader.rs` đã làm, 7 ignore) để số "xanh" trung thực; hoặc (b) in một dòng tổng "corpus tests skipped: N" và để nightly đặt STRICT khi corpus có. Orchestrator quyết: các hàng PARITY dựa trên các test này nên ghi "data-gated, không chạy trên CI".

**F2. Khoảng trống chạy thật: desktop và DLL không có nơi chạy CI.** Xem bảng mục 3. Hai dòng ảnh hưởng nhiều nhất: `pr-platform-win` 1 005 ignore nhưng chỉ 21 chạy (6 binary) ở nightly; `pr-shell` 93 ignore, 0 chạy. Rủi ro: các "native mutation killer" (`w8_*_killers`, `w6_*`) và e2e cửa sổ chỉ bảo vệ khi ai đó chạy tay.
- Đề xuất: giữ nguyên (cần desktop tương tác là lý do thật), nhưng thêm vào `docs/` một dòng "last green native run" do `native-tests.sh` ghi, và đánh dấu các hàng PARITY dựa vào chúng là "chạy tay".

**F3. Comment đầu `native-nightly.yml` sai và đang chặn thêm test chạy được.**
- File:dòng: `.github/workflows/native-nightly.yml:11-14` nói `pr-imaging cache_key/cache_preview_file/memory_headroom/preview_service_disk` cần `libraw.dll/turbojpeg.dll`. Đọc mã: `cache_key.rs:111,779` ("native: real file system", `PhysicalFileSystem` + `TempRoot`), `preview_service_disk.rs:461,485` ("changes a directory ACL"), `cache_disk_store.rs:1047`, `thumbnail_cache.rs:852,933` (ACL/khoá file) đều chỉ cần file system. `memory_headroom.rs:191` chỉ nhắc `#[ignore]` trong chú thích (không có `#[ignore]` thật). `cache_preview_file.rs:11` nói "needs WIC" nhưng `:1014` là `slow`.
- Kịch bản: 9 test pr-imaging chạy được trên runner (file system thuần) bị loại khỏi nightly vì giải thích sai.
- Đề xuất: thêm `pr-imaging` = `cache_disk_store, cache_key, preview_service_disk, thumbnail_cache` vào `$plan` (cần `--test-threads=1` đã có vì dùng icacls); sửa comment. Cảnh báo: các test ACL (icacls /deny) nếu bị kill giữa chừng để lại thư mục `%TEMP%\pr-rs-tests\...` không xoá được (F12).

**F4. `verify_w8_01a.rs` không thể chạy ở đâu (đường dẫn cứng theo máy một người).**
- File:dòng: `pr-app/tests/verify_w8_01a.rs:4` (`const D: &str = "C:/Users/adminvti/AppData/Local/Temp/claude/C--MyProjects-PhotoReview-rs/95c30150-.../scratchpad/orc"`), 10 `#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]` (dòng 13, 35, 75, ...).
- Kịch bản: các test oracle `format_fixed`, snapshot text, pan... không bao giờ chạy trên máy khác hoặc sau khi thư mục phiên bị dọn; đường dẫn chứa tên người dùng và id phiên. Vừa là "ignore không có nơi chạy" vừa là mã chết.
- Đề xuất: chuyển sang `tools/dev/oracle.sh` (đã có skill oracle) sinh golden vào `tests/golden` rồi test đọc golden, hoặc xoá file nếu đã có golden tương đương. Ít nhất đọc đường dẫn từ biến môi trường `PHOTOREVIEW_RS_ORACLE_DIR` thay vì hằng cứng.

**F5. Ràng buộc thời gian thật trong test chạy mặc định (flaky khi máy tải).**
- `pr-core/tests/session_writer.rs:1518-1530` (`bounded_flush_one_budget_covers_lock_wait_and_save`, không ignore): nhả khoá sau `sleep(700ms)`, ngân sách 1200 ms, khẳng định `elapsed < 1500 ms`. Chỉ còn 300 ms dư; một runner Windows tải nặng hoặc quét antivirus làm chậm là đỏ. Không có đồng hồ giả như các test khác trong file (`ManualTimer`).
- `pr-core/tests/diagnostics_file_log.rs:300` (`< 1500 ms`) và `:1278` (`< 1000 ms`): "shutdown/flush không treo trên log không ghi được", đo bằng `Instant` thật.
- `pr-platform-win/tests/w6_rerun_killers.rs:157`: `elapsed < 4s` trên kênh có `recv_timeout(4s)` (vô nghĩa: recv_timeout đã bảo đảm, dòng 157 luôn đúng khi tới được).
- `pr-bench/tests/slow_link_fs.rs:1219`: `golden_limiter_consume_immediate` đòi < 500 ms bằng đồng hồ thật.
- `pr-bench/tests/engine.rs:910-918`: `sleep(30ms)` rồi đòi `b - a >= 20_000_000` ns; ổn nếu `timestamp` là QPC, nhưng phụ thuộc độ phân giải bộ định thời hệ thống.
- Đề xuất: nới giới hạn lên bội số lớn (ví dụ 5-10 lần) hoặc dùng cổng sự kiện thay vì đo thời gian; với `session_writer.rs:1518` dùng `ManualTimer` đã có.

**F6. `pr-core/tests/updates_checker.rs:225-270,757-767`: vòng chờ 1 ms/20 ms bằng `Instant` thật làm "timeout 50 ms".**
- `slow_server_times_out` (dòng 250-261) cho `timeout = 50 ms` và giả lập "treo" bằng vòng `sleep(1ms)`; `caller_cancel_throws` (dòng 263-275) huỷ sau `sleep(20ms)`. Kết quả đúng bất kể tốc độ máy (nhánh do giả lập), nên rủi ro flaky thấp-trung bình: chỉ hỏng nếu thread bị trì hoãn quá 50 ms trước khi vòng bắt đầu, trường hợp đó vẫn trả `Timeout` nên vẫn đúng. Ghi nhận để không báo lại; không cần sửa. (Mức: thấp, giữ ở đây vì là mẫu hay lặp.)

**F7. Test chạy mặc định cần môi trường cụ thể của máy (xanh/đỏ theo ổ đĩa).**
- `pr-platform-win/tests/w6_p01_killers.rs:243-275`: `assert_eq!(source.get_volume_drive_type(&temp), Some(DriveType::Fixed))` và `query_mount_point(&temp).expect("temp dir has a volume")` với `%TEMP%` thật. Đỏ nếu `%TEMP%` nằm trên ổ mạng, ổ rời, RAM disk hoặc Dev Drive; còn đọc registry thật (chỉ đọc, `recycle_bin_settings.rs:397-409`) và `get_user_quota_bytes`. Không ghi gì, nhưng phụ thuộc máy.
- `pr-platform-win/tests/dialogs_service.rs:316`, `pr-shell/tests/startup_composition_root.rs:209`: dùng `%TEMP%` chỉ làm giá trị chuỗi (an toàn).
- Đề xuất: chấp nhận mọi `DriveType` hợp lệ cho `temp`, hoặc `#[ignore = "native: needs fixed local volume"]`.

**F8. `pr-core/tests/session_store.rs:543-556`: `#[ignore = "native: reads the real environment"]` gọi `SessionStore::from_environment()` + `wait_for_startup_sweep()`.**
- Kịch bản: khi chạy tay `cargo test -- --include-ignored` **không** đặt `PHOTOREVIEW_RS_DATA_ROOT`/`ISOLATE_CONFIG`, `AppPaths::from_environment()` (`app_paths.rs:57-66`) trỏ vào `%LOCALAPPDATA%\PhotoReview\...\sessions` thật và nền sweep xoá `*.tmp` cũ hơn 1 ngày (`session/store.rs:9`). Rủi ro nhỏ (chỉ file tmp), nhưng là chạm dữ liệu thật. Nightly đã đặt hai biến nên an toàn; chỉ lệnh chạy tay thiếu.
- `pr-core/tests/w2_fs_mutation_killers.rs:635-660` (child process) tương tự về ý tưởng nhưng chỉ đặt biến trace.
- Đề xuất: test tự kiểm biến môi trường (`assert!(isolated)`) hoặc `tools/dev/native-tests.sh` luôn xuất hai biến (đối chiếu với nightly; chưa đọc script, suy luận).

**F9. `pr-bench/tests/cli_tool_path_guard.rs:238` + thư mục %TEMP% cố định `PhotoReview-Benchmark-Reports`.**
- Test mặc định `resolve_report_directory(... None)` chỉ so tiền tố, không tạo thư mục: không va chạm. Nhưng `pr-app/tests/file_action_fakes/mod.rs:441` dùng `%TEMP%\pr-file-action-fakes` **cố định** làm gốc `AppPaths` cho `OperationJournal` thật ("never written"). Nếu một test nào đó ghi journal, các worktree/test song song dùng chung thư mục và ghi đè nhau. Không thấy test ghi hiện nay (suy luận, chưa chạy). Đề xuất: dùng `TempRoot` hoặc ghi chú bất biến "không ghi" bằng `FakeFileSystem`.

### Thấp

**F10. `pr-core/tests/settings_shortcuts.rs:201-216`: `#[ignore]` đã lỗi thời.** Ghi "IGNORED until W1-02 merges ... serde_ext ... still an unimplemented stub". `pr-core/src/model/serde_ext.rs` hiện có `serialize_lenient`/`serialize_settings_enum` và không còn `todo!()`/`unimplemented!()` (grep 0). Test này không nằm trong nightly. Đề xuất: bỏ `#[ignore]`, chạy một lần (hoặc xoá nếu golden khác đã bao phủ: `settings_app_settings.rs:399`, `settings_store.rs:549`). Suy luận: có thể vẫn đỏ; chưa chạy.

**F11. Khoảng trống chạy của nhóm "slow" file-system thuần.** `pr-core` `diagnostics_perf_csv.rs:628` (1,2 s, 1 test), `file_actions_compaction.rs` (4 test slow), `session_writer.rs` (1 test, "waits the 2 s shutdown budget"); `pr-imaging` `cache_preview_file.rs:1014`, `preview_file_repair.rs` (cấp phát > 2 GiB), `w4_mutation_killers3.rs` (2 GiB), `verify_navigation_pace.rs` (2^31 bản ghi, release); `pr-turbojpeg` `decoder_pure.rs` (2 test 2 GiB), `area_resampler.rs` (12 Mpixel). Không có trong nightly dù không cần desktop/DLL (trừ `pr-turbojpeg`, nhóm này thuần). Đề xuất: thêm các binary thuần vào nightly với `--release` cho nhóm 2 GiB (cẩn thận bộ nhớ runner 7 GB: chỉ 1 test cấp phát một lần).

**F12. `TempRoot` có thể dùng lại thư mục cũ của tiến trình đã chết.** `pr-test-support/src/temp_root.rs:15-26`: tên `pid-n-label`, `create_dir_all` chấp nhận thư mục tồn tại, `Drop` nuốt lỗi `remove_dir_all`. PID Windows được tái sử dụng; một lần chạy trước bị kill (hoặc để lại ACL deny như `thumbnail_cache.rs:852`, `preview_service_disk.rs:461`) có thể để thư mục cùng tên và test mới thấy file cũ. Rủi ro rất thấp. Đề xuất: `create_dir` (không `_all` cho lá) và thêm nanos vào tên.

**F13. `pr-app/tests/startup_launch_cwd.rs:41-47`: đổi `current_dir` toàn tiến trình.** An toàn hôm nay vì binary chỉ có 1 `#[test]` (đếm: 1); nếu ai thêm test thứ hai trong file, chạy song song sẽ đua `cwd`. Ngược lại `pr-core/tests/io_physical.rs:341` và `text_dotnet_path.rs:118` **đọc** `current_dir` (không đổi) nên không bị ảnh hưởng chỉ khi chúng ở binary khác (đúng hiện tại). Đề xuất: thêm chú thích cảnh báo "một test duy nhất".

**F14. `pr-platform-win/tests/dialogs_native_pickers.rs:46,191`: `sleep(100ms)` trong test ignore desktop.** `shell_menus_host.rs:278-284,730-752` (50-100 ms), `host_ui_dispatcher_native.rs:817` (`sleep(60)` "crosses the 50 ms slice"). Tất cả đã `#[ignore = "native: needs desktop"]` nên không ảnh hưởng CI; chỉ flaky khi chạy tay. Các file khác tự ghi "every wait is bounded; no sleeps" (`host_shell_window_fixes.rs:2`) nên đây là ngoại lệ. Đề xuất: thay bằng vòng chờ điều kiện có hạn.

**F15. `pr-core/tests/diagnostics_perf_csv.rs:1326-1330,1450-1505`: vòng quay `Instant` 1,6-2,2 s trong test không ignore (đếm: dòng 631 là test ignore; các dòng này kiểm tra tránh "writer thread leak").** Thời gian thật; chậm CI thêm vài giây và khẳng định theo "không quá N lần flush". Rủi ro thấp (chỉ đúng/sai theo số lần). Đề xuất: giữ, ghi rõ ngưỡng. (Chưa đọc hết thân test; suy luận.)

**F16. Test "chỉ kiểm không panic" (đã có assert yếu).**
- `pr-app/tests/shell_viewing_scaling.rs:89-95`: `choose_never_panics_on_zero_and_negative_sizes` gọi `choose` với `let _ = ...` và không khẳng định kết quả.
- `pr-app/tests/coordinators_preload_adapter.rs:241-243`: `let _ = adapter.try_consume_preloaded_key(&key); let _ = adapter.is_idle(); let _ = adapter.get_viewer_decode_delay();` (kết quả bị bỏ, chỉ đếm `reads == 7` ở cuối, nên có assert gián tiếp).
- `pr-app/tests/overlay_layer.rs:379` (`#[should_panic(expected = "empty")]`): ổn.
- Các test `let _ = f(); assert_eq!(input_before, input_after)` (`windowing_fullscreen.rs:860`, `pr-bench/tests/metrics.rs:235`, `models.rs:867`) là hợp lệ (kiểm bất biến đầu vào).
- Đề xuất: ở `shell_viewing_scaling.rs:89` thêm khẳng định về giá trị trả về (ví dụ không phải `Heavy` với vùng 0) hoặc đổi tên thành test "smoke".

**F17. `pr-perf/tests/perf_analyze_nav.rs:618,625` (`manual benchmark`) và `pr-imaging-raw/tests/raw_nef.rs` (`perf: wall-clock budget is machine dependent`).** Không có nơi chạy; đúng chủ ý. Chỉ ghi vào bảng; không phải lỗi.

**F18. `unsafe` production ngoài `pr-platform-win/pr-libraw/pr-turbojpeg` (trái AGENTS.md:41, nếu hiểu chặt).** `pr-core/src/io/physical.rs:763-828` (`MoveFileExW`, `DeleteFileW`), `io/journal_compaction_files.rs:415-465`, `text/dotnet_path.rs:133,186` (`GetLongPathNameW`); `pr-imaging/src/cache/atomic_file.rs:320-341`, `pixels.rs:174,185`, `pixel_memory.rs:115,182`. Có `// SAFETY:` hay không chưa kiểm từng khối ở lane này (skill `unsafe-audit` làm). Là code production, không phải test; ghi để orchestrator quyết (AGENTS.md có lẽ có ngoại lệ cho pr-core/pr-imaging, chưa đọc đủ).
- Trong `tests/`: chỉ `pr-imaging/tests/pixel_memory.rs:39-60` (`GlobalAlloc` đếm, mỗi khối có `// SAFETY:`), là cách duy nhất đếm cấp phát; nên được liệt vào ngoại lệ "test-only" trong AGENTS.md.

**F19. `win32_comctl.rs:540-575,653-668`: `read_dir(%TEMP%)` đếm file bắt đầu bằng `PhotoReviewRs-comctl-<pid>-`.** Quét thư mục `%TEMP%` thật (có thể hàng nghìn file) mỗi lần gọi; tiền tố theo pid nên không va chạm. Đã `#[ignore = desktop]`. Chỉ chậm.

**F20. Chuẩn hoá lý do `#[ignore]`.** Chuỗi lý do dùng ~35 biến thể (ví dụ `native`, `native: real files`, `native: real disk`, `native: real files under TempRoot`, `native: needs desktop`, `native: requires desktop; ...`). Không thể lọc theo nhóm bằng một regex (chính nightly phải liệt kê tên binary tay, dòng 49-66). Đề xuất: tiền tố chuẩn `native-fs:`, `native-desktop:`, `native-dll:`, `slow:`, `manual:` để nightly/`native-tests.sh` chọn theo nhãn.

**F21. `pr-core/tests/session_writer.rs:1356` `sleep(300ms)` rồi khẳng định "không có ghi thêm".** Âm tính sau thời gian ngủ: xanh giả khi thread nền chậm hơn 300 ms. Đề xuất: dùng `writer.when_idle()` như dòng 1363.

**F22. Cổng/tên pipe/mutex cố định: không thấy.** Pipe tests dùng tên duy nhất (`forward_client_native.rs:15`, `forward_pipe_native.rs:83-85`); không tìm thấy `127.0.0.1:<port>` cố định (grep 0). `w6_platform_native.rs:45` dùng mutex tên thật theo thư mục (đã bao trong nightly, `--test-threads=1`). Chỉ cảnh báo: hai worktree chạy nightly/native song song sẽ đụng mutex này (suy luận). Ghi vào để loại trừ nghi vấn, không cần sửa.

**F23. Registry thật: `shell_integration.rs:23,147-153,172-184` ghi `HKCU\Software\PhotoReviewRsTests\...`.** Đã cô lập tốt (khoá riêng, kiểm tiền tố, xoá cây khi Drop). Rủi ro duy nhất: tiến trình bị kill giữa chừng để lại khoá. Đề xuất: ghi vào `janitor.sh` (dọn `HKCU\Software\PhotoReviewRs*`). Không có test ghi `HKCU` ngoài hai file trên (grep).

## 3. Bảng binary có `#[ignore]` KHÔNG chạy ở đâu trên CI

Đối chiếu với `native-nightly.yml` hiện tại (`$plan`: 17 binary `pr-core`, 6 `pr-platform-win`, 2 `pr-app`; tất cả bằng `--include-ignored --test-threads=1`; không bắt buộc, không nằm trong `needs`). `ci.yml` không có `--include-ignored` (grep). "Số ignore" là số thuộc tính thật.

| Crate | Binary / tệp (số ignore) | Lý do ignore | Chạy ở đâu | Ghi chú |
|---|---|---|---|---|
| pr-core | 17 binary trong `$plan` (143) | file system, journal, TempRoot | nightly | Đã được gate |
| pr-core | `diagnostics_perf_csv` (1), `file_actions_compaction` (4), `session_writer` (1) | slow | KHÔNG | F11 |
| pr-core | `settings_shortcuts` (1) | chờ W1-02 (lỗi thời) | KHÔNG | F10 |
| pr-app | `folder_load_load` (3), `windowing_placement_store` (4) | file system | nightly | |
| pr-app | `verify_w8_01a` (10) | oracle ở đường dẫn cục bộ | KHÔNG (không ở máy nào khác) | F4 |
| pr-app | `presenter_present_a`, `presenter_state`, `shell_viewing_image_layer` (1 mỗi file) | chỉ nhắc `#[ignore]` trong chú thích hoặc 1 test | cần kiểm | Số đếm gồm các tệp có 1 ignore; chưa đọc từng cái |
| pr-bench | `cli_tool_path_guard` (4: junction), `workload_runner` (1: race decoder) | file system, TempRoot | KHÔNG | nightly loại pr-bench; riêng junction chỉ chạm TempRoot nên có thể thêm |
| pr-imaging | `cache_disk_store` (1), `cache_key` (2), `preview_service_disk` (2), `thumbnail_cache` (2) | ACL/real fs | KHÔNG | F3: chạy được trên runner |
| pr-imaging | `cache_preview_file` (1), `preview_file_repair` (1), `w4_mutation_killers3` (1), `verify_navigation_pace` (1) | slow (bộ nhớ lớn) | KHÔNG | F11 |
| pr-imaging-raw | `raf_reader` (7), `raw_nef` (1) | cần corpus RAW / perf | KHÔNG | F1, F17 |
| pr-libraw | `availability` (4), `decoder_native` (23), `native` (9), `synthetic_raw` (4) = 40 | `libraw.dll` (+ corpus) | KHÔNG | P-55 chặn tải DLL; `native-tests.sh` chạy tay |
| pr-turbojpeg | `availability` (1), `decoder_native` (41), `native` (20), `native_golden` (1) = 63; `decoder_pure` (2), `area_resampler` (1) slow; `src/native.rs` (1) | `turbojpeg.dll`, slow | KHÔNG | cùng lý do DLL; `decoder_pure` và `area_resampler` là slow thuần |
| pr-perf | `perf_analyze_nav` (2) | manual benchmark | KHÔNG (chủ ý) | F17 |
| pr-platform-win | 6 binary trong `$plan` (21) | fs + WARP (composer) | nightly | Composer: nightly chưa xác nhận WARP (comment `yml:56-58`) |
| pr-platform-win | 120 binary ngoài `$plan` (984) | desktop/GPU/WIC/Win32 | KHÔNG | F2 |
| pr-platform-win | `src/` 12 tệp (85): `render/*`, `dialogs/native/*`, `live_operation_registry`, `dark_title_bar`, `memory_probe`, `natural_comparer`, `wic/decoder` | desktop/GPU/WIC | KHÔNG | nightly chỉ chạy `--test <bin>`, không có `--lib --include-ignored` |
| pr-shell | 20 binary (89): `ui_smoke*`, `startup_*`, `composition_*`, `exe_process`, ... | desktop/exe | KHÔNG | `src/startup/window.rs` (4) cũng KHÔNG |
| pr-test-support | `src/temp_root.rs` (1) | chỉ nhắc `#[ignore = "native"]` trong chú thích | n/a | không phải thuộc tính thật (script đếm 1 do pr-test-support; chưa xác minh) |

Tổng ước lượng: ~1 483 ignore thật; chạy ở nightly ~171; không chạy ở đâu ~1 312 (trong đó 1 105 desktop, 106 DLL, 10 oracle cục bộ, ~91 các loại khác). Số này là suy luận từ regex; nightly "chạy" chỉ khi nó xanh (không bắt buộc, chưa có dữ liệu chạy trong báo cáo này).

## 4. Việc cần orchestrator quyết (không tự sửa trong lane này)

1. PARITY: các hàng chỉ dựa vào test RAW corpus (CR2/CR3/RW2/ORF/ARW EXIF) cần ghi "data-gated, xanh giả trên CI" (F1). Danh sách file:dòng ở F1; chưa tra số hàng PARITY cụ thể vì lane này cấm sửa PARITY.
2. Có muốn thêm `pr-imaging` (4 binary) và nhóm "slow thuần" vào `native-nightly.yml` không (F3, F11).
3. Xoá hoặc chuyển `verify_w8_01a.rs` sang oracle chuẩn (F4).
4. Có chấp nhận `unsafe` trong `pr-core`/`pr-imaging` (F18) hay cần chuyển sang crate cho phép.
