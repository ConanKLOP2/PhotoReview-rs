//! W1-02: lenient enum / flag JSON (`pr_core::model::serde_ext`) against the C# tests and the golden files.
//!
//! DIVERGENCE (accepted, contracts/README.md D5): serde_json reads the bare token `-0` as a float, so it is unreadable
//! (plain: default, settings: `None`, flags: default) where System.Text.Json reads the integer 0. The text `"-0"` inside
//! a JSON string goes through the string path and matches C#. The golden inputs contain no bare `-0`.

use std::collections::HashMap;

use pr_core::model::serde_ext::{
    FlagEnum, LenientEnum, deserialize_lenient_or, deserialize_settings_enum, format_flags, parse_flags_text,
    parse_lenient_text, serialize_settings_enum,
};
use pr_core::model::{
    DecoderBackend, ExifInfoFields, FileOperationType, FitWidthAnchor, ImageSortMode, ImageTransition, InitialViewMode,
    InstanceMode, JournalDurability, JournalState, KeyboardZoomAnchor, KineticGlideSmoothing, LoadingMode,
    MouseWheelAction, RawFullDecode, RawPairMode, ScalingQuality, TitleBarFields,
};
use pr_test_support::golden::{self, str_of};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------- helpers

fn read<T: for<'de> Deserialize<'de>>(json: &str) -> T {
    serde_json::from_str(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn write<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serialize")
}

fn read_settings<T: LenientEnum>(token: &str) -> Result<Option<T>, serde_json::Error> {
    let mut de = serde_json::Deserializer::from_str(token);
    let value = deserialize_settings_enum::<T, _>(&mut de)?;
    de.end()?;
    Ok(value)
}

fn read_custom<T: LenientEnum>(token: &str, default: T) -> Result<T, serde_json::Error> {
    let mut de = serde_json::Deserializer::from_str(token);
    let value = deserialize_lenient_or::<T, _>(&mut de, default)?;
    de.end()?;
    Ok(value)
}

fn read_key<T: LenientEnum + for<'de> Deserialize<'de> + std::hash::Hash>(
    string_token: &str,
) -> Result<Option<T>, serde_json::Error> {
    let map: HashMap<T, i64> = serde_json::from_str(&format!("{{{string_token}:1}}"))?;
    Ok(map.keys().next().copied())
}

fn member_named<T: LenientEnum>(name: &str) -> T {
    T::MEMBERS
        .iter()
        .copied()
        .find(|m| m.name() == name)
        .unwrap_or_else(|| panic!("{} has no member {name}", T::TYPE_NAME))
}

/// Calls `$f::<Type>($args...)` for the enum type named by `$name`.
macro_rules! with_enum {
    ($name:expr, $f:ident, $($arg:expr),* $(,)?) => {
        match $name {
            "ImageSortMode" => $f::<ImageSortMode>($($arg),*),
            "InitialViewMode" => $f::<InitialViewMode>($($arg),*),
            "LoadingMode" => $f::<LoadingMode>($($arg),*),
            "DecoderBackend" => $f::<DecoderBackend>($($arg),*),
            "RawPairMode" => $f::<RawPairMode>($($arg),*),
            "RawFullDecode" => $f::<RawFullDecode>($($arg),*),
            "MouseWheelAction" => $f::<MouseWheelAction>($($arg),*),
            "ScalingQuality" => $f::<ScalingQuality>($($arg),*),
            "ImageTransition" => $f::<ImageTransition>($($arg),*),
            "InstanceMode" => $f::<InstanceMode>($($arg),*),
            "JournalState" => $f::<JournalState>($($arg),*),
            "FileOperationType" => $f::<FileOperationType>($($arg),*),
            "JournalDurability" => $f::<JournalDurability>($($arg),*),
            "FitWidthAnchor" => $f::<FitWidthAnchor>($($arg),*),
            "KeyboardZoomAnchor" => $f::<KeyboardZoomAnchor>($($arg),*),
            "KineticGlideSmoothing" => $f::<KineticGlideSmoothing>($($arg),*),
            other => panic!("unknown enum {other}"),
        }
    };
}

/// Runs `$body` once per enum type with `$t` bound to the type.
macro_rules! for_each_enum {
    (|$t:ident| $body:block) => {{
        macro_rules! one {
            ($ty:ty) => {{
                type $t = $ty;
                $body
            }};
        }
        one!(ImageSortMode);
        one!(InitialViewMode);
        one!(LoadingMode);
        one!(DecoderBackend);
        one!(RawPairMode);
        one!(RawFullDecode);
        one!(MouseWheelAction);
        one!(ScalingQuality);
        one!(ImageTransition);
        one!(InstanceMode);
        one!(JournalState);
        one!(FileOperationType);
        one!(JournalDurability);
        one!(FitWidthAnchor);
        one!(KeyboardZoomAnchor);
        one!(KineticGlideSmoothing);
    }};
}

const ENUM_NAMES: [&str; 16] = [
    "ImageSortMode",
    "InitialViewMode",
    "LoadingMode",
    "DecoderBackend",
    "RawPairMode",
    "RawFullDecode",
    "MouseWheelAction",
    "ScalingQuality",
    "ImageTransition",
    "InstanceMode",
    "JournalState",
    "FileOperationType",
    "JournalDurability",
    "FitWidthAnchor",
    "KeyboardZoomAnchor",
    "KineticGlideSmoothing",
];

// ---------------------------------------------------------------- LenientEnumConverterTests

// C#: LenientEnumConverterTests.ReadLoadingModeMatchesStandardNamesCaseInsensitive
#[test]
fn read_loading_mode_matches_standard_names_case_insensitive() {
    let rows = [
        ("\"Preview\"", LoadingMode::Preview),
        ("\"preview\"", LoadingMode::Preview),
        ("\"PREVIEW\"", LoadingMode::Preview),
        ("\"Original\"", LoadingMode::Original),
        ("\"original\"", LoadingMode::Original),
        ("\"Fast\"", LoadingMode::Fast),
        ("\"fast\"", LoadingMode::Fast),
    ];
    for (json, expected) in rows {
        assert_eq!(read::<LoadingMode>(json), expected, "row {json}");
    }
}

// C#: LenientEnumConverterTests.ReadImageSortModeResolvesAliasesAndNames
#[test]
fn read_image_sort_mode_resolves_aliases_and_names() {
    let rows = [
        ("\"Size\"", ImageSortMode::SizeDescending),
        ("\"size\"", ImageSortMode::SizeDescending),
        ("\"PortraitFirst\"", ImageSortMode::Name),
        ("\"portraitfirst\"", ImageSortMode::Name),
        ("\"Name\"", ImageSortMode::Name),
        ("\"SizeAscending\"", ImageSortMode::SizeAscending),
    ];
    for (json, expected) in rows {
        assert_eq!(read::<ImageSortMode>(json), expected, "row {json}");
    }
}

// C#: LenientEnumConverterTests.ReadFileOperationTypeResolvesDeleteAliasToRecycle
#[test]
fn read_file_operation_type_resolves_delete_alias_to_recycle() {
    let rows = [
        ("\"Delete\"", FileOperationType::Recycle),
        ("\"delete\"", FileOperationType::Recycle),
        ("\"Recycle\"", FileOperationType::Recycle),
        ("\"Move\"", FileOperationType::Move),
        ("\"Copy\"", FileOperationType::Copy),
    ];
    for (json, expected) in rows {
        assert_eq!(read::<FileOperationType>(json), expected, "row {json}");
    }
}

// C#: LenientEnumConverterTests.ReadInitialViewModeResolvesPercentageAliasesAndNames
#[test]
fn read_initial_view_mode_resolves_percentage_aliases_and_names() {
    let rows = [
        ("\"Fit\"", InitialViewMode::Fit),
        ("\"fit\"", InitialViewMode::Fit),
        ("\"100%\"", InitialViewMode::Percent100),
        ("\"200%\"", InitialViewMode::Percent200),
        ("\"400%\"", InitialViewMode::Percent400),
        ("\"Percent100\"", InitialViewMode::Percent100),
        ("\"percent200\"", InitialViewMode::Percent200),
        ("\"Percent400\"", InitialViewMode::Percent400),
    ];
    for (json, expected) in rows {
        assert_eq!(read::<InitialViewMode>(json), expected, "row {json}");
    }
}

// C#: LenientEnumConverterTests.ReadJournalStateResolvesCorrectly
#[test]
fn read_journal_state_resolves_correctly() {
    let rows = [
        ("\"Prepared\"", JournalState::Prepared),
        ("\"Committed\"", JournalState::Committed),
        ("\"Failed\"", JournalState::Failed),
        ("\"committed\"", JournalState::Committed),
    ];
    for (json, expected) in rows {
        assert_eq!(read::<JournalState>(json), expected, "row {json}");
    }
}

// C#: LenientEnumConverterTests.ReadDecoderBackendResolvesCorrectly
#[test]
fn read_decoder_backend_resolves_correctly() {
    let rows = [
        ("\"Wpf\"", DecoderBackend::Wpf),
        ("\"WicDirect\"", DecoderBackend::WicDirect),
        ("\"TurboJpeg\"", DecoderBackend::TurboJpeg),
        ("\"turbojpeg\"", DecoderBackend::TurboJpeg),
    ];
    for (json, expected) in rows {
        assert_eq!(read::<DecoderBackend>(json), expected, "row {json}");
    }
}

// C#: LenientEnumConverterTests.ReadScalingQualityResolvesCorrectly
#[test]
fn read_scaling_quality_resolves_correctly() {
    let rows = [
        ("\"Linear\"", ScalingQuality::Linear),
        ("\"HighQuality\"", ScalingQuality::HighQuality),
        ("\"highquality\"", ScalingQuality::HighQuality),
    ];
    for (json, expected) in rows {
        assert_eq!(read::<ScalingQuality>(json), expected, "row {json}");
    }
}

// C#: LenientEnumConverterTests.ReadUnrecognizedOrEmptyFallsBackToDefault
#[test]
fn read_unrecognized_or_empty_falls_back_to_default() {
    for json in ["\"\"", "\"   \"", "\"UnknownValue\"", "\"Gibberish123!@#\"", "null"] {
        assert_eq!(read::<LoadingMode>(json), LoadingMode::Fast, "row {json}");
        assert_eq!(read::<ImageSortMode>(json), ImageSortMode::Name, "row {json}");
        assert_eq!(read::<InitialViewMode>(json), InitialViewMode::Fit, "row {json}");
        assert_eq!(read::<FileOperationType>(json), FileOperationType::Move, "row {json}");
    }
}

// C#: LenientEnumConverterTests.ReadCustomDefaultValueReturnsSpecifiedDefault
#[test]
fn read_custom_default_value_returns_specified_default() {
    let result = read_custom("\"InvalidTrashValue\"", LoadingMode::Preview).expect("read");
    assert_eq!(result, LoadingMode::Preview);
}

// C#: LenientEnumConverterTests.ReadValidIntegerMapsCorrectly
#[test]
fn read_valid_integer_maps_correctly() {
    for (json, expected) in [
        ("0", LoadingMode::Fast),
        ("1", LoadingMode::Preview),
        ("2", LoadingMode::Original),
    ] {
        assert_eq!(read::<LoadingMode>(json), expected, "row {json}");
    }
}

// C#: LenientEnumConverterTests.ReadInvalidIntegerFallsBackToDefault
#[test]
fn read_invalid_integer_falls_back_to_default() {
    for json in ["999", "-1"] {
        assert_eq!(read::<LoadingMode>(json), LoadingMode::Fast, "row {json}");
    }
}

// C#: LenientEnumConverterTests.ReadUnexpectedJsonTokensDoesNotCrashAndFallsBackToDefault
#[test]
fn read_unexpected_json_tokens_does_not_crash_and_falls_back_to_default() {
    for json in ["true", "false", "{\"foo\": \"bar\"}", "[1, 2, 3]"] {
        assert_eq!(read::<LoadingMode>(json), LoadingMode::Fast, "row {json}");
    }
}

// C#: LenientEnumConverterTests.WriteInitialViewModeOutputsLegacyCompatibleStrings
#[test]
fn write_initial_view_mode_outputs_legacy_compatible_strings() {
    assert_eq!(write(&InitialViewMode::Fit), "\"Fit\"");
    assert_eq!(write(&InitialViewMode::Percent100), "\"100%\"");
    assert_eq!(write(&InitialViewMode::Percent200), "\"200%\"");
    assert_eq!(write(&InitialViewMode::Percent400), "\"400%\"");
}

// C#: LenientEnumConverterTests.WriteOtherEnumsOutputsStandardPascalCaseNames
#[test]
fn write_other_enums_outputs_standard_pascal_case_names() {
    assert_eq!(write(&LoadingMode::Preview), "\"Preview\"");
    assert_eq!(write(&ImageSortMode::SizeDescending), "\"SizeDescending\"");
    assert_eq!(write(&FileOperationType::Recycle), "\"Recycle\"");
    assert_eq!(write(&JournalState::Committed), "\"Committed\"");
    assert_eq!(write(&DecoderBackend::TurboJpeg), "\"TurboJpeg\"");
    assert_eq!(write(&ScalingQuality::HighQuality), "\"HighQuality\"");
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TestConfig {
    loading_mode: LoadingMode,
    image_sort_mode: ImageSortMode,
    initial_view_mode: InitialViewMode,
    file_operation: FileOperationType,
    journal_state: JournalState,
    decoder_backend: DecoderBackend,
    scaling_quality: ScalingQuality,
}

// C#: LenientEnumConverterTests.RoundTripRecordWithAllEnumsPreservesValues
#[test]
fn round_trip_record_with_all_enums_preserves_values() {
    let original = TestConfig {
        loading_mode: LoadingMode::Original,
        image_sort_mode: ImageSortMode::SizeAscending,
        initial_view_mode: InitialViewMode::Percent200,
        file_operation: FileOperationType::Recycle,
        journal_state: JournalState::Committed,
        decoder_backend: DecoderBackend::TurboJpeg,
        scaling_quality: ScalingQuality::HighQuality,
    };
    let json = write(&original);
    let restored: TestConfig = read(&json);
    assert_eq!(restored, original);
}

// C#: LenientEnumConverterTests.EnumDictionaryKey_RoundTripsThroughTheEnumAttributeConverter
#[test]
fn enum_dictionary_key_round_trips_through_the_enum_attribute_converter() {
    let source: HashMap<DecoderBackend, i64> =
        HashMap::from([(DecoderBackend::TurboJpeg, 3), (DecoderBackend::WicDirect, 1)]);
    let json = write(&source);
    let result: HashMap<DecoderBackend, i64> = read(&json);
    assert!(json.contains("\"TurboJpeg\":3"), "{json}");
    assert_eq!(result, source);
}

// C#: LenientEnumConverterTests.ReviewMetricsSnapshot_WithDecoderFallbacks_Serializes
// (only the dictionary serialization; ReviewMetrics itself is another unit)
#[test]
fn review_metrics_snapshot_with_decoder_fallbacks_serializes() {
    let fallbacks: HashMap<DecoderBackend, u64> = HashMap::from([(DecoderBackend::WicDirect, 1)]);
    let json = write(&fallbacks);
    assert!(json.contains("\"WicDirect\":1"), "{json}");
}

// ---------------------------------------------------------------- LenientEnumNumericStringTests

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Shade {
    Dark,
    Mid,
    Light,
}

impl LenientEnum for Shade {
    const TYPE_NAME: &'static str = "Shade";
    const MEMBERS: &'static [Self] = &[Self::Dark, Self::Mid, Self::Light];

    fn name(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Mid => "Mid",
            Self::Light => "Light",
        }
    }

    fn value(self) -> i32 {
        match self {
            Self::Dark => 0,
            Self::Mid => 2,
            Self::Light => 5,
        }
    }

    fn aliases(self) -> &'static [&'static str] {
        &[]
    }

    fn zero() -> Self {
        Self::Dark
    }
}

impl<'de> Deserialize<'de> for Shade {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        deserialize_lenient_or(d, Shade::Light)
    }
}

// C#: LenientEnumNumericStringTests.Read_NumericString_ResolvesDefinedValue
#[test]
fn read_numeric_string_resolves_defined_value() {
    assert_eq!(read::<Shade>("\"2\""), Shade::Mid);
    assert_eq!(read::<Shade>("\" 0 \""), Shade::Dark);
    assert_eq!(read::<Shade>("\"3\""), Shade::Light);
}

// C#: LenientEnumNumericStringTests.ReadAsPropertyName_NumericKey_ResolvesDefinedValue
#[test]
fn read_as_property_name_numeric_key_resolves_defined_value() {
    let map: HashMap<Shade, i32> = read(r#"{ "2": 7 }"#);
    assert_eq!(map.get(&Shade::Mid), Some(&7));
}

// ---------------------------------------------------------------- LenientEnumRobustnessTests

const TOKENS: &[&str] = &[
    "null",
    "true",
    "false",
    "0",
    "1",
    "2",
    "3",
    "-1",
    "99",
    "2147483647",
    "2147483648",
    "9223372036854775807",
    "1.5",
    "1e2",
    "-0",
    "\"\"",
    "\"  \"",
    "\"garbage\"",
    "\"0\"",
    "\"1\"",
    "\" 1 \"",
    "\"-1\"",
    "\"99999999999\"",
    "\"NAME\"",
    "\"name\"",
    "\"\u{130}\"",
    "[]",
    "{}",
    "[1,[2]]",
    "{\"a\":{\"b\":[1,2,{}]}}",
    "\"\\ud800\"",
    "\"\\u0000\"",
    "\"100%\"",
    "\"Size\"",
    "\"Delete\"",
    "\"PortraitFirst\"",
];

// C#: LenientEnumRobustnessTests.LenientEnums_AreDiscovered
#[test]
fn lenient_enums_are_discovered() {
    let mut names = Vec::new();
    for_each_enum!(|E| {
        names.push(<E as LenientEnum>::TYPE_NAME);
    });
    assert_eq!(names, ENUM_NAMES);
    assert!(names.len() >= 8);
}

// C#: LenientEnumRobustnessTests.AnyToken_ReadsAsDefinedValue
#[test]
fn any_token_reads_as_defined_value() {
    for_each_enum!(|E| {
        for token in TOKENS {
            // Only a lone surrogate escape may fail (a JsonException in C#); every other token reads a defined member.
            match serde_json::from_str::<E>(token) {
                Ok(value) => assert!(E::MEMBERS.contains(&value), "{} <- {token}", E::TYPE_NAME),
                Err(_) => assert_eq!(*token, "\"\\ud800\"", "{} <- {token}", E::TYPE_NAME),
            }
        }
    });
}

// C#: LenientEnumRobustnessTests.DefinedMembers_RoundTrip
#[test]
fn defined_members_round_trip() {
    for_each_enum!(|E| {
        for &member in E::MEMBERS {
            let json = write(&member);
            assert_eq!(read::<E>(&json), member, "{} {json}", E::TYPE_NAME);

            let dict: HashMap<E, i64> = HashMap::from([(member, 7)]);
            let back: HashMap<E, i64> = read(&write(&dict));
            assert_eq!(back.get(&member), Some(&7), "{} key {}", E::TYPE_NAME, member.name());
        }
    });
}

// C#: LenientEnumRobustnessTests.Names_CaseInsensitive
#[test]
fn names_case_insensitive() {
    for_each_enum!(|E| {
        for &member in E::MEMBERS {
            let name = member.name();
            let upper = format!("\"  {} \"", name.to_ascii_uppercase());
            let lower = format!("\"{}\"", name.to_ascii_lowercase());
            assert_eq!(read::<E>(&upper), member, "{} {upper}", E::TYPE_NAME);
            assert_eq!(read::<E>(&lower), member, "{} {lower}", E::TYPE_NAME);
        }
    });
}

fn flag_robust<F>(extra: &[&str], exact_default: &str, exact_default_text: &str, pair_text: &str, pair: u32)
where
    F: FlagEnum + for<'de> Deserialize<'de>,
{
    for token in TOKENS.iter().chain(extra) {
        match serde_json::from_str::<F>(token) {
            Ok(value) => assert_eq!(value.bits() & F::ALL_BITS, value.bits(), "{} <- {token}", F::TYPE_NAME),
            Err(_) => assert_eq!(*token, "\"\\ud800\"", "{} <- {token}", F::TYPE_NAME),
        }
    }
    // W2-CC-03: negative = unreadable, not "everything on".
    assert_eq!(read::<F>(exact_default).bits(), F::DEFAULT_BITS, "{exact_default_text}");
    assert_eq!(read::<F>(pair_text).bits(), pair, "{pair_text}");
}

// C#: LenientEnumRobustnessTests.ExifInfoFields_Robust
#[test]
fn exif_info_fields_robust() {
    let extra = [
        "\"FileName, Camera\"",
        "\"filename,LENS\"",
        "\"All\"",
        "\"None\"",
        "-1",
        "1024",
        "9223372036854775807",
        "\"1, 2\"",
    ];
    flag_robust::<ExifInfoFields>(
        &extra,
        "-1",
        "-1 gives Default",
        "\"filename, CAMERA\"",
        (ExifInfoFields::FILE_NAME | ExifInfoFields::CAMERA).bits(),
    );
}

// C#: LenientEnumRobustnessTests.TitleBarFields_Robust
#[test]
fn title_bar_fields_robust() {
    let extra = [
        "\"FolderName, FileName\"",
        "\"foldername,filesize\"",
        "\"All\"",
        "\"None\"",
        "-1",
        "1024",
        "9223372036854775807",
        "\"1, 2\"",
    ];
    flag_robust::<TitleBarFields>(
        &extra,
        "-1",
        "-1 gives Default",
        "\"foldername, FILESIZE\"",
        (TitleBarFields::FOLDER_NAME | TitleBarFields::FILE_SIZE).bits(),
    );
}

// ---------------------------------------------------------------- FlagEnumJsonTests

// C#: FlagEnumJsonTests.Exif_Default_WritesExplicitNames
#[test]
fn exif_default_writes_explicit_names() {
    let json = write(&ExifInfoFields::DEFAULT);
    assert_eq!(
        json,
        "\"DateTaken, Camera, Lens, Iso, FocalLength, Aperture, ShutterSpeed\""
    );
    assert_eq!(read::<ExifInfoFields>(&json), ExifInfoFields::DEFAULT);
}

// C#: FlagEnumJsonTests.Exif_AllAndNone_Write
#[test]
fn exif_all_and_none_write() {
    let all = write(&ExifInfoFields::ALL);
    assert!(!all.contains("All"), "{all}");
    assert!(all.contains("ModifiedDate"), "{all}");
    assert_eq!(read::<ExifInfoFields>(&all), ExifInfoFields::ALL);
    assert_eq!(write(&ExifInfoFields::NONE), "\"None\"");
    assert_eq!(read::<ExifInfoFields>("\"None\""), ExifInfoFields::NONE);
}

// C#: FlagEnumJsonTests.TitleBar_Default_WritesFolderName
#[test]
fn title_bar_default_writes_folder_name() {
    assert_eq!(write(&TitleBarFields::DEFAULT), "\"FolderName\"");
    assert!(!write(&TitleBarFields::ALL).contains("All"));
    assert_eq!(
        read::<TitleBarFields>(&write(&TitleBarFields::ALL)),
        TitleBarFields::ALL
    );
}

// C#: FlagEnumJsonTests.OldAliasNamesAndNumbers_StillRead
#[test]
fn old_alias_names_and_numbers_still_read() {
    assert_eq!(read::<ExifInfoFields>("\"All\""), ExifInfoFields::ALL);
    assert_eq!(read::<ExifInfoFields>("\"Default\""), ExifInfoFields::DEFAULT);
    assert_eq!(read::<TitleBarFields>("\"Default\""), TitleBarFields::DEFAULT);
    assert_eq!(
        read::<TitleBarFields>("9"),
        TitleBarFields::FOLDER_NAME | TitleBarFields::FILE_NAME
    );
    assert_eq!(
        read::<ExifInfoFields>("\"FileName, Camera\""),
        ExifInfoFields::FILE_NAME | ExifInfoFields::CAMERA
    );
}

// C#: FlagEnumJsonTests.UnknownName_KeepsTheValidOnes
#[test]
fn unknown_name_keeps_the_valid_ones() {
    assert_eq!(read::<ExifInfoFields>("\"FileName, Foo\""), ExifInfoFields::FILE_NAME);
    assert_eq!(
        read::<TitleBarFields>("\"iso , Bogus,FileName\""),
        TitleBarFields::FILE_NAME | TitleBarFields::ISO
    );
    assert_eq!(read::<ExifInfoFields>("\"Foo, Bar\""), ExifInfoFields::DEFAULT);
}

// C#: FlagEnumJsonTests.NegativeNumbers_AreUnreadable
#[test]
fn negative_numbers_are_unreadable() {
    for token in ["-1", "-512", "\"-1\""] {
        assert_eq!(read::<ExifInfoFields>(token), ExifInfoFields::DEFAULT, "row {token}");
        assert_eq!(read::<TitleBarFields>(token), TitleBarFields::DEFAULT, "row {token}");
    }
}

// ---------------------------------------------------------------- direct tests

struct SettingsOut(Option<LoadingMode>);

impl Serialize for SettingsOut {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serialize_settings_enum(&self.0, s)
    }
}

#[test]
fn settings_enum_none_is_written_as_sentinel_text() {
    assert_eq!(write(&SettingsOut(None)), "\"-2147483648\"");
    assert_eq!(write(&SettingsOut(Some(LoadingMode::Preview))), "\"Preview\"");
}

#[test]
fn custom_default_is_not_the_zero_member() {
    // The zero member stays readable as itself even with a different default.
    assert_eq!(
        read_custom("0", LoadingMode::Original).expect("read"),
        LoadingMode::Fast
    );
    assert_eq!(
        read_custom("\"Fast\"", LoadingMode::Original).expect("read"),
        LoadingMode::Fast
    );
    assert_eq!(
        read_custom("null", LoadingMode::Original).expect("read"),
        LoadingMode::Original
    );
    assert_eq!(
        read_custom("[1,{\"a\":2}]", LoadingMode::Original).expect("read"),
        LoadingMode::Original
    );
    // Settings: the zero member is a real result, garbage is None.
    assert_eq!(
        read_settings::<LoadingMode>("0").expect("read"),
        Some(LoadingMode::Fast)
    );
    assert_eq!(read_settings::<LoadingMode>("\"nope\"").expect("read"), None);
}

#[test]
fn blank_and_nul_edge_cases() {
    // (text, expected) rows documenting the .NET int.TryParse / Trim / OrdinalIgnoreCase rules.
    let rows: [(&str, Option<LoadingMode>); 14] = [
        ("1\0", Some(LoadingMode::Preview)),
        ("1 \0", Some(LoadingMode::Preview)),
        ("1\0\0", Some(LoadingMode::Preview)),
        ("+1", Some(LoadingMode::Preview)),
        ("001", Some(LoadingMode::Preview)),
        ("-0", Some(LoadingMode::Fast)),
        ("\u{a0}Fast\u{a0}", Some(LoadingMode::Fast)),
        ("\u{3000}Original\u{2003}", Some(LoadingMode::Original)),
        ("\u{200b}Fast", None),
        ("1\0x", None),
        ("\u{0}1", None),
        ("1.0", None),
        ("\u{661}", None),
        ("Fastx", None),
    ];
    for (text, expected) in rows {
        assert_eq!(parse_lenient_text::<LoadingMode>(Some(text)), expected, "row {text:?}");
    }
    assert_eq!(parse_lenient_text::<LoadingMode>(None), None);
    assert_eq!(parse_lenient_text::<LoadingMode>(Some("")), None);
    assert_eq!(parse_lenient_text::<LoadingMode>(Some(" \t\n")), None);
    // Non-ASCII look-alikes of ASCII letters do not match (dotless i, long s).
    assert_eq!(
        parse_lenient_text::<ScalingQuality>(Some("Linear")),
        Some(ScalingQuality::Linear)
    );
    assert_eq!(parse_lenient_text::<ImageSortMode>(Some("PortraitF\u{131}rst")), None);
    assert_eq!(parse_lenient_text::<ImageSortMode>(Some("Name\u{17f}")), None);
    assert_eq!(parse_lenient_text::<JournalState>(Some("Commi\u{17f}tted")), None);
    assert_eq!(
        parse_lenient_text::<JournalState>(Some("COMMITTED")),
        Some(JournalState::Committed)
    );
    // Flags: the same number grammar.
    assert_eq!(
        parse_flags_text::<ExifInfoFields>(Some("1\0")),
        ExifInfoFields::FILE_NAME
    );
    assert_eq!(parse_flags_text::<ExifInfoFields>(Some("-0")), ExifInfoFields::NONE);
    assert_eq!(parse_flags_text::<ExifInfoFields>(Some("1024")), ExifInfoFields::NONE);
    assert_eq!(
        parse_flags_text::<ExifInfoFields>(Some("2147483648")),
        ExifInfoFields::DEFAULT
    );
    assert_eq!(format_flags(ExifInfoFields::from_bits_truncate(0)), "None");
}

#[test]
fn trailing_whitespace_then_nul_number_rule() {
    // .NET int.TryParse: digits, then any run of ASCII white (0x20, 0x09..=0x0D), then only U+0000 to the end.
    // Trim() does not remove the whitespace here because the NUL after it is not whitespace.
    for text in [
        "1\t\0",
        "1\u{b}\0",
        "1\u{c}\0",
        "1\r\0",
        "1\n\0",
        "1 \0",
        "1 \t\r\n\0\0",
    ] {
        assert_eq!(
            parse_lenient_text::<LoadingMode>(Some(text)),
            Some(LoadingMode::Preview),
            "row {text:?}"
        );
        assert_eq!(
            parse_flags_text::<ExifInfoFields>(Some(text)),
            ExifInfoFields::FILE_NAME,
            "flags row {text:?}"
        );
    }
    // Only U+0000 may follow the whitespace run: white space after a NUL is not allowed.
    for text in ["1\0 \0", "1\0\t\0", "1 \0 \0", "1\0\u{a0}\0"] {
        assert_eq!(parse_lenient_text::<LoadingMode>(Some(text)), None, "row {text:?}");
        assert_eq!(
            parse_flags_text::<ExifInfoFields>(Some(text)),
            ExifInfoFields::DEFAULT,
            "flags row {text:?}"
        );
    }
    // Non-ASCII white space between the digits and the NUL is not .NET trailing white.
    for text in ["1\u{a0}\0", "1\u{3000}\0", "1\u{85}\0"] {
        assert_eq!(parse_lenient_text::<LoadingMode>(Some(text)), None, "row {text:?}");
    }
}

/// Test-local enum exercising lookup-table corners the shipped enums never hit: a name/alias collision, an alias
/// with surrounding white space, a blank alias, a negative member and the `i32::MIN` member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Odd {
    First,
    Second,
    Zero,
}

impl LenientEnum for Odd {
    const TYPE_NAME: &'static str = "Odd";
    const MEMBERS: &'static [Self] = &[Self::First, Self::Second, Self::Zero];

    fn name(self) -> &'static str {
        match self {
            Self::First => "First",
            Self::Second => "Second",
            Self::Zero => "Zero",
        }
    }

    fn value(self) -> i32 {
        match self {
            Self::First => i32::MIN,
            Self::Second => -1,
            Self::Zero => 0,
        }
    }

    fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::First => &[" Padded\t", "   "],
            // Collides with the name of `First`; C# `dict[alias] = value` makes the later entry win.
            Self::Second => &["first"],
            Self::Zero => &[],
        }
    }

    fn zero() -> Self {
        Self::Zero
    }
}

#[test]
fn lookup_table_corners_follow_the_csharp_dictionary() {
    // Later entry wins on a name/alias collision.
    assert_eq!(parse_lenient_text::<Odd>(Some("First")), Some(Odd::Second));
    assert_eq!(parse_lenient_text::<Odd>(Some("FIRST")), Some(Odd::Second));
    // Aliases are trimmed before they enter the table.
    assert_eq!(parse_lenient_text::<Odd>(Some("Padded")), Some(Odd::First));
    assert_eq!(parse_lenient_text::<Odd>(Some("  padded ")), Some(Odd::First));
    // Blank aliases are skipped (and blank input is unreadable anyway).
    assert_eq!(parse_lenient_text::<Odd>(Some("   ")), None);
    // i32 range edges in the text number path.
    assert_eq!(parse_lenient_text::<Odd>(Some("-2147483648")), Some(Odd::First));
    assert_eq!(parse_lenient_text::<Odd>(Some("-2147483649")), None);
    assert_eq!(parse_lenient_text::<Odd>(Some("-1")), Some(Odd::Second));
    assert_eq!(parse_lenient_text::<Odd>(Some("2147483648")), None);
    // Integer tokens: i32::MIN is readable; anything outside i32 (including u64 values that would wrap to a
    // negative member) is unreadable.
    assert_eq!(read_settings::<Odd>("-2147483648").expect("read"), Some(Odd::First));
    assert_eq!(read_settings::<Odd>("-1").expect("read"), Some(Odd::Second));
    assert_eq!(read_settings::<Odd>("18446744073709551615").expect("read"), None);
    assert_eq!(read_settings::<Odd>("18446744071562067968").expect("read"), None);
    assert_eq!(read_settings::<Odd>("9223372036854775808").expect("read"), None);
    assert_eq!(read_settings::<Odd>("-2147483649").expect("read"), None);
    assert_eq!(read_settings::<Odd>("\"-2147483648\"").expect("read"), Some(Odd::First));
}

// ---------------------------------------------------------------- golden

fn golden_read<E>(case: &Value)
where
    E: LenientEnum + for<'de> Deserialize<'de> + std::hash::Hash,
{
    let token = str_of(case, "token");
    let name = |r: Result<E, serde_json::Error>| match r {
        Ok(v) => v.name().to_owned(),
        Err(_) => "<error>".to_owned(),
    };
    assert_eq!(
        name(serde_json::from_str::<E>(token)),
        str_of(case, "plain"),
        "plain {case}"
    );

    let settings = match read_settings::<E>(token) {
        Ok(Some(v)) => v.name().to_owned(),
        Ok(None) => "<undefined>".to_owned(),
        Err(_) => "<error>".to_owned(),
    };
    assert_eq!(settings, str_of(case, "settings"), "settings {case}");

    if let Some(expected) = case.get("key").and_then(Value::as_str) {
        let key = match read_key::<E>(token) {
            Ok(Some(v)) => v.name().to_owned(),
            Ok(None) => "<empty>".to_owned(),
            Err(_) => "<error>".to_owned(),
        };
        assert_eq!(key, expected, "key {case}");
    }
}

fn golden_custom<E>(case: &Value)
where
    E: LenientEnum,
{
    let default = member_named::<E>(str_of(case, "default"));
    let got = match read_custom::<E>(str_of(case, "token"), default) {
        Ok(v) => v.name().to_owned(),
        Err(_) => "<error>".to_owned(),
    };
    assert_eq!(got, str_of(case, "plain"), "custom-default {case}");
}

fn golden_write<E>(case: &Value)
where
    E: LenientEnum + Serialize + std::hash::Hash,
{
    let member = member_named::<E>(str_of(case, "member"));
    let text: String = read(&write(&member));
    assert_eq!(text, str_of(case, "value_text"), "value {case}");
    let dict_json = write(&HashMap::from([(member, 1_i64)]));
    let parsed: Value = read(&dict_json);
    let key = parsed
        .as_object()
        .and_then(|o| o.keys().next().cloned())
        .unwrap_or_default();
    assert_eq!(key, str_of(case, "key_text"), "key {case}");
}

struct SettingsOutOf<T>(Option<T>);

impl<T: LenientEnum> Serialize for SettingsOutOf<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serialize_settings_enum(&self.0, s)
    }
}

fn golden_sentinel<E: LenientEnum>(case: &Value) {
    let text: String = read(&write(&SettingsOutOf::<E>(None)));
    assert_eq!(text, str_of(case, "settings_text"), "sentinel {case}");
}

#[test]
fn lenient_enum_matches_golden() {
    let g = golden::load("lenient-enum");
    let mut counts = [0_usize; 4];
    for case in &g.cases {
        let name = str_of(case, "enum");
        assert!(ENUM_NAMES.contains(&name), "unknown enum in {case}");
        match str_of(case, "kind") {
            "read" => {
                with_enum!(name, golden_read, case);
                counts[0] += 1;
            }
            "custom-default" => {
                with_enum!(name, golden_custom, case);
                counts[1] += 1;
            }
            "write" => {
                with_enum!(name, golden_write, case);
                counts[2] += 1;
            }
            "write-sentinel" => {
                with_enum!(name, golden_sentinel, case);
                counts[3] += 1;
            }
            other => panic!("unknown kind {other}"),
        }
    }
    assert!(
        counts.iter().all(|&c| c > 0),
        "every case kind must be present: {counts:?}"
    );
}

fn golden_flags<F>(case: &Value)
where
    F: FlagEnum + Serialize + for<'de> Deserialize<'de>,
{
    match str_of(case, "kind") {
        "read" => {
            let token = str_of(case, "token");
            let got = serde_json::from_str::<F>(token);
            if case.get("error").and_then(Value::as_bool) == Some(true) {
                assert!(got.is_err(), "expected error {case}");
            } else {
                let bits = case["bits"].as_i64().unwrap_or_else(|| panic!("no bits {case}"));
                let value = got.unwrap_or_else(|e| panic!("{case}: {e}"));
                assert_eq!(i64::from(value.bits()), bits, "{case}");
            }
        }
        "write" => {
            let bits = case["bits"].as_i64().unwrap_or_else(|| panic!("no bits {case}"));
            // C# `(T)bits` keeps every bit (including negatives); the writer masks, so truncation is equivalent.
            let value = F::from_bits((bits as i32) as u32 & F::ALL_BITS);
            let text: String = read(&write(&value));
            assert_eq!(text, str_of(case, "json"), "{case}");
        }
        other => panic!("unknown kind {other}"),
    }
}

#[test]
fn flag_enum_json_matches_golden() {
    let g = golden::load("flag-enum-json");
    for case in &g.cases {
        match str_of(case, "type") {
            "ExifInfoFields" => golden_flags::<ExifInfoFields>(case),
            "TitleBarFields" => golden_flags::<TitleBarFields>(case),
            other => panic!("unknown flag type {other}"),
        }
    }
}
