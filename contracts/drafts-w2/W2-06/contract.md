# W2-06: CaptureGroupBuilder (JPEG+RAW pairing, RawPairMode)
- Color: 🟡 | Porter: **sonnet** (identity of groups, OrdinalIgnoreCase keys, sidecar ambiguity rules; group actions move/recycle every member so a wrong
  group is a data-safety bug) | Test porter: haiku | Verifier: sonnet
- C# source (baseline d92d6aae): `src/PhotoReview.Core/Catalog/CaptureGroupBuilder.cs` (182 lines). `CaptureGroup` (62 lines) and `CatalogEntry` are W1-07
  (`catalog::types`, merged); do not edit them.
  (read with `git -C D:\MyProject\PhotoReview show d92d6aae:src/PhotoReview.Core/Catalog/CaptureGroupBuilder.cs`)
- C# tests to port (all at d92d6aae, `tests/PhotoReview.Core.Tests/Catalog/` unless noted):
  - `CaptureGroupBuilderTests.cs` (229 lines): `Build_MatchesSameFolderAndBasenameIgnoringCase_AndIncludesXmp`, `Build_LeavesUnpairedAndAmbiguousFilesSeparate`,
    `Build_IgnoresUnmatchedAndAmbiguousSidecars`, `Build_NonJpegImageNextToRaw_IsNotGrouped` (theory: .tif .tiff .png .bmp .gif),
    `Build_NonJpegImageBesideJpegAndRaw_DoesNotMakePairAmbiguousOrJoinIt`, `Build_SidecarNamedBaseXmpWinsOverFileExtensionSidecars`,
    `Build_SingleFileExtensionSidecar_IsIncludedWhenNoBaseNamedSidecarExists`, `Build_TwoCompetingFileExtensionSidecars_AreAmbiguousAndLeftOutOfTheGroup`,
    `GroupEntries_CollapsedEntryTakesPositionOfFirstMemberSeen_NotTheRepresentative` (theory PreferJpeg / PreferRaw).
    The next three tests of that file use `ReviewCatalog` and belong to W2-10: `GroupEntries_UsesSelectedRepresentativeAndKeepsBothMembersAddressable`,
    `GroupEntries_PreferRawUsesRawMetadata_AndExplorerOrderCollapsesMembers`, `Reset_DefaultModeKeepsPairMembersSeparate`.
    Allocation/timing tests `GroupEntries_TenThousandPaths_GroupsAllPairsWithBoundedAllocation` and `..._TimingReport`: port ONLY the behaviour half as
    `group_entries_ten_thousand_paths_groups_all_pairs` (5 000 `N.jpg`+`N.cr2` pairs => 5 000 grouped entries, each with a group, and it must finish
    without any per-entry O(n) scan, i.e. use the maps below); the C# allocation budget has no Rust equivalent (skip, say so in the report).
  - `CaptureAndEntryMutationGapTests.cs`: `Build_SidecarThatIsNotAnXmpFile_IsNotAttachedToTheGroup`, `Build_XmpSidecarWithMatchingName_IsAttached`.
  - `CompareAndCaptureGapTests.cs`: `Build_TwoBaseNamedXmpCaseVariants_NoSidecarIsClaimedEvenWithASingleFileExtensionSidecar`.
  - Golden test (new, required): `tests/golden/capture-groups.json` (39 cases: 28 `build`, 11 `group-entries`).
- PARITY rows: I-17 (capture groups and pair modes).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/catalog/capture_groups.rs   (stub: contracts-w2/W2-06/capture_groups.rs, compiles clippy-clean)
    crates/pr-core/tests/catalog_capture_groups.rs
- Read-only inputs: `tests/golden/capture-groups.json`. Lead-owned: `catalog/mod.rs` gets `pub mod capture_groups;`.
- Public API (stub, do NOT change signatures): `build(image_paths: &[String], sidecar_paths: Option<&[String]>) -> Vec<CaptureGroup>`,
  `group_entries(entries: &[CatalogEntry], mode: RawPairMode, sidecar_paths: Option<&[String]>) -> Vec<CatalogEntry>`.
- Semantics notes (all verified by the golden):
  1. **Bucket key** = (`get_directory_name(path)` or `""`, `get_file_name_without_extension(path)`), both compared OrdinalIgnoreCase. A bucket keeps its FIRST jpeg path, its
     FIRST raw path and a count per kind; it becomes a group only with count 1 + count 1. Results come out in first-seen bucket order (golden
     `groups-come-out-in-first-seen-bucket-order`). Group paths keep the spelling of the first-seen member (`C:\Photos\a.jpg` + `c:\photos\a.cr2` pair up).
  2. **Which files take part.** Only `.jpg`/`.jpeg` (JPEG side) and the eight RAW extensions (`.cr2 .cr3 .nef .arw .dng .raf .orf .rw2`); `.tif .tiff .png .bmp .gif .webp .heic
     .srw` and extension-less names are ignored and neither pair with a RAW nor make a pair ambiguous (golden `non-jpeg-*`, `all-eight-raw-extensions-pair`).
     A blank path is skipped. `C:\photos\.jpg` + `C:\photos\.cr2` DO pair (stem `""`), as in C# (golden `blank-and-extensionless-image-paths-are-skipped`).
  3. **Ambiguity.** `a.jpg` + `A.JPG` (case variants), `a.jpg` + `a.jpeg`, or two RAWs of one stem => no group for that bucket.
  4. **Sidecar rules (the part most easily gotten wrong):** index only paths whose extension is `.xmp` (any case); key = (folder, stem). Step 1: the
     bucket's own key (folder, base name) exists => exactly one path wins, TWO OR MORE (`a.xmp` + `A.XMP`) => no sidecar and the file-extension sidecars are
     NOT looked at (golden `two-base-named-case-variants-claim-nothing-even-with-one-extension-sidecar`). Step 2 (only when step 1 had no key): look up
     (folder of the jpeg, jpeg file name) and (folder of the raw, raw file name), e.g. key (`dir`, `a.cr2`) for `a.cr2.xmp`; all candidates of both lookups are
     concatenated: exactly one => it, otherwise none (`a.cr2.xmp` + `A.CR2.XMP` is two => none; `a.cr2.xmp` + `a.jpg.xmp` is two => none; one `a.JPG.xmp` => it).
     The sidecar string keeps its own spelling and separators (`C:/photos/a.xmp` is returned as written). A sidecar in another folder never matches.
  5. **`group_entries`.** `Separate` => clone of `entries` (and `build` is not called). The first member seen emits the single entry AT ITS POSITION; the entry is a
     clone of the entry whose path equals the mode's representative (OrdinalIgnoreCase; first entry of a path wins), with `capture_group = Some(group)`. A path that
     appears twice in `entries` makes its bucket ambiguous (golden `duplicate-entry-path-makes-the-jpeg-side-ambiguous`: nothing is grouped, both entries stay).
  6. **Performance** (AGENTS priority 3, 10k-entry folders): O(n) with `HashMap<String, usize>` keyed by `ignore_case_key`; avoid per-group `Vec`s and a second lookup per
     bucket. `if sidecars.is_empty() { None }` fast path in the sidecar choice, like C#.
  7. Never `unwrap()` `CaptureGroup::new`: all three arguments are non-blank here; on the impossible `Err` skip the bucket.
- Traps checklist: §7 items 1 (never `to_lowercase`: use `crate::text::ignore_case_key` for every map key), 7 (the Path helpers come from W2-00; do not use `std::path::Path::parent`),
  8 (`None` sidecar vs blank), 10 (output order = first seen; use `Vec` + index map, never iterate a `HashMap`).
- Depends on (functions called): W2-00 `dotnet_path::{get_directory_name, get_file_name, get_file_name_without_extension}` (MUST be merged first); W1-07
  `types::{CaptureGroup::new/paths accessors, CatalogEntry::{new,path,clone}, get_extension, extension_in, JPEG_EXTENSIONS, RAW_EXTENSIONS}`; W1-01 `RawPairMode`;
  `text::{ignore_case_key, eq_ignore_case}`.
- Done when: every listed C# test has a Rust twin (`// C#: Class.Method`); the golden passes (a `build` case compares jpeg/raw/xmp/paths per group in order; a
  `group-entries` case compares path, length and group of every result entry); clippy clean; stub's `#![allow]` line removed.

## Golden package
- `oracle/W2-06.cs`: command `capture-groups`. Needs `W2-00.cs`. Dispatch line in the header.
- `golden-inputs/capture-groups.json`, `golden-reference/capture-groups.json`.
- Case design: one rejection reason per case (ambiguous jpeg, ambiguous raw, case-variant jpegs, non-image, foreign-folder sidecar, non-xmp sidecar, competing sidecars...),
  plus positive cases for each sidecar rule and each RAW extension, UNC/drive-root/relative inputs, dotted base names (`x.y.jpg` + `x.y.cr2.xmp` vs `x.xmp`).

## Open questions
- None blocking.
