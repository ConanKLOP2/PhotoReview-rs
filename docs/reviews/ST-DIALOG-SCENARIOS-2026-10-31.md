# ST-DIALOG-SCENARIOS (2026-10-31)

Lane `st-dialog-scenarios`. Hai hàng PARITY: `X-dialogs-recovery` và `X-dialogs-zoom-diagnostics-profiles-benchmark`. Nguồn C#: `git show 7eaee2a2:tests/PhotoReview.Shell.Tests/Dialogs/Native/*`. Không sửa `docs/PARITY.md` hay `task_on_progress.md`.

## 1. Kết quả

| Việc | Kết quả |
| --- | --- |
| Lỗi thật tìm thấy | `ProductionSecondaryWindowDialogs::recovery` truyền `allow_permanent_delete: None` cho cửa sổ Recovery. C# (`Win32SecondaryWindowPort.ShowRecovery`) truyền `() => settings?.Current.AllowPermanentDeleteWithoutRecycleBin == true`. Hậu quả: câu hỏi Retry của nhóm Delete không bao giờ cảnh báo xóa vĩnh viễn dù cài đặt đang bật (dịch vụ Retry vẫn đọc cài đặt đúng; chỉ cảnh báo trong hộp thoại bị mất). Đã sửa. |
| Tách closure | Ba closure của composition (Retry, Dismiss, allow-permanent-delete, checker) được rút ra thành `pr_shell::startup::secondary_dialogs::recovery_window_options` (hành vi cũ giữ nguyên, trừ việc sửa lỗi trên) để test được không cần cửa sổ. |
| Test mới (pr-shell, headless, chạy được trên CI) | `crates/pr-shell/tests/dialogs_recovery_composition.rs`: 8 pass (`cargo test -p pr-shell --test dialogs_recovery_composition`). |
| Test mới (pr-platform-win, native, `#[ignore = "native: needs desktop"]`) | 3 file, 19 test, chạy với `--include-ignored --test-threads=1` trên máy này: 7 + 9 + 3 = 19 pass. |
| fmt / clippy | `cargo fmt` cho hai crate; `cargo clippy -p pr-shell --tests -- -D warnings` và `cargo clippy -p pr-platform-win --test <3 file mới> -- -D warnings` sạch. |

File native mới:

- `dialogs_action_profiles_scenarios_native.rs` (7 pass): Enter trong ô tên, mọi đường hủy (nút, Esc, WM_CLOSE, SC_CLOSE), Import x3, Export x2.
- `dialogs_zoom_diagnostics_scenarios_native.rs` (9 pass): Zoom validate (10 giá trị), giá trị sai hiện thông báo và dialog lớn ra, Enter/Esc, 4 đường đóng, WM_QUIT được post lại, không owner/owner đã chết; Diagnostics 15 cặp nhãn-giá trị đúng thứ tự, 5 đường đóng, nút Copy.
- `dialogs_benchmark_scenarios_native.rs` (3 pass): Quick check chạy hồ sơ cố định bất kể chọn gì, lần chạy thứ hai xóa hàng cũ trước kết quả đầu, sắp xếp cột tăng rồi giảm theo giá trị và giữ hàng đã chọn.

Hộp thoại mở trên owner ẩn, điều khiển từ callback `ready` (không chặn); file dialog, journal, scan, session benchmark đều là fake; file ghi trong `TempRoot`.

## 2. Đối chiếu test C# với bản port

Ký hiệu: P = đã có bản port (đã có trước lane, hoặc port trong lane này = "L"); G = phủ bằng golden/presenter test thuần, không có bản native; X = chưa có bản port.

### RecoveryDialogDataSafetyTests (17)

- P (`dialogs_recovery_data_safety.rs`, native, 7 test phủ 11 kịch bản): Retry_Move, Retry_Copy, Retry_Declined, Retry_WhenTheSourceChanged, Retry_Superseded, Retry_OfUserCancelled, ClearSelected, ClearAll, ClearAll_Declined, Clear_Stale, GroupDeleteRetry.
- L (`dialogs_recovery_composition.rs`): phía closure composition của Retry_Move/Copy, Retry_WhenSourceChanged, Retry_Superseded, `Port_Retry_ConfirmsAUserCancelledEntry...` (đồng nghĩa trong gap), `Port_Clear_WritesTheDismissal...`, `Port_RetryQuestion_WarnsAboutPermanentDeletion_OnlyWhenTheSettingAllowsIt` (phát hiện lỗi `None`).
- P (khác): `Port_ShowRecovery_WithoutAJournal` (`startup_secondary_dialogs.rs`), `ReadingTheJournal_ReportsIoAndAccessErrors` (`try_read_recovery_entries` trong `w8_dialogs_mutation_killers.rs`), `Port_ShowRecovery_OpensTheJournalsEntries_OwnedAndModal_AndClosesCleanly` (`ui_smoke_dialogs.rs`, native).
- X: `Port_ShowRecovery_WithAnUnreadableJournal_TellsTheUser` ở mức port đầy đủ, `Port_Dispose_ClosesAnOpenRecovery_AndLaterCallsDoNothing`, `Port_Copy_UsesTheShellsClipboardService` (chỉ nối dây `copy` trong `recovery()` đã đọc, chưa có test riêng).

### RecoveryDialogGapTests (18) và RecoveryDialogTests (43)

- P (native/headless, `dialogs_recovery_flow_e2e.rs`, `dialogs_recovery_window_e2e.rs`, `dialogs_recovery_flow.rs`, `dialogs_recovery_actions.rs`): Retry declined/confirmed/rejected/failed/superseded, đóng lúc có notice không check lại, đóng giữa chừng Retry, ClearAll gồm hàng bị lọc, Filter giữ chọn theo danh tính, mở với hàng trước khi check, journal rỗng, xóa hàng chọn không nhảy sang hàng kề.
- G: huy hiệu và tông màu, chi tiết, bố cục (17 + 58 + 12 golden `recovery-dialog-*`), `ActionLine` bỏ khi không có, `TopIndex`, thông điệp tường thuật trong `pr-app` (`recovery_window_presenter.rs`, `presenters_recovery_*`).
- X (cần cửa sổ thật, chưa port): `TheWaitCursorIsSetTheMomentTheRetryStarts...`, `WhileARetryRuns_TheWaitCursorIsShownOverTheDialog`, `WhenTheFocusedButtonGetsLocked_TheFocusMovesToTheList`, `WhenTheFilterEmptiesTheList_TheFocusMovesToTheFilter`, `Refresh_KeepsTheListScrolledWhereTheUserLeftIt` (mức cửa sổ), `ReadOnlyBoxes_ArePaintedLikeInputFields`, `Modal_DisablesTheOwner_AndEscAndTheCloseButtonClose`, `InTheModalLoop_EscapeIsStillRefusedWhileARetryRuns`, `Layout_AtTwoScales...` (2 ngôn ngữ x 2 DPI), `TheLayout_FollowsTheWindowSize_AndTheListGrowsWithIt`, `TheDialogIsDark_ThemeIsAppliedOnce...`, `OpeningAndClosingRepeatedly_DoesNotLeakGdiOrUserObjects`, `EverythingTheViewTells_ComesFromTheLocalizer`, `ShowInExplorer_*` ở mức native (phần seam có `explorer_arguments_quotes...` trong `dialogs_recovery_details.rs`).
- Ghi chú trung thực: danh sách X của hai file này suy ra từ tên test C# so với tên test Rust (grep `dialogs_recovery*`, `recovery_window_presenter.rs`, `src/dialogs/recovery`), chưa kiểm từng thân test; một vài mục X có thể đã được phủ gián tiếp bằng presenter test.

### ActionProfilesDialogTests (23)

- P trước lane (`dialogs_action_profiles_native.rs`, 11): Opens, EmptyList, Badge, SelectingAnotherRow, OperationAndConfirm, Add, Remove, Apply valid/blank/duplicate/destination escape.
- L (7 test Rust mới, phủ 12 mục C# kể cả các InlineData): `Enter_InTheNameBox`, `EveryCancelPath` (4), `Import_*` (3), `Export_*` (2).
- X: `Modal_DisablesTheOwner_AndRestoresItOnClose`, `ModalInsideModal_...` (modal lồng, chặn), `ChromeIsApplied_ExactlyOnce_WithTheComboAndTheListBox` (cần `RecordingChrome` + combo/list), `Layout_DoesNotOverlapOrCutAnything` (2 ngôn ngữ x 2 DPI), `OpeningAndClosing_DoesNotLeakGdiOrUserObjects`.
- Khối benchmark port `Port_ActionProfiles_ReturnsTheEditedList_OnApply_AndNullOnCancel`: phủ bằng `action_profiles_finish_returns_list_only_for_ok` (`startup_secondary_dialogs.rs`, headless).

### ZoomPromptDialogTests (19)

- P trước lane: `Apply_WithAValidValue` (1).
- L: `Apply_Validates...` (10 giá trị), `InvalidValue_ShowsTheMessage...`, `Enter/Escape`, `EveryWayToClose` (4), `WmQuit`, `WithoutAnOwner_OrADeadOwner`.
- X: `Opens_WithCurrentValueSelected_FocusOnTheBox_OwnerDisabledWhileOpen_AndReEnabledAfter`, `ADialogInstance_CanBeShownOnlyOnce`, `Dialog_IsCenteredOnTheOwner_AndInsideTheWorkArea`, `Chrome_IsAppliedOnce`, `DarkColors_AreReturned...`, `DpiChange_RescalesFontAndLayout`, `TabOrder_IsBoxThenCancelThenApply`, `VietnameseStrings_AreNotClipped`, `ClosedDialog_WindowIsDestroyed`, `ClientWidth_IsTwoHundredDialogUnits`, `RepeatedOpenAndClose_LeaksNoGdiOrUserObjects`, `AnExceptionInTheDialogCallback_ReachesTheCaller...`. Bố cục thuần (layout/preferred size/worked example) vẫn được phủ bởi golden `dialog-zoom-diagnostics`.

### DiagnosticsAndListDialogTests, phần Diagnostics (8)

- P trước lane: `EveryCloseRoute` (close() route).
- L: 15 cặp nhãn-giá trị (cả `RowIndexMapsToTheRightField`), 5 đường đóng, nút Copy.
- X: `LongVietnameseValues_WrapInsteadOfBeingClipped`, `Chrome_IsAppliedOnce...`, `RepeatedOpenAndClose_...Leaks`, `ListAndDiagnostics_DpiChange_Rescales...`. Phần List/SkippedFiles/BatchReview thuộc hàng X-dialogs-native-base, không đụng trong lane này.

### BenchmarkDialogTests (26)

- P trước lane: Opens, Pictures folder, Tooltip + badge, Run/progress/rows/OpenReport/OpenFolder, SelectAll/SelectNone, Cancel/Esc/Close giữa chừng, Browse (2), Run khi không chọn gì.
- L: QuickCheck, ASecondRun clears rows, ClickingAColumnHeader (tăng, giảm, theo giá trị), SortingKeepsTheSelectedRow.
- G: `next_sort`, `compare_f64_dotnet` (`w8_dialogs_killers3.rs`), 45 golden `dialog-benchmark-layout`, 30 golden `dialog-benchmark-results` (thứ tự hàng, best, màu hàng, chuỗi hồ sơ).
- X: `Escape_WhenIdle_ClosesTheWindow`, `TabKey_PostedToTheWindow_IsHandledByTheDialogManager...`, `Rows_AreListedInProfileOrder_AndTheBestOfASharedWorkloadIsMarked` ở mức native (đã golden), `RowColours_BestIsGreen...` ở mức custom-draw native, `SwitchingLanguage_WhileOpen_RetranslatesTheWindow`, `Layout_AtTwoScales...`, `OpeningAndClosingRepeatedly_DoesNotLeakGdiOrUserObjects`, 2 test `Port_*` (`Port_OpensOneBenchmarkWindow_SecondOpenBringsItForward_ReopensAfterClose`, `Port_Dispose_WhileARunIsActive...`).

## 3. Phần còn lại là việc người hoặc cần máy có DPI/ngôn ngữ

- Kịch bản DPI 100%/150% và đổi DPI giữa chừng, và 2 ngôn ngữ (Anh/Việt) cho mọi hộp thoại trên: cần máy/thiết lập hiển thị khác nhau, không đổi ngôn ngữ toàn tiến trình trong test song song.
- GDI/USER object leak khi mở/đóng lặp: cần ngưỡng đo đã thống nhất (hàm `gdi_objects`, `settles_at_or_below` đã có trong fixture, chưa dùng ở các hộp thoại này).
- Modal thật (owner bị tắt, modal lồng): người chạy `win32_dialog_native` / `ui_smoke_dialogs` ở phiên có màn hình.
- Con trỏ chờ, thay đổi focus, cuộn danh sách của RecoveryDialog: cần quan sát thật.
- "Hand-break" (phá tay) cho cả hai hàng: là việc của người sau khi merge.
- Chưa chạy cargo-mutants (theo giới hạn của lane).

## 4. Đề xuất đổi PARITY (cho orchestrator, lane này không sửa)

- `X-dialogs-recovery`: giữ Partial. Cập nhật Ghi chú: "Retry/Dismiss closure từ composition (P-43) đã có test headless (`dialogs_recovery_composition.rs`, 8 pass); lỗi composition `allow_permanent_delete: None` đã sửa, cảnh báo xóa vĩnh viễn trong câu hỏi Retry nay đọc cài đặt hiện tại". Phần còn lại: danh sách X ở mục 2.
- `X-dialogs-zoom-diagnostics-profiles-benchmark`: giữ Partial. Ghi thêm: 19 test native mới (ActionProfiles Import/Export/hủy/Enter, Zoom validate/đóng/WM_QUIT/owner chết, Diagnostics 15 hàng/đóng/copy, Benchmark QuickCheck/lần 2/sort) đều pass trên máy phát triển với `--include-ignored --test-threads=1`. Phần còn lại: DPI/ngôn ngữ/GDI/modal/chrome/dark của từng hộp thoại (mục 2 và 3).
- Không nâng Done: còn kịch bản native chưa port và chưa có hand-break.
- `native-nightly.yml`: các test native mới chưa được thêm vào danh sách nightly (lane không sửa workflow); orchestrator có thể thêm 3 binary `dialogs_*_scenarios_native`.
