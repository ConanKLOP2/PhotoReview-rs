Status: evidence report (2026-11-21). Native mutation lane D for `pr-platform-win`: whole-file run of `shell_input/native.rs`. Measured on one machine (Windows 11, interactive session 1, owner present), `mutants.sh --native --jobs 1`, `MUT_TIMEOUT=600`, own target dir.

# `pr-platform-win` native lane D: `shell_input/native.rs`

## Result

92 identities in the whole-file run (2 hours): **81 caught, 8 unviable, 3 missed, 0 timeouts**. The 3 survivors are decided: 1 equivalent by argument, 1 killed through a pure test seam, 1 unobservable without hardware (P-430).

Test binaries: the `shell_input_*` and `w8_input*`/`w8_surfmut4` set of lane C (`--include-ignored --test-threads=1`, preflight 221 tests executed, exit 0).

| Mutant | Verdict | Evidence |
|---|---|---|
| `native.rs:55:33` `read_modifiers`: `left_win < 0` -> `>` | equivalent (code argument) | `right_win` only feeds `modifiers_from_key_states`, which sets `WINDOWS` when `left_win < 0 \|\| right_win < 0`. When `left_win < 0` the flag is set whatever `right_win` is, so reading `VK_RWIN` as well (the mutant) or not (the original) gives the same modifiers; `GetKeyState` has no side effect. `left_win > 0` never happens for a non-toggle key, so the other direction is the same too. The pure `modifiers_from_key_states` is fully pinned by existing tests. |
| `native.rs:148:25` `current_wheel_device`: match guard `== IMDT_TOUCHPAD` -> `false` | real gap, killed (new) through seam P-430 | `nat_surface_forwarder_killers::wheel_device_from_device_type_is_touchpad_only_for_the_touchpad_type` on the new pure `wheel_device_from_device_type(Option<i32>)`; the guard now lives in the seam, whose guard mutant is caught |
| `native.rs:147:5` `current_wheel_device -> Default::default()` (`Unknown`) | unobservable without hardware (P-430) | The result differs from the original only when the message being dispatched comes from a precision touchpad (`IMDT_TOUCHPAD`); a test has no way to make this machine's input stack report one (a posted thread message reports a non-touchpad source, see `w8_inputdone_killers::a_non_pointer_message_source_is_not_a_touchpad`; synthetic touchpad injection was not tried). Left open, recorded in P-430 |

## Rerun after the seam

Filter `wheel_device`, same binaries plus `nat_surface_forwarder_killers`: 5 identities, 4 caught, 1 missed (the 147:5 above); the seam's own mutants (`-> Default::default()` and the guard) are caught.

## Product change (pure seam, no behaviour change, P-430)

`wheel_device_from_device_type(Option<i32>) -> WheelDeviceHint` (`pub`, `shell_input/native.rs`): the match of `current_wheel_device` moved into it; `current_wheel_device` passes `get_current_input_message_source().map(|s| s.deviceType.0)`. The decision for every input is unchanged.

## Notes

- The run opens hidden windows on the owner's desktop; it needs an interactive session.
