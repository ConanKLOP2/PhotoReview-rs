//! Red tests for `pr_app::presenters::action_profiles` (W8-10a, contract `contracts/W8-10a.md`): the bodies are `todo!()`.
//! Presenter and static checks; the import/export part is in `action_profiles_io.rs`. Golden `action-profiles` (38 cases).

mod presenters_support;

use pr_app::presenters::action_list_json::throw_if_invalid;
use pr_app::presenters::action_profiles::{
    ActionProfileForm, ActionProfilesPresenter, RowId, find_destination_problem, has_invalid_actions,
    index_to_operation, next_free_shortcut, operation_to_index, parse_shortcut,
};
use pr_core::model::enums::FileOperationType;
use pr_core::settings::shortcuts::{ReviewAction, canonicalize};
use pr_l10n::keys;
use pr_l10n::template::LocArg;
use presenters_support::{act, actions_of, english, golden_pairs, localizer, mv, rows, vietnamese};
use serde_json::Value;

fn form(name: &str, shortcut: &str, operation_index: i32, destination: &str, confirm: bool) -> ActionProfileForm {
    ActionProfileForm {
        name: name.to_owned(),
        shortcut: shortcut.to_owned(),
        operation_index,
        destination: destination.to_owned(),
        confirm,
    }
}

fn names(presenter: &ActionProfilesPresenter) -> Vec<String> {
    presenter.actions().iter().map(|a| a.name.clone()).collect()
}

fn row_ids(presenter: &ActionProfilesPresenter) -> Vec<RowId> {
    (0..presenter.actions().len())
        .map(|i| presenter.row_id(i).expect("row id"))
        .collect()
}

#[test]
fn presenter_edits_a_copy() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_EditsACopy
    let mut original = vec![mv("A", "F6")];
    let presenter = ActionProfilesPresenter::new(&original);
    original[0].name = "Changed".to_owned();
    original.push(mv("B", "F7"));

    assert_eq!(names(&presenter), ["A"]);
    assert_eq!(row_ids(&presenter).len(), 1);
}

#[test]
fn open_returns_fields_and_operation_index() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_OpenReturnsFields
    let mut presenter = ActionProfilesPresenter::new(&[
        act("Copy it", "F6", FileOperationType::Copy, "Backup", false),
        act("Bin", "F7", FileOperationType::Recycle, "Out", false),
    ]);
    let first = presenter.row_id(0);
    let second = presenter.row_id(1);
    assert!(first.is_some() && second.is_some() && first != second);

    assert_eq!(presenter.open(first), Some(form("Copy it", "F6", 1, "Backup", false)));
    assert_eq!(presenter.loaded(), first);
    assert_eq!(presenter.open(second).map(|f| f.operation_index), Some(2));
    assert_eq!(presenter.open(None), None);
}

#[test]
fn save_current_writes_trimmed_fields_into_the_open_row() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_SaveCurrentWritesTheOpenRow
    let mut presenter = ActionProfilesPresenter::new(&[mv("A", "F6")]);
    assert!(!presenter.save_current(&ActionProfileForm::empty()));

    let first = presenter.row_id(0);
    presenter.open(first);
    assert!(presenter.save_current(&form("  New name ", " f8 ", 2, "  Dest ", true)));

    let action = &presenter.actions()[0];
    assert_eq!(action.name, "New name");
    assert_eq!(action.shortcut, canonicalize(Some(" f8 ")));
    assert_eq!(action.operation, FileOperationType::Recycle);
    assert_eq!(action.destination, "Dest");
    assert!(action.confirm);
}

#[test]
fn removed_open_row_is_not_written_back() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_RemovedRowIsNotWrittenBack (T4: identity, not an index)
    let mut presenter = ActionProfilesPresenter::new(&[mv("A", "F6"), mv("B", "F7")]);
    let first = presenter.row_id(0).expect("row 0");
    presenter.open(Some(first));

    let next = presenter.remove(first, 0);

    assert_eq!(next, Some(0));
    assert!(!presenter.save_current(&form("ghost", "F9", 0, "x", false)));
    assert_eq!(names(&presenter), ["B"]);
    // The surviving row is unchanged: the ghost text went nowhere.
    assert_eq!(presenter.actions()[0].destination, "Out");
}

#[test]
fn remove_selection_rules_clamp_and_signal_empty() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_RemoveSelectionRules (T5)
    let mut presenter = ActionProfilesPresenter::new(&[mv("A", "F6"), mv("B", "F7"), mv("C", "F8")]);
    let b = presenter.row_id(1).unwrap();
    assert_eq!(presenter.remove(b, 1), Some(1));
    // The list is A, C: removing the last one clamps to the new last index.
    let c = presenter.row_id(1).unwrap();
    assert_eq!(presenter.remove(c, 1), Some(0));
    let a = presenter.row_id(0).unwrap();
    assert_eq!(presenter.remove(a, 0), None);
    assert!(presenter.actions().is_empty());
    assert_eq!(presenter.loaded(), None);
}

#[test]
fn add_appends_move_with_next_free_key_and_localized_name() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_AddUsesNextFreeKey (T6)
    let texts = english();
    let mut presenter = ActionProfilesPresenter::new(&[mv("A", "F6"), mv("B", "Return")]);
    let added = presenter.add(&texts);

    let action = &presenter.actions()[2];
    assert_eq!(presenter.row_id(2), Some(added));
    assert_eq!(action.shortcut, "F7");
    assert_eq!(action.name, texts.get(keys::ACTION_PROFILES_NEW_ACTION_NAME));
    assert_eq!(action.name, "New action");
    assert_eq!(action.operation, FileOperationType::Move);
    assert_eq!(action.destination, "Output");
    assert!(!action.confirm);
    assert_eq!(presenter.actions().len(), 3);
}

#[test]
fn next_free_shortcut_skips_aliases_and_falls_back_to_f6() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_NextFreeShortcut
    assert_eq!(next_free_shortcut(&[mv("A", "f6"), mv("B", "F7")]), "F8");
    let f6_to_f11: Vec<ReviewAction> = (6..12).map(|n| mv(&format!("k{n}"), &format!("F{n}"))).collect();
    assert_eq!(next_free_shortcut(&f6_to_f11), "F12");
    let all: Vec<ReviewAction> = (6..13).map(|n| mv(&format!("k{n}"), &format!("F{n}"))).collect();
    assert_eq!(next_free_shortcut(&all), "F6");
}

#[test]
fn apply_checks_refuse_empty_blank_bad_key_and_aliases() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_ApplyChecks (7 cases; the undefined operation 99 is not
    // representable in Rust, T12)
    assert!(has_invalid_actions(&[]));
    assert!(has_invalid_actions(&[mv(" ", "F6")]));
    assert!(has_invalid_actions(&[mv("A", "Nope")]));
    assert!(has_invalid_actions(&[mv("A", "Return"), mv("B", "Enter")]));
    assert!(!has_invalid_actions(&[mv("A", "F6"), mv("B", "F7")]));

    let texts = english();
    let bad = ActionProfilesPresenter::new(&[act("A", "F6", FileOperationType::Move, "..\\up", false)]);
    let result = bad.apply(&texts);
    assert!(!result.accepted);
    assert_eq!(
        result.message,
        Some(texts.format(
            keys::ACTION_PROFILES_ERROR_DESTINATION_ESCAPES,
            &[LocArg::new("name", "A")]
        ))
    );

    let invalid = ActionProfilesPresenter::new(&[mv("", "F6")]);
    let result = invalid.apply(&texts);
    assert!(!result.accepted);
    assert_eq!(
        result.message,
        Some(texts.get(keys::DIALOG_ACTION_PROFILES_INVALID_MESSAGE))
    );

    let good = ActionProfilesPresenter::new(&[mv("A", "F6")]);
    let result = good.apply(&texts);
    assert!(result.accepted);
    assert_eq!(result.message, None);
}

#[test]
fn recycle_ignores_destination_and_copy_does_not() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_RecycleIgnoresDestination (T13)
    let texts = english();
    assert_eq!(
        find_destination_problem(&[act("Bin", "F6", FileOperationType::Recycle, "..\\up", false)], &texts),
        None
    );
    assert!(find_destination_problem(&[act("Cp", "F6", FileOperationType::Copy, "..\\up", false)], &texts).is_some());
}

#[test]
fn replace_all_swaps_the_whole_list_and_stales_old_rows() {
    // C#: SecondaryDialogPresenterTests.ActionProfiles_ReplaceAll (T14 for the stale part)
    let mut presenter = ActionProfilesPresenter::new(&[mv("A", "F6")]);
    let old = presenter.row_id(0).unwrap();
    presenter.open(Some(old));

    presenter.replace_all(&[mv("X", "F7"), mv("Y", "F8")]);

    assert_eq!(names(&presenter), ["X", "Y"]);
    assert_eq!(presenter.loaded(), None);
    assert!(!presenter.save_current(&form("ghost", "F9", 0, "x", false)));
    assert_eq!(names(&presenter), ["X", "Y"]);
}

#[test]
fn has_invalid_aliased_shortcuts_are_duplicates() {
    // C#: ActionProfilesValidationTests.HasInvalidActions_AliasedShortcuts_AreDuplicates (4 rows)
    for (first, second) in [
        ("Return", "Enter"),
        ("Prior", "PageUp"),
        ("Next", "PageDown"),
        ("f6", "F6"),
    ] {
        assert!(
            has_invalid_actions(&[mv("a", first), mv("b", second)]),
            "{first}/{second}"
        );
    }
}

#[test]
fn has_invalid_distinct_valid_actions_are_accepted() {
    // C#: ActionProfilesValidationTests.HasInvalidActions_DistinctValidActions_AreAccepted
    assert!(!has_invalid_actions(&[mv("a", "F6"), mv("b", "F7")]));
}

#[test]
fn has_invalid_empty_blank_name_bad_shortcut_are_rejected() {
    // C#: ActionProfilesValidationTests.HasInvalidActions_EmptyBlankNameBadShortcut_AreRejected
    assert!(has_invalid_actions(&[]));
    assert!(has_invalid_actions(&[mv(" ", "F6")]));
    assert!(has_invalid_actions(&[mv("a", "Escape")]));
}

#[test]
fn next_free_shortcut_skips_used_keys_including_aliases() {
    // C#: ActionProfilesValidationTests.NextFreeShortcut_SkipsUsedKeysIncludingAliasesOfThem
    assert_eq!(next_free_shortcut(&[]), "F6");
    assert_eq!(next_free_shortcut(&[mv("a", "F6")]), "F7");
    assert_eq!(next_free_shortcut(&[mv("a", "f6"), mv("b", "F7")]), "F8");
}

#[test]
fn default_actions_pass_destination_validation() {
    // C#: ActionProfilesDestinationTests.DefaultActions_PassDestinationValidation (static part only)
    let texts = english();
    let defaults = ReviewAction::defaults(&|n| format!("Action {n}"), "Backup");
    assert_eq!(find_destination_problem(&defaults, &texts), None);
    assert!(find_destination_problem(&[act("x", "Enter", FileOperationType::Copy, "a|b", false)], &texts).is_some());
    assert_eq!(
        find_destination_problem(&[act("r", "Enter", FileOperationType::Recycle, "", false)], &texts),
        None
    );
}

#[test]
fn escaping_destination_names_the_action_in_the_reason() {
    // C#: ActionProfilesDestinationTests.Apply_WithEscapingDestination_ShowsReasonAndKeepsWindowOpen (static assertion)
    let texts = english();
    let bad = [act("Loai-9", "F9", FileOperationType::Move, "..\\x", false)];
    assert_eq!(
        find_destination_problem(&bad, &texts),
        Some(texts.format(
            keys::ACTION_PROFILES_ERROR_DESTINATION_ESCAPES,
            &[LocArg::new("name", "Loai-9")]
        ))
    );
}

#[test]
fn remove_stale_row_removes_nothing() {
    // new: rule 6, a removed (stale) id removes nothing and the list keeps its rows.
    let mut presenter = ActionProfilesPresenter::new(&[mv("A", "F6"), mv("B", "F7")]);
    let a = presenter.row_id(0).unwrap();
    assert_eq!(presenter.remove(a, 0), Some(0));
    let again = presenter.remove(a, 0);
    assert_eq!(names(&presenter), ["B"]);
    assert_eq!(again, Some(0));
}

#[test]
fn replace_all_row_ids_are_new() {
    // new: rule 7, every row id of the new list differs from every id of the old one.
    let mut presenter = ActionProfilesPresenter::new(&[mv("A", "F6"), mv("B", "F7")]);
    let old = row_ids(&presenter);
    presenter.replace_all(&[mv("A", "F6"), mv("B", "F7")]);
    let new = row_ids(&presenter);
    assert_eq!(new.len(), 2);
    assert!(new.iter().all(|id| !old.contains(id)));
    assert_ne!(new[0], new[1]);
    // An id of the old list is not a live row: open returns nothing and clears `loaded`.
    assert_eq!(presenter.open(Some(old[0])), None);
    assert_eq!(presenter.loaded(), None);
}

#[test]
fn row_id_is_none_out_of_range() {
    // new: `row_id` contract, `None` when the index is out of range.
    let presenter = ActionProfilesPresenter::new(&[mv("A", "F6")]);
    assert!(presenter.row_id(0).is_some());
    assert_eq!(presenter.row_id(1), None);
}

#[test]
fn operation_index_maps_unknown_to_move() {
    // new: rule 15, Move 0, Copy 1, Recycle 2; indexes -1, 3 and 99 read as Move.
    assert_eq!(operation_to_index(FileOperationType::Move), 0);
    assert_eq!(operation_to_index(FileOperationType::Copy), 1);
    assert_eq!(operation_to_index(FileOperationType::Recycle), 2);
    assert_eq!(index_to_operation(0), FileOperationType::Move);
    assert_eq!(index_to_operation(1), FileOperationType::Copy);
    assert_eq!(index_to_operation(2), FileOperationType::Recycle);
    for index in [-1, 3, 99] {
        assert_eq!(index_to_operation(index), FileOperationType::Move, "{index}");
    }
}

#[test]
fn text_check_and_key_check_are_separate() {
    // new: T1, `has_invalid_actions` compares parsed keys and `throw_if_invalid` compares canonical text; both reject
    // the alias pairs and both accept distinct keys.
    for (first, second) in [("Return", "Enter"), ("Prior", "PageUp"), ("f6", "F6")] {
        let list = [mv("a", first), mv("b", second)];
        assert!(has_invalid_actions(&list), "{first}/{second}");
        assert!(throw_if_invalid(&list).is_err(), "{first}/{second}");
    }
    let ok = [mv("a", "F6"), mv("b", "F7")];
    assert!(!has_invalid_actions(&ok));
    assert!(throw_if_invalid(&ok).is_ok());
}

#[test]
fn parse_shortcut_accepts_keys_and_rejects_text_digits_and_reserved_keys() {
    // new: rule 14, blank text, digits, comma lists and reserved keys are `None`; aliases parse to the same key.
    for text in ["", "  ", "999", "F6,F7", "Escape", "Tab", "Nope"] {
        assert_eq!(parse_shortcut(text), None, "{text:?}");
    }
    assert!(parse_shortcut("F6").is_some());
    assert_eq!(parse_shortcut("f6"), parse_shortcut("F6"));
    assert_eq!(parse_shortcut("Return"), parse_shortcut("Enter"));
    assert_eq!(parse_shortcut("Prior"), parse_shortcut("PageUp"));
}

#[test]
fn trim_matches_dotnet_whitespace() {
    // new: T2, `char.IsWhiteSpace` trims NBSP (U+00A0) and NEL (U+0085) and keeps a BOM (U+FEFF); golden
    // `script-save-trims-but-not-bom`.
    let mut presenter = ActionProfilesPresenter::new(&[mv("A", "F6")]);
    let first = presenter.row_id(0);
    presenter.open(first);
    assert!(presenter.save_current(&form("\u{a0}x\u{a0}", "F6", 0, "\u{85}d\u{85}", false)));
    assert_eq!(presenter.actions()[0].name, "x");
    assert_eq!(presenter.actions()[0].destination, "d");
    // The BOM is not whitespace: the name keeps it and a blank-looking BOM name is still a name.
    assert!(presenter.save_current(&form("\u{feff}y", "\u{feff}F6", 0, "d", false)));
    assert_eq!(presenter.actions()[0].name, "\u{feff}y");
    assert_eq!(presenter.actions()[0].shortcut, "\u{feff}F6");
}

#[test]
fn golden_action_profiles() {
    // new: golden `action-profiles` (38 cases: has_invalid 10, next_free 7, destination_problem 8, apply 4, script 9).
    let mut failures = Vec::new();
    for (input, expected) in golden_pairs("action-profiles") {
        let name = expected["name"].as_str().unwrap().to_owned();
        let kind = expected["kind"].as_str().unwrap().to_owned();
        let lang = input["lang"].as_str().unwrap_or("en");
        let outcome = run_case(&kind, lang, &input, &expected);
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

fn run_case(kind: &str, lang: &str, input: &Value, expected: &Value) -> Result<(), String> {
    let texts = localizer(lang);
    match kind {
        "has_invalid" => {
            let got = has_invalid_actions(&actions_of(&input["actions"]));
            let want = expected["value"].as_bool().unwrap();
            same(&got, &want)
        }
        "next_free" => {
            let got = next_free_shortcut(&actions_of(&input["actions"]));
            same(&got, &expected["value"].as_str().unwrap().to_owned())
        }
        "destination_problem" => {
            let got = find_destination_problem(&actions_of(&input["actions"]), &texts);
            same(&got, &expected["value"].as_str().map(str::to_owned))
        }
        "apply" => {
            let result = ActionProfilesPresenter::new(&actions_of(&input["actions"])).apply(&texts);
            same(
                &(result.accepted, result.message),
                &(
                    expected["accepted"].as_bool().unwrap(),
                    expected["message"].as_str().map(str::to_owned),
                ),
            )
        }
        "script" => run_script(&texts, input, &expected["steps"][0]),
        other => panic!("unknown golden kind {other}"),
    }
}

fn same<T: PartialEq + std::fmt::Debug>(got: &T, want: &T) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("got {got:?}, want {want:?}"))
    }
}

fn form_of(v: &Value) -> ActionProfileForm {
    form(
        v["name"].as_str().unwrap(),
        v["shortcut"].as_str().unwrap(),
        i32::try_from(v["operation_index"].as_i64().unwrap()).unwrap(),
        v["destination"].as_str().unwrap(),
        v["confirm"].as_bool().unwrap(),
    )
}

/// Runs the `ops` of a script case. A step value is the form (`open`), a bool (`save`), the next selection (`remove`:
/// the index, `-1` for none), the index of the new row (`add`) or null (`open_none`, `replace_all`).
fn run_script(texts: &pr_l10n::localizer::Localizer, input: &Value, out: &Value) -> Result<(), String> {
    let mut presenter = ActionProfilesPresenter::new(&actions_of(&input["actions"]));
    let want_steps = out["steps"].as_array().expect("steps");
    let ops = input["ops"].as_array().expect("ops");
    if ops.len() != want_steps.len() {
        return Err(format!("{} ops but {} expected steps", ops.len(), want_steps.len()));
    }
    for (i, (op, want)) in ops.iter().zip(want_steps).enumerate() {
        let index_of =
            |p: &ActionProfilesPresenter| usize::try_from(op["index"].as_i64().unwrap()).map(|n| p.row_id(n));
        let got: Value = match op["op"].as_str().unwrap() {
            "open" => {
                let row = index_of(&presenter).unwrap();
                match presenter.open(row) {
                    Some(f) => serde_json::json!({
                        "name": f.name, "shortcut": f.shortcut, "operation_index": f.operation_index,
                        "destination": f.destination, "confirm": f.confirm,
                    }),
                    None => Value::Null,
                }
            }
            "open_none" => {
                let _ = presenter.open(None);
                Value::Null
            }
            "save" => Value::Bool(presenter.save_current(&form_of(&op["fields"]))),
            "remove" => {
                let row = index_of(&presenter).unwrap().expect("row to remove");
                let selected = usize::try_from(op["selected"].as_i64().unwrap()).unwrap();
                serde_json::json!(
                    presenter
                        .remove(row, selected)
                        .map_or(-1, |n| i64::try_from(n).unwrap())
                )
            }
            "add" => {
                let id = presenter.add(texts);
                let at = (0..presenter.actions().len()).find(|&n| presenter.row_id(n) == Some(id));
                serde_json::json!(at.map(|n| i64::try_from(n).unwrap()))
            }
            "replace_all" => {
                presenter.replace_all(&actions_of(&op["actions"]));
                Value::Null
            }
            other => panic!("unknown script op {other}"),
        };
        if &got != want {
            return Err(format!("step {i} ({}): got {got}, want {want}", op["op"]));
        }
    }
    let want_rows = rows(&actions_of(&out["actions"]));
    same(&rows(presenter.actions()), &want_rows)
}

#[test]
fn golden_vi_localizer_is_wired() {
    // new: the `vi` golden cases need the Vietnamese catalog; the new-action name differs from English.
    let vi = vietnamese();
    let mut presenter = ActionProfilesPresenter::new(&[]);
    presenter.add(&vi);
    assert_eq!(
        presenter.actions()[0].name,
        vi.get(keys::ACTION_PROFILES_NEW_ACTION_NAME)
    );
    assert_ne!(presenter.actions()[0].name, "New action");
    assert_eq!(presenter.actions()[0].shortcut, "F6");
}
