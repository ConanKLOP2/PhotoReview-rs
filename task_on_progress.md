# Task on progress

**Updated:** 2026-10-14

## State
- Phase 0 started. Repo created locally at `D:\MyProject\PhotoReview-rs` (not on GitHub yet).
- Done: Cargo workspace (10 crates, empty except `pr-core`), `pr_core::app_paths` (port of `AppPaths.cs`, root
  `PhotoReviewRs`) + `pr_core::identity` with unit tests, CI workflow, ADR R0001 (proposed) / R0002 (accepted),
  `tools/sync-from-csharp.ps1` (assets synced from C# `e441d67a`, verified byte-exact), PLAN/PARITY/SYNC-LEDGER docs.
- **Not yet compiled:** no Rust toolchain or MSVC Build Tools on this machine at creation time. Nothing here has been
  built or tested yet.

## Next
1. Install rustup (MSVC) + VS Build Tools (C++ workload); `cargo test --workspace`, `cargo clippy ... -D warnings`; fix.
2. Commit `Cargo.lock`, add `--locked` back to CI, pin the exact toolchain in `rust-toolchain.toml`.
3. Create the GitHub repo, push, check CI.
4. PARITY.md inventory is in (about 245 rows); next: UI spikes (R0001); COM spike; ADR R0003 concurrency.
