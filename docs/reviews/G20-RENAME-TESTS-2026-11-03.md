# G20 Rename part: test inventory (2026-11-03)

Branch `test/g20-rename`, base `origin/master` 10b2b2f, C# baseline `7eaee2a2`. Tests only, no production change.

## Finding: the Rename tests of W9-R2, W9-R4 and W9-R5 already exist

`BACKLOG.md` G20 and `test-strengthening-plan.md` (tp-pr-core T1-T4, tp-pr-app T10-T11, tp-pr-imaging T2-T5) still say the
Rename tests are missing. That text predates the merged W9-R units. I listed every `[Fact]`/`[Theory]` of the C# classes
`RenameFileActionTests`, `RenameFaultInjectionTests`, `FileActionControllerRenameTests`,
`FileActionControllerRenameUndoTests`, `ReviewCatalogRenameTests`, `ImageCacheKeyTests` and
`PreviewImageServiceGapTests2` at `7eaee2a2` and matched each against Rust `fn` names by word set (stop words and the
C# `Async` suffix dropped; 85% word overlap). All 115 distinct names have a Rust twin; the 9 below the threshold were checked by hand:

| C# test | Rust twin |
|---|---|
| `Rename_FailureAfterTheFileWasRenamed_StaysAFailedRecoveryItemNeverDismissed` | `rename_service.rs::rename_failure_after_mutation_is_failed_and_source_removed` |
| `Rename_OsRefusesTheMove_...Dismissed` | `rename_service.rs::rename_failure_before_mutation_is_dismissed_with_original_metadata` |
| `RenameGroup_CaseOnly_SecondMemberFails_...` | `rename_service.rs::rename_group_case_only_member_failure_rolls_back_prior_members_and_dismisses` |
| `RenameGroup_InvalidName_...` | `rename_service.rs::rename_group_invalid_stem_returns_catalog_message_without_journal` |
| `RenameGroup_SidecarNamedAfterTheRawFile_...` | `rename_service.rs::rename_group_compound_xmp_follows_raw_owner_and_normalizes_typed_stem` |
| `Rename_TargetIsAFolder_IsRefused` | `rename_file_action_matrix.rs::rename_target_is_a_folder_is_refused` |
| `FileOperationType_HasNoRenameMember_...` | `rename_file_action_matrix.rs::file_operation_type_has_no_rename_member_for_older_build_journal_compatibility` |
| `EvictCachedPath_Given...UnnormalizedLowerCasePath_...` | `pr-imaging/tests/preview_service_state.rs::evict_cached_path_given_an_unnormalized_lower_case_path_...` |
| `UndoLast_AfterCaptureGroupRename_...` | `pr-app/tests/file_action_rename.rs::undo_last_after_capture_group_rename_uses_remembered_names_for_every_member` |

The matching is by name, not a proof that every assertion equals the C# one (see the strengthening below). The `*Matrix`
fault tests are in `rename_fault_injection.rs`; native cases are `#[ignore]`d with a `native` reason.

## What was added

`crates/pr-core/tests/g20_rename_strengthen.rs`, 3 tests. They are stricter twins of tp-pr-core T1, T2, T3; the weaker
originals stay. Added assertions: both files' bytes after a refused rename (T1, T2), zero mutating `FakeFileSystem` calls
(Move, Copy, Delete, WriteAtomic, OpenAppend, CreateDirectory) after a refusal, the foreign file keeps its stored
spelling `b.jpg` (T2), and for the success case `no_change == false`, exact `destination_path`, destination bytes, equal
`FileStat` before and after, one Prepared and one Committed line of one id, `Source`/`Destination`, no `GroupMembers`, no `Undo` (T3).
All three pass on current master code, so no behaviour change is needed.

## Proposal rows

None. No test needed a production change, so nothing is ignored for that reason and no P-120 row is proposed.

## Not done

* The tp-pr-core T3 second half (JSON key set equals an ordinary Move) is already in `rename_service.rs::rename_journal_line_uses_exactly_the_schema_of_an_ordinary_move`.
* No mutation run on the three new tests; they were not checked by deliberately breaking the service.
* No full workspace test run; native `#[ignore]` tests were not run.

## Proposed wording (not applied)

* BACKLOG G20, Rename part: "2026-11-03 re-check: every C# Rename test of W9-R2/R4/R5 already has a Rust twin
  (115/115 by name, see `G20-RENAME-TESTS-2026-11-03.md`); 3 stricter twins added in `g20_rename_strengthen.rs`. The Rename
  part of G20 is done; the plan items T1-T4, T10, T11 (pr-core/pr-app) and T2-T5 (pr-imaging) are implemented."
* PARITY.md / README.md / task_on_progress.md: no status change; at most add the evidence link to rows `X-w9r2-case-only-file-action-rename`.
