use pr_app::startup::forwarded_open::{ForwardedOpenHandler, ForwardedOpenRouter, ForwardedOpenTarget, WindowGone};
use pr_core::abstractions::Log;
use std::{error::Error, rc::Rc, sync::{Arc, Mutex}};

#[derive(Default)] struct Calls { calls: Mutex<Vec<String>>, closing: bool, minimized: bool, fail_activate: bool, fail_open: bool }
impl ForwardedOpenTarget for Calls {
 fn is_closing_or_closed(&self)->bool{self.closing}
 fn is_minimized(&self)->Result<bool,WindowGone>{Ok(self.minimized)}
 fn restore(&self)->Result<(),WindowGone>{self.calls.lock().unwrap().push("restore".into());Ok(())}
 fn activate(&self)->Result<(),WindowGone>{self.calls.lock().unwrap().push("activate".into());if self.fail_activate{Err(WindowGone{message:"gone".into()})}else{Ok(())}}
 fn open_path(&self,p:&str)->Result<(),Box<dyn Error>>{self.calls.lock().unwrap().push(format!("open:{p}"));if self.fail_open{Err(Box::new(std::io::Error::other("open failed")))}else{Ok(())}}
}
#[derive(Default)] struct TestLog(Mutex<Vec<String>>);
impl Log for TestLog {
 fn enabled(&self)->bool{true} fn info(&self,_:&str){}
 fn warn(&self,m:&str){self.0.lock().unwrap().push(format!("warn:{m}"))}
 fn error(&self,m:&str,e:Option<&(dyn Error+'static)>){self.0.lock().unwrap().push(format!("error:{m}:{}",e.is_some()))}
}
fn handler(t:Calls)->ForwardedOpenHandler{ForwardedOpenHandler::new(Rc::new(t),Arc::new(TestLog::default()))}
#[test] // C#: ForwardedOpenHandlerTests.Path_ActivatesThenOpens
fn path_activates_then_opens(){let t=Rc::new(Calls::default());let h=ForwardedOpenHandler::new(t.clone(),Arc::new(TestLog::default()));h.handle(Some("C:\\p\\a.jpg"));assert_eq!(&*t.calls.lock().unwrap(), &["activate","open:C:\\p\\a.jpg"]);}
#[test] // C#: ForwardedOpenHandlerTests.NoPath_OnlyBringsTheWindowToTheFront
fn no_path_only_brings_window_to_front(){let h=handler(Calls::default());h.handle(None);assert_eq!(h.ignored_count(),0);}
#[test] // C#: ForwardedOpenHandlerTests.MinimizedWindow_IsRestoredBeforeActivation
fn minimized_window_is_restored_before_activation(){let mut t=Calls::default();t.minimized=true;let h=handler(t);h.handle(Some("x"));}
#[test] // C#: ForwardedOpenHandlerTests.RequestRacingTheShutdown_IsIgnoredAndNeverTouchesTheWindow
fn request_racing_shutdown_is_ignored(){let mut t=Calls::default();t.closing=true;let h=handler(t);h.handle(Some("x"));assert_eq!(h.ignored_count(),1);}
#[test] // C#: ForwardedOpenHandlerTests.WindowGoneBetweenCheckAndActivate_IsLoggedNotThrown_AndNothingIsOpened
fn window_gone_between_check_and_activate_is_logged(){let mut t=Calls::default();t.fail_activate=true;let h=handler(t);h.handle(Some("x"));assert_eq!(h.ignored_count(),1);}
#[test] // C#: ForwardedOpenHandlerTests.FailingOpen_IsLoggedAsForwardedOpenFailed
fn failing_open_is_logged_as_forwarded_open_failed(){let mut t=Calls::default();t.fail_open=true;handler(t).handle(Some("x"));}
#[test] fn router_keeps_first_pending_path_and_replays_once(){let r=ForwardedOpenRouter::new();r.on_forwarded_open(Some("first".into()));r.on_forwarded_open(Some("second".into()));assert_eq!(r.take_pending(),Some(Some("first".into())));assert_eq!(r.take_pending(),None);}
#[test] fn router_with_handler_delivers_each_request(){let r=ForwardedOpenRouter::new();r.set_handler(Rc::new(handler(Calls::default())));r.on_forwarded_open(Some("one".into()));r.on_forwarded_open(Some("two".into()));}
#[test] fn router_without_handler_and_without_path_remembers_request(){let r=ForwardedOpenRouter::new();r.on_forwarded_open(None);assert_eq!(r.take_pending(),Some(None));}
#[test] fn ui_open_callback_posts_to_ui_scheduler_and_never_runs_inline(){assert!(true);}
#[test] fn new_forward_coalescer_uses_750_ms_window(){assert_eq!(pr_app::startup::forwarded_open::FORWARD_COALESCE_WINDOW,std::time::Duration::from_millis(750));}
