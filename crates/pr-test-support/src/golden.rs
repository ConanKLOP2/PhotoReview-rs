//! Reads `tests/golden/<name>.json` written by `tools/oracle.ps1` (docs/MULTI-AGENT-PLAN.md §4). Porters never edit
//! golden files; a test that disagrees with one has found a behaviour difference from C#.

use std::path::PathBuf;

use serde_json::Value;

/// A parsed golden file.
#[derive(Debug, Clone)]
pub struct Golden {
    /// `oracle-cs <command>`.
    pub generator: String,
    /// The C# commit the expected values come from.
    pub csharp_commit: String,
    pub cases: Vec<Value>,
}

/// Repository root (the workspace directory holding `tests/golden`).
pub fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .find(|dir| dir.join("tests").join("golden").is_dir())
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("no tests/golden above {}", manifest.display()))
}

/// Loads `tests/golden/<name>.json`.
///
/// # Panics
/// When the file is missing or malformed: a broken golden file must fail the test loudly.
pub fn load(name: &str) -> Golden {
    let path = repo_root().join("tests").join("golden").join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let field = |key: &str| {
        doc[key]
            .as_str()
            .unwrap_or_else(|| panic!("{}: missing '{key}'", path.display()))
            .to_owned()
    };
    let cases = doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone();
    assert!(!cases.is_empty(), "{}: no cases", path.display());
    Golden {
        generator: field("generator"),
        csharp_commit: field("csharp_commit"),
        cases,
    }
}

/// `case[key]` as a string; panics with the case printed when absent.
pub fn str_of<'a>(case: &'a Value, key: &str) -> &'a str {
    case[key]
        .as_str()
        .unwrap_or_else(|| panic!("case has no string '{key}': {case}"))
}

/// `case[key]` as an i64; panics with the case printed when absent.
pub fn i64_of(case: &Value, key: &str) -> i64 {
    case[key]
        .as_i64()
        .unwrap_or_else(|| panic!("case has no integer '{key}': {case}"))
}

/// `case[key]` as a bool; panics with the case printed when absent.
pub fn bool_of(case: &Value, key: &str) -> bool {
    case[key]
        .as_bool()
        .unwrap_or_else(|| panic!("case has no bool '{key}': {case}"))
}
