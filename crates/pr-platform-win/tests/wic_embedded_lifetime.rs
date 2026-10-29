//! Native lifetime regressions for the production EmbeddedThumbnailReader adapter.
#![cfg(windows)]
use pr_imaging::embedded_thumbnail::EmbeddedThumbnailBackend;
use pr_platform_win::wic::{embedded_thumbnail::WicEmbeddedThumbnailBackend, interop::outstanding_on_this_thread};
use pr_test_support::golden;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct TrackedStream {
    bytes: Cursor<Vec<u8>>,
    drops: Arc<AtomicUsize>,
}
impl Read for TrackedStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.bytes.read(bytes)
    }
}
impl Seek for TrackedStream {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.bytes.seek(pos)
    }
}
impl Drop for TrackedStream {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
fn fixture() -> Vec<u8> {
    std::fs::read(golden::repo_root().join("tests/fixtures/embedded-thumbnail/quad-orientation-6.jpg")).unwrap()
}
#[test]
#[ignore = "native: needs WIC"]
fn open_frame_drop_releases_stream_and_com_references() {
    // C# EmbeddedThumbnailReader.ReadFromStream: finally releases frame/decoder/managedStream.
    let before = outstanding_on_this_thread();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut frame = WicEmbeddedThumbnailBackend
        .open(Box::new(TrackedStream {
            bytes: Cursor::new(fixture()),
            drops: drops.clone(),
        }))
        .unwrap()
        .unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert!(frame.main_size().unwrap().0 > 0);
    let mut thumbnail = frame.thumbnail().unwrap();
    let (w, h) = thumbnail.size().unwrap();
    let mut pixels = vec![0; (w * h * 4) as usize];
    thumbnail.copy_pbgra(&mut pixels, (w * 4) as usize).unwrap();
    assert_eq!(frame.orientation(), 6);
    drop(thumbnail);
    drop(frame);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(outstanding_on_this_thread(), before);
}
#[test]
#[ignore = "native: needs WIC"]
fn open_invalid_header_releases_stream_and_com_references() {
    // C# EmbeddedThumbnailReader.TryRead: corrupt input returns null and finally releases COM.
    let before = outstanding_on_this_thread();
    let drops = Arc::new(AtomicUsize::new(0));
    assert!(
        WicEmbeddedThumbnailBackend
            .open(Box::new(TrackedStream {
                bytes: Cursor::new(vec![1, 2, 3, 4]),
                drops: drops.clone(),
            }))
            .is_err()
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(outstanding_on_this_thread(), before);
}
#[test]
#[ignore = "native: needs WIC"]
fn try_read_returns_eager_pixels_and_releases_source_before_return() {
    // C# EmbeddedThumbnailReader.ReadFromStream: eager copy; returned image never depends on source.
    use pr_imaging::{
        decode_types::DecodedImageView,
        embedded_thumbnail::try_read,
        pixel_memory::{NativePixelMemory, PixelBufferImageCodec},
    };
    use std::os::windows::fs::OpenOptionsExt;
    let root = pr_test_support::TempRoot::new("cx3-embedded-lifetime");
    let path = root.path().join("source.jpg");
    std::fs::write(&path, fixture()).unwrap();
    let before_com = outstanding_on_this_thread();
    let before_pixels = NativePixelMemory::live_count();
    let image = try_read(
        &path,
        &pr_core::io::physical::PhysicalFileSystem,
        &WicEmbeddedThumbnailBackend,
        &PixelBufferImageCodec::INSTANCE,
    )
    .unwrap()
    .unwrap();
    assert_eq!(outstanding_on_this_thread(), before_com);
    let expected = image.platform_image().as_pixels().unwrap().bytes().unwrap().to_vec();
    assert!(!expected.is_empty());
    // Exclusive open proves the source handle was released, without sleeps or timing assumptions.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    drop(file);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(image.platform_image().as_pixels().unwrap().bytes().unwrap(), expected);
    drop(image);
    assert_eq!(NativePixelMemory::live_count(), before_pixels);
}
