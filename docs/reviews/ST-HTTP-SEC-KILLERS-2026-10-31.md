# ST-HTTP-SEC-KILLERS (2026-10-31)

Status: finished (killer tests only, production code unchanged). Source: item 5 of section 6 of [ST-GAPMAP2](ST-GAPMAP2-2026-10-31.md).

## Added tests

| Test | File | Target (mutants it is meant to kill) |
| --- | --- | --- |
| `wide_appends_one_nul_terminator_and_encodes_utf16` | `crates/pr-platform-win/src/http/transport.rs` (unit) | `wide` body replaced by empty or `[0]`, dropped terminator, UTF-16 surrogate pair |
| `wide_rejects_an_embedded_nul_anywhere_as_other` | same | NUL guard (`contains('\0')`) removed or inverted, error text |
| `os_error_maps_only_the_winhttp_timeout_code_to_timeout` | same | `==` -> `!=`, arms swapped, classifier bypassed |
| `win_error_keeps_only_the_low_16_bits_of_the_hresult` | same | `& 0xffff` mask removed or changed |
| `resolve_self_link_fails_closed_with_1921`, `resolve_two_link_loop_fails_closed_with_1921`, `resolve_chain_that_enters_a_loop_fails_closed_instead_of_returning_the_last_target`, `resolve_cap_error_is_not_a_not_found`, `resolve_non_link_head_is_not_found_after_one_step`, `resolve_dangling_chain_returns_the_missing_last_target` | `crates/pr-core/tests/w2_sec_killers3.rs` | the manual link walk through `follow_link_chain_for_tests` |

## Run (only these binaries, `-j 2`)

- `cargo test -p pr-core --test w2_sec_killers3`: 6 passed.
- `cargo test -p pr-platform-win --lib http::transport`: 8 passed (4 new, 4 existing).
- `rustfmt --edition 2024 --check` on both files: clean after formatting (`cargo fmt -p` fails here with os error 206, path too long).
- cargo-mutants NOT run (lane limit): the "kills" above are by construction, not measured.

## SEC-01 resolver: fail-closed statement

The walk `follow_link_chain` never returns `Ok` for something it did not resolve: a non-link head is `NotFound`, a loop or a chain over `MAX_FOLLOWED_LINKS` (63) is the OS error 1921 (`ERROR_CANT_RESOLVE_FILENAME`), never `NotFound` (which a caller could read as "does not exist yet"). The new tests pin exactly that (self-link, 2-cycle, tail-into-cycle, error kind). The caller therefore refuses the destination on every failure.

## What remains open (honest limit)

The surviving mutant of the PARITY row (`final_link_target`, guard `is_path_unreachable(&err)` -> `true`, physical.rs ~983) is NOT killable through `follow_link_chain_for_tests`: it sits before the walk, on the `fs::canonicalize` error. For the loop case the mutant is observably equivalent (the slow walk also ends in 1921); a real kill needs an OS error that is neither a loop nor unreachable (bad reparse data), which a test cannot create without unsafe. A production seam taking the canonicalize result would make it testable; not done here (lane forbids production edits). TLS/PAC/trust and the other `X-http-transport` items are unchanged.

## Proposed PARITY changes (orchestrator decides; lane did not edit)

- `X-http-transport` (L69): stays Partial; replace "Other helpers (wide/os_error) ... remain open" with "wide/os_error/win_error pinned by 4 unit tests (mutation not run)".
- `SEC-01 / F-05` (L45): stays Partial; add `w2_sec_killers3::*` (6) to the evidence; the guard-`true` survivor is still open (see above). Do not raise to Done.
