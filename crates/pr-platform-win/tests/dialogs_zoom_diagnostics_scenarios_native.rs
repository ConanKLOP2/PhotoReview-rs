//! Lane st-dialog-scenarios: C# `ZoomPromptDialogTests` and `DiagnosticsAndListDialogTests` (Diagnostics part) scenarios that had no Rust
//! twin yet. Hidden owner window, driven from the `ready` callback; the clipboard is a closure that records the text.

#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use fixture::{
    SC_CLOSE, VK_ESCAPE, VK_RETURN, activate_visual_styles, command, create_owner, destroy_owner, drain,
    has_quit_pending, height, is_visible, item, key, on_sta, set_text, show_on_owner, text_of,
};
use pr_app::presenters::secondary::DiagnosticsDisplay;
use pr_platform_win::dialogs::diagnostics::{
    CLOSE_ID, COPY_ID, DiagnosticsDialog, FIRST_LABEL_ID, FIRST_VALUE_ID, HEADER_ID, NOTE_ID, ROW_COUNT,
    build_copy_text, label_text, value_text,
};
use pr_platform_win::dialogs::native::dialog::{ID_CANCEL, ID_OK};
use pr_platform_win::dialogs::zoom_prompt::{APPLY_ID, CANCEL_ID, EDIT_ID, ERROR_ID, ZoomPromptDialog};
use pr_platform_win::win32::{api, dialog_native, messages};
use std::cell::RefCell;
use std::rc::Rc;

fn distinct_display() -> DiagnosticsDisplay {
    let v = |i: usize| format!("v{i:02}");
    DiagnosticsDisplay {
        cache_hits: v(0),
        cache_misses: v(1),
        hit_rate: v(2),
        source_bytes: v(3),
        source_reads: v(4),
        decode_time: v(5),
        present_latency: v(6),
        explorer_order: v(7),
        explorer_metadata: v(8),
        source_opens: v(9),
        stat_count: v(10),
        session_writes: v(11),
        decoder_fallbacks: v(12),
        present_histogram: v(13),
        cross_thread_presents: v(14),
    }
}

// ---- Zoom prompt -------------------------------------------------------------------------------------------------------------

// C#: ZoomPromptDialogTests.Apply_ValidatesLikeTheSettingsBox_BoundariesAndGarbage
#[test]
#[ignore = "native: needs desktop"]
fn zoom_apply_validates_like_the_settings_box_boundaries_and_garbage() {
    for (text, accepted) in [
        ("10", true),
        ("800", true),
        (" 120 ", true),
        ("9", false),
        ("801", false),
        ("0", false),
        ("-5", false),
        ("abc", false),
        ("", false),
        ("12.5", false),
    ] {
        on_sta(move || {
            let _styles = activate_visual_styles();
            let dialog = ZoomPromptDialog::new(100, None);
            let result = show_on_owner(dialog.native(), |d, _| {
                set_text(d, EDIT_ID, text);
                command(d, APPLY_ID, 0);
                if !d.is_closed() {
                    command(d, CANCEL_ID, 0);
                }
            })
            .expect("show zoom prompt");
            if accepted {
                assert_eq!(result, ID_OK, "{text:?}");
                assert_eq!(dialog.value(), Some(text.trim().parse().unwrap()), "{text:?}");
                assert_eq!(dialog.error_message(), None, "{text:?}");
            } else {
                assert_eq!(result, ID_CANCEL, "{text:?}");
                assert_eq!(dialog.value(), None, "{text:?}");
                assert!(dialog.error_message().is_some_and(|m| !m.is_empty()), "{text:?}");
            }
        });
    }
}

// C#: ZoomPromptDialogTests.InvalidValue_ShowsTheMessageUnderTheBox_KeepsTheDialogOpen_GrowsIt_AndRefocusesTheBox
#[test]
#[ignore = "native: needs desktop"]
fn zoom_invalid_value_shows_the_message_keeps_the_dialog_open_and_grows_it() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ZoomPromptDialog::new(100, None);
        let result = show_on_owner(dialog.native(), |d, _| {
            let before = height(fixture::client_rect(d.hwnd()));
            set_text(d, EDIT_ID, "9999");
            command(d, APPLY_ID, 0);
            assert!(!d.is_closed(), "an invalid value must keep the dialog open");
            assert!(is_visible(d, ERROR_ID));
            assert_eq!(text_of(item(d, ERROR_ID)), dialog.error_message().unwrap());
            assert!(
                height(fixture::client_rect(d.hwnd())) > before,
                "the dialog must grow to show the message"
            );
            let error = fixture::child_rect(d, ERROR_ID);
            let client = fixture::client_rect(d.hwnd());
            assert!(error.bottom <= client.bottom && error.right <= client.right);
            assert!(fixture::needed_text_height(item(d, ERROR_ID)) <= height(error));
            set_text(d, EDIT_ID, "200");
            command(d, APPLY_ID, 0);
        })
        .expect("show zoom prompt");
        assert_eq!(result, ID_OK);
        assert_eq!(dialog.value(), Some(200));
    });
}

// C#: ZoomPromptDialogTests.Enter_InTheBox_Applies_AndEscape_Cancels_ThroughTheDialogManager
#[test]
#[ignore = "native: needs desktop"]
fn zoom_enter_applies_and_escape_cancels_through_the_dialog_manager() {
    for (vk, expected, value) in [(VK_RETURN, ID_OK, Some(300)), (VK_ESCAPE, ID_CANCEL, None)] {
        on_sta(move || {
            let _styles = activate_visual_styles();
            let dialog = ZoomPromptDialog::new(100, None);
            let result = show_on_owner(dialog.native(), |d, _| {
                set_text(d, EDIT_ID, "300");
                assert!(key(d, item(d, EDIT_ID), vk));
                drain(d);
            })
            .expect("show zoom prompt");
            assert_eq!(result, expected);
            assert_eq!(dialog.value(), value);
        });
    }
}

// C#: ZoomPromptDialogTests.EveryWayToCloseWithoutApplying_ReturnsCancel
#[test]
#[ignore = "native: needs desktop"]
fn zoom_every_way_to_close_without_applying_returns_cancel() {
    for how in ["wm_close", "alt_f4", "cancel_button", "outside_close"] {
        on_sta(move || {
            let _styles = activate_visual_styles();
            let dialog = ZoomPromptDialog::new(100, None);
            let result = show_on_owner(dialog.native(), |d, _| {
                set_text(d, EDIT_ID, "400"); // a pending valid value must not leak out
                match how {
                    "wm_close" => {
                        // SAFETY: WM_CLOSE has no pointer arguments and the dialog is live.
                        unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_CLOSE, 0, 0) };
                    }
                    "alt_f4" => {
                        // SAFETY: WM_SYSCOMMAND/SC_CLOSE has no pointer arguments and the dialog is live.
                        unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_SYSCOMMAND, SC_CLOSE, 0) };
                    }
                    "cancel_button" => {
                        command(d, CANCEL_ID, 0);
                    }
                    _ => d.close(),
                }
                assert!(d.is_closed(), "{how}");
            })
            .expect("show zoom prompt");
            assert_eq!(result, ID_CANCEL, "{how}");
            assert_eq!(dialog.value(), None, "{how}");
        });
    }
}

// C#: ZoomPromptDialogTests.WmQuit_ClosesTheDialogAsCancel_AndIsReposted_NotSwallowed
#[test]
#[ignore = "native: needs desktop"]
fn zoom_wm_quit_closes_as_cancel_and_is_reposted() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ZoomPromptDialog::new(100, None);
        let result =
            show_on_owner(dialog.native(), |_, _| api::user32::post_quit_message(7)).expect("show zoom prompt");
        assert_eq!(
            has_quit_pending(),
            Some(7),
            "the nested loop must repost WM_QUIT for the outer loop"
        );
        assert_eq!(result, ID_CANCEL);
        assert_eq!(dialog.value(), None);
    });
}

// C#: ZoomPromptDialogTests.WithoutAnOwner_OrADeadOwner_StillOpensAndCloses
#[test]
#[ignore = "native: needs desktop"]
fn zoom_without_an_owner_or_with_a_dead_owner_still_opens_and_closes() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let no_owner = ZoomPromptDialog::new(100, None);
        let result = no_owner
            .native()
            .show_modal(
                0,
                Some(&|shown| {
                    assert!(api::user32::is_window_visible(shown.hwnd()));
                    command(shown, APPLY_ID, 0);
                }),
                None,
            )
            .expect("show without owner");
        assert_eq!(result, ID_OK);
        let owner = create_owner();
        destroy_owner(owner);
        let dead_owner = ZoomPromptDialog::new(100, None);
        let result = dead_owner
            .native()
            .show_modal(
                owner,
                Some(&|shown| {
                    command(shown, CANCEL_ID, 0);
                }),
                None,
            )
            .expect("show with a dead owner");
        assert_eq!(result, ID_CANCEL);
    });
}

// ---- Diagnostics -------------------------------------------------------------------------------------------------------------

// C#: DiagnosticsAndListDialogTests.Diagnostics_ShowsTheFifteenLabelValuePairs_InTheOrderOfTheWpfWindow / Diagnostics_RowIndexMapsToTheRightField_AndNoTwoRowsShareAValue
#[test]
#[ignore = "native: needs desktop"]
fn diagnostics_shows_the_fifteen_label_value_pairs_in_order() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let display = distinct_display();
        let dialog = DiagnosticsDialog::new(display.clone(), None, None);
        show_on_owner(dialog.native(), |d, _| {
            assert_eq!(text_of(d.hwnd()), pr_l10n::tr::diagnostics_title());
            assert_eq!(text_of(item(d, HEADER_ID)), pr_l10n::tr::diagnostics_header());
            assert_eq!(text_of(item(d, NOTE_ID)), pr_l10n::tr::diagnostics_privacy_note());
            for row in 0..ROW_COUNT {
                assert_eq!(text_of(item(d, FIRST_LABEL_ID + row as i32)), label_text(row).unwrap());
                assert_eq!(
                    text_of(item(d, FIRST_VALUE_ID + row as i32)),
                    value_text(&display, row).unwrap()
                );
                assert_eq!(text_of(item(d, FIRST_VALUE_ID + row as i32)), format!("v{row:02}"));
            }
            d.close();
        })
        .expect("show diagnostics dialog");
    });
}

// C#: DiagnosticsAndListDialogTests.Diagnostics_EveryCloseRoute_ClosesAndReturnsCancel (close button, Escape, Enter, WM_CLOSE, Alt+F4; the close() route is in dialogs_zoom_diagnostics_native)
#[test]
#[ignore = "native: needs desktop"]
fn diagnostics_every_close_route_closes_and_returns_cancel() {
    for how in ["close_button", "escape", "enter", "wm_close", "alt_f4"] {
        on_sta(move || {
            let _styles = activate_visual_styles();
            let dialog = DiagnosticsDialog::new(distinct_display(), None, None);
            let result = show_on_owner(dialog.native(), |d, _| {
                match how {
                    "close_button" => {
                        command(d, CLOSE_ID, 0);
                    }
                    "escape" => {
                        assert!(key(d, item(d, CLOSE_ID), VK_ESCAPE));
                        drain(d);
                    }
                    "enter" => {
                        // SAFETY: DM_GETDEFID has no pointer arguments and the dialog is live.
                        let default = unsafe { dialog_native::user32::send_message(d.hwnd(), 0x0400, 0, 0) } & 0xFFFF;
                        assert_eq!(default as i32, ID_CANCEL);
                        assert!(key(d, item(d, NOTE_ID), VK_RETURN)); // Enter on a non-button: the default button runs
                        drain(d);
                    }
                    "wm_close" => {
                        // SAFETY: WM_CLOSE has no pointer arguments and the dialog is live.
                        unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_CLOSE, 0, 0) };
                    }
                    _ => {
                        // SAFETY: WM_SYSCOMMAND/SC_CLOSE has no pointer arguments and the dialog is live.
                        unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_SYSCOMMAND, SC_CLOSE, 0) };
                    }
                }
                assert!(d.is_closed(), "{how}");
            })
            .expect("show diagnostics dialog");
            assert_eq!(result, ID_CANCEL, "{how}");
        });
    }
}

// C#: DiagnosticsAndListDialogTests.Diagnostics_CopyButton_PutsTheWholeTableOnTheClipboard_AndKeepsTheWindowOpen
#[test]
#[ignore = "native: needs desktop"]
fn diagnostics_copy_button_puts_the_whole_table_on_the_clipboard_and_keeps_the_window_open() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let display = distinct_display();
        let copied = Rc::new(RefCell::new(Vec::<String>::new()));
        let sink = copied.clone();
        let dialog = DiagnosticsDialog::new(
            display.clone(),
            Some(Box::new(move |text: &str| {
                sink.borrow_mut().push(text.to_owned());
                true
            })),
            None,
        );
        let still_open = RefCell::new(false);
        show_on_owner(dialog.native(), |d, _| {
            command(d, COPY_ID, 0);
            *still_open.borrow_mut() = !d.is_closed();
            d.close();
        })
        .expect("show diagnostics dialog");
        assert!(*still_open.borrow());
        assert_eq!(dialog.copy_count(), 1);
        let copied = copied.borrow();
        assert_eq!(copied.len(), 1);
        assert_eq!(copied[0], build_copy_text(&display));
        let lines: Vec<&str> = copied[0].split("\r\n").filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), ROW_COUNT);
        assert_eq!(lines[0], format!("{}\tv00", label_text(0).unwrap()));
        assert_eq!(lines[14], format!("{}\tv14", label_text(14).unwrap()));
    });
}
