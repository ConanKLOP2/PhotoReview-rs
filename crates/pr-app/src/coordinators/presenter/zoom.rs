//! Port of `PhotoReview.App.Shared/Coordinators/ImagePresenter.cs` lines 120 to 159 and 194 to 201 (baseline d92d6aae):
//! `ShowZoomDetailImage` / `ShowZoomDetailImageCore` (the swap of the displayed bitmap by the zoom-detail loader) and
//! `SetViewerZoom`, plus the glue the Rust design needs between the loader's effects and the presenter. Contract:
//! `contracts/W8-07h.md`. Target: `crates/pr-app/src/coordinators/presenter/zoom.rs` (`coordinators::presenter::zoom`).
//!
//! The C# loader calls back into the presenter (`_show`); here the loader RETURNS `ZoomDetailEffect`s and the presenter applies them
//! with [`ImagePresenter::apply_zoom_effects`], in order.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use pr_imaging::decode_types::PlatformImage;

use crate::coordinators::zoom_detail::{ZoomDetailEffects, ZoomDetailRefreshResult};

use super::state::{ImagePresenter, PresenterChanges};

impl ImagePresenter {
    /// C# `SetViewerZoom(double? zoom)` (original-relative zoom, `None` = Fit): `zoom_detail.set_zoom(zoom)` and apply the effects. Called by the main view model
    /// whenever the viewer's zoom mode changed (C# `ViewerState.ZoomModeChanged`).
    pub fn set_viewer_zoom(&mut self, zoom: Option<f64>) -> PresenterChanges {
        todo!()
    }

    /// C# `ZoomDetail.Refresh(effectiveZoom)` (MainViewModel line 712): `zoom_detail.refresh(effective_zoom)`, apply the effects, return the result.
    pub fn refresh_zoom_detail(&mut self, effective_zoom: f64) -> (ZoomDetailRefreshResult, PresenterChanges) {
        todo!()
    }

    /// C# `ZoomDetail.Reset()` (MainViewModel line 917): `zoom_detail.reset()` and apply the effects (the indicator may hide).
    pub fn reset_zoom_detail(&mut self) -> PresenterChanges {
        todo!()
    }

    /// Applies loader effects in order: `ShowImage` -> [`show_zoom_detail_image`](Self::show_zoom_detail_image); `OriginalShown { path }` -> the
    /// `original_shown_hook`; `RawDecodeIndicatorChanged` -> marks `RAW_DECODE_INDICATOR`. Used by every call that can produce effects (`set_viewer_zoom`, `refresh_zoom_detail`,
    /// `reset_zoom_detail`, `pump`, `clear_presentation`, `begin_present`, `on_preview_presented` callers).
    pub(super) fn apply_zoom_effects(&mut self, effects: ZoomDetailEffects) {
        todo!()
    }

    /// C# `ShowZoomDetailImage(image, w, h)` (lines 133 to 138): `is_same_source_swap = true` for exactly the duration of `show_zoom_detail_image_core` (the sink's image
    /// callback reads it synchronously through `ImageChange::is_same_source_swap`), then `false` again. The C# `try/finally` has no Rust twin: a panic is a bug (R0003) and the
    /// presenter is not used after one, so set / call / reset in plain sequence is enough.
    pub(super) fn show_zoom_detail_image(&mut self, image: PlatformImage, original_width: i32, original_height: i32) {
        todo!()
    }

    /// C# `ShowZoomDetailImageCore` (lines 140 to 159), Q-RAW-03: `showing_original = held_original.is_some_and(|held| same_platform_image(held.platform_image(), &image)
    /// && !held.embedded_preview_size().is_some_and(|(w, _)| w > 0))` (decided from the shown bitmap itself: a held original that is still the embedded JPEG is still a RAW
    /// preview and keeps its info line). When showing the original: `info_while_original_shown = current_photo_info.filter(has_raw_preview).cloned()` (ALWAYS reassigned, so a
    /// value of an earlier photo can never be restored), and when it is `Some`, `current_photo_info = Some(info.without_raw_preview())`. Otherwise, when
    /// `info_while_original_shown` is `Some(preview)`: `if current_photo_info.is_some() { current_photo_info = Some(preview) }`, then `info_while_original_shown = None`.
    /// Finally `update_current_image(Some(image), original_width, original_height, None)` (no path: the file identity is kept). Marks `PHOTO_INFO` when the info changed.
    pub(super) fn show_zoom_detail_image_core(
        &mut self,
        image: PlatformImage,
        original_width: i32,
        original_height: i32,
    ) {
        todo!()
    }

    /// The pull side of the viewer-zoom channel (B1 change `IPresentationSink::take_viewer_zoom_change`): after every sink call that can make the view model change the zoom
    /// (`set_current_image`, `apply_initial_view_mode`), `if let Some(zoom) = sink.take_viewer_zoom_change() { set_viewer_zoom(zoom) }`. In C# the view model's
    /// `ZoomModeChanged` handler called `SetViewerZoom` re-entrantly from inside the sink callback; a sink owned by the presenter cannot call back into it, so the presenter
    /// asks right after the call. The order relative to `zoom_detail.on_preview_presented` is therefore the C# order.
    pub(super) fn sync_viewer_zoom_from_sink(&mut self) {
        todo!()
    }
}
