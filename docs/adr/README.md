# ADR index

Accepted records: `D59-decode-error-inner-hresult.md`, `D60-raw-orientation-test-helper.md`, `R0002-coexistence-with-csharp-app.md`, `R0003-concurrency.md`, `ADR-INV-journal-durability-recycle-ui-native-ci.md`, `ADR-I-07-preview-registration-recheck.md`, `ADR-S3-preload-factory-lifetime.md`; `ADR-D-2026-10-31-intentional-differences-from-fix-lanes.md` (DV-1..DV-21, the intentional differences of the 2026-10-31 fix lanes); `R0001-ui-framework.md` is proposed (see its header); the ten `ADR-D-*`, `ADR-F-*`, `ADR-X-*` drafts below are Proposed. The status of every file, checked 2026-10-26, is in the table at the end of this index.

## Drafts for the "Different" rows of `docs/PARITY-CLOSEOUT.md` (DOC-ADR-DRAFTS, status Proposed, not yet reviewed)

| PARITY row | Draft or existing record |
|---|---|
| X-exe-versioninfo | [`ADR-X-exe-versioninfo-no-version-resource.md`](ADR-X-exe-versioninfo-no-version-resource.md) |
| F-06 (group compensation) | [`ADR-F-06-group-compensation-panic.md`](ADR-F-06-group-compensation-panic.md) |
| F-12 (PowerLossSafe thread) | [`ADR-F-12-no-thread-hop-power-loss-safe.md`](ADR-F-12-no-thread-hop-power-loss-safe.md) |
| X-bench-cli-batch | [`ADR-X-bench-cli-batch-bench-cli-output-differences.md`](ADR-X-bench-cli-batch-bench-cli-output-differences.md) |
| X-image-presenter-stat-cancel | [`ADR-X-image-presenter-stat-cancel-presenter-stat-cancel-is-no-event.md`](ADR-X-image-presenter-stat-cancel-presenter-stat-cancel-is-no-event.md) |
| X-image-presenter-ownership | [`ADR-X-image-presenter-ownership-presenter-owns-its-collaborators.md`](ADR-X-image-presenter-ownership-presenter-owns-its-collaborators.md) |
| X-viewer-state-notify | [`ADR-X-viewer-state-notify-viewer-state-change-sets.md`](ADR-X-viewer-state-notify-viewer-state-change-sets.md) |
| D-actions-unknown-operation | [`ADR-D-actions-unknown-operation-config-json-unknown-operation.md`](ADR-D-actions-unknown-operation-config-json-unknown-operation.md) |
| D-presentation-sink-cross-thread | [`ADR-D-presentation-sink-cross-thread-presentation-sink-not-send.md`](ADR-D-presentation-sink-cross-thread-presentation-sink-not-send.md) |
| X-exe-panic-exit | [`ADR-X-exe-panic-exit-escaped-panic-exits-101.md`](ADR-X-exe-panic-exit-escaped-panic-exits-101.md) |
| D-config-paths, P-07, P-14, D-13, D-15 (all `Different (R0002)`) | No new draft: already decided by the accepted [`R0002-coexistence-with-csharp-app.md`](R0002-coexistence-with-csharp-app.md) (separate data root `PhotoReviewRs`, own mutex/pipe names, own language and export folders). |

## Status of every record (checked 2026-10-26 against `origin/master` 8b93fbf)

Each file states its status in its first lines. "PROPOSED-DECISIONS" is `docs/PROPOSED-DECISIONS.md`.

| File | Status | Match with PROPOSED-DECISIONS |
|---|---|---|
| `R0001-ui-framework.md` | Proposed (direction chosen, windowed p95 gate open) | P-06 PROPOSED, spike done 2026-10-18 |
| `R0002-coexistence-with-csharp-app.md` | Accepted (2026-10-14) | no P row |
| `R0003-concurrency.md` | Accepted (2026-10-14) | no P row |
| `D59-decode-error-inner-hresult.md` | Accepted (2026-10-17, owner: user) | P-32 CONFIRMED by user 2026-10-17 |
| `D60-raw-orientation-test-helper.md` | Accepted (2026-10-17) | no P row; unrelated to the contract decision "D60 Wiring done by the lead" of Wave 7 in `contracts/README.md` and P-35 |
| `ADR-INV-journal-durability-recycle-ui-native-ci.md` | Accepted by the project lead 2026-10-23 | P-52 (Q1, Q2, Q7 of Q1-Q8), accepted as proposed |
| `ADR-I-07-preview-registration-recheck.md` | Accepted (2026-10-26, intentional concurrency difference) | no P row; PARITY row `D-preview-registration-cache-recheck` |
| `ADR-S3-preload-factory-lifetime.md` | Accepted and implemented (#225, 2026-10-24) | no P row |
| the ten `ADR-D-*`, `ADR-F-*`, `ADR-X-*` drafts listed above | Proposed (drafts by DOC-ADR-DRAFTS, not accepted by the lead) | no P row names them; they are tracked by `docs/PARITY-CLOSEOUT.md` ("Different" rows) |
| [`ADR-D-2026-10-31-intentional-differences-from-fix-lanes.md`](ADR-D-2026-10-31-intentional-differences-from-fix-lanes.md) | Accepted (2026-10-31, standing instruction; owner may reverse any item) | P-58 and P-59 (and P-57 for DV-11); PARITY rows F-01..F-06 service, F-18, F-13, F-04, P-03, P-05, W4-14c, I-07, X-settings-ui-pages, X-host-shell-window-messages, X-duplicate-cleanup, X-folder-load; DV-13..DV-21: S-64, INV-11, X-http-transport, I-15b, I-02b, X-zoom-detail, X-image-presenter, X-sibling-navigator, X-startup-options-run, X-startup-composition-root, X-main-window-lifecycle, F-23 |
| [`ADR-D-2026-10-31-recycle-c2-shell-outcome.md`](ADR-D-2026-10-31-recycle-c2-shell-outcome.md) | Accepted (2026-10-31, standing instruction; native confirmation pending) | P-61 (DV-22); PARITY row P-03 |
| [`ADR-W10-release.md`](ADR-W10-release.md) | Proposed, recommendations implemented with publication blocked pending input reviews | P-54..P-56, D-W10release-1..8; release-only supersession of P-47 |
| [`ADR-D-2026-10-31-bench-host.md`](ADR-D-2026-10-31-bench-host.md) | Proposed, acted on (2026-10-31, no user confirmation yet) | P-60; leaf crate `pr-bench-win` with `WindowsEngineHost` |
