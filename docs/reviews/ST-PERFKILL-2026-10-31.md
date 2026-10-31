# ST-PERFKILL: 13 missed and 4 timeout mutants of PR #436, 2026-10-31

Status: Partial. Evidence report of lane `st-perfkill` (branch `st-perfkill`, from master `5f26d474`, which already contains PR #436 and `docs/reviews/CX-PERF2-2026-10-31.md`). No production code changed. No PARITY row changes (X-perf-diagnostics, F-23, F-22 write-path stay Partial).

Method: static reading of `crates/pr-core/src/diagnostics/perf_csv.rs` and `crates/pr-core/src/session/writer.rs`, then one new test file, `crates/pr-core/tests/w2_perf3_killers.rs`. **cargo-mutants was NOT run.** Build and test were limited to `cargo test -p pr-core --locked -j 2 --test w2_perf3_killers` and `cargo fmt -p pr-core --check` (fmt: see "Gates"). Where a verdict below is an argument that was not executed it is marked "reasoning, not run".

## New tests (`w2_perf3_killers.rs`, 3 tests, 3 passed)

All waits are bounded `wait_finished(5 s)` condvar waits (return in microseconds when the code is right). No sleep, no clock, no polling, no real user data; the sinks are in-memory.

| Test | Kills |
|---|---|
| `finish_signal_and_queued_row_are_observable_within_a_bound` | `perf_csv.rs:273:9` signal_finished -> (); `:284:9` enqueue -> (); `:284:12` delete ! (all 3 were timeouts). Uses `dispose_wait = ZERO`, so only the bounded wait can see the signal; also checks header, row bytes and `# dropped=0\r\n` trailer. |
| `unstarted_listener_is_already_finished_and_counts_no_rows` | guards the `finished = !started` initial state and the silent-ignore of a never-started listener (supporting, no specific mutant claimed). |
| `panicking_sink_is_treated_as_a_write_failure_and_every_row_is_dropped` | `perf_csv.rs:410:9` guarded -> Ok(()) (timeout). A sink whose `write` panics must set `writer_faulted` and drop both 20 000-char rows. |

## Hand-applied mutants (NOT cargo-mutants)

I edited the source by hand, ran the binary once per change, then ran `git checkout -- crates/pr-core/src/diagnostics/perf_csv.rs` (verified: 0 diffs after each). This is a manual spot check of 3 mutants, not a mutation run.

| Hand-applied change | Result of `w2_perf3_killers` |
|---|---|
| `signal_finished` body removed (273:9) | 2 FAILED (`finish_signal...` at the `wait_finished` assertion after the 5 s bound, `panicking_sink...` likewise); finished in 5.03 s, no hang |
| `Shared::enqueue` returns at once (284:9) | 2 FAILED (row missing from output; `writer_faulted` false), 0.00 s |
| `guarded` returns `Ok(())` without running `op` (410:9) | 2 FAILED (row missing; `assertion failed: listener.writer_faulted()`), 0.00 s |

The unmodified binary: 3 passed. Not hand-applied (reasoning, not run): `284:12 delete !` behaves like the 284:9 case (every started listener returns before queueing).

## Verdict per missed mutant (13) and timeout (4)

Classes: A = truly equivalent (argument), B = observable, killed by a test, C = not observable through public API/output without time control, reason given. No class-A or class-C item is counted as caught.

| Mutant | Class | Argument |
|---|---|---|
| `perf_csv.rs:157:26` `>` -> `>=` | A | Reached only when `value.is_infinite()`; infinity is never `0.0`, so `v > 0.0` and `v >= 0.0` agree. Output identical. |
| `perf_csv.rs:170:24` `>` -> `>=` | A | `digits` is the 17-digit mantissa of a nonzero finite `abs` value (zero returned earlier at line 160), so its first digit is 1..9. `len >= 1` differs from `len > 1` only when `len == 1`, where `ends_with('0')` is false. The loop runs the same iterations. |
| `perf_csv.rs:39:31` `*` -> `+` (1088) and `*` -> `/` (0) | C | `FILE_BUFFER` is only the capacity of the inner `BufWriter` around the real file. The byte stream written on flush/dispose is identical. The only difference is when bytes become visible on disk before a flush, which needs a way to know the writer thread has consumed the rows without flushing; the only public barrier (`dispose`/`wait_finished`) flushes. Observing it needs a wait or poll on a clock. Not claimed equivalent (I/O grouping differs). |
| `perf_csv.rs:366:44` `>` -> `==` and `>` -> `>=` | C | Both change only which idle ticks call `writer.flush()` (`==`: the tail flush is delayed by one interval, then empty flushes every tick; `>=`: empty flushes). Final bytes are identical. The difference is the number of `sink.flush()` calls on idle ticks. I looked for a deterministic design with a counting sink and `idle_flush = ZERO`; every variant needs an idle tick to have happened between two events, and no public hook orders a tick (a queued row suppresses the tick). A negative wait would be a timed assertion, so I did not write it. A test would be possible only as `#[ignore = "slow"]` with a bounded negative wait. |
| `perf_csv.rs:439:36 >= -> <`, `:439:43 && -> \|\|`, `:439:90 && -> \|\|`, `:439:93 delete !` (4) | C | These decide whether the `\?\` (or `\?\UNC\`) prefix is added by hand for a path of 260 or more UTF-16 units. For a normalized local path the file opened is the same either way: std adds the verbatim prefix itself for long paths, and a prefixed short path names the same file. Prefix semantics differ only for UNC input and for middle segments ending in space/dot, which cannot be set up with `TempRoot` on a local drive without a probe I did not run. Equivalence is NOT claimed (the UNC gap stays). |
| `perf_csv.rs:456:5` -> `None` and -> `Some(Default)` (2) | C | The function is `#[cfg(not(windows))]`; it is not compiled on the Windows target, so no test on this host can observe it. A Linux/macOS run would be needed. |
| `writer.rs:123:38` `-` -> `+` | C (outcome A) | `sleep((span + elapsed).min(10 ms))` instead of `(span - elapsed).min(10 ms)`. The loop still checks cancel and `elapsed >= span` every pass, so the returned `TimerOutcome` sequence is the same; only the last sleep can overshoot by up to 10 ms. The one observable difference besides latency is a `Duration` overflow panic when `span` is near `Duration::MAX` (not reachable via `SessionWriter`, which caps the span at `MAX_DEBOUNCE_MILLISECONDS`); to hit it through `ThreadTimer` a test must cancel the token only after the timer thread passed its first cancel check, and no hook orders that without time. Not killed. |

Totals for the 13 missed: A = 2, B = 0, C = 11 (of which 4 are the `:439` path-prefix group, 2 the `:456` cfg group, 2 FILE_BUFFER, 2 idle-flush, 1 timer slice). The 4 timeouts: B = 4, all covered by the 3 tests above (`273`, `284:9`, `284:12`, `410`); the three exercised by hand mutants above failed fast.

Correction to the PR #436 report: the "E4 output-only" explanation for `39:31` is not an equivalence proof (it says so itself); here it is reclassified C with the reason above, and `366:44` likewise.

## Gates and limits

- `cargo test -p pr-core --locked -j 2 --test w2_perf3_killers`: 3 passed (run once unmodified, plus three hand-mutant runs).
- `cargo fmt -p pr-core` was applied once; only the new test file changed. `cargo fmt -p pr-core --check` is run before the commit (result in the PR).
- Not run: cargo-mutants, clippy, workspace tests, any other test binary, UNC or non-Windows probes, latency measurements.
- Not claimed: that the pr-core CSV/writer mutation set is complete; the 41 identities that CX-PERF2 left without a verdict are out of this lane.
