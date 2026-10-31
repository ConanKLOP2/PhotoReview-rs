# CX-PERFCSV mutation verification, 2026-10-31

Branch `feat/cx-perfcsv`, source HEAD `7c93960207bdd9fa232459928778fe7562ba3529`.
Scope: `crates/pr-core/src/diagnostics/perf_csv.rs` and `crates/pr-core/src/session/writer.rs` only.
Production code unchanged. No add, commit, push, or edits to the C# checkout.

## Prior evidence and remaining gap

Read `CL-W2RERUN-2026-10-30.md`, the writer audit in `CL-SEC01-2026-10-30.md`, and
`W2-FS-EVIDENCE-2026-10-29.md`. The old whole-file verdict is not a clean current verdict:
CL-W2RERUN explicitly left 76 perf CSV and 80 writer mutants without a rerun, plus three known
timeouts without assertion killers. The 76/80 figures describe the previous coverage gap;
the current full inventories are **87 perf CSV + 91 writer = 178 mutants** (`cargo mutants --list --json`).
Previous results are historical evidence and are not added to this session's measured counts.

The existing X-perf-diagnostics and F-22 write-path Done claims are downgraded to Partial;
F-23 remains Partial. The F-22 store row is outside this lane and is unchanged.
No equivalence or full-file completion is claimed by this session. No native/FFI source was mutated.

## Deterministic assertion killers

New binary: `crates/pr-core/tests/w2_perfcsv_killers.rs`.

| Test | Mutant targeted | Why it asserts instead of hanging |
|---|---|---|
| `dispose_persists_pending_state_and_rejects_later_updates_without_a_live_timer` | writer.rs:602:9 Shared::dispose -> (); :709:9 SessionWriter::dispose -> () | Injected failed timer start leaves pending state without a live timer; after dispose the fake store must contain it and reject a later update. |
| `drop_persists_pending_state_after_a_failed_timer_start` | writer.rs:743:9 Drop::drop -> () | Fake store checked after Drop; no waiting timer. |
| `row_chunks_require_the_eight_kib_buffer_even_when_capacity_mutates_to_zero` | perf_csv.rs:37:32 * -> + and * -> / | Nonblocking in-memory sink records chunk sizes; 600 short rows must produce a chunk >2000 and <=8192 bytes. A zero buffer writes individual rows and fails the assertion. |
| `completion_state_and_zero_budget_wait_agree_before_and_after_complete` | writer.rs:153:9 Completion::complete -> () | Direct state assertions and a zero-budget wait avoid blocking on a completion that a mutant never signals. |
| `numeric_payload_advances_each_column_and_discards_the_fifth_value` | perf_csv.rs:115:31 += -> -= / *= | Calls build_row directly; underflow becomes an immediate failed test instead of a sink waiter timeout. |
| `failed_save_worker_requeues_both_folders_for_the_next_flush` | writer.rs:594:95 == -> != | Injected worker startup failure requeues two different folders; the next flush must save both. No timer contention or time-based assertion. |

The writer tests use `TempRoot` identities and `FakeFileSystem`; the CSV test uses only an in-memory sink.
No sleeps, mtimes, wall-clock assertions, unsafe code, environment mutation, real user configuration, or Recycle Bin operations.
The C# baseline was read at `7eaee2a2:src/PhotoReview.Core/Session/SessionWriter.cs` (Dispose marks disposed,
flushes, then waits; Update ignores further states). Test comments name the C# tests and mutant locations/operators.

## Commands and validation

Normal builds: `CARGO_TARGET_DIR=<worktree>/target`; mutation builds: `<worktree>/target-mut`.
Both use `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, `CARGO_INCREMENTAL=0`.
Only one cargo-mutants process was active at a time; `-j 1`, timeout 90 seconds per mutant.
No normal cargo test was run with target-mut. No other lane's processes or artifacts were touched.

`powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev/stage-native.ps1` passed and verified both DLLs
in target/debug, target/debug/deps and native/x64. The initial attempts failed Git ownership checking;
the successful invocation used process-local `GIT_CONFIG_COUNT` safe.directory entries for this worktree
and the read-only C# checkout, without changing global Git settings.

Preflight after staging:

```text
cargo test -p pr-core --locked -j 2 --test diagnostics_perf_csv --test session_writer -- --include-ignored --test-threads=1
diagnostics_perf_csv: 49 passed, 0 ignored
session_writer:      48 passed, 0 ignored
```

Mutation run A (killers binary alone):

```text
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 \
  --file '**/diagnostics/perf_csv.rs' --file '**/session/writer.rs' \
  -F 'perf_csv.rs:37:32|writer.rs:(602|709|743):9' \
  --cargo-arg=--test=w2_perfcsv_killers --output mut-out-timeout-killers
```

Found 5; 5 caught, 0 missed/unviable/timeouts. Each log contains actual `running 3 tests`
and an assertion failure, exit 101. This closes the three specifically reproduced timeout gaps
with assert-style detection; it does not close the other historical timeout or coverage gaps.

Mutation run B (remaining selected pure logic):

```text
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 \
  --file '**/diagnostics/perf_csv.rs' --file '**/session/writer.rs' \
  -F 'build_row|key_of|Shared::update|Shared::save_batch|Shared::requeue_skipped' \
  --cargo-arg=--test=diagnostics_perf_csv --cargo-arg=--test=session_writer \
  --cargo-arg=--test=w2_perfcsv_killers \
  --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1 \
  --output mut-out-payload-session
```

Found 21 (10 perf CSV, 11 writer), matching the list filter. Neither source file contains a
`#[cfg(test)]` unit-test module, so no extra `--lib` mutation run is needed for these two files.
Raw artifacts are retained under each output directory's `mutants.out` (outcomes.json, logs, diffs).

Run B encountered four timeouts despite earlier assertion failures in the same logs:
perf_csv.rs:115:31 += -> -=, writer.rs:384:9 update -> Ok(()), :569:9 save_batch -> (), and
:571:46 != -> == in save_batch. They remain timeouts in the run B table; a later killers-only
verdict is recorded separately rather than changing the original result.

Mutation run C (final killers binary alone, timeout assertion proof and flush composition):

```text
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 \
  --file '**/diagnostics/perf_csv.rs' --file '**/session/writer.rs' \
  -F 'perf_csv.rs:115:|writer.rs:(153|384|470|479|507|569|571):|writer.rs:594:95' \
  --cargo-arg=--test=w2_perfcsv_killers --output mut-out-completion-flush
```

The extra test binaries added after run A/B do not change their captured source trees; those logs
show the earlier three-test killers binary. Run C uses the final six-test binary.

Final measured results and the exact NOT RUN inventory follow below.

## Measured verdict

| Run | Tested | Caught | Missed | Unviable | Timeout |
|---|---:|---:|---:|---:|---:|
| `mut-out-timeout-killers` | 5 | 5 | 0 | 0 | 0 |
| `mut-out-payload-session` | 21 | 15 | 1 | 1 | 4 |
| `mut-out-completion-flush` | 10 | 10 | 0 | 0 | 0 |

Run B missed writer.rs:594:95 == -> != in requeue_skipped. Run C catches it with
`failed_save_worker_requeues_both_folders_for_the_next_flush`, using only the final killers binary.
Run C also catches all four run B timeouts, plus the historical Completion::complete and flush no-op gaps.
There were 36 evaluated attempts, covering 30 distinct mutants. The latest measured result for each distinct mutant follows.
Historical results are not combined with this session.

| Source | Tested / total | Caught | Missed | Unviable | Timeout | NOT RUN |
|---|---:|---:|---:|---:|---:|---:|
| `perf_csv.rs` | 12 / 87 | 11 | 0 | 1 | 0 | 75 |
| `writer.rs` | 18 / 91 | 18 | 0 | 0 | 0 | 73 |

**Final distinct verdict: 30/178 evaluated, 29 caught, 0 unresolved missed, 1 unviable, 0 unresolved timeouts; 148 NOT RUN.**
Every caught attempt was checked for actual nonzero test execution, a failed test result, and exit 101.
Unviable is not counted as caught: perf_csv.rs:94:5 build_row -> Default::default() does not compile because PerfRow has no Default implementation.
No mutant is declared equivalent by this session. Production source is unchanged.

The remaining 75 perf CSV and 73 writer mutants are NOT RUN in this session. They were skipped to keep the total lane inside
its roughly 45-minute budget, allowing the final killers-only confirmations and gates. The exact list appears below.
The earlier 76/80 figures describe a previous coverage gap and cannot simply be reduced by these counts, because selected reruns overlap earlier selections.
Other historical timeout mutants, exhaustive idle-flush/native coverage, UNC probes, perf.rs and metrics.rs mutation,
and workspace-wide gates were NOT RUN. No full mutation or parity verdict is claimed.

Final gates (all PASS):

- `cargo fmt -p pr-core --check`.
- `cargo clippy -p pr-core --all-targets --locked -j 2 -- -D warnings`.
- Final `w2_perfcsv_killers`: 6 passed, repeated 20/20 times (120 test executions). Earlier three-/five-test repetitions are not added to this final count.
- `python -I tools/dev/doc-links.py --quiet`: 534 files, 0 errors; 174 proposed references and 41 archive files reported separately by the checker.

Retained local artifacts: all three mutation output directories, plus the preflight and repetition logs moved under `mut-out-timeout-killers`.
Only the lane test, its evidence report, the three lane PARITY rows, one new task status line, and a verbatim archive move are intended handoff changes.

## Exact mutant inventory

Every current listed mutant is included. Paths below are relative to crates/pr-core/src.
NOT RUN describes this session only and does not assert that no historical test ever ran it.

| Mutant | Latest verdict this session |
|---|---|
| `diagnostics/perf_csv.rs:37:32: replace * with +` | caught (assert/test) |
| `diagnostics/perf_csv.rs:37:32: replace * with /` | caught (assert/test) |
| `diagnostics/perf_csv.rs:39:31: replace * with +` | NOT RUN |
| `diagnostics/perf_csv.rs:39:31: replace * with /` | NOT RUN |
| `diagnostics/perf_csv.rs:94:5: replace build_row -> PerfRow with Default::default()` | unviable |
| `diagnostics/perf_csv.rs:106:51: replace && with \|\| in build_row` | caught (assert/test) |
| `diagnostics/perf_csv.rs:106:32: replace \|\| with && in build_row` | caught (assert/test) |
| `diagnostics/perf_csv.rs:106:23: replace == with != in build_row` | caught (assert/test) |
| `diagnostics/perf_csv.rs:106:41: replace == with != in build_row` | caught (assert/test) |
| `diagnostics/perf_csv.rs:108:41: replace && with \|\| in build_row` | caught (assert/test) |
| `diagnostics/perf_csv.rs:108:29: replace == with != in build_row` | caught (assert/test) |
| `diagnostics/perf_csv.rs:115:31: replace += with -= in build_row` | caught (assert/test) |
| `diagnostics/perf_csv.rs:115:31: replace += with *= in build_row` | caught (assert/test) |
| `diagnostics/perf_csv.rs:116:23: delete ! in build_row` | caught (assert/test) |
| `diagnostics/perf_csv.rs:141:5: replace format_value -> String with String::new()` | NOT RUN |
| `diagnostics/perf_csv.rs:141:5: replace format_value -> String with "xyzzy".into()` | NOT RUN |
| `diagnostics/perf_csv.rs:153:5: replace format_g17 -> String with String::new()` | NOT RUN |
| `diagnostics/perf_csv.rs:153:5: replace format_g17 -> String with "xyzzy".into()` | NOT RUN |
| `diagnostics/perf_csv.rs:157:26: replace > with == in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:157:26: replace > with < in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:157:26: replace > with >= in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:160:14: replace == with != in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:169:61: replace != with == in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:170:28: replace && with \|\| in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:170:24: replace > with == in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:170:24: replace > with < in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:170:24: replace > with >= in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:173:19: delete ! in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:173:21: delete - in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:174:36: replace > with == in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:174:36: replace > with < in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:174:36: replace > with >= in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:186:17: replace >= with < in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:187:34: replace + with - in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:187:34: replace + with * in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:188:25: replace <= with > in format_g17` | NOT RUN |
| `diagnostics/perf_csv.rs:202:5: replace csv_escape -> String with String::new()` | NOT RUN |
| `diagnostics/perf_csv.rs:202:5: replace csv_escape -> String with "xyzzy".into()` | NOT RUN |
| `diagnostics/perf_csv.rs:220:5: replace current_thread_number -> i32 with 0` | NOT RUN |
| `diagnostics/perf_csv.rs:220:5: replace current_thread_number -> i32 with 1` | NOT RUN |
| `diagnostics/perf_csv.rs:220:5: replace current_thread_number -> i32 with -1` | NOT RUN |
| `diagnostics/perf_csv.rs:225:23: replace == with != in current_thread_number` | NOT RUN |
| `diagnostics/perf_csv.rs:241:9: replace <impl std::fmt::Debug for PerfCsvListener>::fmt -> std::fmt::Result with Ok(Default::default())` | NOT RUN |
| `diagnostics/perf_csv.rs:273:9: replace Shared::signal_finished with ()` | NOT RUN |
| `diagnostics/perf_csv.rs:279:9: replace Shared::count_dropped with ()` | NOT RUN |
| `diagnostics/perf_csv.rs:284:9: replace Shared::enqueue with ()` | NOT RUN |
| `diagnostics/perf_csv.rs:284:12: delete ! in Shared::enqueue` | NOT RUN |
| `diagnostics/perf_csv.rs:311:9: replace <impl PerfListener for Shared>::on_event_written with ()` | NOT RUN |
| `diagnostics/perf_csv.rs:317:5: replace diag_field -> String with String::new()` | NOT RUN |
| `diagnostics/perf_csv.rs:317:5: replace diag_field -> String with "xyzzy".into()` | NOT RUN |
| `diagnostics/perf_csv.rs:319:50: delete ! in diag_field` | NOT RUN |
| `diagnostics/perf_csv.rs:325:5: replace write_row -> io::Result<()> with Ok(())` | NOT RUN |
| `diagnostics/perf_csv.rs:349:5: replace writer_loop with ()` | NOT RUN |
| `diagnostics/perf_csv.rs:366:44: replace > with == in writer_loop` | NOT RUN |
| `diagnostics/perf_csv.rs:366:44: replace > with < in writer_loop` | NOT RUN |
| `diagnostics/perf_csv.rs:366:44: replace > with >= in writer_loop` | NOT RUN |
| `diagnostics/perf_csv.rs:379:27: replace += with -= in writer_loop` | NOT RUN |
| `diagnostics/perf_csv.rs:379:27: replace += with *= in writer_loop` | NOT RUN |
| `diagnostics/perf_csv.rs:381:33: replace && with \|\| in writer_loop` | NOT RUN |
| `diagnostics/perf_csv.rs:381:51: replace >= with < in writer_loop` | NOT RUN |
| `diagnostics/perf_csv.rs:410:5: replace guarded -> io::Result<()> with Ok(())` | NOT RUN |
| `diagnostics/perf_csv.rs:424:5: replace create_trace_file -> Option<(std::fs::File, std::path::PathBuf)> with None` | NOT RUN |
| `diagnostics/perf_csv.rs:424:5: replace create_trace_file -> Option<(std::fs::File, std::path::PathBuf)> with Some((Default::default(), Default::default()))` | NOT RUN |
| `diagnostics/perf_csv.rs:439:90: replace && with \|\| in create_trace_file` | NOT RUN |
| `diagnostics/perf_csv.rs:439:43: replace && with \|\| in create_trace_file` | NOT RUN |
| `diagnostics/perf_csv.rs:439:36: replace >= with < in create_trace_file` | NOT RUN |
| `diagnostics/perf_csv.rs:439:93: delete ! in create_trace_file` | NOT RUN |
| `diagnostics/perf_csv.rs:456:5: replace create_trace_file -> Option<(std::fs::File, std::path::PathBuf)> with None` | NOT RUN |
| `diagnostics/perf_csv.rs:456:5: replace create_trace_file -> Option<(std::fs::File, std::path::PathBuf)> with Some((Default::default(), Default::default()))` | NOT RUN |
| `diagnostics/perf_csv.rs:478:9: replace PerfCsvListener::try_start -> Option<PerfCsvListener> with None` | NOT RUN |
| `diagnostics/perf_csv.rs:478:9: replace PerfCsvListener::try_start -> Option<PerfCsvListener> with Some(Default::default())` | NOT RUN |
| `diagnostics/perf_csv.rs:511:9: replace PerfCsvListener::try_start_from_environment -> Option<PerfCsvListener> with None` | NOT RUN |
| `diagnostics/perf_csv.rs:511:9: replace PerfCsvListener::try_start_from_environment -> Option<PerfCsvListener> with Some(Default::default())` | NOT RUN |
| `diagnostics/perf_csv.rs:529:9: replace PerfCsvListener::start_for_test -> PerfCsvListener with Default::default()` | NOT RUN |
| `diagnostics/perf_csv.rs:542:9: replace PerfCsvListener::unstarted -> PerfCsvListener with Default::default()` | NOT RUN |
| `diagnostics/perf_csv.rs:556:9: replace PerfCsvListener::start -> io::Result<PerfCsvListener> with Ok(Default::default())` | NOT RUN |
| `diagnostics/perf_csv.rs:597:9: replace PerfCsvListener::enqueue with ()` | NOT RUN |
| `diagnostics/perf_csv.rs:602:9: replace PerfCsvListener::dropped_count -> i64 with 0` | NOT RUN |
| `diagnostics/perf_csv.rs:602:9: replace PerfCsvListener::dropped_count -> i64 with 1` | NOT RUN |
| `diagnostics/perf_csv.rs:602:9: replace PerfCsvListener::dropped_count -> i64 with -1` | NOT RUN |
| `diagnostics/perf_csv.rs:607:9: replace PerfCsvListener::writer_faulted -> bool with true` | NOT RUN |
| `diagnostics/perf_csv.rs:607:9: replace PerfCsvListener::writer_faulted -> bool with false` | NOT RUN |
| `diagnostics/perf_csv.rs:615:9: replace PerfCsvListener::dispose with ()` | NOT RUN |
| `diagnostics/perf_csv.rs:628:9: replace PerfCsvListener::wait_finished -> bool with true` | NOT RUN |
| `diagnostics/perf_csv.rs:628:9: replace PerfCsvListener::wait_finished -> bool with false` | NOT RUN |
| `diagnostics/perf_csv.rs:631:60: delete ! in PerfCsvListener::wait_finished` | NOT RUN |
| `diagnostics/perf_csv.rs:639:9: replace <impl Drop for PerfCsvListener>::drop with ()` | NOT RUN |
| `session/writer.rs:46:9: replace <impl fmt::Display for InvalidDebounce>::fmt -> fmt::Result with Ok(Default::default())` | NOT RUN |
| `session/writer.rs:94:9: replace <impl DebounceTimer for ThreadTimer>::start -> Result<Box<dyn RunningTimer>, TimerError> with Ok(Box::new(Default::default()))` | NOT RUN |
| `session/writer.rs:114:9: replace <impl RunningTimer for ThreadTimerRun>::wait -> Result<TimerOutcome, TimerError> with Ok(Default::default())` | NOT RUN |
| `session/writer.rs:120:24: replace >= with < in <impl RunningTimer for ThreadTimerRun>::wait` | NOT RUN |
| `session/writer.rs:123:38: replace - with + in <impl RunningTimer for ThreadTimerRun>::wait` | NOT RUN |
| `session/writer.rs:123:38: replace - with / in <impl RunningTimer for ThreadTimerRun>::wait` | NOT RUN |
| `session/writer.rs:142:9: replace Completion::completed -> Self with Default::default()` | NOT RUN |
| `session/writer.rs:148:9: replace Completion::flag -> MutexGuard<'_, bool> with MutexGuard::new()` | NOT RUN |
| `session/writer.rs:148:9: replace Completion::flag -> MutexGuard<'_, bool> with MutexGuard::from_iter([true])` | NOT RUN |
| `session/writer.rs:148:9: replace Completion::flag -> MutexGuard<'_, bool> with MutexGuard::new(true)` | NOT RUN |
| `session/writer.rs:148:9: replace Completion::flag -> MutexGuard<'_, bool> with MutexGuard::from(true)` | NOT RUN |
| `session/writer.rs:148:9: replace Completion::flag -> MutexGuard<'_, bool> with MutexGuard::from_iter([false])` | NOT RUN |
| `session/writer.rs:148:9: replace Completion::flag -> MutexGuard<'_, bool> with MutexGuard::new(false)` | NOT RUN |
| `session/writer.rs:148:9: replace Completion::flag -> MutexGuard<'_, bool> with MutexGuard::from(false)` | NOT RUN |
| `session/writer.rs:153:9: replace Completion::complete with ()` | caught (assert/test) |
| `session/writer.rs:161:9: replace Completion::is_complete -> bool with true` | NOT RUN |
| `session/writer.rs:161:9: replace Completion::is_complete -> bool with false` | NOT RUN |
| `session/writer.rs:166:9: replace Completion::wait with ()` | NOT RUN |
| `session/writer.rs:167:15: delete ! in Completion::wait` | NOT RUN |
| `session/writer.rs:174:9: replace Completion::wait_timeout -> bool with true` | NOT RUN |
| `session/writer.rs:174:9: replace Completion::wait_timeout -> bool with false` | NOT RUN |
| `session/writer.rs:176:15: delete ! in Completion::wait_timeout` | NOT RUN |
| `session/writer.rs:197:9: replace <impl Drop for CompleteOnDrop>::drop with ()` | NOT RUN |
| `session/writer.rs:207:9: replace <impl fmt::Display for PanicError>::fmt -> fmt::Result with Ok(Default::default())` | NOT RUN |
| `session/writer.rs:229:9: replace WriteLock::held -> MutexGuard<'_, bool> with MutexGuard::new()` | NOT RUN |
| `session/writer.rs:229:9: replace WriteLock::held -> MutexGuard<'_, bool> with MutexGuard::from_iter([true])` | NOT RUN |
| `session/writer.rs:229:9: replace WriteLock::held -> MutexGuard<'_, bool> with MutexGuard::new(true)` | NOT RUN |
| `session/writer.rs:229:9: replace WriteLock::held -> MutexGuard<'_, bool> with MutexGuard::from(true)` | NOT RUN |
| `session/writer.rs:229:9: replace WriteLock::held -> MutexGuard<'_, bool> with MutexGuard::from_iter([false])` | NOT RUN |
| `session/writer.rs:229:9: replace WriteLock::held -> MutexGuard<'_, bool> with MutexGuard::new(false)` | NOT RUN |
| `session/writer.rs:229:9: replace WriteLock::held -> MutexGuard<'_, bool> with MutexGuard::from(false)` | NOT RUN |
| `session/writer.rs:234:9: replace WriteLock::acquire with ()` | NOT RUN |
| `session/writer.rs:243:9: replace WriteLock::acquire_timeout -> bool with true` | NOT RUN |
| `session/writer.rs:243:9: replace WriteLock::acquire_timeout -> bool with false` | NOT RUN |
| `session/writer.rs:262:9: replace WriteLock::release with ()` | NOT RUN |
| `session/writer.rs:272:9: replace <impl Drop for ReleaseOnDrop<'_>>::drop with ()` | NOT RUN |
| `session/writer.rs:324:9: replace <impl fmt::Debug for SessionWriter>::fmt -> fmt::Result with Ok(Default::default())` | NOT RUN |
| `session/writer.rs:336:5: replace key_of -> String with String::new()` | caught (assert/test) |
| `session/writer.rs:336:5: replace key_of -> String with "xyzzy".into()` | caught (assert/test) |
| `session/writer.rs:344:5: replace spawn_save_thread -> io::Result<Completion> with Ok(Default::default())` | NOT RUN |
| `session/writer.rs:360:9: replace <impl Drop for InFlightGuard<'_>>::drop with ()` | NOT RUN |
| `session/writer.rs:369:9: replace Shared::gate -> MutexGuard<'_, GateState> with MutexGuard::new()` | NOT RUN |
| `session/writer.rs:369:9: replace Shared::gate -> MutexGuard<'_, GateState> with MutexGuard::from_iter([Default::default()])` | NOT RUN |
| `session/writer.rs:369:9: replace Shared::gate -> MutexGuard<'_, GateState> with MutexGuard::new(Default::default())` | NOT RUN |
| `session/writer.rs:369:9: replace Shared::gate -> MutexGuard<'_, GateState> with MutexGuard::from(Default::default())` | NOT RUN |
| `session/writer.rs:373:9: replace Shared::hooks -> MutexGuard<'_, Hooks> with MutexGuard::new()` | NOT RUN |
| `session/writer.rs:373:9: replace Shared::hooks -> MutexGuard<'_, Hooks> with MutexGuard::from_iter([Default::default()])` | NOT RUN |
| `session/writer.rs:373:9: replace Shared::hooks -> MutexGuard<'_, Hooks> with MutexGuard::new(Default::default())` | NOT RUN |
| `session/writer.rs:373:9: replace Shared::hooks -> MutexGuard<'_, Hooks> with MutexGuard::from(Default::default())` | NOT RUN |
| `session/writer.rs:377:9: replace Shared::log_error with ()` | NOT RUN |
| `session/writer.rs:384:9: replace Shared::update -> Result<(), BlankFolder> with Ok(())` | caught (assert/test) |
| `session/writer.rs:395:56: replace == with != in Shared::update` | caught (assert/test) |
| `session/writer.rs:441:9: replace Shared::run_timer with ()` | NOT RUN |
| `session/writer.rs:449:76: replace == with != in Shared::run_timer` | NOT RUN |
| `session/writer.rs:457:16: delete ! in Shared::run_timer` | NOT RUN |
| `session/writer.rs:457:73: replace == with != in Shared::run_timer` | NOT RUN |
| `session/writer.rs:470:9: replace Shared::flush with ()` | caught (assert/test) |
| `session/writer.rs:479:9: replace Shared::write_pending with ()` | caught (assert/test) |
| `session/writer.rs:489:16: delete ! in Shared::write_pending` | NOT RUN |
| `session/writer.rs:490:32: replace += with -= in Shared::write_pending` | NOT RUN |
| `session/writer.rs:490:32: replace += with *= in Shared::write_pending` | NOT RUN |
| `session/writer.rs:507:9: replace Shared::write_batch with ()` | caught (assert/test) |
| `session/writer.rs:510:12: delete ! in Shared::write_batch` | NOT RUN |
| `session/writer.rs:524:12: delete ! in Shared::write_batch` | NOT RUN |
| `session/writer.rs:559:12: delete ! in Shared::write_batch` | NOT RUN |
| `session/writer.rs:569:9: replace Shared::save_batch with ()` | caught (assert/test) |
| `session/writer.rs:571:46: replace != with == in Shared::save_batch` | caught (assert/test) |
| `session/writer.rs:589:9: replace Shared::requeue_skipped with ()` | caught (assert/test) |
| `session/writer.rs:594:56: replace && with \|\| in Shared::requeue_skipped` | caught (assert/test) |
| `session/writer.rs:594:39: replace == with != in Shared::requeue_skipped` | caught (assert/test) |
| `session/writer.rs:594:59: delete ! in Shared::requeue_skipped` | caught (assert/test) |
| `session/writer.rs:594:95: replace == with != in Shared::requeue_skipped` | caught (assert/test) |
| `session/writer.rs:602:9: replace Shared::dispose with ()` | caught (assert/test) |
| `session/writer.rs:617:9: replace Shared::wait_for_in_flight with ()` | NOT RUN |
| `session/writer.rs:619:27: replace == with != in Shared::wait_for_in_flight` | NOT RUN |
| `session/writer.rs:626:30: replace > with == in Shared::wait_for_in_flight` | NOT RUN |
| `session/writer.rs:626:30: replace > with < in Shared::wait_for_in_flight` | NOT RUN |
| `session/writer.rs:626:30: replace > with >= in Shared::wait_for_in_flight` | NOT RUN |
| `session/writer.rs:680:9: replace SessionWriter::update -> Result<(), BlankFolder> with Ok(())` | NOT RUN |
| `session/writer.rs:686:9: replace SessionWriter::flush with ()` | NOT RUN |
| `session/writer.rs:693:9: replace SessionWriter::flush_async -> io::Result<JoinHandle<()>> with Ok(JoinHandle::new())` | NOT RUN |
| `session/writer.rs:693:9: replace SessionWriter::flush_async -> io::Result<JoinHandle<()>> with Ok(JoinHandle::from_iter([()]))` | NOT RUN |
| `session/writer.rs:693:9: replace SessionWriter::flush_async -> io::Result<JoinHandle<()>> with Ok(JoinHandle::new(()))` | NOT RUN |
| `session/writer.rs:693:9: replace SessionWriter::flush_async -> io::Result<JoinHandle<()>> with Ok(JoinHandle::from(()))` | NOT RUN |
| `session/writer.rs:701:9: replace SessionWriter::when_idle with ()` | NOT RUN |
| `session/writer.rs:709:9: replace SessionWriter::dispose with ()` | caught (assert/test) |
| `session/writer.rs:716:9: replace SessionWriter::set_bounded_wait_for_tests with ()` | NOT RUN |
| `session/writer.rs:723:9: replace SessionWriter::set_after_drain_for_tests with ()` | NOT RUN |
| `session/writer.rs:730:9: replace SessionWriter::set_before_shutdown_wait_for_tests with ()` | NOT RUN |
| `session/writer.rs:736:9: replace SessionWriter::set_save_spawner_for_tests with ()` | NOT RUN |
| `session/writer.rs:743:9: replace <impl Drop for SessionWriter>::drop with ()` | caught (assert/test) |
