//! pr_core::text and pr_core::time against golden files generated from .NET (tools/oracle.ps1).

use std::cmp::Ordering;

use pr_core::text::{cmp_ignore_case, eq_ignore_case, ignore_case_key};
use pr_core::time::{DateTimeUtc, SourceKind};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};

fn sign(o: Ordering) -> i64 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

// C#: string.Equals / string.Compare with StringComparison.OrdinalIgnoreCase.
#[test]
fn ordinal_ignore_case_matches_dotnet() {
    let g = golden::load("ordinal-ignore-case");
    for case in &g.cases {
        let (a, b) = (str_of(case, "a"), str_of(case, "b"));
        assert_eq!(eq_ignore_case(a, b), bool_of(case, "equals"), "equals {a:?} {b:?}");
        assert_eq!(
            sign(cmp_ignore_case(a, b)),
            i64_of(case, "compare"),
            "compare {a:?} {b:?}"
        );
        assert_eq!(
            ignore_case_key(a) == ignore_case_key(b),
            bool_of(case, "equals"),
            "key {a:?} {b:?}"
        );
    }
}

// C#: JsonSerializer.Serialize(DateTime[Kind=Utc]).
#[test]
fn datetime_json_write_matches_system_text_json() {
    let g = golden::load("datetime-json");
    let writes: Vec<_> = g.cases.iter().filter(|c| str_of(c, "kind") == "write").collect();
    assert!(!writes.is_empty());
    for case in writes {
        let value = DateTimeUtc::from_ticks(i64_of(case, "ticks")).expect("golden ticks in range");
        assert_eq!(value.to_json(), str_of(case, "json"));
    }
}

// C#: JsonSerializer.Deserialize<DateTime>, offsets converted to UTC.
#[test]
fn datetime_json_parse_matches_system_text_json() {
    let g = golden::load("datetime-json");
    let parses: Vec<_> = g.cases.iter().filter(|c| str_of(c, "kind") == "parse").collect();
    assert!(!parses.is_empty());
    for case in parses {
        let text = str_of(case, "json");
        let parsed = DateTimeUtc::parse_json(text);
        if !bool_of(case, "ok") {
            assert_eq!(parsed, None, "{text:?} must be rejected");
            continue;
        }
        let (value, kind) = parsed.unwrap_or_else(|| panic!("{text:?} must parse"));
        assert_eq!(value.ticks(), i64_of(case, "ticks"), "{text:?}");
        let expected_kind = match str_of(case, "source_kind") {
            "Utc" => SourceKind::Utc,
            "Local" => SourceKind::Offset,
            "Unspecified" => SourceKind::Unspecified,
            other => panic!("unknown kind {other}"),
        };
        assert_eq!(kind, expected_kind, "{text:?}");
    }
}
