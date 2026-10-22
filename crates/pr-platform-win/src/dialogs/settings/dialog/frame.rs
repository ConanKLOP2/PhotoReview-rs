//! Port of `SettingsDialog.cs` lines 1 to 332 (options record, ids, fields, constructor, properties, `BuildItems`, `Flatten`, `OnInitialized`,
//! `FocusExternalEditor`; `EnsurePageCreated`, `EnsureAllPagesCreated` and `IsPageCreated` are in `create.rs`), baseline `d92d6aae`. Unit W8-19j. Contract: `contracts/W8-19j.md`.
//!
//! This file is the spine of the Settings window and the ONLY place that declares its state (precedent D-W8B4M-1, D-W8B5-302): [`SettingsContent`] holds every C#
//! field, the `impl DialogContent` below routes each C# `override` to the unit that owns it, and [`SettingsDialog`] is the handle with one forwarding method per
//! `internal` C# member (so no other unit edits this file). Shape and reasons: D-W8B5-901 .. D-W8B5-905.
//!
//! Rules every unit of this directory follows (D-W8B5-904, D-W8B5-905):
//! - a `RefCell` borrow is NEVER held across an OS call, a page call, a hook or a callback: copy what is needed, release, then act;
//! - a page is reached with `try_borrow_mut`; a page that is already borrowed (it is the one running) is skipped, never panics;
//! - the content never stores a `NativeDialog` except in `link` (set by `on_initialized`, taken by `on_closed`, so the `Rc` cycle ends with the window).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::presenters::settings::{SettingsPresenter, SettingsPresenterOptions};
use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_core::abstractions::SettingsTarget;
use pr_core::abstractions::ShellIntegration;
use pr_core::settings::app_settings::{AppSettings, SettingsTexts};
use pr_core::settings::shortcuts::ReviewAction;
use pr_imaging::decoder_traits::ImageDecoderFactory;
use pr_l10n::localizer::CurrentChangedSubscription;

use super::messages::{LocalAction, WindowPoster};
use crate::dialogs::action_profiles::DialogWarningSink;
use crate::dialogs::file_dialog::ShellFileDialog;
use crate::dialogs::native::chrome::NativeDialogChrome;
use crate::dialogs::native::dialog::{DialogContent, DialogError, NativeDialog};
use crate::dialogs::native::template::DialogItem;
use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::{
    ActionProfilesUnavailable, ContextUiAction, SettingsPageContext, SharedSettingsPresenter,
};
use crate::dialogs::settings::page_view::{SettingsPageView, TextFn};
use crate::host::message_loop::MessageLoop;
use crate::win32::structs::{Hgdi, Hwnd, Lparam, Lresult, Msg, Wparam};

// ---- ids of the frame (C# lines 54 to 76; a page control id is `1000 * (page index + 1) + n`) -------------------------------------------------------------

/// C# `HeaderId`: the "Settings" heading above the navigation list (drawn with the header font).
pub const HEADER_ID: i32 = 100;
/// C# `NavId`: the owner-draw navigation list box.
pub const NAV_ID: i32 = 101;
/// C# `VersionId`: the version text under the navigation list.
pub const VERSION_ID: i32 = 102;
/// C# `PageTitleId`: the title of the showing page (header font).
pub const PAGE_TITLE_ID: i32 = 103;
/// C# `ScrollId`: the vertical scroll bar of the page area.
pub const SCROLL_ID: i32 = 104;
/// C# `ViewportBase`: the page container of page `i` has id `110 + i`.
pub const VIEWPORT_BASE: i32 = 110;
/// C# `ApplyId`.
pub const APPLY_ID: i32 = 120;
/// C# `DefaultsId` ("Restore defaults").
pub const DEFAULTS_ID: i32 = 121;
/// C# `ResetPageId` ("Reset this page").
pub const RESET_PAGE_ID: i32 = 122;
/// C# `ExportId`.
pub const EXPORT_ID: i32 = 123;
/// C# `ImportId`.
pub const IMPORT_ID: i32 = 124;
/// C# `SaveId` = `IdOk` (the default button).
pub const SAVE_ID: i32 = crate::dialogs::native::dialog::ID_OK;
/// C# `CancelId` = `IdCancel`.
pub const CANCEL_ID: i32 = crate::dialogs::native::dialog::ID_CANCEL;
/// C# `PageIdStride`: the id space of one page.
pub const PAGE_ID_STRIDE: i32 = 1000;
/// C# `WmAppLanguage` = `WM_APP + 0x35`: posted when the ambient language changed (any thread).
pub const WM_APP_LANGUAGE: u32 = 0x8000 + 0x35;
/// C# `WmAppQueue` = `WM_APP + 0x36`: posted when work was queued for the UI thread of the window.
pub const WM_APP_QUEUE: u32 = 0x8000 + 0x36;
/// C# `ForwardAllSubclassId` (`"ST35"`): the subclass id of the page containers (every child message is forwarded to the dialog).
pub const FORWARD_ALL_SUBCLASS_ID: usize = 0x5354_3335;
/// C# `WheelSubclassId` (`"WH35"`): the subclass id of the navigation list and of the page comboboxes (wheel only; the list is a direct child of the dialog, so its
/// `WM_COMMAND` needs no forwarding).
pub const WHEEL_SUBCLASS_ID: usize = 0x5748_3335;
/// C# `KeyBoxSubclassId` (`"KB35"`): the subclass id of a key capture box.
pub const KEY_BOX_SUBCLASS_ID: usize = 0x4B42_3335;

/// C# `ViewportId(pageIndex)`: `VIEWPORT_BASE + page_index` (wrapping).
pub fn viewport_id(page_index: i32) -> i32 {
    todo!()
}

/// C# `PageIdBase(pageIndex)`: `PAGE_ID_STRIDE * (page_index + 1)` (wrapping). The first control id of the page is this value plus 1.
pub fn page_id_base(page_index: i32) -> i32 {
    todo!()
}

// ---- options and arguments ---------------------------------------------------------------------------------------------------------------------------

/// The page-owned list of a created window: a page is shared between the frame and its own callbacks (D-W8B5-706).
pub type PageHandle = Rc<RefCell<Box<dyn SettingsPageView>>>;

/// C# `SettingsDialogOptions.OpenUrl`: opens a URL; `Err(reason)` when it cannot (C# threw).
pub type OpenUrlFn = Rc<dyn Fn(&str) -> Result<(), String>>;

/// C# `SettingsDialogOptions.StartExplorer`: starts Explorer with the arguments; `Some(reason)` = failed.
pub type StartExplorerFn = Rc<dyn Fn(&str) -> Option<String>>;

/// C# `SettingsDialogOptions.ShowActionProfiles`: opens the Action profiles editor MODAL over the Settings window (owner, current list) -> the edited list, `Ok(None)` when
/// cancelled. `Err(ActionProfilesUnavailable)` = the editor could not be opened (C# `throw`).
pub type ShowActionProfilesFn =
    Rc<dyn Fn(Hwnd, &[ReviewAction]) -> Result<Option<Vec<ReviewAction>>, ActionProfilesUnavailable>>;

/// C# `SettingsDialogOptions` (`null` member = the real thing). Differences: `Pages` are MOVED in; `OpenUrl` returns the failure text instead of throwing; `PhysicalMemoryBytes`
/// is `SettingsDialogArgs::presenter.physical_memory_bytes`; `Version` has no assembly to read (the composition root passes it, D-W8B5-903).
#[derive(Default)]
pub struct SettingsDialogOptions {
    /// Open/Save dialog (Import, Export, the exe picker of the Files page). `None` = the buttons do nothing.
    pub files: Option<Rc<dyn ShellFileDialog>>,
    /// Modal warnings (a task dialog); tests pass a recorder. `None` = `TaskDialogWarningSink::instance()`.
    pub warnings: Option<Rc<dyn DialogWarningSink>>,
    /// `SettingsTarget::ExternalEditor` opens the Files page and focuses the editor path box (the remembered page is not changed).
    pub target: SettingsTarget,
    /// The pages instead of `catalog::create_pages()` (tests); must be in `PAGES` order and as many as `PAGES`.
    pub pages: Option<Vec<Box<dyn SettingsPageView>>>,
    /// Opens a URL (the update page). `None` = ShellExecute (`host::default_open_url`).
    pub open_url: Option<OpenUrlFn>,
    /// Starts Explorer with arguments; `Some(reason)` = failed. `None` = `SettingsPresenter::try_start_explorer`.
    pub start_explorer: Option<StartExplorerFn>,
    /// The version text under the navigation list and the version of the update check. `None` = empty text.
    pub version: Option<String>,
    /// Is Ctrl down (tests simulate Ctrl+Tab)? `None` = `GetKeyState(VK_CONTROL) < 0`.
    pub control_key_down: Option<Rc<dyn Fn() -> bool>>,
    /// Nested Action profiles editor; `None` = the Shortcuts page says "could not open the editor".
    pub show_action_profiles: Option<ShowActionProfilesFn>,
}

impl std::fmt::Debug for SettingsDialogOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsDialogOptions").finish_non_exhaustive()
    }
}

/// The C# constructor parameters `(AppSettings current, SettingsStore? store, IImageDecoderFactory? decoderFactory, LocalizationService? localization, IUpdateChecker?
/// updateChecker, SettingsDialogOptions? options, INativeDialogChrome? chrome)`: the store, localization, update checker, shell integration and RAM size are parts of
/// `presenter` (D-W8B5-903).
pub struct SettingsDialogArgs {
    /// The settings being edited (the presenter clones them).
    pub current: AppSettings,
    /// What the presenter needs (`SettingsPresenterOptions::new(..)` plus `store`, `localization`, `update_checker`, `shell_integration`, `executable_path`).
    pub presenter: SettingsPresenterOptions,
    /// Decoder factory the Performance page asks about TurboJpeg; `None` = unknown.
    pub decoder_factory: Option<Arc<dyn ImageDecoderFactory>>,
    /// Seams.
    pub options: SettingsDialogOptions,
    /// `None` = `default_chrome()` of the native base.
    pub chrome: Option<Rc<dyn NativeDialogChrome>>,
}

impl std::fmt::Debug for SettingsDialogArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsDialogArgs").finish_non_exhaustive()
    }
}

/// What `SettingsDialog::new` can refuse (the C# `ArgumentException`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsDialogError {
    /// C# `ArgumentException("A settings page is required for every SettingsPresenter.Pages entry.", "options")`.
    PageCountMismatch {
        /// `PAGES.len()` (7).
        expected: usize,
        /// The number of pages given in `SettingsDialogOptions::pages`.
        actual: usize,
    },
}

impl fmt::Display for SettingsDialogError {
    /// `PageCountMismatch`: `"A settings page is required for every SettingsPresenter.Pages entry (expected {expected}, got {actual})."`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for SettingsDialogError {}

// ---- state ---------------------------------------------------------------------------------------------------------------------------------------------

/// The `SettingsDialogOptions` members the content keeps (everything but `pages` and `warnings`, which have their own fields).
pub(super) struct ContentOptions {
    pub(super) files: Option<Rc<dyn ShellFileDialog>>,
    pub(super) target: SettingsTarget,
    pub(super) open_url: Option<OpenUrlFn>,
    pub(super) start_explorer: Option<StartExplorerFn>,
    pub(super) version: Option<String>,
    pub(super) control_key_down: Option<Rc<dyn Fn() -> bool>>,
    pub(super) show_action_profiles: Option<ShowActionProfilesFn>,
}

/// What `reset_active_page` needs to build the throw-away presenter of the C# (`new SettingsPresenter(Settings, store: null, localization: null)`): the pieces of the real
/// presenter's options that are not readable back from the presenter (D-W8B5-906). Captured by `SettingsContent::new` BEFORE the options move into the presenter.
#[derive(Clone)]
pub(super) struct ScratchSeed {
    pub(super) physical_memory_bytes: i64,
    pub(super) settings_texts: Arc<dyn SettingsTexts>,
    pub(super) shell_integration: Arc<dyn ShellIntegration>,
}

impl ScratchSeed {
    /// `SettingsPresenter::new(settings, SettingsPresenterOptions { store: None, localization: None, update_checker: None, ..seed })`.
    pub(super) fn presenter(&self, settings: &AppSettings) -> SettingsPresenter {
        todo!()
    }
}

/// C# `_comboItems`, `_notes`, `_keyBoxes`, `_warningTexts`: what `BuildItems` merges from the page builders (ids are unique across pages).
#[derive(Default)]
pub(super) struct DeclaredSets {
    pub(super) combo_items: BTreeMap<i32, Vec<TextFn>>,
    pub(super) notes: BTreeSet<i32>,
    pub(super) key_boxes: BTreeSet<i32>,
    pub(super) warning_texts: BTreeSet<i32>,
}

/// The per-page arrays of the C# (`_scroll`, `_contentHeight`, `_created`). The C# `_dirty` array is write-only in the baseline and is NOT ported (D-W8B5-907).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PageView {
    pub(super) scroll: i32,
    pub(super) content_height: i32,
    pub(super) created: bool,
}

/// The mutable window state (C# `_active`, `_saved`, `_loading`, `_attached`, `_inLanguageRefresh`, `_viewportWidth`, `_viewportHeight`, `_hidden`, `_placed`,
/// `_pageCombos`, `_actionsJson`, `_keyBoxBrush`). `Cell`/`RefCell` only.
pub(super) struct ViewState {
    /// C# `_active`: the showing page, `-1` before the first activation.
    pub(super) active: Cell<i32>,
    /// C# `_saved`.
    pub(super) saved: Cell<bool>,
    /// C# `_loading`: a page is being loaded; command notifications are swallowed.
    pub(super) loading: Cell<bool>,
    /// C# `_attached`: set by `on_initialized`, gates `layout_page`.
    pub(super) attached: Cell<bool>,
    /// C# `_inLanguageRefresh`.
    pub(super) in_language_refresh: Cell<bool>,
    /// C# `_viewportWidth`.
    pub(super) viewport_width: Cell<i32>,
    /// C# `_viewportHeight`.
    pub(super) viewport_height: Cell<i32>,
    /// One entry per page.
    pub(super) pages: RefCell<Vec<PageView>>,
    /// C# `_hidden`: control ids hidden by their page (group ids live in the builders, D-W8B5-713).
    pub(super) hidden: RefCell<BTreeSet<i32>>,
    /// C# `_placed`: the last rectangle `(x, y, w, h)` given to each page control.
    pub(super) placed: RefCell<HashMap<Hwnd, (i32, i32, i32, i32)>>,
    /// C# `_pageCombos`.
    pub(super) page_combos: RefCell<HashSet<Hwnd>>,
    /// C# `_actionsJson`.
    pub(super) actions_json: RefCell<Option<String>>,
    /// C# `_keyBoxBrush` (`0` = none yet).
    pub(super) key_box_brush: Cell<Hgdi>,
}

/// What [`SettingsContent::build_items`] produces.
pub(super) struct Declaration {
    /// C# `_items`.
    pub(super) items: Vec<DialogItem>,
    /// C# `_builders`, one per page.
    pub(super) builders: Vec<SettingsPageBuilder>,
    /// The merged sets.
    pub(super) declared: DeclaredSets,
}

/// C# `SettingsDialog` (without the base class): the `DialogContent` of the Settings window. Not `Send`. Created by [`SettingsDialog::new`].
pub struct SettingsContent {
    /// C# `_presenter`.
    pub(super) presenter: SharedSettingsPresenter,
    pub(super) options: ContentOptions,
    /// See [`ScratchSeed`].
    pub(super) scratch: ScratchSeed,
    /// C# `DecoderFactory`.
    pub(super) decoder_factory: Option<Arc<dyn ImageDecoderFactory>>,
    /// C# `_warnings`.
    pub(super) warnings: Rc<dyn DialogWarningSink>,
    /// C# `_pages`.
    pub(super) pages: Vec<PageHandle>,
    /// C# `_builders`: one per page, after `declare`.
    pub(super) builders: Vec<RefCell<SettingsPageBuilder>>,
    /// C# `_items`: the template items of the frame (the page controls are NOT in it: they are created on first use).
    pub(super) items: Vec<DialogItem>,
    pub(super) declared: DeclaredSets,
    /// C# `_context`: over the weak adapter `host::PageHost`.
    pub(super) context: SettingsPageContext,
    /// The `Send + Sync` poster of C# `PostToUi` (the cross-thread queue and the window handle).
    pub(super) poster: Arc<WindowPoster>,
    /// UI-thread-only work posted with `messages::post_local` (the C# `PostToUi(() => FocusExternalEditor())`).
    pub(super) local_queue: RefCell<VecDeque<LocalAction>>,
    /// The key of this content in the subclass registry (`register_subclass_key`/`from_subclass_key`); `0` = not registered.
    pub(super) subclass_key: Cell<usize>,
    /// The live dialog handle between `on_initialized` and `on_closed` (D-W8B5-901).
    pub(super) link: RefCell<Option<NativeDialog>>,
    /// C# `Localizer.CurrentChanged += OnLocalizerChanged` (dropping it unsubscribes).
    pub(super) subscription: RefCell<Option<CurrentChangedSubscription>>,
    pub(super) view: ViewState,
}

impl std::fmt::Debug for SettingsContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsContent").finish_non_exhaustive()
    }
}

impl SettingsContent {
    /// C# constructor body (lines 119 to 144) without the base call: presenter from `args.presenter` and `args.current`; `warnings` (default `TaskDialogWarningSink::instance()`);
    /// the pages (`options.pages` or `catalog::create_pages()`), `Err(PageCountMismatch)` when their number differs from `PAGES.len()`; `build_items(..)`; the context over a
    /// `PageHost` that holds a `Weak` (`Rc::new_cyclic`); `register_subclass_key()`. Nothing touches the OS.
    pub(super) fn new(args: SettingsDialogArgs) -> Result<Rc<SettingsContent>, SettingsDialogError> {
        todo!()
    }

    /// C# `BuildItems` (lines 200 to 252) and `Flatten`, as a PURE function (no window, no content yet): `items` = header (`Static`, `tr::settings_header`), navigation list
    /// (`ListBox`), version (`Static`, `version` or `""`), page title (`Static`), scroll bar, one viewport `Static` per page, then the buttons Defaults, Reset page, Export,
    /// Import, Cancel, Apply, Save in this order (styles in the contract); and, per page in order, the page is `declare`d on `SettingsPageBuilder::new(page_id_base(i))`.
    /// A builder with `error().is_some()` (D-W8B5-704) makes this function replace that page (inside its `RefCell`) by `PendingSettingsPage::new(key)` declared on a fresh
    /// builder. The sets of the builders are merged into `declared`.
    pub(super) fn build_items(pages: &[PageHandle], version: Option<&str>) -> Declaration {
        todo!()
    }

    /// C# `OnInitialized` (lines 271 to 305): hides and registers the viewports, subscribes the language event, fills the navigation list, selects the initial page, sets
    /// `attached`, activates the page without layout, posts the external-editor focus. Stores `link` and the window in the poster.
    pub(super) fn initialize(&self, dialog: &NativeDialog) {
        todo!()
    }

    /// C# `FocusExternalEditor` (lines 307 to 331): the Files page creates its controls, becomes the showing page (without remembering it) and focuses its editor path box;
    /// `false` when no page handles it.
    pub fn focus_external_editor(&self, dialog: &NativeDialog) -> bool {
        todo!()
    }

    /// The live dialog (`None` before `on_initialized` and after `on_closed`).
    pub(super) fn native(&self) -> Option<NativeDialog> {
        todo!()
    }

    /// The window handle of the live dialog; `0` when there is none.
    pub(super) fn hwnd(&self) -> Hwnd {
        todo!()
    }

    /// Runs `f` with the live dialog; `None` (and `f` is not called) when the window does not exist. Used by the weak adapter. The handle is CLONED out of `link` and the
    /// `link` borrow is released BEFORE `f` runs (`f` re-enters the dialog procedure; a held borrow would make the next `hwnd()` or `with_native` panic).
    pub(super) fn with_native<R>(&self, f: impl FnOnce(&NativeDialog) -> R) -> Option<R> {
        todo!()
    }

    /// `pages[index]` for `index` in range (`None` otherwise, never panics).
    pub(super) fn page(&self, index: i32) -> Option<PageHandle> {
        todo!()
    }

    /// Registers this content in the subclass registry (a `thread_local!` map from a non-zero counter value to a `Weak<SettingsContent>`) and stores the key in `subclass_key`.
    /// Called once at the end of `new`. The key is the `ref_data` of every subclass procedure of this window (D-W8B5-908); a pointer is never used.
    pub(super) fn register_subclass_key(self: &Rc<Self>) {
        todo!()
    }

    /// The content registered under `key`, `None` when the key is unknown or the window is closed (the procedure then passes the message on).
    pub(super) fn from_subclass_key(key: usize) -> Option<Rc<SettingsContent>> {
        todo!()
    }

    /// Removes the registry entry and zeroes `subclass_key` (called by `close::on_closed` and by `Drop`; idempotent: a second call, or a call after the registry was torn down
    /// at thread exit, does nothing and never panics).
    pub(super) fn forget_subclass_key(&self) {
        todo!()
    }
}

impl Drop for SettingsContent {
    /// A content that is dropped without ever being shown (or whose window closed without `on_closed` reaching `forget_subclass_key`) must not leave its dead `Weak` in the
    /// thread-local subclass registry: `forget_subclass_key()` (idempotent, never panics; the registry is reached with `try_with` because the thread may be exiting).
    fn drop(&mut self) {
        self.forget_subclass_key();
    }
}

impl DialogContent for SettingsContent {
    /// C# `Title` = `tr::settings_window_title()`.
    fn title(&self) -> String {
        todo!()
    }

    /// C# `Items`: a clone of `items`.
    fn items(&self) -> Vec<DialogItem> {
        todo!()
    }

    /// C# `Resizable` = `true`.
    fn resizable(&self) -> bool {
        todo!()
    }

    /// C# `HeaderControlIds` = `[HEADER_ID, PAGE_TITLE_ID]`.
    fn header_control_ids(&self) -> Vec<i32> {
        todo!()
    }

    /// C# `InitialFocusId` = `NAV_ID`.
    fn initial_focus_id(&self) -> i32 {
        todo!()
    }

    /// C# `DefaultButtonId` = `SAVE_ID`.
    fn default_button_id(&self) -> i32 {
        todo!()
    }

    /// C# `PreferredClientSize` -> `footer` (19m).
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        todo!()
    }

    /// C# `MinimumClientSize` -> `footer` (19m).
    fn minimum_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        todo!()
    }

    /// C# `LayoutControls` -> `footer` (19m).
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32) {
        todo!()
    }

    /// C# `OnInitialized` -> [`SettingsContent::initialize`].
    fn on_initialized(&self, dialog: &NativeDialog) {
        todo!()
    }

    /// C# `OnOk` -> `apply::try_save_all` (19l).
    fn on_ok(&self, dialog: &NativeDialog) -> bool {
        todo!()
    }

    /// C# `OnCommand` -> `apply::handle_command` (19l).
    fn on_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        todo!()
    }

    /// C# `OnScroll` -> `page_layout::handle_scroll` (19m).
    fn on_scroll(&self, dialog: &NativeDialog, id: i32, code: i32) {
        todo!()
    }

    /// C# `OnWindowMessage` -> `messages::handle_window_message` (19l).
    fn on_window_message(
        &self,
        dialog: &NativeDialog,
        message: u32,
        wparam: Wparam,
        lparam: Lparam,
    ) -> Option<Lresult> {
        todo!()
    }

    /// C# `OnDialogKey` -> `nav::handle_dialog_key` (19j).
    fn on_dialog_key(&self, dialog: &NativeDialog, message: &Msg) -> bool {
        todo!()
    }

    /// C# `StaticTextColor` -> `surface::static_text_color` (19m).
    fn static_text_color(&self, dialog: &NativeDialog, id: i32) -> u32 {
        todo!()
    }

    /// C# `OnClosed` -> `close::on_closed` (19m).
    fn on_closed(&self, dialog: &NativeDialog) {
        todo!()
    }
}

// ---- the handle -----------------------------------------------------------------------------------------------------------------------------------------

/// The Settings window (C# `SettingsDialog`). Cheap to clone; every clone is the same window. Not `Send`. Show it once (`show_modal`).
#[derive(Clone)]
pub struct SettingsDialog {
    native: NativeDialog,
    content: Rc<SettingsContent>,
}

impl std::fmt::Debug for SettingsDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsDialog").finish_non_exhaustive()
    }
}

impl SettingsDialog {
    /// C# constructor: `SettingsContent::new(args)`, then `NativeDialog::with_chrome(content, chrome)`. No window yet.
    pub fn new(args: SettingsDialogArgs) -> Result<SettingsDialog, SettingsDialogError> {
        todo!()
    }

    /// The native base (for `hwnd`, `dpi`, `item`, test helpers).
    pub fn native(&self) -> &NativeDialog {
        &self.native
    }

    /// The content (the `Rc` the native base holds).
    pub fn content(&self) -> &Rc<SettingsContent> {
        &self.content
    }

    /// C# `ShowModal(owner, ready, loop)`: `ready` gets this handle. Returns `ID_OK` (Save) or `ID_CANCEL`.
    pub fn show_modal(
        &self,
        owner: Hwnd,
        ready: Option<&dyn Fn(&SettingsDialog)>,
        message_loop: Option<&Rc<MessageLoop>>,
    ) -> Result<i32, DialogError> {
        todo!()
    }

    /// C# `Close()` (= cancel).
    pub fn close(&self) {
        todo!()
    }

    /// C# `Hwnd`.
    pub fn hwnd(&self) -> Hwnd {
        todo!()
    }

    /// C# `IsClosed`.
    pub fn is_closed(&self) -> bool {
        todo!()
    }

    /// C# `Presenter` (test seam).
    pub fn presenter(&self) -> SharedSettingsPresenter {
        todo!()
    }

    /// C# `Settings`: a clone of the working copy; after a successful Save/Apply it is the saved one.
    pub fn settings(&self) -> AppSettings {
        todo!()
    }

    /// C# `Saved`: at least one Save/Apply succeeded (also when Cancel followed).
    pub fn saved(&self) -> bool {
        todo!()
    }

    /// C# `Pages.Count`.
    pub fn page_count(&self) -> usize {
        todo!()
    }

    /// C# `Pages[index].Key`; `None` out of range.
    pub fn page_key(&self, index: usize) -> Option<String> {
        todo!()
    }

    /// C# `ActivePageIndex`.
    pub fn active_page_index(&self) -> i32 {
        todo!()
    }

    /// C# `ActivePageKey` (`""` before the first activation).
    pub fn active_page_key(&self) -> String {
        todo!()
    }

    /// C# `ScrollOffset(index)`; `0` out of range.
    pub fn scroll_offset(&self, index: usize) -> i32 {
        todo!()
    }

    /// C# `ContentHeight(index)`; `0` out of range.
    pub fn content_height(&self, index: usize) -> i32 {
        todo!()
    }

    /// C# `ViewportHeight`.
    pub fn viewport_height(&self) -> i32 {
        todo!()
    }

    /// C# `PageItems(index)`: the declared items of the page in declaration order (= Tab order); empty out of range.
    pub fn page_items(&self, index: usize) -> Vec<DialogItem> {
        todo!()
    }

    /// C# `IsPageCreated(index)`.
    pub fn is_page_created(&self, index: usize) -> bool {
        todo!()
    }

    /// C# `FocusExternalEditor()`.
    pub fn focus_external_editor(&self) -> bool {
        todo!()
    }

    /// C# `EnsurePageCreated(index)`.
    pub fn ensure_page_created(&self, index: usize) {
        todo!()
    }

    /// C# `EnsureAllPagesCreated()`.
    pub fn ensure_all_pages_created(&self) {
        todo!()
    }

    /// C# `ActivatePage(index, remember, layout)`.
    pub fn activate_page(&self, index: i32, remember: bool, layout: bool) {
        todo!()
    }

    /// C# `CyclePage(forward)`.
    pub fn cycle_page(&self, forward: bool) {
        todo!()
    }

    /// C# `HandleKeyForTest(in Msg)`.
    pub fn handle_key_for_test(&self, message: &Msg) -> bool {
        todo!()
    }

    /// C# `IsKeyBox(hwnd)`.
    pub fn is_key_box(&self, hwnd: Hwnd) -> bool {
        todo!()
    }

    /// C# `IsKeyBoxId(id)`.
    pub fn is_key_box_id(&self, id: i32) -> bool {
        todo!()
    }

    /// C# `CaptureKey(hwnd, wParam, lParam)`.
    pub fn capture_key(&self, hwnd: Hwnd, wparam: Wparam, lparam: Lparam) -> bool {
        todo!()
    }

    /// C# `ActionsJson`.
    pub fn actions_json(&self) -> String {
        todo!()
    }

    /// C# `RefreshActionsJsonFromPages()`.
    pub fn refresh_actions_json_from_pages(&self) -> String {
        todo!()
    }

    /// C# `SetActionsJson(json, source)`; `source_key` is the key of the calling page (`""` = none).
    pub fn set_actions_json(&self, json: &str, source_key: &str) {
        todo!()
    }

    /// C# `CollectAll()`.
    pub fn collect_all(&self) -> SettingsFormState {
        todo!()
    }

    /// C# `TrySaveAll()`.
    pub fn try_save_all(&self) -> bool {
        todo!()
    }

    /// C# `ResetActivePage()`.
    pub fn reset_active_page(&self) {
        todo!()
    }

    /// C# `ImportSettingsFrom(path)`.
    pub fn import_settings_from(&self, path: &std::path::Path) {
        todo!()
    }

    /// C# `Warn(title, message)`.
    pub fn warn(&self, title: &str, message: &str) {
        todo!()
    }

    /// C# `PostToUi(action)` from any thread (through the `Send + Sync` poster).
    pub fn post_to_ui(&self, action: ContextUiAction) {
        todo!()
    }

    /// C# `RefreshLocalizedTexts()`.
    pub fn refresh_localized_texts(&self) {
        todo!()
    }

    /// C# `SetControlTooltip(id, Func<string>)`: the text is asked again each time the tip is shown.
    pub fn set_control_tooltip_fn(&self, id: i32, text: TextFn) {
        todo!()
    }

    /// C# `SetControlTooltip(id, string)`: a fixed text, set once.
    pub fn set_control_tooltip_text(&self, id: i32, text: &str) {
        todo!()
    }

    /// C# `GetControlText(id)`.
    pub fn control_text(&self, id: i32) -> String {
        todo!()
    }

    /// C# `SetControlText(id, text)`.
    pub fn set_control_text(&self, id: i32, text: &str) {
        todo!()
    }

    /// C# `IsControlChecked(id)`.
    pub fn is_control_checked(&self, id: i32) -> bool {
        todo!()
    }

    /// C# `SetControlChecked(id, value)`.
    pub fn set_control_checked(&self, id: i32, value: bool) {
        todo!()
    }

    /// C# `ComboSelectedIndex(id)`.
    pub fn combo_selected_index(&self, id: i32) -> i32 {
        todo!()
    }

    /// C# `SetComboSelectedIndex(id, index)`.
    pub fn set_combo_selected_index(&self, id: i32, index: i32) {
        todo!()
    }

    /// C# `SetComboItems(id, items, selected)`.
    pub fn set_combo_items(&self, id: i32, items: &[String], selected: i32) {
        todo!()
    }

    /// C# `SliderValue(id)`.
    pub fn slider_value(&self, id: i32) -> i32 {
        todo!()
    }

    /// C# `SetSliderValue(id, value)`.
    pub fn set_slider_value(&self, id: i32, value: i32) {
        todo!()
    }

    /// C# `SetSliderRange(id, min, max)`.
    pub fn set_slider_range(&self, id: i32, min: i32, max: i32) {
        todo!()
    }

    /// C# `SetControlEnabled(id, enabled)`.
    pub fn set_control_enabled(&self, id: i32, enabled: bool) {
        todo!()
    }

    /// C# `IsControlEnabled(id)`.
    pub fn is_control_enabled(&self, id: i32) -> bool {
        todo!()
    }

    /// C# `SetControlVisible(id, visible)`.
    pub fn set_control_visible(&self, id: i32, visible: bool) {
        todo!()
    }

    /// C# `IsControlVisible(id)`.
    pub fn is_control_visible(&self, id: i32) -> bool {
        todo!()
    }

    /// C# `FocusControl(id)`.
    pub fn focus_control(&self, id: i32) {
        todo!()
    }

    /// C# `RelayoutActivePage()`.
    pub fn relayout_active_page(&self) {
        todo!()
    }

    /// C# `EnsureVisible(id)`.
    pub fn ensure_visible(&self, id: i32) {
        todo!()
    }
}
