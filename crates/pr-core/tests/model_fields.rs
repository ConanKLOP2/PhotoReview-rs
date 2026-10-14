//! Tests for `pr_core::model::fields` (the title bar and photo information line flag sets).
//!
//! Port of `PhotoReview.Core/Model/TitleBarFields.cs` and `ExifInfoFields.cs` (baseline e441d67a), with the enum-level
//! asserts of `TitleBarFieldsSettingsTests` and `ExifInfoSettingsTests`. The serde twins use test-local structs that
//! stand in for `AppSettings`; tests marked "needs W1-02" call `serde_ext::{serialize_flags, deserialize_flags}`,
//! which W1-02 has implemented and merged.

use pr_core::model::serde_ext::FlagEnum;
use pr_core::model::{ExifInfoFields, TitleBarFields};
use pr_test_support::golden::{self, i64_of, str_of};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bit of each `TitleBarFields` member, by its C# name (the constants of the Rust type).
fn title_bar_constant(name: &str) -> Option<u32> {
    let bits = match name {
        "None" => TitleBarFields::NONE,
        "FolderName" => TitleBarFields::FOLDER_NAME,
        "FolderPath" => TitleBarFields::FOLDER_PATH,
        "IndexCount" => TitleBarFields::INDEX_COUNT,
        "FileName" => TitleBarFields::FILE_NAME,
        "FileSize" => TitleBarFields::FILE_SIZE,
        "Dimensions" => TitleBarFields::DIMENSIONS,
        "ModifiedDate" => TitleBarFields::MODIFIED_DATE,
        "DateTaken" => TitleBarFields::DATE_TAKEN,
        "Camera" => TitleBarFields::CAMERA,
        "Lens" => TitleBarFields::LENS,
        "Iso" => TitleBarFields::ISO,
        "FocalLength" => TitleBarFields::FOCAL_LENGTH,
        "Aperture" => TitleBarFields::APERTURE,
        "ShutterSpeed" => TitleBarFields::SHUTTER_SPEED,
        "All" => TitleBarFields::ALL,
        "Default" => TitleBarFields::DEFAULT,
        _ => return None,
    };
    Some(bits.bits())
}

/// Bit of each `ExifInfoFields` member, by its C# name (the constants of the Rust type).
fn exif_info_constant(name: &str) -> Option<u32> {
    let bits = match name {
        "None" => ExifInfoFields::NONE,
        "FileName" => ExifInfoFields::FILE_NAME,
        "DateTaken" => ExifInfoFields::DATE_TAKEN,
        "Dimensions" => ExifInfoFields::DIMENSIONS,
        "Camera" => ExifInfoFields::CAMERA,
        "Lens" => ExifInfoFields::LENS,
        "Iso" => ExifInfoFields::ISO,
        "FocalLength" => ExifInfoFields::FOCAL_LENGTH,
        "Aperture" => ExifInfoFields::APERTURE,
        "ShutterSpeed" => ExifInfoFields::SHUTTER_SPEED,
        "ModifiedDate" => ExifInfoFields::MODIFIED_DATE,
        "All" => ExifInfoFields::ALL,
        "Default" => ExifInfoFields::DEFAULT,
        _ => return None,
    };
    Some(bits.bits())
}

/// Checks one golden case (members in declaration order, `all`, `default`) against a flag type.
fn check_flag_type<T: FlagEnum>(case: &Value, constant: fn(&str) -> Option<u32>) {
    let type_name = str_of(case, "type");
    let members = case["members"]
        .as_array()
        .unwrap_or_else(|| panic!("{type_name}: no members array"));
    assert_eq!(T::NAMES.len(), members.len(), "{type_name}: member count");
    for ((name, bits), member) in T::NAMES.iter().zip(members) {
        let golden_name = str_of(member, "name");
        let golden_value = i64_of(member, "value");
        assert_eq!(*name, golden_name, "{type_name}: name/order of NAMES");
        assert_eq!(i64::from(*bits), golden_value, "{type_name}.{name}: NAMES value");
        assert_eq!(constant(golden_name), Some(*bits), "{type_name}.{name}: constant value");
    }
    assert_eq!(i64::from(T::ALL_BITS), i64_of(case, "all"), "{type_name}: All");
    assert_eq!(
        i64::from(T::DEFAULT_BITS),
        i64_of(case, "default"),
        "{type_name}: Default"
    );
}

// C#: TitleBarFieldsSettingsTests.Default_IsFolderNameOnly (the TitleBarFields.Default asserts).
#[test]
fn title_bar_default_is_folder_name_only() {
    assert_eq!(TitleBarFields::DEFAULT, TitleBarFields::FOLDER_NAME);
    assert_eq!(TitleBarFields::default(), TitleBarFields::FOLDER_NAME);
    assert!(TitleBarFields::DEFAULT.contains(TitleBarFields::FOLDER_NAME));
    assert!(!TitleBarFields::DEFAULT.contains(TitleBarFields::FOLDER_PATH));
    assert!(!TitleBarFields::DEFAULT.contains(TitleBarFields::FILE_NAME));
}

// C#: ExifInfoSettingsTests.Defaults_LineOffAndFieldsExcludeFileNameAndDimensions (the ExifInfoFields asserts).
#[test]
fn exif_default_excludes_file_name_and_dimensions() {
    let seven_fields = ExifInfoFields::DATE_TAKEN
        | ExifInfoFields::CAMERA
        | ExifInfoFields::LENS
        | ExifInfoFields::ISO
        | ExifInfoFields::FOCAL_LENGTH
        | ExifInfoFields::APERTURE
        | ExifInfoFields::SHUTTER_SPEED;
    assert_eq!(ExifInfoFields::DEFAULT, seven_fields);
    assert_eq!(ExifInfoFields::default(), seven_fields);
    assert!(!ExifInfoFields::DEFAULT.contains(ExifInfoFields::FILE_NAME));
    assert!(!ExifInfoFields::DEFAULT.contains(ExifInfoFields::DIMENSIONS));
    assert!(!ExifInfoFields::DEFAULT.contains(ExifInfoFields::MODIFIED_DATE));
}

// No C# twin: the golden of the C# oracle (`flag-fields`): every member, All and Default of both types.
#[test]
fn flag_fields_match_golden() {
    let g = golden::load("flag-fields");
    let mut checked = 0;
    for case in &g.cases {
        match str_of(case, "type") {
            "TitleBarFields" => check_flag_type::<TitleBarFields>(case, title_bar_constant),
            "ExifInfoFields" => check_flag_type::<ExifInfoFields>(case, exif_info_constant),
            other => panic!("unknown flag type in golden: {other}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 2, "both flag types must be in the golden");
}

// No C# twin: the operator behaviour the C# code relies on (`|`, `&`, `|=`, HasFlag, `& ~x`).
#[test]
fn operators_behave_like_csharp_flags() {
    let folder_path = TitleBarFields::FOLDER_PATH;
    let file_name = TitleBarFields::FILE_NAME;
    assert_eq!((folder_path | file_name).bits(), 10);
    assert_eq!(folder_path & file_name, TitleBarFields::NONE);
    assert_eq!(folder_path & (folder_path | file_name), folder_path);

    let mut combined = folder_path;
    combined |= file_name;
    assert_eq!(combined, folder_path | file_name);
    assert!(combined.contains(file_name));
    assert!(combined.contains(TitleBarFields::NONE));
    assert!(!folder_path.contains(file_name));

    // C# `All & ~ShutterSpeed`
    assert_eq!(
        TitleBarFields::ALL.difference(TitleBarFields::SHUTTER_SPEED).bits(),
        8191
    );
    assert_eq!(
        ExifInfoFields::ALL.difference(ExifInfoFields::APERTURE).bits(),
        1023 - 128
    );

    assert_eq!(TitleBarFields::from_bits_truncate(u32::MAX), TitleBarFields::ALL);
    assert_eq!(ExifInfoFields::from_bits_truncate(u32::MAX), ExifInfoFields::ALL);
    assert_eq!(TitleBarFields::from_bits_truncate((1 << 20) | 2), folder_path);
    assert_eq!(ExifInfoFields::from_bits_truncate(4095), ExifInfoFields::ALL);
}

/// Stands in for the `TitleBarFields` part of `AppSettings` in the lenient-read and round-trip tests.
#[derive(Debug, Serialize, Deserialize)]
struct TitleBarCfg {
    #[serde(default, rename = "TitleBarFields")]
    fields: TitleBarFields,
    #[serde(default, rename = "CompareSizeEnabled")]
    compare_size_enabled: bool,
}

/// Stands in for the `ExifInfoFields` part of `AppSettings` in the lenient-read and round-trip tests.
#[derive(Debug, Serialize, Deserialize)]
struct ExifInfoCfg {
    #[serde(default, rename = "ExifInfoFields")]
    fields: ExifInfoFields,
    #[serde(default, rename = "CompareSizeEnabled")]
    compare_size_enabled: bool,
}

// C#: TitleBarFieldsSettingsTests.Load_LenientFieldValues (serde twin; the rows are the C# InlineData rows).
// needs W1-02 (deserialize_flags)
#[test]
fn title_bar_lenient_field_values() {
    let rows: [(&str, TitleBarFields); 6] = [
        (
            r#""folderpath, FILENAME""#,
            TitleBarFields::FOLDER_PATH | TitleBarFields::FILE_NAME,
        ),
        ("3", TitleBarFields::FOLDER_NAME | TitleBarFields::FOLDER_PATH),
        (r#""Nonsense""#, TitleBarFields::DEFAULT), // unknown name: default, not a failed load
        ("[1, 2]", TitleBarFields::DEFAULT),
        ("null", TitleBarFields::DEFAULT),
        (r#""None""#, TitleBarFields::NONE),
    ];
    for (json_value, expected) in rows {
        let text = format!(r#"{{ "ConfigVersion": 3, "TitleBarFields": {json_value}, "CompareSizeEnabled": false }}"#);
        let loaded: TitleBarCfg = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(loaded.fields, expected, "TitleBarFields from {json_value}");
        assert!(
            !loaded.compare_size_enabled,
            "the rest of the config still loads ({json_value})"
        );
    }
}

// C#: TitleBarFieldsSettingsTests.Load_OldConfigWithoutTheSetting_UsesDefault (serde twin with a test-local struct).
// needs W1-02 (deserialize_flags)
#[test]
fn title_bar_missing_property_uses_default() {
    let text = r#"{ "ConfigVersion": 3, "CompareHashEnabled": false }"#;
    let loaded: TitleBarCfg = serde_json::from_str(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    assert_eq!(loaded.fields, TitleBarFields::DEFAULT);
}

// C#: TitleBarFieldsSettingsTests.SaveThenLoad_RoundTrips (serde twin; the rows are the C# InlineData rows).
// needs W1-02 (serialize_flags, deserialize_flags)
#[test]
fn title_bar_save_then_load_round_trips() {
    let rows = [
        TitleBarFields::ALL,
        TitleBarFields::NONE,
        TitleBarFields::FOLDER_PATH | TitleBarFields::FILE_NAME | TitleBarFields::FILE_SIZE,
        TitleBarFields::INDEX_COUNT | TitleBarFields::DATE_TAKEN,
        TitleBarFields::ALL.difference(TitleBarFields::SHUTTER_SPEED),
    ];
    for fields in rows {
        let cfg = TitleBarCfg {
            fields,
            compare_size_enabled: false,
        };
        let text = serde_json::to_string_pretty(&cfg).unwrap_or_else(|e| panic!("write {fields:?}: {e}"));
        let reloaded: TitleBarCfg = serde_json::from_str(&text).unwrap_or_else(|e| panic!("read {text}: {e}"));
        assert_eq!(reloaded.fields, fields, "round trip of {fields:?}");
    }
}

// C#: TitleBarFieldsSettingsTests.Save_WritesReadableFieldNames (serde twin).
// needs W1-02 (serialize_flags)
#[test]
fn title_bar_save_writes_readable_names() {
    let cfg = TitleBarCfg {
        fields: TitleBarFields::FOLDER_NAME | TitleBarFields::FILE_NAME,
        compare_size_enabled: false,
    };
    let json = serde_json::to_string_pretty(&cfg).unwrap_or_else(|e| panic!("write: {e}"));
    assert!(json.contains(r#""TitleBarFields": "FolderName, FileName""#), "{json}");
}

// C#: ExifInfoSettingsTests.Load_LenientFieldValues (serde twin; the rows are the C# InlineData rows).
// needs W1-02 (deserialize_flags)
#[test]
fn exif_lenient_field_values() {
    let rows: [(&str, ExifInfoFields); 10] = [
        (r#""camera, LENS""#, ExifInfoFields::CAMERA | ExifInfoFields::LENS),
        ("12", ExifInfoFields::DIMENSIONS | ExifInfoFields::CAMERA),
        ("4095", ExifInfoFields::ALL), // unknown bits dropped
        (
            r#""iso, ShutterSpeed""#,
            ExifInfoFields::ISO | ExifInfoFields::SHUTTER_SPEED,
        ),
        (r#""Exposure""#, ExifInfoFields::DEFAULT), // never-shipped grouped name: unknown = default
        (r#""Shutter""#, ExifInfoFields::DEFAULT),  // unknown name: default, not a failed load
        (r#""Aperture""#, ExifInfoFields::APERTURE),
        ("[1, 2]", ExifInfoFields::DEFAULT),
        ("null", ExifInfoFields::DEFAULT),
        (r#""None""#, ExifInfoFields::NONE),
    ];
    for (json_value, expected) in rows {
        let text = format!(r#"{{ "ConfigVersion": 3, "ExifInfoFields": {json_value}, "CompareSizeEnabled": false }}"#);
        let loaded: ExifInfoCfg = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(loaded.fields, expected, "ExifInfoFields from {json_value}");
        assert!(
            !loaded.compare_size_enabled,
            "the rest of the config still loads ({json_value})"
        );
    }
}

// C#: ExifInfoSettingsTests.Load_OldConfigWithoutTheSettings_UsesDefaults (serde twin with a test-local struct).
// needs W1-02 (deserialize_flags)
#[test]
fn exif_missing_property_uses_default() {
    let text = r#"{ "ConfigVersion": 3, "CompareHashEnabled": false }"#;
    let loaded: ExifInfoCfg = serde_json::from_str(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    assert_eq!(loaded.fields, ExifInfoFields::DEFAULT);
}

// C#: ExifInfoSettingsTests.SaveThenLoad_RoundTrips (serde twin; the field rows are the C# InlineData rows).
// needs W1-02 (serialize_flags, deserialize_flags)
#[test]
fn exif_save_then_load_round_trips() {
    let rows = [
        ExifInfoFields::ALL,
        ExifInfoFields::NONE,
        ExifInfoFields::FILE_NAME | ExifInfoFields::ISO | ExifInfoFields::SHUTTER_SPEED,
        ExifInfoFields::FOCAL_LENGTH | ExifInfoFields::APERTURE,
        ExifInfoFields::ALL.difference(ExifInfoFields::APERTURE),
        ExifInfoFields::CAMERA | ExifInfoFields::LENS | ExifInfoFields::DATE_TAKEN,
    ];
    for fields in rows {
        let cfg = ExifInfoCfg {
            fields,
            compare_size_enabled: false,
        };
        let text = serde_json::to_string_pretty(&cfg).unwrap_or_else(|e| panic!("write {fields:?}: {e}"));
        let reloaded: ExifInfoCfg = serde_json::from_str(&text).unwrap_or_else(|e| panic!("read {text}: {e}"));
        assert_eq!(reloaded.fields, fields, "round trip of {fields:?}");
    }
}

// C#: ExifInfoSettingsTests.Save_WritesReadableFieldNames (serde twin).
// needs W1-02 (serialize_flags)
#[test]
fn exif_save_writes_readable_names() {
    let cfg = ExifInfoCfg {
        fields: ExifInfoFields::FILE_NAME | ExifInfoFields::LENS,
        compare_size_enabled: false,
    };
    let json = serde_json::to_string_pretty(&cfg).unwrap_or_else(|e| panic!("write: {e}"));
    assert!(json.contains(r#""ExifInfoFields": "FileName, Lens""#), "{json}");
}
