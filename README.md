# PhotoReviewRs

Rust rewrite of [PhotoReview](https://github.com/ConanKLOP2/PhotoReview), a Windows photo review app (Explorer order,
fast switching, Compare, recoverable Move/Copy/Delete with Undo, camera RAW). Goal: identical behaviour, at least
equal speed. The C# app stays in use alongside this one; the two never share data (see `docs/adr/R0002`).

**Status:** under construction, not usable yet (Waves 1 to 9 are on master (except the conditional W8-12j) and Wave 10 code is merged, but parity is not closed (see `bash tools/dev/parity.sh stats`), and attended measurements, the RAW corpus and the release review are open; the two-week trial comes later, see [`docs/TRIAL-PLAN.md`](docs/TRIAL-PLAN.md)). Current state: [`task_on_progress.md`](task_on_progress.md). Plan: [`docs/PLAN.md`](docs/PLAN.md).

## Requirements

- Windows 10/11 x64
- Rust (rustup, MSVC toolchain); `rust-toolchain.toml` selects the version
- Visual Studio Build Tools with "Desktop development with C++" (MSVC linker + Windows SDK)

## Build and test

```powershell
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Shared assets

Language catalogs and test fixtures come from the C# repo:

```powershell
./tools/sync-from-csharp.ps1 -CSharpRepo ..\PhotoReview -Commit origin/master
./tools/sync-from-csharp.ps1 -Check
```
