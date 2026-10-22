//! The PARITY row to test map of unit W8-20b (`contracts/W8-20b.md`, "PARITY map"): every `K-*` (section 1), `C-*` (section 2) and `W-*` (section 4) row of `docs/PARITY.md`, plus the
//! status row `K-43`, with the Rust test files that prove it. DATA, not a stub: the porter of 20b only CORRECTS a proof name that does not exist (the guard test
//! `ui_parity_map::every_named_proof_file_exists` says which) and never deletes a row.
//!
//! `proofs` entries are `"<crate>:<test file stem>"` (the file `crates/<crate>/tests/<stem>.rs`, or `crates/<crate>/tests/<stem>/mod.rs`); `smoke` entries are
//! `"<ui_smoke file stem>::<test fn>"` (a `fn` of that name in `crates/pr-shell/tests/<stem>.rs`). The proofs of a row are the pure/unit proofs of the unit that owns the behaviour; the
//! `smoke` entries are the whole-shell proofs of this unit. A row is `Done` in `docs/PARITY.md` only when every proof passes AND (for `Gate::Native`) the lead recorded a desktop run.
#![allow(dead_code)] // each test binary that includes this table uses a different part of it

/// How a row may be closed in `docs/PARITY.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Proven by pure tests (fakes, goldens); `Done` as soon as they pass.
    Pure,
    /// Needs a window: `Partial (W8-..)` until the lead records an `--include-ignored` desktop run (docs/WAVE-8-PLAN.md, Risks).
    Native,
    /// An intentional difference: needs a `Different` row (and an ADR for a behaviour change); the text says which.
    Different(&'static str),
    /// The C# behaviour belongs to retired WPF code and has no Rust twin: the row is closed as `N/A` with this reason.
    Retired(&'static str),
    /// Not contracted yet (P-44, unit group W8-18x after the P-17 sync patch): the row stays `Partial`.
    Pending(&'static str),
}

/// One row of the map.
#[derive(Debug, Clone, Copy)]
pub struct ParityRow {
    /// The PARITY id, exactly as in `docs/PARITY.md` (`K-01`, `C-18`, `W-04`).
    pub id: &'static str,
    /// Short name of the item (for the failure message of the guard test).
    pub item: &'static str,
    /// Pure/unit proofs of the owning units, `"<crate>:<test file stem>"`.
    pub proofs: &'static [&'static str],
    /// Whole-shell proofs of this unit, `"<ui_smoke file stem>::<test fn>"`.
    pub smoke: &'static [&'static str],
    /// How the row may be closed.
    pub gate: Gate,
}

const fn row(
    id: &'static str,
    item: &'static str,
    proofs: &'static [&'static str],
    smoke: &'static [&'static str],
    gate: Gate,
) -> ParityRow {
    ParityRow {
        id,
        item,
        proofs,
        smoke,
        gate,
    }
}

const NAV: &str = "ui_smoke::smoke_arrow_keys_home_and_end_navigate_and_clamp_at_the_ends";
const ZOOM: &str = "ui_smoke::smoke_plus_minus_and_wheel_zoom_and_f_returns_to_fit";
const OPEN: &str = "ui_smoke::smoke_open_folder_shows_the_first_image_and_the_folder_title";
const BIN: &str = "ui_smoke::smoke_delete_sends_the_image_to_the_bin_and_shows_the_next_one";
const SKIP: &str = "ui_smoke::smoke_space_skips_to_the_next_image";
const ESC: &str = "ui_smoke::smoke_escape_closes_the_window_and_the_run_returns_zero";
const TOOLBAR: &str = "ui_smoke_overlay::toolbar_is_drawn_from_the_first_frame_at_the_top_left_and_gone_after_close";
const AUTOHIDE: &str = "ui_smoke_overlay::settings_save_reevaluates_auto_hide_and_the_toolbar_follows_the_mouse";
const DIALOGS: &str =
    "ui_smoke_dialogs::native_dialogs_open_owned_and_modal_and_the_shell_closes_cleanly_with_one_open";
const SECONDARY: &str =
    "ui_smoke_dialogs::secondary_windows_open_through_the_real_shell_and_the_shell_closes_cleanly_with_them_open";
const RECOVERY: &str = "ui_smoke_dialogs::recovery_opens_owned_and_modal_checks_in_the_background_changes_no_data_and_the_shell_closes_cleanly_with_it_open";
const BENCHMARK: &str =
    "ui_smoke_dialogs::benchmark_opens_modeless_runs_real_profiles_and_the_shell_closes_cleanly_while_it_runs";

/// The map. Order: section 1 (`K-01`..`K-43`), section 2 (`C-01`..`C-28`), section 4 (`W-01`..`W-14`).
#[rustfmt::skip] // one row per line: the table is read and diffed as data
pub const PARITY_ROWS: &[ParityRow] = &[
    // ---- 1. Keyboard shortcuts and mouse gestures ----------------------------------------------------------------------------------------------------------------------
    row("K-01", "Next image: Right", &["pr-app:shortcut_router", "pr-app:viewmodels_main_navigation", "pr-platform-win:shell_input_dispatch_keys"], &[NAV], Gate::Native),
    row("K-02", "Previous image: Left", &["pr-app:shortcut_router", "pr-app:viewmodels_main_navigation", "pr-platform-win:shell_input_dispatch_keys"], &[NAV], Gate::Native),
    row("K-03", "First image: Home", &["pr-app:shortcut_router", "pr-app:viewmodels_main_navigation", "pr-platform-win:shell_input_e2e"], &[NAV], Gate::Native),
    row("K-04", "Last image: End", &["pr-app:shortcut_router", "pr-app:viewmodels_main_navigation", "pr-platform-win:shell_input_e2e"], &[NAV], Gate::Native),
    row("K-05", "Next sibling folder: PageDown", &["pr-app:coordinators_sibling_navigator", "pr-core:catalog_sorting", "pr-app:shortcut_router"], &[], Gate::Pure),
    row("K-06", "Previous sibling folder: PageUp", &["pr-app:coordinators_sibling_navigator", "pr-core:catalog_sorting", "pr-app:shortcut_router"], &[], Gate::Pure),
    row("K-07", "Undo: Ctrl+Z", &["pr-app:file_action_undo", "pr-app:viewmodels_main_commands", "pr-app:shortcut_router"], &[], Gate::Pure),
    row("K-08", "Recycle: Delete", &["pr-app:file_action_recycle", "pr-app:shortcut_router", "pr-app:coordinators_file_action_gate"], &[BIN], Gate::Native),
    row("K-09", "Action profiles: Enter, F3, F4, F5", &["pr-app:action_profiles", "pr-app:shortcut_router", "pr-app:file_action_move_copy"], &[], Gate::Pure),
    row("K-10", "Skip: Space", &["pr-app:viewmodels_main_session", "pr-app:shortcut_router", "pr-platform-win:shell_input_e2e"], &[SKIP], Gate::Native),
    row("K-11", "Compare toggle: C", &["pr-app:viewmodels_compare", "pr-app:shortcut_router", "pr-platform-win:host_main_window_viewing"], &[], Gate::Native),
    row("K-12", "Toggle Fit: F", &["pr-app:viewport_fit", "pr-app:pointer_zoom", "pr-app:shortcut_router"], &[ZOOM], Gate::Native),
    row("K-13", "Zoom in / out: Add / Subtract", &["pr-app:pointer_zoom", "pr-app:viewer_state", "pr-platform-win:shell_input_e2e"], &[ZOOM], Gate::Native),
    row("K-14", "Zoom 100 %: D1", &["pr-app:pointer_zoom", "pr-app:viewer_state"], &[ZOOM], Gate::Native),
    row("K-15", "Click-zoom toggle: D2", &["pr-app:pointer_click_zoom", "pr-app:shortcut_router"], &[], Gate::Pure),
    row("K-16", "Custom zoom dialog: D3", &["pr-platform-win:shell_menus_e2e", "pr-platform-win:dialogs_zoom_diagnostics", "pr-app:presenters_secondary"], &[DIALOGS], Gate::Native),
    row("K-17", "Fit width: W", &["pr-app:pointer_zoom", "pr-app:viewport_fit"], &[], Gate::Pure),
    row("K-18", "Fit height: H", &["pr-app:pointer_zoom", "pr-app:viewport_fit"], &[], Gate::Pure),
    row("K-19", "Toggle keep zoom across images: K", &["pr-platform-win:shell_input_e2e", "pr-app:viewmodels_main_commands"], &[], Gate::Native),
    row("K-20", "Toggle info overlay: I", &["pr-app:viewmodels_info_overlay", "pr-platform-win:shell_input_e2e"], &[], Gate::Native),
    row("K-21", "Fullscreen: F11", &["pr-app:windowing_fullscreen", "pr-platform-win:host_main_window_windowing", "pr-platform-win:shell_input_e2e"], &[], Gate::Native),
    row("K-22", "Escape priority", &["pr-app:shortcut_router", "pr-platform-win:shell_input_e2e", "pr-app:duplicate_cleanup"], &[ESC], Gate::Native),
    row("K-23", "Move to: M", &["pr-app:file_action_move_copy", "pr-app:viewmodels_main_notify_move", "pr-app:shortcut_router"], &[], Gate::Pure),
    row("K-24", "Copy to: Y", &["pr-app:file_action_move_copy", "pr-app:viewmodels_main_notify_move", "pr-app:shortcut_router"], &[], Gate::Pure),
    row("K-25", "Open folder: Ctrl+O", &["pr-app:shortcut_router", "pr-app:viewmodels_main_navigation", "pr-platform-win:dialogs_folder_picker"], &[OPEN], Gate::Native),
    row("K-26", "Switch JPEG/RAW member", &["pr-app:viewmodels_main_navigation", "pr-app:presenter_state"], &[], Gate::Pure),
    row("K-27", "Arrow keys on a zoomed image pan", &["pr-app:viewport_arrow_pan", "pr-app:kinetic_keyboard_pan"], &[], Gate::Pure),
    row("K-28", "Auto-repeat ignored for some commands", &["pr-app:input_review_command", "pr-platform-win:shell_input_dispatch_keys"], &[], Gate::Pure),
    row("K-29", "Space/Enter not taken from a focused button", &["pr-platform-win:shell_input_dispatch_keys"], &[], Gate::Retired("the Win32 shell has no focusable buttons: the toolbar is self-drawn and swallows presses (W8-15e/15j)")),
    row("K-30", "Compare pane select", &["pr-platform-win:host_main_window_viewing"], &[], Gate::Native),
    row("K-31", "Router priority order", &["pr-app:shortcut_router"], &[], Gate::Pure),
    row("K-32", "Shortcut validation", &["pr-core:settings_normalizer", "pr-platform-win:settings_page_shortcuts"], &[], Gate::Pure),
    row("K-33", "Mouse wheel", &["pr-app:pointer_wheel", "pr-app:mouse_gestures", "pr-app:golden_gestures", "pr-platform-win:shell_input_e2e"], &[ZOOM], Gate::Native),
    row("K-34", "Left click on image: click-to-zoom", &["pr-app:pointer_click_zoom", "pr-platform-win:shell_input_e2e"], &[], Gate::Native),
    row("K-35", "Double-click on image: Fit", &["pr-app:pointer_click_zoom"], &[], Gate::Pure),
    row("K-36", "Left-drag pan plus kinetic glide", &["pr-app:pointer_pan", "pr-app:kinetic_scroller", "pr-platform-win:shell_input_e2e"], &[], Gate::Native),
    row("K-37", "Stop a glide", &["pr-app:pointer_pan", "pr-platform-win:shell_input_e2e"], &[], Gate::Native),
    row("K-38", "Right click: context menu", &["pr-platform-win:shell_menus_e2e", "pr-platform-win:shell_menus_main_window"], &[], Gate::Native),
    row("K-39", "Drag and drop", &["pr-core:catalog_drag_drop", "pr-platform-win:shell_input_drop"], &[], Gate::Native),
    row("K-40", "Mouse activity: toolbar and overlay", &["pr-app:coordinators_autohide", "pr-app:overlay_animator", "pr-app:overlay_status_toolbar"], &[AUTOHIDE], Gate::Native),
    row("K-41", "Settings window: Ctrl+Tab", &["pr-platform-win:settings_dialog_frame_e2e"], &[], Gate::Native),
    row("K-42", "Shortcut capture boxes", &["pr-platform-win:settings_page_shortcuts", "pr-platform-win:dialogs_action_profiles"], &[], Gate::Native),
    row("K-43", "Key mapping (status row of W8-05c)", &["pr-app:input_key_mapping", "pr-app:key_id"], &[], Gate::Pure),
    // ---- 2. Commands and menu items -------------------------------------------------------------------------------------------------------------------------------
    row("C-01", "Toolbar: Open folder", &["pr-app:overlay_status_toolbar", "pr-platform-win:shell_overlay_main_window"], &[TOOLBAR], Gate::Native),
    row("C-02", "Toolbar: Settings", &["pr-app:overlay_status_toolbar", "pr-platform-win:shell_overlay_main_window", "pr-platform-win:settings_dialog_frame_e2e"], &[TOOLBAR], Gate::Native),
    row("C-03", "Toolbar: Fit", &["pr-app:overlay_status_toolbar", "pr-platform-win:shell_overlay_main_window"], &[TOOLBAR], Gate::Native),
    row("C-04", "Toolbar: Tools popup", &["pr-platform-win:shell_overlay_main_window", "pr-platform-win:shell_menus_main_window"], &[TOOLBAR], Gate::Native),
    row("C-05", "Tools: Clear cache", &["pr-app:duplicate_cleanup", "pr-platform-win:host_main_window_viewing"], &[], Gate::Native),
    row("C-06", "Tools: Remove duplicates", &["pr-app:duplicate_cleanup", "pr-core:file_actions_duplicates"], &[], Gate::Pure),
    row("C-07", "Context menu: Undo", &["pr-platform-win:shell_menus_controller", "pr-platform-win:shell_menus_e2e", "pr-core:settings_context_menu"], &[], Gate::Native),
    row("C-08", "Context menu: Move to Recycle Bin", &["pr-platform-win:shell_menus_controller", "pr-app:menu_model", "pr-core:settings_context_menu"], &[], Gate::Native),
    row("C-09", "Context menu: Fit", &["pr-platform-win:shell_menus_controller", "pr-app:menu_model", "pr-platform-win:shell_menus_e2e"], &[], Gate::Native),
    row("C-10", "Context menu: Zoom to N %", &["pr-platform-win:shell_menus_e2e", "pr-app:pointer_click_zoom", "pr-app:menus_zoom_rules"], &[], Gate::Native),
    row("C-11", "Context menu: Zoom submenu", &["pr-app:menus_zoom_rules", "pr-platform-win:shell_menus_e2e", "pr-platform-win:shell_menus_owner_draw"], &[], Gate::Native),
    row("C-12", "Preset or Custom selection", &["pr-app:menus_zoom_rules", "pr-platform-win:shell_menus_e2e"], &[], Gate::Native),
    row("C-13", "Context menu: Open folder / Next folder / Previous folder", &["pr-platform-win:shell_menus_controller", "pr-core:settings_context_menu", "pr-platform-win:shell_menus_e2e"], &[], Gate::Native),
    row("C-14", "Context menu: Open in external editor", &["pr-platform-win:shell_menus_controller", "pr-app:viewmodels_main_commands"], &[], Gate::Pending("P-44: the external-editor settings and dialog are the unit group W8-18x; the menu item is proven by 14e")),
    row("C-15", "Context menu: Settings", &["pr-platform-win:shell_menus_controller", "pr-platform-win:shell_menus_e2e"], &[], Gate::Native),
    row("C-16", "Context menu separator rules", &["pr-platform-win:shell_menus_controller", "pr-core:settings_context_menu"], &[], Gate::Pure),
    row("C-17", "Status panel: View list (skipped files)", &["pr-app:overlay_status_toolbar", "pr-platform-win:dialogs_secondary_window_port", "pr-platform-win:dialogs_native_list_dialog"], &[SECONDARY], Gate::Native),
    row("C-18", "Open a path from the command line", &["pr-app:startup_launch", "pr-shell:startup_native", "pr-shell:startup_run_native", "pr-shell:exe_process"], &[OPEN], Gate::Native),
    row("C-19", "Forwarded open from a second launch", &["pr-app:startup_forwarded_open", "pr-shell:startup_native", "pr-shell:startup_run_native", "pr-shell:exe_process"], &[], Gate::Native),
    row("C-20", "Settings Save side effects", &["pr-app:format_settings_reload_path", "pr-app:viewmodels_main_state", "pr-platform-win:settings_dialog_apply_e2e"], &[], Gate::Native),
    row("C-21", "Run an action profile", &["pr-app:action_profiles", "pr-app:file_action_move_copy", "pr-app:file_action_recycle"], &[], Gate::Pure),
    row("C-22", "Move to / Copy to", &["pr-app:file_action_move_copy", "pr-core:file_actions_destination_policy", "pr-app:viewmodels_main_notify_move"], &[], Gate::Pure),
    row("C-23", "Undo command", &["pr-app:file_action_undo", "pr-app:viewmodels_main_commands"], &[], Gate::Pure),
    row("C-24", "Switch capture member", &["pr-app:viewmodels_main_navigation", "pr-app:presenter_state"], &[], Gate::Pure),
    row("C-25", "Compare mode", &["pr-app:viewmodels_compare", "pr-core:catalog_compare", "pr-platform-win:host_main_window_viewing"], &[], Gate::Native),
    row("C-26", "Window close while a file action runs", &["pr-app:coordinators_file_action_gate", "pr-platform-win:host_main_window_lifecycle", "pr-shell:startup_run_native"], &[], Gate::Native),
    row("C-27", "Startup recovery prompt", &["pr-app:startup_recovery", "pr-shell:startup_run_native"], &[RECOVERY], Gate::Native),
    row("C-28", "Settings repair notice", &["pr-app:startup_settings_load", "pr-core:settings_normalizer"], &[], Gate::Pure),
    // ---- 4. Windows and dialogs -------------------------------------------------------------------------------------------------------------------------------------
    row("W-01", "MainWindow", &["pr-platform-win:host_main_window_core", "pr-platform-win:host_main_window_lifecycle", "pr-platform-win:host_main_window_frame", "pr-platform-win:host_main_window_windowing", "pr-platform-win:shell_overlay_main_window"], &[OPEN, NAV, ESC, TOOLBAR], Gate::Native),
    row("W-02", "SettingsWindow", &["pr-platform-win:settings_dialog_frame_e2e", "pr-platform-win:settings_dialog_state_e2e", "pr-platform-win:settings_dialog_apply_e2e", "pr-platform-win:settings_dialog_layout_e2e", "pr-app:settings_presenter_apply"], &[SECONDARY], Gate::Native),
    row("W-03", "ActionProfilesWindow", &["pr-platform-win:dialogs_action_profiles", "pr-platform-win:dialogs_action_profiles_native", "pr-app:action_profiles_io"], &[], Gate::Native),
    row("W-04", "RecoveryWindow", &["pr-platform-win:dialogs_recovery_window_e2e", "pr-platform-win:dialogs_recovery_flow_e2e", "pr-platform-win:dialogs_recovery_data_safety", "pr-app:recovery_window_presenter"], &[RECOVERY], Gate::Native),
    row("W-05", "DiagnosticsWindow", &["pr-platform-win:dialogs_zoom_diagnostics", "pr-platform-win:dialogs_zoom_diagnostics_native"], &[DIALOGS], Gate::Native),
    row("W-06", "BenchmarkWindow", &["pr-platform-win:dialogs_benchmark_native", "pr-platform-win:dialogs_benchmark_run_native", "pr-app:benchmark_presenter"], &[BENCHMARK], Gate::Native),
    row("W-07", "BatchReviewWindow", &["pr-platform-win:dialogs_native_list_dialog", "pr-app:duplicate_cleanup"], &[], Gate::Native),
    row("W-08", "SkippedFilesWindow", &["pr-platform-win:dialogs_native_list_dialog", "pr-platform-win:dialogs_secondary_window_port_native"], &[SECONDARY], Gate::Native),
    row("W-09", "ClickZoomCustomDialog", &["pr-platform-win:dialogs_zoom_diagnostics_native", "pr-platform-win:shell_menus_e2e"], &[DIALOGS], Gate::Native),
    row("W-10", "Folder picker", &["pr-platform-win:dialogs_folder_picker", "pr-platform-win:dialogs_native_pickers"], &[], Gate::Native),
    row("W-11", "Startup message boxes", &["pr-app:startup_shutdown", "pr-app:startup_recovery", "pr-app:startup_instance", "pr-shell:startup_run_native"], &[], Gate::Native),
    row("W-12", "File-action confirmations", &["pr-app:file_action_core", "pr-app:file_action_recycle", "pr-app:file_action_move_copy", "pr-platform-win:dialogs_service"], &[], Gate::Pure),
    row("W-13", "Other prompts", &["pr-app:duplicate_cleanup", "pr-platform-win:dialogs_service"], &[], Gate::Pure),
    row("W-14", "Recovery and Settings sub-prompts", &["pr-platform-win:dialogs_recovery_flow_e2e", "pr-platform-win:dialogs_action_profiles", "pr-platform-win:settings_dialog_apply_e2e", "pr-platform-win:dialogs_service"], &[], Gate::Native),
];
