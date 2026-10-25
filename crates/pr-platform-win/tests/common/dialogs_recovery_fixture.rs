//! Shared test collaborators for W8-18t/u/v. Native safety checks use injected services and TempRoot.
#![allow(dead_code)] // Each recovery test binary uses a different subset of these collaborators.

use pr_app::coordinators::zoom_detail::ThreadRunner;
use pr_app::coordinators::zoom_detail::{BackgroundJob, BackgroundRunner};
use pr_app::presenters::recovery_presentation::RecoveryTimeFormatter;
use pr_app::presenters::recovery_window::{RecoveryChecker, RecoveryWindowOptions};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::file_actions::recovery_check::RecoveryCheckResult;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_platform_win::dialogs::recovery::RecoveryDialogServices;
use pr_platform_win::dialogs::recovery::RecoveryTextSource;
use pr_platform_win::dialogs::recovery::layout::RecoveryLayoutBase;
use pr_platform_win::dialogs::task_dialog::{TaskDialogKind, TaskDialogPort};
use pr_platform_win::win32::{messages, structs::Hwnd};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;

pub fn entry(id: &str) -> JournalEntry {
    let stamp = JournalDateTime::utc(DateTimeUtc::from_ticks(1000).unwrap());
    JournalEntry {
        id: id.to_owned(),
        op_type: FileOperationType::Move,
        state: JournalState::Failed,
        source: format!("source-{id}.jpg"),
        destination: Some(format!("destination-{id}.jpg")),
        size: 10,
        last_write_utc: stamp,
        timestamp_utc: stamp,
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

struct NoFileChecker;
impl RecoveryChecker for NoFileChecker {
    fn check(&self, _: &JournalEntry) -> Result<RecoveryCheckResult, String> {
        Err("fixture has no real file checker".to_owned())
    }
}

pub fn options() -> RecoveryWindowOptions {
    RecoveryWindowOptions {
        retry: None,
        dismiss: None,
        checker: Arc::new(NoFileChecker),
        allow_permanent_delete: None,
        current_time_text: Arc::new(|| "fixed time".to_owned()),
    }
}

pub fn services() -> RecoveryDialogServices {
    RecoveryDialogServices {
        texts: texts(),
        runner: Arc::new(ThreadRunner),
    }
}

#[derive(Default)]
pub struct QueuedRunner(Mutex<Vec<BackgroundJob>>);
impl BackgroundRunner for QueuedRunner {
    fn run(&self, _: &'static str, job: BackgroundJob) {
        self.0.lock().unwrap().push(job);
    }
}
impl QueuedRunner {
    pub fn run_all(&self) {
        let jobs = std::mem::take(&mut *self.0.lock().unwrap());
        for job in jobs {
            job();
        }
    }
    pub fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

pub type MessageRecord = (String, String, TaskDialogKind);
pub type MessageHook = Rc<dyn Fn(TaskDialogKind)>;
#[derive(Default)]
pub struct FakeMessages {
    pub answer: Cell<i32>,
    pub calls: RefCell<Vec<MessageRecord>>,
    pub events: Rc<RefCell<Vec<String>>>,
    pub on_show: RefCell<Option<MessageHook>>,
}
impl FakeMessages {
    pub fn yes() -> Rc<Self> {
        Rc::new(Self {
            answer: Cell::new(messages::IDYES),
            ..Default::default()
        })
    }
}
impl TaskDialogPort for FakeMessages {
    fn show(&self, _: Hwnd, title: &str, message: &str, kind: TaskDialogKind) -> i32 {
        self.calls
            .borrow_mut()
            .push((title.to_owned(), message.to_owned(), kind));
        self.events.borrow_mut().push(format!("message:{kind:?}"));
        let hook = self.on_show.borrow().clone();
        if let Some(hook) = hook {
            hook(kind);
        }
        if kind == TaskDialogKind::Confirmation {
            self.answer.get()
        } else {
            messages::IDOK
        }
    }
}

struct FixedTime;
impl RecoveryTimeFormatter for FixedTime {
    fn format_local_general(&self, _: DateTimeUtc) -> String {
        "fixed time".to_owned()
    }
}

pub fn texts() -> RecoveryTextSource {
    RecoveryTextSource::new(
        pr_l10n::localizer::built_in_english_localizer(),
        Rc::new(FixedTime),
        ",",
    )
}

pub struct Metrics {
    pub avg: i32,
    pub line: i32,
}

pub type Placement = (i32, i32, i32, i32, i32);
pub type ButtonRow = (i32, i32, i32, i32, Vec<i32>);
#[derive(Default)]
pub struct WindowLayout {
    pub placements: RefCell<Vec<Placement>>,
    pub button_rows: RefCell<Vec<ButtonRow>>,
    pub item_heights: RefCell<Vec<(i32, i32)>>,
}
impl RecoveryLayoutBase for WindowLayout {
    fn dx(&self, n: i32) -> i32 {
        (n * 6 + 2) / 4
    }
    fn dy(&self, n: i32) -> i32 {
        (n * 15 + 4) / 8
    }
    fn line_height(&self) -> i32 {
        15
    }
    fn measure_item_height(&self, id: i32, _: i32) -> i32 {
        use pr_platform_win::dialogs::recovery::layout::*;
        match id {
            HEADER_ID => 20,
            SUMMARY_ID => 24,
            _ => 18,
        }
    }
    fn measure_text_width(&self, _: &str) -> i32 {
        30
    }
    fn measure_text_height(&self, _: &str, _: i32) -> i32 {
        18
    }
    fn get_item_text(&self, _: i32) -> String {
        "measured text".to_owned()
    }
    fn place(&self, id: i32, x: i32, y: i32, w: i32, h: i32) {
        self.placements.borrow_mut().push((id, x, y, w, h));
    }
    fn place_button_row(&self, w: i32, m: i32, y: i32, h: i32, ids: &[i32]) -> i32 {
        self.button_rows.borrow_mut().push((w, m, y, h, ids.to_vec()));
        self.button_row_width(ids)
    }
    fn button_row_width(&self, ids: &[i32]) -> i32 {
        let n = ids.len() as i32;
        n * 75 + (n - 1).max(0) * self.dx(6)
    }
    fn button_width(&self, _: &[i32]) -> i32 {
        75
    }
    fn set_list_item_height(&self, id: i32, h: i32) {
        self.item_heights.borrow_mut().push((id, h));
    }
}
impl RecoveryLayoutBase for Metrics {
    fn dx(&self, n: i32) -> i32 {
        n.wrapping_mul(self.avg).wrapping_add(2) / 4
    }
    fn dy(&self, n: i32) -> i32 {
        n.wrapping_mul(self.line).wrapping_add(4) / 8
    }
    fn line_height(&self) -> i32 {
        self.line
    }
    fn measure_item_height(&self, _: i32, _: i32) -> i32 {
        0
    }
    fn measure_text_width(&self, _: &str) -> i32 {
        0
    }
    fn measure_text_height(&self, _: &str, _: i32) -> i32 {
        0
    }
    fn get_item_text(&self, _: i32) -> String {
        String::new()
    }
    fn place(&self, _: i32, _: i32, _: i32, _: i32, _: i32) {
        panic!("size-only fixture cannot place controls");
    }
    fn place_button_row(&self, _: i32, _: i32, _: i32, _: i32, _: &[i32]) -> i32 {
        panic!("size-only fixture cannot place controls");
    }
    fn button_row_width(&self, ids: &[i32]) -> i32 {
        let n = ids.len() as i32;
        n * self.button_width(ids) + (n - 1).max(0) * self.dx(6)
    }
    fn button_width(&self, _: &[i32]) -> i32 {
        self.dx(50)
    }
    fn set_list_item_height(&self, _: i32, _: i32) {
        panic!("size-only fixture cannot place controls");
    }
}
