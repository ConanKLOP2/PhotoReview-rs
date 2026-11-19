//! Killers for the pr-platform-win P1 mutation survivors in `instance_keys.rs`
//! (see `docs/reviews/PLATFORM-P1-MUT-2026-11-19.md`). Real token reads; no desktop needed.
#[path = "../src/instance_keys.rs"]
mod instance_keys;

use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};

fn handle_count() -> u32 {
    let mut count = 0u32;
    // SAFETY: the pseudo handle of the current process and a valid out parameter.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) }.expect("handle count");
    count
}

#[test]
fn user_sid_reader_returns_a_real_sid() {
    // Kills read_user_sid -> String::new()/"xyzzy"?, `+=` -> `*=` (length never grows), and the
    // size/err guards turned into the "nosid" path.
    let sid = instance_keys::read_user_sid_for_tests();
    assert!(sid.starts_with("S-1-5-"), "unexpected SID {sid:?}");
    assert!(sid.len() > "S-1-5-18".len() - 1);
    assert!(sid.chars().all(|c| c.is_ascii_digit() || c == 'S' || c == '-'));
    assert_eq!(sid, instance_keys::current_user_sid());
}

#[test]
fn user_sid_reader_closes_its_token_handle() {
    // Kills `impl Drop for Token::drop -> ()`: every uncached read would leak one process-token handle.
    let _warm = instance_keys::read_user_sid_for_tests();
    let before = handle_count();
    for _ in 0..400 {
        let _ = instance_keys::read_user_sid_for_tests();
    }
    let after = handle_count();
    assert!(after < before + 100, "token handle leak: {before} -> {after}");
}

#[test]
fn session_id_matches_independent_win32_read() {
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    let mut expected = u32::MAX;
    // SAFETY: valid out parameter, current process id.
    unsafe { ProcessIdToSessionId(std::process::id(), &mut expected) }.expect("session id");
    assert_eq!(instance_keys::read_session_id_for_tests(), expected);
    assert_eq!(instance_keys::current_session_id(), expected);
}
