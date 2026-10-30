//! Mutation killers of lane cl-inputdone for `shell_input/native.rs`, `drop_target.rs`, `surface.rs` and `handlers.rs`. No window, no GUI, no sleeps;
//! the OS reads are `#[ignore = "native: needs desktop"]` (run with `-- --include-ignored --test-threads=1`) and only read process/thread state.
//! Each test names the mutant (file:line, operator) it kills. Evidence: docs/reviews/CL-INPUTDONE-2026-10-30.md.
#![cfg(windows)]

use pr_platform_win::shell_input::native;
use pr_platform_win::win32::api::user32;
use pr_platform_win::win32::messages::{SM_CXDOUBLECLK, SM_CXDRAG, SM_CYDOUBLECLK, SM_CYDRAG};

// native.rs `drag_metrics_pixels -> (0|1|-1, 0|1|-1)`: the exact `GetSystemMetricsForDpi` pair, not only "positive and monotone"
// (the old test let the `(1, 1)` constant survive). Real drag metrics are 4 px at 96 dpi and scale with the dpi.
#[test]
#[ignore = "native: needs desktop"]
fn drag_metrics_are_exactly_the_system_metrics_at_each_dpi() {
    for dpi in [96_u32, 120, 144, 192] {
        let expected = (
            user32::get_system_metrics_for_dpi(SM_CXDRAG, dpi),
            user32::get_system_metrics_for_dpi(SM_CYDRAG, dpi),
        );
        assert_eq!(expected, native::drag_metrics_pixels(dpi), "dpi {dpi}");
        assert!(
            expected.0 > 1 && expected.1 > 1,
            "a real drag rectangle is wider than one pixel: {expected:?}"
        );
    }
}

// native.rs `double_click_size_pixels -> (...)`: same, for `SM_CXDOUBLECLK`/`SM_CYDOUBLECLK`.
#[test]
#[ignore = "native: needs desktop"]
fn double_click_size_is_exactly_the_system_metrics_at_each_dpi() {
    for dpi in [96_u32, 120, 144, 192] {
        let expected = (
            user32::get_system_metrics_for_dpi(SM_CXDOUBLECLK, dpi),
            user32::get_system_metrics_for_dpi(SM_CYDOUBLECLK, dpi),
        );
        assert_eq!(expected, native::double_click_size_pixels(dpi), "dpi {dpi}");
        assert!(expected.0 > 1 && expected.1 > 1, "{expected:?}");
    }
}

// native.rs `cursor_screen_pixel -> Some(Default::default())` / `None`, and a swapped x/y: the result is the exact `GetCursorPos` point. The cursor may
// move between two reads, so the read is repeated (no sleep) until the cursor was still around it.
#[test]
#[ignore = "native: needs desktop"]
fn cursor_screen_pixel_equals_get_cursor_pos_when_the_cursor_is_still() {
    use pr_platform_win::win32::comctl::user32 as comctl_user32;
    for _ in 0..10_000 {
        let before = comctl_user32::get_cursor_pos();
        let read = native::cursor_screen_pixel();
        let after = comctl_user32::get_cursor_pos();
        let (Some(before), Some(after)) = (before, after) else {
            assert!(read.is_none() || before.is_some() || after.is_some());
            if before.is_none() && after.is_none() {
                assert!(read.is_none(), "GetCursorPos failed twice: no point");
                return;
            }
            continue;
        };
        if before.x == after.x && before.y == after.y {
            let read = read.expect("the cursor is readable");
            assert_eq!(f64::from(before.x), read.x);
            assert_eq!(f64::from(before.y), read.y);
            return;
        }
    }
    panic!("the cursor never stood still for one read");
}

// native.rs `current_wheel_device`: `==` -> `!=` and `match guard -> true`. After a non-wheel message was retrieved on this thread, the message source
// is available but is not a touchpad: the answer must be `Unknown`. (The `Touchpad` branch needs a real precision touchpad: not observable here.)
#[test]
#[ignore = "native: needs desktop"]
fn a_non_pointer_message_source_is_not_a_touchpad() {
    use pr_app::input::touchpad_gestures::WheelDeviceHint;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW, PostThreadMessageW, WM_APP};
    // SAFETY: plain integer arguments; the posted thread message has no payload.
    unsafe { PostThreadMessageW(GetCurrentThreadId(), WM_APP, Default::default(), Default::default()) }
        .expect("PostThreadMessageW");
    let mut msg = MSG::default();
    // SAFETY: `msg` is a live local MSG that the OS fills.
    assert!(unsafe { PeekMessageW(&mut msg, None, WM_APP, WM_APP, PM_REMOVE) }.as_bool());
    assert_eq!(WheelDeviceHint::Unknown, native::current_wheel_device());
}

// ---- shell_input/drop_target.rs ----

mod strict_drop_data {
    use std::cell::Cell;
    use std::mem::ManuallyDrop;

    use pr_platform_win::shell_input::drop_target::{files, has_files};
    use pr_platform_win::win32::shell_native::CF_HDROP;
    use windows::Win32::Foundation::{DV_E_FORMATETC, E_NOTIMPL, HGLOBAL};
    use windows::Win32::System::Com::{
        DVASPECT_CONTENT, FORMATETC, IDataObject, IDataObject_Impl, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::core::{BOOL, HRESULT, implement};

    /// A data object that accepts ONLY the exact `CF_HDROP` request C# `DropData.HDropFormat()` builds: no target device, content aspect,
    /// `lindex == -1`, `TYMED_HGLOBAL`. Anything else is `DV_E_FORMATETC`, like a real shell data object.
    #[implement(IDataObject)]
    struct StrictData {
        exact_requests: Cell<u32>,
    }

    fn is_exact(format: &FORMATETC) -> bool {
        format.cfFormat == CF_HDROP
            && format.ptd.is_null()
            && format.dwAspect == DVASPECT_CONTENT.0
            && format.lindex == -1
            && format.tymed == TYMED_HGLOBAL.0 as u32
    }

    #[allow(non_snake_case)]
    impl IDataObject_Impl for StrictData_Impl {
        fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
            // SAFETY: the caller passes a live FORMATETC.
            let format = unsafe { &*pformatetcin };
            if !is_exact(format) {
                return Err(DV_E_FORMATETC.into());
            }
            self.exact_requests.set(self.exact_requests.get() + 1);
            // DROPFILES: pFiles = 20, fWide = 1, then "C:\x\0\0" in UTF-16.
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&20_u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 12]);
            bytes.extend_from_slice(&1_i32.to_le_bytes());
            for unit in r"C:\x".encode_utf16().chain([0, 0]) {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            // SAFETY: a fresh moveable block of the exact size, locked for the copy and unlocked; the medium owns it afterwards.
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.expect("GlobalAlloc");
            // SAFETY: `handle` was just allocated with `bytes.len()` bytes.
            unsafe {
                let ptr = GlobalLock(handle).cast::<u8>();
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
                let _ = GlobalUnlock(handle);
            }
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 {
                    hGlobal: HGLOBAL(handle.0),
                },
                pUnkForRelease: ManuallyDrop::new(None),
            })
        }
        fn GetDataHere(&self, _a: *const FORMATETC, _b: *mut STGMEDIUM) -> windows::core::Result<()> {
            Err(DV_E_FORMATETC.into())
        }
        fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
            // SAFETY: the caller passes a live FORMATETC.
            if is_exact(unsafe { &*pformatetc }) {
                HRESULT(0)
            } else {
                DV_E_FORMATETC
            }
        }
        fn GetCanonicalFormatEtc(&self, _a: *const FORMATETC, _b: *mut FORMATETC) -> HRESULT {
            DV_E_FORMATETC
        }
        fn SetData(&self, _a: *const FORMATETC, _b: *const STGMEDIUM, _c: BOOL) -> windows::core::Result<()> {
            Err(DV_E_FORMATETC.into())
        }
        fn EnumFormatEtc(&self, _d: u32) -> windows::core::Result<windows::Win32::System::Com::IEnumFORMATETC> {
            Err(DV_E_FORMATETC.into())
        }
        fn DAdvise(
            &self,
            _a: *const FORMATETC,
            _b: u32,
            _c: windows::core::Ref<windows::Win32::System::Com::IAdviseSink>,
        ) -> windows::core::Result<u32> {
            Err(E_NOTIMPL.into())
        }
        fn DUnadvise(&self, _a: u32) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
            Err(E_NOTIMPL.into())
        }
    }

    // drop_target.rs:41:17 `delete -` in `hdrop_format` (`lindex: -1` -> `1`): a data object that honours the format index (real shell data objects
    // answer `DV_E_FORMATETC` for index 1 of a single-medium format) sees the mutated request and refuses it, so `has_files` is false.
    #[test]
    fn hdrop_request_is_exactly_cf_hdrop_content_index_minus_one_hglobal() {
        let data: IDataObject = StrictData {
            exact_requests: Cell::new(0),
        }
        .into();
        assert!(has_files(&data), "QueryGetData must be asked with lindex -1");
        assert_eq!(
            vec![r"C:\x".to_string()],
            files(&data),
            "GetData must be asked with lindex -1"
        );
    }
}

// drop_target.rs:128:20 `<` -> `<=` and `<` -> `==` in `parse_drop_files` (`block.len() < DROPFILES_HEADER`): a block of EXACTLY the 20-byte header whose
// `pFiles` points inside the header still has a list (bytes 4..20 here), so the old "same result" argument held only for `pFiles == 20`.
#[test]
fn a_block_of_exactly_the_header_size_is_parsed_when_pfiles_points_into_it() {
    use pr_platform_win::shell_input::drop_target::parse_drop_files;
    let mut block = vec![0_u8; 20];
    block[0..4].copy_from_slice(&4_u32.to_le_bytes()); // pFiles = 4
    block[4..6].copy_from_slice(b"ab"); // ANSI item "ab" then NULs; fWide (bytes 16..20) = 0
    assert_eq!(vec!["ab".to_string()], parse_drop_files(&block));
    // One byte less is rejected before anything is read.
    assert!(parse_drop_files(&block[..19]).is_empty());
}

// drop_target.rs:196:9 `Debug for DropCallbacks::fmt -> Ok(Default::default())` and 211:9 `Display for DropRegisterError::fmt -> Ok(Default::default())`
// write nothing.
#[test]
fn drop_callbacks_debug_and_register_error_display_say_what_they_are() {
    use pr_platform_win::shell_input::drop_target::{DropCallbacks, DropRegisterError};
    use pr_platform_win::win32::structs::Hresult;
    let callbacks = DropCallbacks {
        open_path: Box::new(|_| Ok(())),
        on_failure: Box::new(|_| {}),
        files: None,
        has_files: None,
    };
    assert!(format!("{callbacks:?}").starts_with("DropCallbacks"));
    assert_eq!(
        "The drop target is already registered.",
        DropRegisterError::AlreadyRegistered.to_string()
    );
    let refused = DropRegisterError::Refused(Hresult(-2_147_467_259)).to_string();
    assert!(refused.starts_with("RegisterDragDrop failed: "), "{refused}");
}

// drop_target.rs:407:9 `Debug for ShellDropTarget::fmt -> Ok(Default::default())` writes nothing.
#[test]
fn shell_drop_target_debug_names_the_type_and_the_unregistered_hwnd() {
    use pr_platform_win::shell_input::drop_target::{DropCallbacks, ShellDropTarget};
    let target = ShellDropTarget::new(DropCallbacks {
        open_path: Box::new(|_| Ok(())),
        on_failure: Box::new(|_| {}),
        files: None,
        has_files: None,
    });
    let text = format!("{target:?}");
    assert!(text.starts_with("ShellDropTarget"), "{text}");
    assert!(text.contains("hwnd: 0"), "{text}");
}
