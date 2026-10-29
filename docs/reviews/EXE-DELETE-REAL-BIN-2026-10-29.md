# Exe delete to the real Recycle Bin (2026-10-29, Machine A)

Status: evidence report. Real-exe pass of Delete -> Recycle Bin -> Undo (debug build of branch `claude/delete-real-bin`, master `55ee8bb1`), driven with `keybd_event` and `PrintWindow` like [EXE-WALKTHROUGH-4-2026-10-29.md](EXE-WALKTHROUGH-4-2026-10-29.md).
C# baseline `7eaee2a2` read with `git show` only (`ShellFileRecycler.cs`, `FileActionController.cs`); Rust: `crates/pr-platform-win/src/recycle_bin.rs`, `crates/pr-app/src/coordinators/file_actions/{recycle,undo}.rs`.

## Scope of the approval

The owner allowed ONE temporary JPEG created by the run to be sent to the REAL Recycle Bin with Delete in the exe and the SAME item to be restored. Nothing else of the bin was listed beyond read-only `Shell.Application` enumeration, deleted, restored or emptied; no bulk action, no `SHEmptyRecycleBin`, no ignored native bin test. Every run set `PHOTOREVIEW_RS_DATA_ROOT` (fresh temp dir), `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, a unique `PHOTOREVIEW_RS_INSTANCE_PREFIX` and `PHOTOREVIEW_RS_WARP=1`. The real `%LOCALAPPDATA%\PhotoReviewRs` had neither `config.json` nor `window-placement.json` before and after all runs (checked again at the end: both absent); `%LOCALAPPDATA%\PhotoReview` (C#) not touched.

Deviation to report: in step 6b (first attempt) the exe was started with the locked file as its argument; the exe skipped the unreadable file ("Skipped 1 unreadable file") and the Delete key recycled `d4.jpg`, another temp JPEG of the same run. That item (name `d4.jpg`, deleted-from path inside this run's temp folder, the only non-`Downloads\Compressed` item) was identified unambiguously (exactly one match) and restored with the `undelete` verb on that item only. Net effect on the bin: none. The same applies to step 6a: a read-only JPEG went to the bin and was restored the same way (my first restore check was a harness bug, `.Count` of a single object, so it ran one step later, same single item).

## Recycle Bin before and after

Listing read-only with `(New-Object -ComObject Shell.Application).NameSpace(10).Items()` (name and `System.Recycle.DeletedFrom`).

| | Count | Items |
|---|---|---|
| BEFORE the first run | 36 | all 36 from `C:\Users\adminvti\Downloads\Compressed`: `Dptf.dll`, `esif_uf.exe`, `DptfPolicy*.dll`, `dptf_*.sys/.cat/.inf`, `upe_*.dll`, `Install.cmd`, `DIFxAPI.dll`, `DPTF_Hiding.BAT`, `Drv64.exe`, `pnputil.exe`, `dsp.dv`, `dptf_helper.exe`, `esif_*.dll/.sys`, `DPTF_Intel_DCH_W10_64_V85101037263.zip`, `upe_wwan.dll`, ... |
| AFTER the last run | 36 | identical |
| Diff (sorted names + `DeletedFrom`) | 0 | empty |

No bin item with the unique name existed before; after each run the bin held no item of the run. No other item was touched.

## Results

Fixture per run: `pics\` with `a1.jpg`, `prs-del-<guid>.jpg` (the item under test), `c3.jpg`, `d4.jpg` (generated 320..344 x 240 JPEGs). The unique name sorts last, so the exe opens it as 4/4.

| # | Step | Result | Observation | Screenshot (docs/reviews/img/) |
|---|------|--------|-------------|------------|
| 1 | Bin listing before | PASS | 36 items, none with the unique name. | none |
| 2 | Delete | PASS | File gone from the folder (`a1, c3, d4` left), exe moved to the neighbour `d4.jpg` (`3/3`). Journal: one `Recycle` entry, `Prepared` then `Committed` (same Id, `Source` = the real path, Size 6480, LastWriteUtc kept; 0.25 s apart). Bin: 37 items, exactly one new, name `prs-del-7a8e...97c4.jpg`, `DeletedFrom` = the run's `pics` folder. | `exe-delete-del_2_after_delete.png` |
| 3 | Ctrl+Z in the exe | PASS | File back at the original path, SHA-256 identical (`C8FF7C0ADACC273BADBA777BC14A533476DFC8D783FC10B6F4401CE279C56AAC` before delete and after undo); bin back to 36; the journal got no extra line (a single Recycle undo is not journaled, `undo.rs` doc: "Single Recycle: not journaled"; same as C#). The photo goes back to its remembered review position and the current photo stays on screen (C# rule). Repeated in a second run (hash `8B2A2AE6...C0F6` equal, 8 s wait, `Left` twice showed `1/4 a1.jpg`, so the catalog has 4 entries). | `exe-delete-del3b_2_undone.png` |
| 3-note | Overlay counter after undo | Observation (not a defect) | The info line still reads `3/3 . 3.66 KB . 344x240 . d4.jpg` right after the undo although the catalog has 4 entries; it becomes `1/4` on the next navigation. The text is the presenter's `PhotoInfo` built at present time; C# builds it in `ImagePresenter.PhotoInfo.From(...)` at present time too and its Recycle undo presents only when the catalog was empty (`FileActionController.cs` UndoLastAsync), so this is the C# behaviour. Not changed. | same |
| 4 | Restore by Shell verb if the in-app undo fails | not needed | The in-app undo worked. The verb was used only in steps 5, 6a, 6b (first attempt) and 7, each time after printing the item name and checking that exactly one item matched. | none |
| 5 | Delete, exit the exe, restore with the verb | PASS | After Delete: 37 items, one with the unique name. The exe was closed with `CloseMainWindow` (exit code 0). Item restored with `InvokeVerb('undelete')` on that single item (printed: `prs-del-d2e4...932a.jpg` from the run's `pics` folder). Afterwards: folder `a1, c3, d4, prs-del-...jpg`, SHA-256 of all 4 files identical to the originals, bin 36 with identical names. | none |
| 6a | Read-only attribute file | PASS (parity) | The Shell recycles a read-only file without error (same flags as C#: `FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOCONFIRMMKDIR | FOF_SILENT | FOFX_RECYCLEONDELETE`, no `FOF_NOERRORUI`). Journal Prepared + Committed, one item in the bin, restored with the verb (the read-only attribute came back with it, attribute cleared afterwards by the run). Nothing lost. | none |
| 6b | File locked by another process (`FileShare.None`, own PowerShell process) | PASS | Lock taken after the exe displayed the file (lock verified: opening gives "being used by another process"). Delete: the Shell shows its own "File In Use" dialog (Try Again / Cancel) with the exe's recycle running on the file-action worker. `Escape` (Cancel) then gives the app's "Delete failed" dialog ("... was NOT deleted and is still in the list ... Reason: IFileOperation failed (0x80270000)"); after OK the status line reads "Could not run Move to Recycle Bin: IFileOperation failed ...". Journal: `Prepared` then `Failed` (`Error` with the HRESULT). File still on disk (hash identical), bin unchanged (36), and the entry is back in the list (Right x2 reaches it; it shows "Image error: ... being used by another process (os error 32)" because the lock is still held). Same as C#: no `FOF_NOERRORUI` ("errors still show a dialog like the VB version", `ShellFileRecycler.cs` line 20). A first lock attempt before the exe started: the exe lists the file as unreadable and skips it ("Skipped 1 unreadable file"), see the deviation above. | `exe-delete-del6d_dialog.png`, `exe-delete-del6e_failed_dialog.png`, `exe-delete-del6e_after_dismiss.png` |
| 7 | Kill the exe right after Delete (own PID only), restart | PASS | Three runs, kill after the `Prepared` line appeared (+0, +90, +125 ms): file still at the source, bin unchanged; restart with the same data root: dialog "Interrupted file operations" ("1 file operation was interrupted last time and could not be confirmed. Nothing was replayed or deleted. Open Recovery to review it now?", same text as C# `dialog.startupRecoveryFailed`), journal gets `Failed` with `ErrorCode: SourceStillExistsAfterRecovery` (C# `FileActionService.cs` `JournalErrors.SourceStillExistsAfterRecovery`). One run (+150 ms) was late enough for the `Committed` line and the item to be in the bin: nothing to reconcile, no dialog, the item restored with the verb. The in-between state (item in the bin, only `Prepared` written) is a window of a few ms that could not be hit; its handling is covered by the reconcile rules (`journal_reconcile`) and was not reproduced here. No data lost in any run; after every run the folder hash set and the bin were back to the start. | `exe-delete-del7_restart_dialog.png` |

## Defects

None found. No production code changed, so no new test or mutation run (data-safety tier 2 applies only to changed pure-logic functions).

## Not checked

- Delete of a file on a drive without a Recycle Bin (permanent-delete prompt), Recycle of a capture group, bin full / disabled, network paths.
- The window in which the item is in the bin and the journal holds only `Prepared` (see step 7).
- Delete via the Duplicate Cleanup window (bulk) was deliberately not run.

## Cleanup

Temp folders `prsdel_*` of the runs were removed; the bin holds exactly the 36 items it had at the start.
