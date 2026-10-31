# ST-CSPARITY: so khớp hành vi tĩnh C# với Rust cho 4 vùng có mutant sót (2026-10-31)

Lane: `feat/st-csparity`. Status: Complete (static review, inferred from reading code, NOT run). Không đổi production, không đổi test, không build.

Cách làm: đọc bản gốc C# bằng `git -C C:\MyProjects\PhotoReview show 7eaee2a2:<path>` và đọc bản Rust ở `origin/master` 5f26d474. Mọi kết luận "khác/giống" dưới đây là do đọc code; không chạy oracle, không chạy test. Phần "test C# đã port" dùng so khớp theo tên (chuẩn hoá chữ thường, bỏ dấu gạch dưới) và grep, nên là heuristic: một test có thể được port dưới tên khác (ghi rõ chỗ nào chưa xác nhận).

Tổng kết: 0 khác biệt mức High, 1 mức Medium (đã được tài liệu hoá là Different nhưng chưa có bên phát), 6 mức Low, còn lại là khoảng trống test (không phải khác biệt hành vi đã thấy). Bốn vùng đều là bản dịch gần từng dòng của C#; không tìm thấy lỗi logic trong phần đọc.

## (a) Journal: ghi, đọc, reconcile

C#: `src/PhotoReview.Core/FileActions/OperationJournal.cs` (+ `FileFingerprint.cs`). Rust: `crates/pr-core/src/file_actions/journal.rs`, `reconcile.rs`, `journal_rules.rs`, `journal_line.rs`.

### Đã đối chiếu và GIỐNG (đọc từng nhánh)

| Hành vi | C# | Rust |
|---|---|---|
| Chọn bản ghi hiệu lực: bản cuối của mỗi Id, trừ Failed mang mã reconcile (`PendingUnconfirmed`, `SourceStillExistsAfterRecovery`) đứng ngay sau Committed thì bị bỏ qua | `ComputeLatestEntries` + `IsStaleReconcileAfterCommitted` (OperationJournal.cs:419-438) | `compute_latest_entries` + `is_stale_reconcile_after_committed` (journal.rs:642, journal_rules.rs:51) |
| Quy tắc "previous" là bản hiệu lực trước đó (đã loại bản stale), không phải dòng liền trước | `latest.GetValueOrDefault(entry.Id)` | `latest.get(&entry.id)` |
| Thứ tự `ReadPendingAndFailedOperations`: Prepared trước rồi Failed, mỗi nhóm theo thứ tự xuất hiện đầu tiên của Id | `Dictionary` không xoá, giữ vị trí khi gán lại | `LatestEntries` (Vec + index), gán lại giữ vị trí |
| `Dismiss`: đọc đồng hồ một lần TRƯỚC lock; chỉ dismiss khi bản hiệu lực == snapshot (so giá trị, `None` khác `Some([])` ở GroupMembers, DateTime so theo ticks bỏ qua Kind); xoá Error/ErrorCode; một lần append; rỗng thì không mở file | OperationJournal.cs:186-205, 28-86 | journal.rs:462-485; `JournalDateTime::eq` chỉ so `value` (journal_line.rs:114) |
| `AppendIfUnchangedSince`: lịch sử THÔ của Id (không bỏ qua FA-01), `at` = vị trí cuối bằng anchor, không thấy anchor thì cả lịch sử là "since"; so từng phần tử với `ownSince`; sai thì `false`, không tạo thư mục, không mở append | OperationJournal.cs:682-700 | journal.rs:537-553 (`rposition` + `map_or(0, at+1)` + so slice) |
| `AppendIfStillPending`: bản THÔ cuối của Id phải là Prepared, rồi kiểm lại live marker sau khi đọc lại | :654-672 | reconcile.rs `append_if_still_pending` (`lines.last()`, `is_live`) |
| `AppendIfLatestIs`: dùng lại luật FA-01 trên lịch sử thô | :597-614 | reconcile.rs `effective_latest` + `append_if_latest_is` |
| Failed mã reconcile sau Committed: lỗi/Failed đó bị bỏ ở mọi reader (`ComputeLatestEntries`, `ReadLatestEntry`, `AppendIfLatestIs`) nhưng KHÔNG bị bỏ ở `AppendIfUnchangedSince` và `AppendIfStillPending` | đúng như trên | đúng như trên (có chú thích trong doc) |
| Retry khi sharing/lock violation: 5 lần, nghỉ `10 ms x lần`, chỉ lỗi 0x80070020/21 (OS error 32/33); lần 5 hoặc lỗi khác ném ra ngay | `OpenAppendWithRetry`, `AppendAttempts = 5` (:282-300) | `open_append_with_retry`, `APPEND_ATTEMPTS = 5`, `is_sharing_or_lock_violation` (journal.rs:688, 1012) |
| Mọi lần append: tạo thư mục, một `write` duy nhất, kiểm đuôi dưới handle append (byte cuối khác `\n` thì thêm `\r\n` đầu), durable thì flush-to-disk còn lại `Flush()` | :249-280, 215-222 | journal.rs:653-686, 703-717 |
| Reader đuôi: ngưỡng `>= 1 MiB`, cửa sổ 256 KiB nhân đôi, `boundary` luôn ở đầu dòng, dòng dài hơn cửa sổ thì nới và đọc lại, trả 200 dòng cuối, bỏ một `\r` cuối dòng | :324-398 | journal.rs:893-952 |
| Reconcile: Move cần nguồn mất + đích khớp size VÀ write time (dung sai 2 s); Copy chỉ cần đích khớp; Recycle nguồn còn thì Failed; nhóm: mọi member; Recycle-undo nhóm cần đúng size + write time; repair nhóm do bản cũ chốt | :467-652 | reconcile.rs |
| `matches_moved_destination`: `Length == size && |Δticks| <= 2 s` | FileFingerprint.cs | types.rs:36 |

### Khác biệt quan sát được

| # | Đầu vào cụ thể | C# | Rust | Mức |
|---|---|---|---|---|
| a1 | Hành động trên file có tên chứa surrogate đơn (hợp lệ trên NTFS), ví dụ `C:\p\a\uD800b.jpg` | `AppendLines` gọi `ThrowIfUnrepresentable`, ném `JournalCodedException(UnrepresentablePath)` TRƯỚC khi ghi dòng nào, nên hành động bị từ chối khi chưa đụng file | `journal.rs:654` chú thích "không bao giờ xảy ra" vì `String` Rust không chứa surrogate đơn; grep `UNREPRESENTABLE_PATH` trong `crates/*/src` chỉ thấy hằng số và bảng thông báo (errors.rs:45-69, journal.rs:204), KHÔNG có nơi nào phát mã này. PARITY F-13 nói việc từ chối "thuộc về nơi PathBuf thành String (W3-04, OQ2)" nhưng không có nơi nào làm | Medium (suy luận, chưa chạy: nếu chuyển `PathBuf` sang `String` bằng `to_string_lossy` thì dòng journal ghi tên có U+FFFD, không trỏ tới file thật; Prepared của Move sẽ bị reconcile thành Failed vì không tìm thấy file; Recycle undo không khôi phục được. Cần kiểm xem điểm chuyển đổi thật của W3-04/W9) |
| a2 | Ghi một dòng journal lớn hơn 4096 byte (nhóm nhiều member) | `FileStream` đệm 4096 byte; dữ liệu và một số lỗi chỉ lộ ra ở `Flush`/`Dispose`; nếu process chết giữa chừng thì phần đệm chưa ghi mất | `File` ghi thẳng, mỗi `write_all` đi xuống OS (physical.rs:1342 đã ghi DIVERGENCE) | Low (không đổi nội dung file; chỉ khác cửa sổ tear khi crash, Rust tear ít hơn) |
| a3 | `TryReconcileAppend` | chỉ bắt `IOException`/`UnauthorizedAccessException`; ngoại lệ khác nổi lên bắt ở lớp ngoài (cũng chỉ ghi log và tiếp tục) | mọi `io::Error` thành `false` + log; panic được `catch_unwind` | Low (kết quả quan sát như nhau: entry giữ Prepared, danh sách reconcile giữ các entry khác) |
| a4 | `member.Destination` toàn khoảng trắng | `string.IsNullOrWhiteSpace` (`char.IsWhiteSpace`) | `chars().all(char::is_whitespace)` (tập White_Space của Rust) | Low (tập ký tự trắng khác nhau ở vài mã hiếm; suy luận, chưa chạy) |
| a5 | Đường dẫn trên journal | dùng thẳng chuỗi | `path_text()` dùng `to_string_lossy` (journal.rs:626); cùng gốc với a1 | gộp vào a1 |

Chuyển hành vi có chủ đích đã ghi trong PARITY (không tính là phát hiện mới): `JournalCommitClockFailureTests` không port được (đồng hồ Rust không ném), "PowerLossSafe ghi Prepared trên pool thread" (R0003, blocking).

### Test C# (PhotoReview.Tests) đã/chưa port

Đã port theo tên (không thấy thiếu): `JournalAppendRobustnessTests` 8/8, `JournalReconcileRaceTests` 1/1, `JournalConcurrencyTests` 3/3, `JournalTailCommittedMoveTests`, `OperationJournalGapTests` 4/4, `OperationJournalMutationTests` 25/25, `OperationJournalTests` 28/28, `JournalGroupReconcileTests` 10/10, `JournalGroupDowngradeTests` 8/8, `JournalStartupRecoveryTests` 7/7, `JournalDurableFlush` 3/3. `JournalTransactionTests` và phần lớn `JournalSmallGapsTests` / `JournalGroupEvidenceAndNulMemberTests` được port trong `file_actions_transaction.rs` và `file_actions_compaction.rs` (so tên lần đầu báo thiếu vì tôi chưa quét hai file này; quét lại thấy `build_blank_line_followed_by_entries_...`, `fail_retry_superseded_...`).

Chưa xác nhận / chưa port:

- `JournalCommitClockFailureTests` (2 test): cố ý không port (PARITY F-12).
- `JournalDurabilityTests`: `OldConfigWithoutJournalDurability_LoadsAsFast`, `JournalDurability_ParsesLeniently`, `Settings_RoundTrip_PreservesPowerLossSafe_AndProviderReadsLive` không thấy bản port theo tên (chỉ thấy `normalize_undefined_journal_durability_is_reset_to_fast` ở settings_normalizer.rs:286); cần kiểm tay xem settings_store/model_enums đã phủ chưa.
- `JournalSmallGapsTests.JournalCodedException_*` (2 test về `JournalCodedException` không tham số): không thấy; Rust dùng kiểu lỗi khác nên có thể không cần.
- Mutation: CX-JRNL2 ghi 48/179 mutant của `journal.rs` đã bắt, 131 chưa chứng minh. Từ đọc code, các nhánh dễ sót nhất là: nhánh `at = -1` của `append_if_unchanged_since`, thứ tự `Prepared` rồi `Failed` khi cùng Id lặp, cờ `unspecified` của `JournalDateTime` trong so sánh Dismiss, và nhánh cửa sổ nhân đôi khi một dòng dài hơn 256 KiB. Đề xuất test C# gốc để port thêm nếu chưa có: `OperationJournalMutationTests` (các case dismiss với GroupMembers khác nhau), `JournalReconcileRaceTests`.

## (b) perf_csv.rs `format_g17`, session writer `requeue_skipped`

C#: `src/PhotoReview.Core/Diagnostics/PerfCsvListener.cs`, `src/PhotoReview.Core/Session/SessionWriter.cs`. Rust: `crates/pr-core/src/diagnostics/perf_csv.rs`, `crates/pr-core/src/session/writer.rs`.

### perf_csv.rs

Giống (đã đọc): định dạng `G17` (cùng quy tắc .NET Core 3.0+: 17 chữ số có nghĩa chính xác, bỏ số 0 đuôi, dạng khoa học khi số mũ `< -4` hoặc `>= 17`, số mũ ít nhất hai chữ số `E+17`, `E-05`), `-0` cho âm không, `NaN`, `Infinity`, `-Infinity`; thang chọn ô số: chỉ `long/int/double/bool` là số, tối đa 4 ô a-d, từ thứ năm trở đi vẫn tăng đếm nhưng bị bỏ; `nav`/`gen` điền `nav`, `pathId` điền `pathId` (lần đầu và khi còn rỗng), phần còn lại nối `;`; `bool` thành `1`/`0`; `CsvEscape` (dẹt `\r\n`, `\r`, `\n` thành một khoảng trắng rồi mới bọc dấu nháy kép nếu có `,` hoặc `"`, nhân đôi `"`); thứ tự cột `utcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text`; dòng dữ liệu kết thúc `\n`, header và trailer `# dropped=N` kết thúc `\r\n` (C# `WriteLine` = `Environment.NewLine`, Rust viết cứng `\r\n`); đếm drop (kênh đầy, lỗi dựng hàng, sau Dispose, sau lỗi writer); đếm `unflushed` khi writer lỗi.

Ví dụ suy luận (chưa chạy): `1e-5` -> `1.0000000000000001E-05` (C# và Rust), `1e16` -> `10000000000000000`, `1e17` -> `1E+17`, `0.1` -> `0.10000000000000001`, `5e-324` -> `4.9406564584124654E-324`. `format!("{:.16e}")` của Rust làm tròn đúng chính xác như .NET Core 3.0+.

Khác biệt quan sát được:

| # | Đầu vào | C# | Rust | Mức |
|---|---|---|---|---|
| b1 | Biến môi trường | `PHOTOREVIEW_PERF_TRACE`, flags `PHOTOREVIEW_DIAG_*` | `PHOTOREVIEW_RS_PERF_TRACE`, flags `PHOTOREVIEW_RS_DIAG_*` (ADR R0002, cố ý). Header vẫn IN tên kiểu C# (`PHOTOREVIEW_DIAG_PREREAD=1`) dù tra cứu bằng tên RS: `diag_field` in `DIAG_FLAG_HEADER_NAMES` | Low (cố ý, chỉ ghi để analyzer biết) |
| b2 | `thread` | `Environment.CurrentManagedThreadId` | số thứ tự tăng dần theo luồng đầu tiên gặp (perf_csv.rs `current_thread_number`) | Low (giá trị khác nhau nhưng chỉ dùng để nhóm) |
| b3 | `Channel.CreateBounded(0)` | ném `ArgumentOutOfRange` | `capacity.max(1)` | Low (chỉ test seam) |
| b4 | `# commit=` | `AssemblyInformationalVersion` | tham số `commit` truyền vào, mặc định `unknown` | Low (khác nguồn, cùng định dạng) |

### session/writer.rs `requeue_skipped`

Giống (đã đọc từng dòng): chỉ hoàn lại khi chưa `disposed`; chỉ khi `versions[key] == version` của batch (nếu có snapshot mới hơn thì bỏ, tránh ghi đè bản mới); chỉ khi chưa có `pending[key]` (`TryAdd`); thứ tự đẩy lại theo thứ tự batch; hai điểm gọi giống (khoá ghi không lấy được trong `bounded_wait`, và không khởi động được save worker, kèm nhả khoá). Không hoàn lại khi save thread vượt ngân sách (C# cũng không: worker vẫn ghi). `Dispose` đặt `disposed = true` TRƯỚC `Flush(bounded)`, nên một flush quá hạn trong Dispose không bao giờ hoàn lại (Rust test ở session_writer.rs:556 ghi đúng điều này).

Khác biệt quan sát được:

| # | Đầu vào | C# | Rust | Mức |
|---|---|---|---|---|
| b5 | Khoá `key` | `Dictionary(StringComparer.OrdinalIgnoreCase)`, key = `CanonicalFolder` | `ignore_case_key(canonical)` | Low (hai bảng gấp chữ hoa/thường có thể khác với vài ký tự Unicode; không có test; suy luận, chưa chạy) |
| b6 | Tính `key` trong `RequeueSkipped` | tính lại `KeyOf(state.Folder)` lúc hoàn lại | dùng `key` đã tính khi `update` | Low (khác chỉ khi `CanonicalFolder` không tất định, ví dụ đường dẫn tương đối đổi thư mục hiện hành) |

### Test C# đã/chưa port

- `PerfCsvEscapeTests` 2/2, `PerfCsvListenerDropTests` 1/1, `PerfCsvListenerFaultTests` 3/3, `PerfCsvListenerMappingTests` 15/15, `PerfCsvListenerRobustnessTests` 6/6, `PerfCsvListenerStartupAndFaultCoverageTests` 5/5: tên có trong `diagnostics_perf_csv.rs` / `w2_perfcsv_killers.rs` / `w2_perf2_killers.rs` (so tên heuristic).
- `format_g17` chỉ có test ghim điểm chuyển ở `diagnostics_perf_csv.rs:499` (1e16, 1e17, 0.0001, 1e-5, -0, NaN) và golden `perf-csv-row` (24 ca g17) và `w2_perf2_killers.rs:42`. C# gốc không có test riêng của G17 ngoài `BuildRow_FormatsValuesInvariantly`. Khoảng trống (suy luận): `Infinity`/`-Infinity` không thấy trong các test đã liệt kê ở đây; không thấy ca làm tròn tràn số mũ (`9.9999999999999999e22`) hay số phụ thường (`5e-324`).
- `SessionWriterTests` 15/15 và `SessionWriterShutdownAndVersionTests` 15/15 có bản port trong `session_writer.rs`. `requeue_skipped` có hai mutant `==`->`!=` đã được kill bằng `w2_fs_mutation_killers.rs:109` và `w2_perfcsv_killers.rs:107` (tên file lệch chủ đề nhưng đúng nội dung). Chưa thấy test cho nhánh `disposed` ngay trong `requeue_skipped` ngoài `session_writer.rs:556` (có), và chưa thấy test cho `gate.versions.get(key) == Some(version)` đổi thành `!=` với một folder (suy luận, chưa chạy: test supersede ở `session_writer.rs:853` phủ được).

## (c) physical.rs `match_pattern` / `translate_win32_expression`

C#: `PhysicalFileSystem.EnumerateFiles` chỉ gọi `Directory.EnumerateFiles(dir, pattern ?? "*")` (PhysicalFileSystem.cs:184-188), tức hành vi của .NET `FileSystemEnumerableFactory.NormalizeInputs` + `FileSystemName.TranslateWin32Expression` + `MatchesWin32Expression`, KHÔNG phải `FindFirstFile` thuần của Win32. Rust: `crates/pr-core/src/io/physical.rs:154-345`.

Giống (đọc code, đối chiếu thuật toán .NET đã biết, chưa chạy oracle): chuẩn hoá `""`, `"."`, `"*.*"` thành `"*"`; dịch `?` -> `>` (DOS_QM); `.` theo sau bởi `?` hoặc `*` -> `"` (DOS_DOT); `.` cuối ngay sau `*` đổi `*` thành `<` (DOS_STAR) và bỏ `.`; mọi ký tự khác giữ nguyên; `match_pattern`: biểu thức rỗng hoặc tên rỗng không bao giờ khớp, `*` đầu và phần còn lại không có ký tự đại diện (`" < > * ?`) thì là "kết thúc bằng" theo OrdinalIgnoreCase, ngược lại NFA FsRtl với trạng thái `2*offset` và `2*offset+1`, DOS_STAR không ăn dấu chấm cuối cùng của tên, DOS_QM không ăn gì ở cuối tên hoặc trước `.`, DOS_DOT là `.` hoặc không gì quá cuối tên, `\` thoát ký tự kế. Làm việc trên đơn vị UTF-16 (đúng .NET: `?` chỉ ăn một nửa cặp surrogate).

Ví dụ đầu vào cụ thể (đều suy luận từ code, và 56 mẫu x 23 tên của golden `physical-fs-pattern` đã có sẵn kết quả C# thật):

| Mẫu | Tên | Kết quả mong đợi (C# = Rust) |
|---|---|---|
| `*.*` | `noext` | khớp (thành `*`) |
| `*.` | `noext` / `a.jpg` | khớp / không khớp (`<`) |
| `a?.jpg` | `a.jpg` | khớp (DOS_QM nhả 0 ký tự trước `.`) |
| `?a` | `a` | không khớp |
| `*.jpg` | `x.jpg.tmp` | không khớp |
| `*.JPG` | `a.jpg` | khớp (OrdinalIgnoreCase) |
| `a??.b` | `a<U+1F600>.b` | khớp (`?` ăn mỗi nửa cặp surrogate) |

Khác biệt quan sát được:

| # | Đầu vào | C# | Rust | Mức |
|---|---|---|---|---|
| c1 | `enumerate_files(dir, "sub\\*.jpg")` | `NormalizeInputs` tách `GetDirectoryName` ghép vào `dir`, tìm `*.jpg` trong `dir\sub` | trả `InvalidInput` ("The search pattern ... is not supported", physical.rs:1376). Đã ghi "UNPINNED/out of scope" | Low (không caller nào truyền; suy luận, chưa chạy) |
| c2 | mẫu có gốc (`C:\x`, `C:*`) | `ArgumentException` | `InvalidInput` (cùng thất bại, khác kiểu) | Low |
| c3 | `ToUpperInvariant` trong NFA và `OrdinalIgnoreCase` trong nhánh "kết thúc bằng" | .NET dùng hai bảng gấp chữ hơi khác nhau ở vài ký tự (ví dụ U+0131 `ı`) | Rust dùng một `ordinal_upper` cho cả hai | Low (suy luận, chưa chạy; ký tự cực hiếm trong tên ảnh) |

Không tìm thấy khác biệt ở trường hợp `'.'` cuối, `'*.*'`, `?`, `<`, `>`, `"`.

### Test C# đã/chưa port

C# KHÔNG có test trực tiếp cho khớp mẫu của `PhysicalFileSystem.EnumerateFiles` ngoài các test liệt kê thư mục trong `PhysicalFileSystemListingInterruptedTests` / `PhysicalIoMutationGapTests` (không dùng mẫu lạ). Nguồn chân lý là golden `physical-fs-pattern.json` sinh từ oracle C#, được chạy hai lần: `io_physical.rs:289` (chỉ bộ khớp) và `io_physical.rs:1588` (trên file thật). Khoảng trống của golden (suy luận, không có trong 56 mẫu): mẫu `a.` (dấu chấm cuối sau chữ), `..`, `*..`, `\` thoát ký tự (`a\*`), `<` đứng một mình với tên có nhiều dấu chấm (nhánh "không phải dấu chấm cuối" của DOS_STAR chỉ được `<.jpg` + vài `*.` chạm gián tiếp), `"` đứng một mình, `>` ở giữa mẫu nhiều dấu chấm, ký tự astral trong golden. Có một số test Rust riêng (`io_physical.rs:307-333`, `w2_phys4_killers.rs`) chạm được các nhánh này nhưng không có đối chứng C#; các khẳng định trong đó là từ suy luận, không phải từ oracle. Đề xuất: thêm vào `tests/golden-inputs/physical-fs-pattern.json` các mẫu trên rồi dùng skill `oracle` để sinh lại; đây là phần việc cho orchestrator, không làm trong lane tĩnh.

## (d) dialogs/settings/builder/groups.rs: ngắt dòng ở mép phải

C#: `src/PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageBuilder.cs` (`SettingsCheckFlowElement` :279-312, `SettingsGroupElement` :77-100, `SettingsCheckElement` :104-123, `SettingsRadioElement` :243-277). Lưu ý: không phải WPF/WinForms. Bản C# 7eaee2a2 đã là Win32 thuần; `SettingsPageBuilder.cs` giống hệt bản `d92d6aae` (module doc Rust ghi baseline `d92d6aae`; diff rỗng).

`CheckFlowElement::place_visible` giống từng dòng: `left = x + dx(indent)`, `right = x + width`, `gap = dx(10)`, `h = lineHeight + dy(2)`; mỗi ô rộng `textWidth(text) + dx(16)`; xuống dòng khi `cx > left && cx + iw > right` (nghiêm ngặt `>`: ô vừa khít mép phải ở lại cùng dòng; ô đầu tiên của dòng không bao giờ ngắt dù rộng hơn); sang dòng mới `cy += h + dy(4)`; độ rộng đặt `min(iw, right - left)`; trả `(cy - y) + h + dy(5)`; mục ẩn KHÔNG bị bỏ qua (không gọi `IsHidden` trong flow).

Khác biệt quan sát được:

| # | Đầu vào | C# | Rust | Mức |
|---|---|---|---|---|
| d1 | Số học | `int` bình thường (tràn thì quấn) | `i32` bão hoà (`saturating_*`) | Low (chỉ khác với toạ độ gần 2^31, không đạt được) |
| d2 | `right < left` (vùng hẹp hơn thụt lề) | `Math.Min(itemWidth, right - left)` cho chiều rộng âm | `iw.min(sub(right,left))` cũng âm | Giống nhau, không phải khác biệt |

Không có khác biệt hành vi quan sát được ở `GroupElement` (`pad = dx(8)`, `top = lineHeight + dy(5)`, `innerWidth = max(dx(40), width - 2*pad)`, trả `(cursor - y) + dy(8)`), `CheckElement` (`textWidth = max(dx(20), width - indent - dx(14))`, `height = max(lineHeight + dy(2), textHeight + dy(1))`, trả `height + dy(4)`) và `RadioElement` (gợi ý thụt thêm `dx(12)`, rộng tối thiểu `dx(30)`).

### Test C# đã/chưa port

C# `tests/PhotoReview.Shell.Tests/` KHÔNG có test nào cho `SettingsCheckFlowElement` (grep `CheckFlow` ở `7eaee2a2:tests` rỗng); `ISettingsLayoutSurface`/`SettingsPageBuilder` chỉ xuất hiện gián tiếp trong `MouseSettingsPageTests`, `SettingsFilesPageTests`, `SettingsShortcutsPageTests` (qua trang cụ thể). Vì vậy không có test C# để "port"; các test Rust (`settings_builder_layout.rs`, `w8_settingsui_killers.rs` họ `check_family`, `w8_setui_killers.rs:123 check_flow_exact_right_edge_stays_on_row_and_one_pixel_less_wraps`) đều là test Rust dựng từ đọc nguồn C#. Mutant `>`->`>=` ở groups.rs:234 từng SÓT và đã được `w8_setui_killers` giết (CX-SETUI-2026-10-31.md). Đối chiếu của tôi xác nhận rằng test đó khớp đúng nguồn C# (`cursorX + itemWidth > right`). Khoảng trống còn lại là của oracle: chưa có golden cho trình bày thật của một trang chứa `CheckFlow` (ví dụ trang Files/Display) so với C# trên cùng DPI và phông.

## Đề xuất cho orchestrator (không sửa PARITY ở lane này)

1. Hàng PARITY `F-13` (`journal.rs` append): ghi chú "UnrepresentablePath thuộc về W3-04, OQ2" đang là hứa hẹn chưa có chủ. Đề xuất mở việc xác minh điểm chuyển `PathBuf` sang `String` của hành động tệp (grep `to_string_lossy` trong `file_actions/service.rs`, `undo.rs`, `recovery_retry.rs`) và thêm kiểm tra trước khi journal Prepared. Bằng chứng: journal.rs:654, errors.rs:45-69, không có nơi phát mã trong `crates/*/src`.
2. Golden `physical-fs-pattern`: bổ sung 7 mẫu ở mục (c) rồi dùng skill `oracle`.
3. `JournalDurabilityTests` (3 test cấp Settings) và `JournalSmallGapsTests.JournalCodedException_*`: kiểm tay xem đã phủ ở `settings_store.rs` / `model_enums.rs`; nếu chưa, thêm bản port.
4. Không đề xuất đổi trạng thái hàng PARITY nào: bốn vùng đều "Partial" do thiếu bằng chứng mutation, không phải do lệch hành vi.

## Giới hạn của lane

- Không chạy oracle, không chạy mutation, không chạy test; các ví dụ đầu ra ở trên là suy luận từ code và chưa được chạy, trừ phần golden `physical-fs-pattern` là dữ liệu C# thật nhưng tôi không chạy lại.
- Bảng "đã port" dựa trên so khớp tên và grep nên có thể bỏ sót test đặt tên khác.
- Không đọc `JournalLineParser.cs` / `JournalCompactionPlan.cs` / `SessionStore.cs` / `PerfCsv` ngoài `PerfCsvListener.cs`: ngoài phạm vi bốn vùng.
