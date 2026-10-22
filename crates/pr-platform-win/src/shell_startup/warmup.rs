//! Port of `PhotoReview.Shell.Win32/Startup/ShellWarmup.cs` (84 lines, baseline `d92d6aae`): `NoDispatcher` (lines 15-35), `WarmSettingsParser` (37-53), `WarmGraphics`
//! (55-83). Unit W8-17c. Contract: `contracts/W8-17c.md`.
//!
//! perf(startup): work that cannot be avoided but can run on a worker thread in parallel with the runtime and the window. It reads and writes nothing of the user's; a
//! failure only loses the warming.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::sync::Arc;
use std::thread::JoinHandle;

use pr_core::settings::app_settings::SettingsTexts;

use crate::render::image_cache::policy::{StripeScheduler, StripeToken};

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
        todo!()
    }
}

/// The body of C# `WarmSettingsParser`: serialises `AppSettings::new(texts)` with `to_json()` and parses it back with `SettingsStore::parse_text`, then
/// `mark("settingsParserWarm")`. Any error ends the warm-up silently.
pub fn warm_settings_parser_now(texts: &dyn SettingsTexts, mark: &dyn Fn(&str)) {
    todo!()
}

/// C# `WarmSettingsParser()`: [`warm_settings_parser_now`] on a named worker thread inside `catch_unwind`; `None` when the thread could not be started.
pub fn warm_settings_parser(texts: Arc<dyn SettingsTexts>, mark: WarmupMark) -> Option<JoinHandle<()>> {
    todo!()
}

/// The body of C# `WarmGraphics`: an offscreen 1024x768 hardware surface (dpi scale 1.0), a 64x64 `Bgr32` image uploaded and drawn scaled with `HighQualityCubic`, one
/// DirectWrite layout drawn, `end_draw_and_present`, `mark("graphicsWarm")`; everything is dropped at the end. `Err(WarmupFailed)` is any failure (no GPU, RDP): the caller ignores it.
pub fn warm_graphics_now(mark: &dyn Fn(&str)) -> Result<(), WarmupFailed> {
    todo!()
}

/// C# `WarmGraphics()`: [`warm_graphics_now`] on a named worker thread inside `catch_unwind`; `None` when the thread could not be started.
pub fn warm_graphics(mark: WarmupMark) -> Option<JoinHandle<()>> {
    todo!()
}
