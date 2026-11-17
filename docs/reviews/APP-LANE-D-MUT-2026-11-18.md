Status: evidence report (2026-11-18). Whole-file mutation run of the `pr-app` lane D files (`startup/*.rs` except `mod.rs` and `placement_read.rs`, 13 files) against the whole `pr-app` suite, plus a confirmation run of the killers added. Measured on one machine (Windows 11), `--jobs 1`, `MUT_TIMEOUT=900`, 6 hours.

# `pr-app` lane D: startup

## Result (first run, whole suite)

185 identities: 156 caught, 25 unviable, 3 missed, 1 timeout.

| File | Caught | Unviable | Missed | Timeout |
|---|---|---|---|---|
| `context.rs` | 1 | 2 | 0 | 0 |
| `crash.rs` | 6 | 2 | 0 | 0 |
| `early_decode.rs` | 3 | 2 | 0 | 0 |
| `error_log.rs` | 46 | 6 | 1 | 1 |
| `forwarded_open.rs` | 22 | 3 | 0 | 0 |
| `instance.rs` | 8 | 1 | 0 | 0 |
| `launch.rs` | 12 | 1 | 0 | 0 |
| `options.rs` | 6 | 0 | 2 | 0 |
| `recovery.rs` | 4 | 1 | 0 | 0 |
| `settings_load.rs` | 35 | 2 | 0 | 0 |
| `shutdown.rs` | 4 | 0 | 0 | 0 |
| `smoke.rs` | 3 | 1 | 0 | 0 |
| `ui_hop.rs` | 6 | 4 | 0 | 0 |

## Survivors

| Mutant | Verdict | Argument or killer |
|---|---|---|
| `options.rs:41:9` `ProcessEnv::var` -> `None` | killed | `process_env_reads_a_set_variable` (read-only check of `PATH`) |
| `options.rs:72:9` `EnvOptions::from_process` -> `Default::default()` | killed | `from_process_equals_from_env_over_the_process_environment`; under the mutant `default()` and `from_process()` call each other and the test binary aborts (stack overflow), which counts as a failure |
| `error_log.rs:174:40` `start > 0` -> `start >= 0` in `truncate` | equivalent | `truncate` is private and called only when the file is longer than `max_bytes`; then `keep = min(len, max_bytes / 2) < len`, so `start = len - keep > 0` always and the changed comparison cannot differ; valid while that call-site guard stays |
| `error_log.rs:175:15` `start += 1` -> `start *= 1` | timeout | the loop never advances when the cut falls inside a line; detected by non-termination, not by an assertion |

## Confirmation

Rerun of the options positions with `--tests startup_options`: 4 caught (the position filter pulls sibling mutants), 0 missed.

## New tests

Two tests appended to `crates/pr-app/tests/startup_options.rs`. No source change, no seam.

## Not covered

`startup/mod.rs` and `startup/placement_read.rs` were not in this run.
