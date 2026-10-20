//! Port of `PhotoReview.App.Shared/Input/InputPrimitives.cs` (C-06). Coordinates are DIP unless a name says `Pixel`.
//!
//! Owned by W8-01a (decision D-W8-01a-1): the whole C# file lands here, because `viewport` needs `PointD`/`RectD` and
//! `W8-05a` depends on W8-01a. W8-05a adds tests only, no code in this file.
//! Baseline `d92d6aae`. `f64` equality follows C# `double.Equals` (NaN equals NaN, `0.0 == -0.0`): see `f64_same`.

/// C# `double.Equals` for one field: `NaN` equals `NaN`; `0.0` equals `-0.0`. Used by every manual `PartialEq` of this crate.
pub(crate) fn f64_same(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

/// `PointD` (DIP): replaces `System.Windows.Point` at the Input boundary (`X`, `Y` become `x`, `y`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PointD {
    pub x: f64,
    pub y: f64,
}

impl PartialEq for PointD {
    fn eq(&self, other: &Self) -> bool {
        f64_same(self.x, other.x) && f64_same(self.y, other.y)
    }
}

/// `SizeD` (DIP): replaces `System.Windows.Size`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SizeD {
    pub width: f64,
    pub height: f64,
}

impl PartialEq for SizeD {
    fn eq(&self, other: &Self) -> bool {
        f64_same(self.width, other.width) && f64_same(self.height, other.height)
    }
}

/// `RectD` (DIP): replaces `System.Windows.Rect`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RectD {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl PartialEq for RectD {
    fn eq(&self, other: &Self) -> bool {
        f64_same(self.x, other.x)
            && f64_same(self.y, other.y)
            && f64_same(self.width, other.width)
            && f64_same(self.height, other.height)
    }
}

impl RectD {
    /// C# `Right` = `X + Width`.
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    /// C# `Bottom` = `Y + Height`.
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// C# `Contains(PointD)`, closed on all four sides, with the C# formula exactly:
    /// `p.x >= x && p.x - width <= x && p.y >= y && p.y - height <= y` (NOT `p.x <= x + width`: the rounding differs).
    /// A negative width or height contains no point.
    pub fn contains(&self, p: PointD) -> bool {
        p.x >= self.x && p.x - self.width <= self.x && p.y >= self.y && p.y - self.height <= self.y
    }
}

/// `PointerButton` = `System.Windows.Input.MouseButton` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Left = 0,
    Middle = 1,
    Right = 2,
    XButton1 = 3,
    XButton2 = 4,
}

/// `KeyModifiers` = `System.Windows.Input.ModifierKeys` ([Flags], raw bits kept as in C#).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KeyModifiers(pub u8);

impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const ALT: Self = Self(1);
    pub const CONTROL: Self = Self(2);
    pub const SHIFT: Self = Self(4);
    pub const WINDOWS: Self = Self(8);

    /// C# `HasFlag(other)`: `(self & other) == other`. `contains(NONE)` is always true (C# `HasFlag(None)`).
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl std::ops::BitOr for KeyModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
