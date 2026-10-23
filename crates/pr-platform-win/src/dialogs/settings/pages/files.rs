//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/FilesSettingsPage.cs` (223 lines, baseline `d92d6aae`): the "Files & safety" page (WPF `FilesPage`):
//! duplicate compare, camera RAW, WebP/HEIC with the machine codec status, delete safety, external editor path with Browse, operation journal (two radios),
//! "reuse the last Move/Copy folder". Unit W8-19f. Contract: `contracts/W8-19f.md`.
//!
//! The C# HEAD (after the baseline) adds the external editors list ("Manage editors..."): NOT ported here (sync patch P-17), see the contract.
use std::rc::Rc;

use pr_app::presenters::settings::SettingsPresenter;
use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_imaging::webp_heic::WicCodecSupport;
use pr_l10n::tr;

use crate::dialogs::action_profiles::FileFilterText;
use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::SettingsPageContext;
use crate::dialogs::settings::context::with_field_context;
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView, text_fn};
use crate::win32::dialog_native::BN_CLICKED;

/// C# `Func<WicCodecSupport>` constructor parameter: where the codec status comes from (tests pass a fake).
pub type CodecSource = Rc<dyn Fn() -> WicCodecSupport>;

/// C# `Ids` (internal, for tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilesIds {
    pub compare_hash: i32,
    pub compare_size: i32,
    pub raw_support: i32,
    pub raw_full_decode: i32,
    pub raw_pair_mode: i32,
    pub webp_heic: i32,
    pub codec_status: i32,
    pub allow_permanent_delete: i32,
    pub confirm_before_delete: i32,
    pub editor_path: i32,
    pub browse: i32,
    pub journal: i32,
    pub reuse_last_folder: i32,
}

/// C# `FilesSettingsPage`.
pub struct FilesSettingsPage {
    codecs: CodecSource,
    context: PageContextSlot,
    ids: FilesIds,
    journal_radios: Vec<i32>,
}

impl std::fmt::Debug for FilesSettingsPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilesSettingsPage")
            .field("ids", &self.ids)
            .finish_non_exhaustive()
    }
}

impl Default for FilesSettingsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesSettingsPage {
    /// C# `new FilesSettingsPage()`: the codec source is the process-wide probe `crate::wic::availability::current()` (cloned).
    pub fn new() -> Self {
        Self::with_codecs(Rc::new(|| crate::wic::availability::current().clone()))
    }

    /// C# `new FilesSettingsPage(codecs)`: tests pass a fake source.
    pub fn with_codecs(codecs: CodecSource) -> Self {
        Self {
            codecs,
            context: PageContextSlot::new(),
            ids: FilesIds {
                compare_hash: 0,
                compare_size: 0,
                raw_support: 0,
                raw_full_decode: 0,
                raw_pair_mode: 0,
                webp_heic: 0,
                codec_status: 0,
                allow_permanent_delete: 0,
                confirm_before_delete: 0,
                editor_path: 0,
                browse: 0,
                journal: 0,
                reuse_last_folder: 0,
            },
            journal_radios: Vec::new(),
        }
    }

    /// C# `Ids`.
    pub fn ids(&self) -> FilesIds {
        self.ids
    }

    /// C# `JournalRadios`: 2 ids after `declare` (0 = Fast, 1 = Power-loss safe).
    pub fn journal_radios(&self) -> &[i32] {
        &self.journal_radios
    }
}

/// C# `FilesSettingsPage.InitialFolder(path)`: `None` for a blank path, for a path that is not an existing FILE, and for any I/O error; else the file's directory.
pub fn initial_folder(path: &str) -> Option<String> {
    if path.trim().is_empty() {
        return None;
    }
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    std::path::Path::new(path)
        .parent()
        .map(|parent| parent.to_string_lossy().replace('/', "\\"))
}

impl SettingsPageView for FilesSettingsPage {
    /// `"Files"`.
    fn key(&self) -> &str {
        "Files"
    }

    /// Six groups and two loose checks, exactly the C# order and texts (see the contract table).
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        let ids = &mut self.ids;
        page.group(text_fn(tr::settings_group_compare_options), |group| {
            ids.compare_hash = group.check(text_fn(tr::settings_compare_hash_label), 0);
            ids.compare_size = group.check(text_fn(tr::settings_compare_size_label), 0);
        });
        page.group(text_fn(tr::settings_group_raw), |group| {
            ids.raw_support = group.check(text_fn(tr::settings_raw_support_enabled_label), 0);
            group.note(Some(text_fn(tr::settings_raw_support_enabled_hint)), true);
            ids.raw_full_decode = group
                .combo(
                    text_fn(tr::settings_raw_full_decode_label),
                    Some(vec![
                        text_fn(tr::enum_raw_full_decode_never),
                        text_fn(tr::enum_raw_full_decode_on_zoom),
                    ]),
                    0,
                )
                .control;
            group.note(Some(text_fn(tr::settings_raw_full_decode_hint)), true);
            ids.raw_pair_mode = group
                .combo(
                    text_fn(tr::settings_raw_pair_mode_label),
                    Some(vec![
                        text_fn(tr::enum_raw_pair_mode_separate),
                        text_fn(tr::enum_raw_pair_mode_prefer_jpeg),
                        text_fn(tr::enum_raw_pair_mode_prefer_raw),
                    ]),
                    0,
                )
                .control;
            group.note(Some(text_fn(tr::settings_raw_pair_mode_hint)), true);
            group.note(Some(text_fn(tr::settings_raw_pair_mode_shortcut_hint)), true);
        });
        page.group(text_fn(tr::settings_group_webp_heic), |group| {
            ids.webp_heic = group.check(text_fn(tr::settings_webp_heic_support_enabled_label), 0);
            group.note(Some(text_fn(tr::settings_webp_heic_support_enabled_hint)), false);
            ids.codec_status = group.note(None, false);
        });
        ids.allow_permanent_delete = page.check(text_fn(tr::settings_allow_permanent_delete_label), 0);
        page.note(Some(text_fn(tr::settings_allow_permanent_delete_hint)), false);
        ids.confirm_before_delete = page.check(text_fn(tr::settings_confirm_before_delete_label), 0);
        page.note(Some(text_fn(tr::settings_confirm_before_delete_hint)), false);
        page.group(text_fn(tr::settings_group_external_editor), |group| {
            ids.editor_path = group
                .edit(text_fn(tr::settings_external_editor_path_label), 260)
                .control;
            ids.browse = group
                .buttons(true, &[text_fn(tr::settings_external_editor_path_browse)])
                .first()
                .copied()
                .unwrap_or(0);
            group.note(Some(text_fn(tr::settings_external_editor_path_hint)), false);
        });
        page.group(text_fn(tr::settings_group_journal_durability), |group| {
            ids.journal = group.text(Some(text_fn(tr::settings_journal_durability_label)));
            self.journal_radios = group.radio(
                &[
                    text_fn(tr::settings_journal_durability_fast),
                    text_fn(tr::settings_journal_durability_safe),
                ],
                Some(&[
                    Some(text_fn(tr::settings_journal_durability_fast_hint)),
                    Some(text_fn(tr::settings_journal_durability_safe_hint)),
                ]),
                0,
            );
        });
        page.group(text_fn(tr::settings_group_actions), |group| {
            ids.reuse_last_folder = group.check(text_fn(tr::settings_move_copy_reuse_last_folder_label), 0);
            group.note(Some(text_fn(tr::settings_move_copy_reuse_last_folder_hint)), false);
        });
    }

    /// Stores the context, then `show_codec_status()`.
    fn attach(&mut self, context: SettingsPageContext) {
        self.context.attach(context);
        self.show_codec_status();
    }

    fn load(&mut self, state: &SettingsFormState) {
        let context = self.context.context();
        context.set_checked(self.ids.compare_hash, state.compare_hash_enabled);
        context.set_checked(self.ids.compare_size, state.compare_size_enabled);
        context.set_checked(self.ids.raw_support, state.raw_support_enabled);
        context.set_combo_index(self.ids.raw_full_decode, state.raw_full_decode_index);
        context.set_combo_index(self.ids.raw_pair_mode, state.raw_pair_mode_index);
        context.set_checked(self.ids.webp_heic, state.webp_heic_support_enabled);
        context.set_checked(self.ids.allow_permanent_delete, state.allow_permanent_delete);
        context.set_checked(self.ids.confirm_before_delete, state.confirm_before_delete);
        context.set_text(self.ids.editor_path, &state.external_editor_path);
        context.set_radio_index(&self.journal_radios, i32::from(state.journal_safe));
        context.set_checked(self.ids.reuse_last_folder, state.move_copy_reuse_last_folder);
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        let context = self.context.context();
        state.compare_hash_enabled = context.is_checked(self.ids.compare_hash);
        state.compare_size_enabled = context.is_checked(self.ids.compare_size);
        state.raw_support_enabled = context.is_checked(self.ids.raw_support);
        state.raw_full_decode_index = context.combo_index(self.ids.raw_full_decode);
        state.raw_pair_mode_index = context.combo_index(self.ids.raw_pair_mode);
        state.webp_heic_support_enabled = context.is_checked(self.ids.webp_heic);
        state.allow_permanent_delete = context.is_checked(self.ids.allow_permanent_delete);
        state.confirm_before_delete = context.is_checked(self.ids.confirm_before_delete);
        state.external_editor_path = context.text(self.ids.editor_path);
        state.journal_safe = context.radio_index(&self.journal_radios) == 1;
        state.move_copy_reuse_last_folder = context.is_checked(self.ids.reuse_last_folder);
    }

    /// `BN_CLICKED` of `browse` -> `browse_for_editor()`, handled; else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        if id != self.ids.browse || notification != BN_CLICKED {
            return false;
        }
        self.browse_for_editor();
        true
    }

    /// `set_focus(editor_path)`, `true`.
    fn focus_external_editor(&mut self) -> bool {
        self.context.context().focus(self.ids.editor_path);
        true
    }

    fn on_language_changed(&mut self) {
        self.show_codec_status()
    }
}

impl FilesSettingsPage {
    fn browse_for_editor(&mut self) {
        let context = self.context.context();
        let Some(files) = context.files() else {
            return;
        };
        let title = tr::settings_external_editor_path_label();
        let filters = FileFilterText::parse(&tr::dialog_file_filter_executable());
        let current_path = context.text(self.ids.editor_path);
        let initial = initial_folder(&current_path);
        match files.pick_open_file(context.owner(), &title, &filters, initial.as_deref()) {
            Err(error) => context.show_warning(&tr::dialog_settings_open_failed_title(), &error.to_string()),
            Ok(Some(path)) if !path.is_empty() => context.set_text(self.ids.editor_path, &path),
            Ok(Some(_)) | Ok(None) => {}
        }
    }

    fn show_codec_status(&self) {
        let codecs = (self.codecs)();
        let status = with_field_context(|field_context| {
            SettingsPresenter::format_webp_heic_codec_status(&codecs, field_context)
        });
        let context = self.context.context();
        context.set_text(self.ids.codec_status, &status);
        context.relayout();
    }
}
