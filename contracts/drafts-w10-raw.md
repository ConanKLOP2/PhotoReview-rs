# Wave 10 RAW/native contract draft roster

Proposed additions to `contracts/README.md` (do not apply here):

| WU | Unit | Rust area | Porter | Evidence |
|---|---|---|---|---|
| W10-N1 | Stage pinned native DLLs | `native/x64/` staging procedure | Sonnet | Hash verification; no committed DLLs |
| W10-N2 | Execute native tests | `pr-libraw`, `pr-turbojpeg` ignored native tests | Sonnet | Windows x64, pinned DLLs, timeout |
| W10-N3 | RAW corpus and strict run | `pr-test-support::raw`, RAW integration tests | Sonnet | Consent, manifest hashes, strict mode |
| W10-N4 | Wave 5 mutation survivors | W5 Rust test suites | Sonnet | Identity-reconciled mutation evidence |

Proposed decisions D-W10RAW-1..4 (drafts only; apply to README after review):

- **D-W10RAW-1 — DLLs stay local and pinned.** Stage only SHA-256-verified C# baseline DLLs under Rust `native/x64/`; never commit DLLs. Network fetch requires owner consent.
- **D-W10RAW-2 — Native run is desktop/manual.** Run ignored LibRaw/TurboJPEG suites on Windows x64 with explicit timeout and serial test threads. Regular CI does not execute them unless it stages verified binaries.
- **D-W10RAW-3 — Corpus fetch requires consent.** The approximately 590 MB CC0 corpus remains absent until the owner explicitly approves the fetch; strict mode makes missing samples fail.
- **D-W10RAW-4 — Mutation status follows identities.** Preserve caught/equivalent/unreconstructed distinctions; do not combine overlapping counts or describe partial scans as complete.

References: contracts/W10-N1.md through W10-N4.md. Hash pins and limitations are transcribed there.
