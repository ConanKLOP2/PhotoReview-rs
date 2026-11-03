Status: evidence report (2026-11-03/04), task Q4, rows X-perf-diagnostics, F-22, F-23. PARITY.md NOT edited; proposed statuses below. Production code unchanged. Base: origin/master 84d2c151.

# Q4: parity close for X-perf-diagnostics, F-22 (SessionWriter), F-23

Setup: worktree `.claude/worktrees/pc-perf-session`, branch `parity/perf-session`, `CARGO_TARGET_DIR` outside the worktree, `CARGO_BUILD_JOBS=2`, mutation through `tools/dev/mutants.sh` (`-j 1`, `--timeout 90`, `--baseline skip`, unmutated preflight each time). `cargo fmt -p pr-core` was not used (os error 206 risk); `rustfmt --edition 2024 --check` on the two new files: pass.

## Native and ordinary test run (this run, master 84d2c151, `--include-ignored --test-threads=1`)

`diagnostics_perf` 17, `diagnostics_perf_csv` 49 (0 ignored, includes the 1.2 s test), `diagnostics_review_metrics` 14, `mut_perfcom_killers` 2, `mut_writer2_killers` 11, `session_writer` 48 (includes the `#[ignore]` 2 s shutdown-budget test and the real-clock tests `production_thread_timer_writes_after_the_debounce`, `bounded_flush_one_budget_covers_lock_wait_and_save`), `w2_fs_mutation_killers` 17, `w2_perf2_killers` 9, `w2_perf3_killers` 3, `w2_perfcsv_killers` 6: all passed, 0 failed.

## New tests (all in `crates/pr-core/tests`, no sleeps, no wall-clock assertions that can fail on correct code)

| File | Test | Kills |
|---|---|---|
| `q4_perf_killers.rs` | `snapshot_maps_each_fallback_count_to_its_own_backend` | `metrics.rs:353:51 == -> !=` (was golden only) |
| `q4_perf_killers.rs` | `path_id_uppercases_ordinary_letters_and_maps_long_s_to_s` (literals from hashlib) | `perf.rs:115:5 -> Default::default()`, `115:10 == -> !=` (were golden only) |
| `q4_perf_killers.rs` | `long_trace_dir::a_long_plain_trace_directory_gets_its_trace_file`, `an_already_extended_long_trace_directory_is_not_prefixed_twice` (both `#[ignore = "native: ..."]`, real FS, TempRoot) | `perf_csv.rs:439:43`, `439:90` (`&& -> ||`), `439:93` (delete `!`) |
| `q4_perf_timestamp_first_call.rs` | `the_first_timestamp_of_a_process_is_never_zero_or_negative` (single-test binary) | `perf.rs:38:76 + -> -` (probabilistic, see below) |

## Mutation results of this run

| Batch | Selection | Tested | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|---|
| metrics.rs whole file | all, test `diagnostics_review_metrics` | 52 / 52 | 44 | 0 | 8 | 0 |
| perf.rs, `--skip "in sha256"`, tests `diagnostics_perf, w2_fs_mutation_killers, w2_perf2_killers`, budget 2400 s | 108 listed | 102 | 95 | 4 | 3 | 0 |
| proof1: perf.rs lines 115, 38, 723, 736, 752 with the new tests (includes the 6 not reached by the budget) | 14 | 14 | 11 | 2 | 1 | 0 |
| proof2: metrics.rs:353 with `q4_perf_killers` | 1 | 1 | 1 | 0 | 0 | 0 |
| proof3: perf.rs:115 with `q4_perf_killers` only | 2 | 2 | 2 | 0 | 0 | 0 |
| proof4: perf_csv.rs:439 (`--native`) | 4 | 4 | 3 | 1 | 0 | 0 |

"Caught" was checked per mutant with the failing test names: a mutant counts only when a non-golden test failed. Golden-only after the first runs: metrics 353:51 and perf 115:5, 115:10; all three are now caught by a non-golden test (proof2, proof3: failing test is the new killer).

perf.rs overall: 108 listed non-sha256 mutants, all 108 evaluated (102 in the main batch, the 6 left over by the budget: `723:9`, `723:12`, `736:9`, `736:12`, `752:9`, `752:12` in proof1, all caught). Survivors:

- `perf.rs:38:71 - -> /` (`i64::MAX - 1` inside `unwrap_or`): equivalent, the branch needs more than 2^63 ticks of 100 ns (about 29 000 years).
- `perf.rs:38:76 + -> *` (`ticks + 1` becomes `ticks * 1`): not killable deterministically. Output differs from the original only by the +1 offset; the first-call test fails the mutant only if the first call measures under 100 ns. It stayed MISSED on this machine. `+ -> -` was caught.
- `perf.rs:101:24 || -> &&` (`path.is_empty() || path.contains('\0')`): equivalent on this toolchain; probed with rustc: `std::path::absolute("")` and `absolute("a\0b")` both return `Err`, and the `Err` arm returns the raw path, the same result as the guard.
- `perf_csv.rs:439:36 >= -> <`: equivalent, std adds the `\\?\` prefix itself for long paths (the 2 plain/extended tests pass under it).

perf.rs `sha256` (60 mutants at lines 120-159): NOT RUN (excluded for time). The pure hash is covered by `sha256_matches_fips_vectors` (unit) and the golden `perf-path-id`; no mutation verdict.

## Counts per row

### X-perf-diagnostics

Gaps before: 11 perf_csv identities (closed by PERF-DIAGNOSTICS-MUTATION-2026-11-03, 11/11 caught); the 13 earlier misses; `perf.rs`, `metrics.rs` not run; native/UNC probes; `writer.rs` 28 identities (closed by CL-MUT-WRITER2, 28/28).

What ran: see tables. Files: `perf_csv.rs` 87 of 87 have a verdict (no re-run of the 83 earlier ones; 4 of the 13 misses re-run with new killers, 3 now caught), `metrics.rs` 52 of 52 (44 caught, 8 unviable, 0 missed), `perf.rs` 108 of 168 (60 `sha256` NOT RUN; 102+6 evaluated: all caught except the 4 above).

Gaps after (cannot be closed by a machine):
- `perf.rs` sha256: 60 mutants NOT RUN (about 40 minutes at the observed 15-30 s each).
- Unclosed misses with output-only or timing reasons: `perf_csv.rs:39:31` x2 (FILE_BUFFER size), `366:44` (idle flush), `456:5` x2 (`cfg(not(windows))`, no observable on this host), `157:26`, `170:24` (strictly equivalent), `writer.rs:123:38` (timer sleep slice, at most 10 ms later wake-up, not deterministically observable). `38:76 + -> *`.
- UNC (`\\server\share`) trace directory: no UNC share available to the test; needs a person with a share (humanNeeded).
- Timing parity (qpcTicks, ms_since_process_start) is Different by design.

Proposed status: stays `Partial`. Proposed note text: "Q4 2026-11-03: metrics.rs 52/52 tested (44 caught, 8 unviable, 0 missed); perf.rs 108 of 168 (non-sha256 all evaluated, 4 survivors, 3 equivalent, 1 `timestamp +1` unobservable); perf_csv.rs:439 prefix rule killed by 2 native tests (3 of 4 former misses caught, 1 equivalent); 3 golden-only catches now killed by non-golden tests (`q4_perf_killers`). NOT RUN: perf.rs sha256 (60), UNC probe. Evidence docs/reviews/Q4-PARITY-CLOSE-2026-11-03.md".

### F-22 and F-23 (SessionWriter)

Gaps before: "30 identities lack completed verdicts" (CL-MUT-PERFCOM 2 + CL-MUT-WRITER2 28 closed them: 91 of 91 `writer.rs` mutants have a verdict, see CL-MUT-WRITER2-2026-10-31.md); remaining completion/lock/timer/shutdown paths "not exhausted"; timing gap (latency).

What ran in this run: all of `session_writer` (48, incl. `#[ignore]` 2 s test and the real-timer debounce and bounded flush tests), `mut_writer2_killers` 11, `mut_perfcom_killers` 2, `w2_perf2_killers`, `w2_perfcsv_killers`, `w2_fs_mutation_killers`: all passed. `writer.rs` mutation was NOT re-run in this run (time box spent on `metrics.rs` / `perf.rs`); the 91-mutant verdict is the one of CL-MUT-PERFCOM, CL-MUT-WRITER2, CX-PERF2 (not re-verified here).

Gaps after: the PARITY text still says 30 identities lack verdicts, which is stale (91 of 91 now have one); the one genuine open item is `writer.rs:123:38 - -> +` (ThreadTimer sleep slice): output equivalent, wake-up can be up to `TIMER_SLICE` (10 ms) later; a timing upper bound would be flaky, so it is a documented non-killable timing mutant. Real-clock tests exist for the 500 ms path (`production_thread_timer_writes_after_the_debounce`) and the 2 s budgets; they only assert lower bounds and bounded completion. A release-build timing measurement of debounce latency needs an attended session (humanNeeded).

Proposed status F-23: `Partial` kept only for `writer.rs:123:38` (timing-only equivalent) and attended timing; if the owner accepts that mutant as equivalent (timer slice 10 ms), it can be raised to Done. F-22 (writer to store): same, the format/load/sweep row `F-22 (store)` is already Done.

## humanNeeded

1. UNC trace directory (`PHOTOREVIEW_RS_PERF_TRACE=\\server\share\dir`): with a reachable share, run `cargo test -p pr-core --test q4_perf_killers -- --include-ignored` after pointing a copy of `an_already_extended_long_trace_directory_is_not_prefixed_twice` at the share, or start the app with the variable set and confirm a `perf-<pid>-*.csv` appears.
2. Owner decision: accept `writer.rs:123:38` and `perf.rs:38:76 + -> *` as timing/offset equivalents, and `perf.rs` sha256 mutation (60) to run or waive.
3. Optional attended release-build run measuring debounce latency (no PARITY condition requires it in the text, the note says "timing gaps").

## Commands

```
export PY_BIN=$LOCALAPPDATA/Programs/Python/Python312/python.exe TEMP=<outside repo>
MUT_OUT=$TEMP/metrics bash tools/dev/mutants.sh pr-core crates/pr-core/src/diagnostics/metrics.rs --tests diagnostics_review_metrics --budget 1200 --jobs 1
MUT_OUT=$TEMP/perf bash tools/dev/mutants.sh pr-core crates/pr-core/src/diagnostics/perf.rs --tests diagnostics_perf,w2_fs_mutation_killers,w2_perf2_killers --budget 2400 --jobs 1 --skip "in sha256"
```
Proof runs used `--tests q4_perf_killers[,...]` with `-F 'perf.rs:(115|38|723|736|752):'`, `-F 'metrics.rs:353:'`, `--native -F 'perf_csv.rs:439:'`.
Clippy `cargo clippy -p pr-core --all-targets --locked -- -D warnings`: clean.
