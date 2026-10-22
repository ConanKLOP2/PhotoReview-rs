# Proposed PARITY rows for W10-http

Use the exact five-column layout of `docs/PARITY.md`. These are proposals; no status is advanced by writing contracts or stubs.

| ID | Status (Todo / Doing / Done / Different) | Rust location | Proving test | Note |
|---|---|---|---|---|
| P-09 | Partial (W2-18, W10-H1/H2/H3 pending) | `pr_core::updates::checker`; `pr_app::presenters::settings`; `pr_platform_win::http`; `pr_shell::composition::update_http` | `updates_checker::*`, golden `update-checker` (126 cases); proposed `http_policy`, `http_transport_loopback`, `update_http_composition`; N desktop run pending | Core checker and presentation logic are merged; real transport/injection and desktop verification remain. |
| X-http-transport | Todo (contract W10-H1/H2) | `pr_core::updates::checker::HttpTransport`; `pr_platform_win::http::{policy,transport}` | Proposed `http_policy`, `http_transport_loopback`; N proxy/TLS/cancel run pending | Stub only; system/current-user proxy, 50 redirects, trust, deadline and prompt cancellation require implementation and validation. |
| X-http-composition | Todo (contract W10-H3) | `pr_shell::composition::update_http`, `pr_shell::composition::graph`, `pr_shell::startup::secondary_dialogs` | Proposed `update_http_composition`; N Settings dialog run pending | Inject real checker into the existing manual Settings flow; `DialogServices.update_checker` is currently None; existing close and open URL host code also has `todo!()`. |
