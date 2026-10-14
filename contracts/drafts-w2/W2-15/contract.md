# W2-15: NavigationStatWorker (dedicated FIFO thread for per-navigation file-metadata calls)
- Color: 🟡 | Porter: **sonnet** (a small, but concurrency-sensitive unit: Mutex/Condvar queue, panic isolation, a lost-wakeup-free
  thread start; no I/O, no C# culture traps) | Test porter: haiku | Verifier: sonnet (re-read the lock ordering once)
- C# source (baseline d92d6aae): `src/PhotoReview.Core/IO/NavigationStatWorker.cs` (86 lines).
  (read with `git -C C:\MyProjects\PhotoReview show d92d6aae:src/PhotoReview.Core/IO/NavigationStatWorker.cs`)
- C# tests to port (all at d92d6aae):
  - `tests/PhotoReview.Core.Tests/IO/NavigationStatWorkerTests.cs` (4 tests):
    - `RunAsync_ContinuationDoesNotRunOnTheWorkerThread`: **do not port** (Rust has no continuations; `StatTask` is a result slot, so the property is
      trivially true; say so in a comment in the test file).
    - `RunAsync_RunsOffTheCallerAndFaultsWithTheWorkException`: the work runs on a thread other than the caller; a panic yields
      `StatError::Panicked("boom")` (assert `message.contains("boom")`; the payload of `panic!("boom")` is a `&str`, `panic_any(String)` a `String`).
    - `RunAsync_CancelledToken_DoesNotRunTheWork`: pre-cancelled token => `wait()` is `Err(Canceled)`, `ran` flag stays false.
    - `RunAsync_CancelledToken_CompletesWithoutWaitingForTheBusyWorker`: a gate item blocks the worker (`std::sync::mpsc` or `Mutex+Condvar`
      gate with a 20 s timeout), a second item with a pre-cancelled token must have `is_canceled()` true immediately after `run` returns; then
      release the gate and `wait()` the busy item == 1.
  - `tests/PhotoReview.Core.Tests/Instance/InstanceAndWorkerMutationGapTests.cs`, only the two `StatWorker_*` tests (the rest is W2-xx instance code):
    `StatWorker_ItemCancelledWhileQueued_IsCancelledWithoutRunningItsWork`, `StatWorker_WorkThrows_FaultsOnlyThatTask`.
  - NOT here: `tests/PhotoReview.App.Tests/Coordinators/ImagePresenterTests.StatOffUiThread.cs` and `StalePreviewHarness.cs` (App wave: the presenter is
    the only caller).
  - Golden test (new, required): `tests/golden/navigation-stat-worker.json` (7 cases; see Golden package).
- PARITY rows: Q-R29 option C (stat off the UI thread) => suggested new row `Q-R29c | Done | pr_core::io::navigation_stat | io_navigation_stat::* (golden
  navigation-stat-worker)`; the lead picks the row id (docs/PARITY.md has no Q-R29 row today; add it, plus the owning note for `ImagePresenter` in App wave W8).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/io/navigation_stat.rs     (stub: contracts-w2/W2-15/navigation_stat.rs, compiles clippy-clean)
    crates/pr-core/tests/io_navigation_stat.rs
- Lead-owned: `crates/pr-core/src/io/mod.rs` gets `pub mod navigation_stat;` (create `io/mod.rs` + `pub mod io;` in `lib.rs` if W2-14 has not
  already; the stub's `use crate::sync::CancelToken` exists today in `crates/pr-core/src/sync.rs`).
- Read-only input: `tests/golden/navigation-stat-worker.json`.
- Public API (stub, do NOT change signatures): `StatError {Canceled, Panicked(String)}` (+ `Display`, `Error`), `StatTask<T>::{is_completed, is_canceled,
  wait_timeout, wait}`, `NavigationStatWorker::{new, shared, run}`, `Default`, `Clone`.
- Semantics notes (verified against the C# source and by running it, see Golden package):
  1. **Single dedicated thread, FIFO.** One thread per worker, started lazily on the first `run`: check-and-set `started` while holding the queue lock (the stub keeps `started` as a separate `Mutex<bool>`; the porter may fold it into the queue mutex, but never take the two in opposite orders, and never spawn outside the lock: that is the lost-wakeup / double-thread trap), named
     `"PhotoReview navigation stat"`, background (do not join; `std::thread::Builder::spawn` and drop the handle). `Clone` shares queue and thread.
     `shared()` is a process-wide `OnceLock`. A worker created by `new()` that never receives an item never starts a thread.
  2. **Pre-cancelled token.** `run` completes the task as `Canceled` BEFORE returning, without touching the queue or starting the thread, and drops
     `work` unrun (golden `already-cancelled-token...`: `canceled_at_return: true` even while a busy item holds the worker).
  3. **Cancelled while queued.** The token is checked again when the worker dequeues the item; if cancelled, the task completes as `Canceled` and `work`
     is not called. A token cancelled AFTER the work started has no effect: the work runs to completion and its value is returned (C# never observes
     the token once `work()` is running). The worker never cancels cooperatively inside `work`.
  4. **Panic isolation.** Wrap the call in `std::panic::catch_unwind(AssertUnwindSafe(work))`; a panic becomes `StatError::Panicked(text)` with
     `&str`/`String` payloads downcast (otherwise a fixed text such as `"<non-string panic payload>"`). The thread survives and takes the next item
     (golden `panic-faults-only-that-item`). The default panic hook still prints to stderr: acceptable. The C# message is the exception text
     (`boom-<id>` in the golden): only `contains` is asserted.
  5. **Completion is once.** The first completion wins (C# `TrySet*`): `is_completed`/`is_canceled` do not consume; `wait(self)` consumes the slot;
     `wait_timeout` returns `false` on timeout and leaves the result for a later `wait`. Use `Condvar::wait_while`/`wait_timeout_while` (spurious wakeups).
  6. **Priority.** C# raises the thread to `ThreadPriority.AboveNormal`. Do NOT port it with a platform call in `pr-core` (no `windows` crate in this
     crate); leave a comment `// C# runs this thread at AboveNormal; the App layer may raise it` (Open question 1).
  7. **No continuations / no UI thread.** The C# `RunContinuationsAsynchronously` only exists because awaiters are async; nothing to port. `wait*` must
     never be called on the UI thread (documented in the stub): the Rust App layer polls `is_completed()` or waits on a worker thread.
- Traps checklist: §7 items 9 (exception -> Result: keep `Canceled` and `Panicked` as distinct branches, never merge "token cancelled" with "work failed"), 10 (no `HashMap`; FIFO is a `VecDeque`), and the concurrency rules
  of docs/adr/R0003-concurrency.md (no lock held while running `work`; the queue `Mutex` is released before `job()`; never `unwrap()` a poisoned lock:
  recover with `PoisonError::into_inner`, since `catch_unwind` already keeps the job from poisoning, but a user panic hook might).
- Depends on (functions called by the implementation or its tests):
  - W0 `sync::CancelToken` (`is_canceled`; present). No other W2 unit: this unit is independent and can merge in any order.
  - Callers (not this unit): the App-layer `ImagePresenter` port (App wave W8).
- Done when: the 5 ported tests (3 from NavigationStatWorkerTests + 2 `StatWorker_*`) have a Rust twin with `// C#: <Class>.<Method>`; the golden test passes (the Rust side replays each case: a gate item
  holds the worker, items queued in order, `cancel-while-queued` tokens cancelled, gate released; compare `status`, `value`, `canceled_at_return`,
  `ran_order`, `single_worker_thread`; `exception_type`/`message` are C#-specific: assert `faulted` <=> `Panicked` and `message` contains `boom-<id>`);
  every wait uses a 20 s timeout (no unbounded blocking in a test); the tests pass 20 times in a row (`cargo test -p pr-core --test io_navigation_stat
  -- --test-threads=1` loop); clippy clean; the stub's `#![allow(...)]` line removed together with the last `todo!()`.

## Golden package
- `oracle/W2-15.cs`: command `navigation-stat-worker` (`Commands.NavigationStatWorkerCases`). Dispatch line is in the file header; needs `W2-00.cs`
  (`W2Util.StrArray`). Case shape `{ name, busy_first, items: [ { id, kind: "value"|"throw", value, token: "none"|"cancelled-before"|"cancel-while-queued" } ] }`.
  Output per case: `outcomes[]` in item order (`status` value/canceled/faulted, `value`, `exception_type`, `message`, `canceled_at_return`),
  `ran_order` (ids in the order the work ran), `single_worker_thread`. Only deterministic facts are recorded (no thread ids, no timings).
- `golden-inputs/navigation-stat-worker.json`: 7 cases (single value; FIFO behind a busy item; pre-cancelled token; cancelled while queued with
  survivors; throw isolates; mixed; empty queue).
- `golden-reference/navigation-stat-worker.json`: produced here by compiling the unmodified C# `NavigationStatWorker.cs` at d92d6aae with the oracle file
  and a copy of `W2Util.StrArray` in a scratch console project (.NET 10): the full `tools/oracle.ps1` run needs the whole Core build and the shared
  `oracle-cs` project, so the lead regenerates `tests/golden/navigation-stat-worker.json` with `tools/oracle.ps1 -Command navigation-stat-worker`
  and it must be identical (the `message` strings are `boom-<id>`, type `InvalidOperationException`).

## Open questions
1. Thread priority: keep `AboveNormal` only in the App layer (recommended) or add a `windows-sys`-free `SetThreadPriority` call here? Stat latency on
   a busy preload is the reason C# raises it; the stub assumes it is deferred.
2. `StatError::Panicked` vs propagating the panic into `wait()` (resume_unwind): the stub (and this contract) keeps the value-typed error so the
   presenter can treat a failed stat like a failed read, as C# does with a faulted task.
