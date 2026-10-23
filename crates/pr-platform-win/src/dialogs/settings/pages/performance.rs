//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/PerformanceSettingsPage.cs` (164 lines, baseline `d92d6aae`): the "Performance" page (WPF
//! `PerformancePage`): decoder backend, RAM cache share (slider whose range follows the preload window and the machine RAM), preload counts. Unit W8-19d.
//! Contract: `contracts/W8-19d.md`.
//!
//! Properties owned: `DecoderBackend` (index), `ImageCacheRamPercent`, `PreloadForwardCount`, `PreloadBackwardCount`. RAW/WebP/HEIC/compare are on the Files page.
use pr_app::presenters::settings::SettingsPresenter;
use pr_app::presenters::settings_form_state::{SettingsFocusField, SettingsFormState};
use pr_core::settings::perf_options::{MAX_IMAGE_CACHE_RAM_PERCENT, MIN_IMAGE_CACHE_RAM_PERCENT};
use pr_l10n::tr;

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::SettingsPageContext;
use crate::dialogs::settings::context::with_field_context;
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView, text_fn};
use crate::win32::control_constants::CBN_SELCHANGE;

/// C# `TurboJpegIndex`: the position of the TurboJpeg item in the decoder combo (the contract with `SettingsFormState::decoder_backend_index`). Real code.
pub const TURBO_JPEG_INDEX: i32 = 2;

/// `EN_CHANGE` (C# `private const int EnChange = 0x0300` of the page).
pub const EN_CHANGE: u32 = 0x0300;

/// C# `Ids` (internal, for tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerformanceIds {
    pub decoder: i32,
    pub turbo_note: i32,
    pub ram_slider: i32,
    pub ram_value: i32,
    pub ram_hint: i32,
    pub forward: i32,
    pub backward: i32,
}

/// C# `PerformanceSettingsPage`.
#[derive(Debug)]
pub struct PerformanceSettingsPage {
    context: PageContextSlot,
    decoder: i32,
    turbo_note: i32,
    ram_slider: i32,
    ram_value: i32,
    ram_hint: i32,
    forward: i32,
    backward: i32,
    last_decoder: i32,
    turbo_available: bool,
}

impl Default for PerformanceSettingsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl PerformanceSettingsPage {
    /// Ids 0, `last_decoder` 0, `turbo_available` true (C# initialisers), detached context.
    pub fn new() -> Self {
        Self {
            context: PageContextSlot::new(),
            decoder: 0,
            turbo_note: 0,
            ram_slider: 0,
            ram_value: 0,
            ram_hint: 0,
            forward: 0,
            backward: 0,
            last_decoder: 0,
            turbo_available: true,
        }
    }

    /// C# `Ids`.
    pub fn ids(&self) -> PerformanceIds {
        PerformanceIds {
            decoder: self.decoder,
            turbo_note: self.turbo_note,
            ram_slider: self.ram_slider,
            ram_value: self.ram_value,
            ram_hint: self.ram_hint,
            forward: self.forward,
            backward: self.backward,
        }
    }
}

impl SettingsPageView for PerformanceSettingsPage {
    /// `"Performance"`.
    fn key(&self) -> &str {
        "Performance"
    }

    /// One group (title `tr::settings_nav_performance`): decoder combo (3 items, order = `SettingsFormState::decoder_backend_index`), `note(None, true)` (turbo note),
    /// slider (`MIN_IMAGE_CACHE_RAM_PERCENT`..`MAX_IMAGE_CACHE_RAM_PERCENT`), `note(None, true)` (RAM hint), forward edit (45 dlu), backward edit (45 dlu),
    /// `note(Some(preload_window_hint), true)`.
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        let mut ids = [0; 7];
        page.group(text_fn(tr::settings_nav_performance), |group| {
            ids[0] = group
                .combo(
                    text_fn(tr::settings_decoder_backend_label),
                    Some(vec![
                        text_fn(tr::enum_decoder_backend_wpf),
                        text_fn(tr::enum_decoder_backend_wic_direct),
                        text_fn(tr::enum_decoder_backend_turbo_jpeg),
                    ]),
                    0,
                )
                .control;
            ids[1] = group.note(None, true);
            let slider = group.slider(
                text_fn(tr::settings_ram_cache_label),
                MIN_IMAGE_CACHE_RAM_PERCENT,
                MAX_IMAGE_CACHE_RAM_PERCENT,
            );
            ids[2] = slider.slider;
            ids[3] = slider.value;
            ids[4] = group.note(None, true);
            ids[5] = group.edit(text_fn(tr::settings_preload_forward_label), 45).control;
            ids[6] = group.edit(text_fn(tr::settings_preload_backward_label), 45).control;
            group.note(
                Some(text_fn(|| with_field_context(SettingsPresenter::preload_window_hint))),
                true,
            );
        });
        self.decoder = ids[0];
        self.turbo_note = ids[1];
        self.ram_slider = ids[2];
        self.ram_value = ids[3];
        self.ram_hint = ids[4];
        self.forward = ids[5];
        self.backward = ids[6];
    }

    /// Stores the context, `turbo_available = SettingsPresenter::is_turbo_jpeg_available(decoder_factory)`, `set_visible(turbo_note, !turbo_available)`, `update_turbo_note()`.
    fn attach(&mut self, context: SettingsPageContext) {
        self.context.attach(context);
        let context = self.context.context();
        self.turbo_available = SettingsPresenter::is_turbo_jpeg_available(context.decoder_factory().as_deref());
        context.set_visible(self.turbo_note, !self.turbo_available);
        self.update_turbo_note();
    }

    fn load(&mut self, state: &SettingsFormState) {
        let context = self.context.context();
        context.set_combo_index(self.decoder, state.decoder_backend_index);
        self.last_decoder = context.combo_index(self.decoder);
        context.set_text(self.forward, &state.preload_forward_text);
        context.set_text(self.backward, &state.preload_backward_text);
        self.update_range();
        context.set_slider_value(self.ram_slider, state.ram_cache_percent.round_ties_even() as i32);
        self.update_ram_texts();
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        let context = self.context.context();
        state.decoder_backend_index = context.combo_index(self.decoder);
        state.ram_cache_percent = f64::from(context.slider_value(self.ram_slider));
        state.preload_forward_text = context.text(self.forward);
        state.preload_backward_text = context.text(self.backward);
    }

    /// Decoder `CBN_SELCHANGE` (refuse a missing TurboJpeg), preload boxes `EN_CHANGE` (range and texts), else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        let context = self.context.context();
        if id == self.decoder && notification == CBN_SELCHANGE as u32 {
            let chosen = context.combo_index(self.decoder);
            if chosen == TURBO_JPEG_INDEX && !self.turbo_available && self.last_decoder != TURBO_JPEG_INDEX {
                context.set_combo_index(self.decoder, self.last_decoder);
            } else {
                self.last_decoder = chosen;
            }
            return true;
        }
        if (id == self.forward || id == self.backward) && notification == EN_CHANGE {
            self.update_range();
            self.update_ram_texts();
            return true;
        }
        false
    }

    /// `ram_slider` -> `update_ram_value()`.
    fn on_scroll(&mut self, id: i32) {
        if id == self.ram_slider {
            self.update_ram_value();
        }
    }

    fn on_language_changed(&mut self) {
        self.update_turbo_note();
        self.update_ram_texts();
    }

    /// `PreloadForward` -> forward box, `PreloadBackward` -> backward box, else `None`.
    fn focus_target(&self, field: SettingsFocusField) -> Option<i32> {
        match field {
            SettingsFocusField::PreloadForward => Some(self.forward),
            SettingsFocusField::PreloadBackward => Some(self.backward),
            _ => None,
        }
    }
}

impl PerformanceSettingsPage {
    fn update_range(&self) {
        let context = self.context.context();
        let forward = context.text(self.forward);
        let backward = context.text(self.backward);
        let (min, max) = context
            .presenter()
            .borrow()
            .ram_cache_range(Some(&forward), Some(&backward));
        context.set_slider_range(self.ram_slider, min, max);
    }

    fn update_ram_texts(&self) {
        let context = self.context.context();
        let forward = context.text(self.forward);
        let backward = context.text(self.backward);
        let (min, max) = context
            .presenter()
            .borrow()
            .ram_cache_range(Some(&forward), Some(&backward));
        let hint = with_field_context(|field| context.presenter().borrow().ram_cache_hint_text(min, max, field));
        context.set_text(self.ram_hint, &hint);
        self.update_ram_value();
        context.relayout();
    }

    fn update_ram_value(&self) {
        let context = self.context.context();
        let value = context.slider_value(self.ram_slider);
        let text = with_field_context(|field| context.presenter().borrow().ram_cache_value_text(value, field));
        context.set_text(self.ram_value, &text);
    }

    fn update_turbo_note(&self) {
        if !self.turbo_available {
            self.context
                .context()
                .set_text(self.turbo_note, &tr::settings_decoder_backend_turbo_jpeg_unavailable());
        }
    }
}
