Status: evidence report (2026-11-20). Native mutation lane A for `pr-platform-win`: `host/main_window/core.rs` 792:9 (and its neighbour 1247:13) and the 14 `Url::parse` identities of `http/transport.rs`. Measured on one machine (Windows 11, interactive session 1, owner present), `mutants.sh --jobs 1`, `MUT_TIMEOUT=600`, own target dir, no source or test change.

# `pr-platform-win` native lane A: mutation verdict

## Result

16 identities tested, **16 caught, 0 missed, 0 timeouts, 0 unviable**. No killer test was needed and no P-nn row: every mutant was already guarded by existing tests, which only run with `--include-ignored` (native) and with the right test binaries listed.

| File | Filter (`-F`) | Test binaries | Identities | Caught | Missed |
|---|---|---|---|---|---|
| `host/main_window/core.rs` | `core.rs:792:9` | `mut8_main_window_core` (native) | 2 | 1 (792:9) | 1 (1247:13, see below) |
| `host/main_window/core.rs` rerun | `core.rs:1247:13` | `mut8_main_window_core,host_main_window_core` (native) | 1 | 1 | 0 |
| `http/transport.rs` | `in Url::parse` | `http_policy, http_transport_loopback, http_update_wire, http_proxy_env, http_url_parse_killers, http_wire_oracle, mut2_n4_transport_killers, mut3_transport_proofs` | 14 | 14 | 0 |

- `core.rs` 792:9 (`Debug for MainWindowInner -> Ok(Default::default())`) is caught by `mut8_main_window_core::main_window_debug_shows_the_inner_state_with_its_id` (native, `#[ignore]`: it only runs with `--include-ignored`).
- `core.rs` 1247:13 (`delete field prefer_warp from struct SurfaceOptions expression in MainWindow::attach_renderer`) matched the same filter and was missed by the first run only because `host_main_window_core` was not in `--tests`. The existing native test `attach_renderer_forwards_prefer_warp_to_surface_factory` kills it (rerun: 1 of 1 caught).
- `transport.rs` `Url::parse`: the 14 identities are the six `URL_COMPONENTS` field deletions (186:13..191:13), the credentials guard (196:35, 196:40, 196:66), and the comparisons after the crack (200:22, 209:35, 210:12, 210:19, 210:36). All 14 caught by the existing loopback and `http_url_parse_killers` tests. The line numbers moved since the 2026-11-03 run (P-404 extracted two seams), which is why the old 168..181 numbers no longer match.

## Tests skipped because the resource is unavailable here (unmutated preflight fails)

The preflight of the transport run failed on three native tests that do not pass on this machine even without a mutant; they were skipped with `--skip` (55 tests executed, exit 0). None of them touches `Url::parse` beyond what the other tests cover:

- `http_transport_loopback::get_https_to_http_redirect_returns_three_xx_response`: needs a locally trusted HTTPS server ("do not disable certificate validation").
- `http_transport_loopback::native_proxy_pac_trust_and_cancel_matches_windows_defaults`: needs an attended local network rig (outcomes not recorded).
- `mut2_n4_transport_killers::the_header_limit_is_a_single_threshold_set_by_winhttp_below_the_checked_size`: this Windows build's WinHTTP answers `InvalidResponse` for a 65539-byte header pad (the test pins an older WinHTTP threshold). This is a test/OS drift to review, not a product defect. It is not needed by the `Url::parse` mutants (all 14 were caught without it).

## Notes

- The runs open hidden or offscreen windows on the owner's desktop (WARP surfaces); they need an interactive session.
- Not run in this lane: the other `transport.rs` functions (outside `Url::parse`).
