//! Tests of `pr_shell::composition::adapters` (W8-09b slice S1, `contracts/W8-09b.md`, decision D-W8B6-153).
//!
//! The adapters have no C# twin (the C# container needed none), so every test is `new:`. The one exception is the Rust twin of the C# test
//! `WpfPresentationSinkFactory_BuildsAWpfSinkThatForwardsTheCallbacks_AndKeepsTheMetrics`, which the contract names `win32_sink_factory_adapter_forwards_every_callback_and_the_zoom_pull`.
//!
//! Not here (they need `SharedServices`, whose accessors are `todo!()` until slice S3 lands, and the `PlatformServices` fixture S3 owns):
//! `lazy_prewarm_previews_builds_nothing_until_the_first_call`, `lazy_prewarm_previews_logs_and_answers_an_error_key_when_the_service_cannot_be_built`,
//! `lazy_decoder_factory_builds_nothing_until_the_first_call`, and the `to_local_time` half of `LocalizedFormat` (needs `culture::utc_to_local`, slice S2).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_app::coordinators::presenter::state::ImageChange;
use pr_app::coordinators::sink_callbacks::{MainViewModelSinkCallbacks, PresentationSinkFactory};
use pr_app::services::contracts::ClipboardService;
use pr_app::viewmodels::main::FormatContextSource;
use pr_core::abstractions::{BatchReviewItem, DialogService, SettingsTarget, SkippedEntry, SkippedKind};
use pr_imaging::decode_types::PlatformImage;
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::{Localizer, built_in_english, built_in_english_localizer, set_number_culture};
use pr_l10n::template::NumberCulture;
use pr_platform_win::dialogs::dialog_service::{LazySecondaryPort, SecondaryWindowPort, Win32DialogService};
use pr_platform_win::dialogs::folder_dialog::{FolderPicker, ShellFolderDialog};
use pr_platform_win::dialogs::task_dialog::{TaskDialogKind, TaskDialogPort};
use pr_platform_win::shell_viewing::presentation_sink::{ShellPresentationHub, Win32PresentationSinkFactory};
use pr_platform_win::win32::messages::{IDNO, IDYES};
use pr_platform_win::win32::structs::{Hresult, Hwnd};
use pr_shell::composition::adapters::{LocalizedFormat, SharedClipboard, SharedDialogService, Win32SinkFactoryAdapter};
use pr_test_support::TempRoot;

const OWNER: Hwnd = 4242;

// ---- process-wide ambient state (the localizer and the number culture) -------------------------------------------------------------------------------------

static GLOBAL: Mutex<()> = Mutex::new(());

struct Ambient {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for Ambient {
    fn drop(&mut self) {
        Localizer::set_current(built_in_english_localizer());
        set_number_culture(NumberCulture::invariant());
    }
}

fn ambient() -> Ambient {
    let lock = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner);
    Localizer::set_current(built_in_english_localizer());
    set_number_culture(NumberCulture::invariant());
    Ambient { _lock: lock }
}

// ---- SharedDialogService ---------------------------------------------------------------------------------------------------------------------------------

type Calls = Rc<RefCell<Vec<String>>>;

struct FakeTaskDialogs {
    result: Rc<Cell<i32>>,
    calls: Calls,
}

impl TaskDialogPort for FakeTaskDialogs {
    fn show(&self, owner: Hwnd, title: &str, message: &str, kind: TaskDialogKind) -> i32 {
        self.calls
            .borrow_mut()
            .push(format!("task:{owner}:{kind:?}:{title}:{message}"));
        self.result.get()
    }
}

struct FakeFolder {
    answer: Rc<RefCell<Result<Option<String>, Hresult>>>,
    calls: Calls,
}

impl ShellFolderDialog for FakeFolder {
    fn show(&self, owner: Hwnd, _title: &str, initial_folder: Option<&str>) -> Result<Option<String>, Hresult> {
        self.calls
            .borrow_mut()
            .push(format!("folder:{owner}:{}", initial_folder.unwrap_or("<none>")));
        self.answer.borrow().clone()
    }
}

struct FakePort {
    calls: Calls,
    settings_result: Rc<Cell<bool>>,
    batch_result: Rc<Cell<bool>>,
}

impl SecondaryWindowPort for FakePort {
    fn show_settings(&self, owner: Hwnd, target: SettingsTarget) -> bool {
        self.calls.borrow_mut().push(format!("settings:{owner}:{target:?}"));
        self.settings_result.get()
    }

    fn show_recovery(&self, owner: Hwnd) {
        self.calls.borrow_mut().push(format!("recovery:{owner}"));
    }

    fn show_diagnostics(&self, owner: Hwnd) {
        self.calls.borrow_mut().push(format!("diagnostics:{owner}"));
    }

    fn show_benchmark(&self, owner: Hwnd, folder: Option<&Path>) {
        self.calls.borrow_mut().push(format!(
            "benchmark:{owner}:{}",
            folder.map_or("<none>".to_owned(), |p| p.to_string_lossy().into_owned())
        ));
    }

    fn show_skipped_files(&self, owner: Hwnd, entries: &[SkippedEntry]) {
        self.calls
            .borrow_mut()
            .push(format!("skipped:{owner}:{}", entries.len()));
    }

    fn show_batch_review(&self, owner: Hwnd, items: &[BatchReviewItem]) -> bool {
        self.calls.borrow_mut().push(format!("batch:{owner}:{}", items.len()));
        self.batch_result.get()
    }

    fn prompt_custom_zoom(&self, _owner: Hwnd, _current_percent: i32) -> Option<i32> {
        None
    }
}

/// A real `Win32DialogService` over fakes, behind the adapter, plus the knobs and the call log of the fakes.
struct DialogHarness {
    adapter: SharedDialogService,
    calls: Calls,
    task_result: Rc<Cell<i32>>,
    folder_answer: Rc<RefCell<Result<Option<String>, Hresult>>>,
    settings_result: Rc<Cell<bool>>,
    batch_result: Rc<Cell<bool>>,
}

fn dialog_harness() -> DialogHarness {
    let calls: Calls = Rc::default();
    let task_result = Rc::new(Cell::new(IDNO));
    let folder_answer = Rc::new(RefCell::new(Ok(None)));
    let settings_result = Rc::new(Cell::new(false));
    let batch_result = Rc::new(Cell::new(false));
    let port: Rc<dyn SecondaryWindowPort> = Rc::new(FakePort {
        calls: Rc::clone(&calls),
        settings_result: Rc::clone(&settings_result),
        batch_result: Rc::clone(&batch_result),
    });
    let picker = FolderPicker::new(
        Rc::new(FakeFolder {
            answer: Rc::clone(&folder_answer),
            calls: Rc::clone(&calls),
        }),
        Box::new(|| OWNER),
    );
    let service = Win32DialogService::with_task_dialogs(
        Box::new(|| OWNER),
        Box::new(FakeTaskDialogs {
            result: Rc::clone(&task_result),
            calls: Rc::clone(&calls),
        }),
        picker,
        Rc::new(LazySecondaryPort::from_value(Some(port))),
    );
    DialogHarness {
        adapter: SharedDialogService(Rc::new(service)),
        calls,
        task_result,
        folder_answer,
        settings_result,
        batch_result,
    }
}

impl DialogHarness {
    /// The calls recorded since the last `take`.
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.calls.borrow_mut())
    }
}

#[test]
fn shared_dialog_service_forwards_every_method_including_show_settings_at() {
    // new: every `DialogService` member reaches the wrapped `Win32DialogService` with its arguments and its answer
    let _ambient = ambient();
    let h = dialog_harness();
    let temp = TempRoot::new("w8-09b-s1-dialog");
    let dialog: &dyn DialogService = &h.adapter;

    // Confirmation: the answer follows the pressed button.
    h.task_result.set(IDYES);
    assert!(dialog.show_confirmation("Title {1}", "Question?"));
    assert_eq!(
        h.take(),
        vec![format!("task:{OWNER}:Confirmation:Title {{1}}:Question?")]
    );
    h.task_result.set(IDNO);
    assert!(!dialog.show_confirmation("Title", "Question?"));
    h.take();

    // Message and error use their own icon kind.
    dialog.show_message("M", "info text");
    dialog.show_error("E", "error text");
    assert_eq!(
        h.take(),
        vec![
            format!("task:{OWNER}:Information:M:info text"),
            format!("task:{OWNER}:Error:E:error text"),
        ]
    );

    // Folder picker: the initial folder and the answer both cross the adapter.
    *h.folder_answer.borrow_mut() = Ok(Some("X:\\picked".to_owned()));
    assert_eq!(dialog.pick_folder(Some(temp.path())), Some(PathBuf::from("X:\\picked")));
    assert_eq!(
        h.take(),
        vec![format!("folder:{OWNER}:{}", temp.path().to_string_lossy())]
    );
    *h.folder_answer.borrow_mut() = Ok(None);
    assert_eq!(dialog.pick_folder(None), None);
    assert_eq!(h.take(), vec![format!("folder:{OWNER}:<none>")]);

    // Batch review: the item count and the answer.
    let items = [
        BatchReviewItem {
            path: temp.join("a.jpg"),
            length: Some(1),
        },
        BatchReviewItem {
            path: temp.join("b.jpg"),
            length: None,
        },
    ];
    h.batch_result.set(true);
    assert!(dialog.show_batch_review(&items));
    h.batch_result.set(false);
    assert!(!dialog.show_batch_review(&items));
    assert_eq!(h.take(), vec![format!("batch:{OWNER}:2"), format!("batch:{OWNER}:2")]);

    // The windows without an answer.
    dialog.show_recovery();
    dialog.show_diagnostics();
    dialog.show_benchmark(Some(temp.path()));
    dialog.show_benchmark(None);
    dialog.show_skipped_files(&[SkippedEntry {
        path: temp.join("c.jpg"),
        reason: "locked".to_owned(),
        kind: SkippedKind::FileUnreadable,
    }]);
    assert_eq!(
        h.take(),
        vec![
            format!("recovery:{OWNER}"),
            format!("diagnostics:{OWNER}"),
            format!("benchmark:{OWNER}:{}", temp.path().to_string_lossy()),
            format!("benchmark:{OWNER}:<none>"),
            format!("skipped:{OWNER}:1"),
        ]
    );

    // Settings: `show_settings` opens on the default page; `show_settings_at` MUST keep the target (the trait default would drop it).
    h.settings_result.set(true);
    assert!(dialog.show_settings());
    assert!(dialog.show_settings_at(SettingsTarget::ExternalEditor));
    h.settings_result.set(false);
    assert!(!dialog.show_settings_at(SettingsTarget::Default));
    assert_eq!(
        h.take(),
        vec![
            format!("settings:{OWNER}:Default"),
            format!("settings:{OWNER}:ExternalEditor"),
            format!("settings:{OWNER}:Default"),
        ]
    );
}

// ---- SharedClipboard -------------------------------------------------------------------------------------------------------------------------------

struct FakeClipboard {
    texts: RefCell<Vec<String>>,
    answer: Cell<bool>,
}

impl ClipboardService for FakeClipboard {
    fn try_set_text(&self, text: &str) -> bool {
        self.texts.borrow_mut().push(text.to_owned());
        self.answer.get()
    }
}

#[test]
fn shared_clipboard_forwards() {
    // new: the text and the success flag cross the adapter both ways, and the shared `Rc` is the one that received the text
    let inner = Rc::new(FakeClipboard {
        texts: RefCell::new(Vec::new()),
        answer: Cell::new(true),
    });
    let shared: Rc<dyn ClipboardService> = Rc::clone(&inner) as Rc<dyn ClipboardService>;
    let clipboard = SharedClipboard(shared);

    assert!(clipboard.try_set_text("C:\\photos\\a {1}.jpg"));
    inner.answer.set(false);
    assert!(!clipboard.try_set_text(""));
    assert_eq!(
        *inner.texts.borrow(),
        vec!["C:\\photos\\a {1}.jpg".to_owned(), String::new()]
    );
}

// ---- Win32SinkFactoryAdapter -----------------------------------------------------------------------------------------------------------------------

#[test]
fn win32_sink_factory_adapter_forwards_every_callback_and_the_zoom_pull() {
    // C#: SharedServiceRegistrationTests.WpfPresentationSinkFactory_BuildsAWpfSinkThatForwardsTheCallbacks_AndKeepsTheMetrics
    // (Rust twin, new: the five closures of `MainViewModelSinkCallbacks` reach the sink in the order the sink promises, and the hub sees the image)
    let hub = Rc::new(ShellPresentationHub::new());
    let adapter = Win32SinkFactoryAdapter(Rc::new(Win32PresentationSinkFactory::new(Rc::clone(&hub))));
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let zoom_answers: Rc<RefCell<Vec<Option<Option<f64>>>>> = Rc::new(RefCell::new(vec![Some(Some(3.0)), None]));

    let callbacks = MainViewModelSinkCallbacks {
        set_current_image: {
            let log = Rc::clone(&log);
            let hub = Rc::clone(&hub);
            Box::new(move |image, is_file_change, change| {
                // The view model is told BEFORE the hub (ADR 0005 / C-16): the hub has no image yet.
                log.borrow_mut().push(format!(
                    "image:{}:{is_file_change}:{:?}:{}x{}:{}:hub_has_image={}",
                    image.is_some(),
                    change.presented_path,
                    change.original_width,
                    change.original_height,
                    change.is_same_source_swap,
                    hub.current_image().is_some()
                ));
            })
        },
        set_status_text: {
            let log = Rc::clone(&log);
            Box::new(move |text| log.borrow_mut().push(format!("status:{text}")))
        },
        apply_initial_view_mode: {
            let log = Rc::clone(&log);
            Box::new(move || log.borrow_mut().push("initial".to_owned()))
        },
        on_presented: {
            let log = Rc::clone(&log);
            Box::new(move |path| log.borrow_mut().push(format!("presented:{path}")))
        },
        take_viewer_zoom_change: {
            let zoom_answers = Rc::clone(&zoom_answers);
            Box::new(move || {
                let mut answers = zoom_answers.borrow_mut();
                if answers.is_empty() { None } else { answers.remove(0) }
            })
        },
    };
    let mut sink = adapter.create(callbacks);

    let change = ImageChange {
        presented_path: Some("C:\\p\\a.jpg"),
        original_width: 640,
        original_height: 480,
        is_same_source_swap: true,
    };
    sink.set_current_image(Some(PlatformImage::Opaque(Arc::new(7_u8))), true, &change);
    sink.set_status_text("3 of 9");
    sink.apply_initial_view_mode();
    sink.on_presented("C:\\p\\a.jpg");

    assert_eq!(
        *log.borrow(),
        vec![
            "image:true:true:Some(\"C:\\\\p\\\\a.jpg\"):640x480:true:hub_has_image=false".to_owned(),
            "status:3 of 9".to_owned(),
            "initial".to_owned(),
            "presented:C:\\p\\a.jpg".to_owned(),
        ]
    );
    // The hub (the second consumer of `set_current_image`) holds the very image the sink was given.
    assert_eq!(hub.image_change_count(), 1);
    match hub.current_image() {
        Some(PlatformImage::Opaque(value)) => assert_eq!(value.downcast_ref::<u8>(), Some(&7)),
        _ => panic!("the hub holds no opaque image"),
    }

    // The zoom pull is forwarded, one answer per call, `Some(None)` (= Fit) distinct from "no change".
    assert_eq!(sink.take_viewer_zoom_change(), Some(Some(3.0)));
    assert_eq!(sink.take_viewer_zoom_change(), None);
    assert_eq!(sink.take_viewer_zoom_change(), None);
}

// ---- LocalizedFormat -------------------------------------------------------------------------------------------------------------------------------

const OVERLAY_JSON: &str = r#"{
  "_meta": { "code": "xx", "name": "Test", "nativeName": "Test", "plural": "none" },
  "core.fileAction.busy": "XX busy"
}"#;

fn overlay_localizer() -> Arc<Localizer> {
    let mut warnings = Vec::new();
    let overlay = LanguageCatalog::try_parse(OVERLAY_JSON, "overlay", &mut warnings)
        .unwrap_or_else(|| panic!("overlay catalog is invalid: {}", warnings.join("; ")));
    Arc::new(Localizer::create(built_in_english(), &[overlay], &warnings))
}

/// What one `with_context` call saw: (text of `core.fileAction.busy`, negative sign, group separator).
fn observe(format: &LocalizedFormat) -> (String, String, String) {
    let mut seen = None;
    format.with_context(&mut |context| {
        seen = Some((
            context.texts.get("core.fileAction.busy"),
            context.numbers.negative_sign.clone(),
            context.group_separator.to_owned(),
        ));
    });
    seen.unwrap_or_else(|| panic!("with_context did not call `run`"))
}

#[test]
fn localized_format_uses_the_current_language_and_the_fixed_group_separator() {
    // new: the context is rebuilt from the ambient language and number culture on EVERY call; the group separator is the one given at construction
    let _ambient = ambient();
    let format = LocalizedFormat::with_group_separator("\u{a0}".to_owned());
    assert_eq!(
        observe(&format),
        (
            "A previous operation is still running.".to_owned(),
            "-".to_owned(),
            "\u{a0}".to_owned()
        )
    );

    // Switch the language and the number culture under the SAME value: the next call sees both.
    Localizer::set_current(overlay_localizer());
    set_number_culture(NumberCulture {
        negative_sign: "\u{2212}".to_owned(),
        ..NumberCulture::invariant()
    });
    assert_eq!(
        observe(&format),
        ("XX busy".to_owned(), "\u{2212}".to_owned(), "\u{a0}".to_owned())
    );

    // Another value keeps its own separator (nothing is shared between instances).
    assert_eq!(observe(&LocalizedFormat::with_group_separator(String::new())).2, "");
}
