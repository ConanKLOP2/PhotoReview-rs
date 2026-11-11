//! D-10 / C-28 mutation killers for the Settings window glue (`context.rs`, `dialog/host.rs`), found by the 2026-11-10 time-boxed run
//! (`docs/reviews/D10-SETTINGS-GLUE-2026-11-10.md`). Pure: no window, no real browser (only rejected URLs are passed).
#![cfg(windows)]
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;
#[path = "common/settings_host_fixture.rs"]
mod host_fixture;

use pr_platform_win::dialogs::settings::context::SettingsPageHost;
use pr_platform_win::dialogs::settings::dialog::host::{default_open_url, open_url_info};

// Kills context.rs:82:9 `show_external_editors -> Ok(None)`: a host that does not wire the editors dialog answers `Err` (C# `ShowExternalEditors == null`), never "cancelled".
#[test]
fn show_external_editors_default_is_unavailable_not_cancelled() {
    let host = host_fixture::RecordingHost::new();
    assert!(host.show_external_editors(0, &[], None).is_err());
}

// Kills host.rs:34:5 `default_open_url -> Ok(())` and host.rs:41:8 `delete !` for the rejected inputs; none of these reaches ShellExecute.
#[test]
fn default_open_url_rejects_empty_and_non_web_urls_with_the_reason() {
    assert_eq!(default_open_url(""), Err("empty url".to_owned()));
    assert_eq!(default_open_url("   "), Err("empty url".to_owned()));
    assert_eq!(default_open_url("not a url"), Err("unsupported url scheme".to_owned()));
    assert_eq!(default_open_url("http://"), Err("unsupported url scheme".to_owned()));
}

// Kills frame.rs:149:9, :171:9, :365:9 (`Debug::fmt -> Ok(Default::default())` writes nothing) and :190:9 (the same for `Display`).
#[test]
fn settings_dialog_debug_and_error_texts_are_written() {
    let options = pr_platform_win::dialogs::settings::dialog::SettingsDialogOptions::default();
    assert!(format!("{options:?}").starts_with("SettingsDialogOptions"));
    assert!(format!("{:?}", fixture::args()).starts_with("SettingsDialogArgs"));
    let mut args = fixture::args();
    args.options.pages = Some(Vec::new());
    let Err(error) = pr_platform_win::dialogs::settings::dialog::SettingsDialog::new(args) else {
        panic!("an empty page list must be refused");
    };
    assert_eq!(
        error.to_string(),
        format!(
            "A settings page is required for every SettingsPresenter.Pages entry (expected {}, got 0).",
            7
        )
    );
    let dialog = fixture::dialog();
    assert!(format!("{:?}", dialog.content()).starts_with("SettingsContent"));
}

// P-404: kills host.rs field-deletion mutants of the SHELLEXECUTEINFOW (cbSize, fMask, lpVerb, lpFile, nShow). Builds the struct only; ShellExecuteExW is never called.
#[test]
fn open_url_info_sets_every_shell_execute_field() {
    use windows::Win32::UI::Shell::{
        SEE_MASK_FLAG_DDEWAIT, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let file: Vec<u16> = "https://example.test/x".encode_utf16().chain(Some(0)).collect();
    let info = open_url_info(&verb, &file);
    assert_eq!(info.cbSize as usize, std::mem::size_of::<SHELLEXECUTEINFOW>());
    assert!(info.cbSize > 0);
    assert_eq!(info.fMask, SEE_MASK_FLAG_NO_UI | SEE_MASK_FLAG_DDEWAIT);
    assert_eq!(
        info.fMask & SEE_MASK_NOCLOSEPROCESS,
        0,
        "no hProcess may be requested (handle leak)"
    );
    assert!(std::ptr::eq(info.lpVerb.0, verb.as_ptr()));
    assert!(std::ptr::eq(info.lpFile.0, file.as_ptr()));
    assert_eq!(info.nShow, SW_SHOWNORMAL.0);
    assert!(info.lpParameters.is_null() && info.lpDirectory.is_null());
    assert!(info.hwnd.is_invalid() && info.hProcess.is_invalid());
}

// ---- inputs.rs killers (builder/inputs.rs) ----
use pr_platform_win::dialogs::settings::builder::inputs::{MAX_CONTROLS, SettingsBuilderError, SettingsPageBuilder};
use pr_platform_win::dialogs::settings::builder::layout::SettingsKeyCellSpec;
use pr_platform_win::dialogs::settings::page_view::TextFn;

fn txt(value: &'static str) -> TextFn {
    std::rc::Rc::new(move || value.to_owned())
}

fn style_of(builder: &SettingsPageBuilder, id: i32) -> u32 {
    builder.items().iter().find(|item| item.id == id).expect("item").style
}

// Kills inputs.rs:46:9 `Display::fmt -> Ok(Default::default())`.
#[test]
fn settings_builder_error_display_texts_are_exact() {
    assert_eq!(
        SettingsBuilderError::TooManyControls.to_string(),
        "A settings page can declare at most 999 controls."
    );
    assert_eq!(
        SettingsBuilderError::RadioNeedsTwoOptions.to_string(),
        "A radio group needs at least two options."
    );
    assert_eq!(
        SettingsBuilderError::RadioHintCountMismatch.to_string(),
        "There must be one hint (or null) per radio option."
    );
}

// Kills inputs.rs:75:9 `Debug::fmt -> Ok(Default::default())`.
#[test]
fn settings_page_builder_debug_names_type_id_base_and_item_count() {
    let mut builder = SettingsPageBuilder::new(4321);
    builder.check(txt("a"), 0);
    builder.check(txt("b"), 0);
    let text = format!("{builder:?}");
    assert!(text.contains("SettingsPageBuilder"), "{text}");
    assert!(text.contains("id_base: 4321"), "{text}");
    assert!(text.contains("items: 2"), "{text}");
}

fn fill_to(builder: &mut SettingsPageBuilder, count: i32) {
    for _ in 0..count {
        builder.check(txt("x"), 0);
    }
}

// Kills inputs.rs:432:12 `delete !` in check_flow: success adds one element, a budget failure adds none.
#[test]
fn check_flow_pushes_its_element_only_on_success() {
    let mut ok = SettingsPageBuilder::new(1000);
    let ids = ok.check_flow(&[txt("a"), txt("b")], 0);
    assert_eq!(ids, vec![1001, 1002]);
    assert_eq!(ok.elements().len(), 1);
    assert_eq!(ok.flatten().len(), 1);

    let mut failing = SettingsPageBuilder::new(1000);
    fill_to(&mut failing, MAX_CONTROLS - 1);
    let before = failing.elements().len();
    let ids = failing.check_flow(&[txt("a"), txt("b")], 0);
    assert_eq!(ids, vec![1999, 0]);
    assert_eq!(
        failing.elements().len(),
        before,
        "a failed check_flow must add no element"
    );
    assert_eq!(failing.error(), Some(SettingsBuilderError::TooManyControls));
}

// Kills inputs.rs:494:12 `delete !` in key_grid: success adds one element, a budget failure adds none.
#[test]
fn key_grid_pushes_its_element_only_on_success() {
    let spec = || SettingsKeyCellSpec {
        label: txt("k"),
        clearable: true,
    };
    let mut ok = SettingsPageBuilder::new(1000);
    let cells = ok.key_grid(&[spec()]);
    assert_eq!(cells.len(), 1);
    assert_eq!(ok.elements().len(), 1);

    let mut failing = SettingsPageBuilder::new(1000);
    fill_to(&mut failing, MAX_CONTROLS - 1);
    let before = failing.elements().len();
    let cells = failing.key_grid(&[spec()]);
    assert!(cells.is_empty());
    assert_eq!(
        failing.elements().len(),
        before,
        "a failed key_grid must add no element"
    );
    assert_eq!(failing.error(), Some(SettingsBuilderError::TooManyControls));
}

// Kills inputs.rs:507:34 `==`->`!=`, 507:73 `&`->`|` and `&`->`^`: a pending break_group skips the group box and lands on the next non-group control.
#[test]
fn pending_break_group_skips_group_box_then_marks_next_control() {
    const WS_GROUP: u32 = 0x0002_0000;
    let mut builder = SettingsPageBuilder::new(2000);
    builder.radio(&[txt("one"), txt("two")], None, 0);
    let mut inner = 0;
    let group_id = builder.group(txt("box"), |b| inner = b.check(txt("in"), 0));
    let after = builder.check(txt("after"), 0);
    assert_eq!(style_of(&builder, group_id) & 0x0f, 7, "group box keeps BS_GROUPBOX");
    assert_eq!(
        style_of(&builder, group_id) & WS_GROUP,
        0,
        "group box must not get WS_GROUP"
    );
    assert_ne!(
        style_of(&builder, inner) & WS_GROUP,
        0,
        "first control after the box opens the group"
    );
    assert_eq!(style_of(&builder, after) & WS_GROUP, 0, "flag is consumed once");
}

// Kills inputs.rs:507:34/73 from the other side: a non-group Button and a non-Button control both take WS_GROUP.
#[test]
fn pending_break_group_marks_check_button_and_static_control() {
    const WS_GROUP: u32 = 0x0002_0000;
    let mut button = SettingsPageBuilder::new(3000);
    button.radio(&[txt("one"), txt("two")], None, 0);
    let check = button.check(txt("c"), 0);
    assert_ne!(style_of(&button, check) & WS_GROUP, 0);

    let mut other = SettingsPageBuilder::new(3000);
    other.radio(&[txt("one"), txt("two")], None, 0);
    let label = other.text(Some(txt("t")));
    assert_ne!(style_of(&other, label) & WS_GROUP, 0);
}

// Item 6: the `|`->`^` mutants at the style-constant sites are equivalent because every OR-ed operand set is pairwise disjoint.
#[test]
fn style_operand_sets_used_by_the_builder_are_pairwise_disjoint() {
    use pr_platform_win::dialogs::native::controls::basic::ComboBoxControl;
    use pr_platform_win::dialogs::native::template::DIALOG_STYLE_CHILD;
    use pr_platform_win::win32::control_constants::{
        BS_AUTOCHECKBOX, BS_AUTORADIOBUTTON, BS_GROUPBOX, ES_READONLY, TBS_NOTICKS,
    };
    use pr_platform_win::win32::dialog_chrome::BS_MULTILINE;
    use pr_platform_win::win32::dialog_native::{BS_PUSHBUTTON, ES_AUTOHSCROLL, SS_NOPREFIX, WS_GROUP, WS_TABSTOP};
    let c = DIALOG_STYLE_CHILD;
    let sets: &[&[u32]] = &[
        &[c, BS_GROUPBOX],
        &[c, WS_TABSTOP, BS_AUTOCHECKBOX, BS_MULTILINE],
        &[c, SS_NOPREFIX],
        &[c, ComboBoxControl::DEFAULT_STYLE],
        &[c, WS_TABSTOP, ES_AUTOHSCROLL],
        &[c, WS_TABSTOP, TBS_NOTICKS],
        &[c, WS_TABSTOP, BS_PUSHBUTTON],
        &[c, BS_AUTORADIOBUTTON, BS_MULTILINE],
        &[WS_GROUP, WS_TABSTOP],
        &[c, WS_TABSTOP, BS_AUTOCHECKBOX],
        &[c, WS_TABSTOP, ES_AUTOHSCROLL, ES_READONLY],
    ];
    for set in sets {
        let mut seen = 0u32;
        for operand in *set {
            assert_eq!(seen & operand, 0, "overlapping operands in {set:x?}");
            seen |= operand;
        }
    }
}

// ---- context.rs / builder layout killers ----
use pr_platform_win::dialogs::settings::builder::groups::CheckFlowElement;
use pr_platform_win::dialogs::settings::builder::rows::{ButtonsElement, SettingsLayoutElement, SettingsLayoutSurface};
use pr_platform_win::dialogs::settings::context::SettingsPageContext;

// Kills context.rs:140:9 `Debug::fmt -> Ok(Default::default())` (writes nothing).
#[test]
fn settings_page_context_debug_names_the_type() {
    let text = format!("{:?}", SettingsPageContext::detached());
    assert!(text.starts_with("SettingsPageContext"), "{text}");
}

// context.rs:220:55 (`unwrap_or(-1)` -> `unwrap_or(1)`), :404:9 (`hwnd -> Default::default()`), :419:9 (3 variants) and :426:9 (`Ok(None)`) are
// equivalent, see the report: the fallback needs > i32::MAX ids, Hwnd = isize defaults to the same 0, and the detached `warn` is a no-op.
// The unchanged observable contract is pinned here.
#[test]
fn detached_context_answers_the_documented_defaults() {
    let ctx = SettingsPageContext::detached();
    assert_eq!(ctx.owner(), 0);
    assert_eq!(ctx.radio_index(&[1, 2, 3]), -1);
    assert!(ctx.edit_action_profiles(&[]).is_none());
}

/// Fixed-metric surface: dx/dy identity, every label 0 wide, so a check-flow item is 16 wide and a button 50 wide.
struct FlatSurface {
    placed: Vec<(i32, i32, i32)>,
}

impl SettingsLayoutSurface for FlatSurface {
    fn dx(&self, n: i32) -> i32 {
        n
    }
    fn dy(&self, n: i32) -> i32 {
        n
    }
    fn line_height(&self) -> i32 {
        16
    }
    fn field_height(&self) -> i32 {
        23
    }
    fn text_of(&self, _id: i32) -> String {
        String::new()
    }
    fn text_height(&self, _id: i32, _width: i32) -> i32 {
        16
    }
    fn text_width(&self, _text: &str) -> i32 {
        0
    }
    fn is_hidden(&self, _id: i32) -> bool {
        false
    }
    fn combo_drop_height(&self, _id: i32) -> i32 {
        80
    }
    fn place(&mut self, id: i32, x: i32, y: i32, _w: i32, _h: i32) {
        self.placed.push((id, x, y));
    }
    fn hide(&mut self, _id: i32) {}
}

// Kills groups.rs:234:41 `>` -> `>=`: second item ends exactly at `right` (26 + 16 == 42) and must stay on the first row.
#[test]
fn check_flow_item_ending_exactly_at_the_limit_stays_on_the_row() {
    let mut surface = FlatSurface { placed: Vec::new() };
    let used = CheckFlowElement::new(vec![1, 2], 0).place(&mut surface, 0, 0, 42);
    assert_eq!(surface.placed, vec![(1, 0, 0), (2, 26, 0)]);
    assert_eq!(used, 23, "a single row");
    let mut narrow = FlatSurface { placed: Vec::new() };
    CheckFlowElement::new(vec![1, 2], 0).place(&mut narrow, 0, 0, 41);
    assert_eq!(narrow.placed, vec![(1, 0, 0), (2, 0, 22)], "one unit narrower wraps");
}

// Kills rows.rs:333:41 `>` -> `>=`: second button ends exactly at `right` (56 + 50 == 106) and must stay on the first row.
#[test]
fn buttons_item_ending_exactly_at_the_limit_stays_on_the_row() {
    let mut surface = FlatSurface { placed: Vec::new() };
    let used = ButtonsElement::new(vec![1, 2], false).place(&mut surface, 0, 0, 106);
    assert_eq!(surface.placed, vec![(1, 0, 0), (2, 56, 0)]);
    assert_eq!(used, 28, "a single row");
    let mut narrow = FlatSurface { placed: Vec::new() };
    ButtonsElement::new(vec![1, 2], false).place(&mut narrow, 0, 0, 105);
    assert_eq!(narrow.placed, vec![(1, 0, 0), (2, 0, 27)], "one unit narrower wraps");
}
