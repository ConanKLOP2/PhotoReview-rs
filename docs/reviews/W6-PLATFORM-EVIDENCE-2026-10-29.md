# Wave 6 platform rows: native and mutation evidence (2026-10-29, Machine A, claim w6plat-claude)

Status: evidence report. Branch `feat/claude-w6plat` from master `1b8abe63`; C# baseline `7eaee2a2` read with `git show` only.
Covers the `docs/PARITY.md` rows P-01 (W6-01a), W6-01b, P-03 (W6-02a..e), P-04 (W6-03a/e), P-05 (W6-03b..e) and P-11 (W6-04a/b).
Rules followed: `AGENTS.md` (Recycle Bin rule, "Mutation testing and parallel agents"), one native test process at a time, one `cargo-mutants` process at a time with `-j 1`, own `CARGO_TARGET_DIR` outside the worktree, no new Cargo dependency (nothing for `docs/PROPOSED-DECISIONS.md`), no Codex.

## 1. Recycle Bin safety: what ran and what was skipped

No test or mutant in this report deletes from, restores to, sweeps or empties the real Recycle Bin.

- `grep` of `crates/pr-platform-win/{src,tests}` for `SHFileOperation`, `IFileOperation`, `SHEmptyRecycleBin`, `undelete`, `WindowsRecycleBin::new`: the Shell calls live only in `src/recycle_bin.rs` (`native_shell_recycle`, behind the production constructor) and `src/recycle_bin_shell.rs` (`undelete` verb). No test calls either: every recycle test builds `WindowsRecycleBin::with_adapters(...)` with fake adapters and fake restore sessions.
- **Run** (fakes only): `recycle_bin_contract` 1, `recycle_bin_facade` 22, `recycle_bin_settings_capacity` 26, `recycle_eligibility` 17, `recycle_restore_selection` 17, and the in-source `recycle_restore::tests` and `recycle_bin::delete_semantics_tests` of `--lib` (62 passed, 1 ignored unrelated).
- **Skipped on purpose**: any native test of the real bin (there are none in the repo), any mutation of `src/recycle_bin.rs` (the facade holding `native_shell_recycle`) and `src/recycle_bin_shell.rs` (Shell COM glue). The real-bin path keeps only the owner-approved evidence in [EXE-DELETE-REAL-BIN-2026-10-29.md](EXE-DELETE-REAL-BIN-2026-10-29.md).
- Read-only OS queries do run in two new killers (`GetVolumePathName` / `GetDriveType` through `can_recycle` and `is_permanent_delete_allowed`, and the registry-free parsing); they never touch the bin.

## 2. Ordinary and native test runs (this branch, debug)

| Binary | Result |
|---|---|
| `explorer_com_interop` | 14 passed |
| `explorer_window_selector` | 18 passed |
| `forward_client` | 17 passed |
| `display_clock` | 14 passed |
| `--lib` filter `recycle_ explorer forward instance vblank display` | 62 passed, 1 ignored |
| pr-core `instance_forwarding` / `vblank_estimator` | 50 / 8 passed |
| `forward_pipe_native --include-ignored` | 13 passed (after the test fix below; 12 + 1 failed before) |
| `forward_server_native --include-ignored` | 18 passed, 3 repeats stable (17 + 1 failed before) |
| `forward_client_native --include-ignored` | 2 passed |
| `vblank_driver_native --include-ignored` | 6 passed (includes the DWM smoke) |
| `explorer_order_provider --include-ignored` | 2 passed (real Explorer STA pump starts, answers, joins on drop) |
| new `w6_platform_native --include-ignored` | 3 passed (real mutex, real pipe round trip, real DWM + D3DKMT chain) |
| new `w6_mutation_killers` | 37 passed |

DLLs staged with `tools/dev/stage-native.ps1` (libraw and turbojpeg verified) before the native runs.

### Native failures: root cause and decision (both are test defects, production unchanged)

1. `forward_pipe_native::native::live_pipe_has_single_user_dacl` failed with `left: 2032127, right: 268435456`. The test compared the ACE mask of a LIVE pipe with `GENERIC_ALL` (`0x10000000`). The kernel maps generic rights to the object's specific rights when it creates the pipe, so `GetSecurityInfo` returns `FILE_ALL_ACCESS` (`0x1F01FF` = 2032127). C# `LivePipe_HasSingleUserDacl` (`InstanceForwardPipeTests.cs` at `7eaee2a2`) asserts only that every rule's identity is the current user. Fix: the helper takes the expected mask (`GENERIC_ALL` for the constructed descriptor, `FILE_ALL_ACCESS` for the live pipe); single ACE, protected DACL, flags 0 and the SID equality are still asserted.
2. `forward_server_native::native::dispose_stops_native_accept_loop` failed 4 of 4 times at "server pipe was not created". `InstanceForwardServer::start` returns before the listener thread has called `CreateNamedPipeW`, and `WaitNamedPipeW` fails at once while the name does not exist, so the test raced the listener. Production is correct (the real client tests pass against the real server). Fix: the test waits for readiness with a bounded loop (5 s deadline, no fixed sleep), then also asserts what C# `Dispose_StopsListening` asserts, that after `dispose` no instance of the pipe remains.

## 3. Mutation runs

`cargo mutants -p pr-platform-win --baseline skip --timeout 90 -j 1 --file '**/<file>.rs' [-F ...] --cargo-arg=--test=<binary>`, target dir `.claude/worktrees/_mut-target-w6`. A hang (timeout) is counted as detected, as in `docs/reviews/W3-01-JOURNAL-MUTATION.md`. "Round 1" = existing tests only; "round 2" = existing tests plus `w6_mutation_killers` (and the native binaries with `-- --include-ignored` where stated).

| File (rows) | Round 1 tested: caught / missed / unviable / timeouts | Round 2 (same mutants affected) | Survivors left |
|---|---|---|---|
| `explorer_window_selector.rs` (W6-01b) | 30: 23 / 3 / 2 / 2 | 10 selected (`select`, `try_canonicalize_location`, `Display`): 9 caught, 1 unviable | none (the 2 hangs are `percent_decode` loop counters) |
| `forward_pipe.rs` (P-05) | 13: 2 / 9 / 2 / 0 | 13: 10 / 1 / 2 | `PipeSecurityDescriptor::drop` -> `()` (a `LocalFree` leak is not observable) |
| `recycle_eligibility.rs` (P-03) | 31: 24 / 7 / 0 / 0 | 31: 28 / 3 / 0 | 3 equivalent: `Component::CurDir` / `ParentDir` arms and `||` -> `&&` in `normalized_full_path`; Windows `std::path::absolute` already resolves `.` and `..` and fails on blank/NUL input, so the arms are dead (the killers pass under these mutants) |
| `recycle_candidate.rs` (P-03) | 34: 30 / 3 / 0 / 1 | 12 selected: 10 / 1 / 0 / 1 | `name_matches`: `dot > 0` -> `>= 0` is equivalent (the display name is non-empty, so it never equals the empty stem); the hang is a loop counter |
| `recycle_bin_capacity.rs` (P-03) | 33: 24 / 7 / 2 / 0 | 33: 30 / 1 / 2, then the `evaluate_core` mutants alone: 29 / 0 | none |
| `recycle_restore.rs` (P-03) | 26: 25 / 0 / 1 / 0 (`--lib`, `recycle_restore_selection`, `recycle_bin_facade`) | not needed | none |
| `recycle_bin_settings.rs` (P-03) | 96: 53 / 40 / 3 / 0 (time-box reached 21 min, all 96 ran) | `extract_guid` and `get_user_quota_bytes` alone: 28 tested, 27 caught | 35 left: 34 OS adapters (`read_registry_value` and its handle `Drop`, `query_volume_name`, `query_total_bytes`, `query_drive_type` and its mapping arms, `query_mount_point`, `absolute_normalized`, `Debug`, `with_reader`) plus 1 equivalent (`extract_guid:142`, the first byte is always `{` by construction). These need a real registry or volume; not killed |
| `instance_keys.rs` (P-04, P-05) | 34: 14 / 19 / 0 / 1 | 34: 22 / 11 / 0 / 1 | 11 equivalent or environmental: `current_session_id` / `read_session_id` -> `1` (this desktop is session 1; killable only in another session), the `#[cfg(not(windows))]` bodies (172, 190), dead `CurDir` / `ParentDir` arms, `Token::drop` leak, two defensive size guards (`required < size_of::<TOKEN_USER>`, `|| wide_sid null`) |
| `instance_scope.rs` (P-04, P-05) | 67 (`--lib`): 54 / 0 / 12 / 1 | not needed | none (the hang is `release_except` `delete !`) |
| `forward_server.rs` (P-05; `handle_request`, `run_server`, `retry_delay`) | 16: 9 / 7 / 0 / 0 | 17 (incl. `begin_io`; killers + `forward_server_native --include-ignored` + `w6_platform_native`): 17 caught | none; the other mutants (native `NativeForwardPipe` / factory I/O) were not run |
| `forward_client.rs` (P-05; `send_locked`, `transfer`) | 12: 7 / 4 / 1 / 0 | 18 (killers + `forward_client` + `forward_client_native` + `w6_platform_native`): 17 caught, 1 unviable | none; the other native-transport mutants (`connect`, `write_all`, `read_reply`, `sid_bytes`) were not run |
| `vblank_driver.rs` (P-11; `device_name_from`, `map_dwm_timing`, `try_open`, `dwm_timing_impl`) | 15: 11 / 3 / 1 / 0 | 13 (with `vblank_driver_native` + `w6_platform_native`, `--include-ignored`): 12 caught, 1 unviable | none; the other 40 mutants (trait glue, `Drop`, `with_optional_*`) were not run |
| `display_clock.rs` (P-11) | 66: 38 / 22 / 6 / 0 (time-box 18 min, all 66 ran) | not killed | 22: `qpc_frequency` / `qpc_timestamp` wrappers (both cfg variants, 15), `Debug`, `instance()`, `dwm_timing`, `set_clock_worker_priority`, `retire_worker`, `reap_finished_workers`, `worker_main:308` `!=` -> `==` |

All other mutants of the files above not named in a "selected" column keep their round 1 verdict.

### Killers added (`crates/pr-platform-win/tests/w6_mutation_killers.rs`, fakes and read-only queries only)

Each test carries a comment naming the mutant. Summary of the survivors they kill: counter in `select` (`+=` -> `*=`), `Display` of `SelectionReadError`, rooted file URL without a drive letter; `current_pipe_name`, `PipeSecurityDescriptor::for_sid` validation (4 mutants), `as_ptr`, `current_security_descriptor`; `can_recycle` / `is_permanent_delete_allowed` (4 mutants); blank/NUL handling; `trim_ending_separator` (2 mutants); `RecycleBinCapacityGuard::MEBIBYTE`, empty file, zero quota (2 branches), percent capacity, MB cap (7 mutants); `current_user_sid`, `current_session_id` -> `0`, `read_user_sid` (11 mutants); `retry_delay` doubling, `run_server` cancel/retry (3 mutants), `handle_request` complete-request read; client `send_locked` cancellation (3 mutants); `extract_guid` (2 of 3), `get_user_quota_bytes` (3). Every killed mutant was re-run with the killers present (round 2 above); none was only assumed.

### New native tests (`crates/pr-platform-win/tests/w6_platform_native.rs`, all `#[ignore = "native: ..."]`)

- `real_named_mutex_gives_one_owner_per_folder_and_frees_on_dispose`: real `CreateMutexW` through `InstanceScope` with a unique prefix: first claim `Acquired`, repeat `AlreadyHeld`, a second scope gets `OwnedElsewhere` (also for a spelling variant of the folder), another folder is independent, dispose frees the name. Two scopes in one process stand for two launches (the kernel object semantics are the same); a second OS process was not started.
- `real_pipe_forwards_a_path_to_the_owner_and_reports_no_instance_after_dispose`: real DACL-protected pipe, real client: `Delivered` and the owner callback receives the path, a missing path is `Rejected`, after dispose the client reports `NoInstance`.
- `real_dwm_timing_and_display_adapter_chain_succeeds_on_the_primary_monitor`: `DwmGetCompositionTimingInfo` returns a positive grid and the D3DKMT adapter opens for the primary monitor (also kills the three FFI-struct field mutants of `vblank_driver.rs`).

## 4. Per-row verdict

| Row | Old -> new | Reason |
|---|---|---|
| P-01 (W6-01a) | Partial -> Partial | 14 fake-backed tests pass and the real Explorer STA pump starts, answers and joins (`explorer_order_provider` native). COM glue is not mutated by policy; a live Explorer window with a matching folder was not driven, so the vtable/`LocationURL` path against a real window is unproven. |
| W6-01b | Partial -> **Done** | 18 C#-named tests plus the full-file mutation run (30 mutants; 3 survivors killed by 3 added tests; 2 hangs counted as detected). |
| P-03 | Partial -> Partial | Policy mutation complete for eligibility, capacity, candidate, restore (124 mutants, 0 non-equivalent survivors). Open: 34 OS-adapter survivors in `recycle_bin_settings.rs`, `recycle_bin.rs` and `recycle_bin_shell.rs` not mutated (real-bin rule); real-bin behaviour proved only by the owner-approved exe pass. |
| P-04 | Partial -> **Done** | Real mutex ownership proved; `instance_scope.rs` 67/67 and `instance_keys.rs` complete with 11 documented equivalent or environmental survivors. Not run: two OS processes, a session other than 1. |
| P-05 | Partial -> Partial | Native pipe evidence recorded (DACL, `PIPE_REJECT_REMOTE_CLIENTS` mode, round trip, dispose, client current-user check) and pure protocol/loop mutation complete. Open: native I/O glue mutants not run (server `NativeForwardPipe` and factory, client `WindowsForwardTransport`), remote-client rejection and cross-host behaviour not exercised. |
| P-11 | Partial -> Partial | Native DWM timing and D3DKMT adapter open proved; mapping and device-name mutation complete. Open: 22 `display_clock.rs` survivors (QPC wrappers, worker cleanup) and 40 unrun glue mutants in `vblank_driver.rs`. |

## 5. Not run

- Any native test of the real Recycle Bin; mutation of `recycle_bin.rs`, `recycle_bin_shell.rs`.
- The mutants listed as "not run" in the table; `explorer_com_interop.rs`, `explorer_native_view.rs`, `forward_server.rs` / `forward_client.rs` native I/O glue.
- Cross-session and cross-host scenarios; a second OS process for the mutex.
- Full workspace test run (only the crate binaries above, `cargo check --workspace`, and clippy on `pr-platform-win --all-targets`).
