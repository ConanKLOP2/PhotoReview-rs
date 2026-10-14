# R0003: Concurrency and C# → Rust mapping rules

**Status:** Accepted (2026-10-14). Applies to every work unit; contracts link here instead of restating it.

## Context

The C# app uses `Task`/`async`, `CancellationToken`, a WPF `Dispatcher` (UI-thread affinity enforced by tests, C#
ADR 0005), `lock`, `Interlocked`, `IDisposable` and exceptions. Its INVs depend on these exact orderings: one file
action at a time (INV-4), stale results dropped by generation (INV-1/5/7), advance once before I/O (INV-3). The app
does almost no network I/O; its blocking work is disk reads, decodes and Shell COM calls.

## Decision

### Threads, not an async runtime
- **No tokio/async-std.** Blocking work runs on explicit threads; nothing awaits on the UI thread.
- **UI thread:** one, owned by `pr-app`. Only it touches UI state. Everything else reaches it through
  `UiScheduler::post` / `invoke` (`pr_core::abstractions::UiScheduler`). `UiCompletion::wait` is never called on the
  UI thread.
- **Decode pool:** `rayon` thread pool sized by `pr-imaging` (CPU-bound decode/resample only; never file I/O).
- **I/O lanes:** dedicated named threads (`std::thread::Builder`) fed by `crossbeam-channel`: foreground source
  reads, preload reads, journal/session writes, Explorer COM (an STA thread). Priority lanes replace C#'s
  `SourceReadPriority`-aware scheduling.
- A C# method returning `Task<T>` becomes either a blocking `fn -> T` that the caller runs on the right lane, or,
  where the C# caller fires and continues, a function that queues work and returns a handle (`Receiver<T>` or a
  completion type). Never `block_on`.

### Cancellation and staleness
- `CancellationToken` → `pr_core::sync::CancelToken` (cloneable flag). Check it between steps; `token.check()?`
  returns `Err(Canceled)` like `ThrowIfCancellationRequested`.
- Generations (C# `GenerationClock`, W1-12) are `AtomicU64` counters; a result carries the generation it was started
  under and is dropped on arrival when it no longer matches (INV-1, INV-5, INV-7). Dropping, not cancelling, is the
  correctness mechanism; cancelling only saves work.

### Shared state
- `lock (x)` → `std::sync::Mutex` (or `parking_lot::Mutex` where contention matters; lead adds the dependency).
  Lock poisoning is recovered with `unwrap_or_else(PoisonError::into_inner)` where the C# code would just continue;
  never `unwrap()` a lock.
- Never hold a lock across file I/O, a decode, a COM call or a `UiScheduler` call (C# has the same rule implicitly
  via `async`; making it explicit prevents deadlocks the C# code never had).
- `Interlocked`/`volatile` → atomics with `SeqCst` unless the contract justifies weaker ordering.
- `ConcurrentDictionary` → `Mutex<HashMap>` first; change only with a measurement.

### Errors, disposal, panics
- Exceptions → `Result`. File-system errors are `io::Error` (kind + raw OS error preserve the C# exception type).
  Each `catch (X) when (cond)` becomes its own match arm; never collapse them into one `Err(_)`.
- `IDisposable` → `Drop` (RAII guards for markers, staged files, COM objects, native handles).
- Panics are bugs. A worker thread runs its job inside `catch_unwind` and reports a panic as an error to the
  requester (C#: a faulted task), so one bad file cannot kill a lane. The UI thread does not catch panics.
- `unwrap`/`expect` only on invariants proven in the same function, never on I/O or user data.

### Text, numbers, time (see also MULTI-AGENT-PLAN §7)
- Path/file-name comparisons: `pr_core::text` (generated .NET OrdinalIgnoreCase table), never `to_lowercase`.
- Timestamps: `pr_core::time::DateTimeUtc` (.NET ticks; System.Text.Json text form), never raw `SystemTime` on disk.
- Machine-read text (journal, log, CSV) is culture-invariant.

## Planned external crates (lead adds each to `[workspace.dependencies]` in the wave that first needs it)

| Crate | For | First wave |
|---|---|---|
| `serde`, `serde_json` | config, journal, session, golden files | 0 (serde_json), 1 |
| `crossbeam-channel` | I/O lanes | 2 |
| `rayon` | decode pool | 4 |
| `windows` | WIC, Shell COM, Recycle Bin, memory probe | 2/4/6 |
| `indexmap` | ordered maps where C# order leaks into output | 1 |
| `parking_lot` | only if a measured hot lock needs it | — |

## Consequences

- Porters translate `async` code by deciding the lane per call; contracts for yellow/red units name the lane.
- `pr_core::abstractions` traits are blocking and `Send + Sync`; fakes in `pr-test-support` are deterministic
  (`QueuedUiScheduler` runs posted work only when the test says so).
