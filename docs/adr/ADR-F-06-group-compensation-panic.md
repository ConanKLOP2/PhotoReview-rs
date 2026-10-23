# ADR-F-06: A panic in the group compensation propagates instead of returning an unknown-state result

- **Status:** Proposed (draft written by DOC-ADR-DRAFTS, machine C, 2026-10-23; not accepted by the lead)
- **PARITY row:** `F-06 (unexpected exception in the group compensation)` in `docs/PARITY.md`, listed in `docs/PARITY-CLOSEOUT.md` ("Different")
- **Decision owner:** project lead

## Context
C# `FileActionService.ExecuteGroupAsync` has an inner `catch (Exception) when not OOM` (lines 266-276) around compensation and inspection that returns an unknown-state result. In Rust every fake failure is an `io::Error`, already handled per member, so only a panic in a `FileSystem` implementation could reach that arm (W3-04 open question 2).

## Proposed decision
Option B of W3-04: no `catch_unwind` arm. A panic propagates; the RAII guard still releases the gate, the transaction's `Drop` releases the live marker, and the Prepared journal line stays for reconcile. The two C# tests are ported by asserting exactly that with `catch_unwind` inside the test.

## Consequences
Disk and journal outcome equal C#. The caller sees a panic instead of a result. Consistent with the W3-08 "no catch_unwind" rule as cited by W3-04, and with R0003, which treats panics as bugs (worker threads catch them only at the job boundary).

## Alternatives not chosen
Option A: wrap compensation, inspection and outcome in `std::panic::catch_unwind(AssertUnwindSafe(..))` and return the unknown-state result (exact C# parity; adds a production `catch_unwind`).

## Sources
`docs/PARITY.md` row `F-06 (unexpected exception ...)` (line 78 at the time of writing); `contracts/W3-04.md` open question 2 and G9; tests named in the row (`execute_group_compensation_throws_unexpected_type_*`, `compensation_throws_unexpected_exception_type_*`).

## Not verified
Whether the lead chose option B explicitly: the PARITY row says "option B" and W3-04 recommended it, but no accepted decision record was found.
