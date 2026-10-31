# ST-STARTUP-GOLDEN 2026-10-31

> Status: done (lane `st-startup-golden`, item 3 of ST-GAPMAP2 section 6). Evidence for PARITY rows X-startup-options-run and X-startup-pure-17e.

## Scope

Load the oracle golden `shell-startup-texts` (C# 7eaee2a2) by name, which no Rust test did before, and add the option/env combination tests for `ShellStartupOptions`. No production code changed.

New file: `crates/pr-shell/tests/startup_texts_golden.rs` (8 tests, `cfg(windows)`, no window, no process-environment mutation; fake `EnvSource`).

| Test | Covers |
|---|---|
| `startup_failed_texts_match_csharp_golden` | 5 rows `app.startupFailed` (empty, braces, CRLF, Vietnamese message) |
| `recovery_prompt_texts_match_csharp_golden` | 7 rows `recovery_prompt_texts` (0, 1, 2, 3, 21, 1000, 1234567) |
| `closing_rows_match_csharp_golden` | 2 rows `close_secondary_windows_unless_cancelled` |
| `golden_has_only_the_three_known_case_kinds` | 14 rows, no unloaded kind |
| `second_instance_texts_and_constants_match_options_golden` | the 3 texts and the constants row of `shell-startup-options` (exit codes, fault limit, prefetch timeout, forward window, prefix R0002 mapping) |
| `shell_options_match_csharp_golden_including_the_non_env_defaults` | 14 option rows through the full `ShellStartupOptions` (activate, hooks, perf listener, smoke timeout) |
| `option_env_matrix_follows_the_csharp_rules_for_every_combination` | 6 x 8 x 8 = 384 combinations of prefix/WARP/NOWARM, agreement with `EnvOptions` |
| `variables_are_read_by_their_exact_rust_names_only` | old `PHOTOREVIEW_SHELL_*` names and wrong case do not leak |

## Run

`cargo test -p pr-shell --test startup_texts_golden -j 2`: 8 passed, 0 failed (local). `rustfmt --edition 2024` applied to the new file. No workspace build, no mutation run.

## Proposed PARITY changes (orchestrator)

- X-startup-options-run: add `startup_texts_golden` 8/8; the "Remaining: shell-startup-texts golden, full option/env combinations" can drop those two items. Stays Partial (hardware coverage, native glue; UTC log timestamps Different).
- X-startup-pure-17e: golden `shell-startup-texts` now loaded (14/14 rows); stays Partial until attended hardware coverage.
- X-startup-options: add the matrix as extra evidence; already Done.
