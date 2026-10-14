# Task on progress

**Updated:** 2026-10-14

## State
- Phase 0 in progress. Repo: https://github.com/ConanKLOP2/PhotoReview-rs (public), local `D:\MyProject\PhotoReview-rs`.
- On master: Cargo workspace (10 crates, empty except `pr-core`), `pr_core::app_paths` (port of `AppPaths.cs`, root
  `PhotoReviewRs`) + `pr_core::identity`, CI, ADR R0001 (proposed) / R0002 (accepted), `tools/sync-from-csharp.ps1`
  (assets from C# `e441d67a`, byte-exact, `.gitattributes` keeps them so), PLAN/PARITY (about 245 rows)/SYNC-LEDGER.
- Branch `chore/pin-toolchain` (PR open, not merged when written): toolchain pinned to Rust 1.99.0, `Cargo.lock`,
  `--locked` in CI, rustfmt applied. Local results: `cargo fmt --check` OK, `cargo clippy --workspace --all-targets
  -D warnings` 0 warnings, `cargo test --workspace` 11/11 pass, release build runs. Mutation check: renaming
  `APP_DIR_NAME`/`INSTANCE_PREFIX` to `PhotoReview` fails 6 tests.
- Machine: rustup + VS 2022 Build Tools (C++ workload) installed 2026-10-14.

## Next
1. Merge the toolchain PR; confirm CI green on master.
2. UI spikes egui vs Slint per R0001 protocol (measure switch p50/p95 against C# Benchmark.Cli on the same fixture).
3. COM spike: Explorer order for a ~1800-file folder (target ~60 ms like C#).
4. ADR R0003 concurrency; then Phase 1 (Core) starting with Settings (S-* rows in PARITY.md).
