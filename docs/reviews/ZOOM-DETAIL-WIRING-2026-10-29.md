# Zoom detail wiring: D1 never loaded the full-resolution image (2026-10-29, Machine A, claim `zoom-detail-wire`)

Status: fix report with before/after real-exe evidence. Base `aef7f7d4` (master after #336), branch `claude/zoom-detail-wire`. C# reference read with `git -C ../PhotoReview show 7eaee2a2:<path>` only.
Every exe run set `PHOTOREVIEW_RS_DATA_ROOT` (fresh temp dir), `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, a unique `PHOTOREVIEW_RS_INSTANCE_PREFIX` and `PHOTOREVIEW_RS_WARP=1`. One UI session at a time, only the PIDs the driver started were stopped, `Delete` was never pressed.

Real-config check (the Rust app's own `%LOCALAPPDATA%\PhotoReviewRs`), before the first run and after the last run:

| File | LastWriteTime | SHA-256 |
|------|---------------|---------|
| `config.json` | 10:50:07 | `DF7BFB4D0D924A75E142CB1BCC835FAAFA018F536FE41FD2C00F1D31F7B5DC6D` |
| `window-placement.json` | 10:50:56 | `E09243DB2E3DB3E9F53BCFC47C80A4065E02ECFD2115B81E73035AB3F8002268` |

After the last run: identical LastWriteTime and SHA-256 for both files. `%LOCALAPPDATA%\PhotoReview` (C#) was not touched.

## Root cause

`ZoomDetailLoader` (W8-07g), the presenter glue (`ImagePresenter::set_viewer_zoom`, effects, `pump`) and the viewing sink (`SourceSizeSwapping`, #336) were all present and unit-tested, and the view model
already forwarded zoom changes made by ITS OWN commands (`zoom_in`, `zoom_actual_size`, `toggle_fit`, ... call `apply_viewer_update`). But the real window never calls those commands for a zoom:
keys, wheel, click zoom and Fit are handled by `ShellInput` (`pr_platform_win::shell_input::dispatch`), which changes the shared `ViewerState` directly through the pointer controller. In C# the viewer raises
`ZoomModeChanged`, and `MainViewModel` (line 117) answers with `presenter.SetViewerZoom(viewer.EffectiveZoom)`. The Rust `ViewerState` has no event; its `ViewerUpdate::zoom_mode_changed` was consumed by
`ShellInput` only to stop the kinetic glide (`stop_kinetic_impl`). So the presenter never learned that the user left Fit, `ZoomDetailLoader::set_zoom` was never called from the exe, and no load ever started.
That is why the log had no `ZoomDetail original ready` line, why frames at 60 ms, 700 ms and 10 s were pixel-identical, and why a plain 2400x1600 JPEG behaved the same.

## What was wired

| Piece | File | Change |
|---|---|---|
| View model entry (C# `ZoomModeChanged` handler) | `crates/pr-app/src/viewmodels/main/commands.rs` | `MainViewModel::on_viewer_zoom_mode_changed()` = `apply_viewer_update(ViewerUpdate { zoom_mode_changed: true, .. })` -> `presenter.set_viewer_zoom(viewer.effective_zoom())` + `finish_presenter_call` |
| Input seam | `crates/pr-platform-win/src/shell_input/dispatch.rs` | new `ShellInputHost::on_viewer_zoom_mode_changed()` (default no-op); `run_pointer` and `finish_fit_step` call it right after `stop_kinetic_impl`, after every pointer/surface/viewer borrow was released |
| Window host | `crates/pr-platform-win/src/shell_input/main_window.rs` | `MainWindowInputHost` implements it with `self.call(|vm| vm.on_viewer_zoom_mode_changed())`; `with_view_model` already uses `try_borrow_mut` and logs `Input command skipped: the view model is busy (re-entrant call).` instead of panicking (the RefCell family of #318, #332, #333, #336) |

Nothing else was missing: the composition (`pr-shell/src/composition/main_view_model.rs`) already builds the presenter with the runner, the `wake` that posts the pump, the sink and the bridge; `MainWindow::pump_view_model` already
drains the presenter inbox on the UI thread; `LoadingMode` and `RawFullDecode` are read from the settings by the presenter / preview service. No `unwrap`/`expect`/`unsafe` added.

## Real exe, before and after (debug build, WARP, 1200x800 window, `big4000.jpg` = 4000x3000 with 1-pixel black/white vertical stripes over the whole picture and a white box in the middle)

The driver presses `1` (D1, `ZoomActualSize`) and captures the window with `PrintWindow` at 60 ms, 700 ms and 5 s.

| Case | Before (exe built without the call) | After |
|---|---|---|
| `big4000.jpg`, LoadingMode Preview | frames 60 ms = 700 ms = 5 s (same MD5), stripes blurred to grey, no `ZoomDetail` log line | 60 ms frame = the upscaled preview (same MD5 as before), 700 ms = 5 s frame sharp stripes; log `ZoomDetail original ready token=1 4000x3000 ms=47..61` |
| `plain2400.jpg` (2400x1600), Preview | blurry (frame MD5 `725E31ED`), no log line | sharp from the first frame (decode 19 ms, before the 60 ms capture); log `ZoomDetail original ready ... 2400x1600 ms=19` |
| `big4000.jpg` and `plain2400.jpg`, LoadingMode Fast | not run before | load and swap, same log line |
| `big4000.jpg` and `plain2400.jpg`, LoadingMode Original | not run before | no load (the shown bitmap is already the original), frames identical and sharp from the start, no log line |
| `big4000.jpg`, D1 then PageDown (NextFolder, opens the sibling folder `out`) 5 ms later | n/a | no `ZoomDetail original ready` line, the 5 s frame shows the next folder's first image, no stripes carried over (a timing race: observed once, the deterministic proof is the unit test) |
| Synthetic DNG `plain.dng` / `swap.dng` (4000x3000, `python -I tools/dev/make-synthetic-raw.py`), `RawFullDecode=OnZoom` | not run before | status line `Decoding RAW...` at 700 ms (300 ms delay), `ZoomDetail original ready ... 4000x3000 ms=1677`, indicator gone and the status line back to `5/9 - 22.89 MB - 4000x3000 - plain.dng` at 5 s |
| Same DNG, `RawFullDecode=Never` | n/a | no load, no indicator, frames identical |

No jump at the swap: the horizontal edges of the white box are at the same y (110 px and 712 px) in the 60 ms preview frame and in the 5 s original frame (`SourceSizeSwapping` re-anchor of #336).

Screenshots (docs/reviews/img/): `zoom-detail-before-big-5s.png` (blurry, before), `zoom-detail-after-big-60ms.png` (upscaled preview right after D1), `zoom-detail-after-big-5s.png` (sharp stripes after the swap),
`zoom-detail-raw-onzoom-700ms.png` (indicator), `zoom-detail-raw-onzoom-5s.png` (swapped). The synthetic DNG scene is flat colour bars, so the RAW proof is the log line, the indicator and the status text, not sharper pixels.

## Tests

New tests (all green):

- `crates/pr-app/tests/viewmodels_main_navigation.rs`: `input_zoom_past_the_preview_size_starts_exactly_one_load_and_swaps_the_original_in` (one load even after a repeated notification, decoder counter = 1, original shown), `input_zoom_that_the_preview_already_covers_starts_no_load`, `navigating_while_an_input_zoom_load_runs_drops_the_original` (the stale 6720 px original never replaces b.jpg), `input_zoom_back_to_fit_before_the_original_lands_does_not_swap_it_in`. They drive the viewer like the input does (`zoom_to_actual_size` on the shared viewer, then `on_viewer_zoom_mode_changed`).
- `crates/pr-platform-win/tests/shell_input_dispatch_keys.rs`: `zoom_commands_tell_the_host_that_the_zoom_mode_changed_so_the_presenter_can_load_the_original`. Proved to FAIL without the call (temporarily removed in `run_pointer`: `left == right` failed: D1) and pass with it.
- native `crates/pr-platform-win/tests/shell_input_e2e.rs`: `zoom_to_100_percent_through_a_real_key_loads_the_original_and_shows_it` (real MainWindow, real WM_KEYDOWN, 4000x3000 files, navigation then drops the original).

Runs (debug, `--locked`): `zoom_detail` 43/0; pr-app presenter/viewing/zoom binaries (`viewmodels_main_{commands,navigation,notify_move,session,state}`, `presenter_{state,present_a,present_b,missing,photo_info,compare_pair,rename}`, `rename_presenter_matrix`, `services_preview_state`, `pointer_zoom`, `pointer_click_zoom`, `viewer_state*`, `menus_zoom_rules`) all 0 failed (e.g. navigation 66, commands 114, present_b 58, present_a 50, session 42, state 49, notify_move 62); pr-platform-win `shell_input_dispatch` 17 (1 ignored), `_keys` 22, `_pointer` 30, `shell_input_basics` 39 (7 ignored native), `shell_input_surface` 29, `shell_viewing_presentation_sink` 16, `host_main_window_{core,lifecycle,viewing}` 7/8/4 (46 ignored native, not run); pr-shell `composition_{adapters,graph,main_view_model}` 4/3/8 (1 ignored); native `shell_input_e2e --include-ignored --test-threads=1` 16/16. `cargo fmt -p <crate> --check` pr-app, pr-platform-win, pr-shell clean; `cargo check --workspace --locked` clean; `cargo clippy -p pr-app / pr-platform-win / pr-shell --all-targets --locked -- -D warnings` clean.

Mutation (cargo-mutants, only `on_viewer_zoom_mode_changed`, `--baseline skip --timeout 90 -j 1`, `--test=viewmodels_main_navigation`, own target dir outside the worktree): 2 mutants tested in total (the filter also matched one in `refresh_view`): 1 caught (`on_viewer_zoom_mode_changed -> Default::default()`, my function), 1 missed (`delete field viewer from struct ChangeSet expression in refresh_view`, pre-existing code that this PR does not touch, not killed here), 0 unviable, 0 timeouts. The function has no other mutable operator (the `zoom_mode_changed: true` literal is not generated as a mutant); it is not a full mutation verdict of `commands.rs`.

NOT run: full `cargo test --workspace`, the ignored native tests of `host_main_window_*` and `shell_input_basics`, mutation of the `pr-platform-win` glue (policy: native glue is tested by native tests), a real-camera RAW corpus.

## Not done / open

- Stale completion (INV-1) and "Fit before the original lands" are proved only by the tests, not forced in the exe (the decode takes 20 to 60 ms; the RAW decode takes 1.7 s and was not interrupted by hand).
- A zoom change that arrives while the view model is borrowed (a re-entrant call) is skipped with a warning and not retried; the next zoom change or navigation (`take_viewer_zoom_change` pull) re-syncs. No such path was observed in the exe.
- No real camera RAW (only synthetic DNG), no hardware GPU run (WARP), no wheel / touchpad (`Refresh`) run in the exe, the native twin of `ExifRotatedSource_UsesPostOrientationOriginalSize` is still open.
- A dev build has no `libraw.dll` / `turbojpeg.dll` next to the exe unless `tools/dev/stage-native.ps1` ran; the JPEG runs worked without them (WIC), the RAW runs used the staged ones.
