# Mutation evidence: MUT-routing-startup-rest (Machine A)

Date: 2026-10-29. Branch `claude/mutation-routing-startup-rest`. Tool: cargo-mutants 27.1.0.
Command shape: `cargo mutants -p <crate> --baseline skip --timeout 90 -j 1 --shuffle --file '**/<path>.rs' --output <dir> --cargo-arg=--test=<binary> ...`,
one `CARGO_TARGET_DIR` outside the worktree (`_mut-target-rest-a` / `-b`), at most two processes at a time, a 15 minute box per file.
These files have no `#[cfg(test)]` unit tests, so no `--lib` pass was needed. Run on the whole file unless the table says otherwise.
Small rename-related functions are inside the whole-file runs; no `-F` restriction was needed (every file has at most 80 mutants, except `settings_field_map.rs`).

## First pass (existing tests only)

| File | Test binaries | Tested / total | Caught | Missed | Unviable | Timeouts |
|---|---|---|---|---|---|---|
| `pr-app/src/input/shortcut_router.rs` | rename_file_shortcut_router, shortcut_router | 26/26 | 23 | 1 | 2 | 0 |
| `pr-app/src/menus/builder.rs` | menu_model, rename_file_menu_model | 34/34 | 28 | 0 | 6 | 0 |
| `pr-app/src/presenters/settings_field_map.rs` | settings_field_map | 172/249 (box hit, shuffled) | 149 | 0 | 23 | 0 |
| `pr-core/src/settings/context_menu.rs` | settings_context_menu, rename_file_settings | 68/68 | 59 | 0 | 9 | 0 |
| `pr-core/src/settings/shortcuts.rs` | settings_shortcuts, rename_file_settings | 80/80 | 79 | 0 | 1 | 0 |
| `pr-core/src/settings/normalizer.rs` | settings_normalizer, rename_file_settings | 67/67 | 66 | 1 | 0 | 0 |
| `pr-app/src/startup/context.rs` | startup_settings_load | 3/3 | 1 | 0 | 2 | 0 |
| `pr-app/src/startup/early_decode.rs` | startup_early_decode, startup_settings_load | 5/5 | 2 | 1 | 2 | 0 |
| `pr-app/src/startup/error_log.rs` | startup_error_log | 54/54 | 37 | 11 | 5 | 1 |
| `pr-app/src/startup/forwarded_open.rs` | startup_forwarded_open | 22/22 | 16 | 4 | 2 | 0 |
| `pr-app/src/startup/instance.rs` | startup_instance | 9/9 | 7 | 1 | 1 | 0 |
| `pr-app/src/startup/launch.rs` | startup_launch, startup_launch_cwd | 13/13 | 9 | 3 | 1 | 0 |
| `pr-app/src/startup/options.rs` | startup_options | 8/8 | 8 | 0 | 0 | 0 |
| `pr-app/src/startup/settings_load.rs` | startup_settings_load, startup_early_decode | 37/37 | 14 | 21 | 2 | 0 |
| `pr-app/src/startup/ui_hop.rs` | startup_ui_hop | 10/10 | 4 | 2 | 4 | 0 |

Totals: 608 tested of 685 (all except 77 mutants of `settings_field_map.rs`): 502 caught, 45 missed, 60 unviable, 1 timeout.

NOT run: 77 of the 249 mutants of `settings_field_map.rs` (the run hit its 15 minute box; the order is shuffled, so the untested ones are a random subset). No survivor was found in the 172 tested. Not a full verdict for that file.
`startup/{crash,placement_read,recovery,shutdown,smoke}.rs` were not in the task. `menus/builder.rs` and the rename mutants were all caught or unviable (0 missed): the rename routing (`rename_file` binding, `RENAME_FILE` command, `ContextMenuItemId::RenameFile` menu entry) needed no new test.

## Survivors and how each was dealt with

New tests: `crates/pr-app/tests/routing_startup_mutation_killers.rs` (16 tests) and `crates/pr-core/tests/routing_settings_mutation_killers.rs` (1 test). Each test names its mutant in a comment.

| Mutant | Verdict |
|---|---|
| shortcut_router.rs:257 `parse_key` guard `!t.trim().is_empty()` -> true | EQUIVALENT: `try_parse` itself trims and refuses empty text, so the guard is redundant. No test. |
| normalizer.rs:395 Debug for SettingsValidator | killed (pr-core killers) |
| early_decode.rs:35 Debug for EarlyDecodePlan | killed |
| forwarded_open.rs:34 Display WindowGone, :63 Debug Handler, :120 Debug Router | killed |
| forwarded_open.rs:147 guard `existing.is_none()` -> false | killed; real behaviour (C# `_pendingForwardPath ??= path`, checked at 7eaee2a2): a later path fills a pending request that had none |
| instance.rs:24 Display InstanceStartError | killed |
| ui_hop.rs:39 Display HopLost, :55 Debug UiHopSender | killed |
| launch.rs:53 `+=` -> `-=` / `*=`, :58 `!=` -> `==` | killed (two directories whose names contain NUL make `get_full_path` fail; the warning must say 2, and no warning when none failed) |
| settings_load.rs:36, :125 Debug; :52 Display StartupLoadError; :138-:169 the nine `SettingsTexts` members (18 mutants) | killed |
| error_log.rs:15 `256 * 1024` -> `+` | killed |
| error_log.rs:157 `>` -> `>=` (file exactly at the limit) | killed |
| error_log.rs:170:17 `<` -> `<=` (file without a trailing newline) | killed |
| error_log.rs:171 `+=` -> `-=` | killed (exact text after a cut inside a line) |
| error_log.rs:171 `+=` -> `*=` | TIMEOUT: the loop never ends, so the mutant is detected only by the harness time limit; it stays reported as a timeout, not as caught |
| error_log.rs:181, :182, :187 line-ending arms | killed |
| error_log.rs:78 `delete !` (empty stack lines), :71 guard `code <= 0` -> false | killed |
| error_log.rs:72:53 `|` -> `^` | EQUIVALENT: the masked low word and `0x8007_0000` share no bit, so `|` and `^` give the same value |
| error_log.rs:170:40 `start > 0` -> `>= 0` | EQUIVALENT (unobservable): `truncate` is private and only called when `len > max_bytes`, so `start = len - min(len, max_bytes/2) >= 1` always |

## Re-run of the killed mutants against the new tests alone

`cargo mutants ... -F <regex> --cargo-arg=--test=routing_startup_mutation_killers` (pr-core: `routing_settings_mutation_killers`):
- normalizer Debug: 1/1 caught. early_decode Debug: 1/1 caught. instance Display: 1/1 caught. ui_hop: 2/2 caught.
- forwarded_open: 5 selected, 4 caught (the 4 former survivors); the 1 missed is `guard -> true`, a different mutant that the older tests already caught.
- launch (`in ShellLaunch::parse`): 6 selected, 4 caught (includes the 3 former survivors); the 2 missed (:46, :83) are other mutants the older tests already caught.
- settings_load (Debug, Display, `LocalizerSettingsTexts`): 21/21 caught.
- error_log (whole file, new tests alone): the 9 killable former survivors are no longer in the missed list; the remaining misses are the 2 equivalents above, the `*=` timeout, and mutants that only the older tests cover (configure, current_path, report, report_details, append, `delete !` in append_at, `code <= 0 -> true`, the other `|`/`&` swaps of line 72).

## Gate

`cargo fmt -p pr-app --check`, `cargo fmt -p pr-core --check`, `cargo clippy -p pr-app -p pr-core --all-targets --locked -- -D warnings` clean; both killers binaries pass (16 + 1). No production code was changed and no real bug was exposed.
