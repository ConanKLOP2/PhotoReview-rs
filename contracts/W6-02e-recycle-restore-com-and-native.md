# W6-02e: Shell restore orchestration and bounded native integration

- Color: 🔴 | C# spec baseline: `d92d6aae` (SYNC-LEDGER baseline)
- Rust crate: `pr-platform-win`
- C# source: `src/PhotoReview.Platform.Windows/Shell/RecycleBinShell.cs`; restore entry points and verification helpers in `src/PhotoReview.Platform.Windows/WindowsRecycleBin.cs`
- C# tests: restore delegation/proof cases in `tests/PhotoReview.Integration.Tests/WindowsRecycleBinRestoreGuardTests.cs`; bounded behavior in `RecycleBinPlatformGapTests.cs`; native twins `tests/PhotoReview.App.Tests/HotPath/NativeRecycleBinTests.cs` and its exact-folder helper `TestRecycleBinCleanup.cs`.
- Depends on: W6-02c facade, W6-02d pure selector/verb helpers, and W6-02a/b volume/settings support as required by construction.

## Owned Rust files

- `crates/pr-platform-win/src/recycle_bin_shell.rs` (COM/Shell adapter and restore enumeration)
- `crates/pr-platform-win/src/recycle_restore.rs` (restore delegate and proof polling, if kept separate from facade)
- `crates/pr-platform-win/tests/recycle_bin_restore.rs`
- `crates/pr-platform-win/tests/recycle_bin_native.rs` (ignored Windows-only test module)
- module declarations in `crates/pr-platform-win/src/lib.rs` only.

## Port scope

Keep all COM `unsafe` in `pr-platform-win`, with a `// SAFETY:` comment for every unsafe block. Model the C# session/entry/verbs ownership with RAII and release each COM object exactly once. Enumerate item metadata, use W6-02d matching, require exactly one candidate, open its context menu, and invoke exactly one localized or canonical restore route. Refuse before enumeration if the original path already exists to avoid the Shell's modal replace/rename prompt. Confirm completion by size AND UTC write-time match, preserving the C# bounded retry semantics; keep restore work off the UI thread as required by R0003.

### Recycle Bin safety rules (absolute)

- Default verification is fake-only: injected session, entries, verbs, clock/check seam, and temp filesystem. It must not read, write, enumerate, delete, sweep, or empty the user's real Recycle Bin.
- If native tests are retained, mark them ignored with a `native` reason and use a unique per-test source folder and uniquely created files. Cleanup may remove only bin entries whose `DeletedFrom` metadata equals that exact folder (ordinal-ignore-case equality; no prefix, wildcard, time, or filename-only matching). It may delete only the corresponding item payload/metadata pair proved to belong to those test-created entries.
- Native tests must never enumerate-and-delete all bin items, clear the bin, remove unrelated entries, or invoke cleanup with a broad/root folder. If exact ownership cannot be established, leave the item and report cleanup failure; do not broaden cleanup.
- Never run native-bin tests or their mutation checks against the user's bin. Mutation checks target fake-backed paths only.

## Verification

- Port `TryRestore` existing-target refusal/no-shell and free-target delegation; size-plus-time restore proof; bounded final-check behavior; fake session enumeration/disposal; no-match, ambiguous-match, per-item skip, unrelated error propagation; exactly-one verb invocation.
- Fake tests must assert the session, entry, and verb objects are disposed even on early return/error. Mutation expectations include existing-file precheck, unique candidate requirement, size/time proof, and exactly-once verb invocation.
- Native integration is optional evidence and must be ignored/bounded; do not assert a user's bin capacity or pre-existing contents. A native test can operate only on its own created files and must clean up only those exact items.
- No fixed-delay asserts or unbounded polling. Any wait uses the source's finite attempt bound, and test seams shorten it without timing-based pass criteria.

## Completion gate

Pass final workspace fmt, clippy, and tests. Run native checks only on an isolated Windows environment where the native dependency is staged and the exact-item safeguards above are verified. Otherwise report native COM behavior as unverified; do not claim full native parity. Update PARITY with fake coverage and the native evidence actually obtained.
