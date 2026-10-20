//! Port of `PhotoReview.App.Shared/Viewport/ViewportContracts.cs` (types only: `ScrollBarPolicy`, `ViewportInput`,
//! `ViewportLayout`), `Viewport/ViewportSnapshot.cs`, and `ViewModels/ViewerStretchMode.cs`.
//!
//! Decision D-W8-01a-2: `ViewerStretchMode` is declared here because `ViewportInput` needs it. W8-01d and W8-06a import it
//! from this module; the C# file moves with W8-01a, not W8-01d.
//! The engine bodies `ViewportLayoutEngine.Compute` and `ClampOffset` are W8-01b (`viewport::layout`), not here.
//! Baseline `d92d6aae`. Field names: C# `ViewportWidth` becomes `viewport_width` and so on; C# `ImageRect` is `image_rect`.

use crate::input::primitives::{RectD, f64_same};

/// `ViewModels/ViewerStretchMode.cs`: `None = 0`, `Uniform = 1`. Variant names are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewerStretchMode {
    #[default]
    None = 0,
    Uniform = 1,
}

/// `ScrollBarPolicy` (C-08). `Auto` = WPF behaviour; `Overlay` and `Hidden` exist only in the Win32 shell (NE-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollBarPolicy {
    #[default]
    Auto = 0,
    Overlay = 1,
    Hidden = 2,
}

/// `ViewportInput` (C-08), a C# `record struct`. Lengths are DIP.
/// `Default` equals C# `default(ViewportInput)`: all zeros, `scroll_bars = Auto`, `stretch = None`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewportInput {
    /// DIP, client area of the image scroll (window minus border if any; overlay bars do not subtract).
    pub client_width: f64,
    pub client_height: f64,
    /// DIP (WPF: `SystemParameters.VerticalScrollBarWidth` through `DarkScrollBars.xaml`).
    pub scroll_bar_thickness: f64,
    pub scroll_bars: ScrollBarPolicy,
    pub stretch: ViewerStretchMode,
    /// `ViewerState.ImageWidth` (NaN in Fit).
    pub image_width: f64,
    pub image_height: f64,
    /// `ViewerState.MaxImageWidth` / `MaxImageHeight`.
    pub max_image_width: f64,
    pub max_image_height: f64,
    /// Natural size of the bitmap (DIP) used for `Stretch = Uniform`.
    pub bitmap_width: f64,
    pub bitmap_height: f64,
}

impl PartialEq for ViewportInput {
    /// C# `record struct` equality: field by field with `EqualityComparer<double>.Default` (see `f64_same`).
    fn eq(&self, other: &Self) -> bool {
        f64_same(self.client_width, other.client_width)
            && f64_same(self.client_height, other.client_height)
            && f64_same(self.scroll_bar_thickness, other.scroll_bar_thickness)
            && self.scroll_bars == other.scroll_bars
            && self.stretch == other.stretch
            && f64_same(self.image_width, other.image_width)
            && f64_same(self.image_height, other.image_height)
            && f64_same(self.max_image_width, other.max_image_width)
            && f64_same(self.max_image_height, other.max_image_height)
            && f64_same(self.bitmap_width, other.bitmap_width)
            && f64_same(self.bitmap_height, other.bitmap_height)
    }
}

/// `ViewportLayout` (C-08): result of `ViewportLayoutEngine.Compute`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewportLayout {
    /// `ScrollViewer.ViewportWidth` / `ViewportHeight`.
    pub viewport_width: f64,
    pub viewport_height: f64,
    /// `ScrollViewer.ExtentWidth` / `ExtentHeight`.
    pub extent_width: f64,
    pub extent_height: f64,
    /// Scroll bar visible.
    pub horizontal_bar_visible: bool,
    pub vertical_bar_visible: bool,
    /// Image element position in extent coordinates (centred when smaller than the viewport).
    pub image_rect: RectD,
    /// Largest horizontal and vertical offset.
    pub max_horizontal_offset: f64,
    pub max_vertical_offset: f64,
}

impl PartialEq for ViewportLayout {
    /// C# `record struct` equality (`f64_same` per double field, `RectD` equality for `image_rect`).
    fn eq(&self, other: &Self) -> bool {
        f64_same(self.viewport_width, other.viewport_width)
            && f64_same(self.viewport_height, other.viewport_height)
            && f64_same(self.extent_width, other.extent_width)
            && f64_same(self.extent_height, other.extent_height)
            && self.horizontal_bar_visible == other.horizontal_bar_visible
            && self.vertical_bar_visible == other.vertical_bar_visible
            && self.image_rect == other.image_rect
            && f64_same(self.max_horizontal_offset, other.max_horizontal_offset)
            && f64_same(self.max_vertical_offset, other.max_vertical_offset)
    }
}

/// `ViewportSnapshot` (record class): immutable measurements compared between two Fit passes.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewportSnapshot {
    pub zoom: f64,
    pub stretch: ViewerStretchMode,
    pub max_image_width: f64,
    pub max_image_height: f64,
    pub actual_image_width: f64,
    pub actual_image_height: f64,
    pub extent_width: f64,
    pub extent_height: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub horizontal_offset: f64,
    pub vertical_offset: f64,
    pub horizontal_scroll_bar_visible: bool,
    pub vertical_scroll_bar_visible: bool,
}

impl PartialEq for ViewportSnapshot {
    /// C# record equality (`f64_same` per double field).
    fn eq(&self, other: &Self) -> bool {
        f64_same(self.zoom, other.zoom)
            && self.stretch == other.stretch
            && f64_same(self.max_image_width, other.max_image_width)
            && f64_same(self.max_image_height, other.max_image_height)
            && f64_same(self.actual_image_width, other.actual_image_width)
            && f64_same(self.actual_image_height, other.actual_image_height)
            && f64_same(self.extent_width, other.extent_width)
            && f64_same(self.extent_height, other.extent_height)
            && f64_same(self.viewport_width, other.viewport_width)
            && f64_same(self.viewport_height, other.viewport_height)
            && f64_same(self.horizontal_offset, other.horizontal_offset)
            && f64_same(self.vertical_offset, other.vertical_offset)
            && self.horizontal_scroll_bar_visible == other.horizontal_scroll_bar_visible
            && self.vertical_scroll_bar_visible == other.vertical_scroll_bar_visible
    }
}

impl std::fmt::Display for ViewportSnapshot {
    /// C# `ToString()`, invariant culture (decision D-W8-01a-5). Exact text:
    /// `Zoom={zoom:F2} Stretch={Stretch} MaxImage=({w:F0},{h:F0}) Actual=({w:F0},{h:F0}) Extent=({w:F0},{h:F0})
    /// Viewport=({w:F0},{h:F0}) Offset=({h:F1},{v:F1}) Scrollbars=({Visible|Collapsed},{Visible|Collapsed})`.
    /// The stretch name is `None` or `Uniform`; a bar is `Visible` or `Collapsed`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stretch = match self.stretch {
            ViewerStretchMode::None => "None",
            ViewerStretchMode::Uniform => "Uniform",
        };
        let bar = |visible: bool| if visible { "Visible" } else { "Collapsed" };
        write!(
            f,
            "Zoom={} Stretch={} MaxImage=({},{}) Actual=({},{}) Extent=({},{}) Viewport=({},{}) Offset=({},{}) Scrollbars=({},{})",
            format_fixed(self.zoom, 2),
            stretch,
            format_fixed(self.max_image_width, 0),
            format_fixed(self.max_image_height, 0),
            format_fixed(self.actual_image_width, 0),
            format_fixed(self.actual_image_height, 0),
            format_fixed(self.extent_width, 0),
            format_fixed(self.extent_height, 0),
            format_fixed(self.viewport_width, 0),
            format_fixed(self.viewport_height, 0),
            format_fixed(self.horizontal_offset, 1),
            format_fixed(self.vertical_offset, 1),
            bar(self.horizontal_scroll_bar_visible),
            bar(self.vertical_scroll_bar_visible),
        )
    }
}

/// .NET `ToString("F<decimals>")` for one double, invariant culture: fixed decimals, rounded on the exact binary value.
/// An exact tie (0.125 at `F2`, 2.5 at `F0`) rounds to EVEN, as the oracle golden `viewport-snapshot-text` records
/// ("0.12", "2"); the contract's "half away from zero" note was wrong and the golden wins (open question O4).
/// `-0.0` and values that round to zero keep their sign ("-0.00", "-0"); NaN is `NaN`; infinities are `Infinity` / `-Infinity`.
pub fn format_fixed(value: f64, decimals: usize) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    let negative = value.is_sign_negative();
    if value.is_infinite() {
        return if negative {
            "-Infinity".to_owned()
        } else {
            "Infinity".to_owned()
        };
    }

    // Every finite f64 has a finite binary expansion of at most 1074 fractional digits, so 1100 digits are exact.
    let exact = format!("{:.1100}", value.abs());
    let (int_part, frac_part) = exact.split_once('.').unwrap_or((exact.as_str(), ""));

    // Digits to keep: the integer part followed by `decimals` fractional digits.
    let mut digits: Vec<u8> = int_part.bytes().map(|b| b - b'0').collect();
    let frac_bytes = frac_part.as_bytes();
    for index in 0..decimals {
        digits.push(frac_bytes.get(index).map_or(0, |b| b - b'0'));
    }
    // The first dropped digit decides. Above 5 rounds up; below 5 rounds down. Exactly 5 with no other non-zero digit
    // is a tie and rounds to even (the last kept digit decides); exactly 5 followed by a non-zero digit rounds up.
    let round_up = match frac_bytes.get(decimals) {
        Some(b'5') => {
            let rest_nonzero = frac_bytes[decimals + 1..].iter().any(|b| *b != b'0');
            let last_kept_odd = digits.last().is_some_and(|d| d % 2 == 1);
            rest_nonzero || last_kept_odd
        }
        Some(b) => *b > b'5',
        None => false,
    };
    if round_up {
        let mut carry = true;
        for digit in digits.iter_mut().rev() {
            if !carry {
                break;
            }
            if *digit == 9 {
                *digit = 0;
            } else {
                *digit += 1;
                carry = false;
            }
        }
        if carry {
            digits.insert(0, 1);
        }
    }

    let int_len = digits.len() - decimals;
    let mut text = String::with_capacity(digits.len() + 2);
    if negative {
        text.push('-');
    }
    for digit in &digits[..int_len] {
        text.push(char::from(b'0' + digit));
    }
    if decimals > 0 {
        text.push('.');
        for digit in &digits[int_len..] {
            text.push(char::from(b'0' + digit));
        }
    }
    text
}
