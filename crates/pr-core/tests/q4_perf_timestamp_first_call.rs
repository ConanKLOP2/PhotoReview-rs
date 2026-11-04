//! Q4: the FIRST `timestamp()` call of a process. This binary holds exactly one test and nothing else calls
//! `timestamp()` before it, so the base `Instant` is created by this very call (elapsed is only a few ticks).
//! Kills `perf.rs:38:76` (`+ 1` -> `- 1` / `* 1`) with high probability: the first value is `elapsed_ticks + 1`,
//! the mutants give `elapsed_ticks - 1` or `elapsed_ticks`, which is 0 or negative whenever the first call
//! measures under 1 µs (the normal case). It can never fail on the correct code (`ticks + 1 >= 1`).

use pr_core::diagnostics::perf::timestamp;

// C#: PhotoReviewPerf.Ms / Stopwatch.GetTimestamp callers use 0 as "not started"; the Rust port promises "+1: never 0".
#[test]
fn the_first_timestamp_of_a_process_is_never_zero_or_negative() {
    let first = timestamp();
    assert!(first >= 1, "first timestamp {first}");
    assert!(timestamp() >= first);
}
