use pr_platform_win::dialogs::native::template::{
    build, class_name, window_class_name, DialogControlClass as C, DialogItem, TemplateError,
    ATOM_MARKER, DIALOG_STYLE_CHILD, HEADER_BYTES,
};

fn u16le(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes([bytes[at], bytes[at + 1]]) }
fn u32le(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) }

#[test]
fn header_has_style_ex_style_count_zero_geometry_and_title() {
    // C#: DialogTemplateTests.Header_HasStyleExStyleCountZeroGeometryAndTitle
    let bytes = build("Title", 0x1234_5678, 0x8765_4321, &[]).unwrap();
    assert_eq!(HEADER_BYTES, 22);
    assert_eq!(u32le(&bytes, 0), 0x1234_5678);
    assert_eq!(u32le(&bytes, 4), 0x8765_4321);
    assert_eq!(u16le(&bytes, 8), 0);
    assert_eq!(&bytes[10..18], &[0; 8]);
    assert_eq!(&bytes[18..22], &[0; 4]);
    assert_eq!(&bytes[22..], &[b'T',0,b'i',0,b't',0,b'l',0,b'e',0,0,0]);
}

#[test]
fn items_are_dword_aligned_in_order_with_id_class_atom_text_and_no_creation_data() {
    // C#: DialogTemplateTests.Items_AreDwordAligned_InOrder_WithIdClassAtomTextAndNoCreationData
    let items = [DialogItem::new(C::Button, 17, "OK", 0x5001_0000), DialogItem::new(C::Edit, 23, "Name", 0x5001_0000)];
    let bytes = build("D", 1, 2, &items).unwrap();
    let first = 26;
    assert_eq!(u32le(&bytes, first), 0x5001_0000);
    assert_eq!(u16le(&bytes, first + 16), 17);
    assert_eq!(u16le(&bytes, first + 18), ATOM_MARKER);
    assert_eq!(u16le(&bytes, first + 20), C::Button as u16);
    assert_eq!(&bytes[first + 22..first + 28], &[b'O',0,b'K',0,0,0]);
    let second = (first + 28 + 3) & !3;
    assert_eq!(second % 4, 0);
    assert_eq!(u32le(&bytes, second), 0x5001_0000);
    assert_eq!(u16le(&bytes, second + 16), 23);
    assert_eq!(u16le(&bytes, second + 18), ATOM_MARKER);
    assert_eq!(u16le(&bytes, second + 20), C::Edit as u16);
    assert_eq!(&bytes[second + 22..second + 32], &[b'N',0,b'a',0,b'm',0,b'e',0,0,0]);
    assert_eq!(u16le(&bytes, second + 32), 0);
}

#[test]
fn predefined_class_atoms_are_the_win32_ones() {
    // C#: DialogTemplateTests.PredefinedClassAtoms_AreTheWin32Ones
    for (class, expected) in [(C::Button,0x80),(C::Edit,0x81),(C::Static,0x82),(C::ListBox,0x83),(C::ScrollBar,0x84),(C::ComboBox,0x85)] {
        assert_eq!(class as u16, expected);
    }
}

#[test]
fn empty_title_and_text_are_stored_as_single_terminators() {
    // C#: DialogTemplateTests.AnEmptyOrNullTitleAndText_AreStoredAsASingleTerminator
    let bytes = build("", 0, 0, &[DialogItem::new(C::Static, 1, "", 0)]).unwrap();
    assert_eq!(&bytes[22..24], &[0,0]);
    assert_eq!(&bytes[46..48], &[0,0]);
}

#[test]
fn common_control_classes_use_names_and_window_class_names_are_total() {
    assert_eq!(class_name(C::ListView), Some("SysListView32"));
    assert_eq!(class_name(C::Progress), Some("msctls_progress32"));
    assert_eq!(class_name(C::Trackbar), Some("msctls_trackbar32"));
    for (c, name) in [(C::Button,"Button"),(C::Edit,"Edit"),(C::Static,"Static"),(C::ListBox,"ListBox"),(C::ScrollBar,"ScrollBar"),(C::ComboBox,"ComboBox"),(C::ListView,"SysListView32"),(C::Progress,"msctls_progress32"),(C::Trackbar,"msctls_trackbar32")] { assert_eq!(window_class_name(c), name); }
    assert_eq!(DIALOG_STYLE_CHILD, pr_platform_win::win32::messages::WS_CHILD | pr_platform_win::win32::messages::WS_VISIBLE);
}

#[test]
fn excessive_control_count_is_reported() {
    let items = vec![DialogItem::new(C::Button, 0, "", 0); usize::from(u16::MAX) + 1];
    assert!(matches!(build("", 0, 0, &items), Err(TemplateError::TooManyControls { count }) if count == items.len()));
}
