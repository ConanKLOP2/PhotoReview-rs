//! Pure tests for the settings page framework contract.

#[rustfmt::skip]
use pr_platform_win::dialogs::settings::page_view::{
    SettingsPageView, fixed_text, page_title, text_fn,
};
use pr_app::presenters::settings_form_state::{SettingsFocusField, SettingsFormState};
use pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder;
use pr_platform_win::dialogs::settings::context::SettingsPageContext;

struct MinimalPage {
    key: String,
}

impl SettingsPageView for MinimalPage {
    fn key(&self) -> &str {
        &self.key
    }

    fn declare(&mut self, _page: &mut SettingsPageBuilder) {}

    fn attach(&mut self, _context: SettingsPageContext) {}

    fn load(&mut self, _state: &SettingsFormState) {}

    fn collect(&mut self, _state: &mut SettingsFormState) {}
}

#[test]
#[rustfmt::skip]
fn default_methods_do_nothing_and_answer_false_or_none() {
    let mut page = MinimalPage {
        key: "Nope".to_owned(),
    };
    assert!(!page.on_command(10, 2));
    page.on_scroll(10);
    page.on_language_changed();
    assert_eq!(page.focus_target(SettingsFocusField::default()), None);
    assert!(!page.focus_external_editor());
    page.on_closed();
    page.on_activated();
}

#[test]
fn title_defaults_to_the_localized_page_title() {
    for key in [
        "General",
        "Display",
        "Mouse",
        "Performance",
        "Shortcuts",
        "Files",
        "Diagnostics",
    ] {
        let page = MinimalPage { key: key.to_owned() };
        assert_eq!(page.title(), page_title(key));
    }
}

#[test]
fn title_of_an_unknown_key_is_empty() {
    assert_eq!(page_title("Nope"), "");
    assert_eq!(page_title(""), "");
    assert_eq!(page_title("general"), "");
}

struct OverriddenPage(MinimalPage);

#[rustfmt::skip]
impl SettingsPageView for OverriddenPage {
    fn key(&self) -> &str { &self.0.key }
    fn title(&self) -> String { "custom title".to_owned() }
    fn declare(&mut self, _page: &mut SettingsPageBuilder) {}
    fn attach(&mut self, _context: SettingsPageContext) {}
    fn load(&mut self, _state: &SettingsFormState) {}
    fn collect(&mut self, _state: &mut SettingsFormState) {}
}

#[test]
#[rustfmt::skip]
fn title_can_be_overridden() {
    let page = OverriddenPage(MinimalPage { key: "General".to_owned() });
    assert_eq!(page.title(), "custom title");
}

#[test]
fn text_fn_and_fixed_text_return_the_text_each_time() {
    let fixed = fixed_text("fixed");
    assert_eq!(fixed(), "fixed");
    assert_eq!(fixed(), "fixed");
    let dynamic = text_fn(|| "dynamic".to_owned());
    assert_eq!(dynamic(), "dynamic");
    assert_eq!(dynamic(), "dynamic");
}
