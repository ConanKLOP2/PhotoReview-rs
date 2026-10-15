//! Golden runner of the W3-05 `UndoService` (`tests/golden/undo-scenarios.json`): 100 scripted scenarios recorded from the real
//! C# `UndoService` + `OperationJournal` by `tools/oracle-cs/Commands/W3-05.cs` (command `undo-scenarios`, baseline d92d6aae).
//!
//! Each case builds the same fakes as the oracle (`W305Fs` = [`undo_support::ScriptedFs`] over the in-memory disk, `W305Bin` =
//! [`undo_support::ScriptedBin`], shared gate = [`undo_support::TestGate`]), runs the steps, and records, per step, the undo
//! result, the bin calls, the move-override calls and the undo state; then the final disk listing and the whole journal text with
//! every 32-hex id replaced by `<guid1>`, `<guid2>`, ... in order of first appearance. Everything is compared with the recorded
//! `steps_out` and `final` (JSON value equality; the journal text byte for byte, INV-6 line format, D-03).
//!
//! Red until the W3-05 bodies land: the first step that reaches `UndoService` panics with `todo!()`.

mod undo_support;

use std::sync::{Arc, Mutex};

use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::types::{CaptureGroupActionResult, CaptureGroupMemberResult, FileActionResult, UndoResult};
use pr_core::file_actions::undo::{MoveOverride, UndoService};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::text::ignore_case_key;
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeClock, golden};
use serde_json::{Value, json};
use undo_support::*;

/// Number of scenarios in the golden file (contract: "100 scripted scenarios").
const CASE_COUNT: usize = 100;

// ---------------------------------------------------------------------------------------------------------------
// JSON -> model
// ---------------------------------------------------------------------------------------------------------------

/// `ticks` (a .NET tick count) as a [`DateTimeUtc`].
fn stamp(ticks: &Value) -> DateTimeUtc {
    let ticks = ticks.as_i64().expect("ticks is an integer");
    DateTimeUtc::from_ticks(ticks).expect("ticks in range")
}

/// `null` or a string.
fn opt_text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn text(value: &Value) -> String {
    value.as_str().expect("string field").to_owned()
}

fn operation(value: &Value) -> FileOperationType {
    match value.as_str() {
        Some("Move") => FileOperationType::Move,
        Some("Copy") => FileOperationType::Copy,
        Some("Recycle") => FileOperationType::Recycle,
        other => panic!("unknown operation {other:?}"),
    }
}

fn journal_state(value: &Value) -> JournalState {
    match value.as_str() {
        Some("Prepared") => JournalState::Prepared,
        Some("Committed") => JournalState::Committed,
        Some("Failed") => JournalState::Failed,
        Some("Dismissed") => JournalState::Dismissed,
        other => panic!("unknown journal state {other:?}"),
    }
}

fn bin_mode(name: &str) -> BinMode {
    match name {
        "restore" => BinMode::Restore,
        "fail" => BinMode::Fail,
        "restore_report_false" => BinMode::RestoreReportFalse,
        "report_true_only" => BinMode::ReportTrueOnly,
        "restore_other_stamp" => BinMode::RestoreOtherStamp,
        "restore_wrong_size" => BinMode::RestoreWrongSize,
        other => panic!("unknown bin mode {other}"),
    }
}

/// `{source, destination, size, ticks, permanent}` (C# `W305Member`).
fn group_member(value: &Value) -> JournalGroupMember {
    JournalGroupMember {
        source: text(&value["source"]),
        destination: opt_text(&value["destination"]),
        size: value["size"].as_i64().expect("member size"),
        last_write_utc: JournalDateTime::utc(stamp(&value["ticks"])),
        permanent: value["permanent"].as_bool().unwrap_or(false),
    }
}

/// C# `W305Entry`: the journal line of a group result (or `null`).
fn journal_entry(value: &Value) -> Option<Box<JournalEntry>> {
    if value.is_null() {
        return None;
    }
    let members = match &value["group_members"] {
        Value::Array(items) => Some(items.iter().map(group_member).collect()),
        _ => None,
    };
    Some(Box::new(JournalEntry {
        id: text(&value["id"]),
        op_type: operation(&value["type"]),
        state: journal_state(&value["state"]),
        source: text(&value["source"]),
        destination: opt_text(&value["destination"]),
        size: value["size"].as_i64().expect("entry size"),
        last_write_utc: JournalDateTime::utc(stamp(&value["last_write_ticks"])),
        timestamp_utc: JournalDateTime::utc(stamp(&value["timestamp_ticks"])),
        error: opt_text(&value["error"]),
        error_code: opt_text(&value["error_code"]),
        permanent: value["permanent"].as_bool(),
        undo: value["undo"].as_bool(),
        group_id: opt_text(&value["group_id"]),
        group_members: members,
    }))
}

/// C# `W305Single`: a `register` step's `FileActionResult`.
fn parse_single(value: &Value) -> FileActionResult {
    let mut result = FileActionResult::new(
        value["succeeded"].as_bool().expect("succeeded"),
        operation(&value["operation"]),
        text(&value["source"]),
        opt_text(&value["destination_path"]),
        value["size"].as_i64().expect("size"),
        stamp(&value["last_write_ticks"]),
        opt_text(&value["error"]),
    );
    result.rejected = value["rejected"].as_bool().unwrap_or(false);
    result.permanently_deleted = value["permanently_deleted"].as_bool().unwrap_or(false);
    result
}

/// C# `W305Group`: a `register_group` step's `CaptureGroupActionResult`.
fn parse_group(value: &Value) -> CaptureGroupActionResult {
    let members = value["members"]
        .as_array()
        .expect("members")
        .iter()
        .map(|item| {
            let mut result = CaptureGroupMemberResult::new(
                group_member(&item["member"]),
                item["completed"].as_bool().expect("completed"),
                item["conflict"].as_bool().unwrap_or(false),
            );
            result.error = opt_text(&item["error"]);
            result.source_exists = item["source_exists"].as_bool().unwrap_or(false);
            result
        })
        .collect();
    let mut result = CaptureGroupActionResult::new(
        value["succeeded"].as_bool().expect("succeeded"),
        value["rejected"].as_bool().unwrap_or(false),
        operation(&value["operation"]),
        text(&value["group_id"]),
        journal_entry(&value["entry"]),
        members,
        opt_text(&value["error"]),
    );
    result.permanently_deleted = value["permanently_deleted"].as_bool().unwrap_or(false);
    result
}

/// C# `W305Result`: the JSON of an undo result, as the oracle records it.
fn result_json(result: &UndoResult) -> Value {
    json!({
        "succeeded": result.succeeded,
        "operation": result.operation.map(|op| match op {
            FileOperationType::Move => "Move",
            FileOperationType::Copy => "Copy",
            FileOperationType::Recycle => "Recycle",
        }),
        "source": result.source.clone(),
        "destination": result.destination.clone(),
        "error_message": result.error_message.clone(),
        "rejected": result.rejected,
        "restored_paths": result.restored_paths.clone(),
    })
}

/// One scripted action (C# `Run(JsonArray)`): `{add, size, ticks}`, `{delete}`, `{move, to}`, `{append_line}`.
fn parse_action(value: &Value) -> Action {
    if let Some(path) = value.get("add") {
        return Action::Add {
            path: text(path),
            size: usize::try_from(value["size"].as_u64().expect("add size")).expect("size fits"),
            stamp: stamp(&value["ticks"]),
        };
    }
    if let Some(path) = value.get("delete") {
        return Action::Delete(text(path));
    }
    if let Some(from) = value.get("move") {
        return Action::MoveTo {
            from: text(from),
            to: text(&value["to"]),
        };
    }
    if let Some(line) = value.get("append_line") {
        return Action::AppendLine(text(line));
    }
    panic!("unknown action {value}");
}

fn parse_actions(value: &Value) -> Vec<Action> {
    value
        .as_array()
        .expect("actions array")
        .iter()
        .map(parse_action)
        .collect()
}

/// The `bin` spec of a case or a `set_bin` step: replaces the modes of the fake bin (C# `SetBin`).
fn apply_bin_spec(bin: &ScriptedBin, spec: &Value) {
    bin.clear_modes();
    bin.set_default(bin_mode(spec["default"].as_str().unwrap_or("restore")));
    if let Some(by_path) = spec["by_path"].as_object() {
        for (path, mode) in by_path {
            bin.set_mode_for(path, bin_mode(&text(mode)));
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------------------------------------------

/// Replaces every 32-lowercase-hex run by `<guid1>`, `<guid2>`, ... in order of first appearance (C#
/// `W305NormalizeGuids`, `Regex "[0-9a-f]{32}"`: leftmost, non-overlapping).
fn normalize_guids(text: &str) -> String {
    let bytes = text.as_bytes();
    let is_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let mut seen: Vec<&str> = Vec::new();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if index + 32 <= bytes.len() && bytes[index..index + 32].iter().all(|&b| is_hex(b)) {
            let guid = &text[index..index + 32];
            let number = match seen.iter().position(|known| *known == guid) {
                Some(position) => position + 1,
                None => {
                    seen.push(guid);
                    seen.len()
                }
            };
            out.push_str(&format!("<guid{number}>"));
            index += 32;
        } else {
            let ch = text[index..].chars().next().expect("index inside the text");
            out.push(ch);
            index += ch.len_utf8();
        }
    }
    out
}

/// C# `Listing()` of the final state: every file except the journal, sorted OrdinalIgnoreCase.
fn final_files(f: &Fixture) -> Value {
    let mut files: Vec<(String, u64, i64)> = f
        .disk
        .all_files()
        .into_iter()
        .filter(|path| ignore_case_key(&path.to_string_lossy()) != ignore_case_key(JOURNAL))
        .map(|path| {
            let stat = f.disk.peek_stat(&path).expect("listed file has a stat");
            (
                path.to_string_lossy().into_owned(),
                stat.length,
                stat.last_write_utc.ticks(),
            )
        })
        .collect();
    files.sort_by_key(|(path, _, _)| path.to_ascii_lowercase());
    Value::Array(
        files
            .into_iter()
            .map(|(path, size, ticks)| json!({ "path": path, "size": size, "ticks": ticks }))
            .collect(),
    )
}

// ---------------------------------------------------------------------------------------------------------------
// The runner
// ---------------------------------------------------------------------------------------------------------------

/// Builds the case's fixture and runs its steps. Returns `(steps_out, final)` as the oracle records them.
fn run_case(case: &Value) -> (Value, Value) {
    let f = Fixture::at(stamp(&case["clock_ticks"]));
    let overrides: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));

    for file in case["files"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let size = usize::try_from(file["size"].as_u64().expect("file size")).expect("size fits");
        f.add(&text(&file["path"]), size, stamp(&file["ticks"]));
    }
    for dir in case["dirs"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        f.disk.add_dir(text(dir));
    }
    for line in case["journal_lines"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        run_action(&f.disk, &Action::AppendLine(text(line)));
    }
    apply_bin_spec(&f.bin, &case["bin"]);

    // The undo clock is the journal clock unless the case gives another one (C# `undoClock`).
    let undo_clock: Arc<dyn pr_core::abstractions::Clock> = match case["undo_clock_ticks"].as_i64() {
        Some(ticks) => Arc::new(FakeClock::at(stamp(&json!(ticks)))),
        None => f.clock.clone(),
    };
    let gate = (case["shared_gate"].as_bool() == Some(true)).then(TestGate::new);
    let mut undo: UndoService = f.undo().with_clock(undo_clock);
    if let Some(gate) = &gate {
        undo = undo.with_gate(gate.clone());
    }
    if case["override"].as_bool() == Some(true) {
        let over: MoveOverride = f.recording_override(overrides.clone());
        undo = undo.with_move_override(over);
    }

    let mut steps_out = Vec::new();
    for step in case["steps"].as_array().expect("steps array") {
        // C# `ResetHooks`, then the step's hooks.
        f.fs.reset_step();
        f.bin.clear_calls();
        overrides.lock().unwrap_or_else(|e| e.into_inner()).clear();
        if let Some(hooks) = step.get("hooks") {
            if let Some(moves) = hooks["move_fail"].as_object() {
                for (path, message) in moves {
                    f.fs.fail_move_of(path, &text(message));
                }
            }
            if let Some(leaves) = hooks["move_leaves_source"].as_bool() {
                f.fs.set_move_leaves_source(leaves);
            }
            for hook in hooks["after_move"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
                let nth = usize::try_from(hook["nth"].as_u64().expect("nth")).expect("nth fits");
                f.fs.after_move(nth, parse_actions(&hook["actions"]));
            }
            for hook in hooks["on_append"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
                let nth = usize::try_from(hook["nth"].as_u64().expect("nth")).expect("nth fits");
                f.fs.on_append(nth, parse_actions(&hook["actions"]));
            }
            if let Some(from) = hooks["append_fail_from"].as_u64() {
                let from = usize::try_from(from).expect("from fits");
                let message =
                    opt_text(&hooks["append_fail_message"]).unwrap_or_else(|| "simulated journal failure".to_owned());
                f.fs.fail_appends_from(from, &message);
            }
        }

        let mut step_result = Value::Null;
        match text(&step["op"]).as_str() {
            "register" => undo.register(&parse_single(&step["result"])),
            "register_group" => undo.register_group(&parse_group(&step["result"])),
            "undo_last" => step_result = result_json(&undo.undo_last()),
            "undo_move" => step_result = result_json(&undo.undo_move()),
            "push_history" => undo
                .move_history_mut()
                .push((text(&step["source"]), text(&step["destination"]))),
            "gate_begin" => step_result = json!({ "began": undo.try_begin() }),
            "gate_end" => undo.end(),
            "fs" => {
                for action in parse_actions(&step["actions"]) {
                    run_action(&f.disk, &action);
                }
            }
            "set_bin" => apply_bin_spec(&f.bin, &step["bin"]),
            other => panic!("unknown op {other}"),
        }

        let overrides_now: Vec<Value> = overrides
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(from, to)| json!([from, to]))
            .collect();
        steps_out.push(json!({
            "result": step_result,
            "bin_calls": f.bin.calls(),
            "override_calls": overrides_now,
            "state": {
                // Oldest first: the C# Stack is enumerated top first and the oracle reverses it.
                "move_history": undo.move_history().iter().map(|(source, destination)| json!([source, destination])).collect::<Vec<_>>(),
                "has_last_action": undo.has_last_undo_action(),
                "fingerprints": undo.move_fingerprint_count(),
                "busy": undo.is_busy(),
            },
        }));
    }

    let final_state = json!({
        "files": final_files(&f),
        "journal": normalize_guids(&f.journal_text()),
    });
    (Value::Array(steps_out), final_state)
}

#[test]
fn undo_scenarios_match_the_csharp_oracle() {
    let golden = golden::load("undo-scenarios");
    assert_eq!(
        golden.cases.len(),
        CASE_COUNT,
        "the golden file holds {CASE_COUNT} scenarios"
    );
    for case in &golden.cases {
        let name = text(&case["name"]);
        let (steps_out, final_state) = run_case(case);
        let expected_steps = &case["steps_out"];
        if let (Value::Array(actual), Value::Array(expected)) = (&steps_out, expected_steps) {
            for (index, (got, want)) in actual.iter().zip(expected.iter()).enumerate() {
                assert_eq!(got, want, "case `{name}`: step {index} differs from the C# oracle");
            }
            assert_eq!(actual.len(), expected.len(), "case `{name}`: number of steps");
        } else {
            panic!("case `{name}`: steps_out is not an array");
        }
        assert_eq!(
            final_state["files"], case["final"]["files"],
            "case `{name}`: final files"
        );
        assert_eq!(
            final_state["journal"].as_str().expect("journal text"),
            case["final"]["journal"].as_str().expect("expected journal text"),
            "case `{name}`: journal text (byte for byte)"
        );
    }
}
