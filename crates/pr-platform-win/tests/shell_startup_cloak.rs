use pr_core::abstractions::UiScheduler;
use pr_platform_win::shell_startup::cloak::{
    SAFETY_TIMEOUT, StartupCloak, StartupCloakOptions, TimerHandle, try_set_cloaked,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Scheduler {
    queue: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
}
impl UiScheduler for Scheduler {
    fn post(&self, job: Box<dyn FnOnce() + Send>) {
        self.queue.lock().unwrap().push(job);
    }
    fn invoke(&self, job: Box<dyn FnOnce() + Send>) -> pr_core::abstractions::UiCompletion {
        let (tx, completion) = pr_core::abstractions::UiCompletion::channel();
        job();
        let _ = tx.send(Ok(()));
        completion
    }
}
impl Scheduler {
    fn run(&self) {
        let jobs = std::mem::take(&mut *self.queue.lock().unwrap());
        for job in jobs {
            job();
        }
    }
}

#[derive(Default)]
struct Record {
    calls: Mutex<Vec<(isize, bool)>>,
    accept: bool,
}
impl Record {
    fn set(&self, hwnd: isize, cloaked: bool) -> bool {
        self.calls.lock().unwrap().push((hwnd, cloaked));
        self.accept
    }
}

#[derive(Default)]
struct Manual {
    job: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    canceled: Arc<Mutex<bool>>,
}
struct Handle(Arc<Mutex<bool>>);
impl TimerHandle for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        *self.0.lock().unwrap() = true;
    }
}
fn opts(r: Arc<Record>, m: Arc<Manual>, revealed: Arc<Mutex<usize>>, scheduler: Duration) -> StartupCloakOptions {
    StartupCloakOptions {
        on_revealed: Some(Arc::new(move || *revealed.lock().unwrap() += 1)),
        set_cloaked: Some(Arc::new(move |h, c| r.set(h, c))),
        safety_timeout: Some(scheduler),
        schedule_timeout: Some(Arc::new(move |_, job| {
            *m.job.lock().unwrap() = Some(job);
            Box::new(Handle(m.canceled.clone()))
        })),
    }
}

// C#: StartupCloakTests.Hide_CloaksTheWindowOnceAndKeepsItHiddenUntilTheFirstPresent
#[test]
fn hide_cloaks_once_and_keeps_hidden_until_first_present() {
    let r = Arc::new(Record {
        accept: true,
        ..Record::default()
    });
    let m = Arc::new(Manual::default());
    let shown = Arc::new(Mutex::new(0));
    let c = StartupCloak::new(
        Arc::new(Scheduler::default()),
        opts(r.clone(), m, shown, SAFETY_TIMEOUT),
    );
    c.hide(42);
    c.hide(42);
    assert!(c.is_hidden());
    assert_eq!(*r.calls.lock().unwrap(), [(42, true)]);
}
// C#: StartupCloakTests.NoteFirstPresent_UncloaksOnceAndReportsTheReveal
#[test]
fn note_first_present_uncloaks_once_and_reports_reveal() {
    let r = Arc::new(Record {
        accept: true,
        ..Record::default()
    });
    let m = Arc::new(Manual::default());
    let shown = Arc::new(Mutex::new(0));
    let c = StartupCloak::new(
        Arc::new(Scheduler::default()),
        opts(r.clone(), m, shown.clone(), SAFETY_TIMEOUT),
    );
    c.hide(42);
    c.note_first_present();
    c.note_first_present();
    assert!(!c.is_hidden());
    assert!(c.is_revealed());
    assert_eq!(*shown.lock().unwrap(), 1);
    assert_eq!(*r.calls.lock().unwrap(), [(42, true), (42, false)]);
}
// C#: StartupCloakTests.Hide_WhenDwmRefuses_LeavesTheWindowVisibleAndNeverReports
#[test]
fn hide_when_dwm_refuses_leaves_visible_and_never_reports() {
    let r = Arc::new(Record::default());
    let m = Arc::new(Manual::default());
    let shown = Arc::new(Mutex::new(0));
    let c = StartupCloak::new(
        Arc::new(Scheduler::default()),
        opts(r.clone(), m, shown.clone(), SAFETY_TIMEOUT),
    );
    c.hide(42);
    c.note_first_present();
    assert!(!c.is_hidden());
    assert_eq!(*shown.lock().unwrap(), 0);
    assert_eq!(*r.calls.lock().unwrap(), [(42, true)]);
}
// C#: StartupCloakTests.Hide_WithNoHandleOrAfterReveal_DoesNothing
#[test]
fn hide_with_no_handle_or_after_reveal_does_nothing() {
    let r = Arc::new(Record {
        accept: true,
        ..Record::default()
    });
    let m = Arc::new(Manual::default());
    let shown = Arc::new(Mutex::new(0));
    let c = StartupCloak::new(
        Arc::new(Scheduler::default()),
        opts(r.clone(), m, shown, SAFETY_TIMEOUT),
    );
    c.hide(0);
    c.reveal();
    c.hide(42);
    assert!(r.calls.lock().unwrap().is_empty());
    assert!(!c.is_hidden());
}

// C#: StartupCloakTests.SafetyTimeout_RevealsAWindowThatNeverGetsAFrame
#[test]
fn safety_timeout_reveals_window_that_never_gets_a_frame() {
    let r = Arc::new(Record {
        accept: true,
        ..Record::default()
    });
    let m = Arc::new(Manual::default());
    let shown = Arc::new(Mutex::new(0));
    let scheduler = Arc::new(Scheduler::default());
    let c = StartupCloak::new(
        scheduler.clone(),
        opts(r.clone(), m.clone(), shown.clone(), Duration::from_millis(50)),
    );
    c.hide(7);
    assert!(c.is_hidden());
    let job = m.job.lock().unwrap().take().unwrap();
    job();
    assert!(c.is_hidden());
    assert_eq!(*shown.lock().unwrap(), 0);
    scheduler.run();
    assert!(!c.is_hidden());
    assert!(c.is_revealed());
    assert_eq!(*shown.lock().unwrap(), 1);
    assert_eq!(*r.calls.lock().unwrap(), [(7, true), (7, false)]);
}
// C#: StartupCloakTests.SafetyTimeout_AfterTheFirstPresent_DoesNotRevealASecondTime
#[test]
fn safety_timeout_after_first_present_does_not_reveal_twice() {
    let r = Arc::new(Record {
        accept: true,
        ..Record::default()
    });
    let m = Arc::new(Manual::default());
    let shown = Arc::new(Mutex::new(0));
    let c = StartupCloak::new(
        Arc::new(Scheduler::default()),
        opts(r.clone(), m.clone(), shown.clone(), SAFETY_TIMEOUT),
    );
    c.hide(7);
    c.note_first_present();
    assert!(*m.canceled.lock().unwrap());
    assert_eq!(*shown.lock().unwrap(), 1);
    assert_eq!(*r.calls.lock().unwrap(), [(7, true), (7, false)]);
}
// C#: StartupCloakTests.Stop_CancelsTheSafetyTimeoutSoAClosedWindowIsNeverTouched
#[test]
fn stop_cancels_timeout_without_touching_closed_window() {
    let r = Arc::new(Record {
        accept: true,
        ..Record::default()
    });
    let m = Arc::new(Manual::default());
    let shown = Arc::new(Mutex::new(0));
    let c = StartupCloak::new(
        Arc::new(Scheduler::default()),
        opts(r.clone(), m.clone(), shown.clone(), SAFETY_TIMEOUT),
    );
    c.hide(7);
    c.stop();
    assert!(*m.canceled.lock().unwrap());
    assert_eq!(*shown.lock().unwrap(), 0);
    assert!(c.is_hidden());
    assert_eq!(*r.calls.lock().unwrap(), [(7, true)]);
}
// C#: StartupCloakTests.SafetyTimeoutDefault_IsThreeSeconds
#[test]
fn safety_timeout_default_is_three_seconds() {
    assert_eq!(SAFETY_TIMEOUT, Duration::from_secs(3));
    assert_eq!(StartupCloakOptions::default().safety_timeout, None);
}

#[test]
fn try_set_cloaked_with_handle_zero_is_false() {
    assert!(!try_set_cloaked(0, true));
    assert!(!try_set_cloaked(0, false));
}

#[test]
fn start_cloak_defaults_use_documented_constants() {
    let options = StartupCloakOptions::default();
    assert_eq!(options.safety_timeout, None);
    assert_eq!(SAFETY_TIMEOUT, Duration::from_secs(3));
}

#[test]
fn deadline_job_posts_reveal_to_scheduler_instead_of_running_on_timer_thread() {
    let scheduler = Arc::new(Scheduler::default());
    let r = Arc::new(Record {
        accept: true,
        ..Record::default()
    });
    let m = Arc::new(Manual::default());
    let shown = Arc::new(Mutex::new(0));
    let c = StartupCloak::new(
        scheduler.clone(),
        opts(r.clone(), m.clone(), shown.clone(), SAFETY_TIMEOUT),
    );
    c.hide(9);
    m.job.lock().unwrap().take().unwrap()();
    assert!(c.is_hidden());
    assert_eq!(*shown.lock().unwrap(), 0);
    scheduler.run();
    assert!(!c.is_hidden());
    assert_eq!(*shown.lock().unwrap(), 1);
}

#[test]
fn dropping_cloak_cancels_deadline() {
    let r = Arc::new(Record {
        accept: true,
        ..Record::default()
    });
    let m = Arc::new(Manual::default());
    let shown = Arc::new(Mutex::new(0));
    let c = StartupCloak::new(
        Arc::new(Scheduler::default()),
        opts(r, m.clone(), shown, SAFETY_TIMEOUT),
    );
    c.hide(7);
    drop(c);
    assert!(*m.canceled.lock().unwrap());
}

#[test]
fn a_deadline_firing_after_cloak_was_dropped_reveals_once_without_panic() {
    let scheduler = Arc::new(Scheduler::default());
    let r = Arc::new(Record {
        accept: true,
        ..Record::default()
    });
    let m = Arc::new(Manual::default());
    let shown = Arc::new(Mutex::new(0));
    let c = StartupCloak::new(
        scheduler.clone(),
        opts(r.clone(), m.clone(), shown.clone(), SAFETY_TIMEOUT),
    );
    c.hide(7);
    let job = m.job.lock().unwrap().take().unwrap();
    drop(c);
    job();
    scheduler.run();
    assert_eq!(*shown.lock().unwrap(), 1);
    assert_eq!(*r.calls.lock().unwrap(), [(7, true), (7, false)]);
}

#[test]
fn startup_cloak_golden_replays() {
    let text = include_str!("../../../tests/golden/startup-cloak.json");
    assert!(text.contains("startup-cloak"));
    assert!(text.contains("hide"));
}
