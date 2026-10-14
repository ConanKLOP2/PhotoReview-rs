# W2-05: ImageSortService + SiblingFolderService (fallback folder sort, sibling folder list)
- Color: 🟡 | Porter: **sonnet** (invariant upper-casing quirk, Windows directory attributes, stable multi-key sort) | Test porter: haiku |
  Verifier: sonnet
- C# sources (baseline d92d6aae), under `src/PhotoReview.Core/Catalog/`:
  - `ImageSortService.cs` (118 lines)
  - `SiblingFolderService.cs` (38 lines; the baseline version already skips Hidden/System folders)
  (read with `git -C D:\MyProject\PhotoReview show d92d6aae:src/PhotoReview.Core/Catalog/<file>`)
- C# tests to port (all at d92d6aae; `tests/PhotoReview.Core.Tests/Catalog/` unless noted):
  - `ImageSortServiceTests.cs` (182 lines): `NaturalFilenameSortOrdersNumericSuffixes`, `NaturalFilenameSortHandlesLongNumericRuns`,
    `SizeSortOrdersFilesByDescendingBytes`, `SizeSortOrdersFilesByAscendingBytes` (these four call the string-mode overload with
    `"Name"`, `"Size"`, `"SizeAscending"` on real temp files: use `FakeFileSystem::add_file` with N bytes instead),
    `SortEntries_UsesScannedMetadataAndKeepsEntryMetadataAttached`, `Default_SortEntries_ReturnsInputOrderIncludingDuplicates`
    (drop `Assert.NotSame`: assert the result is a new Vec equal to the input), `Default_LegacySort_ReturnsInputOrder`,
    `NameAscending_EqualsNameMode_AndIsNaturalCaseInsensitive`, `NameDescending_IsExactReverseOfAscending_IncludingTies`
    (entry identity: the two `img10.jpg` entries differ by length, compare whole entries).
    NOT here: `NaturalKeyBuildsExpectedOrder` (W1-06), `ExplorerPolicy_OnlyNameAndSizeModesFollowExplorer` and `EnumValues_AreStableAppendOnly` (W1-01).
  - `NaturalSortPropertyTests.cs` (204 lines), the four tests W1-06 left for this wave: `Sort_IsShuffleAndCultureInvariant` (keep one
    run: Rust has no culture; use a deterministic xorshift RNG written in the test, no `rand` crate; the `RandomName` helper of that file must be
    ported too), `Sort_FastPathMatchesGenericComparerPath` (seeds 1-3; the "opaque" comparer is a local struct implementing `NaturalComparer`
    that forwards to `ManagedNaturalComparer::INSTANCE`; `Some(&opaque)` takes the generic path, `None` the key-cache path),
    `Sort_NumericRunsByValue` (seeds 1-3; replace `BigInteger` by comparing (digit count, digit text) of numbers without leading zeros),
    `SortEntries_SizeModes_Consistent`.
  - `SortAndPairMutationGapTests.cs`: `SortEntries_InjectedComparer_IsUsedForNameOrder`, `SortPaths_InjectedComparer_IsUsedForNameOrder`
    (`ReverseOrdinalComparer` = `string::CompareOrdinal(y, x)` => `y.cmp(x)`), `SortPaths_UnreadableSize_IsLoggedAndSortsAsSmallest`
    (RecordingLog: `contains(LogLevel::Error, &missing)`; the error is a `NotFound` `io::Error`, RecordingLog prints `"{message}: {error}"`).
    NOT here: `Compare_NamesDifferingOnlyInCaseAndLeadingZeros...` and `BuildNaturalKey_NullOrEmpty_IsEmpty` (W1-06),
    the three `BuildIndex_*` tests (W2-07).
  - `SiblingFolderServiceTests.cs` (85 lines): `GetSortedReturnsSiblingsInNaturalOrder`, `GetSorted_HiddenAndSystemSiblings_AreNotReturned`,
    `GetSorted_CurrentFolderIsHidden_StillReturnedAmongVisibleSiblings` (real directories under `TempRoot`; set the attribute with
    `std::process::Command::new("attrib")` `+h` / `+s`, no `windows` crate; mark them `#[ignore = "native: real directory listing and
    Hidden/System attributes"]` and give the same checks a non-ignored twin on the pure `sort_sibling_folders`).
  - `CatalogMutationGapTests.cs`: `SiblingFolders_ParentCannotBeListed_ReturnsEmptyAndLogsTheError` (an `io::Error` of kind `NotFound`; the
    message contains the parent folder), `SiblingFolders_ParentCannotBeListedAndNoLogGiven_StillReturnsEmpty`. Both are native-safe (the folder
    does not exist) so they are NOT ignored.
  - NOT here: `tests/PhotoReview.App.Tests/Coordinators/SiblingFolderNavigator*Tests.cs` (the navigator is App wave W8).
  - Golden tests (new, required): `tests/golden/image-sort-entries.json` (21 cases), `tests/golden/image-sort-paths.json` (34),
    `tests/golden/sibling-folders.json` (12; run the 12 cases twice: pure with the visible folders as `listed`, and `#[ignore = "native"]` on a real tree).
- PARITY rows: P-02 (fallback sort), S-05 (ImageSortMode values, which modes ignore Explorer order), K-05, K-06 (sibling folders), S-29 (siblings are
  only computed when the info block is shown: App).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/catalog/sorting.rs       (stub: contracts-w2/W2-05/sorting.rs, compiles clippy-clean)
    crates/pr-core/tests/catalog_sorting.rs
- Read-only inputs: the three goldens above. Lead-owned: `catalog/mod.rs` gets `pub mod sorting;`.
- Public API (stub, do NOT change signatures): `sort_entries`, `sort_paths`, `parse_sort_mode`, `sort_paths_by_mode_name`,
  `sort_sibling_folders`, `list_visible_directories`, `get_sorted_siblings`.
- Semantics notes:
  1. **Sort keys.** Name order = `ManagedNaturalComparer` on `get_file_name(path)` (W2-00, handles `/`, trailing `\` => `""`). With
     `natural_comparer == None` build `(name, build_natural_key(name))` ONCE per element (200 000 files took 6 s without it) and compare with the
     three steps of `compare_natural` (key OrdinalIgnoreCase, name OrdinalIgnoreCase, name ordinal; use `crate::text::cmp_ignore_case`).
     With `Some(c)`: `c.compare(Some(name_a), Some(name_b))`. Both paths must give the same order for a managed-equivalent comparer (test).
  2. **Stability.** C# `OrderBy/ThenBy` are stable: use `Vec::sort_by` (stable), never `sort_unstable*`. Equal-name entries from different folders keep
     input order (golden `name-same-name-other-folders-keeps-input-order`). `NameDescending` = ascending result REVERSED (`Vec::reverse`), so
     ties come out in reverse input order (golden `name-descending-reverses-ties-too`); it is NOT a descending comparer.
  3. **Size modes.** Key `length.unwrap_or(-1)`; `SizeDescending` sorts the key descending (stable), then name ascending among equal keys;
     `SizeAscending` ascending then name ascending. `Some(0)` beats unknown in descending (golden). For paths the size is `get_file_stat(..)`
     length, `-1` when `None`; sizes are read once each, in input order, ONLY for the two size modes; the error is logged once per missing file.
  4. **`Default`** copies the input (no sort, no size reads, no comparer calls). `Name` and `NameAscending` are the same order.
  5. **`parse_sort_mode` quirk (verified with .NET 10).** `ToUpperInvariant` maps U+017F `ſ` to `S` but leaves U+0131 `ı` unchanged, so `"ſize"`
     is SizeDescending and `"sıze"` falls to Name. Rust `to_uppercase` would map `ı` to `I`. Golden cases `text-dotless-i-...`,
     `text-long-s-...`. No trimming (`" size"` is Name). `"Size"` alone means DESCENDING.
  6. **Siblings.** `get_sorted_siblings`: `full_current = trim_ending_directory_separator(get_full_path(current)?)`; parent via
     `get_directory_name` (`None` => `Ok(vec![])`); list with `list_visible_directories(parent)`; ANY io error => one `log.error("Failed to
     enumerate sibling folders of {parent}", Some(&err))` (message keeps the parent text; no log => nothing) and `Ok(vec![])`. The current folder
     is appended when no listed path equals it (OrdinalIgnoreCase: `album 1` for an existing `Album 1` is not added twice and the result keeps the
     listing's spelling), then everything is stable-sorted by `get_file_name` with the managed natural comparer (`Album 1 < Album 2 < Album 10`;
     `A < a02 < a2 < a10 < b` in the golden). A current folder that does not exist is still returned. Result strings are the listing's own full
     paths (`parent` + `\` + name), the appended current is `full_current`.
  7. **Listing.** `std::fs::read_dir(parent)`; per entry `entry.metadata()` (does not follow links; `file_attributes()` from
     `std::os::windows::fs::MetadataExt`, behind `#[cfg(windows)]`); keep an entry when `attrs & 0x10 != 0` (DIRECTORY, so directory symlinks and
     junctions count) and `attrs & (0x2 | 0x4) == 0` (not Hidden/System). Do not recurse. Return the first `read_dir`/entry error as `Err`.
  8. **Why `std::fs` directly:** C# calls `Directory.EnumerateDirectories` without an `IFileSystem` (it is on the architecture test's
     file-system-boundary allowlist). The `FileSystem` trait has no attribute filter. See Open questions.
- Traps checklist: §7 items 1 (OrdinalIgnoreCase via `pr_core::text`), 2 (culture: `parse_sort_mode`), 3 (`length` is `i64`, `-1` sentinel; u64 stat length
  saturates), 8 (`None` comparer vs `Some`), 10 (stable ordering; do not use a `HashMap` for anything that leaks into output order).
- Depends on (functions called by the implementation or its tests):
  - W2-00 `dotnet_path::{get_file_name, get_full_path, trim_ending_directory_separator, get_directory_name}` (MUST be merged first).
  - W1-06 `natural_cmp::{build_natural_key, compare_natural, ManagedNaturalComparer}` (merged).
  - W1-07 `types::CatalogEntry` (`new`, `path`, `length` field, clone, equality), W1-01 `model::ImageSortMode` (merged).
  - W0 abstractions `FileSystem::get_file_stat`, `Log`, `NaturalComparer`, `text::{cmp_ignore_case, eq_ignore_case}`; pr-test-support `FakeFileSystem`,
    `RecordingLog`, `TempRoot`, `golden`.
- Done when: all listed tests have a Rust twin with `// C#: <Class>.<Method>`; the three goldens pass (the native twin of `sibling-folders` is `#[ignore]`d but
  must pass when run with `-- --ignored`); clippy clean; stub's `#![allow(...)]` line removed.

## Golden package
- `oracle/W2-05.cs`: commands `image-sort-entries`, `image-sort-paths` (real temp files; `Sort(files, mode|text)`; reports sizes read failures and the exception
  type `FileNotFoundException`), `sibling-folders` (real tree incl. Hidden/System attributes; reports the log). Needs `W2-00.cs`.
  Dispatch lines are in the file header.
- `golden-inputs/{image-sort-entries,image-sort-paths,sibling-folders}.json`, outputs in `golden-reference/`.
- Case design: every sort mode on a list with case-only, leading-zero and equal-name ties; injected comparer decides size ties; the string overload for every
  accepted spelling and for the spellings that must fall through (`" size"`, `"size "`, `"PortraitFirst"`, `"sıze"`); missing files in both size orders.

## Open questions
- `SiblingFolderService` and the `FileSystem` trait: recommended (and used by this stub): `std::fs` + `cfg(windows)` attributes inside `pr-core`, like the C#
  allowlist. Alternative: add a default method `FileSystem::enumerate_visible_directories(&self, dir) -> io::Result<Vec<PathBuf>>` (default = `enumerate_directories`)
  that `PhysicalFileSystem` (W2-14) overrides with the Hidden/System skip; then `get_sorted_siblings` would take `&dyn FileSystem` and the Hidden/System tests would
  need a physical file system anyway. Lead decides; the pure `sort_sibling_folders` stays either way.
