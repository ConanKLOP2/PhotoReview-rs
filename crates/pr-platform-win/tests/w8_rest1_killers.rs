//! Mutation killers of lane cl-rest1 for `shell_startup/{locale,warmup,monitor_probe}.rs`, `shell_menus/popup_host.rs` and the tail of
//! `host/main_window/windowing.rs`. No window, no GUI, no DWM, no sleeps. Each test names the mutant (file:line, operator) it kills.
//! Locale tests assert exact text only when the user locale is `en-US` (the CI and development locale) and a weaker shape otherwise.
//! Evidence: docs/reviews/CL-REST1-2026-10-30.md.
#![cfg(windows)]

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pr_app::presenters::recovery_presentation::RecoveryTimeFormatter;
use pr_app::presenters::secondary::NumberFormatting;
use pr_app::services::initial_viewport::{MonitorProbe, PixelRect};
use pr_app::windowing::contracts::WindowShowState::Normal;
use pr_app::windowing::fullscreen::{FullscreenController, WindowOps};
use pr_app::windowing::monitor_layout::FixedMonitorLayout;
use pr_core::time::DateTimeUtc;
use pr_platform_win::host::main_window::windowing::{
    FullscreenNative, ShellFullscreen, ShellWindowingSeams, WindowingState,
};
use pr_platform_win::shell_startup::locale::{
    WindowsNumberFormatting, WindowsRecoveryTimeFormatter, group_separator, local_long_time_text,
};
use pr_platform_win::shell_startup::monitor_probe::Win32MonitorProbe;
use pr_platform_win::shell_startup::warmup::warm_graphics;

#[path = "common/windowing_fixture.rs"]
mod windowing_fixture;
use windowing_fixture::{FakeNative, FakeOps, HWND, MONITOR, NativeCall, WINDOW, WORK};

fn user_locale_name() -> String {
    let mut locale = [0u16; 85];
    // SAFETY: the output buffer is live and has the documented locale-name maximum capacity.
    let length = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut locale) };
    if length <= 1 {
        return String::new();
    }
    String::from_utf16_lossy(&locale[..(length - 1) as usize])
}

fn is_en_us() -> bool {
    user_locale_name() == "en-US"
}

// ---- shell_startup/locale.rs ----

// locale.rs:19:5 `locale_name -> [0; N]` / `[1; N]`, :22:15 `> -> ==` / `<`, :27:18 `== -> !=`, :32:8 `> -> ==` / `<`, :32:56 `- -> +` / `/`, :25:5
// `query_locale -> None / Some("") / Some("xyzzy")`, :159:5 `group_separator -> String::new() / "xyzzy"`: the separator is exactly the locale's ',' without a NUL.
#[test]
fn locale_group_separator_is_exactly_comma_for_en_us() {
    let separator = group_separator();
    assert!(!separator.contains('\0'), "{separator:?}");
    assert!(separator.chars().count() <= 1, "{separator:?}");
    if is_en_us() {
        assert_eq!(separator, ",");
    }
}

// locale.rs:35:5 `format_number -> ""/"xyzzy"`, :58:18 `== -> !=`, :71:10 `> -> ==`/`<`, :72:44 `- -> +`/`/`, :85:9 `grouped_i64 -> ""/"xyzzy"`.
#[test]
fn locale_grouped_i64_formats_exact_en_us_text() {
    if !is_en_us() {
        return;
    }
    let formatter = WindowsNumberFormatting;
    assert_eq!(formatter.grouped_i64(0), "0");
    assert_eq!(formatter.grouped_i64(999), "999");
    assert_eq!(formatter.grouped_i64(1_000), "1,000");
    assert_eq!(formatter.grouped_i64(1_234_567), "1,234,567");
    assert_eq!(formatter.grouped_i64(-1_234_567), "-1,234,567");
    assert_eq!(formatter.grouped_i64(i64::MAX), "9,223,372,036,854,775,807");
}

// locale.rs:89:9 `grouped_f64 -> ""/"xyzzy"`, :89:12 `delete !`, :92:32 `>= -> <`, :93:20 `+ -> -`/`*`, :95:20 `- -> +`/`/`, :97:38 `|| -> &&`,
// :97:20 `< -> ==`/`>`/`<=`, :97:49 `> -> ==`/`<`/`>=`.
#[test]
fn locale_grouped_f64_rounds_halves_away_from_zero_and_handles_the_extremes() {
    let formatter = WindowsNumberFormatting;
    // Non finite and out of range values fall back to the plain `to_string` text in every locale.
    assert_eq!(formatter.grouped_f64(f64::NAN), f64::NAN.to_string());
    assert_eq!(formatter.grouped_f64(f64::INFINITY), f64::INFINITY.to_string());
    assert_eq!(formatter.grouped_f64(f64::NEG_INFINITY), f64::NEG_INFINITY.to_string());
    assert_eq!(formatter.grouped_f64(1e30), 1e30_f64.to_string());
    assert_eq!(formatter.grouped_f64(-1e30), (-1e30_f64).to_string());
    // 2^63 is NOT above `i64::MAX as f64` (== 2^63): it saturates to i64::MAX and is grouped; -2^63 is not below `i64::MIN as f64` either.
    assert_eq!(
        formatter.grouped_f64(9_223_372_036_854_775_808.0),
        formatter.grouped_i64(i64::MAX)
    );
    assert_eq!(
        formatter.grouped_f64(-9_223_372_036_854_775_808.0),
        formatter.grouped_i64(i64::MIN)
    );
    // Halves go away from zero; the sign of the zero side matters (`value >= 0.0`).
    assert_eq!(formatter.grouped_f64(0.0), formatter.grouped_i64(0));
    assert_eq!(formatter.grouped_f64(0.5), formatter.grouped_i64(1));
    assert_eq!(formatter.grouped_f64(-0.5), formatter.grouped_i64(-1));
    assert_eq!(formatter.grouped_f64(0.4), formatter.grouped_i64(0));
    assert_eq!(formatter.grouped_f64(2.5), formatter.grouped_i64(3));
    assert_eq!(formatter.grouped_f64(-2.5), formatter.grouped_i64(-3));
    assert_eq!(formatter.grouped_f64(1234.4), formatter.grouped_i64(1234));
    assert_eq!(formatter.grouped_f64(-1234.6), formatter.grouped_i64(-1235));
    if is_en_us() {
        assert_eq!(formatter.grouped_f64(1_234.5), "1,235");
        assert_eq!(formatter.grouped_f64(-1_234.5), "-1,235");
        assert_eq!(formatter.grouped_f64(0.4), "0");
    }
}

// locale.rs:105:9 `percent_one_decimal -> ""/"xyzzy"`, :105:28 `* -> +`/`/`, :106:12 `delete !`, :58:18 (format_number) via the decimal text.
#[test]
fn locale_percent_one_decimal_scales_the_ratio_by_one_hundred() {
    let formatter = WindowsNumberFormatting;
    assert_eq!(formatter.percent_one_decimal(f64::NAN), format!("{}%", f64::NAN));
    assert_eq!(
        formatter.percent_one_decimal(f64::INFINITY),
        format!("{}%", f64::INFINITY)
    );
    if is_en_us() {
        assert_eq!(formatter.percent_one_decimal(0.5), "50.0%");
        assert_eq!(formatter.percent_one_decimal(1.0), "100.0%");
        assert_eq!(formatter.percent_one_decimal(0.123), "12.3%");
        assert_eq!(formatter.percent_one_decimal(0.005), "0.5%");
        assert_eq!(formatter.percent_one_decimal(12.5), "1,250.0%");
        assert_eq!(formatter.percent_one_decimal(-0.25), "-25.0%");
    }
}

// locale.rs:130:9 `format_local_general -> ""/"xyzzy"`, :170:5 `format_system_time -> ""/"xyzzy"`, :172:18 `== -> !=`, :196:10 `> -> ==`/`<`,
// :197:44 `- -> +`/`/` (a trailing NUL in the text). The expected local time is computed with the same time zone API in the test.
#[test]
fn locale_recovery_time_formatter_formats_the_local_short_date_and_time() {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    // 2020-07-01 12:00:00 UTC in .NET ticks.
    let ticks = 637_292_016_000_000_000_i64;
    let instant = DateTimeUtc::from_ticks(ticks).expect("valid ticks");
    let formatted = WindowsRecoveryTimeFormatter.format_local_general(instant);
    assert!(!formatted.is_empty());
    assert!(!formatted.contains('\0'), "{formatted:?}");
    assert!(
        formatted.chars().filter(char::is_ascii_digit).count() >= 6,
        "{formatted:?}"
    );
    if !is_en_us() {
        return;
    }
    let filetime = (ticks - 504_911_232_000_000_000) as u64;
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // SAFETY: filetime is a live local scalar; utc is a live writable SYSTEMTIME.
    unsafe { FileTimeToSystemTime((&filetime as *const u64).cast(), &mut utc) }.expect("filetime");
    // SAFETY: both SYSTEMTIME values are live locals; a null zone selects the machine zone.
    unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.expect("local");
    let hour12 = match local.wHour % 12 {
        0 => 12,
        h => h,
    };
    let suffix = if local.wHour < 12 { "AM" } else { "PM" };
    let expected = format!(
        "{}/{}/{} {}:{:02} {}",
        local.wMonth, local.wDay, local.wYear, hour12, local.wMinute, suffix
    );
    assert_eq!(formatted, expected);
}

// locale.rs:165:5 `local_long_time_text -> ""/"xyzzy"`, :170:5, :196:10, :197:44: the en-US pattern is `h:mm:ss tt` (C# "T", seconds kept since FIX-G10-G13), no NUL.
#[test]
fn locale_local_long_time_text_has_the_en_us_clock_shape() {
    let text = local_long_time_text();
    assert!(!text.is_empty());
    assert!(!text.contains('\0'), "{text:?}");
    assert_ne!(text, "xyzzy");
    if is_en_us() {
        let (clock, suffix) = text.split_once(' ').expect("time and AM/PM");
        assert!(suffix == "AM" || suffix == "PM", "{text:?}");
        let mut parts = clock.split(':');
        let hour: u32 = parts.next().expect("h:mm:ss").parse().expect("hour digits");
        assert!((1..=12).contains(&hour), "{text:?}");
        for part in [parts.next().expect("minutes"), parts.next().expect("seconds")] {
            assert_eq!(part.len(), 2, "{text:?}");
            assert!(part.chars().all(|c| c.is_ascii_digit()), "{text:?}");
        }
        assert!(parts.next().is_none(), "{text:?}");
    }
}

// ---- shell_startup/warmup.rs ----

// warmup.rs:127:5 `warm_graphics -> None`: the worker thread is started and its mark (when the device exists) arrives.
#[test]
fn warm_graphics_starts_a_worker_that_finishes() {
    let marks = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&marks);
    let handle = warm_graphics(Box::new(move |mark| {
        sink.lock().unwrap().push(mark.to_owned());
    }))
    .expect("the warm-graphics thread starts");
    handle.join().expect("the worker never panics out of catch_unwind");
    let marks = marks.lock().unwrap();
    // No GPU/WARP device: no mark; otherwise exactly one.
    assert!(marks.is_empty() || *marks == ["graphicsWarm"], "{marks:?}");
}

// ---- shell_startup/monitor_probe.rs ----

fn probe_primary_work_area() -> (pr_app::services::initial_viewport::MonitorId, PixelRect) {
    let probe = Win32MonitorProbe;
    let monitor = probe.monitor_primary().expect("a primary monitor exists");
    let area = probe
        .monitor_work_area(monitor)
        .expect("the primary monitor has a work area");
    (monitor, area)
}

// monitor_probe.rs:47:9 `monitor_primary -> None / Some(Default)`, :48:17 `!= -> ==`, :53:9 `monitor_work_area -> None / Some(Default)`, :18:5 `pixel_rect -> Default`.
#[test]
fn probe_primary_monitor_has_a_positive_work_area() {
    let (monitor, area) = probe_primary_work_area();
    assert_ne!(monitor.0, 0);
    assert!(area.right > area.left && area.bottom > area.top, "{area:?}");
    assert!(
        area.right - area.left >= 640 && area.bottom - area.top >= 400,
        "{area:?}"
    );
}

// monitor_probe.rs:84:9 `work_areas -> vec![] / vec![Default]`: the primary work area is one of the enumerated areas, and none is empty.
#[test]
fn probe_work_areas_contain_the_primary_work_area() {
    let (_, primary) = probe_primary_work_area();
    let areas = Win32MonitorProbe.work_areas();
    assert!(!areas.is_empty());
    assert!(areas.contains(&primary), "{primary:?} not in {areas:?}");
    assert!(areas.iter().all(|a| a.right > a.left && a.bottom > a.top), "{areas:?}");
}

// monitor_probe.rs:33:9 `monitor_nearest_rect -> None / Some(Default)`, :42:17 `!= -> ==`: a rect inside the primary work area maps to the primary monitor,
// and the work area is of that monitor.
#[test]
fn probe_nearest_rect_inside_the_primary_work_area_is_the_primary_monitor() {
    let probe = Win32MonitorProbe;
    let (primary, area) = probe_primary_work_area();
    let inside = PixelRect {
        left: area.left + 10,
        top: area.top + 10,
        right: area.left + 110,
        bottom: area.top + 110,
    };
    let nearest = probe.monitor_nearest_rect(inside).expect("a monitor is always nearest");
    assert_ne!(nearest.0, 0);
    assert_eq!(nearest, primary);
    assert_eq!(probe.monitor_work_area(nearest), Some(area));
}

// monitor_probe.rs:61:9 `dpi_for -> 0 / 1`: a real monitor has at least 96 dpi.
#[test]
fn probe_dpi_is_at_least_96_for_the_primary_monitor() {
    let (primary, _) = probe_primary_work_area();
    let dpi = Win32MonitorProbe.dpi_for(primary);
    assert!((96..=960).contains(&dpi), "{dpi}");
}

// monitor_probe.rs:76:9 `frame_insets -> None / Some(Default)`: the overlapped window frame has a caption (negative top) and borders at every dpi.
#[test]
fn probe_frame_insets_describe_a_caption_and_borders() {
    let probe = Win32MonitorProbe;
    for dpi in [96_u32, 144, 192] {
        let insets = probe
            .frame_insets(dpi)
            .expect("AdjustWindowRectExForDpi works for a plain style");
        assert!(insets.left < 0 && insets.top < 0, "{dpi}: {insets:?}");
        assert!(insets.right > 0 && insets.bottom > 0, "{dpi}: {insets:?}");
        assert!(
            -insets.top > -insets.left,
            "{dpi}: the caption is taller than the border: {insets:?}"
        );
    }
    let small = probe.frame_insets(96).unwrap();
    let large = probe.frame_insets(192).unwrap();
    assert!(large.top < small.top, "insets grow with dpi: {small:?} vs {large:?}");
}

// ---- host/main_window/windowing.rs ----

fn active_shell_fullscreen() -> (Rc<FakeNative>, Rc<ShellFullscreen>) {
    let ops = Rc::new(FakeOps::baseline());
    let native = Rc::new(FakeNative::new(Normal));
    let controller = FullscreenController::new(Box::new(FixedMonitorLayout::new(vec![WORK])));
    let ops: Rc<dyn WindowOps> = ops;
    let native_dyn: Rc<dyn FullscreenNative> = native.clone();
    (native, Rc::new(ShellFullscreen::new(controller, ops, native_dyn)))
}

// windowing.rs:340:31 `refit: !active || hwnd == 0` -> `&&`: a window that is NOT in F11 is never moved, even with a handle and a window off the monitor.
#[test]
fn refit_of_an_inactive_window_off_the_monitor_never_moves_it() {
    let (native, fullscreen) = active_shell_fullscreen();
    native.window.set(Some(WINDOW));
    assert!(!fullscreen.refit(HWND));
    assert!(native.calls().is_empty(), "{:?}", native.calls());
}

// windowing.rs:340:31 `||` -> `&&`: an ACTIVE F11 window with a null handle is never moved either; the same window with a real handle is moved onto the monitor.
#[test]
fn refit_with_a_null_handle_while_active_never_moves_and_a_real_handle_does() {
    let (native, fullscreen) = active_shell_fullscreen();
    assert!(fullscreen.apply(HWND, true));
    native.window.set(Some(WINDOW));
    assert!(!fullscreen.refit(0));
    assert!(
        !native.calls().contains(&NativeCall::MoveTo(MONITOR)),
        "{:?}",
        native.calls()
    );
    assert!(fullscreen.refit(HWND));
    assert_eq!(native.calls().last(), Some(&NativeCall::MoveTo(MONITOR)));
}

// windowing.rs:386:9 `<Debug for ShellWindowingSeams>::fmt -> Ok(Default::default())` and :401:9 `<Debug for WindowingState>::fmt` write nothing.
#[test]
fn windowing_seams_and_state_debug_name_their_types() {
    let seams = format!("{:?}", ShellWindowingSeams::default());
    assert!(seams.starts_with("ShellWindowingSeams"), "{seams}");
    let state = format!("{:?}", WindowingState::default());
    assert!(state.starts_with("WindowingState"), "{state}");
}
