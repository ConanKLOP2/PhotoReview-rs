//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/DialogTemplate.cs` (127 lines, baseline `d92d6aae`). Unit W8-18d. Contract: `contracts/W8-18d.md`.
//!
//! WP-35 GD1: builds a `DLGTEMPLATE` (not the `Ex` form) in memory for `CreateDialogIndirectParam`: a header plus one `DLGITEMTEMPLATE` per control, each aligned
//! to a DWORD. The template declares only class, id, text and style; every coordinate is 0 (the layout is computed at `WM_INITDIALOG`, it depends on DPI, font
//! and string length). Pure: no OS call. The wrapper `win32::dialog_native::user32::create_dialog_indirect_param` copies the bytes into a 4-aligned buffer.
//!
//! Also here: the `DialogControlClass` enum and `DialogItem` of the same C# file, and `DIALOG_STYLE_CHILD`, which C# keeps in `DialogStyles` inside
//! `ZoomPromptDialog.cs` (W8-18p re-exports it, D-W8B5-203).
use std::fmt;

use crate::win32::messages::{WS_CHILD, WS_VISIBLE};

/// The marker that says "the class is an atom" in a `DLGITEMTEMPLATE` (C# `AtomMarker`).
pub const ATOM_MARKER: u16 = 0xFFFF;

/// Size of the fixed header: style 4 + exStyle 4 + cdit 2 + 4 x short + menu 2 + class 2 = 22 (C# `HeaderBytes`).
pub const HEADER_BYTES: usize = 4 + 4 + 2 + 8 + 2 + 2;

/// `WS_CHILD | WS_VISIBLE` (C# `DialogStyles.Child`, declared in `ZoomPromptDialog.cs`; D-W8B5-203).
pub const DIALOG_STYLE_CHILD: u32 = WS_CHILD | WS_VISIBLE;

/// The standard control classes of a dialog (the atoms of `DLGITEMTEMPLATE`) plus the three common-control classes (C# `DialogControlClass : ushort`).
///
/// `0x1001..=0x1003` are internal identifiers outside the atom range `0x0080..=0x0085`: those classes are written by NAME (`class_name`).
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DialogControlClass {
    Button = 0x0080,
    Edit = 0x0081,
    Static = 0x0082,
    ListBox = 0x0083,
    ScrollBar = 0x0084,
    ComboBox = 0x0085,
    ListView = 0x1001,
    Progress = 0x1002,
    Trackbar = 0x1003,
}

impl DialogControlClass {
    /// The `ushort` value of the C# enum (the atom for the six standard classes).
    pub const fn value(self) -> u16 {
        self as u16
    }
}

/// One child control of the template (C# `DialogItem(Class, Id, Text, Style, ExStyle = 0)`). The geometry is NOT part of the template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogItem {
    /// The control class.
    pub class: DialogControlClass,
    /// The control id; written as a `ushort` (C# `(ushort)item.Id`: the low 16 bits, no range check).
    pub id: i32,
    /// The initial text (`""` and the C# `null` both give one UTF-16 terminator).
    pub text: String,
    /// `WS_*` and class style bits.
    pub style: u32,
    /// `WS_EX_*` bits (C# default `0`).
    pub ex_style: u32,
}

impl DialogItem {
    /// C# `new DialogItem(class, id, text, style)` with `ExStyle = 0`.
    pub fn new(class: DialogControlClass, id: i32, text: impl Into<String>, style: u32) -> Self {
        Self {
            class,
            id,
            text: text.into(),
            style,
            ex_style: 0,
        }
    }

    /// C# `new DialogItem(class, id, text, style, exStyle)`.
    pub fn with_ex_style(self, ex_style: u32) -> Self {
        Self { ex_style, ..self }
    }
}

/// What can go wrong while building a template (C# `ArgumentException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateError {
    /// More than `u16::MAX` controls ("Too many controls for a dialog template.").
    TooManyControls { count: usize },
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyControls { .. } => f.write_str("Too many controls for a dialog template."),
        }
    }
}

impl std::error::Error for TemplateError {}

/// C# `DialogTemplate.Build(title, style, exStyle, items)`.
///
/// Layout (little endian, `BinaryWriter` with `Encoding.Unicode`): `style u32, exStyle u32, cdit u16 (= items.len()), x i16 = 0, y i16 = 0, cx i16 = 0,
/// cy i16 = 0, menu u16 = 0, class u16 = 0, title (UTF-16 units + 0)`; then per item: zero padding up to a multiple of 4 (measured from the start of the
/// buffer), `style u32, exStyle u32, 4 x i16 = 0, id u16, class (name units + 0, or ATOM_MARKER + atom), text (units + 0), cbExtra u16 = 0`.
pub fn build(title: &str, style: u32, ex_style: u32, items: &[DialogItem]) -> Result<Vec<u8>, TemplateError> {
    if items.len() > usize::from(u16::MAX) {
        return Err(TemplateError::TooManyControls { count: items.len() });
    }
    let mut out = Vec::new();
    put_u32(&mut out, style);
    put_u32(&mut out, ex_style);
    put_u16(&mut out, items.len() as u16);
    for _ in 0..4 {
        put_u16(&mut out, 0);
    }
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_string(&mut out, title);
    for item in items {
        while out.len() % 4 != 0 {
            out.push(0);
        }
        put_u32(&mut out, item.style);
        put_u32(&mut out, item.ex_style);
        for _ in 0..4 {
            put_u16(&mut out, 0);
        }
        put_u16(&mut out, item.id as u16);
        if let Some(name) = class_name(item.class) {
            put_string(&mut out, name);
        } else {
            put_u16(&mut out, ATOM_MARKER);
            put_u16(&mut out, item.class.value());
        }
        put_string(&mut out, &item.text);
        put_u16(&mut out, 0);
    }
    Ok(out)
}
fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_string(out: &mut Vec<u8>, value: &str) {
    for unit in value.encode_utf16() {
        put_u16(out, unit);
    }
    put_u16(out, 0);
}

/// C# `ClassName(controlClass)`: the window class NAME of a common control (`control_constants::{WC_LISTVIEW, PROGRESS_CLASS, TRACKBAR_CLASS}` =
/// `SysListView32`, `msctls_progress32`, `msctls_trackbar32`); `None` for the six standard classes, which use an atom.
pub fn class_name(class: DialogControlClass) -> Option<&'static str> {
    match class {
        DialogControlClass::ListView => Some("SysListView32"),
        DialogControlClass::Progress => Some("msctls_progress32"),
        DialogControlClass::Trackbar => Some("msctls_trackbar32"),
        _ => None,
    }
}

/// C# `WindowClassName(controlClass)`: the class name for `CreateWindowEx` (the Settings pages create their controls lazily, not from the template): the
/// common-control name, else `"Button"`, `"Edit"`, `"Static"`, `"ListBox"`, `"ScrollBar"`, `"ComboBox"`. Total: every variant has a name (the C# `default`
/// arm that throws `ArgumentOutOfRangeException` is unrepresentable).
pub fn window_class_name(class: DialogControlClass) -> &'static str {
    match class {
        DialogControlClass::Button => "Button",
        DialogControlClass::Edit => "Edit",
        DialogControlClass::Static => "Static",
        DialogControlClass::ListBox => "ListBox",
        DialogControlClass::ScrollBar => "ScrollBar",
        DialogControlClass::ComboBox => "ComboBox",
        DialogControlClass::ListView => "SysListView32",
        DialogControlClass::Progress => "msctls_progress32",
        DialogControlClass::Trackbar => "msctls_trackbar32",
    }
}
