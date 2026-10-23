# Review tĩnh: painter hộp thoại và render Direct2D (W8-18l..18n, W8-12b..12i)

Machine C (cloud). Phạm vi: `crates/pr-platform-win/src/dialogs/native/painter/` (mod, controls, frame, geometry, widgets) và `crates/pr-platform-win/src/render/` (surface, draw_context, keep_alive, device/{mod,create,resources,loss}, image_cache/{policy,upload}, text/{renderer,layout_cache}) cùng test `dialogs_painter_*.rs`, `render_*.rs`, `gfx_d2d_dwrite.rs`.

**Mọi phát hiện dưới đây: suy luận từ mã, chưa chạy thử.** Không chạy cargo, không chạy test.

**Mức độ đọc (trung thực):**
- Đọc kỹ từng dòng: `painter/mod.rs` (vòng đời subclass), `painter/controls.rs` (guard GDI, `paint_to`, `repaint_now`), `widgets.rs` (trackbar, progress, combo text), `render/surface.rs` (vòng đời frame), `render/keep_alive.rs`, `device/loss.rs`, `image_cache/policy.rs` (phần cache/stripe), phần lớn `draw_context.rs` (unsafe + draw_image/clip/opacity/transform).
- Đọc lướt (chỉ các khối `unsafe` và SAFETY): `device/create.rs`, `device/resources.rs`, `device/mod.rs`, `text/renderer.rs`, `image_cache/upload.rs`.
- Chưa đọc từng dòng: `painter/geometry.rs`, `painter/frame.rs`, các hàm `check_layout`/`combo_layout` của `widgets.rs`, `text/layout_cache.rs`, phần xếp tile (`compute_tiles`) và `try_snap_pixel_exact`. Chưa đối chiếu số học với C# cho các hàm này (chỉ `paint_scale`, `scaled`, `progress_fill_width`, `tick_xs` đã so khớp mô tả/C#).
- Trong mã production của `render/` không có `unwrap`/`expect`/`panic!`/`todo!` (các lần xuất hiện nằm trong `#[cfg(test)]`); mọi khối `unsafe` đều có comment SAFETY.

Tổng: 0 cao / 1 trung bình / 5 thấp.

## Đã đọc và thấy đúng
- `painter/mod.rs`: `PaintState` được `Box::into_raw` rồi giải phóng đúng một lần ở `WM_NCDESTROY` (hoặc ngay khi `set_window_subclass` thất bại); `kind` được sao chép trước khi xử lý thông điệp nên không đọc `state` sau `Destroy`; `remove_window_subclass` trước khi gọi `def_subclass_proc`; panic bị `pending_panic::guarded` bắt và thông điệp vẫn đi tiếp xuống `def_subclass_proc`.
- `painter/controls.rs`: mọi đối tượng GDI nằm trong guard (`Owned`, `Reselect`, `MemoryBuffer`, `PaintGuard`, `DcGuard`); thứ tự khai báo bảo đảm `Reselect` chạy trước `Owned` nên không xoá bút/cọ đang được chọn vào DC; `paint_to` lùi về vẽ thẳng khi không tạo được bộ đệm.
- `widgets.rs` trackbar: `TBM_GETPTICS` kiểm null, chỉ đọc `count - 2` giá trị và sao chép ngay; `combo_item_text` dùng bộ đệm `length + 1`.
- `surface.rs`: `drawing` được đặt sau khi lấy được context; `end_draw_and_present` đặt `drawing = false` trước khi gọi `EndDraw` nên lỗi không kẹt khung; ánh xạ mất thiết bị (`0x8899000C`, `0x887A0005`, `0x887A0007`) đúng.
- `keep_alive.rs`: thiết bị D3D/D2D được tạo, dùng và huỷ chỉ trên luồng keep-warm; cờ `signaled` dưới mutex tránh mất tín hiệu đánh thức; mọi `panic` bị bắt và đánh dấu `failed`.
- `image_cache/policy.rs`: danh sách sự kiện `upload_completed` được gọi sau khi nhả `borrow_mut`; `serial` ngăn stripe cũ ghi vào ảnh mới; tính tổng byte dùng `saturating_*`.

## Phát hiện

### M1 (trung bình, chỉ nghi ngờ): sau khi đổi kích thước thất bại, surface không còn target nhưng vẫn cho `begin_draw_context`
- File: `render/device/resources.rs` `resize` (dòng ~90-123) + `render/surface.rs` `begin_draw_context` (dòng ~229-248).
- `resize` gọi `release_target()` trước `ResizeBuffers`; nếu `resize_buffers` trả lỗi KHÔNG phải mất thiết bị, hàm trả `BuffersFailed` và `surface.resize` trả `Err(Device…)` mà target đã bị bỏ, không tạo lại. Tương tự khi `create_target()?` lỗi sau khi resize xong. `begin_draw_context` chỉ lấy `resources.context()` rồi `BeginDraw`; comment SAFETY ghi "the target is attached" nhưng không có kiểm tra nào.
- Kịch bản: `ResizeBuffers` thất bại một lần (ví dụ `E_OUTOFMEMORY` khi kéo cửa sổ rất lớn) → các khung sau gọi `BeginDraw` không có target → `EndDraw` trả lỗi ≠ mất thiết bị → `classify_end_draw` = `Failed` → mỗi khung bị bỏ im lặng (`PresentResult::Failed`) cho tới lần `resize` thành công kế tiếp. Cửa sổ có thể đứng hình nếu người dùng không đổi kích thước nữa. Giống hành vi C# (`ReleaseTarget; ResizeBuffers; CreateTarget`), nhưng ở C# ngoại lệ lan lên cho tầng trên xử lý.
- Chưa chắc vì chưa thấy người gọi (`host::main_window`) có thử `resize` lại hoặc `recreate_device` sau `Err`. Gợi ý: sau lỗi `resize` buộc `recreate_device()`, hoặc `begin_draw_context` từ chối khi chưa có target.

### L1 (thấp, chỉ nghi ngờ): `RefCell` của `BudgetedImageCache` được giữ qua các lời gọi GPU/COM
- File: `render/image_cache/policy.rs`: `get_or_upload` giữ `state.borrow_mut()` khi gọi `backend.upload` / `upload_rows`; `run_stripe` giữ khi gọi `upload_rows`; `dispose`/`clear_locked` giữ khi gọi `backend.release` và `unsubscribe_device_recreated`.
- Quy tắc D-W8B4I-1 cấm giữ borrow qua lời gọi OS/hook. Nếu backend (hoặc handler `device_recreated` được surface gọi đồng bộ) chạm lại cache — ví dụ `on_device_recreated` → `clear()` → `state.borrow_mut()` — sẽ `BorrowMutError` panic trên luồng UI.
- Hiện đường này khó xảy ra: `D2dRenderSurface::recreate_device` chỉ chạy trong `resize`/`end_draw_and_present`/`present`, không nằm trong `upload`. Ghi lại vì việc thêm một lời gọi `recreate` vào đường upload (ví dụ xử lý `DEVICE_REMOVED` ngay khi upload) sẽ biến thành panic.

### L2 (thấp, xác nhận bằng cách đọc): `draw_image_oriented` bỏ qua kết quả `pop_transform`
- File: `render/draw_context.rs` `draw_image_oriented`: `let _ = self.pop_transform();` với comment "It can only fail when the push was skipped". Không đúng hoàn toàn: nếu `push_transform` bị bỏ qua (context không dùng được) và ngăn xếp transform đang có phần tử của người gọi, `pop_transform` THÀNH CÔNG và gỡ nhầm transform bao quanh.
- Kịch bản: khung có `push_transform(T)` của người gọi → thiết bị bị huỷ/giải phóng giữa chừng → `draw_image_oriented`: push bỏ qua, pop gỡ `T`. Hiếm (cùng cổng `context()` cho cả hai).

### L3 (thấp, chỉ nghi ngờ): `pop_clip` giảm độ sâu dù không pop được clip của D2D
- File: `draw_context.rs` `pop_clip`: `self.frame.pop_clip()?` giảm bộ đếm trước, rồi chỉ gọi `PopAxisAlignedClip` nếu `context()` là `Some`. Nếu `context()` là `None` (thiết bị đã giải phóng giữa khung) thì clip của D2D còn mở mà bộ đếm báo cân bằng → `end()` báo `balanced` trong khi D2D chưa cân bằng. Hậu quả có thể là `EndDraw` lỗi `D2DERR_UNBALANCED_...`. Cực hiếm.

### L4 (thấp, xác nhận): `dialog_proc` gọi `get_class_name` cho mỗi `NM_CUSTOMDRAW`
- File: `painter/mod.rs` `dialog_proc` → `is_list_view(header.hwndFrom)` → `get_class_name` (cấp phát chuỗi) cho mọi notification `NM_CUSTOMDRAW` của mọi control con của hộp thoại, kể cả nút/toolbar. Chỉ là chi phí (mỗi giai đoạn vẽ mỗi dòng của list view lớn); đúng chức năng.

### L5 (thấp, chỉ nghi ngờ): lệch số học giữa C# và Rust cho giá trị quá lớn
- `painter/geometry.rs`, `widgets.rs`: C# `(int)Math.Round(x)` không kiểm tràn cho giá trị ngoài miền `int` cho kết quả `int.MinValue`; Rust `as i32` bão hoà (`i32::MAX`) hoặc `0` cho NaN. Mô tả trong mã đã ghi nhận ("saturating cast"). Chỉ khác với đầu vào cực đoan (kích thước control hàng tỷ pixel); ghi lại để biết đó là khác biệt có chủ ý, không phải lỗi.

## Test yếu / rỗng (suy luận từ mã)
- Phần lớn test của painter và render là test native `#[ignore]` (cần desktop/GPU): `render_text.rs` 20/20, `gfx_d2d_dwrite.rs` 16/20, `render_image_cache_upload.rs` ≈22/37, `dialogs_painter_controls.rs` ≈12/18, `dialogs_painter_frame.rs` ≈9/13, `dialogs_painter_widgets.rs` ≈6/17, `render_keep_alive.rs` 5/29, `dialogs_painter_attach.rs` hầu hết. Mọi hành vi vẽ thật (màu pixel, contrast, double buffering, rò GDI) chỉ được kiểm khi chạy tay trên Windows.
- `tests/render_keep_alive.rs:590-600` `open_hardware_work_is_none_without_a_real_gpu_otherwise_a_runnable_device`: nhánh `Ok(None)` (máy chỉ có WARP, phổ biến trên CI) chỉ `println!`, không khẳng định gì → đạt giả trên máy không GPU (và vẫn `#[ignore]`).
- `tests/dialogs_painter_controls.rs:205-211` `push_button_draw_never_panics_on_degenerate_sizes`: chỉ kiểm không panic (`let _ = …`), phù hợp với tên nhưng không kiểm kết quả.
- Các test v5_classic/v6_manifest (ví dụ `…_v5_classic`, `…_v6_manifest`) chỉ gọi `run(false|true, helper)`: có khẳng định bên trong helper, ổn; nhưng đều `#[ignore]`.
- Không có test cho L2, L3, M1 (sau lỗi `resize` thì khung kế tiếp ra sao).

## Không kiểm tra được
- Mọi hành vi thật trên Windows: kết quả vẽ GDI/Direct2D, độ tương phản thật của chữ, rò GDI handle (chỉ suy ra từ guard), thứ tự thông điệp subclass với comctl32 v5/v6, mất thiết bị thật, `ResizeBuffers` lỗi, `Present` bị che (occluded).
- Số học bố cục (`check_layout`, `combo_layout`, `frame`, `geometry`) chưa đối chiếu từng giá trị với C# `NativeDialogChromePainter.cs`.
- Hiệu năng/độ trễ khung và hành vi keep-warm trên GPU Intel UHD 630 (chỉ có thể đo trên máy thật).
