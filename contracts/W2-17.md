# W2-17: Perf diagnostics (PerfCsvListener, PhotoReviewPerf event source, ReviewMetrics)
- Color: 🟡 | Porter: **sonnet** (justification below) | Test porter: haiku (mapping, escape, bucket and golden-driven tests; the
  lifecycle/fault tests go to the porter) | Verifier: sonnet
- Why not Haiku (docs/MULTI-AGENT-PLAN row W2-17 says Haiku): three .NET quirks that a 🟢 port misses (`double.ToString("G17")`,
  `ToUpperInvariant` of a path, `EventSource` payload typing) plus a writer thread with fault accounting, a bounded queue, a timed
  dispose and a process-wide listener registry. Same rule as README D6/D7: a unit that depends on .NET behaviour goes to Sonnet from the start.
- C# sources (baseline d92d6aae), under `src/PhotoReview.Core/Diagnostics/`:
  - `PerfCsvListener.cs` (367 lines)
  - `PhotoReviewPerf.cs` (254 lines, incl. `PathId`, `Ms`, `MsSinceProcessStart`, `StartupMark`)
  - `ShellPerfMarks.cs` (12 lines, three constants)
  - `ReviewMetrics.cs` (183 lines, incl. `ReviewMetricsSnapshot`, `SourceOpenEntry`, `PresentLatencyBucket`)
  (read with `git -C C:\MyProjects\PhotoReview show d92d6aae:src/PhotoReview.Core/Diagnostics/<file>`)
  Not ported here: `src/PhotoReview.App/Diagnostics/PhotoReviewPerf.cs` (only `global using` aliases), `PerfDispatcherHooks.cs` (App, W8),
  `FileLog`/`NullLog`/`BenchmarkStatistics`/`DiagOptions` (W1-15 or other units).
- C# tests to port (all at d92d6aae, `tests/PhotoReview.Core.Tests/Diagnostics/`):
  - `PerfCsvEscapeTests.cs` (25 lines): `CsvEscape_FlattensLineBreaks` (5 cases), `CsvEscape_QuotesCommasAndDoublesQuotes_WithoutLineBreaks`.
  - `PerfCsvListenerMappingTests.cs` (234 lines): `BuildRow_MapsNamedAndPositionalPayload`, `BuildRow_GenFillsNav`,
    `BuildRow_SecondNavOrPathId_IsOrdinaryValue`, `BuildRow_OtherNamesNeverFillNavOrPathId`, `BuildRow_MoreThanFourNumerics_AreCappedAtFour`,
    `BuildRow_TextValues_SkipNullsAndJoin`, `BuildRow_MismatchedLengths_UseTheShorterList`, `BuildRow_NullLists_YieldEmptyRow`,
    `BuildRow_FormatsValuesInvariantly`, `Enqueue_BuilderThrows_IsCountedAsDropped` (the builder closure panics: `panic!`),
    `Enqueue_AfterDispose_IsCountedAsDropped`, `Enqueue_Normal_IsWrittenNotDropped` (assert `",Evt,,,1,,,,\n"` in the bytes),
    `Writer_DoesNotFlushPerRow` (`Write` impl counting `flush()` calls, `< 10`), `Writer_FlushesAfterOneSecond` (sleep 1.2 s then enqueue; mark
    `#[ignore = "slow: 1.2 s wall clock"]`, it must pass with `--ignored`), `Header_ContainsVersionAndDiagFlags` (`commit` is a parameter now).
  - `PerfCsvListenerRobustnessTests.cs` (201 lines): `CsvEscape_RoundTrips` (seeds 1-3: a deterministic xorshift RNG written in the test, no `rand`;
    the `SplitCsvLine` reader is ported too), `ConcurrentProducers_NothingLost`, `DisposeUnderLoad_ProducesWellFormedFile`,
    `Dispose_Twice_AndEventsAfterwardsIgnored`, `TryStartFromEnvironment_BadPaths_ReturnNull` (use `try_start` with an injected lookup and a
    `TempRoot`; the bad values are `None`, `""`, `"   "`, a path to an existing file, a path below that file, `Z:\definitely\missing\drive\|invalid`;
    assert the temp dir still holds only `not-a-dir`).
    NOT ported: `Rows_AreCultureInvariant` (4 theories): Rust has no culture; keep ONE test with `de-DE`-style expectations
    (`12.5`, `0.125`, `1234567`, no U+066B) driven by `build_row`/`format_value`, comment `// C#: PerfCsvListenerRobustnessTests.Rows_AreCultureInvariant`.
  - `PerfCsvListenerDropTests.cs` (44 lines): `DroppedRowsAreCountedAndReported` (a gated `Write` stub; `capacity: 4`, 3000 events through
    `PhotoReviewPerf::log().preload_paused(85, 512)`; the same `>= emitted - 500` bounds).
  - `PerfCsvListenerFaultTests.cs` (109 lines): `IdleWriter_FlushesTheTailOfABurst` (`idle_flush: Some(50 ms)`),
    `WriterFault_CountsEveryAbandonedRow` (trailer == 200), `DisposeTimeout_DefersTheTrailerUntilTheWriterTaskEnds`
    (`dispose_wait: Some(100 ms)`, `wait_finished(20 s)` instead of `FinishTask`).
  - `PerfCsvListenerStartupAndFaultCoverageTests.cs` (136 lines): `TryStartFromEnvironment_ValidDirectory_WritesAWellFormedTraceFile` (through
    `try_start` with an injected lookup, `local_now` returning a fixed `LocalTimeParts`; assert the name `perf-<pid>-<yyyyMMdd-HHmmss>.csv`),
    `Header_ListsOnlyTheDiagnosticFlagsThatAreSet`, `Header_NoDiagnosticFlags_EmptyDiagField`,
    `EnqueueAfterWriterFault_CountsEachRowAsDroppedAndDisposeStaysQuiet`, `NeverStartedListener_IgnoresEnqueueAndDisposes`.
  - `PhotoReviewPerfTests.cs` (73 lines): `PathId_StableAcrossCulturesAndCase` (one run, no culture), `PathId_RelativeEqualsFull`,
    `EventIds_UniqueAndInformational` (assert on `EVENT_TABLE`: unique, positive, 26 entries, id 10 absent),
    `MsSinceProcessStart_Plausible_UnderContention` (64 threads, `0 <= v <= 86_400_000`), `Ms_NonNegative`.
  - `PhotoReviewPerfEventTests.cs` (146 lines): the 4 delivery tests (`InputAndLookupEvents`, `LoadPipelineEvents`, `PresentationEvents`,
    `PreloadFolderAndStartupEvents`: a capture `PerfListener` collecting `PerfEvent`s and comparing `(id, name, payload)`; `Int` payload for
    `int`/`long`), `StartupMark_EmitsStartupEvent`, `Ms_ConvertsTicksToMilliseconds`.
  - `ReviewMetricsTests.cs` (213 lines): all 9 facts (including `NewFieldsAreSerialized`: `serde_json::to_string(&snapshot)` contains the 7 listed property names) and the
    `PresentLatencyIsBucketed` theory (16 rows); `ConcurrentRecordingIsThreadSafe` with `std::thread::scope`.
  - `ReviewMetricsTrimTests.cs` (21 lines): `SourceOpenTable_TrimsToThreeQuartersOfCap`.
  - NOT here: `DiagOptionsTests`, `FileLog*Tests`, `BenchmarkStatisticsTests` (W1-15 / other), `tests/PhotoReview.App.Tests/Diagnostics/
    PerfDispatcherHooksTests.cs` (W8), `tests/PhotoReview.Integration.Tests/PerfSessionDiagnosticsTests.cs` (needs the whole pipeline; W7/W8).
  - Golden tests (new, required): `tests/golden/perf-csv-escape.json` (27 cases), `perf-csv-row.json` (49), `perf-csv-file.json` (17),
    `perf-path-id.json` (30), `review-metrics.json` (14).
- PARITY rows: P-16 (diagnostic env vars, `PHOTOREVIEW_PERF_TRACE` CSV perf trace; the Rust names are `PHOTOREVIEW_RS_PERF_TRACE` and
  `PHOTOREVIEW_RS_DIAG_*`, R0002 / README D4), plus the perf-trace rows the lead keeps for the CSV format (the file is read by `pr_perf::csv`, W1-16)
  and the benchmark report (W7) which embeds `ReviewMetricsSnapshot`.
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/diagnostics/metrics.rs     (stub: contracts-w2/W2-17/stubs/crates/pr-core/src/diagnostics/metrics.rs)
    crates/pr-core/src/diagnostics/perf.rs        (stub: .../perf.rs)
    crates/pr-core/src/diagnostics/perf_csv.rs    (stub: .../perf_csv.rs)
    crates/pr-core/tests/diagnostics_review_metrics.rs
    crates/pr-core/tests/diagnostics_perf.rs
    crates/pr-core/tests/diagnostics_perf_csv.rs
- Lead-owned edits for this unit (the porter does NOT touch them): `crates/pr-core/src/diagnostics/mod.rs` gets `pub mod metrics; pub mod perf;
  pub mod perf_csv;`; `crates/pr-core/Cargo.toml` gets `sha2 = "0.10"` (workspace dependency, same decision as W2-12 Q1; a 60-line hand-written
  SHA-256 is the fallback); the five golden files are copied to `tests/golden/`. `dotnet_path` (W2-00) must be merged first.
- Read-only inputs: the five goldens (`pr_test_support::golden::load`, `TempRoot`), `crates/pr-core/src/text/mod.rs` (`ordinal_upper`, `ignore_case_key`), `time.rs` (`DateTimeUtc`),
  `model/enums.rs` (`DecoderBackend`, `LenientEnum::{MEMBERS, name, value}`), `diagnostics/diag_options.rs` (the three `*_VAR` names). One unit test asserts that
  `DIAG_FLAG_HEADER_NAMES[i].replacen("PHOTOREVIEW_", "PHOTOREVIEW_RS_", 1)` equals `PRE_READ_VAR` / `PRELOAD_WORKERS_VAR` / `DISABLE_DISK_CACHE_VAR`.
- Public API: exactly the stub signatures. Do not rename `PerfValue`, `PerfEvent`, `PerfListener`, `PerfSubscription`, the 26 event methods,
  `EVENT_TABLE`, `PerfRow`, `LocalTimeParts`, the constants. The porter may ADD private items (statics, helper fns) in the owned files.

## Semantics notes (checked against C# d92d6aae and the 5 goldens, which were generated by the real C# code)

### A. `format_g17`, `format_value`, `csv_escape`, `build_row` (perf_csv.rs)
1. **`format_value`** (C# `FormatValue`): `Null` -> `""`; `Bool` -> `"1"`/`"0"`; `Double` -> `format_g17`; `Int` -> invariant decimal (`i64::to_string`,
   no grouping); `Str` -> verbatim. `int` and `long` both arrive as `Int`.
2. **`format_g17`** = .NET Core 3.0+ `ToString("G17", InvariantCulture)`: the exact 17-significant-digit correctly rounded decimal expansion, trailing
   zeros removed, then positional or scientific by the decimal exponent X of the first digit: **scientific iff `X >= 17 || X < -4`** (so `1e16` is
   `10000000000000000`, `1e17` is `1E+17`, `0.0001` is `0.0001`, `1e-5` is `1.0000000000000001E-05`). Scientific text: first digit, then `.` and the
   rest only if more than one digit, `E`, sign `+`/`-` ALWAYS, exponent with at least 2 digits (`E+17`, `E-05`, `E+308`). Specials: `NaN`, `Infinity`,
   `-Infinity`; `-0.0` is `-0`, `0.0` is `0`. Reference algorithm (reproduces all 24 `g17-*` golden values (19 of them given by IEEE bits), prototyped against the golden):
   ```rust
   pub fn format_g17(v: f64) -> String {
       if v.is_nan() { return "NaN".into(); }
       if v.is_infinite() { return if v > 0.0 { "Infinity" } else { "-Infinity" }.into(); }
       let neg = v.is_sign_negative();
       if v == 0.0 { return if neg { "-0" } else { "0" }.into(); }
       let s = format!("{:.16e}", v.abs());              // "d.dddddddddddddddde[-]X": 17 correctly rounded digits
       let (m, e) = s.split_once('e').unwrap();
       let x: i32 = e.parse().unwrap();
       let mut digits: String = m.chars().filter(|c| *c != '.').collect();
       while digits.len() > 1 && digits.ends_with('0') { digits.pop(); }
       let body = if x >= 17 || x < -4 {
           let rest = if digits.len() > 1 { format!(".{}", &digits[1..]) } else { String::new() };
           format!("{}{}E{}{:02}", &digits[..1], rest, if x < 0 { '-' } else { '+' }, x.abs())
       } else if x >= 0 {
           let int_len = x as usize + 1;
           if digits.len() <= int_len { format!("{}{}", digits, "0".repeat(int_len - digits.len())) }
           else { format!("{}.{}", &digits[..int_len], &digits[int_len..]) }
       } else {
           format!("0.{}{}", "0".repeat((-x - 1) as usize), digits)
       };
       if neg { format!("-{body}") } else { body }
   }
   ```
   Do NOT use `{}`/`{:?}` of f64 (`0.1` would print `0.1`, not `0.10000000000000001`) and do not use `{:e}` (no `+`, no 2-digit exponent).
3. **`csv_escape`** (C# `CsvEscape`): `""` returns `""`. If the value has `\r` or `\n`, replace `\r\n` with ONE space, then every remaining `\r` or `\n`
   with one space (so `a\r\r\nb` -> `a` + 2 spaces + `b`, `a\n\r\nb` likewise: golden). Then if the result contains `,` `"` `\n` or `\r` (only `,` and
   `"` can be left) wrap in quotes with `"` doubled. Nothing else is touched: NUL, TAB, U+0085 and U+2028 stay (U+2028 does NOT trigger quoting;
   golden `é,\u2028x` is quoted only because of the comma). Leading/trailing spaces are kept and do not trigger quoting.
4. **`build_row`** (C# `BuildRow`, mapping pinned by `perf-csv-row`): iterate `i in 0..min(names.len, values.len)` (None for either = no payload at all);
   apply the FIRST matching branch, in this order:
   1. `name == "nav" || name == "gen"` AND `nav` is still empty -> `nav = format_value(v)`;
   2. `name == "pathId"` AND `path_id` is still empty -> `path_id = format_value(v)`;
   3. `v` is `Int`, `Double` or `Bool` -> the next of `a,b,c,d` (the first 4 only; later numerics are counted but dropped);
   4. `v` is not `Null` -> push `format_value(v)` onto `text`.
   `text` is joined with `;`; empty strings count (`["a","","b"]` -> `a;;b`, `["",""]` -> `;`). Consequences the golden pins: a null/empty `nav` value does not
   "use up" the nav slot (a later `gen` fills it) and a null `nav` is NOT added to text; a SECOND `nav`/`gen`/`pathId` is an ordinary value (numeric -> a..d,
   string -> text); names are case sensitive; a string that looks like a number is text; a `Str` in the `nav` slot is written as is (`x`).
   `utc_ticks = DateTimeUtc::now().ticks()`, `qpc_ticks = perf::timestamp()`, `thread = current_thread_number()`.
5. **`current_thread_number`**: a `thread_local!` that takes the next value of a static `AtomicI32` (starting at 1) on first use. Only "small positive,
   stable per thread" is required.

### B. CSV file format and writer (perf_csv.rs)
6. **Bytes** (golden `perf-csv-file` compares the whole text with `utcTicks,qpcTicks,thread` and the header `commit`/`qpcFrequency` masked; compare
   with `\r\n` and `\n` exactly as written): UTF-8 WITHOUT BOM.
   - line 1: `# commit=<commit> diag=<list> qpcFrequency=10000000` + **`\r\n`** (C# `WriteLine` on Windows; write the literal `\r\n`, never
     `writeln!`'s platform default). `<commit>` is the `commit` argument or `unknown`. `<list>`: for each name of `DIAG_FLAG_HEADER_NAMES` in that FIXED
     order (`PREREAD`, `PRELOAD_WORKERS`, `DISABLE_DISKCACHE`; not the order they were set in), when the variable value is non-empty, `NAME=value` with
     the RAW value (not trimmed, not validated: `" 4 "`, `"yes"`, `"1;2"` are written as they are), joined with `;`; an empty list leaves `diag= qpcFrequency=` (`diag=` + empty + ONE space; golden `empty-file-no-flags`).
   - line 2: `utcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text` + `\r\n`.
   - each row: `utc,qpc,thread,` then `csv_escape` of event, nav, pathId, a, b, c, d, text joined by `,`, terminated by a bare **`\n`**. 11 fields.
   - trailer: `# dropped=<n>` + `\r\n`, written once after the last row, then flush.
7. **Diag flags in the header.** `DIAG_FLAG_HEADER_NAMES` are the C# spellings and are what the CSV and `pr_perf` use (the file must stay readable by
   the same analyzer). The Rust app reads `PHOTOREVIEW_RS_DIAG_*` (README D4, `diag_options::{PRE_READ_VAR, PRELOAD_WORKERS_VAR,
   DISABLE_DISK_CACHE_VAR}`). So: `start_for_test(.., diag)` calls `diag(<C# name>)`; `try_start(lookup, ..)` passes `|n| lookup(&n.replacen("PHOTOREVIEW_",
   "PHOTOREVIEW_RS_", 1))`; `try_start_from_environment` passes `std::env::var_os(..).map(lossy)`. The golden env is keyed by the C# names and the test
   closure answers by the C# name (no `set_var`: it is `unsafe` in edition 2024 and global).
8. **Start** (`try_start`): `dir = lookup(TRACE_ENV_VAR)`; `None` or `trim().is_empty()` -> `None`. The directory is used RAW (no trim).
   `create_dir_all(dir)`, file name `perf-<std::process::id()>-<yyyyMMdd-HHmmss from local_now()>.csv` (zero padded 4/2/2-2/2/2), open with create+truncate
   and write access (Windows default share mode already allows readers; add `share_mode(FILE_SHARE_READ)` only if the file-system boundary rules allow
   `std::os::windows`, it is optional). Any error in these steps -> `None`, nothing left behind. If writing the header or starting the thread fails after
   the file exists: dispose, best-effort `remove_file`, `None`. The header is written into the buffered writer BEFORE the thread starts (it reaches the
   file with the first flush; the empty-file golden has header and trailer written together).
9. **Queue and drop accounting.** Bounded `sync_channel(capacity)`; producers use `try_send`; every failure is `dropped += 1`:
   - full queue (C# `DropWrite` + `itemDropped` callback), closed queue (after `dispose`), writer faulted (checked FIRST: nobody reads any more), builder
     panic (`catch_unwind(AssertUnwindSafe(build))`; a panicking payload must never reach the emitting thread).
   - never-started listener (`unstarted()`): `enqueue` returns silently WITHOUT counting (C# `_channel is null`). After `dispose` the listener counts.
   - `capacity` 0 is a rendezvous channel in Rust and "one slot or invalid" in C#: not used by any test; clamp to `max(1, capacity)`.
10. **Writer thread** (named `perf-csv-writer`; `std::thread::Builder`, ADR R0003): loop `recv_timeout(idle_flush)`:
    - `Timeout`: if `unflushed > 0` flush and set `unflushed = 0`; reset `last_flush`; continue (idle flush of a burst tail; test `IdleWriter_...` 50 ms).
    - `Ok(row)`: `unflushed += 1`; write the row; if more than 1000 ms since `last_flush` (C# `Ms(lastFlush) >= 1000`): flush, `unflushed = 0`, reset.
      Flushing happens ONLY by that rule, on idle, and at the end: never per row (a counting sink sees `< 10` flushes for 50 rows). A row spill from a
      `BufWriter` into the sink is not a `flush()` call, which is why `BufWriter` is acceptable.
    - `Disconnected` (sender dropped by `dispose`): drain is finished, leave the loop.
    - any `io::Error` from write or flush: `writer_faulted = true`; `dropped += unflushed` (rows already handed to the writer since the last SUCCESSFUL
      flush are counted as lost EVEN IF they later reach the file); then every row still in the queue is received and counted (`dropped += 1` each);
      stop writing rows. The thread does NOT write the trailer or exit yet: it keeps `recv()`ing (counting each received row as dropped) until
      `Disconnected`. Reason: `WriterFault_CountsEveryAbandonedRow` sets `Fail = false` AFTER the fault and before `dispose`, and expects the trailer
      (200) to reach the sink then; a trailer attempted at fault time would hit the still-failing sink and be lost (C# writes it from `Dispose`).
    - at the end (always): best-effort flush; then the trailer step below.
11. **Trailer / dispose.** `dispose` is idempotent (`AtomicBool::swap`): drop the sender (closes the queue), wait up to `dispose_wait` for the writer
    thread to signal `finished`, then unsubscribe. The TRAILER is written by the writer thread itself as the last thing it does (after the best-effort
    flush): if `writer_faulted`, first drain/count leftover queued rows; write `# dropped=<current count>\r\n`, flush (errors ignored), drop the sink,
    set `finished` and `notify`. Consequence pinned by `DisposeTimeout_...`: if the writer thread is blocked inside a sink write when `dispose` times out,
    `dispose` returns WITHOUT touching the sink (bytes written stay 0), and the single trailer appears when the thread gets unblocked (`wait_finished`).
    A broken stream makes the trailer write fail silently: `dispose` never panics or returns an error.
12. **Subscription.** `start_*` creates `Arc<Shared>`, subscribes it (`PhotoReviewPerf::log().subscribe(arc as Arc<dyn PerfListener>)`) AFTER the
    sender is installed, stores the `PerfSubscription` in the mutex. `dispose` takes it out (drops = unsubscribes). `Drop for PerfCsvListener` calls
    `dispose()`. `Shared::on_event_written` -> `enqueue(|| build_row(event.name, Some(event.payload_names), Some(&event.payload)))`.

### C. `PhotoReviewPerf` (perf.rs)
13. **Event source.** C# `EventSource` is replaced by a process-wide registry (`LISTENERS` in the stub) plus a `static AtomicUsize` listener count.
    `is_enabled()` is `count > 0` (relaxed). Every event method: `if !self.is_enabled() { return }` then build a `PerfEvent { id, name, payload_names,
    payload }` and call every listener synchronously on the calling thread. Take a CLONE of the `Vec<Arc<dyn PerfListener>>` under the read lock and
    call listeners after releasing it (a listener may subscribe/unsubscribe; holding the lock would deadlock). `subscribe` pushes `(next_id, listener)`
    and increments the count; `PerfSubscription::drop` removes the id and decrements. A listener must not panic; the emitting side does NOT
    `catch_unwind` (the CSV listener already does).
14. **Event table** = `EVENT_TABLE`, copied from the C# `[Event(n)]` attributes (checked): ids 1-9 and 11-27, id **10 never existed** and must stay
    unused. Method names are the CSV `event` column and are PascalCase in `PerfEvent.name` (`KeyInput`, `DiskCacheRead`, `RenderedFrame`, ...), parameter
    names in `payload_names` keep the C# camelCase (`inputDelayMs`, `pathId`, `msSinceAssign`, `gen`, `availableMb`, `msSinceProcessStart`). Payload value
    types: C# `long`/`int` -> `Int`, `double` -> `Double`, `bool` -> `Bool` (`Decode`'s `downscaled`, `fallback`), `string` -> `Str`. The `decode` event
    has payload names `nav, pathId, ms, targetWidth, downscaled, fallback`; the Rust method args are in the same order.
15. **`nav_context`/`set_nav_context`**: C# `AsyncLocal<long>`. Rust: `thread_local!` `Cell<i64>`, default 0. This is a documented DIVERGENCE (no
    async flow; callers copy the value across threads by hand); no golden case, one unit test.
16. **`timestamp()`**: `Instant`-based, `elapsed.as_nanos() / 100` ticks since a process-wide `OnceLock<Instant>` base, plus 1 so it is never 0 (a
    sentinel "0 = not started" in a caller would otherwise misfire on the first call). `STOPWATCH_FREQUENCY` is the constant 10_000_000 that the CSV
    header writes (`qpcFrequency=`). `ms(start)` = `(timestamp() - start) as f64 * 1000.0 / 10_000_000.0` (`f64`, same expression order as C#;
    test: `ms(timestamp() - 5_000_000)` in `[499, 10_500]`).
17. **`ms_since_process_start`**: `(DateTimeUtc::now().ticks() - process_start.ticks()) as f64 / 10_000.0` (ticks to ms). `process_start` is a
    `OnceLock<DateTimeUtc>` set by `init_process_start(Some(t))` or, when `None`/never called, by the first call of `ms_since_process_start` (so the
    first `Startup` event reports ~0 ms: DIVERGENCE from C# `Process.StartTime`, which includes runtime start; the platform layer passes the real
    creation time through `init_process_start(Some(..))`). The `OnceLock` also removes the C# torn-`DateTime?` hazard (the `Lazy` comment).
    `startup_mark(phase)`: `if log.is_enabled() { log.startup(phase, ms_since_process_start()) }`.
18. **`path_id(path)`** (golden `perf-path-id`, 30 cases incl. UNC, `\\?\`, `/` separators, trailing dots/spaces, `..`, and the .NET-vs-Rust casing
    traps): `full = dotnet_path::get_full_path(path)`; `upper = full.chars().map(to_upper_invariant_char).collect::<String>()`;
    `id = hex_lower(SHA256(upper.as_bytes())[..8])` (exactly 16 chars, lower case). `to_upper_invariant_char(c)` = `if c == '\u{017F}' { 'S' }
    else { text::ordinal_upper(c) }`. This is .NET `ToUpperInvariant` (VERIFIED in W2-12 for all 1423 non-ASCII mappings: it equals `ordinal_upper` except
    U+017F; `ß`, `ı`, `ŉ`, `ﬁ` do not change; `ǅ` and `ǆ` both become `Ǆ`, so they share an id in the golden). NEVER `str::to_uppercase` (`ß` -> `SS`,
    `ı` -> `I`). When `get_full_path` returns `Err` (empty path, NUL; C# throws `ArgumentException`) fall back to hashing `to_upper(path)` unchanged;
    a perf helper must not panic. DIVERGENCE, no golden case. Relative paths resolve against the current directory (test only compares with the id of
    `get_full_path` of the same input).
19. **`ShellPerfMarks`**: three `&str` constants already complete in the stub (`d3dDeviceReady`, `swapChainReady`, `firstPresent`).

### D. `ReviewMetrics` (metrics.rs; golden `review-metrics`)
20. **Counters** are `AtomicI64` with `SeqCst` (README D3: keep the C# `long`); `Interlocked.Increment` wraps, so use `fetch_add` (wrapping by
    definition) and never `checked_add`.
21. **Clamping**: `record_ui_assign(ms)`, `record_source_read(bytes, ms, ..)` and `record_presented(ms)` use `max(0, x)` (golden
    `negative-inputs-are-clamped`: a `-50 bytes, -20 ms` read still counts one `SourceReads`). `record_source_read(.., include_in_decode_ewma)`: C#
    default is `true` (the Rust method has no default: callers pass it).
22. **EWMA**: `update(ms)`: if `ms <= 0` nothing; loop: `old = f64::from_bits(bits as u64)`; `next = if old <= 0.0 { ms as f64 } else
    { old + 0.2 * (ms as f64 - old) }` (exactly this expression: golden compares the result bits `{:016X}` after every source read, so `0.8*old + 0.2*ms`
    is WRONG); `compare_exchange(oldbits, next.to_bits() as i64)` retry. `decode_milliseconds_ewma()` is `f64::from_bits(load as u64)`; `0` bits = 0.0.
    A read with `include_in_decode_ewma == false` or `ms == 0` is not a sample (golden `ewma-zero-ms-is-not-a-sample`).
23. **Histogram**: `present_bucket_index(ms)` = first `i` with `ms <= UPPER_BOUNDS[i]`, else 7 (`8 -> 0`, `9 -> 1`, `500 -> 6`, `501 -> 7`).
    Snapshot histogram is ALWAYS 8 entries with the labels `<=8 <=16 <=33 <=50 <=100 <=200 <=500 >500`.
24. **Source-open table**: key = `ignore_case_key(path)` (OrdinalIgnoreCase, trap 1), value = (FIRST spelling seen, count); a later spelling
    of the same file increments the count and keeps the first spelling. `record_source_open`: `source_open_count += 1` first, then the table; if the
    table has MORE than `MAX_TRACKED_SOURCE_OPEN_PATHS` (4096) entries, trim UNDER the same mutex: `excess = len - 4096 * 3 / 4`; remove the `excess`
    entries with the lowest count (C#: ties arbitrary; Rust: break ties by key ascending so the port is deterministic; the golden compares only entries with
    count > 1 after a trim: `stable_top_only`). The total count is unaffected. Exact sizes pinned by golden: 4096 distinct -> 4096; 4097 distinct ->
    **3072**; 12 290 distinct (2 hot) -> 4090 and both hot paths survive.
25. **`snapshot()`**: `top_source_opens` = entries sorted by count DESCENDING then `cmp_ignore_case(path)` ASCENDING, `take(10)`
    (golden `source-opens-ties-ordered-by-path-ignoring-case`: `a, B, z, _x, Ä, ä2` because `_` (0x5F) sorts after `Z` once upper-cased and `Ä`
    is `U+00C4`). `decoder_fallbacks` = `(backend, count)` ascending by `LenientEnum::value()` (the stub keeps `HashMap<i32, i64>`; map the key back to
    `DecoderBackend` by searching `DecoderBackend::MEMBERS` for the same `value()`); `decoder_fallback_count()` is the sum.
26. **`Serialize` for `ReviewMetricsSnapshot`**: a JSON object with the 19 properties in the order of the stub doc (PascalCase), `DecoderFallbacks`
    an OBJECT `{ "WicDirect": 2, "TurboJpeg": 1 }` (key = `LenientEnum::name()`, order by backend value), `DecoderFallbackCount` the computed sum
    written between `DecoderFallbacks` and `SourceOpenCount`, `TopSourceOpens` an array of `{"Path","Count"}`, `PresentHistogram` an array of
    `{"Label","Count"}`. Compare the golden through `serde_json::Value` (`snapshot` field), NOT as text: System.Text.Json's default encoder escapes `<`,
    `>` and every non-ASCII char (`\u003C=8`, `\u00C4`), `serde_json` does not. Byte-level output belongs to the report writer (W7), which should
    document that choice; `pr_perf` readers parse JSON and are unaffected.
27. **`tracked_source_open_paths()`** is the table length (golden field `tracked_source_open_paths`).

## Traps (docs/MULTI-AGENT-PLAN.md §7)
- §7.1 OrdinalIgnoreCase: `ignore_case_key`/`ordinal_upper` only; `path_id` uses the invariant upper-case (U+017F special case), not `to_uppercase`.
- §7.2 Culture/invariance: every number goes through `format_value`; no `{}` of `f64` in the CSV; `\r\n` literal in header/trailer.
- §7.3 Arithmetic: counters wrap like C# `long` (D3); `bytes`/`ms` are clamped at 0, not saturated; `i64` payloads (`i64::MIN` golden `long-extremes`).
- §7.4 Number formatting: G17 is the whole point of `perf-csv-row`; do not "fix" `0.10000000000000001`.
- §7.5 Time: `DateTimeUtc` ticks (100 ns) for `utcTicks`; `qpcTicks` are Stopwatch ticks at 10 MHz; the Rust value is monotonic, not QueryPerformanceCounter.
- §7.8 Null vs empty: `Null` payload is skipped from `text`, `Str("")` is kept (`a;;b`); `names`/`values` `None` yields an empty row.
- §7.9 Exceptions to `Result`: no `Result` here: the writer must swallow I/O errors into `writer_faulted`/`dropped`; `panic` in the row builder is caught.
- §7.10 Order: `HashMap` for the open table and fallbacks is fine ONLY because the snapshot sorts (count desc then path, backend value asc).
- `BufWriter` capacity: use the std default (8 KiB) or smaller for the writer, NOT a 64 KiB buffer "like the C# FileStream". `DroppedRowsAreCountedAndReported`
  needs the writer to block after ~a few hundred rows (`>= emitted - 500` dropped of 3000): a 64 KiB buffer would absorb ~1700 rows before the first
  sink write and fail it. (C# `StreamWriter` buffer is 1024 chars.) With 8 KiB the 200-row fault/timeout tests only reach the sink on the 1 s idle
  flush or at buffer overflow; they poll up to 20 s, so they pass either way.
- Test isolation: the listener registry is process-wide, like the C# `[Collection("GlobalState")]`. Tests in one test binary that attach a
  listener and count rows (`ConcurrentProducers_NothingLost`, `DroppedRowsAreCountedAndReported`, the event-delivery tests) must hold one
  `static LOCK: Mutex<()>`; `cargo test` runs them in parallel threads otherwise and rows of one test land in another's file. Use unique `nav` values
  (`7_700_001`) like C#. Never call `std::env::set_var` (edition 2024 `unsafe`, global): inject the lookup.
- Fault tests need a `Write` stub (`ProbeStream`: `fail: Arc<AtomicBool>`, optional gate `Arc<(Mutex<bool>, Condvar)>` for the first write, `written`
  counter). Do not use `std::sync::mpsc` timing to wait: poll a condition with a deadline (C# used `SpinWait.SpinUntil`).

## Depends on
- W2-00 `text::dotnet_path::get_full_path` (path_id). MUST be merged first (or `path_id` tests use absolute paths only until then).
- W0 `text::{ordinal_upper, ignore_case_key, cmp_ignore_case}`, `time::DateTimeUtc`; W1-01 `model::enums::DecoderBackend`; W1-02
  `LenientEnum`; W1-15 `diag_options::*_VAR` (only the three names, for the header mapping); `pr-test-support` (`TempRoot`, `golden`).
- External crate: `sha2` (lead, see Owned files). No other new crates; `mpsc::sync_channel` is std (crossbeam is not needed for one reader).
- Consumed later by: W7 benchmark report (`ReviewMetricsSnapshot` JSON), W8 App/Shell (`PhotoReviewPerf::log()` call sites guarded by
  `is_enabled()`, `PerfCsvListener::try_start_from_environment`), W2-14/W2-16 style I/O units are NOT consumers. The `pr_perf::csv` reader (W1-16)
  is the reader of the file this unit writes; its golden `perf-csv.json` is separate and unchanged.

## Done when
- Every test listed above has a Rust twin with `// C#: <Class>.<Method>` (the slow 1.2 s one `#[ignore]`d but passing with `-- --ignored`).
- The five goldens pass: `perf-csv-escape` and `perf-csv-row` byte-exact on `escaped` and the 8 `out` fields (doubles built from `bits` when present);
  `perf-csv-file` equal on the masked text (mask with the same regexes the oracle uses: header `commit=\S*`, `qpcFrequency=\d+`, first three columns
  of every data row `^\d+,\d+,\d+,`); `perf-path-id` equal on `id` for all 30 paths (the `full` field is a W2-00 concern, optional extra assert);
  `review-metrics` equal on `snapshot` (as `Value`), `ewma_bits`, `ewma_bits_after_each_source_read` and `tracked_source_open_paths`.
- `EVENT_TABLE` matches the 26 C# `[Event]` attributes; id 10 absent; each event method has one delivery assertion (`id`, `name`, `payload_names`,
  payload).
- `cargo clippy --all-targets -- -D warnings` clean; the stub's `#![allow(unused_variables, dead_code)]` line removed from the three files; no
  `unsafe`; no `std::env::set_var`.
- PARITY P-16 note updated by the lead: Rust variable names `PHOTOREVIEW_RS_PERF_TRACE` / `PHOTOREVIEW_RS_DIAG_*`, header keeps the C# names.

## Golden package
- `oracle/W2-17.cs`: five commands, dispatch lines are in the file header (`perf-csv-escape`, `perf-csv-row`, `perf-csv-file`, `perf-path-id`,
  `review-metrics`). Internal members (`CsvEscape`, `BuildRow`, `StartForTest`, `TrackedSourceOpenPaths`) are reached by reflection; no
  `InternalsVisibleTo` needed. Needs `PhotoReview.Core` only (and nothing from W2-00: `PathId` is called directly).
- `golden-inputs/{perf-csv-escape,perf-csv-row,perf-csv-file,perf-path-id,review-metrics}.json`, outputs in `golden-reference/` (generated by
  the C# build at d92d6aae; reviewed against the C# source for this contract, no defect found).
- Case design: escape (27: every CR/LF permutation, quotes, NUL, TAB, U+0085, U+2028); row (49: mapping branches, 24 `g17-*` cases, 19 of them given by IEEE bits
  including denormal min, max, `-0`, NaN, both infinities and both exponent switch points); file (17: header flag order/raw values, all 26 events
  through the real `PhotoReviewPerf.Log` methods, escaping, `long` extremes, non-ASCII text); path-id (30: separators, `..`, trailing dot/space,
  UNC, `\\?\`, `ß ǆ ǅ ı İ ſ µ ÿ ﬁ`, surrogate pairs, Greek final sigma, CJK); metrics (14: EWMA bit patterns after each read, clamping, all
  histogram boundaries, tie ordering, trim at 4096/4097/12290).
- Not covered by the golden (unit tests only): `nav_context`, `timestamp`/`ms`, `ms_since_process_start`, idle/1 s flush cadence, fault and dispose-timeout
  paths, `try_start` file creation, concurrency, relative `path_id`.
- Machine dependence: the golden masks utc/qpc/thread, commit and frequency; path-id inputs are all absolute drive/UNC paths, so the output does not
  depend on the machine or its current directory.

## Open questions
- Q1 (casing helper): `path_id` needs `to_upper_invariant` and W2-12 defines the same function inside `SessionStore`. Recommended: the lead hoists
  one `text::to_upper_invariant(&str) -> String` (ordinal_upper per char + U+017F) and both units call it; until then this unit keeps a private
  `fn` of three lines in `perf.rs` (identical rule, pinned by this golden).
- Q2 (`Process.StartTime`): is `init_process_start(Some(OS creation time))` going to be called by `pr-platform-win` (W6) before the first
  `startup_mark`? If not, `Startup` rows report time since first use and `RenderedFrame`-vs-startup timelines lose the runtime-load segment
  (C# included it). Behaviour is safe either way; the perf analyzer only uses `Startup` deltas.
- Q3 (`qpcTicks` meaning): the Rust `timestamp()` is not QueryPerformanceCounter, so a CSV written by Rust cannot be aligned to ETW/PerfView traces by
  `qpcTicks` (C# could). Acceptable (no external tool consumes it in the port); say so in `docs/perf.md` if that file exists.
- Q4 (snapshot JSON escaping): see note 26. W7 should decide whether the benchmark report keeps System.Text.Json's `\u003C`-style escapes
  (byte-identical reports) or plain UTF-8; the unit test here is escape-agnostic on purpose.
