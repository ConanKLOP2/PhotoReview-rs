//! Port of `NativeDialogChromePainter.cs` lines 1-333 and the pure helpers around them (baseline `d92d6aae`): the classification of a control, the predicates that
//! decide which messages the hand painter reacts to, the colour table of the painter, the scale helper and the list view custom-draw correction. Unit W8-18l.
//! Contract: `contracts/W8-18l.md`.
//!
//! Pure except [`kind_for`] (reads the window style) and nothing here calls GDI. The drawing is `painter::controls` (W8-18m) and `painter::{widgets, frame}`
//! (W8-18n); the window procedures and the attach/detach are `painter::mod` (also W8-18l).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use crate::dialogs::native::template::DialogControlClass;
use crate::win32::structs::Hwnd;

/// C# `ChromePaintKind` (how the painter draws a control). The `i32` values are the C# enum values (`None = 0` .. `HeaderFiller = 12`); the range tests of
/// the predicates below depend on the order.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChromePaintKind {
    /// No hand painter (the control keeps the classic system look; its text is coloured by `WM_CTLCOLOR*`).
    None = 0,
    PushButton = 1,
    CheckBox = 2,
    RadioButton = 3,
    GroupBox = 4,
    ComboBox = 5,
    Progress = 6,
    Trackbar = 7,
    EtchedStatic = 8,
    /// Border only (`WM_NCPAINT`): edit boxes. The inside is coloured by `WM_CTLCOLOREDIT`.
    EditBorder = 9,
    /// Border only: list boxes.
    ListBoxBorder = 10,
    /// Border only: list views (the inside by `LVM_SETBKCOLOR` and the custom-draw correction).
    ListViewBorder = 11,
    /// The header of a list view: the area right of the last column is repainted dark.
    HeaderFiller = 12,
}

impl ChromePaintKind {
    /// The C# enum value.
    pub const fn value(self) -> i32 {
        self as i32
    }

    /// The kind with this C# value, `None` for a value outside `0..=12` (the C# `(ChromePaintKind)state->Kind` cast cannot fail; Rust never stores another value).
    pub fn from_value(value: i32) -> Option<Self> {
        todo!()
    }
}

// ---- classification ------------------------------------------------------------------------------------------------------------------------------------------

/// C# `KindFor(hwnd, controlClass)` (lines 65-114) with the style read from the window: `kind_for_style(class, GetWindowLongPtr(hwnd, GWL_STYLE) as u32)`.
pub fn kind_for(hwnd: Hwnd, class: DialogControlClass) -> ChromePaintKind {
    todo!()
}

/// The pure core of `KindFor`. Button: `BS_ICON | BS_BITMAP` set -> `None`; else by `style & BS_TYPEMASK` (0x000F): 0 and 1 -> `PushButton`; 2, 3, 5, 6 ->
/// `PushButton` when `BS_PUSHLIKE` (0x1000) else `CheckBox`; 4 and 9 -> `PushButton` when `BS_PUSHLIKE` else `RadioButton`; 7 -> `GroupBox`; anything else (8
/// `BS_USERBUTTON`, 10, 11 `BS_OWNERDRAW`, 12..15 split button and command link) -> `None`. ComboBox: `(style & CBS_TYPE_MASK) == 3` (drop-down list) and neither `CBS_OWNERDRAWFIXED` nor
/// `CBS_OWNERDRAWVARIABLE` -> `ComboBox`, else `None`. Edit -> `EditBorder`. ListBox -> `ListBoxBorder`. ListView -> `ListViewBorder`. Progress: no
/// `PBS_MARQUEE` -> `Progress`, else `None`. Trackbar: no `TBS_VERT` -> `Trackbar`, else `None`. Static: `style & SS_TYPEMASK` is `SS_ETCHEDHORZ`,
/// `SS_ETCHEDVERT` or `SS_ETCHEDFRAME` -> `EtchedStatic`, else `None`. ScrollBar -> `None`.
pub fn kind_for_style(class: DialogControlClass, style: u32) -> ChromePaintKind {
    todo!()
}

/// C# `IsFullPaint(kind)`: `PushButton ..= EtchedStatic` (values 1 to 8): the hand painter draws the whole client area.
pub fn is_full_paint(kind: ChromePaintKind) -> bool {
    todo!()
}

/// C# `HasFlatBorder(kind)`: `EditBorder ..= ListViewBorder` (9 to 11) or `Progress`: the border is repainted flat at `WM_NCPAINT` / `WM_PRINT`.
pub fn has_flat_border(kind: ChromePaintKind) -> bool {
    todo!()
}

/// C# `TracksHover(kind)`: `PushButton`, `CheckBox`, `RadioButton`, `ComboBox`, `Trackbar`.
pub fn tracks_hover(kind: ChromePaintKind) -> bool {
    todo!()
}

/// C# `RepaintsAfterSystemPaint(kind, message)` (lines 375-393): the messages after which the SYSTEM control has painted by itself (`GetDC`, outside
/// `WM_PAINT`), so the hand painter must paint over it before returning. ComboBox: `WM_SETFOCUS 0x7`, `WM_KILLFOCUS 0x8`, `WM_ENABLE 0xA`, `WM_SETFONT 0x30`,
/// `WM_CANCELMODE 0x1F`, `WM_KEYDOWN 0x100`, `WM_CHAR 0x102`, `WM_LBUTTONDOWN 0x201`, `WM_LBUTTONUP 0x202`, `WM_LBUTTONDBLCLK 0x203`, `WM_MOUSEWHEEL 0x20A`,
/// `CB_RESETCONTENT 0x14B`, `CB_SELECTSTRING 0x14D`, `CB_SETCURSEL 0x14E`, `CB_SHOWDROPDOWN 0x14F`. PushButton, CheckBox, RadioButton: `WM_SETFOCUS`,
/// `WM_KILLFOCUS`, `WM_ENABLE`, `BM_SETCHECK 0xF1`, `BM_SETSTATE 0xF3`, `BM_SETSTYLE 0xF4`. Every other kind: `false`.
pub fn repaints_after_system_paint(kind: ChromePaintKind, message: u32) -> bool {
    todo!()
}

// ---- what the control procedure does with a message ------------------------------------------------------------------------------------------------------------

/// What `ControlProc` (C# lines 186-295) does with one message of a control with a hand painter. The variants are the `case` arms of the C# `switch`, in the
/// C# order; "then default" means `DefSubclassProc` runs first (or last) and its result is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlMessageAction {
    /// Not handled: `DefSubclassProc` and return its result.
    PassThrough,
    /// `WM_ERASEBKGND` of a full-paint kind: return `1`, nothing erased (no flicker).
    EraseBackgroundHandled,
    /// `WM_PAINT` of a full-paint kind: `BeginPaint`, paint, `EndPaint`, return `0`.
    PaintWindow,
    /// `WM_PRINTCLIENT` of a full-paint kind: paint into the DC in `wParam`, return `0`.
    PrintClient,
    /// `WM_MOUSEMOVE` of a hover kind: when not yet hot, mark hot, `TrackMouseEvent(TME_LEAVE)`, `InvalidateRect(.., false)`; then default.
    TrackHover,
    /// `WM_MOUSELEAVE` of a hover kind: mark not hot, `InvalidateRect(.., false)`; then default.
    LeaveHover,
    /// `WM_NCPAINT` of a flat-border kind: default first, then the border over it through `GetWindowDC`; return the default result.
    PaintNonClientBorder,
    /// `WM_PRINT` with `PRF_NONCLIENT` in the low 32 bits of `lParam` of a flat-border kind: default first, then the border into the DC in `wParam`.
    PrintNonClientBorder,
    /// `WM_PAINT` of a `HeaderFiller`: default first, then `GetDC` and the filler; return the default result.
    PaintHeaderFiller,
    /// `WM_PRINTCLIENT` of a `HeaderFiller`: default first, then the filler into the DC in `wParam`.
    PrintHeaderFiller,
    /// `WM_UPDATEUISTATE` of a full-paint kind: default first, then `InvalidateRect(.., false)`.
    InvalidateAfterDefault,
    /// A message for which `repaints_after_system_paint` is true: default first, then `RepaintNow`.
    RepaintAfterDefault,
    /// `WM_NCDESTROY`: remove the subclass, remove the property, free the state; then default.
    Destroy,
}

/// The `switch` of `ControlProc` as a pure function. Order of the tests (the first that matches wins, as the C# `case ... when`): `WM_ERASEBKGND` and full
/// paint; `WM_PAINT` and full paint; `WM_PRINTCLIENT` and full paint; `WM_MOUSEMOVE` and hover; `WM_MOUSELEAVE` and hover; `WM_NCPAINT` and flat border;
/// `WM_PRINT` and flat border and `(lparam as u32) & PRF_NONCLIENT != 0`; `WM_PAINT` and `HeaderFiller`; `WM_PRINTCLIENT` and `HeaderFiller`;
/// `WM_UPDATEUISTATE` and full paint; `repaints_after_system_paint(kind, message)`; `WM_NCDESTROY`; else `PassThrough`.
pub fn control_message_action(kind: ChromePaintKind, message: u32, lparam: isize) -> ControlMessageAction {
    todo!()
}

// ---- scale, colours ---------------------------------------------------------------------------------------------------------------------------------------

/// C# `Math.Max(1.0, User32.GetDpiForWindow(hwnd) / 96.0)` of `Draw`: never below `1.0` (a window at 0 dpi, the OS answer for a dead window, paints at 1.0).
pub fn paint_scale(dpi: u32) -> f64 {
    todo!()
}

/// C# `S(scale, value)`: `Math.Max(1, (int)Math.Round(value * scale))` (`Math.Round` of a `double` is to even: `round_ties_even`). Saturating cast (a huge value
/// cannot panic).
pub fn scaled(scale: f64, value: i32) -> i32 {
    todo!()
}

/// C# `UiState(hwnd)` decoded: the two bits of the answer to `WM_QUERYUISTATE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UiState {
    /// `UISF_HIDEFOCUS` (1): no focus rectangle.
    pub hide_focus: bool,
    /// `UISF_HIDEACCEL` (2): accelerator underlines hidden (`DT_HIDEPREFIX`).
    pub hide_accel: bool,
}

/// The decoding of the `WM_QUERYUISTATE` answer (the low 32 bits).
pub fn ui_state_from_flags(raw: u32) -> UiState {
    todo!()
}

/// The `Dark.*` keys the painter uses (C# `Key("...")` strings), one constant each so no literal is repeated.
pub mod keys {
    pub const WINDOW: &str = "Dark.Window";
    pub const INPUT: &str = "Dark.Input";
    pub const SURFACE: &str = "Dark.Surface";
    pub const BUTTON: &str = "Dark.Button";
    pub const BUTTON_HOVER: &str = "Dark.ButtonHover";
    pub const BUTTON_PRESSED: &str = "Dark.ButtonPressed";
    pub const BORDER: &str = "Dark.Border";
    pub const BORDER_STRONG: &str = "Dark.BorderStrong";
    pub const BORDER_SUBTLE: &str = "Dark.BorderSubtle";
    pub const ACCENT: &str = "Dark.Accent";
    pub const ACCENT_HOVER: &str = "Dark.AccentHover";
    pub const SELECTION: &str = "Dark.Selection";
    pub const TEXT_STRONG: &str = "Dark.TextStrong";
    pub const TEXT_SOFT: &str = "Dark.TextSoft";
    pub const TEXT_SECONDARY: &str = "Dark.TextSecondary";
    pub const TEXT_MUTED: &str = "Dark.TextMuted";
    pub const TEXT_FAINT: &str = "Dark.TextFaint";
    /// Every key above, in this order (a test checks that each is in `pr_app::overlay::palette::TABLE`).
    pub const ALL: [&str; 17] = [
        WINDOW,
        INPUT,
        SURFACE,
        BUTTON,
        BUTTON_HOVER,
        BUTTON_PRESSED,
        BORDER,
        BORDER_STRONG,
        BORDER_SUBTLE,
        ACCENT,
        ACCENT_HOVER,
        SELECTION,
        TEXT_STRONG,
        TEXT_SOFT,
        TEXT_SECONDARY,
        TEXT_MUTED,
        TEXT_FAINT,
    ];
}

/// The `COLORREF` painted when a key is missing from the palette (magenta `0x00FF00FF`: visible, never black on black). The C# `DialogColors.FromKey` throws
/// `KeyNotFoundException` and the control keeps no dark look; with constant keys this is a programming error that the key-table test catches.
pub const MISSING_COLOR: u32 = 0x00FF_00FF;

/// C# `Key(key)` = `DialogColors.FromKey(key)`: the `COLORREF` of a `Dark.*` key, `MISSING_COLOR` when unknown. Forwards to `chrome::DialogColors::from_key`.
pub fn color(key: &str) -> u32 {
    todo!()
}

// ---- list view custom draw ----------------------------------------------------------------------------------------------------------------------------------------

/// What the correction of a list view `NM_CUSTOMDRAW` decided (C# `AdjustListViewCustomDraw`, lines 340-366), without touching memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomDrawAdjustment {
    /// The value to answer (`CDRF_*` flags).
    pub result: isize,
    /// Clear `CDIS_SELECTED` (1) in `uItemState`.
    pub clear_selected_flag: bool,
    /// New text colour (`clrText`), a `COLORREF`; `Some` only for a selected row.
    pub text_color: Option<u32>,
    /// New text background (`clrTextBk`), a `COLORREF`; `Some` only for a selected row.
    pub back_color: Option<u32>,
}

/// The decision of `AdjustListViewCustomDraw` with the item state query as a closure (`selected()` is called ONLY in the item pre-paint branch, at most once).
/// `stage == CDDS_PREPAINT (1)`: `result | CDRF_NOTIFYITEMDRAW (32)` unless `result & CDRF_SKIPDEFAULT (4)` is set, in which case `result` unchanged.
/// `stage & CDDS_ITEM (0x10000) != 0 && stage & 0xFFFF == 1` (item pre-paint, also the sub-item one `0x30001`): when `selected()`: `clear_selected_flag`,
/// `text_color = TextStrong`, `back_color = Dark.Selection` and `result | CDRF_NEWFONT (2)`. Anything else: `result` unchanged and nothing else.
pub fn adjust_custom_draw(stage: u32, result: isize, selected: impl FnOnce() -> bool) -> CustomDrawAdjustment {
    todo!()
}
