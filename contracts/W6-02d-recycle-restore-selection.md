# W6-02d: Recycle item matching and restore-verb selection

- Color: 🔴 | C# spec baseline: `d92d6aae` (SYNC-LEDGER baseline)
- Rust crate: `pr-platform-win`
- C# source: `src/PhotoReview.Platform.Windows/WindowsRecycleBin.cs`, `RecycleItemFailure`, `RecycleCandidateSelector`, and `RestoreVerb`
- C# tests: `tests/PhotoReview.Integration.Tests/WindowsRecycleBinCandidateTests.cs` and `WindowsRecycleBinRestoreVerbTests.cs`
- Depends on: none for pure matching/verb logic; W6-02e consumes these helpers.

## Owned Rust files

- `crates/pr-platform-win/src/recycle_candidate.rs`
- `crates/pr-platform-win/src/restore_verb.rs`
- `crates/pr-platform-win/tests/recycle_candidate.rs`
- `crates/pr-platform-win/tests/restore_verb.rs`
- `crates/pr-platform-win/src/lib.rs` only for module declaration/re-export.

## Port scope

Port pure candidate matching: original deleted-from folder and display name (including Explorer's extension-hidden display form), exact expected size, and either accepted timestamp representation with the source's sub-second tolerance. Preserve the rule that ambiguous matches do not select an item. Port the per-item exception classification (`COM`, invalid cast, format only) so an unreadable individual candidate can be skipped while unrelated errors propagate.

Port restore label normalization and localized-name matching plus canonical `undelete` fallback. Invoke exactly one route: first recognized localized verb by index, otherwise canonical verb. Do not try several routes after a failure.

## Verification

- Port each named candidate test, including mismatched folder/name/size/time, alternate timestamp, hidden extension and multi-dot name, ambiguity, and per-item vs unrelated exception.
- Port verb tests for known multilingual labels, non-restore verbs, accelerator removal, first match, recognized-index route, and canonical fallback.
- Tests use synthetic records and callbacks only; do not enumerate the real Recycle Bin or invoke a Shell verb.
- Mutation expectations: tests must kill weakened path/size/time matching, accepting ambiguous candidates, classifying unrelated exceptions as skippable, changing localized labels/normalization, or invoking more than one route. Note justified equivalent mutants.
- No sleeps or unbounded test runs.

## Completion gate

Pass `cargo fmt --all --check`, workspace clippy, and workspace tests at final integration. Include the Rust test names corresponding to the C# tests in PARITY. Native COM validation is owned by W6-02e.
