//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/FilesSettingsPage.cs` (223 lines, baseline `d92d6aae`): the "Files & safety" page (WPF `FilesPage`):
//! duplicate compare, camera RAW, WebP/HEIC with the machine codec status, delete safety, external editor path with Browse, operation journal (two radios),
//! "reuse the last Move/Copy folder". Unit W8-19f. Contract: `contracts/W8-19f.md`.
//!
//! The C# HEAD (after the baseline) adds the external editors list ("Manage editors..."): NOT ported here (sync patch P-17), see the contract.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::rc::Rc;

use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_imaging::webp_heic::WicCodecSupport;

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::SettingsPageContext;
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView};

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
        todo!()
    }

    /// C# `new FilesSettingsPage(codecs)`: tests pass a fake source.
    pub fn with_codecs(codecs: CodecSource) -> Self {
        todo!()
    }

    /// C# `Ids`.
    pub fn ids(&self) -> FilesIds {
        todo!()
    }

    /// C# `JournalRadios`: 2 ids after `declare` (0 = Fast, 1 = Power-loss safe).
    pub fn journal_radios(&self) -> &[i32] {
        todo!()
    }
}

/// C# `FilesSettingsPage.InitialFolder(path)`: `None` for a blank path, for a path that is not an existing FILE, and for any I/O error; else the file's directory.
pub fn initial_folder(path: &str) -> Option<String> {
    todo!()
}

impl SettingsPageView for FilesSettingsPage {
    /// `"Files"`.
    fn key(&self) -> &str {
        todo!()
    }

    /// Six groups and two loose checks, exactly the C# order and texts (see the contract table).
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        todo!()
    }

    /// Stores the context, then `show_codec_status()`.
    fn attach(&mut self, context: SettingsPageContext) {
        todo!()
    }

    fn load(&mut self, state: &SettingsFormState) {
        todo!()
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        todo!()
    }

    /// `BN_CLICKED` of `browse` -> `browse_for_editor()`, handled; else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        todo!()
    }

    /// `set_focus(editor_path)`, `true`.
    fn focus_external_editor(&mut self) -> bool {
        todo!()
    }

    fn on_language_changed(&mut self) {
        todo!()
    }
}
