# ADR-F-12: File actions run on the caller (file-action lane) thread; no pool hop in PowerLossSafe mode

- **Status:** Proposed (draft written by DOC-ADR-DRAFTS, machine C, 2026-10-23; not accepted by the lead)
- **PARITY row:** `F-12 (PowerLossSafe Prepared on a pool thread)` in `docs/PARITY.md`, listed in `docs/PARITY-CLOSEOUT.md` ("Different")
- **Decision owner:** project lead

## Context
C# runs mutations and, in PowerLossSafe mode, the Prepared append on the thread pool. R0003 forbids an async runtime and runs blocking work on explicit threads. W3-04 open question 6 noted that Rust runs the whole action on the caller's thread; only the order append(durable) < move < append(durable) is ported.

## Proposed decision
Run `FileActionService::execute` entirely on the caller's thread (the app's file-action lane, never the UI thread). Port the ordering half of `SafeMode_PreparedAndMutationRunOffCallerThread_*` as `safe_mode_prepared_and_mutation_order_only_no_thread_hop`. Keep the `Different` row, or relabel it "same by design" if the lead prefers (W3-04 allows both).

## Consequences
Behaviour on disk and in the journal is unchanged; thread identity differs and is not observable by the user. No twin for the C# `(FileOperationType)99` arm (closed enum), nor for a throwing clock or `File.Exists` (infallible in Rust); the row says replacement tests exist.

## Alternatives not chosen
Hop to a worker thread inside `execute` (matches C# thread identity, adds a thread and channel for no observable gain).

## Sources
`docs/PARITY.md` row `F-12 (PowerLossSafe Prepared on a pool thread)`; `docs/adr/R0003-concurrency.md`; `contracts/W3-04.md` open question 6.

## Not verified
That the app really calls `execute` only from the file-action lane in every path (W3-04 states it; not re-checked here).
