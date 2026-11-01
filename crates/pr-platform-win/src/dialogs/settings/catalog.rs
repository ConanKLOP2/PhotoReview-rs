//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageCatalog.cs` (56 lines, baseline `d92d6aae`): the list of Win32 Settings pages that exist, and the
//! placeholder page for one that does not. Unit W8-19a. Contract: `contracts/W8-19a.md`.
//!
//! ADDING A PAGE = one line in [`FACTORIES`], kept in ALPHABETICAL order of the key so merges do not conflict (the C# comment). The seven pages exist in the baseline,
//! so [`is_complete`] is `true` and the secondary window router (W8-18o) opens Settings through this catalog.
use std::fmt;

use super::builder::inputs::SettingsPageBuilder;
use super::context::SettingsPageContext;
use super::page_view::{PageContextSlot, SettingsPageView};
use super::pages::diagnostics::DiagnosticsSettingsPage;
use super::pages::display::DisplaySettingsPage;
use super::pages::files::FilesSettingsPage;
use super::pages::general::GeneralSettingsPage;
use super::pages::mouse::MouseSettingsPage;
use super::pages::performance::PerformanceSettingsPage;
use super::pages::shortcuts::ShortcutsSettingsPage;
use pr_app::presenters::settings::load::PAGES;
use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_l10n::{keys, localizer::Localizer};

/// A page constructor (C# `Func<ISettingsPageView>`): a plain function pointer, no captures.
pub type PageFactory = fn() -> Box<dyn SettingsPageView>;

/// The registered pages: `(key, factory)` in alphabetical key order (the order of the C# `Dictionary` initializer, which is what `ImplementedKeys` enumerates).
///
/// The seven entries (D-W8B5-708) are `Diagnostics`, `Display`, `Files`, `General`, `Mouse`, `Performance`, `Shortcuts`; each factory is
/// `|| Box::new(<Page>::new())` for the page type of the same-named unit (`pages::diagnostics::DiagnosticsSettingsPage` ... `pages::shortcuts::ShortcutsSettingsPage`,
/// each with `pub fn new() -> Self`). The page constructors are implemented (W8-19d..19i); only `create_pages` calls them.
pub const FACTORIES: &[(&str, PageFactory)] = &[
    ("Diagnostics", || Box::new(DiagnosticsSettingsPage::new())),
    ("Display", || Box::new(DisplaySettingsPage::new())),
    ("Files", || Box::new(FilesSettingsPage::new())),
    ("General", || Box::new(GeneralSettingsPage::new())),
    ("Mouse", || Box::new(MouseSettingsPage::new())),
    ("Performance", || Box::new(PerformanceSettingsPage::new())),
    ("Shortcuts", || Box::new(ShortcutsSettingsPage::new())),
];

/// C# `ImplementedKeys`: the keys that have a Win32 page, in [`FACTORIES`] order.
pub fn implemented_keys() -> Vec<&'static str> {
    FACTORIES.iter().map(|(key, _)| *key).collect()
}

/// C# `PendingKeys`: the keys of `pr_app::presenters::settings::load::PAGES` (navigation order) that have no factory.
pub fn pending_keys() -> Vec<&'static str> {
    PAGES
        .iter()
        .map(|page| page.key)
        .filter(|key| !FACTORIES.iter().any(|(factory_key, _)| factory_key == key))
        .collect()
}

/// C# `IsComplete`: `pending_keys().is_empty()`.
pub fn is_complete() -> bool {
    pending_keys().is_empty()
}

/// C# `CreatePages()`: one page per entry of `pr_app::presenters::settings::load::PAGES`, in that order; a key without a factory gets a [`PendingSettingsPage`].
pub fn create_pages() -> Vec<Box<dyn SettingsPageView>> {
    let nav_keys: Vec<&str> = PAGES.iter().map(|page| page.key).collect();
    create_pages_with(&nav_keys, FACTORIES)
}

/// [`create_pages`] over an explicit table and navigation list (the testable core; the production call passes [`FACTORIES`] and `PAGES`' keys).
pub fn create_pages_with(nav_keys: &[&str], factories: &[(&str, PageFactory)]) -> Vec<Box<dyn SettingsPageView>> {
    nav_keys
        .iter()
        .map(|key| {
            factories
                .iter()
                .find(|(factory_key, _)| factory_key == key)
                .map(|(_, factory)| factory())
                .unwrap_or_else(|| Box::new(PendingSettingsPage::new(*key)))
        })
        .collect()
}

/// [`pending_keys`] over an explicit table and navigation list.
pub fn pending_keys_with(nav_keys: &[&str], factories: &[(&str, PageFactory)]) -> Vec<String> {
    nav_keys
        .iter()
        .filter(|key| !factories.iter().any(|(factory_key, _)| factory_key == *key))
        .map(|key| (*key).to_owned())
        .collect()
}

/// C# `PendingSettingsPage(key)`: the placeholder of a page that has no Win32 view yet. It declares ONE full-width note with the text of
/// `settings.pagePending` and reads/writes no value, so Save keeps that page's settings as they were loaded.
pub struct PendingSettingsPage {
    key: String,
    context: PageContextSlot,
}

impl fmt::Debug for PendingSettingsPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingSettingsPage")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl PendingSettingsPage {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            context: PageContextSlot::new(),
        }
    }
}

impl SettingsPageView for PendingSettingsPage {
    fn key(&self) -> &str {
        &self.key
    }

    /// `page.note(Some(text_fn(|| Localizer::current().get(keys::SETTINGS_PAGE_PENDING))), false)`.
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        use super::page_view::text_fn;
        page.note(
            Some(text_fn(|| Localizer::current().get(keys::SETTINGS_PAGE_PENDING))),
            false,
        );
    }

    fn attach(&mut self, context: SettingsPageContext) {
        self.context.attach(context);
    }

    /// Nothing (C# empty body).
    fn load(&mut self, state: &SettingsFormState) {
        let _ = state;
    }

    /// Nothing (C# empty body).
    fn collect(&mut self, state: &mut SettingsFormState) {
        let _ = state;
    }
}
