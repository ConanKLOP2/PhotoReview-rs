//! Review-confirm tests of group G13 (`docs/reviews/G7-G15-2026-11-03.md`), pr-164 L5: a forwarded open whose router was dropped (the attachment is only a `Weak`)
//! is logged, never lost silently. New (Rust only): C# `dispatcher.Post(() => OnForwardedOpen(path))` cannot lose the request this way.
use std::error::Error;
use std::sync::{Arc, Mutex};

use pr_app::startup::forwarded_open::{ForwardedOpenRouter, ui_open_callback_logged};
use pr_core::abstractions::{Log, UiCompletion, UiScheduler};

struct InlineUi;
impl UiScheduler for InlineUi {
    fn post(&self, job: Box<dyn FnOnce() + Send>) {
        job();
    }
    fn invoke(&self, job: Box<dyn FnOnce() + Send>) -> UiCompletion {
        let (tx, done) = UiCompletion::channel();
        job();
        let _ = tx.send(Ok(()));
        done
    }
}

#[derive(Default)]
struct WarnLog(Mutex<Vec<String>>);
impl Log for WarnLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _: &str) {}
    fn warn(&self, message: &str) {
        self.0.lock().expect("log").push(message.to_owned());
    }
    fn error(&self, _: &str, _: Option<&(dyn Error + 'static)>) {}
}

#[test]
fn forwarded_open_after_the_attached_router_was_dropped_is_logged_not_lost_silently() {
    let log = Arc::new(WarnLog::default());
    let callback = ui_open_callback_logged(Arc::new(InlineUi), log.clone());
    let router = ForwardedOpenRouter::new();
    router.attach_to_current_thread();
    drop(router);

    callback(Some("late.jpg".to_owned()));

    assert_eq!(
        *log.0.lock().expect("log"),
        ["Forwarded launch: no router is attached to the UI thread; the request was dropped"]
    );
}
