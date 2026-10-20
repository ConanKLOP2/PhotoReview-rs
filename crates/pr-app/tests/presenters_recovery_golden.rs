//! Red golden test for `pr_app::presenters::recovery_presentation` / `recovery_filter` (W8-10f, contract
//! `contracts/W8-10f.md`): the bodies are `todo!()`.
//!
//! Reads `tests/golden/recovery-presentation.json` (oracle `recovery-presentation`, baseline `d92d6aae`, 48 cases) and runs
//! every case; mismatches are collected and reported together, each named by the case `name`. The golden carries its inputs.
//! The time-dependent members are not in the golden (D-10f-2) and are tested in `presenters_recovery_presentation.rs`.
//! C#: RecoveryPresenterTests / RecoveryPresenterCoverageTests / RecoveryPresenterMutationGapTests (golden twin, all fns).

use pr_app::presenters::recovery_filter::{RecoveryFilter, RecoveryTone};
use pr_app::presenters::recovery_presentation::{
    RecoveryTexts, RecoveryTimeFormatter, allows_retry, matches, nearest_existing_folder, path_status_tone, rgb,
    verdict_tone,
};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{
    RecoveryCheckResult, RecoveryGroupMemberCheck, RecoveryPathCheck, RecoveryPathStatus, RecoveryVerdict,
};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_l10n::localizer::built_in_english_localizer;
use pr_test_support::golden::load;
use serde_json::{Value, json};

/// The golden has no time text; a fixed one keeps the tests deterministic.
struct FixedTime;

impl RecoveryTimeFormatter for FixedTime {
    fn format_local_general(&self, utc: DateTimeUtc) -> String {
        format!("<{}>", utc.ticks())
    }
}

fn verdict_of(name: &str) -> RecoveryVerdict {
    match name {
        "CanRetry" => RecoveryVerdict::CanRetry,
        "AlreadyDone" => RecoveryVerdict::AlreadyDone,
        "Conflict" => RecoveryVerdict::Conflict,
        "SourceChanged" => RecoveryVerdict::SourceChanged,
        "DestinationChanged" => RecoveryVerdict::DestinationChanged,
        "Lost" => RecoveryVerdict::Lost,
        "Unknown" => RecoveryVerdict::Unknown,
        "RecycleUnverifiable" => RecoveryVerdict::RecycleUnverifiable,
        "NotRecycled" => RecoveryVerdict::NotRecycled,
        "PermanentlyDeleted" => RecoveryVerdict::PermanentlyDeleted,
        "PartiallyPermanentlyDeleted" => RecoveryVerdict::PartiallyPermanentlyDeleted,
        other => panic!("golden: unknown verdict '{other}'"),
    }
}

fn status_of(name: &str) -> RecoveryPathStatus {
    match name {
        "Exists" => RecoveryPathStatus::Exists,
        "Missing" => RecoveryPathStatus::Missing,
        "Changed" => RecoveryPathStatus::Changed,
        "Unreadable" => RecoveryPathStatus::Unreadable,
        other => panic!("golden: unknown status '{other}'"),
    }
}

fn tone_of(name: &str) -> RecoveryTone {
    match name {
        "Neutral" => RecoveryTone::Neutral,
        "Good" => RecoveryTone::Good,
        "Info" => RecoveryTone::Info,
        "Warn" => RecoveryTone::Warn,
        "Bad" => RecoveryTone::Bad,
        other => panic!("golden: unknown tone '{other}'"),
    }
}

fn filter_of(name: &str) -> RecoveryFilter {
    match name {
        "All" => RecoveryFilter::All,
        "CanRetry" => RecoveryFilter::CanRetry,
        "AlreadyDone" => RecoveryFilter::AlreadyDone,
        "Problems" => RecoveryFilter::Problems,
        other => panic!("golden: unknown filter '{other}'"),
    }
}

fn operation_of(name: &str) -> FileOperationType {
    match name {
        "Move" => FileOperationType::Move,
        "Copy" => FileOperationType::Copy,
        "Recycle" => FileOperationType::Recycle,
        other => panic!("golden: unknown operation '{other}'"),
    }
}

fn state_of(name: &str) -> JournalState {
    match name {
        "Prepared" => JournalState::Prepared,
        "Committed" => JournalState::Committed,
        "Failed" => JournalState::Failed,
        "Dismissed" => JournalState::Dismissed,
        other => panic!("golden: unknown state '{other}'"),
    }
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned)
}

fn member_of(v: &Value) -> JournalGroupMember {
    JournalGroupMember {
        source: v["source"].as_str().expect("member source").to_owned(),
        destination: opt_str(&v["destination"]),
        size: v["size"].as_i64().unwrap_or(0),
        last_write_utc: JournalDateTime::default(),
        permanent: v["permanent"].as_bool().unwrap_or(false),
    }
}

fn entry_of(v: &Value) -> JournalEntry {
    JournalEntry {
        id: "0".repeat(32),
        op_type: operation_of(v["type"].as_str().expect("entry type")),
        state: JournalState::Failed,
        source: v["source"].as_str().expect("entry source").to_owned(),
        destination: opt_str(&v["destination"]),
        size: v["size"].as_i64().unwrap_or(0),
        last_write_utc: JournalDateTime::default(),
        timestamp_utc: JournalDateTime::default(),
        error: None,
        error_code: opt_str(&v["error_code"]),
        permanent: None,
        undo: v["undo"].as_bool(),
        group_id: None,
        group_members: v["group_members"]
            .as_array()
            .map(|list| list.iter().map(member_of).collect()),
    }
}

fn path_check_of(v: &Value) -> RecoveryPathCheck {
    RecoveryPathCheck::new(
        v["path"].as_str().expect("check path"),
        status_of(v["status"].as_str().expect("check status")),
        v["size"].as_i64(),
        None,
        v["folder_exists"].as_bool().unwrap_or(true),
    )
}

fn result_of(v: &Value) -> RecoveryCheckResult {
    RecoveryCheckResult {
        entry: entry_of(&v["entry"]),
        source: path_check_of(&v["source"]),
        destination: v.get("destination").filter(|d| !d.is_null()).map(path_check_of),
        verdict: verdict_of(v["verdict"].as_str().expect("verdict")),
        group_members: v["group_members"].as_array().map(|list| {
            list.iter()
                .map(|m| RecoveryGroupMemberCheck {
                    member: member_of(&m["member"]),
                    check: result_of(&m["check"]),
                })
                .collect()
        }),
    }
}

fn optional_result(v: &Value) -> Option<RecoveryCheckResult> {
    if v.is_null() { None } else { Some(result_of(v)) }
}

fn same<T: PartialEq + std::fmt::Debug>(got: T, want: T) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("got {got:?}, want {want:?}"))
    }
}

/// Runs an enumerating function (`*_all`, `*_matrix`) over the `in` column of the golden rows and compares the `out` column.
fn each_row(output: &Value, mut check: impl FnMut(&str, &Value) -> Result<(), String>) -> Result<(), String> {
    let rows = output.as_array().ok_or("output is not a row array")?;
    let mut problems = Vec::new();
    for row in rows {
        let input = row["in"].as_str().ok_or("row without `in`")?;
        if let Err(why) = check(input, &row["out"]) {
            problems.push(format!("[{input}] {why}"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

fn run_case(
    texts: &RecoveryTexts<'_>,
    name: &str,
    function: &str,
    input: &Value,
    output: &Value,
) -> Result<(), String> {
    match function {
        "rgb_all" => each_row(output, |tone, out| {
            let (r, g, b) = rgb(tone_of(tone));
            same(json!([r, g, b]), out.clone())
        }),
        "verdict_tone_all" => each_row(output, |v, out| {
            same(
                format!("{:?}", verdict_tone(verdict_of(v))),
                out.as_str().unwrap().to_owned(),
            )
        }),
        "verdict_text_all" => each_row(output, |v, out| {
            same(texts.verdict_text(verdict_of(v)), out.as_str().unwrap().to_owned())
        }),
        "explain_text_all" => each_row(output, |v, out| {
            same(texts.explain_text(verdict_of(v)), out.as_str().unwrap().to_owned())
        }),
        "action_text_all" => each_row(output, |v, out| {
            same(texts.action_text(verdict_of(v)), out.as_str().unwrap().to_owned())
        }),
        "path_status_all" => each_row(output, |s, out| {
            let status = status_of(s);
            let got = format!("{} | {:?}", texts.path_status_text(status), path_status_tone(status));
            same(got, out.as_str().unwrap().to_owned())
        }),
        "matches_matrix" => each_row(output, |key, out| {
            let (filter, verdict) = key.split_once('/').expect("filter/verdict");
            let verdict = if verdict == "null" {
                None
            } else {
                Some(verdict_of(verdict))
            };
            same(matches(filter_of(filter), verdict), out.as_bool().unwrap())
        }),
        "operation_text_all" => each_row(output, |op, out| {
            same(texts.operation_text(operation_of(op)), out.as_str().unwrap().to_owned())
        }),
        "state_text_all" => each_row(output, |s, out| {
            same(texts.state_text(state_of(s)), out.as_str().unwrap().to_owned())
        }),
        "format_size" => same(
            texts.format_size(input["bytes"].as_i64().expect("bytes")),
            output.as_str().unwrap().to_owned(),
        ),
        "allows_retry" => same(
            allows_retry(optional_result(&input["check"]).as_ref()),
            output.as_bool().unwrap(),
        ),
        "group_status_text" => {
            let destination = if input["destination"].is_null() {
                None
            } else {
                Some(path_check_of(&input["destination"]))
            };
            same(
                texts.group_status_text(
                    verdict_of(input["verdict"].as_str().unwrap()),
                    &member_of(&input["member"]),
                    &path_check_of(&input["source"]),
                    destination.as_ref(),
                ),
                output.as_str().unwrap().to_owned(),
            )
        }
        "retry_confirm_text" => same(
            texts.retry_confirm_text(
                &entry_of(&input["entry"]),
                optional_result(&input["check"]).as_ref(),
                input["allow_permanent_delete"].as_bool().unwrap(),
            ),
            output.as_str().unwrap().to_owned(),
        ),
        "nearest_existing_folder" => {
            let existing: Vec<String> = input["existing"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_str().unwrap().to_owned())
                .collect();
            let got = nearest_existing_folder(input["path"].as_str().unwrap(), &|p| existing.iter().any(|e| e == p));
            same(got, opt_str(output))
        }
        other => panic!("case {name}: unknown golden function '{other}'"),
    }
}

#[test]
fn golden_recovery_presentation() {
    // new: golden `recovery-presentation` (48 cases), covering the golden-marked tests of RecoveryPresenterTests,
    // RecoveryPresenterCoverageTests and RecoveryPresenterMutationGapTests listed in the contract.
    let english = built_in_english_localizer();
    let time = FixedTime;
    let texts = RecoveryTexts {
        loc: &english,
        time: &time,
        group_separator: ",",
    };
    let golden = load("recovery-presentation");
    assert_eq!(golden.cases.len(), 48);
    let mut failures = Vec::new();
    for case in &golden.cases {
        let name = case["name"].as_str().expect("case name");
        let function = case["fn"].as_str().expect("case fn");
        if let Err(why) = run_case(&texts, name, function, &case["input"], &case["output"]) {
            failures.push(format!("{name}: {why}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} golden failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
