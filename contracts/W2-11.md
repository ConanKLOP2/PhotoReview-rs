# W2-11: DragDropInputService (dropped paths -> open folder / open image's folder)
- Color: 🟢 | Porter: **haiku** (about 60 lines of straight-line logic, one private helper, every branch pinned by 35 goldens; no concurrency, no
  I/O beyond two `FileSystem` predicates) | Test porter: haiku | Verifier: sonnet
- C# source (baseline d92d6aae): `src/PhotoReview.Core/Catalog/DragDropInputService.cs` (72 lines; enum `DragDropInputKind`, record
  `DragDropInputResult`, static `DragDropInputService.Parse`, private `Normalize`, `IsSameFolder`)
  (read with `git -C C:\MyProjects\PhotoReview show d92d6aae:src/PhotoReview.Core/Catalog/DragDropInputService.cs`)
- C# tests to port (all at d92d6aae; `tests/PhotoReview.Core.Tests/Catalog/` unless noted; real temp dirs become `FakeFileSystem` entries):
  - `DragDropInputServiceTests.cs` (79 lines): `ParseEmptyOrNullReturnsInvalid`, `ParseExistingDirectoryReturnsFolderKind` (uses `Path.GetTempPath()`, which has a trailing separator: use a fake dir with one, expect `get_full_path` of it),
    `ParseImageFileReturnsImageKindWithInitialImage`, `ParseHeicFile_FollowsTheWebpHeicSwitch` (both `true`/`false`),
    `ParseUnsupportedFileIsInvalid`, `ParseSameFolderWithTrailingSeparator_IsNotIgnored`.
  - `CatalogMutationGapTests.cs` (the DragDrop tests only): `DragDropResult_IsValid_NeedsAKindOtherThanInvalidAndAFolder` (6 cases),
    `DragDropParse_SingleFolder_HasNoWarningAndNothingIgnored`, `DragDropParse_SameFolderTwiceInDifferentCase_IsNotCountedAsIgnored`,
    `DragDropParse_FolderPlusOtherPaths_IgnoresThemAndWarnsOnlyTheFirstFolderIsUsed`,
    `DragDropParse_SeveralImagesOfOneFolder_AcceptsThemAllWithoutWarning`,
    `DragDropParse_ImageAndUnsupportedFile_CountsTheUnsupportedOneAsIgnoredAndWarns`,
    `DragDropParse_ImagesFromTwoFolders_KeepsTheFirstFolderAndWarnsAboutMixedFolders`.
  - `CatalogServicesRobustnessTests.cs`: `DragDrop_InvalidPaths_AreInvalidNotExceptions` (`"<>|?*"`, `"C:\a\0b.jpg"`, a 5000-char name, `"   "`;
    nothing exists in the fake, expect Invalid / `NoSupportedImages` with ignored 3), `DragDrop_NullAndEmpty` (the `Assert.NotNull(Warning)` becomes
    `warning == Some(NoValidInput)`).
  - NOT here: the English text asserted at `tests/PhotoReview.Core.Tests/FileActions/JournalErrorCodeTests.cs:171`
    (`Assert.Equal("No valid file or folder.", DragDropInputService.Parse([]).Warning)`) becomes a check that
    `DragDropWarning::NoValidInput.catalog_key()` exists in `assets/languages/en.json` with that text (the localizer itself is another unit);
    `MainViewModel.OpenPathAsync` (App wave) is the only production caller.
  - Golden test (new, required): `tests/golden/drag-drop.json` (35 cases). Run each case against a `FakeFileSystem` populated from `dirs`/`files`
    (see Golden package for the root/parent-folder rule).
- PARITY rows: K-39 (drag and drop: first item opens; a file opens its folder at that file, a folder opens the folder). Q-FMT-WEBP-HEIC (switch gating).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/catalog/drag_drop.rs      (stub: contracts-w2/W2-11/drag_drop.rs, compiles clippy-clean)
    crates/pr-core/tests/catalog_drag_drop.rs
- Read-only input: `tests/golden/drag-drop.json`. Lead-owned: `catalog/mod.rs` gets `pub mod drag_drop;`.
- Public API (stub, do NOT change signatures): `DragDropInputKind`, `DragDropWarning` (+ `catalog_key`), `DragDropInputResult` (+ `is_valid`), `parse`.
  Private: `is_same_folder`.
- Semantics notes:
  1. **Valid paths.** Keep paths where `!trim().is_empty()` (C# `IsNullOrWhiteSpace`: Unicode whitespace; use `str::trim`, which is
     `char::is_whitespace`). Input order kept, duplicates kept. `None` and `Some(&[])` behave the same. Blank entries are never counted
     as ignored (goldens `blank-entries-*`). The path text is NOT trimmed when used (`" a"` stays as is).
  2. **Folder wins over files.** The first valid path with `directory_exists` decides, even if an image comes earlier (golden
     `file-before-folder-the-folder-wins`, `folder-named-like-an-image-is-a-folder`). `ignored` counts valid paths that are NOT `is_same_folder`
     of it, including missing paths and files. `folder_path = get_full_path(folder)` with the trailing separator KEPT (golden
     `folder-trailing-separator-is-kept-in-the-result`); `/` and `..` are normalized by `get_full_path` (W2-00).
  3. **`is_same_folder`**: raw OrdinalIgnoreCase equality first (`crate::text::eq_ignore_case`), then `trim_ending_directory_separator(get_full_path(..))`
     equality. `get_full_path` failure => `false`.
  4. **Image drop.** Needs `file_exists` AND `types::is_supported_with(path, raw, webp_heic)`; the two switches are independent (golden
     `raw-switch-does-not-enable-webp`). `initial = get_full_path(images[0])` (the FIRST EXISTING SUPPORTED image, not the first path).
     `folder = get_directory_name(initial)`; `None` only for a root-like path and is unreachable for a real file => `FolderUnknown` still ported.
     `accepted` compares `get_directory_name(get_full_path(p))` to `folder` OrdinalIgnoreCase (a `None` directory never equals). The same image dropped twice is
     accepted twice (golden `same-image-twice-is-accepted-twice`).
  5. **Counts and warnings.** `ignored = valid.len() - accepted.len()` on this branch (valid, not images: missing and unsupported files count).
     Warning precedence: `MixedFolders` (`accepted.len() != images.len()`) over `UnsupportedIgnored` (ignored > 0) over `None`. `Invalid` results carry
     `folder_path`/`initial_image_path` = `None`, so `is_valid()` is `kind != Invalid && folder_path.is_some()`.
  6. **Warning mapping.** C# stores the localized `Tr` text; the stub stores a `DragDropWarning` and `catalog_key` returns the `core.dragDrop.*` key
     (all six exist in `assets/languages/en.json`). The golden stores the member name, so tests compare enum names.
  7. **Path types.** `FileSystem::{file_exists, directory_exists}` take `&Path`; the implementation passes `Path::new(p)` and works on `&str`
     elsewhere. Only `ParseHeicFile_...` asserts `initial_image_path` for a switch-off case (`None`); `DragDrop_InvalidPaths` in C# only asserts
     Invalid/`folder_path == None`; the `ignored == 3` and `NoSupportedImages` checks come from the golden `invalid-paths-are-ignored-not-thrown-on`.
  8. **Divergence (documented in the stub):** C# never throws here for real paths (`Directory.Exists`/`File.Exists` return false on invalid input), but
     `Path.GetFullPath(path)` after an `Exists` hit can only fail with a fake file system; treat as "does not exist" and add a test comment.
- Traps checklist: §7 items 1 (OrdinalIgnoreCase for folder and name comparisons, via `pr_core::text`), 4 (path normalization: `/`, `..`, trailing `\`
  come from W2-00 `dotnet_path`, never hand-written), 8 (`None` vs empty input), 10 (stable input order for `images[0]`; no HashSet).
  Also: the C# `FirstOrDefault(Directory.Exists)` is lazy and stops at the first hit; use `iter().find`.
- Depends on (functions called by the implementation or its tests):
  - W2-00 `dotnet_path::{get_full_path, get_directory_name, trim_ending_directory_separator}` (MUST be merged first).
  - W1-07 `catalog::types::is_supported_with` (merged). W0 abstractions `FileSystem::{directory_exists, file_exists}`, `text::eq_ignore_case`;
    pr-test-support `FakeFileSystem`, `golden`.
  - No dependency on other W2 units. App wave (W8 MainViewModel) consumes it later.
- Done when: all listed tests have a Rust twin with `// C#: <Class>.<Method>`; the golden passes all 35 cases; every `todo!()` and the stub's `#![allow(...)]`
  line removed; clippy clean; `catalog_key` strings match `en.json` (test).

## Golden package
- `oracle/W2-11.cs`: command `drag-drop` (needs `W2-00.cs`; dispatch lines in the file header). Each case builds a real temp tree
  (`dirs`, `files` as 1-byte files), replaces `{root}` in `paths`, calls `Parse(paths, raw, webp_heic)` and writes back `{root}`.
  Result fields: `kind`, `is_valid`, `folder_path`, `initial_image_path`, `ignored_path_count`, `warning` (member name).
- `golden-inputs/drag-drop.json` (35 cases, 5.2 KB), `golden-reference/drag-drop.json`. Case design: null/empty/blank; folder spellings (trailing `\`,
  `/`, `..`, case, twice); folder-vs-file precedence; single, repeated, mixed-folder and mixed-validity images; each switch alone; invalid characters.
- Rust test harness: pick a fixed fake root (for example `C:\fake-root`), create `dirs`, and for each file its parents and the file, substitute `{root}`
  in paths and in expected output. `FakeFileSystem` must compare case-insensitively like NTFS (check it before relying on cases such as
  `same-folder-twice-in-another-case`, `images-same-folder-different-spelling-are-accepted`; otherwise normalize case in the test adapter, not in the
  implementation). Paths with `\u0000` and 5000 chars simply do not exist in the fake.

## Open questions
- Does `FakeFileSystem` match case-insensitively and accept `/` separators? If not, W2-11 tests need either a fix there (lead-owned) or
  `#[ignore = "native"]` twins on a `TempRoot` with `PhysicalFileSystem` (W2-14).
- `parse` takes `Option<&[String]>`; if the App prefers `&[&str]` or `impl IntoIterator`, the lead must decide before W8 (no change needed for W2-11).
