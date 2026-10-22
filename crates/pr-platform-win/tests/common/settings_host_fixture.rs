//! Shared fake host for the settings framework tests (W8-19a, binding API for W8-19d..19m): `RecordingHost` implements `SettingsPageHost`, records one text line
//! per call and answers from preset values keyed by control id. Pure: no OS, no window. Contract: `contracts/W8-19a.md`, "Test plan".
#![allow(dead_code)] // each test crate that includes this file uses a different part

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_app::presenters::settings::SettingsPresenter;
use pr_core::settings::shortcuts::ReviewAction;
use pr_imaging::decoder_traits::ImageDecoderFactory;
use pr_platform_win::dialogs::file_dialog::ShellFileDialog;
use pr_platform_win::dialogs::settings::context::{
    ActionProfilesUnavailable, ContextUiAction, SettingsPageContext, SettingsPageHost, SettingsUiPost,
};
use pr_platform_win::dialogs::settings::page_view::TextFn;
use pr_platform_win::win32::structs::Hwnd;

type Presenter = Rc<RefCell<SettingsPresenter>>;

thread_local! {
    /// Hosts of this thread by poster key: the poster handed out by `ui_post` is `Send + Sync`, so it cannot hold the `Rc`; it finds the host here and runs the
    /// action immediately (it is only ever called from the test thread).
    static HOSTS: RefCell<HashMap<usize, Weak<RecordingHost>>> = RefCell::new(HashMap::new());
}

struct ImmediatePoster {
    key: usize,
}

impl SettingsUiPost for ImmediatePoster {
    fn post(&self, action: ContextUiAction) {
        let host = HOSTS.with(|hosts| hosts.borrow().get(&self.key).and_then(Weak::upgrade));
        if let Some(host) = host {
            let context = host.context();
            action(&context);
        }
    }
}

/// A recording `SettingsPageHost`.
pub struct RecordingHost {
    me: RefCell<Weak<RecordingHost>>,
    key: usize,
    log: RefCell<Vec<String>>,
    pub texts: RefCell<HashMap<i32, String>>,
    pub checked: RefCell<HashMap<i32, bool>>,
    pub combo: RefCell<HashMap<i32, i32>>,
    pub slider: RefCell<HashMap<i32, i32>>,
    pub enabled: RefCell<HashMap<i32, bool>>,
    pub visible: RefCell<HashMap<i32, bool>>,
    pub shortcut_texts: RefCell<BTreeMap<String, String>>,
    pub actions_json: RefCell<String>,
    pub refreshed_actions_json: RefCell<String>,
    pub files: RefCell<Option<Rc<dyn ShellFileDialog>>>,
    pub version: RefCell<Option<String>>,
    pub explorer_error: RefCell<Option<String>>,
    pub open_url_result: RefCell<Result<(), String>>,
    pub action_profiles: RefCell<Result<Option<Vec<ReviewAction>>, ActionProfilesUnavailable>>,
    pub presenter: RefCell<Presenter>,
    pub hwnd: Cell<Hwnd>,
    warnings: RefCell<Vec<(String, String)>>,
    /// Owner and actions the last `show_action_profiles` call received.
    pub last_profiles_call: RefCell<Option<(Hwnd, Vec<ReviewAction>)>>,
    /// Tooltip providers by control id (the last one set), to call them back.
    pub tooltips: RefCell<HashMap<i32, TextFn>>,
}

impl RecordingHost {
    pub fn new() -> Rc<RecordingHost> {
        thread_local! {
            static NEXT_KEY: Cell<usize> = const { Cell::new(1) };
        }
        let key = NEXT_KEY.with(|next| {
            let key = next.get();
            next.set(key + 1);
            key
        });
        let host = Rc::new(RecordingHost {
            me: RefCell::new(Weak::new()),
            key,
            log: RefCell::new(Vec::new()),
            texts: RefCell::default(),
            checked: RefCell::default(),
            combo: RefCell::default(),
            slider: RefCell::default(),
            enabled: RefCell::default(),
            visible: RefCell::default(),
            shortcut_texts: RefCell::default(),
            actions_json: RefCell::default(),
            refreshed_actions_json: RefCell::default(),
            files: RefCell::new(None),
            version: RefCell::new(None),
            explorer_error: RefCell::new(None),
            open_url_result: RefCell::new(Ok(())),
            action_profiles: RefCell::new(Ok(None)),
            presenter: RefCell::new(SettingsPageContext::detached().presenter()),
            hwnd: Cell::new(0x1234),
            warnings: RefCell::default(),
            last_profiles_call: RefCell::new(None),
            tooltips: RefCell::default(),
        });
        *host.me.borrow_mut() = Rc::downgrade(&host);
        HOSTS.with(|hosts| hosts.borrow_mut().insert(key, Rc::downgrade(&host)));
        host
    }

    /// A context over this host.
    pub fn context(&self) -> SettingsPageContext {
        // The host is always created by `new`, which stores the weak self reference; the strong one is alive while a test uses it.
        match self.me.borrow().upgrade() {
            Some(host) => SettingsPageContext::new(host),
            None => SettingsPageContext::detached(),
        }
    }

    /// One line per host call, in order (`hwnd` is not recorded).
    pub fn calls(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    pub fn clear_calls(&self) {
        self.log.borrow_mut().clear();
    }

    /// `(title, message)` of every `warn`.
    pub fn warnings(&self) -> Vec<(String, String)> {
        self.warnings.borrow().clone()
    }

    fn record(&self, line: String) {
        self.log.borrow_mut().push(line);
    }
}

impl Drop for RecordingHost {
    fn drop(&mut self) {
        let key = self.key;
        let _ = HOSTS.try_with(|hosts| hosts.borrow_mut().remove(&key));
    }
}

impl SettingsPageHost for RecordingHost {
    fn presenter(&self) -> Presenter {
        self.record("presenter".to_owned());
        Rc::clone(&self.presenter.borrow())
    }
    fn hwnd(&self) -> Hwnd {
        self.hwnd.get()
    }
    fn files(&self) -> Option<Rc<dyn ShellFileDialog>> {
        self.record("files".to_owned());
        self.files.borrow().clone()
    }
    fn decoder_factory(&self) -> Option<Arc<dyn ImageDecoderFactory>> {
        self.record("decoder_factory".to_owned());
        None
    }
    fn version(&self) -> Option<String> {
        self.record("version".to_owned());
        self.version.borrow().clone()
    }
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.record(format!("open_url {url}"));
        self.open_url_result.borrow().clone()
    }
    fn start_explorer(&self, arguments: &str) -> Option<String> {
        self.record(format!("start_explorer {arguments}"));
        self.explorer_error.borrow().clone()
    }
    fn show_action_profiles(
        &self,
        owner: Hwnd,
        actions: &[ReviewAction],
    ) -> Result<Option<Vec<ReviewAction>>, ActionProfilesUnavailable> {
        self.record(format!("show_action_profiles {owner} {}", actions.len()));
        *self.last_profiles_call.borrow_mut() = Some((owner, actions.to_vec()));
        self.action_profiles.borrow().clone()
    }

    fn control_text(&self, id: i32) -> String {
        self.record(format!("text {id}"));
        self.texts.borrow().get(&id).cloned().unwrap_or_default()
    }
    fn set_control_text(&self, id: i32, text: &str) {
        self.record(format!("set_text {id} {text}"));
        self.texts.borrow_mut().insert(id, text.to_owned());
    }
    fn is_control_checked(&self, id: i32) -> bool {
        self.record(format!("is_checked {id}"));
        self.checked.borrow().get(&id).copied().unwrap_or(false)
    }
    fn set_control_checked(&self, id: i32, value: bool) {
        self.record(format!("set_checked {id} {value}"));
        self.checked.borrow_mut().insert(id, value);
    }
    fn combo_selected_index(&self, id: i32) -> i32 {
        self.record(format!("combo_index {id}"));
        self.combo.borrow().get(&id).copied().unwrap_or(-1)
    }
    fn set_combo_selected_index(&self, id: i32, index: i32) {
        self.record(format!("set_combo_index {id} {index}"));
        self.combo.borrow_mut().insert(id, index);
    }
    fn set_combo_items(&self, id: i32, items: &[String], selected: i32) {
        self.record(format!("set_combo_items {id} [{}] {selected}", items.join(",")));
        self.combo.borrow_mut().insert(id, selected);
    }
    fn slider_value(&self, id: i32) -> i32 {
        self.record(format!("slider_value {id}"));
        self.slider.borrow().get(&id).copied().unwrap_or(0)
    }
    fn set_slider_value(&self, id: i32, value: i32) {
        self.record(format!("set_slider_value {id} {value}"));
        self.slider.borrow_mut().insert(id, value);
    }
    fn set_slider_range(&self, id: i32, min: i32, max: i32) {
        self.record(format!("set_slider_range {id} {min} {max}"));
    }
    fn set_control_tooltip(&self, id: i32, text: TextFn) {
        self.record(format!("set_tooltip {id} {}", text()));
        self.tooltips.borrow_mut().insert(id, text);
    }
    fn current_shortcut_texts(&self) -> BTreeMap<String, String> {
        self.record("shortcut_texts".to_owned());
        self.shortcut_texts.borrow().clone()
    }
    fn set_control_enabled(&self, id: i32, enabled: bool) {
        self.record(format!("set_enabled {id} {enabled}"));
        self.enabled.borrow_mut().insert(id, enabled);
    }
    fn is_control_enabled(&self, id: i32) -> bool {
        self.record(format!("is_enabled {id}"));
        self.enabled.borrow().get(&id).copied().unwrap_or(false)
    }
    fn set_control_visible(&self, id: i32, visible: bool) {
        self.record(format!("set_visible {id} {visible}"));
        self.visible.borrow_mut().insert(id, visible);
    }
    fn focus_control(&self, id: i32) {
        self.record(format!("focus {id}"));
    }
    fn relayout_active_page(&self) {
        self.record("relayout".to_owned());
    }
    fn warn(&self, title: &str, message: &str) {
        self.record(format!("warn {title}|{message}"));
        self.warnings.borrow_mut().push((title.to_owned(), message.to_owned()));
    }
    fn ui_post(&self) -> Arc<dyn SettingsUiPost> {
        self.record("ui_post".to_owned());
        Arc::new(ImmediatePoster { key: self.key })
    }
    fn actions_json(&self) -> String {
        self.record("actions_json".to_owned());
        self.actions_json.borrow().clone()
    }
    fn refresh_actions_json_from_pages(&self) -> String {
        self.record("refresh_actions_json".to_owned());
        self.refreshed_actions_json.borrow().clone()
    }
    fn set_actions_json(&self, json: &str, source_key: &str) {
        self.record(format!("set_actions_json {json} {source_key}"));
        *self.actions_json.borrow_mut() = json.to_owned();
    }
}
