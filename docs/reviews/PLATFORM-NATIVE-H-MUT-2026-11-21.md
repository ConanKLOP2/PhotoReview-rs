Status: evidence report (2026-11-21), run done, all 16 survivors decided (16 killed by new tests, 0 equivalent, 0 left open); 1 timeout is an endless loop. Native mutation lane H for `pr-platform-win`: whole-file run of `render/surface.rs` and `render/draw_context.rs`. Measured on one machine (Windows 11, interactive session 1, owner present), `mutants.sh --native --jobs 1`, `MUT_TIMEOUT=600`, own target dir.

# `pr-platform-win` native lane H: `render/surface.rs` and `render/draw_context.rs`

## Result

192 identities, 4 hours: **152 caught, 16 missed, 23 unviable, 1 timeout, 0 not run**.

Test set (`--include-ignored --test-threads=1`, preflight 688 tests, exit 0): the library unit tests (`--lib`, where both files keep their native tests) and the binaries `gfx_d2d_dwrite`, `host_swap_chain_clock`, `render_image_cache_upload`, `render_text`, `render_text_cache`, `render_image_cache_policy`, `shell_overlay_paint`, `shell_overlay_golden`, `shell_viewing_composer_native`, `shell_viewing_composer_twins_native`, `shell_viewing_crossfade_native`, `shell_viewing_scrollbars_paint_native`, `w8_viewing_killers`, `host_main_window_core`, `host_main_window_frame`, `host_main_window_lifecycle`, `shell_viewing_composer`, `shell_viewing_keep_warm`, `dialogs_zoom_diagnostics`.

Skipped as the earlier lanes skip tests that need unavailable resources: `render_keep_alive` (its test `real_keep_alive_on_this_machine_either_runs_or_disables_itself` waits 60 s for a keep-alive event this machine does not deliver, so the first preflight failed; the binary tests `keep_alive.rs`, not these files) and the `win32_*` binaries.

## The 16 survivors and their killers

All killers are inline `#[ignore = "native: needs desktop"]` tests next to the existing ones (the module-private `test_kit` and `HiddenWindow` live there).

| Survivor (line:col, mutant) | Killer |
|---|---|
| `surface.rs` 66 `Debug for D2dRenderSurface -> Ok(Default::default())` | `debug_names_the_surface_and_its_drawing_flag`: the text names the type and shows `drawing: false`, then `true` during a frame |
| `draw_context.rs` 193 `Debug for D2dDrawContext -> Ok(Default::default())` | `debug_names_the_draw_context_and_its_active_flag` (`active: true`, then `false` after `end`) |
| `surface.rs` 156 `is_warp -> true`, 387 trait `is_warp -> true` | `hardware_surface_is_not_warp`: an offscreen surface created with `use_hardware` reports `false`, also through `&dyn RenderSurface`. It returns early with a message when `create_device(Hardware)` fails on the machine (WARP fallback, nothing to distinguish) |
| `surface.rs` 197 `last_end_draw -> Default`, 203 `last_present -> Default` | `frame_timings_are_recorded_after_a_window_frame`: zero before the first frame, greater than zero after a window frame; an offscreen frame has `last_end_draw > 0` and `last_present == 0` |
| `surface.rs` 220 resize guard `com::is_device_lost(hresult) -> false` | `finish_resize_recreates_on_device_loss_and_errors_on_other_failures` (split P-440): the three loss codes recreate the device (handler count and generation 1, 2, 3), `DXGI_ERROR_INVALID_CALL` is the `Device` error without a recreate, `Resized` is `Ok` |
| `surface.rs` 367 trait `hwnd -> 0`, 379 trait `dpi_scale -> 0.0 / 1.0 / -1.0`, 383 trait `frame_latency_wait_handle -> 0` | `trait_surface_forwards_window_hwnd_dpi_and_wait_handle`: a window surface resized to dpi 1.25, read through `&dyn RenderSurface` (hwnd of the window, 1.25, the non-zero wait handle) |
| `draw_context.rs` 363 `rect_is_empty(source) \|\| rect_is_empty(destination)` with `&&` | `draw_image_with_an_empty_destination_draws_nothing`: zero and negative widths and heights draw nothing and are not counted as skipped (the early return is observable, the mutant killed) |
| `draw_context.rs` 617 `draw_tile` source bottom `region.bottom() - texture.y` with `+` | `tiled_crop_inside_a_lower_tile_row_matches_a_single_texture`: crops inside tile rows with `texture.y > 0` of a 300 x 300 noise image drawn pixel for pixel like the single-texture twin. The existing whole-image tiled test did not see it: only a crop that keeps the wrong bottom inside the bitmap differs |
| `draw_context.rs` 645 trait `clear -> ()`, 668 trait `draw_rectangle -> ()` | `trait_clear_and_draw_rectangle_draw_through_the_inherent_context`: both called as `<D2dDrawContext as DrawContext>::...`, the target is white and the stroke black |

## The timeout

`draw_context.rs` 580:35 `clip_depth -= 1` replaced by `/= 1` in `D2dDrawContext::end`: the `while clip_depth > 0` loop never ends, the 600 s `MUT_TIMEOUT` kills it. It is detected by hanging and no test can do better; not counted as a survivor.

## Rerun

`mutants.sh --native --jobs 1 --cargo-test-arg --lib` on both files with `-F` regexes of the survivor functions (preflight 311 tests, exit 0): **39 identities, 61 minutes, 36 caught, 0 missed, 2 unviable, 1 timeout** (the endless loop above). All 16 survivors are among the 36 caught. The line numbers of `surface.rs` after 210 moved by 5 (the `finish_resize` split).

## Production change

`D2dRenderSurface::resize` keeps its behaviour; the `match` over `ResizeOutcome` is now a private `finish_resize(outcome)` that `resize` calls (P-440, see [PROPOSED-DECISIONS](../PROPOSED-DECISIONS.md)). No new public item. P-441..P-447 were not needed.

## Notes

- The run opens hidden windows on the owner's desktop; it needs an interactive session.
- Lanes not started after this one: `display_clock.rs`, `host/ui_dispatcher.rs`, `instance_scope.rs`, `forward_{pipe,server,client}.rs`.
