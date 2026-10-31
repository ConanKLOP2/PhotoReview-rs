# ST-APP-RACES 2026-10-31: review tĩnh đua luồng / khoá / huỷ tác vụ nền trong `crates/pr-app`

Status: report (lane tĩnh). Kết quả: 1 cao, 5 trung bình, 5 thấp (mục 2 liệt kê các khu vực đã kiểm và đạt). Không đổi mã production, không thêm test.

Phạm vi và trung thực: chỉ đọc mã và `grep` tại master `93e44a1d` (worktree `st-app-races`). Không build, không chạy test. Mọi điều dưới đây là **suy luận từ mã, chưa chạy**; không có phát hiện nào được tái hiện bằng test. Đã đọc kỹ: `coordinators/file_action_gate.rs`, `coordinators/file_actions/{actions,undo}.rs`, `coordinators/folder_load/{load,skipped,state}.rs`, `coordinators/duplicate_cleanup.rs`, `coordinators/zoom_detail.rs` (runner, inbox), `misc/file_hash.rs`, `viewmodels/main/{state,commands,navigation}.rs` (phần worker/gate), `coordinators/presenter/{present_a,missing,photo_info}.rs` (điểm gửi job), `presenters/settings/apply.rs`, `startup/{context,settings_load,error_log}.rs`. Chỉ đọc lướt: `presenters/benchmark.rs`, `presenters/recovery_window.rs`, `overlay/*`, `input/*`, `viewport/*`, `windowing/*`. Đối chiếu C# `7eaee2a2` chỉ ở mức ghi nhớ trong chú thích mã (không mở lại nguồn C#); chỗ nào nói về C# mà chưa đọc nguồn thì ghi "suy luận".

Quy ước mức: Cao = có thể khoá vĩnh viễn một chức năng hoặc làm sai trạng thái dữ liệu; Trung bình = kẹt một luồng công việc đến khi người dùng làm lại thao tác; Thấp = hiếm, tự hồi phục, hoặc chỉ ảnh hưởng chẩn đoán.

## 0. Tóm tắt kiến trúc liên quan (để đọc các phát hiện)

- `WorkerPort::submit(lane, job)` (`coordinators/folder_load/state.rs:60`) được cài bằng `MainWorkerPort` (`viewmodels/main/state.rs:679-688`): **bỏ qua `lane`**, mỗi job một thread (`ThreadRunner`, `coordinators/zoom_detail.rs:52-58`), và chạy `inbox.send(wrap(job()))`. Chú thích tại `state.rs:660` nói "A panic of the job is the caller's `catch_unwind`". Nghĩa là: **nếu job panic mà người gọi không bọc `catch_unwind`, sự kiện hoàn tất không bao giờ được gửi** và coordinator ở trạng thái "đang chờ" mãi.
- "Lane" `FileAction` thực chất không tuần tự hoá bằng thread; việc tuần tự hoá do `FileActionGate` (`file_action_gate.rs`) bảo đảm. Gate dùng `GateTurn` RAII (Drop nhả cổng), `Mutex<GateState>` chỉ giữ ngắn, `post` ngoài khoá. Đã kiểm: không có lời gọi chặn dưới khoá gate, không đảo thứ tự khoá (xem mục 3).
- Chống kết quả cũ: `load_id` (folder load), `ticket` (file action / undo / duplicate), `token` điều hướng (presenter), `folder_generation` (sibling), `generation` (file_hash). Đa số đúng; ngoại lệ ở mục 2.

## 1. Phát hiện

### H1. Cao: job băm trùng lặp không có `catch_unwind` -> cổng file-action bị giữ vĩnh viễn

- Vị trí: `crates/pr-app/src/coordinators/duplicate_cleanup.rs:296-331` (closure `job` của `WorkerLane::Hashing`); gate được giữ ở `viewmodels/main/commands.rs:719-760` (`run_exclusive` + `duplicate_gate_turn`).
- So sánh: mọi job FileAction khác đều bọc (`actions.rs:252`, `:802`, `undo.rs:101`, `duplicate_cleanup.rs:651`, rename); job `CacheClear` bọc (`:445`). Riêng job `Hashing` thì không. Chú thích trên `MainWorkerPort` xem đây là việc của người gọi.
- Kịch bản: `duplicates::find`, hàm băm (`hasher.get`) hoặc `file_system` panic (ví dụ `unwrap`/index trong `pr-core`, hoặc panic của reader) trong thread băm. Thread chết, `DuplicateCleanupEvent::HashFinished` không bao giờ vào inbox. Hệ quả: `active_ticket` và `hash_gate` slot còn nguyên; `duplicate_gate_turn` còn `Some` nên `GateTurn::Exclusive` không được nhả; `FileActionGate::is_held()` mãi mãi `true`; `try_enter` và `run_queued` bị từ chối -> Undo, Recycle, Move/Copy, Rename, dọn trùng đều thành no-op im lặng đến khi thoát app. Esc (`cancel_duplicate_check`) chỉ huỷ token, không tạo sự kiện nên không gỡ được.
- Test hiện có: có test cho `RecycleOutcome::Panicked` (`crates/pr-app/tests/duplicate_cleanup.rs:1257`); **không thấy** test cho panic ở job Hashing (suy luận từ `grep catch_unwind`, chưa mở hết thư mục tests).
- C#: `RemoveDuplicatesAsync` chạy trong `try/finally` của `RunExclusiveAsync`; một exception của task băm làm `await` ném và `finally` nhả cổng (suy luận từ chú thích mã; chưa mở nguồn C#). Rust lệch ở chỗ panic của thread nền không đến được `finally`.
- Đề xuất: bọc thân job trong `catch_unwind(AssertUnwindSafe(..))`, map panic sang `HashOutcome::Failed(text)` (hoặc thêm biến thể), để `on_hash_finished` đi đường kết thúc batch. Hoặc gốc rễ: xem F-ROOT ở mục 5.

### M1. Trung bình: job quét thư mục (`run_scan`) không có `catch_unwind` -> tải thư mục kẹt ở `Scanning`

- Vị trí: `coordinators/folder_load/load.rs:283-290` (`WorkerLane::Scan`, closure gọi `run_scan(input)` trực tiếp). Probe (`:109-120` của `skipped.rs`) và Explorer (`load.rs:326-`) đều có bọc; scan thì không.
- Kịch bản: panic trong `enumerate_files_with_stat`, `sort_entries`, `CatalogEntry::new`... -> `FolderLoadEvent::ScanFinished` không đến; `active.phase` mãi `Scanning`, `pending_open` của view model không hoàn tất, các lệnh bị `park_if_order_pending` (Next/Prev/Skip, và lệnh file đã nhận turn: `commands.rs:192-208` cất `turn` vào `record.turn`) chờ cổng thứ tự không bao giờ mở. Lệnh file đã có turn thì **giữ cổng file-action** (cùng hậu quả như H1 nhưng gỡ được bằng cách mở thư mục khác, vì `load()` thay `active` và trả `order_gate_released`: `load.rs:196-199`).
- Test: `ScanFailure::Canceled/Failed` có test; panic trong scan thì không thấy (suy luận).
- C#: exception trong `Task.Run` chảy về `LoadAsync` catch -> `OnFailed` (suy luận từ chú thích `FolderLoadError`).
- Đề xuất: bọc `run_scan`, map panic sang `ScanFailure::Failed(FolderLoadError::Io(io::Error::other("scan panicked")))` để đi đường `report_load_error` + `finish_load`.

### M2. Trung bình: worker băm của `FileHashService` không bắt panic -> waiter treo vĩnh viễn, mục in-flight chết

- Vị trí: `misc/file_hash.rs:424-466` (`thread::spawn(move || { ... compute_and_cache_parts(...) ... })`); vòng chờ ở `:476-485`.
- Kịch bản: `compute_and_cache_parts` panic (reader/`SourceBytesCache`/`fs`). Thread chết trước khi gán `state.result` và trước khi gỡ khỏi `in_flight`. Mọi caller đang chờ lặp `while state.result.is_none() && !cancel.is_canceled()` (wait_timeout 50 ms kiểu poll) **không bao giờ thoát** trừ khi token của chính nó bị huỷ. Mục `in_flight[key]` vẫn có `result == None`, nên mọi lần gọi sau cho cùng đường dẫn (so sánh ảnh, dọn trùng) đều `waiters += 1` rồi treo. Với `CancelToken::none()` (so sánh: `present_b.rs:689-` dùng `CancelToken::none()`) thì treo vĩnh viễn thread phụ + trạng thái hash so sánh không bao giờ xong.
- Test: có test chia sẻ in-flight, huỷ, đổi file; không thấy test worker panic (suy luận).
- Đề xuất: `catch_unwind` quanh `compute_and_cache_parts`, panic -> `Err(FileHashError::Io(Arc::new(io::Error::other(..))))`, luôn đặt `result` và gỡ khỏi map (đặt vào `Drop` guard để chắc chắn).

### M3. Trung bình: các job presenter / zoom-detail / overlay không bắt panic -> pha điều hướng kẹt

- Vị trí: `presenter/present_a.rs:405-410` (`preview.wait()`), `:426-431` (`request.wait()`), `present_b.rs:282-` (so sánh preview), `:368-` (kích thước file), `:438-` (hash), `:689-` (kích thước gốc), `zoom_detail.rs:579-` (decode gốc), `viewmodels/main/state.rs:1364-` (tìm thư mục anh em cho overlay), `viewmodels/main/navigation.rs:624-` (tìm thư mục anh em; `ownership` thì có bọc ở `:228`).
- Kịch bản: panic trong `wait()`/`compare_file_size`/`try_get_original_dimensions`/`find_*`. Sự kiện không đến; `Phase::AwaitPreview/AwaitCompare/AwaitDimensions` giữ nguyên cho token đó. Với presenter, lần điều hướng sau (token mới) thay thế nên tự hồi phục (mức Thấp-TB). Với `zoom_detail.rs:557-559` (`if self.load.is_some() { return; }`) thì slot `load` còn nên "đang decode" cho đến `cancel_pending`/điều hướng. `pending_worker_jobs` (overlay, sibling, ownership) không giảm -> `is_idle()` sai, chỉ ảnh hưởng vòng chờ của test (`state.rs:1198-1214`).
- Hạn chế tác động: `PreviewImageService` có thể tự bắt panic của worker bên dưới (chưa kiểm). Nên coi là rủi ro phòng thủ.
- Test: chưa thấy test panic cho các job này (suy luận).
- Đề xuất: sửa tập trung ở `ThreadRunner::run`/`MainWorkerPort::submit` (xem F-ROOT) thay vì từng chỗ.

### M4. Trung bình: `Mutex<UndoService>` được giữ suốt `undo_last()` blocking (đã biết ở FIX-UNDELETE-HANG), còn nguyên và chỉ an toàn nhờ gate

- Vị trí: giữ khoá: `coordinators/file_actions/undo.rs:101-104` (worker, toàn bộ `guard.undo_last()`); lấy khoá trên UI thread: `undo.rs:74` (`undo_last`), `actions.rs:613-617` (`with_undo`, gọi tại `actions.rs:531,533,535,942,1011,1081,1084,1310`).
- Phân tích hiện trạng: trên master `93e44a1d` chưa có thay đổi giữ khoá nào khác ngoài `recycle_restore.rs` (hạn 120 s, PR đã nêu ở [`FIX-UNDELETE-HANG-2026-10-31.md`](FIX-UNDELETE-HANG-2026-10-31.md)). Tôi đã dò mọi chỗ khoá `Mutex<UndoService>` trong `pr-app`: chỉ ba file trên; `MainViewModel::undo_service()` (`state.rs:959`) không có người gọi ngoài test; `pr-shell` chỉ lấy `Arc` để truyền vào (`composition/main_view_model.rs:332-397`). Mọi chỗ UI-thread khoá đều nằm trong lượt gate đã giữ (Undo = `run_exclusive`, `commands.rs:340-361`; các action = `run_queued`) và hai loại này loại trừ nhau (`file_action_gate.rs`: `try_enter` từ chối khi `queue_length != 0`, `run_queued` từ chối khi `held`). Vì vậy **hiện không có đường UI thread chờ khoá đang bị worker giữ**. Hệ quả của việc restore treo 120 s là cổng bị giữ (mọi lệnh file thành no-op), không phải UI đóng băng.
- Rủi ro còn lại (cấu trúc, không phải lỗi hiện tại): bất biến "chỉ khoá trong lượt gate" chỉ nằm trong chú thích; thêm một chỗ gọi `with_undo` ngoài gate (ví dụ cập nhật trạng thái enable của menu Undo, hay `has_last_undo_action` để vẽ nút) sẽ làm UI thread chờ tới 120 s. `rename` (`actions.rs:1310` -> `register`) chạy trong lượt queued nên an toàn.
- Test: không có test khẳng định bất biến này (ví dụ test "UI thread không chờ khi Undo đang chạy"). Thiếu.
- Đề xuất: (a) không giữ khoá suốt `undo_last`: tách trạng thái đọc nhanh (`last_undo_action_identity`, `is_busy`) khỏi phần chạy lâu bằng `Arc<Mutex<Inner>>` + `AtomicBool` hoặc lấy `action` ra khỏi khoá rồi chạy ngoài khoá; hoặc (b) tối thiểu thêm `try_lock` + trạng thái "Undo đang chạy" ở mọi điểm đọc từ UI và ghi bất biến vào doc của `with_undo`. Chọn (b) nếu muốn giữ parity với C# (không khoá).

### M5. Trung bình (parity, cần quyết định): `MainWorkerPort` bỏ qua `WorkerLane`, nên các "lane" không tuần tự hoá

- Vị trí: `viewmodels/main/state.rs:681-688` (`let _ = lane;`), chú thích mở O2 tại `:658-660`.
- Kịch bản: hai job cùng lane có thể chạy song song: ví dụ Scan của lần mở thư mục cũ (đã huỷ) và Scan mới; hoặc `FileAction` Recycle của dọn trùng + một job khác nếu gate bị bỏ qua. Hiện an toàn nhờ gate/`load_id`, nhưng lane-based serialisation mà tài liệu mô tả (`WorkerLane` doc "The port maps a lane to a thread") không có thật. Với Scan: hai quét song song cùng ổ đĩa (chậm hơn trên HDD/NAS) vì token chỉ huỷ ở các điểm kiểm (`load.rs:88-110`), không huỷ I/O đang chạy.
- Test: không có (chỉ có fake đồng bộ trong test).
- Đề xuất: hoặc ghi rõ trong ADR rằng lane chỉ là nhãn, hoặc cài lane thật (một thread chung cho FileAction, thread riêng cho Scan có thể bỏ qua job đã huỷ).

### L1. Thấp: `directory_exists` / `file_exists` trên UI thread

- Vị trí: `coordinators/folder_load/load.rs:228` (`load()` đồng bộ, phần "synchronous prefix" chạy trên UI thread), `coordinators/file_actions/move_copy.rs:83-84` (kiểm thư mục đích đã nhớ), `presenters/rename_prompt.rs:54` (`file_exists` khi bấm xác nhận đổi tên), `misc/launch_args.rs:40,45` (khởi động).
- Kịch bản: thư mục/đích nằm trên chia sẻ mạng chậm hoặc ổ ngắt kết nối: UI thread chờ tới timeout của hệ điều hành (hàng giây). C# làm cùng chỗ cho `Directory.Exists` ở phần đồng bộ của `LoadAsync` (suy luận từ chú thích "C# lines"), nên là parity; nhưng các chú thích khác trong mã (`photo_info.rs`, `present_a`) đã chủ trương "stat không chạy trên UI thread" cho NAS.
- Test: không.
- Đề xuất: chuyển `directory_exists` của `load()` vào đầu job Scan (lỗi `DirectoryNotFound` trả về qua `ScanFailure::Failed`), đồng bộ với chính sách stat.

### L2. Thấp: `let _ = self.session_writer.update(session)` nuốt lỗi

- Vị trí: `viewmodels/main/state.rs:1451`. `SessionWriter::update` chỉ trả `Err(BlankFolder)` (`pr-core/src/session/writer.rs:679`), nên chỉ mất khi `session.folder` trống; hậu quả: phiên không được lưu, không log. Mức Thấp. Đề xuất: log một dòng `warn`.

### L3. Thấp: `catalog.restore` và `restore_members` trên danh sách đã đổi

- Vị trí: `actions.rs:1056-1068` (INV-5). Chỉ số `flight.source_index` lấy trước khi I/O; `ReviewCatalog::restore` kẹp chỉ số (`pr-core/src/catalog/review_catalog.rs:311`) nên không tràn, nhưng vị trí khôi phục có thể lệch nếu danh sách đổi giữa chừng (reload cùng generation hiếm). Không panic. Ghi nhận để biết, không cần sửa.

### L4. Thấp: `path_at(i)` panic khi `i` ra ngoài, gọi từ các vòng dùng `count()` đã chụp

- Vị trí: `file_actions/undo.rs:470-474` (`sorted_insert_index` lặp `index < cx.catalog.count()` rồi `path_at(index)` mỗi vòng, an toàn vì đơn luồng và đo lại mỗi vòng), `presenter/missing.rs:133` (đã kiểm `in_range`), `present_a.rs:134` (đã kiểm `slot < count`), `navigation.rs:589` (đã kiểm), `actions.rs:758` (đã kiểm `source_index > 0`). Tất cả đều có chốt chặn; `ReviewCatalog` không có `Sync` và có `assert_owner_thread`. Không phải lỗi; liệt kê vì yêu cầu của lane. Kết luận: **không tìm thấy tràn chỉ số** trong điều hướng/chọn của `pr-app`.

### L5. Thấp: `describe_panic` / panic trong `executor_threw` chỉ ghi log trên đường `Panicked`

- Vị trí: `actions.rs:918` (bọc `finish_success`), `undo.rs:121-124`. Trạng thái phục hồi đã được xử lý (restore khi chưa thành công). Không thấy lỗi; một lưu ý: sau `Panicked` của undo, `UndoService` có thể đã đổi một phần (di chuyển file xong nhưng chưa pop entry) vì khoá bị poison được `into_inner` bỏ qua. `GateHold` là RAII (`pr-core/.../undo.rs:228`) nên cờ busy được nhả. Rủi ro nhất quán Undo sau panic nằm ở `pr-core`, ngoài phạm vi; đề xuất lane `pr-core` kiểm tra thứ tự "xác nhận disk -> pop entry".

## 2. Các mục đã kiểm và đạt (để khỏi kiểm lại)

- **Thứ tự khoá**: không có chỗ nào giữ hai `Mutex` cùng lúc theo hai thứ tự khác nhau. `file_hash.rs` lấy `in_flight` rồi `InFlight.state` (`:382-385`), worker lấy `state` rồi nhả trước khi lấy `in_flight` (`:466-471`) -> cùng thứ tự hoặc không lồng. `FileActionGate` luôn nhả khoá trước khi `scheduler.post`/chạy `waiter()` (`file_action_gate.rs:130-165`, `:226-235`). `CompletionInbox::send` gọi `wake` ngoài khoá (`zoom_detail.rs:100-106`).
- **Kênh đã đóng**: `pr-app` không dùng `mpsc`/`crossbeam` (chỉ `task_logging.rs` đọc `Receiver`); hàng đợi là `Mutex<VecDeque>`, `send` sau khi VM đóng chỉ đẩy vào hàng đợi còn sống. `GateTurn` còn trong inbox bị thả cùng VM -> Drop nhả cổng (`file_action_gate.rs:77-98`).
- **Huỷ khi đổi thư mục / kết quả cũ**: folder load dùng `load_id` + `phase` (`load.rs:389-399`), file action/undo/dup dùng `ticket` (`actions.rs:853-859`, `undo.rs:121-123`, `duplicate_cleanup.rs:469-495`), điều hướng dùng `take_navigation(token)` (`present_a.rs:89-94`), sibling dùng `is_folder_current(folder_generation)` (`sibling_navigator.rs:175`), hash dùng `generation` (`file_hash.rs:307,413,551`), kết quả stat bị huỷ có guard `StatDropCompletion` (an toàn cả khi unwind). Job quét kiểm token ở predicate liệt kê (`load.rs:85-110`) nhưng không huỷ được I/O đang chạy (ghi chú O4 trong mã).
- **unwrap/expect trên dữ liệu người dùng**: `grep` toàn `pr-app/src` loại trừ test: chỉ còn 2 `expect` ở `overlay/panel.rs:161,166` (id hằng không rỗng); các `expect` còn lại nằm trong khối test. JSON settings: `import_settings_from` và `try_import` đi qua `Result` (`settings/apply.rs:193-215`, `action_profiles.rs:304`). Không có `unwrap` trên tên file.
- **Cập nhật UI từ thread khác**: worker chỉ gọi `inbox.send(...)`; mọi thay đổi catalog/sink diễn ra trong `pump`/`dispatch` (`state.rs:1220-1254`). `ReviewCatalog` có `assert_owner_thread`.
- **Điều hướng khi danh sách đổi**: Next/Prev/First/Last tính đích lúc thực thi (`navigation.rs:529-570`, `step_target`), lệnh bị hoãn được tính lại sau khi cổng thứ tự mở; `skip_core` kiểm `index < count` (`:579-587`).
- **Undo/Redo sau lỗi**: `on_undone` xử lý `Panicked`/`Canceled`/thất bại một phần (khôi phục thành viên vào catalog, `undo.rs:226-243`); lượt gate được nhả theo `handle_step`. Không có Redo trong `pr-app`.

## 3. Đối chiếu với ST-INVMAP M3 (panic trong worker làm mất trạng thái)

M3 trong [`ST-INVMAP-2026-10-31.md`](ST-INVMAP-2026-10-31.md) mô tả đúng họ lỗi "worker panic, trạng thái mất". Trong `pr-app`, các job file-action đã được vá (catch -> `Panicked`), nhưng **ba nhóm chưa vá**: H1 (băm trùng lặp), M1 (quét), M2 (băm file) và M3 (presenter/zoom/overlay). Gốc chung ở mục 5.

## 4. Test thiếu (đề xuất, không thêm trong lane này)

| Test đề xuất | Vị trí gợi ý | Kỳ vọng |
|---|---|---|
| Hash job panic -> cổng được nhả | `crates/pr-app/tests/` cạnh test duplicate cleanup | `is_held() == false` sau `HashFinished` giả lập panic; trạng thái không còn `active_ticket` |
| Scan panic -> `on_failed` + `Finished` | cạnh test folder load | `ScanFinished` nhận `Failed`, `order_gate_released` được trả |
| `FileHashService` worker panic | `misc/file_hash.rs` tests | `get` trả `Err`, lần gọi sau cho cùng đường dẫn không treo |
| Bất biến "không khoá `UndoService` ngoài gate" | test kiến trúc (grep) hoặc test với `try_lock` | Undo đang chạy (fake treo) thì mọi lệnh UI trả ngay |
| `MainWorkerPort` panic job | `viewmodels/main` tests | sự kiện `JobPanicked` đến và `pending_worker_jobs` giảm |

## 5. Gốc rễ và đề xuất tập trung (F-ROOT)

Sửa một lần ở `MainWorkerPort::submit` thay vì từng coordinator:

1. Đổi chữ ký `submit` thành nhận thêm `on_panic: fn(String) -> E` (hoặc `E: From<PanicReport>`), và trong `MainWorkerPort` chạy `catch_unwind(AssertUnwindSafe(job))`; nhánh panic gửi `wrap(on_panic(text))`. Mỗi enum sự kiện thêm biến thể `WorkerPanicked { ticket/load_id, text }` (hoặc dùng sẵn `Failed`/`Panicked`).
2. `ThreadRunner::run` cũng nên bọc job bằng `catch_unwind` + `AppLog::error` để ít nhất mọi panic nền đều có dấu vết (hiện thread nền chết im, chỉ có thông báo mặc định của panic hook).
3. `FileHashService` worker: bọc riêng (M2) vì nó không đi qua `WorkerPort`.

Rủi ro của đề xuất: thêm biến thể sự kiện chạm nhiều `match` (đã exhaustively matched); nên làm theo từng coordinator, bắt đầu với H1 (nhỏ nhất, nặng nhất).

## 6. Đề xuất đổi PARITY.md

Không có hàng PARITY nào cần đổi trạng thái từ lane này. Nếu có hàng nói về "FileActionGate"/"DuplicateCleanupController"/"FolderLoadCoordinator", gợi ý ghi chú: `DIVERGENCE: panic của job nền không được chuyển thành kết quả lỗi (C# catch tại await); xem ST-APP-RACES H1/M1`. Bằng chứng: `duplicate_cleanup.rs:296-331`, `folder_load/load.rs:283-290`.
