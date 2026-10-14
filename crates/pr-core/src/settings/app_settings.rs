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

use std::error::Error;
use std::fmt;

use crate::model::enums::{
    DecoderBackend, FitWidthAnchor, ImageSortMode, ImageTransition, InitialViewMode, InstanceMode, JournalDurability,
    KeyboardZoomAnchor, KineticGlideSmoothing, LoadingMode, MiddleClickAction, MouseWheelAction, RawFullDecode,
    RawPairMode, ScalingQuality,
};
use crate::model::fields::{ExifInfoFields, TitleBarFields};
use crate::model::serde_ext::{
    FlagEnum, LenientEnum, deserialize_flags, deserialize_lenient, deserialize_settings_enum, format_flags,
    lenient_text,
};
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
        let root = json::parse(text)?;
        Self::from_json_value(&root, texts)
    }

    /// As [`AppSettings::from_json`] for an already parsed document.
    pub fn from_json_value(
        root: &json::JsonValue,
        texts: &dyn SettingsTexts,
    ) -> Result<Option<AppSettings>, SettingsJsonError> {
        match root {
            json::JsonValue::Null => Ok(None),
            json::JsonValue::Object(entries) => {
                let mut settings = AppSettings::new(texts);
                for (name, value) in entries {
                    let Some(name) = name else {
                        return Err(SettingsJsonError::new(
                            "a property name contains an unpaired surrogate and cannot be read",
                        ));
                    };
                    settings.apply_json_property(name, value, texts)?;
                }
                Ok(Some(settings))
            }
            _ => Err(SettingsJsonError::new("the root of config.json must be a JSON object")),
        }
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
        macro_rules! set {
            ($field:ident, $conv:expr) => {{
                self.$field = $conv(value, name)?;
            }};
        }
        match name {
            "ConfigVersion" => set!(config_version, read_i32),
            "InitialViewMode" => set!(initial_view_mode, read_lenient),
            "LoadingMode" => set!(loading_mode, read_settings_enum),
            "LoggingEnabled" => set!(logging_enabled, read_bool),
            "ImageSortMode" => set!(image_sort_mode, read_settings_enum),
            "CompareHashEnabled" => set!(compare_hash_enabled, read_bool),
            "CompareSizeEnabled" => set!(compare_size_enabled, read_bool),
            "ScalingQuality" => set!(scaling_quality, read_settings_enum),
            "DecoderBackend" => set!(decoder_backend, read_settings_enum),
            "ImageCacheCapacityBytes" => set!(image_cache_capacity_bytes, read_i64),
            "ImageCacheRamPercent" => set!(image_cache_ram_percent, read_i32),
            "MemoryReserveBytes" => set!(memory_reserve_bytes, read_i64),
            "PreloadWorkerCount" => set!(preload_worker_count, read_i32),
            "PreloadMemoryLoadLimit" => set!(preload_memory_load_limit, read_f64),
            "PreviewDiskCacheCapacityBytes" => set!(preview_disk_cache_capacity_bytes, read_i64),
            "UseSourceBytesCache" => set!(use_source_bytes_cache, read_bool),
            "SourceBytesCapacityBytes" => set!(source_bytes_capacity_bytes, read_i64),
            "Actions" => {
                let (actions, nulls) = read_actions(value, texts)?;
                self.actions = actions;
                self.read_notes.null_actions = nulls;
            }
            "Shortcuts" => self.shortcuts = read_shortcuts(value)?,
            "UiLanguage" => set!(ui_language, read_string),
            "JournalDurability" => set!(journal_durability, read_lenient),
            "AllowPermanentDeleteWithoutRecycleBin" => set!(allow_permanent_delete_without_recycle_bin, read_bool),
            "LastMoveToFolder" => set!(last_move_to_folder, read_opt_string),
            "LastCopyToFolder" => set!(last_copy_to_folder, read_opt_string),
            "MoveCopyReuseLastFolder" => set!(move_copy_reuse_last_folder, read_bool),
            "InstanceMode" => set!(instance_mode, read_lenient),
            "ShowInfoOverlay" => set!(show_info_overlay, read_bool),
            "ShowFileInfo" => set!(show_file_info, read_bool),
            "ShowFolderInfo" => set!(show_folder_info, read_bool),
            "ShowExifInfo" => set!(show_exif_info, read_bool),
            "ExifInfoFields" => set!(exif_info_fields, read_flags),
            "MouseWheelAction" => set!(mouse_wheel_action, read_lenient),
            "ClickToZoomEnabled" => set!(click_to_zoom_enabled, read_bool),
            "ClickZoomPercent" => set!(click_zoom_percent, read_i32),
            "KineticPanEnabled" => set!(kinetic_pan_enabled, read_bool),
            "PreloadForwardCount" => set!(preload_forward_count, read_i32),
            "PreloadBackwardCount" => set!(preload_backward_count, read_i32),
            "ToolbarAutoHide" => set!(toolbar_auto_hide, read_bool),
            "ToolbarAutoHideDelayMs" => set!(toolbar_auto_hide_delay_ms, read_i32),
            "InfoOverlayFontSize" => set!(info_overlay_font_size, read_f64),
            "TitleBarFields" => set!(title_bar_fields, read_flags),
            "KeyboardZoomStepPercent" => set!(keyboard_zoom_step_percent, read_i32),
            "ConfirmBeforeDelete" => set!(confirm_before_delete, read_bool),
            "ShowZoomIndicator" => set!(show_zoom_indicator, read_bool),
            "ExternalEditorPath" => set!(external_editor_path, read_string),
            "KineticGlideSmoothing" => set!(kinetic_glide_smoothing, read_settings_enum),
            "InfoOverlayAutoHide" => set!(info_overlay_auto_hide, read_bool),
            "InfoOverlayAutoHideDelayMs" => set!(info_overlay_auto_hide_delay_ms, read_i32),
            "ToolbarOpacityPercent" => set!(toolbar_opacity_percent, read_i32),
            "ArrowKeyNavigatesAtZoomEdge" => set!(arrow_key_navigates_at_zoom_edge, read_bool),
            "ClickZoomKeyTogglesFit" => set!(click_zoom_key_toggles_fit, read_bool),
            "ArrowPanStepPercent" => set!(arrow_pan_step_percent, read_i32),
            "KeepZoomAcrossImages" => set!(keep_zoom_across_images, read_bool),
            "FitWidthAnchor" => set!(fit_width_anchor, read_lenient),
            "FitWidthAnchor2" => set!(fit_width_anchor2, read_settings_enum),
            "MiddleClickAction" => set!(middle_click_action, read_settings_enum),
            "KeyboardZoomAnchor" => set!(keyboard_zoom_anchor, read_settings_enum),
            "ImageTransition" => set!(image_transition, read_lenient),
            "ImageTransitionMs" => set!(image_transition_ms, read_i32),
            "SetZoomAlsoSetsClickLevel" => set!(set_zoom_also_sets_click_level, read_bool),
            "ShowFolderMenuItems" => set!(show_folder_menu_items, read_bool),
            "ShowZoomMenuItems" => set!(show_zoom_menu_items, read_bool),
            "ShowRecycleMenuItem" => set!(show_recycle_menu_item, read_bool),
            "HiddenContextMenuItems" => set!(hidden_context_menu_items, read_string_list),
            "RawSupportEnabled" => set!(raw_support_enabled, read_bool),
            "RawFullDecode" => set!(raw_full_decode, read_lenient),
            "RawPairMode" => set!(raw_pair_mode, read_lenient),
            "WebpHeicSupportEnabled" => set!(webp_heic_support_enabled, read_bool),
            "TouchpadSwipeEnabled" => set!(touchpad_swipe_enabled, read_bool),
            "TouchpadSwipeDistancePerImage" => set!(touchpad_swipe_distance_per_image, read_i32),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// `JsonSerializer.Serialize(settings, AppSettingsJsonContext.Default.AppSettings)`: indented (2 spaces), CRLF line
    /// ends, no trailing newline, every property in [`JSON_KEYS`] order, strings escaped like the default
    /// `JavaScriptEncoder` ([`json::escape_string`]), doubles like [`json::format_f64`].
    ///
    /// `Err` when `preload_memory_load_limit` or `info_overlay_font_size` is NaN or infinite (C#: `ArgumentException`).
    pub fn to_json(&self) -> Result<String, SettingsJsonError> {
        let mut out = String::with_capacity(4096);
        out.push_str("{\r\n");
        for (i, key) in JSON_KEYS.iter().enumerate() {
            out.push_str("  \"");
            out.push_str(key);
            out.push_str("\": ");
            self.write_property(key, &mut out)?;
            if i + 1 < JSON_KEYS.len() {
                out.push(',');
            }
            out.push_str("\r\n");
        }
        out.push('}');
        Ok(out)
    }

    /// Writes the value of one top-level property (nested lines are already indented for depth 1).
    fn write_property(&self, key: &str, out: &mut String) -> Result<(), SettingsJsonError> {
        fn int(out: &mut String, v: impl fmt::Display) {
            out.push_str(&v.to_string());
        }
        fn boolean(out: &mut String, v: bool) {
            out.push_str(if v { "true" } else { "false" });
        }
        fn text(out: &mut String, v: &str) {
            out.push_str(&json::escape_string(v));
        }
        fn opt_text(out: &mut String, v: &Option<String>) {
            match v {
                Some(s) => text(out, s),
                None => out.push_str("null"),
            }
        }
        fn lenient<T: LenientEnum>(out: &mut String, v: T) {
            text(out, lenient_text(v));
        }
        fn settings_enum<T: LenientEnum>(out: &mut String, v: Option<T>) {
            match v {
                Some(m) => text(out, lenient_text(m)),
                None => text(out, "-2147483648"),
            }
        }
        fn double(out: &mut String, v: f64, name: &str) -> Result<(), SettingsJsonError> {
            match json::format_f64(v) {
                Some(s) => {
                    out.push_str(&s);
                    Ok(())
                }
                None => Err(SettingsJsonError::new(format!(
                    "{name} is NaN or infinite and cannot be written as a JSON number"
                ))),
            }
        }
        match key {
            "ConfigVersion" => int(out, self.config_version),
            "InitialViewMode" => lenient(out, self.initial_view_mode),
            "LoadingMode" => settings_enum(out, self.loading_mode),
            "LoggingEnabled" => boolean(out, self.logging_enabled),
            "ImageSortMode" => settings_enum(out, self.image_sort_mode),
            "CompareHashEnabled" => boolean(out, self.compare_hash_enabled),
            "CompareSizeEnabled" => boolean(out, self.compare_size_enabled),
            "ScalingQuality" => settings_enum(out, self.scaling_quality),
            "DecoderBackend" => settings_enum(out, self.decoder_backend),
            "ImageCacheCapacityBytes" => int(out, self.image_cache_capacity_bytes),
            "ImageCacheRamPercent" => int(out, self.image_cache_ram_percent),
            "MemoryReserveBytes" => int(out, self.memory_reserve_bytes),
            "PreloadWorkerCount" => int(out, self.preload_worker_count),
            "PreloadMemoryLoadLimit" => double(out, self.preload_memory_load_limit, key)?,
            "PreviewDiskCacheCapacityBytes" => int(out, self.preview_disk_cache_capacity_bytes),
            "UseSourceBytesCache" => boolean(out, self.use_source_bytes_cache),
            "SourceBytesCapacityBytes" => int(out, self.source_bytes_capacity_bytes),
            "Actions" => {
                if self.actions.is_empty() {
                    out.push_str("[]");
                } else {
                    out.push_str("[\r\n");
                    for (i, a) in self.actions.iter().enumerate() {
                        out.push_str("    {\r\n      \"Name\": ");
                        text(out, &a.name);
                        out.push_str(",\r\n      \"Shortcut\": ");
                        text(out, &a.shortcut);
                        out.push_str(",\r\n      \"Operation\": ");
                        lenient(out, a.operation);
                        out.push_str(",\r\n      \"Destination\": ");
                        text(out, &a.destination);
                        out.push_str(",\r\n      \"Confirm\": ");
                        boolean(out, a.confirm);
                        out.push_str("\r\n    }");
                        if i + 1 < self.actions.len() {
                            out.push(',');
                        }
                        out.push_str("\r\n");
                    }
                    out.push_str("  ]");
                }
            }
            "Shortcuts" => {
                out.push_str("{\r\n");
                let entries = self.shortcuts.entries();
                let last = entries.len();
                for (i, (name, value)) in entries.into_iter().enumerate() {
                    out.push_str("    \"");
                    out.push_str(name);
                    out.push_str("\": ");
                    text(out, value);
                    if i + 1 < last {
                        out.push(',');
                    }
                    out.push_str("\r\n");
                }
                out.push_str("  }");
            }
            "UiLanguage" => text(out, &self.ui_language),
            "JournalDurability" => lenient(out, self.journal_durability),
            "AllowPermanentDeleteWithoutRecycleBin" => boolean(out, self.allow_permanent_delete_without_recycle_bin),
            "LastMoveToFolder" => opt_text(out, &self.last_move_to_folder),
            "LastCopyToFolder" => opt_text(out, &self.last_copy_to_folder),
            "MoveCopyReuseLastFolder" => boolean(out, self.move_copy_reuse_last_folder),
            "InstanceMode" => lenient(out, self.instance_mode),
            "ShowInfoOverlay" => boolean(out, self.show_info_overlay),
            "ShowFileInfo" => boolean(out, self.show_file_info),
            "ShowFolderInfo" => boolean(out, self.show_folder_info),
            "ShowExifInfo" => boolean(out, self.show_exif_info),
            "ExifInfoFields" => text(out, &format_flags(self.exif_info_fields)),
            "MouseWheelAction" => lenient(out, self.mouse_wheel_action),
            "ClickToZoomEnabled" => boolean(out, self.click_to_zoom_enabled),
            "ClickZoomPercent" => int(out, self.click_zoom_percent),
            "KineticPanEnabled" => boolean(out, self.kinetic_pan_enabled),
            "PreloadForwardCount" => int(out, self.preload_forward_count),
            "PreloadBackwardCount" => int(out, self.preload_backward_count),
            "ToolbarAutoHide" => boolean(out, self.toolbar_auto_hide),
            "ToolbarAutoHideDelayMs" => int(out, self.toolbar_auto_hide_delay_ms),
            "InfoOverlayFontSize" => double(out, self.info_overlay_font_size, key)?,
            "TitleBarFields" => text(out, &format_flags(self.title_bar_fields)),
            "KeyboardZoomStepPercent" => int(out, self.keyboard_zoom_step_percent),
            "ConfirmBeforeDelete" => boolean(out, self.confirm_before_delete),
            "ShowZoomIndicator" => boolean(out, self.show_zoom_indicator),
            "ExternalEditorPath" => text(out, &self.external_editor_path),
            "KineticGlideSmoothing" => settings_enum(out, self.kinetic_glide_smoothing),
            "InfoOverlayAutoHide" => boolean(out, self.info_overlay_auto_hide),
            "InfoOverlayAutoHideDelayMs" => int(out, self.info_overlay_auto_hide_delay_ms),
            "ToolbarOpacityPercent" => int(out, self.toolbar_opacity_percent),
            "ArrowKeyNavigatesAtZoomEdge" => boolean(out, self.arrow_key_navigates_at_zoom_edge),
            "ClickZoomKeyTogglesFit" => boolean(out, self.click_zoom_key_toggles_fit),
            "ArrowPanStepPercent" => int(out, self.arrow_pan_step_percent),
            "KeepZoomAcrossImages" => boolean(out, self.keep_zoom_across_images),
            "FitWidthAnchor" => lenient(out, self.fit_width_anchor),
            "FitWidthAnchor2" => settings_enum(out, self.fit_width_anchor2),
            "MiddleClickAction" => settings_enum(out, self.middle_click_action),
            "KeyboardZoomAnchor" => settings_enum(out, self.keyboard_zoom_anchor),
            "ImageTransition" => lenient(out, self.image_transition),
            "ImageTransitionMs" => int(out, self.image_transition_ms),
            "SetZoomAlsoSetsClickLevel" => boolean(out, self.set_zoom_also_sets_click_level),
            "ShowFolderMenuItems" => boolean(out, self.show_folder_menu_items),
            "ShowZoomMenuItems" => boolean(out, self.show_zoom_menu_items),
            "ShowRecycleMenuItem" => boolean(out, self.show_recycle_menu_item),
            "HiddenContextMenuItems" => match &self.hidden_context_menu_items {
                None => out.push_str("null"),
                Some(items) if items.is_empty() => out.push_str("[]"),
                Some(items) => {
                    out.push_str("[\r\n");
                    for (i, item) in items.iter().enumerate() {
                        out.push_str("    ");
                        opt_text(out, item);
                        if i + 1 < items.len() {
                            out.push(',');
                        }
                        out.push_str("\r\n");
                    }
                    out.push_str("  ]");
                }
            },
            "RawSupportEnabled" => boolean(out, self.raw_support_enabled),
            "RawFullDecode" => lenient(out, self.raw_full_decode),
            "RawPairMode" => lenient(out, self.raw_pair_mode),
            "WebpHeicSupportEnabled" => boolean(out, self.webp_heic_support_enabled),
            "TouchpadSwipeEnabled" => boolean(out, self.touchpad_swipe_enabled),
            "TouchpadSwipeDistancePerImage" => int(out, self.touchpad_swipe_distance_per_image),
            _ => {}
        }
        Ok(())
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
// Per-property conversions (private helpers of `apply_json_property`)
// ---------------------------------------------------------------------------------------------------------------

fn conversion_error(name: &str, expected: &str) -> SettingsJsonError {
    SettingsJsonError::new(format!("the value of {name} cannot be converted to {expected}"))
}

fn read_i32(value: &json::JsonValue, name: &str) -> Result<i32, SettingsJsonError> {
    json::to_i32(value).ok_or_else(|| conversion_error(name, "Int32"))
}

fn read_i64(value: &json::JsonValue, name: &str) -> Result<i64, SettingsJsonError> {
    json::to_i64(value).ok_or_else(|| conversion_error(name, "Int64"))
}

fn read_f64(value: &json::JsonValue, name: &str) -> Result<f64, SettingsJsonError> {
    json::to_f64(value).ok_or_else(|| conversion_error(name, "Double"))
}

fn read_bool(value: &json::JsonValue, name: &str) -> Result<bool, SettingsJsonError> {
    json::to_bool(value).ok_or_else(|| conversion_error(name, "Boolean"))
}

/// C# `string` property: `null` reads as `""`.
fn read_string(value: &json::JsonValue, name: &str) -> Result<String, SettingsJsonError> {
    json::to_string_or_null(value)
        .map(Option::unwrap_or_default)
        .ok_or_else(|| conversion_error(name, "String"))
}

/// C# `string?` property: `null` stays `None`.
fn read_opt_string(value: &json::JsonValue, name: &str) -> Result<Option<String>, SettingsJsonError> {
    json::to_string_or_null(value).ok_or_else(|| conversion_error(name, "String"))
}

fn read_lenient<T: LenientEnum>(value: &json::JsonValue, name: &str) -> Result<T, SettingsJsonError> {
    let v = json::to_serde_value(value)?;
    deserialize_lenient::<T, _>(v).map_err(|_| conversion_error(name, T::TYPE_NAME))
}

fn read_settings_enum<T: LenientEnum>(value: &json::JsonValue, name: &str) -> Result<Option<T>, SettingsJsonError> {
    let v = json::to_serde_value(value)?;
    deserialize_settings_enum::<T, _>(v).map_err(|_| conversion_error(name, T::TYPE_NAME))
}

fn read_flags<T: FlagEnum>(value: &json::JsonValue, name: &str) -> Result<T, SettingsJsonError> {
    let v = json::to_serde_value(value)?;
    deserialize_flags::<T, _>(v).map_err(|_| conversion_error(name, T::TYPE_NAME))
}

/// `List<string>?`: `null` -> `None`; array of strings and `null`s -> `Some`.
fn read_string_list(value: &json::JsonValue, name: &str) -> Result<Option<Vec<Option<String>>>, SettingsJsonError> {
    match value {
        json::JsonValue::Null => Ok(None),
        json::JsonValue::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    json::JsonValue::Null => out.push(None),
                    json::JsonValue::String(Some(s)) => out.push(Some(s.clone())),
                    _ => return Err(conversion_error(name, "String")),
                }
            }
            Ok(Some(out))
        }
        _ => Err(conversion_error(name, "List<String>")),
    }
}

/// `List<ReviewAction>`: returns the actions and the number of dropped `null` elements.
fn read_actions(
    value: &json::JsonValue,
    texts: &dyn SettingsTexts,
) -> Result<(Vec<ReviewAction>, u32), SettingsJsonError> {
    match value {
        json::JsonValue::Null => Ok((
            ReviewAction::defaults(&|n| texts.action_default_name(n), &texts.action_default_backup()),
            0,
        )),
        json::JsonValue::Array(items) => {
            let mut actions = Vec::with_capacity(items.len());
            let mut nulls = 0u32;
            for item in items {
                match item {
                    json::JsonValue::Null => nulls += 1,
                    json::JsonValue::Object(entries) => {
                        let mut action = ReviewAction::new(texts.action_default_name(2));
                        for (entry_name, entry_value) in entries {
                            let Some(entry_name) = entry_name else {
                                return Err(conversion_error("Actions", "ReviewAction"));
                            };
                            match entry_name.as_str() {
                                "Name" => action.name = read_string(entry_value, "Name")?,
                                "Shortcut" => action.shortcut = read_string(entry_value, "Shortcut")?,
                                "Destination" => action.destination = read_string(entry_value, "Destination")?,
                                "Operation" => action.operation = read_lenient(entry_value, "Operation")?,
                                "Confirm" => action.confirm = read_bool(entry_value, "Confirm")?,
                                _ => {}
                            }
                        }
                        actions.push(action);
                    }
                    _ => return Err(conversion_error("Actions", "ReviewAction")),
                }
            }
            Ok((actions, nulls))
        }
        _ => Err(conversion_error("Actions", "List<ReviewAction>")),
    }
}

/// `ShortcutMappings`: `null` -> defaults; object -> defaults overlaid with the named strings.
fn read_shortcuts(value: &json::JsonValue) -> Result<ShortcutMappings, SettingsJsonError> {
    match value {
        json::JsonValue::Null => Ok(ShortcutMappings::default()),
        json::JsonValue::Object(entries) => {
            let mut mappings = ShortcutMappings::default();
            for (entry_name, entry_value) in entries {
                let Some(entry_name) = entry_name else {
                    return Err(conversion_error("Shortcuts", "ShortcutMappings"));
                };
                if ShortcutMappings::PROPERTY_NAMES.contains(&entry_name.as_str()) {
                    let text = read_string(entry_value, entry_name)?;
                    if let Some(slot) = mappings.get_mut(entry_name) {
                        *slot = text;
                    }
                }
            }
            Ok(mappings)
        }
        _ => Err(conversion_error("Shortcuts", "ShortcutMappings")),
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
            match self {
                JsonValue::Object(entries) => entries
                    .iter()
                    .rev()
                    .find(|(n, _)| n.as_deref() == Some(name))
                    .map(|(_, v)| v),
                _ => None,
            }
        }
    }

    fn err(message: &str) -> SettingsJsonError {
        SettingsJsonError::new(message)
    }

    struct Parser<'a> {
        bytes: &'a [u8],
        text: &'a str,
        pos: usize,
    }

    impl<'a> Parser<'a> {
        fn peek(&self) -> Option<u8> {
            self.bytes.get(self.pos).copied()
        }

        /// Skips whitespace only (the four JSON whitespace characters).
        fn skip_plain_ws(&mut self) {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                self.pos += 1;
            }
        }

        /// Skips whitespace and comments.
        fn skip_ws(&mut self) -> Result<(), SettingsJsonError> {
            loop {
                self.skip_plain_ws();
                if self.peek() != Some(b'/') {
                    return Ok(());
                }
                match self.bytes.get(self.pos + 1) {
                    Some(b'/') => {
                        self.pos += 2;
                        loop {
                            let rest = &self.text[self.pos..];
                            let Some(c) = rest.chars().next() else { break };
                            match c {
                                '\n' | '\r' => break,
                                '\u{2028}' | '\u{2029}' => return Err(err("invalid character in a comment")),
                                _ => self.pos += c.len_utf8(),
                            }
                        }
                    }
                    Some(b'*') => {
                        self.pos += 2;
                        loop {
                            match self.peek() {
                                None => return Err(err("unterminated comment")),
                                Some(b'*') if self.bytes.get(self.pos + 1) == Some(&b'/') => {
                                    self.pos += 2;
                                    break;
                                }
                                Some(_) => self.pos += 1,
                            }
                        }
                    }
                    _ => return Err(err("unexpected '/'")),
                }
            }
        }

        fn parse_value(&mut self, depth: usize) -> Result<JsonValue, SettingsJsonError> {
            match self.peek() {
                None => Err(err("unexpected end of data")),
                Some(b'{') => self.parse_object(depth + 1),
                Some(b'[') => self.parse_array(depth + 1),
                Some(b'"') => Ok(JsonValue::String(self.parse_string()?)),
                Some(b't') => self.parse_literal("true", JsonValue::Bool(true)),
                Some(b'f') => self.parse_literal("false", JsonValue::Bool(false)),
                Some(b'n') => self.parse_literal("null", JsonValue::Null),
                Some(b'-' | b'0'..=b'9') => self.parse_number(),
                Some(_) => Err(err("unexpected character")),
            }
        }

        fn at_delimiter(&self) -> bool {
            matches!(
                self.peek(),
                None | Some(b' ' | b'\t' | b'\r' | b'\n' | b',' | b'}' | b']' | b'/')
            )
        }

        fn parse_literal(&mut self, word: &str, value: JsonValue) -> Result<JsonValue, SettingsJsonError> {
            if self.text[self.pos..].starts_with(word) {
                self.pos += word.len();
                if self.at_delimiter() {
                    return Ok(value);
                }
            }
            Err(err("invalid literal"))
        }

        fn parse_number(&mut self) -> Result<JsonValue, SettingsJsonError> {
            let start = self.pos;
            if self.peek() == Some(b'-') {
                self.pos += 1;
            }
            match self.peek() {
                Some(b'0') => self.pos += 1,
                Some(b'1'..=b'9') => {
                    while matches!(self.peek(), Some(b'0'..=b'9')) {
                        self.pos += 1;
                    }
                }
                _ => return Err(err("invalid number")),
            }
            if self.peek() == Some(b'.') {
                self.pos += 1;
                if !matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(err("invalid number"));
                }
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            if matches!(self.peek(), Some(b'e' | b'E')) {
                self.pos += 1;
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.pos += 1;
                }
                if !matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(err("invalid number"));
                }
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            if !self.at_delimiter() {
                return Err(err("invalid character after a number"));
            }
            Ok(JsonValue::Number(self.text[start..self.pos].to_owned()))
        }

        fn parse_hex4(&mut self) -> Result<u32, SettingsJsonError> {
            let mut v = 0u32;
            for _ in 0..4 {
                let d = match self.peek() {
                    Some(c @ b'0'..=b'9') => u32::from(c - b'0'),
                    Some(c @ b'a'..=b'f') => u32::from(c - b'a') + 10,
                    Some(c @ b'A'..=b'F') => u32::from(c - b'A') + 10,
                    _ => return Err(err("invalid \\u escape")),
                };
                v = v * 16 + d;
                self.pos += 1;
            }
            Ok(v)
        }

        /// Parses a string starting at the opening quote. `None` = contains an unpaired surrogate escape.
        fn parse_string(&mut self) -> Result<JsonText, SettingsJsonError> {
            self.pos += 1; // opening quote
            let mut out = String::new();
            let mut unpaired = false;
            loop {
                let rest = &self.text[self.pos..];
                let Some(c) = rest.chars().next() else {
                    return Err(err("unterminated string"));
                };
                match c {
                    '"' => {
                        self.pos += 1;
                        return Ok(if unpaired { None } else { Some(out) });
                    }
                    '\\' => {
                        self.pos += 1;
                        let Some(e) = self.peek() else {
                            return Err(err("unterminated string"));
                        };
                        self.pos += 1;
                        match e {
                            b'"' => out.push('"'),
                            b'\\' => out.push('\\'),
                            b'/' => out.push('/'),
                            b'b' => out.push('\u{8}'),
                            b'f' => out.push('\u{c}'),
                            b'n' => out.push('\n'),
                            b'r' => out.push('\r'),
                            b't' => out.push('\t'),
                            b'u' => {
                                let unit = self.parse_hex4()?;
                                if (0xD800..0xDC00).contains(&unit) {
                                    // A high surrogate pairs only with an immediately following low-surrogate escape.
                                    let mut paired = false;
                                    if self.text[self.pos..].starts_with("\\u") {
                                        let save = self.pos;
                                        self.pos += 2;
                                        match self.parse_hex4() {
                                            Ok(low) if (0xDC00..0xE000).contains(&low) => {
                                                let cp = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                                                if let Some(ch) = char::from_u32(cp) {
                                                    out.push(ch);
                                                    paired = true;
                                                }
                                            }
                                            _ => self.pos = save,
                                        }
                                    }
                                    if !paired {
                                        unpaired = true;
                                    }
                                } else if (0xDC00..0xE000).contains(&unit) {
                                    unpaired = true;
                                } else if let Some(ch) = char::from_u32(unit) {
                                    out.push(ch);
                                }
                            }
                            _ => return Err(err("invalid escape sequence")),
                        }
                    }
                    c if (c as u32) < 0x20 => return Err(err("control character in a string")),
                    c => {
                        out.push(c);
                        self.pos += c.len_utf8();
                    }
                }
            }
        }

        fn parse_array(&mut self, depth: usize) -> Result<JsonValue, SettingsJsonError> {
            if depth > MAX_DEPTH {
                return Err(err("maximum depth exceeded"));
            }
            self.pos += 1; // [
            let mut items = Vec::new();
            self.skip_ws()?;
            if self.peek() == Some(b']') {
                self.pos += 1;
                return Ok(JsonValue::Array(items));
            }
            loop {
                items.push(self.parse_value(depth)?);
                self.skip_ws()?;
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                        self.skip_ws()?;
                        if self.peek() == Some(b']') {
                            self.pos += 1;
                            return Ok(JsonValue::Array(items));
                        }
                    }
                    Some(b']') => {
                        self.pos += 1;
                        return Ok(JsonValue::Array(items));
                    }
                    _ => return Err(err("expected ',' or ']'")),
                }
            }
        }

        fn parse_object(&mut self, depth: usize) -> Result<JsonValue, SettingsJsonError> {
            if depth > MAX_DEPTH {
                return Err(err("maximum depth exceeded"));
            }
            self.pos += 1; // {
            let mut entries = Vec::new();
            self.skip_ws()?;
            if self.peek() == Some(b'}') {
                self.pos += 1;
                return Ok(JsonValue::Object(entries));
            }
            loop {
                if self.peek() != Some(b'"') {
                    return Err(err("expected a property name"));
                }
                let name = self.parse_string()?;
                self.skip_plain_ws();
                if self.peek() != Some(b':') {
                    return Err(err("expected ':'"));
                }
                self.pos += 1;
                self.skip_ws()?;
                let value = self.parse_value(depth)?;
                entries.push((name, value));
                self.skip_ws()?;
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                        self.skip_ws()?;
                        if self.peek() == Some(b'}') {
                            self.pos += 1;
                            return Ok(JsonValue::Object(entries));
                        }
                    }
                    Some(b'}') => {
                        self.pos += 1;
                        return Ok(JsonValue::Object(entries));
                    }
                    _ => return Err(err("expected ',' or '}'")),
                }
            }
        }
    }

    /// Parses `text` with the options `config.json` is read with: standard JSON plus `//` and `/* */` comments
    /// wherever whitespace is allowed (see contract for the exact rules) and one trailing comma in objects and arrays.
    /// `Err` for everything System.Text.Json rejects: see the contract's syntax table (BOM, NBSP, form feed, single
    /// quotes, leading zeros, depth above [`MAX_DEPTH`], text after the root value, empty text...).
    pub fn parse(text: &str) -> Result<JsonValue, SettingsJsonError> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            text,
            pos: 0,
        };
        parser.skip_ws()?;
        let value = parser.parse_value(0)?;
        parser.skip_ws()?;
        if parser.pos != parser.bytes.len() {
            return Err(err("unexpected data after the root value"));
        }
        Ok(value)
    }

    /// Parses `[+-]?digits+` (ASCII) into an `i128`, saturating far outside every supported range.
    fn parse_int_digits(text: &str, allow_plus: bool) -> Option<i128> {
        let (negative, digits) = match text.as_bytes().first()? {
            b'-' => (true, &text[1..]),
            b'+' if allow_plus => (false, &text[1..]),
            _ => (false, text),
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut v: i128 = 0;
        for b in digits.bytes() {
            v = v.saturating_mul(10).saturating_add(i128::from(b - b'0'));
            if v > i128::from(u64::MAX) {
                v = i128::from(u64::MAX) + 1;
            }
        }
        Some(if negative { -v } else { v })
    }

    fn to_integer(value: &JsonValue) -> Option<i128> {
        match value {
            JsonValue::Number(token) => {
                if token.bytes().any(|b| matches!(b, b'.' | b'e' | b'E')) {
                    None
                } else {
                    parse_int_digits(token, false)
                }
            }
            JsonValue::String(Some(s)) => parse_int_digits(s, true),
            _ => None,
        }
    }

    /// `int` property from a JSON value: a number token with only `[-]digits` that fits `i32`, or a string
    /// `[+-]?digits+` (no spaces) that fits. `None` = not convertible (C# `JsonException`).
    pub fn to_i32(value: &JsonValue) -> Option<i32> {
        i32::try_from(to_integer(value)?).ok()
    }

    /// `long` property: as [`to_i32`] with the `i64` range.
    pub fn to_i64(value: &JsonValue) -> Option<i64> {
        i64::try_from(to_integer(value)?).ok()
    }

    /// True when `s` matches `[+-]?(digits[.digits*]|.digits)([eE][+-]?digits)?` (ASCII digits only).
    fn is_decimal_literal(s: &str) -> bool {
        let b = s.as_bytes();
        let mut i = 0;
        if matches!(b.first(), Some(b'+' | b'-')) {
            i += 1;
        }
        let int_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let int_digits = i - int_start;
        if i < b.len() && b[i] == b'.' {
            i += 1;
            let frac_start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if int_digits == 0 && i == frac_start {
                return false;
            }
        } else if int_digits == 0 {
            return false;
        }
        if i < b.len() && matches!(b[i], b'e' | b'E') {
            i += 1;
            if i < b.len() && matches!(b[i], b'+' | b'-') {
                i += 1;
            }
            let exp_start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i == exp_start {
                return false;
            }
        }
        i == b.len()
    }

    /// `double` property: a number token (overflow gives +/- infinity), or a string in the grammar of the contract
    /// (`NaN`, `Infinity`, `-Infinity`, or a decimal literal that must be finite).
    pub fn to_f64(value: &JsonValue) -> Option<f64> {
        match value {
            JsonValue::Number(token) => token.parse::<f64>().ok(),
            JsonValue::String(Some(s)) => match s.as_str() {
                "NaN" => Some(f64::NAN),
                "Infinity" => Some(f64::INFINITY),
                "-Infinity" => Some(f64::NEG_INFINITY),
                _ => {
                    if !is_decimal_literal(s) {
                        return None;
                    }
                    s.parse::<f64>().ok().filter(|v| v.is_finite())
                }
            },
            _ => None,
        }
    }

    /// `bool` property: only the literals `true` / `false` (a string, number or null is not convertible).
    pub fn to_bool(value: &JsonValue) -> Option<bool> {
        match value {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// `string` property: a string value (`Some(Some(text))`), or `null` (`Some(None)`). `None` = not convertible
    /// (number, bool, array, object, or a string with an unpaired surrogate escape).
    pub fn to_string_or_null(value: &JsonValue) -> Option<Option<String>> {
        match value {
            JsonValue::Null => Some(None),
            JsonValue::String(Some(s)) => Some(Some(s.clone())),
            _ => None,
        }
    }

    /// Adapter for the W1-02 helpers (`deserialize_lenient`, `deserialize_settings_enum`, `deserialize_flags`), which
    /// take a serde `Deserializer`. `Err` for a string with an unpaired surrogate (C#: the converter's
    /// `GetString()` throws, surfacing as `JsonException`). Numbers that are integers become `i64`/`u64`; any other
    /// number token becomes `f64` (or `Null` when not finite); arrays and objects become empty ones (their content never matters).
    pub fn to_serde_value(value: &JsonValue) -> Result<serde_json::Value, SettingsJsonError> {
        Ok(match value {
            JsonValue::Null => serde_json::Value::Null,
            JsonValue::Bool(b) => serde_json::Value::Bool(*b),
            JsonValue::Number(token) => {
                let is_integer = !token.bytes().any(|b| matches!(b, b'.' | b'e' | b'E'));
                let float = || {
                    token
                        .parse::<f64>()
                        .ok()
                        .and_then(serde_json::Number::from_f64)
                        .map_or(serde_json::Value::Null, serde_json::Value::Number)
                };
                if is_integer {
                    if let Ok(i) = token.parse::<i64>() {
                        serde_json::Value::Number(i.into())
                    } else if let Ok(u) = token.parse::<u64>() {
                        serde_json::Value::Number(u.into())
                    } else {
                        float()
                    }
                } else {
                    float()
                }
            }
            JsonValue::String(Some(s)) => serde_json::Value::String(s.clone()),
            JsonValue::String(None) => return Err(err("string contains an unpaired surrogate")),
            JsonValue::Array(_) => serde_json::Value::Array(Vec::new()),
            JsonValue::Object(_) => serde_json::Value::Object(serde_json::Map::new()),
        })
    }

    /// The text System.Text.Json writes for a string value with the default encoder, INCLUDING the surrounding quotes.
    /// See the contract's escape table (every non-ASCII char as `\uXXXX` upper-case hex with surrogate pairs;
    /// `"` `&` `'` `+` `<` `>` and backtick as `\u00XX`; `\` as `\\`; `\b \t \n \f \r`; other controls and DEL as `\u00XX`).
    pub fn escape_string(s: &str) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '\u{8}' => out.push_str("\\b"),
                '\t' => out.push_str("\\t"),
                '\n' => out.push_str("\\n"),
                '\u{c}' => out.push_str("\\f"),
                '\r' => out.push_str("\\r"),
                '"' | '&' | '\'' | '+' | '<' | '>' | '`' => {
                    let _ = write!(out, "\\u{:04X}", c as u32);
                }
                c if (c as u32) < 0x20 || c as u32 == 0x7F => {
                    let _ = write!(out, "\\u{:04X}", c as u32);
                }
                c if c.is_ascii() => out.push(c),
                c => {
                    let mut buf = [0u16; 2];
                    for unit in c.encode_utf16(&mut buf) {
                        let _ = write!(out, "\\u{:04X}", unit);
                    }
                }
            }
        }
        out.push('"');
        out
    }

    /// The text `Utf8JsonWriter.WriteNumberValue(double)` writes: the shortest digits that round-trip, plain notation
    /// when the decimal exponent is between -5 (exclusive) and 17 (exclusive), otherwise `d[.ddd]E+XX` / `E-XX`
    /// (sign always present, at least two exponent digits). `-0.0` is `-0`. `None` for NaN and the infinities.
    pub fn format_f64(v: f64) -> Option<String> {
        if !v.is_finite() {
            return None;
        }
        let shortest = format!("{v:e}");
        // The shortest form rounds an exact decimal tie half up; .NET rounds half to even.
        // Re-format the exact value at the same digit count (exact formatting is half-even).
        let digit_count = shortest
            .split_once('e')
            .map(|(m, _)| m.chars().filter(char::is_ascii_digit).count())
            .unwrap_or(1);
        let sci = format!("{:.*e}", digit_count.saturating_sub(1), v);
        let sci = if sci.parse::<f64>().ok() == Some(v) {
            sci
        } else {
            shortest
        };
        let (mantissa, exp) = sci.split_once('e')?;
        let exponent: i32 = exp.parse().ok()?;
        let (negative, mantissa) = match mantissa.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, mantissa),
        };
        let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
        let mut out = String::new();
        if negative {
            out.push('-');
        }
        if exponent > -5 && exponent < 17 {
            if exponent >= 0 {
                let int_len = exponent as usize + 1;
                if digits.len() <= int_len {
                    out.push_str(&digits);
                    out.extend(std::iter::repeat_n('0', int_len - digits.len()));
                } else {
                    out.push_str(&digits[..int_len]);
                    out.push('.');
                    out.push_str(&digits[int_len..]);
                }
            } else {
                out.push_str("0.");
                out.extend(std::iter::repeat_n('0', (-exponent - 1) as usize));
                out.push_str(&digits);
            }
        } else {
            out.push_str(&digits[..1]);
            if digits.len() > 1 {
                out.push('.');
                out.push_str(&digits[1..]);
            }
            out.push('E');
            out.push(if exponent < 0 { '-' } else { '+' });
            out.push_str(&format!("{:02}", exponent.abs()));
        }
        Some(out)
    }
}
