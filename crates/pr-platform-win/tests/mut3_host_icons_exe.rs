//! MUT3-HOST (2026-11-06): the production `icons::load` path with a REAL `RT_GROUP_ICON` 32512 resource in the running executable.
//!
//! A cargo test executable has no icon resource, so every earlier test only reached `load_from(0, ..)` (the system module) and `load -> 0` survived
//! (`host/icons.rs:30:5`). This test copies its own executable into a temp folder, adds a 16x16 32 bit icon as `RT_ICON` 1 and `RT_GROUP_ICON` 32512 with
//! `UpdateResource`, and runs the copy with `MUT3_ICON_CHILD=1` so the child test `child_probe` checks `load`, `load_big`, `load_small`, `apply` and the
//! `LR_SHARED` handle identity against the real resource. No window is shown (a hidden `STATIC` popup of the child only). Nothing outside the temp folder
//! is touched.
#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

use pr_platform_win::host::icons::{apply, load, load_big, load_small};
use pr_platform_win::host::native::{ICON_BIG, ICON_SMALL};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::{BeginUpdateResourceW, EndUpdateResourceW, UpdateResourceW};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SendMessageW, WINDOW_EX_STYLE, WM_GETICON, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const RT_ICON: usize = 3;
const RT_GROUP_ICON: usize = 14;
const GROUP_ID: usize = 32512;

/// The `RT_ICON` payload: a BITMAPINFOHEADER (height doubled), a 16x16 BGRA XOR bitmap (opaque red) and the 1 bit AND mask.
fn icon_image() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&40_u32.to_le_bytes()); // biSize
    data.extend_from_slice(&16_i32.to_le_bytes()); // biWidth
    data.extend_from_slice(&32_i32.to_le_bytes()); // biHeight (XOR + AND)
    data.extend_from_slice(&1_u16.to_le_bytes()); // biPlanes
    data.extend_from_slice(&32_u16.to_le_bytes()); // biBitCount
    data.extend_from_slice(&0_u32.to_le_bytes()); // biCompression
    data.extend_from_slice(&1024_u32.to_le_bytes()); // biSizeImage
    data.extend_from_slice(&[0; 16]); // resolution, colours
    for _ in 0..256 {
        data.extend_from_slice(&[0, 0, 255, 255]); // BGRA
    }
    data.extend_from_slice(&[0; 64]); // AND mask: 16 rows x 4 bytes
    data
}

fn icon_group(image_len: usize) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&0_u16.to_le_bytes()); // reserved
    data.extend_from_slice(&1_u16.to_le_bytes()); // type: icon
    data.extend_from_slice(&1_u16.to_le_bytes()); // count
    data.extend_from_slice(&[16, 16, 0, 0]); // width, height, colours, reserved
    data.extend_from_slice(&1_u16.to_le_bytes()); // planes
    data.extend_from_slice(&32_u16.to_le_bytes()); // bit count
    data.extend_from_slice(&(image_len as u32).to_le_bytes());
    data.extend_from_slice(&1_u16.to_le_bytes()); // RT_ICON id
    data
}

fn int_resource(id: usize) -> PCWSTR {
    PCWSTR(id as *const u16)
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("photoreview-rs-mut3-icons-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn add_icon_resources(exe: &std::path::Path) {
    let wide: Vec<u16> = exe.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    let image = icon_image();
    let group = icon_group(image.len());
    // SAFETY: `wide` is a NUL-terminated path of a file this test just copied; the update handle is used only here and closed by EndUpdateResourceW.
    unsafe {
        let update = BeginUpdateResourceW(PCWSTR(wide.as_ptr()), false).expect("BeginUpdateResource");
        UpdateResourceW(
            update,
            int_resource(RT_ICON),
            int_resource(1),
            0,
            Some(image.as_ptr().cast()),
            image.len() as u32,
        )
        .expect("RT_ICON");
        UpdateResourceW(
            update,
            int_resource(RT_GROUP_ICON),
            int_resource(GROUP_ID),
            0,
            Some(group.as_ptr().cast()),
            group.len() as u32,
        )
        .expect("RT_GROUP_ICON");
        EndUpdateResourceW(update, false).expect("EndUpdateResource");
    }
}

#[test]
fn the_production_icon_path_loads_a_real_exe_resource() {
    if std::env::var_os("MUT3_ICON_CHILD").is_some() {
        return; // the child runs `child_probe`, not this test
    }
    let dir = temp_dir();
    let child = dir.join("mut3_icons_child.exe");
    std::fs::copy(std::env::current_exe().expect("current exe"), &child).expect("copy the test exe");
    add_icon_resources(&child);
    let output = Command::new(&child)
        .args(["--exact", "child_probe", "--nocapture", "--test-threads=1"])
        .env("MUT3_ICON_CHILD", "1")
        .output()
        .expect("run the child");
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(text.contains("running 1 test"), "the child ran its probe: {text}");
    assert!(output.status.success(), "the child probe failed: {text}");
}

fn window_icon(hwnd: HWND, kind: usize) -> isize {
    // SAFETY: WM_GETICON has no pointer parameters; the window is alive.
    unsafe { SendMessageW(hwnd, WM_GETICON, Some(WPARAM(kind)), Some(LPARAM(0))) }.0
}

#[test]
fn child_probe() {
    if std::env::var_os("MUT3_ICON_CHILD").is_none() {
        return; // only meaningful inside the patched copy
    }
    // The resource is found: sizes the OS can scale to give a handle; an invalid size never reaches the OS.
    let sixteen = load(16, 16);
    assert_ne!(sixteen, 0, "load(16, 16)");
    assert_ne!(load(32, 32), 0, "load(32, 32)");
    assert_eq!(load(0, 16), 0, "zero width");
    assert_eq!(load(16, 0), 0, "zero height");
    assert_eq!(load(-16, 16), 0, "negative width");
    assert_eq!(load(16, -16), 0, "negative height");
    // `load` and `load_from` guard the same predicate (`cx <= 0 || cy <= 0`), so the guard of `load` is redundant: every pair with a non-positive side
    // gives 0 either way. Sweep the boundary values of both sides (the predicate only changes at 0).
    let sides = [i32::MIN, i32::MIN + 1, -65_536, -16, -2, -1, 0];
    let positives = [1, 2, 16, 32, 256, i32::MAX];
    for &bad in &sides {
        for &other in sides.iter().chain(positives.iter()) {
            assert_eq!(load(bad, other), 0, "load({bad}, {other})");
            assert_eq!(load(other, bad), 0, "load({other}, {bad})");
        }
    }
    assert_eq!(
        load(16, 16),
        sixteen,
        "LR_SHARED hands out the same handle for the same size"
    );
    assert_ne!(load(32, 32), sixteen, "another size is another shared handle");
    // The dpi variants: 0 = system metric, otherwise the metric for that dpi.
    for dpi in [0_u32, 96, 144, 192] {
        assert_ne!(load_big(dpi), 0, "load_big({dpi})");
        assert_ne!(load_small(dpi), 0, "load_small({dpi})");
    }
    assert_ne!(load_big(0), load_small(0), "big and small icons are different sizes");
    // `apply` sets both icons on the window, and only for a real window.
    assert!(!apply(0, 96), "no window, no icon");
    // SAFETY: a hidden STATIC popup created and destroyed on this thread.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("icons"),
            WS_POPUP,
            0,
            0,
            16,
            16,
            None,
            None,
            None,
            None,
        )
    }
    .expect("window");
    assert_eq!(window_icon(hwnd, ICON_BIG), 0, "no big icon before apply");
    assert_eq!(window_icon(hwnd, ICON_SMALL), 0, "no small icon before apply");
    assert!(apply(hwnd.0 as isize, 96), "apply reports that icons were found");
    assert_eq!(window_icon(hwnd, ICON_BIG), load_big(96), "WM_SETICON ICON_BIG");
    assert_eq!(window_icon(hwnd, ICON_SMALL), load_small(96), "WM_SETICON ICON_SMALL");
    // SAFETY: destroys the window created above.
    let _ = unsafe { DestroyWindow(hwnd) };
}
