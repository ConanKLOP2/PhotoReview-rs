# ADR-X-exe-panic-exit: A panic that escapes ShellStartup::run ends the process with exit code 101

- **Status:** Proposed (draft written by DOC-ADR-DRAFTS, machine C, 2026-10-23; not accepted by the lead)
- **PARITY row:** `X-exe-panic-exit` in `docs/PARITY.md`, listed in `docs/PARITY-CLOSEOUT.md` ("Different")
- **Decision owner:** project lead

## Context
`ShellStartup::run` never panics by contract (17e T5) and the panic hook it installs already wrote the forced log. C# ended an unhandled exception with the CLR crash code. R0003 names three places for `catch_unwind`; D-W8B6-41 adds none to `main`. The PARITY row `X-exe-panic-exit` has proving test "planned".

## Proposed decision
No fourth `catch_unwind`: an escaped panic ends the process with Rust's default exit code 101, after the panic hook's log (D-W8B6-54).

## Consequences
Exit code differs from the CLR crash code; no user-visible dialog beyond the log. The exit-code seam (`run_main`) stays testable without a window.

## Alternatives not chosen
Wrap `run` in `catch_unwind` in `main` and map to a C#-like code (adds a production `catch_unwind` outside R0003's list).

## Sources
`docs/PARITY.md` row `X-exe-panic-exit`; `contracts/README.md` D-W8B6-41, D-W8B6-54; `docs/adr/R0003-concurrency.md`.

## Not verified
The proving test is not written (row says planned); the CLR exit code value is not stated in the row.
