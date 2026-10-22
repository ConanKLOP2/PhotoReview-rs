use pr_platform_win::dialogs::native::template::{
    ATOM_MARKER, DIALOG_STYLE_CHILD, DialogControlClass as C, DialogItem, HEADER_BYTES, TemplateError, build,
    class_name, window_class_name,
};

fn u16le(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn u32le(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

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
    assert_eq!(&bytes[22..], &[b'T', 0, b'i', 0, b't', 0, b'l', 0, b'e', 0, 0, 0]);
}

#[test]
fn items_are_dword_aligned_in_order_with_id_class_atom_text_and_no_creation_data() {
    // C#: DialogTemplateTests.Items_AreDwordAligned_InOrder_WithIdClassAtomTextAndNoCreationData (same four items; the C# reader walks the buffer).
    let items = [
        DialogItem::new(C::Static, 100, "Nhãn", 0x5000_0000),
        DialogItem::new(C::Edit, 101, "", 0x5001_0080).with_ex_style(0x200),
        DialogItem::new(C::Button, 2, "Huỷ", 0x5001_0000),
        DialogItem::new(C::ListBox, 302, "x", 0x50A1_0101),
    ];
    let bytes = build("T", 0, 0, &items).unwrap();
    assert_eq!(u16le(&bytes, 8), 4);
    let mut offset = HEADER_BYTES;
    // title
    while u16le(&bytes, offset) != 0 {
        offset += 2;
    }
    offset += 2;
    for item in &items {
        while offset % 4 != 0 {
            assert_eq!(bytes[offset], 0, "padding is zero");
            offset += 1;
        }
        assert_eq!(u32le(&bytes, offset), item.style);
        assert_eq!(u32le(&bytes, offset + 4), item.ex_style);
        for i in (8..16).step_by(2) {
            assert_eq!(u16le(&bytes, offset + i), 0);
        }
        assert_eq!(u32::from(u16le(&bytes, offset + 16)), item.id as u32);
        assert_eq!(u16le(&bytes, offset + 18), ATOM_MARKER);
        assert_eq!(u16le(&bytes, offset + 20), item.class as u16);
        offset += 22;
        let mut text = Vec::new();
        while u16le(&bytes, offset) != 0 {
            text.push(u16le(&bytes, offset));
            offset += 2;
        }
        offset += 2;
        assert_eq!(String::from_utf16(&text).unwrap(), item.text);
        assert_eq!(u16le(&bytes, offset), 0, "cbExtra");
        offset += 2;
    }
    assert_eq!(bytes.len(), offset);
}

#[test]
fn predefined_class_atoms_are_the_win32_ones() {
    // C#: DialogTemplateTests.PredefinedClassAtoms_AreTheWin32Ones
    for (class, expected) in [
        (C::Button, 0x80),
        (C::Edit, 0x81),
        (C::Static, 0x82),
        (C::ListBox, 0x83),
        (C::ScrollBar, 0x84),
        (C::ComboBox, 0x85),
    ] {
        assert_eq!(class as u16, expected);
    }
}

#[test]
fn empty_title_and_text_are_stored_as_single_terminators() {
    // C#: DialogTemplateTests.AnEmptyOrNullTitleAndText_AreStoredAsASingleTerminator
    let bytes = build("", 0, 0, &[DialogItem::new(C::Static, 1, "", 0)]).unwrap();
    assert_eq!(&bytes[22..24], &[0, 0]);
    assert_eq!(&bytes[46..48], &[0, 0]);
}

#[test]
fn common_control_classes_use_names_and_window_class_names_are_total() {
    assert_eq!(class_name(C::ListView), Some("SysListView32"));
    assert_eq!(class_name(C::Progress), Some("msctls_progress32"));
    assert_eq!(class_name(C::Trackbar), Some("msctls_trackbar32"));
    for (c, name) in [
        (C::Button, "Button"),
        (C::Edit, "Edit"),
        (C::Static, "Static"),
        (C::ListBox, "ListBox"),
        (C::ScrollBar, "ScrollBar"),
        (C::ComboBox, "ComboBox"),
        (C::ListView, "SysListView32"),
        (C::Progress, "msctls_progress32"),
        (C::Trackbar, "msctls_trackbar32"),
    ] {
        assert_eq!(window_class_name(c), name);
    }
    assert_eq!(
        DIALOG_STYLE_CHILD,
        pr_platform_win::win32::messages::WS_CHILD | pr_platform_win::win32::messages::WS_VISIBLE
    );
}

#[test]
fn excessive_control_count_is_reported() {
    let items = vec![DialogItem::new(C::Button, 0, "", 0); usize::from(u16::MAX) + 1];
    assert!(matches!(build("", 0, 0, &items), Err(TemplateError::TooManyControls { count }) if count == items.len()));
}

fn class_of(name: &str) -> C {
    match name {
        "Button" => C::Button,
        "Edit" => C::Edit,
        "Static" => C::Static,
        "ListBox" => C::ListBox,
        "ScrollBar" => C::ScrollBar,
        "ComboBox" => C::ComboBox,
        "ListView" => C::ListView,
        "Progress" => C::Progress,
        "Trackbar" => C::Trackbar,
        other => panic!("unknown class {other}"),
    }
}

/// Builds the items of a golden `build` case: either the explicit `items` list or the compact `repeat` object (`count` identical items, id 1..count, style 0).
fn build_case(case: &serde_json::Value) -> Result<Vec<u8>, TemplateError> {
    let items: Vec<DialogItem> = if let Some(r) = case.get("repeat") {
        let class = class_of(r["class"].as_str().expect("class"));
        let text = r["text"].as_str().expect("text");
        (1..=r["count"].as_u64().expect("count") as i32)
            .map(|id| DialogItem::new(class, id, text, 0))
            .collect()
    } else {
        case["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(|i| {
                DialogItem::new(
                    class_of(i["class"].as_str().expect("class")),
                    i["id"].as_i64().expect("id") as i32,
                    i["text"].as_str().expect("text"),
                    i["style"].as_u64().expect("style") as u32,
                )
                .with_ex_style(i["ex_style"].as_u64().expect("ex_style") as u32)
            })
            .collect()
    };
    build(
        case["title"].as_str().expect("title"),
        case["style"].as_u64().expect("style") as u32,
        case["ex_style"].as_u64().expect("ex_style") as u32,
        &items,
    )
}

#[test]
fn golden_build_matches_csharp_bytes() {
    let golden = pr_test_support::golden::load("dialog-template");
    let mut compared = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "build") {
        let name = case["name"].as_str().expect("name");
        let result = build_case(case);
        if case.get("threw").is_some() {
            assert!(result.is_err(), "{name}: expected Err");
        } else {
            let bytes = result.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                bytes.len() as u64,
                case["bytes_length"].as_u64().expect("bytes_length"),
                "{name}: length"
            );
            if let Some(hex) = case["bytes_hex"].as_str() {
                let actual: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                assert_eq!(actual, hex, "{name}: bytes");
            }
        }
        compared += 1;
    }
    assert_eq!(compared, 13);
}
