# CX-TESTGAPS-STALE2 (2026-11-01): port tiếp `StalePreviewOverwriteTests`

Lane `cx-testgaps-stale2`, tiếp nối `CX-TESTGAPS-G19-2026-11-01.md` (PR #543: 3/17 test). Đối chiếu C# `7eaee2a2:tests/PhotoReview.App.Tests/Coordinators/StalePreviewOverwriteTests.cs`. Fakes only, không sleep; thứ tự hoàn tất do cổng + `pump` điều khiển; "worker đã xong" là bộ đếm trong decoder giả.

## Test đã thêm (9) - `crates/pr-app/tests/g19_stale_preview2.rs`

| Test Rust | Test C# |
|---|---|
| `clear_cache_during_decodes_current_navigation_still_presents_and_nothing_stale_is_cached` | `ClearCacheDuringDecodes_...` (cache_count == 0) |
| `superseded_running_decode_finishing_late_is_cached_for_a_later_visit_and_never_shown` | `SupersededRunningDecode_FinishingLate_...` (cache theo khóa riêng, quay lại không decode lần 2) |
| `late_decode_failure_of_an_older_navigation_neither_shows_an_error_nor_touches_the_catalog` | `LateDecodeFailureOfAnOlderNavigation_...` |
| `late_zoom_original_after_navigating_away_changes_nothing_on_screen` | `LateZoomOriginal_AfterNavigatingAway_...` |
| `zoom_original_of_the_current_image_replaces_the_preview_then_navigation_releases_it` | `ZoomOriginal_OfTheCurrentImage_...` |
| `navigation_start_releases_the_previous_zoom_original_at_once` | `NavigationStart_ReleasesThePreviousZoomOriginalImmediately` (kiểm ngay sau `present`, chưa pump) |
| `compare_partner_of_a_superseded_pair_finishing_late_changes_nothing` | `ComparePartnerOfASupersededPair_...` |
| `compare_partner_failing_late_of_a_superseded_navigation_changes_nothing` | `ComparePartnerFailingLate_...` |
| `compare_pair_overwritten_by_a_newer_pair_late_cell_of_the_old_pair_is_dropped` | `ComparePairOverwrittenByANewerPair_...` |

Cùng PR #543: 3 test (burst x2, thumbnail muộn). Tổng 12/17 test C#.

## Kết quả đã chạy

- `cargo test -p pr-app --test g19_stale_preview2 -j 2`: 9/9; chạy lại đúng binary 5 lần: 5/5 pass, không flake.
- `rustfmt` file mới sạch. Chưa chạy clippy/workspace (chờ CI).
- Không phát hiện lỗi production: mọi bất biến INV-1 giữ ở cả 9 tình huống.

## Khác biệt so với C# (trung thực)

- Presenter Rust coi "chưa idle" khi còn job (decode partner, zoom original) bị chặn, nên không `pump_until_idle` trước khi mở cổng muộn. Vì vậy các test muộn không so "đếm sự kiện trước/sau"; chúng kiểm trạng thái cuối (không có ảnh/status/path của điều hướng cũ, compare đúng, catalog nguyên) sau khi bộ đếm decoder báo xong rồi mới `pump_until_idle`.
- Test zoom muộn: zoom vẫn 2.0 nên ảnh mới cũng tự arm original riêng (không gate) - kiểm không có ảnh nào của file cũ tới sink sau ảnh mới.
- Có một khe đua còn lại: sự kiện hoàn tất muộn có thể chưa vào hàng đợi lúc `finished` được ghi, nhưng `pump_until_idle` chờ job đang chạy nên khép lại; chưa quan sát thấy flake trong 5+1 lần chạy. Chưa mutation-test (bị cấm).

## Chưa port (5/17 C# còn lại)

`Burst_TwoFiles_EveryThumbnailAndPreviewOrder`, `Burst_ThreeFilesWithThumbnails_SeededSampleOfOrders` (cần tín hiệu thumbnail theo thứ tự; vét cạn), `JoinedPreloadDecodes_FinishingInReverseOrder` (cần preload thật; fixture dùng `RecordingPreload`), `PostPresentStat_OfASupersededNavigation_...` và `HeaderReadFinishingLate_...` (cần `ScriptedFs` giữ lần stat thứ hai / `read_info` bị giữ; `presenter_present_b.rs` có test supersede gần tương đương).

## Hàng PARITY nên đổi (không sửa trong PR này)

- Hàng presenter INV-1 / StalePreview: nâng ghi chú từ "3/17" lên "12/17 test C# đã port (g19_stale_preview.rs + g19_stale_preview2.rs) + test supersede sẵn có"; chưa nâng Done.
- G20 trong backlog: cập nhật số 12/17 và danh sách 5 test còn thiếu ở trên.
