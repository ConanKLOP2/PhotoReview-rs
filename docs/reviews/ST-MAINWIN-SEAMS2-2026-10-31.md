# ST-MAINWIN-SEAMS2 (2026-10-31)

Lane `st-mainwin-seams2`: native killers for the open survivors of `X-main-window-windowing` (PARITY L633), `X-main-window-lifecycle` (L630) and the `WM_DPICHANGED` gap of
`X-host-shell-window-messages` (L602) / `X-host-wndproc-shell-window` (L601). Source of the brief: section 4.3 and 6 of `ST-GAPMAP2-2026-10-31.md`; the "not covered" list of
`ST-MAINWIN-TESTS-2026-10-31.md`.

## Production change (one accessor, no behaviour change)

`MainWindow::display_change_handler() -> Option<Rc<DisplayChangeHandler>>` in `host/main_window/windowing.rs`: returns the `WM_DISPLAYCHANGE` observer that `register_windowing`
put on the shell window. The handler was unreachable from a test binary (its constructor and owner are private); same pattern as `prefetch_host()` of PR #469. No new `unsafe`.

## Tests (11, all `#[ignore = "native: needs desktop"]`, `crates/pr-platform-win/tests/w8_mainwin_seams2.rs`)

Run: `cargo test -p pr-platform-win --test w8_mainwin_seams2 -j 2 -- --include-ignored --test-threads=1`: 11 passed (0.5 to 1.1 s). Hidden windows only, fake windowing seams,
no sleeps (bounded waits on a `Render`-priority sentinel or the frame clock). Not added to `native-nightly.yml` (brief did not ask).

| Item | Tests | Mutants killed (hand applied, then restored) |
|---|---|---|
| `DisplayChangeHandler` | `display_change_handler_never_handles_and_refits_for_wm_displaychange_only`, `display_change_handler_after_the_window_is_gone_does_nothing` | `try_handle ==` -> `!=`; `try_handle` returns `Some(0)`; `Debug::fmt -> Ok(())` |
| `on_viewer_fullscreen_changed` | `a_viewer_fullscreen_change_on_a_destroyed_window_touches_nothing` | `windowing_disposed || is_destroyed` -> `&&` |
| `update_fit_size_if_alive` | `a_current_image_change_in_fit_posts_a_render_job_that_refits_the_decode_box` (+ control `..._outside_fit_does_not_refit`) | body -> `()` |
| F11 placement pair | `closing_in_f11_saves_the_state_and_the_rect_from_before_f11`, `..._from_a_maximised_window_saves_maximized_...`, `closing_after_leaving_f11_saves_no_f11_pair` | `save_placement` `restore_bounds` -> `None`; `restore_state` -> `None` |
| `WM_DPICHANGED` | `dpi_changed_takes_the_new_dpi_from_the_low_word_of_wparam`, `dpi_changed_handlers_see_the_change_applied_and_may_override_the_default_result`, `dpi_changed_invalidates_even_when_the_rectangle_does_not_change` | `low_word(wparam)` -> `high_word`; delete `self.invalidate()` in `apply_dpi_change` |

Hand mutation results (real runs of the changed test binary, one mutant at a time, source restored from memory afterwards; no cargo-mutants):

| Mutant | Result |
|---|---|
| `try_handle ==` -> `!=` | FAILED (the display-change test) |
| `try_handle` -> `Some(0)` | FAILED (6 of 11) |
| `on_viewer_fullscreen_changed ||` -> `&&` | FAILED (the destroyed-window test) |
| `update_fit_size_if_alive -> ()` | FAILED (the Fit refit test) |
| `save_placement` `restore_bounds -> None` / `restore_state -> None` | FAILED, 2 of 11 each (Normal and Maximized pair tests) |
| `Debug for DisplayChangeHandler -> Ok(())` | FAILED |
| `low_word -> high_word` in the `WM_DPICHANGED` arm | FAILED |
| delete `self.invalidate()` in `apply_dpi_change` | FAILED |
| `is_minimized &&` -> `||`, `is_minimized -> false`, `restore !=` -> `==`, `restore -> ()` (minimise x4, killed by the EXISTING `w8_mainwin_frame_tests` test of PR #469) | each FAILED `a_minimised_window_draws_nothing_and_restore_brings_the_frame_back` (14 of 15 pass, 1 fails) |
| close waiter `!disposed && !destroyed` -> `||` | SURVIVED the whole `host_main_window_lifecycle` binary (24 passed): see below |

## Items not killable, and why

- Close waiter `&&` -> `||` (`close_when_file_action_done_waiter`): equivalent in practice, so NOT tested. The two operands only differ when exactly one holds. (a) not disposed,
  window destroyed: the mutant calls `ShellWindow::close()`, which returns at once for a destroyed window (`create.rs` `if destroyed { return }`). (b) disposed, window alive:
  unreachable, `dispose_inner` unregisters the window id (so a posted waiter is dropped by `post_to_window`) and destroys the window in the same call, with no pump in between.
  Verified by running the mutant against the whole `host_main_window_lifecycle` binary: survives.
- `update_fit_size_if_alive` with `delete !` (runs when disposed): same reason as (b), the posted job is dropped once the window is unregistered. Equivalent.
- `on_viewer_fullscreen_changed` with `windowing_disposed` true and the window alive: unreachable (dispose destroys the window; `apply_changes` ignores a disposed window first).
  The destroyed-window half of the guard is what the test pins.
- `WM_DPICHANGED => 0` default arm of `process_message` (deleting it falls to `DefWindowProc`, which also returns 0 here): equivalent for the result; the new test pins the 0 result
  and the "handler wins" rule, not the arm itself.
- Real maximised/minimised F11 on a visible window, multi-monitor unplug, a real DPI change by moving between monitors: a person with a second monitor (unchanged human item).

## Proposed PARITY changes (not applied)

- `X-main-window-windowing` (L633): remaining "3 MISSED needing a native seam" are now killed (`try_handle`, `fmt`, `on_viewer_fullscreen_changed ||`); stay Partial (72 mutants of
  the file still NOT run; real multi-monitor is human).
- `X-main-window-lifecycle` (L630): minimise x4 are killed (PR #469 test, re-proved here by hand mutation); `update_fit_size_if_alive` and the F11 placement pair are now pinned;
  the close-waiter `&&` -> `||` is documented equivalent; stay Partial.
- `X-host-shell-window-messages` (L602): `WM_DPICHANGED` low-word/high-word, handler order and result, and the invalidate are now pinned; `messages.rs` is still not exhausted
  (21 NOT RUN in CX-HOSTREST), so the row stays Partial.

## Build

`cargo test -p pr-platform-win --test w8_mainwin_seams2` and the two reused binaries (`w8_mainwin_frame_tests`, `host_main_window_lifecycle`) only; `cargo fmt -p pr-platform-win --check`
clean; one `cargo clippy -p pr-platform-win --all-targets`: one `type_complexity` warning in the new test (fixed with a type alias afterwards, not re-run to honour the
one-clippy limit, the alias is a plain tuple alias). No `unsafe` added to `src/`; the three `send_message` calls in the test have `// SAFETY:` comments like the shared fixtures.
