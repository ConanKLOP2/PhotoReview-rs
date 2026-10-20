//! Red tests for the import/export part of `pr_app::presenters::action_profiles` (W8-10a, contract
//! `contracts/W8-10a.md`): the bodies are `todo!()`. Golden `action-profiles-io` (13 cases). Temp folders come from
//! `presenters_support::TempDir` and delete themselves; the OS message text is never compared, only its kind.

mod presenters_support;

use std::path::Path;

use pr_app::misc::app_log::AppLog;
use pr_app::presenters::action_profiles::{try_export, try_import};
use pr_core::model::enums::FileOperationType;
use pr_l10n::keys;
use presenters_support::{TempDir, act, actions_of, english, golden_pairs, lf, localizer, mv, rows};

/// The failure path logs to `AppLog`; keep the user's real log file out of the tests.
fn quiet_log() {
    AppLog::global().set_enabled(false);
}

#[test]
fn import_export_round_trip_and_reasons() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_ImportExport
    quiet_log();
    let texts = english();
    let invalid = texts.get(keys::DIALOG_IMPORT_ACTIONS_INVALID_MESSAGE);
    let dir = TempDir::new("roundtrip");
    let path = dir.join("actions.json");

    assert_eq!(
        try_export(
            &path,
            &[act("Vi\u{1ec7}t", "F6", FileOperationType::Copy, "Backup", false)],
            &texts
        ),
        None
    );
    let imported = try_import(&path, &texts).expect("the exported file imports");
    assert_eq!(imported.len(), 1);
    assert_eq!(
        (
            &imported[0].name[..],
            &imported[0].shortcut[..],
            imported[0].operation,
            &imported[0].destination[..]
        ),
        ("Vi\u{1ec7}t", "F6", FileOperationType::Copy, "Backup")
    );

    std::fs::write(&path, "{\"not\":\"a list\"}").unwrap();
    assert_eq!(try_import(&path, &texts), Err(invalid.clone()));

    std::fs::write(&path, "[]").unwrap();
    assert_eq!(try_import(&path, &texts), Err(invalid.clone()));

    let missing = try_import(&dir.join("missing.json"), &texts).expect_err("a missing file fails");
    assert_ne!(missing, invalid);
    assert!(!missing.trim().is_empty());

    assert!(try_export(&dir.join("nope").join("x.json"), &[mv("A", "F6")], &texts).is_some());
}

#[test]
fn try_import_malformed_gives_invalid_message() {
    // C#: ActionProfilesValidationTests.TryImport_MalformedFile_GivesInvalidMessageWithoutThrowing
    quiet_log();
    let texts = english();
    let dir = TempDir::new("malformed");
    let path = dir.join("actions.json");

    std::fs::write(&path, "[null]").unwrap();
    let error = try_import(&path, &texts).expect_err("a null element is invalid");
    assert!(!error.is_empty());
    assert_eq!(error, texts.get(keys::DIALOG_IMPORT_ACTIONS_INVALID_MESSAGE));

    std::fs::write(&path, "not json").unwrap();
    assert_eq!(
        try_import(&path, &texts),
        Err(texts.get(keys::DIALOG_IMPORT_ACTIONS_INVALID_MESSAGE))
    );
}

#[test]
fn try_import_missing_or_illegal_path_reports_failed() {
    // C#: ActionProfilesValidationTests.TryImport_PathWithIllegalChars_ReportsErrorWithoutThrowing ("bad path.json")
    quiet_log();
    let texts = english();
    let error = try_import(Path::new("bad path.json"), &texts).expect_err("the file does not exist");
    assert!(!error.is_empty());
    assert_ne!(error, texts.get(keys::DIALOG_IMPORT_ACTIONS_INVALID_MESSAGE));
}

#[test]
fn try_import_missing_file_reports_failed_with_the_file_name() {
    // C#: ActionProfilesValidationTests.TryImport_MissingFile_ReportsTheIoReason (open question 3: `<path>: <os text>`)
    quiet_log();
    let texts = english();
    let dir = TempDir::new("missing");
    let path = dir.join("pr-import-missing-w8.json");
    let error = try_import(&path, &texts).expect_err("a missing file fails");
    assert!(error.contains("pr-import-missing-w8.json"), "{error}");
    assert!(error.starts_with("Could not read the action profile file: "), "{error}");
}

#[test]
fn try_import_valid_file_returns_rows() {
    // C#: ActionProfilesValidationTests.TryImport_ValidFile_ReturnsClones
    quiet_log();
    let texts = english();
    let dir = TempDir::new("valid");
    let path = dir.join("pr-import-ok.json");
    std::fs::write(
        &path,
        "[{\"Name\":\"x\",\"Shortcut\":\"F6\",\"Operation\":\"Move\",\"Destination\":\"Out\",\"Confirm\":false}]",
    )
    .unwrap();
    let list = try_import(&path, &texts).expect("a valid file imports");
    assert_eq!(rows(&list), rows(&[mv("x", "F6")]));
}

#[test]
fn try_import_invalid_utf8_reports_failed_not_invalid() {
    // new: rule 12, invalid UTF-8 is an I/O-side failure (`failed`), not substituted with U+FFFD (P-04 / D20).
    quiet_log();
    let texts = english();
    let dir = TempDir::new("utf8");
    let path = dir.join("actions.json");
    std::fs::write(&path, [b'[', 0xFF, 0xFE, b']']).unwrap();
    let error = try_import(&path, &texts).expect_err("invalid UTF-8 fails");
    assert!(error.starts_with("Could not read the action profile file: "), "{error}");
}

#[test]
fn try_export_unwritable_path_returns_message_and_writable_succeeds() {
    // C#: ReviewRound7WindowTests R7-9 (the `TryExport` part)
    quiet_log();
    let texts = english();
    let dir = TempDir::new("export");
    let actions = pr_core::settings::shortcuts::ReviewAction::defaults(&|n| format!("Action {n}"), "Backup");

    // The target is an existing directory: the write fails.
    let error = try_export(dir.path(), &actions, &texts).expect("a directory cannot be written");
    assert!(!error.trim().is_empty());
    assert!(error.starts_with("Could not save the action profile file: "), "{error}");

    let file = dir.join("actions.json");
    assert_eq!(try_export(&file, &actions, &texts), None);
    assert!(file.is_file());
}

#[test]
fn golden_action_profiles_io() {
    // new: golden `action-profiles-io` (13 cases: try_import 8, try_export 5).
    quiet_log();
    let mut failures = Vec::new();
    for (input, expected) in golden_pairs("action-profiles-io") {
        let name = expected["name"].as_str().unwrap().to_owned();
        let texts = localizer(input["lang"].as_str().unwrap());
        let dir = TempDir::new("golden");
        let outcome: Result<(), String> = match expected["kind"].as_str().unwrap() {
            "try_import" => {
                let path = dir.join(input["file"].as_str().unwrap());
                if let Some(content) = input["file_text"].as_str() {
                    std::fs::write(&path, content).unwrap();
                }
                check_import(&expected, try_import(&path, &texts), &input, &texts)
            }
            "try_export" => {
                let actions = actions_of(&input["actions"]);
                let path = match input["target"].as_str().unwrap() {
                    "file" => dir.join("actions.json"),
                    "directory" => dir.path().to_path_buf(),
                    "missing_folder" => dir.join("nope").join("x.json"),
                    other => panic!("unknown target {other}"),
                };
                let got = try_export(&path, &actions, &texts);
                if expected["result"] == "ok" {
                    match (got, std::fs::read_to_string(&path)) {
                        (None, Ok(text)) => {
                            let want = lf(expected["file_text"].as_str().unwrap());
                            if lf(&text) == want {
                                Ok(())
                            } else {
                                Err(format!("file {text:?}, want {want:?}"))
                            }
                        }
                        (got, file) => Err(format!("export gave {got:?}, file {file:?}")),
                    }
                } else if got.is_some_and(|m| !m.trim().is_empty()) {
                    Ok(())
                } else {
                    Err("expected a failure message".to_owned())
                }
            }
            other => panic!("unknown golden kind {other}"),
        };
        if let Err(why) = outcome {
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

fn check_import(
    expected: &serde_json::Value,
    got: Result<Vec<pr_core::settings::shortcuts::ReviewAction>, String>,
    input: &serde_json::Value,
    texts: &pr_l10n::localizer::Localizer,
) -> Result<(), String> {
    match (expected["result"].as_str().unwrap(), got) {
        ("ok", Ok(list)) => {
            let want = actions_of(&expected["actions"]);
            if rows(&list) == rows(&want) {
                Ok(())
            } else {
                Err(format!("got {list:?}, want {want:?}"))
            }
        }
        ("invalid", Err(message)) => {
            let want = expected["message"].as_str().unwrap();
            if message == want {
                Ok(())
            } else {
                Err(format!("got {message:?}, want {want:?}"))
            }
        }
        ("failed", Err(message)) => {
            let invalid = texts.get(keys::DIALOG_IMPORT_ACTIONS_INVALID_MESSAGE);
            let file = input["file"].as_str().unwrap();
            if message == invalid {
                Err("a missing file must not read as an invalid file".to_owned())
            } else if expected["error_names_file"] == true && !message.contains(file) {
                Err(format!("message {message:?} does not name {file}"))
            } else {
                Ok(())
            }
        }
        (want, other) => Err(format!("want {want}, got {other:?}")),
    }
}
