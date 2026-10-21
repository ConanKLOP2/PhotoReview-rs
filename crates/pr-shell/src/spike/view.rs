//! Fit / source-pixel zoom / pan maths of the spike viewer (pure, no Windows types).
//!
//! `scale` is screen pixels per source pixel (1.0 = 100% = one source pixel per device pixel, C# ADR 0008).

const ZOOM_STEP: f32 = 1.25;
const MAX_SCALE: f32 = 32.0;

/// Where the image is drawn: `dest` in client pixels, `src` in source pixels, both `[left, top, right, bottom]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub dest: [f32; 4],
    pub src: [f32; 4],
    /// Above 100% the viewer shows exact pixels instead of smoothing.
    pub nearest: bool,
    pub scale: f32,
}

/// Zoom/pan state of the current image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    fit: bool,
    scale: f32,
    /// Source-pixel coordinate shown at the centre of the viewport.
    center: (f32, f32),
}

impl Default for View {
    fn default() -> Self {
        Self {
            fit: true,
            scale: 1.0,
            center: (0.0, 0.0),
        }
    }
}

/// Scale that shows the whole image in the viewport (never above 100%).
#[must_use]
pub fn fit_scale(image: (f32, f32), viewport: (f32, f32)) -> f32 {
    (viewport.0 / image.0).min(viewport.1 / image.1).min(1.0)
}

impl View {
    #[must_use]
    pub fn is_fit(&self) -> bool {
        self.fit
    }

    /// Back to Fit (new image, `F` key).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Effective scale for this image and viewport.
    #[must_use]
    pub fn effective_scale(&self, image: (f32, f32), viewport: (f32, f32)) -> f32 {
        if self.fit {
            fit_scale(image, viewport)
        } else {
            self.scale
        }
    }

    /// 100%: one source pixel per device pixel, keeping the current centre.
    pub fn actual_size(&mut self, image: (f32, f32)) {
        let current = self.center_of(image);
        self.fit = false;
        self.scale = 1.0;
        self.center = current;
    }

    fn center_of(&self, image: (f32, f32)) -> (f32, f32) {
        if self.fit {
            (image.0 / 2.0, image.1 / 2.0)
        } else {
            self.center
        }
    }

    /// Wheel zoom around the cursor position (client pixels).
    pub fn zoom_at(&mut self, notches: f32, cursor: (f32, f32), image: (f32, f32), viewport: (f32, f32)) {
        let fit = fit_scale(image, viewport);
        let old_scale = self.effective_scale(image, viewport);
        let old_center = self.center_of(image);
        let new_scale = (old_scale * ZOOM_STEP.powf(notches)).clamp(fit.min(MAX_SCALE), MAX_SCALE);
        if new_scale <= fit {
            self.reset();
            return;
        }
        // The source point under the cursor stays under the cursor.
        let offset = (cursor.0 - viewport.0 / 2.0, cursor.1 - viewport.1 / 2.0);
        let anchor = (old_center.0 + offset.0 / old_scale, old_center.1 + offset.1 / old_scale);
        self.fit = false;
        self.scale = new_scale;
        self.center = (anchor.0 - offset.0 / new_scale, anchor.1 - offset.1 / new_scale);
        self.clamp_center(image);
    }

    /// Drag by a client-pixel delta (ignored in Fit).
    pub fn pan(&mut self, delta: (f32, f32), image: (f32, f32)) {
        if self.fit {
            return;
        }
        self.center = (
            self.center.0 - delta.0 / self.scale,
            self.center.1 - delta.1 / self.scale,
        );
        self.clamp_center(image);
    }

    fn clamp_center(&mut self, image: (f32, f32)) {
        self.center = (self.center.0.clamp(0.0, image.0), self.center.1.clamp(0.0, image.1));
    }

    /// The visible part of the image and where it lands in the viewport.
    #[must_use]
    pub fn layout(&self, image: (f32, f32), viewport: (f32, f32)) -> Layout {
        let scale = self.effective_scale(image, viewport);
        let (cx, cy) = self.center_of(image);
        let half = (viewport.0 / 2.0 / scale, viewport.1 / 2.0 / scale);
        let src = [
            (cx - half.0).max(0.0),
            (cy - half.1).max(0.0),
            (cx + half.0).min(image.0),
            (cy + half.1).min(image.1),
        ];
        let dest = [
            (src[0] - cx) * scale + viewport.0 / 2.0,
            (src[1] - cy) * scale + viewport.1 / 2.0,
            (src[2] - cx) * scale + viewport.0 / 2.0,
            (src[3] - cy) * scale + viewport.1 / 2.0,
        ];
        Layout {
            dest,
            src,
            nearest: scale > 1.0,
            scale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMG: (f32, f32) = (6000.0, 4000.0);
    const VP: (f32, f32) = (1600.0, 900.0);

    #[test]
    fn fit_scale_shows_whole_image_and_never_upscales() {
        assert!((fit_scale(IMG, VP) - 0.225).abs() < 1e-6);
        assert_eq!(fit_scale((100.0, 50.0), VP), 1.0);
    }

    #[test]
    fn layout_in_fit_covers_the_image_and_is_centred() {
        let l = View::default().layout(IMG, VP);
        assert_eq!(l.src, [0.0, 0.0, 6000.0, 4000.0]);
        assert!((l.dest[0] - 125.0).abs() < 1e-3 && (l.dest[2] - 1475.0).abs() < 1e-3);
        assert!(l.dest[1].abs() < 1e-3 && (l.dest[3] - 900.0).abs() < 1e-3);
        assert!(!l.nearest);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor_fixed() {
        let mut v = View::default();
        let cursor = (400.0, 300.0);
        let before = v.layout(IMG, VP);
        let src_x = before.src[0] + (cursor.0 - before.dest[0]) / before.scale;
        v.zoom_at(6.0, cursor, IMG, VP);
        let after = v.layout(IMG, VP);
        let src_x_after = after.src[0] + (cursor.0 - after.dest[0]) / after.scale;
        assert!((src_x - src_x_after).abs() < 0.5, "{src_x} vs {src_x_after}");
        assert!(after.scale > before.scale);
    }

    #[test]
    fn actual_size_is_one_source_pixel_per_device_pixel_with_nearest_above() {
        let mut v = View::default();
        v.actual_size(IMG);
        let l = v.layout(IMG, VP);
        assert_eq!(l.scale, 1.0);
        assert!(((l.src[2] - l.src[0]) - 1600.0).abs() < 1e-3);
        v.zoom_at(1.0, (800.0, 450.0), IMG, VP);
        assert!(v.layout(IMG, VP).nearest);
    }

    #[test]
    fn zooming_out_to_fit_returns_to_fit() {
        let mut v = View::default();
        v.zoom_at(3.0, (800.0, 450.0), IMG, VP);
        assert!(!v.is_fit());
        v.zoom_at(-30.0, (800.0, 450.0), IMG, VP);
        assert!(v.is_fit());
    }

    #[test]
    fn pan_moves_the_view_and_stays_inside_the_image() {
        let mut v = View::default();
        v.actual_size(IMG);
        let start = v.layout(IMG, VP).src[0];
        v.pan((-100.0, 0.0), IMG);
        assert!((v.layout(IMG, VP).src[0] - (start + 100.0)).abs() < 1e-3);
        v.pan((-1.0e9, 1.0e9), IMG);
        let l = v.layout(IMG, VP);
        assert!(l.src[2] <= IMG.0 && l.src[1] >= 0.0);
    }

    #[test]
    fn pan_in_fit_does_nothing() {
        let mut v = View::default();
        v.pan((50.0, 50.0), IMG);
        assert!(v.is_fit());
    }
}
