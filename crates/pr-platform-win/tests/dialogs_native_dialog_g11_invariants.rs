//! Pure invariants for the G11 hardening decided in P-67 (native-dialog L3, L4). No window, no GDI call.
//! The Rust side deliberately differs from C# `NativeDialog.FirstTabStop` / `ApplyFonts` here (P-67, owner-confirmed).

use pr_platform_win::dialogs::native::dialog::{dpi::plan_font_swap, focus::is_focusable_tab_stop};

const WS_TABSTOP: u32 = 0x0001_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_DISABLED: u32 = 0x0800_0000;

// P-67 (b), L4: whatever creation returns, the dialog never ends with a deleted font and never deletes the handle it keeps.
#[test]
fn plan_font_swap_never_deletes_a_kept_handle_and_never_loses_a_font() {
    for old_font in [0, 10, 11] {
        for old_header in [0, 20, 21] {
            for new_font in [0, 10, 11] {
                for new_header in [0, 20, 21] {
                    for want_header in [false, true] {
                        let ctx = format!("{old_font} {old_header} {new_font} {new_header} {want_header}");
                        let swap = plan_font_swap(old_font, old_header, new_font, new_header, want_header);
                        if swap.delete_old_font {
                            assert!(old_font != 0 && old_font != swap.font, "{ctx}");
                        }
                        if swap.delete_old_header {
                            assert!(old_header != 0 && old_header != swap.header, "{ctx}");
                        }
                        if old_font != 0 || new_font != 0 {
                            assert_ne!(swap.font, 0, "body font lost: {ctx}");
                        }
                        if want_header && (old_header != 0 || new_header != 0) {
                            assert_ne!(swap.header, 0, "header font lost: {ctx}");
                        }
                        // A failed creation keeps the old handle alive.
                        if new_font == 0 && old_font != 0 {
                            assert_eq!(swap.font, old_font, "{ctx}");
                            assert!(!swap.delete_old_font, "{ctx}");
                        }
                    }
                }
            }
        }
    }
}

// P-67 (b), L3: WS_TABSTOP decides, WS_DISABLED vetoes, other style bits (WS_VISIBLE, class bits) do not matter.
#[test]
fn is_focusable_tab_stop_requires_tabstop_and_rejects_disabled_whatever_the_other_bits() {
    for other in [0, WS_VISIBLE, 0x0000_0003, WS_VISIBLE | 0x0000_00FF] {
        assert!(is_focusable_tab_stop(other | WS_TABSTOP), "other={other:#x}");
        assert!(
            !is_focusable_tab_stop(other | WS_TABSTOP | WS_DISABLED),
            "other={other:#x}"
        );
        assert!(!is_focusable_tab_stop(other), "other={other:#x}");
        assert!(!is_focusable_tab_stop(other | WS_DISABLED), "other={other:#x}");
    }
}
