# Exe walkthrough 2, settings, tool windows, edge folders (2026-10-29, Machine A)

Status: evidence report. Second real-exe pass (debug build of branch `claude/exe-walkthrough-2`, base `5946f0f6` plus a merge of `origin/claude/exe-walkthrough`, the two fixes of PR #332 D1 and D2), driven like the first pass
([EXE-WALKTHROUGH-2026-10-29.md](EXE-WALKTHROUGH-2026-10-29.md)): `keybd_event`, `SetCursorPos` + `mouse_event`, `PrintWindow`, `EnumWindows`. The C# baseline `7eaee2a2` was read with `git show` only.
Every run set `PHOTOREVIEW_RS_DATA_ROOT` (fresh temp dir), `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, a unique `PHOTOREVIEW_RS_INSTANCE_PREFIX` and `PHOTOREVIEW_RS_WARP=1`. One UI session at a time.

Real-config check (the Rust app's own `%LOCALAPPDATA%\PhotoReviewRs`), before the first run and after the last run, identical:

| File | LastWriteTime | SHA-256 |
|------|---------------|---------|
| `config.json` | 2026-10-29T10:50:07.1654780+07:00 | `DF7BFB4D0D924A75E142CB1BCC835FAAFA018F536FE41FD2C00F1D31F7B5DC6D` |
| `window-placement.json` | 2026-10-29T10:50:56.8116890+07:00 | `E09243DB2E3DB3E9F53BCFC47C80A4065E02ECFD2115B81E73035AB3F8002268` |
| `cache\` | file count 13 | 13 |

`%LOCALAPPDATA%\PhotoReview` (the C# app) was not touched. Nothing was deleted through the shell or sent to the Recycle Bin (`Delete`, "Remove (1) copies" and "Remove originals" were not pressed).

Test setup note: a dev build has no `<exe dir>\Languages` folder (the release staging copies `assets/languages` there, `tools/stage-release.ps1`), so the language dropdown listed only `Auto (Windows)` and `English` until the three
catalogs were copied next to the dev exe. That is a setup gap of the dev loop, not a product defect (the shipped layout has the folder).

## Result table

| # | Item | Result | Observation | Screenshot (docs/reviews/img/) |
|---|------|--------|-------------|------------|
| 1a | Settings pages open | PASS | Seven pages, all render: General, Display, Mouse & zoom, Performance, Shortcuts, Files & safety, Diagnostics (buttons: Restore defaults, Reset this page, Export, Import, Cancel, Apply, Save). | `exe-walkthrough2-settings_vi.png` |
| 1b | Language dropdown en to vi and back | FAIL before fix, PASS after | Settings texts switched at once both ways (checked against `assets/languages/vi.json`: Cài đặt, Chung, Hiển thị, Áp dụng, Lưu ...). The main window (toolbar `Fit` to `Vừa khung`) did NOT follow on Apply; it followed only after Save, and every Apply wrote `Unhandled panic RefCell already borrowed at host/main_window/lifecycle.rs:326` to `app.log` (5 ERROR lines in one session). After the D3 fix Apply updates the main window while Settings is open, no `app.log` is created. | `exe-walkthrough2-apply_fixed.png` |
| 1c | Numeric setting persists after restart | PASS | Info overlay font size 17 written to `config.json` of the data root (`"InfoOverlayFontSize": 17`), shown as 17 after restarting the exe. | none |
| 1d | Cancel / Apply / Save | PASS | Cancel discarded an edit (12 stayed, config unchanged); Apply wrote 14 to `config.json` and kept the window open; Save wrote the value and closed the window. | none |
| 1e | Restore defaults | PASS | 14 went back to the default 12 on the page without a confirmation prompt. | none |
| 1f | Export then Import settings | PASS | Export dialog: `WM_SETTEXT` of the path into its Edit + Enter wrote the JSON (`InfoOverlayFontSize: 17`); after Restore defaults, Import of that file showed 17 again. | none |
| 1g | Shortcuts: rebind and use | PASS | "Toggle info overlay" I rebound to J by pressing J in its box, Save wrote `"ToggleInfoOverlay": "J"`; in the main window J hides the status line and I no longer does anything. | none |
| 2a | Recovery, Diagnostics, Benchmark windows | PASS | Each opens dark and fully painted (Recovery: empty list state, Diagnostics: figures, Benchmark: profile list); closes with Escape and with the X button; no window left behind. | none |
| 2b | Benchmark run and cancel | PASS | "Quick check" on the 4-photo temp folder finished (Pass, report written to `%TEMP%\PhotoReviewRs-Benchmark-Reports\...\summary.json`); "Select all" + "Run benchmark" then Cancel at 26/30 showed "Canceled." and the source folder was unchanged (s1 to s4 still there). | none |
| 2c | Action profiles | PASS | Reached through Settings > Shortcuts > "Edit action profiles..."; list of the four default actions, dark; Escape and X close it and return to Settings. | none |
| 2d | Zoom prompt (`D3`) | PASS | "Custom click zoom level" dialog, dark, Escape and X close it. | none |
| 2e | Clear preview cache | PASS | "Confirm clearing the cache" (Yes / No); Escape cancels, Yes shows the status "Preview cache cleared." | none |
| 2f | `app.log` after the tool windows | PASS | No ERROR from these windows (the only ERROR lines of the session were the D3 panics of item 1). | none |
| 3 | Second-instance forwarding | PASS | Instance A on `s1.jpg`, instance B (same data root and prefix) started with `s3.jpg`: B exited 0 after 125 ms, A showed `3/4 · 14.39 KB · 800×600 · s3.jpg`. | `exe-walkthrough2-second_instance.png` |
| 4 | 3000-file folder | PASS | Observation on this machine (debug build, WARP, cold data root; NOT a performance claim): first frame with the photo and `1/3000` about 2.4 s after process start (window visible about 2.0 s, polled every 50 ms, includes about 0.3 s of PowerShell launch). Holding Right for 6 s (165 key repeats) reached `166/3000`; `Responding` stayed True in all 8 samples; WorkingSet 947 MB before, 1004 MB after, 968 MB right after the first frame. | `exe-walkthrough2-big_3000.png` |
| 5 | Corrupt / 0-byte / non-image `.jpg` | PASS | Truncated JPEG shows the decoded top part with a grey rest; garbled bytes, 0-byte file and a text file named `.jpg` give the status `Image error: <name> — WIC call failed (0x88982F61 / 0x88982F50)`, a black view; Right goes on to the next good photo; no crash. | `exe-walkthrough2-bad_files.png` |
| 6 | Folder with only non-images, empty folder (Ctrl+O) | PASS | Both: status `No supported images were found in this folder.`, title shows the folder; Right/Left/Home/End/F/Add/Space keys in that state do nothing and the app stays responsive. | none |
| 7a | Path with spaces and non-ASCII (`Ảnh đám cưới (1)`) | PASS | Opens through Ctrl+O, title and photo render; F2 rename to `ảnh mới.jpg` worked; Enter moved it to `Group-2`. | none |
| 7b | Long paths | PASS | 232-character folder: opens, rename and move work. 329-character folder: opens, renders, move to `Group-2` works; rename is refused in the dialog with "The full path would be too long (260 characters or more)." because the path already exceeds 260, same rule as C# `RenameNamePolicy.MaxPath`. | none |
| 8 | Read-only / locked file | PASS (adapted) | A read-only attribute does not stop rename or move (NTFS allows it, C# `File.Move` too): `r1.jpg` was renamed and moved. A file held open by another process (FileShare.None) gives `Could not run Action 2: The process cannot access the file because it is being used by another process. (os error 32)` and `Could not rename r2.jpg: ... (os error 32)`; file unchanged, no crash. | `exe-walkthrough2-locked_file_errors.png` |
| 9 | Close, exit code, `app.log` | PASS | Every instance exited 0 (Alt+F4); after the fix no run created an `app.log`; the pre-fix run had 5 D3 ERROR lines. | none |

## Defects found

| Id | Defect | Root cause | Status |
|----|--------|------------|--------|
| D3 | Apply in Settings: `Unhandled panic RefCell already borrowed` (`crates/pr-platform-win/src/host/main_window/lifecycle.rs:326`), logged once per Apply; the main window kept the old language until Save (C# `MainWindow.OnLanguageChanged` refreshes the text when the language changes). | Settings runs modal inside `MainWindow::update_view_model` (view model borrowed mutably). Apply changes the settings and the language; the overlay's posted refresh (`MainWindowOverlayViewModel::set_settings` / `refresh_localized_text`) called `update_view_model` again and panicked. Same family as D1 (the first pass fixed only the reads). | Fixed, commit `8962ecaa`: the overlay view model queues the settings (last wins) and the language refresh while the view model is busy and applies them after the modal command returns (`apply_deferred`). Test `startup_overlay_vm::overlay_vm_writes_while_a_modal_command_holds_the_view_model_are_applied_afterwards` (did not compile before: `apply_deferred` is new, and the no-window `set_settings` path used `borrow_mut`, which panics while the view model is held). Re-run in the exe: Apply en to vi to en updates the main window while Settings is open, no `app.log`. |

Defects left open: none.

## Harness notes (not product defects)

- Sending keys with `SwitchToThisWindow(main)` while a modal dialog (Rename) is open let Enter and F2 reach the disabled main window (a second Rename dialog opened, a file moved). A real mouse click on the main window and then Enter did not: the dialog took the key. Keys for dialogs must target the dialog handle.
- The Vietnamese IME of this machine garbles typed letters; text for dialogs was set with `WM_SETTEXT`, digits and shortcut letters were sent as virtual keys.
- Gates for the fix: `cargo fmt -p pr-shell --check`, `cargo check --workspace --locked`, `cargo clippy -p pr-shell --all-targets --locked -- -D warnings`, `cargo test -p pr-shell --locked` all green.

## Not checked

- Delete, "Remove (1) copies", "Remove originals" (Recycle Bin), RAW files and RAW+JPEG pairs, Import/Export of action profiles, "Manage editors...", "Check for updates" (network), "Open languages folder" / "Export strings to translate" / "Reload translations" buttons, the other Settings pages' controls beyond Display, Shortcuts and General.
- Second instance with a different prefix or `Instance mode` other than single window; large folders of real photos (the 3000 files were copies of one 2.7 KB JPEG, so decode cost is not representative); other DPI and multiple monitors.
