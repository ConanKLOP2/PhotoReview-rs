# W2-13: SessionWriter (debounced, bounded, thread-based session persistence)
- Color: 🔴 | Porter: opus | Test porter: haiku (the test file needs a hand-fired timer and gate decorators; the hints below are exact) | Verifier: opus (5 of the 30 tests are race/shutdown regressions found by mutation testing in C#; run the mutations at the end)
- C# source (baseline d92d6aae): `src/PhotoReview.Core/Session/SessionWriter.cs` (292 lines). Context: `Session/SessionStore.cs` (W2-12), `Abstractions/ILog.cs`.
- C# tests to port (all `tests/PhotoReview.Core.Tests/Session/`; Rust names mirror the C# ones, each with `// C#: <Class>.<Method>`):
  - `SessionWriterTests.cs` (15): `ManyUpdatesCoalesceIntoOneWrite`, `UpdateAfterWriteStartsNewWindow`, `NewerSnapshotWinsOverOlderTimerBatch`, `FlushWritesImmediatelyAndOnlyOnce`, `FlushOnFolderChangePersistsOldFolder`, `EachFolderKeepsItsOwnPendingState`, `UpdateCopiesTheState`, `DisposeFlushes`, `Dispose_ReturnsWithinBound_WhenWriteInFlight`, `Dispose_WhileTimerHoldsDrainedBatch_StillWritesIt`, `Dispose_WhileTimerHoldsDrainedBatch_ReturnsOnlyAfterTheWriteLanded`, `WriteBatch_NonIoException_DoesNotDropRestOfBatch`, `Update_TrailingSeparatorVariants_NewestWins`, `Flush_WriterBusy_ReturnsAfterBoundedWaitAndRequeues`. `Update_NullSkipped_DoesNotThrow`: NO Rust twin (`Vec<String>` cannot be null); say so.
  - `SessionWriterShutdownAndVersionTests.cs` (15 methods, 18 cases): `Update_CustomDebounce_IsPassedToTheTimer`, `Update_NoDebounceGiven_UsesDefaultDebounce`, `Update_AfterDispose_IsIgnored`, `Update_FolderThatCannotBeCanonicalised_DoesNotThrowAndOtherFoldersAreStillWritten`, `Flush_StaleBatchDrainedBeforeNewerUpdate_DoesNotOverwriteTheNewerState`, `Flush_SaveWorkerCannotBeStarted_ReleasesTheWriterLockAndTheStateIsWrittenByTheNextFlush`, `Flush_TimedOutBatchWhoseStateWasSuperseded_IsNotRequeuedOverTheNewerState`, `Flush_FailingWrite_IsLoggedWithTheFolderAndTheException`, `Dispose_NothingEverPending_LogsNoError`, `Dispose_AfterACompletedWrite_LogsNoError`, `Dispose_WhenTheDrainedBatchIsWrittenWhileWaiting_ReturnsWithoutTimeoutError`, `Dispose_WriteStuckLongerThanShutdownWait_GivesUpAfterTheBoundIncludingTimeAlreadySpent` (`#[ignore = "slow: waits the 2 s shutdown budget"]`), `Dispose_SaveStallsAfterTheLockWasAcquired_StillReturnsWithinTheBoundAndWritesLaterInTheBackground`, `Constructor_InvalidDebounce_Throws` (rows `0` and `4294967295 ms`; the negative rows have no `Duration` twin, say so), `BrokenTimer_DoesNotFaultOrWedgeTheWriter`.
  - Own Rust tests: `Completion` (new/completed/complete idempotent/wait_timeout true+false); `ThreadTimer`: `start(20 ms, token)` + `wait` gives `Elapsed`; a token cancelled before/while waiting gives `Canceled` and returns well before the span (use span 30 s and assert it returns, hang-guarded); `SessionWriter: Send + Sync`; dropping a writer with a pending state writes it (`Drop` = `dispose`).
  - Golden (small, timing-free): `tests/golden/session-writer-sequence.json` (see "Golden package" below). The races/bounded waits have no golden; the C# tests are the oracle. The folder identity used for coalescing (`KeyOf`) is also pinned by W2-12's golden `session-key`.
- PARITY rows: F-23 (500 ms debounce, newest state per folder, flush on folder change, dispose waits at most 2 s), F-22 (the write part)
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/session/writer.rs
    crates/pr-core/tests/session_writer.rs
- Lead declares: `pub mod writer;` in `session/mod.rs` (see W2-12). No new crates.
- Read-only inputs: none
- Public API (stubbed in writer.rs, do NOT change signatures; the constants, `InvalidDebounce`, `TimerOutcome`, `TimerError`, traits, `Completion` data, private structs are complete):
```rust
pub const DEFAULT_DEBOUNCE: Duration;  pub const SHUTDOWN_WAIT: Duration;  pub const MAX_DEBOUNCE_MILLISECONDS: u64;
pub struct InvalidDebounce(pub Duration);
pub enum TimerOutcome { Elapsed, Canceled }   pub type TimerError = Box<dyn Error + Send + Sync>;
pub trait DebounceTimer: Send + Sync { fn start(&self, span: Duration, cancel: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError>; }
pub trait RunningTimer: Send { fn wait(self: Box<Self>) -> Result<TimerOutcome, TimerError>; }
pub struct ThreadTimer;                       // production DebounceTimer
pub struct Completion { .. }                  // new() completed() complete() is_complete() wait() wait_timeout(Duration)->bool
pub type SaveSpawner = Box<dyn Fn(Box<dyn FnOnce() + Send + 'static>) -> io::Result<Completion> + Send + Sync>;
pub type Hook = Box<dyn Fn() + Send + Sync>;
pub struct SessionWriter { .. }               // Send + Sync; Debug (manual)
impl SessionWriter {
    pub fn new(store: Arc<SessionStore>, log: Option<Arc<dyn Log>>, debounce: Option<Duration>, timer: Option<Arc<dyn DebounceTimer>>) -> Result<Self, InvalidDebounce>
    pub fn update(&self, state: &SessionState) -> Result<(), BlankFolder>
    pub fn flush(&self)
    pub fn flush_async(&self) -> io::Result<JoinHandle<()>>
    pub fn when_idle(&self)
    pub fn dispose(&self)
    pub fn set_bounded_wait_for_tests(&self, Duration)   set_after_drain_for_tests(Option<Hook>)
    pub fn set_before_shutdown_wait_for_tests(&self, Option<Hook>)   set_save_spawner_for_tests(Option<SaveSpawner>)   // #[doc(hidden)]
}
```
  Todo bodies: every `todo!()` (including `Drop for SessionWriter`, which the porter ADDS: `fn drop(&mut self) { self.dispose() }`; the stub has none yet because `Drop` with `todo!()` would abort tests during unwinding).

## Design summary (read this first)
Threads, not async (R0003). Per debounce window ONE short-lived timer thread (`PhotoReview.SessionTimer`); every bounded flush runs its save on ONE dedicated save thread (`PhotoReview.SessionSave`) so a stalled disk can never hang the UI thread or the shutdown. `Mutex<GateState>` replaces `lock (_gate)`; `Condvar in_flight_changed` replaces `Monitor.Wait/PulseAll(_gate)`; a small `WriteLock` (a `Mutex<bool>` + `Condvar`, timed acquire, released by another thread) replaces `SemaphoreSlim(1,1)`; `CancelToken` replaces `CancellationTokenSource`; `DebounceTimer` replaces the `delay` function so tests fire the timer by hand. Panics are caught around the store call (`catch_unwind`, message via `abstractions::panic_message`) so one bad folder cannot kill a lane.

## Semantics notes

### 1. State (all under `gate: Mutex<GateState>`, recover poison with `unwrap_or_else(PoisonError::into_inner)`)
- `disposed: bool`.
- `pending: Vec<(String, SessionState)>`: the newest snapshot per folder key, in FIRST-insertion order (C# `Dictionary` iteration order; replace the value in place, keep the position). Few entries: linear search.
- `versions: HashMap<String, i64>`: per key, a counter incremented by every `update` for that key, never cleared (wrapping add is fine).
- `timer: Option<(u64, CancelToken)>`: the scheduled debounce window; `None` = nothing scheduled. `next_timer_id` makes ids unique.
- `last_run: Completion` (initially `Completion::completed()`): the most recently started timer thread.
- `in_flight: u32`: batches drained by some writer (timer thread or flusher) and not yet written.
- Key: `key_of(folder)` = `SessionStore::canonical_folder(folder)` on `Ok`, the raw `folder` string on ANY `Err(SessionPathError)` (`Invalid | TooLong`; `Blank` cannot occur after the blank check) (C# catches ArgumentException, NotSupportedException, PathTooLongException); the map key is `text::ignore_case_key(&key)` (OrdinalIgnoreCase). So `C:\a\`, `C:\a` and `C:\A\` are one entry and the newest wins.

### 2. `new`
`debounce` default `DEFAULT_DEBOUNCE`; `Err(InvalidDebounce(d))` when `d.is_zero()` or `d.as_millis() > MAX_DEBOUNCE_MILLISECONDS as u128` (zero would write inside `update` on the caller's thread). `timer` default `Arc::new(ThreadTimer)`. Nothing starts until the first `update`.

### 3. `update(state)`
1. `state.folder.trim().is_empty()` -> `Err(BlankFolder)`.
2. `snapshot = state.clone()` BEFORE taking the gate (a deep copy: later mutation by the caller is not observed).
3. Under the gate: if `disposed` return `Ok(())` (nothing recorded, nothing scheduled). `key = key_of(&snapshot.folder)`; `versions[key] += 1`; upsert `pending[key] = snapshot`.
4. If `timer.is_some()` return (the scheduled write will pick up the newest state).
5. Otherwise schedule, STILL under the gate (as C# runs the async method's synchronous part inside the lock): `id = next_timer_id++`, `token = CancelToken::new()`, `timer = Some((id, token.clone()))`, `completion = Completion::new()`, `last_run = completion.clone()`. Call `self.timer.start(debounce, &token)`:
   - `Err(e)`: log `Session write timer failed` (`Log::error(msg, Some(&*e))`), `timer = None`, `completion.complete()`; the pending state stays queued for the next `update`'s timer, `flush` or `dispose`. (C# test `BrokenTimer_DoesNotFaultOrWedgeTheWriter`: the first `_delay` call throws, `calls == 2` after a second update.)
   - `Ok(running)`: spawn the timer thread (`std::thread::Builder::new().name("PhotoReview.SessionTimer")`) running `run_timer(shared, id, running, completion)`. If the OS cannot spawn: same handling as the `Err(e)` branch above (log the `io::Error`).
6. The timer thread ALWAYS completes `completion` before it exits (drop guard, also on panic), so `when_idle` never hangs.

### 4. `run_timer` (timer thread)
```
match running.wait() {
    Err(e)               => { log "Session write timer failed"(e); lock gate; if timer id == id { timer = None }; return }
    Ok(TimerOutcome::Canceled) => return,
    Ok(TimerOutcome::Elapsed)  => {}
}
lock gate; if timer id != Some(id) { return }      // flushed (or replaced) while we waited
timer = None; unlock
catch_unwind(write_pending(bounded = false)); on panic log "Session write failed"(PanicError(msg))
```
Important: a test timer may report `Elapsed` AFTER `flush` cancelled it (C# `FlushWritesImmediatelyAndOnlyOnce` fires the timer after the flush): the id check is what prevents a second write. Never skip it.

### 5. `flush()` (bounded)
```
{ lock gate; t = timer.take() }          // the schedule is forgotten
if let Some((_, token)) = t { token.cancel() }
write_pending(bounded = true)
```
`flush_async` = the same on a new thread named `PhotoReview.SessionFlush` (clone the `Arc<Shared>`, NOT a `SessionWriter`, so dropping the clone cannot trigger `Drop`); `Err` only when the spawn fails. Every flush uses the bounded path (the UI thread flushes on folder change and must never hang behind a stuck write).

### 6. `write_pending(bounded)`
1. Under the gate: `batch` = for each pending entry `(key, state, versions[key])`, in pending order; `pending.clear()`; if the batch is non-empty `in_flight += 1`.
2. Empty batch: return (nothing was counted).
3. A drop guard decrements `in_flight` and `in_flight_changed.notify_all()` when the function leaves, also on panic and early return.
4. Run the `after_drain` hook (clone the `Arc` out of `hooks` first; never hold `hooks` or the gate while calling it). The hook may call `update`/`flush` (a C# test does).
5. `write_batch(batch, bounded)`.

### 7. `write_batch(batch, bounded)` (batch in an `Arc<Vec<..>>`)
- Not bounded (timer thread): `write_lock.acquire()` (blocking); `save_batch`; `write_lock.release()` (guard).
- Bounded (flush, dispose):
  1. `budget = Instant::now()`; `wait = bounded_wait` (default `SHUTDOWN_WAIT`, test override).
  2. `if !write_lock.acquire_timeout(wait)`: log error `Session write skipped: writer busy`; `requeue_skipped(batch)`; return.
  3. The lock is now ours. Start the save on its own thread through the spawner (default: `Builder::new().name("PhotoReview.SessionSave")` thread running the closure and completing the returned `Completion`): the closure runs `save_batch(batch)` and RELEASES `write_lock` in a drop guard (the save thread owns the lock until it is done, even if the caller gave up waiting).
  4. If the spawner returns `Err(e)` (C# D-07): release `write_lock` yourself, log error `Session write skipped: the save worker could not be started` with `e`, `requeue_skipped(batch)`, return. (The closure was dropped without running, so nothing else releases the lock.)
  5. `remaining = wait.saturating_sub(budget.elapsed())`; `if !completion.wait_timeout(remaining)` log error `Session write may be lost at shutdown: the save is still running after the wait budget`. The state is NOT requeued (the save thread still writes it).
  One budget covers the lock wait AND the save: acquiring a free lock says nothing about how long the synchronous save takes (stalled NAS).

### 8. `save_batch(batch)` (runs on timer thread or save thread, lock held)
For each `(key, state, version)` in order: lock the gate; `if versions.get(key) != Some(&version) { continue }` (a newer snapshot arrived while this batch waited: never overwrite it); unlock; then `catch_unwind(|| store.save(&state))`:
`Ok(Ok(()))` fine; `Ok(Err(BlankFolder))` or a panic -> log error `Session write failed: <state.folder>` with the error (panic: wrap its text in a small `PanicError(String)`: `Error`); CONTINUE with the next entry (a failed write never drops the rest of the batch). `store.save` already logs and swallows I/O errors itself.

### 9. `requeue_skipped(batch)`
Under the gate: `if disposed { return }`; for each `(key, state, version)`: if `versions[key] == version` insert `(key, state)` into `pending` only when the key is absent (C# `TryAdd`); a newer snapshot already in `pending` wins. A batch whose version was superseded is dropped (test `Flush_TimedOutBatchWhoseStateWasSuperseded...`).

### 10. `dispose()`
1. Under the gate: if `disposed` return; `disposed = true`. (Second call: no-op.)
2. `started = Instant::now()`; `flush()` (bounded, as above).
3. `wait_for_in_flight(SHUTDOWN_WAIT.saturating_sub(started.elapsed()))`: lock; `if in_flight == 0 return`; call the `before_shutdown_wait` hook WHILE HOLDING the gate (as C#; the hook only signals); `deadline = now + timeout`; loop `while in_flight > 0 { remaining = deadline - now; if remaining is zero { log error "Session write may be lost at shutdown: a write was still in flight"; return } ; guard = in_flight_changed.wait_timeout(guard, remaining) }`.
   Why: the timer thread may have drained the pending states and not written them yet, so `flush` saw an empty batch; without this wait the process could exit under the last write (C# R2-A-04). The total bound is 2 s INCLUDING the time `flush` already spent (test `Dispose_WriteStuckLongerThanShutdownWait...`: bounded wait 1.9 s -> the whole dispose ends near 2 s, not 4 s).
4. `Drop for SessionWriter` calls `dispose()`.

### 11. `when_idle()`
`let c = { lock gate; last_run.clone() }; c.wait()`. It waits for the most recently started TIMER thread only (not for a flusher or a detached save thread; tests that need the detached save to land wait for its own signal, as the C# tests do with `Wait.UntilAsync`).

### 12. Test hooks (all `#[doc(hidden)] pub`, stored in `Mutex<Hooks>`)
`set_bounded_wait_for_tests` (default `SHUTDOWN_WAIT`), `set_after_drain_for_tests`, `set_before_shutdown_wait_for_tests` (invoked with the gate held: must not call writer methods), `set_save_spawner_for_tests(None)` restores the default spawner. Read each setting at the moment it is needed (the tests change them between calls).

### 13. Log messages (tests match on substrings)
`Session write timer failed`, `Session write failed: <folder>`, `Session write skipped: writer busy`, `Session write skipped: the save worker could not be started`, `Session write may be lost at shutdown: the save is still running after the wait budget`, `Session write may be lost at shutdown: a write was still in flight`. All via `Log::error`; `log` is optional (`None` = no logging). Prefer logging after releasing the gate.

## Test-porter kit (exact hints for the awkward parts)
- **ManualTimer** (in `tests/session_writer.rs`): `DebounceTimer::start` records `span` in `requested: Mutex<Vec<Duration>>` and registers a waiter `Arc<(Mutex<Option<TimerOutcome>>, Condvar)>`, returning a `RunningTimer` whose `wait` blocks on that waiter; `fire_all()` sets every registered waiter to `Elapsed`; the running timer also watches the `CancelToken` clone: loop `wait_timeout(5 ms)` and return `Canceled` when the token is cancelled and nobody fired (a registered waiter is set by `fire_all` even after cancel: that is the "fire after flush" test). `start` runs synchronously inside `update`, so `requested.len()` is exact right after `update` returns (the C# `Requests` assertions port 1:1).
- **BrokenTimer**: a `DebounceTimer` whose first `start` returns `Err`, later ones delegate to a ManualTimer; count calls with an `AtomicUsize`.
- **GateFs**: a test decorator implementing `FileSystem` over `Arc<FakeFileSystem>`; `write_all_text_atomic` first runs a configurable hook (`Option<Arc<dyn Fn(&Path) + Send + Sync>>`) that can block on a channel/`Condvar`, signal "entered", or `panic!`; everything else forwards to the fake. This is the Rust image of the C# `WriteHook`. Never `sleep` to wait for a thread: block on a channel with `recv_timeout(Duration::from_secs(20))` and fail with a message on timeout (hang guard).
- **Metrics** for write counts: `SessionStore::new(.., Some(Arc::new(move || { n.fetch_add(1, SeqCst); })))`.
- "Wait until `load` returns X" (C# `Wait.UntilAsync`): have `GateFs` send on a channel AFTER the inner write returns, and wait for that message; no polling.
- Shorten the real waits: tests that C# runs with the 2 s default may call `set_bounded_wait_for_tests(Duration::from_millis(200))`, except `Dispose_WriteStuckLongerThanShutdownWait...` which uses 1900 ms and is `#[ignore = "slow"]`.
- `Dispose_ReturnsWithinBound_WhenWriteInFlight`: dispose on a helper thread, result via `recv_timeout(20 s)`; after releasing the gate, "a" lands, "b" is never written.
- `Flush_FailingWrite_IsLoggedWithTheFolderAndTheException`: C# throws an `InvalidDataException` from the write hook and asserts `Assert.Same(boom, error.Exception)`. In Rust the GateFs hook `panic!("boom")`s (an `io::Error` would be swallowed by `store.save` and logged by the STORE, not the writer); assert exactly one writer error whose message contains the folder and whose error text contains `boom` (`Assert.Same` has no twin: the panic is wrapped in `PanicError`).
- `before_shutdown_wait` hook: clone the `Arc` out of `hooks` before taking the gate (so `hooks` is not held), then call it with the gate held.
- `Update_FolderThatCannotBeCanonicalised...`: folder `"C:\\bad\0folder"` is accepted by `update` (key falls back to the raw string); `flush` logs through the store (path error) and the other folder is still written.

## Traps checklist
MULTI-AGENT-PLAN section 7 items 1 (`ignore_case_key` for keys), 9 (each C# `catch` is its own arm: timer start failure, timer wait failure, spawn failure, save panic), 10 (`pending` is an ordered Vec, never a `HashMap`). R0003: no tokio; no lock held across file I/O, `store.save`, a hook (except the documented shutdown hook), a log call or a thread spawn that can block (spawning under the gate in `update` is intended and cheap); drop guards for `in_flight`, the write lock and `Completion`; `catch_unwind` around the store call and the unbounded timer-thread write; never `unwrap()` a lock.

## Depends on (OTHER units whose FUNCTIONS this unit's code or tests call)
- W2-12 (this wave, same dependency direction as C#): `SessionStore::{save, load, canonical_folder, get_path}`, `SessionState`, `BlankFolder`. Port this unit after W2-12 is merged (the tests call `load`).
- Wave 0 (merged): `sync::CancelToken`, `text::ignore_case_key`, `abstractions::{Log, panic_message}`.
- Test support (merged): `FakeFileSystem`, `RecordingLog`, `TempRoot` not needed. Metrics: a closure (W2-17 `ReviewMetrics` is NOT needed).

## Done when
All listed tests pass, 20 consecutive runs of `cargo test -p pr-core --test session_writer -- --test-threads=1` pass (races), clippy `-D warnings` and `cargo fmt` clean, every C# test has a Rust twin with `// C#: <Class>.<Method>` or a stated reason, `#![allow(unused_variables, dead_code)]` removed. Verifier mutations that must make a test fail: drop the timer-id check in `run_timer`; drop the version check in `save_batch`; do not decrement `in_flight` on early return; do not release `write_lock` when the spawner fails; wait the full 2 s after the flush in `dispose`; requeue after dispose; requeue a superseded version; replace `ignore_case_key` by the raw key; coalesce nothing (schedule one timer per update); write after `dispose`; make `update` store the caller's state without cloning.

## Golden package
- `oracle/W2-13.cs`: command `session-writer-sequence` (dispatch line in the file header; needs `W2-00.cs` for `W2Util`). Drives a real `SessionWriter`
  over a real temp `SessionStore` with a hand-fired `delay` (TaskCompletionSource list); ops `update` / `fire` / `flush` / `dispose`; reports the
  constructor exception type, the timer spans (ms), per op `{exception, timer_requests, files}` and per probe folder the loaded `{current, skipped}`.
- `golden-inputs/session-writer-sequence.json` (14 cases): constructor bounds (`null`, 1234, 0, -1, 4294967294, 4294967295), coalescing, new window
  after a write, trailing-separator/case variants as one folder, per-folder state, late timer fire after flush, flush/dispose with nothing pending,
  update after dispose, blank folder (`ArgumentException`), uncanonicalisable folder.
- `golden-reference/`: EMPTY. Not generated (the oracle runner `tools/oracle.ps1` needs the file copied into `tools/oracle-cs`, outside this
  draft folder); the Golden Generator role copies the file, adds the dispatch line and runs it.
- Rust twin: `tests/session_writer.rs` golden test builds the writer with the ManualTimer; `timer: Some(..)` spans compare as ms; ops map 1:1
  (`BlankFolder` <-> `ArgumentException`; the negative debounce row is unrepresentable with `Duration`: assert only the C# value `ArgumentOutOfRangeException`
  is documented, skip the row in Rust; `4294967295` => `InvalidDebounce`). `fire` = `fire_all()` then `when_idle()`.

## Open questions
- Q1: the production `ThreadTimer` polls the cancel token every <= 10 ms. If the lead prefers zero polling, `CancelToken` would need a wake-up primitive (`sync.rs` is lead-owned); not needed for correctness.
- Q2: `Completion` is defined here but `store.rs`/`navigation_stat.rs` have their own small completion helpers; the lead may hoist one `Completion` into `pr_core::sync` later (D8).
- Q3: `set_*_for_tests` are `pub #[doc(hidden)]` because integration tests (`crates/pr-core/tests/`) cannot see `pub(crate)`. Alternative: put the tests inside `writer.rs` under `#[cfg(test)]` (then they would be owned by the porter of this file).
