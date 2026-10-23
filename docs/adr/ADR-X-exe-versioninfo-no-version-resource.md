# ADR-X-exe-versioninfo: The Rust exe carries no VERSIONINFO resource (release builds may add one)

- **Status:** Proposed (draft written by DOC-ADR-DRAFTS, machine C, 2026-10-23; not accepted by the lead)
- **PARITY row:** `X-exe-versioninfo` in `docs/PARITY.md`, listed in `docs/PARITY-CLOSEOUT.md` ("Different")
- **Decision owner:** project lead

## Context
The C# `PhotoReview.Shell.Win32` exe is built by .NET, so Windows Properties shows a product version. The Rust exe is linked with a manifest and an icon only: `crates/pr-shell/build_support/res_writer.rs` states "No `VERSIONINFO`" (open question 3 of W8-20a). The PARITY row says `Different (no VERSIONINFO resource, D-W8B6-53; release-only RT_VERSION proposed by W10-R2, new ADR pending, P-47)`.

## Proposed decision
Keep the W8 default: no `RT_VERSION` in ordinary builds. Add one only for release builds, from a validated `vX.Y.Z` tag (`PHOTOREVIEW_RS_VERSION`), when W10-R1 ships; the ADR that accepts this replaces the `Different` row by `Done` once a PE-parsing test (`exe_version_resource_matches_tag`) passes. This is the option B recommended by D-W10release-3.

## Consequences
Properties shows no version until the release unit lands. A release-only resource needs a small resource writer (D-W8B6-53 estimates about 60 lines) and a PE parser in tests. Local builds keep `0.0.0-dev` out of the exe.

## Alternatives not chosen
A: never add a version resource (no writer risk, Properties stays empty). C: add it to all builds (consistent, but exposes `0.0.0-dev`).

## Sources
`docs/PARITY.md` row `X-exe-versioninfo`; `contracts/README.md` D-W8B6-53 and D-W10release-3; `contracts/W10-R2.md`; `docs/PROPOSED-DECISIONS.md` P-47; `crates/pr-shell/build_support/res_writer.rs`.

## Not verified
Exact `VS_VERSIONINFO` layout and SmartScreen behaviour (W10-R2 marks them unverified). P-47 is still `PROPOSED`.
