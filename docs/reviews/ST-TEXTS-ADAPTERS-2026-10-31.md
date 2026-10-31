# ST-TEXTS-ADAPTERS 2026-10-31

Lane `st-texts-adapters` (việc 4 của [ST-GAPMAP2](ST-GAPMAP2-2026-10-31.md)). Hàng `X-composition-texts-adapters`.

## Đã có (không lặp)

[CX-TEXTS](CX-TEXTS-2026-10-31.md) và [CL-TEXTS2](CL-TEXTS2-2026-10-31.md): `w8_texts_killers.rs` đã phủ `LocalizedFormat::new/default`, `with_group_separator`, `to_local_time` với hai thời điểm cố định, Debug của adapter, khoá cache và lỗi decoder qua handle preview.

## Test mới

File `crates/pr-shell/tests/composition_texts_adapters.rs` (3 test, chạy `cargo test -p pr-shell --test composition_texts_adapters`: 3 passed):

- `to_local_time_is_a_bounded_whole_minute_offset_independent_of_the_separator`: chênh lệch local - UTC nằm trong 14 giờ, là số phút nguyên, và không phụ thuộc dấu phân cách nhóm.
- `lazy_previews_forward_a_successful_viewer_preview_through_the_handle`: nhánh thành công của `start_viewer_preview` (ảnh 4x4 qua handle); test cũ chỉ có nhánh lỗi.
- `lazy_decoder_factory_builds_nothing_until_the_first_call_then_forwards_both_methods`: `LazyDecoderFactory` không resolve khi khởi tạo, resolve đúng 1 lần, chuyển tiếp `create` và `is_registered`.

Chưa chạy: mutation (người dùng hạn chế). Chỉ chạy đúng binary mới và `cargo fmt -p pr-shell`.

## Không làm được: nhánh lỗi registry

`LazyPrewarmPreviews::current_cache_key` (Err -> log + `SourceNoLongerExists`), `start_viewer_preview` (panic) và `LazyDecoderFactory::resolve` (log + panic) cần `services.preview_service()` / `decoder_factory()` trả `Err`. Đã đọc registry: `Err` chỉ từ `PreviewImageService::new` (thư mục cache trống, capacity <= 0, spawn thread) và `SourceBytesCache::new`. Với `AppPaths` luôn nối `cache`, capacity luôn `.max(1)` và `source_bytes_for_percent` chặn byte <= 0 trước khi tạo cache, không đầu vào fixture nào gây `Err`. Muốn kiểm thử cần seam sản xuất (ví dụ cho `SharedServiceInputs` một hook lỗi); việc đó thuộc loại 3, không làm ở lane này.

## Đề xuất PARITY (orchestrator quyết)

`X-composition-texts-adapters` (L672): giữ Partial. Thêm con trỏ tới báo cáo này (nhánh thành công của preview và `LazyDecoderFactory` đã có test); ghi rõ nhánh `Err` registry không tới được nếu không có seam.
