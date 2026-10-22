//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/NativeDialogChrome.cs` (156 lines, baseline `d92d6aae`). Unit W8-18d. Contract: `contracts/W8-18d.md`.
//!
//! The dark look of the native dialogs, independent of the system theme: the title bar (`DWMWA_USE_IMMERSIVE_DARK_MODE`, through
//! `host::main_window::windowing::apply_title_bar`), every control loses its visual style (`SetWindowTheme("", "")` = classic look) and is then hand-painted with
//! the `Dark.*` palette by the chrome painter (W8-18l..18n). The dialog background, static text and edit boxes are coloured by the `WM_CTLCOLOR*` answers of the
//! dialog itself (W8-18h..18k). Never fails: a missing function leaves the classic look.
//!
//! Three C# items: `INativeDialogChrome` (trait `NativeDialogChrome`), `DialogColors` (COLORREFs of the dark palette) and `DarkDialogChrome`.
//!
//! Seams (D-W8B5-202): the C# `DarkDialogChrome` calls the static `NativeDialogChromePainter` and three OS helpers. Rust cannot name the painter (W8-18l, a later
//! unit that depends on this one), so the painter is the trait `ChromePainter`, implemented by the painter group; the OS helpers are the trait `ChromeOs`
//! (precedent: `host::message_loop::LoopOs`), so the whole decision logic is tested without a window.
use std::fmt;
use std::rc::Rc;

use super::template::DialogControlClass;
use crate::win32::structs::Hwnd;

/// A child control that was created (C# `DialogChromeControl(Hwnd, Class)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogChromeControl {
    /// The control window.
    pub hwnd: Hwnd,
    /// Its class.
    pub class: DialogControlClass,
}

/// C# `INativeDialogChrome`: the dark-mode seam of a dialog. The production implementation is `DarkDialogChrome`; tests record the calls to check the "dark
/// flags" without looking at pixels. Called EXACTLY ONCE per dialog, at `WM_INITDIALOG`.
pub trait NativeDialogChrome {
    /// C# `Apply(dialog, controls)`: title bar, dialog subclass, and the theme + painter of every control.
    fn apply(&self, dialog: Hwnd, controls: &[DialogChromeControl]);

    /// C# `ApplyControls(dialog, controls)` (default interface method): controls created AFTER `apply` (the Settings pages create theirs lazily): only the
    /// control theme and painter, never the title bar. The default calls `apply` (what the test recorder wants).
    fn apply_controls(&self, dialog: Hwnd, controls: &[DialogChromeControl]) {
        self.apply(dialog, controls)
    }
}

/// C# `DialogColors`: `COLORREF` values (`0x00BBGGRR`) of the dark palette (`pr_app::overlay::palette`, the same table as the overlay).
#[derive(Debug, Clone, Copy)]
pub struct DialogColors;

impl DialogColors {
    /// `Dark.Window` (C# `Window`).
    pub fn window() -> u32 {
        Self::from_key("Dark.Window").unwrap_or(0)
    }

    /// `Dark.Input` (C# `Input`).
    pub fn input() -> u32 {
        Self::from_key("Dark.Input").unwrap_or(0)
    }

    /// `Dark.WindowText` (C# `Text`).
    pub fn text() -> u32 {
        Self::from_key("Dark.WindowText").unwrap_or(0)
    }

    /// `Dark.TextStrong` (C# `TextStrong`).
    pub fn text_strong() -> u32 {
        Self::from_key("Dark.TextStrong").unwrap_or(0)
    }

    /// `Dark.TextSecondary` (C# `TextSecondary`).
    pub fn text_secondary() -> u32 {
        Self::from_key("Dark.TextSecondary").unwrap_or(0)
    }

    /// `Dark.Warning` (C# `Warning`).
    pub fn warning() -> u32 {
        Self::from_key("Dark.Warning").unwrap_or(0)
    }

    /// C# `FromKey(key)` with the C# `KeyNotFoundException` turned into `None` (as `overlay::palette::get`): the colour of a `Dark.*` key as a `COLORREF`.
    pub fn from_key(key: &str) -> Option<u32> {
        let color = pr_app::overlay::palette::get(key)?;
        Some(Self::to_color_ref(color.r, color.g, color.b))
    }

    /// C# `ToColorRef(r, g, b)`: `round(r * 255) | round(g * 255) << 8 | round(b * 255) << 16`. `r * 255` is a single-precision product, the rounding is
    /// `Math.Round` (to even), the cast to `uint` is the C# one.
    pub fn to_color_ref(r: f32, g: f32, b: f32) -> u32 {
        fn channel(value: f32) -> u32 {
            ((value * 255.0_f32) as f64).round_ties_even() as u32
        }
        channel(r) | (channel(g) << 8) | (channel(b) << 16)
    }
}

/// The painter of the hand-drawn controls (C# static `NativeDialogChromePainter`, W8-18l..18n). Implemented by the painter group; the chrome only decides WHICH
/// controls get it.
pub trait ChromePainter {
    /// C# `AttachDialog(dialog)`: subclass the dialog so the custom draw of its list views is corrected (idempotent). `false` when it could not be attached.
    fn attach_dialog(&self, dialog: Hwnd) -> bool;

    /// C# `Attach(hwnd, KindFor(hwnd, class))`: attach the hand painter chosen for this control (by class and current window style; idempotent). `false` for
    /// the kind `None` (no hand painter) or when it could not be attached.
    fn attach_control(&self, hwnd: Hwnd, class: DialogControlClass) -> bool;

    /// C# `Attach(header, ChromePaintKind.HeaderFiller)`: the list view header, whose leftover area right of the last column is repainted dark.
    fn attach_header_filler(&self, header: Hwnd) -> bool;
}

/// The OS queries and effects of `DarkDialogChrome` that are not `SetWindowTheme` (`SetWindowTheme` is the closure `SetThemeFn`).
pub trait ChromeOs {
    /// C# `TitleBar.Apply(dialog)`: dark title bar (result = whether the OS took the dark-mode attribute; information only).
    fn apply_title_bar(&self, dialog: Hwnd) -> bool;

    /// C# `ComboBoxControl.ListWindow(combo)`: the drop-down list window of a combo box, `0` when it cannot be read.
    fn combo_list_window(&self, combo: Hwnd) -> Hwnd;

    /// C# `User32.SendMessage(listView, LVM_GETHEADER, 0, 0)`: the header window of a list view, `0` when none.
    fn list_view_header(&self, list_view: Hwnd) -> Hwnd;
}

/// The real `ChromeOs`: `host::main_window::windowing::apply_title_bar(dialog, None)`, `chrome::combo_list_window`, and `LVM_GETHEADER` through
/// `win32::dialog_native::user32::send_message`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Win32ChromeOs;

impl ChromeOs for Win32ChromeOs {
    fn apply_title_bar(&self, dialog: Hwnd) -> bool {
        crate::host::main_window::windowing::apply_title_bar(dialog, None)
    }

    fn combo_list_window(&self, combo: Hwnd) -> Hwnd {
        combo_list_window(combo)
    }

    fn list_view_header(&self, list_view: Hwnd) -> Hwnd {
        if list_view == 0 {
            return 0;
        }
        // SAFETY: LVM_GETHEADER takes no pointer arguments.
        unsafe {
            crate::win32::dialog_native::user32::send_message(
                list_view,
                crate::win32::control_constants::LVM_GETHEADER,
                0,
                0,
            ) as Hwnd
        }
    }
}

/// The drop-down list window of a combo box (`GetComboBoxInfo`, field `hwndList`), `0` for `combo == 0` or when the call fails. The single implementation:
/// `ComboBoxControl::list_window` (W8-18f) forwards here.
pub fn combo_list_window(combo: Hwnd) -> Hwnd {
    if combo == 0 {
        return 0;
    }
    crate::win32::comctl::user32::get_combo_box_info(combo).map_or(0, |info| info.hwndList.0 as isize)
}

/// C# `DarkDialogChrome.ClassicTheme`: `""` = no theme (classic look). BOTH the sub-app name and the sub-id list are empty; an empty app name alone falls back
/// to the light default theme.
pub const CLASSIC_THEME: &str = "";

/// C# `DarkDialogChrome.ThemeFor(controlClass)`: `Some(CLASSIC_THEME)` for Button, Edit, ListBox, ScrollBar, ComboBox, ListView, Progress and Trackbar (they
/// have a hand painter or a flat border); `None` = leave alone (Static: its text is coloured by `WM_CTLCOLORSTATIC`).
pub fn theme_for(class: DialogControlClass) -> Option<&'static str> {
    match class {
        DialogControlClass::Static => None,
        _ => Some(CLASSIC_THEME),
    }
}

/// C# `DarkDialogChrome.ThemeForControl(hwnd, controlClass)`: today only the class matters; the signature stays so the Settings pages can call it.
pub fn theme_for_control(hwnd: Hwnd, class: DialogControlClass) -> Option<&'static str> {
    let _ = hwnd;
    theme_for(class)
}

/// C# `Action<nint, string, string?>`: `(control, sub_app_name, sub_id_list)` for `SetWindowTheme`. The default (`DarkDialogChrome::new`) is
/// `win32::dialog_native::uxtheme::set_window_theme(hwnd, Some(app), ids)` with the `Err` ignored; tests pass a recorder.
pub type SetThemeFn = Box<dyn Fn(Hwnd, &str, Option<&str>)>;

/// C# `DarkDialogChrome : INativeDialogChrome`.
pub struct DarkDialogChrome {
    set_theme: SetThemeFn,
    painter: Rc<dyn ChromePainter>,
    os: Rc<dyn ChromeOs>,
}

impl DarkDialogChrome {
    /// C# `new DarkDialogChrome()` (and `Instance`): real `SetWindowTheme`, real `Win32ChromeOs`, with the production painter given by the caller.
    pub fn new(painter: Rc<dyn ChromePainter>) -> Self {
        Self {
            set_theme: Box::new(|hwnd, app, ids| {
                let _ = crate::win32::dialog_native::uxtheme::set_window_theme(hwnd, Some(app), ids);
            }),
            painter,
            os: Rc::new(Win32ChromeOs),
        }
    }

    /// C# `internal DarkDialogChrome(Action<nint, string, string?> setTheme)` plus the two seams.
    pub fn with_seams(painter: Rc<dyn ChromePainter>, set_theme: SetThemeFn, os: Rc<dyn ChromeOs>) -> Self {
        Self { set_theme, painter, os }
    }
}

impl fmt::Debug for DarkDialogChrome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DarkDialogChrome").finish_non_exhaustive()
    }
}

impl NativeDialogChrome for DarkDialogChrome {
    /// C# `Apply`: `os.apply_title_bar(dialog)`; `painter.attach_dialog(dialog)`; then the same as `apply_controls`.
    fn apply(&self, dialog: Hwnd, controls: &[DialogChromeControl]) {
        let _ = self.os.apply_title_bar(dialog);
        let _ = self.painter.attach_dialog(dialog);
        self.apply_controls(dialog, controls);
    }

    /// C# `ApplyControls` = `ApplyControlThemes`: per control in order: `set_theme(hwnd, theme, Some(theme))` when `theme_for_control` is `Some`; then
    /// `painter.attach_control(hwnd, class)`; then the parts: ComboBox -> its drop-down list window (when non-zero): `set_theme(list, "", Some(""))`;
    /// ListView -> its header (when non-zero): `set_theme(header, "", Some(""))` then `painter.attach_header_filler(header)`.
    fn apply_controls(&self, dialog: Hwnd, controls: &[DialogChromeControl]) {
        let _ = dialog;
        for control in controls {
            if let Some(theme) = theme_for_control(control.hwnd, control.class) {
                (self.set_theme)(control.hwnd, theme, Some(theme));
            }
            let _ = self.painter.attach_control(control.hwnd, control.class);
            match control.class {
                DialogControlClass::ComboBox => {
                    let list = self.os.combo_list_window(control.hwnd);
                    if list != 0 {
                        (self.set_theme)(list, CLASSIC_THEME, Some(CLASSIC_THEME));
                    }
                }
                DialogControlClass::ListView => {
                    let header = self.os.list_view_header(control.hwnd);
                    if header != 0 {
                        (self.set_theme)(header, CLASSIC_THEME, Some(CLASSIC_THEME));
                        let _ = self.painter.attach_header_filler(header);
                    }
                }
                _ => {}
            }
        }
    }
}
