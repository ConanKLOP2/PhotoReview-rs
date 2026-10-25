//! W9-R1: CaptureGroupRenamePlan oracle and C# named regressions at7eaee2a2.
use pr_core::{
    catalog::types::CaptureGroup,
    file_actions::rename::{CaptureGroupRenamePlan, RenameNameProblem as Problem},
};
use pr_test_support::golden;
use serde_json::{Value, json};
use std::collections::BTreeMap;

// Oracle W9-R1.cs; all declared group cases, including exact new spellings and sidecars.
#[test]
fn golden_capture_group_plan_matches_all_11_cases() {
    let input: Value = serde_json::from_str(include_str!(
        "../../../tests/golden-inputs/staged/w9-r1-rename-policy.json"
    ))
    .unwrap();
    let golden = golden::load("w9-r1-rename-policy");
    let cases = golden
        .cases
        .iter()
        .filter(|case| case["kind"] == "group")
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 11);
    for (case, spec) in cases.into_iter().zip(input["group"].as_array().unwrap()) {
        let group = CaptureGroup::new(
            case["jpeg"].as_str().unwrap(),
            case["raw"].as_str().unwrap(),
            case["xmp"].as_str().map(str::to_owned),
        )
        .unwrap();
        let plan = CaptureGroupRenamePlan::build(
            &group,
            case["typed"].as_str(),
            spec["long_path_aware"].as_bool().unwrap_or(false),
        );
        let names = plan
            .file_names
            .as_ref()
            .map(|names| {
                names
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let actual = json!({"kind":"group","jpeg":case["jpeg"],"raw":case["raw"],"xmp":case["xmp"],"typed":case["typed"],"problem":format!("{:?}",plan.check.problem),"new_file_name":plan.check.new_file_name,"unchanged":plan.check.is_unchanged,"case_only":plan.check.differs_only_by_case,"names":names});
        assert_eq!(actual, *case);
    }
}

// C#: CaptureGroupRenamePlanTests.Build_XmpNotMatchingAnyMember_ReturnsSidecarNotMappable.
#[test]
fn build_xmp_not_matching_any_member_returns_sidecar_not_mappable() {
    let group = CaptureGroup::new(r"C:\p\a.JPG", r"C:\p\a.NEF", Some(r"C:\p\other.xmp".into())).unwrap();
    let plan = CaptureGroupRenamePlan::build(&group, Some("b"), false);
    assert_eq!(plan.check.problem, Problem::SidecarNotMappable);
    assert!(plan.file_names.is_none());
}

fn group(jpeg: &str, raw: &str, xmp: Option<&str>) -> CaptureGroup {
    CaptureGroup::new(
        format!(r"C:\Shots\{jpeg}"),
        format!(r"C:\Shots\{raw}"),
        xmp.map(|path| format!(r"C:\Shots\{path}")),
    )
    .unwrap()
}

// C#: CaptureGroupRenamePlanTests.Build_JpegAndRawWithoutXmp_MapsBothMembers.
#[test]
fn build_jpeg_and_raw_without_xmp_maps_both_members() {
    let plan = CaptureGroupRenamePlan::build(&group("IMG.jpg", "IMG.cr2", None), Some("new"), false);
    assert!(plan.check.is_valid());
    assert_eq!(plan.check.new_file_name, "new.jpg");
    assert!(!plan.check.is_unchanged && !plan.check.differs_only_by_case);
    let names = plan.file_names.unwrap();
    assert_eq!(names.len(), 2);
    assert_eq!(names.get(r"C:\Shots\IMG.jpg").unwrap(), "new.jpg");
    assert_eq!(names.get(r"C:\Shots\IMG.cr2").unwrap(), "new.cr2");
}

macro_rules! sidecar_case {
    ($name:ident, $source:literal, $xmp:literal, $expected:literal) => {
        #[doc = concat!("C#: CaptureGroupRenamePlanTests.", $source)]
        #[test]
        fn $name() {
            let group = group("IMG.jpg", "IMG.cr2", Some($xmp));
            let plan = CaptureGroupRenamePlan::build(&group, Some("new"), false);
            assert!(plan.check.is_valid());
            let names = plan.file_names.unwrap();
            assert_eq!(names.len(), 3);
            assert_eq!(names.get(group.xmp_path().unwrap()).unwrap(), $expected);
            assert_eq!(names.get(group.raw_path()).unwrap(), "new.cr2");
        }
    };
}
sidecar_case!(
    build_xmp_named_after_base_follows_stem,
    "Build_XmpNamedAfterBase_FollowsStem",
    "IMG.xmp",
    "new.xmp"
);
sidecar_case!(
    build_xmp_named_after_raw_follows_raw_file_name,
    "Build_XmpNamedAfterRaw_FollowsRawFileName",
    "IMG.cr2.xmp",
    "new.cr2.xmp"
);
sidecar_case!(
    build_xmp_named_after_jpeg_follows_jpeg_file_name,
    "Build_XmpNamedAfterJpeg_FollowsJpegFileName",
    "IMG.jpg.xmp",
    "new.jpg.xmp"
);
sidecar_case!(
    build_xmp_owner_compared_ignoring_case_is_mapped,
    "Build_XmpOwnerComparedIgnoringCase_IsMapped",
    "img.CR2.XMP",
    "new.cr2.XMP"
);

// C#: CaptureGroupRenamePlanTests.Build_UpperCaseExtensions_AreKept.
#[test]
fn build_upper_case_extensions_are_kept() {
    let plan = CaptureGroupRenamePlan::build(&group("IMG.JPG", "IMG.CR2", Some("IMG.XMP")), Some("new"), false);
    assert!(plan.check.is_valid());
    let names = plan.file_names.unwrap();
    for (path, name) in [
        (r"C:\Shots\IMG.JPG", "new.JPG"),
        (r"C:\Shots\IMG.CR2", "new.CR2"),
        (r"C:\Shots\IMG.XMP", "new.XMP"),
    ] {
        assert_eq!(names.get(path).unwrap(), name);
    }
}

// C#: CaptureGroupRenamePlanTests.Build_TypedJpegExtension_IsStripped.
#[test]
fn build_typed_jpeg_extension_is_stripped() {
    for typed in ["new.jpg", "new.JPG", "  new.jpg "] {
        let plan = CaptureGroupRenamePlan::build(&group("IMG.jpg", "IMG.cr2", None), Some(typed), false);
        assert!(plan.check.is_valid());
        let names = plan.file_names.unwrap();
        assert_eq!(names.get(r"C:\Shots\IMG.jpg").unwrap(), "new.jpg");
        assert_eq!(names.get(r"C:\Shots\IMG.cr2").unwrap(), "new.cr2");
    }
}

// C#: CaptureGroupRenamePlanTests.Build_TypedRawExtension_StaysPartOfStemForWholeGroup.
#[test]
fn build_typed_raw_extension_stays_part_of_stem_for_whole_group() {
    let plan = CaptureGroupRenamePlan::build(&group("IMG.jpg", "IMG.cr2", Some("IMG.xmp")), Some("a.cr2"), false);
    assert!(plan.check.is_valid());
    let names = plan.file_names.unwrap();
    for (path, name) in [
        (r"C:\Shots\IMG.jpg", "a.cr2.jpg"),
        (r"C:\Shots\IMG.cr2", "a.cr2.cr2"),
        (r"C:\Shots\IMG.xmp", "a.cr2.xmp"),
    ] {
        assert_eq!(names.get(path).unwrap(), name);
    }
}

// C#: CaptureGroupRenamePlanTests.Build_InvalidName_ReturnsProblemAndNoMap.
#[test]
fn build_invalid_name_returns_problem_and_no_map() {
    for (typed, problem) in [
        ("a:b", Problem::InvalidCharacter),
        ("", Problem::Empty),
        (".jpg", Problem::Empty),
        ("a.", Problem::TrailingDotOrSpace),
        ("CON", Problem::ReservedDeviceName),
    ] {
        let plan = CaptureGroupRenamePlan::build(&group("IMG.jpg", "IMG.cr2", Some("IMG.xmp")), Some(typed), false);
        assert_eq!(plan.check.problem, problem);
        assert!(!plan.check.is_valid());
        assert!(plan.file_names.is_none());
    }
}

// C#: CaptureGroupRenamePlanTests.Build_OnlyCaseDiffers_IsCaseOnlyOnPlan.
#[test]
fn build_only_case_differs_is_case_only_on_plan() {
    let plan = CaptureGroupRenamePlan::build(&group("IMG.jpg", "IMG.cr2", Some("IMG.xmp")), Some("img"), false);
    assert!(plan.check.is_valid() && plan.check.differs_only_by_case && !plan.check.is_unchanged);
    let names = plan.file_names.unwrap();
    assert_eq!(names.get(r"C:\Shots\IMG.xmp").unwrap(), "img.xmp");
}

// C#: CaptureGroupRenamePlanTests.Build_SameName_IsUnchanged.
#[test]
fn build_same_name_is_unchanged() {
    let plan = CaptureGroupRenamePlan::build(&group("IMG.jpg", "IMG.cr2", Some("IMG.xmp")), Some("IMG"), false);
    assert!(plan.check.is_valid() && plan.check.is_unchanged && !plan.check.differs_only_by_case);
    assert_eq!(plan.file_names.unwrap().get(r"C:\Shots\IMG.xmp").unwrap(), "IMG.xmp");
}

// C#: CaptureGroupRenamePlanTests.Build_MapKeysCompareIgnoringCase.
#[test]
fn build_map_keys_compare_ignoring_case() {
    let plan = CaptureGroupRenamePlan::build(&group("IMG.jpg", "IMG.cr2", None), Some("new"), false);
    let names = plan.file_names.unwrap();
    assert!(names.contains_key(r"C:\SHOTS\IMG.JPG"));
    assert_eq!(names.get(r"C:\Shots\img.CR2").unwrap(), "new.cr2");
}

// C#: CaptureGroupRenamePlanTests.Build_PathTooLongForRaw_ReturnsPathTooLong.
#[test]
fn build_path_too_long_for_raw_returns_path_too_long() {
    let folder = format!("C:\\{}\\", "d".repeat(197));
    let group = CaptureGroup::new(format!("{folder}a.jpg"), format!("{folder}a.cr2"), None).unwrap();
    let plan = CaptureGroupRenamePlan::build(&group, Some(&"x".repeat(56)), false);
    assert_eq!(plan.check.problem, Problem::PathTooLong);
    assert!(plan.file_names.is_none());
    // Stronger than the C# twin: JPEG itself fits, only RAW's folder crosses the limit.
    let group = CaptureGroup::new(r"C:\p\a.jpg", format!("{folder}a.cr2"), None).unwrap();
    let plan = CaptureGroupRenamePlan::build(&group, Some(&"x".repeat(56)), false);
    assert_eq!(plan.check.problem, Problem::PathTooLong);
    assert!(plan.file_names.is_none());
}

// C#: CaptureGroupRenamePlanTests.Build_LongPathAware_SkipsPathBudget.
#[test]
fn build_long_path_aware_skips_path_budget() {
    let folder = format!("C:\\{}\\", "d".repeat(197));
    let group = CaptureGroup::new(format!("{folder}a.jpg"), format!("{folder}a.cr2"), None).unwrap();
    let plan = CaptureGroupRenamePlan::build(&group, Some(&"x".repeat(56)), true);
    assert!(plan.check.is_valid());
    assert_eq!(plan.file_names.unwrap().len(), 2);
}

// C# Build_NullGroup_Throws is enforced by the non-null &CaptureGroup parameter.
// There is no nullable group value at this Rust API boundary.
