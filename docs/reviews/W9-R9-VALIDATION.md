# W9-R9 rename inventory and fault-matrix evidence

Verified 2026-10-27 against C# `7eaee2a2`; Rust base `da42ca1e` (R1-R5 and the R3 mutation follow-up merged). This slice adds tests, not production code. Final workspace/PR evidence is recorded separately when that gate completes.

## Scope and evidence

- Dedicated action inventory:42C# methods mapped below to existing R2/R3 twins or new R9 assertions. Existing R1 naming-policy tables remain separate. The R2 native case-sensitive probe is supplemented by an R9 native service/journal refusal test.
- Dedicated fault inventory:12C# methods;10new scenario twins here, with the two existing R3 scenarios retained in `file_actions_matrix.rs`. They are not duplicated. Two additional fake test functions guard atomic-rename fault classification and second-order retry interruption.
- Owner rerun: action binary20passed/1native ignored, fault binary12passed; zero failures. Separate explicit native run1/1PASS: case-sensitive TempRoot with two distinct case variants, service rejects the rename, both contents survive and no journal line is appended. No real Recycle Bin is used; this is filesystem/service evidence, not GUI/RAW/decoder evidence.
- Normal matrices:66numbered mutation points and286fault runs. Nested reconcile:70runs. Second-order retry:132outer action-fault runs and834nested retry-fault runs. Every applicable plan checks its numbered call and mutation kind was reached. Same-folder atomic rename excludes partial-copy/copy-not-deleted faults.
- Assertions cover exact payload uniqueness, seeded foreign-file preservation, complete returned-success/restored effects, failed Move `SourceRemoved`, stored spelling, ordered moved-on/undo journal evidence, committed size and exact mtime, latest Prepared elimination after restart, same/fresh-instance idempotence, retry state and same-named foreign follow-up moves. Live-verdict summaries retain operation/Undo/member mapping and ignore Dismissed/Ids, like the C# helper.
- C# `FaultInvariants.FollowUpsAsync` and `FaultInjectionSecondOrder.RetryAllAsync` discard individual retry return values, then verify resulting disk/journal state. This port additionally checks successful retry result/entry/persistence evidence. It does not invent a requirement to classify every failed retry result; no full mutation verdict is claimed for this tests-only change.

## Measured runs

| Scenario | Numbered points | Fault runs | Nested reconcile |
|---|---:|---:|---:|
| Rename |5|22|25|
| CaseOnly |5|22|R3 owns25|
| Group |7|30|45|
| Taken preflight |0|0|0|
| RenameUndo |11|48|0|
| CaseUndo |11|48|0|
| GroupUndo |17|72|0|
| TwoRenames |10|44|0|

Second-order retry outer/nested runs: Rename10/22, CaseOnly10/22, Group14/134, RenameUndo22/88, CaseUndo22/88, GroupUndo34/436, TwoRenames20/44. The two R3 scenarios have their earlier measured30group-case fault runs and25case-only nested reconciles; those are separate inherited evidence from #282, not added to the R9 owner rerun totals above.

## Dedicated action inventory

The exact method identifiers below come from `tests/PhotoReview.Core.Tests/FileActions/RenameFileActionTests.cs` at the pinned C# baseline. Existing owners are verified by their assertions; the R2 second-member rollback test uses a behavior-oriented Rust name and a production-method comment, so it was checked manually rather than treated as missing by a text search.

| C# method | Rust test owner |
|---|---|
| `CaseOnlyRename_FileStoredUnderAThirdSpelling_IsNeverLostOrDone` | `rename_recovery::case_only_rename_file_stored_under_a_third_spelling_is_never_lost_or_done` |
| `CaseOnlyRename_MoveFails_NothingChangedAndTheLineIsDismissedNotLost` | `rename_file_action_matrix::case_only_rename_move_fails_nothing_changed_and_line_is_dismissed_not_lost` |
| `CaseOnlyRename_RecoveryRetryOfAFailedLine_FinishesTheRename` | `rename_recovery::case_only_rename_recovery_retry_of_failed_line_finishes_the_rename` |
| `CaseOnlyRename_RecoveryVerdict_FollowsTheStoredSpelling` | `rename_recovery::case_only_rename_recovery_verdict_follows_the_stored_spelling` |
| `CaseOnlyRename_StartupReconcile_FollowsTheStoredSpelling` | `rename_recovery::case_only_rename_startup_reconcile_follows_the_stored_spelling` |
| `CaseOnlyRename_Succeeds_StoresTheNewSpellingAndIsCommitted` | `rename_service::case_only_rename_succeeds_stores_the_new_spelling_and_is_committed`<br>`rename_service::native_case_only_rename_preserves_requested_stored_spelling`<br>`rename_service::native_service_rename_and_case_only_rename_use_only_owned_temp_files` |
| `CaseOnlyRename_ToTheSpellingTheFileAlreadyHas_IsNoChange` | `rename_file_action_matrix::case_only_rename_to_the_spelling_the_file_already_has_is_no_change` |
| `CaseOnlyRename_TwoDifferentFilesInACaseSensitiveFolder_IsRefused` | `rename_service::native_case_sensitive_directory_reports_two_case_variants`<br>`rename_file_action_matrix::case_only_rename_two_different_files_in_case_sensitive_folder_is_refused`<br>`rename_file_action_matrix::case_only_rename_native_two_different_files_in_case_sensitive_folder_is_refused` |
| `CaseOnlyRename_TwoEntriesDifferingByCase_AreTwoFiles_Conflict` | `rename_recovery::case_only_rename_two_entries_differing_by_case_are_two_files_conflict` |
| `CaseOnlyRename_Undo_RestoresTheOldSpelling` | `rename_recovery::case_only_rename_undo_restores_the_old_spelling`<br>`rename_recovery::case_only_rename_native_move_undo_reconcile_uses_temp_root` |
| `CaseOnlyRename_UndoMovedTheFileButTheCheckFailed_DropsTheEntryByTheStoredSpelling` | `rename_recovery::case_only_rename_undo_moved_the_file_but_the_check_failed_drops_the_entry_by_the_stored_spelling` |
| `CaseOnlyRename_UndoOfAnAlreadyUndoneRename_DoesNotRunTwiceAndDropsTheEntry` | `rename_recovery::case_only_rename_undo_of_an_already_undone_rename_does_not_run_twice_and_drops_the_entry` |
| `CaseOnlyRename_UndoWhenTheFileWasRenamedBackByHand_DropsTheDeadHistoryEntry` | `rename_recovery::case_only_rename_undo_when_the_file_was_renamed_back_by_hand_drops_the_dead_history_entry` |
| `FileOperationType_HasNoRenameMember_SoAnOlderBuildReadsEveryJournalLine` | `rename_file_action_matrix::file_operation_type_has_no_rename_member_for_older_build_journal_compatibility` |
| `OrdinaryMoveOrCopyIntoTheSourceFolder_IsStillRefused` | `rename_file_action_matrix::ordinary_move_or_copy_into_the_source_folder_is_still_refused` |
| `Rename_FailureAfterTheFileWasRenamed_StaysAFailedRecoveryItemNeverDismissed` | `rename_service::rename_failure_after_mutation_is_failed_and_source_removed`<br>`rename_service::rename_destination_verification_failure_is_failed_and_source_removed` |
| `Rename_FileLockedByAnotherProgram_FailsCleanlyAndIsDismissed` | `rename_file_action_matrix::rename_file_locked_by_another_program_fails_cleanly_and_is_dismissed` |
| `Rename_ForeignFileAppearsAtTheTargetAfterThePreflight_IsNeverOverwritten` | `rename_file_action_matrix::rename_foreign_file_appears_at_the_target_after_the_preflight_is_never_overwritten` |
| `Rename_InvalidTarget_IsRejectedBeforeAnythingIsJournaled` | `rename_service::rename_invalid_target_is_rejected_before_anything_is_journaled` |
| `Rename_JournalLine_UsesExactlyTheSchemaOfAnOrdinaryMove` | `rename_service::rename_journal_line_uses_exactly_the_schema_of_an_ordinary_move` |
| `Rename_OsRefusesTheMove_NothingChangedSoTheLineIsDismissedNotARecoveryItem` | `rename_service::rename_failure_before_mutation_is_dismissed_with_original_metadata`<br>`rename_service::rename_uncertain_source_stat_stays_failed_but_does_not_claim_source_removed` |
| `Rename_SourceMissing_FailsWithoutJournal` | `rename_file_action_matrix::rename_source_missing_fails_without_journal` |
| `Rename_Success_MovesInPlaceAndJournalsOneCommittedMove` | `rename_service::rename_success_moves_in_place_and_journals_one_committed_move`<br>`rename_service::rename_pre_cancel_and_journal_append_failure_preserve_failure_evidence`<br>`rename_service::rename_failed_terminal_append_reports_persisted_mutation`<br>`rename_service::native_service_rename_and_case_only_rename_use_only_owned_temp_files` |
| `Rename_TargetIsAFolder_IsRefused` | `rename_file_action_matrix::rename_target_is_a_folder_is_refused` |
| `Rename_TargetTakenByAnotherFile_IsRefusedAndTouchesNothing` | `rename_service::rename_target_taken_by_another_file_is_refused_and_touches_nothing` |
| `Rename_TargetTakenByAnotherFileWithOtherCase_IsRefused` | `rename_file_action_matrix::rename_target_taken_by_another_file_with_other_case_is_refused` |
| `Rename_ThenUndo_AfterTheOriginalNameWasTakenByAnotherFile_IsRefused` | `rename_file_action_matrix::rename_then_undo_after_the_original_name_was_taken_by_another_file_is_refused` |
| `Rename_ThenUndo_PutsTheOriginalNameBack` | `rename_file_action_matrix::rename_then_undo_puts_the_original_name_back` |
| `Rename_ToTheSameName_IsANoChangeSuccessWithNoJournalLine` | `rename_service::rename_to_the_same_name_is_a_no_change_success_with_no_journal_line` |
| `Rename_WhileAnotherActionIsRunning_IsRejectedAsBusy` | `rename_file_action_matrix::rename_while_another_action_is_running_is_rejected_as_busy` |
| `RenameGroup_AMemberCannotBeRenamed_RollsTheOthersBackAndDismissesTheLine` | `rename_file_action_matrix::rename_group_member_cannot_be_renamed_rolls_others_back_and_dismisses` |
| `RenameGroup_CaseOnly_RecoveryRetryOfAFailedGroupLine_FinishesEveryMember` | `rename_file_action_matrix::rename_group_case_only_recovery_retry_of_failed_group_line_finishes_every_member` |
| `RenameGroup_CaseOnly_RenamesAllMembersUndoesAndReconcilesClean` | `rename_file_action_matrix::rename_group_case_only_renames_all_members_undoes_and_reconciles_clean` |
| `RenameGroup_CaseOnly_SecondMemberFails_FirstIsPutBackAndTheLineIsDismissed` | `rename_service::rename_group_case_only_member_failure_rolls_back_prior_members_and_dismisses` |
| `RenameGroup_EveryMemberAlreadyNamed_IsASuccessWithNoJournalLine` | `rename_file_action_matrix::rename_group_every_member_already_named_is_a_success_with_no_journal_line` |
| `RenameGroup_InvalidName_IsRejectedBeforeAnything_IsTouched` | `rename_service::rename_group_invalid_stem_returns_catalog_message_without_journal` |
| `RenameGroup_OneTargetTaken_ChangesNothingAndJournalsNothing` | `rename_service::rename_group_one_target_taken_changes_nothing_and_journals_nothing` |
| `RenameGroup_PartlyAlreadyNamed_RenamesOnlyTheOthers` | `rename_service::rename_group_mixed_noop_compound_owner_all_noop_and_case_variants`<br>`rename_file_action_matrix::rename_group_partly_already_named_renames_only_the_others` |
| `RenameGroup_RenamesEveryMemberInOneJournalGroup_EachKeepingItsExtension` | `rename_service::rename_group_renames_every_member_in_one_journal_group_each_keeping_its_extension` |
| `RenameGroup_SidecarNamedAfterTheRawFile_FollowsTheRawFile` | `rename_service::rename_group_compound_xmp_follows_raw_owner_and_normalizes_typed_stem` |
| `RenameGroup_ThenUndo_RestoresEveryName` | `rename_file_action_matrix::rename_group_then_undo_restores_every_name` |
| `RenameGroup_TwoMembersToTheSameTarget_IsRefusedBeforeAnythingIsJournaled` | `rename_file_action_matrix::rename_group_two_members_to_the_same_target_is_refused_before_anything_is_journaled` |

## Local evidence hashes

Logs are under `%TEMP%/goal-r9-owner-*`; source files are the two test files in this PR. Tests used the owned `target-r9`, jobs2 and900-second outer guards with retained process handles; focused/native exit codes were both0. A second cache `worktree/target-r9` from the CLI round is retained but unused by owner checks. Cleanup policy rejections are not bypassed.

| File | SHA-256 |
|---|---|
| `goal-r9-owner-focused-result.txt` | `1410ECC0CE6C450B67B1C6DBA280D27FA446A964EADA9649F204B853BC509198` |
| `goal-r9-owner-focused-out.log` | `68165A7F50AC68C3EF1A290453837A7EF3C8B93602FD0541764C4469D2BEAD1C` |
| `goal-r9-owner-focused-err.log` | `14EBFC031513A0EDD36A8E058F5AB6B8BB6B09273FA19E2D4EBD244FD7C94233` |
| `goal-r9-owner-native-result.txt` | `F0DEFA4AEF11C2C3770A181E5E85AC97D7A9B61678549AC990C35557625E41C3` |
| `goal-r9-owner-native-out.log` | `47A083BB9D4D7232713C77FD3FA30CF8D35CA04F0BC5E90A25194040FBCD749A` |
| `goal-r9-owner-native-err.log` | `99297874273B33B6B735BF64EBA1E68DDA8E054CD247C5B7A0AFA99D3794043A` |
