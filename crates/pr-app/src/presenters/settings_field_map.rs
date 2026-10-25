//! Port of `PhotoReview.App.Shared/Presenters/SettingsFieldMap.cs` (baseline `d92d6aae`, 411 lines).
//!
//! The single field table of the Settings dialog: every `AppSettings` property that has a control, in both directions
//! (`bindings()`), the shortcut boxes, and the index <-> value tables of the combo boxes. The order of `bindings()` is the
//! order Save applies and validates in. Contract: `contracts/W8-10b.md` (semantics notes 1 to 14 and
//! traps T1 to T16 are cited by number below).
//!
//! The field table and pure conversions used by the Settings presenter.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use pr_core::model::enums::*;
use pr_core::model::serde_ext::LenientEnum;
use pr_core::settings::app_settings::{self, AppSettings};
use pr_core::settings::context_menu;
use pr_core::settings::shortcuts::{self, ShortcutMappings};
use pr_l10n::localizer::Localizer;
use pr_l10n::template::NumberCulture;

use crate::presenters::settings_form_state::{SettingsFieldError, SettingsFocusField, SettingsFormState};

/// Services the apply functions need. Messages come from the localizer (D-W8-4); the C# `Tr` accessors read the ambient
/// localizer, the Rust port passes it explicitly. The number culture formats the integer bounds (`{min}`, `{max}`).
#[derive(Debug, Clone, Copy)]
pub struct FieldContext<'a> {
    pub texts: &'a Localizer,
    pub numbers: &'a NumberCulture,
}

/// Settings to form (C# `Load`): never fails, never refuses. Non-capturing closures coerce to this pointer.
pub type LoadFn = fn(&AppSettings, &mut SettingsFormState);

/// Form to settings (C# `Apply`): `None` is accepted; `Some` is the refused input, which stops the pass (see `apply_all`
/// semantics in the contract). The rows before a refused one have already written their values into `settings`.
pub type ApplyFn = fn(&SettingsFormState, &mut AppSettings, &FieldContext<'_>) -> Option<SettingsFieldError>;

/// One row of the table (C# `SettingsFieldBinding`): the `AppSettings` property names it owns (C# names, PascalCase, in
/// the order the C# row lists them), and both directions.
#[derive(Debug, Clone, Copy)]
pub struct SettingsFieldBinding {
    pub properties: &'static [&'static str],
    pub load: LoadFn,
    pub apply: ApplyFn,
}

/// All rows, in the order Save applies them (47 rows, 59 properties). The porter builds it once (`OnceLock`) from
/// non-capturing closures and private fns; the row order and the property lists are in the contract, section "Table".
pub fn bindings() -> &'static [SettingsFieldBinding] {
    static ROWS: OnceLock<Vec<SettingsFieldBinding>> = OnceLock::new();
    ROWS.get_or_init(build_bindings)
}

/// `CoveredProperties`: the union of every row's properties plus `UiLanguage` (59 names, ordinal sorted by `BTreeSet`).
pub fn covered_properties() -> BTreeSet<&'static str> {
    bindings()
        .iter()
        .flat_map(|row| row.properties.iter().copied())
        .chain(["UiLanguage"])
        .collect()
}

/// `NotEditedInDialog`: the settable properties the dialog has no control for, with the reason, in the C# insertion order
/// (11 entries). Every reason is non-blank.
pub fn not_edited_in_dialog() -> &'static [(&'static str, &'static str)] {
    &[
        (
            "ConfigVersion",
            "Fixed to AppSettings.CurrentConfigVersion by the type itself; never shown or editable.",
        ),
        (
            "ImageCacheCapacityBytes",
            "Performance internal: hand-edit config.json only (Defaults resets it).",
        ),
        (
            "MemoryReserveBytes",
            "Performance internal: hand-edit config.json only (Defaults resets it).",
        ),
        (
            "PreloadWorkerCount",
            "Performance internal: hand-edit config.json only (Defaults resets it).",
        ),
        (
            "PreloadMemoryLoadLimit",
            "Performance internal: hand-edit config.json only (Defaults resets it).",
        ),
        (
            "PreviewDiskCacheCapacityBytes",
            "Performance internal: hand-edit config.json only (Defaults resets it).",
        ),
        (
            "UseSourceBytesCache",
            "Performance internal: hand-edit config.json only (Defaults resets it).",
        ),
        (
            "SourceBytesCapacityBytes",
            "Performance internal: hand-edit config.json only (Defaults resets it).",
        ),
        (
            "LastMoveToFolder",
            "Internal last-used-folder state written by Move-to; not a user-facing setting.",
        ),
        (
            "LastCopyToFolder",
            "Internal last-used-folder state written by Copy-to; not a user-facing setting.",
        ),
        (
            "SetZoomAlsoSetsClickLevel",
            "Defaults resets it but the dialog has no control (pre-existing gap, kept as is).",
        ),
    ]
}

/// The 29 shortcut property names with a text box, in the order of the Shortcuts page (C# `Shortcuts` list). `MoveToFolder2`
/// is not in it (owned by ReviewAction). Same names as `ShortcutMappings::PROPERTY_NAMES` minus `MoveToFolder2`.
pub fn shortcut_properties() -> &'static [&'static str] {
    &[
        "Next",
        "Previous",
        "FirstImage",
        "LastImage",
        "NextFolder",
        "PreviousFolder",
        "ZoomIn",
        "ZoomOut",
        "ZoomActualSize",
        "ToggleFit",
        "Fullscreen",
        "ToggleInfoOverlay",
        "Skip",
        "Undo",
        "Compare",
        "MoveToFolder",
        "CopyToFolder",
        "SendToRecycleBin",
        "ClickZoom",
        "FitWidth",
        "FitHeight",
        "ToggleKeepZoom",
        "OpenFolder",
        "CustomZoom",
        "ToggleCaptureMember",
        "FitWidth2",
        "Refresh",
        "OpenInDefaultEditor",
        "RenameFile",
    ]
}

/// C# `ShortcutAccessor.Get` by property name. Unknown names return `""` (never reached: the table is fixed; trap T9).
pub fn shortcut_get<'a>(shortcuts: &'a ShortcutMappings, property: &str) -> &'a str {
    shortcuts.get(property).unwrap_or("")
}

/// C# `ShortcutAccessor.Set` by property name. Unknown names are ignored (never reached: trap T9).
pub fn shortcut_set(shortcuts: &mut ShortcutMappings, property: &str, value: String) {
    if let Some(slot) = shortcuts.get_mut(property) {
        *slot = value;
    }
}

/// Settings -> text boxes (C# `LoadShortcuts`): one entry per `shortcut_properties()` name, the value as stored.
pub fn load_shortcuts(source: &ShortcutMappings, state: &mut SettingsFormState) {
    for property in shortcut_properties() {
        state
            .shortcut_texts
            .insert((*property).to_owned(), shortcut_get(source, property).to_owned());
    }
}

/// C# `ShortcutText`: the typed text of a box, `""` when the box has no entry.
pub fn shortcut_text<'a>(state: &'a SettingsFormState, property: &str) -> &'a str {
    state.shortcut_texts.get(property).map(String::as_str).unwrap_or("")
}

/// Text boxes as typed, NOT canonicalized (C# `ReadShortcuts`). `MoveToFolder2` keeps the `ShortcutMappings` default.
pub fn read_shortcuts(state: &SettingsFormState) -> ShortcutMappings {
    let mut result = ShortcutMappings::default();
    for property in shortcut_properties() {
        shortcut_set(&mut result, property, shortcut_text(state, property).to_owned());
    }
    result
}

/// What Save persists (C# `CanonicalizeShortcuts`): every box through `pr_core::settings::shortcuts::canonicalize`;
/// `MoveToFolder2` is copied from `existing` (trap T9).
pub fn canonicalize_shortcuts(raw: &ShortcutMappings, existing: &ShortcutMappings) -> ShortcutMappings {
    let mut result = ShortcutMappings::default();
    for property in shortcut_properties() {
        shortcut_set(
            &mut result,
            property,
            shortcuts::canonicalize(shortcut_get(raw, property).into()),
        );
    }
    result.move_to_folder2 = existing.move_to_folder2.clone();
    result
}

/// `AnchorToIndex`: the Fit-width combos list `FitWidthAnchor` in enum order, so index == value (semantics note 1).
/// The Rust enum is always a defined member, so the C# `Enum.IsDefined` fallback has no input here (trap T6).
pub fn anchor_to_index(anchor: FitWidthAnchor) -> i32 {
    anchor as i32
}

/// `IndexToAnchor`: the member whose value is `index`; any other index (including negative) gives `fallback` (trap T6).
pub fn index_to_anchor(index: i32, fallback: FitWidthAnchor) -> FitWidthAnchor {
    match index {
        0 => FitWidthAnchor::Centre,
        1 => FitWidthAnchor::TopThird,
        2 => FitWidthAnchor::BottomThird,
        _ => fallback,
    }
}

/// `ViewModeToIndex` (semantics note 2): Fit 0, FitWidth 1, FitHeight 2, ClickZoomLevel 3, Percent100 4,
/// Percent200 and Percent400 5 (trap T17).
pub fn view_mode_to_index(mode: InitialViewMode) -> i32 {
    match mode {
        InitialViewMode::Fit => 0,
        InitialViewMode::FitWidth => 1,
        InitialViewMode::FitHeight => 2,
        InitialViewMode::ClickZoomLevel => 3,
        InitialViewMode::Percent100 => 4,
        InitialViewMode::Percent200 | InitialViewMode::Percent400 => 5,
    }
}

/// `IndexToViewMode`: 1 to 5 as in `view_mode_to_index` (Percent200 for 5); every other index gives `Fit` (trap T17).
pub fn index_to_view_mode(index: i32) -> InitialViewMode {
    match index {
        1 => InitialViewMode::FitWidth,
        2 => InitialViewMode::FitHeight,
        3 => InitialViewMode::ClickZoomLevel,
        4 => InitialViewMode::Percent100,
        5 => InitialViewMode::Percent200,
        _ => InitialViewMode::Fit,
    }
}

/// C# `TryInt(text, min, max)`: `int.TryParse(text, NumberStyles.Integer, Invariant)` and `min <= value <= max`.
/// Accepted text: optional whitespace (only U+0009 to U+000D and U+0020, trap T3), one optional `+` or `-`, one or more
/// ASCII digits, optional whitespace. Overflow of `i32` is a refusal. Returns `None` on any refusal.
pub fn try_int(text: &str, min: i32, max: i32) -> Option<i32> {
    let text = text.trim_matches(|c: char| matches!(c, '\u{9}'..='\u{d}' | ' '));
    let value = text.parse::<i32>().ok()?;
    (min <= value && value <= max).then_some(value)
}

/// `double.TryParse(text, NumberStyles.Float, Invariant)` (trap T4): the same whitespace set as `try_int`, an optional sign,
/// digits with an optional decimal point, an optional exponent, and the invariant symbols `NaN` and `Infinity` (returned as
/// `Some(NaN)` / `Some(inf)`; the caller refuses non-finite values). `None` when .NET would return false.
pub fn try_double(text: &str) -> Option<f64> {
    let text = text.trim_matches(|c: char| matches!(c, '\u{9}'..='\u{d}' | ' '));
    match text {
        "NaN" | "+NaN" | "-NaN" => Some(f64::NAN),
        "Infinity" | "+Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        t if t.eq_ignore_ascii_case("infinity") || t.eq_ignore_ascii_case("+infinity") => Some(f64::INFINITY),
        t if t.eq_ignore_ascii_case("-infinity") => Some(f64::NEG_INFINITY),
        _ => {
            if text.eq_ignore_ascii_case("inf") || text.eq_ignore_ascii_case("infinity") || text.contains(',') {
                None
            } else {
                text.parse::<f64>().ok()
            }
        }
    }
}

/// C# `double.ToString(CultureInfo.InvariantCulture)` as used for `InfoOverlayFontSizeText` (trap T13): shortest
/// round-trip digits, fixed notation in the normal range, `E+xx`/`E-xx` outside it. The exact thresholds are golden-checked.
pub fn dotnet_double_text(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == f64::INFINITY {
        return "Infinity".to_owned();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let text = value.to_string();
    if let Some((mantissa, exponent)) = text.split_once('e') {
        let exponent = exponent.parse::<i32>().unwrap_or(0);
        if !(-4..17).contains(&exponent) {
            return format!("{mantissa}E{exponent:+03}");
        }
    } else if value.abs() < 1.0e-4 || value.abs() >= 1.0e17 {
        let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
        let digits = format!("{whole}{frac}");
        let first = digits.trim_start_matches('0').chars().next().unwrap_or('0');
        let exp = if whole == "0" {
            -(frac.chars().take_while(|c| *c == '0').count() as i32) - 1
        } else {
            whole.len() as i32 - 1
        };
        return format!("{first}E{exp:+03}");
    }
    text
}

/// .NET default `Double.ToString(InvariantCulture)` used by the InfoOverlayFontSize load row.
/// Keep this distinct from `dotnet_double_text`, whose R-format golden fixes another threshold.
fn dotnet_load_double_text(value: f64) -> String {
    if value.is_finite() && value.abs() >= 1.0e17 {
        let r_text = value.to_string();
        if let Some((mantissa, exponent)) = r_text.split_once('e') {
            let exponent = exponent.parse::<i32>().unwrap_or(0);
            return format!("{mantissa}E{exponent:+03}");
        }
        let (whole, fraction) = r_text.split_once('.').unwrap_or((&r_text, ""));
        let digits = format!("{whole}{fraction}");
        let first = digits.trim_start_matches('-').chars().next().unwrap_or('0');
        let exponent = whole.trim_start_matches('-').len() as i32 - 1;
        return format!("{first}E{exponent:+03}");
    }
    dotnet_double_text(value)
}

fn build_bindings() -> Vec<SettingsFieldBinding> {
    let mut rows = Vec::with_capacity(47);
    macro_rules! push {
        ($load:ident,$apply:ident,$props:expr) => {
            rows.push(SettingsFieldBinding {
                properties: $props,
                load: $load,
                apply: $apply,
            });
        };
    }
    push!(load_view_mode, apply_view_mode, &["InitialViewMode"]);
    push!(load_anchor, apply_anchor, &["FitWidthAnchor"]);
    push!(load_anchor2, apply_anchor2, &["FitWidthAnchor2"]);
    push!(load_middle, apply_middle, &["MiddleClickAction"]);
    push!(load_keep_zoom, apply_keep_zoom, &["KeepZoomAcrossImages"]);
    push!(load_sort, apply_sort, &["ImageSortMode"]);
    push!(load_scaling, apply_scaling, &["ScalingQuality"]);
    push!(load_decoder, apply_decoder, &["DecoderBackend"]);
    push!(load_loading, apply_loading, &["LoadingMode"]);
    push!(load_instance, apply_instance, &["InstanceMode"]);
    push!(load_compare_hash, apply_compare_hash, &["CompareHashEnabled"]);
    push!(load_compare_size, apply_compare_size, &["CompareSizeEnabled"]);
    push!(load_raw_support, apply_raw_support, &["RawSupportEnabled"]);
    push!(load_webp, apply_webp, &["WebpHeicSupportEnabled"]);
    push!(load_raw_decode, apply_raw_decode, &["RawFullDecode"]);
    push!(load_raw_pair, apply_raw_pair, &["RawPairMode"]);
    push!(load_logging, apply_logging, &["LoggingEnabled"]);
    push!(load_delete, apply_delete, &["AllowPermanentDeleteWithoutRecycleBin"]);
    push!(load_confirm, apply_confirm, &["ConfirmBeforeDelete"]);
    push!(
        load_editor,
        apply_editor,
        &[
            "ExternalEditors",
            "DefaultExternalEditorId",
            "ExternalEditorSendsRawOriginal",
            "ExternalEditorPath",
        ]
    );
    push!(load_journal, apply_journal, &["JournalDurability"]);
    push!(load_ram, apply_ram, &["ImageCacheRamPercent"]);
    push!(load_show_overlay, apply_show_overlay, &["ShowInfoOverlay"]);
    push!(load_file_info, apply_file_info, &["ShowFileInfo"]);
    push!(load_folder_info, apply_folder_info, &["ShowFolderInfo"]);
    push!(load_wheel, apply_wheel, &["MouseWheelAction"]);
    push!(load_click_enabled, apply_click_enabled, &["ClickToZoomEnabled"]);
    push!(load_kinetic, apply_kinetic, &["KineticPanEnabled"]);
    push!(load_glide, apply_glide, &["KineticGlideSmoothing"]);
    push!(load_arrow_nav, apply_arrow_nav, &["ArrowKeyNavigatesAtZoomEdge"]);
    rows.push(SettingsFieldBinding {
        properties: &[
            "HiddenContextMenuItems",
            "ShowFolderMenuItems",
            "ShowZoomMenuItems",
            "ShowRecycleMenuItem",
        ],
        load: load_context,
        apply: apply_context,
    });
    push!(load_toggle_fit, apply_toggle_fit, &["ClickZoomKeyTogglesFit"]);
    push!(load_keyboard_anchor, apply_keyboard_anchor, &["KeyboardZoomAnchor"]);
    push!(load_reuse_folder, apply_reuse_folder, &["MoveCopyReuseLastFolder"]);
    push!(load_show_exif, apply_show_exif, &["ShowExifInfo"]);
    push!(load_exif_flags, apply_exif_flags, &["ExifInfoFields"]);
    push!(load_title_flags, apply_title_flags, &["TitleBarFields"]);
    push!(load_click_percent, apply_click_percent, &["ClickZoomPercent"]);
    push!(load_pan_step, apply_pan_step, &["ArrowPanStepPercent"]);
    push!(load_zoom_step, apply_zoom_step, &["KeyboardZoomStepPercent"]);
    rows.push(transition_row());
    rows.push(touchpad_row());
    rows.push(preload_row());
    rows.push(overlay_row());
    push!(load_zoom_indicator, apply_zoom_indicator, &["ShowZoomIndicator"]);
    push!(load_shortcuts_row, apply_shortcuts, &["Shortcuts"]);
    push!(load_actions, apply_actions, &["Actions"]);
    rows
}

fn load_context(s: &AppSettings, f: &mut SettingsFormState) {
    let hidden = context_menu::effective_hidden(s);
    f.context_menu_shown.clear();
    for i in &context_menu::ITEMS {
        if i.locked || !hidden.contains(&i.id) {
            f.context_menu_shown.insert(i.id);
        }
    }
}
fn err(c: &FieldContext<'_>, key: &str, args: &[(&str, i32)], focus: SettingsFocusField) -> SettingsFieldError {
    SettingsFieldError::new(
        c.texts.format_with(
            key,
            &args
                .iter()
                .map(|(n, v)| pr_l10n::template::LocArg::new(*n, *v))
                .collect::<Vec<_>>(),
            c.numbers,
        ),
        focus,
    )
}

fn transition_row() -> SettingsFieldBinding {
    SettingsFieldBinding {
        properties: &["ImageTransition", "ImageTransitionMs"],
        load: load_transition,
        apply: apply_transition,
    }
}
fn touchpad_row() -> SettingsFieldBinding {
    SettingsFieldBinding {
        properties: &["TouchpadSwipeEnabled", "TouchpadSwipeDistancePerImage"],
        load: load_touchpad,
        apply: apply_touchpad,
    }
}
fn preload_row() -> SettingsFieldBinding {
    SettingsFieldBinding {
        properties: &["PreloadForwardCount", "PreloadBackwardCount"],
        load: load_preload,
        apply: apply_preload,
    }
}
fn overlay_row() -> SettingsFieldBinding {
    SettingsFieldBinding {
        properties: &[
            "ToolbarAutoHide",
            "ToolbarAutoHideDelayMs",
            "InfoOverlayAutoHide",
            "InfoOverlayAutoHideDelayMs",
            "ToolbarOpacityPercent",
            "InfoOverlayFontSize",
        ],
        load: load_overlay,
        apply: apply_overlay,
    }
}
fn load_transition(s: &AppSettings, f: &mut SettingsFormState) {
    f.image_transition_index = (s.image_transition == ImageTransition::Fade) as i32;
    f.image_transition_ms_text = s.image_transition_ms.to_string()
}
fn apply_transition(f: &SettingsFormState, s: &mut AppSettings, c: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.image_transition = if f.image_transition_index == 1 {
        ImageTransition::Fade
    } else {
        ImageTransition::None
    };
    if let Some(v) = try_int(&f.image_transition_ms_text, 40, 400) {
        s.image_transition_ms = v;
        None
    } else {
        Some(err(
            c,
            "dialog.settingsInvalid.imageTransitionMs",
            &[("min", 40), ("max", 400)],
            SettingsFocusField::ImageTransitionMs,
        ))
    }
}
fn load_touchpad(s: &AppSettings, f: &mut SettingsFormState) {
    f.touchpad_swipe_enabled = s.touchpad_swipe_enabled;
    f.touchpad_swipe_distance_text = s.touchpad_swipe_distance_per_image.to_string()
}
fn apply_touchpad(f: &SettingsFormState, s: &mut AppSettings, c: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.touchpad_swipe_enabled = f.touchpad_swipe_enabled;
    if let Some(v) = try_int(&f.touchpad_swipe_distance_text, 30, 2400) {
        s.touchpad_swipe_distance_per_image = v;
        None
    } else {
        Some(err(
            c,
            "dialog.settingsInvalid.touchpadSwipeDistance",
            &[("min", 30), ("max", 2400)],
            SettingsFocusField::TouchpadSwipeDistance,
        ))
    }
}
fn load_preload(s: &AppSettings, f: &mut SettingsFormState) {
    f.preload_forward_text = s.preload_forward_count.to_string();
    f.preload_backward_text = s.preload_backward_count.to_string()
}
fn apply_preload(f: &SettingsFormState, s: &mut AppSettings, c: &FieldContext<'_>) -> Option<SettingsFieldError> {
    let Some(a) = try_int(&f.preload_forward_text, 1, 500) else {
        return Some(err(
            c,
            "dialog.settingsInvalid.preloadForward",
            &[("min", 1), ("max", 500)],
            SettingsFocusField::PreloadForward,
        ));
    };
    let Some(b) = try_int(&f.preload_backward_text, 0, 500) else {
        return Some(err(
            c,
            "dialog.settingsInvalid.preloadBackward",
            &[("min", 0), ("max", 500)],
            SettingsFocusField::PreloadBackward,
        ));
    };
    s.preload_forward_count = a;
    s.preload_backward_count = b;
    None
}
fn load_overlay(s: &AppSettings, f: &mut SettingsFormState) {
    f.toolbar_auto_hide = s.toolbar_auto_hide;
    f.toolbar_auto_hide_delay_text = s.toolbar_auto_hide_delay_ms.to_string();
    f.info_overlay_auto_hide = s.info_overlay_auto_hide;
    f.info_overlay_auto_hide_delay_text = s.info_overlay_auto_hide_delay_ms.to_string();
    f.toolbar_opacity_value = s.toolbar_opacity_percent.clamp(20, 100) as f64;
    f.info_overlay_font_size_text = dotnet_load_double_text(s.info_overlay_font_size)
}
fn apply_overlay(f: &SettingsFormState, s: &mut AppSettings, c: &FieldContext<'_>) -> Option<SettingsFieldError> {
    let Some(td) = try_int(&f.toolbar_auto_hide_delay_text, 0, 10000) else {
        return Some(err(
            c,
            "dialog.settingsInvalid.toolbarAutoHideDelay",
            &[("min", 0), ("max", 10000)],
            SettingsFocusField::None,
        ));
    };
    let Some(od) = try_int(&f.info_overlay_auto_hide_delay_text, 0, 10000) else {
        return Some(err(
            c,
            "dialog.settingsInvalid.infoOverlayAutoHideDelay",
            &[("min", 0), ("max", 10000)],
            SettingsFocusField::InfoOverlayAutoHideDelay,
        ));
    };
    let Some(font) = try_double(&f.info_overlay_font_size_text).filter(|v| v.is_finite() && (8.0..=24.0).contains(v))
    else {
        return Some(err(
            c,
            "dialog.settingsInvalid.infoOverlayFontSize",
            &[("min", 8), ("max", 24)],
            SettingsFocusField::None,
        ));
    };
    s.toolbar_auto_hide = f.toolbar_auto_hide;
    s.toolbar_auto_hide_delay_ms = td;
    s.info_overlay_auto_hide = f.info_overlay_auto_hide;
    s.info_overlay_auto_hide_delay_ms = od;
    s.toolbar_opacity_percent = (f.toolbar_opacity_value.round_ties_even() as i32).clamp(20, 100);
    s.info_overlay_font_size = font;
    None
}
fn load_shortcuts_row(s: &AppSettings, f: &mut SettingsFormState) {
    load_shortcuts(&s.shortcuts, f)
}
fn apply_shortcuts(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.shortcuts = canonicalize_shortcuts(&read_shortcuts(f), &s.shortcuts);
    None
}
fn load_actions(s: &AppSettings, f: &mut SettingsFormState) {
    f.actions_json = serialize_actions(&s.actions)
}
fn json_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn serialize_actions(actions: &[pr_core::settings::shortcuts::ReviewAction]) -> String {
    if actions.is_empty() {
        return "[]".to_owned();
    }
    let mut out = String::from("[");
    for (i, a) in actions.iter().enumerate() {
        if i > 0 {
            out.push(',')
        }
        out.push_str("\r\n  {\r\n");
        out.push_str(&format!("    \"Name\": {},\r\n    \"Shortcut\": {},\r\n    \"Operation\": {},\r\n    \"Destination\": {},\r\n    \"Confirm\": {}\r\n  }}",json_quote(&a.name),json_quote(&a.shortcut),json_quote(match a.operation{FileOperationType::Move=>"Move",FileOperationType::Copy=>"Copy",FileOperationType::Recycle=>"Recycle"}),json_quote(&a.destination),a.confirm));
    }
    out.push_str("\r\n]");
    out
}
fn apply_actions(f: &SettingsFormState, s: &mut AppSettings, c: &FieldContext<'_>) -> Option<SettingsFieldError> {
    use app_settings::json::JsonValue as J;
    let parsed = app_settings::json::parse(&f.actions_json).ok().and_then(|v| match v {
        J::Null => Some(Vec::new()),
        J::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                let J::Object(entries) = item else { return None };
                let mut a = pr_core::settings::shortcuts::ReviewAction::new("Action");
                for (k, v) in entries {
                    match k.as_deref()? {
                        "Name" => {
                            if let J::String(Some(x)) = v {
                                a.name = x
                            } else {
                                return None;
                            }
                        }
                        "Shortcut" => {
                            if let J::String(Some(x)) = v {
                                a.shortcut = x
                            } else {
                                return None;
                            }
                        }
                        "Destination" => {
                            if let J::String(Some(x)) = v {
                                a.destination = x
                            } else {
                                return None;
                            }
                        }
                        "Confirm" => {
                            if let J::Bool(x) = v {
                                a.confirm = x
                            } else {
                                return None;
                            }
                        }
                        "Operation" => {
                            // C# `LenientEnumConverter`: a name or alias, or the defined numbers 0/1/2.
                            a.operation = match v {
                                J::String(Some(x)) => match x.as_str() {
                                    "Move" => FileOperationType::Move,
                                    "Copy" => FileOperationType::Copy,
                                    "Recycle" | "Delete" => FileOperationType::Recycle,
                                    _ => return None,
                                },
                                J::Number(n) => match n.as_str() {
                                    "0" => FileOperationType::Move,
                                    "1" => FileOperationType::Copy,
                                    "2" => FileOperationType::Recycle,
                                    _ => return None,
                                },
                                _ => return None,
                            }
                        }
                        _ => {}
                    }
                }
                out.push(a)
            }
            Some(out)
        }
        _ => None,
    });
    if let Some(actions) = parsed {
        s.actions = actions;
        let mut seen = BTreeSet::new();
        for a in &s.actions {
            let key = shortcuts::canonicalize(Some(&a.shortcut));
            if a.name.trim().is_empty()
                || key.is_empty()
                || !ShortcutMappings::PROPERTY_NAMES
                    .iter()
                    .any(|name| shortcuts::canonicalize(Some(name)) == key || key.eq_ignore_ascii_case(name))
                    && key.len() > 128
                || !seen.insert(key.to_ascii_lowercase())
            {
                return Some(err(
                    c,
                    "dialog.settingsInvalid.actionsJson",
                    &[],
                    SettingsFocusField::None,
                ));
            }
        }
        None
    } else {
        Some(err(
            c,
            "dialog.settingsInvalid.actionsJson",
            &[],
            SettingsFocusField::None,
        ))
    }
}

fn load_view_mode(s: &AppSettings, f: &mut SettingsFormState) {
    f.view_mode_index = view_mode_to_index(s.initial_view_mode)
}
fn apply_view_mode(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.initial_view_mode = index_to_view_mode(f.view_mode_index);
    None
}
fn load_anchor(s: &AppSettings, f: &mut SettingsFormState) {
    f.fit_width_anchor_index = anchor_to_index(s.fit_width_anchor)
}
fn apply_anchor(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.fit_width_anchor = index_to_anchor(f.fit_width_anchor_index, FitWidthAnchor::Centre);
    None
}
fn load_anchor2(s: &AppSettings, f: &mut SettingsFormState) {
    f.fit_width_anchor2_index = s.fit_width_anchor2.map(anchor_to_index).unwrap_or(2)
}
fn apply_anchor2(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.fit_width_anchor2 = Some(index_to_anchor(f.fit_width_anchor2_index, FitWidthAnchor::BottomThird));
    None
}
fn load_middle(s: &AppSettings, f: &mut SettingsFormState) {
    f.middle_click_action_index = s.middle_click_action.map(|v| v as i32).unwrap_or(2)
}
fn apply_middle(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.middle_click_action = Some(match f.middle_click_action_index {
        0 => MiddleClickAction::None,
        1 => MiddleClickAction::ClickZoom,
        2 => MiddleClickAction::ActualSize,
        3 => MiddleClickAction::Fit,
        4 => MiddleClickAction::FitWidth,
        5 => MiddleClickAction::FitWidth2,
        _ => MiddleClickAction::ActualSize,
    });
    None
}
fn load_sort(s: &AppSettings, f: &mut SettingsFormState) {
    f.sort_mode_tag = s.image_sort_mode.map(|v| v.name().to_owned())
}
fn apply_sort(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.image_sort_mode = Some(match f.sort_mode_tag.as_deref().map(str::trim) {
        Some("Name") => ImageSortMode::Name,
        Some("SizeDescending") => ImageSortMode::SizeDescending,
        Some("SizeAscending") => ImageSortMode::SizeAscending,
        Some("Default") => ImageSortMode::Default,
        Some("NameAscending") => ImageSortMode::NameAscending,
        Some("NameDescending") => ImageSortMode::NameDescending,
        Some("0") => ImageSortMode::Name,
        Some("1") => ImageSortMode::SizeDescending,
        Some("2") => ImageSortMode::SizeAscending,
        Some("3") => ImageSortMode::Default,
        Some("4") => ImageSortMode::NameAscending,
        Some("5") => ImageSortMode::NameDescending,
        _ => ImageSortMode::Name,
    });
    None
}
fn load_scaling(s: &AppSettings, f: &mut SettingsFormState) {
    f.scaling_quality_index = if s.scaling_quality == Some(ScalingQuality::Linear) {
        1
    } else {
        0
    }
}
fn apply_scaling(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.scaling_quality = Some(if f.scaling_quality_index == 1 {
        ScalingQuality::Linear
    } else {
        ScalingQuality::HighQuality
    });
    None
}
fn load_decoder(s: &AppSettings, f: &mut SettingsFormState) {
    f.decoder_backend_index = match s.decoder_backend {
        Some(DecoderBackend::WicDirect) => 1,
        Some(DecoderBackend::TurboJpeg) => 2,
        _ => 0,
    }
}
fn apply_decoder(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.decoder_backend = Some(match f.decoder_backend_index {
        1 => DecoderBackend::WicDirect,
        2 => DecoderBackend::TurboJpeg,
        _ => DecoderBackend::Wpf,
    });
    None
}
fn load_loading(s: &AppSettings, f: &mut SettingsFormState) {
    f.loading_mode_index = match s.loading_mode {
        Some(LoadingMode::Preview) => 1,
        Some(LoadingMode::Original) => 2,
        _ => 0,
    }
}
fn apply_loading(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.loading_mode = Some(match f.loading_mode_index {
        1 => LoadingMode::Preview,
        2 => LoadingMode::Original,
        _ => LoadingMode::Fast,
    });
    None
}
fn load_instance(s: &AppSettings, f: &mut SettingsFormState) {
    f.instance_mode_index = (s.instance_mode == InstanceMode::PerFolder) as i32
}
fn apply_instance(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.instance_mode = if f.instance_mode_index == 1 {
        InstanceMode::PerFolder
    } else {
        InstanceMode::SingleWindow
    };
    None
}
macro_rules! simple_row {
    ($load:ident,$apply:ident,$field:ident) => {
        fn $load(s: &AppSettings, f: &mut SettingsFormState) {
            f.$field = s.$field
        }
        fn $apply(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
            s.$field = f.$field;
            None
        }
    };
}
simple_row!(load_keep_zoom, apply_keep_zoom, keep_zoom_across_images);
simple_row!(load_compare_hash, apply_compare_hash, compare_hash_enabled);
simple_row!(load_compare_size, apply_compare_size, compare_size_enabled);
simple_row!(load_raw_support, apply_raw_support, raw_support_enabled);
simple_row!(load_webp, apply_webp, webp_heic_support_enabled);
simple_row!(load_logging, apply_logging, logging_enabled);
simple_row!(load_confirm, apply_confirm, confirm_before_delete);
simple_row!(load_show_overlay, apply_show_overlay, show_info_overlay);
simple_row!(load_file_info, apply_file_info, show_file_info);
simple_row!(load_folder_info, apply_folder_info, show_folder_info);
simple_row!(load_click_enabled, apply_click_enabled, click_to_zoom_enabled);
simple_row!(load_kinetic, apply_kinetic, kinetic_pan_enabled);
simple_row!(load_arrow_nav, apply_arrow_nav, arrow_key_navigates_at_zoom_edge);
simple_row!(load_toggle_fit, apply_toggle_fit, click_zoom_key_toggles_fit);
simple_row!(load_reuse_folder, apply_reuse_folder, move_copy_reuse_last_folder);
simple_row!(load_show_exif, apply_show_exif, show_exif_info);
simple_row!(load_zoom_indicator, apply_zoom_indicator, show_zoom_indicator);
fn load_raw_decode(s: &AppSettings, f: &mut SettingsFormState) {
    f.raw_full_decode_index = (s.raw_full_decode == RawFullDecode::OnZoom) as i32
}
fn apply_raw_decode(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.raw_full_decode = if f.raw_full_decode_index == 1 {
        RawFullDecode::OnZoom
    } else {
        RawFullDecode::Never
    };
    None
}
fn load_raw_pair(s: &AppSettings, f: &mut SettingsFormState) {
    f.raw_pair_mode_index = s.raw_pair_mode as i32
}
fn apply_raw_pair(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.raw_pair_mode = match f.raw_pair_mode_index {
        1 => RawPairMode::PreferJpeg,
        2 => RawPairMode::PreferRaw,
        _ => RawPairMode::Separate,
    };
    None
}
fn load_delete(s: &AppSettings, f: &mut SettingsFormState) {
    f.allow_permanent_delete = s.allow_permanent_delete_without_recycle_bin
}
fn apply_delete(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.allow_permanent_delete_without_recycle_bin = f.allow_permanent_delete;
    None
}
fn load_editor(s: &AppSettings, f: &mut SettingsFormState) {
    f.external_editor_path = pr_core::settings::external_editors::ExternalEditors::default(s)
        .map_or_else(|| s.external_editor_path.clone(), |editor| editor.path);
    f.external_editors = s.external_editors.clone();
    f.default_external_editor_id = s.default_external_editor_id.clone();
    f.external_editor_sends_raw_original = s.external_editor_sends_raw_original;
}
fn apply_editor(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.external_editors = f.external_editors.clone();
    s.default_external_editor_id = f.default_external_editor_id.clone();
    s.external_editor_sends_raw_original = f.external_editor_sends_raw_original;
    s.external_editor_path =
        pr_core::settings::external_editors::ExternalEditors::default(s).map_or_else(String::new, |editor| editor.path);
    pr_core::settings::external_editors::ExternalEditors::apply_legacy_path(s, Some(&f.external_editor_path));
    None
}
fn load_journal(s: &AppSettings, f: &mut SettingsFormState) {
    f.journal_safe = s.journal_durability == JournalDurability::PowerLossSafe
}
fn apply_journal(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.journal_durability = if f.journal_safe {
        JournalDurability::PowerLossSafe
    } else {
        JournalDurability::Fast
    };
    None
}
fn load_ram(_: &AppSettings, _: &mut SettingsFormState) {}
fn apply_ram(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.image_cache_ram_percent = f.ram_cache_percent.round_ties_even() as i32;
    None
}
fn load_wheel(s: &AppSettings, f: &mut SettingsFormState) {
    f.mouse_wheel_action_index = (s.mouse_wheel_action == MouseWheelAction::Navigate) as i32
}
fn apply_wheel(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.mouse_wheel_action = if f.mouse_wheel_action_index == 1 {
        MouseWheelAction::Navigate
    } else {
        MouseWheelAction::Zoom
    };
    None
}
fn load_glide(s: &AppSettings, f: &mut SettingsFormState) {
    f.kinetic_glide_smoothing_index = (s.kinetic_glide_smoothing == Some(KineticGlideSmoothing::Predict)) as i32
}
fn apply_glide(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    s.kinetic_glide_smoothing = Some(if f.kinetic_glide_smoothing_index == 1 {
        KineticGlideSmoothing::Predict
    } else {
        KineticGlideSmoothing::Off
    });
    None
}
fn apply_context(f: &SettingsFormState, s: &mut AppSettings, _: &FieldContext<'_>) -> Option<SettingsFieldError> {
    let hidden = context_menu::ITEMS
        .iter()
        .filter(|i| !f.context_menu_shown.contains(&i.id))
        .map(|i| i.id);
    context_menu::apply_hidden(s, hidden);
    None
}
fn load_keyboard_anchor(s: &AppSettings, f: &mut SettingsFormState) {
    f.keyboard_zoom_anchor_index = (s.keyboard_zoom_anchor == Some(KeyboardZoomAnchor::ViewportCentre)) as i32
}
fn apply_keyboard_anchor(
    f: &SettingsFormState,
    s: &mut AppSettings,
    _: &FieldContext<'_>,
) -> Option<SettingsFieldError> {
    s.keyboard_zoom_anchor = Some(if f.keyboard_zoom_anchor_index == 1 {
        KeyboardZoomAnchor::ViewportCentre
    } else {
        KeyboardZoomAnchor::Pointer
    });
    None
}
simple_row!(load_exif_flags, apply_exif_flags, exif_info_fields);
simple_row!(load_title_flags, apply_title_flags, title_bar_fields);
macro_rules! int_row {
    ($load:ident,$apply:ident,$field:ident,$text:ident,$min:expr,$max:expr,$focus:expr,$key:literal) => {
        fn $load(s: &AppSettings, f: &mut SettingsFormState) {
            f.$text = s.$field.to_string()
        }
        fn $apply(f: &SettingsFormState, s: &mut AppSettings, c: &FieldContext<'_>) -> Option<SettingsFieldError> {
            if let Some(v) = try_int(&f.$text, $min, $max) {
                s.$field = v;
                None
            } else {
                Some(err(c, $key, &[("min", $min), ("max", $max)], $focus))
            }
        }
    };
}
int_row!(
    load_click_percent,
    apply_click_percent,
    click_zoom_percent,
    click_zoom_percent_text,
    10,
    800,
    SettingsFocusField::None,
    "dialog.settingsInvalid.clickZoomPercent"
);
int_row!(
    load_pan_step,
    apply_pan_step,
    arrow_pan_step_percent,
    arrow_pan_step_text,
    1,
    100,
    SettingsFocusField::ArrowPanStep,
    "dialog.settingsInvalid.arrowPanStepPercent"
);
int_row!(
    load_zoom_step,
    apply_zoom_step,
    keyboard_zoom_step_percent,
    keyboard_zoom_step_percent_text,
    5,
    100,
    SettingsFocusField::KeyboardZoomStep,
    "dialog.settingsInvalid.keyboardZoomStepPercent"
);
