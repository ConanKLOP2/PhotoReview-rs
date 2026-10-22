use pr_platform_win::dialogs::native::edit_centering::{
    ES_READ_ONLY, PROP, SUBCLASS_ID, WM_CTL_COLOR_STATIC, band_rect, padding_for_metrics,
};

// C#: DialogEditCenteringTests.PaddingForMetrics
#[test]
fn padding_for_metrics_table() {
    let cases = [
        (44, 20, 16, 4, 11),
        (30, 20, 16, 4, 4),
        (20, 20, 16, 4, 0),
        (26, 20, 16, 4, 2),
        (44, 40, 16, 4, 2),
        (20, 10, 3, 5, 3),
        (10, 20, 16, 4, 0),
    ];
    for (client, height, ascent, leading, expected) in cases {
        assert_eq!(padding_for_metrics(client, height, ascent, leading), expected);
    }
    let _ = padding_for_metrics(i32::MIN, i32::MAX, i32::MIN, i32::MAX);
    let _ = padding_for_metrics(i32::MAX, i32::MIN, i32::MAX, i32::MIN);
}

// C#: DialogEditCenteringTests.BandRect
#[test]
fn band_rect_cases() {
    let rect = band_rect(100, 50, 96, 40).unwrap();
    assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (2, 2, 98, 8));
    assert!(band_rect(100, 40, 96, 36).is_none());
    let rect = band_rect(101, 50, 96, 40).unwrap();
    assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (2, 2, 99, 8));
}

// C#: DialogEditCenteringTests.Constants
#[test]
fn property_name_and_subclass_id_constants() {
    assert_eq!(SUBCLASS_ID, 0x4544_4354);
    assert_eq!(PROP, "PhotoReviewRs.EditCentering");
    assert_eq!(WM_CTL_COLOR_STATIC, 0x0138);
    assert_eq!(ES_READ_ONLY, 0x0800);
}
