Status: evidence report (2026-11-01), lane cl-mut-bench3; follow-up of [CL-MUT-BENCH2](CL-MUT-BENCH2-2026-10-31.md). Complete for `image_executor.rs` and the open `tool_path_guard.rs` survivors; PARITY not raised.

# CL-MUT-BENCH3: image_executor.rs and tool_path_guard.rs survivors

Flags: `--baseline skip --timeout 300 -j 1`, libtest flags after `--cargo-test-arg=--`, `--include-ignored --test-threads=1`, own `target-mut`, one cargo-mutants at a time, sources touched before each run. `tools/dev/mutants.sh` on origin/master has no `-F`/`--skip`/shard pass-through, so a local script with the same flags and env was used (mutants.sh not edited; lane cl-mut-core3 owns it). Preflight "running N tests": tool_path_guard `--lib` 20 + `cli_tool_path_guard` 17 + killers; image_executor `--lib` 20 + `image_executor` 28. No `no tests/golden above` in any log. Timeout counts as not detected.

## Results (real numbers)

| File | Run | Bins | Tested | Caught | Missed | Timeout | Unviable |
|---|---|---|---|---|---|---|---|
| `tool_path_guard.rs` | the 16 predecessor survivors + 10 same-line siblings (-F on lines 62,225,226,229,231,245,276,299,304,309,310,324,329) | lib + cli_tool_path_guard + mut_bench3_killers | 26 | 19 | 6 | 0 | 1 |
| `tool_path_guard.rs` | 245:25 alone | mut_bench3_killers only | 1 | 1 | 0 | 0 | 0 |
| `image_executor.rs` | full file, 3 shards `--shard k/3` | lib + image_executor | 87 (29+29+29) | 62 | 5 | 1 | 19 |
| `image_executor.rs` | the 6 survivors (5 missed + 1 timeout) | mut_bench3_killers only | 7 (one extra mutant, 568:17, matched the filter) | 6 | 1 (568:17) | 0 | 0 |

Nothing in either file is NOT RUN.

## tool_path_guard.rs
- Killed: 62:21, 225, 229, 231, 304, 309, 310, 324, 329 (caught by the existing lib / `cli_tool_path_guard` tests, now verified); 276:35 and 276:90 caught.
- 245:25 (`==` to `!=` in `replace_leaf`): new killer `replace_leaf_rename_permission_denied_is_the_pathless_access_denied` (a leaf held open without FILE_SHARE_DELETE: `rename` answers PermissionDenied, text must be the path-less "Access to the path is denied."). Caught with the killers binary alone.
- Equivalent (6 missed, none open):
  - 226:9 delete arm `Some(ERROR_PATH_NOT_FOUND)`: code 3 maps to `ErrorKind::NotFound`, the fallback arm gives the identical `DirectoryNotFoundException`.
  - 276:62 `|` to `&` and to `^`: `^` equals `|` for the two distinct bits. With `&` the mask is 0, so the directory/read-only cases fall to `rename`, which itself fails with PermissionDenied and the same path-less message (visible result identical).
  - 299:5 (to true / to false) and 299:76 `||` to `&&`: the `#[cfg(not(windows))]` body, not compiled on Windows (dead code here).

## image_executor.rs (87 mutants)
- DefaultFileLog `enabled` (true/false), `info`, `warn`, `error` (5 mutants): all caught by the inline killer `default_file_log_forwards_info_warn_error_to_the_log_file` (lib), closing the open item of CL-MUT-BENCH2.
- 5 missed + 1 timeout, all now killed in `mut_bench3_killers.rs` (killers binary alone):
  - 76:9 `ExecutorError` Display: `executor_error_display_names_the_cause`.
  - 139:9 `BenchmarkExecutorOptions` Debug: `executor_options_debug_names_the_type_and_fields`.
  - 157:9 `TeardownHandle` Debug: `teardown_handle_debug_names_the_type`.
  - 256:9 `LoggingScope` Debug: `logging_scope_debug_shows_whether_the_restore_is_pending`.
  - 295:9 `WindowedPreloadTarget` Debug: `windowed_target_debug_shows_the_window`.
  - 178:9 `TeardownHandle::finish` to `()` (was TIMEOUT, 300 s): `dispose_completes_the_teardown_handle` uses a bounded `wait_timeout`, so the mutant fails fast.
- 568:17 (delete field `options` in `PreloadSchedulerDeps`): caught in the shard run (lib + image_executor); with the killers binary alone it is missed, which is expected (that binary does not exercise the scheduler options). Not a gap.

No production code changed.

## Proposed PARITY changes (not applied)
- X-bench-cli-guard: all 57 mutants of `tool_path_guard.rs` now have a verdict (no open non-equivalent survivor); the PARITY owner may consider the mutation gate closed.
- image_executor row: 87 tested, 62 caught + 19 unviable + 6 killed by new tests; the owner may do the same.

## Artifacts for the orchestrator to delete (agent does not rm)
`C:\MyProjects\PhotoReview-rs\.claude\worktrees\cl-mut-bench3\target-mut`, `...\target`, `...\mut-out-ie0`, `mut-out-ie1`, `mut-out-ie2`, `mut-out-kill`, `mut-out-tpg`, `mut-out-tpg245`, and the `mut-run-*.log` / `mut-pre-*.log` files in the same worktree root.
