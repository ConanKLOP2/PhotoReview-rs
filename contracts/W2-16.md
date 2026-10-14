# W2-16: FileLog (asynchronous, rotating, file-backed `Log`; INV-10)
- Color: 🟡 | Porter: **sonnet** (a background writer thread, a bounded queue, a shared 2 s budget and many timing-sensitive tests; no unsafe
  design decisions left open. Not haiku: the concurrency and the "never lose / never duplicate an entry" rules are easy to break. Not opus: the
  C# is 345 lines with every rule already written down below) | Test porter: sonnet (the timing tests need the same understanding as the
  implementation; the three golden tests alone could go to haiku) | Verifier: sonnet
- C# source (baseline d92d6aae), under `src/PhotoReview.Core/Diagnostics/`:
  - `FileLog.cs` (345 lines)
  (read with `git -C C:\MyProjects\PhotoReview show d92d6aae:src/PhotoReview.Core/Diagnostics/FileLog.cs`)
- C# tests to port (all at d92d6aae; `tests/PhotoReview.Core.Tests/Diagnostics/`):
  - `FileLogTests.cs`: `QueueIsBoundedWhenLogFileCannotBeWritten`, `ShutdownDoesNotStallOnUnwritableLogFile` (< 1500 ms),
    `DisabledLoggingCreatesNoDirectoryOrFile` (INV-10), `ExplicitlyEnabledLoggingWritesFormattedEntries`, `DisablingLoggingStopsWrites`,
    `ConcurrentLoggingPreservesAllEntries`, `LogFileRotatesAtThreshold`, `PartialBatchFailure_DoesNotDuplicateFlushedEntries`,
    `ThrowingExceptionToString_DoesNotRepeatEarlierLinesNorBlockLaterOnes` (see Open questions 3),
    `DrainKeepsUnwrittenEntriesWhenTheProducerTrimsDuringTheWrite`.
    `DisposeWithBlockedWriterDoesNotDisposeHandlesUnderTheWriter`: port the observable half only (a stuck writer, `dispose`, release the gate,
    `wait_writer_exit` is true, the entry is on disk); there are no handles.
  - `FileLogLifecycleTests.cs`: `UseAfterDispose_NeverThrows`, `EnableAfterDispose_DoesNotStartWriter`, `Dispose_DrainsEveryQueuedEntryToDisk`,
    `DisposeUnderLoad_WritersNeverThrow` (with its `AssertOnDiskIsPerThreadPrefix` helper), `OddMessages_WrittenVerbatim`,
    `Timestamp_IsCultureIndependent` (3 theory rows: Rust has no culture; keep one row and assert the digits are ASCII, the year 4 digits).
  - `FileLogRotationFailureTests.cs`: `ReadOnlyBackup_LoggingContinues`, `LockedBackup_LoggingContinues` (hold the backup with
    `OpenOptions::new().read(true).share_mode(0)`, `std::os::windows::fs::OpenOptionsExt`, `#[cfg(windows)]`), `Rotation_NoEntryLost`,
    `UnwritableLog_EntriesAreKeptForTheNextDrain` (same `share_mode(0)` holder on the log file).
  - `FileLogTimingAndHandleTests.cs`: `Shutdown_StuckWriter_UsesOneSharedBudget` (1.5 s..3.9 s), `Flush_DrainedSetWhileEntryQueued_WaitsUntilWritten`,
    `Flush_ReleasedWhenDrainFinishes`, `QueueCap_Boundary` (exactly `MAX_QUEUED_ENTRIES` kept, the oldest dropped only past it),
    `EnabledButIdle_CreatesNothingOnDisk`, `LogAtExactLimit_IsRotated` (`>=` threshold).
    Port the observable half of `Dispose_StuckWriter_DefersHandleRelease` (writer alive after `dispose`, `set_enabled(true)` leaves `enabled()` false,
    after the gate opens the writer exits and the entry is on disk).
    NOT ported (no wait handles exist in Rust, nothing to assert; list each in the test file header as `// C#: ... not ported: handle lifetime`):
    `Dispose_ClosesWaitHandles`, `Flush_SignalDisposed_ReturnsImmediately`, `Flush_DrainedEventDisposed_ReturnsImmediately` (these use reflection on
    private fields, `HandlesReleased`, `FirstChanceException`).
  - Golden tests (new, required): `tests/golden/file-log-format.json`, `tests/golden/file-log-backup-name.json`, `tests/golden/file-log-bytes.json`.
    Each is a case table with real expectations; read them with the `golden` helper of pr-test-support.
- PARITY rows: P-08 (logging: queue 10 000, rotate at 10 MiB to `app.1.log`), INV-10 (off by default, off creates nothing), D-08 (log line format),
  S-04 (the setting itself is App: only the `Log` side is here).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/diagnostics/file_log.rs        (stub: contracts-w2/W2-16/file_log.rs, compiles clippy-clean)
    crates/pr-core/tests/diagnostics_file_log.rs
- Read-only inputs: the three goldens above. Lead-owned: `diagnostics/mod.rs` gets `pub mod file_log;`, `lib.rs`, `Cargo.toml` (see Open questions 1).
- Public API (stub, do NOT change signatures): `FileLog` (`new`, `from_app_paths`, `with_time_source`, `default_instance`, `file_path`,
  `set_enabled`, `flush`, `shutdown`, `dispose`, `Drop`, `impl Log`), `LogTimestamp::format`, `LocalTimeSource`, `SystemLocalTime`,
  `current_thread_number`, `format_entry`, `backup_path`, consts `MAX_QUEUED_ENTRIES`, `DEFAULT_MAX_LOG_BYTES`, `MIN_MAX_LOG_BYTES`,
  and the `#[doc(hidden)]` test seams `pending_count`, `dropped_count`, `is_writer_alive`, `wait_writer_exit`, `set_drain_hook`,
  `set_after_batch_written_hook`, `set_before_entry_written_hook`. Private `Entry`/`Hooks`/`Inner` may be reshaped freely.
- Semantics notes (all verified against C# d92d6aae):
  1. **INV-10.** A new log is disabled. Constructors start nothing and touch no disk (no thread, no directory). `set_enabled(true)` starts the
     writer; even an enabled log with an empty queue never creates the directory or file (the directory is created inside a drain, which returns
     early on an empty queue). `info/warn/error` on a disabled or stopping log return at once and queue nothing. `enabled()` is `_enabled`.
  2. **Construction.** Blank path => `Err(InvalidInput)` (C# `ArgumentException.ThrowIfNullOrWhiteSpace`); stored path = absolute path
     (`std::path::absolute`; C# `Path.GetFullPath`: does not touch the disk, normalises `..` and `/`; the porter must check that
     `std::path::absolute` collapses `..` as `GetFullPath` does, else normalise by hand with W2-00 `dotnet_path::get_full_path`);
     `max_log_bytes = max(1024, given)`.
  3. **Entry.** Taken at `Write` time on the CALLER's thread: level (`"INFO"`, `"WARN"`, `"ERROR"`), message, local timestamp, the thread number.
     The error is rendered to text at that moment too (stub decision; Open questions 3).
  4. **Line.** `"{yyyy-MM-dd HH:mm:ss.fff} [{LEVEL}] [T{n}] {message}"`; with an error: head + `"\n"` + error text (a bare LF, even though the writer
     then adds CRLF; golden `error with exception: LF between head and exception text, CRLF at the end`). Messages are verbatim (no
     `format!`-style handling of `{0}`, `{{x}}`; LF, CRLF and non-BMP text are kept; golden `message with LF and unicode...`). Milliseconds are
     TRUNCATED from the tick count (`.9999999` => `.999`; `format` takes an already truncated `millisecond`, so the truncation lives in
     `SystemLocalTime::now`, and the golden test must convert ticks with `ms = (ticks % 10_000_000) / 10_000`). Years print with 4 ASCII digits.
  5. **File.** Append, UTF-8 WITHOUT BOM, every line ends with `\r\n` (`StreamWriter.WriteLine` on Windows), never a bare `\n`
     (write `line` then `"\r\n"` explicitly; do not rely on the platform). Shared with readers (`FileShare.ReadWrite`; Rust's default
     `share_mode` on Windows is wider and fine). The file is opened per drain and flushed once per batch.
  6. **Queue.** FIFO, capacity `MAX_QUEUED_ENTRIES` = 10 000. Overflow: after enqueueing, if the count exceeds the cap the OLDEST entry is removed and
     `dropped` incremented; so exactly 10 000 are kept and the 10 001st push drops one. `pending_count` counts entries still queued (including
     written-but-not-yet-removed ones during a batch).
  7. **Drain** (writer thread; wakes on signal or every 250 ms; also once more after `stopping`):
     `drain_hook()`; empty queue => set `drained`, return. Else `writing = true`; create the parent directory; rotate when the file exists and
     `len >= max_log_bytes` (`>=`: golden test `LogAtExactLimit_IsRotated`); a rotation failure (IO error, read-only or locked backup) is swallowed and
     the entry appended to the current file; rotation is retried next drain. Then loop: snapshot the queue; format every unflushed entry BEFORE
     touching the stream; write each unflushed line (`before_entry_written(i)` before each); flush; on failure flush what was written and mark
     entries `0..=last_written` `flushed` so a retry does not write them twice, then propagate. On success mark all `flushed`, clear
     `last_drain_failed`, run `after_batch_written`, then pop from the front ONLY those snapshot entries that are still the queue head (identity,
     `Arc::ptr_eq`): the producer may have trimmed the head meanwhile (test `DrainKeepsUnwrittenEntriesWhenTheProducerTrimsDuringTheWrite`).
     Any error outside that: `last_drain_failed = true`, entries stay queued for the next signal. `finally`: `writing = false`; `drained` is set
     when the queue is empty OR the last drain failed.
  8. **Write** (producer): `drained.reset()`, `last_drain_failed = false`, enqueue, trim, signal. Never panics and never blocks on I/O.
  9. **Flush.** Budget 2000 ms. Loop while `((queue non-empty && !last_drain_failed) || writing) && remaining > 0`: signal the writer, then if `drained`
     is already set sleep 1 ms (the empty-queue drain can run between a producer's reset and enqueue; waiting on a set event would burn the whole budget)
     else wait on `drained` up to `remaining`; subtract the elapsed time (at least 1 ms). Returns at once on an unwritable log.
  10. **Shutdown / Dispose.** `stopping = true`, `enabled = false`, signal; one SHARED 2000 ms budget: wait for the writer to exit (needs a
      join with a timeout: `JoinHandle` has none, so the writer sets an `exited` flag + condvar that `wait_writer_exit` waits on, then
      `join` only when it has already exited) then `flush` with the remaining budget. A stuck writer (parked in the drain hook) must not block
      longer than ~2 s in total (test: 1.5 s..3.9 s). `dispose` runs `shutdown` once (`disposed` flag, `compare_exchange`); `set_enabled(true)`
      after dispose is a no-op; every other method after dispose is a no-op that never panics. `Drop` = `dispose`. The writer thread must
      hold an `Arc<Inner>` (never the `FileLog`) or `Drop` would never run.
  11. **set_enabled(false)**: `enabled = false`, signal, `flush()` (entries queued before are written; later ones are rejected).
      **set_enabled(true)** (also after a plain `shutdown`, which does not set `disposed`: the log restarts) takes a lock, sets `stopping = false`, `enabled = true`, starts the writer when none is alive (thread name
      `PhotoReview.LogWriter`, detached/background; it must not keep the process alive).
  12. **Thread number.** C# prints `Environment.CurrentManagedThreadId` (1 for the main thread). Rust has no such id: `current_thread_number`
      is a process-wide counter taken lazily per thread (thread-local), starting at 1. The golden masks the number (`[T]`), so only
      `format_entry` with an explicit number is pinned; tests assert `\[T\d+\]`.
  13. **`default_instance`** is a `OnceLock<FileLog>` at `AppPaths::from_environment().log_file`; never disposed (the writer is a background thread,
      the process may exit without a flush: callers call `shutdown`).
  14. **`backup_path`** (naming, golden `file-log-backup-name`): `<dir>\<GetFileNameWithoutExtension>.1<GetExtension>`; replace an existing backup.
      `GetExtension`: the part from the LAST dot of the file name, dot included, `""` when there is no dot or the dot is last (`"noext."` =>
      ext `""`, stem `"noext"`, backup `noext.1`: the trailing dot is lost; not in the golden, add a unit test calling `backup_path` directly; through `FileLog::new` Win32 path normalisation already strips the trailing dot). The file name is the part after the
      last `\` or `/`.
- Traps checklist (docs/MULTI-AGENT-PLAN.md §7): 1 not relevant (no case folding), 2 (invariant culture: never `chrono`/locale; format digits by hand,
  ASCII only), 4 (rotation by `std::fs::rename`, which replaces on Windows; do not `remove` first), 6 (no panic in the writer or a producer; a
  poisoned mutex must not take the log down: use `lock().unwrap_or_else(PoisonError::into_inner)`), 8 (CRLF only on write; the bare LF between head and
  error text), 10 (ordering: single FIFO queue and a single writer; no `HashMap`). Specific traps:
  - **`Path::extension()` differs from `Path.GetExtension`**: Rust `Path::new(".log").extension()` is `None` and `file_stem` is `".log"`, so the Rust
    std API would give `.log.1` where C# gives `.1.log` (golden `.log`). Likewise `"a..log"` => stem `"a."`, backup `a..1.log`. Split by hand on
    the last `.` of the file name.
  - **`Condvar`-based `drained` must be "set unless reset"**: the spurious-wakeup and the reset/enqueue race of note 9 is the whole reason for the 1 ms sleep.
  - **`DateTime.Now` is local time.** `std::time::SystemTime` is UTC and `std` has no local time. See Open questions 1.
  - **Do not format with `{}` of `io::Error`/`Display` of anything with a stack trace into the golden path**: the golden injects fixed texts.
  - **UTF-8 `String`**: lone surrogates cannot exist in a Rust `&str`, C# would write `?`/U+FFFD for them; not testable here, no action.
- Depends on (functions called by the implementation or its tests):
  - W0 `abstractions::Log` (merged), `app_paths::AppPaths` (merged, has `log_file`, `from_environment`), pr-test-support `golden`, `TempRoot`.
  - W2-00 `dotnet_path::get_full_path` only when `std::path::absolute` proves insufficient (see note 2; otherwise no W2 dependency).
  - No other W2 unit. `FileLog` itself is consumed later by App (`AppLog`, wave W8), not by any other W2 unit.
  - Crates: `std` only, plus `windows-sys` (feature `Win32_System_SystemInformation`) for `GetLocalTime` (Open questions 1). No `chrono`, no `time`.
- Done when: every listed test has a Rust twin with `// C#: <Class>.<Method>` (the four handle tests are listed as not ported); the three goldens pass;
  the whole file passes 20 times in a row (`for i in 1..20`; timing tests must not be flaky: use bounded waits on events, not sleeps, where the C# did);
  `cargo clippy -p pr-core --all-targets -- -D warnings` clean; the stub's `#![allow(unused_variables, dead_code)]` line removed with the last `todo!()`.

## Golden package
- `oracle/W2-16.cs` (copy to `tools/oracle-cs/Commands/W2-16.cs`; dispatch lines are in its header):
  - `file-log-format`: calls the private static `FileLog.FormatEntry` by reflection on an `Entry` built from `ticks` (as a string: JSON number precision), level,
    message, thread id, optional exception text (a `FixedTextException` whose `ToString()` is the given text); the same call is repeated under ar-SA,
    th-TH and fa-IR and the oracle FAILS if the text differs. Output `line`. 25+ cases: ms truncation, sub-ms ticks, midnight, end of day, zero padding,
    leap day, years in th/ar-SA ranges, every level.
  - `file-log-backup-name`: a 2048-byte file named `<name>` with `maxLogBytes = 1024`, one entry, `flush`; lists the sorted names left in the folder and
    whether each has 2048 bytes. 14 names (`.log`, `noext`, `a..log`, `x.tar.gz`, `ảnh.log`, `app.1.log`, ...). Verified: `.log` => `.1.log`, `noext` =>
    `noext.1`, `a..log` => `a..1.log`.
  - `file-log-bytes`: real log, entries written, `flush`, the file bytes reported as hex and text with `yyyy-MM-dd HH:mm:ss.fff [LEVEL] [Tn]` masked to
    `<ts> [LEVEL] [T]`, plus `has_bom` (always false). Cases: one info; three levels; error with exception text (LF then text then CRLF); multi-line
    exception text; error without exception; LF and unicode and braces; CRLF in the message; empty message; two entries (no BOM).
- `golden-inputs/{file-log-format,file-log-backup-name,file-log-bytes}.json`, outputs in `golden-reference/` (generated by the oracle at d92d6aae;
  reviewed here: names, ordering and masked texts match the C# above, no defect found).
- Test-side helpers (porter writes, in the test file): a ticks-to-`LogTimestamp` converter (proleptic Gregorian, civil-from-days algorithm; ticks are of
  `DateTime`, 100 ns since 0001-01-01), the same masking regex written by hand (no `regex` crate): replace `^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}
  \[(INFO|WARN|ERROR)\] \[T\d+\]` at the start of each line by `<ts> [LEVEL] [T]`, and hex encoding.

## Open questions
1. **Local time source.** C# `DateTime.Now` is local time and `std` cannot produce it. Options: (a) `windows-sys` with `GetLocalTime` in `SystemLocalTime`
   (the stub assumes this; needs a `[target.'cfg(windows)'.dependencies] windows-sys` entry in `crates/pr-core/Cargo.toml`, which is lead-owned and is
   not in the current manifest; `windows-sys` is not in the workspace-root `Cargo.toml` `[workspace.dependencies]` either, so the lead adds both); (b) keep `pr-core` free of platform code and let `pr-app` inject a `LocalTimeSource` (then `FileLog::new` could not have
   a default and `default_instance` would move to the app crate). Lead decides; the trait and `with_time_source` work for both. Until decided, the porter
   implements `SystemLocalTime` with (a) behind `#[cfg(windows)]` and a UTC fallback elsewhere.
2. **`FileLog` constructors return `io::Result`** (C# throws `ArgumentException` for a blank path and `GetFullPath` throws for NUL / too long). Alternative: panic
   like C#; rejected because AGENTS.md forbids panics on user input paths.
3. **Error text.** C# stores the `Exception` and calls `ToString()` on the writer thread, with a fallback `"\n[exception text unavailable: <Type> / <Type>]"`
   when `ToString` throws (test `ThrowingExceptionToString_...`). A Rust `&dyn Error` cannot cross threads, so the stub renders it on the caller's
   thread. Recommended text: `Display` then each `source()` as `"\n ---> {source}"` (a rough analogue of .NET's inner-exception chain; no golden pins it,
   the golden passes fixed text through `error_text`). A `Display` that panics is caught with `catch_unwind` and replaced by
   `"[exception text unavailable]"` (the type name is not available in Rust). The porter should port the C# throwing test as a panicking `Display` impl.
   Because the oracle can only inject the text, add an `#[doc(hidden)] pub fn error_with_text(&self, message, error_text: Option<String>)` ONLY if the
   golden test needs it; otherwise use a test error type whose `Display` is the fixed text.
4. **`FileLog::Default` and `Drop`**: a never-disposed process singleton plus a detached writer means the last entries can be lost at process exit unless the
   caller runs `shutdown`; same as C#. Documented, no change.
