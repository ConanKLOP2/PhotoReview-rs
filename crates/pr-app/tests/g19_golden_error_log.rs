//! G18 (docs/reviews/pr-164.md M1/L2): every `format`, `truncate`, `unusable` and `report` case of
//! `tests/golden/shell-error-log.json` is compared with the Rust log (the older golden test only checked one
//! invented entry). The `thrown` cases need a C# stack trace and have no Rust counterpart.
use pr_app::startup::error_log::{self, ErrorDetails, InnerErrorDetails};
use pr_core::time::DateTimeUtc;
use pr_test_support::TempRoot;
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;

static CONFIG_LOCK: Mutex<()> = Mutex::new(());

fn hresult(type_name: &str) -> i32 {
    (match type_name {
        "InvalidOperationException" => 0x8013_1509u32,
        "IOException" => 0x8013_1620,
        "ArgumentException" => 0x8007_0057,
        "Exception" => 0x8013_1500,
        "UnauthorizedAccessException" => 0x8007_0005,
        other => panic!("unknown exception type {other}"),
    }) as i32
}

fn qualified(type_name: &str) -> String {
    format!(
        "System.{}{type_name}",
        if type_name == "IOException" { "IO." } else { "" }
    )
}

fn details(exception: &Value) -> ErrorDetails {
    let ty = str_of(exception, "type");
    let mut d = ErrorDetails::new(qualified(ty), hresult(ty), str_of(exception, "message"));
    if let Some(inner) = exception.get("inner") {
        d.inner = Some(InnerErrorDetails {
            type_name: qualified(str_of(inner, "type")),
            message: str_of(inner, "message").to_owned(),
        });
    }
    d
}

fn utc(text: &str) -> DateTimeUtc {
    DateTimeUtc::parse_json(text).unwrap().0
}

#[test]
fn shell_error_log_format_matches_every_golden_case() {
    let g = golden::load("shell-error-log");
    let mut formats = 0;
    for case in g.cases.iter().filter(|c| c["kind"] == "format") {
        let got = error_log::format(
            utc(str_of(case, "utc")),
            str_of(case, "context"),
            &details(&case["exception"]),
        );
        assert_eq!(got, str_of(case, "text"), "case {}", str_of(case, "id"));
        formats += 1;
    }
    assert_eq!(formats, 14);
}

#[test]
fn shell_error_log_constants_match_the_golden() {
    let g = golden::load("shell-error-log");
    let c = g.cases.iter().find(|c| c["kind"] == "constants").unwrap();
    assert_eq!(i64_of(c, "max_bytes") as u64, error_log::MAX_BYTES);
    assert_eq!(i64_of(c, "max_message_chars") as usize, error_log::MAX_MESSAGE_CHARS);
    assert_eq!(i64_of(c, "max_stack_frames") as usize, error_log::MAX_STACK_FRAMES);
    assert_eq!(str_of(c, "file_name"), error_log::FILE_NAME);
}

#[test]
fn shell_error_log_truncate_matches_every_golden_case() {
    let g = golden::load("shell-error-log");
    let dir = TempRoot::new("g19-golden-error-log-truncate");
    let mut n = 0;
    for case in g.cases.iter().filter(|c| c["kind"] == "truncate") {
        let id = str_of(case, "id");
        let pad = ".".repeat(i64_of(case, "pad") as usize);
        let old: String = (0..i64_of(case, "lines"))
            .map(|i| format!("OLD-{i:03}{pad}\n"))
            .collect();
        let path = dir.join(format!("truncate-{n}.log"));
        std::fs::write(&path, &old).unwrap();
        let limit = i64_of(case, "limit");
        let d = ErrorDetails::new("System.InvalidOperationException", 0x8013_1509u32 as i32, "fresh");
        error_log::append_at(&path, "after", &d, limit as u64, utc("2026-10-13T06:07:08Z"));
        let text = std::fs::read_to_string(&path).unwrap();
        let olds: Vec<i64> = text
            .lines()
            .filter_map(|l| {
                l.strip_prefix("OLD-")
                    .and_then(|r| r.get(..3))
                    .and_then(|d| d.parse().ok())
            })
            .collect();
        assert_eq!(text.len() as i64, i64_of(case, "text_length"), "case {id}: text_length");
        assert_eq!(
            (text.len() as i64) < limit,
            case["under_limit"].as_bool().unwrap(),
            "case {id}: under_limit"
        );
        assert_eq!(
            text.starts_with("OLD-"),
            case["starts_with_old"].as_bool().unwrap(),
            "case {id}: starts_with_old"
        );
        assert_eq!(
            text.contains("[after]"),
            case["has_after_tag"].as_bool().unwrap(),
            "case {id}: has_after_tag"
        );
        assert_eq!(
            olds.first().copied().unwrap_or(-1),
            i64_of(case, "first_old"),
            "case {id}: first_old"
        );
        assert_eq!(
            olds.last().copied().unwrap_or(-1),
            i64_of(case, "last_old"),
            "case {id}: last_old"
        );
        assert_eq!(
            olds.len() as i64,
            i64_of(case, "kept_old_lines"),
            "case {id}: kept_old_lines"
        );
        n += 1;
    }
    assert_eq!(n, 5);
}

#[test]
fn shell_error_log_unusable_paths_never_throw_like_the_golden() {
    let g = golden::load("shell-error-log");
    let mut n = 0;
    for case in g.cases.iter().filter(|c| c["kind"] == "unusable") {
        let path = match str_of(case, "path_kind") {
            "bad-drive" => "Z:\\no\\such\\drive\\<>|\\shell-errors.log".to_owned(),
            _ => "a".repeat(5000),
        };
        let d = ErrorDetails::new("System.InvalidOperationException", 0x8013_1509u32 as i32, "e");
        let threw =
            std::panic::catch_unwind(|| error_log::append(Path::new(&path), "ctx", &d, error_log::MAX_BYTES)).is_err();
        assert_eq!(threw, case["threw"].as_bool().unwrap(), "case {}", str_of(case, "id"));
        n += 1;
    }
    assert_eq!(n, 2);
}

#[test]
fn shell_error_log_report_matches_every_golden_case() {
    let _lock = CONFIG_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let g = golden::load("shell-error-log");
    let dir = TempRoot::new("g19-golden-error-log-report");
    let root = dir.path().to_string_lossy().into_owned();
    let mut n = 0;
    for case in g.cases.iter().filter(|c| c["kind"] == "report") {
        let id = str_of(case, "id");
        let path = case["configure"].as_str().map(|p| p.replace("$ROOT", &root));
        error_log::configure(path.as_ref().map(std::path::PathBuf::from));
        let err = std::io::Error::other("boom");
        for context in case["reports"].as_array().unwrap() {
            error_log::report(context.as_str().unwrap(), &err);
        }
        let configured = error_log::current_path();
        let exists = path.as_ref().is_some_and(|p| Path::new(p).is_file());
        let folder = path
            .as_ref()
            .is_some_and(|p| Path::new(p).parent().is_some_and(Path::is_dir));
        let lines: Vec<String> = match path.as_ref().filter(|_| exists) {
            Some(p) => std::fs::read_to_string(p)
                .unwrap()
                .split("\r\n")
                .filter(|l| !l.is_empty() && !l.starts_with("    "))
                .map(str::to_owned)
                .collect(),
            None => Vec::new(),
        };
        error_log::configure(None);
        assert_eq!(
            configured.is_some(),
            case["current_path_set"].as_bool().unwrap(),
            "case {id}: current_path_set"
        );
        assert_eq!(exists, case["file_exists"].as_bool().unwrap(), "case {id}: file_exists");
        assert_eq!(
            folder,
            case["folder_created"].as_bool().unwrap(),
            "case {id}: folder_created"
        );
        // Rust errors carry their own type name and HRESULT, so only timestamp shape, context and message are comparable.
        let want: Vec<String> = case["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(lines.len(), want.len(), "case {id}: line count");
        for (got, want) in lines.iter().zip(&want) {
            let (stamp, rest) = got.split_once(' ').unwrap();
            assert!(
                stamp.len() == 20 && stamp.ends_with('Z'),
                "case {id}: timestamp {stamp}"
            );
            let context = |s: &str| s[..=s.find(']').unwrap()].to_owned();
            assert_eq!(
                context(rest),
                context(want.strip_prefix("<utc> ").unwrap()),
                "case {id}: context"
            );
            assert!(got.ends_with(": boom"), "case {id}: message {got}");
        }
        n += 1;
    }
    assert_eq!(n, 3);
}
