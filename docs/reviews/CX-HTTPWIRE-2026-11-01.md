# CX-HTTPWIRE, 2026-11-01

Lane `cx-httpwire`, branch `feat/cx-httpwire`, base `origin/master` 71d9ea76. C# read-only at `7eaee2a2`.

## Findings (what was already merged)

The rows `X-http-transport` and `X-http-composition` in `docs/PARITY.md` still say "Todo ... stubs only". That is stale. Master already has:

- W10-H1/H2/H3 implemented (#330): `pr_platform_win::http::{policy,transport}`, `pr_shell::composition::update_http`, checker injected into the Settings flow.
- Later fixes: killers for `wide`/`os_error` and SEC-01 (#447), autologon disabled and control characters in redirects rejected (#484, DV-14), update repo `ConanKLOP2/PhotoReview-rs` (#495, DV-22, P-61).
- Tests: `http_policy` (4), `http_transport_loopback` (18 F + 2 ignored N), `update_http_composition` (9 F + 1 ignored N), `updates_checker` (includes `no_release_published_404_is_a_failure_not_an_update`), private `http::transport::tests`.
- The two ignored panic placeholders (`native_proxy_pac_trust_and_cancel_matches_windows_defaults`, `get_https_to_http_redirect_returns_three_xx_response`, `native_settings_close_cancels_and_open_page_uses_shell`) are `#[ignore]` and not listed in `native-nightly.yml`, so they do not turn CI red. They stay honest placeholders for human runs.

## Added

`crates/pr-platform-win/tests/http_update_wire.rs` (7 F tests): the real `UpdateChecker` over the real `WinHttpTransport` against a bounded loopback server, asserting the status mapping on the wire (previously only checked with a fake transport): 404 (DV-22) -> `BadResponse`; 500/503/403/401 -> `BadResponse`; 429, 403 + `X-RateLimit-Remaining: 0`, 403 + `Retry-After` -> `RateLimited`, 403 + remaining 7 -> `BadResponse`; 200 newer / equal -> `UpdateAvailable` / `UpToDate`; HTML body -> `BadResponse`; 302 to the release is followed; connection refused -> `Offline`. No production code changed, no dependency, no unsafe, no real network.

## Run

- `cargo test -p pr-platform-win --test http_update_wire -j 2`: 7 passed, 0 failed.
- `cargo fmt -p pr-platform-win --check`: clean.
- Not run (limit): other test binaries, clippy, workspace; CI covers them.

## Decisions

No new P-nn: the 404 case is already P-61/DV-22 ("no release -> Failed(BadResponse)"); the new test pins that behaviour.

## PARITY rows proposed (orchestrator edits)

- `X-http-transport`: Todo -> Partial. Evidence: #330, #447, #484, `http_policy`, `http_transport_loopback`, `http_update_wire`. Remaining: proxy/PAC, Windows trust, HTTPS->HTTP downgrade over real TLS, cancel on an attended rig (human).
- `X-http-composition`: Todo -> Partial. Evidence: #330, `update_http_composition` (9 F). Remaining: attended Settings close / late completion / ShellExecute open (human). Note the old text "`DialogServices.update_checker` is None" and the `todo!()` claim are no longer true (grep finds no `todo!()` in these paths).

## Needs a human

- Proxy/PAC, certificate trust and TLS rig (`native_proxy_pac_trust_and_cancel_...`, HTTPS downgrade): needs a trusted local HTTPS server or a real network.
- Attended Settings run: close during check, late completion, open page via ShellExecute.
- Release in `ConanKLOP2/PhotoReview-rs`: until one exists the check shows a failure (P-61).
