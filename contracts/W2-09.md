# W2-09: ExplorerSnapshotValidator (INV-7)
- Color: 🟡 | Porter: **sonnet** (error-class routing of .NET path exceptions, extended-length prefix, drive-root quirk) | Test porter: haiku | Verifier: sonnet
- C# source (baseline d92d6aae): `src/PhotoReview.Core/Catalog/ExplorerSnapshotValidator.cs` (132 lines)
  (read with `git -C D:\MyProject\PhotoReview show d92d6aae:src/PhotoReview.Core/Catalog/ExplorerSnapshotValidator.cs`)
- C# tests to port (all at d92d6aae, `tests/PhotoReview.Core.Tests/Catalog/` unless noted):
  - `ExplorerSnapshotValidatorTests.cs` (120 lines): `TryValidateUnavailableStatusReturnsFalseWithReason`, `TryValidateValidSnapshotReturnsTrueAndOrderedList`,
    `TryValidateMissingExpectedFileReturnsFalse`, `TryValidateDuplicateItemReturnsFalse`, `TryValidatePathOutsideFolderReturnsFalse`, `TryValidateEmptyCandidatePathReturnsFalse`,
    `CanonicalizeFolderAndSamePathWorkCorrectly` (use `C:\photos`). NOT `ProviderContractIsFakeableWithoutCom` (needs `ExplorerOrderProvider`, W6-01).
  - `ExplorerSnapshotValidatorEdgeCaseTests.cs` (228): `NonAvailableStatusIsRejectedWithItsReasonOrStatusName` (theory, 6 rows), `EquivalentSpellingsOfTheSameFileAreAccepted` (7 rows),
    `PathsThatAreNotDirectChildrenAreRejectedWithTheirReasonAndNeverThrow` (9 rows), `ScannedFileWithInvalidCharacterIsRejectedInsteadOfThrowing`, `DuplicatesDifferingOnlyByCaseAreRejected`,
    `DuplicateOfAnUnscannedFileIsStillRejected`, `UnscannedNonImageEntriesAreIgnoredButOrderOfTheRestIsKept`, `ScannedFileMissingFromSnapshotIsIncomplete`, `EmptySnapshotIsValidOnlyForAnEmptyScan`,
    `SnapshotFolderSpellingDoesNotChangeTheVerdict` (4 rows), `DriveRootFolderIsSupported`, `UncFolderIsSupportedInBothSpellings`, `LongPathsBeyondMaxPathAreSupported`,
    `NormalizePathStripsExtendedPrefixOnly` (4 rows), `AnyPermutationValidatesToItself` (deterministic xorshift RNG), `AnySingleMutationIsDetected`, `CanonicalizeFolderIsIdempotent`
  - `ExplorerSnapshotValidatorRobustnessTests.cs` (132): `InvalidCandidatePath_ReturnsFalse` (3 rows), `InvalidScannedPath_ReturnsFalse`, `NullPieces_ReturnFalse` (only the rows with a
    folder: `""`, `"   "`; the `null` folder and the `null` path list have no Rust twin, say so), `EquivalentSpellings_Validate`, `ExtraFiles_Ignored`, `Fuzz_PermutationOracle` (seeds 1-3).
    The last two tests of that file call `ReviewCatalog` and are ported in W2-10: `ExtendedLengthScannedPaths_AreReturnedVerbatimAndAcceptedByCatalog`, `ValidatorOutput_IsAcceptedByCatalog`.
  - `ExplorerSnapshotValidatorEquivalenceTests.cs` (44) + `ExplorerSnapshotValidatorReference.cs` (121): `MatchesOriginal_OnMutatedSnapshots`. Port the Reference class as a private test-only fn
    set (`reference_try_validate`, it is a verbatim copy of the pre-optimisation algorithm, 121 lines) and run the 3000-iteration loop with a deterministic RNG. NOTE: the C# test builds the
    odd folder spelling `@"\?\" + folder` (ONE backslash, a relative-looking path) on purpose or by accident; keep it.
  - `CatalogMutationGapTests.cs`: `ExplorerSnapshotValidator_NullSnapshot_IsAnIncompleteSnapshotNotACrash` and `..._NullScannedFiles_...` (no Rust twin: skip with reason),
    `NormalizePath_StripsTheExtendedPrefixOnlyFromDriveLetterPaths` (7 rows)
  - `tests/PhotoReview.App.Tests/CacheExplorerRegressionTests.cs`: `ExplorerValidatorPreservesCompleteNativeOrder`, `ExplorerValidatorRejectsDuplicateNativeItems`,
    `ExplorerValidatorRejectsItemOutsideFolder`, `ExplorerValidatorRejectsIncompleteSnapshots`, `ExplorerValidatorFallsBackOnMalformedNativePaths` (reasons contain `duplicate` / `outside` / `complete`;
    `"not-a-path"` resolves against the process directory => outside folder)
  - NOT here: `ExplorerSnapshotValidatorBenchmarkTests` (manual benchmark, no behaviour), `tests/PhotoReview.Integration.Tests/ExplorerOrderServiceTests.cs` (W6-01).
  - Golden test (new, required): `tests/golden/snapshot-validator.json` (105 cases: 69 `validate`, 23 `normalize`, 13 `same_path`).
- PARITY rows: P-01 (Explorer view order: validation), INV-7 (late/invalid Explorer snapshots never reorder the catalog).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/catalog/snapshot_validator.rs   (stub: contracts-w2/W2-09/snapshot_validator.rs, compiles clippy-clean)
    crates/pr-core/tests/catalog_snapshot_validator.rs
- Read-only inputs: `tests/golden/snapshot-validator.json`. Lead-owned: `catalog/mod.rs` gets `pub mod snapshot_validator;`.
- Public API (stub, do NOT change signatures): `try_validate(&ExplorerViewSnapshot, &[String]) -> Result<Vec<String>, String>` (Err = the reason text), `canonicalize_folder`, `normalize_path`, `same_path`
  (all `Result<_, PathError>`).
- Semantics notes (every item is a golden case; the stub doc has the 4 steps):
  1. **Reason routing.** The ONLY outcome that depends on WHICH .NET call threw: errors in step 2 (snapshot folder, any scanned path) and in `normalize_path(candidate)` are `outside-folder`; an error inside
     `in_folder` escapes to the outer handler and becomes `incomplete-snapshot`. That happens for a candidate that is a root: `C:\`, `\\?\C:\`, `\\srv\share` have no directory, `get_directory_name_raw` gives `""`,
     `same_path("", folder)` fails (`GetFullPath("")`) => `incomplete-snapshot`, NOT `outside-folder` (golden `reject-drive-root-candidate-is-incomplete-not-outside`, `reject-unc-root-...`,
     `reject-extended-drive-root-candidate`). A blank/NUL scanned path or folder is `outside-folder`. An empty/whitespace candidate is `empty-path` (checked before normalizing).
  2. **Order of checks per candidate**: empty-path, normalize (outside), in-folder (outside), duplicate, membership. So a duplicate AFTER an outside item reports `outside-folder`; a duplicate of an unscanned file is
     `duplicate-item`. Case-variant duplicates are duplicates. Unscanned entries are skipped but their relative order does not matter; `nonempty snapshot + empty scan` is VALID with an empty result (golden).
  3. **`expected` keeps the FIRST scanned string** per normalized key; the result holds the scanned strings (so a catalog that kept `\\?\` keeps accepting the order), never the normalized spelling.
     Two scanned spellings of one file count once (`expected.len()` = number of distinct keys).
  4. **`normalize_path`**: see the stub doc. The golden pins: `\\?\C:` => `C:`, `\\?\1:\x` => `1:\x` (only `full[5] == ':'` is checked, not a letter), `\\?\unc\srv\share\a.jpg` (lower case) => `\\srv\share\a.jpg`,
     `\\?\UNC\` => `\\`, `\\?\x` and `\\?\Volume{1}\a.jpg` unchanged, `\\?\C:\a\..\b` => `C:\a\..\b` (dots NOT resolved because `get_full_path` leaves `\\?\` paths alone), `\\.\D:\a\..\b` => `\\.\D:\b`.
  5. **`in_folder`**: the cheap test is `get_directory_name_raw(path)` equals the canonical folder (OrdinalIgnoreCase); the fallback `same_path(directory, folder)` makes `\\?\C:\photos\x\..` (folder, unresolved dots) still
     accept `C:\photos\a.jpg` (golden `folder-extended-with-dotdot-is-not-resolved`). Do not drop the fallback.
  6. **GetFullPath side effects visible in the verdict**: a candidate with a trailing space or dot (`C:\photos\a.jpg `, `a.jpg.`) equals the scanned `a.jpg`; `a.jpg:stream` is a different name (=> `incomplete-snapshot`).
  7. **Performance (AGENTS priority 3):** 50 000-file snapshots; one `HashMap<String, usize>`/`HashSet<String>` keyed by `ignore_case_key`, no per-item `Vec` allocations beyond the keys, `result` pre-sized.
- Traps checklist: §7 items 1, 7, 9 (each `catch (X) when` is a distinct `match`/`?` route, see note 1), 8 (`reason: None` vs `Some("")` of the snapshot: the empty string is kept), 10 (result order = snapshot order).
- Depends on (functions called): W2-00 `dotnet_path::{get_full_path, get_directory_name_raw, trim_ending_directory_separator}` (MUST be merged first); W1-07 `explorer_reason::*` constants, `explorer_types::{ExplorerViewSnapshot,
  ExplorerOrderStatus}` (already merged in Wave 0); `text::{eq_ignore_case, ignore_case_key}`. Tests call only this unit's functions (the two tests that need `ReviewCatalog` live in W2-10's test file).
- Done when: each listed test has a Rust twin (`// C#: Class.Method`); the golden passes (`validate`: `valid`, `reason` (null <=> `Ok`), `ordered`; `normalize` and `same_path` incl. `{"throws":"ArgumentException"}` <=> `Err(PathError::Argument)`); the
  Reference-equivalence loop passes; clippy clean; stub's `#![allow]` line removed.

## Golden package
- `oracle/W2-09.cs`: command `snapshot-validator` (builds an `ExplorerViewSnapshot` with any `ExplorerOrderStatus`/`Reason`, plus `NormalizePath`/`CanonicalizeFolder`/`SamePath` tables). Needs `W2-00.cs`.
  Dispatch line in the header. `golden-inputs/snapshot-validator.json`, `golden-reference/snapshot-validator.json`.
- Case design: one verdict reason per case; each status; each of 8 equivalent spellings for the file and 7 for the folder; root candidates; NUL/blank/empty folder; long paths (120+120+60 char segments built so folder and file agree);
  scanned spelling duplicates and `\\?\` returned verbatim.

## Open questions
- `ExplorerViewSnapshot.ordered_paths` is a plain `Vec<String>` in Rust, so the C# "`OrderedPaths is null` => incomplete-snapshot" branch is unreachable; the contract keeps it as a documented non-port.
