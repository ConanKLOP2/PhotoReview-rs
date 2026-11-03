//! P-320: a nested `EN_CHANGE` raised by a page's own `set_text` while `load_all` runs must neither re-enter the page (`BorrowMutError`) nor run `refresh` twice.
//! Host branch under test: `SettingsContent::handle_command` (`view.loading` -> `true`, a borrowed page -> `true`, D-W8B5-805).
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;
use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_platform_win::dialogs::settings::{
    builder::inputs::SettingsPageBuilder,
    context::SettingsPageContext,
    dialog::{
        SettingsDialog,
        frame::{DEFAULTS_ID, page_id_base},
    },
    page_view::{SettingsPageView, fixed_text},
    pages::shortcuts::EN_CHANGE,
};
use std::{cell::Cell, rc::Rc};

/// Mirrors the Shortcuts page: `load` writes the box then calls `refresh`; `on_command(EN_CHANGE)` calls `refresh`; `refresh` reads the box and borrows the presenter.
struct EchoPage {
    key: &'static str,
    edit: i32,
    context: Option<SettingsPageContext>,
    refreshes: Rc<Cell<u32>>,
}

impl EchoPage {
    fn refresh(&mut self) {
        self.refreshes.set(self.refreshes.get() + 1);
        let context = self.context.as_ref().unwrap();
        let _ = context.text(self.edit);
        let _ = context.presenter().borrow_mut();
    }
}

impl SettingsPageView for EchoPage {
    fn key(&self) -> &str {
        self.key
    }
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        self.edit = page.edit(fixed_text("Echo"), 60).control;
    }
    fn attach(&mut self, context: SettingsPageContext) {
        self.context = Some(context);
    }
    fn load(&mut self, _: &SettingsFormState) {
        self.context.as_ref().unwrap().set_text(self.edit, "loaded");
        self.refresh();
    }
    fn collect(&mut self, _: &mut SettingsFormState) {}
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        if notification == EN_CHANGE && id == self.edit {
            self.refresh();
            return true;
        }
        false
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn nested_en_change_during_load_refreshes_once_and_does_not_reenter_the_page() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let refreshes = Rc::new(Cell::new(0));
        let mut args = fixture::args();
        let echo = |refreshes: Rc<Cell<u32>>| EchoPage {
            key: "Echo",
            edit: 0,
            context: None,
            refreshes,
        };
        let mut probe = echo(refreshes.clone());
        probe.declare(&mut SettingsPageBuilder::new(page_id_base(0)));
        let box_id = probe.edit;
        let events = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut pages: Vec<Box<dyn SettingsPageView>> = vec![Box::new(echo(refreshes.clone()))];
        for key in ["Display", "Mouse", "Performance", "Shortcuts", "Files", "Diagnostics"] {
            pages.push(Box::new(fixture::ProbePage {
                key,
                events: events.clone(),
            }));
        }
        args.options.pages = Some(pages);
        let dialog = SettingsDialog::new(args).unwrap();
        let ready = |d: &SettingsDialog| {
            d.ensure_all_pages_created();
            // A user edit (not loading) reaches the page once.
            refreshes.set(0);
            d.set_control_text(box_id, "typed");
            assert_eq!(refreshes.get(), 1, "a user edit must refresh exactly once");
            // Defaults -> load_all -> page.load -> set_text("loaded") raises a NESTED EN_CHANGE inside the &mut page borrow.
            refreshes.set(0);
            desktop::command(d.native(), DEFAULTS_ID, 0);
            assert_eq!(d.control_text(box_id), "loaded");
            assert_eq!(
                refreshes.get(),
                1,
                "load must refresh once; the nested EN_CHANGE is swallowed"
            );
            d.close();
        };
        dialog.show_modal(0, Some(&ready), None).unwrap();
    });
}
