//! Tests for `pr_imaging::exif_query` (C# `ExifQueryInterpreter`, baseline d92d6aae): the `Read_*` tests of
//! `tests/PhotoReview.Imaging.Tests/Metadata/ExifMetadataMutationGapTests.cs` (region "ExifQueryInterpreter"), plus the
//! golden replay of `tests/golden/exif-query.json` (its inputs are `tests/golden-inputs/exif-query.json`).
//!
//! The `AsInteger` / `AsRational` / `AsString` tests live in `src/exif_query.rs` (`mod tests`): those helpers are
//! crate-private, except `as_integer` (amendment A3).

use std::cell::RefCell;
use std::collections::HashMap;

use pr_core::time::DateTimeUtc;
use pr_imaging::exif_parser::{
    TAG_DATE_TIME, TAG_DATE_TIME_DIGITIZED, TAG_DATE_TIME_ORIGINAL, TAG_EXPOSURE_TIME, TAG_F_NUMBER, TAG_FOCAL_LENGTH,
    TAG_ISO, TAG_LENS_MODEL, TAG_MAKE, TAG_MODEL,
};
use pr_imaging::exif_query::{JPEG_IFD_ROOT, QueryValue, TIFF_IFD_ROOT, read};
use pr_imaging::exif_summary::{ExifRational, ExifSummary};
use pr_test_support::golden::{self, repo_root};
use serde_json::{Value, json};

const TICKS_PER_SECOND: i64 = 10_000_000;
const TICKS_PER_DAY: i64 = 86_400 * TICKS_PER_SECOND;

/// `{root}/exif/{ushort=N}` for a tag of the EXIF sub-IFD, `{root}/{ushort=N}` for IFD0 (decimal tag, no padding).
fn tag_path(root: &str, exif: bool, tag: u16) -> String {
    if exif {
        format!("{root}/exif/{{ushort={tag}}}")
    } else {
        format!("{root}/{{ushort={tag}}}")
    }
}

/// C# `Query(root, values)` + `ExifQueryInterpreter.Read`: the value stored under the tag path, `None` for any other path.
fn read_tags(root: &str, values: &[(bool, u16, QueryValue)]) -> Option<ExifSummary> {
    let query = |path: &str| -> Option<QueryValue> {
        values
            .iter()
            .find(|(exif, tag, _)| path == tag_path(root, *exif, *tag))
            .map(|(_, _, value)| value.clone())
    };
    read(&query, root)
}

/// C# `Packed(n, d)`: an unsigned RATIONAL packed as `(d << 32) | n`.
fn packed(numerator: u32, denominator: u32) -> u64 {
    (u64::from(denominator) << 32) | u64::from(numerator)
}

fn chars(text: &str) -> QueryValue {
    QueryValue::Chars(text.encode_utf16().collect())
}

fn text(value: &str) -> QueryValue {
    QueryValue::Text(value.to_owned())
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `yyyy-MM-ddTHH:mm:ss` of a `DateTimeUtc` read as a wall-clock time (ticks 0 = 0001-01-01T00:00:00, kind unspecified).
fn format_date(date: DateTimeUtc) -> String {
    let mut days = date.ticks() / TICKS_PER_DAY;
    let seconds = (date.ticks() % TICKS_PER_DAY) / TICKS_PER_SECOND;
    let mut year = 1;
    loop {
        let length = if is_leap_year(year) { 366 } else { 365 };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }
    let mut month = 1;
    while days >= days_in_month(year, month) {
        days -= days_in_month(year, month);
        month += 1;
    }
    let day = days + 1;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

fn rational(value: ExifRational) -> Value {
    json!({ "numerator": value.numerator, "denominator": value.denominator })
}

/// The golden `summary` shape of an `ExifSummary` (or `null`), so the replay compares whole objects.
fn summary_json(summary: Option<&ExifSummary>) -> Value {
    match summary {
        None => Value::Null,
        Some(s) => json!({
            "date_taken": s.date_taken.map(format_date),
            "camera_make": s.camera_make,
            "camera_model": s.camera_model,
            "lens_model": s.lens_model,
            "iso": s.iso,
            "focal_length": s.focal_length.map(rational),
            "f_number": s.f_number.map(rational),
            "exposure_time": s.exposure_time.map(rational),
        }),
    }
}

// ---------------------------------------------------------------- ExifQueryInterpreter.Read

// C#: ExifMetadataMutationGapTests.Read_EveryTag_MapsToItsSummaryField (JpegIfdRoot)
#[test]
fn read_every_tag_maps_to_its_summary_field_jpeg() {
    let summary = read_tags(
        JPEG_IFD_ROOT,
        &[
            (true, TAG_DATE_TIME_ORIGINAL, text("2024:05:01 14:03:22")),
            (false, TAG_MAKE, text("Canon")),
            (false, TAG_MODEL, chars("EOS R5")),
            (true, TAG_LENS_MODEL, text("RF50")),
            (true, TAG_ISO, QueryValue::U16(400)),
            (true, TAG_FOCAL_LENGTH, QueryValue::U64(packed(50, 1))),
            (true, TAG_F_NUMBER, QueryValue::U64(packed(28, 10))),
            (true, TAG_EXPOSURE_TIME, QueryValue::U64(packed(1, 250))),
        ],
    )
    .expect("summary");

    assert_eq!(format_date(summary.date_taken.expect("date")), "2024-05-01T14:03:22");
    assert_eq!(summary.camera_make.as_deref(), Some("Canon"));
    assert_eq!(summary.camera_model.as_deref(), Some("EOS R5"));
    assert_eq!(summary.lens_model.as_deref(), Some("RF50"));
    assert_eq!(summary.iso, Some(400));
    assert_eq!(summary.focal_length, Some(ExifRational::new(50, 1)));
    assert_eq!(summary.f_number, Some(ExifRational::new(28, 10)));
    assert_eq!(summary.exposure_time, Some(ExifRational::new(1, 250)));
}

// C#: ExifMetadataMutationGapTests.Read_EveryTag_MapsToItsSummaryField (TiffIfdRoot)
#[test]
fn read_every_tag_maps_to_its_summary_field_tiff() {
    let summary = read_tags(
        TIFF_IFD_ROOT,
        &[
            (true, TAG_DATE_TIME_ORIGINAL, text("2024:05:01 14:03:22")),
            (false, TAG_MAKE, text("Canon")),
            (false, TAG_MODEL, chars("EOS R5")),
            (true, TAG_LENS_MODEL, text("RF50")),
            (true, TAG_ISO, QueryValue::U16(400)),
            (true, TAG_FOCAL_LENGTH, QueryValue::U64(packed(50, 1))),
            (true, TAG_F_NUMBER, QueryValue::U64(packed(28, 10))),
            (true, TAG_EXPOSURE_TIME, QueryValue::U64(packed(1, 250))),
        ],
    )
    .expect("summary");

    assert_eq!(format_date(summary.date_taken.expect("date")), "2024-05-01T14:03:22");
    assert_eq!(summary.camera_make.as_deref(), Some("Canon"));
    assert_eq!(summary.camera_model.as_deref(), Some("EOS R5"));
    assert_eq!(summary.lens_model.as_deref(), Some("RF50"));
    assert_eq!(summary.iso, Some(400));
    assert_eq!(summary.focal_length, Some(ExifRational::new(50, 1)));
    assert_eq!(summary.f_number, Some(ExifRational::new(28, 10)));
    assert_eq!(summary.exposure_time, Some(ExifRational::new(1, 250)));
}

// C#: ExifMetadataMutationGapTests.Read_Dates_OriginalBeatsDigitizedBeatsDateTime (original, digitized, dateTime) = (true, true, true)
#[test]
fn read_dates_original_beats_digitized_beats_date_time_all_present_takes_original() {
    let summary = read_tags(
        JPEG_IFD_ROOT,
        &[
            (true, TAG_DATE_TIME_ORIGINAL, text("2024:05:01 14:03:22")),
            (true, TAG_DATE_TIME_DIGITIZED, text("2023:04:02 13:02:21")),
            (false, TAG_DATE_TIME, text("2022:03:03 12:01:20")),
        ],
    )
    .expect("summary");
    assert_eq!(format_date(summary.date_taken.expect("date")), "2024-05-01T14:03:22");
}

// C#: ExifMetadataMutationGapTests.Read_Dates_OriginalBeatsDigitizedBeatsDateTime (true, false, true)
#[test]
fn read_dates_original_beats_digitized_beats_date_time_original_and_date_time_takes_original() {
    let summary = read_tags(
        JPEG_IFD_ROOT,
        &[
            (true, TAG_DATE_TIME_ORIGINAL, text("2024:05:01 14:03:22")),
            (false, TAG_DATE_TIME, text("2022:03:03 12:01:20")),
        ],
    )
    .expect("summary");
    assert_eq!(format_date(summary.date_taken.expect("date")), "2024-05-01T14:03:22");
}

// C#: ExifMetadataMutationGapTests.Read_Dates_OriginalBeatsDigitizedBeatsDateTime (true, false, false)
#[test]
fn read_dates_original_beats_digitized_beats_date_time_original_only_takes_original() {
    let summary = read_tags(
        JPEG_IFD_ROOT,
        &[(true, TAG_DATE_TIME_ORIGINAL, text("2024:05:01 14:03:22"))],
    )
    .expect("summary");
    assert_eq!(format_date(summary.date_taken.expect("date")), "2024-05-01T14:03:22");
}

// C#: ExifMetadataMutationGapTests.Read_Dates_OriginalBeatsDigitizedBeatsDateTime (false, true, true)
#[test]
fn read_dates_original_beats_digitized_beats_date_time_digitized_and_date_time_takes_digitized() {
    let summary = read_tags(
        JPEG_IFD_ROOT,
        &[
            (true, TAG_DATE_TIME_DIGITIZED, text("2023:04:02 13:02:21")),
            (false, TAG_DATE_TIME, text("2022:03:03 12:01:20")),
        ],
    )
    .expect("summary");
    assert_eq!(format_date(summary.date_taken.expect("date")), "2023-04-02T13:02:21");
}

// C#: ExifMetadataMutationGapTests.Read_Dates_OriginalBeatsDigitizedBeatsDateTime (false, true, false)
#[test]
fn read_dates_original_beats_digitized_beats_date_time_digitized_only_takes_digitized() {
    let summary = read_tags(
        JPEG_IFD_ROOT,
        &[(true, TAG_DATE_TIME_DIGITIZED, text("2023:04:02 13:02:21"))],
    )
    .expect("summary");
    assert_eq!(format_date(summary.date_taken.expect("date")), "2023-04-02T13:02:21");
}

// C#: ExifMetadataMutationGapTests.Read_Dates_OriginalBeatsDigitizedBeatsDateTime (false, false, true)
#[test]
fn read_dates_original_beats_digitized_beats_date_time_date_time_only_takes_date_time() {
    let summary = read_tags(JPEG_IFD_ROOT, &[(false, TAG_DATE_TIME, text("2022:03:03 12:01:20"))]).expect("summary");
    assert_eq!(format_date(summary.date_taken.expect("date")), "2022-03-03T12:01:20");
}

// C#: ExifMetadataMutationGapTests.Read_EmptyArrays_AreIgnoredWithoutThrowing
#[test]
fn read_empty_arrays_are_ignored_without_throwing() {
    let summary = read_tags(
        JPEG_IFD_ROOT,
        &[
            (false, TAG_MAKE, text("Canon")),
            (true, TAG_ISO, QueryValue::U16s(Vec::new())),
            (true, TAG_FOCAL_LENGTH, QueryValue::U64s(Vec::new())),
            (true, TAG_F_NUMBER, QueryValue::I64s(Vec::new())),
        ],
    )
    .expect("summary");

    assert_eq!(summary.camera_make.as_deref(), Some("Canon"));
    assert_eq!(summary.iso, None);
    assert_eq!(summary.focal_length, None);
    assert_eq!(summary.f_number, None);
}

// Mutation kill (no C# twin): an empty string at ANY level of the date chain stops it (note 2), so the next level is
// not used even though it holds a valid date.
#[test]
fn read_date_empty_string_at_any_level_stops_the_chain() {
    let valid = text("2022:03:03 12:01:20");
    let digitized_empty = read_tags(
        JPEG_IFD_ROOT,
        &[
            (true, TAG_DATE_TIME_DIGITIZED, text("")),
            (false, TAG_DATE_TIME, valid.clone()),
            (false, TAG_MAKE, text("Canon")),
        ],
    )
    .expect("summary");
    assert_eq!(digitized_empty.date_taken, None);
    let original_empty = read_tags(
        JPEG_IFD_ROOT,
        &[
            (true, TAG_DATE_TIME_ORIGINAL, text("")),
            (true, TAG_DATE_TIME_DIGITIZED, valid),
            (false, TAG_MAKE, text("Canon")),
        ],
    )
    .expect("summary");
    assert_eq!(original_empty.date_taken, None);
}

// C#: ExifMetadataMutationGapTests.Read_NullQuery_Throws has no Rust twin: `read` takes a `&dyn Fn`, which cannot be null
// (type system).

// ---------------------------------------------------------------- golden replay

/// An integer written in the golden inputs: a JSON number, or a decimal string for 32- and 64-bit values.
fn json_int(value: &Value) -> i128 {
    let parsed = match value {
        Value::Number(n) => n.as_i64().map(i128::from).or_else(|| n.as_u64().map(i128::from)),
        Value::String(s) => s.parse::<i128>().ok(),
        _ => None,
    };
    parsed.unwrap_or_else(|| panic!("golden input: integer expected, got {value}"))
}

fn json_ints(value: &Value) -> Vec<i128> {
    value
        .as_array()
        .expect("golden input: array expected")
        .iter()
        .map(json_int)
        .collect()
}

/// One input value of `tests/golden-inputs/exif-query.json` (`{type, value}`) as the query result the C# `Func` returned.
fn query_value(entry: &Value) -> QueryValue {
    let kind = entry["type"].as_str().expect("golden input: type");
    let value = &entry["value"];
    match kind {
        "string" => text(value.as_str().expect("golden input: string")),
        "chars" => chars(value.as_str().expect("golden input: chars")),
        "u8" => QueryValue::U8(u8::try_from(json_int(value)).expect("u8")),
        "u16" => QueryValue::U16(u16::try_from(json_int(value)).expect("u16")),
        "i16" => QueryValue::I16(i16::try_from(json_int(value)).expect("i16")),
        "u32" => QueryValue::U32(u32::try_from(json_int(value)).expect("u32")),
        "i32" => QueryValue::I32(i32::try_from(json_int(value)).expect("i32")),
        "u16[]" => QueryValue::U16s(
            json_ints(value)
                .into_iter()
                .map(|v| u16::try_from(v).expect("u16"))
                .collect(),
        ),
        "i16[]" => QueryValue::I16s(
            json_ints(value)
                .into_iter()
                .map(|v| i16::try_from(v).expect("i16"))
                .collect(),
        ),
        "u32[]" => QueryValue::U32s(
            json_ints(value)
                .into_iter()
                .map(|v| u32::try_from(v).expect("u32"))
                .collect(),
        ),
        "i32[]" => QueryValue::I32s(
            json_ints(value)
                .into_iter()
                .map(|v| i32::try_from(v).expect("i32"))
                .collect(),
        ),
        "u64" => QueryValue::U64(u64::try_from(json_int(value)).expect("u64")),
        "i64" => QueryValue::I64(i64::try_from(json_int(value)).expect("i64")),
        "u64[]" => QueryValue::U64s(
            json_ints(value)
                .into_iter()
                .map(|v| u64::try_from(v).expect("u64"))
                .collect(),
        ),
        "i64[]" => QueryValue::I64s(
            json_ints(value)
                .into_iter()
                .map(|v| i64::try_from(v).expect("i64"))
                .collect(),
        ),
        // `double` and `bytes` are CLR types no conversion accepts: `QueryValue::Other` stands for them.
        "double" | "bytes" => QueryValue::Other,
        other => panic!("golden input: unknown value type {other}"),
    }
}

/// Replays every case of `tests/golden/exif-query.json` (inputs from `tests/golden-inputs/exif-query.json`): the query
/// paths in order, then the summary fields. Every disagreement is collected so one run reports them all.
#[test]
fn golden_exif_query_replays_every_case() {
    let golden = golden::load("exif-query");
    let inputs_path = repo_root().join("tests").join("golden-inputs").join("exif-query.json");
    let inputs_text =
        std::fs::read_to_string(&inputs_path).unwrap_or_else(|e| panic!("read {}: {e}", inputs_path.display()));
    let inputs: Value = serde_json::from_str(&inputs_text).expect("golden inputs are JSON");
    let inputs = inputs["cases"].as_array().expect("golden inputs: cases array");
    assert_eq!(inputs.len(), golden.cases.len(), "input and golden case counts differ");

    let mut failures = Vec::new();
    for (input, expected) in inputs.iter().zip(&golden.cases) {
        let name = input["name"].as_str().expect("case name");
        assert_eq!(
            expected["name"].as_str(),
            Some(name),
            "golden case order differs from the inputs"
        );
        let root = input["root"].as_str().expect("case root");
        let values: HashMap<String, QueryValue> = input["values"]
            .as_object()
            .expect("case values")
            .iter()
            .map(|(path, entry)| (path.clone(), query_value(entry)))
            .collect();

        let queried = RefCell::new(Vec::<String>::new());
        let query = |path: &str| -> Option<QueryValue> {
            queried.borrow_mut().push(path.to_owned());
            values.get(path).cloned()
        };
        let summary = read(&query, root);
        let queried = queried.into_inner();

        let expected_queried = &expected["queried"];
        if json!(queried) != *expected_queried {
            failures.push(format!(
                "{name}: queried {}, expected {expected_queried}",
                json!(queried)
            ));
        }
        let actual_summary = summary_json(summary.as_ref());
        if actual_summary != expected["summary"] {
            failures.push(format!(
                "{name}: summary {actual_summary}, expected {}",
                expected["summary"]
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} exif-query golden cases differ:\n{}",
        failures.len(),
        golden.cases.len(),
        failures.join("\n")
    );
}
