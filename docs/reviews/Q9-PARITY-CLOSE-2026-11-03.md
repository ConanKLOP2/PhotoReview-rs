Status: evidence report (2026-11-03/04), task Q9, branch `parity/bench` from origin/master 84d2c151. Rows: X-benchmark-compare, X-benchmark-parity-harness, X-attended-remeasure, X-bench-slowlink-fs, X-bench-executor, X-bench-workload-runner, X-bench-cli-guard, X-benchmark-presenter. PARITY.md NOT edited (the lead does). One test file changed, no production code.

# Q9: closing the bench cluster as far as a machine can

Machine: Windows 10 Pro 19045 desktop, Ryzen 5 7600X (12 logical CPUs), NVIDIA GTX 1660 SUPER + AMD Radeon (two physical adapters), free RAM about 6 GB during the run (other agents building), `CARGO_BUILD_JOBS=2`, own target dir `C:\Users\PC\Projects\target-pc-bench`, mutation target `%TEMP%\mut-target-pr-bench`.

No `PARITY promotion rule` section exists in AGENTS.md (grep for "promot" finds none); the rule used is the one in the rows themselves (mutation verdict for pure logic, native run for `#[ignore = "native"]` tests, attended runs and corpus stay Partial, an intentional difference needs an ADR / `Different` row; P-53 tiers).

## What ran in this run (all counts are from this run's logs)

| Run | Command (short) | Result |
|---|---|---|
| pr-bench full, incl. native | `cargo test -p pr-bench --locked --no-fail-fast -- --include-ignored --test-threads=1` | 401 passed, 2 failed, 0 ignored. The 2 failures are `bench_recycle_bin::adv::non_denied_names_like_dotnet` and `adv::reserved_con_in_dir_ok` (row X-bench-recycle-bin, NOT in this cluster): `NUL.txt` / `CON` in a TempRoot return `Uncategorized` instead of `ok` on Windows 10 19045 (the tests were probed on a newer Windows; same kind of machine difference as `D4-IMPL` notes). Not investigated further; listed as a finding. The 5 native tests of this cluster passed: `cli_tool_path_guard` 17 of 17 (junction, hard link, symlink, nested junction; the earlier "UNC sandbox failure" does not reproduce), `workload_runner::file_action_native_scratch_race_reports_per_operation_counts` and the lib native test `native_file_action_io_reads_copies_and_removes_real_files` |
| pr-app presenter binaries | `cargo test -p pr-app --test benchmark_presenter --test st_bench_presenter_session --test w8_presenter_killers2 --test w8_rest2_killers --test benchmark_presentation --test benchmark_presentation_golden -- --include-ignored --test-threads=1` | 41 + 5 + 2 + 2 + 23 + 1 = 74 passed, 0 failed |
| pr-bench-win | `cargo test -p pr-bench-win --locked -- --include-ignored` | `host_wiring` 7 passed |
| harness self-test | `powershell tools/dev/test/bench-compare-selftest.ps1` | PASSED (13 checks) |
| clippy | `cargo clippy -p pr-bench --all-targets --locked -- -D warnings` | clean (run after the edit) |
| rustfmt | `rustfmt --edition 2024 --check crates/pr-bench/tests/slow_link_fs.rs` | clean (`cargo fmt -p` not used: os error 206) |

### Mutation (about 47 minutes of mutants in total, `tools/dev/mutants.sh --native`, `-j 1`, `--timeout 120`, preflight "N tests" OK each time, no "no tests/golden above")

| Run | Scope | Tested | Caught | Missed | Timeout | Unviable |
|---|---|---|---|---|---|---|
| M1 | `slow_link_fs.rs` limiter, stream, InvalidBandwidth, SystemTimeSource (`-F 'SharedBandwidthLimiter|ThrottledReadStream|InvalidBandwidth|SystemTimeSource'`), 8 bins (slow_link_fs, w7_mutation_killers, w7_followup_killers, w8_misc_killers, w10_bench_killers, mut_bench2/3_killers, `--lib`; 106 tests) | 46 | 33 | 1 | 11 | 1 |
| M2 | the 11 timeouts plus siblings (18 mutants), same bins + slow_link_reader, AFTER the budget guard below | 18 | 12 | 0 | 6 | 0 |
| M3 | the 6 remaining timeouts plus siblings (10 mutants: 134:25, 139:23 x2, 170:20, 174:66, 202:9 and same-line siblings), bin `w8_misc_killers` alone (9 tests) | 10 | 10 | 0 | 0 | 0 |
| M4 | reproduction of CL-MUT-BENCH3 for `image_executor.rs` (76:9, 139:9, 157:9, 178:9, 256:9, 295:9 plus one sibling), bin `mut_bench3_killers` alone | 7 | 6 | 1 (568:17, expected: needs the scheduler tests, see CL-MUT-BENCH3) | 0 | 0 |
| M5 | reproduction of `tool_path_guard.rs:245:25`, bin `mut_bench3_killers` alone | 1 | 1 | 0 | 0 | 0 |

Timeouts are never counted as caught. The 11 M1 timeouts were hangs of the whole test run: with a mutated limiter loop an unrelated test of another binary (real-clock limiter, or the inline `Fake` clock without a call budget) never returns, so the process hits the 120 s cargo-mutants timeout before `w8_misc_killers` runs. Fix in this run (test only, `tests/slow_link_fs.rs`): the integration `FakeTime` now asserts a call budget (`now_ticks` and `sleep` below 1 000 000), so a never-ending loop panics at once. That turned 5 of the 11 into real failures in the full suite (M2: 151:43, 155:38, 167:9, 168:68, 174:76 caught). The other 6 still hang a real-clock test in the full suite and are caught by `w8_misc_killers` alone (M3); the existing killers `consume_terminates_and_waits_the_exact_time_per_chunk` etc. fail within seconds (bounded by a clock budget and a 20 s channel wait). Verdict for the limiter/stream chunk: 46 of 46 decided = 38 caught in the full suite (33 in M1 + 5 former timeouts in M2) + 6 caught only by `w8_misc_killers` alone (M3: 134:25, 139:23 twice, 170:20, 174:66, 202:9) + 1 equivalent + 1 unviable; 0 missed that is not equivalent, 0 timeouts undecided. "Caught by the killers binary alone" means the full suite still hangs on a real-clock test for those 6 (a CI timeout, which is a failure but not a fail-fast one).

Equivalent (1): `slow_link_fs.rs:203:17` `read > 0` to `>=` in `ThrottledReadStream::read`: `consume(0)` returns at once (`bytes <= 0`), so metering an empty read cannot change any observable (already documented in `w8_misc_killers.rs`).

## Per row

### X-benchmark-compare (and X-benchmark-parity-harness)
- Gaps before: Gate 0 of contract W10-B1 (JSON key set and order, .NET `R` token grammar of non-timing fields) not implemented in `compare.rs`; real host in the `pr-bench` binary unwired (the `pr-bench-win` binary has it since PR #490); attended W10-B2 timing measurement; no real C# summary fixture.
- Ran: `bench_compare` 15, `bench_compare_cli` 5, `bench_report` 7 passed; selftest 13 checks passed; `pr-bench-win` host_wiring 7 passed. Mutation counts of compare.rs (246) and report.rs (53) were NOT rerun (they use only synthetic summaries, not golden, so the golden-infra caveat does not apply; the old counts are kept, not re-verified here).
- Gaps after: unchanged. Gate 0 key-order / R-token check is missing production behaviour (not a test gap); it needs a real C# summary fixture from the oracle, an owner/lead decision on strictness, and a production change in `parse_summary` (proposal below). No real timing exists.
- Proposed status: stays Partial; replace "engine backend wired but no real host" by "real host in `pr-bench-win` (P-60); remaining: Gate 0 key order / R tokens, attended W10-B2".

### X-attended-remeasure (contract W10-B2)
- Gaps before: Todo. The protocol needs a person: display on, foreground window, AC power, 24 MP folder, 24 production runs (+12 spike), IME typing, second monitor at another DPI.
- Machine part done: recorded the preconditions of this desktop (two physical adapters GTX 1660 SUPER and AMD Radeon, Ryzen 5 7600X, Windows 10 19045, a desktop so AC is given); checked the P0 driver: `pr_platform_win::ui_driver::post_key` is now implemented (`ui_driver.rs:63`, no `todo!()`), `PHOTOREVIEW_RS_FRAME_LOG` exists (`shell_viewing/frame_log.rs`), but the "bin/tool to be added by a later unit" that wraps a 250 ms timer around the poster does not exist on master (grep finds no caller of `post_key` outside tests) and `docs/drafts/bench-attended/` does not exist. So even the production half of the matrix cannot be automated today; no measurement was produced and none is claimed.
- Gaps after: all of the protocol. Proposed status: Todo (unchanged); update note: "post_key exists, 250 ms driver tool missing".

### X-bench-slowlink-fs
- Gaps before: limiter/stream mutation not rerun with the correct flags (46 mutants; CL-MISC had 38 tested with timeouts); documented DIVERGENCE items (D65: `enumerate_*_with_stat` list first then sleep; monotonic `Instant`; no negative `Duration`).
- Ran: `slow_link_fs` 37, `slow_link_reader` 4, `w7_*`, `w8_misc_killers`, `w10`, `mut_bench*` all green; M1 to M3 above. FileSystem forwarding mutants (45) were NOT rerun (CL-MUT-BENCH result kept: 36 caught, 1 equivalent `delay_metadata >`, 8 unviable).
- Gaps after: only the DIVERGENCE items, which need the owner's acceptance (they are listed in P-35 / D65) and an ADR or `Different` row.
- Proposed status: Partial now; `Done; Different (D65 as in P-35)` once the lead confirms that P-35 counts as the ADR for D65 (no `adr/` file exists for it).

### X-bench-executor
- Gaps before: `image_executor.rs` remaining inventory (listed as ~57 NOT RUN) and `DefaultFileLog` forwarding.
- Ran: `image_executor` 28 + lib 20 green; M4 reproduced the 6 CL-MUT-BENCH3 killers (6 caught, 568:17 caught only with the scheduler tests as reported). The full-file run (87: 62 caught, 19 unviable, 6 killed by new tests, all decided) is from CL-MUT-BENCH3 (2026-11-01), NOT rerun in full here.
- Gaps after: executor has no golden (fake-decoder tests only; real WIC decoding is exercised only by `pr-bench-win` and the attended run).
- Proposed status: Partial with updated text ("all 87 mutants decided, CL-MUT-BENCH3; 6 killers reproduced 2026-11-03; real decoder path needs the attended run"). Promotion to Done is the lead's call: the mutation condition is met on paper, but I did not reproduce the full run.

### X-bench-workload-runner
- Gaps before: D67 divergences (lowest-index parallel error, empty-list InvalidArgument); mutation split of CL-MUT-BENCH2 (22 survivors caught only with 4 bins).
- Ran: `workload_runner` 35 (incl. the native scratch-race test), `mut_bench2_killers`, `w10_bench_killers` green. Mutation NOT rerun (127 mutants, about 2 hours).
- Gaps after: D67 (owner acceptance), mutation split is a method issue (single-bin vs 4-bin).
- Proposed status: Partial unchanged; Done; Different after the lead accepts D67 via P-35.

### X-bench-cli-guard
- Gaps before: UNC sandbox failure and replace implementation difference.
- Ran: `cli_tool_path_guard` 17 of 17 with `--include-ignored` (the UNC failure from CX-BENCH does not reproduce on this machine); M5 reproduced the 245:25 killer. CL-MUT-BENCH3 decided all 57 (26 targeted run: 19 caught, 6 equivalent, 1 unviable, plus 245:25 caught) - not rerun in full.
- Gaps after: the replace implementation is a probe + POSIX-style `rename` (std `fs::rename` is `MoveFileExW` with replace on Windows, so the visible behaviour matches; the minor note stays documented).
- Proposed status: Partial with updated text; Done is the lead's call (no open mutation or native gap that a machine can close).

### X-benchmark-presenter
- Gaps before: attended run (real iteration, report, real Recycle Bin) only; `dispose` mutants already closed by `w8_rest2_killers`.
- Ran: 74 presenter tests green (above). Mutation (114 mutants of W8-PRESENTER-MUTATION, 2 dispose mutants of CL-REST2RERUN) not rerun here; the presenter tests are plain F tests, so the golden/flag caveats do not affect them. The pr-shell native `benchmark_*_native` tests belong to X-dialogs-zoom-diagnostics-profiles-benchmark and were not run.
- Gaps after: attended W10-B2 only.
- Proposed status: Partial, text shortened to "only the attended run".

## Findings and proposals (no production change)
1. `bench_recycle_bin` has 2 failing tests on Windows 10 19045 (`NUL.txt`, `CON` in a directory). Needs a person to confirm on the C# side which Windows the probe ran on, then either gate by OS build or fix the expectation. Not touched.
2. Compare Gate 0: `parse_summary` should reject a key set/order different from the C# `BenchmarkReport` and a number token not in .NET `R` form (proposal: add a strict key-order walk plus a token check, test-first with a golden produced by the C# oracle; D-W10BENCH decision on whether lenient parsing is intended).
3. Real-clock limiter tests have no hang guard: any future limiter bug makes the integration binaries hang to the job timeout. Consider a global test-time budget.

## Files
- `crates/pr-bench/tests/slow_link_fs.rs` (call budget in the fake clock, test only)
- this file
Mutation outputs: `%TEMP%\mut-out-pr-bench-*`; logs `C:\Users\PC\Projects\pcb-*.log`; target dirs `C:\Users\PC\Projects\target-pc-bench` and `%TEMP%\mut-target-pr-bench` (to delete by the lead).
