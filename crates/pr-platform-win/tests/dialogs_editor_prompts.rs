//! W9-E3: source-derived pure checks for ExternalEditorProductName and TextPromptDialog (C# 7eaee2a2).
use pr_platform_win::dialogs::native::external_editor_product_name::first_non_blank;
use pr_platform_win::dialogs::native::text_prompt_dialog as prompt;
use pr_platform_win::dialogs::zoom_prompt::{DialogMeasure, Placement};

// C#: ListDialogContent.ForChoice has no heading and retains selected index and list order.
#[test]
fn choice_content_preserves_selection_and_copies_lines_with_choice_labels() {
    let mut warnings = Vec::new();
    let english = pr_l10n::catalog::LanguageCatalog::try_parse(
        include_str!("../../../assets/languages/en.json"),
        "en.json",
        &mut warnings,
    )
    .unwrap();
    let localizer = pr_l10n::localizer::Localizer::create(&english, &[], &warnings);
    let mut lines = vec!["First".to_owned(), "Second".to_owned()];
    let content = pr_platform_win::dialogs::native::list_dialog::ListDialogContent::for_choice(
        "Choose",
        "An editor",
        &lines,
        1,
        &localizer,
    );
    lines.clear();
    assert_eq!(content.title, "Choose");
    assert_eq!(content.heading, None);
    assert_eq!(content.intro.as_deref(), Some("An editor"));
    assert_eq!(content.intro_color_key, "Dark.TextSecondary");
    assert_eq!(content.lines, ["First", "Second"]);
    assert_eq!(content.initial_selected_index, 1);
    assert_eq!((content.initial_width_dlu, content.initial_height_dlu), (260, 150));
    assert_eq!(
        content.confirm_text,
        Some(localizer.get("dialog.externalEditor.setDefaultConfirm"))
    );
    assert_eq!(content.cancel_text, localizer.get("common.cancel"));
}

#[test]
fn first_non_blank_product_name_wins_else_description_trimmed_else_none() {
    assert_eq!(
        first_non_blank(Some(" Product "), Some("Description")),
        Some("Product".to_owned())
    );
    assert_eq!(
        first_non_blank(Some(" \t"), Some(" Description ")),
        Some("Description".to_owned())
    );
    assert_eq!(
        first_non_blank(None, Some(" Description ")),
        Some("Description".to_owned())
    );
    assert_eq!(first_non_blank(Some("\n"), Some("\t")), None);
    assert_eq!(first_non_blank(None, None), None);
}

struct Measure;
impl DialogMeasure for Measure {
    fn dx(&self, n: i32) -> i32 {
        n * 2
    }
    fn dy(&self, n: i32) -> i32 {
        n * 3
    }
    fn line_height(&self) -> i32 {
        20
    }
    fn measure_item_height(&self, id: i32, width: i32) -> i32 {
        assert_eq!(id, prompt::LABEL_ID);
        assert!(width > 0);
        17
    }
    fn measure_text_width(&self, _: &str) -> i32 {
        panic!("not used by this layout")
    }
    fn measure_text_height(&self, _: &str, _: i32) -> i32 {
        panic!("not used by this layout")
    }
    fn item_text(&self, _: i32) -> String {
        panic!("not used by this layout")
    }
    fn button_width(&self, _: &[i32]) -> i32 {
        panic!("not used by this layout")
    }
    fn button_row_width(&self, ids: &[i32]) -> i32 {
        assert_eq!(ids, [prompt::CANCEL_ID, prompt::APPLY_ID]);
        600
    }
}

#[test]
fn text_prompt_layout_keeps_wrapped_label_and_wide_button_row() {
    assert_eq!(prompt::control_height(&Measure), 42);
    assert_eq!(prompt::preferred_client_size(&Measure), (648, 203));
    let plan = prompt::layout_plan(&Measure, 700);
    assert_eq!(
        plan.placements,
        vec![
            Placement::new(prompt::LABEL_ID, 24, 30, 652, 17),
            Placement::new(prompt::EDIT_ID, 24, 59, 652, 42)
        ]
    );
    assert_eq!(plan.button_row.client_width, 700);
    assert_eq!(plan.button_row.right_margin, 24);
    assert_eq!(plan.button_row.top, 131);
    assert_eq!(plan.button_row.height, 42);
    assert_eq!(plan.button_row.ids, [prompt::CANCEL_ID, prompt::APPLY_ID]);
}

#[test]
fn text_prompt_items_keep_tab_order_initial_empty_edit_and_default_apply() {
    let items = prompt::build_items("Name", "Cancel", "Apply");
    assert_eq!(
        items.iter().map(|item| item.id).collect::<Vec<_>>(),
        [prompt::LABEL_ID, prompt::EDIT_ID, prompt::CANCEL_ID, prompt::APPLY_ID]
    );
    assert_eq!(
        items.iter().map(|item| item.text.as_str()).collect::<Vec<_>>(),
        ["Name", "", "Cancel", "Apply"]
    );
    assert_eq!(items[1].ex_style, 0x200);
    assert_eq!(items[3].style & 0xf, 1);
}
