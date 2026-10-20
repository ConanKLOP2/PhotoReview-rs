//! Shared helpers of the W8-02b formatter tests (`format_exif`, `format_status`, `format_title_bar`,
//! `format_settings_reload_path`): the English and Vietnamese catalogs, the number cultures of the golden cases,
//! a recorded `DateText`, and the golden row to model conversions.
//!
//! Port support for `PhotoReview.App.Shared/ViewModels/{ExifFormatter,StatusFormatter,TitleBarFormatter,SettingsReloadPath}.cs`
//! (C# baseline d92d6aae). Contract `contracts/W8-02b.md` ("Rust harness"). Golden files are read-only (`tests/golden/`).
//! Each test crate uses a different subset of these helpers, hence the allow below.
#![allow(dead_code)] // shared by four test crates; each uses a subset

use std::sync::{Arc, OnceLock};

use pr_app::viewmodels::format::context::{DateText, FormatContext};
use pr_core::catalog::types::CaptureGroup;
use pr_core::model::RawPairMode;
use pr_core::time::DateTimeUtc;
use pr_imaging::exif_summary::{ExifRational, ExifSummary};
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::{Localizer, built_in_english, built_in_english_localizer};
use pr_l10n::template::NumberCulture;
use pr_test_support::golden;
use serde_json::Value;

/// The English catalog (C# `BuiltInCatalog.EnglishLocalizer`).
pub fn english() -> Arc<Localizer> {
    built_in_english_localizer()
}

/// The Vietnamese catalog over the English fallback, read once from the shipped `assets/languages/vi.json`.
pub fn vietnamese() -> Arc<Localizer> {
    static VI: OnceLock<Arc<Localizer>> = OnceLock::new();
    Arc::clone(VI.get_or_init(|| {
        let path = golden::repo_root().join("assets").join("languages").join("vi.json");
        let text = std::fs::read_to_string(&path).expect("assets/languages/vi.json");
        let mut warnings = Vec::new();
        let vi = LanguageCatalog::try_parse(&text, &path.to_string_lossy(), &mut warnings).expect("valid vi catalog");
        assert!(warnings.is_empty(), "{warnings:?}");
        Arc::new(Localizer::create(built_in_english(), &[vi], &[]))
    }))
}

/// `localizer_code` of a golden case: `"en"` or `"vi"`.
pub fn localizer_for(code: &str) -> Arc<Localizer> {
    match code {
        "en" => english(),
        "vi" => vietnamese(),
        other => panic!("unknown localizer_code {other:?}"),
    }
}

/// `NumberCulture::invariant()`.
pub fn invariant_numbers() -> NumberCulture {
    NumberCulture::invariant()
}

/// `vi-VN`: decimal comma, group ".", as the golden `number_culture` rows give it.
pub fn vi_numbers() -> NumberCulture {
    let mut numbers = NumberCulture::invariant();
    numbers.decimal_separator = ",".to_owned();
    numbers.negative_sign = "-".to_owned();
    numbers
}

/// The `number_culture` object of a golden case.
pub fn number_culture(case: &Value) -> NumberCulture {
    let row = &case["number_culture"];
    let mut numbers = NumberCulture::invariant();
    numbers.negative_sign = text_of(row, "negative_sign");
    numbers.positive_sign = text_of(row, "positive_sign");
    numbers.decimal_separator = text_of(row, "decimal_separator");
    numbers.nan_symbol = text_of(row, "nan_symbol");
    numbers.positive_infinity_symbol = text_of(row, "positive_infinity_symbol");
    numbers.negative_infinity_symbol = text_of(row, "negative_infinity_symbol");
    numbers
}

/// `number_culture.group_separator` of a golden case (the N0 group separator, contract Q1).
pub fn group_separator(case: &Value) -> String {
    text_of(&case["number_culture"], "group_separator")
}

/// A string member that must be present in a golden object.
pub fn text_of(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("golden value has no string {key:?}: {value}"))
        .to_owned()
}

/// A nullable string member (`null` or missing gives `None`).
pub fn opt_text(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_owned)
}

/// A nullable integer member, as `i32` (C# `int`).
pub fn opt_i32(value: &Value, key: &str) -> Option<i32> {
    value[key]
        .as_i64()
        .map(|v| i32::try_from(v).unwrap_or_else(|_| panic!("{key} out of i32 range: {v}")))
}

/// An integer member, as `i32` (C# `int`).
pub fn i32_of(value: &Value, key: &str) -> i32 {
    opt_i32(value, key).unwrap_or_else(|| panic!("golden value has no int {key:?}: {value}"))
}

/// A nullable integer member, as `i64`.
pub fn opt_i64(value: &Value, key: &str) -> Option<i64> {
    value[key].as_i64()
}

/// A nullable bool member.
pub fn opt_bool(value: &Value, key: &str) -> Option<bool> {
    value[key].as_bool()
}

/// A wall-clock value (C# `DateTime`, kind Unspecified). Accepts `yyyy-MM-ddTHH:mm:ss` (golden) and `yyyy-MM-dd HH:mm:ss`.
/// Built through the W4-04a parser, which the golden already pins.
pub fn date(text: &str) -> DateTimeUtc {
    ExifSummary::parse_date(Some(&text.replace('T', " "))).unwrap_or_else(|| panic!("date {text:?} does not parse"))
}

/// A nullable `{n, d}` rational object.
pub fn rational_of(value: &Value) -> Option<ExifRational> {
    if value.is_null() {
        return None;
    }
    Some(ExifRational::new(u32_of(value, "n"), u32_of(value, "d")))
}

fn u32_of(value: &Value, key: &str) -> u32 {
    let raw = golden::i64_of(value, key);
    u32::try_from(raw).unwrap_or_else(|_| panic!("{key} out of u32 range: {raw}"))
}

/// The `exif` object of a golden case as an `ExifSummary`; `None` for a `null` object.
pub fn exif_of(value: &Value) -> Option<ExifSummary> {
    if value.is_null() {
        return None;
    }
    Some(ExifSummary {
        date_taken: opt_text(value, "date_taken").map(|text| date(&text)),
        camera_make: opt_text(value, "camera_make"),
        camera_model: opt_text(value, "camera_model"),
        lens_model: opt_text(value, "lens_model"),
        iso: opt_i32(value, "iso"),
        focal_length: rational_of(&value["focal_length"]),
        f_number: rational_of(&value["f_number"]),
        exposure_time: rational_of(&value["exposure_time"]),
    })
}

#[derive(Debug, Clone)]
struct DateEntry {
    ticks: i64,
    general: Option<String>,
    invariant: String,
}

/// A `DateText` that answers only the dates it was told about. Asking for an unrecorded date is a test bug and panics
/// with the case name, never a formatter bug.
#[derive(Debug, Clone)]
pub struct RecordedDates {
    case: String,
    entries: Vec<DateEntry>,
}

impl RecordedDates {
    /// The `date_texts` rows of a golden case: `ticks` (string), `general` (`null` = the culture rejects the date), `invariant`.
    pub fn from_case(case: &Value) -> Self {
        let name = golden::str_of(case, "name").to_owned();
        let entries = case["date_texts"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|row| DateEntry {
                        ticks: text_of(row, "ticks")
                            .parse()
                            .unwrap_or_else(|e| panic!("case {name}: ticks not an integer: {e}")),
                        general: opt_text(row, "general"),
                        invariant: text_of(row, "invariant"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { case: name, entries }
    }

    /// Hand-written rows: `(wall clock text, general text or None, invariant text)`.
    pub fn new(rows: &[(&str, Option<&str>, &str)]) -> Self {
        let entries = rows
            .iter()
            .map(|(wall, general, invariant)| DateEntry {
                ticks: date(wall).ticks(),
                general: general.map(str::to_owned),
                invariant: (*invariant).to_owned(),
            })
            .collect();
        Self {
            case: "hand-written".to_owned(),
            entries,
        }
    }

    /// No recorded dates: any date request panics.
    pub fn none() -> Self {
        Self {
            case: "no dates".to_owned(),
            entries: Vec::new(),
        }
    }

    fn entry(&self, wall_clock: DateTimeUtc) -> &DateEntry {
        self.entries
            .iter()
            .find(|entry| entry.ticks == wall_clock.ticks())
            .unwrap_or_else(|| {
                panic!(
                    "case {}: no recorded date text for ticks {}",
                    self.case,
                    wall_clock.ticks()
                )
            })
    }
}

impl DateText for RecordedDates {
    fn general(&self, wall_clock: DateTimeUtc) -> Option<String> {
        self.entry(wall_clock).general.clone()
    }

    fn invariant_general(&self, wall_clock: DateTimeUtc) -> String {
        self.entry(wall_clock).invariant.clone()
    }
}

/// The dates the hand-written fixtures show: DateTaken of the fixture EXIF (2024-05-01 14:03:22, "g" = "05/01/2024 14:03")
/// plus the extra `(wall clock, "g" text)` rows; `general` and `invariant` carry the same text.
pub fn fixture_dates(extra: &[(&str, &str)]) -> RecordedDates {
    let mut rows: Vec<(&str, Option<&str>, &str)> =
        vec![("2024-05-01 14:03:22", Some("05/01/2024 14:03"), "05/01/2024 14:03")];
    rows.extend(extra.iter().map(|(wall, text)| (*wall, Some(*text), *text)));
    RecordedDates::new(&rows)
}

/// Everything the formatters read from the environment, owned, so a test can lend it out as a `FormatContext`.
pub struct Env {
    pub texts: Arc<Localizer>,
    pub numbers: NumberCulture,
    pub group_separator: String,
    pub dates: RecordedDates,
}

impl Env {
    pub fn new(texts: Arc<Localizer>, numbers: NumberCulture, group_separator: &str, dates: RecordedDates) -> Self {
        Self {
            texts,
            numbers,
            group_separator: group_separator.to_owned(),
            dates,
        }
    }

    pub fn ctx(&self) -> FormatContext<'_> {
        FormatContext {
            texts: self.texts.as_ref(),
            numbers: &self.numbers,
            group_separator: &self.group_separator,
            dates: &self.dates,
        }
    }
}

/// English, invariant numbers, the fixture dates.
pub fn en_env() -> Env {
    Env::new(english(), invariant_numbers(), ",", fixture_dates(&[]))
}

/// Vietnamese, invariant numbers, the fixture dates.
pub fn vi_env() -> Env {
    Env::new(vietnamese(), invariant_numbers(), ",", fixture_dates(&[]))
}

/// The environment of one golden case (its `localizer_code`, `number_culture` and `date_texts`).
pub fn env_for_case(case: &Value) -> Env {
    Env::new(
        localizer_for(&text_of(case, "localizer_code")),
        number_culture(case),
        &group_separator(case),
        RecordedDates::from_case(case),
    )
}

/// The `group` object of a settings-reload golden case as a `CaptureGroup` (`null` gives `None`).
pub fn capture_group_of(value: &Value) -> Option<CaptureGroup> {
    if value.is_null() {
        return None;
    }
    Some(
        CaptureGroup::new(text_of(value, "jpeg"), text_of(value, "raw"), None)
            .unwrap_or_else(|e| panic!("golden group rejected: {e:?}")),
    )
}

/// A `RawPairMode` by its C# name (`"Separate"`, `"PreferJpeg"`, `"PreferRaw"`).
pub fn pair_mode_of(name: &str) -> RawPairMode {
    match name {
        "Separate" => RawPairMode::Separate,
        "PreferJpeg" => RawPairMode::PreferJpeg,
        "PreferRaw" => RawPairMode::PreferRaw,
        other => panic!("unknown RawPairMode {other:?}"),
    }
}
