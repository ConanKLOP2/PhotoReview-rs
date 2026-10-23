use pr_platform_win::dialogs::benchmark::results::{column_click_sub_item, row_color_key};
use pr_platform_win::dialogs::benchmark::run::{enabled_flags, focus_target, status_line_height};
use pr_platform_win::dialogs::zoom_prompt::DialogMeasure;

#[test]
fn row_color_key_table() {
    assert_eq!(row_color_key(true, false, 0), "Dark.SuccessBackground");
    assert_eq!(row_color_key(false, true, 0), "Dark.ErrorBackground");
    assert_eq!(row_color_key(false, false, 0), "Dark.Panel");
    assert_eq!(row_color_key(false, false, 1), "Dark.RowAlt");
    assert_eq!(row_color_key(true, true, 1), "Dark.SuccessBackground");
}

#[test]
fn column_click_sub_item_of_zero_is_none() {
    assert_eq!(column_click_sub_item(0), None);
}

#[test]
fn nmlistview_isubitem_is_at_offset_28() {
    #[repr(C)]
    struct NmlistviewLayout {
        hwnd_from: isize,
        id_from: usize,
        code: u32,
        padding: u32,
        item: i32,
        sub_item: i32,
    }
    assert_eq!(std::mem::offset_of!(NmlistviewLayout, sub_item), 28);
}

#[test]
fn enabled_flags_follow_the_csharp_order_and_polarity() {
    assert_eq!(
        enabled_flags(true),
        [
            (110, false),
            (111, false),
            (112, true),
            (103, false),
            (109, false),
            (105, false),
            (102, false)
        ]
    );
    assert_eq!(
        enabled_flags(false),
        [
            (110, true),
            (111, true),
            (112, false),
            (103, true),
            (109, true),
            (105, true),
            (102, true)
        ]
    );
}

#[test]
fn focus_target_rescues_a_disabled_focus() {
    assert_eq!(focus_target(true, true), Some(112));
    assert_eq!(focus_target(false, true), Some(110));
    assert_eq!(focus_target(true, false), None);
    assert_eq!(focus_target(false, false), None);
}

struct Measure(std::cell::Cell<i32>);
impl DialogMeasure for Measure {
    fn dx(&self, v: i32) -> i32 {
        v
    }
    fn dy(&self, v: i32) -> i32 {
        v
    }
    fn line_height(&self) -> i32 {
        1
    }
    fn measure_item_height(&self, _: i32, _: i32) -> i32 {
        0
    }
    fn measure_text_width(&self, _: &str) -> i32 {
        0
    }
    fn measure_text_height(&self, _: &str, width: i32) -> i32 {
        self.0.set(width);
        7
    }
    fn item_text(&self, _: i32) -> String {
        String::new()
    }
    fn button_width(&self, _: &[i32]) -> i32 {
        0
    }
    fn button_row_width(&self, _: &[i32]) -> i32 {
        0
    }
}

#[test]
fn status_line_height_is_zero_for_empty_text_and_uses_the_client_width_minus_two_margins() {
    let m = Measure(std::cell::Cell::new(-1));
    assert_eq!(status_line_height(&m, "", 100, 10), 0);
    assert_eq!(m.0.get(), -1);
    assert_eq!(status_line_height(&m, "status", 100, 10), 7);
    assert_eq!(m.0.get(), 80);
    let _ = status_line_height(&m, "status", 1, 10);
    assert_eq!(m.0.get(), 1);
}
