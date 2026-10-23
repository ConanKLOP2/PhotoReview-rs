//! Port of `PhotoReview.Shell.Win32/Startup/ShellWarmup.cs` (84 lines, baseline `d92d6aae`): `NoDispatcher` (lines 15-35), `WarmSettingsParser` (37-53), `WarmGraphics`
//! (55-83). Unit W8-17c. Contract: `contracts/W8-17c.md`.
//!
//! perf(startup): work that cannot be avoided but can run on a worker thread in parallel with the runtime and the window. It reads and writes nothing of the user's; a
//! failure only loses the warming.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::render::{
    draw_context::NativeTextLayout,
    image_cache::{
        policy::{StripeScheduler, StripeToken},
        upload::{D2dGpuImageCache, ResourcesBitmapDevice},
    },
    surface::D2dRenderSurface,
    text::renderer::DWriteTextRenderer,
};
use pr_app::input::primitives::{PointD, RectD};
use pr_app::services::render::TextTrimming;
use pr_app::viewport::render_geometry::{ColorF, ImageInterpolation};
use pr_core::settings::{
    app_settings::{AppSettings, SettingsTexts},
    store::SettingsStore,
};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use std::rc::Rc;

/// Any failure of a warm-up (no GPU, RDP, a driver error): the caller ignores it, the real device falls back to WARP at window creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarmupFailed;

/// A startup mark sink (C# `PhotoReviewPerf.StartupMark`); `Send` because the warm-ups run on a worker.
pub type WarmupMark = Box<dyn Fn(&str) + Send>;

/// C# `NoDispatcher`: the stripe scheduler of the warm-up cache. It must never be asked to schedule (the warm-up image is uploaded whole, nothing is prefetched).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoStripeScheduler;

impl StripeScheduler for NoStripeScheduler {
    /// Does nothing (the C# `NoDispatcher.Post` ran the action inline, which a stripe job must not be: a stripe of a 64x64 image never exists).
    fn schedule_stripe(&self, token: StripeToken) {
        let _ = token;
    }
}

/// The body of C# `WarmSettingsParser`: serialises `AppSettings::new(texts)` with `to_json()` and parses it back with `SettingsStore::parse_text`, then
/// `mark("settingsParserWarm")`. Any error ends the warm-up silently.
pub fn warm_settings_parser_now(texts: &dyn SettingsTexts, mark: &dyn Fn(&str)) {
    let Ok(defaults) = AppSettings::new(texts).to_json() else {
        return;
    };
    if SettingsStore::parse_text(&defaults, texts).is_ok() {
        mark("settingsParserWarm");
    }
}

/// C# `WarmSettingsParser()`: [`warm_settings_parser_now`] on a named worker thread inside `catch_unwind`; `None` when the thread could not be started.
pub fn warm_settings_parser(texts: Arc<dyn SettingsTexts>, mark: WarmupMark) -> Option<JoinHandle<()>> {
    thread::Builder::new()
        .name("warm-settings-parser".to_owned())
        .spawn(move || {
            let _ = catch_unwind(AssertUnwindSafe(|| warm_settings_parser_now(&*texts, &*mark)));
        })
        .ok()
}

/// The body of C# `WarmGraphics`: an offscreen 1024x768 hardware surface (dpi scale 1.0), a 64x64 `Bgr32` image uploaded and drawn scaled with `HighQualityCubic`, one
/// DirectWrite layout drawn, `end_draw_and_present`, `mark("graphicsWarm")`; everything is dropped at the end. `Err(WarmupFailed)` is any failure (no GPU, RDP): the caller ignores it.
pub fn warm_graphics_now(mark: &dyn Fn(&str)) -> Result<(), WarmupFailed> {
    let mut surface = D2dRenderSurface::create_offscreen_surface(1024, 768, 1.0, true).map_err(|_| WarmupFailed)?;
    let device = ResourcesBitmapDevice::new(surface.resources().clone());
    let cache = D2dGpuImageCache::new(device, Rc::new(NoStripeScheduler), 1024, 768).map_err(|_| WarmupFailed)?;
    let pixels = PixelBuffer::allocate(64, 64, PixelLayout::Bgr32).map_err(|_| WarmupFailed)?;
    let image = cache.get_or_upload_tiled(&pixels).map_err(|_| WarmupFailed)?;
    let context = surface.begin_draw_context().map_err(|_| WarmupFailed)?;
    context.draw_image(
        &*image as &dyn std::any::Any,
        RectD {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 700.0,
        },
        None,
        ImageInterpolation::HighQualityCubic,
        1.0,
    );
    let text = DWriteTextRenderer::new(None, "en-US", 256).map_err(|_| WarmupFailed)?;
    let layout = text
        .create_layout(
            "Aa Ee 📂 ⚙ ⋮ 100% IMG_0001.JPG",
            "Segoe UI",
            12.0,
            false,
            300.0,
            TextTrimming::CharacterEllipsis,
        )
        .map_err(|_| WarmupFailed)?;
    let native_layout = layout.native_layout().ok_or(WarmupFailed)?;
    let native_layout = WarmupTextLayout(native_layout);
    context.draw_text(
        &native_layout,
        PointD { x: 0.0, y: 0.0 },
        ColorF {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
    );
    let _ = context;
    surface.end_draw_and_present().map_err(|_| WarmupFailed)?;
    mark("graphicsWarm");
    Ok(())
}

struct WarmupTextLayout(windows::Win32::Graphics::DirectWrite::IDWriteTextLayout);
impl NativeTextLayout for WarmupTextLayout {
    fn native_layout(&self) -> &windows::Win32::Graphics::DirectWrite::IDWriteTextLayout {
        &self.0
    }
}

/// C# `WarmGraphics()`: [`warm_graphics_now`] on a named worker thread inside `catch_unwind`; `None` when the thread could not be started.
pub fn warm_graphics(mark: WarmupMark) -> Option<JoinHandle<()>> {
    thread::Builder::new()
        .name("warm-graphics".to_owned())
        .spawn(move || {
            let _ = catch_unwind(AssertUnwindSafe(|| warm_graphics_now(&*mark)));
        })
        .ok()
}
