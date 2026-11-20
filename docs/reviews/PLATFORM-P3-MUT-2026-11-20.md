Status: evidence report (2026-11-20). Whole-file verdict for the `pr-platform-win` P3 files `render/image_cache/policy.rs`, `render/image_cache/upload.rs` and `shell_overlay/overlay/{state,input,autohide}.rs`. Measured on one machine (Windows 11, interactive session 1, WARP offscreen surfaces), `--jobs 1`, `MUT_TIMEOUT=600`, three independent processes with their own target dirs.

# `pr-platform-win` P3: mutation verdict

## Result

The whole-file run (before this PR) covered the five files: 164 caught, 32 unviable, 5 timeouts (detected, endless loops), 42 survivors. All 42 survivors are decided here: **42 of 42 killed or detected (no equivalent, none left open)**; one behaviour-neutral test seam is logged as P-411. `state.rs` and `autohide.rs` had no survivor (their timeouts are hang detections) and were not rerun.

Why so many survivors were false: the first run did not execute the existing tests that already guard these functions. `upload.rs` is guarded by `#[ignore = "native: needs desktop"]` tests (offscreen WARP, no window shown), which cargo-mutants skips unless `-- --include-ignored` is passed; `policy.rs` and `input.rs` are guarded by `render_image_cache_policy`, `shell_overlay_input` and `shell_overlay_behavior`, which the first run did not list in `--tests`. With the right test binaries most survivors were already caught; the rest needed 8 new tests and one assertion.

## Rerun (survivor functions only, new test set)

| File | Name filter (`-F`) | Identities | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|---|
| `policy.rs` | `get_or_upload\|write_rows`, `--tests render_image_cache_policy,render_image_cache_upload` | 9 | 5 | 0 | 1 | 3 (hang, detected) |
| `input.rs` | `pointer_down\|pressed_button\|wheel\|set_button_enabled`, `--tests shell_overlay_input,shell_overlay_behavior,shell_overlay_main_window` | 18 | 16 | 1 (see below) | 1 | 0 |
| `input.rs` rerun of that miss | `OverlayInput for ShellOverlay>::wheel` | 2 | 2 | 0 | 0 | 0 |
| `upload.rs` (device) | `fmt\|attach_to_surface\|raise_device_recreated\|max_bitmap_size\|generation\|create_bitmap` | 13 | 12 | 0 | 1 | 0 |
| `upload.rs` (backend) | `release\|subscribe_device_recreated\|tail_at\|upload_rows\|gpu_bytes` | 20 | 19 | 1 (see below) | 0 | 0 |
| `upload.rs` (adapter) | `run_stripe\|prefetch\|retain\|clear\|subscribe_upload_completed` | 7 | 7 | 0 | 0 | 0 |
| `upload.rs` rerun of that miss | backend `unsubscribe_device_recreated` | 1 | 1 | 0 | 0 | 0 |

The `upload.rs` runs use `--tests render_image_cache_upload,render_image_cache_policy --cargo-test-arg -- --cargo-test-arg --include-ignored --cargo-test-arg --skip --cargo-test-arg panorama --cargo-test-arg --test-threads=1` (the 240 MB panorama test is skipped to keep a mutant short; the other native tests cover every mutated line). Preflight: 98 tests executed, exit 0.

The three `policy.rs` timeouts (`write_rows -> Ok(())`, `> -> ==`, `> -> <`) never advance `uploaded_rows`, so the stripe job reschedules itself forever: a hang, counted as detected.

## Classification of the 42 old survivors

| Mutants | Verdict | Killer |
|---|---|---|
| `policy.rs` 348:32 guard -> false | killed (new) | `get_or_upload_of_a_complete_cached_image_returns_early_and_keeps_the_pending_source` (the early return keeps the pending source `Arc`; the fall-through path drops it) |
| `policy.rs` 363:72 `==` -> `!=` | killed (new) | `get_or_upload_completing_a_prefetched_image_protects_it_from_eviction` and `..._touches_it` (`set_budget_bytes(0)` keeps only the protected image; the eviction order shows the touch) |
| `policy.rs` 382:16 `delete !` | killed (new) | `get_or_upload_duplicate_from_a_reentering_backend_is_released_and_the_existing_entry_wins` (the fake backend caches the same buffer while `upload` runs) |
| `policy.rs` 651:16 `>` -> `>=` | killed (new) | `write_rows_never_uploads_an_empty_range` (a fake image that never reports completion; the second `get_or_upload` must not ask the backend for rows `[h, h)`) |
| `policy.rs` 649:9 `-> Ok(())`, 651:16 `==`, `<` | detected (timeout, hang) | existing stripe tests |
| `upload.rs` `ResourcesBitmapDevice` (88, 114, 124, 133 x3, 137 x3, 152, 158, 162 x2, 169 x2) | already killed by existing native tests once `--include-ignored` is passed | `resources_bitmap_device_*`, `device_lost_recreate_empties_the_cache_and_the_next_frame_draws`, `upload_with_data_round_trips_pixels`, `thousand_uploads_and_evictions_do_not_leak_bitmaps_and_stay_in_budget` |
| `upload.rs` 274, 290 x3, 341, 366, 370 x2 (`release_tiles`, `tail_at`, `upload_rows`, `release`, subscribe) | already killed by the native round-trip and striped-upload tests | `upload_with_data_round_trips_pixels`, `striped_upload_matches_a_synchronous_upload`, `failed_upload_releases_the_bitmaps_created_so_far` |
| `upload.rs` 374:9 backend `unsubscribe_device_recreated -> ()` | killed (new, pure) | `disposing_the_cache_unsubscribes_it_from_the_device` (the `NoDevice` fake counts handlers) |
| `upload.rs` 388:9 `gpu_bytes -> 0` | killed (new, pure) | `tiled_image_gpu_bytes_is_the_sum_of_the_texture_rectangles_times_four` (the old test used an empty tile list, where 0 is the right answer) |
| `upload.rs` 430:9 `Debug for D2dGpuImageCache` | killed (new, pure) | `cache_debug_names_the_adapter_and_the_inner_cache` |
| `upload.rs` 469, 491, 495, 507, 517 x2, 521 | 469, 507, 521 killed by existing native tests; 491, 495, 517 killed (new, pure) | `adapter_prefetch_and_retain_forward_their_results_and_errors`, `adapter_upload_completed_subscriptions_get_distinct_ids`; native `upload_completed_is_raised_once_per_prefetch_after_the_last_stripe` |
| `input.rs` 48:38 `match guard element.is_enabled -> true` | killed (new, with seam P-411) | `pointer_down_on_a_disabled_button_is_swallowed_without_pressing_it` (the overlay never disables a button itself, so the test needs `set_button_enabled_for_tests`) |
| `input.rs` `pressed_button -> None`, `wheel -> true` | already killed by `buttons_releasing_outside_the_button_does_not_click` and `wheel_is_consumed_over_a_visible_panel_and_passed_through_elsewhere` once `shell_overlay_input` is in `--tests` | existing |

## New identities found by the rerun

- `input.rs` trait forwarder `<impl OverlayInput for ShellOverlay>::wheel -> true` missed: `impl_overlay_input_forwards_every_method` only asserted `wheel` over a panel. Killed by `assert!(!input.wheel(FAR_AWAY))` in that test (2 of 2 caught on rerun).
- The seam adds the mutant `set_button_enabled_for_tests -> ()` (caught).
- Unviable (do not compile): `policy.rs` 334 `get_or_upload -> Ok(Rc::new(Default::default()))`, `upload.rs` 146 `create_bitmap -> Ok(Default::default())`, `input.rs` `pressed_button -> Some(Default::default())`.

## Product change (test seam, no behaviour change, P-411)

`ShellOverlay::set_button_enabled_for_tests` (`#[doc(hidden)]`, `shell_overlay/overlay/input.rs`): the C# tests set `ButtonElement.IsEnabled = false` directly; here the overlay never disables a button, so the disabled-button branch of `pointer_down` is otherwise unreachable from a test.

## Notes

- The `upload.rs` killers are native (`#[ignore]`, real Direct2D on offscreen WARP): they need an interactive desktop session and `--include-ignored`. A runner without a desktop cannot prove those mutants dead.
- This is not a full mutation verdict of the five files: only the functions of the survivors of the first whole-file run were rerun (cargo-mutants filters match by function name, so their neighbours were tested too: 67 distinct identities: 59 caught, 3 hang timeouts, 3 unviable, 2 missed; both misses fixed and rerun, 3 of 3 caught).
