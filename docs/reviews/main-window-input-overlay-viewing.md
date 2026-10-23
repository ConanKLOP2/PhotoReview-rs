# Review tĩnh: cửa sổ chính, input/menu, overlay, viewing (W8-13g..13k, 14a..14g, 15f..15j, 16b..16i)

Machine C (cloud). Phạm vi đọc: phần ĐÃ cài đặt của
- cửa sổ chính (13g..13k): `crates/pr-platform-win/src/host/main_window/{core,lifecycle,frame,viewing,windowing,scrollbars}.rs`
- input (14a..14d): `shell_input/{native,handlers,click_counter,dispatch,surface,drop_target,main_window,environment}.rs`
- menu (14e..14g): `shell_menus/{controller,mapping,message_handler,owner_draw_style,popup_host,owner_draw,main_window}.rs`
- overlay (15f..15j): `shell_overlay/{host,messages,native,tooltips,main_window}.rs`, `shell_overlay/overlay/{state,input,paint,autohide}.rs`
- viewing (16b..16i): `shell_viewing/{compare,crossfade,composer,prefetcher,keep_warm,frame_log,frame_stats,pacer,presentation_sink,viewport_controller,scrollbar_native}.rs`, `shell_viewing/scrollbars/{state,input,paint}.rs`
- test: `crates/pr-platform-win/tests/{host_main_window_*,shell_input_*,shell_menus_*,shell_overlay_*,shell_viewing_*}.rs` và `tests/common/*fixture*.rs` (đọc có chọn lọc, và một script đếm test không có `assert`).

So với C# tại `7eaee2a2` (`ShellMainWindow*.cs`, `Input/ShellInput.cs`, `Overlay/ShellMainWindow.Overlay.cs`, `Overlay/DeferredTooltips.cs`, `Menus/ContextMenuMessageHandler.cs`, `PhotoReview.App.Shared/ViewModels/MainViewModel.cs`).

**Mọi phát hiện dưới đây: suy luận từ mã, chưa chạy thử.** Không chạy cargo, không chạy test. Số dòng lấy bằng `grep -n` trên cây `claude/review-b4` (từ master `c40b59d`).

**Bỏ qua (`todo!()` hoặc ngoài phạm vi):** `shell_input/main_window.rs:266` (`OpenInDefaultEditor`, xem M1 vì nhánh này đi tới được), `shell_menus/main_window.rs:226,230,234` (ba hàm W9-E9), toàn bộ `pr-shell/src/startup/{window,overlay_vm}.rs` (17e chưa cài; chỉ đọc phần doc để biết ý đồ thiết kế). Chưa đọc kỹ: `shell_overlay/overlay/{hit_test,layout}.rs`, `shell_viewing/scrollbars/layout.rs`, `host/shell_window/*` và `host/frame_clock/*` (đơn vị 13a..13f, ngoài phạm vi), `render/*`.

**Ghi chú về R0003.** ADR nói "UI thread does not catch panics", còn yêu cầu review hỏi `catch_unwind` ở ranh giới FFI. Trong phạm vi này mọi ranh giới FFI đều đã có chốt chặn: `extern "system"` của cửa sổ đi qua `pending_panic::guarded` (đọc `host/wndproc.rs`, `host/ui_dispatcher.rs:416`), COM `IDropTarget` đi qua `com_guarded` (`drop_target.rs:287`), `subclass_proc` đi qua `run_guarded` (`owner_draw.rs:206`). Hệ quả đã dùng ở H1: một panic bị `guarded` bắt chỉ được ném lại bởi `MessageLoop` sau `DispatchMessage` (`message_loop.rs:380,383`) hoặc bởi `NativePopupMenuHost::show` (`popup_host.rs:280`), KHÔNG bởi vòng lặp lồng nhau do OS sở hữu (`IFileDialog::Show`, `TaskDialog`).

Tổng: 3 cao / 2 trung bình / 5 thấp.

## Đã đọc và thấy đúng
- Khối `unsafe`: quét bằng script (mỗi dòng `unsafe` phải có `SAFETY` trong 5 dòng trước), 0 khối thiếu `// SAFETY:` trong phạm vi; riêng `unsafe extern "system" fn subclass_proc` (`owner_draw.rs:226`) là khai báo hàm, các khối bên trong đều có `SAFETY`.
- `drop_target.rs`: `MediumGuard` giải phóng `STGMEDIUM` đúng một lần, `LockGuard` mở khoá trên mọi đường, `parse_drop_files` kiểm biên theo `GlobalSize` (offset `fWide` 16, độ dài header 20 đúng `DROPFILES`), mọi phương thức COM qua `com_guarded`; `RegisterDragDrop` rồi `RevokeDragDrop` đối xứng, `dispose` idempotent.
- `owner_draw.rs`: `Rc::into_raw`/`from_raw` cân bằng (kể cả khi `SetWindowSubclass` thất bại), subclass gỡ trước rồi mới nhả tham chiếu; mọi bút/cọ GDI tạo trong `draw_item`/`stroke_lines`/`fill_solid` đều được xoá; font stock không bị xoá (`font_is_stock`); `DcRelease` trả DC trên mọi đường.
- `popup_host.rs`: submenu chưa gắn được huỷ bởi `MenuGuard`, root luôn `DestroyMenu` trong khối có `ShowingGuard`; session (font, brush, subclass) sống lâu hơn menu.
- `tooltips.rs`: `cbSize = size_of::<TTTOOLINFOW>()`, bộ đệm UTF-16 sống hết cuộc gọi `TTM_ADDTOOL`, bộ đệm `TTM_GETTEXT` đúng cỡ 512; không giữ `RefCell` qua `send_message`.
- `frame.rs`: `PresentingGuard` đặt lại cờ cả khi panic; `surface` mượn từng bước bằng `try_borrow*`, không giữ qua lời gọi layout/composer/overlay ngoài đoạn vẽ; `draw_presentation` chỉ mượn view model rất ngắn.
- `composer.rs`, `crossfade.rs`, `presentation_sink.rs`, `scrollbars/input.rs`: các handler được sao ra khỏi `RefCell` trước khi gọi; thứ tự `begin_draw_context` → `end_draw_and_present` luôn cân bằng khi đã bắt đầu.
- `windowing.rs`: `ShellFullscreen::apply` idempotent, `refit` dùng `try_borrow` nên không panic khi vào lại trong `SetWindowPos`.
- `dispatch.rs`: thứ tự quyết định của `handle_key_down_impl` khớp `ShellInput.HandleKeyDown` (C# dòng 183-230: mũi tên pan, Esc popup, Esc huỷ băm trùng, Space/Enter của nút, router); `WM_CAPTURECHANGED` trong lúc `run_pointer` được nhớ và phát lại một lần (`BusyScope`, `replay_capture_lost`).
- `click_counter.rs`, `handlers.rs`: số học `wrapping_sub`/`abs() <= slop/2`, tách word có dấu; keyboard handler nuốt `WM_KEYUP`/`WM_CHAR` của phím đã xử lý như C#.
- `fan_out_changes`: thứ tự `image_changing` → ảnh → tiêu đề → chỉ số → viewer → compare → feature khớp lời giải thích; `pump_view_model` có `ClearOnDrop` và cờ `pump_again` cho vòng lặp lồng nhau.

## Phát hiện

### H1 (cao, xác nhận bằng cách đọc đường đi; hệ quả khi chạy là suy luận): `RefCell` của view model bị giữ qua hộp thoại modal, mọi lần vào lại (vẽ, bơm, đóng, mở file chuyển tiếp) panic
- Mã: b4-main-window H1. Đơn vị: W8-13h (chủ), W8-14c, W8-14f, W8-13j.
- Chuỗi giữ borrow:
  - `host/main_window/lifecycle.rs:324-330` `update_view_model`: `borrow_mut()` (dòng 326) giữ trong suốt `f(&mut view_model)`.
  - `shell_input/main_window.rs:584-599` `with_view_model` (`try_borrow_mut` dòng 587) cũng giữ borrow trong `f`; `shell_menus/main_window.rs:319-334` y hệt.
  - `f` có thể là lệnh mở hộp thoại modal đồng bộ: `pick_and_open_folder` (`pr-app/src/viewmodels/main/commands.rs:642-644`, `dialog_service.pick_folder`; nhánh Rust `IFileOpenDialog::Show`, `dialogs/folder_dialog.rs:104`), `show_settings_at` (`session.rs:54`), `show_recovery/diagnostics/skipped_files/benchmark` (`commands.rs:736-753`). Người gọi: phím mở thư mục (`shell_input/main_window.rs:249-251`), mục "Settings" của menu chuột phải (`shell_menus/main_window.rs:244-246`), nút Open/Settings của overlay (qua `OverlayViewModel`, ý đồ 17e trong `pr-shell/src/startup/overlay_vm.rs` ghi rõ "`window.update_view_model(|vm| vm.pick_and_open_folder())` ... a nested message loop inside the call").
- Trong vòng lặp lồng nhau của hộp thoại, các việc sau chạy trên cùng luồng khi view model đang bị mượn `&mut`:
  - khung hình: `present_frame` → `draw_presentation` → `self.view_model.borrow()` (`viewing.rs:546`) → `BorrowError`; tương tự `on_image_changing` (`viewing.rs:395`), `on_navigation_changed` (`viewing.rs:460`), `update_retained` (`viewing.rs:486`), `path_at` (`viewing.rs:105`, từ `run_retry`).
  - bơm completion: `pump_view_model` dùng `borrow_mut()` thường (`lifecycle.rs:346`), cờ `pumping` chưa bật vì lệnh mở hộp thoại không đi qua pump.
  - đóng cửa sổ (thanh tác vụ, tắt máy): `on_closing` → `view_model.borrow()` (`lifecycle.rs:445`).
  - mở file chuyển tiếp từ bản thứ hai: `MainWindow::open_path` → `update_view_model` (`lifecycle.rs:262,326`).
- Kịch bản: người dùng bấm `O` (mở thư mục) → hộp thoại thư mục modal; trong lúc đó (a) mỗi tick khung hình panic `BorrowError`; nếu tick đến qua một việc đã đăng thì `Win32UiDispatcher::execute` bắt và ghi log (`ui_dispatcher.rs:416`), nếu đến qua thông điệp cửa sổ thì `guarded` giữ payload như ở (d) (tôi không lần hết đường phát tick của `frame_clock`); dù đường nào, cửa sổ chính không vẽ lại khi hộp thoại bị kéo qua; (b) một kết quả preload hoàn tất gọi wake → `pump_view_model` panic và mất lượt bơm (cờ `queued` đã xoá, completion nằm chờ tới wake sau); (c) double-click ảnh trong Explorer → open chuyển tiếp bị bỏ (panic, chỉ có log); (d) bấm Đóng trên thanh tác vụ → `WM_CLOSE` → panic trong `on_closing`, `guarded` giữ payload và trả 0 nên cửa sổ không đóng; payload chỉ được ném lại ở `DispatchMessage` kế tiếp của `MessageLoop` ngoài (vòng lặp của `IFileDialog` không gọi `rethrow_pending`) → tiến trình chết SAU khi đóng hộp thoại, xa nguyên nhân. C# không có vấn đề này (không có `RefCell`).
- Các nhánh "bận" im lặng làm sai thêm: `MainWindowInputHost::query` trả `None` → `has_images()` thành `false` (`shell_input/main_window.rs:129-136`), `MainWindowMenuActions::read` trả `Default` (`shell_menus/main_window.rs:127-134`), `ScrollBars` coi "bận" là "không hiện compare" (`scrollbars.rs:56-59`).
- Vì sao test không bắt: `host_main_window_*` có 82/120 test `#[ignore]` (xem M2); fixture `DialogService` trong test không bao giờ chạy vòng lặp lồng nhau; `task_on_progress.md` dòng 25 ghi "14b/13h RefCell risk" ở nhánh `fix/w8-b4-integration` chưa merge, nhưng không nêu đường hộp thoại này.
- Gợi ý: không giữ borrow qua hộp thoại. Cách gọn: lệnh trong view model trả "yêu cầu hộp thoại" (enum) cùng `ChangeSet`, cửa sổ nhả borrow rồi mới mở hộp thoại và đưa kết quả vào lệnh thứ hai; hoặc `DialogService` chạy trên hàng đợi sau khi `update_view_model` trả về. Phòng thủ bổ sung: `pump_view_model`, `on_closing`, `draw_presentation` dùng `try_borrow*` và hoãn (đặt `pump_again`, bỏ khung) thay vì panic.
- Test nên thêm (không cần HWND): một `MainViewModel` thật với `DialogService` giả mà `pick_folder` gọi lại `pump_view_model()` / `draw_presentation` / `on_closing` của cửa sổ giả qua `Rc<RefCell<MainViewModel>>`; kỳ vọng không panic và lệnh mở file chuyển tiếp trong lúc đó được xếp hàng chứ không mất. Thêm test native (một hộp thoại giả modal dựng bằng `DialogBoxIndirect`/`MessageBox` hẹn giờ) cho `update_view_model` + `render_now`.

### H2 (cao, xác nhận bằng cách đọc; chính fixture test thừa nhận): `viewer.borrow_mut()` giữ suốt cuộc gọi controller con trỏ, trong khi `update_layout` đọc lại viewer
- Mã: b4-input H1. Đơn vị: W8-14b (chủ), W8-13h, W8-16h.
- `shell_input/dispatch.rs:641-665` `run_pointer`: `let mut viewer = self.deps.viewer.borrow_mut();` (dòng 647) giữ qua toàn bộ `f(&mut state.pointer, &mut ctx)`. Trong `f`, mọi đường zoom có neo đều gọi `scroll_anchor_to` → `surface.update_layout()` (`pr-app/src/input/pointer/zoom.rs:337-343`, được gọi từ `zoom_to_image_point` dòng ~251 và hai nhánh `resume` dòng ~282/292). `ViewportImageSurface::update_layout` → `ViewportController::update_layout` → `run_layout` (`shell_viewing/viewport_controller.rs:280-284`) gọi `(self.content)()` (dòng 283) và `(self.update_fit_size)()` (dòng 302).
- Trong sản xuất `content` là `MainWindowInner::content_from_viewer` (`lifecycle.rs:358-359`, `self.viewer.borrow()`) và `update_fit_size` là `MainWindowInner::update_fit_size` (`lifecycle.rs:376-379`, `self.viewer.borrow_mut()`). `ViewerHandle` bọc `Rc<RefCell<ViewerState>>` (`pr-app/src/viewmodels/main/state.rs:395`; doc nói `borrow_mut` "panics when already borrowed: a bug").
- Kịch bản: cuộn chuột (hoặc phím `+`, click-zoom, `ZoomActualSize`, phím "fit width") khi có ảnh → `run_pointer` → `scroll_anchor_to` → `content_from_viewer` → `BorrowError` panic trong `WM_MOUSEWHEEL`/`WM_KEYDOWN` → `guarded` giữ payload → `DispatchMessage` trả về → `rethrow_pending` → tiến trình chết ở thao tác zoom đầu tiên. Khi ảnh ở Fit và kích thước ảnh đổi, nhánh `update_fit_size` panic tương tự.
- Hợp đồng `W8-14b.md` mục D-W8B4I-1 nói "Nothing that can re-enter (host calls, `window.*`, posting) happens while a borrow is held": `surface.update_layout()` chính là lời gọi vào lại đó.
- Vì sao test không bắt: `tests/common/shell_input_dispatch_fixture.rs:9-14` nói rõ "The viewport's content callback does NOT read the `ViewerHandle`: `PointerContext::viewer` holds a `borrow_mut` for the whole pure call ... so a layout pass inside the call reads a cache (`Rig::refresh_content`)". Tức fixture né đúng xung đột mà mã sản xuất mắc phải; test E2E thật (`shell_input_e2e.rs:280` `wheel_zooms_at_the_cursor...`) bị `#[ignore = "native: needs desktop"]` và chỉ chạy với "temporary stand-ins" theo `task_on_progress.md` dòng 26.
- Gợi ý: trong `run_pointer`, đừng đưa `RefMut<ViewerState>` vào `PointerContext` cho cả cuộc gọi; hoặc làm `content_from_viewer`/`update_fit_size` chịu được viewer đang bị mượn (đọc từ ảnh chụp do `run_pointer` đặt trước, hoãn `update_fit_size` bằng `post_to_window` ở Render, như bản fixture đang mô phỏng). Cả hai phải khớp với giả định của `PointerInputController` (nó đọc offset ngay sau `update_layout`).
- Test nên thêm (không cần HWND): dựng `ShellInput` với `ViewportController` THẬT mà `content` là closure đọc `viewer.borrow()` đúng như `content_from_viewer` (thay `RigContent` của fixture), gọi `execute(ZoomIn)` và `handle_vertical_wheel` với con trỏ trong ảnh; kỳ vọng không panic và offset cuối khớp C#. Một test tương tự với ảnh ở Fit để chạm `update_fit_size`.

### H3 (cao, xác nhận không có đường gọi; ý đồ tích hợp chưa rõ): thay đổi zoom do con trỏ/phím không bao giờ tới presenter và HUD zoom
- Mã: b4-input H2. Đơn vị: W8-14b/14c (chủ), W8-15j, W8-13h.
- C#: `MainViewModel.cs` dòng 136 `_viewerState.ZoomModeChanged += ... _presenter.SetViewerZoom(_viewerState.EffectiveZoom)` và dòng 140-143 `PropertyChanged` của `DisplayZoomPercent` gọi `InfoOverlay.Refresh()`. Cả hai chạy khi BẤT KỲ nơi nào đổi viewer, kể cả `PointerInputController`.
- Rust: `PointerInputController` ghi thẳng vào `ViewerState` và gom `ViewerUpdate` (`pr-app/src/input/pointer/wheel.rs:173`). `run_pointer` chỉ đọc nó để dừng đà và vẽ lại (`dispatch.rs:674-680`: `zoom_mode_changed` → `stop_kinetic_impl`, `changes` không rỗng → `window.invalidate()`); `MainWindowInput::on_view_model_changes` cũng chỉ dừng đà (`shell_input/main_window.rs:410-414`). `MainViewModel::apply_viewer_update` (`state.rs:1319`, nơi gọi `presenter.set_viewer_zoom` và `refresh_overlay`) chỉ được gọi từ các lệnh zoom của chính view model (`commands.rs:414-447`), không từ shell. `apply_viewer_changes` của cửa sổ chỉ làm fullscreen + vô hiệu layout (`lifecycle.rs:104-112` qua `fan_out_viewer`). `OverlayModel` có `zoom_indicator_text` lấy từ `viewer.display_zoom_percent()` (`state.rs:1376,1406`), và `MainWindowOverlay::on_view_model_changed` bỏ qua `changes.viewer` (`shell_overlay/main_window.rs:423-433`).
- Kịch bản: zoom bằng cuộn chuột hoặc phím → (1) chỉ báo "150%" trên ảnh đứng yên ở giá trị cũ cho tới khi một thay đổi khác của view model làm overlay bẩn; (2) `ImagePresenter::set_viewer_zoom` không được gọi, nên bộ nạp chi tiết zoom (`zoom_detail`, bản "full-resolution khi phóng to", ADR 0008) không biết mức zoom mới: ảnh có thể ở độ phân giải preview khi xem 100%. (2) là suy luận từ cấu trúc: tôi không đọc `zoom_detail` tới cùng để chắc bộ nạp chỉ đổi theo `set_viewer_zoom`.
- Vì sao test không bắt: test của 14b/14c dùng viewer thật nhưng không có view model/presenter; test overlay đưa `OverlayModel` thẳng; không test nào ghép wheel → presenter.
- Gợi ý: `MainWindowInputServices` thêm "đưa `ViewerUpdate` cho view model" (gọi `apply_viewer_update` qua `update_view_model`, nhớ H1/H2 về borrow), hoặc `run_pointer` trả `ViewerUpdate` cho cửa sổ để `apply_changes` xử lý; `MainWindowOverlay::on_view_model_changed` nên đánh dấu bẩn khi `changes.viewer` chứa `DisplayZoomPercent`.
- Test nên thêm: ghép `ShellInput` + `MainViewModel` thật (presenter ghi lại): cuộn chuột zoom → kỳ vọng `set_viewer_zoom(Some(z))` được gọi đúng một lần và `overlay_model().zoom_indicator_text` đổi; kiểm lại với Fit (`None`).

### M1 (trung bình, xác nhận bằng cách đọc): `ReviewCommandType::OpenInDefaultEditor => todo!()` đi tới được bằng phím tắt cấu hình
- Mã: b4-input M1. Đơn vị: W8-14c (chủ), W9-E9.
- `shell_input/main_window.rs:266`. `ShortcutRouter` phát lệnh này khi người dùng gán phím cho `OpenInDefaultEditor` (`pr-core/src/settings/shortcuts.rs:72,109,130`: mặc định `""`, nhưng trang Settings cho gán, `settings_field_map.rs:145`, và `config.json` nhập từ C# có thể chứa nó). `handle_key_down_impl` → `execute_guarded` → `host.execute` → panic → `guarded` giữ payload → tiến trình chết ở lần bấm phím.
- Hệ quả tương tự trong menu: ba hàm `open_in_external_editor_by_id`, `add_external_editor`, `choose_default_external_editor` được cài `todo!()` và che luôn bản mặc định an toàn của trait (`shell_menus/controller.rs:71-80` trả `open_in_external_editor()`/`Ok(())`); hiện `ContextMenuController::execute` (`controller.rs:198-233`) chưa phát các id đó nên chưa tới được, nhưng ghép mục menu của W9-E9 sẽ làm panic ngay.
- Vì sao test không bắt: không test nào cho `ReviewCommand::OPEN_IN_DEFAULT_EDITOR` đi qua `MainWindowInputHost`.
- Gợi ý: tới khi W9-E9 xong, trả `Err(Box<dyn Error>)` "chưa hỗ trợ" thay vì `todo!()` để `execute_guarded` báo trạng thái; xoá override `todo!()` của menu (để trait mặc định chạy).
- Test nên thêm: `settings.shortcuts.open_in_default_editor = "E"`, bấm `E` qua `ShellInput`, kỳ vọng không panic và có dòng trạng thái "action failed"; test tương tự cho `ContextMenuActions` mặc định.

### M2 (trung bình, xác nhận bằng cách đếm): phần lớn test của cửa sổ chính và toàn bộ test tích hợp đều `#[ignore]`, và các fixture pure tránh đúng các xung đột `RefCell` thật
- Mã: b4-main-window M1. Đơn vị: W8-13g..13k, W8-14b/14c, W8-14f.
- Đếm bằng `grep` (có thể lệch vài đơn vị): `host_main_window_core/frame/lifecycle/viewing/windowing.rs` = 120 test, 82 `#[ignore]`; `shell_input_e2e.rs` 15 test, toàn bộ ignore; `shell_menus_e2e.rs` 17/18 ignore; `shell_menus_owner_draw.rs` 16/16; `shell_overlay_native.rs` 5/5; `shell_viewing_scrollbars_paint_native.rs` 4/4. Các test không ignore là phần thuần (bảng, hàm toán, `fan_out_changes` với recorder).
- Cùng với `test-quality.md` M1 (CI không chạy `--ignored`), nghĩa là H1 và H2 không có test nào chạy được trên CI. Và với H2, test thuần chạy được lại dùng cache nội dung để né xung đột (`shell_input_dispatch_fixture.rs:9-14`, `RigContent`); với H1 không có fixture nào cho `DialogService` chạy vòng lặp lồng nhau.
- Không tìm thấy test không `assert` kiểu giả trong phạm vi: script liệt kê 25 test không chứa `assert|expect|unwrap|verify|panic`, xem tay 8 mẫu đều gọi helper có assert (`close(...)`, `delay_case`, `zoom_preset_case`, `keep_zoom_across_images`, `text_index`) hoặc là kiểm tra biên dịch hợp lệ (`main_window_is_not_send`, `hub_and_sink_are_not_send`).
- Gợi ý: một job CI `--ignored` cho nhóm `native: needs desktop` chạy trên runner có desktop (hoặc WARP offscreen đã có), và đổi `RigContent` của fixture sang closure đọc `ViewerHandle` thật (H2). Bắt buộc trước khi merge nhánh `fix/w8-b4-integration`.
- Test nên thêm: xem H1 và H2 (hai test thuần không cần OS).

### L1 (thấp, xác nhận bằng cách đọc): `UiLocalPoster` tự động `Send` dù doc ghi `!Send`; đăng từ luồng khác làm mất việc
- Mã: b4-main-window L1. Đơn vị: W8-13g.
- `host/main_window/core.rs:510-512` `pub struct UiLocalPoster { dispatcher: Win32UiDispatcher }`: `Win32UiDispatcher` chứa `Arc<Shared>` (`ui_dispatcher.rs:162-169`, `Mutex`, atomics, `Arc<dyn Log>`) nên `Send + Sync` và `UiLocalPoster` cũng vậy; doc ở dòng 508 và hàm `new` nói "Clone, !Send", "must be created on the UI thread". `post` (`core.rs:544-548`) đẩy việc vào `LOCAL_JOBS` THREAD-LOCAL (`core.rs:379`) rồi đăng một job đánh thức `Send`.
- Kịch bản: một clone của `poster` (public trong `ViewportCallbacks.poster`) bị đưa sang luồng khác và gọi `post` → việc nằm trong hàng đợi thread-local của luồng đó (không bao giờ chạy), job đánh thức chạy trên UI thread tìm hàng đợi rỗng và bỏ qua; ngoài ra mất cặp đếm FIFO nếu hai luồng xen nhau.
- Chỉ có `main_window_is_not_send` (`tests/host_main_window_core.rs:156`) được ghim.
- Gợi ý: thêm `PhantomData<*const ()>` vào `UiLocalPoster` (như `NativeTooltipHost`, `tooltips.rs:84`); hoặc `debug_assert` đúng luồng trong `post`.
- Test nên thêm: `AmbiguousIfSend` cho `UiLocalPoster` (biên dịch) và một test native đăng từ luồng khác phải bị từ chối.

### L2 (thấp, xác nhận bằng cách đọc): luồng hẹn giờ không có chốt panic, lỗi tạo luồng bị bỏ qua
- Mã: b4-main-window L2. Đơn vị: W8-13g.
- `core.rs:466-488` `TimerQueue::run` gọi `(entry.action)()` (dòng 475) ngoài `catch_unwind`; một panic (ví dụ từ `Log::warn` của `Win32UiDispatcher::request_wake`, `ui_dispatcher.rs:402`) giết luồng, mọi `post_after` sau đó không bao giờ chạy: hạn chót `MAX_DEFER_MS` của composer (`composer.rs:154-166`) và các lần thử lại của prefetcher (`viewing.rs:148-168`). `core.rs:501-503` `let _ = std::thread::Builder...spawn(...)` bỏ qua lỗi tạo luồng, `DELAYED_JOBS` (`core.rs:531`) rò. R0003: "A worker thread runs its job inside `catch_unwind` and reports a panic".
- Hậu quả giới hạn (nâng cấp same-image vẫn promote khi upload xong qua `on_upload_completed`), nên mức thấp.
- Gợi ý: bọc `action` trong `catch_unwind` (ghi log), ghi log lỗi `spawn`.
- Test nên thêm: hàng đợi với một action panic rồi một action thường; kỳ vọng action thường vẫn chạy.

### L3 (thấp, xác nhận bằng cách đọc): tra cứu prefetch dùng một luồng OS mới cho mỗi yêu cầu và panic của luồng bị nuốt
- Mã: b4-viewing L1. Đơn vị: W8-13j/16d.
- `host/main_window/viewing.rs:112-146` `start_lookup` gọi `std::thread::Builder::new().name("prefetch-lookup").spawn(...)` cho mỗi `navigated()` (và ba lần thử lại sau khi dừng, `prefetcher.rs:RETRY_DELAYS_MS`). Giữ phím mũi tên ~30 lần/giây tạo ~30 luồng/giây, không huỷ lượt cũ (lượt cũ chỉ bị bỏ khi về tới `apply_lookup` vì `generation`). R0003 quy định làn I/O là luồng đặt tên cố định, không tạo luồng theo yêu cầu. Khối `try_get_cached_preview_path` trong luồng không có `catch_unwind`: panic làm luồng chết, không đăng kết quả, kế hoạch prefetch đứng (`planned` giữ giá trị cũ).
- Gợi ý: một làn `prefetch-lookup` dùng `crossbeam-channel` (hoặc rayon) với `catch_unwind`, bỏ yêu cầu có `generation` cũ trước khi chạy.
- Test nên thêm: `PrefetchHost` giả kiểm đếm luồng không tăng khi gọi `navigated` 1000 lần liên tiếp (cần làn thật).

### L4 (thấp, xác nhận bằng cách đọc): `FrameLog::record` ghi file đồng bộ trên UI thread mỗi khung (khi bật)
- Mã: b4-viewing L2. Đơn vị: W8-16d.
- `shell_viewing/frame_log.rs` `record` gọi `file.write_all` không đệm cho mỗi khung; `frame.rs:318-321` gọi nó trong `present_frame`. Mặc định tắt (`PHOTOREVIEW_RS_FRAME_LOG`), khớp C# `AutoFlush`; nhưng lúc bật nó có thể tự gây khựng trong chính phép đo khựng mà nó phục vụ (R0003: không I/O trên luồng UI; ưu tiên 3 của AGENTS.md).
- Gợi ý: đẩy dòng log qua `mpsc` sang một luồng ghi (giữ định dạng và thứ tự), hoặc ghi chú rõ trong doc rằng đo khi bật có độ lệch.
- Test nên thêm: không cần.

### L5 (thấp, xác nhận bằng cách đọc; giữ nguyên tương đương C#): `WM_CONTEXTMENU` với toạ độ màn hình (-1, -1) bị hiểu là phím Menu
- Mã: b4-menus L1. Đơn vị: W8-14e.
- `shell_menus/message_handler.rs:68` `if x == -1 && y == -1` mở menu ở giữa vùng client. Trên cấu hình nhiều màn hình có màn hình phụ ở trái/trên màn hình chính, một chuột phải tại đúng điểm ảo (-1, -1) cũng cho `lparam = -1`. C# có cùng điều kiện (`ContextMenuMessageHandler.cs:27`), nên đây là hành vi tương đương, ghi lại để biết. Tác động cực nhỏ.
- Gợi ý: phân biệt bằng `GetMessageExtraInfo`/trạng thái phím Menu, hoặc bỏ qua.
- Test nên thêm: một test thuần với `lparam` -1 và một điểm client hợp lệ ở (-1, -1) trên màn hình phụ (hiện tại cả hai cho cùng kết quả).

## Ghi chú không tính là phát hiện
- `ViewModelOverlayCommands` và `MainWindowOverlay::attach` bỏ `deps.log` (`shell_overlay/main_window.rs:270`, `log: _`) và `DeferredTooltips::create_now` im lặng khi tạo tooltip thất bại (`tooltips.rs:401`): khó chẩn đoán nhưng khớp C# (`DeferredTooltips.cs` cũng chỉ đặt `_failed`).
- `ShellOverlay::with_toolbar/with_status/...` dùng `expect("ShellOverlay is disposed")` (`overlay/state.rs:416-464`) trên API công khai; chỉ test và 17e dùng, không nằm trên đường I/O hay dữ liệu người dùng.
- `owner_draw::run_guarded` nuốt panic và ghi log, trong khi mọi chốt khác ném lại; là quyết định D-W8B4I-9 bám theo C#, khác chính sách "panic là lỗi" của R0003 nhưng có chủ ý.
## Tái kiểm REVIEW-B4 ngày 2026-10-23, Rust `48f96aa`

**Mọi phát hiện: suy luận từ mã, chưa chạy thử.** Không chạy cargo/test/native UI, không tạo HWND, không sửa Rust. Tổng giữ **3 cao / 2 trung bình / 5 thấp**, 0 phát hiện mới; các mục dưới bổ sung bằng chứng và sửa giới hạn của báo cáo cũ. `git diff c40b59d origin/master` trong năm thư mục production thuộc phạm vi B4 không có thay đổi ở thời điểm đối chiếu; #210 chủ yếu thêm test/bằng chứng mutation, không sửa các đường H1/H2/H3.

### Ba lỗi cao vẫn có đường đi trong mã hiện tại

| Mã | Nơi bảo đảm/thiếu chốt | Kịch bản và test nên thêm |
|---|---|---|
| H1, W8-13h/14c/14f | `crates/pr-platform-win/src/host/main_window/lifecycle.rs:324-330` vẫn giữ mutable borrow khi gọi closure, `:445` lại borrow trong on_closing | Gọi command mở modal qua update_view_model, callback modal giả gọi pump/on_closing/read-view: borrow xung đột. Test hồi quy dùng chính shared VM của MainWindow, không fixture tách VM; assert không panic và completion không mất. Không kết luận đã tái hiện native. |
| H2, W8-14a/13i | `crates/pr-platform-win/src/shell_input/dispatch.rs:647` giữ viewer.borrow_mut xuyên controller; `shell_input/surface.rs:129-130` gọi viewport.update_layout đồng bộ; `host/main_window/lifecycle.rs:359` content_from_viewer đọc cùng viewer | Anchored wheel/click zoom làm surface.update_layout đi qua content_from_viewer khi viewer đang mutable borrow. Test phải nối viewer thật với viewport callback thật; cache snapshot của fixture có thể che đường lỗi. Assert zoom/offset đúng, không chỉ không panic. |
| H3, W8-14a/13i/16f | `shell_input/dispatch.rs:674-680` chỉ stop glide/invalidate từ ViewerUpdate; `crates/pr-app/src/viewmodels/main/state.rs:1319-1326` mới cập nhật presenter qua apply_viewer_update | Zoom từ input core có update nhưng không đi qua VM apply_viewer_update. C# `src/PhotoReview.App.Shared/ViewModels/MainViewModel.cs:136-143` subscribe ZoomModeChanged và PropertyChanged để set presenter zoom/refresh HUD. Test wheel/phím/click -> kiểm presenter zoom và HUD bằng giá trị cụ thể, rồi completion full-resolution còn thuộc ảnh hiện tại. |

Đường input `shell_input/main_window.rs:266` vẫn `ReviewCommandType::OpenInDefaultEditor => todo!()`: M1 còn. Q4 đã chốt ở P-52/#211: xử lý tạm an toàn có thông báo, không cần hỏi lại. Reviewer chỉ ghi nhận, không sửa code.

### Unsafe, handle và R0003

- Quét cú pháp 54 file `.rs` của `host/main_window`, `shell_input`, `shell_menus`, `shell_overlay`, `shell_viewing`: 38 dòng có `unsafe {`, 0 dòng thiếu `SAFETY:` trong 5 dòng trước. Đây chỉ là kiểm chú thích, **không** phải chứng minh không UB, không bao gồm mọi khai báo unsafe extern hoặc FFI wrapper ngoài phạm vi.
- Đọc lại `shell_input/drop_target.rs:280-287`: pdwEffect được kiểm null và đi qua panic boundary; COM method dùng `com_guarded`. Không xác minh con trỏ do COM client sai giao ước bằng chạy thử. Không gọi ReleaseStgMedium trên dữ liệu người dùng trong lần này.
- `shell_menus/popup_host.rs:254-280`: session tạo trước guard menu, TrackPopupMenu xong thì root/menu guard được huỷ trước `pending_panic::rethrow_pending`. Đây là biên bắt/ném lại cần giữ; bỏ catch FFI không phải cách sửa H1. R0003 nói panic UI là bug; không nên nuốt BorrowError bằng catch rộng để làm app tiếp tục với state mất đồng bộ.
- `host/main_window/lifecycle.rs:540-582` dispose lần lượt unsubscribe hub/window/layout, dispose feature ngược thứ tự, bỏ size source callback, release renderer và viewport rồi unregister. Đọc thứ tự này chưa chứng minh mọi cycle được cắt; test vòng tham chiếu từ pr-203 M2 vẫn cần chạy và định danh nguồn giữ Rc.

### Đọc bổ sung vùng trước đây chưa đọc kỹ

Đã đọc `shell_overlay/overlay/hit_test.rs`, `layout.rs`, `paint.rs`, `input.rs` và `shell_viewing/scrollbars/layout.rs`. Pointer hover/request_render được gọi sau khi nhả state borrow ở hit_test; key click lấy focused button ra trước khi gọi click_button. Scrollbar hit-test che vùng compare trước khi xét bar; `to_dip` dùng normalized_dpi. Chưa tìm ra sai lệch mới đủ căn cứ; không khẳng định callback renderer tuỳ ý có thể re-enter là lỗi production khi chưa có caller cụ thể.

### Cập nhật test và đường dẫn giao việc

Đếm cú pháp `#[test]`/`#[ignore` trong 5 binary `host_main_window_*` hiện tại:

| Binary | Test | Ignore |
|---|---:|---:|
| core | 28 | 22 |
| frame | 18 | 16 |
| lifecycle | 23 | 16 |
| viewing | 14 | 11 |
| windowing | 38 | 17 |
| Tổng | 121 | 82 |

Con số 82/120 ở M2 là snapshot cũ; hiện là 82/121 theo quét cú pháp, không phải kết quả test runner. Test mutation thêm không tự chứng minh H1/H2/H3 đã được bắt.

**Đính chính phạm vi prompt G1/G2 của BACKLOG:** đường thật theo `contracts/W8-13g.md` là `crates/pr-platform-win/src/host/main_window/`, không có thư mục `src/shell_main_window/`. Main view model nằm ở `crates/pr-app/src/viewmodels/main/`, không phải glob `main_view_model*`. Người triển khai chỉ được claim/sửa file thật; không tạo mô-đun song song để khớp đường dẫn sai trong prompt cũ. Việc này là sửa sự thật tài liệu, không phải quyết định sản phẩm.
