# W2-07: ComparePairService + DuplicateFinder (compare pairs, duplicate cleanup decisions)
- Color: 🟡 (DuplicateFinder feeds a recycle batch: a wrong survivor rule recycles a whole content group) | Porter: **sonnet** (hand-written .NET regex semantics,
  `int.TryParse`, Unicode digits, stable multi-key sorts, `Distinct`/`GroupBy` order) | Test porter: haiku | Verifier: sonnet
- C# sources (baseline d92d6aae):
  - `src/PhotoReview.Core/Catalog/ComparePairService.cs` (146 lines) -> `crates/pr-core/src/catalog/compare.rs`
  - `src/PhotoReview.Core/FileActions/DuplicateFinder.cs` (120 lines) -> `crates/pr-core/src/file_actions/duplicates.rs`
- C# tests to port (all at d92d6aae; `tests/PhotoReview.Core.Tests/`):
  - Catalog/`ComparePairServiceTests.cs` (135 lines): `FindMatchesNumberedVariantWithOriginal`, `FindReturnsNullWhenNoPairExists`, `FindStaysWithinSelectedFolder`,
    `FindRejectsSingleFileList`, `BuildIndexMatchesFindForEveryPath`, `BuildIndexPairsOriginalWithSmallestNumbered`, `BuildIndexOmitsUnpairedOriginal`,
    `BuildIndexMatchesFindWithMixedCase` (theory, 2 rows)
  - Catalog/`ComparePairFuzzTests.cs` (47): `BuildIndex_MatchesFind` (theory seeds 1-5; deterministic xorshift RNG in the test; the `ToLowerInvariant` comparisons of folder and
    extension become `ignore_case_key` comparisons; `Equals(found, indexed)` is exact tuple equality)
  - Catalog/`ComparePairTieOrderTests.cs` (55): `BuildIndex_SpellingsOfOneFullPath_KeepInputOrder`, `BuildIndex_PathsDifferingOnlyByCase_OrdinalOrderPicksTheOriginal` (theory)
  - Catalog/`SortAndPairMutationGapTests.cs`: `BuildIndex_NumberedVariantsInPathOrderDifferentFromNumericOrder_PicksTheLowestNumber`, `BuildIndex_VariantsWithTheSameNumber_KeepTheirPathOrder`,
    `BuildIndex_ADriveRootEntry_DoesNotThrow`
  - Catalog/`CompareAndCaptureGapTests.cs`: `BuildIndex_NestedNumbering_EqualsFindForEveryPath` (the `Build_TwoBase...` test there is W2-06)
  - FileActions/`DuplicateFinderTests.cs` (268): `FindAsync_WhenFilesEmpty_ReturnsEmpty`, `FindAsync_HashThrowsOperationCanceledWithoutTokenCancellation_PropagatesIt` (closure returns
    `HashFailure::Canceled`, result `Err(Canceled)`), `FindAsync_SamePathListedTwiceOrRecased_IsNotItsOwnDuplicate`, `FindAsync_RemoveNumbered_SelectsOnlyNumberedCopies`,
    `FindAsync_RemoveOriginal_SelectsOnlyNonNumberedOriginals`, `FindAsync_RemoveNumbered_WhenGroupContainsOnlyNumberedCopies_KeepsOneSurvivor`,
    `FindAsync_RemoveOriginal_WhenGroupContainsOnlyOriginals_KeepsOneSurvivor`, `FindAsync_DifferentSizes_SkipsHashComputation`, `FindAsync_SameSizeDifferentHash_NotTreatedAsDuplicate`,
    `FindAsync_FileErrorDuringHash_IgnoresAndContinues` (`HashFailure::Unreadable(io::Error)`), `FindAsync_WhenCancelled_ThrowsOperationCanceledException` (token cancelled before the call),
    `FindAsync_HashThrowsArgumentException_SkipsThatFileAndKeepsTheRest`, `FindAsync_StatThrowsNotSupported_SkipsThatFileAndKeepsTheRest` (`FakeFileSystem::fail(FsOp::Stat, path, kind)`)
  - FileActions/`DuplicateFinderSkipsUnremovableGroupsTests.cs`: `FindAsync_GroupWithoutMatchingMember_IsNeverHashed` (theory, 2 rows)
  - FileActions/`CopyOwnershipFollowupsTests.cs`: `FindAsync_CancelledDuringSizeScan_StopsAtTheNextPath` (needs a stat hook that cancels: write a test-local `struct CancelOnFirstStat` implementing
    `FileSystem` whose only working method is `get_file_stat` (cancels the token, counts calls, returns `None`); the other methods are `unimplemented!()`; assert 1 call and `Err(Canceled)`)
  - FileActions/`FileActionSafetyEdgeTests.cs`: `DuplicateFinder_SamePathTwice_KeepsTheFile`, `DuplicateFinder_HashCollisionAcrossSizes_KeepsASurvivorPerSizeGroup`
  - FileActions/`FileActionsSmallMutationGapTests.cs`: `FindAsync_TwoEmptyFiles_AreDuplicatesOfEachOther`, `FindAsync_FilesWithNoSameSizePartner_AreNeverHashed`,
    `FindAsync_PathWhoseStatFails_IsSkippedAndTheOthersAreStillCompared` (theory io / unauthorized / argument / notsupported: one `fail(FsOp::Stat, ..)` per `io::ErrorKind`; all four give the same result)
  - NOT here: `DuplicateCleanupController*Tests` (App wave W8).
  - Golden tests (new, required): `tests/golden/compare-pair.json` (34 cases), `tests/golden/numbered-pattern.json` (43 stems + 16 variant pairs), `tests/golden/duplicates.json` (27 cases).
- PARITY rows: C-25 (Compare mode pairing), C-06 (duplicate removal: size groups, numbered pattern, survivor rule).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/catalog/compare.rs            (stub: contracts-w2/W2-07/compare.rs)
    crates/pr-core/src/file_actions/duplicates.rs    (stub: contracts-w2/W2-07/duplicates.rs)
    crates/pr-core/tests/catalog_compare.rs
    crates/pr-core/tests/file_actions_duplicates.rs
- Read-only inputs: the three goldens. Lead-owned: `catalog/mod.rs` gets `pub mod compare;`, `file_actions/mod.rs` gets `pub mod duplicates;`.
- Public API (stubs, do NOT change signatures): compare.rs `numbered_stem_base`, `numbered_suffix`, `find`, `CompareIndex { get, len, is_empty, iter }`, `build_index`;
  duplicates.rs `HashFailure`, `find`, `has_numbered_suffix`.
- Semantics notes:
  1. **The two regexes are hand-written** (no `regex` crate; do not add one). .NET semantics to reproduce (all verified in the golden `numbered-pattern`):
     `\d` = `char.IsDigit` (BMP Unicode Nd: `natural_cmp::is_dotnet_digit`; `"a (１)"`, `"a (٣)"`, `"a (१)"` match, `"a (𝟏)"` U+1D7CF does NOT);
     `.` does not match `\n`; `$` matches at the end or just before ONE final `\n` (`"a (1)\n"` matches, `"a (1)\n\n"`, `"a (1)\r\n"`, `"a (1)\nb"` do not);
     `^(.*) \(\d+\)$`: group 1 may be empty (`" (1)"` => `""`), is greedy, and cannot contain `\n` (`"a\n (1)"` => no match, `"x\ny (1)"` => no match) while
     the DuplicateFinder pattern ` \(\d+\)$` is not anchored at the start (`"x\ny (1)"` => true). The suffix pattern `\((\d+)\)$` needs no space (`"a(1)"` => 1,
     `"a ((1)"` => 1) and `int.TryParse` accepts ASCII digits only: non-ASCII digit runs, `99999999999`, `2147483648` => `i32::MAX`; `2147483647` => `i32::MAX` too (it IS the value);
     `"(01)"` => 1, `"(0)"` => 0. A space, `NBSP`, a tab before `(` are not the required single space.
  2. **`find`**: follow the 5 steps in the stub doc. The pair is `(original, numbered)` with the spellings of the LISTED files (the query may use another case: golden
     `query-spelled-in-another-case-returns-the-listed-spellings`). A query that is not in the list but "looks numbered" still pairs through step 4b
     (`DSC0001 (7).JPG` => (`DSC0001.JPG`, `DSC0001 (1).JPG`)). `available` is sorted by `get_full_path` OrdinalIgnoreCase then Ordinal, stable (two spellings of one file keep input order;
     `shot.jpg` vs `SHOT.jpg`: the ordinal tie puts `SHOT.jpg` first, so it is the original). `str::cmp` equals `string.CompareOrdinal` here because the strings are already equal ignoring case.
  3. **`build_index`**: the 4 steps in the stub doc. It must equal `find` for every listed path EXCEPT one C# quirk the golden pins on purpose (`trailing-space-in-extension-find-and-index-disagree`):
     `find` takes the extension of the RAW path (`.jpg ` with a trailing space) while `build_index` groups by the extension of the FULL path (`.jpg`, `GetFullPath` drops the trailing space/dot),
     so for `["C:\p\a.jpg ", "C:\p\a (1).jpg"]` `find` is `None` but the index pairs them. Do not "fix" it.
  4. **Keys of `CompareIndex`** are OrdinalIgnoreCase (`ignore_case_key`); `get("c:\\photos\\dsc0001.jpg")` finds `C:\Photos\DSC0001.JPG`. Folder `None` (a drive root file like `C:\` itself) equals `None`.
  5. **`duplicates::find`**: the 4 steps in the stub doc. Pay attention to: (a) the first spelling of a path listed twice/recased wins and a path is hashed at most once; (b) size groups
     are visited in order of first appearance of the SIZE, files inside in input order, and a size group is skipped when it has fewer than 2 files or no member satisfies the naming rule;
     (c) content groups are keyed `"{size}|{hash}"` compared OrdinalIgnoreCase (hash `ABC` == `abc`), so equal hashes of different sizes never merge; (d) the "never remove a whole group" rule drops
     the FIRST matching member in HASH order (= input order inside a size group); (e) `cancel.check()` before the empty-list shortcut, before every stat and before every hash call;
     (f) a stat that fails or a path that is not a file (directory, missing) is skipped silently; an empty file has size 0 and takes part.
  6. `has_numbered_suffix(stem)` is applied to `get_file_name_without_extension(path)`; the extension never matters (`img (1).png` is numbered).
- Traps checklist: §7 items 1 (never `to_lowercase`), 2/3 (digits and `int.TryParse`, `i32` overflow => `MAX`), 8 (`None` folder vs `Some("")`), 9 (each `catch (X) when` is a match arm: `HashFailure::Canceled`
  propagates, `Unreadable` skips), 10 (hash/removal order = insertion order: `Vec` + index map; never iterate a `HashMap`), plus the .NET regex notes above.
- Depends on (functions called): W2-00 `dotnet_path::{get_full_path, get_directory_name, get_file_name_without_extension}` (MUST be merged first); W1-06 `natural_cmp::is_dotnet_digit`;
  W1-07 `types::get_extension`; `text::{eq_ignore_case, cmp_ignore_case, ignore_case_key}`; `sync::{CancelToken, Canceled}`; `FileSystem::get_file_stat` (+ pr-test-support `FakeFileSystem`, `FsOp`, `golden`).
  No dependency on other W2 units.
- Done when: each listed C# test has a Rust twin (`// C#: Class.Method`); the three goldens pass (`compare-pair`: every `finds[i].pair` and every `index` entry, `index.len()`, and `index_find_agrees`
  recomputed as `index.get(f) == find(files, f)`; `duplicates`: create the files in a `FakeFileSystem` under `C:\golden-dup`, hash closure per case `hash` field ("content" = file text, "constant",
  `by_name`), compare `removed` names and `hashed` names in call order); clippy clean; stub `#![allow]` lines removed.

## Golden package
- `oracle/W2-07.cs`: commands `compare-pair` (Find for each query + the whole BuildIndex, sorted by key, + agreement flag per file), `numbered-pattern` (reads the PRIVATE regexes/`NumberedSuffix`/
  `IsNumberedVariantOf` by reflection so the real code answers, not a copy), `duplicates` (real temp folder, injected hash). Needs `W2-00.cs`. Dispatch lines in the header.
- `golden-inputs/{compare-pair,numbered-pattern,duplicates}.json`, outputs in `golden-reference/`.
- Case design: one behaviour per case (smallest-number rule, same-number path order, nested numbering, other-case folder/extension, UNC, drive root, full-width / Arabic-Indic / overflow suffixes, no-space
  parenthesis, double space, numbered-without-original, unicode stems incl. `İ`/`ı`, the Find/BuildIndex disagreement, NUL path throws `ArgumentException`); for duplicates: survivor rules in both
  directions, hash-text case, size in the key, skip-without-matching-member in both directions, call order, missing file + folder + recased listing.

## Open questions
- `HashFailure` is a new small enum in the stub (C# throws four exception types that all mean "skip" and one that means "stop"); if the lead prefers `Result<String, io::Error>` with
  `ErrorKind::Interrupted` as the cancel marker, only the signature of `find` and one match arm change.
