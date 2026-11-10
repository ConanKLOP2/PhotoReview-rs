Status: evidence report (2026-11-06), task P8, branch `test/mut8-main-window-core` from origin/master e19b591b. One new test file and this file; no production change, no edit of PARITY, README, BACKLOG, PROPOSED-DECISIONS, task_on_progress or SYNC-LEDGER.

# P8: `pr-platform-win/src/host/main_window/core.rs` (X-main-window-core), mutation under OWNER RULE B (strict, P-391)

Rule B: a mutant is closed only if caught, unviable (a real rustc error), or proven equivalent over the whole domain or every input class. A timeout is never caught; infrastructure failures count as NOT RUN.

## Method

* `cargo mutants --list --file crates/pr-platform-win/src/host/main_window/core.rs`: 142 mutants (same 142 as P3). ALL 142 were run again in one process (the 69 decided in P3 were not excluded), 41 minutes.
* Run 1: `bash tools/dev/mutants.sh pr-platform-win crates/pr-platform-win/src/host/main_window/core.rs --cargo-arg --test=<binary> ... --budget 40000 --jobs 1 --cargo-test-arg -- --cargo-test-arg --include-ignored --cargo-test-arg --test-threads=1`, `MUT_TIMEOUT=300`, `CARGO_BUILD_JOBS=2`, TEMP/TMP `C:\Users\PC\Projects\target-mut8-tmp`. Test binaries (13): `host_main_window_core`, `host_main_window_frame`, `host_main_window_lifecycle`, `host_main_window_viewing`, `host_main_window_windowing`, `mut3_mainwindow_debug`, `mut_core3_killers`, `mut_zoomcore_killers`, `native_initial_viewport`, `w8_mainwin_frame_tests`, `w8_mainwin_killers`, `w8_mainwin_seams2`, `w8_rest1_killers`. Preflight of the unmutated command: 184 tests, exit 0. Result: 142 tested, 89 caught, 21 missed, 32 unviable, 0 timeout.
* Run 2 (the 21 survivors, `-F` filters; 22 mutants matched because two share a line:col): `--cargo-arg --lib` (inline tests, `registry_tests`, `tests`) plus the 13 binaries above plus 12 more that build a `MainWindow` (`rc_dialogs_startup_input`, `rename_shell_e2e`, `shell_input_dispatch_pointer`, `shell_input_e2e`, `shell_input_pan_glide_native`, `shell_input_surface`, `shell_menus_e2e`, `shell_menus_main_window`, `shell_overlay_input`, `shell_overlay_main_window`, `shell_overlay_paint`, `shell_startup_placement`). Preflight: 710 tests, exit 0. 10 caught (9 of the 21 survivors plus one neighbour already caught in run 1), 12 missed (11 that need a new test, plus 792:9).
* Run 3 (the 12 remaining survivors, with the new binary `mut8_main_window_core`): all 11 targeted mutants caught; `792:9` still MISSED.
* No test of the unmutated tree fails on this machine for these binaries (plain `cargo test` of all 25 binaries and the lib: exit 0), so nothing was skipped.
* Checked: no log of run 1 holds an infrastructure marker (paging file, LNK1102, E0463, out of memory, runaway allocation); every unviable log holds a real rustc error; every caught mutant has a failing test in its log. There was no timeout and no build failure, so NOT RUN is 0.

## Counts (tested = caught + missed + unviable + timeout)

| file | listed | tested | caught | missed | unviable | timeout | NOT RUN |
|---|---:|---:|---:|---:|---:|---:|---:|
| `host/main_window/core.rs` | 142 | 142 | 109 | 1 | 32 | 0 | 0 |

(Run 1 alone: 89 caught, 21 missed, 32 unviable. After run 2: 98 caught, 12 missed. After run 3: 109 caught, 1 missed.)

## P3 survivors and timeouts: fate

| mutant | P3 | fate now | killer |
|---|---|---|---|
| 145:9 `Debug for MainWindowOptions -> Ok(Default::default())` | timeout | caught (run 1) | `mut3_mainwindow_debug::main_window_options_debug_shows_the_three_plain_fields` |
| 356:9 `Debug for MainWindowDeps` | timeout | caught (run 1) | `mut3_mainwindow_debug::deps_and_viewport_callbacks_debug_name_their_types` |
| 456:33 `+ -> -` in `TimerQueue::schedule` | missed | caught (run 1) | `host_main_window_core` post_after tests (`* ` variant is unviable) |
| 473:30 `due <= now` guard `-> true` | missed | caught (run 1) | same |
| 630:5 `with_window -> None` | missed | caught (run 1) | `mut_zoomcore_killers::registry_resolves_a_live_window_by_id_and_as_current` (`Some(Default)` variant unviable) |
| 639:5 `with_current_window -> None`, 640:63 `delete !` | missed | caught (run 1) | same |
| 666:53 `> -> >=` in `register_window` | missed | caught (run 2, lib) | `core_registry_tests::register_window_drops_dead_entries_on_the_way` (`==` and `<` were caught in run 1) |
| 684:5 `post_to_window -> ()` | missed | caught (run 1) | `host_main_window_core` post/stripe tests |
| 709:9 `schedule_stripe -> ()` | missed | caught (run 1) | `host_main_window_core::stripe_scheduler_never_runs_inline_and_runs_on_the_ui_thread_at_background` |
| 792:9 `Debug for MainWindowInner -> Ok(Default::default())` | missed | OPEN | see below |

The two timeouts of P3 were NOT counted as caught on faith: they were rerun and the mutated tests fail by assertion (no timeout in the new run).

## Survivors of run 1 and their fate

| mutant | fate | killer |
|---|---|---|
| 430:9 `TimerEntry::eq -> true`, `-> false`; 430:18 `== -> !=`; 430:31 `&& -> \|\|`; 430:43 `== -> !=` | caught (run 2, lib) | `core_registry_tests::timer_entries_are_equal_only_when_due_and_arrival_both_match` (the heap uses `cmp`, never `eq`, so only the lib test reaches it) |
| 673:5 `unregister_window -> ()` | caught (run 2, lib) | `core_registry_tests::unregister_window_removes_only_that_id` |
| 700:9 `Debug for DispatcherStripeScheduler` | caught (run 2, lib) | `core_registry_tests::dispatcher_stripe_scheduler_debug_names_the_type_and_the_window` |
| 1073:9 `MainWindowInner::options -> &Box::leak(Default)` | caught (run 2) | a binary of the wider set (the first 13 do not read it) |
| 996:9 `publish_viewport_size -> ()` | caught (run 3) | `mut8_main_window_core::layout_update_publishes_the_new_viewport_size_for_worker_threads` |
| 1003:9 `register_keep_warm -> ()` | caught (run 3) | `mut8_main_window_core::keep_warm_registration_is_stored_and_its_frame_log_records_the_window_frames` (no frame log file without the registration) |
| 1103:9 `set_keep_warm -> ()` | caught (run 3) | same test (the setup is dropped at once: the file exists but holds no record of the 3 frames) |
| 1166:9 `strong_count -> 1` | caught (run 3) | `mut8_main_window_core::strong_count_sees_the_upgrade_held_by_a_window_event` (the count inside a frame delivered by the render tick is above 1) |
| 1258:18, 1258:32 `> -> >=` in `attach_renderer` | caught (run 3) | `mut8_main_window_core::attach_renderer_leaves_the_surface_size_alone_for_a_zero_client_side` (client 0 x 480, 640 x 0, 0 x 0 via `WM_SIZE`) |
| 1293:25, 1293:46 `> -> >=`, 1293:29 `&& -> \|\|` in `open_initial` | caught (run 3) | `mut8_main_window_core::open_initial_keeps_the_decode_box_for_a_zero_client_side` |
| 1295:74, 1295:106 `/ -> *` in `open_initial` | caught (run 3) | `mut8_main_window_core::open_initial_divides_the_client_pixels_by_the_dpi_scale` (144 dpi via `WM_DPICHANGED`, 960 x 720 client; the test asserts that the `*` box differs from the `/` box) |

## OPEN: 1 mutant

* `core.rs` 792:9 `replace <impl fmt::Debug for MainWindowInner>::fmt -> fmt::Result with Ok(Default::default())` (MISSED in runs 1, 2 and 3). `MainWindowInner` is private to the crate and no public path formats it: `MainWindow`'s own `Debug` prints `self.inner.id` field by field, no public function hands an `&MainWindowInner` to a test, and a lib test cannot build one without the whole native fixture (`main_window_fixture`, pr-app support modules, `extern crate self` is not declared). Killing it needs a production seam (for example `MainWindow`'s `Debug` going through `{:?}` of the inner state, or a `#[cfg(test)] pub(crate)` accessor used by an in-crate test), which this task excluded. It is not an equivalent mutant: the formatted text differs. Same proposal as P3-4.

## New test

`crates/pr-platform-win/tests/mut8_main_window_core.rs`: 7 tests, all `#[ignore = "native: needs desktop"]` (hidden real shell window and WARP offscreen surface; no attended desktop; the client size and the dpi are driven by `WM_SIZE` / `WM_DPICHANGED` messages, so no minimised window is shown). No sleeps; waits are bounded (`wait_until`). No `unsafe` in the file.

## Checks

`rustfmt --edition 2024 --check` on the new file; `cargo clippy -p pr-platform-win --tests -- -D warnings`; `cargo test -p pr-platform-win --test mut8_main_window_core -- --include-ignored --test-threads=1` (7 passed); `PY_BIN=... bash tools/dev/pre-pr.sh` (result in the final report of the lane).

## NOT RUN

None (142 of 142 tested). Open: 792:9 (needs a seam).

## Follow-up P-397 (2026-11-10): 792:9 closed

`MainWindow`'s `Debug` now prints the inner state through `{:?}` (`MainWindow { inner: MainWindowInner { id: .. , .. }, .. }`), so the public type reaches the `Debug for MainWindowInner` impl. New native test `mut8_main_window_core::main_window_debug_shows_the_inner_state_with_its_id` (`#[ignore]`, 1 passed). cargo-mutants re-run (`-F core.rs:792:9`, `-j 1`, `--baseline skip`, test binary `mut8_main_window_core`, `--include-ignored`): 792:9 CAUGHT (the filter also matched `core.rs:1247:13`, also caught): 2 of 2 caught. Open: 0.
