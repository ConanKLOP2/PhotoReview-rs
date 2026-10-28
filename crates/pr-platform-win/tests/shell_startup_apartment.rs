//! W8-20a: the STA apartment guard, real COM on spawned threads (new tests, no C# twin).
#![cfg(windows)]
use pr_platform_win::shell_startup::apartment::{ApartmentError, StaApartment};

#[test]
fn enter_gives_a_guard_on_a_fresh_thread() {
    std::thread::spawn(|| assert!(StaApartment::enter().is_ok()))
        .join()
        .unwrap();
}

#[test]
fn two_nested_guards_both_drop() {
    std::thread::spawn(|| {
        let a = StaApartment::enter().unwrap();
        let b = StaApartment::enter().unwrap();
        drop(b);
        drop(a);
    })
    .join()
    .unwrap();
}

#[test]
fn enter_after_the_mta_is_changed_mode() {
    std::thread::spawn(|| {
        pr_platform_win::wic::interop::ensure_com_initialized().unwrap();
        assert_eq!(StaApartment::enter().err(), Some(ApartmentError::ChangedMode));
    })
    .join()
    .unwrap();
}

#[test]
fn display_texts_name_the_cause() {
    assert_eq!(
        ApartmentError::ChangedMode.to_string(),
        "the thread is already in the multithreaded COM apartment"
    );
    assert_eq!(
        ApartmentError::Failed(-2147024809).to_string(),
        "CoInitializeEx failed (HRESULT 0x80070057)"
    );
}
