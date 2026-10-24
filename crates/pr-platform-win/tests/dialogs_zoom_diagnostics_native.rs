//! Desktop checks for W8-18p. Further C# native scenarios remain open in PARITY.md.

#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use fixture::{activate_visual_styles, command, on_sta, set_text, show_on_owner};
use pr_app::presenters::secondary::DiagnosticsDisplay;
use pr_platform_win::dialogs::diagnostics::DiagnosticsDialog;
use pr_platform_win::dialogs::native::dialog::{ID_CANCEL, ID_OK};
use pr_platform_win::dialogs::zoom_prompt::{APPLY_ID, EDIT_ID, ZoomPromptDialog};

// C#: ZoomPromptDialogTests.Apply_WithAValidValue_ReturnsOkAndTheValue
#[test]
#[ignore = "native: needs desktop"]
fn apply_with_a_valid_value_returns_ok_and_the_value() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ZoomPromptDialog::new(150, None);
        let result = show_on_owner(dialog.native(), |d, _| {
            set_text(d, EDIT_ID, "175");
            command(d, APPLY_ID, 0);
            assert!(d.is_closed());
        })
        .expect("show zoom prompt");
        assert_eq!(result, ID_OK);
        assert_eq!(dialog.value(), Some(175));
    });
}

fn empty_display() -> DiagnosticsDisplay {
    DiagnosticsDisplay {
        cache_hits: String::new(),
        cache_misses: String::new(),
        hit_rate: String::new(),
        source_bytes: String::new(),
        source_reads: String::new(),
        decode_time: String::new(),
        present_latency: String::new(),
        explorer_order: String::new(),
        explorer_metadata: String::new(),
        source_opens: String::new(),
        stat_count: String::new(),
        session_writes: String::new(),
        decoder_fallbacks: String::new(),
        present_histogram: String::new(),
        cross_thread_presents: String::new(),
    }
}

// C#: DiagnosticsAndListDialogTests.Diagnostics_EveryCloseRoute_ClosesAndReturnsCancel (close() route)
#[test]
#[ignore = "native: needs desktop"]
fn diagnostics_close_returns_cancel() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = DiagnosticsDialog::new(empty_display(), None, None);
        let result = show_on_owner(dialog.native(), |d, _| d.close()).expect("show diagnostics dialog");
        assert_eq!(result, ID_CANCEL);
        assert_eq!(dialog.native().hwnd(), 0);
    });
}
