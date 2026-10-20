//! Port of `PhotoReview.App.Shared/Presenters/SettingsPresenter.cs` (C# baseline `d92d6aae`), W8-10d, first half: the class
//! header (lines 1 to 122 and 231 to 328): fields, constructor, page list, `LoadFields`, the RAM-cache slider arithmetic, the
//! hint texts, the WebP/HEIC status line, the TurboJPEG check and `ApplyDefaults`. Contract: `contracts/W8-10d.md`.
//!
//! The struct is defined HERE for the whole class. `validate.rs` (W8-10d) and `apply.rs` / `shortcuts.rs` (W8-10e) add
//! `impl SettingsPresenter` blocks in their own files; the fields they use are `pub(super)`. Nothing in this file touches
//! the OS except the process-wide "last page" memory.
//!
//! Texts (D-W8-4): the C# `Tr.*` accessors read the ambient localizer. Every function that produces UI text takes the
//! [`FieldContext`] of `presenters::settings_field_map` (`texts: &Localizer`, `numbers: &NumberCulture`), so tests choose the
//! language and the culture. Message keys are the dotted camelCase form of the C# accessor (`Tr.SettingsRamCacheHint` is
//! `settings.ramCache.hint`).
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use pr_core::abstractions::ShellIntegration;
use pr_core::model::enums::{
    DecoderBackend, FitWidthAnchor, InitialViewMode, InstanceMode, JournalDurability, LoadingMode, MouseWheelAction,
    ScalingQuality,
};
use pr_core::model::fields::{ExifInfoFields, TitleBarFields};
use pr_core::settings::app_settings::{
    AppSettings, DEFAULT_ARROW_PAN_STEP_PERCENT, DEFAULT_CLICK_ZOOM_PERCENT, DEFAULT_IMAGE_TRANSITION_MS,
    DEFAULT_INFO_OVERLAY_AUTO_HIDE_DELAY_MS, DEFAULT_INFO_OVERLAY_FONT_SIZE, DEFAULT_KEYBOARD_ZOOM_STEP_PERCENT,
    DEFAULT_TOOLBAR_AUTO_HIDE_DELAY_MS, DEFAULT_TOOLBAR_OPACITY_PERCENT, DEFAULT_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE,
    MAX_KEYBOARD_ZOOM_STEP_PERCENT, MIN_KEYBOARD_ZOOM_STEP_PERCENT, MIN_TOOLBAR_OPACITY_PERCENT, SettingsTexts,
};
use pr_core::settings::normalizer::SettingsValidator;
use pr_core::settings::perf_options::{
    self as perf, MAX_IMAGE_CACHE_RAM_PERCENT, MAX_PRELOAD_COUNT, MIN_PRELOAD_BACKWARD_COUNT, MIN_PRELOAD_FORWARD_COUNT,
};
use pr_core::settings::shortcuts::{ReviewAction, ShortcutMappings};
use pr_core::settings::store::SettingsStore;
use pr_core::sync::CancelToken;
use pr_core::updates::checker::UpdateCheck;
use pr_imaging::decoder_traits::ImageDecoderFactory;
use pr_imaging::preload_order::PreloadWindow;
use pr_imaging::ram_budget::{bytes_for_percent, minimum_cache_percent};
use pr_imaging::webp_heic::WicCodecSupport;
use pr_l10n::template::LocArg;

use crate::localization::service::LocalizationService;
use crate::presenters::settings_field_map::{FieldContext, bindings, try_int};
use crate::presenters::settings_form_state::SettingsFormState;

/// The page the dialog opens on when nothing (known) is remembered.
const DEFAULT_PAGE_KEY: &str = "General";

/// C# `s_lastPageKey`: process-wide, session-only.
fn last_page() -> &'static Mutex<String> {
    static LAST_PAGE: OnceLock<Mutex<String>> = OnceLock::new();
    LAST_PAGE.get_or_init(|| Mutex::new(DEFAULT_PAGE_KEY.to_owned()))
}

/// C# `SettingsPageInfo(Key, TitleKey)`: one page of the dialog (left navigation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsPageInfo {
    /// What the view tags its navigation items with.
    pub key: &'static str,
    /// The `pr_l10n` key of the heading (`Tr.SettingsNav*`).
    pub title_key: &'static str,
}

/// C# `Pages`: the seven pages in navigation order.
pub const PAGES: [SettingsPageInfo; 7] = [
    SettingsPageInfo {
        key: "General",
        title_key: "settings.nav.general",
    },
    SettingsPageInfo {
        key: "Display",
        title_key: "settings.nav.display",
    },
    SettingsPageInfo {
        key: "Mouse",
        title_key: "settings.nav.mouse",
    },
    SettingsPageInfo {
        key: "Performance",
        title_key: "settings.nav.performance",
    },
    SettingsPageInfo {
        key: "Shortcuts",
        title_key: "settings.nav.shortcuts",
    },
    SettingsPageInfo {
        key: "Files",
        title_key: "settings.nav.files",
    },
    SettingsPageInfo {
        key: "Diagnostics",
        title_key: "settings.nav.diagnostics",
    },
];

/// The localization service as the presenter shares it with the app. `LocalizationService` is owned by the UI thread and its
/// `switch` / `reload` take `&mut self`, so the shared handle is a mutex (never held across a call into the view).
pub type SharedLocalization = Arc<Mutex<LocalizationService>>;

/// The constructor arguments of C# `SettingsPresenter(current, store, localization, updateChecker, physicalMemoryBytes)` plus the
/// two public C# properties `ShellIntegration` and `ExecutablePath` and the texts the C# code reads from the ambient `Tr`.
pub struct SettingsPresenterOptions {
    /// `None` = a dialog built without a store (tests): nothing is persisted, shortcut validation uses a fallback validator.
    pub store: Option<SettingsStoreHandle>,
    /// `None` = no language group.
    pub localization: Option<SharedLocalization>,
    /// `None` = no checker configured: a check ends as `UpdateFailure::BadResponse` (no HTTP transport exists in Rust yet,
    /// see the contract, open question O5).
    pub update_checker: Option<Arc<dyn UpdateCheck>>,
    /// C# defaults a missing value to `RamBudgetPolicy.GetPhysicalMemoryBytes()`; the caller does that (tests pass 32 GiB).
    pub physical_memory_bytes: i64,
    /// Texts of `pr_core::settings` (fallback `SettingsValidator`, `SettingsStore::parse_text`, `AppSettings::new`).
    pub settings_texts: Arc<dyn SettingsTexts>,
    /// C# `ShellIntegration` property (default `WindowsShellIntegration`, the registry).
    pub shell_integration: Arc<dyn ShellIntegration>,
    /// C# `ExecutablePath` property (default `Environment.ProcessPath`); `None` = unavailable.
    pub executable_path: Option<String>,
}

/// The store handle: `SettingsStore` is `Send + Sync` and shared in an `Arc`.
pub type SettingsStoreHandle = Arc<SettingsStore>;

impl fmt::Debug for SettingsPresenterOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsPresenterOptions")
            .field("has_store", &self.store.is_some())
            .field("has_localization", &self.localization.is_some())
            .field("physical_memory_bytes", &self.physical_memory_bytes)
            .field("executable_path", &self.executable_path)
            .finish_non_exhaustive()
    }
}

impl SettingsPresenterOptions {
    /// The required parts; store, localization and update checker start as `None`, `executable_path` as
    /// `std::env::current_exe()` (lossy string, `None` when unavailable).
    pub fn new(
        physical_memory_bytes: i64,
        settings_texts: Arc<dyn SettingsTexts>,
        shell_integration: Arc<dyn ShellIntegration>,
    ) -> Self {
        Self {
            store: None,
            localization: None,
            update_checker: None,
            physical_memory_bytes,
            settings_texts,
            shell_integration,
            executable_path: std::env::current_exe()
                .ok()
                .map(|path| path.to_string_lossy().into_owned()),
        }
    }
}

/// C# `SettingsPresenter`. UI-thread object (the dialog's), except [`UpdateCheckJob`] which runs on a worker.
pub struct SettingsPresenter {
    /// C# `Settings`: the working copy the dialog edits.
    pub(super) settings: AppSettings,
    pub(super) store: Option<SettingsStoreHandle>,
    pub(super) localization: Option<SharedLocalization>,
    // The four fields below are read by W8-10e (`apply.rs`); until it lands they are write-only.
    #[allow(dead_code)]
    pub(super) update_checker: Option<Arc<dyn UpdateCheck>>,
    pub(super) physical_memory_bytes: i64,
    pub(super) settings_texts: Arc<dyn SettingsTexts>,
    /// C# `ShellIntegration` / `ExecutablePath` (W8-10e).
    #[allow(dead_code)]
    pub(super) shell_integration: Arc<dyn ShellIntegration>,
    #[allow(dead_code)]
    pub(super) executable_path: Option<String>,
    /// C# `_updateCts` / `_updateUrl` (W8-10e).
    #[allow(dead_code)]
    pub(super) update_cancel: Option<CancelToken>,
    pub(super) update_url: Option<String>,
    /// C# `_fallbackValidator` (W8-10d, `validate.rs`).
    pub(super) fallback_validator: Option<SettingsValidator>,
    /// C# `_cachedProbeActionsJson` / `_cachedProbeActions` (W8-10d, `validate.rs`).
    pub(super) cached_probe_actions_json: Option<String>,
    pub(super) cached_probe_actions: Vec<ReviewAction>,
}

impl fmt::Debug for SettingsPresenter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsPresenter")
            .field("has_store", &self.store.is_some())
            .field("has_localization", &self.localization.is_some())
            .field("physical_memory_bytes", &self.physical_memory_bytes)
            .finish_non_exhaustive()
    }
}

impl SettingsPresenter {
    /// C# constructor. `settings` = `current.clone()` (C# clones through the config.json JSON contract so every property
    /// survives; a derived `Clone` copies every field, which is the same thing). Starts with no update URL, no cached probe.
    pub fn new(current: &AppSettings, options: SettingsPresenterOptions) -> Self {
        Self {
            settings: current.clone(),
            store: options.store,
            localization: options.localization,
            update_checker: options.update_checker,
            physical_memory_bytes: options.physical_memory_bytes,
            settings_texts: options.settings_texts,
            shell_integration: options.shell_integration,
            executable_path: options.executable_path,
            update_cancel: None,
            update_url: None,
            fallback_validator: None,
            cached_probe_actions_json: None,
            cached_probe_actions: Vec::new(),
        }
    }

    /// C# `Settings { get; }`: the working copy (the caller reads it after a successful save).
    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// C# `HasLocalization`.
    pub fn has_localization(&self) -> bool {
        self.localization.is_some()
    }

    // ---- Pages ----

    /// C# `InitialPageKey`: the last page remembered in this process when it is one of [`PAGES`], else `"General"`.
    pub fn initial_page_key() -> String {
        let remembered = Self::remembered_page_key();
        if PAGES.iter().any(|page| page.key == remembered) {
            remembered
        } else {
            DEFAULT_PAGE_KEY.to_owned()
        }
    }

    /// C# `RememberPage(key)`: session-only memory (a process-wide `Mutex<String>`, initial value `"General"`), any text is
    /// stored (an unknown key is ignored later by `initial_page_key`).
    pub fn remember_page(key: &str) {
        *last_page().lock().unwrap_or_else(PoisonError::into_inner) = key.to_owned();
    }

    /// C# `RememberedPageKey`: the stored text as it is.
    pub fn remembered_page_key() -> String {
        last_page().lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// C# `NextPageIndex(current, count, forward)`: `((current + (forward ? 1 : -1)) % count + count) % count` with C#
    /// (truncating) `%`. `count` is 7 in production; the caller never passes 0.
    pub fn next_page_index(current: i32, count: i32, forward: bool) -> i32 {
        let step = if forward { 1 } else { -1 };
        (current.wrapping_add(step) % count + count) % count
    }

    /// C# `PageTitle(key)`: `ctx.texts.get(title_key)`, or `""` for an unknown key.
    pub fn page_title(key: &str, ctx: &FieldContext<'_>) -> String {
        PAGES
            .iter()
            .find(|page| page.key == key)
            .map(|page| ctx.texts.get(page.title_key))
            .unwrap_or_default()
    }

    // ---- Load ----

    /// C# `LoadFields()`: a fresh `SettingsFormState::default()`, every row of `settings_field_map::bindings()` loaded in table
    /// order, then `ram_cache_percent = f64::from(clamp(settings.image_cache_ram_percent, min, max))` where `(min, max)` is
    /// `ram_cache_range(state.preload_forward_text, state.preload_backward_text)` (so the slider range follows the TEXT the rows
    /// just wrote). The working copy is NOT modified (the clamped value is not written back). `Math.Clamp(v, min, max)` order:
    /// `v < min` gives `min`, then `v > max` gives `max`.
    pub fn load_fields(&self) -> SettingsFormState {
        let mut state = SettingsFormState::default();
        for binding in bindings() {
            (binding.load)(&self.settings, &mut state);
        }
        let (min, max) = self.ram_cache_range(
            Some(state.preload_forward_text.as_str()),
            Some(state.preload_backward_text.as_str()),
        );
        // C# Math.Clamp order: below min gives min, then above max gives max.
        let mut percent = self.settings.image_cache_ram_percent;
        if percent < min {
            percent = min;
        }
        if percent > max {
            percent = max;
        }
        state.ram_cache_percent = f64::from(percent);
        state
    }

    // ---- RAM cache slider ----

    /// C# `RamCacheRange(forwardText, backwardText)`: `forward = parse_or_fallback(forward_text, settings.preload_forward_count,
    /// MIN_PRELOAD_FORWARD_COUNT, MAX_PRELOAD_COUNT)`, `backward` likewise with `MIN_PRELOAD_BACKWARD_COUNT`; the pair is a
    /// `PreloadWindow::new(forward, backward)`; result `(ram_budget::minimum_cache_percent(self.physical_memory_bytes, window),
    /// MAX_IMAGE_CACHE_RAM_PERCENT)`. Each box falls back on its own.
    pub fn ram_cache_range(&self, forward_text: Option<&str>, backward_text: Option<&str>) -> (i32, i32) {
        let forward = parse_or_fallback(
            forward_text,
            self.settings.preload_forward_count,
            MIN_PRELOAD_FORWARD_COUNT,
            MAX_PRELOAD_COUNT,
        );
        let backward = parse_or_fallback(
            backward_text,
            self.settings.preload_backward_count,
            MIN_PRELOAD_BACKWARD_COUNT,
            MAX_PRELOAD_COUNT,
        );
        (
            minimum_cache_percent(self.physical_memory_bytes, PreloadWindow::new(forward, backward)),
            MAX_IMAGE_CACHE_RAM_PERCENT,
        )
    }

    /// C# `RamCacheHintText(min, max)`: `ctx.texts.format_with("settings.ramCache.hint", {total: format_gb(physical), min, max})`.
    pub fn ram_cache_hint_text(&self, min: i32, max: i32, ctx: &FieldContext<'_>) -> String {
        ctx.texts.format_with(
            "settings.ramCache.hint",
            &[
                LocArg::new("total", format_gb(self.physical_memory_bytes, ctx)),
                LocArg::new("min", min),
                LocArg::new("max", max),
            ],
            ctx.numbers,
        )
    }

    /// C# `RamCacheValueText(percent)`: `settings.ramCache.value` with `{percent}` and `{size}` =
    /// `format_gb(ram_budget::bytes_for_percent(percent, physical))`.
    pub fn ram_cache_value_text(&self, percent: i32, ctx: &FieldContext<'_>) -> String {
        ctx.texts.format_with(
            "settings.ramCache.value",
            &[
                LocArg::new("percent", percent),
                LocArg::new(
                    "size",
                    format_gb(bytes_for_percent(percent, self.physical_memory_bytes), ctx),
                ),
            ],
            ctx.numbers,
        )
    }

    // ---- Hints ----

    /// C# static `PreloadWindowHint()`: `settings.preloadWindow.hint` with `{min}` = `MIN_PRELOAD_BACKWARD_COUNT`, `{max}` = `MAX_PRELOAD_COUNT`.
    pub fn preload_window_hint(ctx: &FieldContext<'_>) -> String {
        ctx.texts.format_with(
            "settings.preloadWindow.hint",
            &[
                LocArg::new("min", MIN_PRELOAD_BACKWARD_COUNT),
                LocArg::new("max", MAX_PRELOAD_COUNT),
            ],
            ctx.numbers,
        )
    }

    /// C# static `ToolbarOpacityHint()`: `settings.toolbarOpacity.hint` with `{min}` = `MIN_TOOLBAR_OPACITY_PERCENT`.
    pub fn toolbar_opacity_hint(ctx: &FieldContext<'_>) -> String {
        ctx.texts.format_with(
            "settings.toolbarOpacity.hint",
            &[LocArg::new("min", MIN_TOOLBAR_OPACITY_PERCENT)],
            ctx.numbers,
        )
    }

    /// C# static `KeyboardZoomStepPercentHint()`: `settings.keyboardZoomStepPercent.hint` with `{min}` and `{max}` (the two
    /// `*_KEYBOARD_ZOOM_STEP_PERCENT` constants).
    pub fn keyboard_zoom_step_percent_hint(ctx: &FieldContext<'_>) -> String {
        ctx.texts.format_with(
            "settings.keyboardZoomStepPercent.hint",
            &[
                LocArg::new("min", MIN_KEYBOARD_ZOOM_STEP_PERCENT),
                LocArg::new("max", MAX_KEYBOARD_ZOOM_STEP_PERCENT),
            ],
            ctx.numbers,
        )
    }

    /// C# static `ToolbarOpacityValueText(sliderValue)`: `"{round} %"` where `round` is `Math.Round` (ties to EVEN,
    /// `f64::round_ties_even`) cast to `int` and written with the culture's negative sign.
    pub fn toolbar_opacity_value_text(slider_value: f64, ctx: &FieldContext<'_>) -> String {
        // C# `(int)Math.Round(v)`: ties to even; `as i32` saturates where C# is unspecified.
        let rounded = slider_value.round_ties_even() as i32;
        let sign = if rounded < 0 {
            ctx.numbers.negative_sign.as_str()
        } else {
            ""
        };
        format!("{sign}{} %", rounded.unsigned_abs())
    }

    /// C# static `FormatWebpHeicCodecStatus(codecs)` (Q-FMT-WEBP-HEIC): `webp` = `settings.webpHeicSupportEnabled.codecAvailable` or
    /// `.webpMissing`; `heif` = `.codecAvailable` when `codecs.heif()`, `.hevcMissing` when only `heif_container`, else
    /// `.heifMissing`; the result is `settings.webpHeicSupportEnabled.status` with `{webp}` and `{heif}`.
    pub fn format_webp_heic_codec_status(codecs: &WicCodecSupport, ctx: &FieldContext<'_>) -> String {
        let webp = if codecs.webp {
            ctx.texts.get("settings.webpHeicSupportEnabled.codecAvailable")
        } else {
            ctx.texts.get("settings.webpHeicSupportEnabled.webpMissing")
        };
        let heif = if codecs.heif() {
            ctx.texts.get("settings.webpHeicSupportEnabled.codecAvailable")
        } else if codecs.heif_container {
            ctx.texts.get("settings.webpHeicSupportEnabled.hevcMissing")
        } else {
            ctx.texts.get("settings.webpHeicSupportEnabled.heifMissing")
        };
        ctx.texts.format_with(
            "settings.webpHeicSupportEnabled.status",
            &[LocArg::new("webp", webp), LocArg::new("heif", heif)],
            ctx.numbers,
        )
    }

    /// C# static `IsTurboJpegAvailable(decoderFactory)`: `true` for `None` (no factory to ask), else
    /// `factory.is_registered(DecoderBackend::TurboJpeg)`.
    pub fn is_turbo_jpeg_available(decoder_factory: Option<&dyn ImageDecoderFactory>) -> bool {
        decoder_factory.is_none_or(|factory| factory.is_registered(DecoderBackend::TurboJpeg))
    }

    // ---- Defaults ----

    /// C# `ApplyDefaults()`: resets the listed properties of the working copy to their built-in values (everything not listed
    /// keeps its value: language, actions, window and other settings the dialog has no "default" for) and returns `load_fields()`.
    /// The porter reproduces the 65 assignments `s.X = ...` of C# lines 293 to 326 one by one, in order (counted with grep; `s.Shortcuts` is `ShortcutMappings::default()`). "new AppSettings().X" is
    /// `AppSettings::new(&*self.settings_texts).x`; constants come from `pr_core::settings::{perf_options, app_settings}`;
    /// `HiddenContextMenuItems = null` is `None`.
    pub fn apply_defaults(&mut self) -> SettingsFormState {
        let texts = Arc::clone(&self.settings_texts);
        let d = AppSettings::new(&*texts);
        let s = &mut self.settings;
        s.logging_enabled = false;
        s.journal_durability = JournalDurability::Fast;
        s.allow_permanent_delete_without_recycle_bin = false;
        s.confirm_before_delete = false;
        s.show_zoom_indicator = false;
        s.external_editor_path = String::new();
        s.image_cache_capacity_bytes = perf::IMAGE_CACHE_CAPACITY_BYTES;
        s.image_cache_ram_percent = perf::IMAGE_CACHE_RAM_PERCENT;
        s.memory_reserve_bytes = perf::MEMORY_RESERVE_BYTES;
        s.preload_worker_count = perf::PRELOAD_WORKER_COUNT;
        s.preload_memory_load_limit = perf::PRELOAD_MEMORY_LOAD_LIMIT;
        s.preload_forward_count = perf::PRELOAD_FORWARD_COUNT;
        s.preload_backward_count = perf::PRELOAD_BACKWARD_COUNT;
        s.preview_disk_cache_capacity_bytes = perf::PREVIEW_DISK_CACHE_CAPACITY_BYTES;
        s.use_source_bytes_cache = perf::USE_SOURCE_BYTES_CACHE;
        s.source_bytes_capacity_bytes = perf::SOURCE_BYTES_CAPACITY_BYTES;
        s.initial_view_mode = InitialViewMode::Fit;
        s.loading_mode = Some(LoadingMode::Preview);
        s.image_sort_mode = d.image_sort_mode;
        s.scaling_quality = Some(ScalingQuality::HighQuality);
        s.decoder_backend = d.decoder_backend;
        s.compare_hash_enabled = true;
        s.compare_size_enabled = true;
        s.shortcuts = ShortcutMappings::default();
        s.fit_width_anchor = FitWidthAnchor::Centre;
        s.keep_zoom_across_images = false;
        s.fit_width_anchor2 = d.fit_width_anchor2;
        s.middle_click_action = d.middle_click_action;
        s.instance_mode = InstanceMode::SingleWindow;
        s.show_info_overlay = true;
        s.show_file_info = true;
        s.show_folder_info = false;
        s.mouse_wheel_action = MouseWheelAction::Zoom;
        s.click_to_zoom_enabled = false;
        s.click_zoom_percent = DEFAULT_CLICK_ZOOM_PERCENT;
        s.kinetic_pan_enabled = d.kinetic_pan_enabled;
        s.kinetic_glide_smoothing = d.kinetic_glide_smoothing;
        s.arrow_key_navigates_at_zoom_edge = d.arrow_key_navigates_at_zoom_edge;
        s.click_zoom_key_toggles_fit = d.click_zoom_key_toggles_fit;
        s.arrow_pan_step_percent = DEFAULT_ARROW_PAN_STEP_PERCENT;
        s.keyboard_zoom_step_percent = DEFAULT_KEYBOARD_ZOOM_STEP_PERCENT;
        s.keyboard_zoom_anchor = d.keyboard_zoom_anchor;
        s.set_zoom_also_sets_click_level = d.set_zoom_also_sets_click_level;
        s.show_folder_menu_items = d.show_folder_menu_items;
        s.show_zoom_menu_items = d.show_zoom_menu_items;
        s.show_recycle_menu_item = d.show_recycle_menu_item;
        s.hidden_context_menu_items = None; // None = derive from the legacy defaults = the default hidden set
        s.move_copy_reuse_last_folder = false;
        s.image_transition = d.image_transition;
        s.image_transition_ms = DEFAULT_IMAGE_TRANSITION_MS;
        s.touchpad_swipe_enabled = d.touchpad_swipe_enabled;
        s.touchpad_swipe_distance_per_image = DEFAULT_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE;
        s.show_exif_info = d.show_exif_info;
        s.exif_info_fields = ExifInfoFields::DEFAULT;
        s.toolbar_auto_hide = d.toolbar_auto_hide;
        s.toolbar_auto_hide_delay_ms = DEFAULT_TOOLBAR_AUTO_HIDE_DELAY_MS;
        s.info_overlay_auto_hide = d.info_overlay_auto_hide;
        s.info_overlay_auto_hide_delay_ms = DEFAULT_INFO_OVERLAY_AUTO_HIDE_DELAY_MS;
        s.toolbar_opacity_percent = DEFAULT_TOOLBAR_OPACITY_PERCENT;
        s.info_overlay_font_size = DEFAULT_INFO_OVERLAY_FONT_SIZE;
        s.title_bar_fields = TitleBarFields::DEFAULT;
        s.raw_support_enabled = d.raw_support_enabled;
        s.webp_heic_support_enabled = d.webp_heic_support_enabled;
        s.raw_full_decode = d.raw_full_decode;
        s.raw_pair_mode = d.raw_pair_mode;
        self.load_fields()
    }
}

/// C# private `ParseOrFallback(text, fallback, min, max)`: `settings_field_map::try_int(text, min, max)` when `text` is `Some`
/// and valid and in range, else `fallback.clamp(min, max)` (`Math.Clamp`: a bound is never NaN, so `i32::clamp` is safe here).
fn parse_or_fallback(text: Option<&str>, fallback: i32, min: i32, max: i32) -> i32 {
    match text.and_then(|text| try_int(text, min, max)) {
        Some(value) => value,
        None => fallback.clamp(min, max),
    }
}

/// C# private `FormatGb(bytes)`: `bytes > 0` => `bytes / 1 GiB` written with ONE decimal in the current culture ("0.0":
/// `one_decimal_half_up` with the culture's decimal separator), else `"?"`. GiB are shown as "GB", as Windows does.
pub(crate) fn format_gb(bytes: i64, ctx: &FieldContext<'_>) -> String {
    if bytes > 0 {
        let gib = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        one_decimal_half_up(gib).replace('.', &ctx.numbers.decimal_separator)
    } else {
        "?".to_owned()
    }
}

/// .NET custom format `"0.0"` for a positive finite double, invariant culture: the value is first reduced to 15
/// significant digits and that digit string is then rounded HALF UP (away from zero) at one decimal. (`format_fixed` is
/// the standard `F1` format, which rounds an exact tie to even: 1.25 gives "1.2" there but "1.3" here.)
fn one_decimal_half_up(value: f64) -> String {
    let scientific = format!("{value:.14e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|digit| digit - b'0')
        .collect();
    // value * 10 = 0.d0 d1 d2 ... * 10^(exponent + 2): `whole` leading digits form the integer part.
    let whole = exponent + 2;
    let digit_at = |index: i32| {
        usize::try_from(index)
            .ok()
            .and_then(|at| digits.get(at))
            .copied()
            .unwrap_or(0)
    };
    let tenths = if whole < 0 {
        0
    } else {
        let mut number: u64 = 0;
        for index in 0..whole {
            number = number.saturating_mul(10).saturating_add(u64::from(digit_at(index)));
        }
        number.saturating_add(u64::from(digit_at(whole) >= 5))
    };
    format!("{}.{}", tenths / 10, tenths % 10)
}
