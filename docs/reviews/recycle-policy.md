# Review Recycle Bin: policy, dung lượng và chọn đường restore

Machine C (cloud), 2026-10-23. Rust master `8042d67`, C# baseline `7eaee2a2`. **Mọi phát hiện: suy luận từ mã, chưa chạy thử.** Không chạy Cargo, test native, enumerate hoặc bất kỳ thao tác nào trên thùng rác thật. Chỉ đọc mã và test fake; không sửa Rust.

Tổng: **0 cao / 1 trung bình / 1 thấp**. M1 là khoảng hở kiểm tra dữ liệu adapter; L1 là phần tích hợp parity còn thiếu, PARITY đã ghi Partial. Không có bằng chứng runtime mất file.

## Phạm vi

Đọc `crates/pr-platform-win/src/{recycle_bin_capacity,recycle_bin_settings,recycle_candidate,recycle_eligibility,recycle_bin_shell,restore_verb}.rs`, orchestration `recycle_restore.rs:115-168`, proof polling và các đoạn test fake. Đối chiếu C# `src/PhotoReview.Platform.Windows/{RecycleBinCapacity,WindowsRecycleBin}.cs`, `Shell/RecycleBinShell.cs`, contracts W6-02d/e. Facade `recycle_bin.rs` chỉ đọc đường dựng adapter và các cổng destructive/restore; không kiểm toán lại toàn bộ journal/Undo (báo cáo invariants).

## M1 — trung bình: không kiểm lại kiểu/độ dài registry sau lần đọc dữ liệu

**Suy luận từ mã, chưa chạy thử.** Chủ W6-02b; nguy cơ fail-closed bị suy yếu trong khoảng registry đổi giữa hai lần query, chưa chứng minh dẫn tới Shell xóa vĩnh viễn.

- `recycle_bin_settings.rs:380-395` query kiểu và độ dài trước, đòi REG_DWORD/4. `:401-410` query dữ liệu lần hai và truyền lại `&mut kind`, `&mut byte_count`; `:411-413` chỉ kiểm status rồi trả Dword(value), không kiểm lại hai output.
- API [RegQueryValueExW của Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regqueryvalueexw) trả kiểu trong lpType và số byte thực chép trong lpcbData. Đây là output của từng lần gọi, không phải bộ lọc buộc kiểu của lần đầu.
- Kịch bản cụ thể: query đầu của NoRecycleFiles thấy DWORD, 4 byte; trước query hai, writer khác thay bằng REG_BINARY 4 byte toàn 0. Query hai thành công, cập nhật kind=REG_BINARY; adapter vẫn trả Dword(0). `read_dword:118-127` chấp nhận; `read_policy:160-178` coi switch false. Với volume và các setting khác hợp lệ, evaluate có thể trả Fits thay vì Unknown cho dữ liệu sai kiểu. Tương tự query hai trả DWORD nhưng length nhỏ hơn 4 sẽ được zero-fill từ value khởi tạo rồi chấp nhận. Chưa chạy registry race thật; đây là kịch bản dựa trên API và control flow.
- Không tuyên bố NoRecycleFiles REG_BINARY buộc Windows permanently-delete: Shell có thể bỏ qua setting sai kiểu, và chưa xác minh hành vi đó. Lỗi đã đủ cụ thể ở việc dữ liệu malformed bị phân loại Dword, trái invariant fail-closed của adapter.
- C# `RecycleBinCapacity.cs:219-233` lấy object từ RegistryKey.GetValue rồi chỉ nhận int. Chưa kiểm mã nội bộ .NET của GetValue, nên không khẳng định mọi race C# đã được loại bỏ.
- Test `recycle_bin_settings_capacity.rs:199`, `read_policy_non_dword_value_errors_and_capacity_guard_fails_closed`, tiêm RegistryValue::Other ở seam cao hơn adapter; không thể phát hiện kind đổi trong native query thứ hai.
- Test nên thêm: seam query mức thấp hoặc helper validate kết quả read thật. Script metadata DWORD/4, read thành công với BINARY/4, DWORD/1 và DWORD/0 => Other hoặc InvalidData, capacity Unknown và fake shell không được gọi. Đối chứng DWORD/4 vẫn đúng u32::MAX; query ERROR_MORE_DATA/ACCESS_DENIED vẫn fail closed. Không tạo/sửa policy registry thật.

## L1 — thấp: helper restore verb có test nhưng chưa dùng trong production

**Suy luận từ mã, chưa chạy thử.** Chủ W6-02d/e; phần tích hợp còn thiếu, không coi đây là lỗi restore đã tái hiện.

- `restore_verb.rs:55-65` có invoke: ưu tiên index của tên restore đã nhận dạng, không có mới canonical. Test `recycle_restore_selection.rs:286`, `recognised_name_invokes_only_that_verb`; `:309` `unrecognised_language_uses_canonical_verb`; `:353` `failing_verb_propagates_without_second_attempt` kiểm helper qua module path riêng ở test `:3-4`.
- Production `recycle_bin_shell.rs:151-166` bind IContextMenu rồi luôn gán `lpVerb=RESTORE_VERB` (`undelete`, constant :34). Không gọi helper. `src/lib.rs` chưa khai báo module restore_verb; grep production không thấy caller.
- C# `Shell/RecycleBinShell.cs:87-88` gọi RestoreVerb.Invoke với danh sách verbs; `WindowsRecycleBin.cs:315-324` ưu tiên index rồi fallback. Contract W6-02e §Port scope cũng yêu cầu đúng một localized/canonical route.
- Kịch bản phân biệt cần test fake: menu có tên restore nhận dạng, route index thành công còn canonical trả lỗi. C# chọn index; adapter Rust hiện chọn canonical và trả lỗi. Đây là mô hình test để chứng minh khác biệt lựa chọn, không khẳng định Shell Windows thật thường hỏng canonical. Không có nguy cơ adapter gọi verb Delete từ đoạn mã đã đọc.
- `docs/PARITY.md:385` đã ghi P-03 Partial và mô tả canonical-only trung thực. Vì vậy giữ mức thấp; không cáo buộc tài liệu hứa Full hoặc các pure test sai. Cần nối helper/adapter và có test mức backend, hoặc ADR chấp nhận canonical-only; không chỉ bật module rồi coi xong.
- C# test nguồn: `WindowsRecycleBinRestoreVerbTests.RecognisedNameInvokesOnlyThatVerb`, `UnrecognisedLanguageUsesCanonicalVerb`, `FailingVerbPropagatesWithoutSecondAttempt`. Test mới đi qua adapter seam phải chứng minh route đúng và không thử route thứ hai khi route đầu lỗi, đồng thời COM/menu fake được drop.

## Những bảo vệ đã thấy, không lặp thành phát hiện

| Đường | Bằng chứng / test đọc | Giới hạn |
|---|---|---|
| Capacity | `recycle_bin_capacity.rs:65-89` policy/loại ổ/GUID/nuke trước size; thiếu size chỉ bỏ kiểm size sau các switch; `evaluate_safety_margin_boundary_matches_csharp` | Không coi Fits(None) là lỗi thiếu size: đó là cổng size-independent theo C# |
| Giá trị DWORD và tràn | `read_dword:121` u32->i64; cap MB thật tối đa u32 nên nhân MiB nằm trong i64; quota>i64::MAX bị từ chối | Không báo panic do fake tùy ý trả i64::MAX như lỗi production registry |
| Eligibility | `recycle_eligibility.rs:47-79` chỉ cho permanent khi UNC hoặc loại ổ no-bin xác định; Unknown không được permanent | Không tự chứng minh TOCTOU giữa precheck và delete đã đóng; G25 còn nguyên |
| Chọn candidate | `recycle_candidate.rs:89-103` folder/name/size/time, tolerance dưới 1 giây; backend `recycle_restore.rs:157-160` đòi đúng 1 match | Metadata không phải hash; không khẳng định chứng minh byte identity |
| Existing target / proof | Test facade `try_restore_refuses_existing_destination_without_backend_call:633`; backend chứng minh size và timestamp sau invoke | Cổng facade trước invoke không khóa chương trình ngoài; chưa thử native race |
| Vòng đời COM | Session giữ folder trước apartment trong thứ tự field drop; worker khai báo session trước entries/matches; PWSTR giải phóng cả khi UTF-16 lỗi | Không có native/COM verdict runtime |

## PROMPT Codex

**G30 — sửa adapter registry (M1, trung bình, W6-02b).** Được sửa `crates/pr-platform-win/src/recycle_bin_settings.rs`, `crates/pr-platform-win/tests/recycle_bin_settings_capacity.rs`, `task_on_progress.md`. Kiểm kiểu/độ dài của chính query thành công trả bytes; seam thấp phải cho fake đổi output giữa probe/read. Không sửa giá trị registry thật. Thêm các input/expectation M1; nối fake capacity/shell để chứng minh Unknown chặn action. Không giảm kiểm tra lần đầu, không nuốt lỗi. Nếu chọn API đọc một lần có type filter, kiểm contract của API bằng nguồn chính thức.

**G31 — tích hợp hoặc quyết định parity (L1, thấp, W6-02d/e).** Phạm vi sau quyết định: `crates/pr-platform-win/src/{lib,restore_verb,recycle_bin_shell,recycle_restore}.rs`, `crates/pr-platform-win/tests/recycle_restore_selection.rs`, test fake cạnh backend, `docs/PARITY.md` (P-03), `docs/adr/` nếu có khác biệt chủ ý, `task_on_progress.md`. Giữ một route duy nhất, RAII đúng một lần, không fallback sau lỗi. Không chỉ kiểm helper độc lập. Chưa quyết định thì giữ Partial và mở đề xuất docs, không bịa kết quả native.

Cả hai: cấm xóa/ignore/làm yếu test, đổi golden theo mã, chạy native/bin thật hoặc mở rộng journal/undo. Kiểm tra có hang guard, target dir riêng: `cargo fmt -p pr-platform-win --check`; `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings`; `cargo test -p pr-platform-win --locked --test recycle_bin_settings_capacity`; `cargo test -p pr-platform-win --locked --test recycle_restore_selection`; `cargo test -p pr-platform-win --locked`. Không --include-ignored; ghi đúng lệnh chưa chạy. Không mutation COM/native glue.
