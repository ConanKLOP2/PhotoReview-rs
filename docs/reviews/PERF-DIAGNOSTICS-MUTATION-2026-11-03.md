Status: evidence report (2026-11-03), lane mut-perf-diagnostics. Complete for the 11 identities named below; PARITY status NOT raised. Production code and tests unchanged (no survivor, so no test was added).

# X-perf-diagnostics: re-run of the 11 identities without a completed verdict

Question: the PARITY row X-perf-diagnostics says "11 identities still lack completed verdicts". Those are the 11 `perf_csv.rs` mutants that `docs/reviews/CX-PERF2-2026-10-31.md` never ran (its "64/75 previously NOT RUN evaluated", so 11 of 75 left): `PerfCsvListener::{dropped_count x3, writer_faulted x2, dispose, wait_finished x3, Drop::drop}` plus `PerfCsvListener::enqueue` (597, started there without a verdict). `docs/reviews/CL-MUT-PERFCOM-2026-10-31.md` had already run them (15 of 15 selected caught, with the 4 former timeouts); this report repeats the measurement on current master (`0e8c3f7`) with exactly the two test binaries named in the task.

## Result (from `mutants.out/outcomes.json` via `tools/dev/mutants.sh`)

| Tested / total | Caught | Missed | Unviable | Timeouts | Not run |
|---|---|---|---|---|---|
| 11 / 11 | 11 | 0 | 0 | 0 | 0 of the selection |

Preflight (unmutated, same arguments): `cargo test -p pr-core --test w2_perf2_killers --test w2_perfcsv_killers`, 15 tests executed (9 + 6), exit 0. The mutation phase took 5 minutes (`-j 1`, `--timeout 90`, `--baseline skip`, budget 1800 s). cargo-mutants exit code 0.

Every mutant log was checked: it contains `running 9 tests` (the `w2_perf2_killers` binary fails first, cargo stops before the second binary), `test result: FAILED` and `Failure(101)`; none was a timeout and none was caught by a golden. Failing tests per mutant (all in `crates/pr-core/tests/w2_perf2_killers.rs`):

| Mutant (`diagnostics/perf_csv.rs`) | Failing killer(s) |
|---|---|
| `597:9 PerfCsvListener::enqueue -> ()` | `dropping_a_listener_drains_its_row_and_writes_the_trailer`, `failing_sink_counts_the_failed_row_and_every_queued_row`, `listener_drains_rows_and_accounts_each_post_dispose_drop` |
| `602:9 dropped_count -> 0`, `-> 1`, `-> -1` (3 mutants) | `failing_sink_counts_the_failed_row_and_every_queued_row`, `listener_drains_rows_and_accounts_each_post_dispose_drop` (each of the 3) |
| `607:9 writer_faulted -> true` / `-> false` | `listener_drains_rows_and_accounts_each_post_dispose_drop` (one of them) / `failing_sink_counts_the_failed_row_and_every_queued_row` (the other); the log names are `_col_9` and `_col_9_001` |
| `615:9 dispose -> ()` | the three tests of the `597` row |
| `628:9 wait_finished -> true` / `-> false` | the three tests of the `597` row (each) |
| `631:60 delete ! in wait_finished` | the three tests of the `597` row |
| `639:9 <impl Drop for PerfCsvListener>::drop -> ()` | `dropping_a_listener_drains_its_row_and_writes_the_trailer` |

`w2_perfcsv_killers` was part of the test command but is not what caught these (the first failing binary stops the run).

## Not covered by this report (still open, nothing here changes them)

- Only these 11 of the 87 `perf_csv.rs` mutants were run. The 13 earlier misses (ST-PERFKILL / CX-PERF2: `157:26 > -> >=`, `170:24 > -> >=`, `writer.rs:123:38`, `39:31` x2 buffer size, `366:44`, `439:*`, `456:5` x2 and others) were NOT re-run and keep their explanations in CX-PERF2.
- `perf.rs` and `metrics.rs`: NOT run (no mutation verdict).
- `session/writer.rs` (the other file of CX-PERF2): 28 identities from CL-MUT-PERFCOM still NOT RUN.
- Native/UNC path probes: NOT run.

## Command (worktree root, `PY_BIN` = Python 3.12, `CARGO_BUILD_JOBS=3`)

```
TEMP=<repo>/.claude/tmp-mut-perfdiag MUT_OUT=$TEMP/out bash tools/dev/mutants.sh pr-core \
  crates/pr-core/src/diagnostics/perf_csv.rs --tests w2_perf2_killers,w2_perfcsv_killers \
  --budget 1800 --jobs 1 -F 'perf_csv.rs:(597|602|607|615|628|631|639):'
```

`cargo mutants --list` with the same filter printed exactly the 11 lines above (597, 602 x3, 607 x2, 615, 628 x2, 631, 639) before the run.
