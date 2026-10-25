//! Shared test collaborators for W8-18t/u/v. Native safety checks use injected services and TempRoot.
#![allow(dead_code)] // Each recovery test binary uses a different subset of these collaborators.

use pr_app::presenters::recovery_presentation::RecoveryTimeFormatter;
use pr_core::time::DateTimeUtc;
use pr_platform_win::dialogs::recovery::RecoveryTextSource;
use pr_platform_win::dialogs::recovery::layout::RecoveryLayoutBase;
use std::rc::Rc;

struct FixedTime;
impl RecoveryTimeFormatter for FixedTime {
    fn format_local_general(&self, _: DateTimeUtc) -> String {
        "fixed time".to_owned()
    }
}

pub fn texts() -> RecoveryTextSource {
    RecoveryTextSource::new(
        pr_l10n::localizer::built_in_english_localizer(),
        Rc::new(FixedTime),
        ",",
    )
}

pub struct Metrics {
    pub avg: i32,
    pub line: i32,
}
impl RecoveryLayoutBase for Metrics {
    fn dx(&self, n: i32) -> i32 {
        n.wrapping_mul(self.avg).wrapping_add(2) / 4
    }
    fn dy(&self, n: i32) -> i32 {
        n.wrapping_mul(self.line).wrapping_add(4) / 8
    }
    fn line_height(&self) -> i32 {
        self.line
    }
    fn measure_item_height(&self, _: i32, _: i32) -> i32 {
        0
    }
    fn measure_text_width(&self, _: &str) -> i32 {
        0
    }
    fn measure_text_height(&self, _: &str, _: i32) -> i32 {
        0
    }
    fn get_item_text(&self, _: i32) -> String {
        String::new()
    }
    fn place(&self, _: i32, _: i32, _: i32, _: i32, _: i32) {
        panic!("size-only fixture cannot place controls");
    }
    fn place_button_row(&self, _: i32, _: i32, _: i32, _: i32, _: &[i32]) -> i32 {
        panic!("size-only fixture cannot place controls");
    }
    fn button_row_width(&self, ids: &[i32]) -> i32 {
        let n = ids.len() as i32;
        n * self.button_width(ids) + (n - 1).max(0) * self.dx(6)
    }
    fn button_width(&self, _: &[i32]) -> i32 {
        self.dx(50)
    }
    fn set_list_item_height(&self, _: i32, _: i32) {
        panic!("size-only fixture cannot place controls");
    }
}
