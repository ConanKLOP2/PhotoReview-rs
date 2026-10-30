# CL-INPUTDONE 2026-10-30: mutation audit of four rows that said Done

Status: partial. One row keeps Done with full numbers; three rows are lowered to Partial because the mutation run is not complete (sampled) or a Win32 branch cannot be observed. Lane `cl-inputdone` (branch `feat/cl-inputdone`). Source of the question: `docs/reviews/PARITY-GAP-MAP-2026-10-30.md` (lane cl-audit2) (rows `X-input-native`, `X-input-message-handlers`, `X-input-viewport-surface`, `X-input-drop-target`). Continues [CL-INPUT2](CL-INPUT2-2026-10-30.md).

Killers file: `crates/pr-platform-win/tests/w8_inputdone_killers.rs` (8 tests, 3 of them `#[ignore = "native: needs desktop"]` and only read process state; no window, no sleeps; 20 repeated runs green). No production code changed.

Command: `cargo mutants -p pr-platform-win --baseline skip --timeout 90 --build-timeout 900 -j 1 --file "**/shell_input/<f>.rs" [-F|--shard ...] --output <worktree>/mut-out-<group> --cargo-arg=--test=<binary>... --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`, own `target-mut`, `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, `stage-native.ps1` run first. Native `#[ignore]` tests of `shell_input_basics` and `shell_input_drop` were therefore part of every run.

Pitfall found (important for other lanes): passing the test arguments after a bare `--` of the cargo-mutants command line (`... -- --include-ignored --test-threads=1`) makes `cargo test` fail with `unexpected argument '--include-ignored'` for EVERY mutant, so every mutant is reported "caught". The first three hours of runs of this lane were discarded for that reason. The arguments must be `--cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`. Check one `mutants.out/log/*.log` for `test result:` before trusting a "caught" count.

## Results (valid runs only)

| File / scope | Test binaries | Listed | Tested | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|---|---|
| `native.rs` OS-read half (`-F drag_metrics\|double_click\|current_wheel\|cursor_screen\|tick_count\|message_time\|read_modifiers\|dpi_of`) | `w8_inputdone_killers`, `shell_input_basics` (+ ignored) | 41 | 41 | 38 | 3 | 0 | 0 |
| `native.rs` pure helpers, shard 1/3 of `modifiers_from_key_states\|signed_*\|low_word\|is_extended\|is_key_repeat` | `shell_input_basics` | 51 | 17 | 15 | 0 | 2 | 0 |
| `handlers.rs`, shard 1/3 | `shell_input_basics` | 42 | 14 | 14 | 0 | 0 | 0 |
| `drop_target.rs`, whole file | `shell_input_drop` (+ ignored), `w8_inputdone_killers` | 47 | 47 | 39 | 7 | 1 | 0 |
| the 6 killable `drop_target.rs` survivors, killers binary alone | `w8_inputdone_killers` | 14 | 14 | 13 | 1 | 0 | 0 |
| `surface.rs`, shard 1/6 | `shell_input_surface` | 126 | 21 | 21 | 0 | 0 | 0 |

Rerun of the killed mutants with the killers binary alone (the 14 rows above are `-F hdrop_format|parse_drop_files|DropCallbacks|DropRegisterError`, 13 caught; the 1 missed is `ShellDropTarget::register -> Ok(())`, which only the native `register_*` tests of `shell_input_drop` kill and which the full `drop_target.rs` run did kill). `Debug for ShellDropTarget` was rerun on its own: 1 of 1 caught.

## Survivors of the first pass and what happened

| Mutant | Verdict |
|---|---|
| `drop_target.rs:41:17` `delete -` in `hdrop_format` (`lindex: -1` -> `1`) | Killed. The old note said the fake data object ignores `lindex`; the killers file has a strict `IDataObject` that answers `DV_E_FORMATETC` unless the request is exactly CF_HDROP, no device, content aspect, `lindex -1`, `TYMED_HGLOBAL`. Not a native gap any more. |
| `drop_target.rs:128:20` `<` -> `<=` and `<` -> `==` in `parse_drop_files` | Killed (the old "equivalent" argument held only for `pFiles == 20`): a block of exactly 20 bytes whose `pFiles` points into the header still has a list. |
| `drop_target.rs:196:9`, `211:9`, `407:9` `Debug`/`Display` `fmt -> Ok(Default::default())` | Killed (`drop_callbacks_debug_and_register_error_display_say_what_they_are`, `shell_drop_target_debug_names_the_type_and_the_unregistered_hwnd`). |
| `drop_target.rs:95:18` `\|\|` -> `&&` in `files` (`size == 0 \|\| ptr.is_null()`) | Equivalent. `kernel32::global_lock` returns `Err` for a null pointer, so `ptr.is_null()` is never true there; with `size == 0` and a non-null pointer the mutant parses an empty slice, which `parse_drop_files` answers with the empty list. |
| `native.rs:55:33` `<` -> `>` in `read_modifiers` (`if left_win < 0`) | Equivalent. The only effect is that the right Windows key is also read when the left one is down; both feed `left_win < 0 \|\| right_win < 0`, so the flag is the same. |
| `native.rs:147:5` `current_wheel_device -> Default::default()` and `native.rs:148:25` match guard `deviceType == IMDT_TOUCHPAD` -> `false` | NOT observable here. `Default` is `Unknown`, which is the answer on every machine without a precision touchpad; the `Touchpad` branch needs a message produced by real touchpad hardware (`GetCurrentInputMessageSource` cannot be faked from user mode). The `==` -> `!=` and guard -> `true` mutants are killed by `a_non_pointer_message_source_is_not_a_touchpad`. A pure `wheel_device_from_type(i32)` seam would remove this survivor; it is a production change, left for a lane that is allowed to make it. |
| `native.rs` `drag_metrics_pixels` / `double_click_size_pixels` `(1, 1)` etc. (the 2 missed of 2026-10-23) | Killed: `drag_metrics_are_exactly_the_system_metrics_at_each_dpi`, `double_click_size_is_exactly_the_system_metrics_at_each_dpi` compare with `GetSystemMetricsForDpi` at four DPIs and require a value above 1. A `cx` / `cy` swap is not observable when the user's metrics are square (they are on this machine). |
| `native.rs` `cursor_screen_pixel -> Some(Default::default())` / `None` | Killed by `cursor_screen_pixel_equals_get_cursor_pos_when_the_cursor_is_still` (exact point, read again until the cursor stood still around the read). |

## Rows

| Row | Before | After | Why |
|---|---|---|---|
| `X-input-native` | Done, 69 of 92 evaluated, 18 missed historic | Partial | 58 of 92 evaluated by this lane (all 41 OS-read mutants, 17 of 51 pure); everything observable is killed (0 missed apart from the equivalent `read_modifiers` mutant); the `Touchpad` branch of `current_wheel_device` is unobservable without hardware, the native tests are `#[ignore]` and not run by CI, and the pure half was rerun as one shard of three only (15 of 17 caught, 2 unviable; 42 caught / 4 timeout / 5 unviable on 2026-10-23 for the same file, unchanged since 2026-10-21). |
| `X-input-message-handlers` | Done, 13 of 42 | Partial | This lane ran shard 1/3: 14 of 14 caught, 0 missed, with the 13 of 2026-10-23: 27 of 42 mutants evaluated, 0 missed. 15 mutants (the other two shards) were not run in the time-box. Nothing suggests a gap; the row is lowered only because the run is incomplete. |
| `X-input-viewport-surface` | Done, 13 of 126 | Partial | Shard 1/6: 21 of 21 caught, 0 missed, with the 13 of 2026-10-23: 34 of 126 evaluated. About 100 pure forwarding mutants (`ViewportControllerApi` pass-through) not run in the time-box. |
| `X-input-drop-target` | Done, 17 of 47 (3 missed called equivalent) | Done | Whole file: 47 of 47 evaluated, 6 survivors killed by new tests, 1 equivalent, 1 unviable. Remaining gap kept in the row: the COM registration tests are native `#[ignore]` and not run on a desktop CI. |

Not run: `surface.rs` shards 2..6, `handlers.rs` shards 2..3, `native.rs` pure helpers shards 2..3 (34 of 51), the Win32 glue itself (`RegisterDragDrop` and the OS calls are exercised only by the native tests, never mutated away from them), any check with a person at the machine, a precision touchpad, or the real OS window procedure.
