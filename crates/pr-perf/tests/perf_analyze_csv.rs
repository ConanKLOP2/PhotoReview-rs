//! Tests of `pr_perf::csv` (W1-16). Port of `PhotoReview.PerfAnalysis/PerfAnalyzeCsv.cs` (C# baseline e441d67a)
//! plus the golden `perf-csv` command (`tests/golden/perf-csv.json`, every case checked).

use std::path::Path;

use pr_perf::csv::{
    self, DiagFlags, PerfCsvFile, PerfRow, STOPWATCH_FREQUENCY, decode_bytes, parse_double_finite, parse_integer_i32,
    parse_integer_i64, parse_text, read,
};
use pr_test_support::TempRoot;
use pr_test_support::golden;
use serde_json::Value;
use serde_json::value::RawValue;

/// Hex decoder for the `raw_hex` golden inputs (lower or upper case, even length).
fn hex_decode(s: &str) -> Vec<u8> {
    assert!(s.len() % 2 == 0, "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex byte"))
        .collect()
}

/// The golden `sample` file content (the same text as `tools/diag/samples/perf-sample.csv`).
fn sample_content() -> String {
    let golden = golden::load("perf-csv");
    let case = golden
        .cases
        .iter()
        .find(|c| c["kind"] == "file" && c["name"] == "sample")
        .expect("golden has the 'sample' file case");
    case["content"].as_str().expect("sample content").to_owned()
}

/// Writes `bytes` to `<root>/<name>` and reads it back with the public `read`.
fn read_bytes(root: &TempRoot, name: &str, bytes: &[u8]) -> PerfCsvFile {
    let path = root.join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    read(&path).expect("read fixture")
}

fn row_with_text(text: &str) -> PerfRow {
    PerfRow {
        utc_ticks: 1,
        qpc_ticks: 1,
        thread: 1,
        event: "Ev".to_owned(),
        nav: text.to_owned(),
        path_id: "p".to_owned(),
        a: text.to_owned(),
        b: text.to_owned(),
        c: text.to_owned(),
        d: text.to_owned(),
        text: String::new(),
    }
}

// ---- CSV parsing (C# PerfAnalyzeTests) ----

#[test]
fn read_sample_csv_parses_82_rows_and_skips_comments_and_header() {
    // C#: PerfAnalyzeTests.ParsesDataRowsAndSkipsCommentsAndHeader
    let root = TempRoot::new("perf-csv-sample");
    let file = read_bytes(&root, "perf-sample.csv", sample_content().as_bytes());

    assert_eq!(file.qpc_frequency, 10_000_000);
    assert_eq!(file.dropped_rows, 0);
    assert_eq!(file.rows.len(), 82);
    assert!(file.rows.iter().all(|r| !r.event.starts_with('#')));
    assert!(file.rows.iter().all(|r| r.event != "utcTicks"));
}

#[test]
fn read_trailer_dropped_comment_sets_dropped_rows_and_frequency() {
    // C#: PerfAnalyzeTests.ReadsDroppedCountFromTrailerComment
    let root = TempRoot::new("perf-csv-dropped");
    let text = "# commit=test diag= qpcFrequency=5000000\n\
                utcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text\n\
                1,1000,1,ShowStart,1,,0,,,,Preview\n\
                # dropped=7\n";
    let file = read_bytes(&root, "perf-1-x.csv", text.as_bytes());

    assert_eq!(file.qpc_frequency, 5_000_000);
    assert_eq!(file.dropped_rows, 7);
    assert_eq!(file.rows.len(), 1);
}

// ---- PerfRow numeric columns (C# BenchmarkStatisticsEdgeTests) ----

#[test]
fn perf_row_non_finite_numbers_are_none_for_nan_infinity_overflow_and_garbage() {
    // C#: BenchmarkStatisticsEdgeTests.PerfRow_NonFiniteNumbersAreNull (InlineData NaN, Infinity, -Infinity, 1e999, "", abc)
    for text in ["NaN", "Infinity", "-Infinity", "1e999", "", "abc"] {
        let row = row_with_text(text);
        assert_eq!(row.a_num(), None, "a_num of {text:?}");
        assert_eq!(row.d_num(), None, "d_num of {text:?}");
    }
}

#[test]
fn perf_row_finite_numbers_parse() {
    // C#: BenchmarkStatisticsEdgeTests.PerfRow_FiniteNumbersParse (InlineData 12.5, -3, 1e3)
    assert_eq!(row_with_text("12.5").a_num(), Some(12.5));
    assert_eq!(row_with_text("-3").a_num(), Some(-3.0));
    assert_eq!(row_with_text("1e3").a_num(), Some(1000.0));
}

#[test]
fn perf_row_nav_id_parses_integer_nav_column() {
    // C#: (no direct test) PerfRow.NavId
    let mut row = row_with_text("");
    row.nav = " 5 ".to_owned();
    assert_eq!(row.nav_id(), Some(5));
    row.nav = "007".to_owned();
    assert_eq!(row.nav_id(), Some(7));
    row.nav = "+9".to_owned();
    assert_eq!(row.nav_id(), Some(9));
    row.nav = "2147483648".to_owned();
    assert_eq!(row.nav_id(), Some(2_147_483_648));
    row.nav = "1.5".to_owned();
    assert_eq!(row.nav_id(), None);
    row.nav = String::new();
    assert_eq!(row.nav_id(), None);
}

#[test]
fn qpc_to_ms_multiplies_before_dividing() {
    // C#: (no direct test) PerfCsvFile.QpcToMs
    let file = PerfCsvFile {
        path: Path::new("x.csv").to_path_buf(),
        diag_flags: DiagFlags::new(),
        qpc_frequency: 10_000_000,
        dropped_rows: 0,
        rows: Vec::new(),
    };
    assert_eq!(file.qpc_to_ms(12345), 12345.0 * 1000.0 / 10_000_000.0);
    assert_eq!(file.qpc_to_ms(0), 0.0);
}

#[test]
fn stopwatch_frequency_is_ten_megahertz() {
    // C#: (no direct test) Stopwatch.Frequency on Windows 10+
    assert_eq!(STOPWATCH_FREQUENCY, 10_000_000);
}

#[test]
fn diag_flags_are_case_insensitive_and_keep_first_spelling_and_order() {
    // C#: (no direct test) DiagFlags (StringComparer.OrdinalIgnoreCase dictionary)
    let mut flags = DiagFlags::from_pairs([
        ("PHOTOREVIEW_DIAG_PRELOAD_WORKERS", "4"),
        ("PHOTOREVIEW_DIAG_PREREAD", "1"),
    ]);
    flags.insert("photoreview_diag_preload_workers", "8");
    assert_eq!(flags.len(), 2);
    assert_eq!(flags.get("PHOTOREVIEW_DIAG_preload_workers"), Some("8"));
    let items: Vec<(&str, &str)> = flags.iter().collect();
    assert_eq!(
        items,
        vec![
            ("PHOTOREVIEW_DIAG_PRELOAD_WORKERS", "8"),
            ("PHOTOREVIEW_DIAG_PREREAD", "1")
        ]
    );
}

#[test]
fn parse_integer_rejects_non_ascii_digits_and_non_net_whitespace() {
    // C#: (no direct test) long.TryParse(NumberStyles.Integer, Invariant)
    assert_eq!(parse_integer_i64("5\0"), Some(5));
    assert_eq!(parse_integer_i64("5 \0"), Some(5));
    assert_eq!(parse_integer_i64("5\0 "), None);
    assert_eq!(parse_integer_i64("\u{00A0}3"), None);
    assert_eq!(parse_integer_i64("- 5"), None);
    assert_eq!(parse_integer_i64("-0"), Some(0));
    assert_eq!(parse_integer_i64("9223372036854775808"), None);
    assert_eq!(parse_integer_i32("2147483648"), None);
    assert_eq!(parse_integer_i32("-2147483648"), Some(i32::MIN));
    assert_eq!(parse_integer_i64("1,5"), None);
    assert_eq!(parse_integer_i64(""), None);
}

#[test]
fn parse_double_accepts_net_float_grammar_only() {
    // C#: (no direct test) double.TryParse(NumberStyles.Float, Invariant) + IsFinite
    assert_eq!(parse_double_finite("5."), Some(5.0));
    assert_eq!(parse_double_finite(".5"), Some(0.5));
    assert_eq!(parse_double_finite("1.e5"), Some(100_000.0));
    assert_eq!(parse_double_finite("1e-999"), Some(0.0));
    assert_eq!(parse_double_finite("."), None);
    assert_eq!(parse_double_finite("1e"), None);
    assert_eq!(parse_double_finite("0x10"), None);
    assert_eq!(parse_double_finite("1,5"), None);
    assert_eq!(parse_double_finite("\u{221E}"), None);
}

#[test]
fn decode_bytes_drops_utf8_bom_and_replaces_invalid_bytes() {
    // C#: (no direct test) File.ReadLines decoding; golden invalid-utf8 covers the replacement rule
    assert_eq!(decode_bytes(&[0xEF, 0xBB, 0xBF, b'a']), "a");
    assert_eq!(decode_bytes(&[]), "");
    assert_eq!(decode_bytes(&[b'a', 0xC0, b'b']), "a\u{FFFD}b");
}

#[test]
fn parse_text_splits_lines_on_lone_carriage_return() {
    // C#: (no direct test) StreamReader.ReadLine line endings
    let text = "# qpcFrequency=2000\rutcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text\r1,2,3,Ev,4,,,,,,t\r";
    let file = parse_text(Path::new("mem.csv"), text);
    assert_eq!(file.qpc_frequency, 2000);
    assert_eq!(file.rows.len(), 1);
    assert_eq!(file.rows[0].nav, "4");
    assert_eq!(file.path, Path::new("mem.csv"));
}

#[test]
fn parse_text_quotes_follow_csv_state_machine() {
    // C#: (no direct test) PerfAnalyzeCsv.SplitCsvLine quoting
    let text = "1,2,3,Ev,4,,,,,,\"a,\"\"b\"\"\"\n1,2,3,Ev,4,,,,,,ab\"c,d\"\n";
    let file = parse_text(Path::new("q.csv"), text);
    assert_eq!(file.rows.len(), 2);
    assert_eq!(file.rows[0].text, "a,\"b\"");
    assert_eq!(file.rows[1].text, "abc,d");
}

// ---- golden perf-csv (every case) ----

fn check_golden_file_case(case: &Value, root: &TempRoot) {
    let name = case["name"].as_str().expect("file case name");
    let ctx = format!("golden file case '{name}'");
    let mut bytes: Vec<u8> = match case.get("raw_hex").and_then(Value::as_str) {
        Some(hex) => hex_decode(hex),
        None => case["content"].as_str().unwrap_or_default().as_bytes().to_vec(),
    };
    if case["bom"].as_bool().unwrap_or(false) {
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.append(&mut bytes);
        bytes = with_bom;
    }
    let file = read_bytes(root, &format!("perf-{name}.csv"), &bytes);

    assert_eq!(
        file.qpc_frequency,
        case["qpc_frequency"].as_i64().expect("qpc"),
        "{ctx}: qpc_frequency"
    );
    assert_eq!(
        file.dropped_rows,
        case["dropped_rows"].as_i64().expect("dropped"),
        "{ctx}: dropped_rows"
    );

    let got_flags: Vec<[String; 2]> = file
        .diag_flags
        .iter()
        .map(|(k, v)| [k.to_owned(), v.to_owned()])
        .collect();
    let want_flags: Vec<[String; 2]> = case["diag_flags"]
        .as_array()
        .expect("diag_flags array")
        .iter()
        .map(|pair| {
            [
                pair[0].as_str().expect("flag key").to_owned(),
                pair[1].as_str().expect("flag value").to_owned(),
            ]
        })
        .collect();
    assert_eq!(got_flags, want_flags, "{ctx}: diag_flags");

    let want_rows = case["rows"].as_array().expect("rows array");
    assert_eq!(file.rows.len(), want_rows.len(), "{ctx}: row count");
    for (i, (row, want)) in file.rows.iter().zip(want_rows).enumerate() {
        let cells = want.as_array().expect("row array");
        let rctx = format!("{ctx}: row {i}");
        assert_eq!(row.utc_ticks, cells[0].as_i64().expect("utc"), "{rctx} utc");
        assert_eq!(row.qpc_ticks, cells[1].as_i64().expect("qpc"), "{rctx} qpc");
        assert_eq!(
            i64::from(row.thread),
            cells[2].as_i64().expect("thread"),
            "{rctx} thread"
        );
        let strings = [
            &row.event,
            &row.nav,
            &row.path_id,
            &row.a,
            &row.b,
            &row.c,
            &row.d,
            &row.text,
        ];
        for (offset, got) in strings.iter().enumerate() {
            let want_s = cells[3 + offset].as_str().expect("string cell");
            assert_eq!(got.as_str(), want_s, "{rctx} string column {}", 3 + offset);
        }
        assert_opt_f64(row.a_num(), &cells[11], &format!("{rctx} a_num"));
        assert_opt_f64(row.b_num(), &cells[12], &format!("{rctx} b_num"));
        assert_opt_f64(row.c_num(), &cells[13], &format!("{rctx} c_num"));
        assert_opt_f64(row.d_num(), &cells[14], &format!("{rctx} d_num"));
        assert_opt_i64(row.nav_id(), &cells[15], &format!("{rctx} nav_id"));
    }
}

fn assert_opt_f64(got: Option<f64>, want: &Value, ctx: &str) {
    if want.is_null() {
        assert_eq!(got, None, "{ctx}");
    } else {
        let w = want.as_f64().expect("number cell");
        assert_eq!(got, Some(w), "{ctx}");
    }
}

fn assert_opt_i64(got: Option<i64>, want: &Value, ctx: &str) {
    if want.is_null() {
        assert_eq!(got, None, "{ctx}");
    } else {
        assert_eq!(got, want.as_i64(), "{ctx}");
    }
}

#[test]
fn golden_perf_csv_file_cases_match_csharp_for_every_case() {
    // C#: PerfCsvReader.Read (golden command perf-csv, kind file)
    let golden = golden::load("perf-csv");
    let root = TempRoot::new("perf-csv-golden-file");
    let mut count = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "file") {
        check_golden_file_case(case, &root);
        count += 1;
    }
    assert_eq!(count, 28, "all 28 file cases checked");
}

/// The golden file read twice: `Value` for the structure, and the literal text of the double fields.
/// serde_json's default float parser is not correctly rounded for long mantissas (its `float_roundtrip` feature is
/// off in this workspace), so `Value::as_f64` can sit one ulp away from the number C# wrote (golden num case
/// `123456789.123456789`). The expected doubles are therefore re-parsed from their literal text with Rust's
/// correctly rounded `str::parse::<f64>`.
#[derive(serde::Deserialize)]
struct RawDoc<'a> {
    #[serde(borrow)]
    cases: Vec<&'a RawValue>,
}

#[derive(serde::Deserialize)]
struct RawDoubles<'a> {
    #[serde(borrow, default)]
    a_num: Option<&'a RawValue>,
    #[serde(borrow, default)]
    ms: Option<&'a RawValue>,
}

fn golden_text() -> String {
    let path = golden::repo_root().join("tests").join("golden").join("perf-csv.json");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn exact_double(literal: Option<&RawValue>) -> Option<f64> {
    literal.map(|raw| raw.get().parse::<f64>().expect("golden double literal"))
}

#[test]
fn golden_perf_csv_num_cases_match_csharp_for_every_case() {
    // C#: PerfRow.ANum / NavId (golden command perf-csv, kind num)
    let text = golden_text();
    let doc: RawDoc = serde_json::from_str(&text).expect("golden document");
    let mut count = 0;
    for raw in &doc.cases {
        let case: Value = serde_json::from_str(raw.get()).expect("golden case");
        if case["kind"] != "num" {
            continue;
        }
        let doubles: RawDoubles = serde_json::from_str(raw.get()).expect("num case doubles");
        let input = case["text"].as_str().expect("num text");
        let ctx = format!("golden num case {input:?}");
        let row = row_with_text(input);
        assert_eq!(row.a_num(), exact_double(doubles.a_num), "{ctx}: a_num");
        assert_opt_i64(row.nav_id(), &case["nav_id"], &format!("{ctx}: nav_id"));
        count += 1;
    }
    assert_eq!(count, 91, "all 91 num cases checked");
}

#[test]
fn golden_perf_csv_qpc_cases_match_csharp_for_every_case() {
    // C#: PerfCsvFile.QpcToMs (golden command perf-csv, kind qpc)
    let text = golden_text();
    let doc: RawDoc = serde_json::from_str(&text).expect("golden document");
    let mut count = 0;
    for raw in &doc.cases {
        let case: Value = serde_json::from_str(raw.get()).expect("golden case");
        if case["kind"] != "qpc" {
            continue;
        }
        let doubles: RawDoubles = serde_json::from_str(raw.get()).expect("qpc case doubles");
        let freq = case["freq"].as_i64().expect("freq");
        let delta = case["delta"].as_i64().expect("delta");
        let file = PerfCsvFile {
            path: Path::new("qpc.csv").to_path_buf(),
            diag_flags: DiagFlags::new(),
            qpc_frequency: freq,
            dropped_rows: 0,
            rows: Vec::new(),
        };
        let got = file.qpc_to_ms(delta);
        let ctx = format!("golden qpc case freq={freq} delta={delta}");
        match exact_double(doubles.ms) {
            None => assert!(!got.is_finite(), "{ctx}: expected non-finite, got {got}"),
            Some(want) => assert_eq!(got, want, "{ctx}"),
        }
        count += 1;
    }
    assert_eq!(count, 13, "all 13 qpc cases checked");
}

#[test]
fn csv_module_exposes_read_and_decode_entry_points() {
    // C#: (no direct test) PerfCsvReader.Read wiring: read = std::fs::read + decode_bytes + parse_text
    let root = TempRoot::new("perf-csv-wiring");
    let path = root.join("perf-wiring.csv");
    std::fs::write(&path, b"# qpcFrequency=77\r\n").expect("write");
    let file = csv::read(&path).expect("read");
    assert_eq!(file.qpc_frequency, 77);
    assert!(csv::read(&root.join("missing.csv")).is_err());
}

#[test]
fn comment_prefix_skips_mixed_hash_and_space_runs_before_key_value() {
    // C#: (no direct test) ParseCommentLine TrimStart('#', ' ') skips a '#' that follows a space
    let file = parse_text(Path::new("c.csv"), "# #qpcFrequency=5\n#  # dropped=3\n");
    assert_eq!(file.qpc_frequency, 5);
    assert_eq!(file.dropped_rows, 3);
}
