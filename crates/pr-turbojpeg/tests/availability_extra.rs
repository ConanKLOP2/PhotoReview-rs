use pr_turbojpeg::availability::{probe, probe_with};
use pr_turbojpeg::native::DLL_NAME;
use std::error::Error;
use std::sync::{Arc, Mutex};

type Log = Arc<Mutex<Vec<&'static str>>>;
struct D(Log);
impl Drop for D {
    fn drop(&mut self) {
        self.0.lock().unwrap().push("dispose");
    }
}

fn dll_present() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(DLL_NAME)))
        .is_some_and(|p| p.is_file())
}

#[test]
fn probe_is_cached_same_reference_without_dll() {
    assert!(std::ptr::eq(probe(), probe()));
}

#[test]
fn probe_without_dll_has_exact_not_loaded_reason() {
    if dll_present() {
        return;
    }
    let o = probe();
    assert!(!o.available);
    assert_eq!(
        o.reason.as_deref(),
        Some("Could not load turbojpeg.dll (not found or incompatible with the current process architecture).")
    );
}

#[test]
fn err_path_order_is_create_then_free_and_load_once() {
    let log: Log = Arc::default();
    let (l1, l2, l3) = (log.clone(), log.clone(), log.clone());
    let o = probe_with(
        "x.dll",
        move || {
            l1.lock().unwrap().push("load");
            Some(7u8)
        },
        move |h| {
            assert_eq!(h, 7);
            l2.lock().unwrap().push("free");
        },
        move || {
            l3.lock().unwrap().push("create");
            Err::<D, _>("bad".into()) as Result<D, Box<dyn Error + Send + Sync>>
        },
    );
    assert_eq!(o.reason.as_deref(), Some("x.dll loaded but failed to initialize: bad"));
    assert_eq!(*log.lock().unwrap(), vec!["load", "create", "free"]);
}

#[test]
fn ok_path_order_is_load_create_dispose_free() {
    let log: Log = Arc::default();
    let (l1, l2, l3) = (log.clone(), log.clone(), log.clone());
    let o = probe_with(
        "x.dll",
        move || {
            l1.lock().unwrap().push("load");
            Some(1u8)
        },
        move |_| {
            l2.lock().unwrap().push("free");
        },
        move || {
            l3.lock().unwrap().push("create");
            Ok(D(l3.clone()))
        },
    );
    assert!(o.available && o.reason.is_none());
    assert_eq!(*log.lock().unwrap(), vec!["load", "create", "dispose", "free"]);
}

#[test]
fn dll_name_is_interpolated_in_not_loaded_reason() {
    let o = probe_with::<u8, D>("other.dll", || None, |_| panic!(), || panic!());
    assert_eq!(
        o.reason.as_deref(),
        Some("Could not load other.dll (not found or incompatible with the current process architecture).")
    );
}

#[test]
fn display_not_debug_of_error() {
    let o = probe_with(
        "a.dll",
        || Some(0u8),
        |_| {},
        || Err::<D, Box<dyn Error + Send + Sync>>(Box::new(std::io::Error::other("m"))),
    );
    assert_eq!(o.reason.as_deref(), Some("a.dll loaded but failed to initialize: m"));
}
