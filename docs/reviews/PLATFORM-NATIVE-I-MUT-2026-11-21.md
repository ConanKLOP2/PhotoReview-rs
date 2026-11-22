Status: evidence report (2026-11-21), run done, all 23 survivors decided (12 killed by new tests, 11 not killable: 8 not compiled on Windows, 2 unreachable Windows fallbacks, 1 equivalent); 20 timeouts are hangs. Native mutation lane I for `pr-platform-win`: whole-file run of `display_clock.rs`, `host/ui_dispatcher.rs` and `instance_scope.rs`. Measured on one machine (Windows 11, interactive session 1, owner present), `mutants.sh --native --jobs 1`, `MUT_TIMEOUT=600`, own target dir.

# `pr-platform-win` native lane I: `display_clock.rs`, `host/ui_dispatcher.rs`, `instance_scope.rs`

## Result

223 identities, 8.5 hours: **151 caught, 23 missed, 29 unviable, 20 timeout, 0 not run**.

Test set (`--include-ignored --test-threads=1`, preflight 549 tests, exit 0): the library unit tests (`--lib`, where `instance_scope.rs` keeps its tests) and the binaries `display_clock`, `host_swap_chain_clock`, `host_frame_clock`, `w6_glue_killers`, `w6_p01_killers`, `w6_platform_native`, `host_ui_dispatcher`, `host_ui_dispatcher_native`, `w8_host3_killers`, `w8_host_mutation_killers`, `w8_hostrerun2_killers`, `w8_hostrerun_killers`, `host_message_loop_native`, `nat_dispatch_killers`, `nat_surface_forwarder_killers`, `shell_viewing_viewport_controller`. cargo-mutants builds every test target of the crate (`cargo test --no-run`) but runs only these.

Skipped as the earlier lanes skip tests that need unavailable resources: the `win32_*` binaries, `render_keep_alive` (60 s wait for an event this machine does not deliver) and the `forward_{pipe,server}_native` binaries (their files `forward_*.rs` are a separate lane, not started).

## The 20 timeouts (hangs, detected)

Each hangs the test run for the full 600 s `MUT_TIMEOUT`; an endless loop or a lost wake-up cannot be told apart from a failing test by anything better than the hang, so they are counted as detected, not as survivors.

- `instance_scope.rs` 231:23 `!=` to `==` in `wait_for_releases`, 266:16 `delete !` in `release_except`.
- `host/ui_dispatcher.rs` (18): `from_current_thread` (239:9 `None`, 241:34), `is_shut_down` 256:9, `has_high_priority_work` 261:9, `run_high_priority_batch` 284:9, `on_wake_message` 317:9 and 322:61, `reject_wrong_thread` 360:9, `dequeue_high` 370:9, `try_enqueue` 375:9 (`true` and `false`), `request_wake` 390:9 and 390:12, `execute` 415:9, `UiScheduler::post` 467:9, `check_access` 479:9 and 479:37, `post_with_priority` 483:9.

`ui_dispatcher.rs` has no survivor at all.

## The 23 survivors

### Killed (12), by new tests inline in `instance_scope.rs`

| Survivor (line:col, mutant) | Killer |
|---|---|
| 51 `Display for InstanceScopeError -> Ok(Default::default())` | `blank_prefix_error_displays_its_message` |
| 72 `Debug for InstanceScope -> Ok(Default::default())` | `debug_output_names_the_scope` |
| 431 `Drop for InstanceScope::drop -> ()` | `dropping_the_scope_disposes_its_claims` (fake claims, no explicit dispose; with real claims the same effect comes from the `HeldClaim` drop chain, so only the fake can see it) |
| 487 `HeldClaim::dispose_claim -> Ok(())`, 502 `ScopeClaim::dispose -> Ok(())` | `held_claim_names_itself_stops_listening_and_frees_the_name_on_dispose`: the claim object stays alive after `dispose()`, so only the explicit close can free the mutex name (the old `w6_platform_native` test drops the claim through `InstanceScope::dispose`, whose drop chain closes the handle anyway) |
| 494 `mutex_name -> ""` and `"xyzzy"` | the same test (`claim.mutex_name() == mutex`) |
| 498 `stop_listening -> ()` | the same test: after `stop_listening` the real pipe answers `NoInstance` while a second `try_create` of the name still finds the mutex held |
| 538:48 `\|\|` to `&&`, 538:56 `==` to `!=` | `create_mutex_handles_foreign_objects_errors_and_closes_on_drop`: a named event owns the name, `CreateMutexW` fails with `ERROR_INVALID_HANDLE`, the result is `Ok(None)` |
| 538:25 `==` to `!=` | the same test, error branch: a name with a path inside the `Local\` namespace makes `CreateMutexW` fail with another code, which must be an `Err` (the mutant turns it into `Ok(None)`) |
| 572 `Drop for OwnedKernelHandle -> ()` | the same test: after `drop(first)` the name can be created again (the leaked handle keeps it, `ERROR_ALREADY_EXISTS`) |

### Not killable (11)

| Survivors | Verdict |
|---|---|
| `instance_scope.rs` 582:9 `create_mutex -> Ok(None)` and `-> Ok(Some(Default::default()))` | `#[cfg(not(windows))]` stub: not compiled on this machine (the mutant builds and the tests never see it). Equivalent for this build; the non-Windows stub only returns "instance scope requires Windows". |
| `display_clock.rs` 433:5 `qpc_frequency` (3 mutants), 438:5 `qpc_timestamp` (3 mutants) | `#[cfg(not(windows))]` Instant-based twins: not compiled on Windows. |
| `display_clock.rs` 405:70 `&&` to `\|\|`, 405:83 `>` to `>=` in the Windows `qpc_frequency` | The `else` branch is the fallback for a failing `QueryPerformanceFrequency` or a non-positive frequency; the API cannot fail on Windows XP and later, and a successful call never returns 0, so `!= 0 && > 0`, `!= 0 \|\| > 0` and `!= 0 && >= 0` all select the same branch. Not observable: [P-448](../PROPOSED-DECISIONS.md). |
| `instance_scope.rs` 508:9 `Drop for HeldClaim::drop -> ()` | Equivalent: `InstanceForwardServer` has its own `Drop` that calls `dispose` (`forward_server.rs`), and `dispose` is idempotent, so the explicit `server.dispose()` only changes the order of two releases (server stop before mutex close instead of after, since `mutex` is declared before `server`). Both complete inside the drop; no observer can sit between them. Kept inside P-448. |

## Rerun

`mutants.sh --native --jobs 1 --cargo-test-arg --lib` on `instance_scope.rs` with `-F` regexes of the survivor functions (preflight 316 tests, exit 0): **21 identities, 30 minutes, 16 caught, 3 missed, 2 unviable, 0 timeout**. The 3 missed are exactly the not-compiled 582:9 pair and the equivalent 508:9; all 12 killable survivors are among the 16 caught.

## Production change

None. No `#[doc(hidden)]` seam was needed; P-449..P-455 unused.

## Notes

- The run opens windows on the owner's desktop; it needs an interactive session.
- Lanes not started after this one: `forward_{pipe,server,client}.rs`.
