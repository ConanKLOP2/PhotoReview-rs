# ADR-X-viewer-state-notify: Viewer state notifications become change sets and return values

- **Status:** Proposed (draft written by DOC-ADR-DRAFTS, machine C, 2026-10-23; not accepted by the lead)
- **PARITY row:** `X-viewer-state-notify` in `docs/PARITY.md`, listed in `docs/PARITY-CLOSEOUT.md` ("Different")
- **Decision owner:** project lead

## Context
C# `ViewerState` raises `PropertyChanged`, `ZoomModeChanged` and `SourceSizeSwapping`. In Rust `pr_app::viewmodels::viewer_state` returns `ViewerChanges` and `ViewerUpdate` values (D-W8-1, D-06a-2: events become return values).

## Proposed decision
Keep the return-value model. Subscribers are replaced by one fan-out, `MainWindow::apply_changes(&ChangeSet)` with a fixed order (D-W8B4M-2).

## Consequences
No event re-entrancy; fan-out order is fixed and testable. Any new subscriber must be added to the fan-out.

## Alternatives not chosen
Port the three events as callbacks (structure closer to C#, re-entrancy and borrow risks).

## Sources
`docs/PARITY.md` row `X-viewer-state-notify`; `contracts/W8-06a.md` D-06a-2; `contracts/README.md` D-W8B4M-2.

## Not verified
The row says only "covered by the goldens above"; which goldens was not traced.
