# ST-SHELL-REVIEW 2026-10-31: review tĩnh `crates/pr-shell`

Status: review tĩnh, chưa chạy. Mọi phát hiện là suy luận từ mã; không build, không chạy test, không chạm Thùng rác thật, `%LOCALAPPDATA%\PhotoReview` hay cấu hình thật. Không sửa mã Rust.

Rust: `origin/master` @ `a61cb3aa`. C# gốc: `7eaee2a2` (`Startup/ShellStartup.cs`, `Program.cs`).

Tổng: **0 cao / 6 trung bình / 8 thấp**. Không thấy đường mất dữ liệu hay lỗi INV-4 trong cây `pr-shell`; các mục trung bình là mất yêu cầu mở file, mất cảnh báo, và thiếu giá trị cấu hình.

## Phạm vi đã đọc

- `src/entry.rs`, `src/main.rs`, `src/startup/{run,state,phase_one,window,shutdown,recovery,smoke,instance,options,graph,error,forward_target,overlay_vm,composition_root}.rs` (toàn văn), `src/startup/secondary_dialogs.rs` (grep chỗ nuốt lỗi và đọc phần Settings/Diagnostics).
- `src/composition/{graph,platform,factories,adapters,main_view_model}.rs` (toàn văn), `registry.rs` (dòng 60-500 và grep), `texts.rs`/`decoders.rs` (grep).
- Ngoài crate, chỉ để lần theo luồng: `pr-app/src/startup/{launch,options,shutdown,instance,forwarded_open,settings_load}.rs`, `pr-app/src/misc/{launch_args,app_log}.rs`, `pr-core/src/app_paths.rs`, `pr-core/src/settings/store.rs` (hàm `load`), `pr-core/src/instance/handoff.rs`, `pr-platform-win/src/host/main_window/lifecycle.rs` (`update_view_model`), `pr-platform-win/src/recycle_bin.rs`.
- Test: tên test trong `crates/pr-shell/tests/*.rs` (đọc tên, không đọc từng dòng). Danh sách file ở mục cuối.
- C#: `ShellStartup.cs` dòng 205-255 (`FailStartup`) và 640-684 (`CloseSecondaryWindows`, `Cleanup`).

## Phát hiện

### M1 (Trung bình) — yêu cầu mở file chuyển tiếp bị mất (panic `BorrowMutError`) khi một lệnh modal đang giữ view model

- File:line: `crates/pr-platform-win/src/host/main_window/lifecycle.rs:324-328` (`update_view_model` dùng `borrow_mut()`), gọi từ `lifecycle.rs:261-263` (`open_path`) ← `crates/pr-shell/src/startup/forward_target.rs:255-258` ← `crates/pr-app/src/startup/forwarded_open.rs:139-150` (`handle`, sau khi đã `activate`). Chính `crates/pr-shell/src/startup/overlay_vm.rs:5-8,90-96,172-196` mô tả: lệnh modal (Settings, chọn thư mục) chạy bên trong `update_view_model` với view model bị mượn mutable.
- Kịch bản (suy luận, chưa chạy): đang mở hộp Settings (hoặc chọn thư mục, benchmark do `pick_and_open_folder`), người dùng bấm đúp một ảnh trong Explorer. Tiến trình thứ hai chuyển đường dẫn qua pipe, bộ gộp 750 ms gửi lên UI thread, vòng lặp lồng của hộp thoại chạy job đó, `open_path` gọi `borrow_mut()` khi đã có borrow, panic. `ui_dispatcher.rs:410-416` bắt panic (job bị nuốt, chỉ còn dòng "Unhandled panic" nếu hook chạy), cửa sổ đã được `activate` nhưng ảnh không mở, không có thông báo cho người dùng.
- Test hiện có: `tests/startup_overlay_vm.rs:59,74,103,124` phủ việc ĐỌC/GHI settings khi modal giữ borrow (sửa sau walkthrough 2026-10-29) nhưng không phủ đường forwarded-open; `tests/startup_forward_target.rs:54` chỉ kiểm cloning khỏi slot; `tests/startup_run_native.rs:354` chỉ kiểm forward khi đang khởi động.
- Đề xuất: trong `MainWindowForwardTarget::open_path`, nếu view model đang bị mượn (`try_borrow_mut` lỗi) thì xếp đường dẫn vào hàng đợi như `Deferred` của `overlay_vm.rs` và áp dụng sau khi lệnh modal trả về; hoặc đổi `MainWindowInner::update_view_model` sang `try_borrow_mut` với hàng đợi. Thêm một test native `#[ignore]` (mở modal giả, bắn forward, kiểm tra ảnh mở sau khi đóng modal).

### M2 (Trung bình) — `WindowsRecycleBin` vẫn dùng `NullLog` ở production (ST-COM-RECYCLE T5 chưa xử lý)

- File:line: `crates/pr-shell/src/composition/platform.rs:351-355` (truyền `log` cho `WindowsRecycleBinRestoreBackend::new(Arc::clone(&log))` nhưng facade thì không) → `crates/pr-platform-win/src/recycle_bin.rs:52-62` (`WindowsRecycleBin::new` hard-code `Arc::new(NullLog)`).
- Kịch bản: mọi cảnh báo "Recycle refused", "restore refused", lỗi đọc cài đặt Thùng rác (dòng 136, 142, 149, 182, 225 của `recycle_bin.rs`) bị bỏ; người dùng báo "không xoá được" thì không có dấu vết phân biệt `TooLarge`/`BinDisabled`/`Unknown`. Lưu ý thêm: `GlobalLog` (`startup/graph.rs:93-113`) chuyển tới `AppLog`, mà `AppLog` mặc định tắt (`settings.logging_enabled`), nên ngay cả sau khi sửa, `warn` chỉ ghi khi người dùng bật log; từ chối xoá liên quan an toàn dữ liệu nên nên dùng `error_forced` hoặc ít nhất `warn` có đường ghi bắt buộc.
- Test hiện có: không (không test nào kiểm facade production nhận log). `tests/composition_registry.rs` dựng `PlatformServices` bằng fake.
- Đề xuất: thêm `WindowsRecycleBin::with_log(restore_backend, log)` và gọi từ `platform.rs:353`; thêm test ở `pr-shell` khẳng định facade production ghi một dòng vào log giả khi bị từ chối (dùng `PlatformOverrides` hoặc một bin tiêm adapter).

### M3 (Trung bình, parity C#) — hộp lỗi khởi động hiện ra khi khoá instance vẫn giữ và pipe vẫn nghe: yêu cầu mở file từ tiến trình sau bị nuốt

- File:line: `crates/pr-app/src/startup/shutdown.rs:68-78` (`run_fail_startup`: `log_failure`, `show_failure_box` rồi mới `force_exit_code`/`dispose_main_window`/`quit`; `stop_listening` và `dispose_instance_scope` chỉ chạy ở `run_shutdown`, sau khi vòng lặp thoát), `crates/pr-shell/src/startup/shutdown.rs:108-131`, `crates/pr-shell/src/startup/run.rs:310-313`. C# `ShellStartup.cs:218-251` cùng thứ tự.
- Kịch bản: khởi động lỗi sau khi đã lấy khoá (ví dụ `attach_renderer` hết GPU, `StartupError::Overlay`, view model lỗi). Hộp lỗi modal mở; người dùng bỏ đó vài phút. Trong lúc đó mọi lần mở ảnh khác đều thấy mutex bị giữ, chuyển đường dẫn "thành công" (kết quả `Delivered` hoặc `Unknown` tính là đã giao, `pr-core/src/instance/handoff.rs:20,53`) tới một tiến trình sẽ chết; router không có handler nên `pending` (`forwarded_open.rs:139-150`) giữ một đường dẫn rồi bỏ. Người dùng nhấp đúp ảnh và không thấy gì.
- Test hiện có: `tests/startup_run_native.rs:56,74,390` kiểm hộp lỗi + mã thoát 1, không kiểm khoá được thả trước hộp; `pr-app/tests/startup_shutdown.rs` kiểm thứ tự bằng fake.
- Đề xuất: thêm bước `stop_listening` (và có thể `dispose_instance_scope`) vào đầu `run_fail_startup` trước `show_failure_box`; cập nhật test thứ tự. Đây là thay đổi lệch C# có chủ đích, cần ghi vào bảng Different của PARITY (orchestrator quyết).

### M4 (Trung bình) — `version` của Settings luôn `None` ở production; kiểm tra cập nhật luôn "invalid version"

- File:line: `crates/pr-shell/src/startup/composition_root.rs:76,177` (`ShellServicesOptions::version`, mặc định `None` vì `options.rs` dùng `ShellServicesOptions::default()`), `startup/secondary_dialogs.rs:219` (`version: self.host.version.clone()`), `crates/pr-platform-win/src/dialogs/settings/dialog/frame.rs:138` ("`None` = empty text"), `crates/pr-core/src/updates/checker.rs:244-245` (`AppVersion::try_parse(None)` thất bại). `crates/pr-app/src/misc/build_info.rs` có đủ mã tạo dòng phiên bản nhưng không có nơi nào ngoài chính file đó và test gọi (`grep build_info::` trống).
- C#: `SettingsDialog.cs:135` `Options.Version ?? BuildInfo.GetVersion(assembly)`. Rust không có nhánh thay thế tương đương.
- Kịch bản: mở Settings: dòng phiên bản dưới danh sách trống; bấm "Kiểm tra cập nhật": `CheckForUpdate` nhận `None`. Hiện vô hại vì checker không có feed (`composition/update_http.rs:9`), nhưng khi bật feed thì mọi lần kiểm tra báo lỗi "invalidVersion".
- Test hiện có: `tests/startup_secondary_dialogs.rs:66` dùng `version: None` có chủ đích; không test khẳng định giá trị mặc định production.
- Đề xuất: ở nơi dựng `ShellServicesOptions` mặc định (hoặc trong `ProductionSecondaryWindowDialogs`) điền `build_info` từ `env!("CARGO_PKG_VERSION")` và build time; thêm test khẳng định `DialogHostParts.version` production khác rỗng. PARITY: hàng `BuildInfo`/Settings version cần kiểm lại (liệt kê cho orchestrator).

### M5 (Trung bình, độ chắc thấp) — không có bước flush/dispose tường minh cho `SessionWriter` khi tắt; dựa vào `Drop` của Arc cuối trước `process::exit`

- File:line: `crates/pr-shell/src/startup/shutdown.rs:81-86` (`dispose_services` chỉ `drop(graph)` rồi `drop(services)`), `crates/pr-shell/src/entry.rs:55-63` (`process::exit(code)` ngay sau `run`), `crates/pr-core/src/session/writer.rs:34,708,740` (đợi tối đa 2 s ở `dispose`/`Drop`). `SharedServices` nằm trong `Arc` và được các luồng nền giữ: `LazyPrewarmPreviews` (`composition/adapters.rs:147-191`) được luồng `PhotoReview.EarlyDecodePlan` và `PhotoReview.EarlyDecodeStart` (`phase_one.rs:341-353`, `run.rs:259-265`) cầm; bộ lập lịch preload cũng có thể cầm (chưa kiểm).
- Kịch bản (suy luận, chưa chạy): mở một ảnh RAW lớn rồi đóng ngay trong lúc giải mã đầu tiên còn chạy. Arc cuối của `SharedServices` (do đó của `SessionWriter`) chưa rơi khi `run` trả về; `process::exit` bỏ qua `Drop`, ghi phiên đang chờ (vị trí xem gần nhất) có thể mất. Không ảnh hưởng journal (ghi đồng bộ, INV-6) nhưng khác C# (`ServiceProvider.Dispose()` luôn dispose writer).
- Test hiện có: `tests/startup_run_native.rs:461` (không rò cửa sổ/dispatcher), `:755` (thứ tự cửa sổ trước container); không test về flush phiên.
- Đề xuất: giữ một `Arc<SessionWriter>` trong `StartupGraph` (hoặc lấy từ `services.session_writer()` trong `dispose_services`) và gọi `dispose()` tường minh trước khi thả graph; thêm test với writer giả đếm lần dispose, kể cả khi còn một Arc ngoài.

### M6 (Trung bình, độ chắc thấp) — `config.json` hỏng được đặt lại về mặc định mà không báo người dùng

- File:line: `crates/pr-core/src/settings/store.rs:312-323` (parse lỗi: ghi log "Corrupt config.json detected, resetting to defaults", sao lưu `.corrupt`, rồi `persist` mặc định ở dòng 337-348); `crates/pr-app/src/startup/settings_load.rs` `repair_notice` chỉ đọc `last_load_repairs`, mà nhánh parse lỗi không thêm mục nào; callback lỗi khởi động chỉ ghi `AppLog::error_forced` (`composition/registry.rs:208-226`), ra file log, không ra hộp.
- Kịch bản: `config.json` bị cắt cụt (mất điện), mọi phím tắt, thư mục Move/Copy, hồ sơ action quay về mặc định; bản sao còn trong `config.json.corrupt*` nhưng người dùng không biết. Chưa đối chiếu hành vi C# cho nhánh toàn file hỏng (C# có `LastLoadRepairs` cho giá trị lẻ, `SettingsStore.cs:87-120`); nếu C# cũng im lặng thì chỉ là parity.
- Test hiện có: test `SettingsStore` ở `pr-core` (ngoài phạm vi lane này) và `tests/startup_phase_one.rs`; không test nào kiểm "người dùng được báo".
- Đề xuất: khi nhánh hỏng xảy ra, thêm một mục vào `last_load_repairs` (hoặc cờ riêng) để `repair_notice` hiện hộp "đã khôi phục từ bản sao". Cần quyết định sản phẩm.

### L1 (Thấp) — `install_process_hooks` chạy sau khi các luồng warm-up đã khởi động

- `crates/pr-shell/src/startup/run.rs:81-88` (warm settings parser, warm graphics) so với `:121-123` (`install_process_hooks`). Panic trong luồng warm trước khi hook cài chỉ ra hook mặc định (GUI subsystem: không đâu thấy). Cửa sổ vài ms. Đề xuất: cài hook trước bước 3 của `run`. Test: `startup_run_native.rs:156` kiểm hook sau khi cài, không kiểm thứ tự.
- Cùng chỗ: tham số `log` của `install_process_hooks` bị bỏ (`run.rs:177` `let _ = log;`); hook luôn dùng `AppLog::global()`. Vô hại, nhưng chữ ký gây hiểu nhầm.

### L2 (Thấp) — tên file chứa ký tự không hợp lệ UTF-16 (surrogate lẻ) bị mất khi chạy từ dòng lệnh

- `crates/pr-shell/src/entry.rs:25-31` (`to_string_lossy` thay surrogate lẻ bằng U+FFFD) → `crates/pr-app/src/startup/launch.rs:64-71` (`file_exists` thất bại), nên đối số bị bỏ im lặng, cửa sổ mở trống. Test hiện có cố ý khẳng định phép thay thế này (`tests/entry_args.rs:30`); C# chuỗi .NET giữ nguyên surrogate nên mở được. Rất hiếm (tên file kiểu này không tạo được bằng Explorer, nhưng NTFS cho phép). Đề xuất: giữ `OsString` đến `ShellLaunch` (ghi vào PARITY như Different nếu chấp nhận).
- Tên Unicode thường, UNC, `\\?\`: `launch_args.rs:35-60` + `dotnet_path::get_full_path`/`get_path_root` (đã đọc `get_path_root`, xử lý `\\?\`, `\\?\UNC\`, `\\server\share`; `\\?\` trả về nguyên văn) không thấy lỗi. Thiếu test ở mức `ShellLaunch::parse` cho ca `\\?\C:\Ảnh\x.jpg` và `\\server\share\Ảnh` (grep chỉ thấy `UNC` trong `pr-app/tests/misc_launch_args.rs` ở mức `normalize_one`).

### L3 (Thấp) — biến môi trường `PHOTOREVIEW_RS_*` đọc rải rác với quy tắc khác nhau

| Biến | Nơi đọc | Quy tắc |
|---|---|---|
| `INSTANCE_PREFIX` | `pr-app/src/startup/options.rs:61-64` | `var_os` lossy, không rỗng; khoảng trắng giữ nguyên, sau đó `InstanceScope` từ chối (parity C#) |
| `WARP`, `NOWARM` | `options.rs:65-66` | đúng `"1"`, không trim, không nhận `true` |
| `ISOLATE_CONFIG` | `pr-core/src/app_paths.rs:60,86-91` | `var()` (UTF-8 chặt), trim, `1` hoặc `true` |
| `DATA_ROOT` | `app_paths.rs:59` | `var()`: giá trị không phải Unicode bị bỏ qua và dữ liệu rơi về `%LOCALAPPDATA%` |
| `DIAG_*` | `pr-core/src/diagnostics/diag_options.rs:67` | `var_os` lossy, nhiều quy tắc riêng |
| `GPU_KEEPWARM*`, `FRAME_LOG` | `pr-platform-win/src/shell_viewing/keep_warm.rs:299-301` | `var()`; keepwarm tắt khi `0/off/false/no` |
| `GPU_ADAPTER` | `pr-platform-win/src/render/device/create.rs:167` | `var()`, chỉ số thập phân |

Hệ quả: `PHOTOREVIEW_RS_WARP=true` không có tác dụng trong khi `PHOTOREVIEW_RS_ISOLATE_CONFIG=true` có; `DATA_ROOT` chứa surrogate lẻ lặng lẽ ghi vào dữ liệu thật (quan trọng cho cô lập test, AGENTS.md). Ngoài ra `WindowsLiveOperationRegistry` luôn dùng `pr_core::identity::INSTANCE_PREFIX` (`composition/platform.rs:346-349`) chứ không dùng `ShellStartupOptions::instance_name_prefix`, nên hai tiến trình chạy với hai prefix khác nhau (kiểu test) vẫn chia chung dấu live-operation. Đề xuất: một helper `env_flag`/`env_string` dùng chung; `DATA_ROOT` dùng `var_os` và báo lỗi nếu không phải Unicode. Test: `tests/startup_options.rs:19` phủ 3 biến đầu bằng `EnvSource` giả.

### L4 (Thấp) — `LazyDecoderFactory::resolve` và `start_viewer_preview` panic khi registry trả `Err`

- `crates/pr-shell/src/composition/adapters.rs:184-190,214-224`. Comment nói "không thể xảy ra", nhưng `decoder_factory()` phụ thuộc `source_bytes_policy()?` và kết quả `Err` bị `OnceLock` nhớ vĩnh viễn (`registry.rs:117-121`). Nếu lỗi xảy ra, mở Settings gọi `create`/`is_registered` trên UI thread trong vòng lặp lồng của hộp thoại: panic bị `catch_unwind` của dispatcher/vòng lặp chính bắt, hộp thoại biến mất không thông báo. Test: `tests/composition_adapters.rs` (tên không thấy ca `Err`). Đề xuất: trả lỗi có kiểu lên `DialogServices` (hiện chỉ cần `is_registered` = false và log forced).

### L5 (Thấp) — hộp thoại Settings/Recovery tạo lỗi thì chỉ có `log.error` (không bắt buộc)

- `crates/pr-shell/src/startup/secondary_dialogs.rs:230-234` ("Settings dialog could not be created" rồi `None`); tương tự Recovery (`:238-`) qua `try_read_recovery_entries`. Người dùng bấm bánh răng mà không thấy gì; log mặc định tắt nên không còn dấu vết. Đề xuất: `error_forced` hoặc `show_error`. Test: `tests/startup_secondary_dialogs.rs` (không có ca `SettingsDialog::new` lỗi).

### L6 (Thấp) — bốn lệnh overlay dùng `borrow()` có thể panic

- `crates/pr-shell/src/startup/overlay_vm.rs:172-196` (`show_skipped_files`, `show_recovery`, `show_diagnostics`, `show_benchmark`): `self.view_model.borrow()` panic nếu một lệnh `update_view_model` đang giữ borrow (cùng lớp lỗi `overlay_model`/`settings` đã sửa bằng `try_borrow`). Hiện khó chạm vì cửa sổ chính bị vô hiệu khi modal mở, nhưng phím tắt hoặc nút overlay được đẩy qua hàng đợi sau modal có thể chạm. Test: `tests/startup_overlay_vm.rs:103` kiểm "borrow chung", không kiểm "đang bị mượn mutable". Đề xuất: `try_borrow` và bỏ qua hoặc xếp hàng.

### L7 (Thấp) — cửa sổ đã `ShowWindow` trước khi dựng view model; lỗi ở giữa để lại HWND ẩn bởi cloak

- `crates/pr-shell/src/startup/window.rs:183` (`show_command`), `:201` (`factory()?`), `:233-250` (`MainWindow::new`), `:264-265` (`attach_renderer`, `attach_overlay` dùng `?`). Nếu một bước thất bại, `window` (`Rc<ShellWindow>` cục bộ, `self.main` còn trống ở `factory()?`) bị thả khi hàm trả `Err`; nhánh `Closed` gọi `MessageLoop::quit(0)` nhưng `forced_exit=1` thắng (`shutdown.rs:133-136`), mã thoát đúng. Không thấy rò tài nguyên (cloak và frame clock nằm trong state, bị thả ở `run_shutdown`). Chỉ cần lưu ý thứ tự này đúng chủ đích (comment `window.rs:5-7`). Test: `startup_run_native.rs:390` phủ lỗi sau khi cửa sổ tồn tại.

### L8 (Thấp) — `localization()` rơi về đường dẫn tương đối nếu `current_exe()` lỗi

- `crates/pr-shell/src/composition/registry.rs:271-275`: `.ok().and_then(..).unwrap_or_default()` rồi `.join("Languages")` cho `shipped_dir = "Languages"` tương đối theo cwd; C# dùng `AppContext.BaseDirectory`. Rất hiếm (`current_exe` lỗi), hậu quả là ngôn ngữ đóng gói không nạp được, rơi về tiếng Anh nội trang. Đề xuất: ghi `warn` và dùng thư mục chứa `argv[0]`. Test: không.

## Kiểm đã làm và không thấy lỗi

- **INV-4 (cổng thao tác file chung)**: `FileActionService` và `UndoService` đều là `OnceLock` đơn trong `SharedServices` (`composition/registry.rs:316-345`); `UndoService::with_gate(file_action_service())`, `RecoveryRetryService` nhận `Some(file_action_service())` (`registry.rs:348-362`, `factories.rs:41-60`); `FileActionController` và `DuplicateCleanupController` dùng chung một `Arc<FileActionServiceExecutor>` (`main_view_model.rs:306-345`); `FileActionGate` tầng UI (xếp hàng) chỉ có một thể hiện `FileActionGate::new(host.ui_scheduler)` (`main_view_model.rs:402`). Mọi `SharedServices::clone()` chia sẻ một `Arc<Inner>` nên `graph.view_model`, `dialog_services_of` và `LazyDecoderFactory` cùng một thể hiện. Không có `FileActionService::new`/`UndoService::new`/`RecoveryRetryService::new` nào khác ngoài `registry.rs`/`factories.rs` trong `pr-shell`, `pr-app`, `pr-platform-win`. Thiếu: không có test `Arc::ptr_eq` giữa cổng của `FileActionController` trong view model thật và `services.file_action_service()` (chỉ có test cổng cho recovery retry ở `composition_registry.rs:316`); view model giữ executor ở trường riêng tư nên khó kiểm, nên cân nhắc test hành vi: giữ `try_begin` trên `file_action_service()` rồi thử Move qua view model, kỳ vọng "busy".
- **Thứ tự tắt**: `run_shutdown` khớp C# `Cleanup` (`pr-app/src/startup/shutdown.rs:36-47` so `ShellStartup.cs:667-684`); `dispatcher.shutdown()` đặt cuối (`run.rs:160-163`); các `ui_hop` giữ `Weak<StartupState>`. `dispose_services` thả graph trước services theo chủ đích.
- **Khởi động lỗi giữa chừng**: `services` được lưu vào state trước khi chạy `graph_builder` (`phase_one.rs:264`), nên hộp lỗi dùng được `dialog_service`; `coalescer`/`instance` lưu vào state ngay khi tạo (`phase_one.rs:419-423`), `run_shutdown` thả; spawn luồng thất bại làm rơi `sender` nên ra `HopLost`, không treo (`phase_one.rs:331-332`, `:467-479`); `Win32UiDispatcher::new`/`MessageLoop::new` lỗi trả `EXIT_STARTUP_FAILED` và `dispatcher.shutdown()` (`run.rs:90-104`). Test: `startup_phase_one.rs:11,32`, `startup_run_native.rs:38-74,390,461`.
- **Singleton thứ hai**: `try_take_lock` → `forward_to_owner` ở luồng `PhotoReview.Handoff` (timeout 1,5 s); `Err(Canceled)` → `Ok(false)` (`instance.rs:78-85`); kết quả `Unknown` coi là đã giao theo Q-R12. Yêu cầu tới trước khi cửa sổ sẵn sàng được giữ một đường dẫn trong router (`forwarded_open.rs:139-150`: chỉ một đường dẫn, "đầu thắng", parity C#) rồi phát lại ở `phase_two` bước 4. Test: `startup_run_native.rs:354`, `startup_instance.rs:13`.
- **Cổng đồng thời với khởi động**: tái hòa giải journal chạy sau khoá và sau khi cửa sổ hiện (`recovery.rs:26-93`); không nhận được dấu hiệu tranh chấp với thao tác người dùng đầu tiên (journal tự có khoá riêng, ngoài phạm vi lane này). Test thứ tự: `startup_run_native.rs:722`.
- **panic/unwrap trên dữ liệu người dùng**: không có `unwrap()`/`expect()` ở mã không-test của `pr-shell`; chỉ hai `panic!` (L4) và `unwrap_or_default` vô hại. Vòng lặp UI bắt panic tối đa `MAX_LOOP_FAULTS=50` lần rồi thoát mã 1 (`run.rs:134-155`).
- **`NullLog` còn lại**: `factories.rs:161` (`log.unwrap_or_else(NullLog)` chỉ khi `wpf_fallback` có; `registry.rs` luôn truyền `Some(log())`), các `NullLog` khác trong `pr-shell/src` chỉ ở test. Ngoài `pr-shell` còn nhiều chỗ khác (`grep NullLog` ra 20+ file); chỉ `recycle_bin.rs` là production được nối từ composition root (M2).

## Hàng PARITY cần orchestrator xem

- Hàng của `BuildInfo` / phiên bản trong Settings và kiểm tra cập nhật: mã đã port nhưng chưa nối (M4, bằng chứng `grep build_info::` chỉ ra chính file).
- Hàng `ShellStartup` fail-path: thêm ghi chú Different nếu chấp nhận hộp lỗi hiện trước khi thả khoá (M3) hoặc ghi quyết định sửa.
- Hàng `WindowsRecycleBin`: đã ghi ở `ST-COM-RECYCLE-2026-10-31.md` (T5); lane này xác nhận T5 còn mở tại `a61cb3aa`.

## Thứ tự ưu tiên sửa đề xuất

1. M2 (rẻ nhất, một constructor), 2. M4 (một dòng cấu hình + test), 3. M1 (hàng đợi mở file khi modal), 4. M3, 5. M5, 6. L-series gộp một PR "shell hygiene" (L1, L3, L5, L6).

## Test đã đọc tên (không chạy)

`entry_args`, `startup_options`, `startup_phase_one`, `startup_instance`, `startup_forward_target`, `startup_overlay_vm`, `startup_run_native`, `startup_composition_root`, `startup_secondary_dialogs`, `composition_registry`, `composition_graph`, `composition_adapters`, `composition_main_view_model`, `composition_factories`, cộng `pr-app/tests/startup_shutdown.rs`, `misc_launch_args.rs`.
