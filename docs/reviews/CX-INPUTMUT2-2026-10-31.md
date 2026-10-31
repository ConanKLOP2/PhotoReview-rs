Status: Partial - requested native/handler shards evaluated; 105 surface mutants not run within the time-box.

# CX-INPUTMUT2 2026-10-31

Lane `feat/cx-inputmut2`, base `1ba2dacf77308c501cc6d5b003b7b6c97bb0017a`.
Continues [CX-INPUTMUT](CX-INPUTMUT-2026-10-31.md). All work is in this
worktree. The orchestrator owns commits and shared status files.

## Scope and isolation

Requested: native pure-helper shard 0/3 (17), handlers 0/3 and 2/3 (28),
surface 0/6 and 2/6 through 5/6 (105), and ShellStartup evidence. These
counts describe the requested listings, not executed coverage. Raw Win32/COM
glue is excluded. No production or unsafe code changes are planned.

All Cargo runs use `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=2`,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`,
`PHOTOREVIEW_RS_ISOLATE_CONFIG=1` and a worktree-local
`PHOTOREVIEW_RS_DATA_ROOT`. Normal target is `target`; mutation target is
`target-mut`. Only one mutation process runs at a time.

Oracle read only: C# `7eaee2a2`,
`src/PhotoReview.Shell.Win32/Input/InputNative.cs` and
`src/PhotoReview.Shell.Win32/Input/KeyboardMessageHandler.cs`. The negative
key-state predicate, independent modifier flags and scan-code byte mask
underpin the new inline tests.

## Preflight

`cargo test -p pr-platform-win --locked --test <binary> -- --include-ignored --test-threads=1`
was bounded to 900 seconds by its owning Python subprocess. Actual logs show
nonzero `running N tests` and success:

| Binary | Passed | Failed | Ignored |
|---|---:|---:|---:|
| shell_input_basics | 46 | 0 | 0 |
| shell_input_surface | 29 | 0 | 0 |
| shell_startup_cloak | 14 | 0 | 0 |
| shell_startup_placement | 19 | 0 | 0 |
| shell_startup_warmup | 5 | 0 | 0 |
| w8_inputmut2_killers | 2 | 0 | 0 |

The first five binaries ran in one bounded command with repeated `--test`.
The killers ran separately. Native warm graphics initialization passed.
These startup component tests do not by themselves prove ShellStartup composition.

## New independent probes

`crates/pr-platform-win/tests/w8_inputmut2_killers.rs` contains inline data,
no golden files, filesystem access, Win32 calls, unsafe code or sleeps:

- 32 pressed-key combinations, each with three up/toggle-only states and two
  negative states (192 cases), including either and both Windows keys.
- All 256 scan bytes, four outside-byte noise patterns, and unchanged non-Shift
  keys (7,168 calls), checking both Shift variants and byte masking.

## ShellStartup composition

One existing native scenario passed (parent and isolated child each report
`running 1 test`, exit 0):

```text
cargo test -p pr-shell --locked --test startup_run_native startup_with_jpeg_opens_window_then_draws_first_image_then_closes_cleanly -- --include-ignored --exact --test-threads=1
```

The owning subprocess had a 900-second bound; compilation took 1m 07s
and the parent test finished in 3.51 seconds. The child test finished
in 2.96 seconds. This is one distinct test, not two. It exercises the production
graph and actual ShellStartup run with fixture JPEGs, WARP, temporary data and
config, unique instance prefix, no process hooks, and `TempFolderRecycleBin`.
It asserts first image presented, `windowShown` before `firstPresent`, and clean
exit code zero. No decoder DLL staging was necessary. The other 22 tests in the
binary were filtered out. No real user config, C# app data or real Recycle Bin
was touched. This does not cover RAW startup, hardware rendering, or all startup
error/shutdown/forwarded-instance scenarios.

The initial PowerShell launcher reported exit 1 while both test summaries
passed and Python's `check=True` produced no exception. Its redirected native
stderr was rendered as PowerShell `NativeCommandError` records. To verify the
actual result independently, the already-built test executable was rerun with
the same exact test/filter and a 360-second subprocess bound. Both child and
parent passed, the parent finished in 1.42 seconds, and the explicit
`TEST_EXE_EXIT 0` plus owning launcher exit 0 were verified. This repeats the
same one scenario and is not counted as a second distinct test.

## Mutation results

Native pure-helper shard 0/3 completed with cargo-mutants exit 0:
**17 evaluated, 11 caught, 6 unviable, 0 missed, 0 timeouts** in approximately
11 minutes including copy/build. All 11 caught logs contain `running 46 tests`
and non-golden assertion failures. Golden-only catches: **0**.
The six unviable entries are `| -> &` and `| -> ^` at native.rs:63, :66 and :69:
compiler E0369, no `BitAnd` / `BitXor` implementation for `KeyModifiers`.
They are not counted caught or equivalent. No native helper survivor or
production bug was found.

Handlers shard 0/3 completed with cargo-mutants exit 0: **14 evaluated,
14 caught, 0 missed, 0 unviable, 0 timeouts**, approximately 10 minutes.
Every caught log has nonzero executed tests and a non-golden failure.
The Debug formatter mutant is caught by `keyboard_handler_ignores_other_messages`.
Golden-only catches: **0**. No survivor or production bug was found.

Handlers shard 2/3 completed with cargo-mutants exit 0: **14 evaluated,
14 caught, 0 missed, 0 unviable, 0 timeouts**. Every caught log has nonzero
executed tests and a non-golden failure. Golden-only catches: **0**.
No survivor or production bug was found.

| Requested selection | Listed | Evaluated | Caught | Missed | Unviable | Timeout | Not run |
|---|---:|---:|---:|---:|---:|---:|---:|
| native.rs pure helpers, 0/3 | 17 | 17 | 11 | 0 | 6 | 0 | 0 |
| handlers.rs, 0/3 | 14 | 14 | 14 | 0 | 0 | 0 | 0 |
| handlers.rs, 2/3 | 14 | 14 | 14 | 0 | 0 | 0 | 0 |
| surface.rs, 0/6 and 2/6..5/6 | 105 | 0 | 0 | 0 | 0 | 0 | 105 |
| Total distinct requested identities | 150 | 45 | 39 | 0 | 6 | 0 | 105 |

Evaluated includes compiler-unviable entries; 39 distinct viable mutants
actually reached tests. Surface entries have no verdict, not a passing or
equivalent disposition. The six unviable entries are not detected mutations.
There were no observed MISSED mutants requiring a new survivor-killer or
equivalence explanation. The new file instead adds independent edge probes;
its two representative killers-only rechecks are recorded below separately.

### Exact mutation command form

```text
cargo mutants -p pr-platform-win --baseline skip --timeout 300 --build-timeout 600 -j 1 --no-shuffle --gitignore=true --file **/shell_input/native.rs -F modifiers_from_key_states|signed_.*word|low_word|is_extended|is_key_repeat --shard 0/3 --cargo-arg=--test=shell_input_basics --cargo-test-arg=--test --cargo-test-arg=shell_input_basics --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1 --output <worktree>/mut-out-native0-final
```

The regex is passed as one quoted shell argument. Handler runs use the same
flags and binary, change the file to `**/shell_input/handlers.rs`, omit `-F`,
and select their explicit shard and output directory. The extra `--cargo-arg`
restricts the no-run build phase too; the requested libtest separator remains.

### Setup runs excluded from verdict totals

Two earlier owned sessions were interrupted with Ctrl+C and exited 1, with no
completed mutant verdicts. The first used the requested test flags alone;
its no-run phase built every integration binary in this very large crate.
The second restricted the build binary but copied `target-mut` into its temp
tree: cargo-mutants 27.1.0 defaults Git ignore OFF. The final command adds
`--gitignore=true`; its temp tree was inspected and contained neither
`target-mut` nor the mutation output directories. Only the completed final
run above counts as evaluated coverage. Neither aborted run produced test
assertion evidence or a timeout verdict. Process metadata enumeration via CIM
was denied, so the owned sessions were stopped through their session handles;
no process was selected or killed by executable name.

## Proposed PARITY rows (orchestrator only)

No edits to PARITY.md or task_on_progress.md. Keep all four rows Partial.

| Row | Proposed evidence update and remaining gap |
|---|---|
| X-input-native | Add this lane's exact helper shard 0/3 result: 17 evaluated, 11 caught, 6 compiler-unviable, no missed/timeouts/golden-only catches. Retain prior lanes' helper/OS-read results separately, including hardware-unobservable touchpad behavior and native CI gaps. Do not relabel unviable operators as detected/equivalent. No full-file mutation or hardware parity claim. |
| X-input-message-handlers | Add shards 0/3 and 2/3: 28/28 caught, no missed/unviable/timeouts/golden-only catches. All caught verdicts have non-golden assertion evidence. Retain the predecessor's separate shard 1/3 evidence and its warning about the older 13-entry overlap; do not derive an identity union from aggregate historical counts. |
| X-input-viewport-surface | Keep the predecessor's debt. This lane listed all five requested shards (0/6, 2/6, 3/6, 4/6, 5/6), 21 entries each, 105 total; none was mutation-tested. Existing surface binary passed 29/29. A passing baseline is not mutation coverage. |
| X-input-main-window | Add the single isolated native ShellStartup JPEG/first-present/clean-close scenario and startup component baselines. Full startup behavior, RAW/decoder matrix, startup failure/recovery/forwarded-instance and hardware scenarios remain unverified by this lane. No new main_window.rs mutation verdict. |

## Explicitly NOT RUN

- All 105 requested surface mutant entries: five listed shards, no execution.
  The earlier surface shard 1/6 is prior-lane evidence, not newly run here.
- Native helper shards 1/3 and 2/3, OS-read/native.rs mutants outside the pure
  filter, environment.rs, dispatch.rs and main_window.rs mutation in this lane.
- Raw Win32/COM glue mutation, precision touchpad hardware, attended cursor/
  touch input, full RAW corpus, hardware renderer or decoder matrix, full
  application parity, performance/latency measurements, and the other 22 tests
  filtered out of startup_run_native.
- Workspace fmt/clippy/tests, package clippy, CI and native nightly. Only the
  explicitly recorded test binaries and package fmt gate were executed.

## Final gates and cleanup

Final-source `cargo fmt -p pr-platform-win --check`: exit 0. The killers binary
was rerun with a 300-second bound: `running 2 tests`, both passed,
`CARGO_EXIT 0`, owning launcher exit 0. These two tests did not need native APIs.
Basics and killers were then run together unmutated with the same native
flags: 46 + 2 passed, `CARGO_EXIT 0`, owning launcher exit 0. Earlier baselines
ran under Python `check=True`; the PowerShell stderr wrapper issue described
above does not imply a failed test assertion.

Janitor dry-run exited 0, printed `TOTAL 0`, and reported that process metadata
was unavailable, suppressing worktree/target candidates. No `--apply` run was
used because it is not scoped to this lane's output directories. After all
owned mutation processes exited, cleanup of the explicitly requested owned
artifact paths was attempted; the rejection is recorded below.

Final document gate: `python -I tools/dev/doc-links.py --quiet` exited 0,
**621 files, 0 errors**, 180 PROPOSED items (not errors), 42 archive references
unchecked by policy. `git diff --check` exited 0. `git diff --name-only` was
empty: every original tracked file remained unchanged. Final deliverables are
the new killers file and this evidence file; root scratch files remain because
cleanup was rejected. Overall lane duration was approximately 60 minutes.

No git add, commit or push.

### Cleanup blocked by automatic approval review

All owned mutation sessions exited before cleanup (two aborted setup sessions:
exit 1; three main runs and the proof run: exit 0). A native PowerShell cleanup
command verified that each absolute target was a child of this worktree, rejected
unexpected reparse points, and used `Remove-Item -LiteralPath <ownedPath>
-Recurse -Force -ErrorAction Stop`. It also selected only lane-owned root files
named `inputmut2-*` for removal. Automatic approval review rejected process
creation with **`blocked by policy`**, before any part of the command executed.
No alternate deletion mechanism or retry was attempted.

Remaining for the orchestrator, all under this worktree:

- `target-mut`
- `mut-out-native0`, `mut-out-native0-focused`, `mut-out-native0-final`
- `mut-out-handlers0`, `mut-out-handlers2`, `mut-out-proof`
- Root-level `inputmut2-*` scratch logs, listings and audit script.

The two aborted cargo-mutants setup copies also remain in the OS temporary
directory: `cargo-mutants-cx-inputmut2-w4aLzW.tmp` and
`cargo-mutants-cx-inputmut2-pD6GQW.tmp`, under
`C:\Users\adminvti\AppData\Local\Temp`. They were observed after the successful
proof exited; no manual edits or cleanup outside the assigned worktree were
attempted. Normal `target` remains as well. Exact completed identities and
failing tests have been embedded in this report, so mutation outputs can be
removed without losing the reported evidence.

## Killers-only proof and final accounting

After both handler processes exited 0, an independent cargo-mutants process
ran the two selected identities against **only** `w8_inputmut2_killers`:

```text
cargo mutants -p pr-platform-win --baseline skip --timeout 300 --build-timeout 600 -j 1 --no-shuffle --gitignore=true --file **/shell_input/native.rs --file **/shell_input/handlers.rs -F native.rs:61:5:|handlers.rs:88:52:.*with.\^ --cargo-arg=--test=w8_inputmut2_killers --cargo-test-arg=--test --cargo-test-arg=w8_inputmut2_killers --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1 --output <worktree>/mut-out-proof
```

The regex was one quoted argument. Result: **2 evaluated, 2 caught, 0 missed,
0 unviable, 0 timeouts**, exit 0, 56 seconds. Each log shows `running 2 tests`
and exactly one of the new independent tests failing; no golden data was used.
The modifiers probe rejects native.rs:61:5's default return; the scan-code
probe rejects handlers.rs:88:52's `& -> ^`. The exact log evidence is embedded
below. These repeat two main-run identities.

Overall: **47 completed mutant evaluations, 45 distinct identities**;
**41 caught evaluations, 6 unviable, 0 missed, 0 timeouts, 0 golden-only catches**.
Among distinct identities: **39 caught, 6 unviable**. Do not add the proof
rechecks to distinct coverage. The 105 selected surface entries remain unrun.
No production or unsafe code was changed, and no observed survivor required
a bug fix or equivalence disposition.

## Exact completed outcomes: mut-out-handlers0

| Identity | Verdict | Failing non-golden test / compiler evidence |
|---|---|---|
| crates/pr-platform-win/src/shell_input/handlers.rs:85:9: replace KeyboardMessageHandler::resolve_virtual_key -> i32 with 0 | CaughtMutant | a_handled_key_swallows_its_key_up_and_char_but_an_unhandled_one_does_not, a_handled_shift_key_up_is_matched_by_the_raw_wparam, key_down_decodes_virtual_key_extended_bit_repeat_bit_and_message_time, resolve_virtual_key_takes_the_shift_side_from_the_scan_code, right_shift_maps_to_the_right_shift_key_id, sys_key_up_is_swallowed_like_key_up_and_an_unhandled_key_down_resets_the_swallow_state, the_no_handled_key_marker_never_matches_a_real_virtual_key |
| crates/pr-platform-win/src/shell_input/handlers.rs:85:9: replace KeyboardMessageHandler::resolve_virtual_key -> i32 with 1 | CaughtMutant | a_handled_key_swallows_its_key_up_and_char_but_an_unhandled_one_does_not, a_handled_shift_key_up_is_matched_by_the_raw_wparam, key_down_decodes_virtual_key_extended_bit_repeat_bit_and_message_time, resolve_virtual_key_takes_the_shift_side_from_the_scan_code, right_shift_maps_to_the_right_shift_key_id, sys_key_up_is_swallowed_like_key_up_and_an_unhandled_key_down_resets_the_swallow_state, the_no_handled_key_marker_never_matches_a_real_virtual_key |
| crates/pr-platform-win/src/shell_input/handlers.rs:85:9: replace KeyboardMessageHandler::resolve_virtual_key -> i32 with -1 | CaughtMutant | a_handled_key_swallows_its_key_up_and_char_but_an_unhandled_one_does_not, a_handled_shift_key_up_is_matched_by_the_raw_wparam, key_down_decodes_virtual_key_extended_bit_repeat_bit_and_message_time, resolve_virtual_key_takes_the_shift_side_from_the_scan_code, right_shift_maps_to_the_right_shift_key_id, sys_key_up_is_swallowed_like_key_up_and_an_unhandled_key_down_resets_the_swallow_state, the_no_handled_key_marker_never_matches_a_real_virtual_key |
| crates/pr-platform-win/src/shell_input/handlers.rs:85:24: replace != with == in KeyboardMessageHandler::resolve_virtual_key | CaughtMutant | a_handled_key_swallows_its_key_up_and_char_but_an_unhandled_one_does_not, a_handled_shift_key_up_is_matched_by_the_raw_wparam, key_down_decodes_virtual_key_extended_bit_repeat_bit_and_message_time, resolve_virtual_key_takes_the_shift_side_from_the_scan_code, right_shift_maps_to_the_right_shift_key_id, sys_key_up_is_swallowed_like_key_up_and_an_unhandled_key_down_resets_the_swallow_state, the_no_handled_key_marker_never_matches_a_real_virtual_key |
| crates/pr-platform-win/src/shell_input/handlers.rs:88:52: replace & with \| in KeyboardMessageHandler::resolve_virtual_key | CaughtMutant | a_handled_shift_key_up_is_matched_by_the_raw_wparam, resolve_virtual_key_takes_the_shift_side_from_the_scan_code, right_shift_maps_to_the_right_shift_key_id |
| crates/pr-platform-win/src/shell_input/handlers.rs:88:52: replace & with ^ in KeyboardMessageHandler::resolve_virtual_key | CaughtMutant | a_handled_shift_key_up_is_matched_by_the_raw_wparam, resolve_virtual_key_takes_the_shift_side_from_the_scan_code, right_shift_maps_to_the_right_shift_key_id |
| crates/pr-platform-win/src/shell_input/handlers.rs:88:45: replace >> with << in KeyboardMessageHandler::resolve_virtual_key | CaughtMutant | a_handled_shift_key_up_is_matched_by_the_raw_wparam, resolve_virtual_key_takes_the_shift_side_from_the_scan_code, right_shift_maps_to_the_right_shift_key_id |
| crates/pr-platform-win/src/shell_input/handlers.rs:89:22: replace == with != in KeyboardMessageHandler::resolve_virtual_key | CaughtMutant | a_handled_shift_key_up_is_matched_by_the_raw_wparam, resolve_virtual_key_takes_the_shift_side_from_the_scan_code, right_shift_maps_to_the_right_shift_key_id |
| crates/pr-platform-win/src/shell_input/handlers.rs:99:9: replace <impl fmt::Debug for KeyboardMessageHandler>::fmt -> fmt::Result with Ok(Default::default()) | CaughtMutant | keyboard_handler_ignores_other_messages |
| crates/pr-platform-win/src/shell_input/handlers.rs:108:9: replace <impl WindowMessageHandler for KeyboardMessageHandler>::try_handle -> Option<Lresult> with None | CaughtMutant | a_handled_key_swallows_its_key_up_and_char_but_an_unhandled_one_does_not, a_handled_shift_key_up_is_matched_by_the_raw_wparam, key_down_decodes_virtual_key_extended_bit_repeat_bit_and_message_time, right_shift_maps_to_the_right_shift_key_id, sys_key_down_sets_the_system_flag, sys_key_up_is_swallowed_like_key_up_and_an_unhandled_key_down_resets_the_swallow_state, the_no_handled_key_marker_never_matches_a_real_virtual_key |
| crates/pr-platform-win/src/shell_input/handlers.rs:108:9: replace <impl WindowMessageHandler for KeyboardMessageHandler>::try_handle -> Option<Lresult> with Some(Default::default()) | CaughtMutant | a_handled_key_swallows_its_key_up_and_char_but_an_unhandled_one_does_not, a_handled_shift_key_up_is_matched_by_the_raw_wparam, key_down_decodes_virtual_key_extended_bit_repeat_bit_and_message_time, keyboard_handler_ignores_other_messages, right_shift_maps_to_the_right_shift_key_id, sys_key_down_sets_the_system_flag, sys_key_up_is_swallowed_like_key_up_and_an_unhandled_key_down_resets_the_swallow_state, the_no_handled_key_marker_never_matches_a_real_virtual_key |
| crates/pr-platform-win/src/shell_input/handlers.rs:109:13: delete match arm WM_KEYDOWN \| WM_SYSKEYDOWN in <impl WindowMessageHandler for KeyboardMessageHandler>::try_handle | CaughtMutant | a_handled_key_swallows_its_key_up_and_char_but_an_unhandled_one_does_not, a_handled_shift_key_up_is_matched_by_the_raw_wparam, key_down_decodes_virtual_key_extended_bit_repeat_bit_and_message_time, right_shift_maps_to_the_right_shift_key_id, sys_key_down_sets_the_system_flag, sys_key_up_is_swallowed_like_key_up_and_an_unhandled_key_down_resets_the_swallow_state, the_no_handled_key_marker_never_matches_a_real_virtual_key |
| crates/pr-platform-win/src/shell_input/handlers.rs:125:13: delete match arm WM_KEYUP \| WM_SYSKEYUP in <impl WindowMessageHandler for KeyboardMessageHandler>::try_handle | CaughtMutant | a_handled_key_swallows_its_key_up_and_char_but_an_unhandled_one_does_not, a_handled_shift_key_up_is_matched_by_the_raw_wparam, sys_key_up_is_swallowed_like_key_up_and_an_unhandled_key_down_resets_the_swallow_state, the_no_handled_key_marker_never_matches_a_real_virtual_key |
| crates/pr-platform-win/src/shell_input/handlers.rs:133:13: delete match arm WM_CHAR \| WM_SYSCHAR in <impl WindowMessageHandler for KeyboardMessageHandler>::try_handle | CaughtMutant | a_handled_key_swallows_its_key_up_and_char_but_an_unhandled_one_does_not |

## Exact completed outcomes: mut-out-handlers2

| Identity | Verdict | Failing non-golden test / compiler evidence |
|---|---|---|
| crates/pr-platform-win/src/shell_input/handlers.rs:172:39: replace == with != in <impl WindowMessageHandler for PointerMessageHandler>::try_handle | CaughtMutant | x_buttons_pick_xbutton1_or_xbutton2_from_the_high_word_and_never_handle |
| crates/pr-platform-win/src/shell_input/handlers.rs:183:85: replace != with == in <impl WindowMessageHandler for PointerMessageHandler>::try_handle | CaughtMutant | left_up_move_and_capture_changed_never_handle, mouse_move_decodes_coordinates_and_the_left_button_flag |
| crates/pr-platform-win/src/shell_input/handlers.rs:183:71: replace & with \| in <impl WindowMessageHandler for PointerMessageHandler>::try_handle | CaughtMutant | mouse_move_decodes_coordinates_and_the_left_button_flag |
| crates/pr-platform-win/src/shell_input/handlers.rs:183:71: replace & with ^ in <impl WindowMessageHandler for PointerMessageHandler>::try_handle | CaughtMutant | left_up_move_and_capture_changed_never_handle, mouse_move_decodes_coordinates_and_the_left_button_flag |
| crates/pr-platform-win/src/shell_input/handlers.rs:214:9: replace <impl fmt::Debug for WheelMessageHandler>::fmt -> fmt::Result with Ok(Default::default()) | CaughtMutant | wheel_handler_ignores_other_messages |
| crates/pr-platform-win/src/shell_input/handlers.rs:220:9: replace <impl WindowMessageHandler for WheelMessageHandler>::try_handle -> Option<Lresult> with None | CaughtMutant | wheel_handler_converts_the_screen_point_through_the_port_and_reads_delta_and_ctrl, wheel_handler_reads_sign_extended_parameters |
| crates/pr-platform-win/src/shell_input/handlers.rs:220:9: replace <impl WindowMessageHandler for WheelMessageHandler>::try_handle -> Option<Lresult> with Some(Default::default()) | CaughtMutant | wheel_handler_converts_the_screen_point_through_the_port_and_reads_delta_and_ctrl, wheel_handler_ignores_other_messages, wheel_handler_reads_sign_extended_parameters |
| crates/pr-platform-win/src/shell_input/handlers.rs:220:41: replace && with \|\| in <impl WindowMessageHandler for WheelMessageHandler>::try_handle | CaughtMutant | wheel_handler_converts_the_screen_point_through_the_port_and_reads_delta_and_ctrl, wheel_handler_reads_sign_extended_parameters |
| crates/pr-platform-win/src/shell_input/handlers.rs:220:24: replace != with == in <impl WindowMessageHandler for WheelMessageHandler>::try_handle | CaughtMutant | wheel_handler_converts_the_screen_point_through_the_port_and_reads_delta_and_ctrl, wheel_handler_ignores_other_messages, wheel_handler_reads_sign_extended_parameters |
| crates/pr-platform-win/src/shell_input/handlers.rs:220:56: replace != with == in <impl WindowMessageHandler for WheelMessageHandler>::try_handle | CaughtMutant | wheel_handler_converts_the_screen_point_through_the_port_and_reads_delta_and_ctrl, wheel_handler_ignores_other_messages |
| crates/pr-platform-win/src/shell_input/handlers.rs:227:57: replace != with == in <impl WindowMessageHandler for WheelMessageHandler>::try_handle | CaughtMutant | wheel_handler_converts_the_screen_point_through_the_port_and_reads_delta_and_ctrl, wheel_handler_reads_sign_extended_parameters |
| crates/pr-platform-win/src/shell_input/handlers.rs:227:43: replace & with \| in <impl WindowMessageHandler for WheelMessageHandler>::try_handle | CaughtMutant | wheel_handler_converts_the_screen_point_through_the_port_and_reads_delta_and_ctrl |
| crates/pr-platform-win/src/shell_input/handlers.rs:227:43: replace & with ^ in <impl WindowMessageHandler for WheelMessageHandler>::try_handle | CaughtMutant | wheel_handler_converts_the_screen_point_through_the_port_and_reads_delta_and_ctrl |
| crates/pr-platform-win/src/shell_input/handlers.rs:228:38: replace == with != in <impl WindowMessageHandler for WheelMessageHandler>::try_handle | CaughtMutant | wheel_handler_converts_the_screen_point_through_the_port_and_reads_delta_and_ctrl, wheel_handler_reads_sign_extended_parameters |

## Exact completed outcomes: mut-out-native0-final

| Identity | Verdict | Failing non-golden test / compiler evidence |
|---|---|---|
| crates/pr-platform-win/src/shell_input/native.rs:61:5: replace modifiers_from_key_states -> KeyModifiers with Default::default() | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag |
| crates/pr-platform-win/src/shell_input/native.rs:62:13: replace < with == in modifiers_from_key_states | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag, native_tests::read_modifiers_no_key_down_is_none |
| crates/pr-platform-win/src/shell_input/native.rs:62:13: replace < with > in modifiers_from_key_states | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag |
| crates/pr-platform-win/src/shell_input/native.rs:62:13: replace < with <= in modifiers_from_key_states | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag, native_tests::read_modifiers_no_key_down_is_none |
| crates/pr-platform-win/src/shell_input/native.rs:63:31: replace \| with & in modifiers_from_key_states | Unviable | E0369, no implementation for `KeyModifiers & KeyModifiers` |
| crates/pr-platform-win/src/shell_input/native.rs:63:31: replace \| with ^ in modifiers_from_key_states | Unviable | E0369, no implementation for `KeyModifiers ^ KeyModifiers` |
| crates/pr-platform-win/src/shell_input/native.rs:65:12: replace < with == in modifiers_from_key_states | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag, native_tests::read_modifiers_no_key_down_is_none |
| crates/pr-platform-win/src/shell_input/native.rs:65:12: replace < with > in modifiers_from_key_states | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag |
| crates/pr-platform-win/src/shell_input/native.rs:65:12: replace < with <= in modifiers_from_key_states | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag, native_tests::read_modifiers_no_key_down_is_none |
| crates/pr-platform-win/src/shell_input/native.rs:66:31: replace \| with & in modifiers_from_key_states | Unviable | E0369, no implementation for `KeyModifiers & KeyModifiers` |
| crates/pr-platform-win/src/shell_input/native.rs:66:31: replace \| with ^ in modifiers_from_key_states | Unviable | E0369, no implementation for `KeyModifiers ^ KeyModifiers` |
| crates/pr-platform-win/src/shell_input/native.rs:68:14: replace < with == in modifiers_from_key_states | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag, native_tests::read_modifiers_no_key_down_is_none |
| crates/pr-platform-win/src/shell_input/native.rs:68:14: replace < with > in modifiers_from_key_states | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag |
| crates/pr-platform-win/src/shell_input/native.rs:68:14: replace < with <= in modifiers_from_key_states | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag, native_tests::read_modifiers_no_key_down_is_none |
| crates/pr-platform-win/src/shell_input/native.rs:69:31: replace \| with & in modifiers_from_key_states | Unviable | E0369, no implementation for `KeyModifiers & KeyModifiers` |
| crates/pr-platform-win/src/shell_input/native.rs:69:31: replace \| with ^ in modifiers_from_key_states | Unviable | E0369, no implementation for `KeyModifiers ^ KeyModifiers` |
| crates/pr-platform-win/src/shell_input/native.rs:71:21: replace \|\| with && in modifiers_from_key_states | CaughtMutant | modifiers_from_key_states_maps_each_key_to_its_flag, native_tests::read_modifiers_maps_each_key_state_to_its_flag |

## Exact completed outcomes: mut-out-proof

| Identity | Verdict | Failing non-golden test / compiler evidence |
|---|---|---|
| crates/pr-platform-win/src/shell_input/handlers.rs:88:52: replace & with ^ in KeyboardMessageHandler::resolve_virtual_key | CaughtMutant | resolve_shift_ignores_every_bit_outside_the_scan_code_byte |
| crates/pr-platform-win/src/shell_input/native.rs:61:5: replace modifiers_from_key_states -> KeyModifiers with Default::default() | CaughtMutant | modifiers_distinguish_all_pressed_combinations_from_zero_and_toggle_states |
