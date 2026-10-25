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
    ActionProfilesUnavailable, ContextUiAction, SettingsPageContext, SettingsUiPost, SharedSettingsPresenter,
};
use crate::dialogs::settings::page_view::{SettingsPageView, TextFn};
use crate::host::message_loop::MessageLoop;
use crate::win32::structs::{Hgdi, Hwnd, Lparam, Lresult, Msg, Wparam};

thread_local! {
    static SUBCLASS_REGISTRY: RefCell<HashMap<usize, std::rc::Weak<SettingsContent>>> = RefCell::new(HashMap::new());
    static NEXT_SUBCLASS_KEY: Cell<usize> = const { Cell::new(1) };
}

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
    VIEWPORT_BASE.wrapping_add(page_index)
}

/// C# `PageIdBase(pageIndex)`: `PAGE_ID_STRIDE * (page_index + 1)` (wrapping). The first control id of the page is this value plus 1.
pub fn page_id_base(page_index: i32) -> i32 {
    PAGE_ID_STRIDE.wrapping_mul(page_index.wrapping_add(1))
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
        match self {
            Self::PageCountMismatch { expected, actual } => write!(
                f,
                "A settings page is required for every SettingsPresenter.Pages entry (expected {expected}, got {actual})."
            ),
        }
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
        SettingsPresenter::new(
            settings,
            SettingsPresenterOptions {
                store: None,
                localization: None,
                update_checker: None,
                physical_memory_bytes: self.physical_memory_bytes,
                settings_texts: Arc::clone(&self.settings_texts),
                shell_integration: Arc::clone(&self.shell_integration),
                executable_path: None,
            },
        )
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
    /// Rust lifecycle guard: cleanup and page callbacks run at most once, including reentrant cleanup.
    pub(super) closed: Cell<bool>,
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

#[cfg(test)]
pub(super) fn test_content() -> Rc<SettingsContent> {
    struct InertShell;
    impl pr_core::abstractions::ShellIntegration for InertShell {
        fn get_state(&self, _: &str) -> pr_core::abstractions::ShellMenuState {
            pr_core::abstractions::ShellMenuState::NotRegistered
        }
        fn register(&self, _: &str) -> std::io::Result<()> {
            panic!("unexpected Shell write")
        }
        fn unregister(&self) -> std::io::Result<()> {
            panic!("unexpected Shell write")
        }
    }
    SettingsContent::new(SettingsDialogArgs {
        current: AppSettings::new(&pr_core::settings::app_settings::EnglishSettingsTexts),
        presenter: SettingsPresenterOptions::new(
            16 << 30,
            Arc::new(pr_core::settings::app_settings::EnglishSettingsTexts),
            Arc::new(InertShell),
        ),
        decoder_factory: None,
        options: SettingsDialogOptions::default(),
        chrome: None,
    })
    .unwrap()
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
        let scratch = ScratchSeed {
            physical_memory_bytes: args.presenter.physical_memory_bytes,
            settings_texts: Arc::clone(&args.presenter.settings_texts),
            shell_integration: Arc::clone(&args.presenter.shell_integration),
        };
        let presenter = Rc::new(RefCell::new(SettingsPresenter::new(&args.current, args.presenter)));
        let SettingsDialogOptions {
            files,
            warnings,
            target,
            pages,
            open_url,
            start_explorer,
            version,
            control_key_down,
            show_action_profiles,
        } = args.options;
        let warnings = warnings.unwrap_or_else(crate::dialogs::action_profiles::TaskDialogWarningSink::instance);
        let pages = pages.unwrap_or_else(crate::dialogs::settings::catalog::create_pages);
        let expected = pr_app::presenters::settings::load::PAGES.len();
        if pages.len() != expected {
            return Err(SettingsDialogError::PageCountMismatch {
                expected,
                actual: pages.len(),
            });
        }
        let pages: Vec<PageHandle> = pages.into_iter().map(|page| Rc::new(RefCell::new(page))).collect();
        let declaration = Self::build_items(&pages, version.as_deref());
        let page_views = vec![PageView::default(); pages.len()];
        let options = ContentOptions {
            files,
            target,
            open_url,
            start_explorer,
            version,
            control_key_down,
            show_action_profiles,
        };
        let decoder_factory = args.decoder_factory;
        let content = Rc::new_cyclic(|weak| {
            let host = Rc::new(crate::dialogs::settings::dialog::host::PageHost::new(weak.clone()));
            let context = SettingsPageContext::new(host);
            SettingsContent {
                presenter,
                options,
                scratch,
                decoder_factory,
                warnings,
                pages,
                builders: declaration.builders.into_iter().map(RefCell::new).collect(),
                items: declaration.items,
                declared: declaration.declared,
                context,
                poster: Arc::new(WindowPoster::new()),
                local_queue: RefCell::new(VecDeque::new()),
                subclass_key: Cell::new(0),
                link: RefCell::new(None),
                subscription: RefCell::new(None),
                view: ViewState {
                    active: Cell::new(-1),
                    saved: Cell::new(false),
                    loading: Cell::new(false),
                    attached: Cell::new(false),
                    closed: Cell::new(false),
                    in_language_refresh: Cell::new(false),
                    viewport_width: Cell::new(0),
                    viewport_height: Cell::new(0),
                    pages: RefCell::new(page_views),
                    hidden: RefCell::new(BTreeSet::new()),
                    placed: RefCell::new(HashMap::new()),
                    page_combos: RefCell::new(HashSet::new()),
                    actions_json: RefCell::new(None),
                    key_box_brush: Cell::new(0),
                },
            }
        });
        content.register_subclass_key();
        Ok(content)
    }

    /// C# `BuildItems` (lines 200 to 252) and `Flatten`, as a PURE function (no window, no content yet): `items` = header (`Static`, `tr::settings_header`), navigation list
    /// (`ListBox`), version (`Static`, `version` or `""`), page title (`Static`), scroll bar, one viewport `Static` per page, then the buttons Defaults, Reset page, Export,
    /// Import, Cancel, Apply, Save in this order (styles in the contract); and, per page in order, the page is `declare`d on `SettingsPageBuilder::new(page_id_base(i))`.
    /// A builder with `error().is_some()` (D-W8B5-704) makes this function replace that page (inside its `RefCell`) by `PendingSettingsPage::new(key)` declared on a fresh
    /// builder. The sets of the builders are merged into `declared`.
    pub(super) fn build_items(pages: &[PageHandle], version: Option<&str>) -> Declaration {
        use crate::dialogs::native::template::{DIALOG_STYLE_CHILD, DialogControlClass as Class};
        const TAB: u32 = 0x0001_0000;
        const TEXT: u32 = DIALOG_STYLE_CHILD | 0x80;
        let mut items = vec![
            DialogItem::new(Class::Static, HEADER_ID, pr_l10n::tr::settings_header(), TEXT),
            DialogItem::new(
                Class::ListBox,
                NAV_ID,
                "",
                DIALOG_STYLE_CHILD | TAB | 1 | 0x100 | 0x10 | 0x40,
            )
            .with_ex_style(0x200),
            DialogItem::new(Class::Static, VERSION_ID, version.unwrap_or_default(), TEXT),
            DialogItem::new(Class::Static, PAGE_TITLE_ID, "", TEXT),
            DialogItem::new(Class::ScrollBar, SCROLL_ID, "", DIALOG_STYLE_CHILD | 1),
        ];
        for index in 0..pages.len() {
            items.push(
                DialogItem::new(
                    Class::Static,
                    viewport_id(i32::try_from(index).unwrap_or(i32::MAX)),
                    "",
                    TEXT | 0x0200_0000 | 0x0400_0000,
                )
                .with_ex_style(0x0001_0000),
            );
        }
        for (id, text) in [
            (DEFAULTS_ID, pr_l10n::tr::settings_restore_defaults()),
            (RESET_PAGE_ID, pr_l10n::tr::settings_reset_page()),
            (EXPORT_ID, pr_l10n::tr::settings_export_settings()),
            (IMPORT_ID, pr_l10n::tr::settings_import_settings()),
            (CANCEL_ID, pr_l10n::tr::common_cancel()),
            (APPLY_ID, pr_l10n::tr::common_apply()),
            (SAVE_ID, pr_l10n::tr::common_save()),
        ] {
            items.push(DialogItem::new(
                Class::Button,
                id,
                text,
                DIALOG_STYLE_CHILD | TAB | if id == SAVE_ID { 1 } else { 0 },
            ));
        }
        let mut builders = Vec::with_capacity(pages.len());
        let mut declared = DeclaredSets::default();
        for (index, page_handle) in pages.iter().enumerate() {
            let Some(mut page) = page_handle.try_borrow_mut().ok() else {
                continue;
            };
            let key = page.key().to_owned();
            let mut builder = SettingsPageBuilder::new(page_id_base(i32::try_from(index).unwrap_or(i32::MAX)));
            page.declare(&mut builder);
            if builder.error().is_some() {
                *page = Box::new(crate::dialogs::settings::catalog::PendingSettingsPage::new(key));
                builder = SettingsPageBuilder::new(page_id_base(i32::try_from(index).unwrap_or(i32::MAX)));
                page.declare(&mut builder);
            }
            declared.combo_items.extend(builder.combo_items().clone());
            declared.notes.extend(builder.notes().iter().copied());
            declared.key_boxes.extend(builder.key_boxes().iter().copied());
            declared.warning_texts.extend(builder.warning_texts().iter().copied());
            builders.push(builder);
        }
        Declaration {
            items,
            builders,
            declared,
        }
    }

    /// C# `OnInitialized` (lines 271 to 305): hides and registers the viewports, subscribes the language event, fills the navigation list, selects the initial page, sets
    /// `attached`, activates the page without layout, posts the external-editor focus. Stores `link` and the window in the poster.
    pub(super) fn initialize(&self, dialog: &NativeDialog) {
        if let Ok(mut link) = self.link.try_borrow_mut() {
            *link = Some(dialog.clone());
        }
        for index in 0..self.pages.len() {
            let id = viewport_id(i32::try_from(index).unwrap_or(i32::MAX));
            let viewport = crate::win32::dialog_native::user32::get_dlg_item(dialog.hwnd(), id);
            dialog.add_child_host(viewport);
            self.forward(dialog, viewport, true);
            crate::win32::api::user32::show_window(viewport, crate::win32::messages::SW_HIDE);
        }
        let nav = dialog.item(NAV_ID);
        self.forward(dialog, nav, false);
        crate::dialogs::native::controls::basic::ListBoxControl::clear(nav);
        for page in &self.pages {
            if let Ok(page) = page.try_borrow() {
                crate::dialogs::native::controls::basic::ListBoxControl::add(nav, &page.title());
            }
        }
        let key = if self.options.target == SettingsTarget::ExternalEditor {
            "Files".to_owned()
        } else {
            SettingsPresenter::initial_page_key()
        };
        let index = self
            .pages
            .iter()
            .position(|page| page.borrow().key() == key)
            .unwrap_or(0);
        crate::dialogs::native::controls::basic::ListBoxControl::select_single(nav, i32::try_from(index).unwrap_or(0));
        let poster = Arc::clone(&self.poster);
        *self.subscription.borrow_mut() = Some(pr_l10n::localizer::on_current_changed(move || {
            poster.post_language_changed()
        }));
        self.poster.set_hwnd(dialog.hwnd());
        self.view.attached.set(true);
        self.activate_page(
            dialog,
            i32::try_from(index).unwrap_or(0),
            self.options.target != SettingsTarget::ExternalEditor,
            false,
        );
        if self.options.target == SettingsTarget::ExternalEditor {
            self.post_local(
                dialog,
                Box::new(|content, dialog| {
                    content.focus_external_editor(dialog);
                }),
            );
        }
    }

    /// C# `FocusExternalEditor` (lines 307 to 331): the Files page creates its controls, becomes the showing page (without remembering it) and focuses its editor path box;
    /// `false` when no page handles it.
    pub fn focus_external_editor(&self, dialog: &NativeDialog) -> bool {
        let Some(index) = self.pages.iter().position(|page| page.borrow().key() == "Files") else {
            return false;
        };
        self.ensure_page_created(dialog, index);
        if self.view.active.get() != i32::try_from(index).unwrap_or(-1) {
            self.activate_page(dialog, i32::try_from(index).unwrap_or(-1), false, true);
        }
        self.pages
            .get(index)
            .and_then(|page| page.try_borrow_mut().ok())
            .is_some_and(|mut page| page.focus_external_editor())
    }

    /// The live dialog (`None` before `on_initialized` and after `on_closed`).
    pub(super) fn native(&self) -> Option<NativeDialog> {
        self.link.try_borrow().ok()?.as_ref().cloned()
    }

    /// The window handle of the live dialog; `0` when there is none.
    pub(super) fn hwnd(&self) -> Hwnd {
        self.native().map_or(0, |dialog| dialog.hwnd())
    }

    /// Runs `f` with the live dialog; `None` (and `f` is not called) when the window does not exist. Used by the weak adapter. The handle is CLONED out of `link` and the
    /// `link` borrow is released BEFORE `f` runs (`f` re-enters the dialog procedure; a held borrow would make the next `hwnd()` or `with_native` panic).
    pub(super) fn with_native<R>(&self, f: impl FnOnce(&NativeDialog) -> R) -> Option<R> {
        let dialog = self.native()?;
        Some(f(&dialog))
    }

    /// `pages[index]` for `index` in range (`None` otherwise, never panics).
    pub(super) fn page(&self, index: i32) -> Option<PageHandle> {
        let index = usize::try_from(index).ok()?;
        self.pages.get(index).cloned()
    }

    /// Registers this content in the subclass registry (a `thread_local!` map from a non-zero counter value to a `Weak<SettingsContent>`) and stores the key in `subclass_key`.
    /// Called once at the end of `new`. The key is the `ref_data` of every subclass procedure of this window (D-W8B5-908); a pointer is never used.
    pub(super) fn register_subclass_key(self: &Rc<Self>) {
        let key = NEXT_SUBCLASS_KEY.with(|next| {
            let current = next.get().max(1);
            next.set(current.wrapping_add(1).max(1));
            current
        });
        self.subclass_key.set(key);
        SUBCLASS_REGISTRY.with(|registry| {
            if let Ok(mut registry) = registry.try_borrow_mut() {
                registry.insert(key, Rc::downgrade(self));
            }
        });
    }

    /// The content registered under `key`, `None` when the key is unknown or the window is closed (the procedure then passes the message on).
    pub(super) fn from_subclass_key(key: usize) -> Option<Rc<SettingsContent>> {
        if key == 0 {
            return None;
        }
        SUBCLASS_REGISTRY
            .try_with(|registry| registry.try_borrow().ok()?.get(&key)?.upgrade())
            .ok()
            .flatten()
    }

    /// Removes the registry entry and zeroes `subclass_key` (called by `close::on_closed` and by `Drop`; idempotent: a second call, or a call after the registry was torn down
    /// at thread exit, does nothing and never panics).
    pub(super) fn forget_subclass_key(&self) {
        let key = self.subclass_key.replace(0);
        if key != 0 {
            let _ = SUBCLASS_REGISTRY.try_with(|registry| {
                if let Ok(mut registry) = registry.try_borrow_mut() {
                    registry.remove(&key);
                }
            });
        }
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
        pr_l10n::tr::settings_window_title()
    }

    /// C# `Items`: a clone of `items`.
    fn items(&self) -> Vec<DialogItem> {
        self.items.clone()
    }

    /// C# `Resizable` = `true`.
    fn resizable(&self) -> bool {
        true
    }

    /// C# `HeaderControlIds` = `[HEADER_ID, PAGE_TITLE_ID]`.
    fn header_control_ids(&self) -> Vec<i32> {
        vec![HEADER_ID, PAGE_TITLE_ID]
    }

    /// C# `InitialFocusId` = `NAV_ID`.
    fn initial_focus_id(&self) -> i32 {
        NAV_ID
    }

    /// C# `DefaultButtonId` = `SAVE_ID`.
    fn default_button_id(&self) -> i32 {
        SAVE_ID
    }

    /// C# `PreferredClientSize` -> `footer` (19m).
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        self.preferred_size(dialog)
    }

    /// C# `MinimumClientSize` -> `footer` (19m).
    fn minimum_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        self.minimum_size(dialog)
    }

    /// C# `LayoutControls` -> `footer` (19m).
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32) {
        self.layout_controls(dialog, client_width, client_height)
    }

    /// C# `OnInitialized` -> [`SettingsContent::initialize`].
    fn on_initialized(&self, dialog: &NativeDialog) {
        self.initialize(dialog)
    }

    /// C# `OnOk` -> `apply::try_save_all` (19l).
    fn on_ok(&self, dialog: &NativeDialog) -> bool {
        self.try_save_all(dialog)
    }

    /// C# `OnCommand` -> `apply::handle_command` (19l).
    fn on_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        self.handle_command(dialog, id, notification)
    }

    /// C# `OnScroll` -> `page_layout::handle_scroll` (19m).
    fn on_scroll(&self, dialog: &NativeDialog, id: i32, code: i32) {
        self.handle_scroll(dialog, id, code)
    }

    /// C# `OnWindowMessage` -> `messages::handle_window_message` (19l).
    fn on_window_message(
        &self,
        dialog: &NativeDialog,
        message: u32,
        wparam: Wparam,
        lparam: Lparam,
    ) -> Option<Lresult> {
        self.handle_window_message(dialog, message, wparam, lparam)
    }

    /// C# `OnDialogKey` -> `nav::handle_dialog_key` (19j).
    fn on_dialog_key(&self, dialog: &NativeDialog, message: &Msg) -> bool {
        self.handle_dialog_key(dialog, message)
    }

    /// C# `StaticTextColor` -> `surface::static_text_color` (19m).
    fn static_text_color(&self, dialog: &NativeDialog, id: i32) -> u32 {
        self.static_text_color(dialog, id)
    }

    /// C# `OnClosed` -> `close::on_closed` (19m).
    fn on_closed(&self, dialog: &NativeDialog) {
        self.on_closed(dialog)
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
        let chrome = args.chrome.clone();
        let content = SettingsContent::new(args)?;
        let native = NativeDialog::with_chrome(content.clone(), chrome);
        Ok(Self { native, content })
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
        let ready_callback = |_: &NativeDialog| {
            if let Some(callback) = ready {
                callback(self);
            }
        };
        self.native.show_modal(owner, Some(&ready_callback), message_loop)
    }

    /// C# `Close()` (= cancel).
    pub fn close(&self) {
        self.native.close()
    }

    /// C# `Hwnd`.
    pub fn hwnd(&self) -> Hwnd {
        self.native.hwnd()
    }

    /// C# `IsClosed`.
    pub fn is_closed(&self) -> bool {
        self.native.is_closed()
    }

    /// C# `Presenter` (test seam).
    pub fn presenter(&self) -> SharedSettingsPresenter {
        Rc::clone(&self.content.presenter)
    }

    /// C# `Settings`: a clone of the working copy; after a successful Save/Apply it is the saved one.
    pub fn settings(&self) -> AppSettings {
        self.content.presenter.borrow().settings().clone()
    }

    /// C# `Saved`: at least one Save/Apply succeeded (also when Cancel followed).
    pub fn saved(&self) -> bool {
        self.content.view.saved.get()
    }

    /// C# `Pages.Count`.
    pub fn page_count(&self) -> usize {
        self.content.pages.len()
    }

    /// C# `Pages[index].Key`; `None` out of range.
    pub fn page_key(&self, index: usize) -> Option<String> {
        self.content.pages.get(index).map(|page| page.borrow().key().to_owned())
    }

    /// C# `ActivePageIndex`.
    pub fn active_page_index(&self) -> i32 {
        self.content.view.active.get()
    }

    /// C# `ActivePageKey` (`""` before the first activation).
    pub fn active_page_key(&self) -> String {
        let index = self.active_page_index();
        self.page_key(usize::try_from(index).unwrap_or(usize::MAX))
            .unwrap_or_default()
    }

    /// C# `ScrollOffset(index)`; `0` out of range.
    pub fn scroll_offset(&self, index: usize) -> i32 {
        self.content
            .view
            .pages
            .try_borrow()
            .ok()
            .and_then(|pages| pages.get(index).map(|page| page.scroll))
            .unwrap_or(0)
    }

    /// C# `ContentHeight(index)`; `0` out of range.
    pub fn content_height(&self, index: usize) -> i32 {
        self.content
            .view
            .pages
            .try_borrow()
            .ok()
            .and_then(|pages| pages.get(index).map(|page| page.content_height))
            .unwrap_or(0)
    }

    /// C# `ViewportHeight`.
    pub fn viewport_height(&self) -> i32 {
        self.content.view.viewport_height.get()
    }

    /// C# `PageItems(index)`: the declared items of the page in declaration order (= Tab order); empty out of range.
    pub fn page_items(&self, index: usize) -> Vec<DialogItem> {
        self.content
            .builders
            .get(index)
            .and_then(|builder| builder.try_borrow().ok().map(|builder| builder.items().to_vec()))
            .unwrap_or_default()
    }

    /// C# `IsPageCreated(index)`.
    pub fn is_page_created(&self, index: usize) -> bool {
        self.content.is_page_created(index)
    }

    /// C# `FocusExternalEditor()`.
    pub fn focus_external_editor(&self) -> bool {
        self.content.focus_external_editor(&self.native)
    }

    /// C# `EnsurePageCreated(index)`.
    pub fn ensure_page_created(&self, index: usize) {
        self.content.ensure_page_created(&self.native, index)
    }

    /// C# `EnsureAllPagesCreated()`.
    pub fn ensure_all_pages_created(&self) {
        self.content.ensure_all_pages_created(&self.native)
    }

    /// C# `ActivatePage(index, remember, layout)`.
    pub fn activate_page(&self, index: i32, remember: bool, layout: bool) {
        self.content.activate_page(&self.native, index, remember, layout)
    }

    /// C# `CyclePage(forward)`.
    pub fn cycle_page(&self, forward: bool) {
        self.content.cycle_page(&self.native, forward)
    }

    /// C# `HandleKeyForTest(in Msg)`.
    pub fn handle_key_for_test(&self, message: &Msg) -> bool {
        self.content.handle_dialog_key(&self.native, message)
    }

    /// C# `IsKeyBox(hwnd)`.
    pub fn is_key_box(&self, hwnd: Hwnd) -> bool {
        self.content.is_key_box(&self.native, hwnd)
    }

    /// C# `IsKeyBoxId(id)`.
    pub fn is_key_box_id(&self, id: i32) -> bool {
        self.content.is_key_box_id(id)
    }

    /// C# `CaptureKey(hwnd, wParam, lParam)`.
    pub fn capture_key(&self, hwnd: Hwnd, wparam: Wparam, lparam: Lparam) -> bool {
        self.content.capture_key(&self.native, hwnd, wparam, lparam)
    }

    /// C# `ActionsJson`.
    pub fn actions_json(&self) -> String {
        self.content.actions_json()
    }

    /// C# `RefreshActionsJsonFromPages()`.
    pub fn refresh_actions_json_from_pages(&self) -> String {
        self.content.refresh_actions_json_from_pages()
    }

    /// C# `SetActionsJson(json, source)`; `source_key` is the key of the calling page (`""` = none).
    pub fn set_actions_json(&self, json: &str, source_key: &str) {
        self.content.set_actions_json(&self.native, json, source_key)
    }

    /// C# `CollectAll()`.
    pub fn collect_all(&self) -> SettingsFormState {
        self.content.collect_all()
    }

    /// C# `TrySaveAll()`.
    pub fn try_save_all(&self) -> bool {
        self.content.try_save_all(&self.native)
    }

    /// C# `ResetActivePage()`.
    pub fn reset_active_page(&self) {
        self.content.reset_active_page(&self.native)
    }

    /// C# `ImportSettingsFrom(path)`.
    pub fn import_settings_from(&self, path: &std::path::Path) {
        self.content.import_settings_from(&self.native, path)
    }

    /// C# `Warn(title, message)`.
    pub fn warn(&self, title: &str, message: &str) {
        self.content.warn(&self.native, title, message)
    }

    /// C# `PostToUi(action)` from any thread (through the `Send + Sync` poster).
    pub fn post_to_ui(&self, action: ContextUiAction) {
        self.content.poster.post(action)
    }

    /// C# `RefreshLocalizedTexts()`.
    pub fn refresh_localized_texts(&self) {
        self.content.refresh_localized_texts(&self.native)
    }

    /// C# `SetControlTooltip(id, Func<string>)`: the text is asked again each time the tip is shown.
    pub fn set_control_tooltip_fn(&self, id: i32, text: TextFn) {
        self.content.set_control_tooltip_fn(&self.native, id, text)
    }

    /// C# `SetControlTooltip(id, string)`: a fixed text, set once.
    pub fn set_control_tooltip_text(&self, id: i32, text: &str) {
        self.content.set_control_tooltip_text(&self.native, id, text)
    }

    /// C# `GetControlText(id)`.
    pub fn control_text(&self, id: i32) -> String {
        self.content.get_control_text(&self.native, id)
    }

    /// C# `SetControlText(id, text)`.
    pub fn set_control_text(&self, id: i32, text: &str) {
        self.content.set_control_text(&self.native, id, text)
    }

    /// C# `IsControlChecked(id)`.
    pub fn is_control_checked(&self, id: i32) -> bool {
        self.content.is_control_checked(&self.native, id)
    }

    /// C# `SetControlChecked(id, value)`.
    pub fn set_control_checked(&self, id: i32, value: bool) {
        self.content.set_control_checked(&self.native, id, value)
    }

    /// C# `ComboSelectedIndex(id)`.
    pub fn combo_selected_index(&self, id: i32) -> i32 {
        self.content.combo_selected_index(&self.native, id)
    }

    /// C# `SetComboSelectedIndex(id, index)`.
    pub fn set_combo_selected_index(&self, id: i32, index: i32) {
        self.content.set_combo_selected_index(&self.native, id, index)
    }

    /// C# `SetComboItems(id, items, selected)`.
    pub fn set_combo_items(&self, id: i32, items: &[String], selected: i32) {
        self.content.set_combo_items(&self.native, id, items, selected)
    }

    /// C# `SliderValue(id)`.
    pub fn slider_value(&self, id: i32) -> i32 {
        self.content.slider_value(&self.native, id)
    }

    /// C# `SetSliderValue(id, value)`.
    pub fn set_slider_value(&self, id: i32, value: i32) {
        self.content.set_slider_value(&self.native, id, value)
    }

    /// C# `SetSliderRange(id, min, max)`.
    pub fn set_slider_range(&self, id: i32, min: i32, max: i32) {
        self.content.set_slider_range(&self.native, id, min, max)
    }

    /// C# `SetControlEnabled(id, enabled)`.
    pub fn set_control_enabled(&self, id: i32, enabled: bool) {
        self.content.set_control_enabled(&self.native, id, enabled)
    }

    /// C# `IsControlEnabled(id)`.
    pub fn is_control_enabled(&self, id: i32) -> bool {
        self.content.is_control_enabled(&self.native, id)
    }

    /// C# `SetControlVisible(id, visible)`.
    pub fn set_control_visible(&self, id: i32, visible: bool) {
        self.content.set_control_visible(&self.native, id, visible)
    }

    /// C# `IsControlVisible(id)`.
    pub fn is_control_visible(&self, id: i32) -> bool {
        self.content.is_control_visible(id)
    }

    /// C# `FocusControl(id)`.
    pub fn focus_control(&self, id: i32) {
        self.content.focus_control(&self.native, id)
    }

    /// C# `RelayoutActivePage()`.
    pub fn relayout_active_page(&self) {
        self.content.relayout_active_page(&self.native)
    }

    /// C# `EnsureVisible(id)`.
    pub fn ensure_visible(&self, id: i32) {
        self.content.ensure_visible(&self.native, id)
    }
}
