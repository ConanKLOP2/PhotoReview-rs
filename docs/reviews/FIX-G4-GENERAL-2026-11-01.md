# FIX-G4-GENERAL (2026-11-01)

Trạng thái: đã sửa (chờ CI).

## Lỗi
`GeneralSettingsPage::collect` (`crates/pr-platform-win/src/dialogs/settings/pages/general.rs`) gọi `set_combo_index` (ghi) cho View mode và Instance mode thay vì đọc `combo_index` vào state. Bấm Save làm mất lựa chọn. C# baseline `7eaee2a2` `GeneralSettingsPage.cs:131-132` đọc `Context.ComboIndex(...)` vào `state.ViewModeIndex`/`InstanceModeIndex`. Nguồn: ST-BACKLOG-NOW G4, `settings-ui.md` H1.

## Thay đổi
- Sửa: 2 dòng trong `collect` đọc combo vào state.
- Test mới: `collect_reads_view_and_instance_mode_from_the_combos` trong `crates/pr-platform-win/tests/settings_page_general.rs`.

## Bằng chứng
- Trước khi sửa: test mới THẤT BẠI (`left: 0, right: 1` tại assert view_mode).
- Sau khi sửa: `cargo test -p pr-platform-win --test settings_page_general -j 2` = 13 pass, 0 fail. `cargo fmt -p pr-platform-win --check` sạch.
- Không chạy clippy/workspace (theo giới hạn build); chờ CI.

## Hàng PARITY nên đổi
Chưa xác định chính xác hàng trang Settings General trong docs/PARITY.md (không sửa file này theo quy tắc lane). Nếu hàng General/Settings pages có ghi lỗi H1/G4 thì bỏ ghi chú đó; không nâng Done chỉ vì bản sửa này (các trang Display/Files/Mouse/Performance/Diagnostics vẫn thiếu test load/collect).
