# SMALL-ITEMS: five small open items (ST-LOOP28, 2026-11-20)

Status: evidence report. Branch `test/small-items`, base `origin/master` fda051bf. Seams and one parity fix are logged as P-420 to P-423 in `docs/PROPOSED-DECISIONS.md`. TEMP `mut-small`, one mutation run at a time per crate, `-j 1`, budget 300000 s, `MUT_TIMEOUT=600`.

## 1. X-exe-entry: `StaApartment::enter` error branch

Finding: `StaApartment::enter` itself was already covered (`shell_startup_apartment`, including the real `ChangedMode` of a thread that is already in the MTA). The untested part was the `Err` arm of `main_entry`, which returns `!` and so could not be called.
Change (P-420, no behaviour change): `entry.rs` gets `enter_apartment_or_log(enter, log) -> Option<G>` and `APARTMENT_LOG_MESSAGE`; `main_entry` calls it with `StaApartment::enter` and `AppLog::global().error_forced`.
Tests (`w8_entry_killers`, 4 new, 7 of 7 pass): error logs once and gives `None` (fake `Err`), `Ok` keeps the guard and logs nothing, the real `ChangedMode` after the MTA gives `None` plus one log call, the log message text.
Mutation, `entry.rs` whole file with `w8_entry_killers` and `entry_args`: 11 tested, 7 caught, 1 unviable (`enter_apartment_or_log -> Some(Default::default())`, `G` has no `Default`), 3 missed: `production_run -> 0, 1, -1`, the known unobservable trio of [CL-ENTRY](CL-ENTRY-2026-10-30.md) (the function opens a real window; killed only by the native exe tests, which are not part of this run). `main_entry` itself is not mutated (returns `!`). A first run of the same command reported 11 unviable because the scratch copy was taken while a file of the worktree was half edited; it is discarded and the rerun above is the verdict.

## 2. C-28: box text read back

`startup_repair_notice_native` now reads the text of the real TaskDialog with UI Automation (the PowerShell reader that `startup_unanswered_owner_native` already uses) and checks the catalog sentence `settings.load.repaired` (English, `UiLanguage` fixed in the config) around the repaired field name `ImageCacheRamPercent`. Both native tests pass on this desktop (`--include-ignored --test-threads=1`, 2 of 2). Test only, no production change. The only open part of C-28 stays the attended run of the real exe with an invalid `config.json`.

## 3. X-platform-adapters: `REG_EXPAND_SZ`

Verdict: real (small) divergence, fixed (P-422). C# `key.GetValue(null) as string` expands `REG_EXPAND_SZ` (`WindowsShellIntegration.cs` line 66, no `DoNotExpandEnvironmentNames`); Rust read the raw text, so a command stored by an installer or a person as `REG_EXPAND_SZ` with `%VAR%` gave `RegisteredElsewhere` where C# gives `Registered`. Values written by the app stay `REG_SZ`.
Change: `read_default_string` expands with `ExpandEnvironmentStringsW` (new private `expand_environment_strings`, feature `Win32_System_Environment`).
Tests: 1 inline unit test (set variable expands, unset variable and text without `%` stay), native `a_reg_expand_sz_command_is_read_with_its_variables_expanded` (private HKCU test tree), native `a_command_stored_with_a_non_string_registry_type_is_not_a_registration` (a `REG_BINARY` command is not a string). `shell_integration` native 10 of 10, lib 7 of 7.
Mutation, `read_default_string|expand_environment_strings` (20 mutants, tests: `shell_integration` native plus `--lib`): 17 caught, 1 unviable (187:41, `written as usize < ..` does not parse), 2 missed. 146:32 `||` -> `&&` was killed by the REG_BINARY test added after the run (rerun result below). 187:21 `||` -> `&&` is equivalent by argument: the second `ExpandEnvironmentStringsW` call can return 0 or more than `needed` only if the environment changes between the two calls or the call fails after the sizing call succeeded, which no test can provoke; the guard is defensive and is kept.
REG_BINARY rerun (146:32): 1 tested, 1 caught (`a_command_stored_with_a_non_string_registry_type_is_not_a_registration`). Totals for the two functions: 20 identities, 18 caught, 1 unviable, 1 equivalent by argument.

## 4. benchmark.rs cancel-poll cadence (832:14, 833:59, 833:17 x2) and Gate 0

Finding: the four identities only changed how often the token was polled inside the loop; the poll after the loop decided the result, so no test could see them without a way to cancel the token between entries. The P-404 note said a `CancelToken` hook was needed; it is not: the cadence is observable with an entry source that cancels the token at a chosen file and fails at a chosen entry.
Change (P-421, no behaviour change): `enumerate_image_files` keeps its signature and delegates to `enumerate_scan_entries(entries, limit, cancel)` over `ScanEntry { is_dir, path }`; the adapter maps `read_dir` entries one to one (same error text and order).
Tests (`st28_benchmark_scan_cadence`, 4): polled at every multiple of 256 (cancel at file 256, failure at 257, and at 512), not polled between multiples (cancel at 255 or 1, failure at 256; 257 / 512), directories do not count (a directory before every file), no-cancel listing plus limit and the final poll. `benchmark_presenter` 41 and `mut2_n4_benchmark_presenter` 3 still pass.
Mutation (`enumerate_image_files|enumerate_scan_entries`, 12 mutants, three test binaries): 12 tested, 12 caught, 0 missed, 0 unviable, 0 timeout. The four cadence identities (now in `enumerate_scan_entries`) are among them and are caught.
Gate 0 (contract W10-B1: JSON key set and order, .NET `R` number tokens of non-timing fields): decision, no code. It needs a real C# `summary.json` fixture and an owner choice of strictness; a parser written without the fixture would encode a guess about the `R` form. The attended W10-B2 run was not started. Logged as P-423 (OPEN, owner). X-benchmark-compare and X-benchmark-parity-harness stay Partial.

## 5. Stale "Still open" lists in `task_on_progress.md`

ST-LOOP13's list named items that were closed since: D-10 Settings glue (ST-LOOP14, 15), `physical.rs` (ST-LOOP16), pr-app lanes A to D (ST-LOOP18..21), overlay input (ST-LOOP26), benchmark 832/833 (this note). The ST-LOOP22 remark "2 equivalent without libraw.dll, to be checked once" is closed by #740. The list now keeps only what is still open: 679:13 and 680:13 (P-402), the 12 pre-existing `Url::parse` mutants of win-seams, selector 376:22; ST-LOOP21's `startup/mod.rs` and `placement_read.rs` remain as written there.

## Not run

Whole-file mutation of `shell_integration.rs` outside the two functions above; `production_run` native exe runs; the attended benchmark run; full `cargo test --workspace` (see the PR body for what pre-pr ran).
