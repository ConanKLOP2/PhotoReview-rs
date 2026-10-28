//! W9-R11 (FEAT-RENAME, native part): the Win32 name box of "Rename..." / F2 on a REAL window (modal, STA thread, hidden behind a test-owned owner).
//! Port of `tests/PhotoReview.Shell.Tests/Dialogs/Native/RenamePromptDialogTests.cs` at C# `7eaee2a2` (the pixel/menu theory `ContextMenu_RenameEntry_IsPaintedAndTakesARow`
//! lives in `rename_shell_e2e.rs`, which owns a main window). Contract: `contracts/W9-R11.md`.
//!
//! The real policy is wired through `RenamePromptPresenter`, so the tables below are the user's view of `RenameNamePolicy`. The user is simulated with `WM_COMMAND`,
//! `SetWindowText` and `IsDialogMessage`; nothing moves the real cursor or takes the focus. Every test is `#[ignore = "native: ..."]` and owns its windows.
#![cfg(windows)]

#[path = "common/w9_e2_capture.rs"]
mod capture;
#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use fixture::{
    child_rect, client_rect, command, drain, height, item, key, needed_text_height, on_sta, set_text, show_on_owner,
    text_of, width,
};
use pr_app::presenters::rename_prompt::{NameTaken, RenameCheck, RenamePromptPresenter};
use pr_core::abstractions::RenamePromptRequest;
use pr_core::file_actions::rename::RenameNamePolicy;
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;
use pr_platform_win::dialogs::native::dialog::{ID_CANCEL, ID_OK};
use pr_platform_win::dialogs::rename_prompt::RenamePromptDialog;
use pr_platform_win::win32::structs::Rect;
use pr_platform_win::win32::{api, dialog_native, messages};

const CURRENT_PATH: &str = r"C:\Photos\Trip\IMG_0042.JPG";

/// C# `Request(taken)`: the presenter over `RenameNamePolicy.Check(typed, CurrentPath)` and an optional name-taken question.
fn request(taken: Option<NameTaken>) -> RenamePromptRequest {
    #[allow(clippy::arc_with_non_send_sync)] // the request type is Arc-based; the tests use it on one STA thread
    let check: RenameCheck = Arc::new(|typed| RenameNamePolicy::check(Some(typed), CURRENT_PATH, false));
    RenamePromptPresenter::new(CURRENT_PATH.to_owned(), check, taken)
        .expect("a source path")
        .to_request()
}

/// C# `Show(request, body)`: runs `body` inside the modal box and returns the dialog result and the value.
fn show(
    request: RenamePromptRequest,
    body: impl Fn(&RenamePromptDialog, &pr_platform_win::dialogs::native::dialog::NativeDialog),
) -> (i32, Option<String>) {
    let prompt = RenamePromptDialog::new(request, None);
    let result = show_on_owner(prompt.native(), |dialog, _| body(&prompt, dialog)).expect("the dialog ran");
    (result, prompt.value())
}

fn message(key: &str) -> String {
    Localizer::current().get(key)
}

// ---- behaviour --------------------------------------------------------------------------------------------------------------------------------

// C#: RenamePromptDialogTests.Opens_WithTheStemSelected_TheExtensionFixedBesideIt_AndNothingToComplainAbout
#[test]
#[ignore = "native: needs desktop"]
fn opens_with_the_stem_selected_the_extension_fixed_beside_it_and_nothing_to_complain_about() {
    let (result, _) = on_sta(|| {
        show(request(None), |prompt, d| {
            assert!(api::user32::is_window_visible(d.hwnd()));
            assert_eq!(text_of(d.hwnd()), pr_l10n::tr::dialog_rename_title());
            assert_eq!(
                text_of(item(d, RenamePromptDialog::LABEL_ID)),
                pr_l10n::tr::dialog_rename_label()
            );
            assert_eq!(prompt.current_text(), "IMG_0042", "no extension in the box");
            assert_eq!(prompt.selection(), (0, 8), "the whole name part is selected");
            assert_eq!(prompt.extension_text(), ".JPG", "the extension is shown, not editable");
            assert!(fixture::is_visible(d, RenamePromptDialog::EXTENSION_ID));
            assert_eq!(dialog_native::user32::get_focus(), item(d, RenamePromptDialog::EDIT_ID));
            assert_eq!(
                text_of(item(d, RenamePromptDialog::RENAME_ID)),
                pr_l10n::tr::dialog_rename_ok()
            );
            assert!(prompt.is_rename_enabled(), "the unchanged name is not an error");
            assert_eq!(prompt.error_message(), None);
            assert!(!fixture::is_visible(d, RenamePromptDialog::ERROR_ID));
            command(d, ID_CANCEL, 0);
        })
    });
    assert_eq!(result, ID_CANCEL);
}

// C#: RenamePromptDialogTests.AFileWithoutAnExtension_ShowsNoExtensionLabelText
#[test]
#[ignore = "native: needs desktop"]
fn a_file_without_an_extension_shows_no_extension_label_text() {
    on_sta(|| {
        #[allow(clippy::arc_with_non_send_sync)] // the request type is Arc-based; the tests use it on one STA thread
        let validate: pr_core::abstractions::RenameValidateFn = Arc::new(|_, _| None);
        let request = RenamePromptRequest {
            initial_name: "README".to_owned(),
            extension: String::new(),
            validate,
        };
        show(request, |prompt, d| {
            assert_eq!(prompt.current_text(), "README");
            assert_eq!(prompt.extension_text(), "");
            command(d, ID_CANCEL, 0);
        });
    });
}

// C#: RenamePromptDialogTests.InvalidNames_AreRefusedInPlaceWhileTyping_OkIsDisabled_AndEnterDoesNothing (the 16 InlineData rows).
#[test]
#[ignore = "native: needs desktop"]
fn invalid_names_are_refused_in_place_while_typing_ok_is_disabled_and_enter_does_nothing() {
    const CASES: [(&str, &str); 16] = [
        ("", "dialog.rename.error.empty"),
        ("   ", "dialog.rename.error.empty"),
        ("a?b", "dialog.rename.error.invalidCharacter"),
        ("a:b", "dialog.rename.error.invalidCharacter"),
        ("a*b", "dialog.rename.error.invalidCharacter"),
        ("a\"b", "dialog.rename.error.invalidCharacter"),
        ("a<b", "dialog.rename.error.invalidCharacter"),
        ("a>b", "dialog.rename.error.invalidCharacter"),
        ("a|b", "dialog.rename.error.invalidCharacter"),
        ("a/b", "dialog.rename.error.invalidCharacter"),
        ("a\\b", "dialog.rename.error.invalidCharacter"),
        ("name.", "dialog.rename.error.trailingDot"),
        ("CON", "dialog.rename.error.reservedName"),
        ("nul", "dialog.rename.error.reservedName"),
        ("COM1", "dialog.rename.error.reservedName"),
        ("lpt9", "dialog.rename.error.reservedName"),
    ];
    on_sta(|| {
        for (typed, key_name) in CASES {
            let expected = message(key_name);
            let (result, value) = show(request(None), |prompt, d| {
                set_text(d, RenamePromptDialog::EDIT_ID, typed);
                assert!(!prompt.is_rename_enabled(), "'{typed}' must disable OK");
                assert_eq!(prompt.error_message().as_deref(), Some(expected.as_str()), "'{typed}'");
                assert!(fixture::is_visible(d, RenamePromptDialog::ERROR_ID));
                assert_eq!(text_of(item(d, RenamePromptDialog::ERROR_ID)), expected);
                // Enter reaches OK through the dialog manager but the button is disabled: the box stays open, nothing is returned.
                key(d, item(d, RenamePromptDialog::EDIT_ID), fixture::VK_RETURN);
                drain(d);
                assert!(dialog_native::user32::is_window(d.hwnd()));
                command(d, ID_OK, 0); // even a direct OK command re-validates
                assert!(dialog_native::user32::is_window(d.hwnd()));
                command(d, ID_CANCEL, 0);
            });
            assert_eq!(result, ID_CANCEL, "'{typed}'");
            assert_eq!(value, None, "'{typed}'");
        }
    });
}

// C#: RenamePromptDialogTests.ATooLongName_IsRefused_AndTheErrorGoesWhenTheNameIsFixed
#[test]
#[ignore = "native: needs desktop"]
fn a_too_long_name_is_refused_and_the_error_goes_when_the_name_is_fixed() {
    let (result, value) = on_sta(|| {
        show(request(None), |prompt, d| {
            set_text(d, RenamePromptDialog::EDIT_ID, &"x".repeat(256));
            assert!(!prompt.is_rename_enabled());
            assert_eq!(
                prompt.error_message().as_deref(),
                Some(message("dialog.rename.error.tooLong").as_str())
            );
            set_text(d, RenamePromptDialog::EDIT_ID, &"x".repeat(200));
            assert!(prompt.is_rename_enabled());
            assert_eq!(prompt.error_message(), None);
            assert!(!fixture::is_visible(d, RenamePromptDialog::ERROR_ID));
            command(d, ID_OK, 0);
        })
    });
    assert_eq!(result, ID_OK);
    assert_eq!(value, Some("x".repeat(200)));
}

// C#: RenamePromptDialogTests.ValidNames_EnableOk_AndAreReturnedTrimmed (the five InlineData rows).
#[test]
#[ignore = "native: needs desktop"]
fn valid_names_enable_ok_and_are_returned_trimmed() {
    on_sta(|| {
        for typed in ["holiday 01", "a.v2", "Ảnh đẹp ★", "IMG_0042", "img_0042"] {
            let (result, value) = show(request(None), |prompt, d| {
                set_text(d, RenamePromptDialog::EDIT_ID, &format!("  {typed}  "));
                assert!(prompt.is_rename_enabled(), "'{typed}'");
                assert_eq!(prompt.error_message(), None, "'{typed}'");
                command(d, ID_OK, 0);
            });
            assert_eq!(result, ID_OK, "'{typed}'");
            assert_eq!(value.as_deref(), Some(typed));
        }
    });
}

// C#: RenamePromptDialogTests.TheExtensionIsKept_TheTypedTextIsOnlyTheNamePart
#[test]
#[ignore = "native: needs desktop"]
fn the_extension_is_kept_the_typed_text_is_only_the_name_part() {
    let (_, value) = on_sta(|| {
        show(request(None), |_, d| {
            set_text(d, RenamePromptDialog::EDIT_ID, "sunset");
            command(d, ID_OK, 0);
        })
    });
    let value = value.expect("a typed name");
    assert_eq!(value, "sunset");
    // What the caller does with the returned text: the policy appends the ORIGINAL extension (and drops a typed copy of it).
    let name = |typed: &str| RenameNamePolicy::check(Some(typed), CURRENT_PATH, false).new_file_name;
    assert_eq!(name(&value), "sunset.JPG");
    assert_eq!(name("sunset.jpg"), "sunset.JPG");
    assert_eq!(name("a.v2"), "a.v2.JPG");
}

// C#: RenamePromptDialogTests.ANameThatIsTaken_IsRefusedWhileTypingForTheListedOnes_AndOnTheConfirmingCallForTheOthers
#[test]
#[ignore = "native: needs desktop"]
fn a_name_that_is_taken_is_refused_while_typing_for_the_listed_ones_and_on_the_confirming_call_for_the_others() {
    let (result, value) = on_sta(|| {
        let checked_on_disk: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&checked_on_disk);
        #[allow(clippy::arc_with_non_send_sync)] // the question type is Arc-based; the tests use it on one STA thread
        let taken: NameTaken = Arc::new(move |file_name, is_commit| {
            if is_commit {
                log.borrow_mut().push(file_name.to_owned());
            }
            file_name.eq_ignore_ascii_case("IMG_0001.JPG") || (is_commit && file_name.eq_ignore_ascii_case("late.JPG"))
        });
        let exists = |name: &str| {
            Localizer::current().format("dialog.rename.error.exists", &[LocArg::new("name", name.to_owned())])
        };
        show(request(Some(taken)), |prompt, d| {
            set_text(d, RenamePromptDialog::EDIT_ID, "IMG_0001");
            assert_eq!(prompt.error_message(), Some(exists("IMG_0001.JPG")));
            assert!(!prompt.is_rename_enabled());
            assert!(checked_on_disk.borrow().is_empty(), "typing never asks the disk");

            set_text(d, RenamePromptDialog::EDIT_ID, "late");
            assert!(prompt.is_rename_enabled(), "only the confirming call can know");
            command(d, ID_OK, 0);
            assert!(
                dialog_native::user32::is_window(d.hwnd()),
                "the box stays open on the confirming refusal"
            );
            assert_eq!(prompt.error_message(), Some(exists("late.JPG")));
            assert!(!prompt.is_rename_enabled());
            assert_eq!(*checked_on_disk.borrow(), ["late.JPG"]);

            set_text(d, RenamePromptDialog::EDIT_ID, "later");
            assert!(prompt.is_rename_enabled());
            command(d, ID_OK, 0);
        })
    });
    assert_eq!(result, ID_OK);
    assert_eq!(value.as_deref(), Some("later"));
}

// C#: RenamePromptDialogTests.Enter_InTheBox_Renames_AndEscape_Cancels_ThroughTheDialogManager
#[test]
#[ignore = "native: needs desktop"]
fn enter_in_the_box_renames_and_escape_cancels_through_the_dialog_manager() {
    let ((enter, enter_value), (escape, escape_value)) = on_sta(|| {
        let enter = show(request(None), |_, d| {
            set_text(d, RenamePromptDialog::EDIT_ID, "kept");
            assert!(key(d, item(d, RenamePromptDialog::EDIT_ID), fixture::VK_RETURN));
            drain(d);
        });
        let escape = show(request(None), |_, d| {
            set_text(d, RenamePromptDialog::EDIT_ID, "kept");
            assert!(key(d, item(d, RenamePromptDialog::EDIT_ID), fixture::VK_ESCAPE));
            drain(d);
        });
        (enter, escape)
    });
    assert_eq!(enter, ID_OK);
    assert_eq!(enter_value.as_deref(), Some("kept"));
    assert_eq!(escape, ID_CANCEL);
    assert_eq!(escape_value, None);
}

// C#: RenamePromptDialogTests.TheErrorLineGrowsTheDialogAndShrinksItBack_NothingOverlapsOrLeavesTheClientArea
#[test]
#[ignore = "native: needs desktop"]
fn the_error_line_grows_the_dialog_and_shrinks_it_back_nothing_overlaps_or_leaves_the_client_area() {
    on_sta(|| {
        show(request(None), |_, d| {
            let before = client_rect(d.hwnd());
            set_text(d, RenamePromptDialog::EDIT_ID, "a?b");
            let with_error = client_rect(d.hwnd());
            assert!(height(with_error) > height(before), "the error line needs room");
            let edit = child_rect(d, RenamePromptDialog::EDIT_ID);
            let error = child_rect(d, RenamePromptDialog::ERROR_ID);
            let ok = child_rect(d, RenamePromptDialog::RENAME_ID);
            assert!(error.top >= edit.bottom, "the error is below the box");
            assert!(ok.top >= error.bottom, "the buttons are below the error");
            assert!(error.right <= with_error.right && ok.bottom <= with_error.bottom);
            assert!(needed_text_height(item(d, RenamePromptDialog::ERROR_ID)) <= height(error));

            set_text(d, RenamePromptDialog::EDIT_ID, "fine");
            assert_eq!(height(client_rect(d.hwnd())), height(before));
            command(d, ID_CANCEL, 0);
        });
    });
}

// C#: RenamePromptDialogTests.TheExtensionSitsRightOfTheBox_OnTheSameRow
#[test]
#[ignore = "native: needs desktop"]
fn the_extension_sits_right_of_the_box_on_the_same_row() {
    on_sta(|| {
        show(request(None), |_, d| {
            let edit = child_rect(d, RenamePromptDialog::EDIT_ID);
            let extension = child_rect(d, RenamePromptDialog::EXTENSION_ID);
            assert!(extension.left >= edit.right, "extension is to the right of the box");
            assert!(
                extension.top >= edit.top && extension.bottom <= edit.bottom,
                "same row as the box"
            );
            assert!(width(extension) > 0 && width(edit) > width(extension));
            command(d, ID_CANCEL, 0);
        });
    });
}

// ---- real pixels ------------------------------------------------------------------------------------------------------------------------------

struct RestoreLocalizer(Arc<Localizer>);
impl Drop for RestoreLocalizer {
    fn drop(&mut self) {
        Localizer::set_current(Arc::clone(&self.0));
    }
}

// C#: RenamePromptDialogTests.Dialog_PaintsTheBoxTheExtensionTheErrorAndTheButtons, the four rows (96 / 144 DPI x en / vi). Window-DC BitBlt, never PrintWindow.
#[test]
#[ignore = "native: real desktop pixels at 96/144 DPI, en/vi"]
fn dialog_paints_the_box_the_extension_the_error_and_the_buttons() {
    on_sta(|| {
        let _restore = RestoreLocalizer(Localizer::current());
        let mut warnings = Vec::new();
        let english = LanguageCatalog::try_parse(
            include_str!("../../../assets/languages/en.json"),
            "en.json",
            &mut warnings,
        )
        .expect("en catalog");
        let vietnamese = LanguageCatalog::try_parse(
            include_str!("../../../assets/languages/vi.json"),
            "vi.json",
            &mut warnings,
        )
        .expect("vi catalog");
        let measured = std::cell::Cell::new(0);
        for (language, overlays) in [("en", Vec::new()), ("vi", vec![vietnamese])] {
            Localizer::set_current(Arc::new(Localizer::create(&english, &overlays, &warnings)));
            for dpi in [96u32, 144] {
                let _styles = fixture::activate_visual_styles();
                show(request(None), |_, d| {
                    let frame = fixture::window_rect(d.hwnd());
                    let current = d.dpi() as i32;
                    let suggested = windows::Win32::Foundation::RECT {
                        left: frame.left,
                        top: frame.top,
                        right: frame.left + (frame.right - frame.left) * dpi as i32 / current,
                        bottom: frame.top + (frame.bottom - frame.top) * dpi as i32 / current,
                    };
                    // SAFETY: WM_DPICHANGED synchronously reads the live suggested RECT; the packed DPI has the same nonzero X and Y components.
                    unsafe {
                        dialog_native::user32::send_message(
                            d.hwnd(),
                            messages::WM_DPICHANGED,
                            ((dpi << 16) | dpi) as usize,
                            (&suggested as *const windows::Win32::Foundation::RECT) as isize,
                        );
                    }
                    assert_eq!(d.dpi(), dpi);
                    set_text(d, RenamePromptDialog::EDIT_ID, "a?b"); // shows the error line too
                    // A window-DC BitBlt returns BLACK for the part that is off screen (small screen at 144 DPI): measure at the origin.
                    let _ = api::user32::set_window_pos(
                        d.hwnd(),
                        0,
                        0,
                        0,
                        0,
                        0,
                        messages::SWP_NOSIZE | messages::SWP_NOZORDER | messages::SWP_NOACTIVATE,
                    );
                    drain(d);
                    let shot = capture::Shot::capture(d.hwnd());
                    shot.assert_not_flat(d.hwnd(), &format!("rename dialog {language} {dpi}"));
                    for (name, id) in [
                        ("label", RenamePromptDialog::LABEL_ID),
                        ("edit", RenamePromptDialog::EDIT_ID),
                        ("extension", RenamePromptDialog::EXTENSION_ID),
                        ("error", RenamePromptDialog::ERROR_ID),
                        ("cancel", RenamePromptDialog::CANCEL_ID),
                    ] {
                        let rect: Rect = fixture::window_rect(item(d, id));
                        assert!(rect.right > rect.left && rect.bottom > rect.top, "{name} has no area");
                        shot.assert_not_flat(item(d, id), &format!("rename dialog {name} {language} {dpi}"));
                        measured.set(measured.get() + 1);
                    }
                    command(d, ID_CANCEL, 0);
                });
            }
        }
        assert_eq!(measured.get(), 20);
    });
}
