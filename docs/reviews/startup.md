# Review tĩnh: startup và composition (W8-17a..17c, W8-09b)

Machine C (cloud). Phạm vi đọc: phần ĐÃ cài đặt của
- `crates/pr-platform-win/src/shell_startup/{cloak,dpi,locale,placement,warmup}.rs`
- `crates/pr-app/src/startup/{launch,error_log,forwarded_open}.rs`
- `crates/pr-shell/src/startup/{composition_root,secondary_dialogs}.rs`
- `crates/pr-shell/src/composition/{registry,adapters}.rs` (đọc lướt `factories`, `decoders`, `platform`, `texts`)
- test: `shell_startup_*.rs`, `startup_*.rs` (pr-app, pr-shell), `composition_*.rs`.

So với C# tại `7eaee2a2`/`d92d6aae` (`Win32SecondaryWindowPort.cs`, `ShellCompositionRoot.cs`, `Win32DialogService.cs`).

**Mọi phát hiện dưới đây: suy luận từ mã, chưa chạy thử.** Không chạy cargo, không chạy test.

**Ngoài phạm vi đọc được (vẫn là `todo!()` trên master):** `pr-shell/src/entry.rs`, `startup/{run,state,shutdown,window,phase_one,instance,forward_target,graph,options,error,overlay_vm,recovery,smoke}.rs`, `composition/{graph,main_view_model,update_http}.rs`, `shell_startup/{apartment,monitor_probe,local_time}.rs`, và phần lớn `pr-app/src/startup/*` (crash, early_decode, instance, options, settings_load, shutdown, smoke, ui_hop, recovery, placement_read). Các đơn vị 17d/17e, 20a, 20b chưa merge nên không có gì để review ở đó; lưu ý bất kỳ lỗi nào trong chúng chưa thể phát hiện.

Tổng: 1 cao / 4 trung bình / 6 thấp.

## Đã đọc và thấy đúng
- `cloak.rs`: khoá `Mutex` không giữ qua lời gọi DWM/`schedule_timeout`; `reveal` idempotent; `catch_unwind` quanh callback; `DWMWA_CLOAK` truyền `i32` (4 byte) đúng cỡ qua `dwm_set_window_attribute`.
- `placement.rs`: `WINDOWPLACEMENT.length` được đặt bởi `new_window_placement()`/wrapper; `show_command_for`, `apply_before_show` (cửa sổ không hiển thị trên màn hình nào vẫn mở Maximized) khớp mô tả.
- `warmup.rs`: mọi lỗi gói trong `catch_unwind` trên luồng riêng.
- `locale.rs`: bộ đệm 85/128 u16 đúng cỡ; độ dài `GetLocaleInfoEx`/`GetNumberFormatEx` được dùng để cắt (n-1).
- `error_log.rs`, `launch.rs`, `forwarded_open.rs`: logic gộp yêu cầu, cắt log giữ nửa cuối theo ranh giới dòng, chuẩn hoá đối số khớp mô tả.
- `registry.rs`: không có chu trình khởi tạo trong các `OnceLock::get_or_init` (đã lần theo file_system→metrics, journal→settings_store, file_action→journal, undo→file_action, decoder_factory→source_bytes_policy, preview_service→...).

## Phát hiện

### H1 (cao, xác nhận bằng cách đọc): `WindowsRecoveryTimeFormatter::format_local_general` coi tick .NET là FILETIME
- File: `pr-platform-win/src/shell_startup/locale.rs`, hàm `format_local_general` (khoảng dòng 118-125): `let filetime = u64::try_from(utc.ticks())...` rồi `FileTimeToSystemTime(&filetime)`.
- `DateTimeUtc::ticks()` là tick .NET từ 0001-01-01 (`pr-core/src/time.rs`, `UNIX_EPOCH_TICKS = 621355968000000000`, `FILETIME_EPOCH_TICKS = 504911232000000000` là hiệu số giữa hai gốc). `FileTimeToSystemTime` tính từ 1601-01-01. Thiếu bước trừ `504_911_232_000_000_000`.
- Kịch bản: mục journal lúc 2026-10-23 12:00 UTC → tick ≈ 6,39e17 → hiểu là FILETIME → hiển thị năm khoảng 3627. Cửa sổ Recovery hiện ngày giờ sai ~1601 năm; không có lỗi nào được ném (năm hợp lệ tới 30827).
- Vì sao test không bắt: `tests/shell_startup_locale.rs:78-100` bị `#[ignore]`, cần múi giờ UTC, và chỉ khẳng định `formatted.contains("2020") || formatted.contains("20")` — năm sai 3620 vẫn chứa "20" nên đúng giả.
- Gợi ý: `filetime = ticks - FILETIME_EPOCH_TICKS` (từ chối nếu âm); đổi test thành so chuỗi cụ thể/năm == 2020. Dòng `.checked_div(1).filter(|_| ticks > 0)` vô nghĩa, nên bỏ.

### M1 (trung bình, xác nhận): `SharedDialogService` không chuyển ba phương thức prompt
- File: `pr-shell/src/composition/adapters.rs` — `impl DialogService for SharedDialogService` (khoảng dòng 40-85) chỉ chuyển 11 phương thức; thiếu `prompt_add_external_editor`, `prompt_default_external_editor`, `prompt_rename`. Trait `DialogService` (`pr-core/src/abstractions/dialog.rs:49,77,83`) có bản mặc định trả `None`. `Win32DialogService` (dialog_service.rs) chuyển hai trong số đó; cái thứ ba (`prompt_rename`) đã nêu ở `dialogs-base` M1.
- Chính comment ở `show_settings_at` ("MUST forward (the trait default would drop `target`)") cho thấy cùng loại lỗi bị bỏ sót ở đây.
- Kịch bản: view model gọi `dialogs.prompt_add_external_editor(..)` qua `Arc<dyn DialogService>` = `SharedDialogService` → mặc định `None` → "Add more editors…" / "Set default editor…" / F2 Rename coi như huỷ, im lặng, dù cổng cửa sổ phụ đã cài.
- Gợi ý: chuyển đủ ba phương thức; thêm test kiểm tra mọi phương thức của trait được chuyển (có thể bằng fake ghi lại cuộc gọi).

### M2 (trung bình, xác nhận): cửa sổ Recovery không bao giờ biết cài đặt "cho phép xoá vĩnh viễn"
- File: `pr-shell/src/startup/secondary_dialogs.rs`, `recovery()` đặt `allow_permanent_delete: None`. `RecoveryWindowPresenter` (`pr-app/src/presenters/recovery_window.rs:709`) dùng `allow_permanent_delete.as_ref().is_some_and(|f| f())` nên `None` = luôn `false`.
- C# (`Win32SecondaryWindowPort.ShowRecovery`): `() => settings?.Current.AllowPermanentDeleteWithoutRecycleBin == true`, đọc lúc hỏi.
- Kịch bản: người dùng bật "Allow permanent delete" trong Settings, mở Recovery, thử lại một mục mà đích là thùng rác không có → văn bản xác nhận luôn nói như thể xoá vĩnh viễn bị cấm (hoặc ngược lại tuỳ lời nhắc), khác C#.
- Gợi ý: truyền closure đọc `settings_store.current().allow_permanent_delete_without_recycle_bin`.

### M3 (trung bình, xác nhận): gọi cổng cửa sổ phụ trước khi `bind_dialog_services` khoá cổng vĩnh viễn ở `None`
- File: `pr-shell/src/startup/composition_root.rs` (factory trong `ShellServices::build`, khoảng dòng 205-245) + `pr-platform-win/src/dialogs/dialog_service.rs` `LazySecondaryPort::get`.
- Khi `graph_slot` chưa được gán, factory ghi log "requested before the service graph was bound" và trả `None`; `LazySecondaryPort` lưu `Some(None)` (xem `get`: `self.value.set(answer)`) nên về sau, dù `bind_dialog_services` thành công, `get()` vẫn trả `None`.
- Kịch bản: bất kỳ đường nào gọi `show_settings`/`show_recovery`/... trước khi W8-17e ràng buộc đồ thị (ví dụ yêu cầu từ người dùng/forwarded open sớm) → Settings, Recovery, Diagnostics, Benchmark… không bao giờ mở được trong cả phiên, chỉ có một dòng log.
- Test `factory_before_the_graph_is_bound_answers_no_port_and_logs` (`startup_composition_root.rs:244-253`) chỉ khẳng định `None` và log; không kiểm tra phục hồi sau `bind`.
- Gợi ý: không lưu kết quả `None` do "chưa bind" (trả lỗi tạm), hoặc buộc `bind` trước khi tạo `Win32DialogService`.

### M4 (trung bình, xác nhận; đã biết là chưa xong): cổng sản xuất gọi `SettingsDialog::new` vẫn là `todo!()`
- File: `secondary_dialogs.rs` `settings()` → `SettingsDialog::new(args)`; `pr-platform-win/src/dialogs/settings/dialog/frame.rs:496-497` là `todo!()` (W8-19j..19m chưa merge).
- Kịch bản: khi đồ thị thật được nối (17e), mở Settings sẽ panic trên luồng UI. Hiện chưa chạm tới vì `startup/run.rs` cũng còn `todo!()`. Ghi lại để không bỏ sót khi ghép.
- Ngoài ra C# `ShowSettings` còn truyền `ShowExternalEditors`; bản Rust (`SettingsDialogOptions`) chỉ truyền `show_action_profiles`.

### L1 (thấp, xác nhận): `local_long_time_text` bỏ giây
- File: `locale.rs` `local_long_time_text` dùng `format_system_time(&now, false)` với cờ `TIME_NOSECONDS`; C# dùng `ToString("T")` (long time, có giây) cho dòng "done" của Recovery (`RecoveryWindowPresenter.cs:242`). Doc comment của hàm cũng nói "long time pattern".
- Kịch bản: "Checked … at 14:05" thay vì "14:05:37". Gợi ý: chỉ dùng `TIME_NOSECONDS` cho nhánh `g`, còn `T` truyền cờ 0.

### L2 (thấp, xác nhận): vòng tham chiếu `Arc` trong `SharedServices`
- File: `registry.rs`: `preload_scheduler_factory` lưu closure giữ `shared = self.clone()` (cell trong `Inner`) và `decoder_factory` lưu `decoder_decorator` giữ `shared` — `Inner → cell → closure → Arc<Inner>`.
- Hệ quả (chưa chắc, phụ thuộc cách tắt máy): `Drop` của `SettingsStore`/`SessionStore`/`SessionWriter`/dịch vụ preview không bao giờ chạy; mọi thứ phải dọn bằng lời gọi tường minh (`shutdown.rs` còn `todo!()`). Trong test, mỗi `SharedServices` rò rỉ luồng nền.
- Gợi ý: closure giữ `Weak<Inner>` hoặc chỉ các `Arc` con cần dùng.

### L3 (thấp, xác nhận): lỗi tạm thời bị lưu vĩnh viễn trong `OnceLock<Result<..>>`
- File: `registry.rs` (`session_writer`, `file_hash_service`, `decoder_factory`, `thumbnail_cache`, `source_bytes_policy`, `preview_service`). Một `Err` (thread không khởi động được, thư mục cache lỗi) được ghi nhớ; mọi lần gọi sau đều trả cùng `Err`. Ngoài ra `source_bytes_policy`/`preview_service` đọc settings MỘT lần ở lần gọi đầu; nếu gọi trước `load` thì giữ mặc định cả phiên (comment đã cảnh báo, nhưng không có kiểm tra thời điểm).

### L4 (thấp, xác nhận): panic trên luồng UI từ adapter lazy
- File: `adapters.rs` `LazyDecoderFactory::resolve` (dòng ~188) và `LazyPrewarmPreviews::start_viewer_preview` (dòng ~221) `panic!("{error}")` khi registry trả `Err`. Comment nói "không thể xảy ra", nhưng `decoder_factory()` có thể `Err` qua `source_bytes_policy()?` (L3). `LazyDecoderFactory` được `DialogServices` dùng khi mở Settings (luồng UI) → panic xuyên khung OS (R0003). Gợi ý: trả decoder dự phòng/ghi log rồi từ chối thao tác.

### L5 (thấp, chỉ nghi ngờ): thread hạn chót của cloak không khởi động được
- File: `cloak.rs` `schedule_on_thread`: nếu `spawn` lỗi, chỉ `cancel.cancel()`; không có hạn chót an toàn. Nếu khung hình đầu tiên không bao giờ được present, cửa sổ bị che (cloak) vĩnh viễn. Cực hiếm. Cũng vậy: `reveal_inner` bỏ qua kết quả `set_cloaked(.., false)`, không thử lại nếu DWM từ chối.

### L6 (thấp, chỉ nghi ngờ): `percent_one_decimal` dùng bảng ngôn ngữ cứng
- File: `locale.rs` `percent_one_decimal`: chọn dấu cách/vị trí `%` theo danh sách mã ngôn ngữ cố định; .NET lấy mẫu từ `NumberFormatInfo.PercentPositivePattern` của văn hoá. Các ngôn ngữ ngoài danh sách (ví dụ `ar`, `he`, `ko`…) mặc định `{số}%`. Chưa kiểm được .NET trả mẫu nào cho từng văn hoá → chưa chắc.

## Test yếu / rỗng (suy luận từ mã)
- `pr-shell/tests/startup_composition_root.rs:255-265` `host_registrations_are_all_mapped`: đọc `tests/golden/shell-composition-registrations.json` theo đường dẫn tương đối; `cargo test` chạy với thư mục làm việc là thư mục gói (`crates/pr-shell`), nơi không có `tests/golden/` (golden nằm ở `tests/golden/` gốc repo; `ls crates/pr-shell/tests/golden` không tồn tại) → luôn rơi vào nhánh `eprintln!("skip…"); return;` và PASS mà không kiểm gì. Chỉ `contains` khi file có.
- `pr-platform-win/tests/shell_startup_locale.rs:78-100`: xem H1 (ignore + `contains("20")`).
- Ignore: `startup_wiring_native.rs` 13/13, `shell_startup_native.rs` 4/4, `startup_secondary_dialogs.rs` 4/8, `shell_startup_locale.rs` 2/6, `shell_startup_warmup.rs` 1/5, `composition_decoders.rs` 3/13, `composition_registry.rs` 1/29.
- `pr-shell/tests/ui_smoke*.rs` (ui_smoke, ui_smoke_dialogs, ui_smoke_overlay, ui_smoke_perf, ui_smoke_startup, ui_smoke_viewing): khoảng 40 test, mọi thân hàm 0 assert và đều `#[ignore]` (chỗ giữ chỗ cho kiểm tra trên desktop). `ui_parity_map.rs` có 6 test 0 assert trực tiếp (dùng helper) — chỉ ghi nhận, sẽ gộp vào báo cáo test-quality.
- Không có test cho: sự phục hồi cổng sau `bind_dialog_services` (M3), chuyển tiếp đủ các phương thức của `SharedDialogService` (M1), `allow_permanent_delete` của Recovery (M2), định dạng năm của `format_local_general` (H1).

## Không kiểm tra được
- Mọi thứ cần Windows/desktop: DWM cloak thật, `SetWindowPlacement` trên nhiều màn hình, `GetUserDefaultLocaleName`/`GetNumberFormatEx` theo từng văn hoá, múi giờ máy, hành vi `SystemTimeToTzSpecificLocalTime`.
- Thứ tự khởi động thật và tắt máy (run/shutdown/instance/forward_target còn `todo!()`), nên không xác nhận được M3, L2 trong luồng đầy đủ.
- `win32::startup` (`set_process_dpi_awareness_context`, so sánh DPI context) chỉ đọc chỗ gọi.
- `factories.rs`, `decoders.rs`, `platform.rs`, `texts.rs` chỉ đọc lướt (không có `unsafe`, chỉ hai `panic!` ở adapters); chưa đối chiếu từng chuỗi văn bản với C#.
