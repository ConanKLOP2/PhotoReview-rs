//! Port of `PhotoReview.App.Shared/Coordinators/ImagePresenter.cs` lines 120 to 159 and 194 to 201 (baseline d92d6aae):
//! `ShowZoomDetailImage` / `ShowZoomDetailImageCore` (the swap of the displayed bitmap by the zoom-detail loader) and
//! `SetViewerZoom`, plus the glue the Rust design needs between the loader's effects and the presenter. Contract:
//! `contracts/W8-07h.md`. Target: `crates/pr-app/src/coordinators/presenter/zoom.rs` (`coordinators::presenter::zoom`).
//!
//! The C# loader calls back into the presenter (`_show`); here the loader RETURNS `ZoomDetailEffect`s and the presenter applies them
//! with [`ImagePresenter::apply_zoom_effects`], in order.

use pr_imaging::decode_types::PlatformImage;

use crate::coordinators::zoom_detail::{
    ZoomDetailEffect, ZoomDetailEffects, ZoomDetailRefreshResult, same_platform_image,
};

use super::state::{ImagePresenter, PresenterChanges};

impl ImagePresenter {
    /// C# `SetViewerZoom(double? zoom)` (original-relative zoom, `None` = Fit): `zoom_detail.set_zoom(zoom)` and apply the effects. Called by the main view model
    /// whenever the viewer's zoom mode changed (C# `ViewerState.ZoomModeChanged`).
    pub fn set_viewer_zoom(&mut self, zoom: Option<f64>) -> PresenterChanges {
        self.set_viewer_zoom_inner(zoom);
        self.take_changes()
    }

    /// C# `ZoomDetail.Refresh(effectiveZoom)` (MainViewModel line 712): `zoom_detail.refresh(effective_zoom)`, apply the effects, return the result.
    pub fn refresh_zoom_detail(&mut self, effective_zoom: f64) -> (ZoomDetailRefreshResult, PresenterChanges) {
        let (result, effects) = self.zoom_detail.refresh(effective_zoom);
        self.apply_zoom_effects(effects);
        (result, self.take_changes())
    }

    /// C# `ZoomDetail.Reset()` (MainViewModel line 917): `zoom_detail.reset()` and apply the effects (the indicator may hide).
    pub fn reset_zoom_detail(&mut self) -> PresenterChanges {
        let effects = self.zoom_detail.reset();
        self.apply_zoom_effects(effects);
        self.take_changes()
    }

    /// `set_viewer_zoom` without taking the accumulated changes (used by the sink pull inside a larger call).
    fn set_viewer_zoom_inner(&mut self, zoom: Option<f64>) {
        let effects = self.zoom_detail.set_zoom(zoom);
        self.apply_zoom_effects(effects);
    }

    /// Applies loader effects in order: `ShowImage` -> [`show_zoom_detail_image`](Self::show_zoom_detail_image); `OriginalShown { path }` -> the
    /// `original_shown_hook`; `RawDecodeIndicatorChanged` -> marks `RAW_DECODE_INDICATOR`. Used by every call that can produce effects (`set_viewer_zoom`, `refresh_zoom_detail`,
    /// `reset_zoom_detail`, `pump`, `clear_presentation`, `begin_present`, `on_preview_presented` callers).
    pub(super) fn apply_zoom_effects(&mut self, effects: ZoomDetailEffects) {
        for effect in effects {
            match effect {
                ZoomDetailEffect::ShowImage {
                    image,
                    original_width,
                    original_height,
                } => self.show_zoom_detail_image(image, original_width, original_height),
                ZoomDetailEffect::OriginalShown { path } => {
                    if let Some(hook) = self.original_shown_hook.as_mut() {
                        hook(&path);
                    }
                }
                ZoomDetailEffect::RawDecodeIndicatorChanged { .. } => {
                    self.changes |= PresenterChanges::RAW_DECODE_INDICATOR;
                }
            }
        }
    }

    /// C# `ShowZoomDetailImage(image, w, h)` (lines 133 to 138): `is_same_source_swap = true` for exactly the duration of `show_zoom_detail_image_core` (the sink's image
    /// callback reads it synchronously through `ImageChange::is_same_source_swap`), then `false` again. The C# `try/finally` has no Rust twin: a panic is a bug (R0003) and the
    /// presenter is not used after one, so set / call / reset in plain sequence is enough.
    pub(super) fn show_zoom_detail_image(&mut self, image: PlatformImage, original_width: i32, original_height: i32) {
        self.is_same_source_swap = true;
        self.show_zoom_detail_image_core(image, original_width, original_height);
        self.is_same_source_swap = false;
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
        let showing_original = self.zoom_detail.held_original().is_some_and(|held| {
            same_platform_image(held.platform_image(), &image)
                && !held.embedded_preview_size().is_some_and(|(width, _)| width > 0)
        });
        if showing_original {
            self.info_while_original_shown = self.current_photo_info.clone().filter(|info| info.has_raw_preview());
            if let Some(saved) = &self.info_while_original_shown {
                self.current_photo_info = Some(saved.without_raw_preview());
                self.changes |= PresenterChanges::PHOTO_INFO;
            }
        } else if let Some(preview) = self.info_while_original_shown.take() {
            if self.current_photo_info.is_some() {
                self.current_photo_info = Some(preview);
                self.changes |= PresenterChanges::PHOTO_INFO;
            }
        }
        self.update_current_image(Some(image), original_width, original_height, None);
    }

    /// The pull side of the viewer-zoom channel (B1 change `IPresentationSink::take_viewer_zoom_change`): after every sink call that can make the view model change the zoom
    /// (`set_current_image`, `apply_initial_view_mode`), `if let Some(zoom) = sink.take_viewer_zoom_change() { set_viewer_zoom(zoom) }`. In C# the view model's
    /// `ZoomModeChanged` handler called `SetViewerZoom` re-entrantly from inside the sink callback; a sink owned by the presenter cannot call back into it, so the presenter
    /// asks right after the call. The order relative to `zoom_detail.on_preview_presented` is therefore the C# order.
    pub(super) fn sync_viewer_zoom_from_sink(&mut self) {
        if let Some(zoom) = self.sink.take_viewer_zoom_change() {
            self.set_viewer_zoom_inner(zoom);
        }
    }
}

// The in-crate tests drive the pieces no public call reaches without a present (W8-07j): the zoom swap, the file identity and the sink pull.
// They reuse the shared fakes of `tests/support`.
#[cfg(test)]
#[path = "../../../tests/support/presenter_fakes.rs"]
mod presenter_fakes;
#[cfg(test)]
#[path = "../../../tests/support/preview_fakes.rs"]
mod preview_fakes;

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use pr_imaging::decode_types::PlatformImage;
    use pr_imaging::decoder_traits::ImageDecoder;

    use super::super::photo_info::PhotoInfo;
    use super::super::state::PresenterChanges;
    use super::presenter_fakes::{EmbeddedOnFullDecoder, PresenterFixture};
    use super::preview_fakes::{SizedDecoder, TIMEOUT, lock, sized_image};

    fn image() -> PlatformImage {
        PlatformImage::Opaque(Arc::new(0_u8))
    }

    fn info(raw_preview_width: i32) -> PhotoInfo {
        PhotoInfo {
            file_name: "a.jpg".to_owned(),
            width: 6000,
            height: 4000,
            exif: None,
            raw_preview_width,
            raw_preview_height: if raw_preview_width > 0 { 1333 } else { 0 },
        }
    }

    /// A fixture with the preview of "a.jpg" on screen and the loader armed for it (what a present does, without the present).
    fn armed(decoder: Arc<dyn ImageDecoder>) -> (PresenterFixture, String) {
        let mut f = PresenterFixture::new(decoder, &["a.jpg"]);
        let path = f.path("a.jpg");
        let token = f.clock.next_navigation();
        let key = f.service.get_current_cache_key(&path).expect("cache key");
        let preview = sized_image(600, 400, 6000, 4000, true);
        let effects = f
            .presenter
            .zoom_detail
            .on_preview_presented(token, &path, &key, &preview);
        f.presenter.apply_zoom_effects(effects);
        f.presenter
            .update_current_image(Some(preview.platform_image().clone()), 6000, 4000, Some(&path));
        f.presenter.current_photo_info = Some(info(2000));
        lock(&f.sink_log).images.clear();
        f.presenter.take_changes();
        (f, path)
    }

    /// Pumps until the loader shows the original; returns the changes accumulated on the way.
    fn pump_to_original(f: &mut PresenterFixture) -> PresenterChanges {
        let deadline = std::time::Instant::now() + TIMEOUT;
        let mut changes = PresenterChanges::NONE;
        while !f.presenter.zoom_detail().is_showing_original() {
            assert!(
                std::time::Instant::now() < deadline,
                "the original was not shown in time"
            );
            f.presenter.wait_for_event(std::time::Duration::from_millis(50));
            changes |= f.pump();
        }
        changes
    }

    // New test: no C# twin (UpdateCurrentImage order, R10 file identity).
    #[test]
    fn update_current_image_sets_the_dimensions_first_and_keeps_the_file_identity_over_a_pathless_swap() {
        let mut f = PresenterFixture::new(SizedDecoder::new(), &[]);
        f.presenter.current_navigation_path = Some("nav".to_owned());

        f.presenter.update_current_image(Some(image()), 10, 20, Some("a"));
        f.presenter.update_current_image(Some(image()), 11, 21, Some("a"));
        f.presenter.update_current_image(Some(image()), 12, 22, Some("b"));
        f.presenter.update_current_image(Some(image()), 13, 23, None);
        f.presenter.update_current_image(Some(image()), 14, 24, Some("c"));
        f.presenter.update_current_image(None, 99, 99, None);
        f.presenter.update_current_image(Some(image()), 15, 25, Some("c"));

        let log = lock(&f.sink_log);
        assert_eq!(
            log.file_change_flags(),
            vec![false, false, true, false, true, false, false],
            "first image, same-file upgrade, other file, pathless swap, other file again (identity kept over the swap), clear, first after clear"
        );
        let sizes: Vec<_> = log
            .images
            .iter()
            .map(|c| (c.original_width, c.original_height))
            .collect();
        assert_eq!(
            sizes,
            vec![(10, 20), (11, 21), (12, 22), (13, 23), (14, 24), (0, 0), (15, 25)]
        );
        assert!(log.images.iter().all(|c| c.presented_path.as_deref() == Some("nav")));
        assert!(log.images[5].image.is_none());
        assert_eq!(f.presenter.current_original_width(), 15);
    }

    // New test: no C# twin.
    #[test]
    fn reset_file_identity_makes_the_next_image_not_a_file_change() {
        let mut f = PresenterFixture::new(SizedDecoder::new(), &[]);
        f.presenter.update_current_image(Some(image()), 1, 1, Some("a"));
        f.presenter.reset_file_identity();
        f.presenter.update_current_image(Some(image()), 1, 1, Some("b"));
        assert_eq!(lock(&f.sink_log).file_change_flags(), vec![false, false]);
    }

    // New test: no C# twin (the flag is a window, not a state).
    #[test]
    fn same_source_swap_flag_is_true_only_inside_the_zoom_detail_swap() {
        let mut f = PresenterFixture::new(SizedDecoder::new(), &[]);

        f.presenter.show_zoom_detail_image(image(), 6000, 4000);
        assert!(!f.presenter.is_same_source_swap());
        f.presenter.update_current_image(Some(image()), 600, 400, Some("a"));

        let log = lock(&f.sink_log);
        assert!(log.images[0].is_same_source_swap, "the sink reads it synchronously");
        assert!(
            !log.images[1].is_same_source_swap,
            "an ordinary update never carries it"
        );
        assert!(!log.images[0].is_file_change, "a swap passes no path");
    }

    // New test: no C# twin (UpdateStatus).
    #[test]
    fn update_status_stores_both_values_and_calls_the_sink() {
        let mut f = PresenterFixture::new(SizedDecoder::new(), &[]);
        f.presenter.update_status("Loading".to_owned(), true);
        assert_eq!(f.presenter.status_text(), "Loading");
        assert!(f.presenter.status_needs_attention());
        assert!(f.presenter.take_changes().contains(PresenterChanges::STATUS));
        assert_eq!(lock(&f.sink_log).statuses, vec!["Loading".to_owned()]);
    }

    // New test: no C# twin (ShowZoomDetailImageCore, Q-RAW-03).
    #[test]
    fn swapping_to_the_original_hides_the_raw_preview_size_and_swapping_back_restores_it() {
        let (mut f, _path) = armed(SizedDecoder::new());

        f.presenter.set_viewer_zoom(Some(2.0));
        let changes = pump_to_original(&mut f);

        assert!(changes.contains(PresenterChanges::PHOTO_INFO));
        assert_eq!(f.presenter.current_photo_info().map(|i| i.raw_preview_width), Some(0));
        assert_eq!(
            f.presenter
                .info_while_original_shown
                .as_ref()
                .map(|i| i.raw_preview_width),
            Some(2000)
        );
        {
            let log = lock(&f.sink_log);
            let swap = log.images.last().expect("the swap reached the sink");
            assert!(swap.is_same_source_swap);
            assert!(!swap.is_file_change, "same file");
            assert_eq!((swap.original_width, swap.original_height), (6000, 4000));
        }

        let changes = f.presenter.set_viewer_zoom(None);
        assert!(!f.presenter.zoom_detail().is_showing_original());
        assert!(changes.contains(PresenterChanges::PHOTO_INFO));
        assert_eq!(
            f.presenter.current_photo_info().map(|i| i.raw_preview_width),
            Some(2000)
        );
        assert!(
            f.presenter.info_while_original_shown.is_none(),
            "the saved copy is cleared"
        );
        assert!(!lock(&f.sink_log).images.last().is_some_and(|c| c.is_file_change));
    }

    // New test: no C# twin ("restore only if info exists": never resurrected after a clear).
    #[test]
    fn the_saved_preview_info_is_not_resurrected_after_the_info_was_cleared() {
        let (mut f, _path) = armed(SizedDecoder::new());
        f.presenter.set_viewer_zoom(Some(2.0));
        pump_to_original(&mut f);
        f.presenter.current_photo_info = None;

        f.presenter.set_viewer_zoom(None);

        assert!(f.presenter.current_photo_info().is_none());
        assert!(f.presenter.info_while_original_shown.is_none());
    }

    // New test: no C# twin ("always reassign": a value of an earlier photo can never be restored).
    #[test]
    fn a_stale_saved_info_of_an_earlier_photo_is_dropped_when_the_original_shows() {
        let (mut f, _path) = armed(SizedDecoder::new());
        f.presenter.current_photo_info = Some(info(0));
        let mut stale = info(777);
        stale.file_name = "stale.jpg".to_owned();
        f.presenter.info_while_original_shown = Some(stale);

        f.presenter.set_viewer_zoom(Some(2.0));
        pump_to_original(&mut f);
        assert!(f.presenter.info_while_original_shown.is_none());
        f.presenter.set_viewer_zoom(None);

        assert_eq!(
            f.presenter.current_photo_info().map(|i| i.file_name.as_str()),
            Some("a.jpg")
        );
        assert_eq!(f.presenter.current_photo_info().map(|i| i.raw_preview_width), Some(0));
    }

    // New test: no C# twin (the decision is made from the shown bitmap: a held original that is still the embedded JPEG keeps its RAW info).
    #[test]
    fn a_held_original_that_is_still_the_embedded_jpeg_keeps_the_raw_preview_size() {
        let (mut f, _path) = armed(Arc::new(EmbeddedOnFullDecoder));

        f.presenter.set_viewer_zoom(Some(2.0));
        f.pump_until(|f| f.presenter.zoom_detail().is_showing_original());

        assert_eq!(
            f.presenter.current_photo_info().map(|i| i.raw_preview_width),
            Some(2000)
        );
        assert!(f.presenter.info_while_original_shown.is_none());
        assert!(lock(&f.sink_log).images.last().is_some_and(|c| c.is_same_source_swap));
    }

    // New test: no C# twin (the OriginalShown measure hook is an effect of the loader).
    #[test]
    fn the_original_shown_hook_receives_the_path() {
        let (mut f, path) = armed(SizedDecoder::new());
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = Arc::clone(&seen);
        f.presenter
            .set_original_shown_hook(Some(Box::new(move |p: &str| lock(&sink).push(p.to_owned()))));

        f.presenter.set_viewer_zoom(Some(2.0));
        pump_to_original(&mut f);

        assert_eq!(*lock(&seen), vec![path]);
    }

    // New test: no C# twin (the sink pull replaces the re-entrant ZoomModeChanged; the zoom reaches the loader right after the image call).
    #[test]
    fn a_zoom_change_asked_by_the_sink_reaches_the_loader() {
        let (mut f, _path) = armed(SizedDecoder::new());
        assert!(!f.presenter.zoom_detail().pending_load());
        lock(&f.sink_log).zoom_change = Some(Some(2.0));

        f.presenter.update_current_image(Some(image()), 600, 400, Some("x"));

        assert!(
            f.presenter.zoom_detail().pending_load(),
            "the loader got zoom 2.0 and started the original decode"
        );
        assert!(lock(&f.sink_log).zoom_change.is_none(), "taken once");
        f.pump_until_idle();
    }
}
