//! W4-08b repair-round tests: quality 95, cancellation, error classification, i32 payload bound, CRLF message.
use std::io::Write;
use std::sync::Mutex;

use pr_core::model::DecoderBackend;
use pr_core::sync::CancelToken;
use pr_imaging::cache::preview_file::*;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};

struct Rec(Mutex<Vec<i32>>);
impl PreviewPayloadCodec for Rec {
    fn encode_jpeg(&self, _p: &PixelBuffer, q: i32, out: &mut dyn Write) -> std::io::Result<()> {
        self.0.lock().unwrap().push(q);
        out.write_all(&[0xFF, 0xD8, 0, 0, 0xFF, 0xD9])
    }
    fn decode_jpeg(&self, _p: &[u8], _l: PixelLayout, _w: i32, _h: i32) -> Result<PixelBuffer, PreviewCacheError> {
        unreachable!()
    }
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pr-w4-08b-repair-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

// C#: PreviewCacheFile.WriteAtomicallyAsync -> WicImageEncoder.EncodeJpeg(pixels, stream, DefaultJpegQuality=95)
#[test]
fn encoder_receives_quality_95() {
    let d = dir("q");
    let codec = Rec(Mutex::new(vec![]));
    let px = PixelBuffer::allocate(4, 4, PixelLayout::Bgr32).unwrap();
    write_atomically(
        &px,
        DecoderBackend::Wpf,
        1,
        0,
        0,
        &d.join("a.pv4"),
        false,
        None,
        &codec,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(*codec.0.lock().unwrap(), vec![95]);
    let _ = std::fs::remove_dir_all(&d);
}

// C#: AtomicCacheFile.WriteAsync(cancellationToken): OperationCanceledException, no file, no temp.
#[test]
fn canceled_token_yields_canceled_and_no_file() {
    let d = dir("c");
    let codec = Rec(Mutex::new(vec![]));
    let px = PixelBuffer::allocate(4, 4, PixelLayout::Bgr32).unwrap();
    let t = CancelToken::new();
    t.cancel();
    let r = write_atomically(
        &px,
        DecoderBackend::Wpf,
        1,
        0,
        0,
        &d.join("a.pv4"),
        false,
        None,
        &codec,
        &t,
    );
    assert!(matches!(r, Err(PreviewCacheError::Canceled)), "{r:?}");
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

// C#: DiskCacheStore.IsCacheEntryFailure: OutOfMemory (InsufficientMemoryException) is NOT an entry failure.
#[test]
fn error_classification_and_display() {
    assert!(!PreviewCacheError::OutOfMemory("x".into()).is_cache_entry_failure());
    assert!(!PreviewCacheError::Canceled.is_cache_entry_failure());
    assert!(PreviewCacheError::InvalidData("x".into()).is_cache_entry_failure());
    assert!(PreviewCacheError::Io(std::io::Error::other("x")).is_cache_entry_failure());
    assert_eq!(PreviewCacheError::InvalidData("abc".into()).to_string(), "abc");
    assert_eq!(PreviewCacheError::OutOfMemory("oom".into()).to_string(), "oom");
    assert_eq!(PreviewCacheError::Canceled.to_string(), "The operation was canceled.");
}

// C#: payloadLength > int.MaxValue => "implausibly large" (needs > 2 GiB of lazily committed zero pages).
#[test]
#[ignore = "slow: allocates more than 2 GiB"]
fn payload_over_i32_max_is_implausibly_large() {
    let len = 24 + 2 + (i32::MAX as usize) + 1;
    let mut b = vec![0u8; len];
    b[..4].copy_from_slice(b"PRVC");
    b[4] = 7;
    b[6] = 1;
    for at in [8usize, 12, 16, 20] {
        b[at..at + 4].copy_from_slice(&1i32.to_le_bytes());
    }
    match parse_entry(&b) {
        Err(PreviewCacheError::InvalidData(m)) => assert_eq!(m, "Preview cache entry payload is implausibly large."),
        other => panic!("{other:?}"),
    }
}

// C#: ArgumentOutOfRangeException.Message uses Environment.NewLine (CRLF on Windows); golden has "\r\n".
#[cfg(windows)]
#[test]
fn orientation_message_uses_crlf_like_dotnet_on_windows() {
    let e = build_entry_prefix(DecoderBackend::Wpf, 9, 4, 4, 4, 4, None).unwrap_err();
    assert_eq!(
        e.to_string(),
        "EXIF orientation must be 1-8. (Parameter 'orientation')\r\nActual value was 9."
    );
}
