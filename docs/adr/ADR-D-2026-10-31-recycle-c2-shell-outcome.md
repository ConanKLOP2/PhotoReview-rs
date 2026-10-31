# ADR: Recycle send reports what the Shell did (DV-22, ST-COM-RECYCLE C2)

Date: 2026-10-31

Status: accepted under the standing "run end to end" instruction (proposal row `P-60` in `docs/PROPOSED-DECISIONS.md`); merged code, native confirmation pending. The owner may reverse it with a follow-up PR.

## Context

`WindowsRecycleBin::send_to_recycle_bin` (`crates/pr-platform-win/src/recycle_bin.rs`) called `IFileOperation::DeleteItem(&item, None)` and then only checked that the source file was gone. That evidence is the same for a recycle and for a permanent delete. Cases the guard cannot predict (a path over `MAX_PATH`, a wrong size, a bin that is full or disabled in a way the settings do not show) make the Shell delete for good while the call returns `Ok` and the journal says Recycle. C# `ShellFileRecycler` has the same hole (C# baseline `7eaee2a2`, read only); the C# behaviour is not the oracle for a safe answer here.

## Decision (DV-22)

- `IFileOperationProgressSink` (`delete_sink::DeleteSink`, `#[implement]`, 12 methods, only `PostDeleteItem` records). Passed to `DeleteItem` (per item) and registered with `Advise` (whole operation); the two reports are merged keeping the worst.
- `classify_post_delete(hr, new_item_present)`: failed HRESULT is `Failed(hr)`; success with a new item is `MovedToRecycleBin`; success without one is `DeletedPermanently`. A sink that was never called leaves `Unknown`.
- The seam `shell_recycle` returns `io::Result<ShellRecycleOutcome>` (a breaking change of `WindowsRecycleBin::with_adapters`; all callers were tests).
- `send_to_recycle_bin` succeeds only for `MovedToRecycleBin` with the source gone. `DeletedPermanently` is an error naming the permanent delete (and a log warning); `Failed` carries the HRESULT, or "left the source in place" when the source survives; `Unknown` is an error (or "left in place").
- Paths longer than 259 UTF-16 units are refused with `InvalidInput` before the size read and before the Shell call.

## Consequences

- The journal and the UI never claim Recycle for a file the Shell reported deleted for good. If the Shell already deleted it, the data is gone: the error says so, it does not undo anything.
- Fail closed on `Unknown`: if the real Shell never calls the sink for a normal recycle, every recycle would fail although the file is in the bin. This was NOT verified against the real Shell (the Recycle Bin rule forbids it in agent lanes). The attended test `recycle_bin_facade::native_attended_recycle_of_own_temp_file_reports_moved_to_recycle_bin` (`#[ignore]`) must be run by the owner once with a human watching; it sends only its own temp file and the human removes that item from the bin by hand.
- Whether `psiNewlyCreated` is non-null for a real recycle, and whether the `TSF_DELETE_RECYCLE_IF_POSSIBLE` flag needs an extra check, is design assumption until that run.

## Tests

`recycle_bin_facade` (four outcomes plus seam error, path length 259/260 and UTF-16 counting, fake seam), `recycle_delete_sink` (the COM sink driven through its interface with items parsed from a temp folder; no Shell delete). PARITY row P-03 stays Partial.
