//! Port of `PhotoReview.App.Shared/ViewModels/ViewerState.cs` (450 lines; `public sealed partial class ViewerState : ObservableObject`).
//! Plan row W8-06a, batch B1, contract `contracts/W8-06a.md`.
//!
//! Decisions this file follows (the contract has the full list):
//! - D-W8-1: every C# `[ObservableProperty]` notification becomes a bit in a returned `ViewerChanges` set. A property is in the set
//!   only when its value changed (`f64_same` for doubles: NaN equals NaN, `-0.0` equals `0.0`, as C# `EqualityComparer`).
//!   Dependents named by `[NotifyPropertyChangedFor]` are added whenever their source changed.
//! - The C# `ZoomModeChanged` event becomes `ViewerUpdate::zoom_mode_changed`. The C# `SourceSizeSwapping` event becomes the
//!   `on_swapping` callback of `swap_source_size`, called with the state BEFORE the size changes.
//! - `ViewerStretchMode` is `viewport::types::ViewerStretchMode` (W8-01a, D-W8-01a-2). The C# default of `_stretch` is `Uniform`,
//!   not the `None` default of that enum: `new()` sets it.
//! - `ViewerState` implements the `viewport::fit::FitViewer` seam (W8-01d). The trait method returns `()`, so the impl keeps the
//!   zoom-mode flag in `pending_fit_zoom_mode_changed` (open question OQ-1 in the contract).

use crate::input::primitives::f64_same;
use crate::viewport::fit::FitViewer;
use crate::viewport::types::ViewerStretchMode;
use pr_core::model::enums::{InitialViewMode, ScalingQuality};

/// C# `ViewerState.MinZoom` (click-to-zoom range lower bound, 10 %). `SetZoom` clamps to `[MIN_ZOOM, MAX_ZOOM]`.
pub const MIN_ZOOM: f64 = 0.10;
/// C# `ViewerState.MaxZoom` (800 %).
pub const MAX_ZOOM: f64 = 8.0;
/// C# `ViewerState.MinStepZoom` (25 %): the lowest zoom a step reaches.
pub const MIN_STEP_ZOOM: f64 = 0.25;
/// C# `ViewerState.MaxStepZoom` (400 %): the highest zoom a step reaches.
pub const MAX_STEP_ZOOM: f64 = 4.0;
/// C# `ViewerState.ZoomStep` default (Q-R41: the setting `KeyboardZoomStepPercent` overrides it).
pub const DEFAULT_ZOOM_STEP: f64 = 0.25;
/// C# `ViewerState.ActualSizeZoom` (one source pixel per device pixel, ADR 0008).
pub const ACTUAL_SIZE_ZOOM: f64 = 1.0;

/// C# `ViewerState.MinPlausibleDpiScale` / `MaxPlausibleDpiScale`: outside this closed range a DPI reading is broken (-> 1.0).
const MIN_PLAUSIBLE_DPI_SCALE: f64 = 0.25;
const MAX_PLAUSIBLE_DPI_SCALE: f64 = 16.0;
/// C# `IsFit`: `|Zoom - 1.0| < 0.001` (strict).
const IS_FIT_TOLERANCE: f64 = 0.001;
/// C# `CalculateStepZoom` edge tolerance (`MaxStepZoom - 0.0005` and `MinStepZoom + 0.0005`).
const STEP_EDGE_TOLERANCE: f64 = 0.0005;

/// The `ViewerProperty` names below are the C# property names, used by golden comparisons (`csharp_name`).
/// One variant per C# observable property or computed property that the C# code notifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewerProperty {
    Zoom,
    Stretch,
    SourcePixelWidth,
    SourcePixelHeight,
    DpiScale,
    IsFullscreen,
    ScalingQuality,
    ForceHighQualityScaling,
    MaxImageWidth,
    MaxImageHeight,
    IsFit,
    ImageWidth,
    ImageHeight,
    DisplayZoomPercent,
    EffectiveScalingQuality,
}

impl ViewerProperty {
    /// Every variant, in the fixed order of `ViewerChanges::names`.
    pub const ALL: [ViewerProperty; 15] = [
        ViewerProperty::Zoom,
        ViewerProperty::Stretch,
        ViewerProperty::SourcePixelWidth,
        ViewerProperty::SourcePixelHeight,
        ViewerProperty::DpiScale,
        ViewerProperty::IsFullscreen,
        ViewerProperty::ScalingQuality,
        ViewerProperty::ForceHighQualityScaling,
        ViewerProperty::MaxImageWidth,
        ViewerProperty::MaxImageHeight,
        ViewerProperty::IsFit,
        ViewerProperty::ImageWidth,
        ViewerProperty::ImageHeight,
        ViewerProperty::DisplayZoomPercent,
        ViewerProperty::EffectiveScalingQuality,
    ];

    /// The C# property name (`nameof(ViewerState.X)`), e.g. `"DisplayZoomPercent"`.
    pub fn csharp_name(self) -> &'static str {
        match self {
            ViewerProperty::Zoom => "Zoom",
            ViewerProperty::Stretch => "Stretch",
            ViewerProperty::SourcePixelWidth => "SourcePixelWidth",
            ViewerProperty::SourcePixelHeight => "SourcePixelHeight",
            ViewerProperty::DpiScale => "DpiScale",
            ViewerProperty::IsFullscreen => "IsFullscreen",
            ViewerProperty::ScalingQuality => "ScalingQuality",
            ViewerProperty::ForceHighQualityScaling => "ForceHighQualityScaling",
            ViewerProperty::MaxImageWidth => "MaxImageWidth",
            ViewerProperty::MaxImageHeight => "MaxImageHeight",
            ViewerProperty::IsFit => "IsFit",
            ViewerProperty::ImageWidth => "ImageWidth",
            ViewerProperty::ImageHeight => "ImageHeight",
            ViewerProperty::DisplayZoomPercent => "DisplayZoomPercent",
            ViewerProperty::EffectiveScalingQuality => "EffectiveScalingQuality",
        }
    }

    fn bit(self) -> u16 {
        1u16 << (self as u16)
    }
}

/// The set of properties a mutating call changed (C# `PropertyChanged` names, as a bit set). `Default` is the empty set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewerChanges(u16);

impl ViewerChanges {
    /// The empty set (nothing changed).
    pub const EMPTY: ViewerChanges = ViewerChanges(0);

    /// Adds one property.
    pub fn insert(&mut self, property: ViewerProperty) {
        self.0 |= property.bit();
    }

    /// True when `property` is in the set.
    pub fn contains(self, property: ViewerProperty) -> bool {
        self.0 & property.bit() != 0
    }

    /// True when nothing changed.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Set union.
    pub fn union(self, other: ViewerChanges) -> ViewerChanges {
        ViewerChanges(self.0 | other.0)
    }

    /// The C# names of the members of the set, in `ViewerProperty::ALL` order (not in insertion order).
    pub fn names(self) -> Vec<&'static str> {
        ViewerProperty::ALL
            .iter()
            .filter(|p| self.contains(**p))
            .map(|p| p.csharp_name())
            .collect()
    }
}

/// What a mutating call did: the properties that changed, and whether the C# `ZoomModeChanged` event would be raised.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewerUpdate {
    /// Properties whose value changed (with their dependents, see the module doc).
    pub changes: ViewerChanges,
    /// C# `ZoomModeChanged` was raised. `SetZoom`, `ResetFit` and `ApplyInitialViewMode` (via them) raise it.
    pub zoom_mode_changed: bool,
}

/// C# `FitAxis` (private enum): which Fit dimension the current zoom came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FitAxis {
    None,
    Width,
    Height,
}

/// C# `ViewerState`: zoom, stretch mode, source size, DPI scale, fullscreen and scaling quality of the viewed image.
/// Single owner (the view model); no shared handles, no observer lists (D-W8-1).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewerState {
    zoom: f64,
    stretch: ViewerStretchMode,
    source_pixel_width: i32,
    source_pixel_height: i32,
    dpi_scale: f64,
    is_fullscreen: bool,
    scaling_quality: ScalingQuality,
    force_high_quality_scaling: bool,
    max_image_width: f64,
    max_image_height: f64,
    zoom_step: f64,
    fit_axis: FitAxis,
    fit_axis_viewport: f64,
    pending_fit_zoom_mode_changed: bool,
}

/// C# `ViewerState.CalculateDisplaySize(originalWidth, originalHeight, zoom, dpiScale)`: the on-screen DIP size of an image of
/// `original_width` x `original_height` source pixels at original-relative `zoom`. `scale = zoom / normalize_dpi(dpi_scale)`.
pub fn calculate_display_size(original_width: i32, original_height: i32, zoom: f64, dpi_scale: f64) -> (f64, f64) {
    let scale = zoom / normalize_dpi(dpi_scale);
    (f64::from(original_width) * scale, f64::from(original_height) * scale)
}

/// C# `ViewerState.CalculateStepZoom(current, step)` (internal static): the zoom one step away, or `None` for "no change".
/// NaN is propagated like C# `Math.Min`/`Math.Max` (NOT `f64::min`, which ignores NaN); see contract trap T3.
pub fn calculate_step_zoom(current: f64, step: f64) -> Option<f64> {
    if step > 0.0 {
        if current >= MAX_STEP_ZOOM - STEP_EDGE_TOLERANCE {
            return None;
        }
        Some(math_min(current + step, MAX_STEP_ZOOM))
    } else {
        if current <= MIN_STEP_ZOOM + STEP_EDGE_TOLERANCE {
            return None;
        }
        if current > MAX_STEP_ZOOM {
            return Some(MAX_STEP_ZOOM);
        }
        Some(math_max(current + step, MIN_STEP_ZOOM))
    }
}

impl Default for ViewerState {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewerState {
    /// C# `new ViewerState()` with its field initialisers: zoom 1.0, `Uniform`, size 0 x 0, DPI 1.0, not fullscreen,
    /// `HighQuality`, no force, both viewport limits `+Infinity`, zoom step 0.25.
    pub fn new() -> Self {
        Self {
            zoom: 1.0,
            stretch: ViewerStretchMode::Uniform,
            source_pixel_width: 0,
            source_pixel_height: 0,
            dpi_scale: 1.0,
            is_fullscreen: false,
            scaling_quality: ScalingQuality::HighQuality,
            force_high_quality_scaling: false,
            max_image_width: f64::INFINITY,
            max_image_height: f64::INFINITY,
            zoom_step: DEFAULT_ZOOM_STEP,
            fit_axis: FitAxis::None,
            fit_axis_viewport: 0.0,
            pending_fit_zoom_mode_changed: false,
        }
    }

    // ---- read-only views (C# get accessors and computed properties) ----

    /// C# `Zoom`: original-relative zoom. In Fit it is 1.0 by convention.
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// C# `Stretch`.
    pub fn stretch(&self) -> ViewerStretchMode {
        self.stretch
    }

    /// C# `SourcePixelWidth` (0 = unknown).
    pub fn source_pixel_width(&self) -> i32 {
        self.source_pixel_width
    }

    /// C# `SourcePixelHeight` (0 = unknown).
    pub fn source_pixel_height(&self) -> i32 {
        self.source_pixel_height
    }

    /// C# `DpiScale` as set (not normalised; `normalize_dpi` applies at the use sites).
    pub fn dpi_scale(&self) -> f64 {
        self.dpi_scale
    }

    /// C# `IsFullscreen`.
    pub fn is_fullscreen(&self) -> bool {
        self.is_fullscreen
    }

    /// C# `ScalingQuality` (the setting).
    pub fn scaling_quality(&self) -> ScalingQuality {
        self.scaling_quality
    }

    /// C# `ForceHighQualityScaling` (set by the Refresh command for the image on screen).
    pub fn force_high_quality_scaling(&self) -> bool {
        self.force_high_quality_scaling
    }

    /// C# `MaxImageWidth` (viewport bound in DIP; `+Infinity` when no Fit bound is set).
    pub fn max_image_width(&self) -> f64 {
        self.max_image_width
    }

    /// C# `MaxImageHeight`.
    pub fn max_image_height(&self) -> f64 {
        self.max_image_height
    }

    /// C# `ZoomStep` (not observable in C#; no change set).
    pub fn zoom_step(&self) -> f64 {
        self.zoom_step
    }

    /// C# `IsFit`: `Stretch == Uniform && |Zoom - 1.0| < 0.001`.
    pub fn is_fit(&self) -> bool {
        self.stretch == ViewerStretchMode::Uniform && (self.zoom - 1.0).abs() < IS_FIT_TOLERANCE
    }

    /// C# `ImageWidth` (DIP): NaN (auto) in Fit or while the source size is unknown, else `calculate_display_size` at `Zoom`.
    pub fn image_width(&self) -> f64 {
        self.display_size().0
    }

    /// C# `ImageHeight` (DIP); see `image_width`.
    pub fn image_height(&self) -> f64 {
        self.display_size().1
    }

    /// C# `FitZoom`: original-relative zoom that fills the Fit viewport on both axes (0 = unknown).
    pub fn fit_zoom(&self) -> f64 {
        if self.source_pixel_width <= 0 || self.source_pixel_height <= 0 {
            return 0.0;
        }
        let usable = |v: f64| v.is_finite() && v > 1.0;
        if !usable(self.max_image_width) || !usable(self.max_image_height) {
            return 0.0;
        }
        let dpi = normalize_dpi(self.dpi_scale);
        let fit = math_min(
            (self.max_image_width * dpi) / f64::from(self.source_pixel_width),
            (self.max_image_height * dpi) / f64::from(self.source_pixel_height),
        );
        if fit.is_finite() && fit > 0.0 { fit } else { 0.0 }
    }

    /// C# `FitWidthZoom`: zoom at which the width fills `MaxImageWidth` at the current DPI (0 = unknown).
    pub fn fit_width_zoom(&self) -> f64 {
        self.fit_axis_zoom(self.max_image_width, self.source_pixel_width)
    }

    /// C# `FitHeightZoom`: zoom at which the height fills `MaxImageHeight` at the current DPI (0 = unknown).
    pub fn fit_height_zoom(&self) -> f64 {
        self.fit_axis_zoom(self.max_image_height, self.source_pixel_height)
    }

    /// C# `DisplayZoomPercent`: `(int)Math.Round(zoom_or_fit * 100)` with C# default rounding (ties to even, trap T9).
    pub fn display_zoom_percent(&self) -> i32 {
        let value = if self.is_fit() {
            let fit = self.fit_zoom();
            if fit > 0.0 { fit } else { 1.0 }
        } else {
            self.zoom
        };
        (value * 100.0).round_ties_even() as i32
    }

    /// C# `EffectiveZoom`: `None` in Fit, else `Some(Zoom)`.
    pub fn effective_zoom(&self) -> Option<f64> {
        if self.is_fit() { None } else { Some(self.zoom) }
    }

    /// C# `EffectiveScalingQuality`: `HighQuality` when `force_high_quality_scaling`, else `scaling_quality`.
    pub fn effective_scaling_quality(&self) -> ScalingQuality {
        if self.force_high_quality_scaling {
            ScalingQuality::HighQuality
        } else {
            self.scaling_quality
        }
    }

    // ---- raw property setters (C# generated setters: change only when the value differs, see D-W8-1) ----

    /// C# `Zoom` setter, raw (no clamp, no mode change, no `ZoomModeChanged`). Changes: `Zoom`, `IsFit`, `ImageWidth`,
    /// `ImageHeight`, `DisplayZoomPercent` when the value differs.
    pub fn set_zoom_raw(&mut self, value: f64) -> ViewerChanges {
        if f64_same(self.zoom, value) {
            return ViewerChanges::EMPTY;
        }
        self.zoom = value;
        Self::zoom_like_changes(ViewerProperty::Zoom)
    }

    /// C# `Stretch` setter. Changes: `Stretch`, `IsFit`, `ImageWidth`, `ImageHeight`, `DisplayZoomPercent` when it differs.
    pub fn set_stretch(&mut self, value: ViewerStretchMode) -> ViewerChanges {
        if self.stretch == value {
            return ViewerChanges::EMPTY;
        }
        self.stretch = value;
        Self::zoom_like_changes(ViewerProperty::Stretch)
    }

    /// C# `DpiScale` setter. Changes: `DpiScale`, `ImageWidth`, `ImageHeight`, `DisplayZoomPercent` when it differs.
    pub fn set_dpi_scale(&mut self, value: f64) -> ViewerChanges {
        if f64_same(self.dpi_scale, value) {
            return ViewerChanges::EMPTY;
        }
        self.dpi_scale = value;
        Self::size_changes(ViewerProperty::DpiScale)
    }

    /// C# `MaxImageWidth` setter. Changes: `MaxImageWidth`, `DisplayZoomPercent` when it differs.
    pub fn set_max_image_width(&mut self, value: f64) -> ViewerChanges {
        if f64_same(self.max_image_width, value) {
            return ViewerChanges::EMPTY;
        }
        self.max_image_width = value;
        Self::limit_changes(ViewerProperty::MaxImageWidth)
    }

    /// C# `MaxImageHeight` setter. Changes: `MaxImageHeight`, `DisplayZoomPercent` when it differs.
    pub fn set_max_image_height(&mut self, value: f64) -> ViewerChanges {
        if f64_same(self.max_image_height, value) {
            return ViewerChanges::EMPTY;
        }
        self.max_image_height = value;
        Self::limit_changes(ViewerProperty::MaxImageHeight)
    }

    /// C# `ScalingQuality` setter. Changes: `ScalingQuality`, `EffectiveScalingQuality` when it differs.
    pub fn set_scaling_quality(&mut self, value: ScalingQuality) -> ViewerChanges {
        if self.scaling_quality == value {
            return ViewerChanges::EMPTY;
        }
        self.scaling_quality = value;
        Self::of(&[ViewerProperty::ScalingQuality, ViewerProperty::EffectiveScalingQuality])
    }

    /// C# `ForceHighQualityScaling` setter. Changes: `ForceHighQualityScaling`, `EffectiveScalingQuality` when it differs.
    pub fn set_force_high_quality_scaling(&mut self, value: bool) -> ViewerChanges {
        if self.force_high_quality_scaling == value {
            return ViewerChanges::EMPTY;
        }
        self.force_high_quality_scaling = value;
        Self::of(&[
            ViewerProperty::ForceHighQualityScaling,
            ViewerProperty::EffectiveScalingQuality,
        ])
    }

    /// C# `IsFullscreen` setter. Changes: `IsFullscreen` when it differs.
    pub fn set_fullscreen(&mut self, value: bool) -> ViewerChanges {
        if self.is_fullscreen == value {
            return ViewerChanges::EMPTY;
        }
        self.is_fullscreen = value;
        Self::of(&[ViewerProperty::IsFullscreen])
    }

    /// C# `ZoomStep` setter (plain property, no notification, no clamp: negative values are accepted, trap T11).
    pub fn set_zoom_step(&mut self, value: f64) {
        self.zoom_step = value;
    }

    // ---- commands (C# methods) ----

    /// C# `SetSourceSize(width, height, newImage)`. Negative sizes become 0. The Fit-axis memory is forgotten when `new_image` or
    /// the (clamped) size changed. Changes: `SourcePixelWidth`/`SourcePixelHeight` (+ their dependents) when they differ.
    pub fn set_source_size(&mut self, width: i32, height: i32, new_image: bool) -> ViewerUpdate {
        let width = width.max(0);
        let height = height.max(0);
        if new_image || width != self.source_pixel_width || height != self.source_pixel_height {
            self.clear_fit_axis();
        }
        let mut changes = ViewerChanges::EMPTY;
        if self.source_pixel_width != width {
            self.source_pixel_width = width;
            changes = changes.union(Self::size_changes(ViewerProperty::SourcePixelWidth));
        }
        if self.source_pixel_height != height {
            self.source_pixel_height = height;
            changes = changes.union(Self::size_changes(ViewerProperty::SourcePixelHeight));
        }
        ViewerUpdate {
            changes,
            zoom_mode_changed: false,
        }
    }

    /// C# `SwapSourceSize(width, height)`: the bitmap behind the same image changed its pixel size. When zoomed (not Fit, both old
    /// and new sizes > 0) `on_swapping` is called with the state BEFORE the size changes (C# `SourceSizeSwapping`); the Fit-axis
    /// memory survives and a Fit-width/height zoom is recomputed for the new size. Otherwise it is `set_source_size` without
    /// `new_image`. Same size after clamping: no change and no callback.
    pub fn swap_source_size(
        &mut self,
        width: i32,
        height: i32,
        on_swapping: &mut dyn FnMut(&ViewerState),
    ) -> ViewerUpdate {
        let width = width.max(0);
        let height = height.max(0);
        if width == self.source_pixel_width && height == self.source_pixel_height {
            return ViewerUpdate::default();
        }
        if self.source_pixel_width <= 0 || self.source_pixel_height <= 0 || width <= 0 || height <= 0 || self.is_fit() {
            return self.set_source_size(width, height, false);
        }
        on_swapping(&*self);
        let saved_axis = self.fit_axis;
        let saved_viewport = self.fit_axis_viewport;
        let mut update = self.set_source_size(width, height, false);
        self.fit_axis = saved_axis;
        self.fit_axis_viewport = saved_viewport;
        let dpi = normalize_dpi(self.dpi_scale);
        let fit_zoom = match saved_axis {
            FitAxis::Width => (saved_viewport * dpi) / f64::from(width),
            FitAxis::Height => (saved_viewport * dpi) / f64::from(height),
            FitAxis::None => 0.0,
        };
        if fit_zoom.is_finite() && fit_zoom > 0.0 {
            let changes = self.set_zoom_raw(fit_zoom.clamp(MIN_ZOOM, MAX_ZOOM));
            update.changes = update.changes.union(changes);
        }
        update
    }

    /// C# `SetZoom(value)`: `ZoomStep`-free absolute zoom. NaN is ignored (no change, no event). Otherwise: `Stretch = None`,
    /// both viewport limits to `+Infinity`, the Fit-axis memory is set to `None`, `Zoom = clamp(value, MIN_ZOOM, MAX_ZOOM)`,
    /// `zoom_mode_changed = true`.
    pub fn set_zoom(&mut self, value: f64) -> ViewerUpdate {
        self.set_zoom_core(value, FitAxis::None, 0.0)
    }

    /// C# `ZoomToActualSize()` = `set_zoom(ACTUAL_SIZE_ZOOM)`.
    pub fn zoom_to_actual_size(&mut self) -> ViewerUpdate {
        self.set_zoom(ACTUAL_SIZE_ZOOM)
    }

    /// C# `ZoomToFitWidth()`: `set_zoom(fit_width_zoom)` with the Fit-width axis remembering `MaxImageWidth`; no-op when unknown.
    pub fn zoom_to_fit_width(&mut self) -> ViewerUpdate {
        let fit = self.fit_width_zoom();
        if fit > 0.0 {
            // T4: read the limit before `set_zoom_core` resets it to +Infinity.
            let viewport = self.max_image_width;
            self.set_zoom_core(fit, FitAxis::Width, viewport)
        } else {
            ViewerUpdate::default()
        }
    }

    /// C# `ZoomToFitHeight()`: the height twin of `zoom_to_fit_width`.
    pub fn zoom_to_fit_height(&mut self) -> ViewerUpdate {
        let fit = self.fit_height_zoom();
        if fit > 0.0 {
            let viewport = self.max_image_height;
            self.set_zoom_core(fit, FitAxis::Height, viewport)
        } else {
            ViewerUpdate::default()
        }
    }

    /// C# `ZoomIn()` = `StepZoom(ZoomStep)`.
    pub fn zoom_in(&mut self) -> ViewerUpdate {
        self.step_zoom(self.zoom_step)
    }

    /// C# `ZoomOut()` = `StepZoom(-ZoomStep)`.
    pub fn zoom_out(&mut self) -> ViewerUpdate {
        self.step_zoom(-self.zoom_step)
    }

    /// C# `WheelZoom(delta)`: `delta > 0` steps up by `ZoomStep`, any other delta (including 0) steps down.
    pub fn wheel_zoom(&mut self, delta: i32) -> ViewerUpdate {
        let step = if delta > 0 { self.zoom_step } else { -self.zoom_step };
        self.step_zoom(step)
    }

    /// C# `ResetFit(viewportWidth, viewportHeight)`: Fit-axis memory cleared, `Zoom = 1.0`, `Stretch = Uniform`,
    /// `update_viewport(w, h, force: true)`, and `zoom_mode_changed = true` ALWAYS (even when nothing changed).
    pub fn reset_fit(&mut self, viewport_width: f64, viewport_height: f64) -> ViewerUpdate {
        self.clear_fit_axis();
        let mut changes = self.set_zoom_raw(1.0);
        changes = changes.union(self.set_stretch(ViewerStretchMode::Uniform));
        changes = changes.union(self.update_viewport(viewport_width, viewport_height, true).changes);
        ViewerUpdate {
            changes,
            zoom_mode_changed: true,
        }
    }

    /// C# `UpdateViewport(viewportWidth, viewportHeight, force)`. First, when the Fit-axis memory is set and both readings are
    /// usable (> 1 and finite), the memory takes the dimension it fills. Then, unless `force` or `Stretch == Uniform`, nothing else.
    /// Otherwise a usable reading sets both viewport limits. `zoom_mode_changed` is never set here.
    pub fn update_viewport(&mut self, viewport_width: f64, viewport_height: f64, force: bool) -> ViewerUpdate {
        let usable =
            viewport_width > 1.0 && viewport_height > 1.0 && viewport_width.is_finite() && viewport_height.is_finite();
        if self.fit_axis != FitAxis::None && usable {
            self.fit_axis_viewport = if self.fit_axis == FitAxis::Width {
                viewport_width
            } else {
                viewport_height
            };
        }
        if !force && self.stretch != ViewerStretchMode::Uniform {
            return ViewerUpdate::default();
        }
        let mut changes = ViewerChanges::EMPTY;
        if usable {
            changes = changes.union(self.set_max_image_width(viewport_width));
            changes = changes.union(self.set_max_image_height(viewport_height));
        }
        ViewerUpdate {
            changes,
            zoom_mode_changed: false,
        }
    }

    /// C# `ApplyInitialViewMode(mode, viewportWidth, viewportHeight, clickZoomPercent, keepZoomAcrossImages)`.
    /// Returns `None` (nothing done) when `keep_zoom_across_images`; otherwise `Some(update)`. Mode table in the contract.
    /// `DEFAULT_CLICK_ZOOM_PERCENT` is the value the C# default parameter takes; the caller passes it explicitly.
    pub fn apply_initial_view_mode(
        &mut self,
        mode: InitialViewMode,
        viewport_width: f64,
        viewport_height: f64,
        click_zoom_percent: i32,
        keep_zoom_across_images: bool,
    ) -> Option<ViewerUpdate> {
        if keep_zoom_across_images {
            return None;
        }
        let update = match mode {
            InitialViewMode::Fit => self.reset_fit(viewport_width, viewport_height),
            InitialViewMode::FitWidth => {
                // T8: the viewport refresh comes before the zoom.
                let first = self.update_viewport(viewport_width, viewport_height, true);
                Self::merge(first, self.zoom_to_fit_width())
            }
            InitialViewMode::FitHeight => {
                let first = self.update_viewport(viewport_width, viewport_height, true);
                Self::merge(first, self.zoom_to_fit_height())
            }
            InitialViewMode::ClickZoomLevel => self.set_zoom(f64::from(click_zoom_percent) / 100.0),
            InitialViewMode::Percent200 | InitialViewMode::Percent400 => self.set_zoom(2.0),
            _ => self.set_zoom(1.0),
        };
        Some(update)
    }

    /// C# `ToggleFullscreen()`: `IsFullscreen = !IsFullscreen`.
    pub fn toggle_fullscreen(&mut self) -> ViewerUpdate {
        let changes = self.set_fullscreen(!self.is_fullscreen);
        ViewerUpdate {
            changes,
            zoom_mode_changed: false,
        }
    }

    /// C# `ExitFullscreen()`: `IsFullscreen = false` (no change when already false).
    pub fn exit_fullscreen(&mut self) -> ViewerUpdate {
        let changes = self.set_fullscreen(false);
        ViewerUpdate {
            changes,
            zoom_mode_changed: false,
        }
    }

    /// True when a `FitViewer::reset_fit` call raised `ZoomModeChanged` since the last call of this method. The Fit driver (W8-01d)
    /// cannot see the `ViewerUpdate` through the `()` trait method, so the shell calls this after `apply_fit_start`/`apply_fit_resume`.
    pub fn take_pending_zoom_mode_changed(&mut self) -> bool {
        std::mem::take(&mut self.pending_fit_zoom_mode_changed)
    }

    // ---- private helpers ----

    fn of(properties: &[ViewerProperty]) -> ViewerChanges {
        let mut changes = ViewerChanges::EMPTY;
        for property in properties {
            changes.insert(*property);
        }
        changes
    }

    /// Source plus the dependents of `Zoom` / `Stretch`.
    fn zoom_like_changes(source: ViewerProperty) -> ViewerChanges {
        Self::of(&[
            source,
            ViewerProperty::IsFit,
            ViewerProperty::ImageWidth,
            ViewerProperty::ImageHeight,
            ViewerProperty::DisplayZoomPercent,
        ])
    }

    /// Source plus the dependents of the source size and the DPI scale.
    fn size_changes(source: ViewerProperty) -> ViewerChanges {
        Self::of(&[
            source,
            ViewerProperty::ImageWidth,
            ViewerProperty::ImageHeight,
            ViewerProperty::DisplayZoomPercent,
        ])
    }

    /// Source plus the dependent of the viewport limits.
    fn limit_changes(source: ViewerProperty) -> ViewerChanges {
        Self::of(&[source, ViewerProperty::DisplayZoomPercent])
    }

    fn merge(first: ViewerUpdate, second: ViewerUpdate) -> ViewerUpdate {
        ViewerUpdate {
            changes: first.changes.union(second.changes),
            zoom_mode_changed: first.zoom_mode_changed || second.zoom_mode_changed,
        }
    }

    fn display_size(&self) -> (f64, f64) {
        if self.is_fit() || self.source_pixel_width <= 0 || self.source_pixel_height <= 0 {
            (f64::NAN, f64::NAN)
        } else {
            calculate_display_size(
                self.source_pixel_width,
                self.source_pixel_height,
                self.zoom,
                self.dpi_scale,
            )
        }
    }

    /// Shared by `fit_width_zoom` and `fit_height_zoom`.
    fn fit_axis_zoom(&self, limit: f64, source: i32) -> f64 {
        if source <= 0 || !limit.is_finite() || limit <= 1.0 {
            return 0.0;
        }
        let fit = (limit * normalize_dpi(self.dpi_scale)) / f64::from(source);
        if fit.is_finite() && fit > 0.0 { fit } else { 0.0 }
    }

    fn clear_fit_axis(&mut self) {
        self.fit_axis = FitAxis::None;
        self.fit_axis_viewport = 0.0;
    }

    fn step_base(&self) -> f64 {
        let fit = self.fit_zoom();
        if self.is_fit() && fit > 0.0 { fit } else { self.zoom }
    }

    fn step_zoom(&mut self, step: f64) -> ViewerUpdate {
        match calculate_step_zoom(self.step_base(), step) {
            Some(next) => self.set_zoom(next),
            None => ViewerUpdate::default(),
        }
    }

    fn set_zoom_core(&mut self, value: f64, axis: FitAxis, axis_viewport: f64) -> ViewerUpdate {
        if value.is_nan() {
            return ViewerUpdate::default();
        }
        let mut changes = self.set_stretch(ViewerStretchMode::None);
        changes = changes.union(self.set_max_image_width(f64::INFINITY));
        changes = changes.union(self.set_max_image_height(f64::INFINITY));
        self.fit_axis = axis;
        self.fit_axis_viewport = axis_viewport;
        changes = changes.union(self.set_zoom_raw(value.clamp(MIN_ZOOM, MAX_ZOOM)));
        ViewerUpdate {
            changes,
            zoom_mode_changed: true,
        }
    }
}

impl FitViewer for ViewerState {
    /// C# `ViewerState.ResetFit(viewportWidth, viewportHeight)` through the W8-01d seam. Calls the inherent `reset_fit` (inherent
    /// methods take precedence on `ViewerState::reset_fit`) and records its `zoom_mode_changed` in `pending_fit_zoom_mode_changed`.
    fn reset_fit(&mut self, viewport_width: f64, viewport_height: f64) {
        let update = ViewerState::reset_fit(self, viewport_width, viewport_height);
        self.pending_fit_zoom_mode_changed |= update.zoom_mode_changed;
    }
}

/// C# `NormalizeDpi`: `dpi_scale` when it is in `[MIN_PLAUSIBLE_DPI_SCALE, MAX_PLAUSIBLE_DPI_SCALE]`, else 1.0 (NaN too).
fn normalize_dpi(dpi_scale: f64) -> f64 {
    if (MIN_PLAUSIBLE_DPI_SCALE..=MAX_PLAUSIBLE_DPI_SCALE).contains(&dpi_scale) {
        dpi_scale
    } else {
        1.0
    }
}

/// C# `Math.Min` for doubles: NaN if either operand is NaN (trap T3).
fn math_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// C# `Math.Max` for doubles: NaN if either operand is NaN (trap T3).
fn math_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}
