#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use pr_app::presenters::secondary::NumberFormatting;
use pr_platform_win::dialogs::zoom_prompt::DialogMeasure;

static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct LanguageGuard {
    _guard: MutexGuard<'static, ()>,
}

impl LanguageGuard {
    pub fn english() -> Self { Self { _guard: serial() } }
    pub fn vietnamese() -> Self { Self { _guard: serial() } }
}

#[derive(Default)]
pub struct EnUs;

impl NumberFormatting for EnUs {
    fn grouped_i64(&self, value: i64) -> String { value.to_string() }
    fn grouped_f64(&self, value: f64) -> String { value.to_string() }
    fn percent_one_decimal(&self, value: f64) -> String { format!("{value:.1}%") }
}

#[derive(Default)]
pub struct FakeMeasure {
    pub avg_char_width: i32,
    pub line_height: i32,
    pub item_heights: HashMap<i32, i32>,
    pub default_item_height: i32,
    pub item_texts: HashMap<i32, String>,
    pub text_width: i32,
    pub text_height: i32,
    pub button_width_override: Option<i32>,
}

impl DialogMeasure for FakeMeasure {
    fn dx(&self, n: i32) -> i32 { n.wrapping_mul(self.avg_char_width).wrapping_add(2) / 4 }
    fn dy(&self, n: i32) -> i32 { n.wrapping_mul(self.line_height).wrapping_add(4) / 8 }
    fn line_height(&self) -> i32 { self.line_height }
    fn measure_item_height(&self, id: i32, _: i32) -> i32 { *self.item_heights.get(&id).unwrap_or(&self.default_item_height) }
    fn measure_text_width(&self, _: &str) -> i32 { self.text_width }
    fn measure_text_height(&self, _: &str, _: i32) -> i32 { if self.text_height == 0 { self.line_height } else { self.text_height } }
    fn item_text(&self, id: i32) -> String { self.item_texts.get(&id).cloned().unwrap_or_default() }
    fn button_width(&self, _: &[i32]) -> i32 { self.button_width_override.unwrap_or_else(|| self.dx(50)) }
    fn button_row_width(&self, ids: &[i32]) -> i32 { let n = ids.len() as i32; n * self.button_width(ids) + (n - 1).max(0) * self.dx(6) }
}
