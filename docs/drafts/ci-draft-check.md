# Check of the CI drafts in `docs/drafts/` (DOC-CI-DRAFT-CHECK, machine C, cloud, 2026-10-23)

Files checked: `release.yml`, `ci-deny-job.yml`, `deny.toml`, and, because it is also a CI draft, the YAML block inside `raw-corpus-workflow.yml.md`. None of them is in `.github/workflows` and none was moved there.

## Results

| Check | Tool | Result |
|---|---|---|
| `release.yml` is valid YAML | PyYAML 6.0.3 `safe_load` | ok (top keys `name`, `on`, `permissions`, `concurrency`, `jobs`; `on` loads as `True`, the usual YAML 1.1 reading) |
| `release.yml` is a valid GitHub workflow | actionlint 1.7.12 (`actionlint-py`; shellcheck and pyflakes not installed, so embedded shell is not linted) | no findings |
| `ci-deny-job.yml` is valid YAML | PyYAML | ok (one key, `deny`) |
| `ci-deny-job.yml` as a job | actionlint on a temporary workflow that wraps the job with `on: pull_request, workflow_dispatch, schedule` | no findings |
| `deny.toml` is valid TOML | Python `tomllib` | ok (tables `graph`, `advisories`, `licenses`, `bans`, `sources`) |
| `deny.toml` keys are cargo-deny keys | by reading, `cargo-deny` not installed and not run | `version = 2` for advisories and licenses, `confidence-threshold`, `allow`, `exceptions`, `multiple-versions`, `wildcards`, `highlight`, `deny`, `skip`, `unknown-registry`, `unknown-git`, `allow-registry`, `allow-git`, `[graph] all-features/targets` are all names of the cargo-deny 0.14+ schema as I know it; **not verified by the tool** |
| RAW-corpus YAML block | PyYAML and actionlint on the extracted block | ok, no findings |
| Actions used exist | `git fetch` / `git ls-remote` of `github.com/actions/checkout` | `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1` is a commit and is the tag `v7.0.1` (also `v7`); the SHA is the same one this repo's `ci.yml` uses. `release.yml` and `ci-deny-job.yml` use only this action. `raw-corpus-workflow.yml.md` uses `actions/checkout@v4` (tag exists, `11d5960a...`), not SHA-pinned |

## Findings (nothing changed in the drafts)

1. `release.yml` calls `tools/fetch-release-native.ps1` and `tools/verify-release-native.ps1`; neither exists yet (W10-R4 creates them). It also runs `cargo test -p pr-shell --locked --test exe_resources`, a test file that does not exist yet (W10-R2). The draft therefore cannot run today, as `contracts/README.md` (Wave 10 release lane) already says.
2. `ci-deny-job.yml` ends its install step with a deliberate `throw` until a cargo-deny version is chosen (D-W10release-4). The job would fail on any run that needs the audit; it is a placeholder, not a working job.
3. `raw-corpus-workflow.yml.md` references `tools/stage-native.ps1`, `tools/fetch-raw-samples.ps1`, `tools/verify-raw-samples.ps1` (none exists) and sets `PHOTOREVIEW_RS_RAW_CORPUS_STRICT` and `PHOTOREVIEW_RS_RAW_CORPUS_DIR`. Per `docs/drafts/w10-review.md` (N-2, P-51) the Rust tests read two variable families and nothing reads `PHOTOREVIEW_RS_RAW_CORPUS_DIR`; before use the env block must follow P-51. It pins the action by tag (`@v4`), unlike the other drafts.
4. `deny.toml` `allow-registry` is the crates.io git index URL, which is how `Cargo.lock` spells the registry source (`registry+https://github.com/rust-lang/crates.io-index`); I did not check how the installed cargo-deny maps the sparse index.

## Not verified

- cargo-deny schema and behaviour (not installed; running it needs `cargo metadata`, which this session does not run).
- Embedded PowerShell and bash steps (no shellcheck/PSScriptAnalyzer).
- That the pinned SHA is the intended one for a release (it equals this repo's CI pin at the time of writing).
- Runner behaviour (non-interactive session and `--smoke`, see D-W10release-8).
