# ADR-D-2026-11-03-file-action-gate-token: The INV-4 file-action gate is released by a token, not by a public `end()` (P-71)

- **Status:** Accepted. Owner chose option C of `P-71` in `docs/PROPOSED-DECISIONS.md` (2026-11-03).
- **PARITY row:** `D-file-action-gate-token` in `docs/PARITY.md`
- **Decision owner:** owner

## Context
C# `FileActionService.End()` is `Volatile.Write(ref _inProgress, 0)` (`FileActionService.cs:50`; re-read in the C# repo at the baseline `7eaee2a2` on 2026-11-03: `TryBegin` at `:45`, `End` at `:50`, callers `UndoService.cs:77-93` and `RecoveryRetryService.cs:61-69`, which holds a concrete `FileActionService?`, there is no `IFileActionGate` in C#). Any caller can run it at any time, so a stray or duplicate `End()` clears the in-progress flag of a LATER action and lets two file actions run at once (INV-4). The Rust port copied this: `FileActionService::end()` was `pub` and stored 0 unconditionally, and the trait `FileActionGate` (`undo.rs`) exposed `end` too. Every internal path already used an RAII guard, and no stray caller existed.

## Decision
`pr_core::file_actions::undo` gets a release token and the public `end` goes away:
- `FileActionGate::try_begin(&self) -> Option<GateToken>` (was `bool`); `FileActionGate::end` is removed; `is_busy` is unchanged.
- `GateToken` is not `Clone`, has no public way to be built from a gate without a successful begin, and releases exactly once when dropped (also on a panic) or via `GateToken::release(self)`. `GateToken::new(release)` exists for `FileActionGate` implementors and test doubles.
- `GateFlag` is the shared flag: 0 = free, otherwise the id of the holding token. A token releases with a compare-exchange `id -> 0`, so a release can only ever free its own hold.
- `FileActionService`, `UndoService` (its own gate and the shared one), `RecoveryRetryService` use the token directly; the private `GateGuard`/`GateHold` types are gone. `FileActionService::end` and `UndoService::end` are removed.
- Behaviour of every internal path is unchanged: same results, same journal lines, same errors, same drop order (transaction first, then the gate).

## Consequences
- A stale or duplicate release cannot clear another action's flag; the hazard is gone by construction, not by convention.
- Public API change: callers of `try_begin()` must keep the returned token alive for as long as the gate should stay taken (`#[must_use]`). Production code in `pr-app` and `pr-shell` only calls `is_busy`, so none changed. Tests that held the gate by hand now hold a token.
- Drift from C#: C# has no token and `End()` stays unconditional there. The Rust gate behaves identically for every sequence C# can legally produce (begin, work, end).

## Alternatives not chosen
A: keep `end()` as in C# (hazard stays reachable). B: narrow `end` to `pub(crate)` (the trait would still need a change, and a crate-internal stray `end` stays possible).

## Undo
Restore `FileActionGate::end` and the `bool` return of `try_begin`, make `GateToken` release by storing 0 again, and put `end()` back on `FileActionService` and `UndoService`.

## Tests
`crates/pr-core/tests/file_actions_gate_token.rs`: `a_finished_actions_token_never_frees_a_later_actions_gate` (red on the old API as `stale_release_never_clears_a_later_actions_flag`: a second `end()` freed the running action's gate), `failed_begin_yields_no_token_and_the_holder_keeps_the_gate`, `dropping_or_releasing_the_token_runs_the_release_exactly_once`, `a_panic_while_holding_the_token_still_frees_the_gate`, `execute_while_held_is_rejected_without_releasing_the_holder_then_runs_and_frees_the_gate`. The existing gate, undo, recovery-retry, rename and composition tests were moved to the token API with unchanged assertions.

## Sources
`docs/PROPOSED-DECISIONS.md` row `P-71`; `docs/reviews/FIX-G30-G12-2026-11-01.md` (BACKLOG G12 L1); `crates/pr-core/src/file_actions/{undo,service,recovery_retry}.rs`.

## Not verified
The C# lines above were re-read on 2026-11-03; the C# side is not changed (proposal: PR draft into the C# repo, fragment RP-GATE-END-01). Native Windows tests were not run.
