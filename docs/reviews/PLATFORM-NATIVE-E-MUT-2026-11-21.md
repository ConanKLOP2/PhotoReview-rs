Status: evidence report (2026-11-21). Native mutation lane E for `pr-platform-win`: whole-file verdict of `shell_input/dispatch.rs`. Measured on one machine (Windows 11, interactive session 1, owner present), `mutants.sh --native --jobs 1`, `MUT_TIMEOUT=600`, own target dir.

# `pr-platform-win` native lane E: `shell_input/dispatch.rs`

## Result

`cargo mutants --list` found 196 identities. The first run stopped by itself after 113 (`cargo-mutants` exit code 1 while building the next mutant, cause not found; no mutant was lost, the log ends inside a build), so the rest was run as a second part with a line filter (lines 795 and up; 85 identities, 2 of them already done by part 1).

| Part | Tested | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|
| 1 (lines up to 821) | 113 | 78 | 28 | 7 | 0 |
| 2 (lines 795 and up) | 85 | 63 | 22 | 0 | 0 |

**50 survivors, all decided: 49 killed by 19 new tests, 1 equivalent by argument.** Rerun of every survivor function with the new tests (`-F` on the function names, 78 identities): 75 caught, 1 unviable, 2 missed; one of the two (`VersionAdapter::next -> 0`) needed one more test and then 4 of 4 `VersionAdapter` identities were caught; the other is the equivalent one.

Test binaries (`--include-ignored --test-threads=1`): the `shell_input_*` and `w8_input*` set of lane C (preflight 224 tests, exit 0); rerun with `nat_surface_forwarder_killers` and `nat_dispatch_killers` added (242 tests, exit 0).

## Classification of the 50 survivors

| Mutants | Verdict | Killer (all in `tests/nat_dispatch_killers.rs`) |
|---|---|---|
| 141:9 `Debug for ShellInputDeps` | killed | `shell_input_deps_debug_names_the_type_and_elides_the_fields` |
| 215:5 `unregister_input -> ()` and 656:9 `Drop for InputCore -> ()` | killed (seam P-431) | `dispose_removes_the_registry_entry`, `dropping_an_undisposed_input_removes_its_registry_entry` through `registered_input_count()` |
| 276:9 `SurfaceFitAdapter::viewport_size` (9 constant tuples), 280 `is_loaded -> true`, 284 `update_layout`, 288 `update_fit_size`, 292 `capture`, 296 `scroll_home` | killed | `apply_fit_runs_the_pass_in_order_with_the_surface_size`, `a_fit_on_an_unloaded_surface_stops_after_the_loaded_check`, `every_fit_gets_a_fresh_version_that_stays_current` (the fixture's `FitSpy` records the surface calls and scripts `viewport_size`) |
| 351:9 `VersionAdapter::next -> 0`, 356:9 `current -> 1` | killed | `every_fit_gets_a_fresh_version_that_stays_current` (`current`), `a_newer_fit_supersedes_the_pending_one` (`next`: a constant version never supersedes) |
| 509:9 `rebuild_shortcuts`, 518:9 `note_current_index_changed`, 543:9 `pointer_press_tunnel` | killed | `rebuild_shortcuts_binds_the_keys_of_the_given_settings`, `note_current_index_changed_stops_a_glide_only_for_a_new_index`, `pointer_press_tunnel_stops_a_glide` |
| 620:43 `+` -> `-`/`*`, 630:47 `!=` -> `==` | killed | `pointer_activity_subscriptions_get_distinct_ids_and_unsubscribe_only_themselves` |
| 686:53, 686:91 `is_inside_viewport` `<` -> `<=` | killed | `a_horizontal_wheel_is_taken_inside_the_viewport_and_not_on_its_far_edges` |
| 756:12 `run_pointer` `delete !` | killed | `a_zoom_invalidates_the_window_and_a_plain_move_does_not` |
| 1030:40, 1030:43, 1033:84 `handle_pointer_move_impl` | killed | `a_captured_pan_continues_over_an_overlay_element`, `a_pan_move_without_the_capture_is_routed_over_the_image_unless_compare_is_visible` |
| 1045:43, 1045:46, 1045:75, 1045:78 `handle_left_button_up_impl` | killed | `a_release_without_the_capture_reaches_the_controller_only_over_the_bare_image` (truth table: over image/overlay/compare; `cancel_pan` resets the pan cursor, which shows that the controller ran) |
| 1133, 1137, 1143, 1147, 1151, 1155, 1169, 1173 (`InputCore` port impls: `handle_key_down`, `message_time`, `handle_pointer_down`, `handle_pointer_move`, `handle_left_button_up`, `handle_capture_lost`, both wheel handlers) | killed | `the_key_port_answers_for_bound_and_unbound_keys_and_reads_the_message_time`, `the_pointer_port_presses_pans_releases_and_loses_the_capture`, `the_wheel_port_handles_the_wheel_over_the_viewport_only`; the window reaches the core through `WeakPorts`, so only `key_port()`, `pointer_port()` and `wheel_port()` (used by the main window) go through these impls |
| 1063:39 `handle_capture_lost_impl`: `in_pointer_call` OR `state.try_borrow_mut().is_err()`, `\|\|` -> `&&` | equivalent (code argument) | Every `state.borrow_mut()` that spans a call into the surface or window is taken after `BusyScope::enter` (`run_pointer`, `with_pointer_surface`, `replay_capture_lost`, `apply_fit_impl`, `resume_fit`), so while the state is borrowed `in_pointer_call` is true, and `in_pointer_call` is only true while it is borrowed. The two operands never differ at a point where a `WM_CAPTURECHANGED` can arrive (the short borrows of `clicks` and `take_viewer_update` run no window code). OR and AND give the same answer in every reachable state; the second operand is a defensive guard against a panic. |

Why they survived: the existing tests drive the real window messages and only assert end states (`is_fit`, handled flags); they never looked at which `FitSurface` calls the driver made, never held the capture over an overlay, never released over an overlay or with the compare view visible, and never called the public ports.

## Product and fixture changes

- Seam (read only, no behaviour change, P-431): `#[doc(hidden)] pub fn registered_input_count() -> usize` in `shell_input/dispatch.rs`, the size of the calling thread's `INPUTS` registry.
- Test fixture `tests/common/shell_input_dispatch_fixture.rs`: `RigSurface` gets `fit_spy: Rc<FitSpy>` (call log of the `FitSurface` methods, a scripted `viewport_size`, and two switches that skip the real `update_fit_size` and `update_layout`). Without the switches the real layout overwrites what `reset_fit` was given and the size is not observable. The default is the old behaviour.

## Notes

- The run opens hidden windows on the owner's desktop; it needs an interactive session.
- This is a whole-file verdict for `dispatch.rs` (part 1 + part 2); the survivor rerun covered the survivor functions and their neighbours, not the whole file again.
