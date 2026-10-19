//! Port of `PhotoReview.Shell.Win32/Overlay/OverlayContracts.cs` (overlay part of C-11, WP-17) and of the interface
//! `IOpacityTarget` declared in `Overlay/Animator.cs` (moved here: elements implement it, the animator uses it).
//!
//! Baseline `d92d6aae`. Owned by W8-15a. Decisions are in `contracts/W8-15a.md`.
//! - `IOverlayElement` -> trait [`OverlayElement`] (object-safe host surface; implemented by a blanket impl over `ElementNode`).
//! - `IAnimator` -> trait [`OverlayAnimator`] (W8-15b implements it as `animator::Animator`; the name avoids that struct).
//! - `IOpacityTarget` -> trait [`OpacityTarget`], supertrait of `OverlayElement` (the C# `IOverlayElement.Opacity`).
//! - `OverlayLayoutContext` (C# `readonly record struct`) and `Easing`.

use std::fmt;
use std::time::Duration;

use crate::input::primitives::{PointD, RectD, SizeD};
use crate::services::render::{DrawContext, TextRenderer};

/// C# `OverlayLayoutContext(SizeD Client, double DpiScale, ITextRenderer Text)`.
///
/// `dpi_scale` is carried for the drawing layer only: layout never reads it (C# `OverlayElement` doc). `text` is a shared
/// borrow of the renderer, so the context is `Copy`. No `PartialEq`: nothing compares contexts, and the C# record compares
/// the renderer by reference.
#[derive(Clone, Copy)]
pub struct OverlayLayoutContext<'a> {
    /// Client size in DIP (C# `Client`).
    pub client: SizeD,
    /// Monitor DPI scale (C# `DpiScale`); not used by layout.
    pub dpi_scale: f64,
    /// Text measuring and layout service (C# `Text`).
    pub text: &'a dyn TextRenderer,
}

impl<'a> OverlayLayoutContext<'a> {
    /// C# primary constructor.
    pub fn new(client: SizeD, dpi_scale: f64, text: &'a dyn TextRenderer) -> Self {
        Self {
            client,
            dpi_scale,
            text,
        }
    }
}

impl fmt::Debug for OverlayLayoutContext<'_> {
    /// Prints `client` and `dpi_scale`; the renderer is shown as `"<text renderer>"` (a `dyn` has no Debug).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayLayoutContext")
            .field("client", &self.client)
            .field("dpi_scale", &self.dpi_scale)
            .field("text", &"<text renderer>")
            .finish()
    }
}

/// C# `IOpacityTarget`: an element whose opacity an animator can write. Blanket-implemented for every `ElementNode`
/// (see `element.rs`), so no element implements it by hand.
pub trait OpacityTarget {
    /// Current opacity, always within `0.0..=1.0` (see `ElementBase::set_opacity` for the clamp).
    fn opacity(&self) -> f32;

    /// C# `Opacity` setter: NaN becomes `0.0`, other values are clamped to `0.0..=1.0`.
    fn set_opacity(&mut self, value: f32);
}

/// C# `IOverlayElement`: the surface the host and the animator use. Object-safe: the host holds `dyn OverlayElement`.
pub trait OverlayElement: OpacityTarget {
    /// C# `Id` (matches the WPF `x:Name`, ordinal comparison).
    fn id(&self) -> &str;

    /// C# `IsVisible`: `false` means collapsed (no space, no drawing, no hits).
    fn is_visible(&self) -> bool;

    /// C# `Bounds`: the box after the last arrange, DIP, in window coordinates (margin excluded).
    fn bounds(&self) -> RectD;

    /// C# `Arrange(in OverlayLayoutContext)`: measure in the whole client, then place by alignment.
    fn arrange(&mut self, context: &OverlayLayoutContext<'_>);

    /// C# `Render(IDrawContext)`: skipped when hidden or at opacity `<= 0`; otherwise opacity pushed around the content.
    fn render(&self, dc: &mut dyn DrawContext);

    /// C# `HitTest(PointD)`: visible, hit-test enabled, opacity `> 0`, and inside `Bounds` (click-through otherwise).
    fn hit_test(&self, point: PointD) -> bool;
}

/// C# `IAnimator`: tweens an element's opacity by element id. Implemented by W8-15b (`animator::Animator`, clock injected).
pub trait OverlayAnimator {
    /// C# `FadeTo(elementId, target, duration, easing)`. Same target as the running tween: keep it. Other target: cancel it
    /// and start from the current opacity.
    fn fade_to(&mut self, element_id: &str, target: f32, duration: Duration, easing: Easing);

    /// C# `IsAnimating`: at least one tween is running.
    fn is_animating(&self) -> bool;
}

/// C# `Easing`. Discriminants match C# (`Linear = 0`, `QuadraticOut = 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Easing {
    Linear = 0,
    QuadraticOut = 1,
}
