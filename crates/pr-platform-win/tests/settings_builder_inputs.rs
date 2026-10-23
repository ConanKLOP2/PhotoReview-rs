mod common {
    pub mod settings_builder_fixture;
}

use pr_platform_win::dialogs::settings::builder::inputs::{SettingsBuilderError, SettingsPageBuilder};
use pr_platform_win::dialogs::settings::builder::layout::{KeyGridElement, SettingsKeyCell};
use pr_platform_win::dialogs::settings::builder::rows::SettingsLayoutElement;
use pr_platform_win::dialogs::settings::page_view::TextFn;
use std::rc::Rc;

fn text(value: &'static str) -> TextFn {
    Rc::new(move || value.to_owned())
}

#[test]
fn ids_are_id_base_plus_one_in_declaration_order() {
    let mut builder = SettingsPageBuilder::new(5000);
    let first = builder.check(text("first"), 0);
    let row = builder.edit(text("edit"), 60);
    assert_eq!(first, 5001);
    assert_eq!((row.label, row.control), (5002, 5003));
}

#[test]
fn builder_radio_gives_the_first_radio_group_and_tab_stop_the_others_neither_and_cuts_the_group_after_it() {
    // C#: SettingsFilesPageTests.Builder_Radio_GivesTheFirstRadioGroupAndTabStop_TheOthersNeither_AndCutsTheGroupAfterIt
    let mut builder = SettingsPageBuilder::new(5000);
    let ids = builder.radio(&[text("one"), text("two"), text("three")], None, 0);
    let next = builder.check(text("next"), 0);
    let later = builder.check(text("later"), 0);
    assert_eq!(ids, vec![5001, 5002, 5003]);
    let styles: Vec<_> = builder.items().iter().map(|item| item.style).collect();
    assert_ne!(styles[0] & (0x20000 | 0x10000), 0);
    assert_eq!(styles[1] & (0x20000 | 0x10000), 0);
    assert_eq!(styles[2] & (0x20000 | 0x10000), 0);
    assert_ne!(
        builder.items().iter().find(|item| item.id == next).unwrap().style & 0x20000,
        0
    );
    assert_eq!(
        builder.items().iter().find(|item| item.id == later).unwrap().style & 0x20000,
        0
    );
    assert!(
        ids.iter()
            .all(|id| { builder.items().iter().find(|item| item.id == *id).unwrap().style & 0x0f == 9 })
    );
}

#[test]
fn operation_fixture_returns_declaration_results_and_a_builder_dump() {
    let ops = [serde_json::json!({"op":"check","text":"enabled","indent_dlu":4})];
    let mut builder = SettingsPageBuilder::new(200);
    let results = common::settings_builder_fixture::run_ops(&mut builder, &ops);
    assert_eq!(results, vec![serde_json::json!(201)]);
    let dump = common::settings_builder_fixture::dump(&builder);
    assert_eq!(dump["items"][0]["id"], 201);
    assert_eq!(dump["counts"]["items"], 1);
}

#[test]
fn builder_radio_creates_the_hints_after_every_radio_as_dimmed_notes_and_the_first_hint_opens_the_next_group() {
    // C#: SettingsFilesPageTests.Builder_Radio_CreatesTheHintsAfterEveryRadio_AsDimmedNotes_AndTheFirstHintOpensTheNextGroup
    let mut builder = SettingsPageBuilder::new(7000);
    let radios = [text("one"), text("two")];
    let hints = [Some(text("hint")), None];
    let ids = builder.radio(&radios, Some(&hints), 0);
    assert_eq!(ids, vec![7001, 7002]);
    assert_eq!(builder.items().len(), 3);
    assert_eq!(builder.items()[2].id, 7003);
    assert_eq!(builder.notes().iter().copied().collect::<Vec<_>>(), vec![7003]);
    assert_ne!(builder.items()[2].style & 0x20000, 0);
    assert_eq!(builder.elements().len(), 2);
}

#[test]
fn builder_radio_rejects_fewer_than_two_options_and_a_hint_list_of_the_wrong_length() {
    // C#: SettingsFilesPageTests.Builder_Radio_RejectsFewerThanTwoOptions_AndAHintListOfTheWrongLength
    let mut builder = SettingsPageBuilder::new(0);
    assert_eq!(builder.radio(&[text("one")], None, 0), vec![0]);
    assert_eq!(builder.error(), Some(SettingsBuilderError::RadioNeedsTwoOptions));
    let mut builder = SettingsPageBuilder::new(0);
    let hints = [Some(text("hint"))];
    assert_eq!(builder.radio(&[text("one"), text("two")], Some(&hints), 0), vec![0, 0]);
    assert_eq!(builder.error(), Some(SettingsBuilderError::RadioHintCountMismatch));
}

#[test]
fn group_box_id_precedes_its_children_and_has_no_tab_stop() {
    let mut builder = SettingsPageBuilder::new(40);
    let group_id = builder.group(text("group"), |builder| {
        builder.check(text("child"), 0);
    });
    assert_eq!(group_id, 41);
    assert_eq!(builder.items()[0].id, group_id);
    assert_eq!(builder.items()[1].id, 42);
    assert_eq!(builder.items()[0].style & 0x10000, 0);
    assert!(builder.elements()[0].as_group().is_some());
}

#[test]
fn group_content_closure_is_not_run_when_the_group_box_cannot_be_added() {
    let mut builder = SettingsPageBuilder::new(0);
    for _ in 0..999 {
        builder.text(None);
    }
    let mut called = false;
    assert_eq!(builder.group(text("late"), |_| called = true), 0);
    assert!(!called);
}

#[test]
fn button_text_ampersands_are_doubled_in_item_and_provider_only_for_button_class() {
    let mut builder = SettingsPageBuilder::new(0);
    let ids = builder.buttons(false, &[text("A&B")]);
    assert_eq!(builder.items()[0].text, "A&&B");
    assert_eq!((builder.texts()[&ids[0]])(), "A&&B");
}

#[test]
fn texts_registered_only_for_controls_declared_with_a_text() {
    let mut builder = SettingsPageBuilder::new(0);
    builder.note(Some(text("note")), true);
    builder.text(None);
    builder.combo(text("label"), None, 0);
    assert_eq!(builder.texts().len(), 2);
}

#[test]
fn combo_items_registered_only_when_given_and_empty_list_registers_empty() {
    let mut builder = SettingsPageBuilder::new(0);
    let absent = builder.combo(text("a"), None, 0);
    let empty = builder.combo(text("b"), Some(vec![]), 0);
    assert!(!builder.combo_items().contains_key(&absent.control));
    assert_eq!(builder.combo_items().get(&empty.control).unwrap().len(), 0);
}

#[test]
fn slider_range_registered() {
    let mut builder = SettingsPageBuilder::new(0);
    let row = builder.slider(text("level"), -2, 9);
    assert_eq!(builder.sliders().get(&row.slider), Some(&(-2, 9)));
}

#[test]
fn notes_registered_for_notes_and_radio_hints_only() {
    let mut builder = SettingsPageBuilder::new(0);
    let note = builder.note(Some(text("note")), true);
    builder.radio(&[text("a"), text("b")], Some(&[Some(text("hint")), None]), 0);
    builder.warning(Some(text("warning")));
    assert_eq!(builder.notes().iter().copied().collect::<Vec<_>>(), vec![note, 4]);
}

#[test]
fn warning_and_key_boxes_registered() {
    let mut builder = SettingsPageBuilder::new(0);
    let warning = builder.warning(Some(text("warning")));
    let _cells = builder.key_grid(&[]);
    assert_eq!(
        builder.warning_texts().iter().copied().collect::<Vec<_>>(),
        vec![warning]
    );
    assert!(builder.key_boxes().is_empty());
}

#[test]
fn key_grid_cells_have_clear_only_when_clearable_and_the_glyph_is_u00d7() {
    let mut builder = SettingsPageBuilder::new(0);
    let cells = builder.key_grid(&[
        pr_platform_win::dialogs::settings::builder::layout::SettingsKeyCellSpec {
            label: text("first"),
            clearable: true,
        },
        pr_platform_win::dialogs::settings::builder::layout::SettingsKeyCellSpec {
            label: text("second"),
            clearable: false,
        },
    ]);
    assert_ne!(cells[0].clear, 0);
    assert_eq!(cells[1].clear, 0);
    assert_eq!(
        builder
            .items()
            .iter()
            .find(|item| item.id == cells[0].clear)
            .unwrap()
            .text,
        "×"
    );
}

#[test]
fn max_controls_is_999_the_1000th_records_too_many_controls() {
    let mut builder = SettingsPageBuilder::new(0);
    for _ in 0..999 {
        builder.text(None);
    }
    assert_eq!(builder.text(None), 0);
    assert_eq!(builder.error(), Some(SettingsBuilderError::TooManyControls));
}

#[test]
fn a_failed_combo_keeps_the_label_item_and_adds_no_element() {
    let mut builder = SettingsPageBuilder::new(0);
    for _ in 0..998 {
        builder.text(None);
    }
    let before = builder.elements().len();
    assert_eq!(builder.combo(text("last label"), None, 0).control, 0);
    assert_eq!(builder.items().len(), 999);
    assert_eq!(builder.elements().len(), before);
}

#[test]
fn a_rejected_radio_changes_nothing_and_keeps_a_pending_cut() {
    let mut builder = SettingsPageBuilder::new(0);
    builder.radio(&[text("a"), text("b")], None, 0);
    let before = builder.items().len();
    let rejected_ids = builder.radio(&[text("only")], None, 0);
    assert_eq!(rejected_ids, vec![0]);
    assert_eq!(builder.items().len(), before);
    let next = builder.check(text("next"), 0);
    assert_ne!(
        builder.items().iter().find(|item| item.id == next).unwrap().style & 0x20000,
        0
    );
}

#[test]
fn errors_are_recorded_in_order_and_error_is_the_first() {
    let mut builder = SettingsPageBuilder::new(0);
    builder.radio(&[], None, 0);
    builder.radio(&[text("a"), text("b")], Some(&[]), 0);
    assert_eq!(
        builder.errors(),
        &[
            SettingsBuilderError::RadioNeedsTwoOptions,
            SettingsBuilderError::RadioHintCountMismatch
        ]
    );
    assert_eq!(builder.error(), Some(SettingsBuilderError::RadioNeedsTwoOptions));
}

#[test]
fn set_group_visible_group_visible_and_flatten_find_nested_groups() {
    let mut builder = SettingsPageBuilder::new(0);
    let outer = builder.group(text("outer"), |builder| {
        builder.group(text("inner"), |_| {});
    });
    assert_eq!(builder.group_visible(outer), Some(true));
    assert!(builder.set_group_visible(outer, false));
    assert_eq!(builder.group_visible(outer), Some(false));
    assert!(!builder.flatten().is_empty());
}

#[test]
fn flatten_is_pre_order() {
    let mut builder = SettingsPageBuilder::new(10);
    let group = builder.group(text("group"), |builder| {
        builder.check(text("inside"), 0);
    });
    let flat = builder.flatten();
    assert_eq!(flat.first().unwrap().control_ids(), vec![group, group + 1]);
}

#[test]
fn elements_mut_allows_toggling_visibility() {
    let mut builder = SettingsPageBuilder::new(0);
    builder.group(text("group"), |_| {});
    builder.elements_mut()[0].set_visible(false);
    assert!(!builder.elements()[0].visible());
}

#[test]
fn text_provider_is_called_again_on_each_use() {
    let calls = Rc::new(std::cell::Cell::new(0));
    let observed = calls.clone();
    let provider: TextFn = Rc::new(move || {
        observed.set(observed.get() + 1);
        observed.get().to_string()
    });
    let mut builder = SettingsPageBuilder::new(0);
    let id = builder.note(Some(provider), true);
    assert_eq!(builder.items()[0].text, "1");
    assert_eq!((builder.texts()[&id])(), "2");
}

#[test]
fn key_grid_row_is_as_tall_as_its_tallest_wrapped_label_and_boxes_align_in_columns() {
    let cells = vec![
        SettingsKeyCell {
            label: 1,
            box_id: 2,
            clear: 3,
        },
        SettingsKeyCell {
            label: 4,
            box_id: 5,
            clear: 0,
        },
        SettingsKeyCell {
            label: 6,
            box_id: 7,
            clear: 8,
        },
        SettingsKeyCell {
            label: 9,
            box_id: 10,
            clear: 0,
        },
    ];
    let element = KeyGridElement::new(cells);
    assert_eq!(element.control_ids(), vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
}
