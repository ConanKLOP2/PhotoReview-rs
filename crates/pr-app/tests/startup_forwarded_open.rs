#[rustfmt::skip]
use pr_app::startup::forwarded_open::{ForwardedOpenHandler, ForwardedOpenRouter, ForwardedOpenTarget, WindowGone};
#[rustfmt::skip]
use pr_core::abstractions::Log;
#[rustfmt::skip]
use std::{error::Error, rc::Rc, sync::{Arc, Mutex}};

#[rustfmt::skip]
#[derive(Default)] struct Calls { calls: Mutex<Vec<String>>, closing: bool, minimized: bool, fail_activate: bool, fail_open: bool }
#[rustfmt::skip]
impl ForwardedOpenTarget for Calls {
 fn is_closing_or_closed(&self)->bool{self.closing}
 fn is_minimized(&self)->Result<bool,WindowGone>{Ok(self.minimized)}
 fn restore(&self)->Result<(),WindowGone>{self.calls.lock().unwrap().push("restore".into());Ok(())}
 fn activate(&self)->Result<(),WindowGone>{self.calls.lock().unwrap().push("activate".into());if self.fail_activate{Err(WindowGone{message:"gone".into()})}else{Ok(())}}
 fn open_path(&self,p:&str)->Result<(),Box<dyn Error>>{self.calls.lock().unwrap().push(format!("open:{p}"));if self.fail_open{Err(Box::new(std::io::Error::other("open failed")))}else{Ok(())}}
}
#[rustfmt::skip]
#[derive(Default)] struct TestLog(Mutex<Vec<String>>);
#[rustfmt::skip]
impl Log for TestLog {
 fn enabled(&self)->bool{true} fn info(&self,_:&str){}
 fn warn(&self,m:&str){self.0.lock().unwrap().push(format!("warn:{m}"))}
 fn error(&self,m:&str,e:Option<&(dyn Error+'static)>){self.0.lock().unwrap().push(format!("error:{m}:{}",e.map_or_else(|| "<none>".to_owned(), ToString::to_string)))}
}
#[test] // C#: ForwardedOpenHandlerTests.Path_ActivatesThenOpens
#[rustfmt::skip]
fn path_activates_then_opens(){let t=Rc::new(Calls::default());let h=ForwardedOpenHandler::new(t.clone(),Arc::new(TestLog::default()));h.handle(Some("C:\\p\\a.jpg"));assert_eq!(&*t.calls.lock().unwrap(), &["activate","open:C:\\p\\a.jpg"]);}
#[test] // C#: ForwardedOpenHandlerTests.NoPath_OnlyBringsTheWindowToTheFront
#[rustfmt::skip]
fn no_path_only_brings_window_to_front(){let t=Rc::new(Calls::default());let h=ForwardedOpenHandler::new(t.clone(),Arc::new(TestLog::default()));h.handle(None);assert_eq!(&*t.calls.lock().unwrap(), &["activate"]);assert_eq!(h.ignored_count(),0);}
#[test] // C#: ForwardedOpenHandlerTests.MinimizedWindow_IsRestoredBeforeActivation
#[rustfmt::skip]
fn minimized_window_is_restored_before_activation(){let t=Rc::new(Calls{minimized:true,..Default::default()});let h=ForwardedOpenHandler::new(t.clone(),Arc::new(TestLog::default()));h.handle(Some("x"));assert_eq!(&*t.calls.lock().unwrap(), &["restore","activate","open:x"]);}
#[test] // C#: ForwardedOpenHandlerTests.RequestRacingTheShutdown_IsIgnoredAndNeverTouchesTheWindow
#[rustfmt::skip]
fn request_racing_shutdown_is_ignored(){let t=Rc::new(Calls{closing:true,..Default::default()});let h=ForwardedOpenHandler::new(t.clone(),Arc::new(TestLog::default()));h.handle(Some("x"));assert!(t.calls.lock().unwrap().is_empty());assert_eq!(h.ignored_count(),1);}
#[test] // C#: ForwardedOpenHandlerTests.WindowGoneBetweenCheckAndActivate_IsLoggedNotThrown_AndNothingIsOpened
#[rustfmt::skip]
fn window_gone_between_check_and_activate_is_logged(){let t=Rc::new(Calls{fail_activate:true,..Default::default()});let log=Arc::new(TestLog::default());let h=ForwardedOpenHandler::new(t.clone(),log.clone());h.handle(Some("x"));assert_eq!(&*t.calls.lock().unwrap(), &["activate"]);assert_eq!(h.ignored_count(),1);assert_eq!(&*log.0.lock().unwrap(), &["warn:Forwarded launch: could not activate the main window: gone"]);}
#[test] // C#: ForwardedOpenHandlerTests.FailingOpen_IsLoggedAsForwardedOpenFailed
#[rustfmt::skip]
fn failing_open_is_logged_as_forwarded_open_failed(){let t=Rc::new(Calls{fail_open:true,..Default::default()});let log=Arc::new(TestLog::default());let h=ForwardedOpenHandler::new(t.clone(),log.clone());h.handle(Some("x"));assert_eq!(&*t.calls.lock().unwrap(), &["activate","open:x"]);assert_eq!(&*log.0.lock().unwrap(), &["error:Forwarded open failed:open failed"]);}
#[rustfmt::skip]
#[test] fn router_keeps_first_pending_path_and_replays_once(){let r=ForwardedOpenRouter::new();r.on_forwarded_open(Some("first".into()));r.on_forwarded_open(Some("second".into()));assert_eq!(r.take_pending(),Some(Some("first".into())));assert_eq!(r.take_pending(),None);}
#[rustfmt::skip]
#[test] fn router_with_handler_delivers_each_request(){let r=ForwardedOpenRouter::new();let t=Rc::new(Calls::default());r.set_handler(Rc::new(ForwardedOpenHandler::new(t.clone(),Arc::new(TestLog::default()))));r.on_forwarded_open(Some("one".into()));r.on_forwarded_open(Some("two".into()));assert_eq!(&*t.calls.lock().unwrap(), &["activate","open:one","activate","open:two"]);assert_eq!(r.take_pending(),None);}
#[rustfmt::skip]
#[test] fn router_without_handler_and_without_path_remembers_request(){let r=ForwardedOpenRouter::new();r.on_forwarded_open(None);assert_eq!(r.take_pending(),Some(None));}
#[rustfmt::skip]
#[test] fn ui_open_callback_posts_to_ui_scheduler_and_never_runs_inline(){
 struct QueuedUi { jobs: Mutex<Vec<Box<dyn FnOnce()+Send>>> }
 impl pr_core::abstractions::UiScheduler for QueuedUi {
  fn post(&self, job: Box<dyn FnOnce()+Send>) { self.jobs.lock().unwrap().push(job); }
  fn invoke(&self, job: Box<dyn FnOnce()+Send>) -> pr_core::abstractions::UiCompletion { let (tx, done)=pr_core::abstractions::UiCompletion::channel(); job(); let _=tx.send(Ok(())); done }
 }
 let ui=Arc::new(QueuedUi{jobs:Mutex::new(Vec::new())});
 let router=ForwardedOpenRouter::new();
 let target=Rc::new(Calls::default());
 router.set_handler(Rc::new(ForwardedOpenHandler::new(target.clone(),Arc::new(TestLog::default()))));
 router.attach_to_current_thread();
 let callback=pr_app::startup::forwarded_open::ui_open_callback(ui.clone());
 callback(Some("queued.jpg".to_owned()));
 assert!(target.calls.lock().unwrap().is_empty());
 assert_eq!(ui.jobs.lock().unwrap().len(),1);
 let job=ui.jobs.lock().unwrap().pop().unwrap();
 job();
 assert_eq!(&*target.calls.lock().unwrap(), &["activate","open:queued.jpg"]);
}
#[rustfmt::skip]
#[test] fn new_forward_coalescer_uses_750_ms_window(){assert_eq!(pr_app::startup::forwarded_open::FORWARD_COALESCE_WINDOW,std::time::Duration::from_millis(750));}
