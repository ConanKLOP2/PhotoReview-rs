# Q11: PARITY close attempt for P-03, P-05, P-11 (2026-11-03/04)

Owner goal (2026-11-03): finish PARITY. Base: `origin/master` 84d2c151. Worktree `pc-platform`, branch `parity/platform`, crate `pr-platform-win`.
Result in one line: no row can be raised to Done in this run. The machine closed what it could (native runs, new killer tests, mutation of the
native glue); what is left needs a person, a second account or host, or an owner decision (listed per row, with steps).

Safety: no real Recycle Bin operation, no mutation of `recycle_bin.rs` / `recycle_bin_shell.rs`, the `#[ignore = "attended..."]` test of
`recycle_bin_facade` was NOT run (that binary was run without `--include-ignored`). Pipes used by new tests are private (unique name per process
and test). No window was opened. No production file was changed.

## How it was run

- Native tests: `cargo test -p pr-platform-win --locked --test <t> -- --include-ignored --test-threads=1` (the `native-tests.sh` wrapper runs
  every native binary of the crate including the attended Recycle Bin test, so the binaries were run one by one instead). Desktop:
  Windows 10 Pro 19045, `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`, data root outside the repo. The pinned DLLs of P-50 are not used by these binaries.
- Mutation: `cargo mutants -p pr-platform-win --baseline skip --timeout 120 -j 1 --file <f> -F '<regex>' --cargo-arg=--test=<binary>... --cargo-test-arg=--
  --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`, own `CARGO_TARGET_DIR` per run, `CARGO_BUILD_JOBS=2`, a mutant counts as caught
  only when a test failed (timeouts are NOT caught). `tools/dev/mutants.sh --native --tests` was tried first and abandoned after about 25 minutes
  with 9 mutants done: its `--cargo-test-arg=--test` form makes every mutant rebuild all 150 test binaries of the crate (about 90 s per mutant);
  the form with `--cargo-arg=--test=<binary>` (as in CL-W6RERUN) needs 15-45 s. Those 9 results were discarded, the work was redone with the faster form.
- Mutation time spent: about 80 minutes wall clock over three parallel processes (budget was about 60 minutes of mutation per cluster).
- `cargo fmt -p` did not fail with os error 206 on this machine (`pre-pr.sh` reported `ok fmt -p pr-platform-win`); `rustfmt --edition 2024 --check`
  on the new file also passes.
- Checks: `cargo clippy -p pr-platform-win --locked --test q11_platform_killers -- -D warnings` clean; `bash tools/dev/pre-pr.sh` PASSED (conflict
  markers, BOM, fmt, clippy of the crate, unsafe-audit, doc-links); NOT RUN: workspace-wide clippy, `cargo test --workspace`, CI.

## New tests (all tests, no production change)

`crates/pr-platform-win/tests/q11_platform_killers.rs`: 14 tests, 11 native `#[ignore]` (private pipes, handle count, real primary monitor), all pass
with `--include-ignored --test-threads=1`. Names: `forward_pipe_error_display_names_each_failure`,
`forward_pipe_client_debug_shows_name_and_foreground_flag_without_touching_the_pipe`,
`instance_forward_server_debug_shows_name_and_disposed_flag_without_starting`, `native::transport_debug_names_the_type`,
`native::unconnected_transport_is_not_a_current_user_server_and_has_no_foreground_target`,
`native::same_user_private_server_is_recognised_as_current_user`,
`native::connect_to_a_pipe_that_refuses_this_client_fails_at_once_instead_of_polling_until_the_deadline`,
`native::reply_of_exactly_the_limit_is_returned_without_reading_one_byte_more`,
`native::disconnect_closes_the_client_end_so_the_server_sees_a_hangup`, `native::dropping_a_connected_transport_closes_its_client_end`,
`native::dropping_servers_releases_their_stop_events`,
`native::vblank_chain_on_the_primary_monitor_blocks_for_real_vblanks_and_close_releases_the_adapter` (+ 2 inherited `module_cache_tests` from the
`#[path]` include of `vblank_driver.rs`). The first run of the connect test was wrong (a path under a missing directory gives ERROR_FILE_NOT_FOUND, which
is retried by design); it was fixed to use an inbound-only private pipe (ERROR_ACCESS_DENIED). The first run of the hang-up helper blocked forever
in `GetOverlappedResult` after a read that failed synchronously; fixed by treating a non-pending read error as a hang-up.

Finding about this desktop (not a defect): `D3DKMTWaitForVerticalBlankEvent` on a real adapter handle answers `0xC01E0006` at once instead of blocking
until a vblank. The D3DKMT wait therefore never blocked on this machine; the test accepts "0 after at least 4 ms for 4 waits, or a negative graphics
status" so it also holds on a physical console. The attended measurement of a real vblank grid remains a person's job (P-11 below).

## P-03 (Recycle Bin): stays Partial

Gaps before (from the row): `recycle_bin.rs` and `recycle_bin_shell.rs` not mutated; Shell/COM adapters and attended runs; C2 `PostDeleteItem` sink
never confirmed against the real Shell (attended test `native_attended_recycle_of_own_temp_file_reports_moved_to_recycle_bin` not run; risk: `Unknown`
on every recycle makes the send fail, P-62 fallback B); no native `entries()` / restore test; open review findings T1 (match by DateModified, not deletion
time), T3 (`recycle()` no timeout, error dialog without owner), T6 (STA thread does not pump), T4 partly (localized verb / `QueryContextMenu` path not wired).

What ran in this run (all fakes or read-only OS queries, no real bin): `recycle_bin_contract` 1, `recycle_bin_facade` 34 passed + 1 ignored (the attended
test, deliberately NOT run), `recycle_bin_settings_capacity` 31, `recycle_eligibility` 17, `recycle_restore_selection` 18, `recycle_restore_timeout` 3,
`recycle_delete_sink` 8, `recycle_bin_readonly_delete` 2, `recycle_final_path` 7 (incl. the native junction test in a TempRoot), `w2_recyclesettings_killers` 3,
`w2_recyclesettings2_killers` 4, plus the shared killers `w6_mutation_killers` 37, `w6_p01_killers` 23, `w6_glue_killers` 8, `w6_rerun_killers` 8: all pass.
Mutation: none run for P-03 in this run. NOT RUN: `recycle_bin.rs` 62 mutants and `recycle_bin_shell.rs` 29 mutants (forbidden by the Recycle Bin rule: the
mutated code deletes from the real bin), `recycle_bin_settings.rs` 109 (rerun not repeated; earlier counts in the row), `recycle_restore.rs` 34 and
`recycle_candidate.rs` 34 (recorded complete in the row, not repeated).

Gaps after: unchanged. Nothing more a machine may do without touching the user's real bin.

Proposed status text: keep `Partial`; add only "Q11 2026-11-04: fake and read-only binaries re-run green (listed in `docs/reviews/Q11-PARITY-CLOSE-2026-11-03.md`); no real-bin operation; gaps unchanged".

Human needed (exact steps):
1. Attended C2 run (decides the P-62 risk): on a desktop, with a person watching, run
   `cargo test -p pr-platform-win --test recycle_bin_facade -- --include-ignored --test-threads=1 native_attended_recycle_of_own_temp_file_reports_moved_to_recycle_bin`.
   It sends ONE temp file created by the test to the real bin. Record whether the Shell called `PostDeleteItem` (outcome `MovedToRecycleBin`) or `Unknown`; if
   `Unknown`, the owner chooses P-62 fallback B.
2. Own-item native `entries()` / restore test: write and run (attended) a test that recycles one own temp file, finds it through `entries()`, restores it with the
   `undelete` verb, and checks the content and write time; it must never enumerate-and-delete other items.
3. Owner decisions/code for T1 (needs the journal's recycle time through `try_restore`), T3 and T6; T4 localized-label path.
4. Only after 1 and 2: a time-boxed mutation of `recycle_bin_shell.rs` / `recycle_bin.rs` is still forbidden by AGENTS.md ("never run code that deletes from, sweeps or empties the user's real Recycle Bin, including mutation checks"). The owner must either keep these two files out of the mutation promotion condition or approve a seam so that the mutated code only sees a fake Shell.

## P-05 (named pipes): stays Partial

Gaps before: native I/O glue mutants of `NativeForwardPipe` / `WindowsForwardTransport` not mutated; remote-client rejection and cross-host forwarding not
exercised; `LocalFree` leak in the descriptor (closed by CL-P01).

Native run (this run, all with `--include-ignored --test-threads=1`): `forward_pipe_native` 13, `forward_server_native` 19, `forward_client_native` 3,
`w6_platform_native` 5 (real mutex, real pipe forward, DWM chain), `w6_p01_killers` 23 (incl. the private-bytes descriptor-leak probe), `w6_rerun_killers` 8,
`q11_platform_killers` 14: all pass. The sync fake binary `forward_client` was not rerun here (inline fakes, no native part).

Mutation (new, glue only, file `forward_server.rs` filter `native_pipe::` and `forward_client.rs` filter `native::` + the two `Debug` impls):

| file | tested | caught | missed | unviable | timeout | not run |
|---|---:|---:|---:|---:|---:|---:|
| `forward_client.rs` (71 matched of 85) | 71 | 54 | 15 | 1 | 1 | the 14 non-native/non-Debug `send_locked` mutants (caught in CL-W6RERUN) |
| `forward_server.rs` `native_pipe::` (69 of 103) | 69 | 43 | 23 | 3 | 0 | the 34 pure-logic mutants of `run_server` / `handle_request` / `retry_delay` (caught in CL-W6RERUN except the equivalent `<=`) |

Client missed (15) and timeout (1), classified:
- real gap, killed by this run's tests: Debug x2 (:71, :207), retry guard (:389), `server_is_current_user -> true` (:409), `allow_server_foreground -> Ok(())` (:428),
  `read_reply` `<` -> `<=` (:507), `disconnect -> ()` (:518), `Drop -> ()` (:528): all CAUGHT in the final runs.
- equivalent: `connect` flags `GENERIC_READ | GENERIC_WRITE` `|` -> `^` (:376; disjoint bits give the same value); `connect` `== 0x80070002` -> `!=` (:400; only decides whether
  the missing-pipe poll sleeps, CPU behaviour only); `write_all` `offset == 0` guards and `== -> !=` (:475 x3, :482): unreachable difference, because `transfer`
  already maps a write timeout to `WriteFailed` and an overlapped write on a byte pipe is never partial, so `offset` is 0 on every error path.
- not killable in one account: `sid_bytes` / `process_user_sid` constants (:212 x3, :215 x2, :243 x3) and `allow_server_foreground` `pid == 0` -> `!=` (:436). `Some(vec![..])` on
  both sides compares equal, so only a server running as ANOTHER user tells them apart (the real compare is also what the row calls the DACL/identity check).
  Needs a second account (human step 1 below). `<` -> `<=`/`==` at :215 compares a byte count to `size_of::<TOKEN_USER>()` (16) and is equivalent in practice
  (the probe returns far more than 16 bytes).
- timeout (counts as NOT caught): `connect` `FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED` `|` -> `&` (:380) drops the overlapped flag; the tests then block in a
  synchronous read. It is detected only by the 60 s cargo-mutants timeout, so it stays open as "timeout, not caught". A worker-thread guard did not help because the stuck thread blocks
  process exit.

Server missed (23), classified (no killer written; none is a proven equivalent except the last):
- redundant-by-design (cancellation is done twice, by the stop event and by `CancelIoEx`): `activate -> ()` (:426), `deactivate -> ()` (:430), `deactivate` `==` -> `!=` (:431),
  `cancel` `delete !` (:493), `is_canceled -> false` (:500). Not proven equivalent by experiment; a test would need to observe the second mechanism alone.
- needs a failing `CreateNamedPipeW` during the retry: `wait_retry -> true/false`, `&&` -> `||`, `==` -> `!=`, `delete !` (:504 x2, :507 x3).
- needs a client that hangs up or errors at a precise moment, observable only through the log text: `wait_operation` / `read` BROKEN_PIPE arms (:414 x3, :584 x3), `write_all`
  IO_PENDING guards (:603, :611), `accept` guards (:557, :558; ERROR_PIPE_CONNECTED is a race between `CreateNamedPipeW` and `ConnectNamedPipe`).
- `wait_reply_delivered -> Ok(())` (:621) and `== -> !=` (:623): the existing test `rejected_request_reaches_the_client_as_rejected_not_unknown` is in the test set and
  does not fail, as the row already admits (the race is not deterministic). Real gap.
- equivalent: `hresult_from_win32` `|` -> `^` (:634), disjoint bits (`0x8007_0000 | (code & 0xFFFF)`).
`server_pipe_mode_rejects_remote_clients` (mode flag) stays the only remote-client check; a real remote client needs a second host.

Gaps after: server glue 23 missed (list above), client sid/identity 11 missed + 1 timeout, remote-client and cross-host not exercised.

Proposed status text: keep `Partial`; append "Q11 2026-11-04: native pipe binaries re-run green (13/19/3/5/23/8/14); native glue mutation `forward_client.rs` 71 tested = 54 caught, 15 missed (7 equivalent or unreachable, 8 need a second account), 1 timeout, 1 unviable; `forward_server.rs native_pipe::` 69 tested = 43 caught, 23 missed (1 equivalent, 22 open, listed in Q11), 3 unviable; killers `q11_platform_killers`".

Human needed:
1. Second Windows account: start `PhotoReviewRs.exe` (or a test server) as user B holding the pipe, run the client as user A, and check the client refuses (`server_is_current_user` false). This is the only check that kills the sid/identity mutants.
2. Cross-host: second machine on the same network; from it try `\\<host>\pipe\PhotoReviewRs...` against a running instance and confirm access is refused (`PIPE_REJECT_REMOTE_CLIENTS`). Record the result.
3. Owner decision: accept the 22 open server-glue mutants as "glue, tested by native behaviour not by mutation", or ask for a deterministic reply-delivery test (needs a fake client that delays reading the reply).

## P-11 (display clock / vblank / DWM): stays Partial

Gaps before: remaining `vblank_driver.rs` native glue (28 of 58 mutants tested in CL-W6RERUN); attended measurements; inactive non-Windows bodies.

Native run (this run): `display_clock` 16, `vblank_driver_native` 8, `w6_platform_native` 5 (incl. `real_dwm_timing_and_display_adapter_chain_succeeds_on_the_primary_monitor`),
`w6_glue_killers` 8, `w6_rerun_killers` 8 (incl. `try_open_returns_the_adapter_and_source_that_the_vblank_wait_accepts`), `w6_p01_killers` 23 (incl.
`static_dwm_timing_returns_a_positive_grid`), `q11_platform_killers` 14: all pass.

Mutation (whole `vblank_driver.rs`, 58 mutants, all run): 39 caught, 11 missed, 8 unviable, 0 timeout. First pass 35 caught / 15 missed / 8 unviable; the new vblank test then killed
`wait_for_vblank -> Ok(0/1/-1)` (:164) and `close -> ()` (:175). The 11 missed are all in the inactive `#[cfg(not(windows))]` impl (:189-207: `monitor_from_window` x2, `try_open` x5,
`wait_for_vblank` x3, `dwm_timing` x1), which this target does not compile, so they cannot be observed (the same class CL-W6RERUN recorded). `display_clock.rs`: not rerun (66 mutants,
recorded complete in the row; 2 equivalent `qpc_frequency` and inactive cfg bodies).

Gaps after: attended measurements only (real vblank grid on a physical display; this desktop's D3DKMT wait returns a graphics failure status at once, see above), inactive cfg bodies.

Proposed status text: keep `Partial`; append "Q11 2026-11-04: `vblank_driver.rs` 58 of 58 tested = 39 caught, 11 missed (all inactive non-Windows bodies), 8 unviable; new native test `vblank_chain_on_the_primary_monitor_blocks_for_real_vblanks_and_close_releases_the_adapter`; the D3DKMT wait answers 0xC01E0006 at once on this desktop, so the attended measurement is still open".

Human needed:
1. Attended measurement on a physical monitor (not RDP/VM): run `cargo test -p pr-platform-win --test w6_platform_native -- --include-ignored --test-threads=1 real_dwm_timing_and_display_adapter_chain_succeeds_on_the_primary_monitor`
   and `--test q11_platform_killers -- --include-ignored --test-threads=1 vblank_chain`; additionally run the display-clock benchmark/diagnostic of the exe on a 60 Hz and a high refresh monitor and record the measured
   grid versus the DWM grid (what the C# `WindowsDisplayClock` reports). If the vblank test sees status 0 there, the timing bound (at least 4 ms for 4 waits) is also checked.
2. Owner decision: the 11 inactive non-Windows bodies are either accepted as unobservable or removed from the mutation scope.

## Not done / not claimed

- No row promoted. No PARITY.md, BACKLOG, README, PROPOSED-DECISIONS, task_on_progress or SYNC-LEDGER edit.
- No mutation of `instance_scope.rs` (67), `display_clock.rs` (66), `forward_pipe.rs` (13) in this run; their results are in the rows (CL-P01, CL-W6RERUN).
- Not run: `cargo test --workspace`, workspace clippy, CI, `native-tests.sh` as a whole (it would run the attended Recycle Bin test).
- Mutation raw outputs are in `C:\Users\PC\Projects\mutq11a`, `mutq11b`, `mutq11c` (outside the repo, not committed).
