# Lỗi của app C# mà bản port Rust phát hiện (chưa sửa bên C#)

Machine A, 2026-10-26. C# baseline `7eaee2a2` (repo C# `PhotoReview`, chỉ đọc qua `git -C ../PhotoReview show 7eaee2a2:<path>`).

**Mọi mục dưới đây là suy luận từ mã, chưa chạy thử.** Hai agent chỉ đọc đối chiếu mã C# với các báo cáo Rust trong `docs/reviews/`; hai mục quan trọng nhất (D1, D2) được đọc lại trực tiếp. Số dòng là ở `7eaee2a2` và có thể đã đổi: bên C# phải kiểm lại trên mã hiện tại. File này nằm ở repo Rust làm bằng chứng; sửa ở C# là việc của session/repo C# (repo này không được sửa C#, xem AGENTS.md).

## Lỗi có thật ở C# (rủi ro giảm dần)

| ID | Rủi ro | Vị trí C# | Mô tả | Rust tương ứng |
|---|---|---|---|---|
| D1 | cao, mất dữ liệu | `Core/RecoveryRetryService.cs:222-227` | Retry nhóm Recycle: member `Permanent` đi thẳng `CanRecycle` -> `DeletePermanently(member.Source)`, member thường đi `SendToRecycleBin`, KHÔNG stat lại size/mtime sau khi ghi Prepared. Nhánh Move/Copy ngay dưới (`:236-238`) có kiểm `sourceStat.Length/LastWriteUtc`. File bị thay giữa precheck và lần xoá thì ảnh thay thế bị xoá vĩnh viễn (hoặc recycle nhầm) trong khi journal giữ fingerprint ảnh cũ. | `invariants.md` H1 (G12/W3-06) |
| D2 | trung bình, mất dữ liệu | `App/Coordinators/DuplicateCleanupController.cs:195-196`, `:213-220`, `:244-246` | Sau hộp thoại duyệt batch (modal, có thể hàng phút) chỉ kiểm folder không đổi, executor không bận và `_catalog.IndexOf(path) >= 0`. Không kiểm bản được GIỮ còn tồn tại, không kiểm bản bị xoá còn đúng size/mtime lúc băm SHA-256, và batch không đăng ký Undo (comment ghi rõ). Bản giữ bị xoá/đổi tên ngoài app (đồng bộ đám mây) trong lúc hộp thoại mở -> mọi bản trùng còn lại vào Thùng rác. | `invariants.md` M1 (G6) |
| D3 | thấp, nhật ký | `Core/OperationJournal.cs:~500-505` | `ReconcileOne` coi Recycle đơn là xong chỉ vì `!FileExists(source)`: người dùng tự xoá file trước khi mở lại app -> journal ghi Committed dù file không ở Thùng rác. | `invariants.md` L8 |
| D4 | thấp | `Core/UndoService.cs:314-352` | Undo của Recycle đơn không có `JournalTransaction` (Move đơn `:396` và nhóm `:446`, `:645` có). Crash giữa lúc khôi phục không để lại dấu vết. | `invariants.md` L5 |
| D5 | thấp | `App/Presenters/RecoveryWindowPresenter.cs:325` | `FileActionResult.JournalPersisted/JournalError` chỉ được dùng ở Recovery; `FileActionController` không hiển thị hay log. Ghi dòng Committed lỗi (đĩa đầy) -> UI báo thành công; tự lành ở lần khởi động sau. | `invariants.md` L6 |
| D6 | thấp | `Core/FileActionService.cs:693`, `:806` | Dọn bản Copy dở cần `copiedStat`, chỉ gán sau khi `TryCopyNew` thành công; copy lỗi giữa chừng hoặc process bị giết để lại file đích dở. Có test partial copy nhưng không phủ trường hợp này. | `invariants.md` L7 |
| D7 | thấp | `Hosting/MessageLoop.cs:242-250`, `Hosting/WndProcThunk.cs:17,26-38,77-82`, `Dialogs/Native/NativeDialog.cs:350-367` | Khi `Filtered(message)` trả true thì không gọi `WndProcThunk.RethrowPending()`. `ModelessFilter` gọi `IsDialogMessage` có thể đưa Enter/Esc vào WndProc của dialog; handler ném thì ngoại lệ nằm trong slot `[ThreadStatic]` tới lần dispatch sau, và còn kẹt nếu loop thoát bằng WM_QUIT (thoát sớm ở ~`:223-227`). Test C# chỉ dùng filter giả (`MessageLoopFilterTests.cs`). | `host-dispatcher.md` M1 |
| D8 | thấp, chỉ khi gọi sai | `Core/FileActionService.cs:50`, `Core/UndoService.cs:89-97` | `End()` nhả cổng vô điều kiện (`Volatile.Write(ref _inProgress, 0)`), không phân biệt người giữ. Mọi đường sản xuất hiện đều đúng (`TryBegin` rồi `End` trong `finally`); test gọi `End()` trực tiếp. | `invariants.md` L1 |

## Có một phần

- L2 (`invariants.md`): `UndoService` có cổng riêng nếu không truyền `FileActionService` (`UndoService.cs:72-83`), `CreateRecoveryRetryService(..., fileActionGate = null)` tùy chọn; `SharedServiceRegistration.cs:83-102` truyền đúng. Chưa xác minh `CompositionRootTests.cs:46` kiểm `Undo.IsBusy` khi service bận.
- G5/M3: C# không có lỗi tương ứng (`FileActionService.cs:798-841` bắt mọi exception, ghi Failed và tính `SourceRemoved` từ đĩa). Còn một đoạn nhỏ: `FileActionController.cs:475-505` nhánh `catch` gọi `_catalog.Restore` không stat đĩa (chỉ chạy khi exception thoát khỏi `ExecuteAsync`); comment `:481-483` dựa vào `RemoveOrDegrade` của presenter, chưa xác minh.

## Không phải lỗi C# (lỗi riêng của Rust)

G8 `OpenInDefaultEditor` (C# đã cài: `ShellMainWindow.Input.cs:215-217`), G2 zoom (C# nối ở `MainViewModel.cs:136-144`), settings-ui H1 (C# lưu View/Instance mode: `GeneralSettingsPage.cs:124-132`), startup H1 (C# dùng `DateTime.ToLocalTime()`, `RecoveryPresentation.cs:126`), G1 `RefCell` (C# không có; rủi ro logic khi state đổi trong lúc modal chạy chưa xác minh).

## Chưa đọc

`DuplicateFinder.cs` (cách chọn bản giữ lại), `FileActionController.cs:400-470`, test C# chỉ tìm bằng grep tên. Test nên thêm bên C#: D1 (thay file sau Prepared, kỳ vọng 0 lần `DeletePermanently`), D2 (bản giữ biến mất trong `ShowBatchReview`, kỳ vọng 0 lần `ExecuteAsync`), D7 (callback ném khi filter handled).
