# Manual RAW corpus workflow proposal

Proposal only; this YAML is intentionally in a Markdown document and must not be copied into `.github/` without review, explicit corpus consent, and the repository's normal workflow approval.

This ports the C# baseline `raw-corpus.yml` concept to Rust: manual dispatch only, Windows x64 runner, strict corpus variables, hash-verified fetch, bounded native/corpus execution. It assumes staging and corpus fetch scripts exist and have been reviewed. The proposed fetch step is a future workflow action: owner consent is required before enabling/download execution.

```yaml
name: RAW corpus tests (manual proposal)

on:
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: raw-corpus-${{ github.ref }}
  cancel-in-progress: true

jobs:
  raw-corpus-tests:
    runs-on: windows-latest
    timeout-minutes: 45
    env:
      PHOTOREVIEW_RS_RAW_CORPUS_STRICT: "1"
      PHOTOREVIEW_RS_RAW_CORPUS_DIR: tests/fixtures/raw-corpus
      CARGO_PROFILE_DEV_DEBUG: line-tables-only
      CARGO_INCREMENTAL: "0"
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install pinned Rust toolchain
        run: rustup show

      - name: Stage hash-verified native DLLs
        shell: pwsh
        run: ./tools/stage-native.ps1 -SourceCheckout $env:PHOTO_REVIEW_CSHARP_CHECKOUT

      # Enable only after owner consent and review of the exact data source and license checks.
      - name: Fetch and verify consented RAW corpus
        shell: pwsh
        run: ./tools/fetch-raw-samples.ps1 -ConsentAcknowledged

      - name: Verify complete corpus manifest
        shell: pwsh
        run: ./tools/verify-raw-samples.ps1 -Strict

      - name: Run strict corpus and native tests
        shell: pwsh
        run: cargo test -p pr-libraw -p pr-turbojpeg -p pr-imaging-raw --locked -- --ignored --test-threads=1
```

Commentary: action versions, toolchain setup, staging/fetch/verification script interfaces, external per-command timeouts and test-binary selection are proposals requiring verification. This workflow downloads nothing in this document and must not be placed in `.github/` by this task. The `PHOTO_REVIEW_CSHARP_CHECKOUT` variable and script names are placeholders requiring design review.
