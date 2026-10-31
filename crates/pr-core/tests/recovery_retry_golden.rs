//! Golden replay of `tests/golden/recovery-retry.json` (106 cases, C# oracle `recovery-retry`, baseline d92d6aae; see
//! `tools/oracle-cs/Commands/W3-06.cs`). Each case runs `RecoveryRetryService::retry_move_or_copy` over the scripted
//! disk, bin and gate of `recovery_retry_support`, then compares every observable with the golden row: the result (or
//! the thrown exception), the journal text byte for byte, the files and bin items after, the logged calls (`ops`), the
//! gate state and the live marker. Porters never edit golden files: a disagreement is a behaviour difference from C#.
//!
//! Option A of the stat seam (contract open question 3): the six `needs_stat_seam` cases run; nothing is skipped.

mod recovery_retry_support;

use std::io;
use std::sync::Arc;

use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_retry::RecoveryRetryResult;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_test_support::golden;
use recovery_retry_support::*;
use serde_json::{Map, Value, json};

/// The golden file name (`tests/golden/recovery-retry.json`).
const GOLDEN: &str = "recovery-retry";
/// The number of cases the golden file holds (contract "Done when": all 106 replayed).
const CASE_COUNT: usize = 106;

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_else(|| panic!("no string '{key}' in {v}"))
}

fn ticks_field(v: &Value, key: &str) -> i64 {
    v[key].as_i64().unwrap_or_else(|| panic!("no integer '{key}' in {v}"))
}

fn optional_str(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::to_owned)
}

fn optional_bool(v: &Value, key: &str) -> Option<bool> {
    v[key].as_bool()
}

fn op_type_of(name: &str) -> FileOperationType {
    match name {
        "Move" => FileOperationType::Move,
        "Copy" => FileOperationType::Copy,
        "Recycle" => FileOperationType::Recycle,
        other => panic!("unknown operation type {other}"),
    }
}

fn state_of(name: &str) -> JournalState {
    match name {
        "Prepared" => JournalState::Prepared,
        "Committed" => JournalState::Committed,
        "Failed" => JournalState::Failed,
        "Dismissed" => JournalState::Dismissed,
        other => panic!("unknown journal state {other}"),
    }
}

fn state_name(state: JournalState) -> &'static str {
    match state {
        JournalState::Prepared => "Prepared",
        JournalState::Committed => "Committed",
        JournalState::Failed => "Failed",
        JournalState::Dismissed => "Dismissed",
    }
}

fn type_name(op: FileOperationType) -> &'static str {
    match op {
        FileOperationType::Move => "Move",
        FileOperationType::Copy => "Copy",
        FileOperationType::Recycle => "Recycle",
    }
}

/// One golden `members[]` / seed member object as a journal group member.
fn member_of(m: &Value) -> JournalGroupMember {
    JournalGroupMember {
        source: str_field(m, "source").to_owned(),
        destination: optional_str(m, "destination"),
        size: ticks_field(m, "size"),
        last_write_utc: JournalDateTime::utc(ts(ticks_field(m, "last_write_ticks"))),
        permanent: m["permanent"].as_bool().unwrap_or(false),
    }
}

/// One golden seed object (`W306Entry`): the journal line the test writes before the retry.
fn seed_entry(v: &Value) -> JournalEntry {
    let members = v["members"]
        .as_array()
        .map(|list| list.iter().map(member_of).collect::<Vec<_>>());
    JournalEntry {
        error_code: optional_str(v, "error_code"),
        permanent: optional_bool(v, "permanent"),
        undo: optional_bool(v, "undo"),
        group_id: optional_str(v, "group_id"),
        group_members: members,
        ..entry(
            str_field(v, "id"),
            op_type_of(str_field(v, "type")),
            state_of(str_field(v, "state")),
            str_field(v, "source"),
            v["destination"].as_str(),
            ticks_field(v, "size"),
            ts(ticks_field(v, "last_write_ticks")),
            ts(ticks_field(v, "timestamp_ticks")),
            v["error"].as_str(),
        )
    }
}

/// The `DateTimeUtc` ticks of a journal stamp (golden `last_write_ticks` / `timestamp_ticks`).
fn stamp_ticks(value: DateTimeUtc) -> i64 {
    value.ticks()
}

/// A journal entry as the golden `result.entry` shows it.
fn entry_json(e: &JournalEntry) -> Value {
    json!({
        "id": e.id,
        "type": type_name(e.op_type),
        "state": state_name(e.state),
        "source": e.source,
        "destination": e.destination,
        "size": e.size,
        "last_write_ticks": stamp_ticks(e.last_write_utc.value),
        "timestamp_ticks": stamp_ticks(e.timestamp_utc.value),
        "error": e.error,
        "error_code": e.error_code,
        "permanent": e.permanent,
        "undo": e.undo,
        "group_id": e.group_id,
        "members": e.group_members.as_ref().map(|members| {
            members
                .iter()
                .map(|m| {
                    json!({
                        "source": m.source,
                        "destination": m.destination,
                        "size": m.size,
                        "last_write_ticks": stamp_ticks(m.last_write_utc.value),
                        "permanent": m.permanent,
                    })
                })
                .collect::<Vec<_>>()
        }),
    })
}

/// The C# exception type name a hook's `exception` maps to (`IOException` when absent).
fn error_kind_of(exception: Option<&str>) -> io::ErrorKind {
    match exception.unwrap_or("IOException") {
        "IOException" => io::ErrorKind::Other,
        "UnauthorizedAccessException" => io::ErrorKind::PermissionDenied,
        "ArgumentException" => io::ErrorKind::InvalidInput,
        "NotSupportedException" => io::ErrorKind::Unsupported,
        other => panic!("unknown hook exception {other}"),
    }
}

/// One `hooks[].do[]` action of the golden DSL.
fn op_of(action: &Value) -> Op {
    if let Some(path) = action.get("delete") {
        return Op::Delete(path.as_str().expect("delete path").to_owned());
    }
    if let Some(write) = action.get("write") {
        return Op::Write {
            path: str_field(write, "path").to_owned(),
            len: ticks_field(write, "len") as usize,
            ticks: ticks_field(write, "ticks"),
        };
    }
    if action.get("cancel").is_some() {
        return Op::Cancel;
    }
    if let Some(message) = action.get("throw") {
        return Op::Throw {
            kind: error_kind_of(action["exception"].as_str()),
            message: message.as_str().expect("throw message").to_owned(),
        };
    }
    if action.get("leave_source").is_some() {
        return Op::LeaveSource;
    }
    if let Some(bytes) = action.get("partial") {
        return Op::Partial {
            bytes: bytes.as_i64().expect("partial bytes") as usize,
            message: str_field(action, "message").to_owned(),
        };
    }
    panic!("unknown hook action {action}");
}

/// The static name of a golden hook `on` (the hook rules keep `&'static str`).
fn hook_name(on: &str) -> &'static str {
    match on {
        "stat" => "stat",
        "move" => "move",
        "copy" => "copy",
        "append" => "append",
        "recycle" => "recycle",
        "restore" => "restore",
        "delete" => "delete",
        other => panic!("unknown hook kind {other}"),
    }
}

/// The bin of a case's `bin` object.
fn bin_config_of(bin: &Value) -> BinConfig {
    let restore = match str_field(bin, "restore") {
        "ok" => RestoreMode::Ok,
        "refuse-leave-file" => RestoreMode::RefuseLeaveFile,
        "ok-no-file" => RestoreMode::OkNoFile,
        other => panic!("unknown restore mode {other}"),
    };
    BinConfig {
        no_bin: bin["no_bin"]
            .as_array()
            .map(|list| list.iter().map(|p| p.as_str().expect("path").to_owned()).collect())
            .unwrap_or_default(),
        fits: bin["fits"].as_bool().unwrap_or(true),
        capacity: None,
        restore,
        recycle_noop: bin["recycle_noop"].as_bool().unwrap_or(false),
    }
}

/// The world of one case before the retry: disk, bin, gate and journal seeds (the seed lines are not logged and not
/// counted by the hook rules).
struct Setup {
    rig: Rig,
    snapshot: JournalEntry,
    bin: Option<Arc<ScriptedBin>>,
    gate: Option<Arc<TestGate>>,
}

fn setup(case: &Value) -> Setup {
    let rig = Rig::with_clocks(
        ticks_field(case, "journal_clock_ticks"),
        ticks_field(case, "service_clock_ticks"),
    );

    for file in case["files"].as_array().expect("files") {
        rig.fs.add_file_at_ticks(
            str_field(file, "path"),
            zeros(ticks_field(file, "len") as usize),
            ticks_field(file, "ticks"),
        );
    }
    for item in case.get("binned").and_then(Value::as_array).into_iter().flatten() {
        rig.fs.add_binned(
            str_field(item, "path"),
            ticks_field(item, "len") as usize,
            ts(ticks_field(item, "ticks")),
        );
    }
    let seeds: Vec<JournalEntry> = case["seed"].as_array().expect("seed").iter().map(seed_entry).collect();
    for seed in &seeds {
        rig.append(seed);
    }
    let snapshot_index = case.get("snapshot").and_then(Value::as_u64).unwrap_or(0) as usize;
    let snapshot = seeds[snapshot_index].clone();

    for hook in case.get("hooks").and_then(Value::as_array).into_iter().flatten() {
        let ops: Vec<Op> = hook["do"].as_array().expect("do").iter().map(op_of).collect();
        rig.fs.add_rule(data_rule(
            hook_name(str_field(hook, "on")),
            hook["path"].as_str(),
            hook.get("nth").and_then(Value::as_u64).unwrap_or(1) as usize,
            ops,
        ));
    }

    let bin = case
        .get("bin")
        .filter(|b| !b.is_null())
        .map(|b| rig.bin(bin_config_of(b)));
    let gate_mode = case.get("gate").and_then(Value::as_str).unwrap_or("none");
    let gate = match gate_mode {
        "none" => None,
        "free" | "held" => Some(new_gate()),
        other => panic!("unknown gate mode {other}"),
    };
    if gate_mode == "held" {
        assert!(gate.as_ref().expect("gate").try_begin(), "the test gate starts free");
    }
    Setup {
        rig,
        snapshot,
        bin,
        gate,
    }
}

/// What one case produced, in the golden row's shape.
fn run_case(case: &Value) -> Map<String, Value> {
    let Setup {
        rig,
        snapshot,
        bin,
        gate,
    } = setup(case);
    let mut service = rig.service();
    if let Some(bin) = &bin {
        service = service.with_recycle_bin(bin.clone());
    }
    if let Some(value) = case.get("allow_permanent").and_then(Value::as_bool) {
        service = service.with_allow_permanent_delete(Rig::allow(value));
    }
    if let Some(gate) = &gate {
        service = service.with_file_action_gate(gate.clone());
    }

    rig.fs.clear_ops();
    if case.get("cancel_before").and_then(Value::as_bool) == Some(true) {
        rig.cancel.cancel();
    }
    let marker = (case.get("live").and_then(Value::as_bool) == Some(true))
        .then(|| rig.journal.live_operations().begin(&snapshot.id));
    let confirmed = case.get("confirmed").and_then(Value::as_bool).unwrap_or(false);
    let token: CancelToken = rig.cancel.clone();
    let outcome = service.retry_move_or_copy(&snapshot, confirmed, &token);
    drop(marker);

    let mut row = Map::new();
    match outcome {
        Ok(RecoveryRetryResult {
            succeeded,
            message,
            entry,
            journal_persisted,
            journal_error,
            superseded,
        }) => {
            row.insert(
                "result".to_owned(),
                json!({
                    "succeeded": succeeded,
                    "message": message,
                    "superseded": superseded,
                    "journal_persisted": journal_persisted,
                    "journal_error": journal_error,
                    "entry": entry.as_ref().map(entry_json),
                }),
            );
            row.insert("threw".to_owned(), Value::Null);
        }
        Err(_canceled) => {
            row.insert("result".to_owned(), Value::Null);
            row.insert("threw".to_owned(), json!("TaskCanceledException"));
        }
    }
    row.insert("journal_text".to_owned(), json!(rig.fs.journal_text()));
    row.insert(
        "files_after".to_owned(),
        Value::Array(
            rig.fs
                .files_after()
                .into_iter()
                .map(|(path, len, last_write)| json!({"path": path, "len": len, "ticks": last_write.ticks()}))
                .collect(),
        ),
    );
    row.insert(
        "binned_after".to_owned(),
        Value::Array(rig.fs.binned().into_iter().map(|(path, _, _)| json!(path)).collect()),
    );
    row.insert("ops".to_owned(), json!(rig.fs.ops()));
    row.insert(
        "gate_busy_after".to_owned(),
        gate.as_ref().map_or(Value::Null, |g| json!(g.is_busy())),
    );
    row.insert("live_after".to_owned(), json!(rig.is_live(&snapshot.id)));
    row
}

/// The fields of a golden row the replay must reproduce.
const COMPARED: [&str; 8] = [
    "result",
    "threw",
    "journal_text",
    "files_after",
    "binned_after",
    "ops",
    "gate_busy_after",
    "live_after",
];

/// The text of a panic payload.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default()
}

/// DIVERGENCE H1/G25 (docs/reviews/FIX-H1-2026-10-31.md): the C# oracle deletes a permanent group member without a
/// fresh stat; we deliberately re-stat right before `bin.delete-permanently` so a file that replaced the member after
/// the pre-check is never deleted (`st_invmap_repro.rs` pins the new behaviour). That adds exactly one `stat P` op
/// immediately before each `bin.delete-permanently P` and (H1 follow-up) each `bin.recycle P` of a group member; it is removed from OUR op trace here so every other op of
/// every case still has to match the oracle exactly.
fn divergence_adjusted(field: &str, got: &Value) -> Value {
    if field != "ops" {
        return got.clone();
    }
    let Some(ops) = got.as_array() else {
        return got.clone();
    };
    let text = |i: usize| ops.get(i).and_then(Value::as_str).unwrap_or_default();
    let kept: Vec<Value> = ops
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            let Some(path) = text(*i).strip_prefix("stat ") else {
                return true;
            };
            let next = text(*i + 1);
            next.strip_prefix("bin.delete-permanently ") != Some(path)
                && next.strip_prefix("bin.recycle ") != Some(path)
        })
        .map(|(_, op)| op.clone())
        .collect();
    Value::Array(kept)
}

/// Every golden case replays exactly: all 106 rows, every compared field. A case that panics (the stubs panic with
/// `todo!()`) is one mismatch; the other cases still run and are reported together.
#[test]
fn recovery_retry_golden_replays_every_case() {
    let golden_file = golden::load(GOLDEN);
    assert_eq!(
        golden_file.cases.len(),
        CASE_COUNT,
        "the golden file holds the 106 contract cases"
    );
    let mut mismatches = Vec::new();
    for case in &golden_file.cases {
        let name = str_field(case, "name");
        let actual = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_case(case))) {
            Ok(actual) => actual,
            Err(payload) => {
                mismatches.push(format!("{name}: panicked: {}", panic_text(payload.as_ref())));
                continue;
            }
        };
        for field in COMPARED {
            let expected = &case[field];
            let got = &divergence_adjusted(field, &actual[field]);
            if expected != got {
                mismatches.push(format!("{name} / {field}:\n  golden: {expected}\n  ours:   {got}"));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} golden mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

/// One golden case per test, so a failure names its case (the replay above reports them all at once).
#[test]
fn recovery_retry_golden_single_move_succeeds() {
    replay_named("single-move-succeeds");
}

fn replay_named(name: &str) {
    let golden_file = golden::load(GOLDEN);
    let case = golden_file
        .cases
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no golden case {name}"));
    let actual = run_case(case);
    for field in COMPARED {
        assert_eq!(actual[field], case[field], "{name} / {field}");
    }
}

/// Harness self-check (passes against the stubs, no service call): the seed lines the replay writes are the first
/// lines of every golden `journal_text`, so the scripted disk, the journal format and the journal clock agree with C#.
#[test]
fn recovery_retry_golden_seed_lines_match_the_journal_prefix() {
    let golden_file = golden::load(GOLDEN);
    let mut mismatches = Vec::new();
    for case in &golden_file.cases {
        let name = str_field(case, "name");
        let Setup { rig, .. } = setup(case);
        let ours = rig.fs.journal_text().unwrap_or_default();
        let expected = str_field(case, "journal_text");
        if ours.is_empty() || !expected.starts_with(&ours) {
            mismatches.push(format!("{name}:\n  seeds:  {ours:?}\n  golden: {expected:?}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} seed mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
