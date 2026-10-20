//! Shared fakes for `overlay_button` and `overlay_text` (W8-15c, test porter). Included by both test files with
//! `#[path = "overlay_fakes.rs"] mod overlay_fakes;`; W8-15d and W8-15e may read it, not edit it.
//!
//! Port of `PhotoReview.Shell.Tests/Overlay/OverlayTestSupport.cs` (baseline `d92d6aae`):
//! - [`FakeTextRenderer`] measures deterministically: width = characters x (size / 2), height = size x 1.5. With
//!   `TextTrimming::CharacterEllipsis` and a width above the offered width, the width becomes `floor(max / cw) * cw` (never
//!   negative). It counts the live layouts; a layout gives its lease back on drop (C# `Dispose`).
//! - [`RecordingDrawContext`] records fills (with rounded flag and colour), strokes, texts, clip pushes and the call order.
//!   Its opacity stack keeps the product of the pushed opacities, as the C# fake does.

#![allow(dead_code)] // shared by two test crates; each uses a subset of these fakes

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::services::render::error::RenderError;
use pr_app::services::render::surface::DrawContext;
use pr_app::services::render::text::{TextLayout, TextRenderer, TextStyle, TextTrimming};
use pr_app::services::render::{ColorF, GpuImage, ImageInterpolation};
use pr_imaging::pixels::PixelBuffer;

/// One `create_layout` call: `(text, style, max_width, trimming)` (C# `Requests` tuple).
pub type LayoutRequest = (String, TextStyle, f64, TextTrimming);

/// Deterministic text renderer (C# `FakeTextRenderer`). `fail` makes `create_layout` return `Err` (no layout is created,
/// no lease is counted).
#[derive(Debug, Default)]
pub struct FakeTextRenderer {
    requests: RefCell<Vec<LayoutRequest>>,
    live: Rc<Cell<i64>>,
    /// When `true`, `create_layout` fails like a disposed DirectWrite renderer.
    pub fail: Cell<bool>,
}

impl FakeTextRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every `create_layout` call so far, in order (C# `Requests`).
    pub fn requests(&self) -> Vec<LayoutRequest> {
        self.requests.borrow().clone()
    }

    /// Number of `create_layout` calls so far (C# `Requests.Count`).
    pub fn request_count(&self) -> usize {
        self.requests.borrow().len()
    }

    /// Layouts handed out and not yet dropped (C# `Live`).
    pub fn live(&self) -> i64 {
        self.live.get()
    }
}

impl TextRenderer for FakeTextRenderer {
    fn create_layout(
        &self,
        text: &str,
        style: &TextStyle,
        max_width: f64,
        trimming: TextTrimming,
    ) -> Result<Box<dyn TextLayout>, RenderError> {
        self.requests
            .borrow_mut()
            .push((text.to_owned(), style.clone(), max_width, trimming));
        if self.fail.get() {
            return Err(RenderError::Disposed {
                object: "PhotoReview.Shell.Rendering.DWriteTextRenderer",
            });
        }
        let char_width = style.font_size_dip / 2.0;
        let mut width = text.chars().count() as f64 * char_width;
        if trimming == TextTrimming::CharacterEllipsis && width > max_width {
            width = ((max_width / char_width).floor() * char_width).max(0.0);
        }
        let size = SizeD {
            width,
            height: style.font_size_dip * 1.5,
        };
        self.live.set(self.live.get() + 1);
        Ok(Box::new(FakeLayout {
            live: Rc::clone(&self.live),
            text: text.to_owned(),
            size,
        }))
    }
}

/// Layout lease of [`FakeTextRenderer`]. Dropping it returns the lease (decrements `Live`).
#[derive(Debug)]
pub struct FakeLayout {
    live: Rc<Cell<i64>>,
    text: String,
    size: SizeD,
}

impl TextLayout for FakeLayout {
    fn size(&self) -> SizeD {
        self.size
    }

    fn text(&self) -> &str {
        &self.text
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Drop for FakeLayout {
    fn drop(&mut self) {
        self.live.set(self.live.get() - 1);
    }
}

/// One filled rectangle (`fill_rectangle` or `fill_rounded_rectangle`) with the colour and the opacity in effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fill {
    pub rect: RectD,
    pub rounded: bool,
    pub color: ColorF,
    pub opacity: f32,
}

/// Recording draw context (C# `RecordingDrawContext`). `calls` lists the method names in call order.
#[derive(Debug, Default)]
pub struct RecordingDrawContext {
    pub calls: Vec<&'static str>,
    pub fills: Vec<Fill>,
    /// `draw_rectangle` calls: rectangle, colour, stroke width.
    pub strokes: Vec<(RectD, ColorF, f64)>,
    /// `draw_text` calls: layout text, origin, colour, opacity in effect.
    pub texts: Vec<(String, PointD, ColorF, f32)>,
    /// `push_clip` rectangles, in order.
    pub clips: Vec<RectD>,
    clip_depth: usize,
    opacity_stack: Vec<f32>,
}

impl RecordingDrawContext {
    pub fn new() -> Self {
        Self {
            opacity_stack: vec![1.0],
            ..Self::default()
        }
    }

    fn current_opacity(&self) -> f32 {
        self.opacity_stack.last().copied().unwrap_or(1.0)
    }

    /// Clip pushes not yet popped.
    pub fn clip_depth(&self) -> usize {
        self.clip_depth
    }
}

impl DrawContext for RecordingDrawContext {
    fn clear(&mut self, _color: ColorF) {
        self.calls.push("clear");
    }

    fn draw_image(
        &mut self,
        _image: &dyn GpuImage,
        _destination: RectD,
        _source_pixels: Option<RectD>,
        _interpolation: ImageInterpolation,
        _opacity: f32,
    ) {
        self.calls.push("draw_image");
    }

    fn fill_rectangle(&mut self, rect: RectD, color: ColorF) {
        self.calls.push("fill_rectangle");
        let opacity = self.current_opacity();
        self.fills.push(Fill {
            rect,
            rounded: false,
            color,
            opacity,
        });
    }

    fn fill_rounded_rectangle(&mut self, rect: RectD, _radius: f64, color: ColorF) {
        self.calls.push("fill_rounded_rectangle");
        let opacity = self.current_opacity();
        self.fills.push(Fill {
            rect,
            rounded: true,
            color,
            opacity,
        });
    }

    fn draw_rectangle(&mut self, rect: RectD, color: ColorF, stroke_width: f64) {
        self.calls.push("draw_rectangle");
        self.strokes.push((rect, color, stroke_width));
    }

    fn draw_text(&mut self, layout: &dyn TextLayout, origin: PointD, color: ColorF) -> Result<(), RenderError> {
        self.calls.push("draw_text");
        let opacity = self.current_opacity();
        self.texts.push((layout.text().to_owned(), origin, color, opacity));
        Ok(())
    }

    fn push_clip(&mut self, rect: RectD) {
        self.calls.push("push_clip");
        self.clips.push(rect);
        self.clip_depth += 1;
    }

    fn pop_clip(&mut self) -> Result<(), RenderError> {
        self.calls.push("pop_clip");
        if self.clip_depth == 0 {
            return Err(RenderError::InvalidOperation {
                message: "PopClip without PushClip.",
            });
        }
        self.clip_depth -= 1;
        Ok(())
    }

    fn push_opacity(&mut self, opacity: f32) {
        self.calls.push("push_opacity");
        let product = self.current_opacity() * opacity;
        self.opacity_stack.push(product);
    }

    fn pop_opacity(&mut self) -> Result<(), RenderError> {
        self.calls.push("pop_opacity");
        if self.opacity_stack.len() <= 1 {
            return Err(RenderError::InvalidOperation {
                message: "PopOpacity without PushOpacity.",
            });
        }
        self.opacity_stack.pop();
        Ok(())
    }

    fn try_read_pixels(&mut self) -> Result<Option<PixelBuffer>, RenderError> {
        Ok(None)
    }
}
