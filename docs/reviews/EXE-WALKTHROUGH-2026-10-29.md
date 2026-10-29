# Exe walkthrough, everyday JPEG review (2026-10-29, Machine A)

Status: evidence report. Real `PhotoReviewRs.exe` (debug build of branch `claude/exe-walkthrough`, base `ab632545`) driven on a desktop with `keybd_event`, `SetCursorPos` + `mouse_event`,
`PrintWindow` captures and `EnumWindows`. The C# baseline `7eaee2a2` was read with `git show` only. Test photos: six generated JPEGs in folder `A` (1600x1000, 4000x3000,
portrait 1000x1600, tiny 200x150, 1600x1000 with EXIF orientation 6, 1280x720) plus sibling folders `B`, `C` and `D` (`img.jpg` + `img (1).jpg`, a numbered compare pair).
Every run used `PHOTOREVIEW_RS_DATA_ROOT`, a unique `PHOTOREVIEW_RS_INSTANCE_PREFIX` and `PHOTOREVIEW_RS_WARP=1`; the first two runs lacked `PHOTOREVIEW_RS_ISOLATE_CONFIG=1` (see "Harness incident").

## Result table

| # | Item | Result | Observation | Screenshot (docs/reviews/img/) |
|---|------|--------|-------------|------------|
| 1 | Open at startup; Right/Left/Home/End; status line | PASS | Opens on the argument file; status `1/6 · 84.54 KB · 1600×1000 · a1_1600x1000.jpg`; Right/Left/Home/End move and index/size/name update. | `exe-walkthrough-s01_start.png` |
| 2a | `Add` / `Subtract` | PASS | Add zooms in about the view centre (scroll bars appear); two Subtracts return to about Fit. | not kept |
| 2b | `F` toggle fit, `W` fit width, `H` fit height | PASS | F returns to Fit; W fills the width (vertical scroll bar); H fits the height. | not kept |
| 2c | `D1` actual size | PASS | 4000x3000 shows at 100 % (centre region, both scroll bars). | not kept |
| 2d | `D2` click zoom | PASS | Zooms to the click-zoom percent. A second `D2` does NOT go back to Fit: identical to C# `ToggleClickZoomAsync` (`ClickZoomKeyTogglesFit` is false by default). | not kept |
| 2e | `D3` custom zoom | PASS | Dialog "Custom click zoom level" (10-800 %), dark themed; typing 200 + Enter applies and closes. | not kept |
| 2f | Mouse wheel zoom | PASS | Wheel up zooms in, wheel down zooms out (back to Fit after three notches). | not kept |
| 2g | Drag-pan when zoomed | PASS | Dragging moves both scroll bar thumbs; dragging back restores them. | not kept |
| 2h | `K` keep zoom | PASS | With K on, Home from a 100 % image shows the next image still zoomed; with K off the next image is fitted. Right/Left while zoomed PAN the image (C# `KeyboardPan`), they do not navigate until the edge. | not kept |
| 3a | `I` info overlay | PASS | First I hides the status line, second I shows it again. | not kept |
| 3b | `C` compare, leaving it | PASS | `C` without a pair does nothing (C# router rule). Entering folder `D` shows the numbered pair side by side (`1/2 \| Compare \| img.jpg (24,911 B) ↔ img (1).jpg (25,829 B) \| hash DIFFERENT`); `C` closes it to the single image, `C` again reopens it. | `exe-walkthrough-d1_compare.png` |
| 3c | `F11` fullscreen and back | PASS | Window becomes 1920x1080 without title bar; F11 restores 1200x800. | not kept |
| 3d | `Space` skip | PASS | Moves to the next photo (2/6). | not kept |
| 3e | `PageDown` / `PageUp` | PASS | A to B to C to D; stays on D at the end; PageUp goes back to C. The title shows the folder. | not kept |
| 4 | `O` / `Ctrl+O` folder dialog | PASS | Plain `O` does nothing (the router requires Ctrl, as C#); `Ctrl+O` opens "Choose an image folder" (dark, painted). Typing a path + Enter navigates inside the dialog; clicking "Select Folder" opens that folder (title became `B`). Esc closes it. | `exe-walkthrough-o2_typed.png` |
| 5a | Toolbar: open folder, Fit, menu | PASS | Folder button opens the folder dialog; Fit button fits a zoomed image; the three-dots menu lists Recovery, Diagnostics, Benchmark, Clear preview cache, Remove (1) copies, Remove originals. | not kept (the captures include other desktop windows) |
| 5b | Toolbar gear | FAIL before fix, PASS after | Gear click did nothing: `app.log` had `Unhandled panic RefCell already mutably borrowed at overlay_vm.rs:60` (D1). | `exe-walkthrough-st1.png` (after fix) |
| 5c | Right-click context menu | PASS | Undo last action, Refresh (R), Open folder..., Next folder (PageDown), Previous folder (PageUp), Open in External Editor, Rename... (F2), Copy File Name, Copy Full Pathname, Settings... The Settings entry failed like the gear until the D1 fix. | not kept |
| 5d | `F2` Rename | PASS | "Rename file" dialog opens; a new name + Enter renames the file on disk and the status shows `Renamed a1_1600x1000.jpg to a1_renamed.jpg. Press Ctrl+Z to undo.` | `exe-walkthrough-ren3.png` |
| 6a | Settings window from the gear, pages switch | PASS after fix | General, Display, Mouse & zoom, Performance, Shortcuts, Files & safety, Diagnostics all switch and render. Navigation labels drew `Mouse _zoom` / `Files _safety` until D2 was fixed. | `exe-walkthrough-st1.png`, `exe-walkthrough-nav_fixed.png` |
| 6b | A setting applies and persists | PASS | Ticking "Show current zoom percentage on the photo" and Save shows `74%` on the photo at once, writes `"ShowZoomIndicator": true` to `config.json` of the isolated data root, and the `74%` is still there after a restart. | not kept |
| 7a | `Enter` move to Group-2 | PASS | `a3` moved to `A\Group-2`, the view advanced. | not kept |
| 7b | `Ctrl+Z` undo | PASS | `a3` back in `A`. Undo is single level (a second Ctrl+Z does nothing), like the C# `UndoService`; Copy is not undoable (undo covers Move/Recycle). | not kept |
| 7c | `F3`, `F4` groups, `F5` backup | PASS | F3 moved `a3` to `Group-3`, F4 moved `a4` to `Group-4`, F5 copied `a5` to `Backup` (the original stays). | not kept |
| 7d | `Y` copy to folder, `M` move to folder | PASS | Each opens its own "Choose a folder to copy/move the photo to" dialog; the chosen folder received the file (copy leaves the original, move removes it from `A`). | not kept |
| 8 | `Delete` | NOT EXERCISED | Not pressed (real Recycle Bin). Code check: `crates/pr-shell/src/composition/platform.rs` builds `WindowsRecycleBin::new(WindowsRecycleBinRestoreBackend::new(log))` unless `GraphOverrides::recycle_bin` is set, and `factories.rs` hands that `Arc<dyn RecycleBin>` to the file-action service; the router maps a bare `Delete` to `RECYCLE`. | none |
| 9 | EXIF-rotated, portrait, tiny, large | PASS | The orientation-6 file shows upright as 1000x1600 (text reads top to bottom, the black marker moved to the top left), the portrait fits with side bars, the 200x150 file is upscaled to fit, 4000x3000 fits. | `exe-walkthrough-e1_a5.png` |
| 10 | Alt+F4 / close button | PASS | Process exit code 0 both ways (a wrapper waits for the PID), no leftover windows. `app.log`: the first run had 12 ERROR entries (the D1 panics); runs after the fix wrote no log file at all (no ERROR). | none |

## Defects found

| Id | Defect | Root cause | Status |
|----|--------|------------|--------|
| D1 | Settings never opens from the gear or the context menu; `app.log` gets `Unhandled panic` + `UI loop exception`; the same panic also fires during the Y/M folder pickers (harmless there). | `MainWindow::update_view_model` holds `borrow_mut` on the view model while the modal command runs. Its nested message loop runs the overlay's posted `refresh_model`, whose `overlay_model()` (and `settings()`) in `crates/pr-shell/src/startup/overlay_vm.rs` used `borrow()` and panicked; the panic unwound through the Settings dialog before it showed. The ignored native test `settings_dialog_command_does_not_panic_the_paint` only drove `render_now` + `pump_view_model`, never the posted refresh. | Fixed, commit `1b898e47`; test `startup_overlay_vm::overlay_vm_reads_while_a_modal_command_holds_the_view_model_return_the_last_values` (panics at `overlay_vm.rs:55` before, passes after). Items 5b/6a/6b re-run in the exe. |
| D2 | Owner-draw list rows eat the ampersand: the Settings navigation shows `Mouse _zoom` and `Files _safety` (the `&` is a mnemonic prefix and underlines the space). | `ListBoxControl::draw_item` passed `0x80` (`DT_NOCLIP`) where C# `NativeControls.cs` uses `DtNoPrefix` (`0x800`), for badge, subtitle and title. | Fixed, commit `51e8b5d1`; test `dialogs_native_controls_basic::draw_item_text_flags_keep_an_ampersand_literal` (did not compile before: the named flag constants are new). Re-checked in the exe (`exe-walkthrough-nav_fixed.png`). |

Defects left open: none found in the checked items.

## Not checked

- `Delete` and everything that touches the real Recycle Bin; undo of a Recycle.
- Language switch in Settings (only a checkbox was exercised); Settings Restore defaults, Export, Import, Apply vs Save vs Cancel; editing keys on the Shortcuts page.
- `Shift+M` / `Shift+Y` forced picker, `MoveCopyReuseLastFolder`, confirm-before-action, RAW files and RAW+JPEG pairs, capture-pair member switch, compare click-to-select, kinetic pan, touchpad gestures, middle click, drag and drop, multiple monitors, DPI other than the machine's, second-instance forwarding.
- Auto-hide of toolbar and info, image transition, loading a large folder.
- The menu entries Recovery, Diagnostics, Benchmark, Clear cache and Remove duplicates were listed, not run.

## Harness incident (data outside the isolated root)

The first two runs set `PHOTOREVIEW_RS_DATA_ROOT` but not `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, so (by design, `AppPaths::new`) `config.json`, `window-placement.json` and `cache/` went to
the Rust app's own `%LOCALAPPDATA%\PhotoReviewRs`. The C# app's folder was not touched. Changes in `config.json` from those runs: `LastMoveToFolder` and `LastCopyToFolder` (temp folders),
`ClickZoomPercent` 200 (default 100), `ShowZoomIndicator` true (default false); `window-placement.json` was rewritten (26,26,1226,826); `cache/` gained a few `.pv4` previews of the temp photos.
An attempt to restore these four fields was refused by the permission classifier and was not retried: the owner should reset them (or delete `config.json` to get defaults). All later runs used `ISOLATE_CONFIG=1`.
In the third run the window opened at 182,182 instead of 26,26, so four scripted clicks (assumed window-relative) landed at screen positions outside the app; the helper now converts from the window rectangle.

## Method notes

Ctrl and Alt combinations were sent with `keybd_event` after `SwitchToThisWindow`/`SetForegroundWindow`. The keyboard layout of this machine uses a Vietnamese IME that turned typed text such as `dest` into
`dét`, so dialog paths were pasted from the clipboard. Exit codes came from a wrapper (`Start-Process -PassThru` + `WaitForExit`). The Windows folder picker navigates into a typed path on Enter,
so "Select Folder" was clicked to confirm.
