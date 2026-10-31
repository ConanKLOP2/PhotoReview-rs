# FIX-RECYCLE-C2 - Recycle: report what the Shell did (2026-10-31)

Status: done in code with fakes; the real-Shell behaviour is NOT verified (attended native test exists as an `#[ignore]` skeleton, not run).
Source: finding C2 of [ST-COM-RECYCLE](ST-COM-RECYCLE-2026-10-31.md), design in [FIX-RECYCLE-C1C2](FIX-RECYCLE-C1C2-2026-10-31.md). Decision `P-61`, [ADR DV-22](../adr/ADR-D-2026-10-31-recycle-c2-shell-outcome.md).

## What was done

- `IFileOperationProgressSink` implemented (`delete_sink` in `crates/pr-platform-win/src/recycle_bin.rs`, `#[implement]`, all 12 methods, `PostDeleteItem` records). Registered with `Advise` and passed to `DeleteItem`; `Unadvise` after `PerformOperations`.
- New pure `classify_post_delete` and `merge_outcome`, enum `ShellRecycleOutcome {MovedToRecycleBin, DeletedPermanently, Failed(hr), Unknown}` returned by the `shell_recycle` seam (signature change of `with_adapters`).
- `send_to_recycle_bin` fail closed: only `MovedToRecycleBin` with the source gone is Ok. `DeletedPermanently` returns an error saying the Shell deleted the file permanently (plus a log warning); `Failed(hr)` carries the HRESULT; `Unknown` is an error. If the source still exists, the old "left the source in place" error is kept for `MovedToRecycleBin`, `Failed` and `Unknown`.
- Paths over 259 UTF-16 units are refused (`InvalidInput`) before the size read and the Shell call.
- unsafe: only in `pr-platform-win`, each block with `// SAFETY:`; `unsafe-audit.sh`: OK, no new violation (baseline untouched).

## Tests (run on this machine, no Shell delete, no Recycle Bin)

- `recycle_delete_sink` 8 passed: classify (3 reports, success HRESULT), merge worst, empty slot, the sink driven through its COM interface for moved / permanent / failed / twice, other callbacks return S_OK. `IShellItem`s are parsed from a `TempRoot` folder only.
- `recycle_bin_facade` 32 passed, 1 ignored: the old tests with `Ok(MovedToRecycleBin)` plus 12 new (4 outcomes, source-present variants, HRESULT text, seam error, 260 refused, 259 accepted, UTF-16 counting). `recycle_bin_readonly_delete` 2 passed.
- Native: `native_attended_recycle_of_own_temp_file_reports_moved_to_recycle_bin` (`#[ignore = "attended: needs a human watching, sends a temp file to the real Recycle Bin"]`), NOT run.
- `cargo check -p pr-platform-win`, `cargo fmt -p pr-platform-win`, one `cargo clippy -p pr-platform-win --all-targets -D warnings`: clean. No mutation run (Recycle Bin rule; COM glue is skipped by AGENTS.md).

## Not done / open

- Whether the real Shell fills `psiNewlyCreated` for a recycle and calls the sink at all is an assumption. If it does not, every recycle reports `Unknown` and fails (fail closed, file is in the bin, nothing lost). The owner must run the attended test once before release; otherwise switch `Unknown` handling (option B of P-61).
- `TSF_DELETE_RECYCLE_IF_POSSIBLE` is not checked in the sink. `FOF_NOERRORUI` (design step 5) not changed. L6 (cause of failure) is only improved through the HRESULT text.
- A file the Shell already deleted permanently cannot be restored; the change makes the report truthful, it does not prevent the loss for cases the guard cannot predict.

## PARITY rows (orchestrator)

Row P-03 stays Partial: add "C2: sink + `ShellRecycleOutcome` + 259-unit limit done with fakes (FIX-RECYCLE-C2); native confirmation pending". Do not raise to Done.
