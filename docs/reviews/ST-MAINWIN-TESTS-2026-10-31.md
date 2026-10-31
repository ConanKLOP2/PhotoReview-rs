# ST-MAINWIN-TESTS (2026-10-31)

Lane `st-mainwin-tests`: items 7 and 8 of section 6 of `ST-GAPMAP2-2026-10-31.md`. The brief said `crates/pr-shell/src/windowing/`; the main window actually lives in
`crates/pr-platform-win/src/host/main_window/`, so the tests are in `crates/pr-platform-win/tests/w8_mainwin_frame_tests.rs`.

## Production change (one additive accessor, no behaviour change)

`MainWindow::prefetch_host() -> Rc<dyn PrefetchHost>` in `host/main_window/viewing.rs`: builds the same `MainWindowPrefetchHost` (a `Weak` over the window) that
`attach_renderer` gives the prefetcher. Needed because the host is otherwise unreachable from a test binary. Item 7's other seams (observers for
`on_navigation_changed` / `on_compare_changed`) were NOT needed: both are observable through public state (see below), so no no-op observer hook was added.

## Tests (15, all `#[ignore = "native: needs desktop"]`)

Run on this machine: `cargo test -p pr-platform-win --test w8_mainwin_frame_tests -j 2 -- --include-ignored --test-threads=1`: 15 passed, 2.6 s. They use a hidden
shell window and an offscreen WARP surface; only the minimise test shows the window (minimised, not activated). Not added to `native-nightly.yml` (brief did not ask).

| Group | Tests | What they pin |
|---|---|---|
| 7 end-to-end bars (`ShellScrollBarEndToEndTests`) | `dragging_the_vertical_thumb_...`, `dragging_the_horizontal_thumb_past_both_ends_...`, `clicking_the_track_pages_...`, `bars_are_absent_in_fit_mode_...`, `resizing_the_window_reflows_...`, `zooming_resizes_the_thumb_...`, `thumb_drag_latency_...` | real `WM_LBUTTONDOWN/MOUSEMOVE/LBUTTONUP` through the real window procedure and handler chain, real `ViewportController`; expectations computed from the live layout |
| minimised | `a_minimised_window_draws_nothing_and_restore_brings_the_frame_back` | `is_minimized`, `restore`, a frame with a zero client size presents nothing, the frame after restore does |
| `update_retained` / `on_compare_changed` | `handling_an_image_retains_it_...`, `a_compare_change_rebuilds_the_retained_set`, control `a_change_set_without_compare_leaves_the_retained_set_alone` | retained set observed through the GPU cache: shrink the budget to 1 byte and see which entries survive |
| `on_navigation_changed` | `a_current_index_change_tells_the_prefetcher_...`, control `a_change_set_without_current_index_...` | observed through the prefetcher's direction memory: the window's step resets `last_index` (empty catalog), so 3 -> 4 keeps direction -1 |
| `PrefetchHost` | `prefetch_host_path_at_reads_the_catalog_...`, `prefetch_host_does_nothing_after_the_window_is_gone` | `path_at` over a real 3-file catalog (opened with `open_initial` + `ManualRunner`): in range, -1, count, `i32::MAX`; dead-window no-ops |

Deviation: `Zooming_ThroughTheWheel` zooms the viewer through its API instead of Ctrl+wheel (the fixture registers no input feature; the wheel path is a shell-input test).

Sanity (hand mutation, reverted, no cargo-mutants): making `on_compare_changed`, `on_navigation_changed` early-return and `path_at` return `None` made exactly
`a_compare_change_rebuilds_the_retained_set`, `a_current_index_change_tells_...` and `prefetch_host_path_at_...` fail. Mutants expected to be caught (not run):
`on_compare_changed -> ()`, `on_navigation_changed -> ()`, `update_retained -> ()`, `path_at -> None` and its range operators, `is_minimized -> false`, `is_minimized !=`
-> `==`, `restore -> ()`, `restore !=` -> `==`.

## Not covered (honest gaps)

- `start_lookup` worker thread and `schedule_retry` retry pass on a live window: only visible through `prefetch_calls`, which needs a decoded preview (corpus test).
- Close waiter `&&` -> `||` (`close_when_file_action_done_waiter`): existing `close_while_a_file_action_holds_the_gate_...` already covers the gate; not extended.
- `update_fit_size_if_alive`, F11 placement pair, `draw_presentation` pixels, device-lost crossfade, the 100-step navigation tests (W8-17e).
- The claim push `refs/claims/st-mainwin-tests` was rejected by the remote ("failed"); work was done in the worktree regardless.

## Proposed PARITY changes (not applied)

- `X-main-window-frame`: "not exercised: minimised window ... the bar-press fan-out and the 7 end-to-end bar tests" -> now exercised by `w8_mainwin_frame_tests` (7 bar tests + minimised). Stay Partial (first image, deferred frame still open).
- `X-main-window-lifecycle`: `is_minimized`/`restore` now exercised natively; `on_navigation_changed`/`on_compare_changed` now have observers. Stay Partial (close-waiter `&&`, F11 pair).
- `X-main-window-viewing`: `update_retained` output and `PrefetchHost::path_at` now covered; `start_lookup`/`schedule_retry` effects, `draw_presentation` still open. Stay Partial.
