//! The persisted application settings (`config.json`) and their System.Text.Json-compatible reader and writer.
//!
//! Port of `PhotoReview.Core/Settings/AppSettings.cs` and `AppSettingsJsonContext.cs` (baseline d92d6aae).
//! Contract: `contracts/W2-01/contract.md`. Owner of: `AppSettings`, its constants, the `SettingsTexts` seam that
//! replaces the ambient `Tr.*` lookups of the settings code, `ReviewAction` deserialization and
//! `ShortcutKeyCanonical.CanonicalizeAll(AppSettings)` (lead decision D10).
//!
//! # Why a hand-written reader and writer (not `#[derive(Serialize, Deserialize)]`)
//!
//! `config.json` is read by `System.Text.Json` with options that serde does not have: comments and trailing commas,
//! numbers given as strings, case-sensitive names, duplicate properties where the LAST wins, a maximum depth of 64,
//! `null` accepted for strings but not for numbers or booleans, and (for the salvage path of `SettingsStore`) every
//! property convertible on its own. It is written with 2-space indentation, CRLF line ends, the default (HTML-safe,
//! ASCII-only) string escaper and shortest round-trip doubles. [`json`] holds the DOM, the parser, the scalar
//! conversions and the writer helpers; [`AppSettings::apply_json_property`] is the one place that knows the type of
//! every property.
//!
//! # What cannot be represented in Rust (and where it goes)
//!
//! * The eight `[JsonConverter(typeof(SettingsEnumConverter<T>))]` enum properties hold C#'s "undefined" sentinel
//!   `(T)int.MinValue` for unreadable input. Rust holds `None` in an `Option<T>` field; the normalizer resets it and
//!   reports the property. Use the `*_or_default` getters when a plain enum is needed.
//! * A JSON `null` (or a missing property) for `Actions` becomes the default actions, for `Shortcuts` the default
//!   shortcuts (`SettingsStore.FinishDeserialize` does the same), and for a string it becomes `""`.
//! * `null` entries inside `Actions` are dropped by the reader and counted in [`ReadNotes::null_actions`]; the
//!   normalizer reports `Actions` for them, exactly like C#'s `RemoveAll(a => a is null)`.
#![allow(unused_variables, dead_code)] // D11: stub with todo!() bodies; removed by the porter in the commit that removes the last todo!().

use std::error::Error;
use std::fmt;

use crate::model::enums::{
    DecoderBackend, FitWidthAnchor, ImageSortMode, ImageTransition, InitialViewMode, InstanceMode, JournalDurability,
    KeyboardZoomAnchor, KineticGlideSmoothing, LoadingMode, MiddleClickAction, MouseWheelAction, RawFullDecode,
    RawPairMode, ScalingQuality,
};
use crate::model::fields::{ExifInfoFields, TitleBarFields};
use crate::settings::perf_options as perf;
use crate::settings::shortcuts::{ReviewAction, ShortcutMappings, canonicalize_all};

// ---------------------------------------------------------------------------------------------------------------
// Constants (C# `public const` of AppSettings; C# `MinClickZoomPercent` is `MIN_CLICK_ZOOM_PERCENT`).
// ---------------------------------------------------------------------------------------------------------------

/// `AppSettings.CurrentConfigVersion`.
pub const CURRENT_CONFIG_VERSION: i32 = 3;
/// `LanguageLoader.AutoCode`: follow the Windows UI language. (Lives here because `pr-core` cannot see `pr-l10n`.)
pub const AUTO_LANGUAGE_CODE: &str = "auto";

pub const MIN_CLICK_ZOOM_PERCENT: i32 = 10;
pub const MAX_CLICK_ZOOM_PERCENT: i32 = 800;
pub const MIN_ARROW_PAN_STEP_PERCENT: i32 = 1;
pub const MAX_ARROW_PAN_STEP_PERCENT: i32 = 100;
pub const DEFAULT_ARROW_PAN_STEP_PERCENT: i32 = 10;
pub const DEFAULT_CLICK_ZOOM_PERCENT: i32 = 100;
pub const MIN_TOOLBAR_AUTO_HIDE_DELAY_MS: i32 = 0;
pub const MAX_TOOLBAR_AUTO_HIDE_DELAY_MS: i32 = 10_000;
pub const DEFAULT_TOOLBAR_AUTO_HIDE_DELAY_MS: i32 = 1_500;
pub const MIN_INFO_OVERLAY_AUTO_HIDE_DELAY_MS: i32 = 0;
pub const MAX_INFO_OVERLAY_AUTO_HIDE_DELAY_MS: i32 = 10_000;
pub const DEFAULT_INFO_OVERLAY_AUTO_HIDE_DELAY_MS: i32 = 3_000;
pub const MIN_TOOLBAR_OPACITY_PERCENT: i32 = 20;
pub const MAX_TOOLBAR_OPACITY_PERCENT: i32 = 100;
pub const DEFAULT_TOOLBAR_OPACITY_PERCENT: i32 = 100;
pub const MIN_INFO_OVERLAY_FONT_SIZE: f64 = 8.0;
pub const MAX_INFO_OVERLAY_FONT_SIZE: f64 = 24.0;
pub const DEFAULT_INFO_OVERLAY_FONT_SIZE: f64 = 12.0;
pub const MIN_KEYBOARD_ZOOM_STEP_PERCENT: i32 = 5;
pub const MAX_KEYBOARD_ZOOM_STEP_PERCENT: i32 = 100;
pub const DEFAULT_KEYBOARD_ZOOM_STEP_PERCENT: i32 = 10;
pub const MIN_IMAGE_TRANSITION_MS: i32 = 40;
pub const MAX_IMAGE_TRANSITION_MS: i32 = 400;
pub const DEFAULT_IMAGE_TRANSITION_MS: i32 = 120;
pub const MIN_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE: i32 = 30;
pub const MAX_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE: i32 = 2400;
pub const DEFAULT_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE: i32 = 200;

/// The JSON property names of `AppSettings`, in the order System.Text.Json writes them (C# declaration order).
/// 70 entries; pinned by the golden `settings-json` (`defaults.keys`) and by C# `ConfigKeyContractTests`.
pub const JSON_KEYS: [&str; 70] = [
    "ConfigVersion",
    "InitialViewMode",
    "LoadingMode",
    "LoggingEnabled",
    "ImageSortMode",
    "CompareHashEnabled",
    "CompareSizeEnabled",
    "ScalingQuality",
    "DecoderBackend",
    "ImageCacheCapacityBytes",
    "ImageCacheRamPercent",
    "MemoryReserveBytes",
    "PreloadWorkerCount",
    "PreloadMemoryLoadLimit",
    "PreviewDiskCacheCapacityBytes",
    "UseSourceBytesCache",
    "SourceBytesCapacityBytes",
    "Actions",
    "Shortcuts",
    "UiLanguage",
    "JournalDurability",
    "AllowPermanentDeleteWithoutRecycleBin",
    "LastMoveToFolder",
    "LastCopyToFolder",
    "MoveCopyReuseLastFolder",
    "InstanceMode",
    "ShowInfoOverlay",
    "ShowFileInfo",
    "ShowFolderInfo",
    "ShowExifInfo",
    "ExifInfoFields",
    "MouseWheelAction",
    "ClickToZoomEnabled",
    "ClickZoomPercent",
    "KineticPanEnabled",
    "PreloadForwardCount",
    "PreloadBackwardCount",
    "ToolbarAutoHide",
    "ToolbarAutoHideDelayMs",
    "InfoOverlayFontSize",
    "TitleBarFields",
    "KeyboardZoomStepPercent",
    "ConfirmBeforeDelete",
    "ShowZoomIndicator",
    "ExternalEditorPath",
    "KineticGlideSmoothing",
    "InfoOverlayAutoHide",
    "InfoOverlayAutoHideDelayMs",
    "ToolbarOpacityPercent",
    "ArrowKeyNavigatesAtZoomEdge",
    "ClickZoomKeyTogglesFit",
    "ArrowPanStepPercent",
    "KeepZoomAcrossImages",
    "FitWidthAnchor",
    "FitWidthAnchor2",
    "MiddleClickAction",
    "KeyboardZoomAnchor",
    "ImageTransition",
    "ImageTransitionMs",
    "SetZoomAlsoSetsClickLevel",
    "ShowFolderMenuItems",
    "ShowZoomMenuItems",
    "ShowRecycleMenuItem",
    "HiddenContextMenuItems",
    "RawSupportEnabled",
    "RawFullDecode",
    "RawPairMode",
    "WebpHeicSupportEnabled",
    "TouchpadSwipeEnabled",
    "TouchpadSwipeDistancePerImage",
];

// ---------------------------------------------------------------------------------------------------------------
// Localized texts (replaces the ambient `Tr.*` of the C# settings code)
// ---------------------------------------------------------------------------------------------------------------

/// The localized strings the settings code needs. C# reads them from the ambient `Localizer.Current` (`Tr.*`);
/// `pr-core` cannot see `pr-l10n`, so callers pass an implementation (W2-04 provides one over the real catalogs).
/// [`EnglishSettingsTexts`] is the built-in English catalog, which is what C# uses until a language is set.
pub trait SettingsTexts: Send + Sync {
    /// `Tr.ActionDefaultName(n)`; English `Action {n}`.
    fn action_default_name(&self, n: u32) -> String;
    /// `Tr.ActionDefaultBackup`; English `Backup`.
    fn action_default_backup(&self) -> String;
    /// `Tr.CoreSettingsShortcutInvalid(name)`; English `Shortcut {name} is not valid.`
    fn core_settings_shortcut_invalid(&self, name: &str) -> String;
    /// `Tr.CoreSettingsActionInvalid`; English `Each action needs a name and a valid shortcut.`
    fn core_settings_action_invalid(&self) -> String;
    /// `Tr.CoreSettingsActionBindingName(name)`; English `Action: {name}`.
    fn core_settings_action_binding_name(&self, name: &str) -> String;
    /// `Tr.CoreSettingsShortcutDuplicate(key, names)`; English `Key {key} is used more than once by: {names}.`
    fn core_settings_shortcut_duplicate(&self, key: &str, names: &str) -> String;
    /// `Tr.SettingsLoadRepaired(items)`; English `config.json contained invalid values. These settings were reset to their defaults: {items}.`
    fn settings_load_repaired(&self, items: &str) -> String;
    /// `Tr.DialogImportSettingsRepaired(items)`; English `The imported file contained invalid values. These settings were reset to their defaults: {items}.`
    fn dialog_import_settings_repaired(&self, items: &str) -> String;
    /// `Tr.SettingsLoadShortcutDisabled(name)`; English `The shortcut for {name} conflicted with another action and was turned off. You can assign it again in Settings > Shortcuts.`
    fn settings_load_shortcut_disabled(&self, name: &str) -> String;
}

/// The built-in English catalog (`en.json` at d92d6aae). Complete: no porter work.
#[derive(Debug, Default, Clone, Copy)]
pub struct EnglishSettingsTexts;

impl SettingsTexts for EnglishSettingsTexts {
    fn action_default_name(&self, n: u32) -> String {
        format!("Action {n}")
    }
    fn action_default_backup(&self) -> String {
        "Backup".to_owned()
    }
    fn core_settings_shortcut_invalid(&self, name: &str) -> String {
        format!("Shortcut {name} is not valid.")
    }
    fn core_settings_action_invalid(&self) -> String {
        "Each action needs a name and a valid shortcut.".to_owned()
    }
    fn core_settings_action_binding_name(&self, name: &str) -> String {
        format!("Action: {name}")
    }
    fn core_settings_shortcut_duplicate(&self, key: &str, names: &str) -> String {
        format!("Key {key} is used more than once by: {names}.")
    }
    fn settings_load_repaired(&self, items: &str) -> String {
        format!("config.json contained invalid values. These settings were reset to their defaults: {items}.")
    }
    fn dialog_import_settings_repaired(&self, items: &str) -> String {
        format!("The imported file contained invalid values. These settings were reset to their defaults: {items}.")
    }
    fn settings_load_shortcut_disabled(&self, name: &str) -> String {
        format!(
            "The shortcut for {name} conflicted with another action and was turned off. You can assign it again in Settings > Shortcuts."
        )
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------------------------

/// Stands for every `System.Text.Json.JsonException` the C# settings code can see (syntax error, depth, a value
/// that cannot be converted to the property type, a root that is not an object) and, for the writer, for the
/// `ArgumentException` System.Text.Json throws when a double is NaN or infinite. Only the fact of the error matters
/// to callers (the message is free text for logs); `SettingsStore` passes it to the startup-error callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsJsonError {
    pub message: String,
}

impl SettingsJsonError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SettingsJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for SettingsJsonError {}

// ---------------------------------------------------------------------------------------------------------------
// AppSettings
// ---------------------------------------------------------------------------------------------------------------

/// Facts the JSON reader saw that an `AppSettings` value cannot show. Transient: never written, cleared by
/// `normalizer::normalize`, ignored by `Default` (all zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadNotes {
    /// How many `null` entries the reader dropped from `Actions` (C# keeps them and `Normalize` removes them).
    pub null_actions: u32,
}

/// `AppSettings`: every persisted setting. Field order = C# declaration order = `config.json` order = [`JSON_KEYS`].
/// A property missing from the file keeps the default below (C# property initialisers).
///
/// Fields typed `Option<enum>` are the eight `SettingsEnumConverter` properties: `None` is the C# "undefined"
/// sentinel (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct AppSettings {
    /// `ConfigVersion`, default [`CURRENT_CONFIG_VERSION`].
    pub config_version: i32,
    /// Default `Fit`. Lenient enum (type-level converter): unknown text or number reads as value 0 (`Fit`).
    pub initial_view_mode: InitialViewMode,
    /// (S) default `Preview`.
    pub loading_mode: Option<LoadingMode>,
    pub logging_enabled: bool,
    /// (S) default `Default` (NOT the zero member `Name`).
    pub image_sort_mode: Option<ImageSortMode>,
    /// Default true.
    pub compare_hash_enabled: bool,
    /// Default true.
    pub compare_size_enabled: bool,
    /// (S) default `HighQuality`.
    pub scaling_quality: Option<ScalingQuality>,
    /// (S) default `WicDirect`.
    pub decoder_backend: Option<DecoderBackend>,
    /// Default [`perf::IMAGE_CACHE_CAPACITY_BYTES`].
    pub image_cache_capacity_bytes: i64,
    /// Default [`perf::IMAGE_CACHE_RAM_PERCENT`].
    pub image_cache_ram_percent: i32,
    /// Default [`perf::MEMORY_RESERVE_BYTES`].
    pub memory_reserve_bytes: i64,
    /// Default [`perf::PRELOAD_WORKER_COUNT`].
    pub preload_worker_count: i32,
    /// Default [`perf::PRELOAD_MEMORY_LOAD_LIMIT`].
    pub preload_memory_load_limit: f64,
    /// Default [`perf::PREVIEW_DISK_CACHE_CAPACITY_BYTES`].
    pub preview_disk_cache_capacity_bytes: i64,
    /// Default [`perf::USE_SOURCE_BYTES_CACHE`].
    pub use_source_bytes_cache: bool,
    /// Default [`perf::SOURCE_BYTES_CAPACITY_BYTES`].
    pub source_bytes_capacity_bytes: i64,
    /// Default `ReviewAction::defaults(...)` with the texts given to [`AppSettings::new`].
    pub actions: Vec<ReviewAction>,
    /// Default `ShortcutMappings::default()`.
    pub shortcuts: ShortcutMappings,
    /// Default [`AUTO_LANGUAGE_CODE`]. C# `null` reads as `""`; `SettingsStore::migrate` turns a blank into `auto`.
    pub ui_language: String,
    /// Default `Fast`.
    pub journal_durability: JournalDurability,
    pub allow_permanent_delete_without_recycle_bin: bool,
    /// Default `None`.
    pub last_move_to_folder: Option<String>,
    /// Default `None`.
    pub last_copy_to_folder: Option<String>,
    pub move_copy_reuse_last_folder: bool,
    /// Default `SingleWindow`.
    pub instance_mode: InstanceMode,
    /// Default true.
    pub show_info_overlay: bool,
    /// Default true.
    pub show_file_info: bool,
    pub show_folder_info: bool,
    pub show_exif_info: bool,
    /// Default `ExifInfoFields::DEFAULT`.
    pub exif_info_fields: ExifInfoFields,
    /// Default `Zoom`.
    pub mouse_wheel_action: MouseWheelAction,
    pub click_to_zoom_enabled: bool,
    /// Default [`DEFAULT_CLICK_ZOOM_PERCENT`].
    pub click_zoom_percent: i32,
    pub kinetic_pan_enabled: bool,
    /// Default [`perf::PRELOAD_FORWARD_COUNT`].
    pub preload_forward_count: i32,
    /// Default [`perf::PRELOAD_BACKWARD_COUNT`].
    pub preload_backward_count: i32,
    pub toolbar_auto_hide: bool,
    /// Default [`DEFAULT_TOOLBAR_AUTO_HIDE_DELAY_MS`].
    pub toolbar_auto_hide_delay_ms: i32,
    /// Default [`DEFAULT_INFO_OVERLAY_FONT_SIZE`].
    pub info_overlay_font_size: f64,
    /// Default `TitleBarFields::DEFAULT`.
    pub title_bar_fields: TitleBarFields,
    /// Default [`DEFAULT_KEYBOARD_ZOOM_STEP_PERCENT`].
    pub keyboard_zoom_step_percent: i32,
    pub confirm_before_delete: bool,
    pub show_zoom_indicator: bool,
    /// Default `""`. C# `null` reads as `""`.
    pub external_editor_path: String,
    /// (S) default `Predict`.
    pub kinetic_glide_smoothing: Option<KineticGlideSmoothing>,
    pub info_overlay_auto_hide: bool,
    /// Default [`DEFAULT_INFO_OVERLAY_AUTO_HIDE_DELAY_MS`].
    pub info_overlay_auto_hide_delay_ms: i32,
    /// Default [`DEFAULT_TOOLBAR_OPACITY_PERCENT`].
    pub toolbar_opacity_percent: i32,
    pub arrow_key_navigates_at_zoom_edge: bool,
    pub click_zoom_key_toggles_fit: bool,
    /// Default [`DEFAULT_ARROW_PAN_STEP_PERCENT`].
    pub arrow_pan_step_percent: i32,
    pub keep_zoom_across_images: bool,
    /// Default `Centre` (type-level lenient converter, NOT an (S) property).
    pub fit_width_anchor: FitWidthAnchor,
    /// (S) default `BottomThird`.
    pub fit_width_anchor2: Option<FitWidthAnchor>,
    /// (S) default `ActualSize`.
    pub middle_click_action: Option<MiddleClickAction>,
    /// (S) default `ViewportCentre`.
    pub keyboard_zoom_anchor: Option<KeyboardZoomAnchor>,
    /// Default `None`.
    pub image_transition: ImageTransition,
    /// Default [`DEFAULT_IMAGE_TRANSITION_MS`].
    pub image_transition_ms: i32,
    /// Default true.
    pub set_zoom_also_sets_click_level: bool,
    /// Default true. Legacy flag, mirrored by `context_menu::apply_hidden`.
    pub show_folder_menu_items: bool,
    /// Default false. Legacy flag.
    pub show_zoom_menu_items: bool,
    /// Default false. Legacy flag.
    pub show_recycle_menu_item: bool,
    /// Default `None` (= derive from the three legacy flags; `SettingsStore::migrate` materialises it). A JSON list
    /// may hold `null` elements, so the elements are `Option<String>`.
    pub hidden_context_menu_items: Option<Vec<Option<String>>>,
    /// Default true.
    pub raw_support_enabled: bool,
    /// Default `Never`.
    pub raw_full_decode: RawFullDecode,
    /// Default `Separate`.
    pub raw_pair_mode: RawPairMode,
    /// Default true.
    pub webp_heic_support_enabled: bool,
    /// Default true.
    pub touchpad_swipe_enabled: bool,
    /// Default [`DEFAULT_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE`].
    pub touchpad_swipe_distance_per_image: i32,
    /// Not a setting: see [`ReadNotes`]. Never serialized.
    pub read_notes: ReadNotes,
}

impl AppSettings {
    /// C# `new AppSettings()`: every property initialiser; the default actions use `texts` (C#: the ambient language).
    pub fn new(texts: &dyn SettingsTexts) -> Self {
        Self {
            config_version: CURRENT_CONFIG_VERSION,
            initial_view_mode: InitialViewMode::Fit,
            loading_mode: Some(LoadingMode::Preview),
            logging_enabled: false,
            image_sort_mode: Some(ImageSortMode::Default),
            compare_hash_enabled: true,
            compare_size_enabled: true,
            scaling_quality: Some(ScalingQuality::HighQuality),
            decoder_backend: Some(DecoderBackend::WicDirect),
            image_cache_capacity_bytes: perf::IMAGE_CACHE_CAPACITY_BYTES,
            image_cache_ram_percent: perf::IMAGE_CACHE_RAM_PERCENT,
            memory_reserve_bytes: perf::MEMORY_RESERVE_BYTES,
            preload_worker_count: perf::PRELOAD_WORKER_COUNT,
            preload_memory_load_limit: perf::PRELOAD_MEMORY_LOAD_LIMIT,
            preview_disk_cache_capacity_bytes: perf::PREVIEW_DISK_CACHE_CAPACITY_BYTES,
            use_source_bytes_cache: perf::USE_SOURCE_BYTES_CACHE,
            source_bytes_capacity_bytes: perf::SOURCE_BYTES_CAPACITY_BYTES,
            actions: ReviewAction::defaults(&|n| texts.action_default_name(n), &texts.action_default_backup()),
            shortcuts: ShortcutMappings::default(),
            ui_language: AUTO_LANGUAGE_CODE.to_owned(),
            journal_durability: JournalDurability::Fast,
            allow_permanent_delete_without_recycle_bin: false,
            last_move_to_folder: None,
            last_copy_to_folder: None,
            move_copy_reuse_last_folder: false,
            instance_mode: InstanceMode::SingleWindow,
            show_info_overlay: true,
            show_file_info: true,
            show_folder_info: false,
            show_exif_info: false,
            exif_info_fields: ExifInfoFields::DEFAULT,
            mouse_wheel_action: MouseWheelAction::Zoom,
            click_to_zoom_enabled: false,
            click_zoom_percent: DEFAULT_CLICK_ZOOM_PERCENT,
            kinetic_pan_enabled: false,
            preload_forward_count: perf::PRELOAD_FORWARD_COUNT,
            preload_backward_count: perf::PRELOAD_BACKWARD_COUNT,
            toolbar_auto_hide: false,
            toolbar_auto_hide_delay_ms: DEFAULT_TOOLBAR_AUTO_HIDE_DELAY_MS,
            info_overlay_font_size: DEFAULT_INFO_OVERLAY_FONT_SIZE,
            title_bar_fields: TitleBarFields::DEFAULT,
            keyboard_zoom_step_percent: DEFAULT_KEYBOARD_ZOOM_STEP_PERCENT,
            confirm_before_delete: false,
            show_zoom_indicator: false,
            external_editor_path: String::new(),
            kinetic_glide_smoothing: Some(KineticGlideSmoothing::Predict),
            info_overlay_auto_hide: false,
            info_overlay_auto_hide_delay_ms: DEFAULT_INFO_OVERLAY_AUTO_HIDE_DELAY_MS,
            toolbar_opacity_percent: DEFAULT_TOOLBAR_OPACITY_PERCENT,
            arrow_key_navigates_at_zoom_edge: false,
            click_zoom_key_toggles_fit: false,
            arrow_pan_step_percent: DEFAULT_ARROW_PAN_STEP_PERCENT,
            keep_zoom_across_images: false,
            fit_width_anchor: FitWidthAnchor::Centre,
            fit_width_anchor2: Some(FitWidthAnchor::BottomThird),
            middle_click_action: Some(MiddleClickAction::ActualSize),
            keyboard_zoom_anchor: Some(KeyboardZoomAnchor::ViewportCentre),
            image_transition: ImageTransition::None,
            image_transition_ms: DEFAULT_IMAGE_TRANSITION_MS,
            set_zoom_also_sets_click_level: true,
            show_folder_menu_items: true,
            show_zoom_menu_items: false,
            show_recycle_menu_item: false,
            hidden_context_menu_items: None,
            raw_support_enabled: true,
            raw_full_decode: RawFullDecode::Never,
            raw_pair_mode: RawPairMode::Separate,
            webp_heic_support_enabled: true,
            touchpad_swipe_enabled: true,
            touchpad_swipe_distance_per_image: DEFAULT_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE,
            read_notes: ReadNotes::default(),
        }
    }

    /// `LoadingMode`, or its default `Preview` when the field is the undefined sentinel (`None`).
    pub fn loading_mode_or_default(&self) -> LoadingMode {
        self.loading_mode.unwrap_or(LoadingMode::Preview)
    }

    /// `ImageSortMode`, or its default `Default` when undefined.
    pub fn image_sort_mode_or_default(&self) -> ImageSortMode {
        self.image_sort_mode.unwrap_or(ImageSortMode::Default)
    }

    /// `ScalingQuality`, or its default `HighQuality` when undefined.
    pub fn scaling_quality_or_default(&self) -> ScalingQuality {
        self.scaling_quality.unwrap_or(ScalingQuality::HighQuality)
    }

    /// `DecoderBackend`, or its default `WicDirect` when undefined.
    pub fn decoder_backend_or_default(&self) -> DecoderBackend {
        self.decoder_backend.unwrap_or(DecoderBackend::WicDirect)
    }

    /// `KineticGlideSmoothing`, or its default `Predict` when undefined.
    pub fn kinetic_glide_smoothing_or_default(&self) -> KineticGlideSmoothing {
        self.kinetic_glide_smoothing.unwrap_or(KineticGlideSmoothing::Predict)
    }

    /// `FitWidthAnchor2`, or its default `BottomThird` when undefined.
    pub fn fit_width_anchor2_or_default(&self) -> FitWidthAnchor {
        self.fit_width_anchor2.unwrap_or(FitWidthAnchor::BottomThird)
    }

    /// `MiddleClickAction`, or its default `ActualSize` when undefined.
    pub fn middle_click_action_or_default(&self) -> MiddleClickAction {
        self.middle_click_action.unwrap_or(MiddleClickAction::ActualSize)
    }

    /// `KeyboardZoomAnchor`, or its default `ViewportCentre` when undefined.
    pub fn keyboard_zoom_anchor_or_default(&self) -> KeyboardZoomAnchor {
        self.keyboard_zoom_anchor.unwrap_or(KeyboardZoomAnchor::ViewportCentre)
    }

    /// `JsonSerializer.Deserialize(text, AppSettingsJsonContext.Default.AppSettings)`: the whole document at once.
    ///
    /// `Ok(None)` when the text is the JSON literal `null` (C# returns null). `Err` for any `JsonException` of C#
    /// (see [`json::parse`] for the syntax rules and [`AppSettings::apply_json_property`] for the conversions); the
    /// FIRST failing property aborts, and a root that is not an object is an error. Starts from `AppSettings::new(texts)`
    /// and applies the properties in file order, so a duplicated property is applied twice (the last one wins, but an
    /// earlier unreadable duplicate already failed the call).
    pub fn from_json(text: &str, texts: &dyn SettingsTexts) -> Result<Option<AppSettings>, SettingsJsonError> {
        todo!("W2-01: json::parse then from_json_value")
    }

    /// As [`AppSettings::from_json`] for an already parsed document.
    pub fn from_json_value(
        root: &json::JsonValue,
        texts: &dyn SettingsTexts,
    ) -> Result<Option<AppSettings>, SettingsJsonError> {
        todo!(
            "W2-01: Null -> Ok(None); Object -> new + apply_json_property for each entry (a None name is an Err); anything else is an Err"
        )
    }

    /// Reads ONE property the way System.Text.Json does for that property's C# type and stores it.
    ///
    /// `Ok(true)` when `name` is a known property (case-sensitive, ordinal) and the value was stored; `Ok(false)` when
    /// the name is unknown (the value is ignored; it was already syntax-checked by the parser); `Err` when the value
    /// cannot be converted to the property type. The per-property conversion table is in the contract. `SettingsStore`
    /// calls this once per property for the salvage path.
    pub fn apply_json_property(
        &mut self,
        name: &str,
        value: &json::JsonValue,
        texts: &dyn SettingsTexts,
    ) -> Result<bool, SettingsJsonError> {
        todo!("W2-01: match name over JSON_KEYS; see contract table")
    }

    /// `JsonSerializer.Serialize(settings, AppSettingsJsonContext.Default.AppSettings)`: indented (2 spaces), CRLF line
    /// ends, no trailing newline, every property in [`JSON_KEYS`] order, strings escaped like the default
    /// `JavaScriptEncoder` ([`json::escape_string`]), doubles like [`json::format_f64`].
    ///
    /// `Err` when `preload_memory_load_limit` or `info_overlay_font_size` is NaN or infinite (C#: `ArgumentException`).
    pub fn to_json(&self) -> Result<String, SettingsJsonError> {
        todo!("W2-01")
    }

    /// `ShortcutKeyCanonical.CanonicalizeAll(AppSettings)`: canonicalises all 28 shortcut strings and every action
    /// shortcut in place (`shortcuts::canonicalize_all`). Used by `SettingsNormalizer.Normalize` and `SettingsStore.Persist`.
    pub fn canonicalize_all(&mut self) {
        canonicalize_all(&mut self.shortcuts, &mut self.actions);
    }

    /// `AppSettings.Clone(source)`: a deep copy. C# round-trips through JSON (so it also drops nothing and fixes
    /// nothing); `Clone` gives the same value for every state Rust can hold. Complete: no porter work.
    pub fn deep_clone(&self) -> AppSettings {
        self.clone()
    }
}

impl Default for AppSettings {
    /// `new AppSettings()` with the English texts.
    fn default() -> Self {
        Self::new(&EnglishSettingsTexts)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// JSON: a System.Text.Json-compatible DOM, reader and writer helpers
// ---------------------------------------------------------------------------------------------------------------

/// The lenient JSON reader and the writer helpers behind [`AppSettings`] (and `SettingsStore`'s `MarkUnversionedAsLegacy`
/// / `UiLanguageDeclared`, which peek at the raw document).
pub mod json {
    use super::SettingsJsonError;

    /// Maximum nesting depth (`JsonReaderOptions.MaxDepth` default). The root object or array is depth 1;
    /// the 65th nested level is an error.
    pub const MAX_DEPTH: usize = 64;

    /// A JSON string as written: `None` when it contains an unpaired surrogate escape (`"\ud800"`), which no Rust
    /// `String` can hold. Skipping such a value is fine; reading it into a typed value is a conversion error.
    pub type JsonText = Option<String>;

    /// A parsed JSON value. Objects keep every entry in file order INCLUDING duplicate names.
    #[derive(Debug, Clone, PartialEq)]
    pub enum JsonValue {
        Null,
        Bool(bool),
        /// The number token exactly as written; the parser has validated the JSON number grammar.
        Number(String),
        String(JsonText),
        Array(Vec<JsonValue>),
        Object(Vec<(JsonText, JsonValue)>),
    }

    impl JsonValue {
        /// The value of the LAST entry called `name` (ordinal, case-sensitive) when `self` is an object;
        /// `None` otherwise or when absent. (`JsonElement.TryGetProperty`.)
        pub fn property(&self, name: &str) -> Option<&JsonValue> {
            todo!("W2-01")
        }
    }

    /// Parses `text` with the options `config.json` is read with: standard JSON plus `//` and `/* */` comments
    /// wherever whitespace is allowed (see contract for the exact rules) and one trailing comma in objects and arrays.
    /// `Err` for everything System.Text.Json rejects: see the contract's syntax table (BOM, NBSP, form feed, single
    /// quotes, leading zeros, depth above [`MAX_DEPTH`], text after the root value, empty text...).
    pub fn parse(text: &str) -> Result<JsonValue, SettingsJsonError> {
        todo!("W2-01")
    }

    /// `int` property from a JSON value: a number token with only `[-]digits` that fits `i32`, or a string
    /// `[+-]?digits+` (no spaces) that fits. `None` = not convertible (C# `JsonException`).
    pub fn to_i32(value: &JsonValue) -> Option<i32> {
        todo!("W2-01")
    }

    /// `long` property: as [`to_i32`] with the `i64` range.
    pub fn to_i64(value: &JsonValue) -> Option<i64> {
        todo!("W2-01")
    }

    /// `double` property: a number token (overflow gives +/- infinity), or a string in the grammar of the contract
    /// (`NaN`, `Infinity`, `-Infinity`, or a decimal literal that must be finite).
    pub fn to_f64(value: &JsonValue) -> Option<f64> {
        todo!("W2-01")
    }

    /// `bool` property: only the literals `true` / `false` (a string, number or null is not convertible).
    pub fn to_bool(value: &JsonValue) -> Option<bool> {
        todo!("W2-01")
    }

    /// `string` property: a string value (`Some(Some(text))`), or `null` (`Some(None)`). `None` = not convertible
    /// (number, bool, array, object, or a string with an unpaired surrogate escape).
    pub fn to_string_or_null(value: &JsonValue) -> Option<Option<String>> {
        todo!("W2-01")
    }

    /// Adapter for the W1-02 helpers (`deserialize_lenient`, `deserialize_settings_enum`, `deserialize_flags`), which
    /// take a serde `Deserializer`. `Err` for a string with an unpaired surrogate (C#: the converter's
    /// `GetString()` throws, surfacing as `JsonException`). Numbers that are integers become `i64`/`u64`; any other
    /// number token becomes `f64` (or `Null` when not finite); arrays and objects become empty ones (their content never matters).
    pub fn to_serde_value(value: &JsonValue) -> Result<serde_json::Value, SettingsJsonError> {
        todo!("W2-01")
    }

    /// The text System.Text.Json writes for a string value with the default encoder, INCLUDING the surrounding quotes.
    /// See the contract's escape table (every non-ASCII char as `\uXXXX` upper-case hex with surrogate pairs;
    /// `"` `&` `'` `+` `<` `>` and backtick as `\u00XX`; `\` as `\\`; `\b \t \n \f \r`; other controls and DEL as `\u00XX`).
    pub fn escape_string(s: &str) -> String {
        todo!("W2-01")
    }

    /// The text `Utf8JsonWriter.WriteNumberValue(double)` writes: the shortest digits that round-trip, plain notation
    /// when the decimal exponent is between -5 (exclusive) and 17 (exclusive), otherwise `d[.ddd]E+XX` / `E-XX`
    /// (sign always present, at least two exponent digits). `-0.0` is `-0`. `None` for NaN and the infinities.
    pub fn format_f64(v: f64) -> Option<String> {
        todo!("W2-01")
    }
}
