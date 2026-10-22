//! Tests for `pr_core::model::enums`: the persisted enum members, numeric values, aliases, written text and the
//! `SortModePolicy` split. Port of the enum-level asserts in `PhotoReview.Core/Model/*.cs` (baseline e441d67a).
//! JSON reading and writing belongs to W1-02 (`model::serde_ext`), so no serde call is made here.

use pr_core::model::serde_ext::LenientEnum;
use pr_core::model::{
    DecoderBackend, FileOperationType, FitWidthAnchor, ImageSortMode, ImageTransition, InitialViewMode, InstanceMode,
    JournalDurability, JournalState, KeyboardZoomAnchor, KineticGlideSmoothing, LoadingMode, MiddleClickAction,
    MouseWheelAction, RawFullDecode, RawPairMode, ScalingQuality, uses_explorer_order,
};
use pr_test_support::golden;
use serde_json::Value;

/// The C# commit the golden file `model-enums.json` was generated from.
const GOLDEN_COMMIT: &str = "7eaee2a2b071c7ca05291c86fb78a347f9e6a15f";

/// Asserts the member names (declaration order) and that the numeric values are `0..n-1`.
fn assert_specification<T: LenientEnum>(expected_names: &[&str]) {
    let names: Vec<&str> = T::MEMBERS.iter().map(|member| member.name()).collect();
    assert_eq!(names, expected_names, "{} member names", T::TYPE_NAME);

    let values: Vec<i32> = T::MEMBERS.iter().map(|member| member.value()).collect();
    let expected_values: Vec<i32> = (0..expected_names.len())
        .map(|index| i32::try_from(index).expect("enum has fewer than i32::MAX members"))
        .collect();
    assert_eq!(values, expected_values, "{} numeric values", T::TYPE_NAME);
}

/// `C#: ModelEnumTests.ValuesAndNamesMatchSpecification`
#[test]
fn values_and_names_match_specification() {
    assert_specification::<LoadingMode>(&["Fast", "Preview", "Original"]);
    assert_specification::<ImageSortMode>(&[
        "Name",
        "SizeDescending",
        "SizeAscending",
        "Default",
        "NameAscending",
        "NameDescending",
    ]);
    assert_specification::<InitialViewMode>(&[
        "Fit",
        "Percent100",
        "Percent200",
        "Percent400",
        "FitWidth",
        "FitHeight",
        "ClickZoomLevel",
    ]);
    assert_specification::<FitWidthAnchor>(&["Centre", "TopThird", "BottomThird"]);
    assert_specification::<MiddleClickAction>(&[
        "None",
        "ClickZoom",
        "ActualSize",
        "Fit",
        "FitWidth",
        "FitWidth2",
        "PreviousImage",
        "NextImage",
        "PreviousFolder",
        "NextFolder",
        "OpenFolder",
    ]);
    assert_specification::<FileOperationType>(&["Move", "Copy", "Recycle"]);
    assert_specification::<JournalState>(&["Prepared", "Committed", "Failed", "Dismissed"]);
    assert_specification::<DecoderBackend>(&["Wpf", "WicDirect", "TurboJpeg", "LibRaw"]);
    assert_specification::<ScalingQuality>(&["Linear", "HighQuality"]);
}

/// `C#: ImageSortServiceTests.ExplorerPolicy_OnlyNameAndSizeModesFollowExplorer`
#[test]
fn explorer_policy_only_name_and_size_modes_follow_explorer() {
    assert!(uses_explorer_order(ImageSortMode::Name));
    assert!(uses_explorer_order(ImageSortMode::SizeAscending));
    assert!(uses_explorer_order(ImageSortMode::SizeDescending));
    assert!(!uses_explorer_order(ImageSortMode::Default));
    assert!(!uses_explorer_order(ImageSortMode::NameAscending));
    assert!(!uses_explorer_order(ImageSortMode::NameDescending));
}

/// `C#: ImageSortServiceTests.EnumValues_AreStableAppendOnly`
#[test]
fn enum_values_are_stable_append_only() {
    assert_eq!(ImageSortMode::Name as i32, 0);
    assert_eq!(ImageSortMode::SizeDescending as i32, 1);
    assert_eq!(ImageSortMode::SizeAscending as i32, 2);
    assert_eq!(ImageSortMode::Default as i32, 3);
    assert_eq!(ImageSortMode::NameAscending as i32, 4);
    assert_eq!(ImageSortMode::NameDescending as i32, 5);
}

/// Asserts that `Default::default()` is the single member whose value is 0 (C# `default(T)`).
fn assert_default_is_zero_member<T: LenientEnum + Default>() {
    let default = T::default();
    assert_eq!(default.value(), 0, "{} default must be the zero member", T::TYPE_NAME);
    assert_eq!(
        T::MEMBERS.iter().filter(|member| member.value() == 0).count(),
        1,
        "{} has exactly one zero member",
        T::TYPE_NAME
    );
    assert_eq!(default, T::zero(), "{} default equals LenientEnum::zero", T::TYPE_NAME);
}

/// No C# twin: `default(T)` is the zero member for all 17 persisted enums.
#[test]
fn default_is_the_zero_member() {
    assert_default_is_zero_member::<ImageSortMode>();
    assert_default_is_zero_member::<InitialViewMode>();
    assert_default_is_zero_member::<LoadingMode>();
    assert_default_is_zero_member::<DecoderBackend>();
    assert_default_is_zero_member::<RawPairMode>();
    assert_default_is_zero_member::<RawFullDecode>();
    assert_default_is_zero_member::<MouseWheelAction>();
    assert_default_is_zero_member::<ScalingQuality>();
    assert_default_is_zero_member::<ImageTransition>();
    assert_default_is_zero_member::<InstanceMode>();
    assert_default_is_zero_member::<JournalState>();
    assert_default_is_zero_member::<FileOperationType>();
    assert_default_is_zero_member::<JournalDurability>();
    assert_default_is_zero_member::<FitWidthAnchor>();
    assert_default_is_zero_member::<KeyboardZoomAnchor>();
    assert_default_is_zero_member::<KineticGlideSmoothing>();
    assert_default_is_zero_member::<MiddleClickAction>();
}

/// The `members` array of one golden case.
fn golden_members(case: &Value) -> &[Value] {
    let enum_name = golden::str_of(case, "enum");
    case["members"]
        .as_array()
        .unwrap_or_else(|| panic!("{enum_name}: no 'members' array"))
}

/// Checks the members of `T` against one golden case (count, order, name, value, aliases, written text) and
/// returns them so the caller can check more fields.
fn check_members<T: LenientEnum>(case: &Value) -> Vec<T> {
    let enum_name = golden::str_of(case, "enum");
    let expected = golden_members(case);
    assert_eq!(T::TYPE_NAME, enum_name, "golden case dispatched to the wrong enum");
    assert_eq!(T::MEMBERS.len(), expected.len(), "{enum_name}: member count");

    for (member, json) in T::MEMBERS.iter().zip(expected) {
        let name = golden::str_of(json, "name");
        assert_eq!(member.name(), name, "{enum_name}: member name");
        assert_eq!(
            i64::from(member.value()),
            golden::i64_of(json, "value"),
            "{enum_name}.{name}: numeric value"
        );

        let expected_aliases: Vec<&str> = json["aliases"]
            .as_array()
            .unwrap_or_else(|| panic!("{enum_name}.{name}: no 'aliases' array"))
            .iter()
            .map(|alias| {
                alias
                    .as_str()
                    .unwrap_or_else(|| panic!("{enum_name}.{name}: alias is not a string"))
            })
            .collect();
        assert_eq!(
            member.aliases(),
            expected_aliases.as_slice(),
            "{enum_name}.{name}: aliases"
        );

        assert_eq!(
            member.json_text(),
            golden::str_of(json, "json"),
            "{enum_name}.{name}: written JSON text"
        );
    }
    T::MEMBERS.to_vec()
}

/// Golden-driven: `tests/golden/model-enums.json` (generated by oracle-cs from C#), every case, plus the enum-level
/// `(int)` asserts that sit inside the C# settings tests named in W1-01.
#[test]
fn model_enums_match_golden() {
    let golden = golden::load("model-enums");
    assert_eq!(golden.generator, "oracle-cs model-enums");
    assert_eq!(golden.csharp_commit, GOLDEN_COMMIT);
    assert_eq!(golden.cases.len(), 17, "golden must cover the 17 persisted enums");

    let mut names: Vec<&str> = golden.cases.iter().map(|case| golden::str_of(case, "enum")).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 17, "golden enum names must be 17 distinct enums");

    for case in &golden.cases {
        match golden::str_of(case, "enum") {
            "ImageSortMode" => {
                let members = check_members::<ImageSortMode>(case);
                for (member, json) in members.iter().zip(golden_members(case)) {
                    assert_eq!(
                        uses_explorer_order(*member),
                        golden::bool_of(json, "uses_explorer_order"),
                        "ImageSortMode.{}: uses_explorer_order",
                        member.name()
                    );
                }
            }
            "InitialViewMode" => {
                check_members::<InitialViewMode>(case);
            }
            "LoadingMode" => {
                check_members::<LoadingMode>(case);
            }
            "DecoderBackend" => {
                check_members::<DecoderBackend>(case);
            }
            "RawPairMode" => {
                check_members::<RawPairMode>(case);
            }
            "RawFullDecode" => {
                check_members::<RawFullDecode>(case);
            }
            "MouseWheelAction" => {
                check_members::<MouseWheelAction>(case);
            }
            "ScalingQuality" => {
                check_members::<ScalingQuality>(case);
            }
            "ImageTransition" => {
                check_members::<ImageTransition>(case);
            }
            "InstanceMode" => {
                check_members::<InstanceMode>(case);
            }
            "JournalState" => {
                check_members::<JournalState>(case);
            }
            "FileOperationType" => {
                check_members::<FileOperationType>(case);
            }
            "JournalDurability" => {
                check_members::<JournalDurability>(case);
            }
            "FitWidthAnchor" => {
                check_members::<FitWidthAnchor>(case);
            }
            "KeyboardZoomAnchor" => {
                check_members::<KeyboardZoomAnchor>(case);
            }
            "KineticGlideSmoothing" => {
                check_members::<KineticGlideSmoothing>(case);
            }
            "MiddleClickAction" => {
                check_members::<MiddleClickAction>(case);
            }
            other => panic!("golden case for an unknown enum: {other}"),
        }
    }

    // C#: ImageTransitionSettingsTests.Defaults_AreNoneAnd120Ms (the (int) lines only)
    assert_eq!(ImageTransition::None as i32, 0);
    assert_eq!(ImageTransition::Fade as i32, 1);
    // C#: InstanceModeSettingsTests.Default_IsSingleWindow (the (int) line only)
    assert_eq!(InstanceMode::SingleWindow as i32, 0);
    // C#: MouseSettingsTests.Defaults_KeepTodaysWheelAndKinetic_ClickZoomOff (the (int) line only)
    assert_eq!(MouseWheelAction::Zoom as i32, 0);
    // C#: FitWidthHeightKeepZoomSettingsTests.Defaults_KeepZoomOff_FitWidthAnchorCentre_NewShortcutsAreWHK (the (int) line only)
    assert_eq!(FitWidthAnchor::Centre as i32, 0);
}
