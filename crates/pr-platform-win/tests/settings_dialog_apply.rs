//! SettingsDialog queue and forwarding rules, C# SettingsDialog.cs 909..1014 (W8-19l).
use pr_platform_win::dialogs::settings::context::{SettingsPageContext, SettingsUiPost};
use pr_platform_win::dialogs::settings::dialog::messages::{WindowPoster, is_forwarded, wheel_delta};
use std::sync::{Arc, Mutex};

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
