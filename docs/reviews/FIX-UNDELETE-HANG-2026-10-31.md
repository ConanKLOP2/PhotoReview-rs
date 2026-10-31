# FIX-UNDELETE-HANG (2026-10-31)

Source: ST-WIN32 (PR #443) finding M3. Related, NOT fixed here: ST-INVMAP (PR #441) M2.

## 1. Findings confirmed in code

| Question | C# (`7eaee2a2`) | Rust (before) |
|---|---|---|
| Owner window | `CmInvokeCommandInfo.Window` = 0 (`ComRecycleVerbs.InvokeCanonical`, `RecycleBinShell.cs` ~290-305) | `hwnd: Default::default()` (`recycle_bin_shell.rs` `invoke_restore`) |
| `CMIC_MASK_FLAG_NO_UI` | `Mask` not set | `fMask` not set |
| `nShow` | `ShowNormal` (1) | 1 |
| Wait for the Shell thread | `ShellStaRunner.Run`: `thread.Join()` with no timeout (`ShellCom.cs` ~134-150) | `JoinHandle::join()` with no timeout (`recycle_restore.rs` `restore_on_worker`) |

So the Shell call itself is equivalent: a hidden dialog (original folder gone, name conflict) can stall the Shell thread in both ports.

Caller thread (answer): `try_restore` is never called on the UI thread, in either port.
- C#: `UndoService` wraps it in `Task.Run` (lines ~364 and ~653) - thread pool.
- Rust: `UndoService::undo_last` runs as a job on `WorkerLane::FileAction` (`pr-app/src/coordinators/file_actions/undo.rs` ~100); `recovery_retry.rs:631` is also worker-side.

## 2. Is Rust worse than C#? Yes, in one respect

A hung restore stalls the file-action lane in both ports. In Rust the lane job additionally holds the `Mutex<UndoService>` for the whole call (`undo.rs` ~102). The UI thread locks the same mutex in `FileActionController::undo_last` (~74) and in `with_undo(...register_group...)` (`actions.rs` ~533, 942, 1011, 1081). The C# `UndoService` has no such lock (UI-thread-only by convention). So a hidden Shell dialog freezes the Rust UI on the next delete or Ctrl+Z, but not the C# UI.

## 3. Change (smallest)

`crates/pr-platform-win/src/recycle_restore.rs`:
- The worker sends its `io::Result<bool>` over an `mpsc` channel; the caller uses `recv_timeout(SHELL_RESTORE_TIMEOUT = 120 s)` instead of `join()`.
- Timeout returns `Err(TimedOut, "Recycle Bin restore has not completed ...")`, never `Ok(true)` and never a claim of restore. The undo flow already treats an `Err` from the bin as "the proof decides" (P-13): the file is not at the source, so the undo fails truthfully and stays retryable.
- Worker panic still maps to an error (channel disconnected).
- Seam: `with_restore_timeout(Duration)` (tests use 50 ms). Behaviour with a responsive Shell is unchanged.

Not changed (deliberately): `NO_UI`/owner window. `NO_UI` would turn a legitimate conflict prompt into a silent failure, a product decision beyond "fix hang"; kept equivalent to C#.

Residual risk: the stuck worker is detached and cannot be cancelled. If the user later answers the hidden dialog, the Shell may restore the file after the app reported failure. Effect: a retry sees "target exists" (undo checks the source path first) and nothing is lost or overwritten. The 120 s bound is generous so a user answering a real dialog is rarely cut off. A late send on the closed channel is ignored.

## 4. Tests (fakes only, no Shell, no Recycle Bin)

`crates/pr-platform-win/tests/recycle_restore_timeout.rs`: stuck worker -> `TimedOut` error; fast worker -> result passed through; panicking worker -> "panicked" error.

## 5. ST-INVMAP M2 (recorded only)

`recycle_bin.rs:63` permanent delete uses `std::fs::remove_file`, which removes read-only files; C# `File.Delete` refuses them. Awaiting a product decision; untouched.

## 6. PARITY

No row should change status. If a row covers `WindowsRecycleBin` restore, add a note: "DIVERGENCE: bounded 120 s wait for the Shell worker (C# unbounded)".
