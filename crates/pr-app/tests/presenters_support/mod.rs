//! Shared helpers of the W8-10a tests (`action_profiles`, `action_profiles_io`, `action_list_json`): golden loading,
//! the `en` / `vi` localizers, JSON-to-`ReviewAction` conversion and a self-deleting temp folder.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use pr_core::model::enums::FileOperationType;
use pr_core::settings::shortcuts::ReviewAction;
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::{built_in_english, built_in_english_localizer, Localizer};
use pr_test_support::golden::{load, repo_root};
use serde_json::Value;

/// The English localizer (built-in catalog).
pub fn english() -> Arc<Localizer> {
    built_in_english_localizer()
}

/// The Vietnamese localizer: `assets/languages/vi.json` as an overlay on the built-in English catalog.
pub fn vietnamese() -> Arc<Localizer> {
    let path = repo_root().join("assets").join("languages").join("vi.json");
    let text = std::fs::read_to_string(&path).expect("assets/languages/vi.json");
    let mut warnings = Vec::new();
    let vi = LanguageCatalog::try_parse(&text, &path.to_string_lossy(), &mut warnings).expect("valid catalog");
    assert!(warnings.is_empty(), "{warnings:?}");
    Arc::new(Localizer::create(built_in_english(), &[vi], &[]))
}

/// The localizer a golden case names in its `lang` field (`en` or `vi`).
pub fn localizer(lang: &str) -> Arc<Localizer> {
    match lang {
        "en" => english(),
        "vi" => vietnamese(),
        other => panic!("golden: unknown lang '{other}'"),
    }
}

pub fn operation_of(name: &str) -> FileOperationType {
    match name {
        "Move" => FileOperationType::Move,
        "Copy" => FileOperationType::Copy,
        "Recycle" => FileOperationType::Recycle,
        other => panic!("golden: unknown operation '{other}'"),
    }
}

pub fn operation_name(op: FileOperationType) -> &'static str {
    match op {
        FileOperationType::Move => "Move",
        FileOperationType::Copy => "Copy",
        FileOperationType::Recycle => "Recycle",
    }
}

/// One `{Name, Shortcut, Operation, Destination, Confirm}` golden object as a `ReviewAction`.
pub fn action_of(v: &Value) -> ReviewAction {
    ReviewAction {
        name: v["Name"].as_str().expect("Name").to_owned(),
        shortcut: v["Shortcut"].as_str().expect("Shortcut").to_owned(),
        operation: operation_of(v["Operation"].as_str().expect("Operation")),
        destination: v["Destination"].as_str().expect("Destination").to_owned(),
        confirm: v["Confirm"].as_bool().expect("Confirm"),
    }
}

/// A golden array of action objects.
pub fn actions_of(v: &Value) -> Vec<ReviewAction> {
    v.as_array().expect("actions array").iter().map(action_of).collect()
}

/// A hand-built action.
pub fn act(name: &str, shortcut: &str, operation: FileOperationType, destination: &str, confirm: bool) -> ReviewAction {
    ReviewAction {
        name: name.to_owned(),
        shortcut: shortcut.to_owned(),
        operation,
        destination: destination.to_owned(),
        confirm,
    }
}

/// A Move action to `Out` (the C# test helper `Action(name, shortcut)`).
pub fn mv(name: &str, shortcut: &str) -> ReviewAction {
    act(name, shortcut, FileOperationType::Move, "Out", false)
}

/// Comparable form of an action (name, shortcut, operation name, destination, confirm).
pub fn row(a: &ReviewAction) -> (String, String, &'static str, String, bool) {
    (
        a.name.clone(),
        a.shortcut.clone(),
        operation_name(a.operation),
        a.destination.clone(),
        a.confirm,
    )
}

pub fn rows(list: &[ReviewAction]) -> Vec<(String, String, &'static str, String, bool)> {
    list.iter().map(row).collect()
}

/// Golden cases of `tests/golden/<name>.json` joined by case `name` with the inputs of
/// `tests/golden-inputs/<name>.json`: `(input, expected)`.
pub fn golden_pairs(name: &str) -> Vec<(Value, Value)> {
    let golden = load(name);
    let path = repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let inputs = doc["cases"].as_array().expect("golden-inputs cases").clone();
    golden
        .cases
        .into_iter()
        .map(|expected| {
            let case_name = expected["name"].as_str().expect("case name").to_owned();
            let input = inputs
                .iter()
                .find(|i| i["name"].as_str() == Some(case_name.as_str()))
                .unwrap_or_else(|| panic!("no golden input named '{case_name}' in {name}"))
                .clone();
            (input, expected)
        })
        .collect()
}

/// The oracle ran on Windows, so its indented JSON has `\r\n`; the Rust writer uses `\n` (contract W8-10a rule 16).
pub fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// A fresh temp folder that deletes itself (and only itself) on drop.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("pr-w8-10a-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
