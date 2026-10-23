# ADR-X-image-presenter-ownership: ImagePresenter owns sink, compare view model and zoom loader; completions are pumped

- **Status:** Proposed (draft written by DOC-ADR-DRAFTS, machine C, 2026-10-23; not accepted by the lead)
- **PARITY row:** `X-image-presenter-ownership` in `docs/PARITY.md`, listed in `docs/PARITY-CLOSEOUT.md` ("Different")
- **Decision owner:** project lead

## Context
C# `ImagePresenter` receives collaborators through the constructor and resumes `Task` continuations. The Rust presenter (`state::{ImagePresenter, PresenterEnv, PresenterChanges}`) owns its sink, the compare view model and the zoom loader, borrows catalog, settings, session and format context per call (`PresenterEnv`), and pumps completions (`pump`). The re-entrant `ZoomModeChanged -> SetViewerZoom` becomes a pull (`take_viewer_zoom_change`) right after the sink call (D-W8-1, D-W8-2).

## Proposed decision
Keep the ownership and pump model. It follows R0003 (no async on the UI thread) and avoids `Rc<RefCell>` cycles between presenter, view model and sink.

## Consequences
Call sites must pass a `PresenterEnv`; the composition root (W8-09b) wires `PresentationBridge` instead of a VM/sink cycle. Re-entrancy becomes an explicit pull.

## Alternatives not chosen
Share collaborators as `Rc<RefCell<..>>` and mirror the C# events (closer to C# structure, risks borrow panics; D-W8B4M-2 forbids holding a borrow across a call that can re-enter).

## Sources
`docs/PARITY.md` row `X-image-presenter-ownership`; `contracts/README.md` D-W8B4M-2, D-W8B4I-8; `docs/adr/R0003-concurrency.md`.

## Not verified
D-W8-1 and D-W8-2 are cited by the row; their full text was not located in `contracts/README.md` (only references to them).
