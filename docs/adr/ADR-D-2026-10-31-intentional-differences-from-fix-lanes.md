# ADR: intentional differences introduced by the 2026-10-31 fix lanes (DV-1..DV-12)

Date: 2026-10-31

Status: accepted under the standing "run end to end" instruction (proposal row `P-58` in `docs/PROPOSED-DECISIONS.md`); every item is merged on master and acted on. The owner may reverse any item with a follow-up PR. DV-11 was decided by the user in chat (Q2, `P-57`).

Why one file: all twelve items come from the same review wave (`ST-INVMAP`, `ST-WIN32`, `ST-APP-RACES`, `ST-IMAGING-INV`, `ST-PATHLOSSY`), are small, and share the same reason. AGENTS.md rule: an intentional behaviour difference needs an ADR in `docs/adr/` and a `Different`/DIVERGENCE note on its PARITY row. Each PARITY row below links here by item number. C# baseline `7eaee2a2`, read only.

Common reason: the C# code has the same hole in each case (the C# behaviour is the oracle and the oracle is wrong or unsafe for user photos); the Rust fix is strictly safer and does not change any file, config or journal format. Where a golden oracle trace exists, the comparison is adjusted by a named, narrow filter (`divergence_adjusted`), never by editing the golden.

## DV-1 Group retry re-stats before `delete_permanently` (PR #448, FIX-H1)

- Context: `RecoveryRetryService.ExecuteGroupRetry` pre-checks the members, writes Prepared, then deletes. A file replaced in between is deleted for good (INV-4, INV-6).
- C#: `src/PhotoReview.Core/FileActions/RecoveryRetryService.cs:197` (`ExecuteGroupRetry`), `:225` `_recycleBin.DeletePermanently(member.Source)` with no stat after Prepared.
- Rust: `crates/pr-core/src/file_actions/recovery_retry.rs` `run_group_members`: `stat` plus `same_fingerprint` right before `delete_permanently`; a missing or changed file fails the transaction with `core_recovery_source_changed` (same error as the Move/Copy branch).
- Reason: data safety; an unrecoverable delete must hit the journaled file.
- Remaining risk: size plus last write time is the fingerprint, so an edit that keeps both is not seen; the window between stat and delete is not zero.
- Tests: `st_invmap_repro` (red before), `recovery_retry` 100; golden `recovery_retry_golden::divergence_adjusted` drops only the extra `stat P` before `bin.delete-permanently P`. Report: `docs/reviews/FIX-H1-2026-10-31.md`.

## DV-2 Single Recycle and group-retry Recycle re-stat before the mutation (PR #463, ST-INVMAP L3 and H1-follow-up)

- Context: same race as DV-1 for the plain Recycle path and for the non-permanent member of a group retry; with `permanent` the Recycle becomes a permanent delete when the volume has no bin.
- C#: `FileActionService.cs:163-170` (Recycle branch decides `permanent` from `CanRecycle`, no stat after Prepared); retry `RecoveryRetryService.cs:197-225`.
- Rust: `crates/pr-core/src/file_actions/service.rs` Recycle branch: after `tx.begin()` and `hand_off` stat again; vanished file gives `source_missing`, other size or mtime gives `VERIFY_SIZE_CHANGED`, disk untouched, journal Failed. `recovery_retry.rs` `send_to_recycle_bin` branch: same `same_fingerprint` check. `divergence_adjusted` also drops a `stat P` placed directly before `bin.recycle P`.
- Reason: INV-4 and INV-6 close for these paths; the cost is one `stat`.
- Remaining risk: as DV-1. L1 (`end()` releases unconditionally) is deliberately kept as C#: every caller goes through RAII guards (`GateGuard`/`GateHold`). L4 (settings and action-list export use `std::fs::write`, not atomic) is deliberately kept as C# `File.WriteAllText`; making it atomic would need its own `Different` ADR (parent folder must exist, error texts must stay).
- Tests: `file_actions_service::recycle_permanent_source_replaced_after_prepared_is_refused`, `recycle_source_replaced_after_prepared_is_not_recycled`, `st_invmap_repro::retry_group_recycle_member_replaced_between_precheck_and_mutation_is_not_recycled`. Report: `docs/reviews/FIX-INVMAP-LOW-2026-10-31.md`.

## DV-3 Restore from the Recycle Bin waits at most 120 s (PR #454, ST-WIN32 M3)

- Context: the Shell `undelete` verb may show a hidden modal (original folder gone, name clash) and stall.
- C#: `src/PhotoReview.Platform.Windows/Shell/ShellCom.cs:131-146` `ShellStaRunner.Run` ends with `thread.Join()`, no timeout.
- Rust: `crates/pr-platform-win/src/recycle_restore.rs` `restore_on_worker` sends the result over a channel and the caller uses `recv_timeout(SHELL_RESTORE_TIMEOUT = 120 s)`; timeout returns `Err(TimedOut)`, never `Ok(true)`; the stuck worker is detached.
- Reason: in Rust the stuck call holds `Mutex<UndoService>`, which the UI thread also locks, so a hidden dialog would freeze the UI (C# has no such lock).
- Remaining risk: if the user answers the hidden dialog later the Shell may restore after the app reported failure; a retry then sees "target exists", nothing is overwritten. `NO_UI` was not set on purpose (product decision).
- Test: `recycle_restore_timeout` (fakes: stuck, fast, panicking worker). Report: `docs/reviews/FIX-UNDELETE-HANG-2026-10-31.md`.

## DV-4 A nested `WM_CLOSE` during a `Closing` handler is ignored (PR #456, ST-WIN32 M2)

- Context: a `Closing` handler that itself sends `WM_CLOSE` re-enters the handler chain.
- C#: `src/PhotoReview.Shell.Win32/Hosting/ShellWindow.cs:300` and `:403-406` `RaiseClosing` invokes the multicast event again (re-entrant); `WndProcThunk.cs:79` notes `DefWindowProc(WM_CLOSE)` would destroy the window before `Closing` decided.
- Rust: `host/shell_window/create.rs` field `closing_in_progress: Cell<bool>`; `messages.rs` `raise_closing` returns 0 without destroying for a nested `WM_CLOSE`; the flag is cleared by a `ResetFlag` guard even when the handler panics.
- Reason: no re-entrancy into handlers that are not written for it; the window is never destroyed by the nested message.
- Remaining risk: a handler that relied on a nested close being processed gets nothing; none exists in the port.
- Test: `host_shell_window_fixes::nested_wm_close_during_a_closing_handler_does_not_destroy_the_window` (native `#[ignore]`; red before, green after). Related fix M1 (cookie freed at `WM_NCDESTROY`, no use-after-free) has no C# counterpart. Report: `docs/reviews/FIX-SHELLWINDOW-2026-10-31.md`.

## DV-5 A file name that cannot be represented is refused at folder load (PR #455, FIX-PATHLOSSY, F-13)

- Context: NTFS allows unpaired UTF-16 surrogates; `to_string_lossy()` turned them into U+FFFD, so a later Move/Recycle/Undo could hit a sibling really named with U+FFFD.
- C#: `OperationJournal.cs:227` `ThrowIfUnrepresentable` (called at `:251`) refuses with `UnrepresentablePath` when the operation is journaled.
- Rust: `pr_core::file_actions::errors::path_to_journal_string` gives the same code and text; in addition `folder_load/load.rs` does not load such an entry and lists it in the existing `skipped` list (`SkippedKind::FileUnreadable`); `list_visible_directories` leaves such directories out; `pick_and_open_folder` refuses such a folder.
- Reason: the Rust catalog keeps `String` paths, so the refusal must happen before an entry exists; the skipped list is the existing user-visible channel.
- Remaining risk: the photo is not shown at all (C# shows it and fails on first action). Rare in practice.
- Tests: `lossy_source_with_a_decoy_acts_on_the_wrong_file` (ST-PATHLOSSY repro), the FIX-PATHLOSSY load tests. Reports: `FIX-PATHLOSSY-2026-10-31.md`, `ST-PATHLOSSY-2026-10-31.md`.

## DV-6 After an executor panic the catalog restores only entries still on disk (PR #462, ST-INVMAP M3)

- Context: a panic after the file reached the Recycle Bin showed a vanished photo.
- C#: `src/PhotoReview.App.Shared/Coordinators/FileActionController.cs:475` `catch (Exception ex) when (ex is not OperationCanceledException)` restores unconditionally and registers no Undo; healing happens only when the entry is presented.
- Rust: `crates/pr-app/src/coordinators/file_actions/actions.rs` `executor_threw` asks `deps.file_system.file_exists` per path; a single missing file is not restored, a group restores only the existing members. Without the seam the old behaviour stays. No Undo after a panic.
- Reason: do not show a photo that is gone.
- Remaining risk: a transient `file_exists == false` leaves the entry out until the next load; Undo after a panic is still absent (needs journal reconcile into the single Undo slot).
- Tests: `file_action_core::executor_throws_after_the_file_is_gone_entry_does_not_come_back`, `group_executor_throws_after_some_members_are_gone_only_the_existing_ones_come_back`. Report: `FIX-RESTORE-M3-2026-10-31.md`.

## DV-7 Duplicate cleanup re-checks the whole content group after the review dialog (PR #464, D-07f-5)

- Context: while the modal is open the keeper may be deleted or changed.
- C#: `DuplicateCleanupController.cs:184` shows `ShowBatchReview`; afterwards only `_catalog.IndexOf(path) >= 0` (in-memory) is checked; the keeper is not tracked.
- Rust: `crates/pr-app/src/coordinators/duplicate_cleanup.rs` records `(size, last write, "size|hash")` per hashed file and `verify_content_groups` re-stats every file of each group (copies and keeper); any difference or missing file leaves the group alone and reports `<name>: The source or destination has changed.`
- Reason: do not recycle copies when the good copy is gone.
- Remaining risk: fingerprint is size plus mtime; the hash is not recomputed; batch Undo is still absent (RV-D5, as C#).
- Tests: `duplicate_cleanup::batch_recycle_skips_group_whose_keeper_vanished_during_review` and two siblings (45 pass). Report: `FIX-DUPCLEAN-M1-2026-10-31.md`.

## DV-8 A panic in a background job becomes a failure result (PR #473, D-07f-6, ST-APP-RACES H1 M1 M2)

- Context: a Rust worker panic sends no event, so the file-action gate or the load phase is never released.
- C#: `DuplicateCleanupController.cs:92` `RemoveDuplicatesAsync` has `try`/`catch (Exception)`/`finally`; `FolderLoadCoordinator.cs:59` `LoadAsync` catches any exception of its scan task; a faulted `Task` never leaves a caller waiting.
- Rust: `catch_unwind` in the hash job (becomes `HashOutcome::Failed`), the scan job (`ScanFailure::Failed(Io)`) and the `FileHashService` worker (`FileHashError::Io`, not cached). Each job converts the panic into the failure its coordinator already handles; no enum gains a variant.
- Reason: Rust threads die silently; this reproduces the C# catch/finally outcome.
- Remaining risk: presenter, zoom-detail and sibling-overlay jobs (M3) still drop a result on panic (stale image until the next navigation, no held gate); `MainWorkerPort` ignores `WorkerLane` (M5, lanes are labels only, safe today through the gate and `load_id`); the `Mutex<UndoService>` held during `undo_last` is safe only because every UI-thread lock sits inside the gate turn (M4, an invariant that lives in a comment, no test).
- Tests: `a_panicking_hasher_ends_the_run_with_a_failure_and_recycles_nothing`, `a_panic_in_the_scan_job_fails_the_load_instead_of_leaving_it_scanning`, `file_hash_worker_panic::*`. Reports: `FIX-WORKER-PANICS-2026-10-31.md`, `ST-APP-RACES-2026-10-31.md`.

## DV-9 Imaging cache: persist commit gate, bounded preview-file read, saturating folder estimate (PR #474, ST-IMAGING-INV M1 M2 M3)

- M1 (INV-2). C#: `PreviewImageService.cs:304-316` claims the persist path, writes with `CancellationToken.None`, checks the epoch after the write, then `DeleteStalePersistFile` (`:361`). A stale writer that finishes after an epoch bump can replace a newer payload. Rust: `atomic_file::write_atomic_gated` and `preview_file::write_atomically_gated` take a `CommitGate` called right before `rename_over`; `persist_one` passes a gate that locks `owner_epochs`, checks `owner_epoch` and `epoch`, and keeps the lock across the rename; the old post-write delete stays as safety net. I/O under a lock is the R0003 exception, same as `delete_stale_persist_file` (two persist workers, one rename). Test `stale_persist_writer_finishing_after_new_epoch_does_not_replace_newer_payload` (red before).
- M2 (preview file). C# reads the header before renting a buffer sized by `payloadLength` (`<= int.MaxValue`); Rust allocated by file length first. Rust now checks the length first (`check_entry_file_len`, "implausibly large" is `InvalidData`) and allocates with `try_reserve_exact` (`OutOfMemory` keeps the entry). This moves Rust toward C#; a legitimate maximum file can still need about 2 GiB. Tests in `cache/preview_file.rs`.
- M3 (RAM budget). C# `RamBudgetPolicy.cs:153` `EstimateFolderPreviewBytes` uses `checked(width * 2)` and throws; the golden records the throw and stays unchanged. Rust keeps that function (panic) and adds `estimate_folder_preview_bytes_saturating` (overflow gives `i64::MAX`, "do not preload the whole folder"); `FolderEstimateCache::get_or_compute`, the only preload-queue caller, uses it so one absurd RAW entry cannot make every refresh fail. Test `folder_estimate_cache_raw_entry_with_absurd_dimension_means_do_not_preload_instead_of_panicking`.
- Remaining risk: the gate adds a short lock hold during one rename; no scheduler-level test for M3. M4 (RAW, INV-8) is NOT changed, see DV-9 note below.
- M4 note (verified, not fixed, inherited from C#): `libraw_open_wfile` opens without `FILE_SHARE_DELETE` and keeps the handle until close, so renaming the RAW file during a full decode fails with WinError 32 (experiment outside the repo with the pinned `libraw.dll` on a synthetic DNG; rename only, not run through the Rust `LibRawDecoder`). C# uses the same API. Deferred: read the RAW through `open_read_shared` into memory then `open_buffer`, which touches the RAM budget and the "read each file once" rule and needs its own ADR.
- Report: `FIX-IMAGING-CACHE-2026-10-31.md`, `ST-IMAGING-INV-2026-10-31.md`.

## DV-10 Settings "open update page" accepts only http and https (PR #471, ST-WIN32 L11)

- C#: `GeneralSettingsPage.cs:247-249` `DefaultOpenUrl` runs `Process.Start(new ProcessStartInfo(url) { UseShellExecute = true })` for any string.
- Rust: `default_open_url` rejects everything that is not `http://` or `https://` (`is_web_url`) with `Err("unsupported url scheme")` before `ShellExecuteExW`.
- Reason: `ShellExecute` on `file:`, `ms-*:` or a path would start programs; the only caller already validates the update URL.
- Remaining risk: none known; stricter than C#. Embedded NUL behaviour unchanged (as C#).
- Test: `settings_dialog_state::open_url_accepts_only_http_and_https_without_entering_the_shell`. Report: `FIX-WIN32-LOW-2026-10-31.md`.

## DV-11 Permanent delete refuses a read-only file (PR #460, Q2 and P-57)

- Context: `WindowsRecycleBin::new` wired permanent delete to `std::fs::remove_file`, which deletes read-only files on Windows. That was the Rust divergence; C# refuses.
- C#: `WindowsRecycleBin.cs:104-111` `DeletePermanently` calls the file-delete seam (`File.Delete`, `UnauthorizedAccessException` on read-only), then checks `File.Exists`.
- Rust: `recycle_bin::native_file_delete` calls `PhysicalFileSystem::delete` (`DeleteFileW`; not found is success; access denied is `PermissionDenied`). This restores parity; it is recorded here because the user chose it explicitly (Q2) and the previous Rust behaviour was a divergence.
- Remaining risk: a user who wants such a file removed must clear the read-only flag first (same as C#). Failure leaves the journal Prepared line for reconcile like any delete failure.
- Test: `recycle_bin_readonly_delete` (red before). Q1 (journal default `Fast`) is unchanged on purpose. Report: `FIX-READONLY-DELETE-2026-10-31.md`.

## DV-12 The instance-forward server waits for the client to read the reply (PR #471, ST-WIN32 L2)

- Context: `DisconnectNamedPipe` discards bytes the client has not read, so an `ERR` could be lost and the second instance would exit silently (`Unknown` counts as delivered).
- C#: `InstanceForwardServer.cs:134-135` `WriteAsync` then `FlushAsync`, which waits for the client. The first Rust port omitted that wait; this item restores it with a different mechanism.
- Rust: `ForwardPipe::wait_reply_delivered(deadline)` after `write_all`, at most `REPLY_DELIVERY_TIMEOUT = 1 s`, reads to EOF (the client closes after reading) and is cancellable by `stop_event`, unlike `FlushFileBuffers`. `ERROR_BROKEN_PIPE` on a pending read is `Ok(0)` (L5). A bad client delays the next accept by at most 1 s.
- Reason: same observable guarantee as `FlushAsync` without an uncancellable block.
- Tests: `forward_server_native::server_waits_for_reply_delivery_after_writing_ok_and_err_but_not_after_a_timeout` (fake); native `forward_client_native::rejected_request_reaches_the_client_as_rejected_not_unknown` (`#[ignore]`, green; a red run on the old code could not be produced without a deterministic race). Also in this PR: L13 `requeue_front` keeps the actions after a panicking one in the queue (C# leaves them in `_queue`; this is parity). Report: `FIX-WIN32-LOW-2026-10-31.md`.
