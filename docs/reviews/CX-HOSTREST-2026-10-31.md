# CX-HOSTREST: host shell mutation follow-up, 2026-10-31

Status: Partial. Branch `feat/cx-hostrest`, starting HEAD
`7c93960207bdd9fa232459928778fe7562ba3529`. Tests and evidence only; production
code, dependencies and unsafe-audit baseline unchanged. Orchestrator owns commits.

Read first: AGENTS mutation section, LESSONS, the lane's PARITY rows,
[CX-HOST](CX-HOST-2026-10-30.md), [CX-HOST2](CX-HOST2-2026-10-30.md),
[CL-HOSTX](CL-HOSTX-2026-10-30.md), [CL-HOST2](CL-HOST2-2026-10-30.md),
[CL-HOSTRERUN](CL-HOSTRERUN-2026-10-30.md) and
[CL-HOSTRERUN2](CL-HOSTRERUN2-2026-10-31.md).
The requested CL-HOST3 report and `w8_host3_killers.rs` are absent at this HEAD.
No claim from those absent files is assumed. C# sources were read only with
`git show 7eaee2a2:src/PhotoReview.Shell.Win32/Hosting/{MessageLoop,ShellWindow}.cs`.

## Source, gap and inventory

All paths below are under `crates/pr-platform-win/src/host/`.

| PARITY row | Source | Current listed mutants | Gap at entry |
|---|---|---:|---|
| X-host-shell-window-messages | `shell_window/messages.rs` | 33 | No mutation verdict for messages; dpi has separate rerun evidence |
| X-host-swap-chain-clock / contracts / icons | `swap_chain_clock.rs` | 36 | CL-HOSTX reports 33 caught + 3 unviable; other row gaps remain |
| X-host-message-loop | `message_loop.rs` | 84 | CL-HOSTX reports 11 hangs; these need prompt test failures, not timeout-as-caught |

Inventory: `cargo mutants -p pr-platform-win --list --file '**/host/shell_window/messages.rs' --file '**/host/swap_chain_clock.rs' --file '**/host/message_loop.rs'`.
Total: 153. None of these three files has inline unit tests, so a second `--lib`
mutation pass does not apply. Raw Win32/COM composition is excluded from new
mutation work. Earlier completed swap-chain policy mutation was not repeated.

## Killers and functional validation

New binary: `crates/pr-platform-win/tests/w8_hostrest_killers.rs`.

- Four pure fake tests exercise idle/background passes, clock identity and
  polling, handle filtering/cap, ordinary input and quit. A deterministic
  callback budget panics on the test thread if a mutant makes the loop spin.
  No helper-thread join, sleep or wall-clock deadline is used for the verdict.
- Three native-ignored tests exercise activation, size events and cancellation
  with hidden ShellWindows owned by the test thread. Windows are never shown;
  synchronous messages require no pumping helper thread. Windows are disposed
  explicitly or by their owner on unwind. These are hidden top-level shell
  windows, not message-only dispatcher windows.
- Seven killers pass with `--include-ignored --test-threads=1`; 20 consecutive
  executions pass, 140 test executions. This checks repeatability, not hardware
  performance or attended desktop behavior.
- Covering binaries pass with native enabled: `host_message_loop` 30,
  `host_message_loop_native` 10, `host_shell_window` 32,
  `host_swap_chain_clock` 16: 88 passed, zero failed/ignored.
- Native staging passed, hashes checked against C# baseline: normal target
  `debug`, `debug/deps`, `native/x64`, and mutation target `debug`, `debug/deps`.
  Initial Git ownership errors were resolved with process-local `safe.directory`
  entries for this checkout and C#; no global configuration changed.

Normal builds use `<worktree>/target`; mutation uses `<worktree>/target-mut`.
`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, two build jobs,
one mutation process at a time. No normal cargo test was run in target-mut.
Covering tests use an isolated data root under target and
`PHOTOREVIEW_RS_ISOLATE_CONFIG=1`; the new tests do not access user data.

## Mutation method and ledger

Command shape (run after the unmutated killers command passed):

```text
cargo mutants -p pr-platform-win --baseline skip --timeout 90 -j 1
  --file '**/host/<file>.rs' -F '<file>.rs:(<line>|...):'
  --cargo-arg=--test=w8_hostrest_killers
  --cargo-test-arg=-- --cargo-test-arg=--include-ignored
  --cargo-test-arg=--test-threads=1 --output <worktree>/mut-out-<group>
```

Only the new killers binary is selected. Each caught verdict must contain a
real `running 7 tests` and a failing assertion in its log. Timeouts are not
counted as caught. Final outcome ledger and exact NOT RUN list follow below.

## Gates and limits

`cargo fmt -p pr-platform-win --check`: PASS.
`cargo clippy -p pr-platform-win --all-targets --locked -j 2 -- -D warnings`: PASS.
Killers and covering results above are from this worktree. Workspace tests,
attended hardware/performance measurements and real RAW files are not a claim
of this lane. No PARITY row is raised to Done by a selected partial verdict.

## Final results of this lane

| Source | Listed | Tested | Caught | Missed | Unviable | Timeout | NOT RUN |
|---|---:|---:|---:|---:|---:|---:|---:|
| `shell_window/messages.rs` | 33 | 12 | 12 | 0 | 0 | 0 | 21 |
| `swap_chain_clock.rs` | 36 | 0 | 0 | 0 | 0 | 0 | 36 |
| `message_loop.rs` | 84 | 15 | 15 | 0 | 0 | 0 | 69 |
| Total | 153 | 27 | 27 | 0 | 0 | 0 | 126 |

These are 27 distinct current mutants, all caught by the new killers binary
alone. No surviving or equivalent mutants were claimed in this selected run.
All 27 logs were checked: build success, Test failure code 101, actual
`running 7 tests`, and `test result: FAILED`. Compiler/argument errors are not
counted as caught. Test-thread callback-budget assertions replace spinning
behavior for the selected after_pump/pump_messages mutations.

Logs/JSON remain in `mut-out-messages/mutants.out`,
`mut-out-message-loop/mutants.out`, `mut-out-message-state/mutants.out`;
unmutated covering/repeat/clippy logs and inventory remain in
`target/hostrest-evidence`. These ignored artifacts are local evidence for the
orchestrator; all per-mutant verdicts are preserved below for review.

Filters: `messages.rs:(111|119|120|130|144|147|154):` (8),
`message_loop.rs:(238|239|347|351|356|366|368|424|426|430|432):` (15),
`messages.rs:(45|55|89):` (4). Each Found count agrees with its selection in the
153-entry inventory. These killers-alone reruns are the new mutation proof;
no old broad-suite result is added to the counts. Group elapsed reports: 7m,
8m, 2m respectively; overall work stayed within the approximately 45m budget.

### Caught mutant ledger

```text
crates/pr-platform-win/src/host/shell_window/messages.rs:111:9: replace ShellWindowInner::update_client_size with ()
crates/pr-platform-win/src/host/shell_window/messages.rs:119:9: replace ShellWindowInner::raise_client_size_changed_if_needed with ()
crates/pr-platform-win/src/host/shell_window/messages.rs:120:17: replace == with != in ShellWindowInner::raise_client_size_changed_if_needed
crates/pr-platform-win/src/host/shell_window/messages.rs:130:9: replace ShellWindowInner::update_active with ()
crates/pr-platform-win/src/host/shell_window/messages.rs:130:19: replace == with != in ShellWindowInner::update_active
crates/pr-platform-win/src/host/shell_window/messages.rs:144:9: replace ShellWindowInner::raise_closing with ()
crates/pr-platform-win/src/host/shell_window/messages.rs:147:12: delete ! in ShellWindowInner::raise_closing
crates/pr-platform-win/src/host/shell_window/messages.rs:154:9: replace ShellWindowInner::raise_event with ()
crates/pr-platform-win/src/host/message_loop.rs:238:9: replace MessageLoop::add_frame_clock with ()
crates/pr-platform-win/src/host/message_loop.rs:239:12: delete ! in MessageLoop::add_frame_clock
crates/pr-platform-win/src/host/message_loop.rs:347:9: replace MessageLoop::after_pump -> Result<(), HostError> with Ok(())
crates/pr-platform-win/src/host/message_loop.rs:351:16: delete ! in MessageLoop::after_pump
crates/pr-platform-win/src/host/message_loop.rs:356:12: delete ! in MessageLoop::after_pump
crates/pr-platform-win/src/host/message_loop.rs:366:9: replace MessageLoop::pump_messages -> Option<i32> with None
crates/pr-platform-win/src/host/message_loop.rs:366:9: replace MessageLoop::pump_messages -> Option<i32> with Some(0)
crates/pr-platform-win/src/host/message_loop.rs:366:9: replace MessageLoop::pump_messages -> Option<i32> with Some(1)
crates/pr-platform-win/src/host/message_loop.rs:366:9: replace MessageLoop::pump_messages -> Option<i32> with Some(-1)
crates/pr-platform-win/src/host/message_loop.rs:368:16: delete ! in MessageLoop::pump_messages
crates/pr-platform-win/src/host/message_loop.rs:424:9: replace MessageLoop::poll_clocks -> Result<(), HostError> with Ok(())
crates/pr-platform-win/src/host/message_loop.rs:426:16: delete ! in MessageLoop::poll_clocks
crates/pr-platform-win/src/host/message_loop.rs:430:28: replace && with || in MessageLoop::poll_clocks
crates/pr-platform-win/src/host/message_loop.rs:430:23: replace != with == in MessageLoop::poll_clocks
crates/pr-platform-win/src/host/message_loop.rs:432:52: replace <= with > in MessageLoop::poll_clocks
crates/pr-platform-win/src/host/shell_window/messages.rs:45:13: delete match arm WM_SIZE in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:55:13: delete match arm WM_ACTIVATE in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:55:73: replace != with == in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:89:13: delete match arm WM_CLOSE in ShellWindowInner::process_message
```

### Exact NOT RUN list (126)

This list means not run in this lane. Swap-chain 36 were not repeated because
CL-HOSTX already reports a pure-policy pass; that result is not newly verified
here. Other unselected logic remains a gap; raw Win32/COM glue is excluded.
No claim is made that all 11 historical message-loop timeouts have been
individually mapped or resolved: CL-HOSTX does not enumerate their exact
operators. The 15 exact operators above are the entire new loop verdict.
Broader mutation/native exhaustion is not claimed in this time-box.

```text
crates/pr-platform-win/src/host/message_loop.rs:52:9: replace MessageFilter::as_raw -> Option<&dyn RawMessageFilter> with Some(Box::leak(Box::new(Default::default())))
crates/pr-platform-win/src/host/message_loop.rs:115:9: replace <impl LoopOs for Win32LoopOs>::peek_message -> bool with true
crates/pr-platform-win/src/host/message_loop.rs:115:9: replace <impl LoopOs for Win32LoopOs>::peek_message -> bool with false
crates/pr-platform-win/src/host/message_loop.rs:119:9: replace <impl LoopOs for Win32LoopOs>::is_message_pending -> bool with true
crates/pr-platform-win/src/host/message_loop.rs:119:9: replace <impl LoopOs for Win32LoopOs>::is_message_pending -> bool with false
crates/pr-platform-win/src/host/message_loop.rs:124:9: replace <impl LoopOs for Win32LoopOs>::translate_and_dispatch with ()
crates/pr-platform-win/src/host/message_loop.rs:132:9: replace <impl LoopOs for Win32LoopOs>::poll_handle -> bool with true
crates/pr-platform-win/src/host/message_loop.rs:132:9: replace <impl LoopOs for Win32LoopOs>::poll_handle -> bool with false
crates/pr-platform-win/src/host/message_loop.rs:136:9: replace <impl LoopOs for Win32LoopOs>::wait -> WaitOutcome with Default::default()
crates/pr-platform-win/src/host/message_loop.rs:137:19: replace == with != in <impl LoopOs for Win32LoopOs>::wait
crates/pr-platform-win/src/host/message_loop.rs:142:19: replace < with == in <impl LoopOs for Win32LoopOs>::wait
crates/pr-platform-win/src/host/message_loop.rs:142:19: replace < with > in <impl LoopOs for Win32LoopOs>::wait
crates/pr-platform-win/src/host/message_loop.rs:142:19: replace < with <= in <impl LoopOs for Win32LoopOs>::wait
crates/pr-platform-win/src/host/message_loop.rs:144:26: replace == with != in <impl LoopOs for Win32LoopOs>::wait
crates/pr-platform-win/src/host/message_loop.rs:152:9: replace <impl LoopOs for Win32LoopOs>::post_quit with ()
crates/pr-platform-win/src/host/message_loop.rs:159:5: replace wait_timeout_ms -> u32 with 0
crates/pr-platform-win/src/host/message_loop.rs:159:5: replace wait_timeout_ms -> u32 with 1
crates/pr-platform-win/src/host/message_loop.rs:160:23: replace == with != in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:163:23: replace <= with > in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:167:66: replace / with % in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:167:66: replace / with * in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:167:61: replace - with + in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:167:61: replace - with / in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:167:49: replace + with - in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:167:49: replace + with * in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:167:42: replace * with + in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:167:42: replace * with / in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:168:46: replace - with + in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:168:46: replace - with / in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:168:72: replace - with + in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:168:72: replace - with / in wait_timeout_ms
crates/pr-platform-win/src/host/message_loop.rs:176:9: replace <impl Drop for RunningGuard<'_>>::drop with ()
crates/pr-platform-win/src/host/message_loop.rs:195:9: replace <impl fmt::Debug for MessageLoop>::fmt -> fmt::Result with Ok(Default::default())
crates/pr-platform-win/src/host/message_loop.rs:207:9: replace MessageLoop::with_os -> Result<Self, HostError> with Ok(Default::default())
crates/pr-platform-win/src/host/message_loop.rs:223:9: replace MessageLoop::wait_count -> i64 with 0
crates/pr-platform-win/src/host/message_loop.rs:223:9: replace MessageLoop::wait_count -> i64 with 1
crates/pr-platform-win/src/host/message_loop.rs:223:9: replace MessageLoop::wait_count -> i64 with -1
crates/pr-platform-win/src/host/message_loop.rs:228:9: replace MessageLoop::pass_count -> i64 with 0
crates/pr-platform-win/src/host/message_loop.rs:228:9: replace MessageLoop::pass_count -> i64 with 1
crates/pr-platform-win/src/host/message_loop.rs:228:9: replace MessageLoop::pass_count -> i64 with -1
crates/pr-platform-win/src/host/message_loop.rs:233:9: replace MessageLoop::message_filter_count -> usize with 0
crates/pr-platform-win/src/host/message_loop.rs:233:9: replace MessageLoop::message_filter_count -> usize with 1
crates/pr-platform-win/src/host/message_loop.rs:246:9: replace MessageLoop::remove_frame_clock with ()
crates/pr-platform-win/src/host/message_loop.rs:259:9: replace MessageLoop::add_message_filter -> MessageFilterId with Default::default()
crates/pr-platform-win/src/host/message_loop.rs:267:9: replace MessageLoop::remove_message_filter with ()
crates/pr-platform-win/src/host/message_loop.rs:271:47: replace == with != in MessageLoop::remove_message_filter
crates/pr-platform-win/src/host/message_loop.rs:280:9: replace MessageLoop::quit with ()
crates/pr-platform-win/src/host/message_loop.rs:285:9: replace MessageLoop::quit_with with ()
crates/pr-platform-win/src/host/message_loop.rs:290:9: replace MessageLoop::is_message_pending -> bool with true
crates/pr-platform-win/src/host/message_loop.rs:290:9: replace MessageLoop::is_message_pending -> bool with false
crates/pr-platform-win/src/host/message_loop.rs:297:9: replace MessageLoop::run -> Result<i32, HostError> with Ok(0)
crates/pr-platform-win/src/host/message_loop.rs:297:9: replace MessageLoop::run -> Result<i32, HostError> with Ok(1)
crates/pr-platform-win/src/host/message_loop.rs:297:9: replace MessageLoop::run -> Result<i32, HostError> with Ok(-1)
crates/pr-platform-win/src/host/message_loop.rs:325:9: replace MessageLoop::run_nested -> Result<bool, HostError> with Ok(true)
crates/pr-platform-win/src/host/message_loop.rs:325:9: replace MessageLoop::run_nested -> Result<bool, HostError> with Ok(false)
crates/pr-platform-win/src/host/message_loop.rs:326:15: delete ! in MessageLoop::run_nested
crates/pr-platform-win/src/host/message_loop.rs:371:28: replace == with != in MessageLoop::pump_messages
crates/pr-platform-win/src/host/message_loop.rs:381:23: delete ! in MessageLoop::pump_messages
crates/pr-platform-win/src/host/message_loop.rs:395:9: replace MessageLoop::filtered -> bool with true
crates/pr-platform-win/src/host/message_loop.rs:395:9: replace MessageLoop::filtered -> bool with false
crates/pr-platform-win/src/host/message_loop.rs:442:9: replace MessageLoop::wait -> Result<(), HostError> with Ok(())
crates/pr-platform-win/src/host/message_loop.rs:447:16: delete ! in MessageLoop::wait
crates/pr-platform-win/src/host/message_loop.rs:452:28: replace && with || in MessageLoop::wait
crates/pr-platform-win/src/host/message_loop.rs:452:23: replace != with == in MessageLoop::wait
crates/pr-platform-win/src/host/message_loop.rs:452:45: replace < with == in MessageLoop::wait
crates/pr-platform-win/src/host/message_loop.rs:452:45: replace < with > in MessageLoop::wait
crates/pr-platform-win/src/host/message_loop.rs:452:45: replace < with <= in MessageLoop::wait
crates/pr-platform-win/src/host/message_loop.rs:476:5: replace same_clock -> bool with true
crates/pr-platform-win/src/host/message_loop.rs:476:5: replace same_clock -> bool with false
crates/pr-platform-win/src/host/swap_chain_clock.rs:58:9: replace SwapChainPolicy::period -> i64 with 0
crates/pr-platform-win/src/host/swap_chain_clock.rs:58:9: replace SwapChainPolicy::period -> i64 with 1
crates/pr-platform-win/src/host/swap_chain_clock.rs:58:9: replace SwapChainPolicy::period -> i64 with -1
crates/pr-platform-win/src/host/swap_chain_clock.rs:63:9: replace SwapChainPolicy::last_tick -> i64 with 0
crates/pr-platform-win/src/host/swap_chain_clock.rs:63:9: replace SwapChainPolicy::last_tick -> i64 with 1
crates/pr-platform-win/src/host/swap_chain_clock.rs:63:9: replace SwapChainPolicy::last_tick -> i64 with -1
crates/pr-platform-win/src/host/swap_chain_clock.rs:68:9: replace SwapChainPolicy::deferred -> bool with true
crates/pr-platform-win/src/host/swap_chain_clock.rs:68:9: replace SwapChainPolicy::deferred -> bool with false
crates/pr-platform-win/src/host/swap_chain_clock.rs:74:9: replace <impl ClockPolicy for SwapChainPolicy>::wait_handle -> isize with 0
crates/pr-platform-win/src/host/swap_chain_clock.rs:74:9: replace <impl ClockPolicy for SwapChainPolicy>::wait_handle -> isize with 1
crates/pr-platform-win/src/host/swap_chain_clock.rs:74:9: replace <impl ClockPolicy for SwapChainPolicy>::wait_handle -> isize with -1
crates/pr-platform-win/src/host/swap_chain_clock.rs:80:9: replace <impl ClockPolicy for SwapChainPolicy>::deadline_timestamp -> i64 with 0
crates/pr-platform-win/src/host/swap_chain_clock.rs:80:9: replace <impl ClockPolicy for SwapChainPolicy>::deadline_timestamp -> i64 with 1
crates/pr-platform-win/src/host/swap_chain_clock.rs:80:9: replace <impl ClockPolicy for SwapChainPolicy>::deadline_timestamp -> i64 with -1
crates/pr-platform-win/src/host/swap_chain_clock.rs:80:12: delete ! in <impl ClockPolicy for SwapChainPolicy>::deadline_timestamp
crates/pr-platform-win/src/host/swap_chain_clock.rs:96:9: replace <impl ClockPolicy for SwapChainPolicy>::should_tick -> Result<Option<i64>, HostError> with Ok(None)
crates/pr-platform-win/src/host/swap_chain_clock.rs:96:9: replace <impl ClockPolicy for SwapChainPolicy>::should_tick -> Result<Option<i64>, HostError> with Ok(Some(0))
crates/pr-platform-win/src/host/swap_chain_clock.rs:96:9: replace <impl ClockPolicy for SwapChainPolicy>::should_tick -> Result<Option<i64>, HostError> with Ok(Some(1))
crates/pr-platform-win/src/host/swap_chain_clock.rs:96:9: replace <impl ClockPolicy for SwapChainPolicy>::should_tick -> Result<Option<i64>, HostError> with Ok(Some(-1))
crates/pr-platform-win/src/host/swap_chain_clock.rs:98:36: replace && with || in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:98:31: replace != with == in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:98:72: replace < with == in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:98:72: replace < with > in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:98:72: replace < with <= in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:98:86: replace / with % in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:98:86: replace / with * in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:102:23: replace < with == in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:102:23: replace < with > in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:102:23: replace < with <= in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:106:13: replace && with || in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:106:94: replace > with == in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:106:94: replace > with < in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:106:94: replace > with >= in <impl ClockPolicy for SwapChainPolicy>::should_tick
crates/pr-platform-win/src/host/swap_chain_clock.rs:118:9: replace <impl ClockPolicy for SwapChainPolicy>::on_armed_changed -> Result<(), HostError> with Ok(())
crates/pr-platform-win/src/host/swap_chain_clock.rs:136:9: replace SwapChainFrameClock::create -> Result<Self, HostError> with Ok(Default::default())
crates/pr-platform-win/src/host/swap_chain_clock.rs:147:9: replace SwapChainFrameClock::with_seams -> Result<Self, HostError> with Ok(Default::default())
crates/pr-platform-win/src/host/shell_window/messages.rs:28:5: replace aligned_pointer -> Option<*mut T> with None
crates/pr-platform-win/src/host/shell_window/messages.rs:28:5: replace aligned_pointer -> Option<*mut T> with Some(Default::default())
crates/pr-platform-win/src/host/shell_window/messages.rs:28:18: replace && with || in aligned_pointer
crates/pr-platform-win/src/host/shell_window/messages.rs:28:13: replace != with == in aligned_pointer
crates/pr-platform-win/src/host/shell_window/messages.rs:28:55: replace == with != in aligned_pointer
crates/pr-platform-win/src/host/shell_window/messages.rs:28:37: replace % with / in aligned_pointer
crates/pr-platform-win/src/host/shell_window/messages.rs:28:37: replace % with + in aligned_pointer
crates/pr-platform-win/src/host/shell_window/messages.rs:35:9: replace ShellWindowInner::process_message -> Lresult with Default::default()
crates/pr-platform-win/src/host/shell_window/messages.rs:36:13: delete match arm WM_NCCREATE in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:37:13: delete match arm WM_CREATE in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:38:13: delete match arm WM_DESTROY in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:43:13: delete match arm WM_NCDESTROY in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:46:13: delete match arm WM_DPICHANGED in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:75:13: delete match arm WM_SIZE in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:79:13: delete match arm WM_DPICHANGED in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:80:13: delete match arm WM_GETMINMAXINFO in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:98:13: delete match arm WM_ERASEBKGND in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:99:13: delete match arm WM_PAINT in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:93:29: replace match guard low_word(lparam) == HTCLIENT with true in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:93:29: replace match guard low_word(lparam) == HTCLIENT with false in ShellWindowInner::process_message
crates/pr-platform-win/src/host/shell_window/messages.rs:93:46: replace == with != in ShellWindowInner::process_message
```

## PARITY and handoff

Only the three assigned host rows were edited, row by row. All stay Partial:
messages has 21 current mutants not run; message-loop has 69 not run here and
unexhausted raw OS/native integration; the combined swap-chain/contracts/icons
row retains its other native/resource gaps. Historical timeouts are not
relabelled as caught. No production bug was exposed or production code changed.

One CX-HOSTREST line was added to `task_on_progress.md`; two older HOSTRERUN
status lines were moved verbatim into the existing status archive to keep the
root file under approximately 8 KB. No Git add/commit/push was performed.

Final verification: killers 7/7 pass again after documentation updates; fmt and Clippy pass as above; `python -I tools/dev/doc-links.py --quiet` reports zero errors. `git diff --check` passes. The three modified PARITY rows and the two archived status lines were independently checked against HEAD; the archived lines match the original bytes verbatim.
