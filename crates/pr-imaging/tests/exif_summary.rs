//! Port of the C# tests for `PhotoReview.Imaging/Metadata/ExifSummary.cs` and `ExifSummaryCodec.cs` (baseline d92d6aae):
//! `tests/PhotoReview.Imaging.Tests/Metadata/{ExifSummaryCodecTests, ExifSummaryCodecGapTests,
//! ExifSummaryMutationGapTests}.cs`, plus golden replays of `tests/golden/exif-summary-*.json`.
//!
//! Red tests (W4-04a): every test here fails through `todo!()` in `pr_imaging::exif_summary` (and `ExifRational::value`
//! in `pr_imaging::tiff`) until the bodies are written. Each ported test names its C# twin in a comment.
//!
//! Golden inputs come from `tests/golden-inputs/exif-summary-*.json`, paired by position with the expected values in
//! `tests/golden/exif-summary-*.json` (names are checked). The generator's shorthand is expanded here: a text field
//! `key` is `prefix` + `key` + `{key}_repeat` (`{text, count}`, repeated) + `suffix`.
//!
//! NOT PORTED (documented skip): the C# `CleanText` / `Encode` behaviour for a lone UTF-16 surrogate. A Rust `&str` cannot
//! hold one, and the JSON goldens cannot represent one either, so no Rust input reaches that path.

use std::fmt::{Debug, Write as _};

use pr_core::time::DateTimeUtc;
use pr_imaging::exif_summary::{ExifRational, ExifSummary, MAX_TEXT_LENGTH, RawExifTags, codec};
use pr_test_support::golden;
use serde_json::Value;

const TICKS_PER_SECOND: i64 = 10_000_000;
const TICKS_PER_DAY: i64 = 864_000_000_000;

// ---------------------------------------------------------------------------------------------------------------------
// Helpers (independent of the code under test; self-checked by `test_helpers_match_csharp_known_ticks`)
// ---------------------------------------------------------------------------------------------------------------------

/// Ticks of a .NET `DateTime(year, month, day, hour, minute, second)`, proleptic Gregorian (days-from-civil).
fn ticks_of(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = i64::from((month + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days_since_1970 = era * 146_097 + doe - 719_468;
    let days_since_0001 = days_since_1970 + 719_162;
    let seconds = i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second);
    days_since_0001 * TICKS_PER_DAY + seconds * TICKS_PER_SECOND
}

/// `date_text` of the goldens: `yyyy-MM-ddTHH:mm:ss.fffffff` (civil-from-days, no zone).
fn date_text(date: DateTimeUtc) -> String {
    let ticks = date.ticks();
    let days = ticks.div_euclid(TICKS_PER_DAY);
    let rest = ticks.rem_euclid(TICKS_PER_DAY);
    let z = days + 306; // days since 0000-03-01 (0001-01-01 is day 0 here)
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let seconds = rest / TICKS_PER_SECOND;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:07}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60,
        rest % TICKS_PER_SECOND
    )
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            write!(out, "{byte:02X}").expect("writing to a String cannot fail");
            out
        })
}

fn from_hex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd hex length in {text}");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex byte"))
        .collect()
}

/// A `[numerator, denominator]` golden pair as an `ExifRational`; `None` for JSON null.
fn rational_of(value: &Value) -> Option<ExifRational> {
    let pair = value.as_array()?;
    let part = |i: usize| u32::try_from(pair[i].as_u64().expect("rational part")).expect("u32 rational part");
    Some(ExifRational::new(part(0), part(1)))
}

/// The same pair as plain integers, for comparing with `Debug` output.
fn pair_of(value: &Value) -> Option<(u64, u64)> {
    let pair = value.as_array()?;
    Some((pair[0].as_u64()?, pair[1].as_u64()?))
}

fn rational_pair(rational: ExifRational) -> (u64, u64) {
    (u64::from(rational.numerator), u64::from(rational.denominator))
}

/// Golden shorthand for a text field: optional `prefix`, the `key` value, `{key}_repeat` repeated, optional `suffix`.
/// `None` when neither the value nor a repeat is present (JSON null).
fn expand(input: &Value, key: &str) -> Option<String> {
    let base = input.get(key).and_then(Value::as_str);
    let repeat = input.get(format!("{key}_repeat"));
    if base.is_none() && repeat.is_none() {
        return None;
    }
    let mut text = String::new();
    text.push_str(input.get("prefix").and_then(Value::as_str).unwrap_or(""));
    text.push_str(base.unwrap_or(""));
    if let Some(rep) = repeat {
        let unit = rep["text"].as_str().expect("repeat text");
        let count = usize::try_from(rep["count"].as_u64().expect("repeat count")).expect("repeat count fits usize");
        text.push_str(&unit.repeat(count));
    }
    text.push_str(input.get("suffix").and_then(Value::as_str).unwrap_or(""));
    Some(text)
}

/// `(input, expected)` pairs of one golden. Inputs come from `tests/golden-inputs`, expected values from `tests/golden`.
fn paired(name: &str, expected_count: usize) -> Vec<(Value, Value)> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let inputs = doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: no cases array"));
    let expected = golden::load(name);
    assert_eq!(expected.cases.len(), expected_count, "{name}: golden case count");
    assert_eq!(
        inputs.len(),
        expected.cases.len(),
        "{name}: inputs and golden differ in count"
    );
    inputs
        .iter()
        .zip(expected.cases.iter())
        .map(|(input, golden_case)| {
            assert_eq!(input["name"], golden_case["name"], "{name}: case order differs");
            (input.clone(), golden_case.clone())
        })
        .collect()
}

fn case_name(input: &Value) -> String {
    input["name"].as_str().expect("case name").to_owned()
}

/// `ExifSummary::create` of a create-golden input (owned texts kept alive for the call).
fn create_from_input(input: &Value) -> Option<ExifSummary> {
    let make = expand(input, "make");
    let model = expand(input, "model");
    let lens = expand(input, "lens");
    let raw = RawExifTags {
        date_taken: input["date"].as_str(),
        make: make.as_deref(),
        model: model.as_deref(),
        lens: lens.as_deref(),
        iso: input["iso"].as_i64(),
        focal_length: rational_of(&input["focal"]),
        f_number: rational_of(&input["f_number"]),
        exposure_time: rational_of(&input["exposure"]),
    };
    ExifSummary::create(&raw)
}

/// An unsanitised summary built field by field, as the encode goldens do (C# object initialiser).
fn summary_from_input(input: &Value) -> ExifSummary {
    ExifSummary {
        date_taken: input["date_ticks"]
            .as_i64()
            .map(|ticks| DateTimeUtc::from_ticks(ticks).expect("golden ticks are in range")),
        camera_make: expand(input, "make"),
        camera_model: expand(input, "model"),
        lens_model: expand(input, "lens"),
        iso: input["iso"]
            .as_i64()
            .map(|iso| i32::try_from(iso).expect("encode golden ISO fits i32")),
        focal_length: rational_of(&input["focal"]),
        f_number: rational_of(&input["f_number"]),
        exposure_time: rational_of(&input["exposure"]),
    }
}

/// Pushes a mismatch when the two values differ; both sides are printed for the failure message.
fn cmp<T: PartialEq + Debug>(bad: &mut Vec<String>, field: &str, actual: T, expected: T) {
    if actual != expected {
        bad.push(format!("{field}: got {actual:?}, golden {expected:?}"));
    }
}

/// Every mismatch between a summary and a golden `summary`/`decoded` object (null, every field, and the re-encoded hex).
fn summary_mismatches(actual: Option<&ExifSummary>, expected: &Value) -> Vec<String> {
    let mut bad = Vec::new();
    if expected["is_null"].as_bool() == Some(true) {
        if let Some(found) = actual {
            bad.push(format!("expected null, got {found:?}"));
        }
        return bad;
    }
    let Some(found) = actual else {
        bad.push("expected a summary, got null".to_owned());
        return bad;
    };
    cmp(
        &mut bad,
        "date_ticks",
        found.date_taken.map(DateTimeUtc::ticks),
        expected["date_ticks"].as_i64(),
    );
    cmp(
        &mut bad,
        "date_text",
        found.date_taken.map(date_text),
        expected["date_text"].as_str().map(str::to_owned),
    );
    cmp(
        &mut bad,
        "make",
        found.camera_make.as_deref(),
        expected["make"].as_str(),
    );
    cmp(
        &mut bad,
        "model",
        found.camera_model.as_deref(),
        expected["model"].as_str(),
    );
    cmp(&mut bad, "lens", found.lens_model.as_deref(), expected["lens"].as_str());
    cmp(&mut bad, "iso", found.iso.map(i64::from), expected["iso"].as_i64());
    cmp(
        &mut bad,
        "focal",
        found.focal_length.map(rational_pair),
        pair_of(&expected["focal"]),
    );
    cmp(
        &mut bad,
        "f_number",
        found.f_number.map(rational_pair),
        pair_of(&expected["f_number"]),
    );
    cmp(
        &mut bad,
        "exposure",
        found.exposure_time.map(rational_pair),
        pair_of(&expected["exposure"]),
    );
    cmp(
        &mut bad,
        "is_empty",
        Some(found.is_empty()),
        expected["is_empty"].as_bool(),
    );
    if let Some(hex) = expected["encoded_hex"].as_str() {
        cmp(
            &mut bad,
            "encoded_hex",
            Some(hex_upper(&codec::encode(Some(found)))),
            Some(hex.to_owned()),
        );
    }
    bad
}

fn assert_no_failures(failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The full summary of C# `ExifSummaryCodecTests.Full`: `new DateTime(2024, 5, 1, 14, 3, 22)` and the rest.
fn full_summary() -> ExifSummary {
    ExifSummary {
        date_taken: DateTimeUtc::from_ticks(ticks_of(2024, 5, 1, 14, 3, 22)),
        camera_make: Some("Canon".to_owned()),
        camera_model: Some("Canon EOS R5".to_owned()),
        lens_model: Some("RF24-70mm F2.8 L IS USM".to_owned()),
        iso: Some(400),
        focal_length: Some(ExifRational::new(50, 1)),
        f_number: Some(ExifRational::new(28, 10)),
        exposure_time: Some(ExifRational::new(1, 250)),
    }
}

/// A version-1 block holding only a date field (mask bit DATE, 8 little-endian tick bytes).
fn date_block(ticks: i64) -> Vec<u8> {
    let mut bytes = vec![1, 1];
    bytes.extend_from_slice(&ticks.to_le_bytes());
    bytes
}

// ---------------------------------------------------------------------------------------------------------------------
// Helper self-check (passes now: it guards the expected values used by the ported tests below)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn test_helpers_match_csharp_known_ticks() {
    // Values taken from the goldens (C# DateTime ticks), so a wrong helper cannot hide a wrong port.
    assert_eq!(ticks_of(1970, 1, 1, 0, 0, 0), 621_355_968_000_000_000);
    assert_eq!(ticks_of(2024, 5, 1, 14, 3, 22), 638_501_690_020_000_000);
    assert_eq!(ticks_of(9999, 12, 31, 23, 59, 59), 3_155_378_975_990_000_000);
    assert_eq!(ticks_of(1, 1, 1, 0, 0, 0), 0);
    let at = |ticks: i64| DateTimeUtc::from_ticks(ticks).expect("in range");
    assert_eq!(date_text(at(0)), "0001-01-01T00:00:00.0000000");
    assert_eq!(date_text(at(638_501_690_020_000_000)), "2024-05-01T14:03:22.0000000");
    assert_eq!(date_text(at(3_155_378_975_999_999_999)), "9999-12-31T23:59:59.9999999");
    assert_eq!(date_text(at(630_874_224_000_000_000)), "2000-02-29T12:00:00.0000000");
}

// ---------------------------------------------------------------------------------------------------------------------
// ExifSummaryCodecTests (C# [Trait HotPath])
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn full_round_trips() {
    // C#: ExifSummaryCodecTests.FullRoundTrips
    let full = full_summary();
    assert_eq!(codec::decode(&codec::encode(Some(&full))), Some(full));
}

#[test]
fn partial_round_trips() {
    // C#: ExifSummaryCodecTests.PartialRoundTrips (4 partials, incl. the non-BMP lens text)
    let partials = [
        ExifSummary {
            date_taken: DateTimeUtc::from_ticks(ticks_of(2020, 1, 2, 3, 4, 5)),
            ..Default::default()
        },
        ExifSummary {
            lens_model: Some("\u{1ED0}ng k\u{ED}nh 50mm \u{192}/1.8 \u{1F600}".to_owned()),
            ..Default::default()
        },
        ExifSummary {
            iso: Some(12800),
            exposure_time: Some(ExifRational::new(30, 1)),
            ..Default::default()
        },
        ExifSummary {
            camera_model: Some("X100V".to_owned()),
            f_number: Some(ExifRational::new(2, 1)),
            ..Default::default()
        },
    ];
    for partial in partials {
        let encoded = codec::encode(Some(&partial));
        assert_eq!(codec::decode(&encoded), Some(partial));
    }
}

#[test]
fn empty_is_nothing() {
    // C#: ExifSummaryCodecTests.EmptyIsNothing (encode(null) and encode(Some(empty)) are both []; decode([]) is null)
    assert!(codec::encode(None).is_empty());
    assert!(codec::encode(Some(&ExifSummary::default())).is_empty());
    assert_eq!(codec::decode(&[]), None);
}

#[test]
fn malformed_decodes_to_null() {
    // C#: ExifSummaryCodecTests.MalformedDecodesToNull (every strict prefix, one trailing byte, version 99)
    let bytes = codec::encode(Some(&full_summary()));
    for length in 0..bytes.len() {
        assert_eq!(codec::decode(&bytes[..length]), None, "prefix of {length} bytes");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(codec::decode(&trailing), None, "one trailing byte");
    let mut other_version = bytes.clone();
    other_version[0] = 99;
    assert_eq!(codec::decode(&other_version), None, "version 99");
}

// ---------------------------------------------------------------------------------------------------------------------
// ExifSummaryCodecGapTests (C# [Trait HotPath], RV-T53 codec part)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn encode_text_whose_utf8_is_longer_than_255_bytes_cuts_mid_sequence_and_still_decodes_to_the_sanitised_text() {
    // C#: ExifSummaryCodecGapTests.Encode_TextWhoseUtf8IsLongerThan255Bytes_CutsMidSequenceAndStillDecodesToTheSanitisedText
    // 200 x 'e acute' = 400 UTF-8 bytes; the 255-byte cut lands in the middle of a two-byte sequence.
    let summary = ExifSummary {
        camera_make: Some("\u{E9}".repeat(200)),
        ..Default::default()
    };
    let encoded = codec::encode(Some(&summary));
    assert_eq!(encoded.len(), 2 + 1 + 255);
    assert_eq!(encoded[2], 255);
    let decoded = codec::decode(&encoded).expect("the cut block still decodes");
    // The broken tail (U+FFFD) is beyond the 64-character cap, so the sanitised text is the clean prefix.
    assert_eq!(decoded.camera_make, Some("\u{E9}".repeat(MAX_TEXT_LENGTH)));
}

#[test]
fn decode_make_bit_set_with_zero_length_text_yields_no_make_but_keeps_the_other_fields() {
    // C#: ExifSummaryCodecGapTests.Decode_MakeBitSetWithZeroLengthText_YieldsNoMakeButKeepsTheOtherFields
    // version 1, mask Make|Iso, make length 0, ISO 400 (little-endian int32)
    let data = [1, 2 | 16, 0, 0x90, 0x01, 0x00, 0x00];
    let decoded = codec::decode(&data).expect("the ISO field survives");
    assert_eq!(decoded.camera_make, None);
    assert_eq!(decoded.iso, Some(400));
}

#[test]
fn decode_only_a_zero_length_text_present_is_null() {
    // C#: ExifSummaryCodecGapTests.Decode_OnlyAZeroLengthTextPresent_IsNull
    assert_eq!(codec::decode(&[1, 2, 0]), None);
}

// ---------------------------------------------------------------------------------------------------------------------
// ExifSummaryMutationGapTests (exact boundaries)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn exif_rational_value_is_numerator_over_denominator_or_zero() {
    // C#: ExifSummaryMutationGapTests.ExifRational_Value_IsNumeratorOverDenominatorOrZero (4 rows, tolerance 1e-12)
    let rows = [(1u32, 250u32, 0.004_f64), (3, 2, 1.5), (5, 0, 0.0), (0, 0, 0.0)];
    for (numerator, denominator, expected) in rows {
        let actual = ExifRational::new(numerator, denominator).value();
        assert!(
            (actual - expected).abs() < 1e-12,
            "{numerator}/{denominator}: got {actual}, expected {expected}"
        );
    }
}

#[test]
fn create_iso_must_be_positive_and_at_most_ten_million() {
    // C#: ExifSummaryMutationGapTests.Create_Iso_MustBePositiveAndAtMostTenMillion (5 rows)
    let rows = [
        (0_i64, false),
        (-1, false),
        (1, true),
        (10_000_000, true),
        (10_000_001, false),
    ];
    for (iso, kept) in rows {
        let raw = RawExifTags {
            make: Some("Canon"),
            iso: Some(iso),
            ..Default::default()
        };
        let summary = ExifSummary::create(&raw).expect("the make keeps the summary alive");
        let expected = kept.then(|| i32::try_from(iso).expect("row fits i32"));
        assert_eq!(summary.iso, expected, "iso {iso}");
    }
}

#[test]
fn create_rational_with_a_zero_part_is_dropped() {
    // C#: ExifSummaryMutationGapTests.Create_RationalWithAZeroPart_IsDropped (2 rows)
    for (numerator, denominator) in [(0u32, 5u32), (5, 0)] {
        let rational = ExifRational::new(numerator, denominator);
        let raw = RawExifTags {
            make: Some("Canon"),
            focal_length: Some(rational),
            f_number: Some(rational),
            exposure_time: Some(rational),
            ..Default::default()
        };
        let summary = ExifSummary::create(&raw).expect("the make keeps the summary alive");
        assert_eq!(summary.focal_length, None, "{numerator}/{denominator} focal");
        assert_eq!(summary.f_number, None, "{numerator}/{denominator} f-number");
        assert_eq!(summary.exposure_time, None, "{numerator}/{denominator} exposure");
    }
}

#[test]
fn clean_text_text_starting_with_nul_is_null() {
    // C#: ExifSummaryMutationGapTests.CleanText_TextStartingWithNul_IsNull
    assert_eq!(ExifSummary::clean_text(Some("\0abc")), None);
    assert_eq!(ExifSummary::clean_text(Some("abc\0def")), Some("abc".to_owned()));
}

#[test]
fn clean_text_cut_through_a_surrogate_pair_drops_the_half_character() {
    // C#: ExifSummaryMutationGapTests.CleanText_CutThroughASurrogatePair_DropsTheHalfCharacter
    let text = format!("{}\u{1F600}", "a".repeat(MAX_TEXT_LENGTH - 1));
    assert_eq!(
        ExifSummary::clean_text(Some(&text)),
        Some("a".repeat(MAX_TEXT_LENGTH - 1))
    );
}

#[test]
fn decode_date_at_the_exact_edges_of_the_date_time_range_is_accepted() {
    // C#: ExifSummaryMutationGapTests.Decode_DateAtTheExactEdgesOfTheDateTimeRange_IsAccepted
    let min = codec::decode(&date_block(DateTimeUtc::MIN.ticks())).and_then(|summary| summary.date_taken);
    assert_eq!(min, Some(DateTimeUtc::MIN));
    let max = codec::decode(&date_block(DateTimeUtc::MAX.ticks())).and_then(|summary| summary.date_taken);
    assert_eq!(max, Some(DateTimeUtc::MAX));
}

#[test]
fn decode_date_outside_the_date_time_range_is_null() {
    // C#: ExifSummaryMutationGapTests.Decode_DateOutsideTheDateTimeRange_IsNull (2 rows)
    for ticks in [-1_i64, 3_155_378_976_000_000_000] {
        assert_eq!(codec::decode(&date_block(ticks)), None, "ticks {ticks}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Extra boundary tests (no C# twin): one per mutation class of `clean_text`
// ---------------------------------------------------------------------------------------------------------------------

fn clean(text: &str) -> Option<String> {
    ExifSummary::clean_text(Some(text))
}

#[test]
fn clean_text_cap_is_exactly_64_utf16_units_at_63_64_and_65() {
    // Extra (no C# twin): off-by-one mutants of the cap (63 or 65 kept) fail here.
    assert_eq!(clean(&"a".repeat(63)), Some("a".repeat(63)));
    assert_eq!(clean(&"a".repeat(64)), Some("a".repeat(64)));
    assert_eq!(clean(&"a".repeat(65)), Some("a".repeat(64)));
}

#[test]
fn clean_text_cap_with_trailing_space_is_trimmed_after_the_cap() {
    // Extra (no C# twin): the space after 63 letters is inside the cap and is trimmed; a 64-unit space is cut away.
    assert_eq!(clean(&format!("{} b", "a".repeat(63))), Some("a".repeat(63)));
    assert_eq!(clean(&format!("{} ", "a".repeat(63))), Some("a".repeat(63)));
    assert_eq!(clean(&format!("{} ", "a".repeat(64))), Some("a".repeat(64)));
}

#[test]
fn clean_text_cap_counts_utf16_units_not_chars() {
    // Extra (no C# twin): a `chars()` or byte count mutant keeps the wrong amount of these texts.
    // 40 emoji = 80 UTF-16 units, cap keeps 32 emoji (64 units); 33 emoji = 66 units, still 32.
    assert_eq!(clean(&"\u{1F600}".repeat(40)), Some("\u{1F600}".repeat(32)));
    assert_eq!(clean(&"\u{1F600}".repeat(33)), Some("\u{1F600}".repeat(32)));
    // 63 letters then an emoji: the emoji would need 65 units, its high surrogate is dropped with it.
    assert_eq!(clean(&format!("{}\u{1F600}", "a".repeat(63))), Some("a".repeat(63)));
    // 40 combining pairs = 80 chars and 80 units; cap keeps 32 pairs.
    assert_eq!(clean(&"e\u{301}".repeat(40)), Some("e\u{301}".repeat(32)));
}

#[test]
fn clean_text_trims_after_the_cap_not_before_it() {
    // Extra (no C# twin): 70 spaces then a letter. Cap first keeps 64 spaces, trim leaves nothing: None.
    // A trim-before-cap mutant would keep the letter.
    assert_eq!(clean(&format!("{}a", " ".repeat(70))), None);
}

#[test]
fn clean_text_stops_at_the_first_character_that_does_not_fit_the_cap() {
    // Extra (no C# twin): C# stops at the cap; a later narrower char must not be appended after a dropped wide one.
    assert_eq!(clean(&format!("{}\u{1F600}b", "a".repeat(63))), Some("a".repeat(63)));
    assert_eq!(
        clean(&format!("{}\u{1F600}b", "a".repeat(62))),
        Some(format!("{}\u{1F600}", "a".repeat(62)))
    );
}

#[test]
fn parse_date_accepts_the_last_day_of_every_month_and_rejects_the_day_after() {
    // Extra (no C# twin): kills day-count mutants per month (golden cases only cover a few months).
    let days = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for (index, last) in days.iter().enumerate() {
        let month = index + 1;
        let ok = format!("2024:{month:02}:{last:02} 23:59:59");
        let bad = format!("2024:{month:02}:{:02} 00:00:00", last + 1);
        assert!(ExifSummary::parse_date(Some(&ok)).is_some(), "{ok}");
        assert_eq!(ExifSummary::parse_date(Some(&bad)), None, "{bad}");
    }
    assert_eq!(ExifSummary::parse_date(Some("2023:02:29 00:00:00")), None);
    assert_eq!(ExifSummary::parse_date(Some("1900:02:29 00:00:00")), None);
    assert!(ExifSummary::parse_date(Some("2000:02:29 00:00:00")).is_some());
    assert_eq!(ExifSummary::parse_date(Some("2024:13:01 00:00:00")), None);
    assert_eq!(ExifSummary::parse_date(Some("2024:00:01 00:00:00")), None);
    assert_eq!(ExifSummary::parse_date(Some("2024:01:00 00:00:00")), None);
}

// ---------------------------------------------------------------------------------------------------------------------
// Extra robustness and round-trip tests (no C# twin)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn decode_never_panics_for_every_prefix_and_single_byte_mutation_of_the_full_block() {
    // Extra (no C# twin): decode runs on every preview-cache hit and must not panic on any cut or corrupted block.
    let bytes = codec::encode(Some(&full_summary()));
    for length in 0..=bytes.len() {
        let _ = codec::decode(&bytes[..length]);
    }
    for index in 0..bytes.len() {
        for value in 0..=u8::MAX {
            let mut mutated = bytes.clone();
            mutated[index] = value;
            let _ = codec::decode(&mutated);
        }
    }
}

#[test]
fn encode_then_decode_round_trips_every_create_golden_output() {
    // Extra (no C# twin): decode(encode(create(input))) equals create(input) for every create golden.
    let mut failures = Vec::new();
    for (input, _) in paired("exif-summary-create", 38) {
        let Some(created) = create_from_input(&input) else {
            continue;
        };
        let back = codec::decode(&codec::encode(Some(&created)));
        if back.as_ref() != Some(&created) {
            failures.push(format!("{}: decoded {back:?}, created {created:?}", case_name(&input)));
        }
    }
    assert_no_failures(&failures);
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden replays (tests/golden/exif-summary-*.json)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn golden_create_matches_csharp() {
    // tests/golden/exif-summary-create.json (38 cases): `ExifSummary::create`, every field and the encoded hex.
    let mut failures = Vec::new();
    for (input, expected) in paired("exif-summary-create", 38) {
        let name = case_name(&input);
        let created = create_from_input(&input);
        failures.extend(
            summary_mismatches(created.as_ref(), &expected["summary"])
                .into_iter()
                .map(|m| format!("{name}: {m}")),
        );
    }
    assert_no_failures(&failures);
}

#[test]
fn golden_text_matches_csharp() {
    // tests/golden/exif-summary-text.json (40 cases): `ExifSummary::clean_text` and its UTF-16 length.
    let mut failures = Vec::new();
    for (input, expected) in paired("exif-summary-text", 40) {
        let name = case_name(&input);
        let text = expand(&input, "text");
        let cleaned = ExifSummary::clean_text(text.as_deref());
        let mut bad = Vec::new();
        cmp(&mut bad, "cleaned", cleaned.as_deref(), expected["cleaned"].as_str());
        cmp(
            &mut bad,
            "utf16_length",
            cleaned.as_deref().map(|s| s.encode_utf16().count()),
            expected["utf16_length"]
                .as_u64()
                .map(|n| usize::try_from(n).expect("length fits usize")),
        );
        failures.extend(bad.into_iter().map(|m| format!("{name}: {m}")));
    }
    assert_no_failures(&failures);
}

#[test]
fn golden_date_matches_csharp() {
    // tests/golden/exif-summary-date.json (49 cases): `ExifSummary::parse_date` ticks and text.
    let mut failures = Vec::new();
    for (input, expected) in paired("exif-summary-date", 49) {
        let name = case_name(&input);
        let text = expand(&input, "text");
        let parsed = ExifSummary::parse_date(text.as_deref());
        let mut bad = Vec::new();
        cmp(
            &mut bad,
            "ticks",
            parsed.map(DateTimeUtc::ticks),
            expected["ticks"].as_i64(),
        );
        cmp(
            &mut bad,
            "date_text",
            parsed.map(date_text),
            expected["date_text"].as_str().map(str::to_owned),
        );
        failures.extend(bad.into_iter().map(|m| format!("{name}: {m}")));
    }
    assert_no_failures(&failures);
}

#[test]
fn golden_encode_matches_csharp() {
    // tests/golden/exif-summary-encode.json (22 cases): unsanitised summaries, `encode` bytes, then the decode.
    let mut failures = Vec::new();
    for (input, expected) in paired("exif-summary-encode", 22) {
        let name = case_name(&input);
        let summary = summary_from_input(&input);
        let bytes = codec::encode(Some(&summary));
        let mut bad = Vec::new();
        cmp(
            &mut bad,
            "encoded_hex",
            hex_upper(&bytes),
            expected["encoded_hex"].as_str().expect("hex").to_owned(),
        );
        cmp(
            &mut bad,
            "encoded_length",
            Some(bytes.len()),
            expected["encoded_length"]
                .as_u64()
                .map(|n| usize::try_from(n).expect("length fits usize")),
        );
        cmp(
            &mut bad,
            "is_empty",
            Some(summary.is_empty()),
            expected["is_empty"].as_bool(),
        );
        let decoded = codec::decode(&bytes);
        bad.extend(summary_mismatches(decoded.as_ref(), &expected["decoded"]));
        failures.extend(bad.into_iter().map(|m| format!("{name}: {m}")));
    }
    assert_no_failures(&failures);
}

#[test]
fn golden_decode_matches_csharp() {
    // tests/golden/exif-summary-decode.json (46 cases): `decode` of hand-built blocks, then the re-encoded hex.
    let mut failures = Vec::new();
    for (input, expected) in paired("exif-summary-decode", 46) {
        let name = case_name(&input);
        let bytes = from_hex(input["hex"].as_str().expect("hex input"));
        let decoded = codec::decode(&bytes);
        failures.extend(
            summary_mismatches(decoded.as_ref(), &expected["decoded"])
                .into_iter()
                .map(|m| format!("{name}: {m}")),
        );
    }
    assert_no_failures(&failures);
}
