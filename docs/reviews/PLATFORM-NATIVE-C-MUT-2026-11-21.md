Status: evidence report (2026-11-21). Native mutation lane C for `pr-platform-win`: whole-file run of `shell_input/surface.rs`. Measured on one machine (Windows 11, interactive session 1, owner present), `mutants.sh --native --jobs 1`, `MUT_TIMEOUT=600`, own target dir.

# `pr-platform-win` native lane C: `shell_input/surface.rs`

## Result

126 identities in the whole-file run (5 hours): **120 caught, 4 unviable, 2 missed, 0 timeouts**. Both survivors were real gaps and are now killed (rerun: 3 caught, 1 unviable, 0 missed; the filters also matched two neighbours).

Test binaries (`--include-ignored --test-threads=1`, preflight 221 tests executed, exit 0): `shell_input_basics, shell_input_dispatch, shell_input_dispatch_keys, shell_input_dispatch_pointer, shell_input_drop, shell_input_e2e, shell_input_pan_glide_native, shell_input_surface, w8_inputdone_killers, w8_inputmut2_killers, w8_inputmut_killers, w8_surfmut4_killers`. Rerun: the same plus `nat_surface_forwarder_killers`.

| Mutant | Verdict | Killer |
|---|---|---|
| `surface.rs:305:9` `<impl ViewportControllerApi for ViewportController>::display_timing -> None` | real gap, killed (new) | `nat_surface_forwarder_killers::controller_api_display_timing_forwards_the_display_clock_of_the_window` (a controller with a display clock answers `Some(5, 6)` through the trait object and the clock sees the window handle) |
| `surface.rs:308:9` `<impl ViewportControllerApi for ViewportController>::pointer_position -> None` | real gap, killed (new) | `nat_surface_forwarder_killers::controller_api_pointer_position_forwards_the_cursor_in_viewport_dips` (dpi 2.0, client origin (100, 40), cursor at (120, 80) screen pixels gives `Some((10, 20))`) |

Why they survived: `w8_surfmut4_killers::controller_api_frame_hooks_timing_and_yield_forwarders_reach_the_controller` built a controller with no display clock and no cursor, so it only asserted `is_none()` for both, which a constant `None` also satisfies. The new tests use a controller that does answer.

## Notes

- Test only; no product change, no P-nn row.
- The run opens hidden windows on the owner's desktop; it needs an interactive session.
