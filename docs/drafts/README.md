# Drafts: proposed files not yet applied

These files are proposals. None of them is in `.github/workflows` or the repo root, and none may be copied there without the decision named below. Check reports on them live in [`../archive/drafts/`](../archive/drafts/) (`ci-draft-check.md`: syntax and action check, 2026-10-23).

| File | Purpose | Proposed by | Decision needed to apply it |
|---|---|---|---|
| [`release.yml`](release.yml) | Manual release workflow for `PhotoReviewRs.exe` (existing tag, green `build-test`, one native exe, zip and SHA256) | #172, contract `W10-R1` (target `.github/workflows/release.yml`) | D-W10release-1..8 in `contracts/README.md` (Wave 10 release lane: tag scheme, signing, RT_VERSION, update feed, LibRaw source, `Languages`, runner smoke). Cannot run before `W10-R2` (`exe_resources` test) and `W10-R4` (`fetch-release-native.ps1`, `verify-release-native.ps1`) exist. |
| [`ci-deny-job.yml`](ci-deny-job.yml) | `deny` job fragment for `ci.yml` plus a nightly schedule | #172, contract `W10-R3` | D-W10release-4 (how to install cargo-deny: pinned `cargo install --locked`, pinned action SHA, or no CI audit). The install step ends in a deliberate `throw` until a version is chosen. |
| [`deny.toml`](deny.toml) | cargo-deny configuration (advisories, licences, bans, sources); licence list derived from `cargo metadata --locked --offline` | #172, contract `W10-R3` (target `deny.toml` in the repo root) | D-W10release-4, and a real run: the schema is untested because cargo-deny was not installed (`cargo deny --config docs/drafts/deny.toml check advisories licenses bans sources`). |
| [`raw-corpus-workflow.yml.md`](raw-corpus-workflow.yml.md) | Manual RAW corpus workflow (YAML inside Markdown on purpose) porting the C# `raw-corpus.yml` concept | #173, contract `W10-N3` | Owner consent to download the corpus; P-51 (variable names: the env block must follow it, nothing reads `PHOTOREVIEW_RS_RAW_CORPUS_DIR`); the new scripts `tools/stage-native.ps1`, `tools/fetch-raw-samples.ps1`, `tools/verify-raw-samples.ps1` must exist (not yet in the repo); pin `actions/checkout` by SHA instead of `@v4`. |

Output folders that contracts name under `docs/drafts/` (`bench-attended/`, `bench-parity/`, contracts `W10-B2`, `W10-B3`) do not exist yet; they are created when the attended benchmark runs are recorded.
