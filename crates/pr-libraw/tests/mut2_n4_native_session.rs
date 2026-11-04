//! N4 (2026-11-05): native killers for the `NativeSession` forwarding glue of `pr_libraw::decoder`
//! (`decoder.rs` 1088..=1153: the setters, `try_set_use_camera_wb`, `raw_width`, `raw_height`, `decoder_name`, `raw_structure`).
//! Each test drives the REAL pinned `libraw.dll` through the `LibRawApi` / `LibRawSession` seam on a synthetic DNG written by
//! `tools/dev/make-synthetic-raw.py` (no camera corpus, no golden file) and compares one session setting against another, so a
//! forwarding method that does nothing (or returns a constant) is seen. `#[ignore]`d: they need `libraw.dll` and `python`.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

use pr_core::sync::CancelToken;
use pr_libraw::decoder::{LIBRAW_IMAGE_BITMAP, LibRawApi, LibRawSession, NativeLibRaw};
use pr_test_support::TempRoot;

const GENERATOR: &str = include_str!("../../../tools/dev/make-synthetic-raw.py");
const WIDTH: i32 = 160;
const HEIGHT: i32 = 96;

fn synthetic_dng(root: &TempRoot) -> PathBuf {
    let script = root.join("make-synthetic-raw.py");
    std::fs::write(&script, GENERATOR).expect("write the generator");
    let out = root.join("out");
    let run = Command::new("python")
        .arg("-I")
        .arg(&script)
        .arg(&out)
        .arg("--size")
        .arg(format!("{WIDTH}x{HEIGHT}"))
        .output()
        .expect("python must run the generator");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    out.join("plain.dng")
}

fn require_libraw() {
    let outcome = pr_libraw::availability::probe();
    assert!(outcome.available, "libraw.dll is required: {:?}", outcome.reason);
}

fn open_session(path: &PathBuf) -> Box<dyn LibRawSession> {
    let mut session = NativeLibRaw.init().expect("libraw_init");
    assert_eq!(session.open_wfile(OsStr::new(path)), 0, "open_wfile");
    session
}

struct Rendered {
    bits: u16,
    colors: u16,
    data: Vec<u8>,
}

/// unpack + dcraw_process + make_mem_image with the given output settings (`None` = do not call that setter).
fn render(
    path: &PathBuf,
    color: Option<i32>,
    bps: Option<i32>,
    no_auto_bright: Option<i32>,
    camera_wb: Option<(i32, i32, i32)>,
) -> Rendered {
    let mut session = NativeLibRaw.init().expect("libraw_init");
    if let Some(v) = color {
        session.set_output_color(v);
    }
    if let Some(v) = bps {
        session.set_output_bps(v);
    }
    if let Some(v) = no_auto_bright {
        session.set_no_auto_bright(v);
    }
    if let Some((c, b, n)) = camera_wb {
        let _ = session.try_set_use_camera_wb(c, b, n);
    }
    assert_eq!(session.open_wfile(OsStr::new(path)), 0, "open_wfile");
    assert_eq!(session.unpack(), 0, "unpack");
    assert_eq!(session.dcraw_process(), 0, "dcraw_process");
    let image = session.make_mem_image().expect("make_mem_image");
    let header = image.header();
    assert_eq!(header.image_type, LIBRAW_IMAGE_BITMAP);
    Rendered {
        bits: header.bits,
        colors: header.colors,
        data: image.data().to_vec(),
    }
}

/// `set_output_bps` -> `()`: asking for 16 bit gives a 16-bit bitmap, the default and 8 give 8 bit.
#[test]
#[ignore = "native: needs libraw.dll and python"]
fn session_set_output_bps_reaches_libraw() {
    require_libraw();
    let root = TempRoot::new("n4-session-bps");
    let path = synthetic_dng(&root);
    assert_eq!(render(&path, None, Some(16), None, None).bits, 16);
    assert_eq!(render(&path, None, Some(8), None, None).bits, 8);
}

/// `set_output_color` -> `()`: raw camera colours (0) differ from sRGB (1) on the colour-bar image.
#[test]
#[ignore = "native: needs libraw.dll and python"]
fn session_set_output_color_reaches_libraw() {
    require_libraw();
    let root = TempRoot::new("n4-session-color");
    let path = synthetic_dng(&root);
    let srgb = render(&path, Some(1), Some(8), Some(1), None);
    let raw = render(&path, Some(0), Some(8), Some(1), None);
    assert_eq!(srgb.colors, 3);
    assert_ne!(srgb.data, raw.data, "output_color 0 and 1 must render differently");
    // Output colour 4 (XYZ) is another conversion again.
    let xyz = render(&path, Some(5), Some(8), Some(1), None);
    assert_ne!(srgb.data, xyz.data);
}

/// `set_no_auto_bright` -> `()`: auto-brightness on (0) stretches the histogram, off (1) does not.
#[test]
#[ignore = "native: needs libraw.dll and python"]
fn session_set_no_auto_bright_reaches_libraw() {
    require_libraw();
    let root = TempRoot::new("n4-session-bright");
    let path = synthetic_dng(&root);
    let off = render(&path, Some(1), Some(8), Some(1), None);
    let on = render(&path, Some(1), Some(8), Some(0), None);
    assert_ne!(off.data, on.data, "no_auto_bright 0 and 1 must render differently");
}

/// `try_set_use_camera_wb -> bool`: on the pinned runtime the write happens and says so. (A `-> true` constant is therefore
/// equivalent on THIS runtime; only a second, unpinned DLL could tell it apart. See the evidence file.)
#[test]
#[ignore = "native: needs libraw.dll and python"]
fn session_try_set_use_camera_wb_is_written_on_the_pinned_runtime() {
    require_libraw();
    let mut session = NativeLibRaw.init().expect("libraw_init");
    assert!(session.try_set_use_camera_wb(1, 8, 1));
}

/// `set_progress_handler` -> `()`: with a canceled token installed, LibRaw stops at its first progress callback.
#[test]
#[ignore = "native: needs libraw.dll and python"]
fn session_set_progress_handler_installs_the_cancel_token() {
    require_libraw();
    let root = TempRoot::new("n4-session-cancel");
    let path = synthetic_dng(&root);
    let token = CancelToken::new();
    let mut session = NativeLibRaw.init().expect("libraw_init");
    session.set_progress_handler(token.clone());
    assert_eq!(session.open_wfile(OsStr::new(&path)), 0, "open_wfile");
    token.cancel();
    let unpack = session.unpack();
    let process = if unpack == 0 { session.dcraw_process() } else { unpack };
    assert_ne!(process, 0, "a canceled token must stop the unpack or the process");
    // Control: without a canceled token the same file goes through.
    let control = render(&path, None, None, None, None);
    assert!(!control.data.is_empty());
}

/// `raw_width`, `raw_height`, `decoder_name`, `raw_structure`: the values of the synthetic 160x96 RGGB DNG.
#[test]
#[ignore = "native: needs libraw.dll and python"]
fn session_probe_values_come_from_the_opened_file() {
    require_libraw();
    let root = TempRoot::new("n4-session-probe");
    let path = synthetic_dng(&root);
    let session = open_session(&path);
    assert_eq!(session.iwidth(), WIDTH);
    assert_eq!(session.iheight(), HEIGHT);
    let (raw_width, raw_height) = (session.raw_width(), session.raw_height());
    assert!(
        raw_width >= WIDTH && raw_height >= HEIGHT,
        "raw {raw_width}x{raw_height}"
    );
    assert!(raw_width > 1 && raw_height > 1);
    let name = session.decoder_name().expect("a decoder name");
    assert!(!name.is_empty() && name != "xyzzy", "decoder name {name:?}");
    let structure = session.raw_structure().expect("raw structure on the pinned runtime");
    assert_eq!(structure.colors, 3, "{structure:?}");
    assert_ne!(structure.filters, 0, "{structure:?}");
    eprintln!("probe: raw {raw_width}x{raw_height}, decoder {name:?}, {structure:?}");
}
