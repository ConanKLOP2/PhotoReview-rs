# W6-02a: Recycle eligibility and permanent-delete guard

- Color: 🔴 | C# spec baseline: `d92d6aae` (SYNC-LEDGER baseline)
- Rust crate: `pr-platform-win`
- C# source: `src/PhotoReview.Platform.Windows/WindowsRecycleBin.cs`, `RecycleEligibility`
- C# tests: `tests/PhotoReview.Integration.Tests/RecycleEligibilityTests.cs`; eligibility cases in `RecycleBinPlatformGapTests.cs`
- Depends on: W0 abstractions (`IRecycleBin`) and W6-02b's shared volume/mount-point query adapter. W6-02b does not depend on this policy unit, so the dependency graph remains acyclic.

## Owned Rust files

- `crates/pr-platform-win/src/recycle_eligibility.rs`
- `crates/pr-platform-win/tests/recycle_eligibility.rs`
- `lib.rs` module wiring is lead-owned; do not edit it.

## Port scope

Port `RecycleEligibility.CanRecycle`, `IsPermanentDeleteAllowed`, `IsRealUnc`, and `QueryDriveType` policy using W6-02b's shared volume/mount-point query adapter. A recyclable path is accepted only when the resolved volume is positively `Fixed`; permanent delete is allowed only for a real UNC path or a positively identified `Removable`, `Network`, `CDRom`, or `Ram` volume. Unknown types, failed lookups, invalid paths, and unresolved mount points fail closed. Preserve extended drive-letter handling and distinguish real UNC forms from volume-GUID/device paths as in C# R44.

This unit owns the policy decision only. The IRecycleBin facade's filesystem and Shell actions belong to W6-02c. Do not infer a permanent-delete permission from an unknown drive result.

## Verification

- Mirror the named `RecycleEligibilityTests` scenarios: fixed/removable/network types; junction/mount-point resolution; malformed paths; drive-letter and extended drive-letter forms; UNC, volume GUID and device paths; and mount/drive query failures.
- All policy tests use injected delegates and synthetic paths. They must not call the real Shell, mutate a user's Recycle Bin, or delete files.
- Include boundary cases for both `CanRecycle` and `IsPermanentDeleteAllowed`; assert the resolver receives the normalized expected root/mount point where applicable.
- Mutation expectations: changing fixed-only acceptance, treating unknown as recyclable, widening the UNC recognizer, or allowing permanent delete on `Fixed`/`Unknown` must be killed by tests. If a mutant is equivalent or needs elevated privileges, document the exact reason and leave it as an explicit gap.
- Bound all test commands with CI job/step timeouts; no sleep polling.

## Completion gate

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` pass on the final integration head. This policy unit's tests are fake-only. The native Windows check for the shared volume/mount-point adapter is owned by W6-02b and remains read-only. Update the matching PARITY entry in the implementation PR.
