//! Port of `PhotoReview.App.Shared/Viewport/ViewportContracts.cs` (types only: `ScrollBarPolicy`, `ViewportInput`,
//! `ViewportLayout`), `Viewport/ViewportSnapshot.cs`, and `ViewModels/ViewerStretchMode.cs`.
//!
//! Decision D-W8-01a-2: `ViewerStretchMode` is declared here because `ViewportInput` needs it. W8-01d and W8-06a import it
//! from this module; the C# file moves with W8-01a, not W8-01d.
//! The engine bodies `ViewportLayoutEngine.Compute` and `ClampOffset` are W8-01b (`viewport::layout`), not here.
//! Baseline `d92d6aae`. Field names: C# `ViewportWidth` becomes `viewport_width` and so on; C# `ImageRect` is `image_rect`.

#![allow(unused_variables, unused_imports, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

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
        todo!()
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
        todo!()
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
        todo!()
    }
}

impl std::fmt::Display for ViewportSnapshot {
    /// C# `ToString()`, invariant culture (decision D-W8-01a-5). Exact text:
    /// `Zoom={zoom:F2} Stretch={Stretch} MaxImage=({w:F0},{h:F0}) Actual=({w:F0},{h:F0}) Extent=({w:F0},{h:F0})
    /// Viewport=({w:F0},{h:F0}) Offset=({h:F1},{v:F1}) Scrollbars=({Visible|Collapsed},{Visible|Collapsed})`.
    /// The stretch name is `None` or `Uniform`; a bar is `Visible` or `Collapsed`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

/// .NET `ToString("F<decimals>")` for one double, invariant culture: fixed decimals, rounded half away from zero on the
/// exact binary value (NOT Rust's round-half-even `{:.N}`); `-0.0` keeps its sign ("-0.0"); NaN is `NaN`; infinities are
/// `Infinity` / `-Infinity`. The exact tie rules are settled by the golden `viewport-snapshot-text` (contract open question 4).
pub fn format_fixed(value: f64, decimals: usize) -> String {
    todo!()
}
