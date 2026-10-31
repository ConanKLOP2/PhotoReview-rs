# CL-MUT-BENCH: mutation re-run for pr-bench (2026-10-31)

Command shape (correct flags): `cargo mutants -p pr-bench --baseline skip --timeout 300 -j 1 --file <file> -F <regex> --cargo-test-arg=--test --cargo-test-arg=<bin> --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`
(image_executor also had `--cargo-test-arg=--lib`). Preflight without mutation: `slow_link_fs` 37 tests, `image_executor` 28, `workload_runner` 35,
`cli_tool_path_guard` 17, all green; mutant logs show `running 37 tests` / `running 28 tests` / `running 14 tests`. Separate `target-mut` and
output dirs inside the worktree, one process at a time. `cargo mutants --list -p pr-bench` = 1,000+ mutants, so only the groups below were run (time box ~60 min).

## Honesty caveat found in this run
`tests/golden` is NOT copied into the cargo-mutants temp tree, so every golden test (`*_matches_golden`, `golden_*`) fails there with
`no tests/golden above ...`. A mutant "caught" only by golden tests is therefore NOT really caught. I recomputed each `caught` entry and kept
only those where a non-golden test failed.

## Results

| Row / function group | Tested | Caught (real) | Missed | Timeout | Unviable | Not run |
|---|---|---|---|---|---|---|
| X-bench-slowlink-fs: all `SlowLinkFileSystem` methods (`-F SlowLinkFileSystem`, bin `slow_link_fs`) | 45 | 36 | 1 | 0 | 8 | - |
| X-bench-executor: `DefaultFileLog`, `FailClosedMemoryProbe`, `choose_memory_probe`, `new_scratch_directory_name`, `delete_scratch_directory_after_prune`, `outside_window`, `try_get_cached_preview*`, `apply_logging` (bins `--lib` + `image_executor`) | 30 | 23 | 0 | 0 | 3 | - |
| of those 30: caught only by golden (false) | 4 | | | | | |
| X-bench-workload-runner (127 mutants) | CHUA CHAY | | | | | 127 |
| X-bench-cli-guard `tool_path_guard.rs` (57 mutants) | CHUA CHAY | | | | | 57 |
| rest of image_executor (about 57: Debug impls, build_preload, decode*, dispose, teardown*, ...) | CHUA CHAY | | | | | ~57 |

### slow_link_fs (forwarding now run)
All 21 `FileSystem` forwarding methods, `delay_metadata`, `delay_per_entry`, `with_time_source`, `Debug`: caught except:
- MISSED `slow_link_fs.rs:268:34` `> -> >=` in `delay_metadata`: equivalent. With zero latency, `>=` enters the branch, `whole_ms == 0` and it only calls `yield_now`, same observable behaviour (no sleep, no inner call change).
- Unviable (8): the `Default::default()` replacements for types without `Default`.
Not run in this file (46 mutants): `SharedBandwidthLimiter`, `ThrottledReadStream`, `SystemTimeSource`, `InvalidBandwidth` (earlier evidence with fake clock, not re-run here).

### image_executor
- `info`/`warn`/`error` -> `()` in `DefaultFileLog`: only golden tests failed (golden missing in the mutant tree), so they are NOT verified. Killing them needs writing the process-wide real app log (`FileLog::default_instance()` resolves the real `AppPaths`), and redirecting it needs `set_var` (unsafe, not allowed in pr-bench). Still open.
- `FailClosedMemoryProbe::has_headroom -> true` was golden-only; killer `default_file_log_tests::missing_probe_fails_closed` (inline `#[cfg(test)]`, `choose_memory_probe` is `pub(crate)`) added; rerun of that mutant: caught (`running 15 tests`).
- Unviable (3): `apply_logging`, `choose_memory_probe`, `get_snapshot` with `Default::default()`/`Some(Default)`.
- No production code changed.

## Proposed PARITY changes (not applied)
- X-bench-slowlink-fs: add "forwarding mutants run 2026-10-31: 45 tested, 36 caught, 1 equivalent (delay_metadata `>=`), 8 unviable"; keep Partial (DIVERGENCE items and limiter/stream not re-run).
- X-bench-executor: add the 30-mutant result above; `DefaultFileLog::info/warn/error` forwarding stays unverified; keep Partial.
- X-bench-workload-runner, X-bench-cli-guard: not run; no change.
