//! Startup warm-up behavior tests ported from ShellWarmup.cs.

use std::cell::RefCell;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use pr_core::settings::app_settings::EnglishSettingsTexts;
use pr_platform_win::render::image_cache::policy::{StripeScheduler, StripeToken};
use pr_platform_win::shell_startup::warmup::{
    NoStripeScheduler, WarmupFailed, warm_graphics_now, warm_settings_parser, warm_settings_parser_now,
};

// C#: ShellWarmup.WarmSettingsParser
#[test]
fn warm_settings_parser_now_marks_after_a_successful_parse() {
    let marks = RefCell::new(Vec::new());
    warm_settings_parser_now(&EnglishSettingsTexts, &|mark| marks.borrow_mut().push(mark.to_owned()));
    assert_eq!(*marks.borrow(), ["settingsParserWarm"]);
}

#[test]
fn no_stripe_scheduler_does_nothing() {
    let result = std::panic::catch_unwind(|| {
        NoStripeScheduler.schedule_stripe(StripeToken {
            pixel_buffer_id: 10,
            serial: 20,
        })
    });
    assert!(result.is_ok(), "no-stripe scheduling must be a no-op");
}
#[test]
fn warm_settings_parser_runs_on_a_worker_and_sends_the_mark() {
    let (sender, receiver) = mpsc::channel();
    let handle = warm_settings_parser(
        Arc::new(EnglishSettingsTexts),
        Box::new(move |mark| {
            sender.send(mark.to_owned()).unwrap();
        }),
    );
    assert!(handle.is_some());
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(30)).unwrap(),
        "settingsParserWarm"
    );
    handle.unwrap().join().unwrap();
}

// C#: ShellWarmup.WarmGraphics
#[test]
fn warm_graphics_now_reports_success_or_a_supported_fallback() {
    let marks = RefCell::new(Vec::new());
    let result = warm_graphics_now(&|mark| marks.borrow_mut().push(mark.to_owned()));
    match result {
        Ok(()) => assert_eq!(*marks.borrow(), ["graphicsWarm"]),
        Err(WarmupFailed) => assert!(marks.borrow().is_empty()),
    }
}

// C#: ShellWarmup.WarmGraphics
#[test]
#[ignore = "native: exercises Direct2D and DirectWrite device initialization"]
fn warm_graphics_now_initializes_native_rendering() {
    let marks = RefCell::new(Vec::new());
    assert_eq!(
        warm_graphics_now(&|mark| marks.borrow_mut().push(mark.to_owned())),
        Ok(())
    );
    assert_eq!(*marks.borrow(), ["graphicsWarm"]);
}
