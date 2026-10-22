//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/MouseSettingsPage.cs` (268 lines, baseline `d92d6aae`).
//! Unit W8-19h: the mouse, zoom, and context-menu settings page.

use pr_app::presenters::settings::SettingsPresenter;
use pr_app::presenters::settings_form_state::{SettingsFocusField, SettingsFormState};
use pr_core::settings::context_menu::ContextMenuItemId;
use pr_l10n::tr;

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::{SettingsPageContext, with_field_context};
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView, text_fn};
use crate::win32::control_constants::CBN_SELCHANGE;
use crate::win32::dialog_native::BN_CLICKED;

/// C# `Ids` (internal, for tests): the 19 single control ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseIds {
    pub click_zoom_percent: i32,
    pub click_to_zoom: i32,
    pub arrow_pan_step: i32,
    pub keyboard_zoom_anchor: i32,
    pub zoom_shortcuts_summary: i32,
    pub fit_width_anchor: i32,
    pub keep_zoom: i32,
    pub keyboard_zoom_step: i32,
    pub fit_width_anchor2: i32,
    pub middle_click: i32,
    pub click_zoom_key_toggles_fit: i32,
    pub mouse_wheel: i32,
    pub kinetic_pan: i32,
    pub kinetic_smoothing: i32,
    pub arrow_key_at_edge: i32,
    pub image_transition: i32,
    pub image_transition_ms: i32,
    pub touchpad_swipe: i32,
    pub touchpad_distance: i32,
}

/// C# `MouseSettingsPage`.
#[derive(Debug)]
pub struct MouseSettingsPage {
    context: PageContextSlot,
    ids: MouseIds,
    /// C# `_menuChecks`, in `ITEMS` table order.
    menu_checks: Vec<(ContextMenuItemId, i32)>,
}

impl Default for MouseSettingsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseSettingsPage {
    /// Creates the page with zero control ids and a detached context.
    pub fn new() -> Self {
        Self {
            context: PageContextSlot::new(),
            ids: MouseIds {
                click_zoom_percent: 0,
                click_to_zoom: 0,
                arrow_pan_step: 0,
                keyboard_zoom_anchor: 0,
                zoom_shortcuts_summary: 0,
                fit_width_anchor: 0,
                keep_zoom: 0,
                keyboard_zoom_step: 0,
                fit_width_anchor2: 0,
                middle_click: 0,
                click_zoom_key_toggles_fit: 0,
                mouse_wheel: 0,
                kinetic_pan: 0,
                kinetic_smoothing: 0,
                arrow_key_at_edge: 0,
                image_transition: 0,
                image_transition_ms: 0,
                touchpad_swipe: 0,
                touchpad_distance: 0,
            },
            menu_checks: Vec::new(),
        }
    }

    /// Returns the ids of this page's single controls.
    pub fn ids(&self) -> MouseIds {
        self.ids
    }

    /// Returns context-menu check ids in `ITEMS` order (empty until declaration).
    pub fn menu_checks(&self) -> &[(ContextMenuItemId, i32)] {
        &self.menu_checks
    }

    fn menu_check(&self, item: ContextMenuItemId) -> Option<i32> {
        self.menu_checks
            .iter()
            .find_map(|(candidate, id)| (*candidate == item).then_some(*id))
    }

    fn set_summary(&self, shortcut_texts: &std::collections::BTreeMap<String, String>) {
        let context = self.context.context();
        let summary =
            with_field_context(|field_context| SettingsPresenter::zoom_shortcut_summary(shortcut_texts, field_context));
        if context.text(self.ids.zoom_shortcuts_summary) != summary {
            context.set_text(self.ids.zoom_shortcuts_summary, &summary);
        }
    }

    fn update_dependents(&self) {
        let context = self.context.context();
        context.set_enabled(
            self.ids.image_transition_ms,
            context.combo_index(self.ids.image_transition) == 1,
        );
    }
}

impl SettingsPageView for MouseSettingsPage {
    fn key(&self) -> &str {
        "Mouse"
    }

    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        let mut ids = self.ids;
        let mut menu_checks = Vec::new();

        page.group(text_fn(tr::settings_group_zoom), |group| {
            ids.click_zoom_percent = group.edit(text_fn(tr::settings_click_zoom_percent_label), 45).control;
            group.note(Some(text_fn(tr::settings_click_zoom_percent_hint)), true);
            ids.click_to_zoom = group.check(text_fn(tr::settings_click_to_zoom_label), 0);
            ids.arrow_pan_step = group
                .edit(text_fn(tr::settings_arrow_pan_step_percent_label), 45)
                .control;
            group.note(Some(text_fn(tr::settings_arrow_pan_step_percent_hint)), true);
            ids.keyboard_zoom_anchor = group
                .combo(
                    text_fn(tr::settings_keyboard_zoom_anchor_label),
                    Some(vec![
                        text_fn(tr::enum_keyboard_zoom_anchor_pointer),
                        text_fn(tr::enum_keyboard_zoom_anchor_viewport_centre),
                    ]),
                    0,
                )
                .control;
            group.note(Some(text_fn(tr::settings_keyboard_zoom_anchor_hint)), true);
            group.text(Some(text_fn(tr::settings_zoom_shortcuts_title)));
            ids.zoom_shortcuts_summary = group.text(None);
            group.note(Some(text_fn(tr::settings_zoom_shortcuts_hint)), false);
            ids.fit_width_anchor = group
                .combo(
                    text_fn(tr::settings_fit_width_anchor_label),
                    Some(vec![
                        text_fn(tr::enum_fit_width_anchor_centre),
                        text_fn(tr::enum_fit_width_anchor_top_third),
                        text_fn(tr::enum_fit_width_anchor_bottom_third),
                    ]),
                    0,
                )
                .control;
            group.note(Some(text_fn(tr::settings_fit_width_anchor_hint)), true);
            ids.keep_zoom = group.check(text_fn(tr::settings_keep_zoom_across_images_label), 0);
            group.note(Some(text_fn(tr::settings_keep_zoom_across_images_hint)), false);
            ids.keyboard_zoom_step = group
                .edit(text_fn(tr::settings_keyboard_zoom_step_percent_label), 45)
                .control;
            group.note(
                Some(text_fn(|| {
                    with_field_context(SettingsPresenter::keyboard_zoom_step_percent_hint)
                })),
                true,
            );
            ids.fit_width_anchor2 = group
                .combo(
                    text_fn(tr::settings_fit_width_anchor2_label),
                    Some(vec![
                        text_fn(tr::enum_fit_width_anchor_centre),
                        text_fn(tr::enum_fit_width_anchor_top_third),
                        text_fn(tr::enum_fit_width_anchor_bottom_third),
                    ]),
                    0,
                )
                .control;
            group.note(Some(text_fn(tr::settings_fit_width_anchor2_hint)), true);
            ids.middle_click = group
                .combo(
                    text_fn(tr::settings_middle_click_action_label),
                    Some(vec![
                        text_fn(tr::enum_middle_click_action_none),
                        text_fn(tr::enum_middle_click_action_click_zoom),
                        text_fn(tr::enum_middle_click_action_actual_size),
                        text_fn(tr::enum_middle_click_action_fit),
                        text_fn(tr::enum_middle_click_action_fit_width),
                        text_fn(tr::enum_middle_click_action_fit_width2),
                        text_fn(tr::enum_middle_click_action_previous_image),
                        text_fn(tr::enum_middle_click_action_next_image),
                        text_fn(tr::enum_middle_click_action_previous_folder),
                        text_fn(tr::enum_middle_click_action_next_folder),
                        text_fn(tr::enum_middle_click_action_open_folder),
                    ]),
                    0,
                )
                .control;
            group.note(Some(text_fn(tr::settings_middle_click_action_hint)), true);
            ids.click_zoom_key_toggles_fit = group.check(text_fn(tr::settings_click_zoom_key_toggles_fit_label), 0);
            group.note(Some(text_fn(tr::settings_click_zoom_key_toggles_fit_hint)), false);
        });

        page.group(text_fn(tr::settings_group_mouse_zoom), |group| {
            ids.mouse_wheel = group
                .combo(
                    text_fn(tr::settings_mouse_wheel_action_label),
                    Some(vec![
                        text_fn(tr::enum_mouse_wheel_action_zoom),
                        text_fn(tr::enum_mouse_wheel_action_navigate),
                    ]),
                    0,
                )
                .control;
            group.note(Some(text_fn(tr::settings_mouse_wheel_action_hint)), true);
            ids.kinetic_pan = group.check(text_fn(tr::settings_kinetic_pan_label), 0);
            ids.kinetic_smoothing = group
                .combo(
                    text_fn(tr::settings_kinetic_glide_smoothing_label),
                    Some(vec![
                        text_fn(tr::enum_kinetic_glide_smoothing_off),
                        text_fn(tr::enum_kinetic_glide_smoothing_predict),
                    ]),
                    0,
                )
                .control;
            group.note(Some(text_fn(tr::settings_kinetic_glide_smoothing_hint)), true);
            ids.arrow_key_at_edge = group.check(text_fn(tr::settings_arrow_key_navigates_at_zoom_edge_label), 0);
            group.note(Some(text_fn(tr::settings_arrow_key_navigates_at_zoom_edge_hint)), false);
            ids.image_transition = group
                .combo(
                    text_fn(tr::settings_image_transition_label),
                    Some(vec![
                        text_fn(tr::enum_image_transition_none),
                        text_fn(tr::enum_image_transition_fade),
                    ]),
                    0,
                )
                .control;
            group.note(Some(text_fn(tr::settings_image_transition_hint)), true);
            ids.image_transition_ms = group.edit(text_fn(tr::settings_image_transition_ms_label), 45).control;
            ids.touchpad_swipe = group.check(text_fn(tr::settings_touchpad_swipe_label), 0);
            group.note(Some(text_fn(tr::settings_touchpad_swipe_hint)), false);
            ids.touchpad_distance = group
                .edit(text_fn(tr::settings_touchpad_swipe_distance_label), 45)
                .control;
            group.note(Some(text_fn(tr::settings_touchpad_swipe_distance_hint)), true);
        });

        page.group(text_fn(tr::settings_group_context_menu), |group| {
            group.note(Some(text_fn(tr::settings_context_menu_hint)), false);
            let rows = with_field_context(SettingsPresenter::context_menu_rows);
            for row in rows {
                let item = row.id;
                let check_id = group.check(
                    text_fn(move || {
                        with_field_context(|field_context| {
                            SettingsPresenter::context_menu_item_label(item, field_context)
                        })
                    }),
                    if row.is_child { 12 } else { 0 },
                );
                menu_checks.push((item, check_id));
            }
        });

        self.ids = ids;
        self.menu_checks = menu_checks;
    }

    fn attach(&mut self, context: SettingsPageContext) {
        self.context.attach(context);
        let context = self.context.context();
        let rows = with_field_context(SettingsPresenter::context_menu_rows);
        for row in rows.into_iter().filter(|row| row.locked) {
            if let Some(id) = self.menu_check(row.id) {
                context.set_tooltip(id, text_fn(tr::settings_context_menu_settings_locked_tooltip));
            }
        }
    }

    fn load(&mut self, state: &SettingsFormState) {
        let context = self.context.context();
        context.set_text(self.ids.click_zoom_percent, &state.click_zoom_percent_text);
        context.set_checked(self.ids.click_to_zoom, state.click_to_zoom_enabled);
        context.set_text(self.ids.arrow_pan_step, &state.arrow_pan_step_text);
        context.set_combo_index(self.ids.keyboard_zoom_anchor, state.keyboard_zoom_anchor_index);
        context.set_combo_index(self.ids.fit_width_anchor, state.fit_width_anchor_index);
        context.set_checked(self.ids.keep_zoom, state.keep_zoom_across_images);
        context.set_text(self.ids.keyboard_zoom_step, &state.keyboard_zoom_step_percent_text);
        context.set_combo_index(self.ids.fit_width_anchor2, state.fit_width_anchor2_index);
        context.set_combo_index(self.ids.middle_click, state.middle_click_action_index);
        context.set_checked(self.ids.click_zoom_key_toggles_fit, state.click_zoom_key_toggles_fit);
        context.set_combo_index(self.ids.mouse_wheel, state.mouse_wheel_action_index);
        context.set_checked(self.ids.kinetic_pan, state.kinetic_pan_enabled);
        context.set_combo_index(self.ids.kinetic_smoothing, state.kinetic_glide_smoothing_index);
        context.set_checked(self.ids.arrow_key_at_edge, state.arrow_key_navigates_at_zoom_edge);
        context.set_combo_index(self.ids.image_transition, state.image_transition_index);
        context.set_text(self.ids.image_transition_ms, &state.image_transition_ms_text);
        context.set_checked(self.ids.touchpad_swipe, state.touchpad_swipe_enabled);
        context.set_text(self.ids.touchpad_distance, &state.touchpad_swipe_distance_text);
        for (item, id) in &self.menu_checks {
            context.set_checked(*id, state.context_menu_shown.contains(item));
        }
        self.set_summary(&state.shortcut_texts);
        self.update_dependents();
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        let context = self.context.context();
        state.click_zoom_percent_text = context.text(self.ids.click_zoom_percent);
        state.click_to_zoom_enabled = context.is_checked(self.ids.click_to_zoom);
        state.arrow_pan_step_text = context.text(self.ids.arrow_pan_step);
        state.keyboard_zoom_anchor_index = context.combo_index(self.ids.keyboard_zoom_anchor);
        state.fit_width_anchor_index = context.combo_index(self.ids.fit_width_anchor);
        state.keep_zoom_across_images = context.is_checked(self.ids.keep_zoom);
        state.keyboard_zoom_step_percent_text = context.text(self.ids.keyboard_zoom_step);
        state.fit_width_anchor2_index = context.combo_index(self.ids.fit_width_anchor2);
        state.middle_click_action_index = context.combo_index(self.ids.middle_click);
        state.click_zoom_key_toggles_fit = context.is_checked(self.ids.click_zoom_key_toggles_fit);
        state.mouse_wheel_action_index = context.combo_index(self.ids.mouse_wheel);
        state.kinetic_pan_enabled = context.is_checked(self.ids.kinetic_pan);
        state.kinetic_glide_smoothing_index = context.combo_index(self.ids.kinetic_smoothing);
        state.arrow_key_navigates_at_zoom_edge = context.is_checked(self.ids.arrow_key_at_edge);
        state.image_transition_index = context.combo_index(self.ids.image_transition);
        state.image_transition_ms_text = context.text(self.ids.image_transition_ms);
        state.touchpad_swipe_enabled = context.is_checked(self.ids.touchpad_swipe);
        state.touchpad_swipe_distance_text = context.text(self.ids.touchpad_distance);
        state.context_menu_shown.clear();
        for (item, id) in &self.menu_checks {
            if context.is_checked(*id) {
                state.context_menu_shown.insert(*item);
            }
        }
    }

    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        if id == self.ids.image_transition && notification == CBN_SELCHANGE as u32 {
            self.update_dependents();
            return true;
        }
        if notification == BN_CLICKED
            && let Some(settings_id) = self.menu_check(ContextMenuItemId::Settings)
            && id == settings_id
        {
            self.context.context().set_checked(settings_id, true);
            return true;
        }
        false
    }

    fn on_activated(&mut self) {
        let texts = self.context.context().current_shortcut_texts();
        self.set_summary(&texts);
    }

    fn on_language_changed(&mut self) {
        let texts = self.context.context().current_shortcut_texts();
        self.set_summary(&texts);
    }

    fn focus_target(&self, field: SettingsFocusField) -> Option<i32> {
        match field {
            SettingsFocusField::ArrowPanStep => Some(self.ids.arrow_pan_step),
            SettingsFocusField::KeyboardZoomStep => Some(self.ids.keyboard_zoom_step),
            SettingsFocusField::ImageTransitionMs => Some(self.ids.image_transition_ms),
            SettingsFocusField::TouchpadSwipeDistance => Some(self.ids.touchpad_distance),
            _ => None,
        }
    }
}
