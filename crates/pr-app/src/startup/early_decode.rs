//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 517-518 (`EarlyDecodePlan`), 525-546 (`PrepareEarlyDecodeAsync`) and 548-565 (`StartEarlyDecode`)
//! (baseline `7eaee2a2`). Unit W8-17d. Contract: `contracts/W8-17d.md`.
//!
//! P-1: when the launch names an image FILE, its decode starts right after `config.json` is read instead of when the window is shown (about 450 ms later). Two steps with a
//! hard order between them (C# test `Startup_EarlyDecode_StartsOnlyAfterTheInstanceLockIsHeld`): [`prepare_early_decode`] runs on a worker as soon as the settings file is read
//! and only PREDICTS the decode box and picks the preview service; [`start_early_decode`] runs after this process holds the instance lock (a refused second launch never decodes).
//! The prediction only picks the decode box: when it is wrong the presenter's own request (keyed by the real viewport) decodes again, as before.

use std::fmt;
use std::sync::Arc;

use pr_core::abstractions::Log;
use pr_core::settings::app_settings::AppSettings;
use pr_imaging::decode_types::DecodeBox;

use crate::services::initial_viewport::MonitorProbe;
use crate::services::prewarm::{PrewarmPreviews, PrewarmTask};
use crate::services::viewport_size_source::ViewportSizeSource;
use crate::windowing::contracts::WindowPlacementData;

/// The thread-safe start-up mark sink (C# `Action<string> _mark`): called from the UI thread AND from workers.
pub type MarkFn = Arc<dyn Fn(&str) + Send + Sync>;

/// C# `EarlyDecodePlan(PreviewImageService Previews, DecodeBox Box)`.
#[derive(Clone)]
pub struct EarlyDecodePlan {
    /// The preview service (`services.GetRequiredService<PreviewImageService>()`).
    pub previews: Arc<dyn PrewarmPreviews>,
    /// The predicted decode box.
    pub decode_box: DecodeBox,
}

impl fmt::Debug for EarlyDecodePlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EarlyDecodePlan")
            .field("decode_box", &self.decode_box)
            .finish_non_exhaustive()
    }
}

/// C# `PrepareEarlyDecodeAsync` after its `await placementLoad` (the worker joins the placement prefetch itself and passes the result): `placement` is the saved placement, `None` =
/// first start. `box = predict_decode_box(placement, default_width_dip, default_height_dip, quality_multiplier, probe)`; `None` box -> `None`; else
/// `Some(EarlyDecodePlan { previews, box })`. The whole body runs under `catch_unwind(AssertUnwindSafe(..))`: a panic is logged with `log.error("Early decode preparation failed", ..)`
/// and answers `None` (C# `catch (Exception) when not OOM`: "never break the start-up, the window starts the decode itself").
pub fn prepare_early_decode(
    placement: Option<&WindowPlacementData>,
    previews: Arc<dyn PrewarmPreviews>,
    probe: &dyn MonitorProbe,
    default_width_dip: f64,
    default_height_dip: f64,
    quality_multiplier: f64,
    log: &dyn Log,
) -> Option<EarlyDecodePlan> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let decode_box = crate::services::initial_viewport::predict_decode_box(
            placement,
            default_width_dip,
            default_height_dip,
            quality_multiplier,
            probe,
        )?;
        Some(EarlyDecodePlan { previews, decode_box })
    })) {
        Ok(plan) => plan,
        Err(payload) => {
            let error = std::io::Error::other(pr_core::abstractions::panic_message(payload.as_ref()));
            log.error("Early decode preparation failed", Some(&error));
            None
        }
    }
}

/// C# `StartEarlyDecode` body after its `await plan` (the caller joins the preparation worker): `None` plan -> `None`; else `viewport.set_startup_prediction(Some(plan.decode_box))`
/// FIRST, then `prewarm::start(Some(plan.previews), settings, Some(path), Some(plan.decode_box))`; when it returned `Some(task)` mark `"earlyDecodeStarted"`. The task is dropped
/// (the thread detaches; the app never waits). Returns the started task (tests wait on it).
pub fn start_early_decode(
    plan: Option<EarlyDecodePlan>,
    viewport: &ViewportSizeSource,
    settings: &AppSettings,
    path: &str,
    mark: &dyn Fn(&str),
) -> Option<PrewarmTask> {
    let plan = plan?;
    viewport.set_startup_prediction(Some(plan.decode_box));
    let task = crate::services::prewarm::start(Some(plan.previews), settings, Some(path), Some(plan.decode_box));
    if task.is_some() {
        mark("earlyDecodeStarted");
    }
    task
}
