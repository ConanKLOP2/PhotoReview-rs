# ST-COM-RECYCLE 2026-10-31: review tĩnh COM Thùng rác

Status: review tĩnh, chưa chạy. Mọi phát hiện là suy luận từ mã; không build, không chạy test, không chạm Thùng rác thật, registry thật hay `%LOCALAPPDATA%\PhotoReview`. Không sửa mã Rust.

Rust: `origin/master` @ `93e44a1d`. C# gốc: `7eaee2a2` (`WindowsRecycleBin.cs`, `Shell/RecycleBinShell.cs`, `Shell/ShellFileRecycler.cs`, `Shell/ShellCom.cs` `ShellStaRunner`).

Tổng: **2 cao / 6 trung bình / 6 thấp**. Cả hai mục cao là "có thể báo vào Thùng rác trong khi thực chất xoá vĩnh viễn"; mục C1 cần xác minh native (xem cách xác minh bên dưới). Không có bằng chứng runtime mất file.

## Phạm vi đã đọc

- `crates/pr-platform-win/src/recycle_bin.rs`, `recycle_bin_shell.rs`, `recycle_restore.rs`, `recycle_bin_settings.rs`, cộng `recycle_candidate.rs`, `recycle_eligibility.rs`, `recycle_bin_capacity.rs` (cần để hiểu luồng).
- Test: `crates/pr-platform-win/tests/recycle_bin_facade.rs`, `recycle_restore_selection.rs`, `recycle_restore_timeout.rs`, `recycle_eligibility.rs`, `recycle_bin_settings_capacity.rs`, `recycle_bin_readonly_delete.rs` (đọc tên test và các chỗ liên quan, không đọc từng dòng).
- Nơi gọi: `pr-core/src/file_actions/service.rs:580-596`, `pr-shell/src/composition/platform.rs:136-142`.

## Kết luận nhanh theo câu hỏi của lane

| Câu hỏi | Kết quả |
|---|---|
| Ổ USB/mạng/UNC có bị xoá vĩnh viễn nhầm khi gọi `send_to_recycle_bin`? | Không qua đường chính: `can_recycle_with` chỉ cho `Fixed` và không phải UNC (`recycle_bin.rs:123-129`, `recycle_eligibility.rs:16-34`); test `send_to_recycle_bin_refuses_non_fixed_or_unresolved_volume_before_size_read` (`recycle_bin_facade.rs:402`). Hở: C1, C2. |
| `$Recycle.Bin` theo SID | Mã không đụng thư mục này; mọi thứ qua Shell (`FOLDERID_RecycleBinFolder`, `IFileOperation`). Không có rủi ro tự suy ra đường dẫn. |
| Cài đặt BitBucket theo ổ | Đọc `HKCU\...\BitBucket\Volume\{GUID}` với GUID từ `GetVolumeNameForVolumeMountPointW(mount point)` (`recycle_bin_settings.rs:181-204`), `NukeOnDelete`/`MaxCapacity`; chính sách `NoRecycleFiles`/`RecycleBinSize` đọc cả HKCU và HKLM. Khớp C#. |
| "Don't move files to the Recycle Bin" bật | `BinDisabled` -> `send_to_recycle_bin` trả lỗi, không có đường lùi sang xoá vĩnh viễn (`delete_permanently` từ chối ổ cố định, `recycle_bin.rs:195-204`). Test `send_to_recycle_bin_refuses_disabled_bin_without_shell_or_delete_fallback` (`recycle_bin_facade.rs:451`). Khớp C#, nhưng người dùng không thể xoá file trên ổ đó: đây là quyết định sản phẩm đã có, không phải lỗi. |
| Release / CoUninitialize cân bằng | Cân bằng: `ComApartment` tạo chỉ sau `CoInitializeEx` thành công (kể cả `S_FALSE`), huỷ sau mọi interface nhờ thứ tự khai báo (`recycle_bin.rs:281-308`; `recycle_bin_shell.rs:57-60` field `folder` trước `_apartment`; trong `recycle_restore.rs:151-185` `session` khai báo trước `entries`/`matches` nên bị huỷ sau). Không có PIDL, BSTR trong mã này. |
| Chuỗi UTF-16 không NUL | `recycle_bin.rs:313-319` cấm NUL và thêm NUL cuối; `recycle_bin_settings.rs:340-345` tương tự. Đầu ra Shell: `take_pwstr` chuyển rồi `CoTaskMemFree` kể cả khi chuyển lỗi (`recycle_bin_shell.rs:178-191`). Không rò. |

## Phát hiện

### C1 (Cao, cần xác minh native) — `GetVolumePathNameW` có thể không theo junction/symlink thư mục -> Shell xoá vĩnh viễn

- File:line: `recycle_bin_settings.rs:221-245` (`query_mount_point`), `recycle_eligibility.rs:78-101` (`normalized_full_path` chỉ chuẩn hoá chuỗi), `recycle_bin.rs:123-129`. Doc comment ở dòng 221 khẳng định "follows junctions and mounted folders".
- Kịch bản: `C:\photos` là junction (hoặc symlink thư mục) trỏ sang ổ USB `E:\data`. File `C:\photos\a.jpg` thực chất nằm trên `E:` (không có Thùng rác). `query_mount_point` chỉ nhận đường dẫn chuỗi đã `std::path::absolute`, không `canonicalize`. Theo trí nhớ của tôi, `GetVolumePathNameW` theo mount point của volume (reparse tag mount-point tới `\\?\Volume{}`), còn junction tới thư mục khác ổ thì trả về gốc ổ của chính đường dẫn chuỗi (`C:\`). Nếu đúng: `can_recycle` = true (C: Fixed), capacity guard dùng volume C:, rồi `IFileOperation` với `FOF_NOCONFIRMATION` xoá file trên `E:` vĩnh viễn, không hỏi; `file_exists` = false -> `Ok(())` -> journal ghi "Recycle" (`service.rs:590-596`). Đây là đúng tình huống lane yêu cầu tìm. Tôi chưa chạy, nên đây là suy luận cần xác minh.
- Test hiện có: `recycle_eligibility.rs:95-140` (`can_recycle_junction_to_non_fixed_volume_uses_resolved_mount`) chỉ giả lập `mount_point_of`, tức kiểm tra chính sách chứ không kiểm tra hành vi thật của `GetVolumePathNameW`. Không có test native nào dùng junction thật.
- Đề xuất: (1) xác minh native bằng test `#[ignore]` tạo junction trong thư mục tạm tới một thư mục trên cùng/khác volume và in kết quả `query_mount_point`; (2) trước khi hỏi volume, phân giải đường dẫn thật bằng `std::fs::canonicalize` (hoặc `GetFinalPathNameByHandleW`, bỏ tiền tố `\\?\`) và dùng đường dẫn đã phân giải cho `can_recycle`, capacity và `is_permanent_delete_allowed`; từ chối khi không phân giải được; (3) sửa doc comment cho đúng sau khi xác minh.

### C2 (Cao) — không có bằng chứng "đã vào Thùng rác", chỉ có "file đã biến mất"

- File:line: `recycle_bin.rs:153-157` (`shell_recycle` rồi `file_exists`), `recycle_bin.rs:322-333` (`PerformOperations` không có progress sink), `recycle_bin.rs:336-349` (`HR_CANCELLED`/`HR_ABORT` -> `Ok`).
- Kịch bản: mọi nguyên nhân khiến Shell xoá vĩnh viễn mà guard không đoán trước (C1; đường dẫn quá `MAX_PATH` mà Shell không thể đưa vào Thùng rác, khi `FOF_NOCONFIRMATION` thì không có hộp xác nhận; ước lượng dung lượng/quota sai lệch so với Shell thật; chính sách khác chưa được đọc) đều cho kết quả `file_exists == false` -> `Ok(())`. UI/journal báo "vào Thùng rác", Ctrl+Z sau đó chỉ cho `try_restore` -> `Ok(false)` (không có mục để khớp). Người dùng mất file mà không có cảnh báo trước.
- Test hiện có: `send_to_recycle_bin_reports_failure_if_shell_leaves_source_in_place` (`recycle_bin_facade.rs:431`) kiểm tra chiều ngược lại (file còn nguyên). Không test nào kiểm tra "Shell xoá nhưng không vào Thùng rác" vì seam `shell_recycle` chỉ trả `io::Result<()>`.
- Đề xuất: truyền một `IFileOperationProgressSink` cho `DeleteItem` (tham số thứ hai hiện là `None`, `recycle_bin.rs:330`) và ghi `PostDeleteItem(hrDelete, psiNewlyCreated)`: `psiNewlyCreated` khác null là mục trong Thùng rác; null với `hrDelete` thành công nghĩa là xoá vĩnh viễn -> trả lỗi riêng (không thể thu hồi, nhưng ít nhất không báo "vào Thùng rác" và dừng lô). Sink cũng cho HRESULT từng mục (xem L6). Đổi seam `shell_recycle` thành trả enum `{Recycled, DeletedPermanently, Cancelled}` để test được bằng fake.

### T1 (Trung bình) — khớp mục khi restore dùng DateModified, không dùng thời điểm xoá hay phần mở rộng

- File:line: `recycle_bin_shell.rs:108-149` (chỉ đọc `DeletedFrom`, tên hiển thị, `System.Size`, `System.DateModified`), `recycle_candidate.rs:57-113`.
- Kịch bản A (nhầm/không phân biệt được): cùng thư mục, cùng tên, cùng size, `DateModified` lệch dưới 1 giây (ví dụ cùng một file được copy giữ mtime rồi xoá hai lần: hai mục trong Thùng rác khác thời điểm xoá, metadata y hệt). `matches.len() == 2` -> `Ok(false)` (`recycle_restore.rs:176-178`). An toàn dữ liệu nhưng Ctrl+Z hỏng; mục xoá lần trước không bao giờ được phân biệt. Nếu chỉ còn mục cũ trong Thùng rác (mục mới đã bị dọn), mục cũ khớp một mình và bị khôi phục thay cho bản vừa xoá, nội dung có thể khác dù cùng size và mtime.
- Kịch bản B (đuôi bị ẩn): `name_matches` (`recycle_candidate.rs:105-113`) chấp nhận tên hiển thị không có đuôi nhưng không kiểm đuôi, nên `photo.jpg` và `photo.png` cùng thư mục, cùng size, cùng mtime sẽ cùng khớp một tên hiển thị `photo`; hai mục -> từ chối (an toàn), nhưng hiếm khi cần.
- Test hiện có: `selector_does_not_accept_ambiguous_candidate_by_metadata` (`recycle_restore_selection.rs:65`), `selector_matches_display_name_without_extension` (84). Không có test chọn theo thời điểm xoá, vì mã không đọc thuộc tính này.
- Đề xuất: đọc `System.Recycle.DateDeleted` (fmtid `9B174B33-40FF-11D2-A27E-00C04FC30871`, pid 3) và `System.FileExtension`; truyền thời điểm journal ghi ngay trước khi xoá để lọc các mục xoá trước đó; khi còn nhiều mục khớp thì chọn mục có `DateDeleted` lớn nhất và không sớm hơn thời điểm journal (kèm log). Cần xác nhận hai property này có mặt trên mục Thùng rác thật (chưa chạy).

### T2 (Trung bình) — TOCTOU và hộp thoại modal của verb `undelete` có thể ghi đè hoặc treo

- File:line: `recycle_bin.rs:224-230` (kiểm tra `exists()` rồi mới gọi backend), `recycle_bin_shell.rs:156-167` (`nShow: 1`, không `CMIC_MASK_FLAG_NO_UI`, `hwnd` null), `recycle_restore.rs:188-197`.
- Kịch bản: (a) giữa kiểm tra `exists()` và verb có file mới xuất hiện tại đường dẫn gốc (ví dụ máy ảnh/đồng bộ ghi lại tên cũ) -> Shell hiện hộp thoại Replace/Skip/Rename không chủ sở hữu, có thể bị che; nếu người dùng bấm Replace thì file đang tồn tại bị ghi đè. (b) thư mục gốc đã bị xoá -> Shell hỏi tạo lại thư mục. Worker bị bỏ ngang sau `SHELL_RESTORE_TIMEOUT` 120 giây (`recycle_restore.rs:66,188-196`), nhưng hộp thoại và luồng STA vẫn sống đến khi người dùng trả lời, và kết quả muộn có thể khôi phục/ghi đè sau khi UI đã báo lỗi; thử lại trong lúc đó có thể gọi verb lần hai.
- So với C#: C# (`TryRestore`, `WindowsRecycleBin.cs:119-128`) cũng chỉ kiểm tra trước; điểm mới của Rust là có timeout 120 giây (cải thiện) nhưng không có cơ chế huỷ.
- Test hiện có: `try_restore_refuses_existing_destination_without_backend_call` (`recycle_bin_facade.rs:633`), `recycle_restore_timeout.rs:33`. Không test nào cho "đích xuất hiện sau khi kiểm" hay "thư mục gốc mất".
- Đề xuất: đặt `fMask = CMIC_MASK_FLAG_NO_UI | CMIC_MASK_NOASYNC` (không hộp thoại, đồng bộ) và coi lỗi trả về là "không khôi phục"; kiểm tra thư mục cha tồn tại trước khi gọi (`Ok(false)` nếu không); ghi log khi timeout kèm số luồng còn treo; chặn gọi khôi phục lần hai cho cùng đường dẫn khi worker cũ chưa xong. Cần thử trên máy thật để xác nhận `NO_UI` làm verb thất bại thay vì hỏi (chưa chạy).

### T3 (Trung bình) — `recycle()` không có timeout và hiện hộp thoại lỗi không chủ sở hữu

- File:line: `recycle_bin.rs:290-297` (`.join()` không giới hạn), `recycle_bin.rs:326` (không `FOF_NOERRORUI`), `recycle_bin.rs:330` (không `SetOwnerWindow`).
- Kịch bản: `IFileOperation` gặp lỗi (file đang bị khoá, ACL) thì hiện hộp thoại lỗi; vì không có cửa sổ chủ, hộp thoại có thể nằm sau cửa sổ chính và luồng gọi (file-action worker) bị chặn mãi, trong lúc journal đang ở trạng thái đã `begin`. C# cố ý để hộp thoại lỗi hiện (comment `ShellFileRecycler.cs:20`) và cũng `Join` không giới hạn, nên đây là parity, nhưng phần restore của Rust đã được thêm timeout còn phần recycle thì chưa.
- Test hiện có: không có (seam `shell_recycle` luôn giả lập).
- Đề xuất: áp dụng cùng cơ chế worker tách rời và `recv_timeout` như `restore_on_worker`, hoặc đặt `SetOwnerWindow` bằng cửa sổ chính; quyết định có giữ hộp thoại lỗi (parity C#) hay thêm `FOF_NOERRORUI` và dùng HRESULT từ sink (xem C2) là việc của chủ dự án.

### T4 (Trung bình) — logic chọn verb đã port không chạy ở production; `undelete` gọi không qua `QueryContextMenu`, chưa có test native

- File:line: `crates/pr-platform-win/src/restore_verb.rs` không nằm trong `lib.rs` (chỉ `tests/recycle_restore_selection.rs:3-4` kéo vào bằng `#[path]`); `recycle_bin_shell.rs:151-168` luôn gọi verb canonical `undelete`; không có `QueryContextMenu` nào trong `src` (grep).
- Kịch bản: C# (`RecycleBinShell.cs:222-262`) tạo HMENU, `QueryContextMenu`, đọc tên verb (theo ngôn ngữ OS), rồi gọi theo index, chỉ lùi về `undelete` khi không nhận ra tên. Rust bỏ cả khâu đó, nên các test của `restore_verb` (khoảng chín test trong `recycle_restore_selection.rs:160-372`) (nhận diện nhãn nhiều ngôn ngữ, "chỉ gọi đúng một verb") kiểm tra mã không chạy trong ứng dụng. Việc `IContextMenu::InvokeCommand("undelete")` hoạt động mà chưa gọi `QueryContextMenu` chưa được xác minh trên máy thật (một số trình xử lý menu cần `QueryContextMenu` để khởi tạo); comment C# nói verb canonical là đường lùi hợp lệ nhưng đó là đường lùi sau khi đã query.
- Test hiện có: không có test native cho `recycle_bin_shell.rs` (không có file `*native*` nào cho Thùng rác trong `tests/`).
- Đề xuất: hoặc bỏ `restore_verb.rs` khỏi `PARITY` đã tính và xoá test chết, hoặc nối lại đường C# (tạo HMENU, query, chọn index) vào `invoke_restore`; thêm một test native `#[ignore]` chỉ-đọc cho `entries()` (không gọi verb) và một test restore dùng file do test tự tạo rồi tự xoá (chạy tay có kiểm soát, không chạy trong CI).

### T5 (Trung bình) — `WindowsRecycleBin::new` dùng `NullLog` ở production

- File:line: `recycle_bin.rs:55-61`, gọi từ `pr-shell/src/composition/platform.rs:139-141` (chỉ backend nhận `log`, facade thì không).
- Kịch bản: mọi cảnh báo "Recycle refused (...)" (`recycle_bin.rs:136,142,149,182`), "restore refused, destination already exists" (225) và lỗi "Recycle Bin settings could not be read" (`recycle_bin_capacity.rs:53`) bị nuốt; khi người dùng báo "không xoá được file" thì không có dấu vết nào để phân biệt `TooLarge`, `BinDisabled`, `Unknown`. C# truyền `ILog`.
- Test hiện có: không (các test inject log giả).
- Đề xuất: thêm constructor nhận `Arc<dyn Log>` (hoặc `with_log`) và dùng nó trong composition.

### T6 (Trung bình, độ chắc thấp) — luồng STA không bơm message; restore async có thể bị cắt khi luồng kết thúc

- File:line: `recycle_restore.rs:96,213-221` (`thread::sleep` 30 x 60 ms), `recycle_restore.rs:148-187`, `recycle_bin.rs:292-297`.
- Kịch bản: luồng STA chỉ ngủ trong lúc chờ bằng chứng khôi phục. Nếu trình xử lý verb phát sinh luồng riêng và gọi ngược về object sống trong apartment này, cuộc gọi bị chặn đến khi ngủ xong; sau tối đa khoảng 1,8 giây luồng thoát, `CoUninitialize` chạy trong khi thao tác Shell có thể chưa xong, làm khôi phục dang dở. Phía C# cũng ngủ trên STA không bơm (`ShellStaRunner`), nên đây là parity; chưa có bằng chứng sự cố thật. `IFileOperation::PerformOperations` tự bơm message nên phần recycle không bị ảnh hưởng.
- Test hiện có: không (fake `pause` là no-op).
- Đề xuất: dùng `CMIC_MASK_NOASYNC` (T2) để loại vấn đề, hoặc bơm message trong lúc chờ (`MsgWaitForMultipleObjects`/`PeekMessage`). Xác minh bằng thử native có kiểm soát.

### L1 (Thấp) — đường dẫn dài / không chuẩn

- `recycle_bin.rs:291` dùng `std::path::absolute` (không thêm tiền tố `\\?\`); `SHCreateItemFromParsingName` với đường dẫn quá 260 ký tự thường thất bại, nhưng nếu thành công thì Shell có thể xoá vĩnh viễn (xem C2). `normalized_full_path` giữ `\\?\` cho dạng không phải ổ đĩa. UNC bị từ chối đúng cho `send_to_recycle_bin` (`recycle_eligibility.rs:24-26`) và cho phép xoá vĩnh viễn (50-52). Test: `recycle_eligibility.rs` có các ca UNC, chưa có ca > MAX_PATH. Đề xuất: từ chối rõ ràng đường dẫn > 259 ký tự trước khi gọi Shell, hoặc dựa vào sink của C2.

### L2 (Thấp) — `entries()` giữ mọi mục của Thùng rác trong bộ nhớ

- `recycle_bin_shell.rs:79-99` thu thập toàn bộ `IShellItem2` vào `Vec`, rồi mới lọc (`recycle_restore.rs:157-175`); C# giải phóng từng mục không khớp ngay (`RecycleBinShell.cs:65-95`). Thùng rác hàng chục nghìn mục tốn bộ nhớ/COM ref và thời gian giữ luồng; không sai logic. Đề xuất: trả iterator hoặc lọc trong vòng `Next`.

### L3 (Thấp) — tên có surrogate lẻ không khôi phục được

- `recycle_bin.rs:94-97` từ chối đường dẫn không phải Unicode (`InvalidInput`) cho cả recycle lẫn restore; `recycle_bin_shell.rs:186-187` bỏ qua mục có chuỗi UTF-16 sai (per-item `Format`); `recycle_candidate.rs:75,79` dùng `to_string_lossy`. File NTFS có surrogate lẻ vẫn xoá được qua đường khác nhưng không thể đưa vào Thùng rác/khôi phục; C# xử lý được (`Marshal.PtrToStringUni`). Test: không. Hiếm; chỉ cần lỗi rõ ràng.

### L4 (Thấp) — capacity guard dùng tra cứu mount riêng, không dùng seam của facade

- `recycle_bin_capacity.rs:69,72` gọi `source.get_volume_drive_type(path)` (dùng `query_mount_point` thật, `recycle_bin_settings.rs:187-190`) trong khi eligibility dùng `mount_point_of` được inject (`recycle_bin.rs:123-127`). Production giống nhau, nhưng test inject mount giả không kiểm tra được sự nhất quán giữa hai nhánh, và nếu C1 đúng thì cả hai cùng sai. Đề xuất: truyền mount đã phân giải một lần xuống guard.

### L5 (Thấp) — một giá trị registry sai kiểu làm từ chối mọi lần xoá

- `recycle_bin_settings.rs:118-127,160-179`: giá trị không phải DWORD (ví dụ `RecycleBinSize` kiểu chuỗi do policy sai) -> `Err` -> `Unknown` -> mọi lần recycle bị từ chối (`recycle_bin_capacity.rs:49-56`). Fail-closed và parity với C# (`RegistryKey.GetValue` ném). Lỗi không nhìn thấy được vì T5. Test: `recycle_bin_settings_capacity.rs` có ca non-DWORD.

### L6 (Thấp) — `GetAnyOperationsAborted`/HRESULT từng mục không được dùng

- `recycle_bin.rs:341-343` coi `ERROR_CANCELLED`/`E_ABORT` là thành công (parity `ShellFileRecycler.Throw`), và `PerformOperations` trả `S_OK` dù một mục lỗi. Hệ quả là thông báo lỗi chung "left the source in place" (`recycle_bin.rs:110-115`) thay cho nguyên nhân thật (huỷ, từ chối truy cập, đường dẫn dài). Không mất dữ liệu. Đề xuất: sink của C2 cũng trả HRESULT từng mục để báo đúng nguyên nhân.

## Những điểm đã kiểm và không thấy lỗi

- Xung đột tên khi khôi phục: bị chặn bằng `path.exists()` trước (`recycle_bin.rs:224-230`), mạnh hơn C# vì thư mục tại đường dẫn gốc cũng bị từ chối. Nhưng chỉ chặn ở mức kiểm trước (xem T2).
- Bằng chứng khôi phục dùng cả size và mtime chính xác (`recycle_restore.rs:233-240`, test dòng 427), thời gian chờ 30 x 60 ms đúng C#.
- Chỉ đúng một mục khớp mới được gọi verb (`recycle_restore.rs:176-179`), test ambiguous (`recycle_restore_selection.rs:65`, `recycle_restore.rs:486`).
- Lỗi theo mục (`Com`/`InvalidCast`/`Format`) được bỏ qua, lỗi khác dừng toàn bộ (`recycle_restore.rs:170-173`), khớp `RecycleItemFailure.IsPerItem` của C#; khác C# ở chỗ Rust trả `Err` thay vì `false` khi dừng (người gọi phải xử lý cả hai, không phát hiện chỗ gọi nào coi `Err` là thành công).
- Registry chỉ mở `KEY_QUERY_VALUE`, đóng handle bằng `Drop`, không ghi (`recycle_bin_settings.rs:347-419`).
- Đường dẫn gọi từ `service.rs`: `delete_permanently` chỉ chạy khi `run.permanent`, và façade chỉ cho phép trên ổ xác định là không có thùng rác (`recycle_bin.rs:195-204`); `native_file_delete` từ chối file chỉ-đọc như C# (`recycle_bin.rs:247-249`).

## Đề xuất cho orchestrator

1. Mở thêm lane native có kiểm soát để xác minh C1 (junction thật trong thư mục tạm; có thể cần ổ phụ để chắc chắn) trước khi đóng bất kỳ hàng PARITY liên quan đến Thùng rác.
2. Hàng PARITY cần xem lại (không sửa ở lane này): hàng của `recycle_bin_shell`/`restore_verb`/`WindowsRecycleBin` không nên là Done nếu chưa có test native cho `entries()`/restore và chưa quyết T4; ghi chú rằng `restore_verb.rs` không nằm trong cây module (bằng chứng `lib.rs` không có `mod restore_verb`, `tests/recycle_restore_selection.rs:3-4`).
3. Thứ tự ưu tiên sửa: C2 (sink) và C1 trước, rồi T5 (log) vì rẻ nhất, rồi T2/T3/T1.
