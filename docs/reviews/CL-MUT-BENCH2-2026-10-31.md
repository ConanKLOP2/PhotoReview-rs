Status: evidence report (2026-10-31), lane cl-mut-bench2; follow-up of [CL-MUT-BENCH](CL-MUT-BENCH-2026-10-31.md). Time-boxed, partial: see "Not run".

# CL-MUT-BENCH2: mutation for pr-bench workload_runner and tool_path_guard

Flags as in the predecessor (`--baseline skip --timeout 300 -j 1`, `--test <bin> -- --include-ignored --test-threads=1`, own `target-mut`). Preflight: `workload_runner` 35 tests, `cli_tool_path_guard` 17 tests, green.
Finding: a reused `target-mut` runs a stale artifact of the last mutant (a lib test failed until the sources were touched); sources were touched before every later run. No `no tests/golden above` failure appeared in any log of these runs.

## Results (real numbers)

| Row / file | Bins | Tested | Caught | Missed | Timeout | Unviable | Not run |
|---|---|---|---|---|---|---|---|
| X-bench-workload-runner `workload_runner.rs` (127), bin `workload_runner` only | 1 | 127 distinct (run 1: 103 = 76 caught, 15 missed, 12 unviable; run 2: 29 = 18 caught, 9 missed, 2 unviable; run 2 re-tested some run-1 mutants, so the raw sums double count) | 94 raw | 22 distinct | 0 | 14 raw | 0 |
| same 22 survivors + 15 neighbours on the same lines, bins `--lib` + `workload_runner` + `w10_bench_killers` + `mut_bench2_killers` | 4 | 37 | 36 | 0 | 0 | 1 | - |
| X-bench-cli-guard `tool_path_guard.rs` (57), bin `cli_tool_path_guard` only | 1 | 57 | 37 | 16 | 0 | 4 | 0 |
| `image_executor.rs` rest (~57) | - | CHUA CHAY | | | | | ~57 |

Run 1 of workload_runner stopped after 103 mutants (process ended without a summary); the other 24 were run in a second pass.

### workload_runner survivors of the single-bin run, and how they are now killed
- 538/555/578 `>`, `&&`, `&=` (pixel dimension checks): already killed by `w10_bench_killers` (other bin); caught in the 4-bin rerun.
- 700/701 `is_tolerated_race_failure` (match arm, `||`): killed by the existing inline `--lib` tests; caught in the 4-bin rerun.
- 56 `Display`, 66 `source`: new `mut_bench2_killers.rs` (`workload_error_display_shows_the_inner_text`, `workload_error_source_exposes_the_cause`).
- 164 `-=` (to `+=`, `/=`), 166 `<` to `<=` in `DotNetRandom::internal_sample`: not reachable from the public API (needs a difference of exactly `i32::MAX` or 0); inline tests `internal_sample_never_returns_int_max`, `internal_sample_keeps_a_zero_difference` build the private state.
- 388/397/400 `NativeFileActionIo::read/copy/remove`: inline test `native_file_action_io_reads_copies_and_removes_real_files` (TempRoot).
All 22 are caught when the four bins run together. In the strict single-bin number they stay "missed".

### tool_path_guard: 16 survivors with bin `cli_tool_path_guard` alone (NOT re-run with other bins, no new killers; time box)
`62:21` (== to !=), `225`, `226`, `229`, `231` (match arms of `map_write_error`), `245:25`, `276:62` (two), `299:5` (two), `299:76`, `304`, `309`, `310`, `324`, `329`.
- 62, 225-231 have inline `--lib` tests (`resolve_errors_map_by_kind`, `write_errors_map_*`) that were not part of this run; likely caught there, unverified.
- 304/309/310/324/329 (`local_app_data_child`, `combine_with_base`, `app_data_folder`): inline `--lib` tests exist; unverified.
- 299 (`#[cfg(not(windows))] leaf_refuses_replacement`): dead code on Windows, equivalent here.
- 245, 276: `replace_leaf` / `leaf_refuses_replacement` read-only/share checks: open, need a killer.

### DefaultFileLog (4 golden-only mutants of CL-MUT-BENCH)
New inline killer pair in `image_executor.rs` (`default_file_log_forwards_info_warn_error_to_the_log_file` + child `default_file_log_child_writes_messages`): the test binary re-runs itself with `Command::env("PHOTOREVIEW_RS_DATA_ROOT", TempRoot)` (safe, no `set_var`), the child writes through `DefaultFileLog::info/warn/error`, the parent reads `<root>/logs/app.log` and asserts INFO/WARN/ERROR markers. Passes. The mutation rerun of `DefaultFileLog::info/warn/error` against `--lib` was NOT run (stopped on request); expected to be caught, unverified.

## Not run
- `image_executor.rs` remainder (~57 mutants).
- Rerun of the 16 tool_path_guard survivors against the other bins and of the DefaultFileLog mutants against `--lib`.
No production code changed.

## Proposed PARITY changes (not applied)
- X-bench-workload-runner: all 127 run; 94 caught + 14 unviable on the single bin, the 22 survivors caught with 4 bins; add the killers above. Keep Partial until PARITY owner agrees.
- X-bench-cli-guard: 57 run, 37 caught, 16 survivors unverified against the lib/other bins; keep Partial.
