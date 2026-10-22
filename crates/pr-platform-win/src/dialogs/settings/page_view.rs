//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/ISettingsPageView.cs` (100 lines, baseline `d92d6aae`): the contract of one Settings page
//! (`ISettingsPageView`) and its do-nothing base (`SettingsPageBase`). Unit W8-19a. Contract: `contracts/W8-19a.md`.
//!
//! WP-35 GD3a: the frame (`SettingsDialog`, W8-19j..19m) owns navigation, scrolling, Save/Apply/Cancel/Defaults/Reset/Import/Export, layout, DPI and language;
//! a page only (1) DECLARES its controls with [`SettingsPageBuilder`], (2) copies values between its controls and [`SettingsFormState`], (3) handles its own
//! commands and notes. All rules (validation, `AppSettings` mapping) live in the presenter and the field map. The ORDER of combo items is the contract with
//! `SettingsFormState` (an index), so a page must declare the items in the order of the WPF page.
//!
//! The C# abstract base class becomes the default methods of the trait plus [`PageContextSlot`] (the C# `protected Context` property).
use std::fmt;
use std::rc::Rc;

use pr_app::presenters::settings::SettingsPresenter;
use pr_app::presenters::settings_form_state::{SettingsFocusField, SettingsFormState};

use super::builder::inputs::SettingsPageBuilder;
use super::context::SettingsPageContext;
use super::context::with_field_context;

/// C# `Func<string>`: a text provider evaluated again whenever the text is needed (a language change changes the answer). UI-thread only, hence `Rc`.
pub type TextFn = Rc<dyn Fn() -> String>;

/// Wraps a closure as a [`TextFn`].
pub fn text_fn(f: impl Fn() -> String + 'static) -> TextFn {
    Rc::new(f)
}

/// A [`TextFn`] that always answers the same text (tests, fixed captions).
pub fn fixed_text(text: impl Into<String>) -> TextFn {
    let text: String = text.into();
    Rc::new(move || text.clone())
}

/// C# `SettingsPresenter.PageTitle(key)` with the ambient language: the localized heading of the page `key` (`settings.nav.*`), `""` for a key that is not one of
/// the seven pages. Uses `Localizer::current()` and the current number culture (D-W8-4).
pub fn page_title(key: &str) -> String {
    with_field_context(|context| SettingsPresenter::page_title(key, context))
}

/// C# `ISettingsPageView` + the defaults of `SettingsPageBase`. One implementation per page (`pages::*`), plus the catalog's [`PendingSettingsPage`](super::catalog).
///
/// UI-thread only; the frame keeps pages as `Rc<RefCell<dyn SettingsPageView>>` and reaches OTHER pages with `try_borrow_mut` while a page runs (D-W8B5-706).
pub trait SettingsPageView {
    /// C# `Key`: the page key, one of `pr_app::presenters::settings::load::PAGES` (`General`, `Display`, `Mouse`, `Performance`, `Shortcuts`, `Files`, `Diagnostics`).
    fn key(&self) -> &str;

    /// C# `Title` (virtual): the translated heading. Default = [`page_title`] of [`key`](Self::key).
    fn title(&self) -> String {
        page_title(self.key())
    }

    /// C# `Declare(page)`: called ONCE before the window is created: declare the controls and the layout. Declaration order = Tab order. The page keeps the ids
    /// the builder returns.
    fn declare(&mut self, page: &mut SettingsPageBuilder);

    /// C# `Attach(context)` (virtual, the base stores the context): called once after the controls exist, before [`load`](Self::load): keep the context, fill dynamic
    /// lists. A page stores the context in a [`PageContextSlot`].
    fn attach(&mut self, context: SettingsPageContext);

    /// C# `Load(state)`: form to controls (every property of the page). Called on open, Defaults, Import and Reset page.
    fn load(&mut self, state: &SettingsFormState);

    /// C# `Collect(state)`: controls to form. ONLY the properties of this page; everything else keeps the loaded value.
    fn collect(&mut self, state: &mut SettingsFormState);

    /// C# `OnCommand(id, notification)`: `WM_COMMAND` of a control of this page (`BN_CLICKED`, `CBN_SELCHANGE`, `EN_CHANGE`...). `true` = handled. Default `false`.
    fn on_command(&mut self, _id: i32, _notification: u32) -> bool {
        false
    }

    /// C# `OnScroll(id)`: `WM_HSCROLL` of the trackbar `id` of this page. Default: nothing.
    fn on_scroll(&mut self, _id: i32) {}

    /// C# `OnLanguageChanged()`: the language changed; set again the texts the code sets (hints, values, status). Texts declared with the builder follow by
    /// themselves. Default: nothing.
    fn on_language_changed(&mut self) {}

    /// C# `FocusTarget(field)`: the control that gets the focus when Save is refused at `field`; `None` = this page has no such field. Default `None`.
    fn focus_target(&self, _field: SettingsFocusField) -> Option<i32> {
        None
    }

    /// C# `FocusExternalEditor()` (default interface method): the "open in external editor" command (`SettingsTarget::ExternalEditor`): the Files page focuses its
    /// editor path box and returns `true`; the other pages return `false` (default). Called once by the frame after the first layout.
    fn focus_external_editor(&mut self) -> bool {
        false
    }

    /// C# `OnClosed()`: the window closes. Default: nothing.
    fn on_closed(&mut self) {}

    /// C# `OnActivated()`: the page was just switched to (controls created and loaded): refresh data that depends on another page (the Mouse page's zoom-key summary
    /// follows the key being typed on the Shortcuts page). Default: nothing.
    fn on_activated(&mut self) {}
}

/// C# `SettingsPageBase.Context` (`protected SettingsPageContext Context { get; private set; } = null!`): where a page keeps its context.
///
/// Before [`attach`](Self::attach) the C# property is `null!` (a call is a `NullReferenceException`). Rust cannot panic on the UI thread, so [`context`](Self::context)
/// then answers a DETACHED context whose every read is empty/false/0 and whose every write does nothing (D-W8B5-705).
#[derive(Clone, Default)]
pub struct PageContextSlot {
    context: Option<SettingsPageContext>,
}

impl fmt::Debug for PageContextSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageContextSlot")
            .field("attached", &self.context.is_some())
            .finish()
    }
}

impl PageContextSlot {
    /// An empty slot (detached).
    pub fn new() -> Self {
        Self::default()
    }

    /// C# `Attach` of the base class: stores the context.
    pub fn attach(&mut self, context: SettingsPageContext) {
        self.context = Some(context);
    }

    /// True after [`attach`](Self::attach).
    pub fn is_attached(&self) -> bool {
        self.context.is_some()
    }

    /// The attached context (a cheap clone: it shares the host), or [`SettingsPageContext::detached`] before `attach`.
    pub fn context(&self) -> SettingsPageContext {
        self.context.clone().unwrap_or_else(SettingsPageContext::detached)
    }
}
