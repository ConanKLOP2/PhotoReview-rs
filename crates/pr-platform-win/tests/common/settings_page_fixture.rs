#![allow(dead_code)] // Shared fixture API is consumed by the other settings-page unit tests.

use pr_app::presenters::settings::{SettingsPresenter, SettingsPresenterOptions};
use pr_core::settings::shortcuts::ReviewAction;
use pr_core::{
    abstractions::{ShellIntegration, ShellMenuState},
    settings::app_settings::{AppSettings, EnglishSettingsTexts},
};
use pr_imaging::decoder_traits::ImageDecoderFactory;
use pr_platform_win::dialogs::{
    file_dialog::ShellFileDialog,
    settings::{
        context::{
            ActionProfilesUnavailable, ContextUiAction, SettingsPageContext, SettingsPageHost, SettingsUiPost,
            SharedSettingsPresenter,
        },
        page_view::TextFn,
    },
};
use pr_platform_win::win32::structs::Hwnd;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

pub const GIB: i64 = 1 << 30;
pub const EXE: &str = r"C:\Apps\PhotoReview\PhotoReview.exe";
#[derive(Debug)]
pub struct FakeShell {
    state: std::sync::Mutex<ShellMenuState>,
    calls: std::sync::Mutex<Vec<String>>,
}
impl FakeShell {
    pub fn new(s: ShellMenuState) -> Arc<Self> {
        Arc::new(Self {
            state: std::sync::Mutex::new(s),
            calls: std::sync::Mutex::new(vec![]),
        })
    }
    pub fn state(&self) -> ShellMenuState {
        *self.state.lock().unwrap()
    }
    pub fn set_state(&self, s: ShellMenuState) {
        *self.state.lock().unwrap() = s
    }
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}
impl ShellIntegration for FakeShell {
    fn get_state(&self, _: &str) -> ShellMenuState {
        self.state()
    }
    fn register(&self, e: &str) -> std::io::Result<()> {
        self.calls.lock().unwrap().push(format!("register:{e}"));
        *self.state.lock().unwrap() = ShellMenuState::Registered;
        Ok(())
    }
    fn unregister(&self) -> std::io::Result<()> {
        self.calls.lock().unwrap().push("unregister".into());
        *self.state.lock().unwrap() = ShellMenuState::NotRegistered;
        Ok(())
    }
}
pub fn presenter(f: impl FnOnce(&mut SettingsPresenterOptions)) -> SharedSettingsPresenter {
    let t = EnglishSettingsTexts;
    let s = AppSettings::new(&t);
    presenter_over(&s, f)
}
pub fn presenter_over(s: &AppSettings, f: impl FnOnce(&mut SettingsPresenterOptions)) -> SharedSettingsPresenter {
    let mut o = SettingsPresenterOptions::new(
        16 * GIB,
        Arc::new(EnglishSettingsTexts),
        FakeShell::new(ShellMenuState::NotRegistered),
    );
    o.executable_path = Some(EXE.into());
    f(&mut o);
    Rc::new(RefCell::new(SettingsPresenter::new(s, o)))
}
#[derive(Default)]
struct Post;
impl SettingsUiPost for Post {
    fn post(&self, _: ContextUiAction) {}
}
#[derive(Default)]
pub struct PageHost {
    p: Option<SharedSettingsPresenter>,
    vals: RefCell<BTreeMap<i32, String>>,
    checks: RefCell<BTreeMap<i32, bool>>,
    combos: RefCell<BTreeMap<i32, i32>>,
    sliders: RefCell<BTreeMap<i32, i32>>,
    ranges: RefCell<BTreeMap<i32, (i32, i32)>>,
    visible: RefCell<BTreeMap<i32, bool>>,
    calls: RefCell<Vec<String>>,
    items: RefCell<BTreeMap<i32, usize>>,
    warnings: RefCell<Vec<(String, String)>>,
    args: RefCell<Vec<String>>,
    actions_json: RefCell<String>,
    profile_names: RefCell<Vec<Vec<String>>>,
}
impl PageHost {
    pub fn new() -> Rc<Self> {
        Self::with_presenter(presenter(|_| {}))
    }
    pub fn with_presenter(p: SharedSettingsPresenter) -> Rc<Self> {
        Rc::new(Self {
            p: Some(p),
            ..Self::default()
        })
    }
    pub fn context(self: &Rc<Self>) -> SettingsPageContext {
        SettingsPageContext::new(self.clone())
    }
    pub fn configure_from(&self, b: &pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder) {
        for (id, (a, z)) in b.sliders() {
            self.ranges.borrow_mut().insert(*id, (*a, *z));
        }
        for (id, v) in b.combo_items() {
            self.items.borrow_mut().insert(*id, v.len());
        }
    }
    pub fn text(&self, i: i32) -> String {
        self.vals.borrow().get(&i).cloned().unwrap_or_default()
    }
    pub fn checked(&self, i: i32) -> bool {
        *self.checks.borrow().get(&i).unwrap_or(&false)
    }
    pub fn combo_index(&self, i: i32) -> i32 {
        *self.combos.borrow().get(&i).unwrap_or(&-1)
    }
    pub fn slider_value(&self, i: i32) -> i32 {
        *self.sliders.borrow().get(&i).unwrap_or(&0)
    }
    pub fn slider_range(&self, i: i32) -> (i32, i32) {
        *self.ranges.borrow().get(&i).unwrap_or(&(0, 100))
    }
    pub fn is_visible(&self, i: i32) -> bool {
        *self.visible.borrow().get(&i).unwrap_or(&true)
    }
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
    pub fn clear_calls(&self) {
        self.calls.borrow_mut().clear()
    }
    pub fn warnings(&self) -> Vec<(String, String)> {
        self.warnings.borrow().clone()
    }
    pub fn explorer_args(&self) -> Vec<String> {
        self.args.borrow().clone()
    }
    /// The JSON `refresh_actions_json_from_pages` answers (default empty).
    pub fn put_actions_json(&self, json: &str) {
        *self.actions_json.borrow_mut() = json.into();
    }
    /// The action names of each list handed to `show_action_profiles`.
    pub fn profile_names(&self) -> Vec<Vec<String>> {
        self.profile_names.borrow().clone()
    }
    pub fn put_text(&self, i: i32, v: &str) {
        self.vals.borrow_mut().insert(i, v.into());
    }
    pub fn put_combo_index(&self, i: i32, v: i32) {
        self.combos.borrow_mut().insert(i, v);
    }
    pub fn put_slider_value(&self, i: i32, v: i32) {
        self.sliders.borrow_mut().insert(i, v);
    }
    pub fn put_visible(&self, i: i32, v: bool) {
        self.visible.borrow_mut().insert(i, v);
    }
    pub fn put_explorer_error(&self, _: Option<String>) {}
}
impl SettingsPageHost for PageHost {
    fn presenter(&self) -> SharedSettingsPresenter {
        self.p.as_ref().cloned().unwrap_or_else(|| presenter(|_| {}))
    }
    fn hwnd(&self) -> Hwnd {
        0x1234
    }
    fn files(&self) -> Option<Rc<dyn ShellFileDialog>> {
        None
    }
    fn decoder_factory(&self) -> Option<Arc<dyn ImageDecoderFactory>> {
        None
    }
    fn version(&self) -> Option<String> {
        Some("2.0.1+test".into())
    }
    fn open_url(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn start_explorer(&self, a: &str) -> Option<String> {
        self.args.borrow_mut().push(a.into());
        self.calls.borrow_mut().push(format!("start_explorer {a}"));
        None
    }
    fn show_action_profiles(
        &self,
        _: Hwnd,
        actions: &[ReviewAction],
    ) -> Result<Option<Vec<ReviewAction>>, ActionProfilesUnavailable> {
        self.profile_names
            .borrow_mut()
            .push(actions.iter().map(|a| a.name.clone()).collect());
        Err(ActionProfilesUnavailable)
    }
    fn control_text(&self, i: i32) -> String {
        self.text(i)
    }
    fn set_control_text(&self, i: i32, v: &str) {
        self.vals.borrow_mut().insert(i, v.into());
        self.calls.borrow_mut().push(format!("set_text {i} {v}"));
    }
    fn is_control_checked(&self, i: i32) -> bool {
        self.checked(i)
    }
    fn set_control_checked(&self, i: i32, v: bool) {
        self.checks.borrow_mut().insert(i, v);
        self.calls.borrow_mut().push(format!("set_checked {i} {v}"));
    }
    fn combo_selected_index(&self, i: i32) -> i32 {
        self.combo_index(i)
    }
    fn set_combo_selected_index(&self, i: i32, v: i32) {
        let x = if self.items.borrow().get(&i).is_some_and(|n| v < 0 || v as usize >= *n) {
            -1
        } else {
            v
        };
        self.combos.borrow_mut().insert(i, x);
        self.calls.borrow_mut().push(format!("set_combo_index {i} {v}"));
    }
    fn set_combo_items(&self, i: i32, v: &[String], s: i32) {
        self.items.borrow_mut().insert(i, v.len());
        self.set_combo_selected_index(i, s);
    }
    fn slider_value(&self, i: i32) -> i32 {
        self.slider_value(i)
    }
    fn set_slider_value(&self, i: i32, v: i32) {
        self.sliders
            .borrow_mut()
            .insert(i, v.clamp(self.slider_range(i).0, self.slider_range(i).1));
        self.calls.borrow_mut().push(format!("set_slider_value {i} {v}"));
    }
    fn set_slider_range(&self, i: i32, a: i32, b: i32) {
        self.ranges.borrow_mut().insert(i, (a, b));
        let v = self.slider_value(i).clamp(a, b);
        self.sliders.borrow_mut().insert(i, v);
        self.calls.borrow_mut().push(format!("set_slider_range {i} {a} {b}"));
    }
    fn set_control_tooltip(&self, _: i32, _: TextFn) {}
    fn current_shortcut_texts(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
    fn set_control_enabled(&self, _: i32, _: bool) {}
    fn is_control_enabled(&self, _: i32) -> bool {
        true
    }
    fn set_control_visible(&self, i: i32, v: bool) {
        self.visible.borrow_mut().insert(i, v);
        self.calls.borrow_mut().push(format!("set_visible {i} {v}"));
    }
    fn focus_control(&self, i: i32) {
        self.calls.borrow_mut().push(format!("focus {i}"));
    }
    fn relayout_active_page(&self) {
        self.calls.borrow_mut().push("relayout".into());
    }
    fn warn(&self, t: &str, m: &str) {
        self.warnings.borrow_mut().push((t.into(), m.into()));
    }
    fn ui_post(&self) -> Arc<dyn SettingsUiPost> {
        Arc::new(Post)
    }
    fn actions_json(&self) -> String {
        String::new()
    }
    fn refresh_actions_json_from_pages(&self) -> String {
        self.actions_json.borrow().clone()
    }
    fn set_actions_json(&self, _: &str, _: &str) {}
}
