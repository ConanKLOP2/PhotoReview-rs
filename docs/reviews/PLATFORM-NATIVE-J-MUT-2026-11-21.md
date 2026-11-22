Status: evidence report (2026-11-21), run done, all 15 open survivors decided (5 equivalent, 10 not observable); 1 timeout is a hang. Native mutation lane J for `pr-platform-win`: whole-file run of `forward_pipe.rs`, `forward_server.rs` and `forward_client.rs`. Measured on one machine (Windows 11, interactive session 1, owner present), `mutants.sh --native --jobs 1`, `MUT_TIMEOUT=600`, own target dir.

# `pr-platform-win` native lane J: `forward_pipe.rs`, `forward_server.rs`, `forward_client.rs`

## Result

First run, 201 identities, 5 hours: **142 caught, 42 missed, 16 unviable, 1 timeout, 0 not run**. After the killers below (two reruns of the survivor functions): **169 caught, 15 missed, 16 unviable, 1 timeout**; the 15 are decided, none is a gap a test can reach.

Test set (`--include-ignored --test-threads=1`, preflight 150 tests, exit 0): the binaries `forward_client`, `forward_client_native`, `forward_pipe_native`, `forward_server_native`, `q11_platform_killers`, `w6_mutation_killers`, `w6_p01_killers`, `w6_platform_native`, `w6_rerun_killers` and the new `forward_native_j_killers`. Skipped as the earlier lanes skip tests that need unavailable resources: the `win32_*` binaries and `render_keep_alive`.

## The 1 timeout and the 16 unviable

- `forward_client.rs` 380:47 `|` to `&` in `connect` (`FILE_ATTRIBUTE_NORMAL & FILE_FLAG_OVERLAPPED` is 0, the handle is not overlapped): the test run hangs for the full 600 s; detected as a hang, not a survivor.
- 16 unviable (do not compile): `for_sid`, `current_security_descriptor`, four `lock` bodies, `with_factory`, 307:58 `+` to `*` (`Instant * Duration`), 330:24 and 461:40 (`|` on newtypes, no `BitXor`), native `create`, `ForwardPipeClient::with_transport`, `send_with_timeout`, `send_locked`, 380:47 `^`, and the production `send`.

## Survivors killed (27), by new tests

New file `crates/pr-platform-win/tests/forward_native_j_killers.rs` (every test uses a uniquely named pipe) and additions to `forward_server_native.rs` (fake factory, no native pipe).

| Survivor (line:col, mutant) | Killer |
|---|---|
| `forward_server.rs` 210 `Display for CallbackPanic`, 222 `log_listener_failure -> ()`, 201 `Drop for InstanceForwardServer -> ()`, 307:58 `+` to `-` (delivery deadline in the past) | `forward_server_native.rs`: the callback-failure test now expects the panic text in the log; `server_logs_a_listener_failure_with_its_error_and_the_consecutive_count`; `dropping_a_started_server_cancels_its_factory`; `server_reply_delivery_wait_is_bounded_by_a_deadline_in_the_future` (the fake records the time left on the deadline it receives) |
| 500 `is_canceled`, 504 `wait_retry -> true/false`, 507 `==` to `!=` of the native factory | `native_factory_cancel_wakes_the_retry_wait_and_reports_canceled` (through the seam P-456) |
| 603 and 611 `write_all` guards, 621 `wait_reply_delivered -> Ok(())`, 623 `read == 0` to `!=` | `the_server_keeps_its_pipe_until_the_client_hangs_up_and_a_busy_pipe_makes_the_next_client_wait` (the server holds its only pipe instance until the client hangs up, so the next client waits), `the_listener_takes_the_next_client_promptly_after_a_delivered_request`, `a_client_that_vanishes_after_sending_does_not_stall_the_listener`, `a_slow_client_still_receives_the_reply_the_server_wrote` |
| 584:31 (`false`), 584:46 (`!=`), 414:31 (the `false` side) | `a_client_that_hangs_up_before_sending_anything_is_a_rejected_empty_request` (a connect-and-close client makes the first `ReadFile` fail at once with a broken pipe; found with a probe that printed the arm) and `a_client_that_hangs_up_mid_request_is_a_rejected_request_not_a_read_failure` (pending read) |
| `forward_client.rs` 400:50 (`==` to `!=` on `ERROR_PIPE_BUSY`) | the busy-pipe test above: a busy pipe is waited for, not reported as `NoServer` |
| 436:20 (`pid == 0` to `!=`) | `foreground_permission_for_a_same_process_server_is_granted` |
| 475:68 (`==` to `!=`) | `write_with_an_already_expired_deadline_times_out_before_any_byte_is_written` |
| 212:9 (3 mutants) and 243:9 (3 mutants): `sid_bytes`/`process_user_sid -> Some(vec![..])` | `the_current_process_user_sid_is_a_well_formed_sid` (seam P-459) and `a_system_owned_pipe_server_is_not_recognised_as_the_current_user` (a token that cannot be read must fail closed) |
| `forward_pipe.rs` | only unviable mutants and killed ones; no open survivor |

## The 15 open survivors

| Survivors | Verdict |
|---|---|
| 305:23 `<` to `<=` in `handle_request` | Equivalent: it compares `Instant::now()` with a deadline `read_timeout` in the future; the two clock readings are never equal in a way a test can build, and the `<=` branch would only attempt one more reply write that the deadline check inside `write_all` ends. |
| 376:40 `\|` to `^` (`GENERIC_READ.0 \| GENERIC_WRITE.0`), 634:30 `\|` to `^` in `hresult_from_win32` | Equivalent: the operands share no bit (0x8000_0000 and 0x4000_0000; `code & 0xFFFF` and 0x8007_0000), so `\|` and `^` give the same value. |
| 215:21 `<` to `==` and `<` to `<=` in `sid_bytes` | Equivalent: `required` is the size of a `TOKEN_USER` (16 bytes) plus a SID of at least 12 bytes, so it is never 16; for `required` below 16 the mutant passes an empty buffer to `GetTokenInformation`, which fails and returns `None` through `.ok()?`, the same as the original. |
| 507:40 `&&` to `\|\|` | [P-457](../PROPOSED-DECISIONS.md): differs only inside the cancel race window. |
| 426:13, 430:13, 431:24, 493:16 (`active_pipe` bookkeeping) | [P-458](../PROPOSED-DECISIONS.md): `stop_event` wakes every wait, `CancelIoEx` on the active handle adds nothing. |
| 475:61, 482:31 (`offset == 0` guards to `true`) | [P-460](../PROPOSED-DECISIONS.md): a byte-mode overlapped pipe write is all or nothing, `offset` is always 0. |
| 414:31 (`true`), 558:31 (`true`), 584:31 (`true`) | [P-461](../PROPOSED-DECISIONS.md): no other error code reaches those arms. |

## Rerun

Two reruns of survivor functions with `-F` regexes (preflight 150 and 153 tests, exit 0): 44 identities (18 caught, 26 missed) after the first killers, then 18 identities (15 caught, 3 missed) after the busy-pipe, silent-hang-up and SID tests. Together with the 15 open above, every one of the 42 first-run survivors is accounted for (27 killed, 15 decided). The last 3 missed are 584:31 `true`, 215:21 `==` and `<=`, all decided above.

## Production changes

Two `#[doc(hidden)]`, `cfg(windows)`, behaviour-neutral seams: `forward_server::native_factory_for_tests` (P-456) and `forward_client::current_user_sid_for_tests` (P-459). P-462 and P-463 not needed.

## Notes

- The run opens pipes named `PhotoReviewRs.test.nativej.*`; the foreground-permission test calls `AllowSetForegroundWindow` for the test process itself.
- `a_system_owned_pipe_server_is_not_recognised_as_the_current_user` opens well-known system pipes read-only and closes them at once; it fails when none accepts the client, it never passes silently.
