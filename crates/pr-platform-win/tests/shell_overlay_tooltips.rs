//! Unit W8-15g tests: `pr_platform_win::shell_overlay::tooltips` (`NativeTooltipHost`, `DeferredTooltips`, the pure rectangle and "unchanged" rules).
//!
//! Twins of `NativeTooltipHostTests.cs` (6 tests, trait Native) and of `DeferredTooltipsTests` in `OverlayPartsTests.cs` (3 tests, pure with `FakeTooltips`).
//! Pure tests run everywhere. Native tests (`#[ignore = "native: needs desktop"]`) use a hidden message-less `STATIC` popup owner that is never shown; the
//! tooltip control is never activated by a pump (the tests run no message loop), so it never pops, and the tests that relay the mouse send
//! `TTM_ACTIVATE(false)` afterwards and assert the control is not visible. The real cursor is never touched.
//!
//! The fakes `FakeTooltips` and `FakeLocalPoster` live in this file: the shared `tests/common/overlay_fakes.rs` of 15f does not exist in this branch (the
//! contract says 15g appends to it; a private copy avoids a merge conflict on a file another agent creates).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Mutex;

use pr_app::input::primitives::RectD;
use pr_core::abstractions::UiPriority;
use pr_platform_win::host::error::HostError;
use pr_platform_win::shell_overlay::tooltips::{
    DeferredTooltips, LocalJob, LocalPoster, MAX_WIDTH_DIP, NativeTooltipHost, OverlayTooltip, OverlayTooltips,
    TOOLTIP_CLASS, TTDT_AUTOPOP, TTDT_INITIAL, TTM_DELTOOL, TTM_GETTEXT, TTM_GETTOOLINFO, TTM_RELAYEVENT,
    TooltipFactory, tool_rect_pixels, tools_unchanged,
};
use pr_platform_win::win32::activation_context::ActivationScope;
use pr_platform_win::win32::api::{kernel32, user32};
use pr_platform_win::win32::comctl::{SHELL_MANIFEST, user32 as comctl_user32};
use pr_platform_win::win32::control_constants as cc;
use pr_platform_win::win32::dialog_native::user32 as dialog_user32;
use pr_platform_win::win32::messages::{WM_LBUTTONDOWN, WM_MOUSEMOVE, WS_POPUP};
use pr_platform_win::win32::structs::{Hwnd, Lparam, Wparam};
use windows::Win32::UI::Controls as wc;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

// ---------------------------------------------------------------- helpers

fn tool(id: &str, x: f64, text: &str) -> OverlayTooltip {
    OverlayTooltip {
        id: id.to_owned(),
        bounds: RectD {
            x,
            y: 8.0,
            width: 40.0,
            height: 30.0,
        },
        text: text.to_owned(),
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

/// What a `FakeTooltips` saw (C# `FakeTooltips`).
#[derive(Default)]
struct FakeTooltips {
    syncs: RefCell<Vec<(Vec<OverlayTooltip>, f64)>>,
    relays: RefCell<Vec<(u32, Wparam, Lparam)>>,
    disposed: Cell<u32>,
}

impl OverlayTooltips for FakeTooltips {
    fn sync(&self, tools: &[OverlayTooltip], dpi_scale: f64) {
        self.syncs.borrow_mut().push((tools.to_vec(), dpi_scale));
    }
    fn relay_mouse(&self, message: u32, wparam: Wparam, lparam: Lparam) {
        self.relays.borrow_mut().push((message, wparam, lparam));
    }
    fn dispose(&self) {
        self.disposed.set(self.disposed.get() + 1);
    }
}

/// A poster that keeps the jobs until the test runs them (C# `post` delegate that stores the action).
#[derive(Default)]
struct FakeLocalPoster {
    jobs: RefCell<Vec<(UiPriority, LocalJob)>>,
}

impl FakeLocalPoster {
    fn queued(&self) -> usize {
        self.jobs.borrow().len()
    }
    fn priorities(&self) -> Vec<UiPriority> {
        self.jobs.borrow().iter().map(|(p, _)| *p).collect()
    }
    fn run_all(&self) {
        let jobs: Vec<_> = self.jobs.borrow_mut().drain(..).collect();
        for (_, job) in jobs {
            job();
        }
    }
}

impl LocalPoster for FakeLocalPoster {
    fn post(&self, priority: UiPriority, job: LocalJob) {
        self.jobs.borrow_mut().push((priority, job));
    }
}

/// A factory that counts its runs and hands out `fake`.
fn ok_factory(fake: &Rc<FakeTooltips>, runs: &Rc<Cell<u32>>) -> TooltipFactory {
    let fake = Rc::clone(fake);
    let runs = Rc::clone(runs);
    Box::new(move || {
        runs.set(runs.get() + 1);
        Ok(fake as Rc<dyn OverlayTooltips>)
    })
}

// ---------------------------------------------------------------- pure tests

#[test]
fn tool_rect_pixels_rounds_outwards() {
    // C# Sync_ConvertsDipBoundsToPhysicalPixelsWithTheDpiScale: (10, 8, 40, 30) at 1.5.
    assert_eq!(tool_rect_pixels(rect(10.0, 8.0, 40.0, 30.0), 1.5), (15, 12, 75, 57));
    // Fractional: the box grows on every side (floor left/top, ceil right/bottom).
    assert_eq!(tool_rect_pixels(rect(0.5, 0.5, 0.2, 0.2), 1.0), (0, 0, 1, 1));
    assert_eq!(tool_rect_pixels(rect(10.4, 8.6, 10.0, 10.0), 1.0), (10, 8, 21, 19));
    // Negative x: floor goes away from zero, ceil towards it.
    assert_eq!(tool_rect_pixels(rect(-1.2, -2.5, 1.0, 1.0), 1.0), (-2, -3, 0, -1));
    // Documented difference: NaN gives 0 (C# gives int.MinValue).
    assert_eq!(
        tool_rect_pixels(rect(f64::NAN, f64::NAN, f64::NAN, f64::NAN), 1.0),
        (0, 0, 0, 0)
    );
}

#[test]
fn tools_unchanged_table() {
    let base = vec![tool("a", 8.0, "x"), tool("b", 60.0, "y")];
    assert!(tools_unchanged(&base, 1.0, &base.clone(), 1.0), "same list, same dpi");
    assert!(tools_unchanged(&[], 1.0, &[], 1.0), "both empty");

    let mut text = base.clone();
    text[1].text = "z".to_owned();
    assert!(!tools_unchanged(&base, 1.0, &text, 1.0), "one text");

    let mut id = base.clone();
    id[0].id = "q".to_owned();
    assert!(!tools_unchanged(&base, 1.0, &id, 1.0), "one id");

    let mut bounds = base.clone();
    bounds[0].bounds.width = 41.0;
    assert!(!tools_unchanged(&base, 1.0, &bounds, 1.0), "one bound");

    assert!(!tools_unchanged(&base, 1.0, &base[..1], 1.0), "fewer tools");
    assert!(!tools_unchanged(&base[..1], 1.0, &base, 1.0), "more tools");
    assert!(!tools_unchanged(&base, 1.0, &base, 1.5), "dpi alone");
    assert!(
        !tools_unchanged(&base, f64::NAN, &base, f64::NAN),
        "NaN (never synced) never equals"
    );
    assert!(!tools_unchanged(&base, f64::NAN, &base, 1.0), "NaN old dpi");
}

#[test]
fn constants_match_the_windows_crate() {
    assert_eq!(TTM_DELTOOL, wc::TTM_DELTOOLW);
    assert_eq!(TTM_RELAYEVENT, wc::TTM_RELAYEVENT);
    assert_eq!(TTM_GETTEXT, wc::TTM_GETTEXTW);
    assert_eq!(TTM_GETTOOLINFO, wc::TTM_GETTOOLINFOW);
    assert_eq!(TTDT_INITIAL, wc::TTDT_INITIAL as usize);
    assert_eq!(TTDT_AUTOPOP, wc::TTDT_AUTOPOP as usize);
    assert_eq!(cc::TTM_ADDTOOL, wc::TTM_ADDTOOLW);
    assert_eq!(cc::TTM_SETMAXTIPWIDTH, wc::TTM_SETMAXTIPWIDTH);
    assert_eq!(cc::TTM_GETTOOLCOUNT, wc::TTM_GETTOOLCOUNT);
    assert_eq!(cc::TTM_SETDELAYTIME, wc::TTM_SETDELAYTIME);
    assert_eq!(cc::TTS_ALWAYSTIP, wc::TTS_ALWAYSTIP);
    assert_eq!(cc::TTS_NOPREFIX, wc::TTS_NOPREFIX);
    // T1: the C# `IccWin95Classes` of this unit is the value 4 (`ICC_BAR_CLASSES`), not 0xFF.
    assert_eq!(cc::ICC_BAR_CLASSES, 4);
    assert_eq!(size_of::<wc::TTTOOLINFOW>(), 72, "the comctl32 v6 TOOLINFOW");
    assert_eq!(TOOLTIP_CLASS, "tooltips_class32");
    assert_eq!(MAX_WIDTH_DIP, 420.0);
}

#[test]
fn constructor_rejects_a_missing_owner() {
    // C# Constructor_RejectsAMissingOwner: pure, no OS call happens before the check.
    let error = NativeTooltipHost::new(0).expect_err("owner 0");
    assert!(
        matches!(error, HostError::InvalidArgument { param: "owner", .. }),
        "got {error:?}"
    );
}

#[test]
fn nothing_is_created_until_the_posted_action_runs_then_the_pending_list_is_applied() {
    // C# DeferredTooltipsTests.NothingIsCreatedUntilThePostedActionRuns_ThenThePendingListIsApplied
    let poster = FakeLocalPoster::default();
    let fake = Rc::new(FakeTooltips::default());
    let runs = Rc::new(Cell::new(0));
    let deferred = DeferredTooltips::new(ok_factory(&fake, &runs), &poster);

    assert_eq!(poster.queued(), 1);
    assert_eq!(poster.priorities(), vec![UiPriority::Background]);
    assert_eq!(runs.get(), 0, "the factory waits for the posted job");
    assert!(!deferred.is_created());

    // Latest wins; relay before creation is dropped.
    deferred.sync(&[tool("a", 8.0, "first")], 1.25);
    deferred.sync(&[tool("b", 60.0, "second")], 2.0);
    deferred.relay_mouse(WM_MOUSEMOVE, 0, 0x000A_000A);
    assert!(fake.syncs.borrow().is_empty());
    assert!(fake.relays.borrow().is_empty());

    poster.run_all();
    assert!(deferred.is_created());
    assert_eq!(runs.get(), 1);
    assert_eq!(*fake.syncs.borrow(), vec![(vec![tool("b", 60.0, "second")], 2.0)]);

    // After creation everything forwards.
    deferred.sync(&[tool("c", 1.0, "third")], 1.0);
    deferred.relay_mouse(WM_LBUTTONDOWN, 1, 7);
    assert_eq!(fake.syncs.borrow().len(), 2);
    assert_eq!(fake.syncs.borrow()[1], (vec![tool("c", 1.0, "third")], 1.0));
    assert_eq!(*fake.relays.borrow(), vec![(WM_LBUTTONDOWN, 1, 7)]);
}

#[test]
fn the_pending_dpi_starts_at_one() {
    let poster = FakeLocalPoster::default();
    let fake = Rc::new(FakeTooltips::default());
    let runs = Rc::new(Cell::new(0));
    let _deferred = DeferredTooltips::new(ok_factory(&fake, &runs), &poster);
    poster.run_all();
    assert_eq!(*fake.syncs.borrow(), vec![(Vec::new(), 1.0)]);
}

#[test]
fn disposed_before_creation_never_creates_dispose_releases_the_control() {
    // C# DeferredTooltipsTests.DisposedBeforeCreation_NeverCreates_DisposeReleasesTheControl
    let poster = FakeLocalPoster::default();
    let fake = Rc::new(FakeTooltips::default());
    let runs = Rc::new(Cell::new(0));
    let first = DeferredTooltips::new(ok_factory(&fake, &runs), &poster);
    first.dispose();
    first.dispose();
    first.sync(&[tool("a", 8.0, "x")], 1.0);
    poster.run_all();
    assert_eq!(runs.get(), 0, "disposed before the job ran");
    assert!(!first.is_created());
    assert_eq!(fake.disposed.get(), 0);

    // Created, then disposed: the control is disposed once and dropped; later calls do nothing.
    let poster = FakeLocalPoster::default();
    let second = DeferredTooltips::new(ok_factory(&fake, &runs), &poster);
    poster.run_all();
    assert!(second.is_created());
    second.dispose();
    second.dispose();
    assert_eq!(fake.disposed.get(), 1);
    assert!(!second.is_created());
    let syncs = fake.syncs.borrow().len();
    second.sync(&[tool("a", 8.0, "x")], 1.0);
    second.relay_mouse(WM_MOUSEMOVE, 0, 0);
    assert_eq!(fake.syncs.borrow().len(), syncs);
    assert!(fake.relays.borrow().is_empty());
}

#[test]
fn creation_failure_leaves_no_tooltips_without_throwing() {
    // C# DeferredTooltipsTests.CreationFailure_LeavesNoTooltipsWithoutThrowing
    let poster = FakeLocalPoster::default();
    let runs = Rc::new(Cell::new(0));
    let counted = Rc::clone(&runs);
    let factory: TooltipFactory = Box::new(move || {
        counted.set(counted.get() + 1);
        Err(HostError::Win32 {
            operation: "Creating the tooltip control failed.",
            error: pr_platform_win::win32::structs::Win32Error(1400),
        })
    });
    let deferred = DeferredTooltips::new(factory, &poster);
    poster.run_all();
    assert_eq!(runs.get(), 1);
    assert!(!deferred.is_created());
    deferred.sync(&[tool("a", 8.0, "x")], 1.0);
    deferred.relay_mouse(WM_MOUSEMOVE, 0, 0);
    deferred.dispose();
    assert_eq!(runs.get(), 1, "a failure is final: the factory never runs again");
}

#[test]
fn a_dropped_deferred_tooltips_makes_the_posted_job_a_no_op() {
    // T7: the job holds only a Weak.
    let poster = FakeLocalPoster::default();
    let fake = Rc::new(FakeTooltips::default());
    let runs = Rc::new(Cell::new(0));
    let deferred = DeferredTooltips::new(ok_factory(&fake, &runs), &poster);
    drop(deferred);
    poster.run_all();
    assert_eq!(runs.get(), 0);
    assert!(fake.syncs.borrow().is_empty());
}

#[test]
fn the_factory_runs_once_and_later_syncs_forward_to_the_control() {
    // The contract's "posted twice" case cannot be built through the public API (the job is `FnOnce` and posted once): the observable rule is that
    // the factory runs exactly once and a second sync reaches the same control instead of creating another.
    let poster = FakeLocalPoster::default();
    let fake = Rc::new(FakeTooltips::default());
    let runs = Rc::new(Cell::new(0));
    let deferred = DeferredTooltips::new(ok_factory(&fake, &runs), &poster);
    poster.run_all();
    poster.run_all();
    deferred.sync(&[tool("a", 8.0, "x")], 1.0);
    assert_eq!(runs.get(), 1);
    assert_eq!(fake.syncs.borrow().len(), 2);
}

// ---------------------------------------------------------------- native tests

static NATIVE_LOCK: Mutex<()> = Mutex::new(());

/// Comctl32 v6 active on this thread (T5) plus a hidden `STATIC` popup owner, destroyed on drop (the scope is released after the owner).
struct Env {
    owner: Hwnd,
    _scope: ActivationScope,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Env {
    fn new() -> Self {
        let guard = NATIVE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let scope = ActivationScope::from_manifest_text(SHELL_MANIFEST).expect("activation scope");
        let module = kernel32::get_module_handle().expect("module");
        // SAFETY: the system `STATIC` class has no creation parameter (`param` 0); the window is a hidden popup that is never shown.
        let owner =
            unsafe { user32::create_window_ex(0, "STATIC", "owner", WS_POPUP, 0, 0, 200, 100, 0, 0, module, 0) }
                .expect("owner window");
        Self {
            owner,
            _scope: scope,
            _guard: guard,
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _destroyed = user32::destroy_window(self.owner);
    }
}

fn is_window(hwnd: Hwnd) -> bool {
    // SAFETY: `IsWindow` only reads a handle value.
    unsafe { IsWindow(Some(windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void))) }.as_bool()
}

fn info_of(owner: Hwnd, id: usize, text: *mut u16) -> wc::TTTOOLINFOW {
    wc::TTTOOLINFOW {
        cbSize: size_of::<wc::TTTOOLINFOW>() as u32,
        hwnd: windows::Win32::Foundation::HWND(owner as *mut core::ffi::c_void),
        uId: id,
        lpszText: windows::core::PWSTR(text),
        ..Default::default()
    }
}

fn send_int(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> isize {
    // SAFETY: only used with messages whose parameters are integers (no pointers).
    unsafe { dialog_user32::send_message(hwnd, message, wparam, lparam) }
}

#[test]
#[ignore = "native: needs desktop"]
fn sync_registers_every_tool_with_its_text() {
    let env = Env::new();
    let host = NativeTooltipHost::new(env.owner).expect("host");
    assert_eq!(host.tool_count(), 0);

    host.sync(&[tool("a", 8.0, "Mở thư mục ảnh"), tool("b", 60.0, "Cài đặt")], 1.0);

    assert_eq!(host.tool_count(), 2);
    assert_eq!(host.text_of(0), "Mở thư mục ảnh");
    assert_eq!(host.text_of(1), "Cài đặt");
    assert_eq!(host.text_of(2), "");
    assert!(!host.is_showing());
}

#[test]
#[ignore = "native: needs desktop"]
fn sync_replaces_the_whole_set_and_empty_clears_it() {
    let env = Env::new();
    let host = NativeTooltipHost::new(env.owner).expect("host");
    host.sync(
        &[tool("a", 8.0, "một"), tool("b", 60.0, "hai"), tool("c", 110.0, "ba")],
        1.0,
    );
    assert_eq!(host.tool_count(), 3);

    host.sync(&[tool("b", 60.0, "hai mới")], 1.0);
    assert_eq!(host.tool_count(), 1);
    assert_eq!(host.text_of(0), "hai mới");

    host.sync(&[], 1.0);
    assert_eq!(host.tool_count(), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn sync_same_list_again_is_a_no_op_changed_text_or_dpi_rebuilds() {
    let env = Env::new();
    let host = NativeTooltipHost::new(env.owner).expect("host");
    let tools = [tool("a", 8.0, "x")];
    host.sync(&tools, 1.0);
    host.sync(&tools, 1.0);
    assert_eq!(host.tool_count(), 1);

    // Proof of "no OS call": remove the tool behind the host's back; the same list again must NOT re-add it.
    let mut gone = info_of(env.owner, 1, core::ptr::null_mut());
    // SAFETY: TTM_DELTOOL takes a `TTTOOLINFOW` in lParam; `gone` is a live local with `cbSize`, `hwnd` and `uId` set.
    let _removed = unsafe {
        dialog_user32::send_message(
            host.handle(),
            TTM_DELTOOL,
            0,
            &mut gone as *mut wc::TTTOOLINFOW as Lparam,
        )
    };
    assert_eq!(host.tool_count(), 0);
    host.sync(&tools, 1.0);
    assert_eq!(host.tool_count(), 0, "the same list at the same dpi is a no-op");

    host.sync(&[tool("a", 8.0, "y")], 1.0);
    assert_eq!(host.tool_count(), 1, "changed text rebuilds");
    assert_eq!(host.text_of(0), "y");

    let mut gone = info_of(env.owner, 1, core::ptr::null_mut());
    // SAFETY: as above.
    let _removed = unsafe {
        dialog_user32::send_message(
            host.handle(),
            TTM_DELTOOL,
            0,
            &mut gone as *mut wc::TTTOOLINFOW as Lparam,
        )
    };
    host.sync(&[tool("a", 8.0, "y")], 1.5);
    assert_eq!(host.tool_count(), 1, "a dpi change alone rebuilds");
    assert_eq!(host.text_of(0), "y");

    // A non-positive dpi means 1: equal to the stored 1.5? no (rebuild), then equal to 1 again (no-op).
    host.sync(&[tool("a", 8.0, "y")], 0.0);
    assert_eq!(host.tool_count(), 1);
    let mut gone = info_of(env.owner, 1, core::ptr::null_mut());
    // SAFETY: as above.
    let _removed = unsafe {
        dialog_user32::send_message(
            host.handle(),
            TTM_DELTOOL,
            0,
            &mut gone as *mut wc::TTTOOLINFOW as Lparam,
        )
    };
    host.sync(&[tool("a", 8.0, "y")], -3.0);
    assert_eq!(
        host.tool_count(),
        0,
        "dpi <= 0 is normalized to 1, which is the stored dpi"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn relay_mouse_with_and_without_tools_does_not_throw_and_dispose_is_idempotent() {
    let env = Env::new();
    let host = NativeTooltipHost::new(env.owner).expect("host");
    host.relay_mouse(WM_MOUSEMOVE, 0, 0x000A_000A); // no tools yet
    host.sync(&[tool("a", 8.0, "x")], 1.0);
    host.set_initial_delay(1);
    host.relay_mouse(WM_MOUSEMOVE, 0, 0x000A_000A);
    host.relay_mouse(WM_MOUSEMOVE, 0, 0xFFF6_FFF6u32 as i32 as isize); // negative signed words
    // Never let the control pop: deactivate it, then check it is not visible (no pump ran either).
    send_int(host.handle(), cc::TTM_ACTIVATE, 0, 0);
    assert!(!host.is_showing());
    let handle = host.handle();
    assert!(is_window(handle));

    host.dispose();
    host.dispose();
    host.sync(&[tool("a", 8.0, "x")], 1.0); // nothing after dispose
    host.relay_mouse(WM_MOUSEMOVE, 0, 0);
    assert_eq!(host.tool_count(), 0);
    assert_eq!(host.handle(), 0);
    assert_eq!(host.text_of(0), "");
    assert!(!host.is_showing());
    assert!(!is_window(handle), "the tooltip control is destroyed on dispose");
}

#[test]
#[ignore = "native: needs desktop"]
fn sync_converts_dip_bounds_to_physical_pixels_with_the_dpi_scale() {
    let env = Env::new();
    let host = NativeTooltipHost::new(env.owner).expect("host");
    host.sync(
        &[OverlayTooltip {
            id: "a".to_owned(),
            bounds: rect(10.0, 8.0, 40.0, 30.0),
            text: "x".to_owned(),
        }],
        1.5,
    );

    let mut buffer = [0u16; 512];
    let mut info = info_of(env.owner, 1, buffer.as_mut_ptr());
    // SAFETY: TTM_GETTOOLINFOW takes a `TTTOOLINFOW` in lParam; `info` is a live local with `cbSize` set and `lpszText` pointing at a 512-unit buffer.
    let _found = unsafe {
        dialog_user32::send_message(
            host.handle(),
            TTM_GETTOOLINFO,
            0,
            &mut info as *mut wc::TTTOOLINFOW as Lparam,
        )
    };
    assert_eq!(
        (info.rect.left, info.rect.top, info.rect.right, info.rect.bottom),
        (15, 12, 75, 57)
    );
    // The control reports its own internal bit (0x40) back; what matters is that neither TTF_IDISHWND (1) nor TTF_SUBCLASS (0x10) is set.
    assert_eq!(
        info.uFlags.0 & 0x11,
        0,
        "a rectangle tool: no TTF_IDISHWND, no TTF_SUBCLASS"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn max_tip_width_follows_the_dpi() {
    let env = Env::new();
    let host = NativeTooltipHost::new(env.owner).expect("host");
    host.sync(&[tool("a", 8.0, "x")], 1.5);
    assert_eq!(send_int(host.handle(), wc::TTM_GETMAXTIPWIDTH, 0, 0), 630);
    host.sync(&[tool("a", 8.0, "x")], 2.0);
    assert_eq!(send_int(host.handle(), wc::TTM_GETMAXTIPWIDTH, 0, 0), 840);
    // Round half to even: 420 * 1.0025 = 421.05 -> 421.
    host.sync(&[tool("a", 8.0, "x")], 1.0025);
    assert_eq!(send_int(host.handle(), wc::TTM_GETMAXTIPWIDTH, 0, 0), 421);
}

#[test]
#[ignore = "native: needs desktop"]
fn the_control_has_the_tooltip_class() {
    let env = Env::new();
    let host = NativeTooltipHost::new(env.owner).expect("host");
    assert_eq!(
        comctl_user32::get_class_name(host.handle()),
        Some("tooltips_class32".to_owned())
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn drop_destroys_the_control() {
    let env = Env::new();
    let host = NativeTooltipHost::new(env.owner).expect("host");
    host.sync(&[tool("a", 8.0, "x")], 1.0);
    let handle = host.handle();
    assert!(is_window(handle));
    drop(host);
    assert!(!is_window(handle));
}
