# ADR-X-image-presenter-stat-cancel: A stat cancelled before it ran is "no event"; the `ShowImage superseded during stat` log line has no Rust twin

- **Status:** Proposed (draft written by DOC-ADR-DRAFTS, machine C, 2026-10-23; not accepted by the lead)
- **PARITY row:** `X-image-presenter-stat-cancel` in `docs/PARITY.md`, listed in `docs/PARITY-CLOSEOUT.md` ("Different")
- **Decision owner:** project lead

## Context
C# `StatOffUiThreadAsync` throws `OperationCanceledException` when a stat is cancelled before it runs, and the presenter logs `ShowImage superseded during stat`. In Rust `ImagePresenter::submit_stat` has no async task: a stale worker result is dropped by token and phase.

## Proposed decision
Treat a cancelled-before-run stat as "no event"; do not port the log line. Coverage stays with `presenter_present_a.rs` (`present_token_superseded_*`, `present_superseded_navigation_with_a_later_preview_failure_changes_nothing`).

## Consequences
One diagnostic log line is missing from Rust logs. UI-visible behaviour is unchanged. Follows the R0003 mapping of cancellation to tokens.

## Alternatives not chosen
Emit the same log line when the token check fails in `submit_stat` (parity of logs, needs a distinct path for "cancelled before run").

## Sources
`docs/PARITY.md` row `X-image-presenter-stat-cancel`; `docs/adr/R0003-concurrency.md`.

## Not verified
The C# line numbers of `StatOffUiThreadAsync` and the exact log text were not looked up for this draft.
