# Review tĩnh: chất lượng test (`crates/*/tests/`)

Machine C (cloud). Phạm vi: mọi tệp `crates/*/tests/**/*.rs` (không gồm test đơn vị trong `src/`). Phương pháp: quét bằng một script Python tạm (không chạy cargo, không chạy test), đếm hàm `#[test]`, đếm `assert*!`/`panic!`/`expect_err`/`unwrap_err`/`matches!` trong thân (đã bỏ comment `//`), loại các test uỷ quyền việc khẳng định cho hàm trợ giúp cùng tệp có assert, rồi đọc tay các ca đáng ngờ.

**Mọi phát hiện dưới đây: suy luận từ mã, chưa chạy thử.** Các con số là kết quả của regex, có thể lệch vài đơn vị (macro tự định nghĩa, `#[test]` nằm trong `mod` lồng, thuộc tính viết nhiều dòng).

Tổng: 0 cao / 4 trung bình / 3 thấp. Kèm bảng theo crate và 30 ví dụ rõ nhất (có file:dòng).

## Số liệu (suy luận từ mã)

| Crate | Test | `#[ignore]` | Không có assert (không ignore) | Không assert và không uỷ quyền |
|---|---:|---:|---:|---:|
| pr-app | 2560 | 17 | 136 | 2 |
| pr-core | 1961 | 143 | 129 | 29 |
| pr-platform-win | 2494 | 778 | 34 | 18 |
| pr-imaging | 1109 | 43 | 49 | 3 |
| pr-imaging-raw | 672 | 8 | 54 | 4 |
| pr-bench | 293 | 5 | 7 | 0 |
| pr-libraw | 322 | 36 | 14 | 1 |
| pr-turbojpeg | 224 | 66 | 5 | 4 |
| pr-shell | 141 | 59 | 3 | 1 |
| pr-l10n | 175 | 0 | 3 | 2 |
| pr-perf | 125 | 2 | 1 | 0 |
| pr-test-support | 27 | 0 | 0 | 0 |
| **Cộng** | **10103** | **1157 (11,5 %)** | **435** | **64** |

- 64 test không assert/không uỷ quyền gồm: khoảng 20 kiểm tra biên dịch đúng nghĩa (`*_is_send_and_sync`, `*_is_not_send`, `AmbiguousIfSend`) — hợp lệ; khoảng 15 kiểm tra "không panic" (`let _ = …`) — hợp lệ khi tên đúng như vậy; phần còn lại nêu ở dưới.
- 148 test `#[ignore]` cũng không có assert; trong đó 39 là chỗ giữ chỗ `todo!("W8-20b …")` (`pr-shell/tests/ui_smoke*.rs`, `ui_parity_map.rs`), 1 là thân rỗng.
- Lý do `#[ignore]` phổ biến: "native: needs desktop" (611), "native: needs WIC" (143), "native" (64), "needs turbojpeg.dll" (59), "needs the OS graphics stack" (46), "real files under TempRoot" (27).
- Không tìm thấy `assert!(true)` hay `assert_eq!(x, x)` theo kiểu vô ý; 4 ca `assert_eq!(a, a)` ở `pr-app/tests/input_frame_clock.rs:17,48`, `input_primitives.rs:107`, `verify_w8_01a.rs:255` là kiểm tra có chủ đích của `PartialEq` với NaN.
- Không tìm thấy `#[should_panic]` mới thêm bất thường; không có kiểm tra lịch sử git cho từng test (không so diff từng commit).

## Phát hiện

### M1 (trung bình, xác nhận): CI chạy `cargo test` không có `--ignored`; 1157 test không bao giờ chạy trên CI
- File: `.github/workflows/ci.yml` (job `test`, các lệnh `cargo test -p pr-core --locked`, `cargo test -p pr-platform-win --locked`, `cargo test --workspace --exclude … --locked`; không có `--ignored`/`--include-ignored`) trên `windows-latest`.
- Hệ quả: 778 test của `pr-platform-win` (31 %), 59/141 của `pr-shell` (42 %), 66/224 của `pr-turbojpeg`, 143 của `pr-core`… chỉ được chạy khi có người chạy tay. Nhiều test trong số đó không cần desktop (ví dụ "native: real files under TempRoot" 27 test ở `pr-core/tests/file_actions_journal.rs:610…`, "native: real files" 14 test ở `windowing_placement_store.rs`), chỉ cần ổ đĩa — chạy được trên `windows-latest`.
- Gợi ý: thêm một job (hoặc bước) `cargo test -- --ignored` cho nhóm không cần desktop/DLL, và giữ nhóm cần desktop chạy tay.

### M2 (trung bình, xác nhận): `#[ignore]` với lý do đã hết hiệu lực che các test chạy được
- Mỗi mục kiểm tra bằng cách đọc mã nguồn hiện tại:
  - `pr-core/tests/settings_shortcuts.rs:206` `serialize_review_action_uses_pascal_case_names` — "needs W1-02 serde_ext…"; `pr-core/src/model/serde_ext.rs` hiện không còn `todo!()` (W1-02 đã merge).
  - `pr-core/tests/file_actions_compaction.rs:581` `build_observational_equivalence_smoke_all_read_apis_agree` — "needs W3-01 OperationJournal"; `OperationJournal` đã được dùng bởi `pr-shell` registry.
  - `pr-imaging/tests/adaptive_preview.rs:182,190` — "needs W4-01b DecodeBox::fit"; `DecodeBox::fit` tồn tại (`pr-imaging/src/decode_types.rs:49`).
  - `pr-platform-win/tests/settings_framework.rs:673,767` — "SettingsPageBuilder is a stub (todo!)" và "page constructors are stubs"; builder và 7 trang đã cài đặt (xem `docs/reviews/settings-ui.md`).
- Hệ quả: 6 test (cộng cái do `pending_page…`/catalog thật) không chạy dù mã đã có; `real_create_pages_returns_the_seven_pages_in_nav_order` là test duy nhất xác nhận catalog thật.

### M3 (trung bình, xác nhận): test "đạt" mà không kiểm gì khi thiếu biến môi trường/dữ liệu
- `pr-app/tests/localization_export_verify_1.rs:12`, `:55`, `localization_export_verify_3.rs:19`, `localization_export_verify_5.rs:220,230`: `let Ok(path) = std::env::var("ESCAPED_TXT"|"ESCAPED_ALL"|"NOTES_DIR") else { return };` — không đặt biến thì kết thúc ngay và báo đạt. `vcmp_gen_kill_cases` (verify_5.rs:219) còn GHI tệp vào `NOTES_DIR` khi có biến (là bộ sinh dữ liệu, không phải test).
- `pr-shell/tests/startup_composition_root.rs:255` `host_registrations_are_all_mapped`: đọc `tests/golden/shell-composition-registrations.json` theo đường dẫn tương đối; `cargo test` chạy tại thư mục gói (`crates/pr-shell`), nơi không có thư mục đó → luôn `eprintln!("skip…"); return;`.
- `pr-platform-win/tests/render_keep_alive.rs:590`: nhánh `Ok(None)` (chỉ có WARP, thường gặp trên runner không GPU) chỉ `println!`.
- Rải rác trong `pr-imaging-raw` (`readers_rw2.rs`, `raw_reader_arw.rs`, `readers_cr2.rs`, `w5_18_integration.rs`) và `pr-libraw/tests/decoder_native.rs`: test corpus "skip when tests/fixtures/raw-corpus is absent" (có "strict mode" nhưng mặc định bỏ qua).

### M4 (trung bình, xác nhận): tên test hứa so khớp nhưng chỉ kiểm không panic / kiểm tối thiểu
- Nằm trong bảng 30 ví dụ (các mục 2-8, 10, 11, 22-23). Mẫu rõ nhất: `pr-turbojpeg/tests/decoder_pure.rs:694` `walker_random_and_corrupted_headers_match_pre_refactor_loops`; `pr-platform-win/tests/settings_pages_diag_perf.rs:15` `diagnostics_logging_round_trips`; `pr-platform-win/tests/dialogs_service.rs:497` `folder_picker_policy_golden_title_matches_catalog`.

### L1 (thấp, xác nhận): ghi chú `DIVERGENCE`/`TODO(lead)` đã lạc hậu, còn kỳ vọng bị đổi theo mã
- `pr-app/tests/settings_presenter_save.rs:513-516` `save_actions_json_is_validated`: "C# writes `"Operation":0`; the merged W8-10b row refuses a numeric Operation… so the rows use `"Operation":"Move"`". Hiện `pr-app/src/presenters/action_list_json.rs:117-123` đã chấp nhận số 0/1/2. Test vẫn dùng dạng tên (thay đổi kỳ vọng theo mã), nên đường `"Operation":0` của C# không được kiểm trong presenter (chỉ `action_list_json.rs:259` kiểm 99).
- `pr-core/tests/file_actions_undo.rs:11-19,1530`: `TODO(lead)` "RecoveryRetryService… not merged": `RecoveryRetryService` đã merge (`pr-core/src/file_actions/recovery_retry.rs`, có test riêng), nhưng ba dòng cuối của `UndoLastAsync_FailedGroupDeleteWhereTheRecycledMemberWasRestoredAndEdited_*` (retry không thành công, tệp vẫn tồn tại) vẫn chưa được chuyển, nên test thiếu các khẳng định cốt lõi của C#.

### L2 (thấp, xác nhận): ~39 test chỗ giữ chỗ `todo!()` đều `#[ignore]`
- `pr-shell/tests/ui_smoke.rs` (6), `ui_smoke_dialogs.rs` (7), `ui_smoke_overlay.rs` (6), `ui_smoke_startup.rs` (4), `ui_smoke_viewing.rs` (9), `ui_smoke_perf.rs` (1), `ui_parity_map.rs` (6, "stub: W8-20b red test not written yet"). Thân là `todo!("W8-20b S1: …")`; không chạy, không đo gì; hợp lệ làm hợp đồng nhưng được tính vào số test của crate.

### L3 (thấp, xác nhận): test chỉ khẳng định một hằng so với chính nó
- `pr-platform-win/tests/dialogs_task_dialog_native.rs:39` `assert_eq!(IDCANCEL, 2)` nằm trong một test native; không thể sai bởi hành vi.

## 30 ví dụ rõ nhất (theo crate)

| # | file:dòng | Vấn đề (suy luận từ mã) | Gợi ý |
|---|---|---|---|
| 1 | `pr-platform-win/tests/settings_page_mouse.rs:5` | `mouse_page_constructor_is_implemented`: chỉ `let _ = MouseSettingsPage::new();`; tên chú thích C# `Mouse_LoadsEveryProperty_AndSaveWritesThemBackUnchanged`. Trang có 19 trường + 18 ô menu, không test hành vi. | load → đổi → collect cho từng trường |
| 2 | `pr-platform-win/tests/settings_pages_diag_perf.rs:15` | `diagnostics_logging_round_trips`: gọi `host.put_text(id, "x")` rồi thôi, 0 assert, không load/collect. | kiểm `is_checked` sau load/collect |
| 3 | `pr-platform-win/tests/settings_pages_diag_perf.rs:62` | `focus_target_maps_the_two_preload_fields`: `let _ = p.focus_target(…)`, 0 assert. | `assert_eq!(…, Some(id))` cho hai trường |
| 4 | `pr-platform-win/tests/settings_builder_layout.rs:100` | `group_place_honours_a_hidden_child_and_a_hidden_nested_group`: `let _ = g.place(…)`. | khẳng định toạ độ/độ cao trả về |
| 5 | `pr-platform-win/tests/settings_builder_layout.rs:118` | `check_flow_does_not_skip_hidden_items`: `let _ = e.place(…)`, tên hứa kết quả cụ thể. | như trên |
| 6 | `pr-platform-win/tests/settings_builder_layout.rs:170` | `layout_arithmetic_saturates_at_i32_extremes`: `let _ = e.place(…)`; không khẳng định giá trị bão hoà. | `assert_eq!` trên kết quả |
| 7 | `pr-platform-win/tests/settings_builder_layout.rs:210` | `placed_rectangles_are_the_window_rectangles_and_siblings_do_not_overlap() {}`: thân rỗng, `#[ignore]`. | viết hoặc xoá |
| 8 | `pr-platform-win/tests/render_keep_alive.rs:590` | nhánh `Ok(None)` (WARP) chỉ `println!`; đạt giả trên máy không GPU. | `assert!` hoặc `return` có chủ đích + ghi nhận |
| 9 | `pr-platform-win/tests/dialogs_task_dialog_native.rs:39` | `assert_eq!(IDCANCEL, 2)`: hằng so với hằng; cả file `#[ignore]`. | bỏ dòng, giữ kiểm hộp thoại |
| 10 | `pr-platform-win/tests/shell_startup_locale.rs:95` | `formatted.contains("2020") \|\| formatted.contains("20")` — năm sai 3620 vẫn chứa "20" (xem `docs/reviews/startup.md` H1); test còn `#[ignore]`. | so chuỗi/năm đầy đủ |
| 11 | `pr-platform-win/tests/dialogs_service.rs:497` | `folder_picker_policy_golden_title_matches_catalog`: chỉ `assert!(!title.is_empty())`, tên nói "golden matches catalog". | so với chuỗi golden |
| 12 | `pr-platform-win/tests/settings_framework.rs:673` | `#[ignore]` "SettingsPageBuilder is a stub" — builder đã cài đặt (stale). | bỏ `#[ignore]` |
| 13 | `pr-platform-win/tests/settings_framework.rs:767` | `real_create_pages_returns_the_seven_pages_in_nav_order` `#[ignore]` "page constructors are stubs" — stale; là test duy nhất xác nhận catalog thật. | bỏ `#[ignore]` |
| 14 | `pr-shell/tests/startup_composition_root.rs:255` | `host_registrations_are_all_mapped`: golden theo đường dẫn tương đối không tồn tại ở thư mục gói → luôn "skip" và đạt. | dùng `CARGO_MANIFEST_DIR/../../tests/golden/…` |
| 15 | `pr-shell/tests/ui_smoke.rs:16` | `smoke_open_folder_shows_the_first_image_and_the_folder_title`: `todo!("W8-20b S1…")`, `#[ignore]` (cùng kiểu 38 test khác ở `ui_smoke_*.rs`). | thực hiện khi W8-20b |
| 16 | `pr-shell/tests/ui_parity_map.rs:20` | 6 test `todo!()` "W8-20b red test not written yet" — các kiểm tra cấu trúc thuần (không cần desktop) mà không chạy. | viết trước vì rẻ |
| 17 | `pr-app/tests/localization_export_verify_1.rs:12` | `escape_set_matches_dotnet_unsafe_relaxed_for_every_bmp_char`: `else { return; }` khi thiếu `ESCAPED_TXT` → đạt giả. | chuyển thành golden commit sẵn hoặc `#[ignore]` rõ ràng |
| 18 | `pr-app/tests/localization_export_verify_1.rs:55` | `read_notes_matches_dotnet_on_battery`: tương tự với `NOTES_DIR`. | như trên |
| 19 | `pr-app/tests/localization_export_verify_3.rs:19` | `escape_of_every_scalar_matches_dotnet_exactly`: tương tự với `ESCAPED_ALL`. | như trên |
| 20 | `pr-app/tests/localization_export_verify_5.rs:219` | `vcmp_gen_kill_cases`: bộ sinh dữ liệu (ghi tệp vào `NOTES_DIR`) nằm trong test; không có assert. | chuyển sang `tools/` |
| 21 | `pr-app/tests/settings_presenter_save.rs:513` | `DIVERGENCE` lạc hậu: kỳ vọng đổi sang `"Operation":"Move"` để khớp mã, mà mã nay chấp nhận số (`action_list_json.rs:117-123`); đường `"Operation":0` của C# không còn được kiểm. | đưa lại dạng số của C# |
| 22 | `pr-app/tests/services_contracts.rs:23` | `null_presentation_observer_ignores_every_path`: gọi `on_presented` 3 lần, không khẳng định "không làm gì". | kiểm không có tác dụng phụ (hoặc chấp nhận là smoke) |
| 23 | `pr-turbojpeg/tests/decoder_pure.rs:694` | `walker_random_and_corrupted_headers_match_pre_refactor_loops`: 2000 vòng `let _ = scan_header(...)`; tên nói "match", comment nhắc "golden rows" nhưng không có. | so sánh với vòng lặp cũ/golden |
| 24 | `pr-core/tests/settings_shortcuts.rs:206` | `#[ignore]` "needs W1-02 serde_ext" — W1-02 đã merge (stale); test serialize `ReviewAction` quan trọng cho tương thích C#. | bỏ `#[ignore]` |
| 25 | `pr-core/tests/file_actions_compaction.rs:581` | `#[ignore]` "needs W3-01 OperationJournal" — đã có (stale). | bỏ `#[ignore]` |
| 26 | `pr-core/tests/diagnostics_file_log.rs:574` | `use_after_dispose_never_throws`: dãy lời gọi sau `dispose()` không khẳng định gì (ví dụ không có dòng nào được ghi sau dispose). | đọc tệp log và so |
| 27 | `pr-core/tests/file_actions_undo.rs:1530` | `TODO(lead)`: thiếu retry + `Assert.False(retry.Succeeded)` + `FileExists(Jpeg)` của C#, dù `RecoveryRetryService` đã merge. | chuyển nốt 3 dòng |
| 28 | `pr-imaging/tests/adaptive_preview.rs:182` | `#[ignore]` "needs W4-01b DecodeBox::fit" — `fit` đã có (stale). | bỏ `#[ignore]` |
| 29 | `pr-imaging/tests/adaptive_preview.rs:190` | như trên (hàng portrait). | như trên |
| 30 | `pr-imaging-raw/tests/preview_selector_overflow.rs:31` | `near_i64_max_offset_does_not_panic`: `let _ =` ba lời gọi; chỉ kiểm không panic, không kiểm kết quả `None`/`Err`. | `assert!(… .is_none())` |

## Không kiểm tra được
- Không chạy cargo/test; chưa xác minh các test `#[ignore]` có thật sự chạy được hay không.
- Con số quét bằng regex (xem đầu tệp) có sai số; chưa đọc từng test một. Các test uỷ quyền khẳng định cho hàm trợ giúp ở tệp khác (`tests/common/*`, `pr-test-support`) bị đếm vào "không assert" ở cột 4 nhưng được loại ở cột 5 chỉ khi trợ giúp nằm cùng tệp.
- Không so lịch sử git từng test để biết kỳ vọng nào bị sửa cho khớp mã thay vì C# (chỉ đọc các ghi chú `DIVERGENCE`/`TODO` trong chính tệp và các commit tổng quát như #179 "strengthen vacuous and weak tests").
- Test đơn vị trong `src/` (`#[cfg(test)]`) không thuộc phạm vi.
