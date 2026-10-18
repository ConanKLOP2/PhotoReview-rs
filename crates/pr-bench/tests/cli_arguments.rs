//! Port of `tests/PhotoReview.Integration.Tests/BenchmarkCliArgumentsTests.cs`, the argument rows of
//! `BenchmarkReportFormattingTests.cs` (`PerfAnalyzeArguments_*`), the argument rows of `BenchmarkCliHardeningTests.cs`
//! (`SurplusBenchmarkArguments_AreRejected`, `IsExpectedToolFailure_Classifies`) and the golden checks over
//! `tests/golden/cli-arguments.json` (inputs in `tests/golden-inputs/cli-arguments.json`; oracle `oracle/W7-03d.cs`).
//!
//! Port of `tools/PhotoReview.Benchmark.Cli/BenchmarkCliArguments.cs` (C# baseline d92d6aae). Unit W7-03d, contract
//! `contracts/W7-03d.md`. Red stage: every stub body is `todo!()`, so each test here fails until the porter implements it.
//!
//! Documented skips (not ported here): `RawDecoderBenchmark_IterationsValid/Invalid` (RawDecoderBenchmark) and
//! `PerfSession_SourceBytesCache_*` (PerfSession): other tools of the C# project (contract, "NOT here").
//! `Parsing_IsCultureInvariant` keeps only its invariant-parsing assertions: Rust has no current culture.

use std::collections::HashMap;

use pr_bench::cli::arguments::{
    is_expected_tool_failure, parse_defined_enum, parse_int_in_range, parse_perf_analyze_rules,
    parse_positive_finite_double, parse_positive_int, parse_widths, reject_surplus_benchmark_arguments, resolve_output,
    resolve_profiles,
};
use pr_bench::cli::backend::{ProfileCatalog, ProfileInfo, ToolError};
use pr_test_support::golden;
use serde_json::{Value, json};

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// The 30-row catalog as the golden `catalog` row lists it (`all`, `runnable`, `file_action`), with fields the CLI
/// does not read left at neutral values.
struct GoldenCatalog {
    all: Vec<ProfileInfo>,
    runnable: Vec<ProfileInfo>,
}

impl ProfileCatalog for GoldenCatalog {
    fn all(&self) -> Vec<ProfileInfo> {
        self.all.clone()
    }
    fn runnable(&self) -> Vec<ProfileInfo> {
        self.runnable.clone()
    }
    fn find(&self, id: &str) -> Option<ProfileInfo> {
        self.all.iter().find(|p| p.id.eq_ignore_ascii_case(id)).cloned()
    }
}

fn profile_info(id: &str, file_action: bool) -> ProfileInfo {
    ProfileInfo {
        id: id.to_owned(),
        name: id.to_owned(),
        description: String::new(),
        workload: if file_action { "FileAction" } else { "Sequential" }.to_owned(),
        loading_mode: "Fast".to_owned(),
        workers: 1,
        next_window: 0,
        previous_window: 0,
        iterations: 1,
        correctness_only: false,
        is_file_action: file_action,
    }
}

fn load_catalog() -> GoldenCatalog {
    let golden = golden::load("cli-arguments");
    let row = golden
        .cases
        .iter()
        .find(|c| c["op"] == "catalog")
        .expect("cli-arguments golden has a catalog row");
    let ids = |key: &str| -> Vec<String> {
        row[key]
            .as_array()
            .unwrap_or_else(|| panic!("catalog row has no '{key}' list"))
            .iter()
            .map(|v| v.as_str().expect("profile id is a string").to_owned())
            .collect()
    };
    let file_ids = ids("file_action");
    let make = |id: &String| profile_info(id, file_ids.contains(id));
    GoldenCatalog {
        all: ids("all").iter().map(make).collect(),
        runnable: ids("runnable").iter().map(make).collect(),
    }
}

fn ids_of(profiles: &[ProfileInfo]) -> Vec<String> {
    profiles.iter().map(|p| p.id.clone()).collect()
}

fn golden_inputs(name: &str) -> HashMap<String, Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .expect("golden input has cases")
        .iter()
        .map(|c| (golden::str_of(c, "name").to_owned(), c.clone()))
        .collect()
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("list of strings")
        .iter()
        .map(|v| v.as_str().expect("string").to_owned())
        .collect()
}

/// Runs one golden input through the function its `op` names; the result is compared with the golden `result`.
fn run_case(input: &Value, expected: &Value, catalog: &GoldenCatalog) -> Result<Value, ToolError> {
    let op = golden::str_of(input, "op");
    match op {
        "widths" => parse_widths(golden::str_of(input, "text")).map(|v| json!(v)),
        "positive_int" => {
            parse_positive_int(golden::str_of(input, "text"), golden::str_of(input, "name_arg")).map(|v| json!(v))
        }
        "int_range" => {
            let min = golden::i64_of(input, "min") as i32;
            let max = golden::i64_of(input, "max") as i32;
            parse_int_in_range(
                golden::str_of(input, "text"),
                golden::str_of(input, "name_arg"),
                min,
                max,
            )
            .map(|v| json!(v))
        }
        "positive_double" => {
            parse_positive_finite_double(golden::str_of(input, "text"), golden::str_of(input, "name_arg"))
                .map(|v| json!(v))
        }
        "defined_enum" => {
            // The member table is written in the golden output row: "Name=value", in `Enum.GetNames` order.
            let members: Vec<(String, i32)> = string_list(&expected["members"])
                .into_iter()
                .map(|m| {
                    let (name, value) = m.rsplit_once('=').expect("member is Name=value");
                    (name.to_owned(), value.parse().expect("member value is an integer"))
                })
                .collect();
            let borrowed: Vec<(&str, i32)> = members.iter().map(|(n, v)| (n.as_str(), *v)).collect();
            parse_defined_enum(
                golden::str_of(input, "text"),
                golden::str_of(input, "name_arg"),
                &borrowed,
            )
            .map(|v| json!(v))
        }
        "resolve_profiles" => {
            let list = string_list(&input["args"]);
            resolve_profiles(golden::str_of(input, "mode"), &list, catalog).map(|p| json!(ids_of(&p)))
        }
        "resolve_output" => {
            let list = string_list(&input["args"]);
            Ok(json!(resolve_output(golden::str_of(input, "mode"), &list)))
        }
        "perf_rules" => {
            let list = string_list(&input["args"]);
            parse_perf_analyze_rules(&list).map(|v| json!(v))
        }
        "surplus" => {
            let list = string_list(&input["args"]);
            reject_surplus_benchmark_arguments(&list).map(|()| Value::Null)
        }
        "expected_failure" => Ok(json!(is_expected_tool_failure(golden::str_of(input, "type")))),
        other => panic!("unknown cli-arguments golden op '{other}'"),
    }
}

/// Numbers compare as floats (the golden writes `2.5` and `2` alike); everything else compares structurally.
fn same_value(want: &Value, got: &Value) -> bool {
    if want.is_number() && got.is_number() {
        return want.as_f64() == got.as_f64();
    }
    want == got
}

fn check(name: &str, expected: &Value, got: Result<Value, ToolError>, failures: &mut Vec<String>) {
    let result = &expected["result"];
    match (result.get("throws"), got) {
        (Some(type_name), Err(err)) => {
            let want_message = golden::str_of(result, "message");
            if type_name != err.type_name.as_str() || want_message != err.message {
                failures.push(format!(
                    "{name}: want {type_name} {want_message:?}, got {} {:?}",
                    err.type_name, err.message
                ));
            }
        }
        (Some(type_name), Ok(value)) => failures.push(format!("{name}: want {type_name} error, got ok {value}")),
        (None, Ok(value)) => {
            if !same_value(&result["ok"], &value) {
                failures.push(format!("{name}: want ok {}, got {value}", result["ok"]));
            }
        }
        (None, Err(err)) => failures.push(format!("{name}: want ok {}, got error {err}", result["ok"])),
    }
}

#[test]
fn golden_cli_arguments() {
    let golden = golden::load("cli-arguments");
    let inputs = golden_inputs("cli-arguments");
    let catalog = load_catalog();
    let mut failures = Vec::new();
    for case in golden.cases.iter().filter(|c| c["op"] != "catalog") {
        let name = golden::str_of(case, "name");
        let input = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no golden input named {name}"));
        check(name, case, run_case(input, case, &catalog), &mut failures);
    }
    assert!(
        failures.is_empty(),
        "{} cli-arguments golden mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// C#: BenchmarkCliArgumentsTests.Benchmark_DefaultsToRecommendedAuto
#[test]
fn benchmark_defaults_to_recommended_auto() {
    let catalog = load_catalog();
    let profiles = resolve_profiles("--benchmark", &args(&["--benchmark", "C:/photos"]), &catalog).expect("resolves");
    assert_eq!(ids_of(&profiles), vec!["recommended-auto"]);
}

// C#: BenchmarkCliArgumentsTests.Benchmark_ProfileListParsing (4 rows)
#[test]
fn benchmark_profile_list_parsing() {
    let catalog = load_catalog();
    let rows: [(&str, &[&str]); 4] = [
        ("fast-balanced", &["fast-balanced"]),
        (" fast-balanced , Instant-Review ", &["fast-balanced", "instant-review"]),
        ("fast-balanced,,logging-on,", &["fast-balanced", "logging-on"]),
        ("logging-on,logging-on", &["logging-on", "logging-on"]),
    ];
    for (list, expected) in rows {
        let profiles = resolve_profiles("--benchmark", &args(&["--benchmark", "C:/photos", list]), &catalog)
            .unwrap_or_else(|e| panic!("list {list:?}: {e}"));
        assert_eq!(ids_of(&profiles), expected, "list {list:?}");
    }
}

// C#: BenchmarkCliArgumentsTests.Benchmark_EmptyProfileListIsRejected (4 rows)
#[test]
fn benchmark_empty_profile_list_is_rejected() {
    let catalog = load_catalog();
    for list in ["", " ", ",", " , ,"] {
        let err = resolve_profiles("--benchmark", &args(&["--benchmark", "C:/photos", list]), &catalog)
            .expect_err("an empty list is an error");
        assert_eq!(err.type_name, "ArgumentException", "list {list:?}");
        assert!(
            err.message.contains("No benchmark profile"),
            "list {list:?}: {}",
            err.message
        );
    }
}

// C#: BenchmarkCliArgumentsTests.Benchmark_UnknownProfileIsNamed
#[test]
fn benchmark_unknown_profile_is_named() {
    let catalog = load_catalog();
    let err = resolve_profiles(
        "--benchmark",
        &args(&["--benchmark", "C:/photos", "fast-balanced,nope"]),
        &catalog,
    )
    .expect_err("unknown id");
    assert_eq!(err.type_name, "ArgumentException");
    assert!(err.message.contains("nope"), "{}", err.message);
}

// C#: BenchmarkCliArgumentsTests.BatchModes_SelectProfiles
#[test]
fn batch_modes_select_profiles() {
    let catalog = load_catalog();
    let runnable_ids = ids_of(&catalog.runnable());
    let all_runnable =
        resolve_profiles("--benchmark-all", &args(&["--benchmark-all", "C:/photos"]), &catalog).expect("resolves");
    assert_eq!(ids_of(&all_runnable), runnable_ids);

    let actions = resolve_profiles(
        "--benchmark-actions",
        &args(&["--benchmark-actions", "C:/photos"]),
        &catalog,
    )
    .expect("resolves");
    assert!(!actions.is_empty());
    assert!(actions.iter().all(|p| p.is_file_action && p.workload == "FileAction"));
}

// C#: BenchmarkCliArgumentsTests.UnknownMode_IsRejected
#[test]
fn unknown_mode_is_rejected() {
    let catalog = load_catalog();
    let err = resolve_profiles("--nope", &args(&["--nope", "x"]), &catalog).expect_err("not a benchmark mode");
    assert_eq!(err.type_name, "ArgumentException");
}

// C#: BenchmarkCliArgumentsTests.Output_Resolution (4 rows)
#[test]
fn output_resolution() {
    let rows: [(&str, &str, Option<&str>); 4] = [
        ("--benchmark", "C:/out", None),
        ("--benchmark-all", "C:/out", Some("C:/out")),
        ("--benchmark-actions", "C:/out", Some("C:/out")),
        ("--benchmark-all", " ", None),
    ];
    for (mode, third, expected) in rows {
        assert_eq!(
            resolve_output(mode, &args(&[mode, "C:/photos", third])).as_deref(),
            expected,
            "{mode} {third:?}"
        );
    }
}

// C#: BenchmarkCliArgumentsTests.Output_AbsentWhenNoThirdArgument
#[test]
fn output_absent_when_no_third_argument() {
    assert_eq!(
        resolve_output("--benchmark-all", &args(&["--benchmark-all", "C:/photos"])),
        None
    );
}

// C#: BenchmarkCliArgumentsTests.Widths_Valid (3 rows)
#[test]
fn widths_valid() {
    assert_eq!(parse_widths("0,1920,2560").expect("valid"), vec![0, 1920, 2560]);
    assert_eq!(parse_widths(" 800 , 1600 ").expect("valid"), vec![800, 1600]);
    assert_eq!(parse_widths("1920,,3840,").expect("valid"), vec![1920, 3840]);
}

// C#: BenchmarkCliArgumentsTests.Widths_DuplicatesAreDropped
#[test]
fn widths_duplicates_are_dropped() {
    assert_eq!(parse_widths("0,1920,0,1920").expect("valid"), vec![0, 1920]);
}

// C#: BenchmarkCliArgumentsTests.Widths_Invalid (9 rows)
#[test]
fn widths_invalid() {
    let rows = [
        "",
        ",",
        "-1",
        "+5",
        "19.20",
        "1,920x",
        "abc",
        "99999999999",
        "\u{0661}\u{0662}", // Arabic-Indic digits
    ];
    for text in rows {
        let err = parse_widths(text).expect_err("invalid width list");
        assert_eq!(err.type_name, "ArgumentException", "{text:?}");
    }
}

// C#: BenchmarkCliArgumentsTests.PositiveInt_Valid (3 rows)
#[test]
fn positive_int_valid() {
    assert_eq!(parse_positive_int("1", "n").expect("valid"), 1);
    assert_eq!(parse_positive_int("60", "n").expect("valid"), 60);
    assert_eq!(parse_positive_int("2147483647", "n").expect("valid"), i32::MAX);
}

// C#: BenchmarkCliArgumentsTests.PositiveInt_Invalid (5 rows; the message contains the option name)
#[test]
fn positive_int_invalid() {
    for text in ["0", "-3", "", "8.5", "2147483648"] {
        let err = parse_positive_int(text, "worker count").expect_err("invalid positive int");
        assert_eq!(err.type_name, "ArgumentException", "{text:?}");
        assert!(err.message.contains("worker count"), "{text:?}: {}", err.message);
    }
}

// C#: BenchmarkCliArgumentsTests.Parsing_IsCultureInvariant (the invariant assertions only: Rust has no current culture)
#[test]
fn parsing_is_culture_invariant() {
    assert_eq!(parse_widths("1920,0").expect("valid"), vec![1920, 0]);
    assert_eq!(parse_positive_int("12", "n").expect("valid"), 12);
}

// C#: BenchmarkReportFormattingTests.PerfAnalyzeArguments_BadRulesAreRejected (3 rows)
#[test]
fn perf_analyze_bad_rules_are_rejected() {
    let rows: [&[&str]; 3] = [&["--rules"], &["--rulez", "x.json"], &["--rules", ""]];
    for extra in rows {
        let mut list = vec!["--perf-analyze".to_owned(), "runs".to_owned()];
        list.extend(extra.iter().map(|s| (*s).to_owned()));
        assert!(parse_perf_analyze_rules(&list).is_err(), "{extra:?}");
    }
}

// C#: BenchmarkReportFormattingTests.PerfAnalyzeArguments_RulesPathIsParsed
#[test]
fn perf_analyze_rules_path_is_parsed() {
    assert_eq!(
        parse_perf_analyze_rules(&args(&["--perf-analyze", "runs"])).expect("valid"),
        None
    );
    assert_eq!(
        parse_perf_analyze_rules(&args(&["--perf-analyze", "runs", "--rules", "r.json"]))
            .expect("valid")
            .as_deref(),
        Some("r.json")
    );
}

// C#: BenchmarkCliHardeningTests.SurplusBenchmarkArguments_AreRejected (3 rows, T-B-06)
#[test]
fn surplus_benchmark_arguments_are_rejected() {
    let rows: [[&str; 4]; 3] = [
        ["--benchmark-all", "folder", "out", "extra"],
        ["--benchmark-actions", "folder", "out", "extra"],
        ["--benchmark", "folder", "recommended-auto", "extra"],
    ];
    for row in rows {
        let err = reject_surplus_benchmark_arguments(&args(&row)).expect_err("surplus argument");
        assert_eq!(err.type_name, "ArgumentException", "{row:?}");
    }
}

// C#: BenchmarkCliHardeningTests.IsExpectedToolFailure_Classifies (T-B-06; the type names are the .NET exception names)
#[test]
fn is_expected_tool_failure_classifies() {
    assert!(is_expected_tool_failure("DirectoryNotFoundException"));
    assert!(is_expected_tool_failure("UnauthorizedAccessException"));
    assert!(is_expected_tool_failure("NotSupportedException"));
    assert!(is_expected_tool_failure("ArgumentException"));
    assert!(!is_expected_tool_failure("OperationCanceledException"));
    // APP-T18: the OOM case was a duplicated cancellation line; the reserved runtime exception is NOT an expected failure.
    assert!(!is_expected_tool_failure("OutOfMemoryException"));
}

// Extra (no C# twin): the Widths_Invalid / PositiveInt_Invalid contract that bad numbers are ArgumentException, never FormatException.
#[test]
fn invalid_numbers_are_argument_errors_not_format_errors() {
    let err = parse_int_in_range("abc", "--repeat", 1, 1000).expect_err("not a number");
    assert_eq!(err.type_name, "ArgumentException");
}
