# Exe walkthrough 3, journal recovery, undo after restart, drag and drop, compare, input (2026-10-29, Machine A)

Status: evidence report. Third real-exe pass (debug build of branch `claude/exe-walkthrough-3`, base `3a467801` plus a merge of `origin/claude/zoom-detail-wire`, PR #338), driven like the first two passes
([EXE-WALKTHROUGH-2026-10-29.md](EXE-WALKTHROUGH-2026-10-29.md), [EXE-WALKTHROUGH-2-2026-10-29.md](EXE-WALKTHROUGH-2-2026-10-29.md)): `keybd_event`, `SetCursorPos` + `mouse_event`, `PrintWindow`, `EnumWindows`.
The C# baseline `7eaee2a2` was read with `git show` only (`JournalStartupRecovery.cs`, `OperationJournal.cs`, `RecoveryFileCheck.cs`, `ShortcutRouter.cs`, `DragDropInputService.cs`, `ShellMainWindow.Viewing.cs`, WPF `MainWindow.xaml.cs`).
Every run set `PHOTOREVIEW_RS_DATA_ROOT` (fresh temp dir), `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, a unique `PHOTOREVIEW_RS_INSTANCE_PREFIX` and `PHOTOREVIEW_RS_WARP=1`. One UI session at a time; only PIDs started by the run were stopped.
Photos were generated with System.Drawing (800x600 JPEGs with a big label, one 4000x3000 striped JPEG for the pan test). Journal scenarios were crafted by writing `operations.jsonl` of a fresh data root while the exe was closed.

Real-config check (the Rust app's own `%LOCALAPPDATA%\PhotoReviewRs`), before the first run and after the last run, identical:

| File | LastWriteTime | SHA-256 |
|------|---------------|---------|
| `config.json` | 2026-10-29T10:50:07.1654780+07:00 | `DF7BFB4D0D924A75E142CB1BCC835FAAFA018F536FE41FD2C00F1D31F7B5DC6D` |
| `window-placement.json` | 2026-10-29T10:50:56.8116890+07:00 | `E09243DB2E3DB3E9F53BCFC47C80A4065E02ECFD2115B81E73035AB3F8002268` |

`%LOCALAPPDATA%\PhotoReview` (the C# app) was not touched. `Delete` was never pressed and nothing went to the Recycle Bin (a pending `Recycle` journal line was only reconciled, never replayed).

## Result table

| # | Item | Result | Observation | Screenshot (docs/reviews/img/) |
|---|------|--------|-------------|------------|
| 1a | Prepared Move, source still there, startup | PASS | Startup reconcile appends `Failed` + `ErrorCode PendingUnconfirmed` ("Could not confirm the pending operation; it will not be replayed automatically."), then a system task dialog "Interrupted file operations": "1 file operation was interrupted last time and could not be confirmed. Nothing was replayed or deleted. Open Recovery to review it now?" (Yes/No, same wording as `dialog.startupRecoveryFailed.message.one`; C# `Win32DialogService.ShowConfirmation` is a TaskDialog too, so it is not dark). No file moved. | `exe-walkthrough3-recovery_prompt.png` |
| 1a-No | Dismiss (No) | PASS | Window stays on the photo, files unchanged, journal = Prepared + Failed. A restart shows no second prompt (C# only prompts for the entries the reconcile just failed). | none |
| 1a-Yes | Open Recovery (Yes) | PASS | The dark Recovery window opens at once ("1 pending/failed operation", row `p1.jpg Failed · Move`, badge "Can retry"). Detail: "The source is unchanged and nothing exists at the destination, so the operation can be retried safely." | `exe-walkthrough3-recovery_year3626_before.png` |
| 1a-time | Recovery detail times | FAIL before fix, PASS after | "modified 10/29/3626 12:30 PM": every time was 1600 years late (D4). After the fix it reads `10/29/2026 12:45 PM`, the same as the local file time. | `exe-walkthrough3-recovery_year3626_before.png`, `exe-walkthrough3-recovery_year_after.png` |
| 1a-Retry | Retry Move/Copy from Recovery | PASS | "Confirm retry: Retry Move for p1.jpg?" Yes: the file moved into `Group-2`, journal gets Prepared + Committed, "Retry succeeded", the Recovery window closes (C# `notice.CloseWindow`). | none |
| 1b | Prepared Move whose file was already moved | PASS | Reconcile appends `Committed` (destination matches size and write time, source gone); no dialog; no `app.log`. Also hit for real by item 6 (kill right after Enter). | none |
| 1c | Torn last line, then a real Move | PASS | The app starts; the half line (`{"Id":"torn0001","Type":"Move","State":"Prep`, no newline) is closed with CRLF before the next append (F-13, C# `TailLacksNewline`), the new Prepared and Committed lines follow on their own lines, the file moved, no crash, no `app.log`. | none |
| 1d | Failed entry only | PASS | No prompt, journal untouched, the entry is listed in Recovery (see 1a for the window). | none |
| 1e | 0-byte journal, journal of two blank lines, UTF-8 BOM + unknown `Type` `Teleport` + unknown `State` + non-JSON + `{"Id":"x"}` + `[1,2,3]` + a Committed line | PASS | The app starts in all cases, no dialog (unknown Type or State never invents a pending move, as `JournalLineParser`), journal bytes unchanged, no `app.log`. | none |
| 1e+ | Pending Recycle (source exists), pending Copy (destination missing), pending Move with an invalid path (`C:\<>\|?*\x.jpg`) | PASS | Three lines reconciled to `Failed` (`SourceStillExistsAfterRecovery`, `PendingUnconfirmed`, `PendingUnconfirmed`), plural prompt "3 file operations were interrupted ...", nothing deleted or copied, the `<` and `>` are written as `\u003C` `\u003E` like System.Text.Json; no `app.log`. | `exe-walkthrough3-recovery_prompt_plural.png` |
| 2 | Undo after restart | PASS | Move, close, reopen the folder, `Ctrl+Z`: status "There is no recent Move/Delete to undo." and nothing moves back. Same as C# decision P03 (undo is single level and in memory; `JournalStartupRecovery` does not seed the undo stack). | `exe-walkthrough3-undo_after_restart.png` |
| 3a | Real OLE drag of a jpg onto the window | PASS | A WinForms drag source in a second process (`DoDragDrop` with a FileDrop list) dragged with `mouse_event` onto the window: drop effect Copy, folder `B` opens on the dropped file (`2/2 · p2.jpg`). | `exe-walkthrough3-drop_file.png` |
| 3b | Folder drop | PASS | Folder `C` opens (`1/2`). Two folders at once open the first (C# `OpenPathAsync` takes one path). | none |
| 3c | Non-image drop | PASS | `note.txt`: status "No supported images found." (`core.dragDrop.noSupportedImages`), the current folder stays open. A mixed drop of two jpg from different folders plus a txt opens the first image's folder with no extra message (C# passes only one path to `OpenPathAsync`, so its mixed-folder warning is not reachable either). | none |
| 4a | Compare: click to select the right or left cell | FAIL before fix, PASS after | Before: clicking the right cell did nothing, the green selection stayed on the left, and `Enter` (Move to Group-2) moved the LEFT file (D5). After: the click moves the green border (left or right), and `Enter` moves exactly the selected file (`img (1).jpg`, the right one). | `exe-walkthrough3-compare_click_before.png`, `exe-walkthrough3-compare_click_after.png` |
| 4b | Compare: undo, `C`, `Escape`, 1 photo | PASS | `Ctrl+Z` brings the moved file back and the pair shows again with the right cell selected; `C` leaves compare (single image), `C` re-enters; `Escape` CLOSES the app (C# router rule 2: Escape is Fullscreen-exit or Close, there is no "leave compare" key). A one-photo folder: `C`, Right, Left do nothing, `Ctrl+Z` says nothing to undo, the app stays responsive, no `app.log`. | none |
| 5a | `Shift+M` / `Shift+Y` | PASS | With `MoveCopyReuseLastFolder` on and last folders set, `M` moves and `Y` copies at once into the remembered folder (status "Copied p2.jpg to ...\Dest2"); `Shift+M` / `Shift+Y` open "Choose a folder to move/copy the photo to" instead; `Esc` cancels and nothing moves. | `exe-walkthrough3-shift_m_picker.png` |
| 5b | Kinetic pan | PASS | With `KineticPanEnabled` true (default false, as C#) a fast left flick of 300 px released while moving keeps gliding: stripe edge at x = 700 (start) -> 350 (60 ms after release) -> 204 (460 ms) -> 163 (2.5 s), decelerating. With the setting off the picture stops at the release point (400 stays 400). | none (numbers from pixel samples) |
| 5c | Middle click, mouse back/forward | PASS | Middle click on the image (default `ActualSize`) switches Fit to 100 %. XButton1 and XButton2 do nothing: C# maps no back/forward button (`PointerButton.XButton1/2` exist only in the enum); index, files and status unchanged. | `exe-walkthrough3-middle_click_100.png` |
| 6 | Force-kill right after `Enter` (Move), restart | PASS | Four runs (kill 0, 10, 30, 80 ms after the key). Run 0 hit the real crash window: file already in `Group-2`, journal with only the `Prepared` line. Restart: reconcile appended `Committed`, no prompt, no `app.log`, nothing lost. The other three had both lines already. A moved file is never missing from both places. | none |

## Defects found

| Id | Defect | Root cause | Status |
|----|--------|------------|--------|
| D4 | Recovery window shows every time 1600 years late (`10/29/3626`). | `WindowsRecoveryTimeFormatter::format_local_general` (`crates/pr-platform-win/src/shell_startup/locale.rs`) passed `DateTimeUtc::ticks()` (.NET ticks since 0001-01-01) to `FileTimeToSystemTime`, which counts from 1601-01-01. The existing test only asserted that the text contained `"20"`, which `3620` also satisfies. | Fixed, commit `5f2b107d`: subtract `FILETIME_EPOCH_TICKS` (504 911 232 000 000 000); before 1601 the text is empty. Tests `shell_startup_locale::locale_recovery_time_formatter_uses_the_dotnet_tick_epoch_not_the_filetime_epoch` and `..._returns_empty_before_the_filetime_epoch` (both fail before, pass after). Re-run in the exe. |
| D5 | In compare mode a left click on a cell does not select it: the status line says "click to select" but nothing happens, and the file actions keep targeting the left file. | The Win32 shell baseline left it open ("WP-22 nối chuột vào đây"): `TrySelectCompareAt` / `MainViewModel::select_compare_*` exist and are tested, but `handle_pointer_down_impl` in `crates/pr-platform-win/src/shell_input/dispatch.rs` returned at once while the compare view was visible. The WPF window selects on `CompareLeft_Click` / `CompareRight_Click`. | Fixed, commits `3f1e7b27` and `3fc36669` (rustfmt): `ShellInputHost::select_compare_side` (default no-op, so fakes still compile), called by the press path after `compare_layout::hit_test`; the press is still not consumed (the existing assertion `compare_visible_takes_press_and_wheel_away_from_the_image` is unchanged). Tests `shell_input_dispatch_pointer::compare_visible_left_press_on_a_cell_selects_that_cell` and `..._press_outside_a_cell_or_with_another_button_selects_nothing`. Re-run in the exe (item 4a). |

Defects left open: none found. Observations that need a decision, not fixed:

- After a successful Retry from the Recovery window the main window kept showing the old listing (`2/3`, the moved file still counted). I did not check what C# does here (it refreshes through the file-system watcher), so this is NOT classed as a defect.
- The Recovery header reads "Checked 1 entries at 12:45 PM." (`recovery.*` catalog text, same as C#; only a plural nit).

## Not checked

- `Delete` / Recycle in any form (only a pending `Recycle` journal line was reconciled), "Remove (1) copies", "Remove originals".
- Journal compaction after a large journal (the test journals are tiny), the live-operation marker skip (Q-R27, needs a second process mid-operation), capture-group (RAW+JPEG) journal lines and the group repair path, Recovery "Clear selected" / "Clear all" / dismiss confirmations, Retry of a Copy, retry refusals (`SourceChanged`, `Conflict`).
- A genuine long cross-volume Move interrupted mid-copy (partial destination cleanup), a journal on a read-only or locked file.
- Drop of a RAW file, a drop onto a modal dialog, a drop while a folder loads; real touchpad gestures; multiple monitors and other DPI; keyboard Enter/Space on a compare cell (the Win32 shell has no focusable cell).
- Mutation testing: NOT run. The two fixes are a Win32 formatter (FFI glue) and a pointer-dispatch forward; no journal or recovery decision logic changed.

## Harness notes (not product defects)

- The dev exe was run from `target\debug` after `tools\dev\stage-native.ps1`. Language catalogs were not copied next to it (not needed here).
- The system task dialogs (startup prompt, "Confirm retry", "Retry succeeded") are separate top-level `#32770` windows: capture them by their own handle. The context-menu popup is class `#32768`.
- The drag source is a small WinForms process (`DoDragDrop`, `FileDrop` data object) started with `-STA`; the drag is driven with `SetCursorPos` steps and `mouse_event`, so the drop goes through the real OLE `IDropTarget` of the app.
- A `Get-Process` object for a process another call started has no `ExitCode`; exit was judged from the window and `HasExited`.
- Gates for the two fixes: `cargo fmt -p pr-platform-win --check`, `cargo check --workspace --locked`, `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings` and `cargo test -p pr-platform-win --locked` (179 test binaries ok, 0 failed; ignored native tests not run) all green.
