# FIX-DUPCLEAN-M1 (2026-10-31): duplicate cleanup does not re-check the files after the review dialog

Status: fixed on this branch (data-safety fix). Source: [ST-INVMAP-2026-10-31.md](ST-INVMAP-2026-10-31.md) item M1.

## What the C# does (baseline 7eaee2a2, `DuplicateCleanupController.cs`)
After `ShowBatchReview` returns true the only checks are: folder generation unchanged, executor not busy, and
`remove.Where(path => _catalog.IndexOf(path) >= 0)`. The catalog is an in-memory list, so this does NOT look at the disk. Then
`FileActionService.ExecuteAsync(Recycle)` per file. The keeper is not tracked at all (`DuplicateFinder.FindAsync` returns only
the files to remove). The batch is never registered for Undo (RV-D5 = A, comment in the C# method). So Rust equalled C#: the
bug is real in both.

## Risk
While the modal is open the keeper can be deleted, renamed or overwritten outside the app (or a copy edited). After OK the
remaining copies go to the Recycle Bin although no identical keeper exists any more. Data stays recoverable in the bin, but
without Undo, and the user believes a good copy remains.

## Fix (intentional DIVERGENCE D-07f-5)
- The hash job records, per hashed file, `(size, last write, "size|hash")` before reading it (`HashSlot.snapshot`).
- After the dialog (and in the no-dialog path), `verify_content_groups` re-stats EVERY file of each content group: the copies to
  remove and the keeper(s) that stay. If any differs from the record, or is gone, the whole group is left alone. A file with
  no record is left alone too.
- Skipped files are reported as `<name>: The source or destination has changed.` (`core.undo.sourceOrDestinationChanged`, an
  existing localized text, no new key). If nothing is left: status BatchCanceled + one error dialog; otherwise the lines are
  added to the batch failures shown in the final summary.
- Files: `crates/pr-app/src/coordinators/duplicate_cleanup.rs`.

## Tests (`crates/pr-app/tests/duplicate_cleanup.rs`)
`batch_recycle_skips_group_whose_keeper_vanished_during_review`, `..._keeper_was_replaced_during_review`,
`..._skips_group_when_a_file_to_remove_was_modified_during_review`. Run before the fix: 3 failed (the Recycle was submitted);
after: `cargo test -p pr-app --test duplicate_cleanup -j 2` 45 passed. `cargo clippy -p pr-app --all-targets --locked -- -D warnings`
clean, fmt applied. Not run: workspace tests, mutation (CI).

## Undo (not done, follow-up)
Production recycles go through `FileActionServiceExecutor` -> `FileActionService`, so every file IS journaled (and recoverable
from the Recycle Bin). What is missing is registering the batch in the Undo stack (single `UndoEntry` per file vs one group),
which is the RV-D5 decision and a UI/undo-stack design change. Left for a separate lane.

## Limits
Size + last write time is the fingerprint (same as the journal's); an edit that keeps both is not detected. Hash is not
recomputed (cost).
