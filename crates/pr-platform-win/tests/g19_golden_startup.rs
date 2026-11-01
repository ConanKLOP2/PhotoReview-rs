//! G18 (docs/reviews/pr-164.md M1): the `startup-cloak` and `shell-show-command` goldens are compared case by case
//! (the older `*_golden_replays` tests only looked for a word in the file). Fakes only, no sleeping, no window.
use pr_app::windowing::contracts::WindowShowState;
use pr_core::abstractions::{UiCompletion, UiScheduler};
use pr_platform_win::shell_startup::cloak::{SAFETY_TIMEOUT, StartupCloak, StartupCloakOptions, TimerHandle};
use pr_platform_win::shell_startup::placement::show_command_for;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn shell_show_command_matches_every_golden_case() {
    let g = golden::load("shell-show-command");
    let mut seen = 0;
    for case in &g.cases {
        let state = match case["state"].as_str() {
            None => None,
            Some("Maximized") => Some(WindowShowState::Maximized),
            Some("Normal") => Some(WindowShowState::Normal),
            Some("Minimized") => Some(WindowShowState::Minimized),
            Some(other) => panic!("unknown state {other}"),
        };
        let got = show_command_for(state, bool_of(case, "activate"));
        assert_eq!(i64::from(got), i64_of(case, "result"), "case {}", str_of(case, "id"));
        seen += 1;
    }
    assert_eq!(seen, 8);
}

#[derive(Default)]
struct Queue(Mutex<Vec<Box<dyn FnOnce() + Send>>>);
impl UiScheduler for Queue {
    fn post(&self, job: Box<dyn FnOnce() + Send>) {
        self.0.lock().unwrap().push(job);
    }
    fn invoke(&self, job: Box<dyn FnOnce() + Send>) -> UiCompletion {
        let (tx, done) = UiCompletion::channel();
        job();
        let _ = tx.send(Ok(()));
        done
    }
}

struct Disposed(Arc<Mutex<bool>>);
impl TimerHandle for Disposed {}
impl Drop for Disposed {
    fn drop(&mut self) {
        *self.0.lock().unwrap() = true;
    }
}

#[derive(Default)]
struct Timer {
    job: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    delays_ms: Mutex<Vec<u128>>,
    disposed: Arc<Mutex<bool>>,
}

fn replay(case: &Value) {
    let id = str_of(case, "id");
    let accept = bool_of(case, "accept");
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let revealed = Arc::new(Mutex::new(0usize));
    let timer = Arc::new(Timer::default());
    let queue = Arc::new(Queue::default());
    let (c, r, t) = (calls.clone(), revealed.clone(), timer.clone());
    let cloak = StartupCloak::new(
        queue.clone(),
        StartupCloakOptions {
            on_revealed: Some(Arc::new(move || *r.lock().unwrap() += 1)),
            set_cloaked: Some(Arc::new(move |h, on| {
                c.lock().unwrap().push(format!("set:{h}:{on}"));
                accept
            })),
            safety_timeout: Some(
                case["timeout_ms"]
                    .as_u64()
                    .map_or(SAFETY_TIMEOUT, Duration::from_millis),
            ),
            schedule_timeout: Some(Arc::new(move |delay, job| {
                t.delays_ms.lock().unwrap().push(delay.as_millis());
                *t.job.lock().unwrap() = Some(job);
                *t.disposed.lock().unwrap() = false;
                Box::new(Disposed(t.disposed.clone()))
            })),
        },
    );
    let states = case["states"].as_array().unwrap();
    let ops = case["ops"].as_array().unwrap();
    assert_eq!(states.len(), ops.len(), "case {id}");
    for (op, want) in ops.iter().zip(states) {
        let op = op.as_str().unwrap();
        let mut fire_ran = None;
        match op {
            "note_first_present" => cloak.note_first_present(),
            "reveal" => cloak.reveal(),
            "stop" | "dispose" => cloak.stop(),
            "fire" => {
                // The fake timer thread: runs the stored job only when the handle was not disposed first.
                let job = timer.job.lock().unwrap().take();
                let ran = job.is_some() && !*timer.disposed.lock().unwrap();
                if ran {
                    job.unwrap()();
                    let jobs = std::mem::take(&mut *queue.0.lock().unwrap());
                    for j in jobs {
                        j();
                    }
                }
                fire_ran = Some(ran);
            }
            _ => cloak.hide(
                op.strip_prefix("hide:")
                    .unwrap_or_else(|| panic!("op {op}"))
                    .parse()
                    .unwrap(),
            ),
        }
        let at = format!("case {id} after {op}");
        assert_eq!(cloak.is_hidden(), bool_of(want, "is_hidden"), "{at}: is_hidden");
        assert_eq!(cloak.is_revealed(), bool_of(want, "is_revealed"), "{at}: is_revealed");
        assert_eq!(
            *revealed.lock().unwrap() as i64,
            i64_of(want, "revealed_count"),
            "{at}: revealed_count"
        );
        if let Some(expected) = want["fire_ran"].as_bool() {
            assert_eq!(fire_ran, Some(expected), "{at}: fire_ran");
        }
    }
    let want_calls: Vec<String> = case["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(*calls.lock().unwrap(), want_calls, "case {id}: calls");
    let want_delays: Vec<u128> = case["delays_ms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| u128::from(v.as_u64().unwrap()))
        .collect();
    assert_eq!(*timer.delays_ms.lock().unwrap(), want_delays, "case {id}: delays_ms");
    assert_eq!(
        *timer.disposed.lock().unwrap(),
        bool_of(case, "timer_disposed"),
        "case {id}: timer_disposed"
    );
}

#[test]
fn startup_cloak_replays_every_golden_script() {
    let g = golden::load("startup-cloak");
    let mut scripts = 0;
    for case in &g.cases {
        match case["kind"].as_str() {
            Some("script") => {
                replay(case);
                scripts += 1;
            }
            Some("constants") => assert_eq!(i64_of(case, "safety_timeout_ms") as u128, SAFETY_TIMEOUT.as_millis()),
            other => panic!("unknown kind {other:?}"),
        }
    }
    assert_eq!(scripts, 15);
}
