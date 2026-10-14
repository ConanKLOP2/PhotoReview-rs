//! Tests for `pr_core::catalog::capture_groups`, ported from the C# tests at baseline d92d6aae
//! (`CaptureGroupBuilderTests`, `CaptureAndEntryMutationGapTests`, `CompareAndCaptureGapTests`) plus golden
//! `capture-groups`. The C# allocation budget test has no Rust equivalent (only its behaviour half is ported).

use pr_core::catalog::capture_groups::{build, group_entries};
use pr_core::catalog::types::{CaptureGroup, CatalogEntry};
use pr_core::model::RawPairMode;
use pr_test_support::golden::{self, str_of};

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|x| (*x).to_owned()).collect()
}

fn entry(path: &str) -> CatalogEntry {
    CatalogEntry::new(path).unwrap()
}

#[test]
fn build_matches_same_folder_and_basename_ignoring_case_and_includes_xmp() {
    // C#: CaptureGroupBuilderTests.Build_MatchesSameFolderAndBasenameIgnoringCase_AndIncludesXmp
    let groups = build(
        &s(&[
            "C:\\photos\\IMG_0001.JPG",
            "C:\\photos\\img_0001.cr3",
            "C:\\photos\\Img_0001.xmp",
        ]),
        Some(&s(&["C:\\photos\\IMG_0001.xmp"])),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].jpeg_path(), "C:\\photos\\IMG_0001.JPG");
    assert_eq!(groups[0].raw_path(), "C:\\photos\\img_0001.cr3");
    assert_eq!(groups[0].xmp_path(), Some("C:\\photos\\IMG_0001.xmp"));
    assert_eq!(groups[0].paths().len(), 3);
}

#[test]
fn build_leaves_unpaired_and_ambiguous_files_separate() {
    // C#: CaptureGroupBuilderTests.Build_LeavesUnpairedAndAmbiguousFilesSeparate
    let groups = build(
        &s(&[
            "C:\\photos\\only-raw.nef",
            "C:\\photos\\only-jpeg.jpg",
            "C:\\photos\\ambiguous.jpg",
            "C:\\photos\\ambiguous.jpeg",
            "C:\\photos\\ambiguous.arw",
            "C:\\photos\\valid.jpg",
            "C:\\photos\\valid.nef",
            "C:\\other\\valid.cr2",
        ]),
        None,
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].jpeg_path(), "C:\\photos\\valid.jpg");
    assert_eq!(groups[0].raw_path(), "C:\\photos\\valid.nef");
    assert_eq!(groups[0].xmp_path(), None);
}

#[test]
fn build_ignores_unmatched_and_ambiguous_sidecars() {
    // C#: CaptureGroupBuilderTests.Build_IgnoresUnmatchedAndAmbiguousSidecars
    let groups = build(
        &s(&[
            "C:\\photos\\a.jpg",
            "C:\\photos\\a.cr2",
            "C:\\photos\\b.jpg",
            "C:\\photos\\b.cr2",
        ]),
        Some(&s(&[
            "C:\\photos\\a.xmp",
            "C:\\photos\\a.XMP",
            "C:\\photos\\orphan.xmp",
        ])),
    );
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|g| g.xmp_path().is_none()));
}

#[test]
fn build_non_jpeg_image_next_to_raw_is_not_grouped() {
    // C#: CaptureGroupBuilderTests.Build_NonJpegImageNextToRaw_IsNotGrouped
    for ext in [".tif", ".tiff", ".png", ".bmp", ".gif"] {
        let groups = build(&s(&[&format!("C:\\photos\\A{ext}"), "C:\\photos\\A.dng"]), None);
        assert!(groups.is_empty(), "{ext}");
    }
}

#[test]
fn build_non_jpeg_image_beside_jpeg_and_raw_does_not_make_pair_ambiguous_or_join_it() {
    // C#: CaptureGroupBuilderTests.Build_NonJpegImageBesideJpegAndRaw_DoesNotMakePairAmbiguousOrJoinIt
    let groups = build(
        &s(&["C:\\photos\\A.jpg", "C:\\photos\\A.tif", "C:\\photos\\A.dng"]),
        None,
    );
    assert_eq!(groups.len(), 1);
    assert!(!groups[0].paths().iter().any(|p| p == "C:\\photos\\A.tif"));
}

#[test]
fn build_sidecar_named_base_xmp_wins_over_file_extension_sidecars() {
    // C#: CaptureGroupBuilderTests.Build_SidecarNamedBaseXmpWinsOverFileExtensionSidecars
    let groups = build(
        &s(&["C:\\photos\\a.jpg", "C:\\photos\\a.cr2"]),
        Some(&s(&["C:\\photos\\a.cr2.xmp", "C:\\photos\\a.xmp"])),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].xmp_path(), Some("C:\\photos\\a.xmp"));
}

#[test]
fn build_single_file_extension_sidecar_is_included_when_no_base_named_sidecar_exists() {
    // C#: CaptureGroupBuilderTests.Build_SingleFileExtensionSidecar_IsIncludedWhenNoBaseNamedSidecarExists
    let groups = build(
        &s(&["C:\\photos\\a.jpg", "C:\\photos\\a.cr2"]),
        Some(&s(&["C:\\photos\\A.CR2.xmp"])),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].xmp_path(), Some("C:\\photos\\A.CR2.xmp"));
}

#[test]
fn build_two_competing_file_extension_sidecars_are_ambiguous_and_left_out_of_the_group() {
    // C#: CaptureGroupBuilderTests.Build_TwoCompetingFileExtensionSidecars_AreAmbiguousAndLeftOutOfTheGroup
    let groups = build(
        &s(&["C:\\photos\\a.jpg", "C:\\photos\\a.cr2"]),
        Some(&s(&["C:\\photos\\a.cr2.xmp", "C:\\photos\\a.jpg.xmp"])),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].xmp_path(), None);
    assert_eq!(groups[0].paths().len(), 2);
}

#[test]
fn group_entries_collapsed_entry_takes_position_of_first_member_seen_not_the_representative() {
    // C#: CaptureGroupBuilderTests.GroupEntries_CollapsedEntryTakesPositionOfFirstMemberSeen_NotTheRepresentative
    let order = vec![
        entry("C:\\photos\\a.cr2"),
        entry("C:\\photos\\b.jpg"),
        entry("C:\\photos\\a.jpg"),
    ];
    for mode in [RawPairMode::PreferJpeg, RawPairMode::PreferRaw] {
        let grouped = group_entries(&order, mode, None);
        assert_eq!(grouped.len(), 2);
        let expected = if mode == RawPairMode::PreferJpeg {
            "C:\\photos\\a.jpg"
        } else {
            "C:\\photos\\a.cr2"
        };
        assert_eq!(grouped[0].path(), expected);
        assert_eq!(grouped[1].path(), "C:\\photos\\b.jpg");
    }
}

#[test]
fn group_entries_ten_thousand_paths_groups_all_pairs() {
    // C#: CaptureGroupBuilderTests.GroupEntries_TenThousandPaths_GroupsAllPairsWithBoundedAllocation (behaviour half)
    let mut entries = Vec::with_capacity(10_000);
    for i in 0..5_000 {
        entries.push(entry(&format!("C:\\photos\\{i}.jpg")));
        entries.push(entry(&format!("C:\\photos\\{i}.cr2")));
    }
    let grouped = group_entries(&entries, RawPairMode::PreferJpeg, None);
    assert_eq!(grouped.len(), 5_000);
    assert!(grouped.iter().all(|e| e.capture_group.is_some()));
}

#[test]
fn build_sidecar_that_is_not_an_xmp_file_is_not_attached_to_the_group() {
    // C#: CaptureAndEntryMutationGapTests.Build_SidecarThatIsNotAnXmpFile_IsNotAttachedToTheGroup
    let groups = build(
        &s(&["C:\\shots\\a.jpg", "C:\\shots\\a.cr2"]),
        Some(&s(&["C:\\shots\\a.txt", "C:\\shots\\a.cr2.txt", "  ", "C:\\shots\\a"])),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].xmp_path(), None);
}

#[test]
fn build_lone_same_stem_non_xmp_sidecar_is_ignored() {
    // Extra guard (mirrors C# IndexSidecars `.xmp`-only filter): a single `a.txt` must not become the xmp.
    let groups = build(
        &s(&["C:\p\a.jpg", "C:\p\a.cr2"]),
        Some(&s(&["C:\p\a.txt"])),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].xmp_path(), None);
}

#[test]
fn build_xmp_sidecar_with_matching_name_is_attached() {
    // C#: CaptureAndEntryMutationGapTests.Build_XmpSidecarWithMatchingName_IsAttached
    let groups = build(
        &s(&["C:\\shots\\a.jpg", "C:\\shots\\a.cr2"]),
        Some(&s(&["C:\\shots\\a.xmp"])),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].xmp_path(), Some("C:\\shots\\a.xmp"));
}

#[test]
fn build_two_base_named_xmp_case_variants_claim_no_sidecar_even_with_a_single_file_extension_sidecar() {
    // C#: CompareAndCaptureGapTests.Build_TwoBaseNamedXmpCaseVariants_NoSidecarIsClaimedEvenWithASingleFileExtensionSidecar
    let groups = build(
        &s(&["C:\\photos\\a.jpg", "C:\\photos\\a.cr2"]),
        Some(&s(&["C:\\photos\\a.xmp", "C:\\photos\\A.XMP", "C:\\photos\\a.cr2.xmp"])),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].xmp_path(), None);
    assert_eq!(groups[0].paths().len(), 2);
}

// ---------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/capture-groups.json (oracle-cs capture-groups)
// ---------------------------------------------------------------------------------------------------------------

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .unwrap_or_else(|| panic!("not an array: {v}"))
        .iter()
        .map(|x| x.as_str().unwrap_or_else(|| panic!("not a string: {x}")).to_owned())
        .collect()
}

fn opt_strings(v: &serde_json::Value) -> Option<Vec<String>> {
    if v.is_null() { None } else { Some(strings(v)) }
}

fn check_group(actual: &CaptureGroup, expected: &serde_json::Value, ctx: &str) {
    assert_eq!(actual.jpeg_path(), str_of(expected, "jpeg"), "{ctx} jpeg");
    assert_eq!(actual.raw_path(), str_of(expected, "raw"), "{ctx} raw");
    assert_eq!(actual.xmp_path(), expected["xmp"].as_str(), "{ctx} xmp");
    assert_eq!(actual.paths(), strings(&expected["paths"]).as_slice(), "{ctx} paths");
}

#[test]
fn capture_groups_golden_matches_dotnet() {
    let g = golden::load("capture-groups");
    assert_eq!(g.generator, "oracle-cs capture-groups");
    let mut builds = 0;
    let mut grouped_cases = 0;
    for case in &g.cases {
        let name = str_of(case, "name");
        let sidecars = opt_strings(&case["sidecars"]);
        match str_of(case, "kind") {
            "build" => {
                builds += 1;
                let images = strings(&case["images"]);
                let groups = build(&images, sidecars.as_deref());
                let expected = case["groups"].as_array().expect("groups");
                assert_eq!(groups.len(), expected.len(), "{name}: group count");
                for (a, e) in groups.iter().zip(expected) {
                    check_group(a, e, name);
                }
            }
            "group-entries" => {
                grouped_cases += 1;
                let mode = match str_of(case, "mode") {
                    "Separate" => RawPairMode::Separate,
                    "PreferJpeg" => RawPairMode::PreferJpeg,
                    "PreferRaw" => RawPairMode::PreferRaw,
                    other => panic!("unknown mode {other}"),
                };
                let entries: Vec<CatalogEntry> = case["entries"]
                    .as_array()
                    .expect("entries")
                    .iter()
                    .map(|e| {
                        let mut ce = entry(str_of(e, "path"));
                        ce.length = e["length"].as_i64();
                        ce
                    })
                    .collect();
                let result = group_entries(&entries, mode, sidecars.as_deref());
                let expected = case["result"].as_array().expect("result");
                assert_eq!(result.len(), expected.len(), "{name}: result count");
                for (a, e) in result.iter().zip(expected) {
                    assert_eq!(a.path(), str_of(e, "path"), "{name}: path");
                    assert_eq!(a.length, e["length"].as_i64(), "{name}: length");
                    match (&a.capture_group, &e["group"]) {
                        (None, v) => assert!(v.is_null(), "{name}: missing group"),
                        (Some(_), v) if v.is_null() => panic!("{name}: unexpected group"),
                        (Some(group), v) => check_group(group, v, name),
                    }
                }
            }
            other => panic!("unknown kind {other}"),
        }
    }
    assert_eq!((builds, grouped_cases), (28, 11));
}
