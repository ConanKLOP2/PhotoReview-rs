# W6-02b: Recycle Bin settings and capacity verdict

- Color: 🔴 | C# spec baseline: `d92d6aae` (SYNC-LEDGER baseline)
- Rust crate: `pr-platform-win`
- C# source: `src/PhotoReview.Platform.Windows/RecycleBinCapacity.cs` (`RecycleBinCapacityGuard`, `WindowsRecycleBinSettingsSource`)
- C# tests: `tests/PhotoReview.Integration.Tests/RecycleBinCapacityGuardTests.cs`, `WindowsRecycleBinFitsVerdictTests.cs`, and the registry/volume sections of `RecycleBinPlatformGapTests.cs`
- Depends on: W0 `ILog`, `IRecycleBinSettingsSource`/`IRecycleBin`. This unit owns the shared volume/mount-point query adapter consumed by W6-02a; it does not depend on W6-02a.

## Owned Rust files

- `crates/pr-platform-win/src/recycle_bin_settings.rs`
- `crates/pr-platform-win/src/recycle_bin_capacity.rs`
- `crates/pr-platform-win/tests/recycle_bin_settings_capacity.rs` (registry/volume adapter and capacity verdict cases)
- `lib.rs` module wiring is lead-owned; do not edit it.

## Port scope

Port the settings value types, registry reader, volume GUID/mount-point/quota queries, and fail-closed capacity evaluator. Preserve policy semantics: `NoRecycleFiles`, legacy global and per-volume `NukeOnDelete`, per-volume `MaxCapacity`, fallback 5% of user quota when no volume entry exists, stricter user/machine `RecycleBinSize` cap, configured safety margin, and `Unknown` on unusable/missing data. Preserve DWORD unsigned interpretation and the C# rounding/units exactly; verify numeric formulas against this pinned source before implementation.

Do not include `pr_core::file_actions::RecycleBinCapacity::FirstOverflow` here. That separate Core grouping preflight already belongs to prior-wave code/tests and stays the higher-level caller; this unit implements the Windows-specific settings/verdict service.

## Verification

- Port every named capacity verdict scenario, including fixed vs non-fixed/unresolvable volume, disabled policies, negative/unknown size, capacity boundary, zero capacity, absent volume entry/quota fallback, policy cap, registry type errors, and GUID parsing.
- Registry and API dependencies are injected. Default native adapters are read-only: they may read registry/volume metadata and must never change registry settings or delete/move any item.
- Keep deterministic tests fake-only. A read-only native volume smoke check, if retained, must be ignored with a `native` reason and assert only facts independent of user bin contents/configuration; do not require the user's configured capacity or bin state.
- Mutation expectations: tests must kill changes to fail-closed behavior, min(policy caps), fallback percentage, safety margin boundary, and DWORD conversion. Record any equivalent/unreachable mutants with rationale.
- Never poll via fixed sleeps; put all test runs under a bounded CI timeout.

## Completion gate

Final integrated head passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked`. On Windows, include the bounded `--include-ignored` native adapter check only if it remains user-data-independent and strictly read-only. Update the Windows Recycle Bin PARITY evidence in the implementation PR.
