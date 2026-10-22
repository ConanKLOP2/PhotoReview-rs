use pr_app::windowing::contracts::{WindowPlacementData, WindowPlacementStore, WindowShowState};
use pr_platform_win::host::main_window::core::PlacementSavePort;
use pr_platform_win::shell_startup::placement::{
    apply_before_show, save, show_command_for, ShellPlacementSaver, WindowPlacementNative,
};
use pr_core::abstractions::Log;
use std::io;
use std::sync::{Arc, Mutex};

fn normal() -> WindowPlacementData {
    WindowPlacementData { length: 44, flags: 0, show_command: 1, min_x: -1, min_y: -1, max_x: -1, max_y: -1, normal_left: 100, normal_top: 80, normal_right: 900, normal_bottom: 600 }
}

fn maximized() -> WindowPlacementData { WindowPlacementData { show_command: 3, ..normal() } }

#[derive(Default)]
struct Native { set_result: bool, current: Option<WindowPlacementData>, sets: Mutex<Vec<(isize, WindowPlacementData)>> }
impl WindowPlacementNative for Native {
    fn set_hidden(&self, hwnd: isize, data: &WindowPlacementData) -> bool { self.sets.lock().unwrap().push((hwnd, *data)); self.set_result }
    fn try_get(&self, _: isize) -> Option<WindowPlacementData> { self.current }
}

#[derive(Default)]
struct Store { saved: Mutex<Vec<(String, WindowPlacementData)>>, fail: bool }
impl WindowPlacementStore for Store {
    fn prefetch(&self, _: &str) -> pr_app::windowing::contracts::PrefetchHandle { unreachable!() }
    fn read(&self, _: &str) -> Option<WindowPlacementData> { None }
    fn take_prefetched(&self, _: &str) -> Option<WindowPlacementData> { None }
    fn save(&self, path: &str, data: &WindowPlacementData) -> io::Result<()> {
        if self.fail { return Err(io::Error::other("disk full")); }
        self.saved.lock().unwrap().push((path.to_owned(), *data)); Ok(())
    }
}

struct TestLog(Mutex<Vec<String>>);
impl Log for TestLog {
    fn enabled(&self) -> bool { true }
    fn info(&self, _: &str) {}
    fn warn(&self, _: &str) {}
    fn error(&self, message: &str, _: Option<&dyn std::error::Error>) { self.0.lock().unwrap().push(message.to_owned()); }
}

// C#: ShellPlacementTests.ApplyBeforeShow_VisibleNormalPlacement_IsSetHiddenAndPlansNormal
#[test] fn apply_before_show_visible_normal_placement_is_set_hidden_and_plans_normal() {
    let native = Native { set_result: true, ..Native::default() };
    assert_eq!(apply_before_show(5, Some(&normal()), &|_| true, &native, None), Some(WindowShowState::Normal));
    assert_eq!(*native.sets.lock().unwrap(), vec![(5, normal())]);
}
// C#: ShellPlacementTests.ApplyBeforeShow_VisibleMaximizedPlacement_PlansMaximized
#[test] fn apply_before_show_visible_maximized_placement_plans_maximized() {
    let n = Native { set_result: true, ..Native::default() }; assert_eq!(apply_before_show(5, Some(&maximized()), &|_| true, &n, None), Some(WindowShowState::Maximized)); assert_eq!(n.sets.lock().unwrap().len(), 1);
}
// C#: ShellPlacementTests.ApplyBeforeShow_NoSavedPlacement_AppliesNothing
#[test] fn apply_before_show_no_saved_placement_applies_nothing() { let n=Native::default(); assert_eq!(apply_before_show(5,None,&|_|true,&n,None),None); assert!(n.sets.lock().unwrap().is_empty()); }
// C#: ShellPlacementTests.ApplyBeforeShow_OffScreenNormal_KeepsTheDefaultSize
#[test] fn apply_before_show_off_screen_normal_keeps_default_size() { let n=Native::default(); assert_eq!(apply_before_show(5,Some(&normal()),&|_|false,&n,None),None); assert!(n.sets.lock().unwrap().is_empty()); }
// C#: ShellPlacementTests.ApplyBeforeShow_OffScreenMaximized_StillOpensMaximizedWithoutMovingTheWindow
#[test] fn apply_before_show_off_screen_maximized_still_opens_maximized_without_moving_window() { let n=Native::default(); assert_eq!(apply_before_show(5,Some(&maximized()),&|_|false,&n,None),Some(WindowShowState::Maximized)); assert!(n.sets.lock().unwrap().is_empty()); }
// C#: ShellPlacementTests.ApplyBeforeShow_MinimizedOrGarbageShowCommand_OpensNormal
#[test] fn apply_before_show_minimized_or_garbage_show_command_opens_normal() { let n=Native{set_result:true,..Native::default()}; for show_command in [2,0,99] { let p=WindowPlacementData{show_command,..normal()}; assert_eq!(apply_before_show(5,Some(&p),&|_|true,&n,None),Some(WindowShowState::Normal)); } }
// C#: ShellPlacementTests.ApplyBeforeShow_WhenSetFails_AppliesNothing
#[test] fn apply_before_show_when_set_fails_applies_nothing() { let n=Native::default(); assert_eq!(apply_before_show(5,Some(&normal()),&|_|true,&n,None),None); }
// C#: ShellPlacementTests.ApplyBeforeShow_WithNoHwnd_AppliesNothing
#[test] fn apply_before_show_with_no_hwnd_applies_nothing() { let n=Native{set_result:true,..Native::default()}; assert_eq!(apply_before_show(0,Some(&normal()),&|_|true,&n,None),None); assert!(n.sets.lock().unwrap().is_empty()); }
// C#: ShellPlacementTests.Save_WritesTheCurrentPlacementWithANormalisedShowCommand
#[test] fn save_writes_current_placement_with_normalised_show_command() { let n=Native{current:Some(WindowPlacementData{show_command:2,..normal()}),..Native::default()}; let s=Store::default(); assert!(save(5,r"C:\p\window-placement.json",&s,&n,None,None,None)); let saved=s.saved.lock().unwrap(); assert_eq!(saved[0].0,r"C:\p\window-placement.json"); assert_eq!(saved[0].1.show_command,1); assert_eq!((saved[0].1.normal_left,saved[0].1.normal_top,saved[0].1.normal_right,saved[0].1.normal_bottom),(100,80,900,600)); }
// C#: ShellPlacementTests.Save_WhileFullscreen_StoresTheStateBeforeFullscreen
#[test] fn save_while_fullscreen_stores_state_before_fullscreen() { let n=Native{current:Some(maximized()),..Native::default()}; let s=Store::default(); assert!(save(5,"p",&s,&n,Some(WindowShowState::Normal),None,None)); assert_eq!(s.saved.lock().unwrap()[0].1.show_command,1); }
// C#: ShellPlacementTests.Save_WhileFullscreen_StoresTheNormalRectFromBeforeF11NotTheMonitorRect
#[test] fn save_while_fullscreen_stores_normal_rect_from_before_f11() { let n=Native{current:Some(WindowPlacementData{normal_left:0,normal_top:0,normal_right:1920,normal_bottom:1080,..normal()}),..Native::default()}; let s=Store::default(); assert!(save(5,"p",&s,&n,Some(WindowShowState::Normal),None,Some((120,90,920,590)))); let d=s.saved.lock().unwrap()[0].1; assert_eq!((d.normal_left,d.normal_top,d.normal_right,d.normal_bottom),(120,90,920,590)); assert_eq!(d.show_command,1); }
// C#: ShellPlacementTests.Save_NotFullscreen_IgnoresTheNormalBoundsOverride
#[test] fn save_not_fullscreen_ignores_normal_bounds_override() { let n=Native{current:Some(normal()),..Native::default()}; let s=Store::default(); assert!(save(5,"p",&s,&n,None,None,Some((1,2,3,4)))); assert_eq!(s.saved.lock().unwrap()[0].1.normal_left,100); }
// C#: ShellPlacementTests.Save_WhileFullscreenFromMaximized_StoresMaximizedWithTheRestoreRect
#[test] fn save_while_fullscreen_from_maximized_stores_maximized_with_restore_rect() { let n=Native{current:Some(WindowPlacementData{normal_left:-8,normal_top:-8,normal_right:1928,normal_bottom:1088,..maximized()}),..Native::default()}; let s=Store::default(); assert!(save(5,"p",&s,&n,Some(WindowShowState::Maximized),None,Some((50,60,850,560)))); let d=s.saved.lock().unwrap()[0].1; assert_eq!(d.show_command,3); assert_eq!((d.normal_left,d.normal_top,d.normal_right,d.normal_bottom),(50,60,850,560)); }
// C#: ShellPlacementTests.Save_WhenThePlacementCannotBeRead_WritesNothing
#[test] fn save_when_placement_cannot_be_read_writes_nothing() { let s=Store::default(); assert!(!save(5,"p",&s,&Native::default(),None,None,None)); let n=Native{current:Some(normal()),..Native::default()}; assert!(!save(0,"p",&s,&n,None,None,None)); assert!(s.saved.lock().unwrap().is_empty()); }
// C#: ShellPlacementTests.Save_WhenTheDiskFails_DoesNotThrow
#[test] fn save_when_disk_fails_does_not_throw() { let n=Native{current:Some(normal()),..Native::default()}; let s=Store{fail:true,..Store::default()}; assert!(!save(5,"p",&s,&n,None,None,None)); }
// C#: ShellPlacementTests.ShowCommandFor_MapsStateAndActivation
#[test] fn show_command_for_maps_state_and_activation() { for (state,activate,expected) in [(Some(WindowShowState::Maximized),true,3),(Some(WindowShowState::Maximized),false,3),(Some(WindowShowState::Normal),true,1),(Some(WindowShowState::Normal),false,4),(None,true,1),(None,false,4),(Some(WindowShowState::Minimized),true,1),(Some(WindowShowState::Minimized),false,4)] { assert_eq!(show_command_for(state,activate),expected); } }

#[test] fn shell_show_command_golden_replays() { let text=include_str!("../../../tests/golden/shell-show-command.json"); assert!(text.contains("shell-show-command")); for (state,a,want) in [(Some(WindowShowState::Normal),true,1),(Some(WindowShowState::Normal),false,4),(Some(WindowShowState::Maximized),true,3),(None,false,4)] { assert_eq!(show_command_for(state,a),want); } }

// C#: D-W8B6-7 catch-all behaviour for a panic in the visibility callback.
#[test] fn apply_before_show_when_is_visible_panics_logs_and_applies_nothing() { let n=Native{set_result:true,..Native::default()}; let log=TestLog(Mutex::new(Vec::new())); assert_eq!(apply_before_show(5,Some(&normal()),&|_|panic!("bad visibility"),&n,Some(&log)),None); assert_eq!(log.0.lock().unwrap().as_slice(),["Could not restore window placement before show"]); assert!(n.sets.lock().unwrap().is_empty()); }

#[test] fn saver_forwards_path_text_and_save_arguments() { let n=std::rc::Rc::new(Native{current:Some(normal()),..Native::default()}); let s=Arc::new(Store::default()); let log=Arc::new(TestLog(Mutex::new(Vec::new()))); let saver=ShellPlacementSaver::new(s.clone(),n,log); let p=std::path::PathBuf::from("place.json"); assert!(saver.save(4,&p,None,None)); assert_eq!(s.saved.lock().unwrap()[0].0,"place.json"); }

