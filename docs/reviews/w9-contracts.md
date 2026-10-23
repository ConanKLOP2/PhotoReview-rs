# Review tĩnh: contract Wave 9 so với mã C# (W9-E1..E9, W9-R1..R11)

Machine C (cloud). Phạm vi đọc:
- `contracts/W9-E1..E9.md` và `contracts/W9-R1..R11.md` (toàn văn, kể cả các đoạn "Numbered C# source excerpt"), `docs/WAVE-9-PLAN.md`, `docs/WAVE-9-PLAN-edt.md`, `docs/WAVE-9-PLAN-ren.md`.
- C# tại `/home/user/conanklop2/photoreview` (HEAD = baseline `7eaee2a2`; `git diff d92d6aae 7eaee2a2` dùng để biết đoạn nào thật sự đổi): `src/` và `tests/`.
- Rust trên `master` (`c40b59d`): `crates/*` (stub của các đơn vị W9, phần đã merge của W8, và PR #182 `c4f7329` "move the spec baseline to 7eaee2a2"), `tests/golden*`, `tools/oracle-cs`, `assets/languages/en.json`/`vi.json`.

**Mọi phát hiện dưới đây: suy luận từ mã, chưa chạy thử.** Không chạy cargo, không chạy test, không chạy oracle. Chỉ sửa tài liệu (`contracts/*.md`, hai dòng của `docs/WAVE-9-PLAN-ren.md`); không đụng mã Rust/C#.

Công cụ dùng ngoài đọc tay: ba script Python chạy trên bản sao cục bộ (đối chiếu từng dòng của mọi đoạn trích với tệp C#, kiểm tra tên test có trong đúng lớp/tệp, so khoảng dòng với hunk của `git diff`). Kết quả của chúng được ghi lại trong các mục bên dưới; script không nằm trong repo.

Tổng: 1 cao / 5 trung bình / 11 thấp. Đã sửa 27 chỗ trong 13 tệp (mục "Đã sửa trong contract").

## Đã đọc và thấy đúng
- **Đoạn trích C# trong contract E1..E9 khớp từng dòng**: 2397 dòng "Numbered C# source excerpt" (E1 389, E2 408, E3 207, E4 426, E5 292, E6 193, E7 139, E8 198, E9 145) so với tệp C# cùng số dòng: 0 dòng lệch. Các contract R không có đoạn trích.
- **Tên test C# đều có thật**, đúng tệp và đúng lớp: mọi tên trong bảng "C# tests to port"/"Complete dedicated C# test inventory" của E1..E9, R1..R11 (trừ `ImagePresenterRetargetTests` là tên tệp chứ không phải tên lớp, xem L4). Số lượng khớp tệp: `ReviewCatalogRenameTests` 28, `CaptureGroupRenamePlanTests` 16, `FileActionControllerRenameTests` 20, `FileActionControllerRenameUndoTests` 8, `ImagePresenterRetargetTests` 13, `ZoomDetailLoaderRetargetTests` 10, `MainViewModelRenameTests` 22, `RenamePromptDialogTests` 12 ("All twelve" đúng, `ValidNames_EnableOk_AndAreReturnedTrimmed` là Theory), `RenameFileSettingsTests` 10, `RenameFileShortcutRouterTests` 7, `RenamePromptPresenterTests` 10, `RenameFileMenuModelTests` 9, `RenameNamePolicyTests` 31 (kể cả Theory `Check_LoneSurrogate_...`), `RenameFileActionTests` 42 (kể cả hai Theory `CaseOnlyRename_RecoveryVerdict_...`, `..._StartupReconcile_...`).
- **Khoảng dòng C# trong bảng E3, E4, E5, E7, E8, E9 đúng** (đã mở và đối chiếu: `ListDialog.cs` 24-25/39-50/79-80/154-157/160-165, `SettingsFieldMap.cs` 61/186-206/418, `SettingsPresenter.cs` 298-301, `ShortcutsSettingsPage.cs` 70-71, `SettingsPageContext.cs` 164-186, `SettingsDialog.cs` 54-58, `MainViewModel.cs` 774-881 từng mục, `ReviewCommand.cs` 38-39/61-62/101-102, `ShortcutRouter.cs` 38/84/278-282, `Win32DialogService.cs` 40-50/69-71/165-169, `Win32SecondaryWindowPort.cs` 82-85/107-110/242-295/428-436, `ContextMenuController.cs` 57-69/92-95/179-182, `ShellMainWindow.Menus.cs` 71-75, `ShellMainWindow.Input.cs` 215-217).
- **Khoảng dòng của contract R phủ đúng các hunk thật** của `git diff d92d6aae 7eaee2a2 -- src` cho mọi tệp rename (R1..R8), không bỏ sót hunk nào; chỉ có hai khoảng vượt quá cuối tệp (L1). Mọi tệp C# bị đổi trong `src/` đều có đơn vị chủ (ngoại lệ có chủ ý: `Localization/*.json` đã đồng bộ ở #182, `SettingsWindow.xaml.cs` là WPF, `ActionListJson.cs`/`LenientEnumConverter.cs` đã nêu trong R7 và đã port ở #182).
- **Hành vi mô tả khớp mã C#**: `ExternalEditor.cs` (E1: thứ tự duyệt RAW `Order(OrdinalIgnoreCase)`, `Reconcile` chỉ tính "repair" khi sửa danh sách có sẵn, `BuildArguments` một lượt trái sang phải), `RenameNamePolicy.cs` (R1: bậc `>= 260 && > độ dài đường dẫn hiện tại`, `CheckFileName` từ chối cả dấu cách cuối, tên thiết bị gồm `COM¹²³`/`LPT¹²³`, `ThrowIfNullOrWhiteSpace` cho đường dẫn rỗng), `FileActionService` (R2: từ chối rename map trên group không phải Move ở dòng 96-98), `Win32SecondaryWindowPort` (E8: `Run` trả `IdCancel` khi đã dispose nên không cần kiểm tra riêng), bước 12f và 9b của `ShortcutRouter` (E7, R7: Rust `shortcut_router.rs` đã có 9b đúng "không modifier").
- **Giá trị cố định đúng**: bố cục E2 (`Dx(470),Dy(190)`, tối thiểu `Dx(380)/Dy(170)`, cột trái `clamp(w/3, Dx(100), Dx(170))`), E3 (`Dx(220)`, `Dx(12)`, `Dy(14)`), `RAW_EXTENSIONS` Rust = 8 đuôi như C# `s_rawExtensions`, `Win32_Storage_FileSystem` đã là feature của `pr-platform-win` (E3 không cần sửa `Cargo.toml`).
- **Golden/oracle**: `external-editor-rules` 77 ca, `external-editor-menu` 18 ca, `open-in-default-editor-shortcut` 13 kịch bản + ca `auto_repeat`, `w9-r1-rename-policy` 44 ca, `dialog-external-editors` có; cả năm có `csharp_commit` = `7eaee2a2`; các tệp `tools/oracle-cs/Commands/W9-E1/E2/E6/E7/R1.cs` có; golden `secondary-window-supports` đã liệt kê `ExternalEditors` và `RenamePrompt` (id 8, 9).
- **Phụ thuộc W8 đúng như contract nói**: `zoom_prompt.rs` (W8-18p), `action_profiles.rs` (W8-18q), `secondary_window_port.rs` (W8-18o), `settings/dialog/{frame,host}.rs` (W8-19j..m) vẫn là `todo!()`; các hàm/kiểu E-lane gọi có thật: `ReviewCatalog::find`, `SettingsSource::{begin_edit, commit_edit}`, `show_settings_at`, `io::physical::new_guid_n`, `text::dotnet_path::{get_directory_name, get_file_name_without_extension}`, `text::{eq_ignore_case, cmp_ignore_case}`, `catalog::types::{RAW_EXTENSIONS, extension_in, get_extension}`, `ExternalEditorLauncher::launch` ba tham số, `FileSystem::get_stored_file_names` (thân mặc định đúng như C#), `DialogService::prompt_*_external_editor`, `FileActionResult.no_change`, `CaptureGroupActionRequest.destination_file_names`, `IFileActionExecutor::{rename, rename_group}`, `IFileActionSink::on_files_renamed`, `ImagePresenter::{retarget_path, is_present_in_flight}`, `ZoomDetailLoader::retarget_path`, `MainViewModel::{rename_current, on_files_renamed, prompt_rename_current}`, `ImageCacheKey::with_path`, `PreviewImageService::evict_cached_path`. Tên "bịa" duy nhất còn sót trong contract (`can_apply`, `edit_selected`, `resolve_open_in_default_editor`, `dispatch_external_editor_menu`, `compute_for_shell`) đều nằm trong câu "bản nháp đầu đã bịa ... đã xoá", không phải API đòi hỏi.
- Tất cả khoá ngôn ngữ mà E2, E3, E4, E6, E8, R7 nêu (`externalEditors.*`, `dialog.externalEditor.*`, `settings.externalEditors.*`, `settings.shortcut.openInDefaultEditor*`, `main.menu.externalEditor.*`, `dialog.rename.error.*`, `core.fileAction.renameInvalidName`) có trong `assets/languages/en.json` và `vi.json`.

## Đã sửa trong contract
Mỗi mục: tệp, trước -> sau, bằng chứng. (Số trong ngoặc là mã phát hiện liên quan.)

1. `contracts/W9-E1.md` (4 chỗ, H1):
   - Câu "Only the struct fields are added: the JSON key list, reader, writer, `CURRENT_CONFIG_VERSION`, `PROPERTY_NAMES`, `OPTIONAL_NAMES`, getters/setters ... are the porter's work" -> ghi trạng thái thật sau #182: những thứ đó ĐÃ xong, còn lại là thân `external_editors.rs` (toàn `todo!()`), dòng reconcile của normalizer, `save`/`migrate` của store, test. Bằng chứng: `app_settings.rs:50` (`CURRENT_CONFIG_VERSION: i32 = 4`), `:632-634` và `:970-1002` (`read_external_editors`), `:808-833` (writer, đúng thứ tự `Id, Name, Path, Arguments`); `shortcuts.rs:80-131,172-173,210-211,258-259`; `normalizer.rs`/`store.rs` không có `ExternalEditors`/`reconcile`.
   - Mục 2 (AppSettings) và mục 3 (ShortcutMappings) của "Work list": thêm "đã làm bởi #182; giữ làm đặc tả để đối chiếu"; `OPTIONAL_NAMES` 15 -> 16 vì `RenameFile` đã vào cùng lúc (`shortcuts.rs:115`).
   - Mục 4 (Normalizer): bỏ "the trim line" khỏi việc phải code. Bằng chứng: `normalizer.rs:193-195` duyệt cả `ShortcutMappings::OPTIONAL_NAMES` để `trim`, và danh sách đã có `OpenInDefaultEditor`; chỉ cần test (cùng lập luận R7 đã dùng cho dòng C# 148).
2. `contracts/W9-E2.md` (2 chỗ, H1): "all new ... not yet in `assets/languages/en.json`: the catalog sync is a separate lead unit" -> "có trong `en.json`/`vi.json` từ #182, `pr_l10n::tr` sinh accessor"; "Until the sync lands the native tests stay `#[ignore]`" -> test native vẫn `#[ignore]` vì cần desktop. Bằng chứng: mọi khoá liệt kê trong contract đều có trong cả hai catalog (script kiểm).
3. `contracts/W9-E4.md` (5 chỗ, H1, M5):
   - "The page files are W8-19f / W8-19i **stubs** (`todo!()`)" -> `pages/files.rs` và `pages/shortcuts.rs` là mã ĐÃ merge (0 `todo!()`; `files.rs` mới chỉ có ô đường dẫn d92d6aae, `shortcuts.rs` đã có `ACTION_ROWS` 8 hàng gồm `OpenInDefaultEditor` và `RenameFile`); chỉ `dialog/frame.rs` và `dialog/host.rs` còn là stub (88 và 32 `todo!()`). Thêm: hàng bốn thuộc tính của trình soạn thảo và `load_editor`/`apply_editor` đã có từ #182 (`settings_field_map.rs:354-362, 877-889`) nhưng sai ngữ nghĩa C# (xem M5); `apply_defaults` đã reset ba trường (`settings/load.rs:434-437`).
   - Depends-on: "W8-19f (stub), W8-19i (stub)" -> "(merged)".
   - "`covered_properties()` goes from 59 to 62" và "`ACTION_ROWS` 6 -> 7 -> 8": thêm "đã đúng trên master" (`tests/settings_field_map.rs:601` kỳ vọng 47, `:989` kỳ vọng 62; `shortcuts.rs:136`).
   - Bẫy "keys arrive with the catalog sync" -> "đã có trong catalog đồng bộ (#182)".
4. `contracts/W9-E5.md` (1 chỗ, L5): `catalog.find(shown).capture_group().raw_path()` -> `catalog.find(shown).and_then(|e| e.capture_group.as_ref()).map(|g| g.raw_path())`, kèm ghi chú `CatalogEntry::capture_group` là trường `Option<CaptureGroup>` (`pr-core/src/catalog/types.rs:273`), không phải phương thức; `ReviewCatalog::find` ở `review_catalog.rs:209`.
5. `contracts/W9-E6.md` (1 chỗ, H1): "the catalog sync is a separate lead unit ... E6 is blocked on that sync" -> khoá đã có trong catalog (#182), E6 không bị chặn bởi đồng bộ.
6. `contracts/W9-E7.md` (3 chỗ, H1, L2):
   - Tệp test `crates/pr-app/tests/input_shortcut_router.rs` (không tồn tại) -> `crates/pr-app/tests/shortcut_router.rs` (tệp router test có thật, đã có ở #182).
   - Mục 1 ("`ignores_auto_repeat`: add `OpenInDefaultEditor` ... becomes 15"): đã làm ở #182 (`review_command.rs`: `OpenInDefaultEditor | RenameFile` trong `ignores_auto_repeat`, doc "16 kinds", enum "0..=31"); chỉ cần đối chiếu với ca `auto_repeat` của golden.
   - Mục 2: "`Bindings` ... 27 -> 28" -> hiện `Bindings` có `rename_file` (từ #182) nhưng chưa có `open_in_default_editor`; việc còn lại giữ nguyên.
7. `contracts/W9-E9.md` (1 chỗ): "needs the E6 builder and the catalog sync" -> "(khoá catalog đã đồng bộ, #182)".
8. `contracts/W9-R2.md` (1 chỗ) và `docs/WAVE-9-PLAN-ren.md` (2 khoảng, cùng lỗi) (L1): `CaptureGroupActionRequest.cs:11-15` -> `11-14` (tệp 14 dòng); `FileActionResult.cs:23-27` -> `23-25` (tệp 25 dòng). Bằng chứng: `wc -l`, và `git diff -U0` cho hunk `(11,14)` và `(23,25)`.
9. `contracts/W9-R6.md` (1 chỗ, L4): "ImagePresenterRetargetTests: ..." -> "ImagePresenterRetargetTests.cs (members of the partial class `ImagePresenterTests`): ..." (`ImagePresenterRetargetTests.cs:21` khai báo `partial class ImagePresenterTests`).
10. `contracts/W9-R7.md` (3 chỗ, H1, M6):
    - Đoạn "Data-table edits (...) are NOT in the stub: ... the port changes them together with the tests" -> ghi rằng mọi bảng dữ liệu đó ĐÃ nằm trên master từ #182 (kèm vị trí), phần còn lại của R7 là `RenamePromptPresenter` (toàn `todo!()`), `prompt_rename_current` (stub), test Rust, và các thành viên còn thiếu (M6). Bằng chứng: `context_menu.rs` (`RenameFile = 18`, `shell_only`, `include_shell_only`), `shortcuts.rs:75,115-131`, `review_command.rs`, `shortcut_router.rs` bước 9b, `builder.rs` (`include_shell_only: true`, arm `RenameFile`), `settings/apply.rs` (arm `renameFile`), `settings_field_map.rs:145`.
    - "Golden-pinned settings cases stay at `d92d6aae` until the lead advances `SYNC-LEDGER` ... **Product/lead decision: baseline advance order.**" -> "(Resolved: baseline đã ở `7eaee2a2` nhờ #182, golden và kỳ vọng RenameFile đã cập nhật.)" Bằng chứng: `docs/SYNC-LEDGER.md` dòng "Current spec baseline: ... `7eaee2a2`".
    - Thêm vào "Review amendments" thành viên thiếu: `InitialText`, `Extension` (xem M6).
11. `contracts/W9-R8.md` (2 chỗ, H1, L3):
    - `secondary_window_port.rs:177` -> `:225-228` (comment và thân `supports`).
    - "`EN_CHANGE` ... define it once in `rename_prompt.rs`" -> dùng `win32/dialog_native.rs:33` (`pub const EN_CHANGE: i32`, #182 thêm, là bản sao của `DialogNative.EnChange` mà contract liệt kê ở dòng nguồn đầu tiên); thêm ghi chú các việc đã xong (#182): arm `"RenameFile"` của `shell_menus/controller.rs:213`, arm `RenameFile` của `shell_input/main_window.rs:267`, hai hàng của `pages/shortcuts.rs`; `pages/mouse.rs` đi qua `context_menu_rows` duyệt cả `ITEMS` (19 mục) nên hành vi `includeShellOnly: true` đã đúng, R8 chỉ thêm test.
12. `contracts/W9-R9.md` (1 chỗ, M4): thêm `TwoRenames_Matrix` (`RenameFaultInjectionTests.cs:93`) vào danh sách test; tệp có 12 test nhưng danh sách chỉ có 11.

Kiểm tra lại sau khi sửa: `git diff --stat` chỉ gồm `contracts/W9-*.md` và `docs/WAVE-9-PLAN-ren.md`, thêm báo cáo này.

## Cần quyết định (chưa sửa)

### Phát hiện

### H1 (cao, xác nhận bằng mã): trạng thái "Rust đang có/thiếu gì" trong contract lỗi thời sau #182
- Đơn vị chủ: W9-E1, W9-E4, W9-E7, W9-R7, W9-R8 (và câu về catalog ở W9-E2, E4, E6, E9). Mã: w9-contracts H1.
- Contract được viết ở #165/#176 (baseline d92d6aae). #182 (`c4f7329`, "move the spec baseline ... port the small behaviour changes") đã port: phiên bản config 4 + ba trường + JSON đọc/ghi, `OpenInDefaultEditor` và `RenameFile` trong `PROPERTY_NAMES`/`OPTIONAL_NAMES`/getter/setter, `ReviewCommandType::RenameFile` + `ignores_auto_repeat` cho cả hai lệnh, bước 9b của router, `ContextMenuItemId::RenameFile` + `shell_only`/`include_shell_only`, arm menu builder, arm `settings::apply`, hàng bốn thuộc tính của field map, hai hàng `ACTION_ROWS`, `EN_CHANGE`, arm `RenameFile` của controller; đồng bộ catalog ngôn ngữ; sinh lại golden.
- Kịch bản: porter R7 làm theo đoạn "Data-table edits ... NOT in the stub, the port changes them" và thêm lại `RenameFile = 18`/`rename_file`/hàng bảng → biến thể và mục trùng lặp (lỗi biên dịch hoặc bảng lệch) và đổi các số đếm đã pin trong test; porter E1 viết lại đọc/ghi JSON đã có; porter E4 coi `files.rs`/`shortcuts.rs` là stub rồi chờ; porter E7 thêm `OpenInDefaultEditor` lần hai vào `ignores_auto_repeat`.
- Đã sửa văn bản ở E1, E2, E4, E6, E7, E9, R7, R8 (xem trên). Chưa sửa: `docs/WAVE-9-PLAN-edt.md` bảng "Survey" và mục "Dependencies on W8 units that are still stubs" cũng còn câu cũ (AppSettings "missing (stub adds the three fields only)", catalog "missing", W8-19f/19i "still stubs"); `docs/WAVE-9-PLAN.md` mục 4 đã nói đúng (W8-19f/19i trong lịch sử). Gợi ý: chủ kế hoạch cập nhật bảng Survey cùng ngày, hoặc thêm một dòng "xem review w9-contracts H1".

### M1 (trung bình, xác nhận): không đơn vị nào sở hữu `SharedDialogService` nên các prompt mới không tới được cổng
- Đơn vị chủ: W9-E8 (và W9-R8 cho `prompt_rename`; ảnh hưởng W9-E5, W9-E9). Mã: w9-contracts M1. Cùng gốc với `startup` M1.
- `crates/pr-shell/src/composition/adapters.rs:47-90`: `impl DialogService for SharedDialogService` chỉ chuyển 11 phương thức; `prompt_add_external_editor`, `prompt_default_external_editor`, `prompt_rename` rơi về mặc định `None` của trait. `MainViewModelDeps` nhận `Arc<dyn DialogService>` bọc `SharedDialogService` (contract `W8-09b` D-W8B6-153). Không contract W9 nào liệt kê `adapters.rs` trong "Owned files" (grep `SharedDialogService` trong `contracts/W9-*` không ra gì); E8 chỉ nhắc "composition root adapter (W8-17c, P-43)" ở Downstream.
- Kịch bản: E8 xong, `Win32DialogService` chuyển tiếp đúng, nhưng trong ứng dụng thật `MainViewModel::add_external_editor` gọi qua `SharedDialogService` → luôn `None` → "Add more editors…", "Set default editor…" và F2 coi như huỷ, im lặng.
- Đề xuất: thêm `crates/pr-shell/src/composition/adapters.rs` (ba phương thức chuyển tiếp, và một test kiểm mọi phương thức của trait được chuyển) vào Owned files của E8 (hai phương thức editor) và R8 (`prompt_rename`), hoặc giao cho lead như "shared wiring". Chưa sửa vì là quyết định về quyền sở hữu tệp.

### M2 (trung bình, xác nhận): ánh xạ đường dẫn so sánh không phân biệt hoa thường được stub khai báo bằng `HashMap<String, String>`
- Đơn vị chủ: W9-R1, W9-R2, W9-R4, W9-R5, W9-R6. Mã: w9-contracts M2.
- C# `CaptureGroupRenamePlan.Plan.FileNames`, `CaptureGroupActionRequest.DestinationFileNames`, `ReviewCatalog.RenameEntries(map)`, `IFileActionSink.OnFilesRenamed(map)`, `ImagePresenter.RetargetPath(map)` dùng từ điển `OrdinalIgnoreCase` (`CaptureGroupRenamePlan.cs`: `new Dictionary<string,string>(StringComparer.OrdinalIgnoreCase)`). Test `Build_MapKeysCompareIgnoringCase` (`CaptureGroupRenamePlanTests.cs:162-169`) gọi `ContainsKey(Dir.ToUpperInvariant() + "IMG.JPG")` và `[Dir + "img.CR2"]`.
- Rust stub: `rename.rs` `file_names: Option<HashMap<String, String>>` (chú thích "path keys use OrdinalIgnoreCase semantics when implemented" nhưng không có cơ chế), `types.rs:137`, `catalog/rename.rs:12`, `traits.rs:159`, `presenter/rename.rs:16`: đều `std::collections::HashMap` phân biệt hoa thường. Không contract nào quy định kiểu bản đồ hay chuẩn hoá khoá.
- Kịch bản: test Rust cùng tên (`build_map_keys_compare_ignoring_case`) không thể qua với `HashMap` thô; ở đường chạy thật, bất kỳ nơi nào tra bản đồ bằng đường dẫn viết khác hoa/thường với khoá (ví dụ đường dẫn chuẩn hoá từ một nguồn khác với đường dẫn lưu trong catalog) sẽ không thấy khoá, và thành viên đó bị bỏ qua (suy luận, chưa thấy đường gọi cụ thể vì các thân hàm còn `todo!()`).
- Đề xuất quyết định: một kiểu dùng chung (ví dụ bản đồ khoá chuẩn hoá bằng `text::cmp_ignore_case`/khoá `to_uppercase_invariant`) đặt ở `pr-core`, do R1 sở hữu, R2/R4/R5/R6 dùng; hoặc ghi rõ trong R1 rằng mọi tra cứu phải chuẩn hoá khoá trước. Nếu giữ `HashMap`, loại test `Build_MapKeysCompareIgnoringCase` khỏi danh sách và nói lý do.

### M3 (trung bình, xác nhận): stub `RenamePromptDialog` không đủ để R11 và theo khuôn khác với E2/E3
- Đơn vị chủ: W9-R8, W9-R11. Mã: w9-contracts M3.
- `crates/pr-platform-win/src/dialogs/rename_prompt.rs` (32 dòng): `RenamePromptDialog { request }` với `new(request)` và `show(&mut self) -> Option<String>`; contract R8 gọi đó là "compiling stub is binding". C# `RenamePromptDialog : NativeDialog` (`RenamePromptDialog.cs:13-77`) có hàm tạo `(request, INativeDialogChrome?)`, thuộc tính `Value`, `ErrorMessage`, `IsRenameEnabled`, `Selection`, `CurrentText`, `ExtensionText` mà `RenamePromptDialogTests` (R11, 12 test) đọc trực tiếp (ví dụ `Opens_WithTheStemSelected...`, `InvalidNames_AreRefusedInPlaceWhileTyping_OkIsDisabled...`).
- Khác với E2/E3 (`DialogContent` + bao `NativeDialog` có `native()`, `outcome()`/`value()`), `show()` không có `owner: Hwnd`, không có chrome, và nằm ở `dialogs/` chứ không phải `dialogs/native/` như `TextPromptDialog`.
- Kịch bản: R11 không thể ghi test vị trí chọn/nút Rename/lỗi nếu không có truy cập; porter R8 buộc phải đổi chữ ký mà contract nói là "binding".
- Đề xuất: R8 nêu rõ chữ ký thật theo mẫu `text_prompt_dialog.rs` (nội dung `DialogContent` + bao có `value()`, `error_message()`, `is_rename_enabled()`, `selection()`, `current_text()`, `extension_text()`; `show` nhận owner và chrome) và đánh dấu stub hiện tại là tạm.

### M4 (trung bình, xác nhận): bộ kiểm bất biến của ma trận lỗi cần nhận biết cách viết hoa, nhưng không đơn vị nào sở hữu
- Đơn vị chủ: W9-R9. Mã: w9-contracts M4.
- C# FEAT-RENAME sửa ba tệp hỗ trợ: `FaultInjectionMatrix.cs` (bỏ qua `NoChange: true`; `SourceStillThere` theo tên lưu trữ), `FaultInvariants.cs` (`SourceStillThere(rig, source, destination)` dùng `GetStoredFileNames`; luật `movedOn` cho dòng journal đã bị dòng sau di chuyển tiếp), `FaultInjectionRig.cs` (`Alias`). Contract R9 chỉ nói loại lỗi `rename` của fake FS "dựng trong `rename_fault_injection.rs`".
- Rust: các bất biến nằm trong `crates/pr-core/tests/file_actions_matrix.rs` (ví dụ dòng 1085 `failed.source_removed == present` với `present` là `file_exists` không phân biệt hoa thường). Hai tệp R9 sở hữu là tệp mới, không sửa được tệp này.
- Kịch bản: ma trận `CaseOnlyRename_Matrix`/`GroupCaseOnlyRename_Matrix` chạy trên khung hiện tại sẽ báo vi phạm giả (hoặc bỏ sót mất mát thật) vì `file_exists("a.jpg")` đúng với cả `A.jpg`.
- Đề xuất: R9 được sửa phần bất biến dùng chung của `file_actions_matrix.rs` (hoặc sao chép có tên riêng vào tệp mới) và ghi ba thay đổi C# trên vào contract. (`TwoRenames_Matrix` thiếu trong danh mục đã được thêm, xem "Đã sửa".)

### M5 (trung bình, xác nhận): `load_editor`/`apply_editor` đã merge lệch ngữ nghĩa C# (E4)
- Đơn vị chủ: W9-E4. Mã: w9-contracts M5.
- `crates/pr-app/src/presenters/settings_field_map.rs:877-889`: `load_editor` sao chép `external_editor_path` thô (C#: `ExternalEditors.Default(s)?.Path ?? s.ExternalEditorPath`, `SettingsFieldMap.cs:195`); `apply_editor` đặt `external_editor_path = f.external_editor_path.trim()` rồi trả `None` (C#: đặt từ đường dẫn của editor mặc định, rồi `ExternalEditors.ApplyLegacyPath(s, f.ExternalEditorPath)`, dòng 202-204). C# không `trim` ở đây vì `ApplyLegacyPath` tự trim.
- Kịch bản: khi trang Files bắt đầu ghi danh sách (E4), đường dẫn gõ vào ô không còn di chuyển/tạo/xoá editor mặc định; Save với danh sách khác và ô đường dẫn cũ sẽ giữ đường dẫn cũ. Test C# `Save_TypedPath_MovesTheDefaultEditor`, `Save_ClearedPath_RemovesTheDefaultEditor`, `Save_TypedPathWithoutList_CreatesAnEditor` sẽ đỏ.
- Contract E4 đã được sửa để nói rõ hàng đã có và phải hoàn thiện (không thêm). Cần E1 xong (`apply_legacy_path`) trước.

### L1 (thấp, xác nhận, đã sửa): hai khoảng dòng vượt cuối tệp trong R2
- `CaptureGroupActionRequest.cs:11-15` (tệp 14 dòng), `FileActionResult.cs:23-27` (tệp 25 dòng). Đã sửa thành `11-14` và `23-25` (cả `W9-R2.md` và `WAVE-9-PLAN-ren.md`).

### L2 (thấp, xác nhận, đã sửa): E7 đặt test vào tệp không tồn tại
- `crates/pr-app/tests/input_shortcut_router.rs` không có; tệp thật là `shortcut_router.rs`. Đã sửa.

### L3 (thấp, xác nhận, đã sửa): R8 trỏ `secondary_window_port.rs:177`
- Danh sách `supports` hiện nằm ở dòng 225-228; dòng 177 là phần khai báo struct `Win32SecondaryWindowPort`. Đã sửa. (Chú thích Rust ở `secondary_window_port.rs` vẫn dẫn số dòng C# của d92d6aae, ví dụ "ShowSettings (lines 90-115)", "Run (lines 272-290)"; ở `7eaee2a2` `Run` nằm ở 331-344. Số dòng trong chú thích là việc của porter W8-18o khi sửa tệp.)

### L4 (thấp, xác nhận, đã sửa): R6 gọi `ImagePresenterRetargetTests` là lớp
- Đó là tên tệp; lớp là `ImagePresenterTests` (partial). Đã sửa.

### L5 (thấp, xác nhận, đã sửa): E5 viết `catalog.find(shown).capture_group().raw_path()`
- `CatalogEntry::capture_group` là trường `Option<CaptureGroup>`; đã sửa thành biểu thức hợp lệ.

### L6 (thấp, xác nhận, chưa sửa): E5 mục 5 "the edit is still committed with the materialisation, as in C#"
- C# `AddExternalEditor` khi danh sách đầy (`Count >= MaxEditors`) chỉ trả về, không gọi `Save` (`MainViewModel.cs:841`); bản Rust dùng `begin_edit`/`commit_edit`, và `commit_edit` ghi đĩa (`state.rs:330`). Vì danh sách đầy thì không rỗng nên `materialize_legacy_editor` là không làm gì, nên chỉ khác một lần ghi tệp vô ích. Đề xuất: bỏ câu "as in C#", và không `commit_edit` trong nhánh đầy (huỷ `begin_edit` hoặc đặt `had_override`).

### L7 (thấp, xác nhận, chưa sửa): vài chữ ký stub khác văn bản contract
- `ExternalEditors::build_arguments` trả `Option<String>` (None khi `file_path` rỗng; C# ném `ArgumentException`), còn E5 mục 3 truyền kết quả thẳng như một chuỗi. E1 không giải thích `Option` trong "Work list". `ListDialogContent::for_choice(title, intro, lines, selected, localizer)` có thêm `&Localizer` mà E3 không nêu (`list_dialog.rs:85`). Các porter sẽ tự thấy từ stub, nhưng văn bản "binding stub" nên khớp.

### L8 (thấp, xác nhận, chưa sửa): trạng thái "staged" của oracle R1 đã lạc hậu
- `docs/WAVE-9-PLAN-ren.md` (mục "Adversarial review amendments") và `contracts/W9-R1.md` ("Oracle input lives in `tests/golden-inputs/staged/` until the baseline moves") nói baseline ghim `d92d6aae` không có `RenameNamePolicy` nên oracle R1 bị bọc `#if W9_RENAME_BASELINE`. Từ #182 baseline là `7eaee2a2`, `tests/golden/w9-r1-rename-policy.json` (44 ca, `csharp_commit` 7eaee2a2) đã có, nhưng input vẫn ở `tests/golden-inputs/staged/` và `W9-R1.cs`/`Program.cs`/`OracleCs.csproj` vẫn dùng cờ. Cần quyết định: gỡ cờ, chuyển input lên `tests/golden-inputs/`, và sửa câu trong kế hoạch (việc của vai trò sinh golden); `tools/oracle.ps1` cần kiểm tra cùng lúc để không chạy hai lần.

### L9 (thấp, xác nhận, chưa sửa): `docs/WAVE-9-PLAN-edt.md` còn các câu của H1
- Xem H1; chưa sửa vì là tài liệu kế hoạch, không phải contract, và `tools/autopilot/lib/codex-lib.py planfor` đọc các dòng `| W9-<lane><n> |` của tệp này.

### L10 (thấp, xác nhận, chưa sửa): vài thay đổi test C# không có đơn vị chủ rõ
- `tests/PhotoReview.Shell.Tests/Menus/ContextMenuModelBuilderTests.cs` (thêm `RenameFile` vào danh sách id kỳ vọng và `IncludeShellOnly: true`), `tests/PhotoReview.Core.Tests/Settings/AppSettingsPocoTests.cs` (`ConfigVersion` 3 -> 4, E1 đã nêu qua tên `AppSettingsDefaults`). Hai tệp đầu có đối ứng Rust đã được #182 sửa (`menu_model.rs`, `settings_app_settings.rs`), nên chỉ cần ghi nhận trong R7 rằng chúng đã xong. Các tệp WPF `SettingsWindow*Tests` và `ToolScriptTests`/`TestFilterDriftTests` (kịch bản phát hành) nằm ngoài phạm vi và cũng không được nêu; hợp lý.

### L11 (thấp, chỉ nghi ngờ): cách đặt tên mới ở E8, E9 phụ thuộc vào giả định "khoá catalog sinh accessor `tr::`"
- Các contract giả định `tr::main_menu_external_editor_add()`, `tr::settings_shortcut_open_in_default_editor()`, ... Accessor được sinh khi build từ catalog (`pr-l10n/build.rs`) nên chỉ đúng nếu quy tắc đặt tên chuyển dấu chấm thành gạch dưới; ví dụ có sẵn `tr::main_menu_rename_file()` trong `builder.rs:195` ủng hộ giả định này. Chưa chạy build nên không xác nhận được cho các khoá còn lại.

## Test yếu / rỗng (suy luận từ mã)
- Không có test Rust nào của Wave 9 tồn tại (mọi tệp test "Owned files" đều là tệp mới, đúng như dự kiến); những gì đang có là test cũ đã được #182 cập nhật số đếm.
- Các test C# không có đối ứng ghi sẵn: E8 nêu rõ không có test C# cho ba thành viên cổng (`PromptAddExternalEditor`, `PromptDefaultExternalEditor`, `ShowExternalEditors`) và yêu cầu viết test Rust từ mã nguồn. Đồng ý; thêm một test kiểm `SharedDialogService` chuyển đủ phương thức (M1) vì thiếu nó thì mọi test E8 vẫn xanh trong khi ứng dụng thật không hoạt động.
- E9 nêu test `Menu_EditorSubmenu_PaintsItsEntries` là N, phụ thuộc E6 và kích thước menu có `RenameFile` bị ẩn (`HiddenContextMenuItems = ["RenameFile"]`, `ExternalEditorsUiTests.cs:36`): bản Rust cần đặt cùng ẩn, contract đã nói.

## Không kiểm tra được
- Không có ngữ nghĩa của thân hàm nào để so (mọi hàm của W9 là `todo!()` trên master); review chỉ so tài liệu với C# và với chữ ký/stub.
- Số học bố cục E2/E3 chỉ so ở mức hằng số đã liệt kê (ở trên), không so từng lời gọi `Place`; chuỗi văn bản của khoá ngôn ngữ chưa so từng chuỗi với C#.
- R2/R3: chỉ kiểm khoảng dòng và đối chiếu mô tả với hunk (`git diff`), chưa đọc từng đoạn `FileActionService.cs` 146-193, 522-566, 611-672, 873-905, `UndoService.cs`, `RecoveryRetryService.cs` để so từng câu trong "Semantics".
- R4, R5, R6: chưa mở `ReviewCatalog.cs` 527-621, `FileActionController.cs` 529-782, `ImagePresenter.cs` 225-294 để so từng câu "Semantics"; chỉ kiểm tên test, số lượng test, khoảng dòng và chữ ký stub.
- Nội dung `tests/golden/*.json` (đúng/sai của từng ca) và việc oracle chạy được không được kiểm.
- Mọi thứ cần Windows/desktop (E2, E3, E9 paint test, R8, R11) chỉ so tài liệu với mã C#.
