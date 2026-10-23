//! Desktop checks for the list half of `DiagnosticsAndListDialogTests.cs`.

#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use fixture::{
    RecordingChrome, VK_ESCAPE, VK_RETURN, child_rect, client_rect, command, drain, gdi_objects, height, item, key,
    on_sta, settles_at_or_below, show_on_owner, text_of, user_objects, width,
};
use pr_core::abstractions::{BatchReviewItem, SkippedEntry, SkippedKind};
use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::dialog::{ID_CANCEL, ID_OK, NativeDialog};
use pr_platform_win::dialogs::native::list_dialog::{
    CANCEL_ID, CONFIRM_ID, HEADING_ID, INTRO_ID, LIST_ID, ListDialog, ListDialogContent,
};
use pr_platform_win::win32::{control_constants, dialog_native, messages};
use std::path::PathBuf;
use std::rc::Rc;

fn localizer(vietnamese: bool) -> pr_l10n::localizer::Localizer {
    let mut warnings = Vec::new();
    let english = pr_l10n::catalog::LanguageCatalog::try_parse(
        include_str!("../../../assets/languages/en.json"),
        "en.json",
        &mut warnings,
    )
    .unwrap();
    let overlays = if vietnamese {
        vec![
            pr_l10n::catalog::LanguageCatalog::try_parse(
                include_str!("../../../assets/languages/vi.json"),
                "vi.json",
                &mut warnings,
            )
            .unwrap(),
        ]
    } else {
        Vec::new()
    };
    pr_l10n::localizer::Localizer::create(&english, &overlays, &warnings)
}

struct TestNumbers;
impl pr_app::presenters::secondary::NumberFormatting for TestNumbers {
    fn grouped_i64(&self, value: i64) -> String {
        value.to_string()
    }
    fn grouped_f64(&self, value: f64) -> String {
        format!("{value:.0}")
    }
    fn percent_one_decimal(&self, ratio: f64) -> String {
        format!("{:.1}%", ratio * 100.0)
    }
}

fn skipped_content() -> ListDialogContent {
    ListDialogContent::for_skipped_files(
        &[
            SkippedEntry {
                path: PathBuf::from(r"C:\photos\a.jpg"),
                reason: "Access to the path is denied.".into(),
                kind: SkippedKind::FileUnreadable,
            },
            SkippedEntry {
                path: PathBuf::from(r"C:\photos\b.jpg"),
                reason: "The process cannot access the file because it is being used by another process.".into(),
                kind: SkippedKind::FileUnreadable,
            },
            SkippedEntry {
                path: PathBuf::from(r"C:\photos\sub"),
                reason: "Directory listing interrupted".into(),
                kind: SkippedKind::ListingInterrupted,
            },
        ],
        &localizer(false),
    )
}

fn batch_content() -> ListDialogContent {
    ListDialogContent::for_batch_review(
        &[
            BatchReviewItem {
                path: PathBuf::from(r"C:\photos\a.jpg"),
                length: Some(1234),
            },
            BatchReviewItem {
                path: PathBuf::from(r"C:\photos\gone.jpg"),
                length: None,
            },
        ],
        &localizer(false),
        &TestNumbers,
    )
}

fn list_lines(dialog: &NativeDialog) -> Vec<String> {
    let list = item(dialog, LIST_ID);
    // SAFETY: LB_GETCOUNT has integer arguments only.
    let count = unsafe { dialog_native::user32::send_message(list, control_constants::LB_GETCOUNT, 0, 0) };
    (0..count)
        .map(|index| {
            // SAFETY: LB_GETTEXTLEN has integer arguments only.
            let len = unsafe {
                dialog_native::user32::send_message(list, control_constants::LB_GETTEXTLEN, index as usize, 0)
            };
            let mut buffer = vec![0u16; len as usize + 1];
            // SAFETY: buffer has the length returned by LB_GETTEXTLEN plus a terminator and lives through this synchronous send.
            unsafe {
                dialog_native::user32::send_message(
                    list,
                    control_constants::LB_GETTEXT,
                    index as usize,
                    buffer.as_mut_ptr() as isize,
                )
            };
            String::from_utf16_lossy(&buffer[..len as usize])
        })
        .collect()
}
// C#: DiagnosticsAndListDialogTests.SkippedFiles_ListsOneLinePerEntry_WithTheIntroInTheWarningColor_AndOnlyACloseButton
#[test]
#[ignore = "native: needs desktop"]
fn skipped_files_lists_one_line_per_entry_with_the_intro_in_the_warning_color_and_only_a_close_button() {
    on_sta(|| {
        let expected = skipped_content();
        let dialog = NativeDialog::new(Rc::new(ListDialog::new(expected.clone())));
        show_on_owner(&dialog, |d, _| {
            assert_eq!(text_of(d.hwnd()), expected.title);
            assert_eq!(text_of(item(d, INTRO_ID)), expected.intro.as_deref().unwrap());
            assert_eq!(item(d, HEADING_ID), 0);
            assert_eq!(item(d, CONFIRM_ID), 0);
            assert_eq!(text_of(item(d, CANCEL_ID)), expected.cancel_text);
            // SAFETY: LB_GETCOUNT has integer arguments only.
            let count =
                unsafe { dialog_native::user32::send_message(item(d, LIST_ID), control_constants::LB_GETCOUNT, 0, 0) };
            assert_eq!(count, expected.lines.len() as isize);
            assert_eq!(list_lines(d), expected.lines);
            let dc = dialog_native::user32::get_dc(item(d, INTRO_ID));
            // SAFETY: the child and its DC are live through this synchronous WM_CTLCOLORSTATIC send.
            unsafe {
                dialog_native::user32::send_message(
                    d.hwnd(),
                    dialog_native::WM_CTLCOLORSTATIC,
                    dc as usize,
                    item(d, INTRO_ID),
                )
            };
            assert_eq!(
                dialog_native::gdi32::get_text_color(dc),
                DialogColors::from_key("Dark.Warning").unwrap()
            );
            dialog_native::user32::release_dc(item(d, INTRO_ID), dc);
            command(d, CANCEL_ID, 0);
        })
        .unwrap();
    });
}

// C#: DiagnosticsAndListDialogTests.BatchReview_ShowsHeadingSummaryAndLines_AndConfirmReturnsOk
#[test]
#[ignore = "native: needs desktop"]
fn batch_review_shows_heading_summary_and_lines_and_confirm_returns_ok() {
    on_sta(|| {
        let expected = batch_content();
        let dialog = NativeDialog::new(Rc::new(ListDialog::new(expected.clone())));
        let result = show_on_owner(&dialog, |d, _| {
            assert_eq!(text_of(item(d, HEADING_ID)), expected.heading.as_deref().unwrap());
            assert_eq!(text_of(item(d, INTRO_ID)), expected.intro.as_deref().unwrap());
            assert_eq!(list_lines(d), expected.lines);
            command(d, CONFIRM_ID, 0);
        })
        .unwrap();
        assert_eq!(result, ID_OK);
    });
}

// C#: DiagnosticsAndListDialogTests.BatchReview_EveryWayExceptConfirm_ReturnsCancel_SoNothingIsMoved
#[test]
#[ignore = "native: needs desktop"]
fn batch_review_every_way_except_confirm_returns_cancel_so_nothing_is_moved() {
    for how in ["cancel_button", "escape", "wm_close", "alt_f4", "outside_close"] {
        on_sta(move || {
            let dialog = NativeDialog::new(Rc::new(ListDialog::new(batch_content())));
            let result = show_on_owner(&dialog, |d, _| match how {
                "cancel_button" => {
                    command(d, CANCEL_ID, 0);
                }
                "escape" => {
                    assert!(key(d, item(d, LIST_ID), VK_ESCAPE));
                    drain(d);
                }
                "wm_close" => {
                    // SAFETY: WM_CLOSE has no pointer arguments and the dialog is live.
                    unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_CLOSE, 0, 0) };
                }
                "alt_f4" => {
                    // SAFETY: WM_SYSCOMMAND carries the SC_CLOSE integer and no pointers.
                    unsafe {
                        dialog_native::user32::send_message(d.hwnd(), messages::WM_SYSCOMMAND, fixture::SC_CLOSE, 0)
                    };
                }
                _ => d.close(),
            })
            .unwrap();
            assert_eq!(result, ID_CANCEL, "{how}");
        });
    }
}

// C#: DiagnosticsAndListDialogTests.List_IsResizable_TheListFollowsTheWindow_AndTheButtonsStayAtTheBottomRight
#[test]
#[ignore = "native: needs desktop"]
fn list_is_resizable_the_list_follows_the_window_and_the_buttons_stay_at_the_bottom_right() {
    on_sta(|| {
        let dialog = NativeDialog::new(Rc::new(ListDialog::new(batch_content())));
        show_on_owner(&dialog, |d, _| {
            let before = client_rect(d.hwnd());
            let list = child_rect(d, LIST_ID);
            let confirm = child_rect(d, CONFIRM_ID);
            let frame = fixture::window_rect(d.hwnd());
            pr_platform_win::win32::api::user32::set_window_pos(
                d.hwnd(),
                0,
                frame.left,
                frame.top,
                width(frame) + 200,
                height(frame) + 120,
                0x0002 | 0x0004 | 0x0010,
            )
            .unwrap();
            let after = client_rect(d.hwnd());
            let grown_list = child_rect(d, LIST_ID);
            let grown_confirm = child_rect(d, CONFIRM_ID);
            assert!(width(after) > width(before) && height(after) > height(before));
            assert_eq!(width(grown_list), width(list) + width(after) - width(before));
            assert_eq!(height(grown_list), height(list) + height(after) - height(before));
            assert_eq!(width(before) - confirm.right, width(after) - grown_confirm.right);
            assert_eq!(height(before) - confirm.bottom, height(after) - grown_confirm.bottom);
            pr_platform_win::win32::api::user32::set_window_pos(
                d.hwnd(),
                0,
                frame.left,
                frame.top,
                100,
                100,
                0x0002 | 0x0004 | 0x0010,
            )
            .unwrap();
            let tiny = client_rect(d.hwnd());
            assert!(width(tiny) >= child_rect(d, CONFIRM_ID).right);
            assert!(width(tiny) * 10 >= width(before) * 6);
            assert!(height(tiny) * 2 >= height(before));
            d.close();
        })
        .unwrap();
    });
}

// C#: DiagnosticsAndListDialogTests.List_WithFiftyThousandLines_AddsAllOfThem_AndScrollsHorizontallyForTheLongest
#[test]
#[ignore = "native: needs desktop"]
fn list_with_fifty_thousand_lines_adds_all_of_them_and_scrolls_horizontally_for_the_longest() {
    on_sta(|| {
        let mut entries: Vec<_> = (0..50_000)
            .map(|i| SkippedEntry {
                path: PathBuf::from(format!("C:\\photos\\{i:06}.jpg")),
                reason: "locked".into(),
                kind: SkippedKind::FileUnreadable,
            })
            .collect();
        entries.push(SkippedEntry {
            path: PathBuf::from(format!("C:\\{}.jpg", "x".repeat(400))),
            reason: "locked".into(),
            kind: SkippedKind::FileUnreadable,
        });
        let content = ListDialogContent::for_skipped_files(&entries, &localizer(false));
        let dialog = NativeDialog::new(Rc::new(ListDialog::new(content)));
        show_on_owner(&dialog, |d, _| {
            // SAFETY: LB_GETCOUNT has integer arguments only.
            let count =
                unsafe { dialog_native::user32::send_message(item(d, LIST_ID), control_constants::LB_GETCOUNT, 0, 0) };
            assert_eq!(count, 50_001);
            let style = pr_platform_win::win32::api::user32::get_window_long_ptr(item(d, LIST_ID), -16) as u32;
            assert_ne!(style & dialog_native::WS_HSCROLL, 0);
            d.close();
        })
        .unwrap();
    });
}

// C#: DiagnosticsAndListDialogTests.SkippedFiles_EnterAndEscapeBothClose_AndAFailedOkNeverReportsConfirmed
#[test]
#[ignore = "native: needs desktop"]
fn skipped_files_enter_and_escape_both_close_and_a_failed_ok_never_reports_confirmed() {
    for vk in [VK_RETURN, VK_ESCAPE] {
        on_sta(move || {
            let dialog = NativeDialog::new(Rc::new(ListDialog::new(skipped_content())));
            let result = show_on_owner(&dialog, |d, _| {
                assert!(key(d, item(d, LIST_ID), vk));
                drain(d);
            })
            .unwrap();
            assert_eq!(result, ID_CANCEL, "virtual key {vk}");
        });
    }
}

// C#: DiagnosticsAndListDialogTests.BatchReview_EnterActivatesTheDefaultConfirmButton_LikeTheWpfWindow
#[test]
#[ignore = "native: needs desktop"]
fn batch_review_enter_activates_the_default_confirm_button_like_the_wpf_window() {
    on_sta(|| {
        let dialog = NativeDialog::new(Rc::new(ListDialog::new(batch_content())));
        let result = show_on_owner(&dialog, |d, _| {
            assert!(key(d, item(d, LIST_ID), VK_RETURN));
            drain(d);
        })
        .unwrap();
        assert_eq!(result, ID_OK);
    });
}

// C#: DiagnosticsAndListDialogTests.List_Vietnamese_HeadingIntroAndButtonsAreNotClipped
#[test]
#[ignore = "native: needs desktop"]
fn list_vietnamese_heading_intro_and_buttons_are_not_clipped() {
    on_sta(|| {
        let content = ListDialogContent::for_batch_review(
            &[
                BatchReviewItem {
                    path: PathBuf::from(r"C:\photos\a.jpg"),
                    length: Some(1234),
                },
                BatchReviewItem {
                    path: PathBuf::from(r"C:\photos\gone.jpg"),
                    length: None,
                },
            ],
            &localizer(true),
            &TestNumbers,
        );
        let dialog = NativeDialog::new(Rc::new(ListDialog::new(content)));
        show_on_owner(&dialog, |d, _| {
            let client = client_rect(d.hwnd());
            for id in [HEADING_ID, INTRO_ID, LIST_ID, CANCEL_ID, CONFIRM_ID] {
                let rect = child_rect(d, id);
                assert!(
                    rect.left >= 0 && rect.top >= 0 && rect.right <= width(client) && rect.bottom <= height(client),
                    "control {id} clipped"
                );
            }
            assert!(fixture::needed_text_height(item(d, INTRO_ID)) <= height(child_rect(d, INTRO_ID)));
            assert!(fixture::needed_text_height(item(d, HEADING_ID)) <= height(child_rect(d, HEADING_ID)));
            assert!(child_rect(d, CANCEL_ID).right <= child_rect(d, CONFIRM_ID).left);
            d.close();
        })
        .unwrap();
    });
}

// C#: DiagnosticsAndListDialogTests.Chrome_IsAppliedOnceToEachListAndDiagnosticsDialog (list half)
#[test]
#[ignore = "native: needs desktop"]
fn chrome_is_applied_once_to_each_list_dialog() {
    on_sta(|| {
        let chrome = Rc::new(RecordingChrome::default());
        let dialog = NativeDialog::with_chrome(Rc::new(ListDialog::new(batch_content())), Some(chrome.clone()));
        show_on_owner(&dialog, |d, _| d.close()).unwrap();
        let calls = chrome.apply_calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0]
                .1
                .iter()
                .filter(
                    |control| control.class == pr_platform_win::dialogs::native::template::DialogControlClass::Button
                )
                .count(),
            2
        );
        assert_eq!(
            calls[0]
                .1
                .iter()
                .filter(
                    |control| control.class == pr_platform_win::dialogs::native::template::DialogControlClass::ListBox
                )
                .count(),
            1
        );
    });
}

// C#: DiagnosticsAndListDialogTests.RepeatedOpenAndClose_OfEveryDialog_LeaksNoGdiOrUserObjects (list half)
#[test]
#[ignore = "native: needs desktop"]
fn repeated_open_and_close_of_list_dialogs_leaks_no_gdi_or_user_objects() {
    on_sta(|| {
        let cycle = || {
            for content in [skipped_content(), batch_content()] {
                let dialog = NativeDialog::new(Rc::new(ListDialog::new(content)));
                show_on_owner(&dialog, |d, _| d.close()).unwrap();
            }
        };
        cycle();
        let gdi = gdi_objects();
        let user = user_objects();
        for _ in 0..15 {
            cycle();
        }
        assert!(settles_at_or_below(gdi_objects, gdi), "GDI objects leaked");
        assert!(settles_at_or_below(user_objects, user), "USER objects leaked");
    });
}
