//! SettingsDialog queue and forwarding rules, C# SettingsDialog.cs 909..1014 (W8-19l).
use pr_platform_win::dialogs::settings::context::{SettingsPageContext, SettingsUiPost};
use pr_platform_win::dialogs::settings::dialog::messages::{WindowPoster, is_forwarded, wheel_delta};
use std::sync::{Arc, Mutex};
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

struct Files {
    calls: std::cell::RefCell<Vec<serde_json::Value>>,
    path: Option<String>,
}
impl pr_platform_win::dialogs::file_dialog::ShellFileDialog for Files {
    fn pick_open_file(
        &self,
        _: isize,
        title: &str,
        filters: &[pr_platform_win::dialogs::file_dialog::FileDialogFilter],
        initial: Option<&str>,
    ) -> Result<Option<String>, pr_platform_win::win32::structs::Hresult> {
        self.calls.borrow_mut().push(serde_json::json!({"call":"open","title":title,"filters":filters.iter().map(|f|serde_json::json!({"name":f.name,"pattern":f.pattern})).collect::<Vec<_>>(),"initial_folder_set":initial.is_some()}));
        Ok(self.path.clone())
    }
    fn pick_save_file(
        &self,
        _: isize,
        title: &str,
        filters: &[pr_platform_win::dialogs::file_dialog::FileDialogFilter],
        extension: Option<&str>,
        name: Option<&str>,
        initial: Option<&str>,
    ) -> Result<Option<String>, pr_platform_win::win32::structs::Hresult> {
        self.calls.borrow_mut().push(serde_json::json!({"call":"save","title":title,"filters":filters.iter().map(|f|serde_json::json!({"name":f.name,"pattern":f.pattern})).collect::<Vec<_>>(),"default_extension":extension,"suggested_file_name":name,"initial_folder_set":initial.is_some()}));
        Ok(self.path.clone())
    }
}

#[test]
fn every_apply_case_matches_the_csharp_golden() {
    use pr_platform_win::dialogs::native::dialog::core::DialogContent;
    use pr_platform_win::dialogs::settings::dialog::{SettingsDialog, frame::*};
    use std::rc::Rc;
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/settings-dialog-apply.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 14);
    for row in cases {
        let root = pr_test_support::TempRoot::new("settings-dialog-apply");
        let path = root.join("settings.json");
        let mut args = fixture::args();
        if let Some(logging) = row["settings"]["LoggingEnabled"].as_bool() {
            args.current.logging_enabled = logging;
        }
        if let Some(shortcuts) = row["settings"]["Shortcuts"].as_object() {
            for (name, value) in shortcuts {
                *args.current.shortcuts.get_mut(name).unwrap() = value.as_str().unwrap().to_owned();
            }
        }
        let warnings = Rc::new(fixture::Warnings::default());
        args.options.warnings = Some(warnings.clone());
        let files = Rc::new(Files {
            calls: Default::default(),
            path: if row["save_result"] == "file" || row["pick"] == "file" {
                Some(path.to_str().unwrap().to_owned())
            } else {
                None
            },
        });
        if row["files"] == true {
            args.options.files = Some(files.clone());
        }
        if let Some(text) = row["file_text"].as_str() {
            std::fs::write(&path, text).unwrap();
        }
        let dialog = SettingsDialog::new(args).unwrap();
        match row["kind"].as_str().unwrap() {
            "focus_targets" => {
                use pr_app::presenters::settings_form_state::SettingsFocusField as F;
                use pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder;
                let mut pages = pr_platform_win::dialogs::settings::catalog::create_pages();
                for (i, page) in pages.iter_mut().enumerate() {
                    page.declare(&mut SettingsPageBuilder::new(page_id_base(i as i32)));
                }
                for target in row["targets"].as_array().unwrap() {
                    let field = match target["field"].as_str().unwrap() {
                        "None" => F::None,
                        "ArrowPanStep" => F::ArrowPanStep,
                        "KeyboardZoomStep" => F::KeyboardZoomStep,
                        "ImageTransitionMs" => F::ImageTransitionMs,
                        "TouchpadSwipeDistance" => F::TouchpadSwipeDistance,
                        "PreloadForward" => F::PreloadForward,
                        "PreloadBackward" => F::PreloadBackward,
                        "InfoOverlayAutoHideDelay" => F::InfoOverlayAutoHideDelay,
                        other => panic!("unknown focus {other}"),
                    };
                    let actual = pages
                        .iter()
                        .enumerate()
                        .find_map(|(i, p)| p.focus_target(field).map(|id| (i as i64, i64::from(id))));
                    let expected = target["page"].as_i64().map(|i| (i, target["id"].as_i64().unwrap()));
                    assert_eq!(actual, expected);
                }
            }
            "save_flow" => {
                let actual: Vec<_> = (0..row["calls"].as_u64().unwrap())
                    .map(|_| serde_json::Value::Bool(dialog.try_save_all()))
                    .collect();
                assert_eq!(actual, *row["results"].as_array().unwrap(), "{}", row["name"]);
                assert_eq!(dialog.saved(), row["saved"].as_bool().unwrap());
            }
            "export" => {
                assert!(DialogContent::on_command(
                    dialog.content().as_ref(),
                    dialog.native(),
                    EXPORT_ID,
                    0
                ));
                assert_eq!(path.exists(), row["file_written"].as_bool().unwrap());
                if path.exists() {
                    let written: serde_json::Value =
                        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                    assert_eq!(written["LoggingEnabled"], row["logging_enabled_in_file"]);
                }
            }
            "import" => {
                if row["mode"] == "from_path" {
                    dialog.import_settings_from(&path);
                } else {
                    assert!(DialogContent::on_command(
                        dialog.content().as_ref(),
                        dialog.native(),
                        IMPORT_ID,
                        0
                    ));
                }
                assert_eq!(
                    dialog.settings().logging_enabled,
                    row["logging_enabled"].as_bool().unwrap()
                );
                if row["file_read"] == true {
                    assert_eq!(
                        std::fs::read_to_string(&path).unwrap(),
                        row["file_text"].as_str().unwrap()
                    );
                }
            }
            other => panic!("unhandled kind {other}"),
        }
        let expected = row["warnings"].as_array();
        if let Some(expected) = expected {
            let observed = warnings.0.borrow();
            assert_eq!(observed.len(), expected.len(), "{}", row["name"]);
            for ((title, message), expected) in observed.iter().zip(expected) {
                assert_eq!(title, expected["title"].as_str().unwrap());
                if let Some(nonempty) = expected["message_nonempty"].as_bool() {
                    assert_eq!(!message.is_empty(), nonempty);
                }
                if let Some(text) = expected["message"].as_str() {
                    assert_eq!(message, text);
                }
            }
        }
        if row["calls"].is_array() {
            assert_eq!(
                *files.calls.borrow(),
                *row["calls"].as_array().unwrap(),
                "{}",
                row["name"]
            );
        }
    }
}

#[test]
fn new_poster_drops_posts_and_close_discards_pending_actions() {
    let poster = WindowPoster::new();
    assert_eq!(poster.hwnd(), 0);
    poster.post(Box::new(|_| panic!("detached post must be dropped")));
    assert_eq!(poster.pending(), 0);
    poster.set_hwnd(0x1234);
    poster.post(Box::new(|_| panic!("close must drop this action")));
    assert_eq!(poster.pending(), 1);
    poster.set_hwnd(0);
    assert_eq!(poster.pending(), 0);
    assert!(poster.drain().is_empty());
}

#[test]
fn posts_are_fifo_and_an_action_posting_more_waits_for_the_next_drain() {
    let poster = Arc::new(WindowPoster::new());
    poster.set_hwnd(0x1234);
    let observed = Arc::new(Mutex::new(Vec::new()));
    for value in 0..3 {
        let observed = observed.clone();
        let poster_for_action = poster.clone();
        poster.post(Box::new(move |_| {
            observed.lock().unwrap().push(value);
            if value == 0 {
                poster_for_action.post(Box::new(|_| {}));
            }
        }));
    }
    assert_eq!(poster.pending(), 3);
    let batch = poster.drain();
    assert_eq!(poster.pending(), 0);
    let context = SettingsPageContext::detached();
    for action in batch {
        action(&context);
    }
    assert_eq!(*observed.lock().unwrap(), vec![0, 1, 2]);
    assert_eq!(poster.pending(), 1);
    assert_eq!(poster.drain().len(), 1);
    assert!(poster.drain().is_empty());
}

#[test]
fn post_from_four_threads_loses_nothing() {
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<WindowPoster>();
    let poster = Arc::new(WindowPoster::new());
    poster.set_hwnd(0x1234);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let poster = poster.clone();
            scope.spawn(move || {
                for _ in 0..250 {
                    poster.post(Box::new(|_| {}));
                }
            });
        }
    });
    assert_eq!(poster.pending(), 1000);
    assert_eq!(poster.drain().len(), 1000);
    assert_eq!(poster.pending(), 0);
}

#[test]
fn wheel_delta_is_a_signed_high_word_and_forwarding_keeps_combo_dropdown_ownership() {
    assert_eq!(wheel_delta(0x0078_0000), 120);
    assert_eq!(wheel_delta(0xff88_ffff), -120);
    assert_eq!(wheel_delta(0x8000_0000), -32768);
    assert_eq!(wheel_delta(0x7fff_0000), 32767);
    for wheel_only in [true, false] {
        assert!(is_forwarded(0x20a, wheel_only, false));
        assert!(!is_forwarded(0x20a, wheel_only, true));
    }
    for message in [
        0x111, 0x4e, 0x2b, 0x2c, 0x114, 0x115, 0x132, 0x133, 0x134, 0x135, 0x136, 0x137, 0x138,
    ] {
        assert!(is_forwarded(message, false, false), "message={message:#x}");
        assert!(!is_forwarded(message, true, false), "message={message:#x}");
    }
    for message in [0, 0x100, 0x131, 0x139] {
        assert!(!is_forwarded(message, false, false));
    }
}

#[test]
fn requeue_front_keeps_the_unrun_rest_in_order_ahead_of_newer_posts_and_dies_with_the_window() {
    // ST-WIN32 L13: a panicking action ends the drained batch; the rest goes back to the front (C# TryDequeue loop leaves it queued).
    let poster = WindowPoster::new();
    poster.set_hwnd(0x1234);
    let observed = Arc::new(Mutex::new(Vec::new()));
    for value in 0..4 {
        let observed = observed.clone();
        poster.post(Box::new(move |_| observed.lock().unwrap().push(value)));
    }
    let mut batch = poster.drain();
    assert_eq!(batch.len(), 4);
    let context = SettingsPageContext::detached();
    batch.remove(0)(&context); // action 0 ran, "action 1 panicked" and was consumed
    let _consumed = batch.remove(0);
    let observed_late = observed.clone();
    poster.post(Box::new(move |_| observed_late.lock().unwrap().push(99)));
    poster.requeue_front(batch);
    assert_eq!(poster.pending(), 3);
    for action in poster.drain() {
        action(&context);
    }
    assert_eq!(*observed.lock().unwrap(), vec![0, 2, 3, 99]);

    poster.set_hwnd(0);
    poster.requeue_front(vec![Box::new(|_| panic!("a closed window must not run requeued work"))]);
    assert_eq!(poster.pending(), 0);
}
