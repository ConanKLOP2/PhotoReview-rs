# W6-02c: Windows Recycle Bin facade and guarded send/delete operations

- Color: 🔴 | C# spec baseline: `d92d6aae` (SYNC-LEDGER baseline)
- Rust crate: `pr-platform-win`
- C# source: `src/PhotoReview.Platform.Windows/WindowsRecycleBin.cs`, `WindowsRecycleBin` public facade
- C# tests: `tests/PhotoReview.Integration.Tests/RecycleBinDeleteGuardTests.cs`, `WindowsRecycleBinFitsVerdictTests.cs`, plus refusal/send cases in `RecycleBinPlatformGapTests.cs`; architecture check `tests/PhotoReview.Architecture.Tests/RecycleBinContractTests.cs`
- Depends on: W6-02a eligibility and W6-02b capacity/settings; W0 `IRecycleBin`, `ILog`, settings abstractions.

## Owned Rust files

- `crates/pr-platform-win/src/recycle_bin.rs` (IRecycleBin implementation and action orchestration)
- `crates/pr-platform-win/tests/recycle_bin_facade.rs`
- `crates/pr-platform-win/tests/recycle_bin_contract.rs`
- `lib.rs` module wiring is lead-owned; do not edit it.

## Port scope

Implement the platform `IRecycleBin` facade, including `try_restore`, and preserve the C# fail-closed ordering. Define an
injected restore-backend trait in this unit; W6-02e implements that backend. The facade delegates restore calls through
the trait, so the dependency points from W6-02e to W6-02c and does not form a cycle.

1. `send_to_recycle_bin` validates the path, rejects a volume that cannot recycle, re-reads current file size immediately before Shell invocation, evaluates capacity, invokes the injected recycle operation once, then verifies the source path is gone. A size-read failure or non-Fits verdict refuses before Shell.
2. `can_recycle` and `fits_in_recycle_bin` delegate to W6-02a/W6-02b and preserve logging and false-on-unsafe behavior.
3. `delete_permanently` requires explicit positive no-bin eligibility from W6-02a, invokes only the injected delete operation, and reports failure if the source still exists. It must never become a fallback for failed recycle on a fixed/unknown volume.
4. `try_restore` implements the Core trait method by delegating to the injected restore backend, preserving its `io::Result<bool>` outcome unchanged.
5. Every production `IRecycleBin` implementation must explicitly implement the safety-sensitive trait methods (C# architecture invariant B-03).

Use injectable filesystem/Shell/settings seams so tests cannot reach real deletion paths. Keep capacity grouped preflight (`pr_core::file_actions::RecycleBinCapacity::FirstOverflow`) in Core, outside this platform unit.

## Verification and absolute safety boundary

- Port named guard tests for no-shell-on-refusal, stale preflight size/growth, unreadable size, shell returning while source remains, successful fake shell removal of a test-owned temp file, and permanent-delete refusal/survival. Reuse fake interfaces; no test may invoke the real Shell recycle operation or real permanent deletion.
- Add an architecture test that proves at least one production implementation is discovered and that all production implementations declare `can_recycle`, `fits_in_recycle_bin`, and `delete_permanently` directly.
- Mutation expectations: the tests must fail if any precondition is skipped, the size is not re-read, a failed shell call is treated as success, or unknown/fixed volumes can reach permanent delete. Never run mutation testing against a real Recycle Bin path.
- No ignored/native test in this unit may send/delete a user's item. Native-bin tests belong only to W6-02e and are subject to its stricter exact-item cleanup rules.
- All tests are deterministic and bounded; no fixed-delay asserts or polling loops.

## Completion gate

Pass workspace fmt, clippy, and tests on the final integrated head. Any `--include-ignored` run must exclude Recycle Bin mutation scenarios unless they meet W6-02e's own-item safeguards; never run a sweep/empty operation. Update PARITY with fake-only verification evidence and identify native gaps honestly.
