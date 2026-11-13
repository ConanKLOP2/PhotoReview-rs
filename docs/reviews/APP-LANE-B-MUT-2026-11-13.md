Status: evidence report (2026-11-13). Confirmation run for the `pr-app` lane B survivors (input gestures, overlay panels, info overlay view model) against the whole `pr-app` suite, after adding killer tests. Measured on one machine (Windows 11), `--jobs 1`, `MUT_TIMEOUT=900`.

# `pr-app` lane B killers: confirmation run

The first lane B run used selected test binaries only. Its survivors were rerun against the whole crate suite, killers were written for the real gaps, and this run (144 identities, whole suite, preflight 3271 tests) is the confirmation.

## Result

| Outcome | Count | Notes |
|---|---|---|
| Caught | 133 | a non-golden test fails in each kill |
| Unviable | 2 | do not compile |
| Equivalent by code argument | 7 | argument per mutant below, no run proof |
| Timeout | 2 | `root_length` `+=` -> `*=` (see below); detected by non-termination, not by an assertion |
| Missed with a real gap | 0 | |

## New tests and seams

- `crates/pr-app/tests/w8_lane_b_input_killers.rs`, `w8_lane_b_overlay_killers.rs`, `w8_lane_b_info_overlay_killers.rs`.
- Test seams (behaviour neutral, P-407): `key_mapping::precheck_rejects`, `touchpad_gestures::pan_offset_delta_scaled`, `info_overlay::is_device_for_tests`, `info_overlay::root_length_for_tests`.

## Equivalent by code argument (7)

| Mutant | Argument |
|---|---|
| `mouse_gestures.rs:82:37` `<` -> `<=` in `WheelGestureInterpreter::handle` | line 82 is `let down = self.accumulated < 0`; it is reached only when `abs(accumulated) >= abs(NOTCH_DELTA)` (nonzero), so `accumulated == 0` never occurs |
| `touchpad_gestures.rs:178:37` `<` -> `<=` in `TouchpadSwipeNavigator::handle` | line 178 is `let down = self.accumulated < 0`; reached only when `abs(accumulated) >= abs(required)` with `required >= 1` after the early return, so zero never occurs |
| `element.rs:32:17` `<` -> `<=` in `net_min` | line 32 is the `a < b` branch; equality is handled by the earlier `a == b` branch, so `<=` selects the same result |
| `capture_badge.rs:76:17`, `folder_info.rs:77:17`, `zoom_indicator.rs:78:13` delete field `font_family` | the struct update falls back to `TextStyle::default().font_family`, which is `"Segoe UI"` and equals the explicit value; valid only while that default is unchanged |
| `info_overlay.rs:80:9` `InfoOverlayChanges::none` -> `Default::default()` | derived `Default` is `Self(0)`, the same value |

## The two timeouts

`info_overlay.rs:449:23` and `:457:19` (`+=` -> `*=` in the separator scans of `root_length`) loop for ever on a path with a separator. The killer tests call `root_length` and `display_name` on worker threads with a 20 s bound, so the new tests fail fast; the 900 s timeout therefore comes from another test binary reaching the same function unbounded (not isolated here). The mutants are detected by non-termination, which a CI run would also report, but not by a failing assertion in the killer file.
