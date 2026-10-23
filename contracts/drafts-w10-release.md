# W10 release contract roster (proposal, 2026-10-23)

Only the draft files named in this roster were edited. The lead integrates them into production paths after resolving decisions and dependencies. No PR, commit or branch action is claimed.

| Unit | Contract | Size | Model for later porter | Depends on |
|---|---|---:|---|---|
| R1 | `contracts/W10-R1.md` | M | Sonnet 5.5 medium | R2, R4, W8-17a, W8-20a/20b |
| R2 | `contracts/W10-R2.md` | M | Sonnet 5.5 medium | W8-20a resource writer, P-47, icon sync |
| R3 | `contracts/W10-R3.md` | S | Sonnet 5.5 medium | lockfile licence review, CI owner |
| R4 | `contracts/W10-R4.md` | M | Sonnet 5.5 medium | C# native pins, licence/source review |
| R5 | `contracts/W10-R5.md` | M | Sonnet 5.5 medium | R0002, Rust update checker correction, W8-20a process test |

Sizes describe implementation scope, not measured effort. The model entries are recommendations constrained by AGENTS.md P-34, not spawned agents.

## Decisions for lead

| ID | State and options (benefit / cost) | Recommendation and next action |
|---|---|---|
| D-W10release-1 | Open. A: independent `vX.Y.Z` manual tags (clear product identity / manual version governance); B: mirror C# `v2.0.N` (familiar / collides conceptually with C# and cannot derive from this Rust repo); C: use Cargo `0.1.0` (automatic / mismatches external tag). | A. R1 validates existing tag and R2 embeds its exact version. Lead records an ADR and writes tag policy. |
| D-W10release-2 | Open. A: unsigned (no secret handling / Windows reputation warning); B: signtool with protected cert (familiar signed binary / certificate lifecycle and secret handling); C: Azure Trusted Signing (managed signing / external setup and cost). | A for first release. Document unsigned status; revisit signing after a certificate/service exists. |
| D-W10release-3 | Open; conflicts with P-47/D-W8B6-53. A: retain no RT_VERSION (no writer risk / Properties lacks version); B: release-only RT_VERSION (visible exact tag / new resource parser and ADR); C: all builds RT_VERSION (consistent / `0.0.0-dev` exposed). | B after PE tests pass. Lead resolves P-47, adds ADR and changes `X-exe-versioninfo` from Different to Done when proved. |
| D-W10release-4 | Open. A: pinned `cargo install --locked cargo-deny` (simple / build time); B: pinned cargo-deny action SHA (faster / new third-party action to audit); C: no CI audit (no setup / dependency drift). | A after checking a real version and draft schema. Replace the deliberate throw in `docs/drafts/ci-deny-job.yml`; nightly plus Cargo-change PRs. |
| D-W10release-5 | Open. A: correct Rust update feed to its own release repository (full feature / requires verified URL); B: disable update check for first release (safe / no in-app discovery); C: retain C# feed (no edit / sends Rust users to the wrong product). | B until the Rust release URL is verified, then A with tests. Never C. |
| D-W10release-6 | Open. A: bundle verified LibRaw source archive and licence/notice (offline source access / larger zip); B: durable source URL with hash (smaller / URL retention duty); C: DLL without source offer (small / unresolved LGPL obligations). | A after validating archive content with maintainer/legal review. R4 verifier makes missing files fatal. |
| D-W10release-7 | Open. A: ship `Languages/en.json` and `Languages/vi.json` beside the exe (matches `pr-l10n/src/loader.rs` shipped-folder lookup / adds files); B: embed all catalogs (single exe / changes loader and requires matching tests). | A is supported by `crates/pr-l10n/src/loader.rs` and the proposed path in `crates/pr-shell/src/composition/registry.rs`, but that composition method is still a stub. Decide after validating language selection through the built shell; the release verifier checks whichever layout is chosen. |
| D-W10release-8 | Open. A: runner smoke enabled by default (automatic gate / GitHub-hosted `windows-latest` has a non-interactive session and Direct2D window creation may fail); B: maintainer-machine smoke when runner cannot host the window (real desktop evidence / manual result handling); C: omit smoke (fast / no executable check). | A by default with `continue-on-error: false`. If the runner cannot host the UI, select `run_smoke=false`, run `PhotoReviewRs.exe --smoke` on the maintainer's machine and paste the result through `smoke_result` into release notes before publication. |

## Open items and not verified

- `crates/pr-shell/build.rs` and `build_support/res_writer.rs` remain W8 stubs. `assets/PhotoReviewRs.ico`, `crates/pr-shell/tests/exe_resources.rs`, root `LICENSE`, native pins/notice files and `THIRD-PARTY-NOTICES.md` (new, synced reviewed copy of the C# root file) do not exist at HEAD. The draft release workflow is **not runnable** before R2/R4 and W8-20a land.
- `cargo deny` is not installed here; its configuration schema and a safe pinned installer version are unverified. The proposed deny licence list comes from `cargo metadata --locked --offline` at this HEAD, not from a running deny audit.
- Rust release repository URL, code signing identity, LGPL corresponding-source sufficiency, TurboJPEG package hash/licence path, Windows desktop smoke availability, first-build exe size bounds and SmartScreen display are unverified. English is embedded (`crates/pr-l10n/src/localizer.rs:362`), while `crates/pr-l10n/src/loader.rs` reads shipped catalogs; shell composition for `<exe>/Languages` is still `todo!()` (`crates/pr-shell/src/composition/registry.rs:180-184`). Vietnamese selection in a built release is not verified (D-W10release-7).
- W8-20b UI smoke is a separate prerequisite; P-46 approved recommended W8-B6 options, while P-47 still proposes no RT_VERSION (`docs/PROPOSED-DECISIONS.md`, `contracts/README.md` D-W8B6-105).

## Paste-ready text for lead

For `docs/INDEX.md` (quoted proposal):

> | [`../contracts/W10-R1.md`](../contracts/W10-R1.md) .. [`../contracts/W10-R5.md`](../contracts/W10-R5.md) | W10 release, resources, dependency audit, native DLLs and coexistence contracts | Before release implementation |

For `contracts/README.md` (quoted proposal):

> ### Wave 10 release (contracts only)
> R1 workflow, R2 exe resources/version, R3 cargo-deny, R4 native DLL bundle and notices, R5 R0002 coexistence. Decisions D-W10release-1..8 and open items are in `contracts/drafts-w10-release.md`. Implement only after W8-17a/20a/20b dependencies and resolve P-47 before changing RT_VERSION.
