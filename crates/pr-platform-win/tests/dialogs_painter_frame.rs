//! Unit W8-18n, part 2 (contract `contracts/W8-18n.md`): border thickness and flat borders, group box, etched lines, list view header filler and the wiring of
//! the whole painter through `DarkDialogChrome`, `painter::frame`. Pure tests run everywhere; the native tests are `#[ignore = "native: needs desktop"]` and
//! measure real offscreen windows with `PrintWindow`.

mod common {
    pub mod dialogs_painter_fixture;
}

use std::sync::Mutex;

use common::dialogs_painter_fixture::{
    OwnerWindow, Shot, child, child_ex, client_on_screen, luminance, on_sta, rgb, window_rect,
};
use pr_platform_win::dialogs::native::chrome::{DarkDialogChrome, DialogChromeControl, NativeDialogChrome};
use pr_platform_win::dialogs::native::painter::NativeDialogChromePainter as Painter;
use pr_platform_win::dialogs::native::painter::frame::{
    BorderLayer, EtchedShape, GroupBoxLabel, GroupBoxLayout, border_layers, border_thickness, etched_shape,
    group_box_layout, header_filler_rects,
};
use pr_platform_win::dialogs::native::painter::geometry::{ChromePaintKind as K, keys};
use pr_platform_win::dialogs::native::painter::production_painter;
use pr_platform_win::dialogs::native::template::DialogControlClass as C;
use pr_platform_win::win32::activation_context::ActivationScope;
use pr_platform_win::win32::api::kernel32::{get_proc_address_by_ordinal, load_library_ex};
use pr_platform_win::win32::comctl::SHELL_MANIFEST;
use pr_platform_win::win32::dialog_native::user32::send_message;
use pr_platform_win::win32::structs::{Hwnd, Rect};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

// ---- pure ------------------------------------------------------------------------------------------------------------------------------------------

/// `GetSystemMetricsForDpi` as the contract's examples define it: `SM_CXEDGE (45)` = `2 * dpi / 96`, `SM_CXBORDER (5)` = `dpi / 96`.
fn metric(index: i32, dpi: u32) -> i32 {
    let dpi = i32::try_from(dpi).unwrap_or(i32::MAX);
    match index {
        45 => 2 * dpi / 96,
        5 => dpi / 96,
        _ => 0,
    }
}

#[test]
fn border_thickness_examples() {
    // C#: NativeDialogChromePainter.BorderThickness
    assert_eq!(border_thickness(0, 0x200, 96, &metric), (2, true));
    assert_eq!(border_thickness(0, 0x200, 144, &metric), (3, true));
    assert_eq!(
        border_thickness(0, 0x200, 48, &metric),
        (2, true),
        "dpi 48 is raised to 96"
    );
    assert_eq!(border_thickness(0, 0x20000, 96, &metric), (1, false));
    assert_eq!(border_thickness(0x80_0000, 0, 96, &metric), (1, false));
    assert_eq!(
        border_thickness(0xC0_0000, 0, 96, &metric),
        (0, false),
        "a caption contains WS_DLGFRAME"
    );
    assert_eq!(border_thickness(0x80_0000 | 0x40_0000, 0, 96, &metric), (0, false));
    assert_eq!(border_thickness(0, 0, 96, &metric), (0, false));
    // The client edge wins over the border style and the static edge.
    assert_eq!(border_thickness(0x80_0000, 0x200 | 0x20000, 96, &metric), (2, true));
    assert_eq!(border_thickness(0x80_0000, 0x20000, 96, &metric), (1, false));
    // The minimum is 2 for a client edge and 1 for the others, even when the metric is 0.
    assert_eq!(border_thickness(0, 0x200, 96, &|_, _| 0), (2, true));
    assert_eq!(border_thickness(0, 0x20000, 96, &|_, _| 0), (1, false));
    assert_eq!(border_thickness(0x80_0000, 0, 96, &|_, _| -5), (1, false));
    // The dpi given to the metric is the raised one.
    assert_eq!(
        border_thickness(0, 0x200, 10, &|_, dpi| i32::try_from(dpi).unwrap_or(0)),
        (96, true)
    );
}

#[test]
fn border_layers_examples() {
    let layer = |inset: i32, outer: bool| BorderLayer { inset, outer };
    assert_eq!(border_layers(100, 24, 2, true), [layer(0, true), layer(1, false)]);
    assert_eq!(
        border_layers(100, 24, 3, true),
        [layer(0, true), layer(1, false), layer(2, false)]
    );
    assert_eq!(border_layers(100, 24, 1, true), [layer(0, true)]);
    assert_eq!(border_layers(100, 24, 2, false), [layer(0, true)]);
    assert_eq!(border_layers(100, 24, 1, false), [layer(0, true)]);
    assert!(border_layers(100, 24, 0, true).is_empty());
    assert!(border_layers(100, 24, -3, false).is_empty());
    assert!(border_layers(100, 24, i32::MIN, true).is_empty());
    // A silly edge does not allocate without bound.
    assert!(border_layers(100, 24, i32::MAX, true).len() <= 1024);
}

#[test]
fn group_box_layout_examples() {
    let a = group_box_layout(300, 100, 1.0, 15, 40, true, false);
    assert_eq!(
        a,
        GroupBoxLayout {
            frame: r(0, 7, 300, 100),
            label: Some(GroupBoxLabel {
                patch: r(5, 0, 51, 15),
                text_rect: r(8, 0, 48, 15),
                text_flags: 0x8020,
            }),
        }
    );
    let b = group_box_layout(30, 100, 1.5, 22, 80, true, false);
    assert_eq!(b.frame, r(0, 11, 30, 100));
    let label = b.label.expect("label");
    assert_eq!(label.patch, r(8, 0, 26, 22));
    assert_eq!(
        label.text_rect,
        r(12, 0, 22, 22),
        "the text is clipped to width - left - 2 * pad = 10"
    );
    let none = group_box_layout(300, 100, 1.0, 15, 0, false, false);
    assert_eq!(none.frame, r(0, 7, 300, 100));
    assert_eq!(none.label, None);
    let accel = group_box_layout(300, 100, 1.0, 15, 40, true, true)
        .label
        .expect("label");
    assert_eq!(accel.text_flags, 0x8020 | 0x0010_0000);
    // A box narrower than the padding: the shown width is 0, never negative.
    let narrow = group_box_layout(5, 100, 1.0, 15, 40, true, false).label.expect("label");
    assert_eq!(narrow.text_rect, r(8, 0, 8, 15));
}

#[test]
fn etched_shape_examples() {
    assert_eq!(etched_shape(0x10, 100, 24), EtchedShape::Line(r(0, 12, 100, 13)));
    assert_eq!(etched_shape(0x11, 100, 24), EtchedShape::Line(r(50, 0, 51, 24)));
    assert_eq!(etched_shape(0x12, 100, 24), EtchedShape::Frame(r(0, 0, 100, 24)));
    for other in [0, 1, 0x0E, 0x13, 0x1F] {
        assert_eq!(
            etched_shape(other, 100, 24),
            EtchedShape::Frame(r(0, 0, 100, 24)),
            "{other:#x}"
        );
    }
    // Odd sizes: the integer division truncates.
    assert_eq!(etched_shape(0x10, 7, 5), EtchedShape::Line(r(0, 2, 7, 3)));
    assert_eq!(etched_shape(0x11, 7, 5), EtchedShape::Line(r(3, 0, 4, 5)));
}

#[test]
fn header_filler_rects_examples() {
    assert_eq!(
        header_filler_rects(r(0, 0, 300, 24), r(0, 0, 200, 24)),
        Some([r(200, 0, 300, 24), r(200, 23, 300, 24)])
    );
    assert_eq!(header_filler_rects(r(0, 0, 300, 24), r(0, 0, 300, 24)), None);
    assert_eq!(header_filler_rects(r(0, 0, 300, 24), r(0, 0, 400, 24)), None);
    assert_eq!(
        header_filler_rects(r(0, 2, 300, 24), r(0, 0, 299, 24)),
        Some([r(299, 2, 300, 24), r(299, 23, 300, 24)])
    );
}

// ---- native ----------------------------------------------------------------------------------------------------------------------------------------

const WS_BORDER: u32 = 0x0080_0000;
const WS_EX_CLIENTEDGE: u32 = 0x200;
const BS_GROUPBOX: u32 = 7;
const SS_ETCHEDHORZ: u32 = 0x10;
const SS_ETCHEDFRAME: u32 = 0x12;
const LVS_REPORT: u32 = 1;
const LVM_INSERTCOLUMNW: u32 = 0x1061;
const WM_THEMECHANGED: u32 = 0x031A;

fn run(v6: bool, test: impl FnOnce() + Send + 'static) {
    on_sta(move || {
        let _serial = serial();
        let _activation = if v6 {
            Some(ActivationScope::from_manifest_text(SHELL_MANIFEST).expect("activate comctl32 v6"))
        } else {
            None
        };
        test();
    });
}

/// The colour at (dx, dy) of the WINDOW (non-client area included) of `hwnd`.
fn window_pixel(shot: &Shot, hwnd: Hwnd, dx: i32, dy: i32) -> u32 {
    let window = window_rect(hwnd);
    shot.at(window.left + dx, window.top + dy)
}

fn client_pixel(shot: &Shot, hwnd: Hwnd, dx: i32, dy: i32) -> u32 {
    let client = client_on_screen(hwnd);
    shot.at(client.left + dx, client.top + dy)
}

fn attach_edit_and_list_box_have_a_flat_dark_border() {
    let owner = OwnerWindow::dark();
    let edit = child_ex(&owner, C::Edit, 0, WS_EX_CLIENTEDGE, "text", 120, 24);
    let list = child_ex(&owner, C::ListBox, 0, WS_EX_CLIENTEDGE, "", 120, 60);
    assert!(Painter::attach(edit, K::EditBorder));
    assert!(Painter::attach(list, K::ListBoxBorder));
    let shot = Shot::capture(owner.hwnd);
    for (name, hwnd) in [("edit", edit), ("list box", list)] {
        // The outer pixel is the strong outline, the next one the control's own background (the inner layer), not the system 3D edge.
        assert_eq!(
            window_pixel(&shot, hwnd, 0, 10),
            rgb(keys::BORDER_STRONG),
            "{name} edge"
        );
        assert_eq!(window_pixel(&shot, hwnd, 1, 10), rgb(keys::INPUT), "{name} inner pixel");
        assert_eq!(
            window_pixel(&shot, hwnd, 60, 0),
            rgb(keys::BORDER_STRONG),
            "{name} top edge"
        );
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn edit_and_list_box_have_a_flat_dark_border_v5_classic() {
    // C#: NativeDialogChromeTests.EditAndListBox_HaveAFlatDarkBorder_NotTheSystem3DEdge (without comctl32 v6)
    run(false, attach_edit_and_list_box_have_a_flat_dark_border);
}

#[test]
#[ignore = "native: needs desktop"]
fn edit_and_list_box_have_a_flat_dark_border_v6_manifest() {
    // C#: NativeDialogChromeTests.EditAndListBox_HaveAFlatDarkBorder_NotTheSystem3DEdge (with comctl32 v6)
    run(true, attach_edit_and_list_box_have_a_flat_dark_border);
}

fn insert_column(list_view: Hwnd, index: usize, width: i32) {
    use windows::Win32::UI::Controls::{LVCOLUMNW, LVCOLUMNW_MASK};
    use windows::core::PWSTR;
    let mut title: Vec<u16> = "C\0".encode_utf16().collect();
    let column = LVCOLUMNW {
        mask: LVCOLUMNW_MASK(0x4 | 0x2), // LVCF_TEXT | LVCF_WIDTH
        cx: width,
        pszText: PWSTR(title.as_mut_ptr()),
        ..Default::default()
    };
    // SAFETY: `column` and `title` are live locals for the synchronous call; the control copies the text.
    unsafe {
        send_message(
            list_view,
            LVM_INSERTCOLUMNW,
            index,
            std::ptr::from_ref(&column) as isize,
        )
    };
}

fn header_filler_is_dark() {
    let owner = OwnerWindow::dark();
    let list_view = child(&owner, C::ListView, LVS_REPORT, "", 300, 120);
    for index in 0..3 {
        insert_column(list_view, index, 20);
    }
    // `apply_controls` attaches the list view border and the header filler (and removes the control themes).
    let chrome = DarkDialogChrome::new(production_painter());
    chrome.apply_controls(
        owner.hwnd,
        &[DialogChromeControl {
            hwnd: list_view,
            class: C::ListView,
        }],
    );
    // SAFETY: LVM_GETHEADER takes no parameters.
    let header = unsafe { send_message(list_view, 0x101F, 0, 0) };
    assert_ne!(header, 0);
    assert!(Painter::is_attached(header), "the header has the filler painter");
    let shot = Shot::capture(owner.hwnd);
    // The columns end at x = 60; right of them the system paints the light button face unless the filler repaints it.
    let colour = client_pixel(&shot, header, 150, 8);
    assert!(luminance(colour) <= 0.12, "header filler #{colour:06X} is too light");
    assert_eq!(colour, rgb(keys::SURFACE));
}

#[test]
#[ignore = "native: needs desktop"]
fn list_view_header_filler_right_of_the_last_column_is_dark_v5_classic() {
    // C#: NativeDialogChromeTests.ListViewHeader_FillerRightOfTheLastColumn_IsDark (without comctl32 v6)
    run(false, header_filler_is_dark);
}

#[test]
#[ignore = "native: needs desktop"]
fn list_view_header_filler_right_of_the_last_column_is_dark_v6_manifest() {
    // C#: NativeDialogChromeTests.ListViewHeader_FillerRightOfTheLastColumn_IsDark (with comctl32 v6)
    run(true, header_filler_is_dark);
}

#[test]
#[ignore = "native: needs desktop"]
fn group_box_and_etched_lines_use_the_border_colours() {
    run(false, || {
        let owner = OwnerWindow::dark();
        let group = child(&owner, C::Button, BS_GROUPBOX, "Group", 200, 80);
        let line = child(&owner, C::Static, SS_ETCHEDHORZ, "", 100, 8);
        let frame = child(&owner, C::Static, SS_ETCHEDFRAME, "", 60, 30);
        assert!(Painter::attach(group, K::GroupBox));
        assert!(Painter::attach(line, K::EtchedStatic));
        assert!(Painter::attach(frame, K::EtchedStatic));
        let shot = Shot::capture(owner.hwnd);
        // The bottom edge of the group frame, the horizontal line (height / 2) and the left edge of the etched frame.
        assert_eq!(
            client_pixel(&shot, group, 100, 79),
            rgb(keys::BORDER_STRONG),
            "group frame bottom"
        );
        // The system shrinks an SS_ETCHEDHORZ / SS_ETCHEDVERT static to a 2 px window with an EMPTY client area (the C# painter skips an empty client too), so
        // the line itself is not measurable here: the control keeps its non-client look and only the frame variant is painted.
        let line_client = client_on_screen(line);
        assert!(
            line_client.bottom <= line_client.top,
            "the horizontal etched line has no client area"
        );
        assert_eq!(client_pixel(&shot, frame, 0, 15), rgb(keys::BORDER), "etched frame");
        assert_eq!(
            client_pixel(&shot, frame, 30, 15),
            rgb(keys::WINDOW),
            "inside the etched frame"
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn painter_is_attached_to_every_control_class_it_draws_and_not_to_static_labels() {
    // C#: NativeDialogChromeTests.Painter_IsAttachedToEveryControlClassItDraws_AndNotToStaticLabels, on plain child windows and the real chrome.
    run(false, || {
        let owner = OwnerWindow::dark();
        let button = child(&owner, C::Button, 0, "OK", 80, 24);
        let check = child(&owner, C::Button, 3, "Check", 80, 24);
        let radio = child(&owner, C::Button, 9, "Radio", 80, 24);
        let group = child(&owner, C::Button, BS_GROUPBOX, "Group", 80, 40);
        let combo = child(&owner, C::ComboBox, 3, "", 80, 100);
        let edit = child(&owner, C::Edit, WS_BORDER, "edit", 80, 24);
        let list_box = child(&owner, C::ListBox, WS_BORDER, "", 80, 40);
        let list_view = child(&owner, C::ListView, LVS_REPORT, "", 120, 60);
        let progress = child(&owner, C::Progress, 0, "", 80, 16);
        let trackbar = child(&owner, C::Trackbar, 1, "", 80, 40);
        let etched = child(&owner, C::Static, SS_ETCHEDHORZ, "", 80, 8);
        let label = child(&owner, C::Static, 0, "label", 80, 24);
        let scroll = child(&owner, C::ScrollBar, 0, "", 16, 60);
        let controls = [
            (button, C::Button),
            (check, C::Button),
            (radio, C::Button),
            (group, C::Button),
            (combo, C::ComboBox),
            (edit, C::Edit),
            (list_box, C::ListBox),
            (list_view, C::ListView),
            (progress, C::Progress),
            (trackbar, C::Trackbar),
            (etched, C::Static),
            (label, C::Static),
            (scroll, C::ScrollBar),
        ]
        .map(|(hwnd, class)| DialogChromeControl { hwnd, class });
        DarkDialogChrome::new(production_painter()).apply(owner.hwnd, &controls);
        for (name, hwnd) in [
            ("push button", button),
            ("check box", check),
            ("radio", radio),
            ("group box", group),
            ("combo box", combo),
            ("edit", edit),
            ("list box", list_box),
            ("list view", list_view),
            ("progress", progress),
            ("trackbar", trackbar),
            ("etched line", etched),
        ] {
            assert!(Painter::is_attached(hwnd), "{name} has the painter");
        }
        assert!(
            !Painter::is_attached(label),
            "a static label is coloured by WM_CTLCOLORSTATIC, not painted"
        );
        assert!(!Painter::is_attached(scroll), "scroll bars keep the classic look");
        // SAFETY: LVM_GETHEADER takes no parameters.
        let header = unsafe { send_message(list_view, 0x101F, 0, 0) };
        assert!(
            header != 0 && Painter::is_attached(header),
            "the list view header has the filler painter"
        );
        assert_ne!(
            pr_platform_win::win32::dialog_chrome::get_prop(owner.hwnd, "PhotoReviewRs.ChromeDialog"),
            0,
            "the dialog subclass is attached"
        );
    });
}

/// The pixels (window area) of every child of `owner`, to compare across app modes.
fn snapshot(owner: &OwnerWindow, children: &[Hwnd]) -> Vec<Vec<u32>> {
    let shot = Shot::capture(owner.hwnd);
    children
        .iter()
        .map(|&hwnd| {
            let window = window_rect(hwnd);
            let mut pixels = Vec::new();
            for y in window.top..window.bottom {
                for x in window.left..window.right {
                    pixels.push(shot.at(x, y));
                }
            }
            pixels
        })
        .collect()
}

fn broadcast_theme_changed(owner: &OwnerWindow, children: &[Hwnd]) {
    // SAFETY: integer parameters only.
    unsafe {
        send_message(owner.hwnd, WM_THEMECHANGED, 0, 0);
        for &child in children {
            send_message(child, WM_THEMECHANGED, 0, 0);
        }
    }
}

fn look_does_not_change_whatever_the_preferred_app_mode_is() {
    // The uxtheme ordinals 135 (SetPreferredAppMode) and 136 (FlushMenuThemes) are called by the TEST, as in the C# (a missing ordinal ends the test early).
    let Ok(module) = load_library_ex("uxtheme.dll", 0x0800) else {
        eprintln!("note: uxtheme.dll could not be loaded; the app-mode independence was not measured");
        return;
    };
    let (Some(set_mode), Some(flush)) = (
        get_proc_address_by_ordinal(module, 135),
        get_proc_address_by_ordinal(module, 136),
    ) else {
        eprintln!("note: the uxtheme ordinals 135 / 136 are missing; the app-mode independence was not measured");
        return;
    };
    // SAFETY: undocumented but stable exports (`int SetPreferredAppMode(int)`, `void FlushMenuThemes()`); the addresses come from this module.
    let set_mode: extern "system" fn(i32) -> i32 =
        unsafe { core::mem::transmute::<usize, extern "system" fn(i32) -> i32>(set_mode) };
    // SAFETY: as above.
    let flush: extern "system" fn() = unsafe { core::mem::transmute::<usize, extern "system" fn()>(flush) };

    let owner = OwnerWindow::dark();
    let button = child(&owner, C::Button, 1, "OK", 80, 24);
    let check = child(&owner, C::Button, 3, "Check", 80, 24);
    let radio = child(&owner, C::Button, 9, "Radio", 80, 24);
    let group = child(&owner, C::Button, BS_GROUPBOX, "Group", 80, 40);
    let combo = child(&owner, C::ComboBox, 3, "", 80, 100);
    let edit = child_ex(&owner, C::Edit, 0, WS_EX_CLIENTEDGE, "edit", 80, 24);
    let list_box = child_ex(&owner, C::ListBox, 0, WS_EX_CLIENTEDGE, "", 80, 40);
    let list_view = child_ex(&owner, C::ListView, LVS_REPORT, WS_EX_CLIENTEDGE, "", 120, 60);
    let progress = child(&owner, C::Progress, 1, "", 80, 16);
    let trackbar = child(&owner, C::Trackbar, 1, "", 80, 40);
    let etched = child(&owner, C::Static, SS_ETCHEDHORZ, "", 80, 8);
    let children = [
        button, check, radio, group, combo, edit, list_box, list_view, progress, trackbar, etched,
    ];
    let controls: Vec<_> = [
        (button, C::Button),
        (check, C::Button),
        (radio, C::Button),
        (group, C::Button),
        (combo, C::ComboBox),
        (edit, C::Edit),
        (list_box, C::ListBox),
        (list_view, C::ListView),
        (progress, C::Progress),
        (trackbar, C::Trackbar),
        (etched, C::Static),
    ]
    .into_iter()
    .map(|(hwnd, class)| DialogChromeControl { hwnd, class })
    .collect();
    DarkDialogChrome::new(production_painter()).apply(owner.hwnd, &controls);

    set_mode(0);
    flush();
    broadcast_theme_changed(&owner, &children);
    let baseline = snapshot(&owner, &children);
    for mode in [1, 2, 3, 0] {
        set_mode(mode);
        flush();
        broadcast_theme_changed(&owner, &children);
        assert!(
            snapshot(&owner, &children) == baseline,
            "the look changed with the preferred app mode {mode}"
        );
    }
    // The flips while open: 2 -> 3 and 3 -> 2.
    for (from, to) in [(2, 3), (3, 2)] {
        set_mode(from);
        flush();
        broadcast_theme_changed(&owner, &children);
        let before = snapshot(&owner, &children);
        set_mode(to);
        flush();
        broadcast_theme_changed(&owner, &children);
        assert!(
            snapshot(&owner, &children) == before,
            "the look changed when the mode flipped {from} -> {to} while open"
        );
        assert!(before == baseline, "the look differs from the baseline at mode {from}");
    }
    set_mode(0);
    flush();
}

#[test]
#[ignore = "native: needs desktop"]
fn look_does_not_change_whatever_the_preferred_app_mode_is_v5_classic() {
    // C#: NativeDialogChromeTests.Look_DoesNotChange_WhateverThePreferredAppModeIs_EvenWhenItFlipsWhileOpen (without comctl32 v6)
    run(false, look_does_not_change_whatever_the_preferred_app_mode_is);
}

#[test]
#[ignore = "native: needs desktop"]
fn look_does_not_change_whatever_the_preferred_app_mode_is_v6_manifest() {
    // C#: NativeDialogChromeTests.Look_DoesNotChange_WhateverThePreferredAppModeIs_EvenWhenItFlipsWhileOpen (with comctl32 v6)
    run(true, look_does_not_change_whatever_the_preferred_app_mode_is);
}
