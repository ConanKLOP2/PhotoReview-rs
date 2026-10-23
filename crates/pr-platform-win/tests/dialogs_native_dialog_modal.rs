//! Desktop contract tests for W8-18h. Run explicitly with --ignored on a desktop.
#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use fixture::*;
use pr_platform_win::dialogs::native::dialog::core::{DialogError, ID_CANCEL, ID_OK, NativeDialog};
use pr_platform_win::dialogs::native::template::{DialogControlClass, DialogItem};
use pr_platform_win::win32::{api, dialog_native, messages};

thread_local! { static QUEUED_DIALOG: RefCell<Option<NativeDialog>> = const { RefCell::new(None) }; }
thread_local! { static QUEUED_WORK: Cell<u32> = const { Cell::new(0) }; }
thread_local! { static NESTED_DIALOG_LOOP: RefCell<Option<Rc<pr_platform_win::host::message_loop::MessageLoop>>> = const { RefCell::new(None) }; }

struct CountingFilter(Cell<u32>);
impl pr_platform_win::host::message_loop::MessageFilter for CountingFilter {
    fn pre_filter_message(&self, _: &pr_platform_win::host::contracts::WindowMessage) -> bool {
        self.0.set(self.0.get() + 1);
        false
    }
}

fn content() -> Rc<RecordingContent> {
    let c = Rc::new(RecordingContent::default());
    c.items.borrow_mut().extend([
        DialogItem::new(
            DialogControlClass::Button,
            ID_OK,
            "OK",
            dialog_native::WS_TABSTOP | dialog_native::BS_DEFPUSHBUTTON,
        ),
        DialogItem::new(
            DialogControlClass::Button,
            ID_CANCEL,
            "Cancel",
            dialog_native::WS_TABSTOP,
        ),
    ]);
    c
}
fn dialog(c: &Rc<RecordingContent>) -> NativeDialog {
    NativeDialog::new(c.clone())
}

#[test]
#[ignore = "native: needs desktop"]
fn show_modal_returns_ok_when_ok_is_pressed() {
    // C#: NativeDialog.HandleCommand IDOK.
    on_sta(|| {
        let c = content();
        let d = dialog(&c);
        assert_eq!(
            show_on_owner(&d, |d, _| {
                command(d, ID_OK, 0);
            })
            .unwrap(),
            ID_OK
        );
        assert_eq!(c.events.borrow().iter().filter(|s| *s == "ok").count(), 1);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn show_modal_returns_cancel_for_cancel_button_x_and_close() {
    // C#: NativeDialog.HandleCommand IDCANCEL, HandleMessage WM_CLOSE, Close.
    on_sta(|| {
        for mode in 0..3 {
            let c = content();
            let d = dialog(&c);
            let result = show_on_owner(&d, |d, _| match mode {
                0 => {
                    command(d, ID_CANCEL, 0);
                }
                1 => {
                    // SAFETY: WM_CLOSE carries no pointer.
                    unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_CLOSE, 0, 0) };
                }
                _ => d.close(),
            })
            .unwrap();
            assert_eq!(result, ID_CANCEL);
        }
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn escape_and_enter_go_through_the_dialog_manager() {
    // C#: NativeDialog.OnCancel / OnOk through IsDialogMessage.
    on_sta(|| {
        let c = content();
        c.cancel_handled.set(true);
        let d = dialog(&c);
        assert_eq!(
            show_on_owner(&d, |d, _| {
                assert!(key(d, item(d, ID_CANCEL), VK_ESCAPE));
                drain(d);
                assert!(!d.is_closed());
                assert!(key(d, item(d, ID_OK), VK_RETURN));
            })
            .unwrap(),
            ID_OK
        );
        assert!(c.events.borrow().contains(&"cancel".into()));
        assert!(c.events.borrow().contains(&"ok".into()));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn on_ok_false_keeps_the_dialog_open() {
    // C#: NativeDialog.HandleCommand IDOK with OnOk=false.
    on_sta(|| {
        let c = content();
        c.ok_result.set(false);
        let d = dialog(&c);
        assert_eq!(
            show_on_owner(&d, |d, _| {
                command(d, ID_OK, 0);
                assert!(!d.is_closed());
                c.ok_result.set(true);
                command(d, ID_OK, 0);
            })
            .unwrap(),
            ID_OK
        );
        assert_eq!(c.events.borrow().iter().filter(|e| *e == "ok").count(), 2);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn the_owner_is_disabled_while_open_and_enabled_after() {
    // C#: NativeDialog.ShowModal disables and Cleanup re-enables owner.
    on_sta(|| {
        let owner = create_owner();
        let c = content();
        let d = dialog(&c);
        assert!(dialog_native::user32::is_window_enabled(owner));
        d.show_modal(
            owner,
            Some(&|d| {
                assert!(!dialog_native::user32::is_window_enabled(owner));
                d.close();
            }),
            None,
        )
        .unwrap();
        assert!(dialog_native::user32::is_window_enabled(owner));
        destroy_owner(owner);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn ready_runs_after_the_window_is_visible_and_before_the_loop() {
    // C#: NativeDialog.ShowModal ready ordering.
    on_sta(|| {
        let owner = create_owner();
        let c = content();
        let d = dialog(&c);
        d.show_modal(
            owner,
            Some(&|d| {
                assert!(api::user32::is_window_visible(d.hwnd()));
                assert!(!dialog_native::user32::is_window_enabled(owner));
                assert!(c.events.borrow().contains(&"init".into()));
                d.close();
            }),
            None,
        )
        .unwrap();
        destroy_owner(owner);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn a_dialog_closed_in_ready_skips_the_loop() {
    // C#: NativeDialog.ShowModal ready closes before RunLoop.
    on_sta(|| {
        let c = content();
        let d = dialog(&c);
        assert_eq!(show_on_owner(&d, |d, _| d.end_modal(ID_OK)).unwrap(), ID_OK);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn wm_quit_is_posted_again_and_the_dialog_closes_as_cancel() {
    // C#: NativeDialog.RunLoop WM_QUIT lesson #486.
    on_sta(|| {
        let c = content();
        let d = dialog(&c);
        assert_eq!(
            show_on_owner(&d, |_, _| api::user32::post_quit_message(7)).unwrap(),
            ID_CANCEL
        );
        assert_eq!(has_quit_pending(), Some(7));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn close_from_outside_ends_the_modal_loop() {
    // C#: NativeDialog.EndModal posts WM_NULL to wake GetMessage.
    on_sta(|| {
        let c = content();
        c.window_message_hook.replace(Some(Box::new(|d, message| {
            if message == messages::WM_APP + 3 {
                d.close();
            }
        })));
        let d = dialog(&c);
        assert_eq!(
            show_on_owner(&d, |d, _| {
                // SAFETY: the application message carries no pointer.
                unsafe {
                    api::user32::post_message(d.hwnd(), messages::WM_APP + 3, 0, 0).unwrap();
                }
            })
            .unwrap(),
            ID_CANCEL
        );
        assert!(c.events.borrow().contains(&"message:8003".into()));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn end_modal_before_show_creates_and_destroys_the_window_with_the_earlier_result() {
    // C#: NativeDialog.EndModal before ShowModal.
    on_sta(|| {
        let c = content();
        let d = dialog(&c);
        d.end_modal(ID_OK);
        assert_eq!(
            show_on_owner(&d, |_, _| panic!("ready must be skipped")).unwrap(),
            ID_OK
        );
        assert_eq!(d.hwnd(), 0);
        assert!(c.events.borrow().contains(&"closed".into()));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn a_second_show_is_rejected_after_a_real_show() {
    // C#: NativeDialog.ShowModal once-only guard.
    on_sta(|| {
        let c = content();
        let d = dialog(&c);
        show_on_owner(&d, |d, _| d.close()).unwrap();
        assert!(matches!(d.show_modal(0, None, None), Err(DialogError::AlreadyShown)));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn a_panic_in_a_hook_closes_as_cancel_and_resumes_after_show_returns() {
    // C#: NativeDialog.DialogProc captures OnInitialized exception, Cleanup restores owner.
    on_sta(|| {
        let owner = create_owner();
        let gdi = gdi_objects();
        let user = user_objects();
        let c = content();
        c.panic_in.set(Some("initialized"));
        let d = dialog(&c);
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.show_modal(owner, None, None)));
        assert!(error.is_err());
        assert!(dialog_native::user32::is_window_enabled(owner));
        assert!(settles_at_or_below(gdi_objects, gdi));
        assert!(settles_at_or_below(user_objects, user));
        destroy_owner(owner);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn a_panic_in_ready_unwinds_through_the_cleanup_guard() {
    // C#: NativeDialog.ShowModal finally Cleanup.
    on_sta(|| {
        let owner = create_owner();
        let c = content();
        let d = dialog(&c);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            d.show_modal(owner, Some(&|_| panic!("ready panic")), None)
        }));
        assert!(result.is_err());
        assert_eq!(d.hwnd(), 0);
        assert!(dialog_native::user32::is_window_enabled(owner));
        destroy_owner(owner);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn activate_restores_a_minimized_dialog() {
    // C#: NativeDialog.Activate.
    on_sta(|| {
        let c = content();
        let d = dialog(&c);
        show_on_owner(&d, |d, _| {
            api::user32::show_window(d.hwnd(), messages::SW_MINIMIZE);
            d.activate();
            assert!(api::user32::is_window_visible(d.hwnd()));
            d.close();
        })
        .unwrap();
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn modeless_returns_at_once_without_disabling_the_owner_and_calls_closed_once() {
    // C#: NativeDialog.ShowModeless / Cleanup.
    on_sta(|| {
        let owner = create_owner();
        let c = content();
        let d = dialog(&c);
        let closed = Rc::new(Cell::new(0));
        let count = closed.clone();
        d.show_modeless(
            owner,
            None,
            Some(&|d| assert!(api::user32::is_window_visible(d.hwnd()))),
            Some(Box::new(move || count.set(count.get() + 1))),
        )
        .unwrap();
        assert!(dialog_native::user32::is_window_enabled(owner));
        d.close();
        assert_eq!(closed.get(), 1);
        destroy_owner(owner);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn modeless_filter_is_registered_while_open_and_removed_after() {
    // C#: NativeDialog.ModelessFilter registration and Cleanup.
    let ui = ui_thread::UiThread::start();
    ui.invoke(|| {
        let loop_ = ui_thread::ui_loop();
        let before = loop_.message_filter_count();
        let c = content();
        let d = dialog(&c);
        d.show_modeless(0, Some(loop_.clone()), None, None).unwrap();
        assert_eq!(loop_.message_filter_count(), before + 1);
        d.close();
        assert_eq!(loop_.message_filter_count(), before);
    });
    ui.dispose().unwrap();
}

#[test]
#[ignore = "native: needs desktop"]
fn modeless_closed_inside_init_cleans_up_at_once() {
    // C#: NativeDialog.ShowModeless sees _done after CreateDialogIndirectParam.
    on_sta(|| {
        let c = content();
        let _old = c.layout.replace(Box::new(|d, _, _| d.close()));
        let d = dialog(&c);
        let ready = Cell::new(false);
        d.show_modeless(0, None, Some(&|_| ready.set(true)), None).unwrap();
        assert!(!ready.get());
        assert_eq!(d.hwnd(), 0);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn nested_loop_keeps_dispatcher_work_and_filters_running() {
    // C#: NativeDialogLoopTests nested dispatcher test.
    on_sta(|| {
        let dispatcher = Rc::new(ui_thread::TestDispatcher::new());
        let poster = dispatcher.poster();
        let loop_ = Rc::new(pr_platform_win::host::message_loop::MessageLoop::new(dispatcher).unwrap());
        let filter = Rc::new(CountingFilter(Cell::new(0)));
        loop_.add_message_filter(filter.clone());
        let c = content();
        let d = dialog(&c);
        d.show_modal(
            0,
            Some(&|d| {
                // SAFETY: these application and close messages carry only integer values.
                unsafe {
                    api::user32::post_message(d.hwnd(), messages::WM_APP + 2, 0, 0).unwrap();
                    api::user32::post_message(d.hwnd(), messages::WM_CLOSE, 0, 0).unwrap();
                }
                poster(Box::new(|| QUEUED_WORK.with(|n| n.set(n.get() + 1))));
            }),
            Some(&loop_),
        )
        .unwrap();
        assert!(filter.0.get() > 0, "nested loop must run message filters");
        QUEUED_WORK.with(|n| assert!(n.get() > 0, "nested loop must run dispatcher work"));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn wm_quit_in_the_nested_loop_closes_the_dialog_and_is_reposted_for_the_outer_loop() {
    // C#: NativeDialogLoopTests WM_QUIT in nested loop.
    on_sta(|| {
        let dispatcher = Rc::new(ui_thread::TestDispatcher::new());
        let loop_ = Rc::new(pr_platform_win::host::message_loop::MessageLoop::new(dispatcher.clone()).unwrap());
        NESTED_DIALOG_LOOP.with(|slot| *slot.borrow_mut() = Some(loop_.clone()));

        // Ensure PostThreadMessage in TestDispatcher's poster succeeds before Run starts.
        let mut message = pr_platform_win::win32::structs::Msg::default();
        let _ = api::user32::peek_message(&mut message, 0, 0, 0, messages::PM_NOREMOVE);
        let poster = dispatcher.poster();
        poster(Box::new(|| {
            let loop_ = NESTED_DIALOG_LOOP.with(|slot| slot.borrow().as_ref().cloned().expect("nested loop"));
            let c = content();
            let d = dialog(&c);
            assert_eq!(
                d.show_modal(0, Some(&|_| api::user32::post_quit_message(5)), Some(&loop_))
                    .unwrap(),
                ID_CANCEL
            );
        }));

        // The outer Run consumes the WM_QUIT reposted by RunNested and returns its original exit code.
        assert_eq!(loop_.run().unwrap(), 5);
        NESTED_DIALOG_LOOP.with(|slot| *slot.borrow_mut() = None);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn closing_from_a_queued_callback_ends_the_dialog() {
    // C#: NativeDialogLoopTests queued close callback.
    on_sta(|| {
        let dispatcher = Rc::new(ui_thread::TestDispatcher::new());
        let poster = dispatcher.poster();
        let loop_ = Rc::new(pr_platform_win::host::message_loop::MessageLoop::new(dispatcher).unwrap());
        let c = content();
        let d = dialog(&c);
        QUEUED_DIALOG.with(|slot| *slot.borrow_mut() = Some(d.clone()));
        let result = d
            .show_modal(
                0,
                Some(&|_| {
                    poster(Box::new(|| {
                        QUEUED_DIALOG.with(|slot| slot.borrow().as_ref().unwrap().close())
                    }))
                }),
                Some(&loop_),
            )
            .unwrap();
        QUEUED_DIALOG.with(|slot| *slot.borrow_mut() = None);
        assert_eq!(result, ID_CANCEL);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn a_dialog_content_is_released_after_the_dialog_closes() {
    // C#: NativeDialogTestKit lifetime expectations.
    on_sta(|| {
        let c = content();
        let token = c.token.clone();
        {
            let d = dialog(&c);
            show_on_owner(&d, |d, _| d.close()).unwrap();
        }
        drop(c);
        assert_eq!(Rc::strong_count(&token), 1);
        let c = content();
        let token = c.token.clone();
        {
            let d = dialog(&c);
            d.show_modeless(0, None, None, None).unwrap();
            d.close();
        }
        drop(c);
        assert_eq!(Rc::strong_count(&token), 1);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn on_dialog_key_claims_the_key_before_the_dialog_manager() {
    // C#: NativeDialog.OnDialogKey before IsDialogMessage.
    on_sta(|| {
        for claim in [true, false] {
            let c = content();
            c.dialog_key_handled.set(claim);
            let d = dialog(&c);
            show_on_owner(&d, |d, _| {
                // SAFETY: WM_KEYDOWN and WM_CLOSE carry integer values only.
                unsafe {
                    api::user32::post_message(item(d, ID_OK), messages::WM_KEYDOWN, VK_RETURN as usize, 1).unwrap();
                    if claim {
                        api::user32::post_message(d.hwnd(), messages::WM_CLOSE, 0, 0).unwrap();
                    }
                }
            })
            .unwrap();
            assert_eq!(c.events.borrow().iter().any(|s| s == "ok"), !claim);
            assert!(c.events.borrow().iter().any(|s| s == "key"));
        }
    });
    let ui = ui_thread::UiThread::start();
    ui.invoke(|| {
        let loop_ = ui_thread::ui_loop();
        for claim in [true, false] {
            let c = content();
            c.dialog_key_handled.set(claim);
            let d = dialog(&c);
            d.show_modal(
                0,
                Some(&|d| {
                    // SAFETY: WM_KEYDOWN and WM_CLOSE contain integer values only.
                    unsafe {
                        api::user32::post_message(item(d, ID_OK), messages::WM_KEYDOWN, VK_RETURN as usize, 1).unwrap();
                        if claim {
                            api::user32::post_message(d.hwnd(), messages::WM_CLOSE, 0, 0).unwrap();
                        }
                    }
                }),
                Some(&loop_),
            )
            .unwrap();
            assert_eq!(c.events.borrow().iter().any(|s| s == "ok"), !claim);
            assert!(c.events.borrow().iter().any(|s| s == "key"));
        }
    });
    ui.dispose().unwrap();
}

#[test]
#[ignore = "native: needs desktop"]
fn unhandled_messages_reach_on_window_message_and_its_answer_is_the_message_result() {
    // C#: NativeDialog.HandleMessage default arm, DWLP_MSGRESULT.
    on_sta(|| {
        let c = content();
        c.window_message_result.set(Some(42));
        let d = dialog(&c);
        show_on_owner(&d, |d, _| {
            // SAFETY: WM_APP+1 carries no pointer.
            let handled = unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_APP + 1, 0, 0) };
            assert_eq!(handled, 42);
            c.window_message_result.set(None);
            // SAFETY: WM_APP+1 carries no pointer.
            let unhandled = unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_APP + 1, 0, 0) };
            assert_eq!(unhandled, 0);
            d.close();
        })
        .unwrap();
        assert!(c.events.borrow().contains(&"message:8001".into()));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn a_panic_in_ready_of_a_modeless_dialog_closes_it_and_resumes() {
    // C#: NativeDialog.ShowModeless ready catch.
    on_sta(|| {
        let owner = create_owner();
        let c = content();
        let d = dialog(&c);
        let closed = Rc::new(Cell::new(0));
        let count = closed.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            d.show_modeless(
                owner,
                None,
                Some(&|_| panic!("ready")),
                Some(Box::new(move || count.set(count.get() + 1))),
            )
        }));
        assert!(result.is_err());
        assert!(d.is_closed());
        assert_eq!(closed.get(), 1);
        assert!(dialog_native::user32::is_window_enabled(owner));
        destroy_owner(owner);
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn a_panic_in_a_hook_and_in_the_closed_callback_do_not_abort() {
    // C#: NativeDialog.DialogProc and Cleanup catch first exception.
    on_sta(|| {
        let c = content();
        c.panic_in.set(Some("command"));
        let d = dialog(&c);
        d.show_modeless(0, None, None, Some(Box::new(|| panic!("closed callback"))))
            .unwrap();
        command(&d, 99, 0);
        assert!(d.is_closed());
        assert!(pr_platform_win::host::pending_panic::has_pending());
        let payload = std::panic::catch_unwind(pr_platform_win::host::pending_panic::rethrow_pending)
            .expect_err("the command hook panic must be resumed");
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied());
        assert_eq!(message, Some("hook panic: command"));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn activate_is_a_no_op_before_show_and_after_close() {
    // C#: NativeDialog.Activate checks HWND.
    on_sta(|| {
        let c = content();
        let d = dialog(&c);
        d.activate();
        show_on_owner(&d, |d, _| d.close()).unwrap();
        d.activate();
        assert_eq!(d.hwnd(), 0);
    });
}
