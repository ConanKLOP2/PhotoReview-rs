# ST-POINTER-PAN: native drag, release, glide, click tests (2026-10-31)

Status: done, evidence of a real run on a machine with a desktop. Lane `st-pointer-pan`, PARITY row `X-pointer-click-pan` (K-34, K-35, K-36, K-37, K-15, C-10, K-27 arrows, S-35, S-46, S-51).

## Gap

ST-GAPMAP2 section 6 item 11: "no native test of drag -> release -> glide through real messages". `shell_input_pan_glide_native.rs` (cl-misc, 3 tests) already proved that a fast release HOOKS a glide, but nothing moved the glide after the release, and double click, slow or far second click and a lost capture had no window-level test.

## What was added

All tests are in `crates/pr-platform-win/tests/shell_input_pan_glide_native.rs` (9 tests now: 3 old, 6 new), each `#[ignore = "native: needs desktop"]`, hidden shell window, WARP surface, real `MainWindow`, real window procedure via `SendMessage`. No sleeps.

Test infrastructure (test code only, no production change):

- `common/main_window_fixture.rs`: `BuildOptions.frame_clock: Option<Rc<dyn HostFrameClock>>` (default `None` = the timer clock as before; every existing caller uses `..BuildOptions::default()`).
- The test binary gives the main window a `FakeHostClock` (`common/viewing_fakes.rs`) and a `ShellInputEnvironment` with a test-owned `message_time`, `double_click_time_ms` (500) and `double_click_size_pixels` (8 x 8). A glide frame happens only when the test calls `tick_frame()` (16 ms of fake rendering time per frame); `settle_step` ticks automatically only while the window comes up (`AUTO_FRAMES`), the glide tests switch that off. Smoothing is set to `Off`, so the glide is driven by the fake render times alone.

New tests:

| Test | What it proves (through real messages) | C# twin |
|---|---|---|
| `a_released_drag_glides_further_frame_by_frame_and_then_ends_itself` | after a fast release the offset does not move without a frame; the first frame only anchors the clock; later frames advance monotonically beyond the release point, inside the scroll range; the glide unhooks itself | none (C# end-to-end stops at "a glide is hooked") |
| `a_press_during_the_glide_stops_it_and_later_frames_move_nothing` | a new press unhooks the glide, later frames move nothing | none at window level |
| `a_lost_capture_cancels_the_drag_and_no_glide_follows` | `WM_CAPTURECHANGED` to another window ends the pan; later `WM_MOUSEMOVE`/`WM_LBUTTONUP` pan nothing and start no glide | `OnLostCapture` (pure twin exists; this is the window route) |
| `a_double_click_on_the_zoomed_image_returns_to_fit_and_a_single_click_does_not` | one click changes nothing; the second press 100 ms later within the slop returns to Fit and takes no capture | `ClickCounter` + `IsFitDoubleClick` (pure twins exist) |
| `a_second_click_after_the_double_click_time_is_not_a_double_click` | a press 501 ms later is a plain press (captured), no Fit | same |
| `a_second_click_beyond_the_double_click_slop_is_not_a_double_click` | a press outside the slop is a plain press, no Fit | same |

Not repeated: the pure controller maths (`pointer_pan.rs` 47, `pointer_click_zoom.rs` 16, `kinetic_pan_*`), click-to-zoom and arrow-impulse e2e (`shell_input_e2e.rs`), the 3 cl-misc hook tests.

## Real run (this machine, desktop session)

```
cargo test -p pr-platform-win --test shell_input_pan_glide_native -j 2 -- --include-ignored --test-threads=1
running 9 tests ... test result: ok. 9 passed; 0 failed; 0 ignored; finished in 15.14s
```

`cargo fmt -p pr-platform-win --check` clean; `cargo clippy -p pr-platform-win --test shell_input_pan_glide_native -- -D warnings` clean.

## Hand-applied mutants (each restored with `git checkout`)

| Mutant | Result |
|---|---|
| `dispatch.rs` `on_render_frame`: `is_kinetic_hooked()` negated | 2 FAILED (`a_released_drag_glides_...`, `a_lost_capture_...`), 7 passed |
| `dispatch.rs` `handle_capture_lost_impl`: body removed (no `on_lost_capture`) | 1 FAILED (`a_lost_capture_cancels_the_drag_and_no_glide_follows`) |
| `click_counter.rs`: time window `(0..=double_click_time_ms)` widened to `(0..=i32::MAX)` | 2 FAILED (`a_second_click_after_the_double_click_time_...`, `a_press_during_the_glide_...`) |
| `click_counter.rs`: slop `slop_width / 2` widened to `slop_width * 4` | 1 FAILED (`a_second_click_beyond_the_double_click_slop_...`) |

After restoring, the tree has only the two test files changed.

## Still a human job

Real mouse or pen hardware (physical drag feel, pen hover/contact, OS-generated `WM_LBUTTONDBLCLK` at the system double-click time, capture stolen by a real second window) is not covered by synthetic messages. The row stays Partial for that reason.

## Proposed PARITY change (not applied, by lane rule)

`X-pointer-click-pan`: add to the evidence "native `shell_input_pan_glide_native` 9 (adds glide after release with a fake frame clock, press stops glide, lost capture, double/slow/far click), 2026-10-31 st-pointer-pan" and drop "thiếu test native kéo -> thả -> quán tính" from the gap in `docs/PARITY.md` (gap list row of ST-GAPMAP2). Keep Partial (no attended hardware run). The binary is not yet in `.github/workflows/native-nightly.yml`; the orchestrator can add `shell_input_pan_glide_native` to the pr-platform-win list (green locally with `--include-ignored --test-threads=1`, no DLL staged).
