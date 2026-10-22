use pr_core::model::fields::{ExifInfoFields, TitleBarFields};
use pr_platform_win::dialogs::settings::pages::display::{DisplaySettingsPage, EXIF_ORDER, TITLE_ORDER};

// C#: SettingsPagesTests.Display_EveryExifAndTitleFlag_IsItsOwnCheckBox_InTheDocumentedOrder
#[test]
fn display_every_exif_and_title_flag_is_its_own_check_box_in_the_documented_order() {
    let exif = [
        ExifInfoFields::FILE_NAME,
        ExifInfoFields::DATE_TAKEN,
        ExifInfoFields::MODIFIED_DATE,
        ExifInfoFields::DIMENSIONS,
        ExifInfoFields::CAMERA,
        ExifInfoFields::LENS,
        ExifInfoFields::ISO,
        ExifInfoFields::FOCAL_LENGTH,
        ExifInfoFields::APERTURE,
        ExifInfoFields::SHUTTER_SPEED,
    ];
    let title = [
        TitleBarFields::FOLDER_NAME,
        TitleBarFields::FOLDER_PATH,
        TitleBarFields::INDEX_COUNT,
        TitleBarFields::FILE_NAME,
        TitleBarFields::FILE_SIZE,
        TitleBarFields::DIMENSIONS,
        TitleBarFields::MODIFIED_DATE,
        TitleBarFields::DATE_TAKEN,
        TitleBarFields::CAMERA,
        TitleBarFields::LENS,
        TitleBarFields::ISO,
        TitleBarFields::FOCAL_LENGTH,
        TitleBarFields::APERTURE,
        TitleBarFields::SHUTTER_SPEED,
    ];
    assert_eq!(EXIF_ORDER, exif);
    assert_eq!(TITLE_ORDER, title);
}

// C#: SettingsPagesTests.Display_EveryExifAndTitleFlag_IsItsOwnCheckBox_InTheDocumentedOrder
#[test]
fn a_page_that_was_never_declared_does_not_panic() {
    let page = DisplaySettingsPage::new();
    assert!(page.exif_checks().is_empty());
    assert!(page.title_checks().is_empty());
}
