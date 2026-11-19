//! Killers for the pr-platform-win P1 mutation survivors in `live_operation_registry.rs` and `natural_comparer.rs`
//! (see `docs/reviews/PLATFORM-P1-MUT-2026-11-19.md`). Real named-event round trips with a unique prefix; no desktop.
#[allow(dead_code)]
#[path = "../src/instance_keys.rs"]
mod instance_keys;

use std::sync::{Arc, Mutex};

use pr_core::abstractions::{LiveOperationRegistry, Log, NaturalComparer, NullLog};
use pr_core::catalog::natural_cmp::ManagedNaturalComparer;
use pr_platform_win::live_operation_registry::{WindowsLiveOperationRegistry, current_user_sid_for_tests};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessHandleCount, OpenEventW, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::core::{Error, HRESULT, PCWSTR};

#[derive(Default)]
struct RecordingLog(Mutex<Vec<String>>);

impl Log for RecordingLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _message: &str) {}
    fn warn(&self, _message: &str) {}
    fn error(&self, message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {
        self.0.lock().unwrap().push(message.to_owned());
    }
}

fn prefix(tag: &str) -> String {
    format!("PhotoReviewRsP1k{tag}{}", std::process::id())
}

fn hresult_from_win32(code: u32) -> Error {
    Error::from_hresult(HRESULT((0x8007_0000_u32 | code) as i32))
}

fn open_raw(name: &str) -> bool {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: NUL-terminated UTF-16 name that outlives the call.
    match unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, PCWSTR(wide.as_ptr())) } {
        Ok(handle) => {
            // SAFETY: handle just returned by OpenEventW.
            let _ = unsafe { CloseHandle(handle) };
            true
        }
        Err(_) => false,
    }
}

fn handle_count() -> u32 {
    let mut count = 0u32;
    // SAFETY: current-process pseudo handle and a valid out parameter.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) }.expect("handle count");
    count
}

#[test]
fn marker_round_trip_is_live_until_dropped() {
    // Kills is_live -> true / false, EventMarker::drop -> (), begin/name_for -> String::new().
    let registry = WindowsLiveOperationRegistry::new(Arc::new(NullLog), prefix("rt"));
    let other = WindowsLiveOperationRegistry::new(Arc::new(NullLog), prefix("rt"));
    assert!(!registry.is_live("op-1"));
    let marker = registry.begin("op-1");
    assert!(other.is_live("op-1"));
    assert!(!other.is_live("op-2"));
    assert!(!other.is_live(""));
    drop(marker);
    assert!(
        !other.is_live("op-1"),
        "dropping the marker must close the event handle"
    );
}

#[test]
fn event_name_is_local_prefixed_sid_bound_and_what_begin_creates() {
    // Kills name_for -> Ok(String::new()) / Ok("xyzzy"): the kernel object must exist under the exact Local\ name.
    let prefix = prefix("nm");
    let registry = WindowsLiveOperationRegistry::new(Arc::new(NullLog), &prefix);
    let name = registry.event_name_for_tests("folder\\photo:1").expect("name");
    assert!(name.starts_with(&format!("Local\\{prefix}_LiveOp_")));
    assert_eq!(name.len(), format!("Local\\{prefix}_LiveOp_").len() + 64);
    let digest = pr_core::session::store::sha256(
        format!("liveop|{}|{}", instance_keys::current_user_sid(), "folder\\photo:1").as_bytes(),
    );
    let hex: String = digest.iter().map(|byte| format!("{byte:02X}")).collect();
    assert_eq!(name, format!("Local\\{prefix}_LiveOp_{hex}"));
    assert!(!open_raw(&name));
    let _marker = registry.begin("folder\\photo:1");
    assert!(open_raw(&name));
    assert!(!open_raw("xyzzy"));
}

#[test]
fn current_user_sid_is_the_process_sid() {
    // Kills current_user_sid -> Ok(String::new()) / Ok("xyzzy"), `<` -> `>`, `!=` -> `==`, `+=` -> `-=` / `*=`.
    let sid = current_user_sid_for_tests().expect("sid");
    assert!(sid.starts_with("S-1-5-"), "unexpected SID {sid:?}");
    assert_eq!(sid, instance_keys::current_user_sid());
}

#[test]
fn current_user_sid_closes_its_token_handle() {
    // Kills `impl Drop for OwnedHandle::drop -> ()`.
    let _warm = current_user_sid_for_tests();
    let before = handle_count();
    for _ in 0..400 {
        let _ = current_user_sid_for_tests();
    }
    let after = handle_count();
    assert!(after < before + 100, "token handle leak: {before} -> {after}");
}

#[test]
fn open_failure_classification() {
    // Kills the is_live match guards (== -> !=, guard -> true/false), win32_error_code (-> 0/1, & -> | / ^) and
    // report -> ().
    let log = Arc::new(RecordingLog::default());
    let registry = WindowsLiveOperationRegistry::new(log.clone(), prefix("cl"));
    assert!(
        registry.open_failure_is_live_for_tests("id", &hresult_from_win32(5)),
        "access denied is live"
    );
    assert!(log.0.lock().unwrap().is_empty());
    assert!(
        !registry.open_failure_is_live_for_tests("id", &hresult_from_win32(2)),
        "file not found is not live"
    );
    assert!(log.0.lock().unwrap().is_empty());
    assert!(!registry.open_failure_is_live_for_tests("id-x", &hresult_from_win32(87)));
    let messages = log.0.lock().unwrap().clone();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].contains("could not be checked for journal operation id-x"),
        "{messages:?}"
    );
    // Only the low 16 bits are the Win32 code: other high bits must be masked away (kills `& 0xFFFF` -> `|` / `^`).
    let odd = Error::from_hresult(HRESULT(0x8007_0005_u32 as i32 ^ 0x0001_0000));
    assert!(registry.open_failure_is_live_for_tests("id", &odd));
}

#[test]
fn begin_failure_is_logged_and_not_live() {
    // Kills report -> () through the begin path: an over-long name makes CreateEventW fail.
    let log = Arc::new(RecordingLog::default());
    let registry = WindowsLiveOperationRegistry::new(log.clone(), "P".repeat(40_000));
    let _marker = registry.begin("x");
    let messages = log.0.lock().unwrap().clone();
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("could not be created for journal operation x"));
    assert!(!registry.is_live("x"));
}

#[test]
fn debug_format_names_the_type_and_prefix() {
    // Kills fmt -> Ok(Default::default()).
    let registry = WindowsLiveOperationRegistry::new(Arc::new(NullLog), "DbgPrefix");
    let text = format!("{registry:?}");
    assert!(
        text.contains("WindowsLiveOperationRegistry") && text.contains("DbgPrefix"),
        "{text}"
    );
}

#[test]
fn load_error_fallback_only_for_missing_module() {
    // Kills the ERROR_MOD_NOT_FOUND guard mutants (== -> !=, always/never) of the shlwapi load failure.
    pr_platform_win::natural_comparer::fallback_for_load_error(&hresult_from_win32(126));
    let denied = std::panic::catch_unwind(|| {
        pr_platform_win::natural_comparer::fallback_for_load_error(&hresult_from_win32(5));
    });
    assert!(denied.is_err(), "any other load failure must panic");
}

#[test]
fn native_comparer_uses_shell_order_not_managed_fallback() {
    // Kills native_compare -> None (every comparison would silently use the managed comparer).
    let native = pr_platform_win::natural_comparer::WindowsNaturalComparer::INSTANCE;
    let managed = ManagedNaturalComparer;
    for (left, right) in [("a-1", "a1"), ("a_1", "a1"), ("IMG_1.JPG", "img_1.jpg")] {
        assert_ne!(
            native.compare(Some(left), Some(right)),
            managed.compare(Some(left), Some(right)),
            "{left} vs {right}"
        );
    }
}
